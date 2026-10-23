//! UI tests: the real workspace in GPUI's test window, driven by keystrokes and
//! clicks as a user would, against an SSH server running in the test process
//! (see [`server`]). They read back what a user would see: the terminal's
//! screen as text, the tabs, a pane's status, the host list. A click finds its
//! element by a `.debug_selector(..)` in the app's code ([`Ui::click`]).
//!
//! Built only with the `ui-tests` feature: they take longer than the unit
//! tests, so CI runs them daily and by hand (`.github/workflows/ui-tests.yml`).
//! Locally: `cargo test -p shellwing --features ui-tests ui_tests` (add
//! `--profile ui-tests` on a slow machine: debug builds of libghostty parse
//! slowly; not `--release`, where GPUI does not record what the clicks find).
//!
//! Each scenario runs in a child process of the test binary, with its own
//! environment: known_hosts, the ssh config and `HOME` point into a scratch
//! directory, so no scenario reads or writes the user's `~/.ssh`. (Setting the
//! variables in this process would need `set_var`, which edition 2024 makes
//! unsafe, and would leak between scenarios running at once.) [`scenario!`]
//! writes the pair of tests: `run`, which starts the child, and `scenario`,
//! ignored, which the child runs.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use gpui::{Context, TestAppContext, Window, WindowHandle};
use shellwing_config::Store;

use crate::look::Live;
use crate::session::Status;
use crate::view::TermView;
use crate::workspace::Workspace;

pub mod server;

mod config;
mod config_files;
mod connect;
mod files;
mod forward;
mod host_list;
mod hosts;
mod keyboard;
mod keys;
mod local;
mod lock;
mod mouse;
mod notes;
mod overlays;
mod security;
mod session_life;
mod snippets;
mod sync;
mod tabs;
mod terminal;
mod updates;
mod vault;

pub use server::{OTP, PASSWORD, PROMPT, Policy, Server, USER, WELCOME};

/// The lists that scroll, by their `.debug_selector(..)`: a click on what is
/// in one scrolls it into view first ([`Ui::click`]).
const SCROLLING: [&str; 3] = ["settings-content", "hosts", "editor-body"];

/// How long a step may take before the scenario fails. Generous: debug
/// builds of libghostty parse slowly, and CI machines are slow.
pub const STEP: Duration = Duration::from_secs(30);

/// Writes a scenario: `run` (a test) runs `scenario` (ignored) in a child
/// process with a scratch environment; `scenario` gets a fresh [`Ui`].
macro_rules! scenario {
    ($(#[doc = $doc:literal])* $name:ident, $body:expr) => {
        $(#[doc = $doc])*
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn run() {
                $crate::ui_tests::in_child(module_path!());
            }

            #[test]
            #[ignore = "run in its own process by `run`"]
            fn scenario() {
                let body: fn(&mut $crate::ui_tests::Ui) = $body;
                let mut ui = $crate::ui_tests::Ui::new();
                body(&mut ui);
            }
        }
    };
}
pub(crate) use scenario;

/// Runs the scenario of module `module` (a `module_path!()`) in a child
/// process whose environment points known_hosts, the ssh config and `HOME`
/// into a fresh scratch directory.
pub fn in_child(module: &str) {
    // The test's name, as the harness knows it: without the crate's name.
    let module = module.split_once("::").map_or(module, |(_, rest)| rest);
    let name = format!("{module}::scenario");
    let dir = std::env::temp_dir().join(format!(
        "shellwing-ui-{}-{}",
        module.replace("::", "-"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).expect("scratch dir");
    std::fs::write(dir.join("ssh_config"), "").expect("ssh config");
    std::fs::write(dir.join("known_hosts"), "").expect("known_hosts");
    // The scenario's ssh-agent socket, on which only a scenario that starts
    // an agent ([`Ui::agent`]) listens. Not in `dir`: a Unix socket's path is
    // at most 104 bytes on macOS, whose temporary directory is long.
    let agent = std::env::temp_dir().join(format!("swa-{:016x}.sock", {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        dir.hash(&mut h);
        h.finish()
    }));
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            &name,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("SHELLWING_UI_TEST_DIR", &dir)
        .env("SHELLWING_KNOWN_HOSTS", dir.join("known_hosts"))
        .env("SHELLWING_SSH_CONFIG", dir.join("ssh_config"))
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("XDG_CONFIG_HOME", dir.join("xdg/config"))
        .env("XDG_DATA_HOME", dir.join("xdg/data"))
        .env("XDG_CACHE_HOME", dir.join("xdg/cache"))
        // No session bus: nothing can reach the desktop's keychain (the vault
        // keeps its key in memory in tests anyway).
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        // Never the user's agent; nothing listens there unless the scenario
        // starts one, and an agent that is not there is passed over.
        .env("SSH_AUTH_SOCK", &agent)
        .env_remove("SHELLWING_PASSWORD")
        .output()
        .expect("run the scenario");
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let _ = std::fs::remove_file(&agent);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "scenario {name} failed:\n{stdout}\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A TCP port free right now on 127.0.0.1 (taken and let go: another
/// program could take it in between, which is unlikely).
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}

/// Sends `line` to 127.0.0.1:`port` and reads one line back.
pub fn talk(port: u16, line: &str) -> String {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(STEP)).expect("timeout");
    stream
        .write_all(format!("{line}\n").as_bytes())
        .expect("write");
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).expect("read");
    answer.trim_end().to_string()
}

/// A scenario's app: one window with the workspace, as `main` opens it, and
/// the runtime the network runs on.
pub struct Ui {
    pub cx: TestAppContext,
    pub rt: tokio::runtime::Runtime,
    /// The scenario's scratch directory: `config/` is the app's config
    /// directory, `known_hosts` and `ssh_config` the ones in effect.
    pub dir: PathBuf,
    window: Option<WindowHandle<Workspace>>,
}

impl Ui {
    pub fn new() -> Ui {
        let dir = std::env::var_os("SHELLWING_UI_TEST_DIR")
            .map(PathBuf::from)
            .expect("run through in_child");
        // Never the real list of releases: a scenario that looks for updates
        // names its own ([`shellwing_update::set_test_source`]).
        shellwing_update::set_test_source("http://127.0.0.1:1/release.signed.json", &[]);
        let cx = TestAppContext::single();
        cx.executor().allow_parking();
        // As main does: the notes' editor keys, and none for other windows
        // while shellwing is locked.
        cx.update(shellwing_note_editor::bind_keys);
        cx.update(crate::app_lock::guard_other_windows);
        Ui {
            cx,
            rt: tokio::runtime::Runtime::new().expect("runtime"),
            dir,
            window: None,
        }
    }

    pub fn config_dir(&self) -> PathBuf {
        self.dir.join("config")
    }

    /// Writes a file in the app's config directory (`hosts.toml`,
    /// `settings.toml`, ...).
    pub fn config_file(&self, name: &str, text: &str) {
        std::fs::write(self.config_dir().join(name), text).expect("config file");
    }

    /// Adds `server`'s host key to known_hosts, as after an earlier visit.
    pub fn trust(&self, server: &Server) {
        let path = self.dir.join("known_hosts");
        let mut known = std::fs::read_to_string(&path).unwrap_or_default();
        known.push_str(&server.known_hosts_line());
        known.push('\n');
        std::fs::write(path, known).expect("known_hosts");
    }

    pub fn known_hosts(&self) -> String {
        std::fs::read_to_string(self.dir.join("known_hosts")).unwrap_or_default()
    }

    /// Writes the ssh config in effect.
    pub fn ssh_config(&self, text: &str) {
        std::fs::write(self.dir.join("ssh_config"), text).expect("ssh config");
    }

    /// Opens the window with the host list, as `shellwing` alone does.
    pub fn open(&mut self) {
        self.open_with(None, &[]);
    }

    /// Opens the window connecting to `spec`, as `shellwing <spec>` does.
    pub fn connect(&mut self, spec: &str) {
        self.open_with(Some(spec), &[]);
    }

    /// `shellwing [-L|-R|-D spec]... <spec>`.
    pub fn open_with(&mut self, spec: Option<&str>, forwards: &[&str]) {
        assert!(self.window.is_none(), "one window per scenario");
        let (live, problems) = Live::open(Some(Store::new(self.config_dir())));
        let handle = self.rt.handle().clone();
        let spec = spec.map(str::to_string);
        let forwards: Vec<String> = forwards.iter().map(|f| f.to_string()).collect();
        let window = self.cx.add_window(move |window, cx| {
            let mut ws = Workspace::new(live, problems, handle, None, cx);
            ws.start(spec.as_deref(), &forwards, window, cx);
            ws
        });
        self.window = Some(window);
        self.ws(|ws, window, cx| {
            // As main does: the window's appearance, and its handle, which
            // what the workspace does later in the window needs.
            ws.track_appearance(window, cx);
            let active = ws.active_index();
            ws.activate(active, window, cx);
        });
        self.cx.run_until_parked();
    }

    pub fn window(&self) -> WindowHandle<Workspace> {
        self.window.expect("open the window first")
    }

    pub fn ws<R>(
        &mut self,
        f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> R,
    ) -> R {
        let window = self.window();
        window.update(&mut self.cx, f).expect("the window is open")
    }

    /// Reads or acts on the active tab's focused pane.
    pub fn view<R>(&mut self, f: impl FnOnce(&mut TermView, &mut Context<TermView>) -> R) -> R {
        self.ws(|ws, _, cx| ws.active_view().update(cx, f))
    }

    pub fn tabs(&mut self) -> usize {
        self.ws(|ws, _, _| ws.tab_count())
    }

    pub fn status(&mut self) -> Status {
        self.view(|v, _| v.status())
    }

    /// The active terminal's screen, one line per row.
    pub fn screen(&mut self) -> String {
        self.cx.run_until_parked();
        self.view(|v, _| v.screen_text())
    }

    /// Lets the app and the network run for `d`.
    pub fn settle(&mut self, d: Duration) {
        let start = Instant::now();
        while start.elapsed() < d {
            self.cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.cx.run_until_parked();
    }

    /// Waits until `done` holds; fails with `what` and the screen after [`STEP`].
    pub fn wait_until(&mut self, what: &str, done: impl FnMut(&mut Ui) -> bool) {
        self.wait_until_within(what, STEP, done);
    }

    /// As [`Ui::wait_until`], for a step that may take longer than [`STEP`]
    /// on a busy machine (a program the app runs, such as git).
    pub fn wait_until_within(
        &mut self,
        what: &str,
        limit: Duration,
        mut done: impl FnMut(&mut Ui) -> bool,
    ) {
        let start = Instant::now();
        loop {
            self.cx.run_until_parked();
            if done(self) {
                return;
            }
            if start.elapsed() > limit {
                let screen = if self.window.is_some() && self.tabs() > 0 {
                    self.screen()
                } else {
                    "(no tab)".into()
                };
                panic!("waited {limit:?} for {what}; the screen:\n{screen}");
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// Waits until the active terminal shows `text`, and returns the screen.
    pub fn wait_for(&mut self, text: &str) -> String {
        let mut screen = String::new();
        self.wait_until(&format!("{text:?} on the screen"), |ui| {
            screen = ui.screen();
            screen.contains(text)
        });
        screen
    }

    /// Waits until `text` occurs `n` times on the screen.
    pub fn wait_for_count(&mut self, text: &str, n: usize) -> String {
        let mut screen = String::new();
        self.wait_until(&format!("{n} × {text:?} on the screen"), |ui| {
            screen = ui.screen();
            screen.matches(text).count() >= n
        });
        screen
    }

    /// Presses keys, GPUI's way: "ctrl-shift-k", "enter", "up up".
    pub fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(self.window().into(), keys);
    }

    /// Types `text` as keystrokes.
    pub fn type_text(&mut self, text: &str) {
        let window = self.window().into();
        for c in text.chars() {
            match c {
                ' ' => self.cx.simulate_keystrokes(window, "space"),
                c => self.cx.simulate_input(window, &c.to_string()),
            }
        }
    }

    /// Types `text` and presses Enter.
    pub fn line(&mut self, text: &str) {
        self.type_text(text);
        self.keys("enter");
    }

    /// Answers the password prompt and waits for the shell.
    pub fn login(&mut self) {
        self.wait_for("password:");
        self.line(PASSWORD);
        self.wait_for(WELCOME);
    }

    /// The keys of a window shortcut: `ctrl-shift-<key>`, `cmd-<key>` on
    /// macOS.
    pub fn shortcut(&mut self, key: &str) {
        let keys = if cfg!(target_os = "macos") {
            format!("cmd-{key}")
        } else {
            format!("ctrl-shift-{key}")
        };
        self.keys(&keys);
    }

    /// Opens the palette and types `text` into it, then Enter.
    pub fn palette(&mut self, text: &str) {
        self.shortcut("k");
        self.line(text);
    }

    /// Lets `d` pass, on the clock and for GPUI's timers (which run on a fake
    /// clock in tests: reconnect countdowns, notices).
    pub fn advance(&mut self, d: Duration) {
        let step = Duration::from_millis(100);
        let mut passed = Duration::ZERO;
        while passed < d {
            std::thread::sleep(step);
            self.cx.executor().advance_clock(step);
            self.cx.run_until_parked();
            passed += step;
        }
    }

    /// Draws the window, as a new frame from the system would: layout runs,
    /// and with it what depends on sizes (the terminal's grid).
    pub fn draw(&mut self) {
        let window = self.window().into();
        self.draw_window(window);
    }

    /// Draws `window`, as a new frame would.
    pub fn draw_window(&mut self, window: gpui::AnyWindowHandle) {
        // Not through its view: drawing renders it.
        gpui::AppContext::update_window(&mut self.cx, window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .expect("the window is open");
        self.cx.run_until_parked();
    }

    /// The vault, once it is open.
    pub fn vault(&mut self) -> Option<std::sync::Arc<shellwing_vault::Vault>> {
        self.ws(|ws, _, _| ws.vault_slot().ready())
    }

    /// The note window that is open (the last one opened).
    pub fn note_window(&mut self) -> Option<WindowHandle<crate::note_window::NoteWindow>> {
        self.cx.update(|cx| {
            cx.windows()
                .iter()
                .rev()
                .find_map(|w| w.downcast::<crate::note_window::NoteWindow>())
        })
    }

    /// The snippet library's window, once open.
    pub fn library_window(&mut self) -> Option<WindowHandle<crate::library_window::LibraryWindow>> {
        self.cx.update(|cx| {
            cx.windows()
                .iter()
                .find_map(|w| w.downcast::<crate::library_window::LibraryWindow>())
        })
    }

    /// The snippet window that is open, once it is.
    pub fn snippet_window(&mut self) -> Option<WindowHandle<crate::snippet_window::SnippetWindow>> {
        self.cx.update(|cx| {
            cx.windows()
                .iter()
                .find_map(|w| w.downcast::<crate::snippet_window::SnippetWindow>())
        })
    }

    /// Types `text` into another of the app's windows.
    pub fn type_in(&mut self, window: gpui::AnyWindowHandle, text: &str) {
        self.cx.simulate_input(window, text);
        self.cx.run_until_parked();
    }

    /// Presses keys in another of the app's windows, GPUI's way.
    pub fn keys_in(&mut self, window: gpui::AnyWindowHandle, keys: &str) {
        self.cx.simulate_keystrokes(window, keys);
        self.cx.run_until_parked();
    }

    /// Clicks the element marked `selector` in another of the app's windows
    /// (the snippet library, a note), drawn first.
    /// The element marked `selector` in `window`, if it draws it.
    pub fn find_in(
        &mut self,
        window: gpui::AnyWindowHandle,
        selector: &str,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        gpui::AppContext::update_window(&mut self.cx, window, |_, w, cx| {
            w.refresh();
            w.draw(cx).clear(cx);
        })
        .expect("the window is open");
        let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
        let mut vcx = gpui::VisualTestContext::from_window(window, &self.cx);
        vcx.debug_bounds(selector)
    }

    pub fn click_in(&mut self, window: gpui::AnyWindowHandle, selector: &str) {
        gpui::AppContext::update_window(&mut self.cx, window, |_, w, cx| {
            w.refresh();
            w.draw(cx).clear(cx);
        })
        .expect("the window is open");
        let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
        let mut vcx = gpui::VisualTestContext::from_window(window, &self.cx);
        let at = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("nothing marked {selector:?} is drawn there"))
            .center();
        vcx.simulate_click(at, gpui::Modifiers::none());
        self.cx.run_until_parked();
    }

    /// Acts on the note window that is open.
    pub fn note<R>(
        &mut self,
        f: impl FnOnce(
            &mut crate::note_window::NoteWindow,
            &mut Window,
            &mut Context<crate::note_window::NoteWindow>,
        ) -> R,
    ) -> R {
        let window = self.note_window().expect("a note window");
        let r = window
            .update(&mut self.cx, f)
            .expect("the note window is open");
        self.cx.run_until_parked();
        r
    }

    /// Runs `f` on the import drawer, when it is open.
    pub fn import<R>(
        &mut self,
        f: impl FnOnce(
            &mut crate::import_view::ImportView,
            &mut Context<crate::import_view::ImportView>,
        ) -> R,
    ) -> Option<R> {
        let view = self.ws(|ws, _, _| ws.import_view())?;
        let r = view.update(&mut self.cx, f);
        self.cx.run_until_parked();
        Some(r)
    }

    /// Resizes the window, as dragging its edge does, and draws it.
    pub fn resize(&mut self, width: f32, height: f32) {
        let vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
        self.draw();
    }

    /// The notice showing, with its tone: Settings', the window's or the
    /// active pane's.
    pub fn notice_shown(&mut self) -> Option<crate::notice::Notice> {
        self.cx.run_until_parked();
        if let Some(s) = self.ws(|ws, _, _| ws.settings_view()) {
            return self.cx.read(|cx| s.read(cx).notice_shown());
        }
        let window = self.ws(|ws, _, _| ws.notice_shown());
        if window.is_some() || self.tabs() == 0 {
            return window;
        }
        self.view(|v, _| v.notice_shown())
    }

    /// The notice showing: its text.
    pub fn notice(&mut self) -> Option<String> {
        self.notice_shown().map(|n| n.text.to_string())
    }

    /// Waits until a notice containing `text` shows.
    pub fn wait_for_notice(&mut self, text: &str) {
        self.wait_until(&format!("the notice {text:?}"), |ui| {
            ui.notice().is_some_and(|n| n.contains(text))
        });
    }

    /// Waits until a notice containing `text` shows, then checks it is
    /// colored as `tone`.
    pub fn wait_for_notice_in(&mut self, text: &str, tone: crate::notice::Tone) {
        self.wait_for_notice(text);
        let shown = self.notice_shown().expect("a notice");
        assert_eq!(shown.tone, tone, "{:?}", shown.text);
    }

    /// Answers the question the app asked in a dialog with `button`.
    pub fn answer_prompt(&mut self, button: &str) {
        self.wait_until("a dialog", |ui| ui.cx.has_pending_prompt());
        self.cx.simulate_prompt_answer(button);
        self.cx.run_until_parked();
    }

    /// Where the element marked `.debug_selector(|| selector)` is drawn now.
    /// GPUI records these only with debug assertions: a release build of
    /// the tests takes the `ui-tests` profile, which keeps them on for GPUI.
    pub fn bounds(&mut self, selector: &str) -> gpui::Bounds<gpui::Pixels> {
        self.find(selector)
            .unwrap_or_else(|| panic!("nothing marked {selector:?} is drawn"))
    }

    /// The element marked `selector`, if the window draws it.
    pub fn find(&mut self, selector: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.draw();
        // GPUI looks selectors up by a 'static str; a scenario names a few.
        let selector: &'static str = Box::leak(selector.to_string().into_boxed_str());
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.debug_bounds(selector)
    }

    /// The middle of the element marked `selector`, scrolled into view first
    /// when it is in one of the [`SCROLLING`] lists and beyond what the list
    /// shows, as a user scrolls to what they want to click. (Its bounds are
    /// where it is laid out, shown or not: one scrolled under a page's
    /// header is not under the mouse.) What does not scroll, as a footer
    /// below a list, is clicked where it is.
    fn reach(&mut self, selector: &str) -> gpui::Point<gpui::Pixels> {
        let mut last = None;
        for _ in 0..50 {
            let b = self.bounds(selector);
            let at = b.center();
            let list = SCROLLING
                .iter()
                .filter_map(|s| self.find(s))
                .find(|l| l.left() <= at.x && at.x <= l.right());
            let Some(list) = list else {
                return at;
            };
            let delta = if b.top() < list.top() {
                3.
            } else if b.bottom() > list.bottom() {
                -3.
            } else {
                return at;
            };
            if last == Some(at.y) {
                // It does not move: it is not in the list.
                let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
                let window = vcx.update(|w, _| w.viewport_size());
                assert!(
                    at.y >= gpui::px(0.) && at.y < window.height,
                    "{selector:?} is outside the window and does not scroll into it"
                );
                return at;
            }
            last = Some(at.y);
            let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: list.center(),
                delta: gpui::ScrollDelta::Lines(gpui::point(0., delta)),
                ..Default::default()
            });
            self.cx.run_until_parked();
        }
        panic!("{selector:?} cannot be scrolled into view");
    }

    /// Clicks the middle of the element marked `selector`, as the mouse does.
    pub fn click(&mut self, selector: &str) {
        let at = self.reach(selector);
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.simulate_click(at, gpui::Modifiers::none());
        self.cx.run_until_parked();
    }

    /// Right-clicks the middle of the element marked `selector` (a context
    /// menu).
    pub fn right_click(&mut self, selector: &str) {
        let at = self.reach(selector);
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        let right = gpui::MouseButton::Right;
        vcx.simulate_mouse_move(at, None, gpui::Modifiers::none());
        vcx.simulate_mouse_down(at, right, gpui::Modifiers::none());
        vcx.simulate_mouse_up(at, right, gpui::Modifiers::none());
        self.cx.run_until_parked();
    }

    /// Double-clicks the middle of the element marked `selector`.
    pub fn double_click(&mut self, selector: &str) {
        let at = self.reach(selector);
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        let (button, modifiers) = (gpui::MouseButton::Left, gpui::Modifiers::none());
        for click_count in [1, 2] {
            vcx.simulate_event(gpui::MouseDownEvent {
                position: at,
                modifiers,
                button,
                click_count,
                first_mouse: false,
            });
            vcx.simulate_event(gpui::MouseUpEvent {
                position: at,
                modifiers,
                button,
                click_count,
            });
        }
        self.cx.run_until_parked();
    }

    /// Moves the mouse over the middle of the element marked `selector`.
    pub fn hover(&mut self, selector: &str) {
        let at = self.reach(selector);
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.simulate_mouse_move(at, None, gpui::Modifiers::none());
        self.cx.run_until_parked();
    }

    /// The middle of the active terminal's cell at `row`, `col` (from the
    /// top left of the screen), where it is drawn.
    pub fn cell_point(&mut self, row: usize, col: usize) -> gpui::Point<gpui::Pixels> {
        self.cell_at(row, col, 0.5)
    }

    /// A point in the cell at `row`, `col`, `across` its width from its left
    /// edge (0 to 1), in the middle of its height.
    pub fn cell_at(&mut self, row: usize, col: usize, across: f32) -> gpui::Point<gpui::Pixels> {
        self.draw();
        let (origin, (w, h)) = self.view(|v, _| (v.bounds().origin, v.cell_size()));
        gpui::point(
            origin.x + w * (col as f32 + across),
            origin.y + h * (row as f32 + 0.5),
        )
    }

    /// Where `text` is on the active terminal's screen: its row and the
    /// column it starts at (one character per cell, as for ASCII).
    pub fn find_text(&mut self, text: &str) -> (usize, usize) {
        self.cx.run_until_parked();
        let frame = self.view(|v, _| v.latest_frame());
        for (row, r) in frame.rows.iter().enumerate() {
            if let Some(at) = r.text.find(text) {
                return (row, r.text[..at].chars().count());
            }
        }
        panic!("{text:?} is not on the screen:\n{}", self.screen());
    }

    /// Presses the left button on one cell and lets it go on another, moving
    /// over the cells between, with `modifiers` held: from the left part of
    /// the first to the right part of the last, as a user drags over text
    /// (a cell is in a selection once the mouse is past its middle).
    pub fn drag(&mut self, from: (usize, usize), to: (usize, usize), modifiers: gpui::Modifiers) {
        let (a, b) = (
            self.cell_at(from.0, from.1, 0.2),
            self.cell_at(to.0, to.1, 0.8),
        );
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        let left = gpui::MouseButton::Left;
        vcx.simulate_mouse_move(a, None, modifiers);
        vcx.simulate_mouse_down(a, left, modifiers);
        for i in 1..=4 {
            let t = i as f32 / 4.;
            vcx.simulate_mouse_move(a + (b - a) * t, left, modifiers);
        }
        vcx.simulate_mouse_up(b, left, modifiers);
        self.cx.run_until_parked();
    }

    /// Clicks a cell of the active terminal with `modifiers` held.
    pub fn click_cell(&mut self, row: usize, col: usize, modifiers: gpui::Modifiers) {
        let at = self.cell_point(row, col);
        let mut vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.simulate_mouse_move(at, None, modifiers);
        vcx.simulate_click(at, modifiers);
        self.cx.run_until_parked();
    }

    /// The last web page or file the app asked the system to open.
    pub fn opened_url(&mut self) -> Option<String> {
        self.cx.run_until_parked();
        self.cx.opened_url()
    }

    /// The text on the clipboard.
    pub fn clipboard(&mut self) -> Option<String> {
        self.cx.read_from_clipboard().and_then(|c| c.text())
    }

    /// The Settings page showing, if Settings is open.
    pub fn settings_page(&mut self) -> Option<crate::settings_view::Page> {
        self.ws(|ws, _, cx| ws.settings_page(cx))
    }

    /// What Settings' Sync page says beside Connect, if Settings is open.
    pub fn sync_problem(&mut self) -> Option<String> {
        self.cx.run_until_parked();
        let s = self.ws(|ws, _, _| ws.settings_view())?;
        self.cx.read(|cx| s.read(cx).sync_problem())
    }

    /// The sidebar's host list as shown: each section's title and its hosts.
    pub fn host_list(&mut self) -> Vec<(String, Vec<String>)> {
        self.cx.run_until_parked();
        self.ws(|ws, _, cx| ws.host_list(cx))
    }

    /// The names of the hosts the sidebar shows, in order.
    pub fn host_names(&mut self) -> Vec<String> {
        self.host_list().into_iter().flat_map(|(_, h)| h).collect()
    }

    pub fn host_editor_open(&mut self) -> bool {
        self.ws(|ws, _, _| ws.host_editor_open())
    }

    pub fn group_editor_open(&mut self) -> bool {
        self.ws(|ws, _, _| ws.group_editor_open())
    }

    /// hosts.toml as it is on disk.
    pub fn hosts_toml(&self) -> String {
        std::fs::read_to_string(self.config_dir().join("hosts.toml")).unwrap_or_default()
    }

    pub fn copy_to_clipboard(&mut self, text: &str) {
        self.cx
            .write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
    }

    pub fn panes(&mut self) -> usize {
        self.ws(|ws, _, _| ws.pane_count())
    }

    /// Reloads the config files, as the app does every half second.
    pub fn reload_config(&mut self) {
        self.ws(|ws, _, cx| ws.poll_config(cx));
        self.cx.run_until_parked();
    }

    /// Starts the user's ssh-agent, holding `keys`, where `$SSH_AUTH_SOCK`
    /// points.
    #[cfg(unix)]
    pub fn agent(&self, keys: &[&russh::keys::PrivateKey]) {
        let path = PathBuf::from(std::env::var_os("SSH_AUTH_SOCK").expect("set by in_child"));
        let _ = std::fs::remove_file(&path);
        let listener = {
            let _rt = self.rt.enter();
            tokio::net::UnixListener::bind(&path).expect("agent socket")
        };
        let incoming = futures::stream::unfold(listener, |l| async move {
            Some((l.accept().await.map(|(s, _)| s), l))
        });
        self.rt
            .spawn(russh::keys::agent::server::serve(Box::pin(incoming), ()));
        self.rt.block_on(async {
            let mut agent = russh::keys::agent::client::AgentClient::connect_uds(&path)
                .await
                .expect("reach the agent");
            for key in keys {
                agent.add_identity(key, &[]).await.expect("add a key");
            }
        });
    }

    /// Starts `server` with `policy` on the scenario's runtime.
    pub fn server(&self, policy: Policy) -> Server {
        Server::start(&self.rt, policy)
    }
}

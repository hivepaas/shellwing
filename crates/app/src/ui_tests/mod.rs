//! UI tests: the real workspace in GPUI's test window, driven by keystrokes as
//! a user would, against an SSH server running in the test process (see
//! [`server`]). They read back what a user would see: the terminal's screen as
//! text, the tabs, a pane's status.
//!
//! Built only with the `ui-tests` feature: they take longer than the unit
//! tests, so CI runs them daily and by hand (`.github/workflows/ui-tests.yml`).
//! Locally: `cargo test -p shellwing --features ui-tests ui_tests` (add
//! `--release` on a slow machine: debug builds of libghostty parse slowly).
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

mod connect;
mod files;
mod forward;
mod hosts;
mod local;
mod notes;
mod session_life;
mod tabs;
mod terminal;
mod vault;

pub use server::{OTP, PASSWORD, PROMPT, Policy, Server, USER, WELCOME};

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
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("SHELLWING_PASSWORD")
        .output()
        .expect("run the scenario");
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
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
        let cx = TestAppContext::single();
        cx.executor().allow_parking();
        // As main does: the notes' editor keys.
        cx.update(shellwing_note_editor::bind_keys);
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
    pub fn wait_until(&mut self, what: &str, mut done: impl FnMut(&mut Ui) -> bool) {
        let start = Instant::now();
        loop {
            self.cx.run_until_parked();
            if done(self) {
                return;
            }
            if start.elapsed() > STEP {
                let screen = if self.window.is_some() && self.tabs() > 0 {
                    self.screen()
                } else {
                    "(no tab)".into()
                };
                panic!("waited {STEP:?} for {what}; the screen:\n{screen}");
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

    /// Resizes the window, as dragging its edge does, and draws it.
    pub fn resize(&mut self, width: f32, height: f32) {
        let vcx = gpui::VisualTestContext::from_window(self.window().into(), &self.cx);
        vcx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
        self.draw();
    }

    /// The notice showing, the window's or the active pane's.
    pub fn notice(&mut self) -> Option<String> {
        self.cx.run_until_parked();
        let window = self.ws(|ws, _, _| ws.notice_text());
        if window.is_some() || self.tabs() == 0 {
            return window;
        }
        self.view(|v, _| v.notice_text())
    }

    /// Waits until a notice containing `text` shows.
    pub fn wait_for_notice(&mut self, text: &str) {
        self.wait_until(&format!("the notice {text:?}"), |ui| {
            ui.notice().is_some_and(|n| n.contains(text))
        });
    }

    /// Answers the question the app asked in a dialog with `button`.
    pub fn answer_prompt(&mut self, button: &str) {
        self.wait_until("a dialog", |ui| ui.cx.has_pending_prompt());
        self.cx.simulate_prompt_answer(button);
        self.cx.run_until_parked();
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

    /// Starts `server` with `policy` on the scenario's runtime.
    pub fn server(&self, policy: Policy) -> Server {
        Server::start(&self.rt, policy)
    }
}

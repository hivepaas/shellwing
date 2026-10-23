//! shellwing: a native SSH client.
//!
//! Usage: `shellwing [-L|-R|-D spec]... [[user@]host[:port]]`. Without a host the
//! window opens with the host list. `-L`, `-R` and `-D` forward ports as ssh's
//! options do, in the tab the host opens in. `host` may be a host from `hosts.toml` in the config directory
//! or an alias from `~/.ssh/config` (`$SHELLWING_SSH_CONFIG` overrides the file).
//! Settings and themes come from the config directory too (see
//! `docs/configuration.md`). Authentication follows OpenSSH: agent keys, key
//! files, keyboard-interactive, password; any passphrase, code or password is
//! asked for in the terminal. A new host key is shown with its fingerprint and
//! must be accepted; a changed one is refused. In a debug build
//! `$SHELLWING_PASSWORD` is tried once before asking (a release ignores it),
//! and `$SHELLWING_KNOWN_HOSTS` replaces `~/.ssh/known_hosts` (both meant for
//! scripts and benchmarks).

// A GUI program on Windows: no console window opens beside it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_lock;
mod chrome;
mod colors;
mod copies;
mod editors;
mod environment;
mod files;
mod group_editor;
mod host_editor;
mod hotkeys;
mod import_view;
mod input;
mod keys_view;
mod known_hosts_view;
mod library;
mod library_window;
mod look;
mod note_images;
mod note_window;
mod notice;
mod palette;
mod pane;
mod placement;
mod placing;
mod prompt;
mod scrollbar;
mod settings_view;
mod shell_syntax;
mod sidebar;
mod snippet_window;
mod snippets;
mod snippets_view;
mod sync_panel;
mod syncing;
mod tab_overview;
mod view;
mod workspace;

#[cfg(all(test, feature = "ui-tests"))]
mod ui_tests;

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext as _, Entity, WindowDecorations, WindowOptions};
use shellwing_config::Store;

// The UI-free half, under the names the app's modules use.
use shellwing_core::{Writer, bench, local, secrets, session, target};

/// Icons drawn into the binary: the app has no other files.
struct Assets;

const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/folder-plus.svg",
        include_bytes!("../assets/icons/folder-plus.svg"),
    ),
    ("icons/home.svg", include_bytes!("../assets/icons/home.svg")),
    ("icons/info.svg", include_bytes!("../assets/icons/info.svg")),
    (
        "icons/laptop.svg",
        include_bytes!("../assets/icons/laptop.svg"),
    ),
    (
        "icons/refresh.svg",
        include_bytes!("../assets/icons/refresh.svg"),
    ),
    (
        "icons/upload.svg",
        include_bytes!("../assets/icons/upload.svg"),
    ),
    ("icons/logo.svg", include_bytes!("../assets/icons/logo.svg")),
    ("icons/tabs.svg", include_bytes!("../assets/icons/tabs.svg")),
    (
        "icons/cloud.svg",
        include_bytes!("../assets/icons/cloud.svg"),
    ),
    (
        "icons/check.svg",
        include_bytes!("../assets/icons/check.svg"),
    ),
    ("icons/eye.svg", include_bytes!("../assets/icons/eye.svg")),
    (
        "icons/eye-off.svg",
        include_bytes!("../assets/icons/eye-off.svg"),
    ),
    (
        "icons/terminal.svg",
        include_bytes!("../assets/icons/terminal.svg"),
    ),
    (
        "icons/folder.svg",
        include_bytes!("../assets/icons/folder.svg"),
    ),
    ("icons/file.svg", include_bytes!("../assets/icons/file.svg")),
    (
        "icons/panel-close.svg",
        include_bytes!("../assets/icons/panel-close.svg"),
    ),
    (
        "icons/panel-open.svg",
        include_bytes!("../assets/icons/panel-open.svg"),
    ),
    (
        "icons/search.svg",
        include_bytes!("../assets/icons/search.svg"),
    ),
    (
        "icons/server.svg",
        include_bytes!("../assets/icons/server.svg"),
    ),
    (
        "icons/settings.svg",
        include_bytes!("../assets/icons/settings.svg"),
    ),
    (
        "icons/minimize.svg",
        include_bytes!("../assets/icons/minimize.svg"),
    ),
    (
        "icons/maximize.svg",
        include_bytes!("../assets/icons/maximize.svg"),
    ),
    (
        "icons/close.svg",
        include_bytes!("../assets/icons/close.svg"),
    ),
    // The caption buttons as Windows draws them.
    (
        "icons/win-minimize.svg",
        include_bytes!("../assets/icons/win-minimize.svg"),
    ),
    (
        "icons/win-maximize.svg",
        include_bytes!("../assets/icons/win-maximize.svg"),
    ),
    (
        "icons/win-restore.svg",
        include_bytes!("../assets/icons/win-restore.svg"),
    ),
    (
        "icons/win-close.svg",
        include_bytes!("../assets/icons/win-close.svg"),
    ),
];

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| std::borrow::Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| (*p).into())
            .collect())
    }
}

/// How often the config files are checked for edits.
const CONFIG_POLL: Duration = Duration::from_millis(500);

const USAGE: &str = "usage: shellwing [-L|-R|-D spec]... [[user@]host[:port]]
       shellwing --local
  host     a name from hosts.toml, an alias from ~/.ssh/config, or a host name
  --local  a shell on this computer
  -L spec  [bind:]port:host:hostport  a port here reaches host:hostport from the server
  -R spec  [bind:]port:host:hostport  a port on the server reaches host:hostport from here
  -D spec  [bind:]port                a SOCKS proxy here, going out from the server";

/// The host to open and the forwards for it, from the command line.
fn parse_args(
    mut args: impl Iterator<Item = String>,
) -> Result<(Option<String>, Vec<String>), String> {
    let (mut host, mut forwards) = (None, vec![]);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-L" | "-R" | "-D" => {
                let spec = args.next().ok_or(format!("{a} needs a value"))?;
                forwards.push(format!("{a} {spec}"));
            }
            "--local" if host.is_some() => return Err("only one host can be given".into()),
            "--local" => host = Some(local::SPEC.to_string()),
            s if ["-L", "-R", "-D"].iter().any(|f| s.starts_with(f)) => forwards.push(a),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ if host.is_some() => return Err("only one host can be given".into()),
            _ => host = Some(a),
        }
    }
    for f in &forwards {
        f.parse::<shellwing_ssh::Forward>()
            .map_err(|e| e.to_string())?;
    }
    if !forwards.is_empty() && host.as_deref().is_none_or(|h| h == local::SPEC) {
        return Err("forwards need a host to go through".into());
    }
    Ok((host, forwards))
}

/// Windows starts a GUI program without a console. Started from a terminal,
/// the app takes that one, so `--help`, errors and logs are written there.
#[cfg(windows)]
#[allow(unsafe_code)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: no pointers or handles go in. Without a parent console it
    // fails and changes nothing, which is what a start from Explorer wants.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    attach_console();
    if std::env::args().skip(1).any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let (arg, forwards) = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("shellwing: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let (live, problems) = look::Live::open(Store::default_dir().map(Store::new));
    // The app's language, before anything is said.
    shellwing_i18n::set_language(live.settings().general.language.as_deref());
    // A note's text kept from another device's edit is titled in it.
    shellwing_vault::title_copies(syncing::copy_title);
    // One network runtime for every tab.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("shellwing-net")
        .build()?;
    let rt_handle = rt.handle().clone();
    let bench = std::env::var("SHELLWING_BENCH")
        .ok()
        .filter(|m| !m.is_empty())
        .map(|m| (m, Arc::new(bench::Probe::new())));

    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // Questions are drawn in the app's look on every system, not in
            // each one's own dialog (and Linux has none: GPUI would draw a
            // white box).
            cx.set_prompt_builder(prompt::render);
            // The notes' editor: its keys, while it has the focus.
            shellwing_note_editor::bind_keys(cx);
            app_lock::guard_other_windows(cx);
            let window_bounds = placement::initial(live.store().and_then(|s| s.window_state()), cx);
            // GNOME's Wayland compositor draws no window frame, so Linux gets
            // client-side decorations. macOS and Windows keep the system's frame
            // but hide its title bar, and the app draws its own in its place,
            // with the tabs in it (see chrome::window_chrome).
            let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
            let options = WindowOptions {
                window_bounds: Some(window_bounds),
                window_decorations: client_side.then_some(WindowDecorations::Client),
                titlebar: (!client_side).then(|| gpui::TitlebarOptions {
                    title: Some("shellwing".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(chrome::TRAFFIC_LIGHTS),
                }),
                app_id: Some("shellwing".into()),
                ..Default::default()
            };
            let mut setup = Some((live, problems, bench));
            let Ok(window) = cx.open_window(options, |window, cx| {
                cx.new(|cx| {
                    let (live, problems, bench) = setup
                        .take()
                        .unwrap_or_else(|| (look::Live::open(None).0, vec![], None));
                    let mut ws =
                        workspace::Workspace::new(live, problems, rt_handle.clone(), bench, cx);
                    ws.start(arg.as_deref(), &forwards, window, cx);
                    ws
                })
            }) else {
                eprintln!("cannot open a window");
                cx.quit();
                return;
            };
            let Ok(ws) = window.update(cx, |_, _, cx| cx.entity()) else {
                return;
            };
            let ws: Entity<workspace::Workspace> = ws;
            let _ = window.update(cx, |ws, window, cx| {
                let active = ws.active_index();
                ws.activate(active, window, cx);
                cx.activate(true);
                ws.track_bounds(window, cx);
                ws.track_appearance(window, cx);
                ws.guard_close(window, cx);
            });
            // One window: closing it quits, on macOS too, where apps otherwise stay
            // in the Dock with no window and no way to open one.
            // Note windows go with the main one.
            cx.on_window_closed(|cx, _| {
                let main = cx
                    .windows()
                    .iter()
                    .any(|w| w.downcast::<workspace::Workspace>().is_some());
                if !main {
                    cx.quit();
                }
            })
            .detach();
            // Edits to the config files apply while running.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(CONFIG_POLL).await;
                    ws.update(cx, |ws, cx| ws.poll_config(cx));
                }
            })
            .detach();
        });
    drop(rt);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn parse(args: &[&str]) -> Result<(Option<String>, Vec<String>), String> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn reads_a_host_and_its_forwards() {
        assert_eq!(parse(&[]), Ok((None, vec![])));
        assert_eq!(
            parse(&["--local"]),
            Ok((Some(crate::local::SPEC.into()), vec![]))
        );
        assert_eq!(
            parse(&["-L", "8080:localhost:80", "-D1080", "me@web"]),
            Ok((
                Some("me@web".into()),
                vec!["-L 8080:localhost:80".into(), "-D1080".into()]
            ))
        );
        for (args, why) in [
            (&["-L"][..], "needs a value"),
            (&["-L", "80", "web"], "bad forward"),
            (&["-D", "1080"], "need a host"),
            (&["-x", "web"], "unknown option"),
            (&["a", "b"], "only one host"),
            (&["--local", "web"], "only one host"),
            (&["-D", "1080", "--local"], "need a host"),
        ] {
            let e = parse(args).expect_err(why);
            assert!(e.contains(why), "{args:?}: {e}");
        }
    }
}

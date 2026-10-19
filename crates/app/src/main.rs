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
//! must be accepted; a changed one is refused. `$SHELLWING_PASSWORD` is tried
//! once before asking, and `$SHELLWING_KNOWN_HOSTS` replaces
//! `~/.ssh/known_hosts` (both meant for scripts and benchmarks).

mod chrome;
mod colors;
mod copies;
mod editors;
mod files;
mod group_editor;
mod host_editor;
mod input;
mod keys_view;
mod known_hosts_view;
mod look;
mod palette;
mod pane;
mod placement;
mod prompt;
mod settings_view;
mod sidebar;
mod snippets;
mod snippets_view;
mod syncing;
mod view;
mod workspace;

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
    (
        "icons/arrow-up.svg",
        include_bytes!("../assets/icons/arrow-up.svg"),
    ),
    ("icons/home.svg", include_bytes!("../assets/icons/home.svg")),
    (
        "icons/refresh.svg",
        include_bytes!("../assets/icons/refresh.svg"),
    ),
    (
        "icons/upload.svg",
        include_bytes!("../assets/icons/upload.svg"),
    ),
    ("icons/logo.svg", include_bytes!("../assets/icons/logo.svg")),
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

fn main() -> anyhow::Result<()> {
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
            // Linux has no dialogs of its own: questions are drawn in the
            // app's look, where GPUI would draw a white box.
            if cfg!(not(any(target_os = "macos", target_os = "windows"))) {
                cx.set_prompt_builder(prompt::render);
            }
            let window_bounds = placement::initial(live.store().and_then(|s| s.window_state()), cx);
            // GNOME's Wayland compositor draws no window frame, so Linux gets
            // client-side decorations; macOS and Windows keep their native title bar.
            let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
            let options = WindowOptions {
                window_bounds: Some(window_bounds),
                window_decorations: client_side.then_some(WindowDecorations::Client),
                titlebar: (!client_side).then(|| gpui::TitlebarOptions {
                    title: Some("shellwing".into()),
                    ..Default::default()
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
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
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

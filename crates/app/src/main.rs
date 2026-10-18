//! shellwing: a native SSH client.
//!
//! Usage: `shellwing [[user@]host[:port]]`. Without a host the window opens with
//! the host list. `host` may be a host from `hosts.toml` in the config directory
//! or an alias from `~/.ssh/config` (`$SHELLWING_SSH_CONFIG` overrides the file).
//! Settings and themes come from the config directory too (see
//! `docs/configuration.md`). Authentication follows OpenSSH: agent keys, key
//! files, keyboard-interactive, password; any passphrase, code or password is
//! asked for in the terminal. A new host key is shown with its fingerprint and
//! must be accepted; a changed one is refused. `$SHELLWING_PASSWORD` is tried
//! once before asking, and `$SHELLWING_KNOWN_HOSTS` replaces
//! `~/.ssh/known_hosts` (both meant for scripts and benchmarks).

mod bench;
mod chrome;
mod look;
mod placement;
mod session;
mod sidebar;
mod target;
mod view;
mod workspace;
mod writer;

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext as _, Entity, WindowDecorations, WindowOptions};
use shellwing_config::Store;

pub use writer::Writer;

/// How often the config files are checked for edits.
const CONFIG_POLL: Duration = Duration::from_millis(500);

const USAGE: &str = "usage: shellwing [[user@]host[:port]]   (host may be a name from hosts.toml or an alias from ~/.ssh/config)";

fn main() -> anyhow::Result<()> {
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
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

    gpui_platform::application().run(move |cx: &mut App| {
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
                if let Some(spec) = &arg {
                    ws.open(spec, window, cx);
                }
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
        });
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

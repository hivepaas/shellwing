//! shellwing: a native SSH client.
//!
//! Usage: `shellwing [user@]host[:port]`, where `host` may be an alias from
//! `~/.ssh/config` (`$SHELLWING_SSH_CONFIG` overrides the file). Authentication
//! follows OpenSSH: agent keys, key files, keyboard-interactive, password; any
//! passphrase, code or password is asked for in the terminal. A new host key is
//! shown with its fingerprint and must be accepted; a changed one is refused.
//! `$SHELLWING_PASSWORD` is tried once before asking, and `$SHELLWING_KNOWN_HOSTS`
//! replaces `~/.ssh/known_hosts` (both meant for scripts and benchmarks).

mod bench;
mod chrome;
mod view;
mod writer;

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, Entity, Focusable as _, WindowBounds, WindowDecorations,
    WindowOptions, px, size,
};
use shellwing_ssh::{Auth, Ended, HostKeyPolicy, Options, Prompter, config};
use shellwing_term::TermHandle;

pub use writer::Writer;

/// A dimmed status line written into the terminal itself.
fn status(term: &TermHandle, msg: &str) {
    term.feed(format!("\r\n\x1b[2m[{msg}]\x1b[0m\r\n").into_bytes());
}

/// An error line, in red.
fn failure(term: &TermHandle, msg: &str) {
    term.feed(format!("\r\n\x1b[31m{msg}\x1b[0m\r\n").into_bytes());
}

fn main() -> anyhow::Result<()> {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!(
            "usage: shellwing [user@]host[:port]   (host may be an alias from ~/.ssh/config)"
        );
        std::process::exit(2);
    };
    let resolved = config::resolve(&arg, config::default_config_path().as_deref())?;
    let title = if resolved.alias == resolved.target.host {
        resolved.target.to_string()
    } else {
        format!("{} ({})", resolved.alias, resolved.target)
    };
    let bench_mode = std::env::var("SHELLWING_BENCH").unwrap_or_default();
    let probe = Arc::new(bench::Probe::new());
    let (writer, shown) = Writer::new();

    // The terminal thread wakes the UI through this; one pending wake is enough.
    let (wake_tx, wake_rx) = async_channel::bounded::<()>(1);
    // Events the UI has to act on, such as copied text.
    let (event_tx, event_rx) = async_channel::unbounded::<shellwing_term::Event>();
    let term = {
        let w = writer.clone();
        TermHandle::spawn(
            80,
            24,
            5000,
            Box::new(move |bytes| w.write(bytes)),
            Box::new(move || {
                let _ = wake_tx.try_send(());
            }),
            Box::new(move |ev| {
                let _ = event_tx.try_send(ev);
            }),
        )?
    };
    // Prompt text and echo, shown from their own thread (see Writer).
    {
        let term = term.clone();
        std::thread::Builder::new()
            .name("shellwing-echo".into())
            .spawn(move || {
                for bytes in shown {
                    term.feed(bytes);
                }
            })?;
    }

    {
        let (term, writer, probe) = (term.clone(), writer.clone(), probe.clone());
        std::thread::Builder::new()
            .name("shellwing-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => return failure(&term, &format!("cannot start network runtime: {e}")),
                };
                rt.block_on(async move {
                    let via: Vec<&str> = resolved.jumps.iter().map(|j| j.alias.as_str()).collect();
                    let via = if via.is_empty() {
                        String::new()
                    } else {
                        format!(" via {}", via.join(", "))
                    };
                    status(&term, &format!("connecting to {}{via}…", resolved.target));

                    // Questions from the connection are asked in the terminal.
                    let (prompter, mut questions) = Prompter::new();
                    {
                        let writer = writer.clone();
                        tokio::spawn(async move {
                            while let Some(req) = questions.recv().await {
                                writer.ask(req);
                            }
                        });
                    }
                    let opts = Options {
                        target: resolved,
                        auth: Auth {
                            agent: true,
                            password: std::env::var("SHELLWING_PASSWORD").ok(),
                        },
                        policy: HostKeyPolicy::Ask,
                        known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS").map(Into::into),
                        prompter: Some(prompter),
                        cols: 80,
                        rows: 24,
                        term: "xterm-256color".into(),
                    };
                    let out_term = term.clone();
                    let out_probe = probe.clone();
                    let (end_term, end_writer) = (term.clone(), writer.clone());
                    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                    let session = shellwing_ssh::connect(
                        opts,
                        Box::new(move |bytes| {
                            out_probe.saw_output(bytes);
                            out_term.feed(bytes.to_vec());
                        }),
                        Box::new(move |ended| {
                            end_writer.stop();
                            match ended {
                                Ended::Closed {
                                    exit_status: Some(s),
                                } => status(&end_term, &format!("session closed (exit {s})")),
                                Ended::Closed { exit_status: None } => {
                                    status(&end_term, "session closed")
                                }
                                Ended::Error(e) => {
                                    failure(&end_term, &format!("session error: {e}"))
                                }
                            }
                            let _ = done_tx.send(());
                        }),
                    )
                    .await;
                    match session {
                        Ok(s) => {
                            writer.start_session(s.sender());
                            tokio::spawn(bench::run(bench_mode, writer.clone(), probe));
                            let _ = done_rx.await;
                        }
                        Err(e) => {
                            writer.stop();
                            failure(&term, &e.to_string());
                        }
                    }
                });
            })?;
    }

    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        // GNOME's Wayland compositor draws no window frame, so Linux gets
        // client-side decorations; macOS and Windows keep their native title bar.
        let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_decorations: client_side.then_some(WindowDecorations::Client),
            titlebar: (!client_side).then(|| gpui::TitlebarOptions {
                title: Some(title.clone().into()),
                ..Default::default()
            }),
            app_id: Some("shellwing".into()),
            ..Default::default()
        };
        let Ok(window) = cx.open_window(options, |_, cx| {
            cx.new(|cx| {
                view::TermView::new(
                    term.clone(),
                    writer.clone(),
                    title.clone(),
                    probe.clone(),
                    cx,
                )
            })
        }) else {
            eprintln!("cannot open a window");
            cx.quit();
            return;
        };
        let Ok(view) = window.update(cx, |_, _, cx| cx.entity()) else {
            return;
        };
        let view: Entity<view::TermView> = view;
        let _ = window.update(cx, |v, window, cx| {
            window.focus(&v.focus_handle(cx), cx);
            cx.activate(true);
        });
        // Repaint whenever the terminal thread has a new frame.
        {
            let view = view.clone();
            cx.spawn(async move |cx| {
                while wake_rx.recv().await.is_ok() {
                    view.update(cx, |_, cx| cx.notify());
                }
            })
            .detach();
        }
        cx.spawn(async move |cx| {
            while let Ok(ev) = event_rx.recv().await {
                view.update(cx, |v, cx| v.on_term_event(ev, cx));
            }
        })
        .detach();
    });
    Ok(())
}

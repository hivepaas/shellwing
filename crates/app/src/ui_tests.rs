//! UI tests: the real workspace in GPUI's test window, driven by keystrokes as
//! a user would, against an SSH server running in the test process.
//!
//! Each scenario runs in a child process of the test binary, with its own
//! environment: known_hosts, the ssh config and `HOME` point into a scratch
//! directory, so no scenario reads or writes the user's `~/.ssh`. (Setting the
//! variables in this process would need `set_var`, which edition 2024 makes
//! unsafe, and would leak between tests running at once.)

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use gpui::{TestAppContext, WindowHandle};
use russh::keys::PrivateKey;
use russh::keys::ssh_key::Algorithm;
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use shellwing_config::Store;

use crate::look::Live;
use crate::workspace::Workspace;

const USER: &str = "tester";
const PASSWORD: &str = "opensesame";
/// How long a step may take before the scenario fails. Generous: debug
/// builds of libghostty parse slowly.
const STEP: Duration = Duration::from_secs(20);

/// Runs scenario `name` (an ignored test in this module) in a child process
/// whose environment points known_hosts, the ssh config and `HOME` into a
/// fresh scratch directory.
fn in_child(name: &str) {
    let dir = std::env::temp_dir().join(format!(
        "shellwing-ui-{name}-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    std::fs::write(dir.join("ssh_config"), "").expect("ssh config");
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            &format!("ui_tests::{name}"),
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("SHELLWING_UI_TEST_DIR", &dir)
        .env("SHELLWING_KNOWN_HOSTS", dir.join("known_hosts"))
        .env("SHELLWING_SSH_CONFIG", dir.join("ssh_config"))
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
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

/// The scenario's scratch directory, set by [`in_child`].
fn scratch() -> PathBuf {
    std::env::var_os("SHELLWING_UI_TEST_DIR")
        .map(PathBuf::from)
        .expect("run through in_child")
}

/// A shell as small as a test needs: it echoes what is typed, and on Enter
/// answers `you said: <line>`; `exit` ends it.
struct EchoShell {
    line: Vec<u8>,
}

impl server::Handler for EchoShell {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::Password][..])),
                partial_success: false,
            }
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, "welcome to the test shell\r\n$ ")
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        for &b in data {
            match b {
                b'\r' => {
                    let line = String::from_utf8_lossy(&self.line).into_owned();
                    self.line.clear();
                    if line == "exit" {
                        session.data(channel, "\r\n")?;
                        session.exit_status_request(channel, 0)?;
                        session.eof(channel)?;
                        return session.close(channel);
                    }
                    session.data(channel, format!("\r\nyou said: {line}\r\n$ "))?;
                }
                0x7f => {
                    if self.line.pop().is_some() {
                        session.data(channel, "\x08 \x08")?;
                    }
                }
                _ => {
                    self.line.push(b);
                    session.data(channel, vec![b])?;
                }
            }
        }
        Ok(())
    }
}

/// Starts the echo shell's server on 127.0.0.1 in `rt`; its port, and its host
/// key as a known_hosts line.
fn serve(rt: &tokio::runtime::Runtime) -> (u16, String) {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
    let public = key.public_key().to_openssh().expect("public key");
    let config = std::sync::Arc::new(server::Config {
        methods: MethodSet::from(&[MethodKind::Password][..]),
        keys: vec![key],
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let config = config.clone();
            tokio::spawn(async move {
                let shell = EchoShell { line: vec![] };
                if let Ok(running) = server::run_stream(config, socket, shell).await {
                    let _ = running.await;
                }
            });
        }
    });
    (port, format!("[127.0.0.1]:{port} {public}"))
}

/// The app's window with the workspace, as `main` opens it: connecting to
/// `spec` as when given on the command line, or with the host list.
fn open(
    cx: &mut TestAppContext,
    rt: &tokio::runtime::Runtime,
    spec: Option<&str>,
) -> WindowHandle<Workspace> {
    cx.executor().allow_parking();
    let (live, problems) = Live::open(Some(Store::new(scratch().join("config"))));
    let handle = rt.handle().clone();
    let spec = spec.map(str::to_string);
    let window = cx.add_window(move |window, cx| {
        let mut ws = Workspace::new(live, problems, handle, None, cx);
        ws.start(spec.as_deref(), &[], window, cx);
        ws
    });
    window
        .update(cx, |ws, window, cx| {
            let active = ws.active_index();
            ws.activate(active, window, cx);
        })
        .expect("window");
    window
}

/// Waits until the active terminal shows `what`, and returns the screen.
fn wait_for(cx: &mut TestAppContext, window: WindowHandle<Workspace>, what: &str) -> String {
    let start = Instant::now();
    loop {
        cx.run_until_parked();
        let screen = window
            .update(cx, |ws, _, cx| {
                ws.active_view().update(cx, |v, _| v.screen_text())
            })
            .expect("window");
        if screen.contains(what) {
            return screen;
        }
        assert!(
            start.elapsed() < STEP,
            "waited {STEP:?} for {what:?}; the screen:\n{screen}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Types `text` (no spaces: GPUI reads each character as a keystroke) and
/// presses Enter.
fn type_line(cx: &mut TestAppContext, window: WindowHandle<Workspace>, text: &str) {
    cx.simulate_input(window.into(), text);
    cx.simulate_keystrokes(window.into(), "enter");
}

#[test]
fn password_login_and_a_command() {
    in_child("scenario_password_login_and_a_command");
}

/// A known host: the password is asked for in the terminal, typed, and the
/// shell then answers what is typed.
#[test]
#[ignore = "run by password_login_and_a_command, in its own process"]
fn scenario_password_login_and_a_command() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let (port, known) = serve(&rt);
    std::fs::write(scratch().join("known_hosts"), format!("{known}\n")).expect("known_hosts");
    let mut cx = TestAppContext::single();
    let window = open(&mut cx, &rt, Some(&format!("{USER}@127.0.0.1:{port}")));

    wait_for(&mut cx, window, "password:");
    type_line(&mut cx, window, PASSWORD);
    wait_for(&mut cx, window, "welcome to the test shell");
    type_line(&mut cx, window, "hello-shellwing");
    let screen = wait_for(&mut cx, window, "you said: hello-shellwing");
    assert!(
        !screen.contains(PASSWORD),
        "the password was echoed:\n{screen}"
    );
}

#[test]
fn new_host_key_is_asked_about_and_remembered() {
    in_child("scenario_new_host_key_is_asked_about_and_remembered");
}

/// An unknown host: the key's fingerprint is shown, `yes` accepts it, and it
/// is written to known_hosts.
#[test]
#[ignore = "run by new_host_key_is_asked_about_and_remembered, in its own process"]
fn scenario_new_host_key_is_asked_about_and_remembered() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let (port, known) = serve(&rt);
    let mut cx = TestAppContext::single();
    let window = open(&mut cx, &rt, Some(&format!("{USER}@127.0.0.1:{port}")));

    wait_for(&mut cx, window, "continue connecting");
    type_line(&mut cx, window, "yes");
    wait_for(&mut cx, window, "password:");
    type_line(&mut cx, window, PASSWORD);
    wait_for(&mut cx, window, "welcome to the test shell");
    let saved = std::fs::read_to_string(scratch().join("known_hosts")).unwrap_or_default();
    let key = known.split_once(' ').map(|(_, k)| k).unwrap_or_default();
    assert!(
        saved.contains(key),
        "the host key is not in known_hosts:\n{saved}"
    );
}

#[test]
fn connect_from_the_palette() {
    in_child("scenario_connect_from_the_palette");
}

/// No host on the command line: the palette's shortcut opens it, an address
/// typed there and Enter open a tab that connects.
#[test]
#[ignore = "run by connect_from_the_palette, in its own process"]
fn scenario_connect_from_the_palette() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let (port, known) = serve(&rt);
    std::fs::write(scratch().join("known_hosts"), format!("{known}\n")).expect("known_hosts");
    let mut cx = TestAppContext::single();
    let window = open(&mut cx, &rt, None);
    cx.run_until_parked();

    let palette = if cfg!(target_os = "macos") {
        "cmd-k"
    } else {
        "ctrl-shift-k"
    };
    cx.simulate_keystrokes(window.into(), palette);
    type_line(&mut cx, window, &format!("{USER}@127.0.0.1:{port}"));
    wait_for(&mut cx, window, "password:");
    type_line(&mut cx, window, PASSWORD);
    wait_for(&mut cx, window, "welcome to the test shell");
}

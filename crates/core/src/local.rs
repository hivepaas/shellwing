//! A shell on this computer, in a pane as an SSH session is: the user's
//! shell in a PTY (ConPTY on Windows), its output fed to the terminal, and
//! the pane's input (the Writer's session channel, see `writer`) written to
//! it. What only SSH can do (files, forwards, commands beside the shell) is
//! answered with an error.

use std::io::{Read, Write};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use shellwing_ssh::{Error, Input};
use tokio::sync::mpsc::UnboundedReceiver;

/// The spec that opens one; no SSH target is written so.
pub const SPEC: &str = "local:";

/// The tab's name.
pub const TITLE: &str = "local";

/// The shell to run: `command` (the setting) when it names one, else the
/// user's own.
pub fn shell(command: Option<&str>) -> Vec<String> {
    if let Some(c) = command.filter(|c| !c.trim().is_empty()) {
        let words = crate::command::words(c);
        if !words.is_empty() {
            return words;
        }
    }
    default_shell()
}

#[cfg(windows)]
fn default_shell() -> Vec<String> {
    for p in ["pwsh.exe", "powershell.exe"] {
        if crate::command::which(p).is_some() {
            return vec![p.to_string(), "-NoLogo".to_string()];
        }
    }
    vec![std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())]
}

#[cfg(not(windows))]
fn default_shell() -> Vec<String> {
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    // A login shell on macOS, as Terminal starts one: its profile sets PATH.
    if cfg!(target_os = "macos") {
        vec![sh, "-l".into()]
    } else {
        vec![sh]
    }
}

/// What the shell is called, for the host list: `bash`, `zsh`, `pwsh`…
pub fn shell_name(argv: &[String]) -> String {
    argv.first()
        .map(|p| {
            std::path::Path::new(p)
                .file_stem()
                .map_or(p.clone(), |s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// Where the shell's output goes, from a thread of its own.
pub type OutputFn = Box<dyn FnMut(&[u8]) + Send>;

const NOT_SSH: &str = "this pane is a shell on this computer, not an SSH connection";

/// Starts `argv` in a PTY of `size` (columns, rows). Output goes to `out`;
/// `input` brings the keyboard and new sizes, and once it closes (the pane
/// is gone) the shell is stopped. Answers with the exit status once the
/// shell ends.
pub fn start(
    argv: Vec<String>,
    size: (u16, u16),
    input: UnboundedReceiver<Input>,
    mut out: OutputFn,
) -> Result<tokio::sync::oneshot::Receiver<Option<u32>>, String> {
    let (program, args) = argv.split_first().ok_or("no shell to run")?;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: size.1.max(1),
            cols: size.0.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("cannot open a terminal: {e}"))?;
    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    if let Some(home) = std::env::home_dir() {
        cmd.cwd(home);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "shellwing");
    cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    // The shell holds the other end now; ours would keep it open past its end.
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let master = pair.master;
    let mut killer = child.clone_killer();

    // Output, until the shell (and whatever it left running) lets go of the
    // terminal. Feeding blocks while the terminal is behind: backpressure.
    std::thread::Builder::new()
        .name("shellwing-pty-out".into())
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => out(&buf[..n]),
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let (done_tx, done) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("shellwing-pty-wait".into())
        .spawn(move || {
            let status = child.wait().ok().map(|s| s.exit_code());
            let _ = done_tx.send(status);
        })
        .map_err(|e| e.to_string())?;

    // Input, on a thread of its own: a write blocks while the shell reads
    // nothing. The PTY goes with it, which ends ConPTY's output too.
    std::thread::Builder::new()
        .name("shellwing-pty-in".into())
        .spawn(move || {
            let mut input = input;
            while let Some(i) = input.blocking_recv() {
                match i {
                    Input::Data(bytes) => {
                        if writer
                            .write_all(&bytes)
                            .and_then(|()| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Input::Resize { cols, rows } => {
                        let _ = master.resize(PtySize {
                            rows: rows.clamp(1, u16::MAX as u32) as u16,
                            cols: cols.clamp(1, u16::MAX as u32) as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    Input::Forward { forward, reply } => {
                        let _ = reply.send(Err(Error::Forward {
                            spec: forward.to_string(),
                            why: NOT_SSH.into(),
                        }));
                    }
                    Input::Sftp { reply } => {
                        let _ = reply.send(Err(Error::Sftp(NOT_SSH.into())));
                    }
                    Input::Exec { reply, .. } => {
                        let _ = reply.send(Err(Error::Exec(NOT_SSH.into())));
                    }
                }
            }
            // The pane is gone, or the shell ended: stop what is left.
            let _ = killer.kill();
            drop(master);
        })
        .map_err(|e| e.to_string())?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_are_named_and_chosen() {
        assert_eq!(shell_name(&["/usr/bin/zsh".into(), "-l".into()]), "zsh");
        assert_eq!(shell_name(&["pwsh.exe".into()]), "pwsh");
        assert_eq!(shell(Some("fish -l")), ["fish", "-l"]);
        assert!(!shell(None).is_empty());
        assert!(!shell(Some("  ")).is_empty());
    }

    /// A real shell in a real PTY: it reads what is typed, says it, and its
    /// exit status comes back.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_shell_runs_in_a_pty() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let done = start(
            vec!["/bin/sh".into()],
            (80, 24),
            rx,
            Box::new(move |b| {
                let _ = out_tx.send(b.to_vec());
            }),
        )
        .expect("start");
        tx.send(Input::Resize {
            cols: 100,
            rows: 30,
        })
        .expect("resize");
        tx.send(Input::Data(
            b"stty size; echo hello-$((40+2)); exit 7\n".to_vec(),
        ))
        .expect("type");
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), done)
            .await
            .expect("in time")
            .expect("status");
        assert_eq!(status, Some(7));
        let mut seen = String::new();
        while let Ok(b) = out_rx.recv_timeout(std::time::Duration::from_millis(500)) {
            seen.push_str(&String::from_utf8_lossy(&b));
        }
        assert!(seen.contains("hello-42"), "{seen:?}");
        assert!(
            seen.contains("30 100"),
            "the size reached the shell: {seen:?}"
        );
        // What only SSH does is refused, with a reason.
        let (reply, answer) = tokio::sync::oneshot::channel();
        drop(tx.send(Input::Sftp { reply }));
        if let Ok(r) = answer.await {
            assert!(r.is_err());
        }
    }
}

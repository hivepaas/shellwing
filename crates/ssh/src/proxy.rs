//! Reaching a host through a command (`ProxyCommand`), as OpenSSH does: the
//! command's standard input and output carry the connection.

use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::{Error, Resolved};

/// How much of the end of the command's standard error is kept, to say why
/// it failed.
const STDERR_KEEP: usize = 4096;
/// How long a command whose connection failed is given to end and say why.
const LAST_WORDS: Duration = Duration::from_secs(2);

/// `command` with the tokens OpenSSH allows in `ProxyCommand` expanded: `%h`
/// the host name, `%p` the port, `%r` the user, `%n` the name as given, and
/// `%%` a `%`.
pub fn expand(command: &str, hop: &Resolved) -> Result<String, Error> {
    crate::tokens::expand(command, hop, "ProxyCommand", "hprn")
}

/// The connection through a running command. The command is stopped once
/// this and its [`Said`] are dropped.
pub struct Stream {
    out: ChildStdout,
    input: ChildStdin,
    _child: Arc<Mutex<Child>>,
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().out).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().input).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().input).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().input).poll_shutdown(cx)
    }
}

/// What the command writes on its standard error, read as it comes so it
/// never waits on a full pipe: the end of it, to say why it failed.
pub struct Said {
    child: Arc<Mutex<Child>>,
    tail: Arc<Mutex<Vec<u8>>>,
    read: tokio::task::JoinHandle<()>,
}

impl Said {
    /// Why the connection through the command failed, if the command ended:
    /// the last line it wrote on its standard error, else its exit status.
    /// None while it still runs, when the failure was the SSH connection's.
    pub async fn why(self) -> Option<String> {
        let deadline = tokio::time::Instant::now() + LAST_WORDS;
        let status = loop {
            let ended = self.child.lock().ok().and_then(|mut c| c.try_wait().ok()?);
            match ended {
                Some(s) => break s,
                None if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                None => return None,
            }
        };
        // Its standard error, read to the end; something it started may
        // hold it open, so not for longer than the deadline.
        let _ = tokio::time::timeout_at(deadline, self.read).await;
        let tail = self.tail.lock().map(|t| t.clone()).unwrap_or_default();
        let text = String::from_utf8_lossy(&tail);
        let last = text.lines().map(str::trim).rfind(|l| !l.is_empty());
        Some(match last {
            Some(line) => line.to_string(),
            None => status.to_string(),
        })
    }
}

/// Starts `command`, its tokens expanded, as OpenSSH runs it: on Unix with
/// `exec` in the user's shell, so its quotes, variables and pipes work; on
/// Windows the program with the rest of the line given to it as written.
pub fn start(command: &str) -> std::io::Result<(Stream, Said)> {
    #[cfg(unix)]
    let mut c = {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut c = std::process::Command::new(shell);
        c.arg("-c").arg(format!("exec {command}"));
        c
    };
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let (program, rest) = split_program(command);
        let mut c = std::process::Command::new(program);
        if !rest.is_empty() {
            c.raw_arg(rest);
        }
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };
    // Apps opened from the Finder get a PATH without Homebrew's folders,
    // where cloudflared, aws and the like usually are.
    #[cfg(target_os = "macos")]
    if let Some(path) = with_homebrew(std::env::var_os("PATH")) {
        c.env("PATH", path);
    }
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut c = Command::from(c);
    c.kill_on_drop(true);
    // Started with no signals blocked (see shellwing-spawn), from a thread
    // that the runtime's process handling still covers.
    let rt = tokio::runtime::Handle::current();
    let mut child = shellwing_spawn::unblocked(|| {
        let _rt = rt.enter();
        c.spawn()
    })?;
    let (Some(input), Some(out), Some(mut err)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(std::io::Error::other("its pipes were not made"));
    };
    let tail = Arc::new(Mutex::new(Vec::new()));
    let read = tokio::spawn({
        let tail = tail.clone();
        async move {
            let mut buf = [0u8; 1024];
            while let Ok(n @ 1..) = err.read(&mut buf).await {
                if let Ok(mut t) = tail.lock() {
                    t.extend_from_slice(&buf[..n]);
                    let over = t.len().saturating_sub(STDERR_KEEP);
                    t.drain(..over);
                }
            }
        }
    });
    let child = Arc::new(Mutex::new(child));
    Ok((
        Stream {
            out,
            input,
            _child: child.clone(),
        },
        Said { child, tail, read },
    ))
}

/// The program a Windows command line starts (quoted, or up to the first
/// space) and the rest of the line.
#[cfg(any(windows, test))]
fn split_program(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    let (program, rest) = match line.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
        None => line.split_once(char::is_whitespace).unwrap_or((line, "")),
    };
    (program, rest.trim_start())
}

/// `path` with Homebrew's folders at the end, if it lacks them.
#[cfg(any(target_os = "macos", all(unix, test)))]
fn with_homebrew(path: Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    let mut dirs: Vec<std::path::PathBuf> = path
        .as_deref()
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    let before = dirs.len();
    for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !dirs.iter().any(|x| x.as_os_str() == d) {
            dirs.push(d.into());
        }
    }
    (dirs.len() != before)
        .then(|| std::env::join_paths(dirs).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

    fn hop() -> Resolved {
        let mut r = Resolved::direct(Target {
            user: "ec2-user".into(),
            host: "i-0abc".into(),
            port: 2222,
        });
        r.alias = "box".into();
        r
    }

    #[test]
    fn expands_the_tokens_openssh_takes() {
        assert_eq!(
            expand("ssh -W %h:%p %r@%n 100%%", &hop()).expect("expand"),
            "ssh -W i-0abc:2222 ec2-user@box 100%"
        );
        assert!(expand("nc %h %x", &hop()).is_err());
        assert!(expand("nc %h %", &hop()).is_err());
    }

    #[test]
    fn splits_a_windows_command_line() {
        assert_eq!(
            split_program(r#""C:\Program Files\x\x.exe" -W "a b""#),
            (r"C:\Program Files\x\x.exe", r#"-W "a b""#)
        );
        assert_eq!(
            split_program("powershell.exe  \"aws ssm start-session\""),
            ("powershell.exe", "\"aws ssm start-session\"")
        );
        assert_eq!(split_program("cloudflared"), ("cloudflared", ""));
    }

    // Unix: PATH's separator is `:` (`;` on Windows).
    #[cfg(unix)]
    #[test]
    fn adds_homebrew_once() {
        let p = with_homebrew(Some("/usr/bin:/opt/homebrew/bin".into())).expect("added");
        assert_eq!(p, "/usr/bin:/opt/homebrew/bin:/usr/local/bin");
        assert_eq!(with_homebrew(Some(p)), None);
    }
}

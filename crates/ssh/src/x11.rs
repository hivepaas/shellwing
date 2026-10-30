//! X11 forwarding (`ForwardX11`), as OpenSSH does it. The server is given a
//! made-up cookie. Each connection it opens back here has that cookie swapped
//! for the real one before it reaches the X server, so the real one never
//! leaves this machine, and a connection without the made-up one goes no
//! further.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use russh::Channel;
use russh::client::Msg;
use shellwing_spawn::Unblocked;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::config::X11;

/// What OpenSSH's `generate` adds to the timeout, so the cookie outlives the
/// time connections are let in.
const COOKIE_SLACK: Duration = Duration::from_secs(60);
/// Where `xauth` is if it is not named and not on `PATH`.
const XAUTH_PLACES: [&str; 3] = [
    "/usr/bin/xauth",
    "/usr/X11R6/bin/xauth",
    "/opt/X11/bin/xauth",
];
const COOKIE_LEN: usize = 16;

/// Where an X server listens.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Place {
    Socket(PathBuf),
    Tcp(String, u16),
}

/// `$DISPLAY`, read as X clients and OpenSSH read it: `:0` and `unix:0` are
/// the local socket, `/path/to/socket:0` a socket by its path (XQuartz), and
/// `host:0` the TCP port 6000 + 0. The screen comes after a dot.
fn parse_display(display: &str) -> Option<(Place, u32)> {
    let (host, rest) = display.rsplit_once(':')?;
    let (number, screen) = rest.split_once('.').unwrap_or((rest, "0"));
    let number: u32 = number.parse().ok()?;
    let screen = screen.parse().ok()?;
    let place = if host.is_empty() || host == "unix" {
        Place::Socket(PathBuf::from(format!("/tmp/.X11-unix/X{number}")))
    } else if host.starts_with('/') {
        Place::Socket(PathBuf::from(host))
    } else {
        Place::Tcp(host.to_string(), u16::try_from(6000 + number).ok()?)
    };
    Some((place, screen))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The first `protocol cookie` a `xauth list` printed.
fn cookie_listed(listed: &str) -> Option<(String, Vec<u8>)> {
    listed.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let (_, protocol, cookie) = (words.next()?, words.next()?, words.next()?);
        Some((protocol.to_string(), unhex(cookie)?))
    })
}

/// The `xauth` to run: the one named (`XAuthLocation`), else on `PATH`, else
/// where X installs it; an app opened from the Finder has a short `PATH`.
fn xauth_program(named: Option<&Path>) -> PathBuf {
    if let Some(p) = named {
        return p.to_path_buf();
    }
    let on_path = std::env::var_os("PATH").and_then(|p| {
        let name = if cfg!(windows) { "xauth.exe" } else { "xauth" };
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file())
    });
    on_path
        .or_else(|| XAUTH_PLACES.iter().map(PathBuf::from).find(|p| p.is_file()))
        .unwrap_or_else(|| PathBuf::from("xauth"))
}

fn run_xauth(program: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output_unblocked()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The cookie for `display`: a limited one made for this session if not
/// `trusted` (the X server's SECURITY extension), else the display's own.
/// None where it has none: an X server open to this machine needs none.
fn real_cookie(
    settings: &X11,
    display: &str,
    dir: &Path,
) -> Result<Option<(String, Vec<u8>)>, String> {
    let program = xauth_program(settings.xauth.as_deref());
    // As OpenSSH: `localhost:0` is in the file as the local display.
    let listed_as = match display.strip_prefix("localhost:") {
        Some(rest) => format!("unix:{rest}"),
        None => display.to_string(),
    };
    if settings.trusted {
        let listed = run_xauth(&program, &["list", &listed_as]).unwrap_or_default();
        return Ok(cookie_listed(&listed));
    }
    let file = dir.join("xauthfile");
    let file = file.to_string_lossy();
    let timeout = if settings.timeout.is_zero() {
        // None asked for: as long as xauth lets it, about 136 years.
        u32::MAX as u64
    } else {
        (settings.timeout + COOKIE_SLACK).as_secs()
    };
    run_xauth(
        &program,
        &[
            "-f",
            &file,
            "generate",
            display,
            "MIT-MAGIC-COOKIE-1",
            "untrusted",
            "timeout",
            &timeout.to_string(),
        ],
    )
    .map_err(|why| {
        format!("no limited cookie ({why}); ForwardX11Trusted yes would use the display's own")
    })?;
    let listed = run_xauth(&program, &["-f", &file, "list"])?;
    Ok(cookie_listed(&listed))
}

/// X11 forwarding made ready for one session.
pub(crate) struct Forwarding {
    place: Place,
    pub(crate) screen: u32,
    pub(crate) protocol: String,
    /// What the server is given and its connections must show.
    fake: Vec<u8>,
    /// What the X server gets instead; the same length.
    real: Vec<u8>,
    /// No more connections after this (`ForwardX11Timeout`); a limited cookie
    /// has stopped working by then anyway.
    until: Option<Instant>,
}

impl Forwarding {
    /// Reads `$DISPLAY` and gets its cookie. Why not, if it cannot.
    pub(crate) fn prepare(settings: &X11) -> Result<Forwarding, String> {
        let display = std::env::var("DISPLAY")
            .ok()
            .filter(|d| !d.is_empty())
            .ok_or("DISPLAY is not set: no X server to show windows on")?;
        let (place, screen) =
            parse_display(&display).ok_or_else(|| format!("cannot read DISPLAY {display:?}"))?;
        let dir = std::env::temp_dir().join(format!(
            "shellwing-x11-{}-{}",
            std::process::id(),
            hex(&rand::random::<[u8; 8]>())
        ));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let cookie = real_cookie(settings, &display, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        // No cookie: the server's connections go on with a made-up one, which
        // only an X server open to this machine lets in, as with ssh.
        let made_up = |len: usize| -> Vec<u8> {
            let mut bytes = vec![0u8; len];
            for chunk in bytes.chunks_mut(16) {
                let random = rand::random::<[u8; 16]>();
                chunk.copy_from_slice(&random[..chunk.len()]);
            }
            bytes
        };
        let (protocol, real) =
            cookie?.unwrap_or_else(|| ("MIT-MAGIC-COOKIE-1".to_string(), made_up(COOKIE_LEN)));
        Ok(Forwarding {
            place,
            screen,
            protocol,
            fake: made_up(real.len()),
            real,
            until: (!settings.timeout.is_zero()).then(|| Instant::now() + settings.timeout),
        })
    }

    /// The made-up cookie, as the `x11-req` request carries it.
    pub(crate) fn fake_hex(&self) -> String {
        hex(&self.fake)
    }

    /// Carries one connection from the server to the X server.
    pub(crate) async fn carry(&self, channel: Channel<Msg>) -> std::io::Result<()> {
        if self.until.is_some_and(|until| Instant::now() > until) {
            return Err(std::io::Error::other("ForwardX11Timeout has passed"));
        }
        let mut remote = channel.into_stream();
        let setup = self.swap_cookie(&mut remote).await?;
        match &self.place {
            #[cfg(unix)]
            Place::Socket(path) => {
                let mut local = tokio::net::UnixStream::connect(path).await?;
                local.write_all(&setup).await?;
                tokio::io::copy_bidirectional(&mut remote, &mut local).await?;
            }
            #[cfg(not(unix))]
            Place::Socket(path) => {
                return Err(std::io::Error::other(format!(
                    "no X server socket at {} here",
                    path.display()
                )));
            }
            Place::Tcp(host, port) => {
                let mut local = tokio::net::TcpStream::connect((host.as_str(), *port)).await?;
                local.write_all(&setup).await?;
                tokio::io::copy_bidirectional(&mut remote, &mut local).await?;
            }
        }
        Ok(())
    }

    /// Reads the connection's first message and returns it with the real
    /// cookie in place of the made-up one, which it must show.
    async fn swap_cookie<S>(&self, remote: &mut S) -> std::io::Result<Vec<u8>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        // The X11 connection setup: byte order, one unused, the protocol's
        // version (4), the lengths of the cookie's name and its data (4), two
        // unused; then the name and the data, each padded to 4 bytes.
        let mut head = [0u8; 12];
        remote.read_exact(&mut head).await?;
        let length = |at: usize| -> usize {
            let bytes = [head[at], head[at + 1]];
            usize::from(match head[0] {
                b'B' => u16::from_be_bytes(bytes),
                _ => u16::from_le_bytes(bytes),
            })
        };
        let (name_len, data_len) = (length(6), length(8));
        if !matches!(head[0], b'B' | b'l') {
            return Err(std::io::Error::other("not an X11 connection"));
        }
        let padded = |n: usize| n.next_multiple_of(4);
        let mut rest = vec![0u8; padded(name_len) + padded(data_len)];
        remote.read_exact(&mut rest).await?;
        let name = &rest[..name_len];
        let data_at = padded(name_len);
        if name != self.protocol.as_bytes() || rest[data_at..data_at + data_len] != self.fake[..] {
            return Err(std::io::Error::other(
                "an X11 connection without the cookie the server was given",
            ));
        }
        rest[data_at..data_at + data_len].copy_from_slice(&self.real);
        let mut setup = head.to_vec();
        setup.append(&mut rest);
        Ok(setup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_as_x_clients_read_them() {
        let socket = |n: u32| Place::Socket(PathBuf::from(format!("/tmp/.X11-unix/X{n}")));
        assert_eq!(parse_display(":0"), Some((socket(0), 0)));
        assert_eq!(parse_display(":1.2"), Some((socket(1), 2)));
        assert_eq!(parse_display("unix:3"), Some((socket(3), 0)));
        assert_eq!(
            parse_display("localhost:10.0"),
            Some((Place::Tcp("localhost".into(), 6010), 0))
        );
        assert_eq!(
            parse_display("/private/tmp/com.apple.launchd.x/org.xquartz:0"),
            Some((
                Place::Socket("/private/tmp/com.apple.launchd.x/org.xquartz".into()),
                0
            ))
        );
        assert_eq!(parse_display("nothing"), None);
        assert_eq!(parse_display(":x"), None);
        assert_eq!(parse_display("host:70000"), None);
    }

    #[test]
    fn the_first_cookie_listed_is_read() {
        assert_eq!(
            cookie_listed("box/unix:0  MIT-MAGIC-COOKIE-1  00ff10\nother:1 X y\n"),
            Some(("MIT-MAGIC-COOKIE-1".to_string(), vec![0x00, 0xff, 0x10]))
        );
        assert_eq!(cookie_listed(""), None);
        assert_eq!(cookie_listed("box/unix:0  MIT-MAGIC-COOKIE-1  0g\n"), None);
    }

    fn forwarding() -> Forwarding {
        Forwarding {
            place: Place::Tcp("localhost".into(), 6000),
            screen: 0,
            protocol: "MIT-MAGIC-COOKIE-1".into(),
            fake: vec![1; COOKIE_LEN],
            real: vec![2; COOKIE_LEN],
            until: None,
        }
    }

    /// An X11 setup message, little-endian, with `cookie`.
    fn setup(cookie: &[u8]) -> Vec<u8> {
        let name = b"MIT-MAGIC-COOKIE-1";
        let mut m = vec![b'l', 0, 11, 0, 0, 0];
        m.extend((name.len() as u16).to_le_bytes());
        m.extend((cookie.len() as u16).to_le_bytes());
        m.extend([0, 0]);
        m.extend(name);
        m.extend([0, 0]); // to 4 bytes
        m.extend(cookie);
        m
    }

    #[tokio::test]
    async fn the_made_up_cookie_is_swapped_for_the_real_one() {
        let f = forwarding();
        let (mut ours, mut theirs) = tokio::io::duplex(256);
        theirs.write_all(&setup(&f.fake)).await.expect("write");
        let swapped = f.swap_cookie(&mut ours).await.expect("swap");
        assert_eq!(swapped, setup(&f.real));
    }

    #[tokio::test]
    async fn any_other_cookie_goes_no_further() {
        let f = forwarding();
        let (mut ours, mut theirs) = tokio::io::duplex(256);
        theirs
            .write_all(&setup(&[9; COOKIE_LEN]))
            .await
            .expect("write");
        assert!(f.swap_cookie(&mut ours).await.is_err());
        // Not even the real one: only the server's own is let through.
        let (mut ours, mut theirs) = tokio::io::duplex(256);
        theirs.write_all(&setup(&f.real)).await.expect("write");
        assert!(f.swap_cookie(&mut ours).await.is_err());
    }
}

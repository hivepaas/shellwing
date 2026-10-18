//! SSH sessions for shellwing, over russh.
//!
//! - [`config`] resolves a target through `~/.ssh/config`, including ProxyJump.
//! - [`hostkey`] checks host keys like OpenSSH and, by default, asks before
//!   trusting a new one; nothing is sent before the user answers.
//! - Authentication follows OpenSSH's order: agent, key files (passphrases are
//!   asked for), keyboard-interactive, password.
//! - Every question goes through a [`Prompter`], so the app decides how to ask.

mod auth;
pub mod config;
pub mod hostkey;
pub mod prompt;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::mpsc;

pub use config::Resolved;
use hostkey::Checker;
pub use hostkey::HostKeyPolicy;
pub use prompt::{Prompt, PromptRequest, Prompter};

/// Used when the config sets no ServerAliveInterval.
const DEFAULT_KEEPALIVE: Duration = Duration::from_secs(30);
/// Unanswered keepalives before the connection counts as dead.
const KEEPALIVE_MAX: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl Target {
    /// Parses `[user@]host[:port]`; the user defaults to `$USER`.
    pub fn parse(s: &str) -> Result<Self, Error> {
        let (user, rest) = match s.rsplit_once('@') {
            Some((u, r)) => (u.to_string(), r),
            // USER on Unix, USERNAME on Windows.
            None => (
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "root".into()),
                s,
            ),
        };
        let (host, port) = match rest.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (
                h.to_string(),
                p.parse().map_err(|_| Error::BadTarget(s.into()))?,
            ),
            _ => (rest.to_string(), 22),
        };
        if host.is_empty() || user.is_empty() {
            return Err(Error::BadTarget(s.into()));
        }
        Ok(Target { user, host, port })
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.port == 22 {
            write!(f, "{}@{}", self.user, self.host)
        } else {
            write!(f, "{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Auth {
    /// Try the keys of the platform's ssh-agent.
    pub agent: bool,
    /// Tried once before asking (scripts, benchmarks).
    pub password: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a valid target: {0:?} (expected [user@]host[:port])")]
    BadTarget(String),
    #[error("ssh config: {0}")]
    Config(String),
    #[error(
        "the host key for {host} has changed (known_hosts line {line}). If you trust the new key, run: ssh-keygen -R {host}"
    )]
    HostKeyChanged { host: String, line: usize },
    #[error("{host} is not in known_hosts")]
    HostKeyUnknown { host: String },
    #[error("host key for {host} not accepted")]
    HostKeyRejected { host: String },
    #[error("{host} presented a host certificate; certificates are not supported yet")]
    HostCertificate { host: String },
    #[error("permission denied: the server accepted none of the keys or passwords tried")]
    AuthFailed,
    #[error("cancelled")]
    Cancelled,
    #[error("via {host}: {source}")]
    Jump {
        host: String,
        #[source]
        source: Box<Error>,
    },
    #[error(transparent)]
    Ssh(#[from] russh::Error),
    #[error(transparent)]
    Keys(#[from] russh::keys::Error),
}

/// Input for a running session.
pub enum Input {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

/// A connected interactive shell. Dropping every sender closes the session.
pub struct Session {
    tx: mpsc::UnboundedSender<Input>,
}

impl Session {
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Input::Data(bytes));
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        let _ = self.tx.send(Input::Resize { cols, rows });
    }

    /// A sender that can be moved to other threads.
    pub fn sender(&self) -> mpsc::UnboundedSender<Input> {
        self.tx.clone()
    }
}

/// Why the session ended.
#[derive(Debug)]
pub enum Ended {
    Closed { exit_status: Option<u32> },
    Error(Error),
}

/// Host output, called on a Tokio worker thread. It may block briefly; that is how
/// a slow consumer slows the server down.
pub type OutputFn = Box<dyn FnMut(&[u8]) + Send>;
pub type EndedFn = Box<dyn FnOnce(Ended) + Send>;

pub struct Options {
    pub target: Resolved,
    pub auth: Auth,
    pub policy: HostKeyPolicy,
    /// Override for `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    /// Where questions go (passwords, passphrases, codes, new host keys). Without
    /// one, only non-interactive methods are tried and unknown hosts are refused.
    pub prompter: Option<Prompter>,
    pub cols: u32,
    pub rows: u32,
    pub term: String,
}

/// Connects to one hop, over TCP or through the previous hop, and authenticates.
async fn open_hop(
    hop: &Resolved,
    via: Option<&Handle<Checker>>,
    opts: &Options,
) -> Result<Handle<Checker>, Error> {
    let problem = Arc::new(Mutex::new(None));
    let checker = Checker {
        host: hop.target.host.clone(),
        port: hop.target.port,
        policy: opts.policy,
        known_hosts: opts.known_hosts.clone(),
        prompter: opts.prompter.clone(),
        problem: problem.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(hop.keepalive.unwrap_or(DEFAULT_KEEPALIVE)),
        keepalive_max: KEEPALIVE_MAX,
        ..Default::default()
    });
    let connected = match via {
        None => client::connect(config, (hop.target.host.as_str(), hop.target.port), checker).await,
        Some(prev) => {
            let channel = prev
                .channel_open_direct_tcpip(
                    hop.target.host.clone(),
                    hop.target.port as u32,
                    "127.0.0.1",
                    0,
                )
                .await?;
            client::connect_stream(config, channel.into_stream(), checker).await
        }
    };
    let mut handle = connected.map_err(|e| {
        problem
            .lock()
            .ok()
            .and_then(|mut p| p.take())
            .unwrap_or(Error::Ssh(e))
    })?;
    auth::authenticate(
        &mut handle,
        &hop.target.user,
        &hop.target.host,
        &hop.keys_to_try(),
        &opts.auth,
        opts.prompter.as_ref(),
    )
    .await?;
    Ok(handle)
}

/// Connects (through any ProxyJump hosts), authenticates, opens a PTY shell and
/// starts pumping output into `output`. `ended` is called once when the session finishes.
pub async fn connect(
    opts: Options,
    mut output: OutputFn,
    ended: EndedFn,
) -> Result<Session, Error> {
    let mut jumps: Vec<Handle<Checker>> = Vec::new();
    for hop in &opts.target.jumps {
        let h = open_hop(hop, jumps.last(), &opts)
            .await
            .map_err(|e| Error::Jump {
                host: hop.alias.clone(),
                source: Box::new(e),
            })?;
        jumps.push(h);
    }
    let handle = open_hop(&opts.target, jumps.last(), &opts).await?;

    let mut channel = handle.channel_open_session().await?;
    channel
        .request_pty(false, &opts.term, opts.cols, opts.rows, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Input>();
    tokio::spawn(async move {
        // The jump connections carry this one; keep them until the session ends.
        let _jumps = jumps;
        let mut exit_status = None;
        let result: Result<(), Error> = async {
            loop {
                tokio::select! {
                    msg = channel.wait() => match msg {
                        Some(ChannelMsg::Data { data }) => {
                            tokio::task::block_in_place(|| output(&data));
                        }
                        Some(ChannelMsg::ExitStatus { exit_status: s }) => exit_status = Some(s),
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        _ => {}
                    },
                    input = rx.recv() => match input {
                        Some(Input::Data(b)) => channel.data(&b[..]).await?,
                        Some(Input::Resize { cols, rows }) => channel.window_change(cols, rows, 0, 0).await?,
                        None => break,
                    },
                }
            }
            Ok(())
        }
        .await;
        let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
        ended(match result {
            Ok(()) => Ended::Closed { exit_status },
            Err(e) => Ended::Error(e),
        });
    });
    Ok(Session { tx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets() {
        assert_eq!(
            Target::parse("me@h:2222").ok(),
            Some(Target {
                user: "me".into(),
                host: "h".into(),
                port: 2222
            })
        );
        assert_eq!(Target::parse("me@h").ok().map(|t| t.port), Some(22));
        assert!(Target::parse("me@").is_err());
        assert!(Target::parse("me@h:x").is_err());
    }
}

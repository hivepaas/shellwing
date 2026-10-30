//! SSH sessions for shellwing, over russh.
//!
//! - [`config`] resolves a target through `~/.ssh/config`, including ProxyJump.
//! - [`proxy`] reaches a host through a `ProxyCommand`.
//! - [`hostkey`] checks host keys like OpenSSH and, by default, asks before
//!   trusting a new one; nothing is sent before the user answers.
//! - Authentication follows OpenSSH's order: agent, key files (passphrases are
//!   asked for), keyboard-interactive, password.
//! - Every question goes through a [`Prompter`], so the app decides how to ask.
//! - [`forward`] forwards ports as `-L`, `-R` and `-D` do, for as long as the
//!   session runs.
//! - [`sftp`] browses and transfers files on a channel beside the shell.
//! - [`keys`] makes, reads and writes keys, and adds one to a server.

mod auth;
pub mod config;
pub mod forward;
pub mod hostkey;
pub mod keys;
pub mod known_hosts;
pub mod prompt;
pub mod proxy;
pub mod secrets;
pub mod sftp;
mod tokens;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::mpsc;

pub use config::Resolved;
pub use forward::{Active, Forward};
use forward::{Forwarder, Routes};
use hostkey::Checker;
pub use hostkey::HostKeyPolicy;
pub use prompt::{Prompt, PromptRequest, Prompter};
pub use secrets::{Learned, Secrets};
pub use sftp::Sftp;

/// Used when the config sets no ServerAliveInterval.
const DEFAULT_KEEPALIVE: Duration = Duration::from_secs(30);
/// Unanswered keepalives before the connection counts as dead.
const KEEPALIVE_MAX: usize = 3;
/// How long [`Input::Ping`] waits for the server to answer.
const PING: Duration = Duration::from_secs(5);
/// How much a channel may receive before the server waits for us: 8 MiB, not
/// russh's and OpenSSH's 2 MiB, which hold a download to 2 MiB per round trip
/// (18 MB/s at 50 ms; 65 MB/s with this). It applies to the shell too, where it
/// bounds what still arrives after Ctrl+C in a flood (about 50 ms of parsing).
const WINDOW: u32 = 8 * 1024 * 1024;

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
    /// The agent's socket instead of the host's `IdentityAgent` or
    /// `$SSH_AUTH_SOCK` (tests), for signing in
    /// and for forwarding.
    pub agent_socket: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a valid target: {0:?} (expected [user@]host[:port])")]
    BadTarget(String),
    #[error("ssh config: {0}")]
    Config(String),
    #[error(
        "the host key for {} has changed: known_hosts line {} has {}, the server now shows {}",
        .0.label, .0.line, .0.old.first().map_or("another key", String::as_str), .0.fingerprint
    )]
    HostKeyChanged(Box<ChangedKey>),
    #[error("the host key for {host} is revoked in known_hosts")]
    HostKeyRevoked { host: String },
    #[error("{host} is not in known_hosts")]
    HostKeyUnknown { host: String },
    #[error("host key for {host} not accepted")]
    HostKeyRejected { host: String },
    #[error("{host}'s host certificate is not accepted: {why}")]
    HostCertificate { host: String, why: String },
    #[error("permission denied: the server accepted none of the keys or passwords tried")]
    AuthFailed,
    #[error("cancelled")]
    Cancelled,
    /// The connection went away without the server closing the session, as when
    /// the server restarts or the network drops.
    #[error("the connection was closed")]
    Disconnected,
    #[error("bad forward {spec:?}: {why}")]
    BadForward { spec: String, why: String },
    #[error("cannot forward {spec}: {why}")]
    Forward { spec: String, why: String },
    #[error("{0}")]
    Sftp(String),
    #[error("key: {0}")]
    Key(String),
    #[error("{0}")]
    Exec(String),
    #[error("{host} did not answer within {} s (ConnectTimeout)", .after.as_secs())]
    Timeout { host: String, after: Duration },
    #[error("ProxyCommand {command:?} failed: {why}")]
    Proxy { command: String, why: String },
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
    Resize {
        cols: u32,
        rows: u32,
    },
    /// Starts forwarding a port while the session runs.
    Forward {
        forward: Forward,
        reply: tokio::sync::oneshot::Sender<Result<Active, Error>>,
    },
    /// Opens SFTP on a channel of its own.
    Sftp {
        reply: tokio::sync::oneshot::Sender<Result<Sftp, Error>>,
    },
    /// Runs a command on a channel of its own, with `stdin` as its input.
    Exec {
        command: String,
        stdin: Vec<u8>,
        reply: tokio::sync::oneshot::Sender<Result<Exec, Error>>,
    },
    /// Whether the connection still answers: a channel asked for, and closed,
    /// within [`PING`]. Keepalives find a dead one in minutes; a phone back
    /// from the background, whose connection may have died unheard while it
    /// slept, asks at once.
    Ping {
        reply: tokio::sync::oneshot::Sender<bool>,
    },
}

/// A host whose key is not the one known_hosts has: what is known, and what
/// the server showed, to trust it in place of the old one if the user is
/// sure (see [`known_hosts::replace`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedKey {
    /// The known_hosts file the old key is in (`UserKnownHostsFile`).
    pub file: PathBuf,
    /// `host`, or `[host]:port`.
    pub label: String,
    pub host: String,
    pub port: u16,
    /// The first known_hosts line with the old key, from 1.
    pub line: usize,
    /// The fingerprints known_hosts has for the host.
    pub old: Vec<String>,
    /// The server's key: its fingerprint, kind, and line as known_hosts takes it.
    pub fingerprint: String,
    pub kind: String,
    pub key: String,
}

/// How a command run beside the shell ended, and what it printed (the first
/// [`EXEC_KEEP`] bytes of each stream).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exec {
    pub status: Option<u32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub const EXEC_KEEP: usize = 64 * 1024;

async fn exec(handle: &Handle<Checker>, command: &str, stdin: &[u8]) -> Result<Exec, Error> {
    let mut ch = handle.channel_open_session().await?;
    ch.exec(true, command).await?;
    if !stdin.is_empty() {
        ch.data(stdin).await?;
    }
    ch.eof().await?;
    let mut out = Exec::default();
    let keep = |buf: &mut Vec<u8>, data: &[u8]| {
        let room = EXEC_KEEP.saturating_sub(buf.len());
        buf.extend_from_slice(&data[..data.len().min(room)]);
    };
    while let Some(msg) = ch.wait().await {
        match msg {
            ChannelMsg::Data { data } => keep(&mut out.stdout, &data),
            ChannelMsg::ExtendedData { data, ext: 1 } => keep(&mut out.stderr, &data),
            ChannelMsg::ExitStatus { exit_status } => out.status = Some(exit_status),
            ChannelMsg::Failure => {
                return Err(Error::Exec("the server would not run a command".into()));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// What the session sends the host as its environment: this machine's
/// variables that `SendEnv` names, then `SetEnv`'s.
fn environment(target: &Resolved) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(name, _)| target.send_env.iter().any(|p| config::wildcard(p, name)))
        .collect();
    vars.sort();
    vars.extend(target.set_env.iter().cloned());
    vars
}

/// `data` with each `\n` after a `\r`, as a terminal's line discipline
/// writes output (ONLCR).
fn with_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 16);
    for &b in data {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
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
/// How each of the target's forwards went: running, or why not.
pub type ForwardedFn = Box<dyn FnMut(Result<Active, Error>) + Send + Sync>;
/// The terminal's size (columns, rows), if it has one yet.
pub type SizeFn = Box<dyn Fn() -> Option<(u32, u32)> + Send + Sync>;

pub struct Options {
    pub target: Resolved,
    pub auth: Auth,
    pub policy: HostKeyPolicy,
    /// Override for `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    /// Where questions go (passwords, passphrases, codes, new host keys). Without
    /// one, only non-interactive methods are tried and unknown hosts are refused.
    pub prompter: Option<Prompter>,
    /// Saved passwords and passphrases, tried before asking.
    pub secrets: Option<std::sync::Arc<dyn Secrets>>,
    /// Told about each of `target.forwards` once it is set up, before the shell
    /// starts, so reports come before any output.
    pub forwarded: Option<ForwardedFn>,
    /// Keepalive for hosts that set none; zero: none. None: 30 seconds.
    pub keepalive: Option<Duration>,
    /// The terminal's size when the PTY is asked for, after signing in: it
    /// can have changed since connecting began. `cols` and `rows` without
    /// one, or while it has none.
    pub size: Option<SizeFn>,
    pub cols: u32,
    pub rows: u32,
    pub term: String,
}

/// Connects to one hop, over TCP, its ProxyCommand or the previous hop, and
/// authenticates.
async fn open_hop(
    hop: &Resolved,
    via: Option<&Handle<Checker>>,
    opts: &Options,
    routes: Routes,
    forward_agent: bool,
) -> Result<Handle<Checker>, Error> {
    let problem = Arc::new(Mutex::new(None));
    let key_shown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // The agent the host's IdentityAgent names, unless one is given (tests).
    let agent = match &opts.auth.agent_socket {
        Some(socket) => config::IdentityAgent::At(socket.clone()),
        None => hop.identity_agent.clone(),
    };
    let checker = Checker {
        host: hop.target.host.clone(),
        port: hop.target.port,
        policy: hop.host_key.unwrap_or(opts.policy),
        known_hosts: opts.known_hosts.clone().or_else(|| hop.known_hosts.clone()),
        prompter: opts.prompter.clone(),
        problem: problem.clone(),
        routes,
        forward_agent,
        agent: agent.clone(),
        key_shown: key_shown.clone(),
    };
    let keepalive = hop
        .keepalive
        .or(opts.keepalive)
        .unwrap_or(DEFAULT_KEEPALIVE);
    // A host known_hosts trusts an authority for is asked for a certificate
    // first, as OpenSSH asks; others for a plain key.
    let mut preferred = russh::Preferred::DEFAULT;
    if !checker.authorities().is_empty() {
        preferred.host_key_certificates = std::borrow::Cow::Owned(preferred.key.to_vec());
    }
    let config = Arc::new(client::Config {
        keepalive_interval: (!keepalive.is_zero()).then_some(keepalive),
        keepalive_max: KEEPALIVE_MAX,
        window_size: WINDOW,
        preferred,
        ..Default::default()
    });
    // A ProxyCommand's process starts first: what fails there is its own error.
    let mut through = None;
    let stream = match (via, &hop.proxy_command) {
        (None, Some(command)) => {
            let command = proxy::expand(command, hop)?;
            // A phone runs no other program: the host is for a computer.
            if cfg!(any(target_os = "ios", target_os = "android")) {
                return Err(Error::Proxy {
                    command,
                    why: "commands run on computers, not phones; connect from a computer".into(),
                });
            }
            let (stream, said) = proxy::start(&command).map_err(|e| Error::Proxy {
                command: command.clone(),
                why: e.to_string(),
            })?;
            through = Some((command, said));
            Some(stream)
        }
        _ => None,
    };
    let connecting = async {
        match (via, stream) {
            (None, Some(stream)) => client::connect_stream(config, stream, checker).await,
            (None, None) => {
                client::connect(config, (hop.target.host.as_str(), hop.target.port), checker).await
            }
            (Some(prev), _) => {
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
        }
    };
    // ConnectTimeout, as OpenSSH counts it: the connection and the server's
    // first words, up to its key; not a question about the key.
    let connected = match hop.connect_timeout {
        Some(limit) => {
            let given_up = async {
                tokio::time::sleep(limit).await;
                if key_shown.load(std::sync::atomic::Ordering::Relaxed) {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                c = connecting => c,
                () = given_up => {
                    return Err(Error::Timeout {
                        host: hop.target.host.clone(),
                        after: limit,
                    });
                }
            }
        }
        None => connecting.await,
    };
    let mut handle = match connected {
        Ok(h) => h,
        Err(e) => {
            if let Some(p) = problem.lock().ok().and_then(|mut p| p.take()) {
                return Err(p);
            }
            // A command that ended says why better than the connection it
            // took down with it.
            if let Some((command, said)) = through
                && let Some(why) = said.why().await
            {
                return Err(Error::Proxy { command, why });
            }
            return Err(Error::Ssh(e));
        }
    };
    auth::authenticate(
        &mut handle,
        &hop.target,
        &hop.keys_to_try(),
        &hop.certificate_files,
        auth::StoredKeys::of(hop),
        &opts.auth,
        &agent,
        hop.identities_only,
        opts.prompter.as_ref(),
        opts.secrets.as_deref(),
    )
    .await?;
    Ok(handle)
}

/// Connects (through any ProxyJump hosts), authenticates, forwards the target's
/// ports, opens a PTY shell and starts pumping output into `output`. `ended` is
/// called once when the session finishes.
pub async fn connect(
    mut opts: Options,
    mut output: OutputFn,
    ended: EndedFn,
) -> Result<Session, Error> {
    let mut jumps: Vec<Handle<Checker>> = Vec::new();
    for hop in &opts.target.jumps {
        // Jump hosts never get the agent: only the target asked for it.
        let h = open_hop(hop, jumps.last(), &opts, Routes::default(), false)
            .await
            .map_err(|e| Error::Jump {
                host: hop.alias.clone(),
                source: Box::new(e),
            })?;
        jumps.push(h);
    }
    let routes = Routes::default();
    let forward_agent = opts.target.forward_agent;
    let handle = Arc::new(
        open_hop(
            &opts.target,
            jumps.last(),
            &opts,
            routes.clone(),
            forward_agent,
        )
        .await?,
    );

    // As OpenSSH, a forward that cannot be set up is reported, and the session goes on.
    let mut forwarder = Forwarder::new(handle.clone(), routes);
    for spec in &opts.target.forwards {
        let result = match spec.parse::<Forward>() {
            Ok(f) => forwarder.start(&f).await,
            Err(e) => Err(e),
        };
        if let Some(report) = opts.forwarded.as_mut() {
            report(result);
        }
    }

    let mut channel = handle.channel_open_session().await?;
    let tty = opts.target.tty;
    if tty {
        let (cols, rows) = opts
            .size
            .as_ref()
            .and_then(|now| now())
            .unwrap_or((opts.cols, opts.rows));
        channel
            .request_pty(false, &opts.term, cols, rows, 0, 0, &[])
            .await?;
    }
    // A host takes only the variables its AcceptEnv names, and ignores the
    // rest without a word, as with ssh.
    for (name, value) in environment(&opts.target) {
        channel.set_env(false, name, value).await?;
    }
    if forward_agent {
        channel.agent_forward(false).await?;
    }
    match &opts.target.remote_command {
        Some(command) => {
            let command = tokens::expand(command, &opts.target, "RemoteCommand", "hprndu")?;
            channel.exec(false, command).await?;
        }
        None => channel.request_shell(false).await?,
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<Input>();
    tokio::spawn(async move {
        // The jump connections carry this one; keep them until the session ends.
        let _jumps = jumps;
        let (mut exit_status, mut eof) = (None, false);
        let result: Result<(), Error> = async {
            // A session that is closed in order says so (Eof, Close); the channel
            // just vanishing means the connection died.
            loop {
                tokio::select! {
                    msg = channel.wait() => match msg {
                        // Without a PTY (RequestTTY no) the errors come apart,
                        // and each line ends in a bare \n: a terminal's own
                        // line discipline would add the \r.
                        Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if tty {
                                tokio::task::block_in_place(|| output(&data));
                            } else {
                                let lines = with_crlf(&data);
                                tokio::task::block_in_place(|| output(&lines));
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status: s }) => exit_status = Some(s),
                        // OpenSSH sends the exit status after the output's
                        // end (Eof), so only Close ends the session.
                        Some(ChannelMsg::Eof) => eof = true,
                        Some(ChannelMsg::Close) => break,
                        // Gone after its output ended: closed, if not in order.
                        None if eof => break,
                        None => return Err(Error::Disconnected),
                        _ => {}
                    },
                    input = rx.recv() => match input {
                        Some(Input::Data(b)) => channel.data(&b[..]).await?,
                        Some(Input::Resize { cols, rows }) => channel.window_change(cols, rows, 0, 0).await?,
                        Some(Input::Forward { forward, reply }) => {
                            let _ = reply.send(forwarder.start(&forward).await);
                        }
                        // Not waited for here: the shell's output keeps flowing.
                        Some(Input::Sftp { reply }) => {
                            let handle = handle.clone();
                            tokio::spawn(async move {
                                let _ = reply.send(sftp::open(&handle).await);
                            });
                        }
                        Some(Input::Exec { command, stdin, reply }) => {
                            let handle = handle.clone();
                            tokio::spawn(async move {
                                let _ = reply.send(exec(&handle, &command, &stdin).await);
                            });
                        }
                        Some(Input::Ping { reply }) => {
                            let handle = handle.clone();
                            tokio::spawn(async move {
                                let answered = tokio::time::timeout(PING, async {
                                    let probe = handle.channel_open_session().await?;
                                    probe.close().await
                                })
                                .await;
                                let _ = reply.send(matches!(answered, Ok(Ok(()))));
                            });
                        }
                        None => break,
                    },
                }
            }
            Ok(())
        }
        .await;
        let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
        // Closes the forwarded ports.
        drop(forwarder);
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

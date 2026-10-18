//! SSH sessions for shellwing, over russh.
//!
//! Host keys are checked against `~/.ssh/known_hosts` the way OpenSSH does it: a
//! host is accepted when *any* recorded key for it matches, a host whose recorded
//! keys of the same algorithm all differ is refused (the key changed), and an
//! unknown host is either refused or recorded depending on [`HostKeyPolicy`].

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use russh::client::{self, Handle};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{PublicKey, PublicKeyOrCertificate, known_hosts};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::mpsc;

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
                std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "root".into()),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Refuse hosts that are not in known_hosts.
    Strict,
    /// Record unknown hosts on first use (OpenSSH `StrictHostKeyChecking=accept-new`).
    AcceptNew,
}

#[derive(Clone, Debug, Default)]
pub struct Auth {
    /// Try the keys of the agent at `$SSH_AUTH_SOCK`.
    pub agent: bool,
    pub password: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a valid target: {0:?} (expected [user@]host[:port])")]
    BadTarget(String),
    #[error(
        "the host key for {host} has changed (known_hosts line {line}). If you trust the new key, run: ssh-keygen -R {host}"
    )]
    HostKeyChanged { host: String, line: usize },
    #[error("{host} is not in known_hosts")]
    HostKeyUnknown { host: String },
    #[error("permission denied: the server accepted none of the keys or passwords tried")]
    AuthFailed,
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

/// A connected interactive shell. Dropping it closes the session.
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

struct Checker {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
    known_hosts: Option<PathBuf>,
    problem: Arc<Mutex<Option<Error>>>,
}

impl Checker {
    fn check(&self, key: &PublicKey) -> Result<(), Error> {
        let path = match &self.known_hosts {
            Some(p) => p.clone(),
            None => default_known_hosts().ok_or(russh::keys::Error::NoHomeDir)?,
        };
        let recorded = if path.exists() {
            known_hosts::known_host_keys_path(&self.host, self.port, &path)?
        } else {
            vec![]
        };
        if recorded.iter().any(|(_, k)| k == key) {
            return Ok(());
        }
        if let Some((line, _)) = recorded
            .iter()
            .find(|(_, k)| k.algorithm() == key.algorithm())
        {
            return Err(Error::HostKeyChanged {
                host: self.host_label(),
                line: *line,
            });
        }
        match self.policy {
            HostKeyPolicy::Strict => Err(Error::HostKeyUnknown {
                host: self.host_label(),
            }),
            HostKeyPolicy::AcceptNew => {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                known_hosts::learn_known_hosts_path(&self.host, self.port, key, &path)?;
                Ok(())
            }
        }
    }

    fn host_label(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }
}

fn default_known_hosts() -> Option<PathBuf> {
    std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

impl client::Handler for Checker {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            // Host certificates need a CA check; not supported yet, so refuse them.
            return Ok(false);
        };
        match self.check(key) {
            Ok(()) => Ok(true),
            Err(e) => {
                if let Ok(mut p) = self.problem.lock() {
                    *p = Some(e);
                }
                Ok(false)
            }
        }
    }
}

pub struct Options {
    pub target: Target,
    pub auth: Auth,
    pub policy: HostKeyPolicy,
    /// Override for `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    pub cols: u32,
    pub rows: u32,
    pub term: String,
}

/// Connects, authenticates, opens a PTY shell and starts pumping output into
/// `output`. `ended` is called once when the session finishes.
pub async fn connect(
    opts: Options,
    mut output: OutputFn,
    ended: EndedFn,
) -> Result<Session, Error> {
    let problem = Arc::new(Mutex::new(None));
    let checker = Checker {
        host: opts.target.host.clone(),
        port: opts.target.port,
        policy: opts.policy,
        known_hosts: opts.known_hosts.clone(),
        problem: problem.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(std::time::Duration::from_secs(30)),
        ..Default::default()
    });
    let mut handle = match client::connect(
        config,
        (opts.target.host.as_str(), opts.target.port),
        checker,
    )
    .await
    {
        Ok(h) => h,
        Err(e) => {
            return Err(problem
                .lock()
                .ok()
                .and_then(|mut p| p.take())
                .unwrap_or(Error::Ssh(e)));
        }
    };
    authenticate(&mut handle, &opts.target.user, &opts.auth).await?;

    let mut channel = handle.channel_open_session().await?;
    channel
        .request_pty(false, &opts.term, opts.cols, opts.rows, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Input>();
    tokio::spawn(async move {
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

type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// The ssh-agents this platform offers: `$SSH_AUTH_SOCK` on Unix; on Windows the
/// OpenSSH agent's named pipe, then Pageant.
async fn agents() -> Vec<Agent> {
    let mut found = Vec::new();
    #[cfg(unix)]
    if let Ok(a) = AgentClient::connect_env().await {
        found.push(a.dynamic());
    }
    #[cfg(windows)]
    {
        if let Ok(a) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
            found.push(a.dynamic());
        }
        if let Ok(a) = AgentClient::connect_pageant().await {
            found.push(a.dynamic());
        }
    }
    found
}

async fn authenticate(handle: &mut Handle<Checker>, user: &str, auth: &Auth) -> Result<(), Error> {
    if auth.agent {
        for mut agent in agents().await {
            let identities = agent.request_identities().await.unwrap_or_default();
            for id in identities {
                let key = id.public_key().into_owned();
                let hash = if key.algorithm().is_rsa() {
                    handle.best_supported_rsa_hash().await?.flatten()
                } else {
                    None
                };
                if let Ok(r) = handle.authenticate_publickey_with(user, key, hash, &mut agent).await
                    && r.success()
                {
                    return Ok(());
                }
            }
        }
    }
    if let Some(pw) = &auth.password
        && handle.authenticate_password(user, pw).await?.success()
    {
        return Ok(());
    }
    Err(Error::AuthFailed)
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

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).expect("test key")
    }

    const K1: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDyD7MWle/akSIeBa+0A3sSzhJasvo2RFYfYQpNcG7Rj";
    const K2: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";

    fn checker(file: &std::path::Path, policy: HostKeyPolicy) -> Checker {
        Checker {
            host: "h".into(),
            port: 22,
            policy,
            known_hosts: Some(file.into()),
            problem: Default::default(),
        }
    }

    #[test]
    fn host_keys_follow_openssh() {
        let dir = std::env::temp_dir().join(format!("shellwing-kh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("known_hosts");

        // A stale entry of the same type before the current one must not count as a change.
        std::fs::write(&file, format!("h {K2}\nh {K1}\n")).expect("write");
        assert!(
            checker(&file, HostKeyPolicy::Strict)
                .check(&key(K1))
                .is_ok()
        );

        // Only a different key of the same type: changed.
        std::fs::write(&file, format!("h {K2}\n")).expect("write");
        assert!(matches!(
            checker(&file, HostKeyPolicy::AcceptNew).check(&key(K1)),
            Err(Error::HostKeyChanged { line: 1, .. })
        ));

        // Unknown host: refused when strict, recorded when accept-new.
        std::fs::write(&file, "").expect("write");
        assert!(matches!(
            checker(&file, HostKeyPolicy::Strict).check(&key(K1)),
            Err(Error::HostKeyUnknown { .. })
        ));
        assert!(
            checker(&file, HostKeyPolicy::AcceptNew)
                .check(&key(K1))
                .is_ok()
        );
        assert!(
            checker(&file, HostKeyPolicy::Strict)
                .check(&key(K1))
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

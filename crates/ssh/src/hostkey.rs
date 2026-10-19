//! Host key verification against known_hosts, the way OpenSSH does it.
//!
//! A host is accepted when *any* key recorded for it matches; a stale entry of
//! the same type before the current one does not count as a change. A host whose
//! recorded keys of the presented type all differ is refused outright. An unknown
//! host is refused, recorded, or asked about, depending on [`HostKeyPolicy`]; when
//! asking, nothing is sent to the server until the user has answered.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use russh::Channel;
use russh::client::{self, ChannelOpenHandle, Msg};
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate, known_hosts};

use crate::Error;
use crate::forward::Routes;
use crate::prompt::{Prompt, Prompter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Refuse hosts that are not in known_hosts.
    Strict,
    /// Record unknown hosts on first use (OpenSSH `StrictHostKeyChecking=accept-new`).
    AcceptNew,
    /// Ask before trusting an unknown host (OpenSSH's default).
    Ask,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Known,
    Unknown,
    /// Another key of its kind is known: on that line, with those fingerprints.
    Changed {
        line: usize,
        old: Vec<String>,
    },
    /// known_hosts has this key `@revoked`.
    Revoked,
}

pub(crate) struct Checker {
    pub host: String,
    pub port: u16,
    pub policy: HostKeyPolicy,
    pub known_hosts: Option<PathBuf>,
    pub prompter: Option<Prompter>,
    /// Why the key was refused, for a clearer error than russh's.
    pub problem: Arc<Mutex<Option<Error>>>,
    /// Where connections the server forwards to us (`-R`) go.
    pub routes: Routes,
    /// Agent forwarding was asked for: only then are the server's agent
    /// channels accepted.
    pub forward_agent: bool,
    pub agent_socket: Option<PathBuf>,
}

fn default_known_hosts() -> Option<PathBuf> {
    std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

impl Checker {
    fn path(&self) -> Result<PathBuf, Error> {
        match &self.known_hosts {
            Some(p) => Ok(p.clone()),
            None => default_known_hosts().ok_or(Error::Keys(russh::keys::Error::NoHomeDir)),
        }
    }

    pub(crate) fn verdict(&self, key: &PublicKey) -> Result<Verdict, Error> {
        let path = self.path()?;
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let lines: Vec<crate::known_hosts::Entry> = crate::known_hosts::entries(&path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?
            .into_iter()
            .filter(|e| e.is_for(&self.host, self.port))
            .collect();
        if lines
            .iter()
            .any(|e| e.marker.as_deref() == Some("@revoked") && e.key.fingerprint == fingerprint)
        {
            return Ok(Verdict::Revoked);
        }
        // Keys, not certificate authorities.
        let recorded: Vec<(usize, PublicKey)> = lines
            .iter()
            .filter(|e| e.marker.is_none())
            .filter_map(|e| Some((e.line, PublicKey::from_openssh(&e.key.public).ok()?)))
            .collect();
        if recorded.iter().any(|(_, k)| k.key_data() == key.key_data()) {
            return Ok(Verdict::Known);
        }
        if let Some((line, _)) = recorded
            .iter()
            .find(|(_, k)| k.algorithm() == key.algorithm())
        {
            let old = recorded
                .iter()
                .map(|(_, k)| k.fingerprint(HashAlg::Sha256).to_string())
                .collect();
            return Ok(Verdict::Changed { line: *line, old });
        }
        Ok(Verdict::Unknown)
    }

    fn learn(&self, key: &PublicKey) -> Result<(), Error> {
        let path = self.path()?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        known_hosts::learn_known_hosts_path(&self.host, self.port, key, &path)?;
        Ok(())
    }

    pub(crate) fn label(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }

    async fn check(&self, key: &PublicKey) -> Result<(), Error> {
        match self.verdict(key)? {
            Verdict::Known => Ok(()),
            Verdict::Changed { line, old } => {
                let info = crate::keys::info(key);
                Err(Error::HostKeyChanged(Box::new(crate::ChangedKey {
                    label: self.label(),
                    host: self.host.clone(),
                    port: self.port,
                    line,
                    old,
                    fingerprint: info.fingerprint,
                    kind: info.kind,
                    key: info.public,
                })))
            }
            Verdict::Revoked => Err(Error::HostKeyRevoked { host: self.label() }),
            Verdict::Unknown => match (self.policy, &self.prompter) {
                (HostKeyPolicy::AcceptNew, _) => self.learn(key),
                (HostKeyPolicy::Ask, Some(prompter)) => {
                    let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
                    let algorithm = key
                        .algorithm()
                        .as_str()
                        .trim_start_matches("ssh-")
                        .to_ascii_uppercase();
                    let mut again = false;
                    loop {
                        let prompt = Prompt::HostKey {
                            host: self.label(),
                            algorithm: algorithm.clone(),
                            fingerprint: fingerprint.clone(),
                            again,
                        };
                        let answer = prompter.ask(prompt).await.ok_or(Error::Cancelled)?;
                        let answer = answer.trim();
                        if answer.eq_ignore_ascii_case("yes") || answer == fingerprint {
                            return self.learn(key);
                        }
                        if answer.eq_ignore_ascii_case("no") {
                            return Err(Error::HostKeyRejected { host: self.label() });
                        }
                        again = true;
                    }
                }
                _ => Err(Error::HostKeyUnknown { host: self.label() }),
            },
        }
    }
}

impl client::Handler for Checker {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            // Host certificates need a CA check; not supported yet, so refuse them.
            if let Ok(mut p) = self.problem.lock() {
                *p = Some(Error::HostCertificate { host: self.label() });
            }
            return Ok(false);
        };
        match self.check(key).await {
            Ok(()) => Ok(true),
            Err(e) => {
                if let Ok(mut p) = self.problem.lock() {
                    *p = Some(e);
                }
                Ok(false)
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.routes
            .open(channel, connected_address, connected_port, reply);
        Ok(())
    }

    /// The server asks for the agent (`ssh-add -l` there, a jump onwards):
    /// a fresh connection to the local agent, carried over this channel.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if !self.forward_agent {
            // Not asked for: dropping the reply refuses it.
            return Ok(());
        }
        let socket = self.agent_socket.clone();
        tokio::spawn(async move {
            let Some(agent) = crate::auth::agents(socket.as_deref())
                .await
                .into_iter()
                .next()
            else {
                reply.reject(russh::ChannelOpenFailure::ConnectFailed).await;
                return;
            };
            reply.accept().await;
            let mut local = agent.into_inner();
            let mut remote = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut remote, &mut local).await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            prompter: None,
            problem: Default::default(),
            routes: Default::default(),
            forward_agent: false,
            agent_socket: None,
        }
    }

    #[tokio::test]
    async fn host_keys_follow_openssh() {
        let dir = std::env::temp_dir().join(format!("shellwing-kh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("known_hosts");

        // A stale entry of the same type before the current one must not count as a change.
        std::fs::write(&file, format!("h {K2}\nh {K1}\n")).expect("write");
        assert!(
            checker(&file, HostKeyPolicy::Strict)
                .check(&key(K1))
                .await
                .is_ok()
        );

        // Only a different key of the same type: changed.
        std::fs::write(&file, format!("h {K2}\n")).expect("write");
        let r = checker(&file, HostKeyPolicy::AcceptNew)
            .check(&key(K1))
            .await;
        match r {
            Err(Error::HostKeyChanged(c)) => {
                assert_eq!((c.line, c.old.len()), (1, 1));
                assert_eq!(
                    c.fingerprint,
                    key(K1).fingerprint(HashAlg::Sha256).to_string()
                );
                assert_eq!(c.key, key(K1).to_openssh().expect("line"));
            }
            other => panic!("not changed: {other:?}"),
        }
        // A key known_hosts revokes is refused.
        std::fs::write(&file, format!("@revoked h {K1}\n")).expect("write");
        assert!(matches!(
            checker(&file, HostKeyPolicy::AcceptNew)
                .check(&key(K1))
                .await,
            Err(Error::HostKeyRevoked { .. })
        ));

        // Unknown host: refused when strict, recorded when accept-new.
        std::fs::write(&file, "").expect("write");
        assert!(matches!(
            checker(&file, HostKeyPolicy::Strict).check(&key(K1)).await,
            Err(Error::HostKeyUnknown { .. })
        ));
        // Asking without anyone to ask is the same as strict.
        assert!(matches!(
            checker(&file, HostKeyPolicy::Ask).check(&key(K1)).await,
            Err(Error::HostKeyUnknown { .. })
        ));
        assert!(
            checker(&file, HostKeyPolicy::AcceptNew)
                .check(&key(K1))
                .await
                .is_ok()
        );
        assert!(
            checker(&file, HostKeyPolicy::Strict)
                .check(&key(K1))
                .await
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

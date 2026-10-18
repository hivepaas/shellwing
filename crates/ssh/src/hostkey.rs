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
    Changed { line: usize },
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
        let recorded = if path.exists() {
            known_hosts::known_host_keys_path(&self.host, self.port, &path)?
        } else {
            vec![]
        };
        if recorded.iter().any(|(_, k)| k == key) {
            return Ok(Verdict::Known);
        }
        if let Some((line, _)) = recorded
            .iter()
            .find(|(_, k)| k.algorithm() == key.algorithm())
        {
            return Ok(Verdict::Changed { line: *line });
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
            Verdict::Changed { line } => Err(Error::HostKeyChanged {
                host: self.label(),
                line,
            }),
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
        assert!(matches!(r, Err(Error::HostKeyChanged { line: 1, .. })));

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

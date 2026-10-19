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
use russh::keys::ssh_key::Certificate;
use russh::keys::ssh_key::certificate::CertType;
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

    /// The authorities known_hosts trusts for this host (`@cert-authority`):
    /// with one, the server is asked for a host certificate.
    pub(crate) fn authorities(&self) -> Vec<PublicKey> {
        self.path()
            .map(|p| crate::known_hosts::authorities(&p, &self.host, self.port))
            .unwrap_or_default()
    }

    /// Accepts a host certificate signed by an authority known_hosts trusts
    /// for this host, still valid, naming this host (when it names any), and
    /// neither it nor its authority revoked; as OpenSSH does, it is never
    /// learned.
    pub(crate) fn check_certificate(&self, cert: &Certificate) -> Result<(), Error> {
        let refuse = |why: String| Error::HostCertificate {
            host: self.label(),
            why,
        };
        let authorities = self.authorities();
        if authorities.is_empty() {
            return Err(refuse("known_hosts names no @cert-authority for it".into()));
        }
        if cert.cert_type() != CertType::Host {
            return Err(refuse("it is a user certificate".into()));
        }
        let trusted: Vec<_> = authorities
            .iter()
            .map(|k| k.fingerprint(HashAlg::Sha256))
            .collect();
        cert.validate(trusted.iter()).map_err(|e| {
            refuse(match e {
                russh::keys::ssh_key::Error::CertificateValidation => {
                    "it has expired, is not valid yet, or no trusted authority signed it".into()
                }
                other => other.to_string(),
            })
        })?;
        let principals = cert.valid_principals();
        if !principals.is_empty()
            && !principals
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&self.host))
        {
            return Err(refuse(format!(
                "it is for {}, not {}",
                principals.join(", "),
                self.host
            )));
        }
        let path = self.path()?;
        let revoked: Vec<String> = crate::known_hosts::entries(&path)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.marker.as_deref() == Some("@revoked") && e.is_for(&self.host, self.port))
            .map(|e| e.key.fingerprint)
            .collect();
        let of =
            |k: &russh::keys::ssh_key::public::KeyData| k.fingerprint(HashAlg::Sha256).to_string();
        if revoked.contains(&of(cert.public_key())) || revoked.contains(&of(cert.signature_key())) {
            return Err(refuse(
                "it, or its authority, is revoked in known_hosts".into(),
            ));
        }
        Ok(())
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
        let key = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            PublicKeyOrCertificate::Certificate(cert) => {
                let refused = match self.check_certificate(cert) {
                    Ok(()) => return Ok(true),
                    Err(e) => e,
                };
                // As OpenSSH: a certificate no authority vouches for may still
                // carry a key known_hosts knows.
                let plain = PublicKey::new(cert.public_key().clone(), "");
                if matches!(self.verdict(&plain), Ok(Verdict::Known)) {
                    return Ok(true);
                }
                if let Ok(mut p) = self.problem.lock() {
                    *p = Some(refused);
                }
                return Ok(false);
            }
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

    #[test]
    fn host_certificates_are_checked_as_openssh_checks_them() {
        use russh::keys::ssh_key::certificate::Builder;
        use russh::keys::ssh_key::{Algorithm, PrivateKey};
        let dir = std::env::temp_dir().join(format!("shellwing-hc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("known_hosts");
        let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("ca");
        let host = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs();
        let cert = |kind: CertType, principal: &str, from: u64, to: u64| {
            let mut b = Builder::new_with_random_nonce(
                &mut rand::rng(),
                host.public_key().key_data().clone(),
                from,
                to,
            )
            .expect("builder");
            b.cert_type(kind).expect("type");
            b.valid_principal(principal).expect("principal");
            b.sign(&ca).expect("sign")
        };
        let ca_line = ca.public_key().to_openssh().expect("line");
        std::fs::write(
            &file,
            format!("@cert-authority h,*.example.com {ca_line}\n"),
        )
        .expect("write");
        let c = checker(&file, HostKeyPolicy::Strict);
        let good = cert(CertType::Host, "h", now - 60, now + 3600);
        c.check_certificate(&good)
            .expect("signed by the authority, for h");
        let why = |cert: &Certificate| match c.check_certificate(cert) {
            Err(Error::HostCertificate { why, .. }) => why,
            other => panic!("{other:?}"),
        };
        assert!(why(&cert(CertType::Host, "other", now - 60, now + 3600)).contains("for other"));
        assert!(why(&cert(CertType::User, "h", now - 60, now + 3600)).contains("user certificate"));
        assert!(why(&cert(CertType::Host, "h", now - 7200, now - 3600)).contains("expired"));
        std::fs::write(
            &file,
            format!("@cert-authority h {ca_line}\n@revoked h {ca_line}\n"),
        )
        .expect("write");
        assert!(why(&good).contains("revoked"));
        std::fs::write(&file, "").expect("write");
        assert!(why(&good).contains("no @cert-authority"));
        let _ = std::fs::remove_dir_all(&dir);
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

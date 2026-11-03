//! Saved passwords and passphrases, tried before asking, and the ones typed at a
//! prompt that worked, so the app can offer to save them.

/// Where saved secrets come from; the app backs this with its vault.
pub trait Secrets: Send + Sync {
    /// A saved password for `user@host[:port]`.
    fn password(&self, target: &str) -> Option<String>;
    /// The saved passphrase of the key with this fingerprint (`SHA256:…`).
    fn passphrase(&self, fingerprint: &str) -> Option<String>;
    /// A secret typed at a prompt worked.
    fn learned(&self, secret: Learned);
    /// Keys kept by the app, tried after the key files (see
    /// [`crate::Resolved::vault_key`]).
    fn keys(&self) -> Vec<crate::keys::StoredKey> {
        vec![]
    }
    /// A certificate from the organization `org` (see
    /// [`crate::Resolved::certified_by`]), or why there is none. It may ask a
    /// server: it is called where it can block.
    fn certified(&self, org: &str) -> Result<Certified, String> {
        let _ = org;
        Err("the app is not signed in to a shellwing server".into())
    }
}

/// A key and an organization's certificate for it, made to sign in for a
/// while: held in memory, never written anywhere.
#[derive(Clone)]
pub struct Certified {
    pub key: std::sync::Arc<russh::keys::PrivateKey>,
    pub certificate: russh::keys::ssh_key::Certificate,
}

impl Certified {
    /// `key` with its certificate, a line of a `-cert.pub` file: one that is
    /// for this key.
    pub fn new(key: russh::keys::PrivateKey, certificate: &str) -> Result<Certified, String> {
        let certificate = russh::keys::ssh_key::Certificate::from_openssh(certificate.trim())
            .map_err(|e| format!("not a certificate: {e}"))?;
        if certificate.public_key() != key.public_key().key_data() {
            return Err("the certificate is for another key".into());
        }
        Ok(Certified {
            key: std::sync::Arc::new(key),
            certificate,
        })
    }

    /// When it starts and stops being valid, in seconds since 1970.
    pub fn valid(&self) -> (u64, u64) {
        (
            self.certificate.valid_after(),
            self.certificate.valid_before(),
        )
    }
}

impl std::fmt::Debug for Certified {
    /// Never prints the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Certified({})", self.certificate.key_id())
    }
}

/// A secret that was typed and worked.
#[derive(Clone, PartialEq, Eq)]
pub enum Learned {
    Password {
        target: String,
        password: String,
    },
    Passphrase {
        fingerprint: String,
        key_path: String,
        passphrase: String,
    },
}

impl std::fmt::Debug for Learned {
    /// Never prints the secret itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Learned::Password { target, .. } => write!(f, "Password for {target}"),
            Learned::Passphrase { key_path, .. } => write!(f, "Passphrase for {key_path}"),
        }
    }
}

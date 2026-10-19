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

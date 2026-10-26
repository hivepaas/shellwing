//! The vault on a phone: its key kept by the app in the platform's keychain
//! ([`PlatformKeys`]), the passwords saved in it, and the SSH keys made or
//! kept there, which sessions sign in with as on the desktop.

use std::sync::Arc;

use shellwing_ssh::keys::{self, KeyKind};
use shellwing_vault::{Item, KeyStore};

use crate::{MobileError, Shellwing};

/// The vault's key in the platform's keychain: the app implements it (the
/// iOS Keychain, this device only). Called from the vault's own thread.
#[uniffi::export(with_foreign)]
pub trait PlatformKeys: Send + Sync {
    /// The key, or None before the first.
    fn load(&self) -> Result<Option<Vec<u8>>, MobileError>;
    fn save(&self, key: Vec<u8>) -> Result<(), MobileError>;
    fn forget(&self) -> Result<(), MobileError>;
}

/// [`PlatformKeys`] as the vault asks for it.
pub(crate) struct Keys(pub Arc<dyn PlatformKeys>);

impl KeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        self.0.load().map_err(|e| e.to_string())
    }

    fn save(&self, key: &[u8]) -> Result<(), String> {
        self.0.save(key.to_vec()).map_err(|e| e.to_string())
    }

    fn forget(&self) -> Result<(), String> {
        self.0.forget().map_err(|e| e.to_string())
    }
}

/// How the vault stands.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct VaultState {
    pub ready: bool,
    /// Why it is not, when it is not.
    pub problem: Option<String>,
}

/// A secret saved in the vault, without the secret.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SavedSecret {
    pub id: String,
    /// `user@host:port` for a password; the key's fingerprint for a passphrase.
    pub what: String,
    pub passphrase: bool,
}

/// An SSH key kept in the vault, without its private part.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct KeyRow {
    pub id: String,
    pub name: String,
    /// `ED25519`, `RSA 3072`…
    pub kind: String,
    /// `SHA256:…`.
    pub fingerprint: String,
    /// One line, as `authorized_keys` takes it.
    pub public: String,
}

#[uniffi::export]
impl Shellwing {
    pub fn vault_state(&self) -> VaultState {
        let ready = self.vault.get().is_some();
        VaultState {
            ready,
            problem: if ready { None } else { self.vault.problem() },
        }
    }

    /// What opens the vault on another device, or here when the keychain
    /// lost its key: 13 groups of 4, to write down.
    pub fn recovery_key(&self) -> Option<String> {
        self.vault
            .get()?
            .recovery_key()
            .ok()
            .flatten()
            .map(|k| k.to_string())
    }

    /// The passwords and passphrases saved, without the secrets.
    pub fn saved_secrets(&self) -> Vec<SavedSecret> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        let mut out: Vec<SavedSecret> = v
            .items()
            .unwrap_or_default()
            .into_iter()
            .map(|i| match i {
                Item::Password { id, target } => SavedSecret {
                    id,
                    what: target,
                    passphrase: false,
                },
                Item::Passphrase {
                    id, fingerprint, ..
                } => SavedSecret {
                    id,
                    what: fingerprint,
                    passphrase: true,
                },
            })
            .collect();
        out.sort_by(|a, b| a.what.cmp(&b.what));
        out
    }

    /// Forgets a saved password or passphrase; so does deleting a key.
    pub fn forget(&self, id: String) -> Result<(), MobileError> {
        self.open_vault()?.delete(&id).map_err(MobileError::of)
    }

    /// The SSH keys kept in the vault.
    pub fn keys(&self) -> Vec<KeyRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        v.ssh_keys()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|k| {
                let public = keys::ssh_key::PublicKey::from_openssh(&k.public).ok()?;
                let info = keys::info(&public);
                Some(KeyRow {
                    id: k.id,
                    name: k.name,
                    kind: info.kind,
                    fingerprint: info.fingerprint,
                    public: info.public,
                })
            })
            .collect()
    }

    /// Makes an Ed25519 key named `name` and keeps it in the vault: sessions
    /// sign in with it once its public line is on a host.
    pub fn generate_key(&self, name: String) -> Result<KeyRow, MobileError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(MobileError::Failed("A name is needed.".into()));
        }
        let vault = self.open_vault()?;
        let key = keys::generate(KeyKind::Ed25519, &name).map_err(MobileError::of)?;
        let info = keys::info(key.public_key());
        let private = keys::to_vault(&key).map_err(MobileError::of)?;
        let id = vault
            .add_ssh_key(&name, &info.public, &private)
            .map_err(MobileError::of)?;
        Ok(KeyRow {
            id,
            name,
            kind: info.kind,
            fingerprint: info.fingerprint,
            public: info.public,
        })
    }
}

impl Shellwing {
    pub(crate) fn open_vault(&self) -> Result<Arc<shellwing_vault::Vault>, MobileError> {
        self.vault.get().ok_or_else(|| {
            MobileError::Failed(
                self.vault
                    .problem()
                    .unwrap_or_else(|| "the vault is not open".into()),
            )
        })
    }
}

//! The vault as the SSH code sees it: saved secrets to try, and typed ones that
//! worked, handed to the pane so it can offer to save them.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use shellwing_ssh::{Learned, Secrets};
use shellwing_vault::{KeyStore, Vault};

/// The vault, opened on a thread of its own at start: the keychain may be slow,
/// or ask the user to unlock it, and the window must not wait for that.
#[derive(Clone)]
pub struct VaultSlot(Arc<OnceLock<Result<Arc<Vault>, String>>>);

impl VaultSlot {
    /// Opens the vault with its key in the system keychain.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn open(path: PathBuf) -> Self {
        Self::open_with(path, shellwing_vault::OsKeychain::default())
    }

    /// Opens the vault with its key in `keys`: on iOS and Android, a store the
    /// app backs with the platform keychain.
    pub fn open_with(path: PathBuf, keys: impl KeyStore + 'static) -> Self {
        let slot = VaultSlot(Arc::new(OnceLock::new()));
        let fill = slot.0.clone();
        let opened = std::thread::Builder::new()
            .name("shellwing-vault".into())
            .spawn(move || {
                let _ = fill.set(
                    Vault::open(&path, &keys)
                        .map(Arc::new)
                        .map_err(|e| e.to_string()),
                );
            });
        if let Err(e) = opened {
            let _ = slot.0.set(Err(e.to_string()));
        }
        slot
    }

    /// No vault (no config directory).
    pub fn none() -> Self {
        let slot = VaultSlot(Arc::new(OnceLock::new()));
        let _ = slot.0.set(Err("no config directory".into()));
        slot
    }

    /// Waits for the vault to open; None if it could not.
    pub fn get(&self) -> Option<Arc<Vault>> {
        self.0.wait().as_ref().ok().cloned()
    }

    /// The vault if it is open already; never waits.
    pub fn ready(&self) -> Option<Arc<Vault>> {
        self.0.get()?.as_ref().ok().cloned()
    }

    /// Why the vault could not open, once it tried.
    pub fn problem(&self) -> Option<String> {
        self.0.get()?.as_ref().err().cloned()
    }

    /// Saves a typed secret.
    pub fn save(&self, secret: &Learned) -> Result<(), String> {
        let vault = self
            .get()
            .ok_or_else(|| self.problem().unwrap_or_default())?;
        match secret {
            Learned::Password { target, password } => vault.set_password(target, password),
            Learned::Passphrase {
                fingerprint,
                key_path,
                passphrase,
            } => vault.set_passphrase(fingerprint, key_path, passphrase),
        }
        .map_err(|e| e.to_string())
    }
}

/// One connection's view of the vault.
pub struct SessionSecrets {
    pub vault: VaultSlot,
    pub learned: async_channel::Sender<Learned>,
}

impl Secrets for SessionSecrets {
    fn password(&self, target: &str) -> Option<String> {
        let saved = self.vault.get()?.password(target).ok()??;
        Some(String::from(saved.as_str()))
    }

    fn passphrase(&self, fingerprint: &str) -> Option<String> {
        let saved = self.vault.get()?.passphrase(fingerprint).ok()??;
        Some(String::from(saved.as_str()))
    }

    fn learned(&self, secret: Learned) {
        let _ = self.learned.try_send(secret);
    }

    fn keys(&self) -> Vec<shellwing_ssh::keys::StoredKey> {
        let Some(vault) = self.vault.get() else {
            return vec![];
        };
        vault
            .ssh_keys()
            .unwrap_or_default()
            .into_iter()
            .map(|k| shellwing_ssh::keys::StoredKey {
                id: k.id,
                name: k.name,
                openssh: k.private,
            })
            .collect()
    }
}

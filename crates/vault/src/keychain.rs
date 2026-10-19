//! Where the account key is kept.

use std::sync::Mutex;

/// Holds the vault's account key.
pub trait KeyStore: Send + Sync {
    /// The key, or None if none was stored yet.
    fn load(&self) -> Result<Option<Vec<u8>>, String>;
    fn save(&self, key: &[u8]) -> Result<(), String>;
    /// Removes the key, so only a passphrase or the recovery key opens the
    /// vault.
    fn forget(&self) -> Result<(), String> {
        Err("this keychain cannot forget its key".into())
    }
}

/// The operating system's keychain: Keychain Services on macOS, Credential
/// Manager on Windows, the Secret Service (GNOME Keyring, KWallet) on Linux.
/// Not on iOS or Android, where the app supplies its own [`KeyStore`].
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub struct OsKeychain {
    service: String,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl Default for OsKeychain {
    fn default() -> Self {
        OsKeychain {
            service: "shellwing".into(),
        }
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl OsKeychain {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, "vault account key").map_err(|e| e.to_string())
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl KeyStore for OsKeychain {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        match self.entry()?.get_secret() {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, key: &[u8]) -> Result<(), String> {
        self.entry()?.set_secret(key).map_err(|e| e.to_string())
    }

    fn forget(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// In memory, for tests.
#[derive(Default)]
pub struct MemoryKeys(Mutex<Option<Vec<u8>>>);

impl KeyStore for MemoryKeys {
    fn load(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.clone())
    }

    fn save(&self, key: &[u8]) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = Some(key.to_vec());
        Ok(())
    }

    fn forget(&self) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    }
}

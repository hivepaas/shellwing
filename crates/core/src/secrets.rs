//! The vault as the SSH code sees it: saved secrets to try, and typed ones that
//! worked, handed to the pane so it can offer to save them.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use shellwing_ssh::{Learned, Secrets};
use shellwing_vault::{KeyStore, Lock, Presence, Unlock, Vault};

enum State {
    Opening,
    Ready(Arc<Vault>),
    /// Waits for a passphrase or the recovery key (see [`VaultSlot::unlock`]).
    Locked(Lock),
    Failed(String),
}

struct Inner {
    state: Mutex<State>,
    /// The keychain holds the open vault's key (it opened from there, or was
    /// given it), so no passphrase is needed to open it.
    kept: std::sync::atomic::AtomicBool,
    opened: Condvar,
    path: Option<PathBuf>,
    keys: Option<Arc<dyn KeyStore>>,
}

/// The vault, opened on a thread of its own at start: the keychain may be slow,
/// or ask the user to unlock it, and the window must not wait for that. Without
/// a keychain to give its key, it waits locked for a passphrase or the
/// recovery key.
#[derive(Clone)]
pub struct VaultSlot(Arc<Inner>);

impl VaultSlot {
    /// Opens the vault with its key in the system keychain.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn open(path: PathBuf) -> Self {
        Self::open_with(path, shellwing_vault::OsKeychain::default())
    }

    /// Opens the vault with its key in `keys`: on iOS and Android, a store the
    /// app backs with the platform keychain.
    pub fn open_with(path: PathBuf, keys: impl KeyStore + 'static) -> Self {
        let keys: Arc<dyn KeyStore> = Arc::new(keys);
        let slot = VaultSlot(Arc::new(Inner {
            kept: Default::default(),
            state: Mutex::new(State::Opening),
            opened: Condvar::new(),
            path: Some(path.clone()),
            keys: Some(keys.clone()),
        }));
        let fill = slot.clone();
        let opened = std::thread::Builder::new()
            .name("shellwing-vault".into())
            .spawn(move || {
                let state = Self::state_of(Vault::open(&path, keys.as_ref()));
                fill.keep(matches!(state, State::Ready(_)));
                fill.set(state)
            });
        if let Err(e) = opened {
            slot.set(State::Failed(e.to_string()));
        }
        slot
    }

    /// No vault (no config directory).
    pub fn none() -> Self {
        VaultSlot(Arc::new(Inner {
            kept: Default::default(),
            state: Mutex::new(State::Failed("no config directory".into())),
            opened: Condvar::new(),
            path: None,
            keys: None,
        }))
    }

    fn state_of(opened: shellwing_vault::Result<Vault>) -> State {
        match opened {
            Ok(v) => State::Ready(Arc::new(v)),
            Err(shellwing_vault::Error::Locked(lock)) => State::Locked(lock),
            Err(e) => State::Failed(e.to_string()),
        }
    }

    fn set(&self, state: State) {
        if let Ok(mut s) = self.0.state.lock() {
            *s = state;
        }
        self.0.opened.notify_all();
    }

    /// Waits for the vault to open; None if it could not, or is locked.
    pub fn get(&self) -> Option<Arc<Vault>> {
        let mut s = self.0.state.lock().ok()?;
        while matches!(*s, State::Opening) {
            s = self.0.opened.wait(s).ok()?;
        }
        match &*s {
            State::Ready(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// The vault if it is open already; never waits.
    pub fn ready(&self) -> Option<Arc<Vault>> {
        match &*self.0.state.lock().ok()? {
            State::Ready(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// Why the vault could not open, once it tried; a locked one says so.
    pub fn problem(&self) -> Option<String> {
        match &*self.0.state.lock().ok()? {
            State::Failed(e) => Some(e.clone()),
            State::Locked(Lock::New) => {
                Some("there is no system keychain to keep its key: set a passphrase".into())
            }
            State::Locked(_) => Some("it is locked".into()),
            _ => None,
        }
    }

    /// What it waits for, while locked.
    pub fn locked(&self) -> Option<Lock> {
        match &*self.0.state.lock().ok()? {
            State::Locked(l) => Some(*l),
            _ => None,
        }
    }

    /// Opens the locked vault with a passphrase or the recovery key.
    pub fn unlock(&self, how: Unlock) -> Result<Arc<Vault>, shellwing_vault::Error> {
        let (Some(path), Some(keys)) = (&self.0.path, &self.0.keys) else {
            return Err(shellwing_vault::Error::NotSynced);
        };
        // Opened meanwhile (the passphrase typed while Windows Hello asked).
        if let Some(v) = self.ready() {
            return Ok(v);
        }
        let vault = Arc::new(Vault::unlock(path, keys.as_ref(), how)?);
        // A recovery key puts the key back where it can: one look tells.
        self.keep(vault.kept_in(keys.as_ref()));
        self.set(State::Ready(vault.clone()));
        Ok(vault)
    }

    fn keep(&self, kept: bool) {
        self.0
            .kept
            .store(kept, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the system keychain holds the open vault's key.
    pub fn in_keychain(&self) -> bool {
        self.0.kept.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Puts the open vault's key in the system keychain.
    pub fn keep_in_keychain(&self) -> Result<(), String> {
        let (Some(vault), Some(keys)) = (self.ready(), &self.0.keys) else {
            return Err("the vault is not open".into());
        };
        vault.keep_in(keys.as_ref()).map_err(|e| e.to_string())?;
        self.keep(true);
        Ok(())
    }

    /// Takes the open vault's key out of the system keychain, so every start
    /// asks for the passphrase. Needs one set.
    pub fn leave_keychain(&self) -> Result<(), String> {
        let (Some(vault), Some(keys)) = (self.ready(), &self.0.keys) else {
            return Err("the vault is not open".into());
        };
        vault.leave_keychain(keys.as_ref()).map_err(|e| match e {
            shellwing_vault::Error::Locked(_) => "set a passphrase first".to_string(),
            e => e.to_string(),
        })?;
        self.keep(false);
        Ok(())
    }

    /// Whether [`VaultSlot::lock`] can lock it: open, with a passphrase to
    /// open it again and its key not in the keychain.
    pub fn can_lock(&self) -> bool {
        !self.in_keychain()
            && self
                .ready()
                .is_some_and(|v| v.has_vault_passphrase().unwrap_or(false))
    }

    /// Closes the open vault until its passphrase is typed again. Those
    /// holding it (a connection reading a password) finish with it first.
    pub fn lock(&self) -> bool {
        if !self.can_lock() {
            return false;
        }
        self.set(State::Locked(Lock::Passphrase));
        true
    }

    /// Windows Hello or Touch ID, where this system has it.
    pub fn presence(&self) -> Option<SystemPresence> {
        shellwing_presence::System::here().map(SystemPresence)
    }

    /// Whether the user's presence opens the locked vault on this device.
    pub fn opens_with_presence(&self) -> bool {
        self.locked().is_some()
            && self
                .0
                .path
                .as_deref()
                .is_some_and(Vault::opens_with_presence)
    }

    /// Opens the locked vault with Windows Hello or Touch ID: blocks on the
    /// system's prompt.
    pub fn unlock_with_presence(&self) -> Result<Arc<Vault>, shellwing_vault::Error> {
        let p = self
            .presence()
            .ok_or(shellwing_vault::Error::Locked(Lock::Passphrase))?;
        self.unlock(Unlock::Presence(&p))
    }

    /// Checks it is the user before a secret shows: with Windows Hello or
    /// Touch ID where it opens the vault (`passphrase` None), else with the
    /// vault's passphrase. Blocks on the system's prompt or the KDF.
    pub fn confirm(&self, passphrase: Option<&str>) -> Result<(), shellwing_vault::Error> {
        let vault = self
            .ready()
            .ok_or(shellwing_vault::Error::Locked(Lock::Passphrase))?;
        match passphrase {
            Some(p) => vault.confirm(Unlock::Passphrase(p)),
            None => {
                let p = self
                    .presence()
                    .ok_or(shellwing_vault::Error::Locked(Lock::Passphrase))?;
                vault.confirm(Unlock::Presence(&p))
            }
        }
    }

    /// Lets the user's presence open the vault on this device, or (`false`)
    /// stops it: blocks on the system's prompt.
    pub fn set_presence(&self, on: bool) -> Result<(), String> {
        let vault = self.ready().ok_or("the vault is not open")?;
        let p = self
            .presence()
            .ok_or("this system has neither Windows Hello nor Touch ID")?;
        let done = if on {
            vault.set_presence(&p)
        } else {
            vault.remove_presence(&p)
        };
        done.map_err(|e| match e {
            shellwing_vault::Error::Locked(_) => "set a passphrase first".to_string(),
            e => e.to_string(),
        })
    }

    /// The keychain the vault's key goes in, where there is one.
    pub fn keys(&self) -> Option<Arc<dyn KeyStore>> {
        self.0.keys.clone()
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

/// Windows Hello or Touch ID, as the vault asks for the user's presence.
pub struct SystemPresence(shellwing_presence::System);

impl Presence for SystemPresence {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn enroll(&self, id: &str, reason: &str) -> Result<[u8; 32], String> {
        self.0.enroll(id, reason)
    }

    fn secret(&self, id: &str, reason: &str) -> Result<Option<[u8; 32]>, String> {
        Ok(match self.0.secret(id, reason)? {
            shellwing_presence::Secret::Given(s) => Some(s),
            shellwing_presence::Secret::Refused => None,
        })
    }

    fn forget(&self, id: &str) -> Result<(), String> {
        self.0.forget(id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::MemoryKeys;

    #[test]
    fn a_vault_out_of_the_keychain_locks_and_opens_with_its_passphrase() {
        let dir = std::env::temp_dir().join(format!("shellwing-slot-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let slot = VaultSlot::open_with(dir.join("vault.db"), MemoryKeys::default());
        let vault = slot.get().expect("open");
        assert!(slot.in_keychain());
        assert!(!slot.can_lock() && !slot.lock(), "the keychain opens it");
        assert!(slot.leave_keychain().is_err(), "not without a passphrase");

        vault
            .set_vault_passphrase(Some("correct horse battery"))
            .expect("passphrase");
        drop(vault);
        slot.leave_keychain().expect("left");
        assert!(!slot.in_keychain() && slot.can_lock());
        assert!(slot.lock());
        assert_eq!(slot.locked(), Some(Lock::Passphrase));
        assert!(slot.ready().is_none() && slot.get().is_none());

        assert!(
            slot.unlock(Unlock::Passphrase("wrong horse battery"))
                .is_err()
        );
        slot.unlock(Unlock::Passphrase("correct horse battery"))
            .expect("unlocked");
        assert!(slot.ready().is_some() && !slot.in_keychain());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Unlocking with the user's presence: Windows Hello, Touch ID. Something
//! of this device's gives a secret only once the user proves they are there
//! (a fingerprint, the face, the device's PIN); the account key, sealed
//! with that secret, is kept in vault.db beside its other wraps. The
//! passphrase and the recovery key open the vault as before.

/// What gives a device secret after the user's say-so. Calls block on the
/// system's prompt: never on the UI thread.
pub trait Presence: Send + Sync {
    /// How the user knows it: "Windows Hello", "Touch ID".
    fn name(&self) -> &'static str;
    /// Makes this device's secret for `id` (replacing one there was), and
    /// gives it: the system asks the user.
    fn enroll(&self, id: &str, reason: &str) -> Result<[u8; 32], String>;
    /// The secret made for `id`: the system asks the user. `Ok(None)`: they
    /// said no, or it is gone (the fingerprints changed).
    fn secret(&self, id: &str, reason: &str) -> Result<Option<[u8; 32]>, String>;
    /// Forgets the secret for `id`.
    fn forget(&self, id: &str) -> Result<(), String>;
}

/// In memory, for tests: "present" while `present` is set.
#[derive(Default)]
pub struct MemoryPresence {
    secrets: std::sync::Mutex<std::collections::HashMap<String, [u8; 32]>>,
    pub absent: std::sync::atomic::AtomicBool,
}

impl Presence for MemoryPresence {
    fn name(&self) -> &'static str {
        "the test's presence"
    }

    fn enroll(&self, id: &str, _: &str) -> Result<[u8; 32], String> {
        let mut s = [0u8; 32];
        getrandom::fill(&mut s).map_err(|e| e.to_string())?;
        self.secrets
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id.to_string(), s);
        Ok(s)
    }

    fn secret(&self, id: &str, _: &str) -> Result<Option<[u8; 32]>, String> {
        if self.absent.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(None);
        }
        Ok(self
            .secrets
            .lock()
            .map_err(|e| e.to_string())?
            .get(id)
            .copied())
    }

    fn forget(&self, id: &str) -> Result<(), String> {
        self.secrets.lock().map_err(|e| e.to_string())?.remove(id);
        Ok(())
    }
}

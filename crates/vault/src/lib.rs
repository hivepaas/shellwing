//! The vault: passwords and key passphrases, encrypted at rest.
//!
//! As in `docs/design/vault.md`, minus sync for now:
//!
//! - The Account Key (AK), 32 random bytes, lives in the OS keychain (macOS
//!   Keychain, Windows Credential Manager, Secret Service on Linux).
//! - The Personal space key is wrapped by the AK and stored in `vault.db`.
//! - Each record is a JSON payload sealed under the space key, with its id,
//!   kind, space and key version as the additional data, so a ciphertext
//!   cannot be moved to another record. Sealed values start with the id of the
//!   [`Cipher`] used: XChaCha20-Poly1305 by default, AES-256-GCM where FIPS
//!   algorithms are required.
//!
//! Lookups decrypt every record of the kind asked for; a vault holds dozens,
//! not millions.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

mod cipher;
mod hlc;
mod keychain;
mod recovery;
mod sync;

pub use cipher::Cipher;
pub use keychain::{KeyStore, MemoryKeys, OsKeychain};
pub use recovery::RecoveryKey;
pub use sync::{Backend, Folder, FolderState, SyncReport};

const KEY_LEN: usize = 32;
const SPACE: &str = "personal";
const KEY_VERSION: i64 = 1;
/// The design keeps only "host" or "other" in the clear; secrets are "other".
const KIND: &str = "other";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("vault database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("vault folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("system keychain: {0}")]
    Keychain(String),
    /// The vault has records but the keychain lost its key: only a recovery key
    /// (coming with sync) could open it.
    #[error("the vault's key is missing from the system keychain")]
    KeyMissing,
    #[error("a vault record failed to decrypt (wrong key or damaged data)")]
    Decrypt,
    /// The sync store holds another vault: join it with its recovery key.
    #[error("the sync folder holds another vault; enter its recovery key to join it")]
    OtherVault,
    #[error("the sync folder holds no vault yet")]
    NotSynced,
    #[error("that recovery key does not open the vault in the sync folder")]
    WrongRecoveryKey,
    #[error("sync folder: {0}")]
    Sync(String),
    /// Sealed by a newer version with an algorithm this one does not know.
    #[error("a vault record uses an unknown cipher ({0}); update shellwing")]
    UnknownCipher(u8),
    #[error("a vault record is malformed: {0}")]
    Format(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a record holds, without the secret itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A login password for `user@host[:port]`.
    Password { id: String, target: String },
    /// The passphrase of the key with this SHA256 fingerprint.
    Passphrase {
        id: String,
        fingerprint: String,
        /// Where the key was when saved, to show.
        key_path: String,
    },
}

/// A field and when it was set (a clock stamp), for merging edits.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Field {
    val: String,
    t: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    f: std::collections::BTreeMap<String, Field>,
}

impl Drop for Record {
    fn drop(&mut self) {
        for f in self.f.values_mut() {
            zeroize::Zeroize::zeroize(&mut f.val);
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| Error::Keychain(format!("no randomness: {e}")))?;
    Ok(b)
}

/// A ULID-like id: time first, so ids sort by creation.
fn new_id() -> Result<String> {
    let r: [u8; 10] = random()?;
    let hex: String = r.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{:012x}{hex}", now_ms()))
}

fn record_aad(id: &str) -> Vec<u8> {
    format!("{id}\0{KIND}\0{SPACE}\0{KEY_VERSION}").into_bytes()
}

const SPACE_AAD: &[u8] = b"shellwing space key: personal v1";

pub struct Vault {
    db: Mutex<Connection>,
    /// From the keychain; wraps the space key and the recovery key.
    account: Zeroizing<[u8; KEY_LEN]>,
    /// Behind a lock so joining another vault can replace it while open.
    space_key: std::sync::RwLock<Zeroizing<[u8; KEY_LEN]>>,
    /// What new values are sealed with; old ones open whatever they used.
    cipher: Cipher,
    clock: hlc::Clock,
}

/// A record row as stored: sealed data, its newest stamp, and whether deleted.
pub(crate) struct RawRow {
    pub id: String,
    pub data: Vec<u8>,
    pub hlc: String,
    pub deleted: bool,
}

const RECOVERY_KEY_AAD: &[u8] = b"shellwing recovery key wrapped by the account key v1";

impl Vault {
    /// Opens (or creates) `vault.db` at `path`, unlocking it with the account key
    /// from `keys`; a first run creates both keys.
    pub fn open(path: &Path, keys: &dyn KeyStore) -> Result<Vault> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Connection::open(path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS records (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, space TEXT NOT NULL,
                key_ver INTEGER NOT NULL, data BLOB NOT NULL, hlc TEXT NOT NULL,
                deleted INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS keys (
                id TEXT NOT NULL, ver INTEGER NOT NULL, wrapped BLOB NOT NULL,
                wrapped_by TEXT NOT NULL, PRIMARY KEY (id, ver, wrapped_by));
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        // This device's name in clock stamps: random, made once.
        let device: String = match db
            .query_row("SELECT value FROM meta WHERE key = 'device'", [], |r| {
                r.get(0)
            })
            .optional()?
        {
            Some(d) => d,
            None => {
                let d: String = random::<4>()?.iter().map(|b| format!("{b:02x}")).collect();
                db.execute(
                    "INSERT INTO meta (key, value) VALUES ('device', ?1)",
                    params![d],
                )?;
                d
            }
        };
        let newest: Option<String> =
            db.query_row("SELECT MAX(hlc) FROM records", [], |r| r.get(0))?;
        let has_records: bool =
            db.query_row("SELECT EXISTS(SELECT 1 FROM records)", [], |r| r.get(0))?;
        let account = match keys.load().map_err(Error::Keychain)? {
            Some(k) if k.len() == KEY_LEN => {
                let mut a = Zeroizing::new([0u8; KEY_LEN]);
                a.copy_from_slice(&k);
                a
            }
            Some(_) => return Err(Error::Keychain("the stored key has the wrong size".into())),
            None if has_records => return Err(Error::KeyMissing),
            None => {
                let a = Zeroizing::new(random::<KEY_LEN>()?);
                keys.save(&a[..]).map_err(Error::Keychain)?;
                a
            }
        };
        let wrapped: Option<Vec<u8>> = db
            .query_row(
                "SELECT wrapped FROM keys WHERE id = ?1 AND ver = ?2 AND wrapped_by = 'account'",
                params![SPACE, KEY_VERSION],
                |r| r.get(0),
            )
            .optional()?;
        let cipher = Cipher::default();
        let space_key = match wrapped {
            Some(w) => {
                let (k, legacy) = cipher::open(&account, &w, SPACE_AAD)?;
                let mut s = Zeroizing::new([0u8; KEY_LEN]);
                if k.len() != KEY_LEN {
                    return Err(Error::Decrypt);
                }
                s.copy_from_slice(&k);
                if legacy {
                    db.execute(
                        "UPDATE keys SET wrapped = ?3 WHERE id = ?1 AND ver = ?2 AND wrapped_by = 'account'",
                        params![SPACE, KEY_VERSION, cipher::seal(cipher, &account, &s[..], SPACE_AAD)?],
                    )?;
                }
                s
            }
            None => {
                let s = Zeroizing::new(random::<KEY_LEN>()?);
                db.execute(
                    "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES (?1, ?2, ?3, 'account')",
                    params![SPACE, KEY_VERSION, cipher::seal(cipher, &account, &s[..], SPACE_AAD)?],
                )?;
                s
            }
        };
        let clock = hlc::Clock::new(&device);
        if let Some(n) = &newest {
            clock.observe(n);
        }
        let vault = Vault {
            db: Mutex::new(db),
            account,
            space_key: std::sync::RwLock::new(space_key),
            cipher,
            clock,
        };
        vault.upgrade_untagged()?;
        Ok(vault)
    }

    /// The space key, read-locked for one use.
    pub(crate) fn sk(&self) -> std::sync::RwLockReadGuard<'_, Zeroizing<[u8; KEY_LEN]>> {
        self.space_key.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Seals new values with `cipher` from now on (old ones keep theirs).
    pub fn set_cipher(&mut self, cipher: Cipher) {
        self.cipher = cipher;
    }

    /// Rewrites records sealed before values carried a cipher id.
    fn upgrade_untagged(&self) -> Result<()> {
        let rows: Vec<(String, Vec<u8>)> = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            let mut q = db.prepare("SELECT id, data FROM records WHERE deleted = 0")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?
        };
        for (id, data) in rows {
            let (plain, legacy) = cipher::open(&self.sk(), &data, &record_aad(&id))?;
            if legacy {
                let sealed = cipher::seal(self.cipher, &self.sk(), &plain, &record_aad(&id))?;
                let db = self.db.lock().map_err(|_| Error::Decrypt)?;
                db.execute(
                    "UPDATE records SET data = ?2 WHERE id = ?1",
                    params![id, sealed],
                )?;
            }
        }
        Ok(())
    }

    fn records(&self) -> Result<Vec<(String, Record)>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut q = db
            .prepare("SELECT id, data FROM records WHERE deleted = 0 AND space = ?1 ORDER BY id")?;
        let rows = q.query_map(params![SPACE], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = vec![];
        for row in rows {
            let (id, data) = row?;
            let (plain, _) = cipher::open(&self.sk(), &data, &record_aad(&id))?;
            out.push((id, serde_json::from_slice(&plain)?));
        }
        Ok(out)
    }

    fn find(&self, kind: &str, field: &str, value: &str) -> Result<Option<(String, Record)>> {
        Ok(self
            .records()?
            .into_iter()
            .find(|(_, r)| r.kind == kind && r.f.get(field).is_some_and(|f| f.val == value)))
    }

    fn write(&self, id: &str, record: &Record) -> Result<()> {
        let plain = Zeroizing::new(serde_json::to_vec(record)?);
        let data = cipher::seal(self.cipher, &self.sk(), &plain, &record_aad(id))?;
        let newest = record.f.values().map(|f| f.t.as_str()).max().unwrap_or("");
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO records (id, kind, space, key_ver, data, hlc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data, hlc = excluded.hlc, deleted = 0",
            params![id, KIND, SPACE, KEY_VERSION, data, newest],
        )?;
        Ok(())
    }

    /// Every row, deleted ones too.
    pub(crate) fn raw_rows(&self) -> Result<Vec<RawRow>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut q = db.prepare("SELECT id, data, hlc, deleted FROM records WHERE space = ?1")?;
        let rows = q
            .query_map(params![SPACE], |r| {
                Ok(RawRow {
                    id: r.get(0)?,
                    data: r.get(1)?,
                    hlc: r.get(2)?,
                    deleted: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Stores a merged version from sync.
    pub(crate) fn store_version(&self, id: &str, v: &sync::Version) -> Result<()> {
        match v {
            sync::Version::Live(r) => self.write(id, r),
            sync::Version::Gone(t) => {
                let db = self.db.lock().map_err(|_| Error::Decrypt)?;
                db.execute(
                    "INSERT INTO records (id, kind, space, key_ver, data, hlc, deleted)
                     VALUES (?1, ?2, ?3, ?4, X'', ?5, 1)
                     ON CONFLICT(id) DO UPDATE SET data = X'', hlc = excluded.hlc, deleted = 1",
                    params![id, KIND, SPACE, KEY_VERSION, t],
                )?;
                Ok(())
            }
        }
    }

    /// The recovery key, made (and kept, wrapped by the account key) on first use.
    pub(crate) fn recovery_key_or_new(&self) -> Result<RecoveryKey> {
        let wrapped: Option<Vec<u8>> = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            db.query_row(
                "SELECT wrapped FROM keys WHERE id = 'recovery' AND wrapped_by = 'account'",
                [],
                |r| r.get(0),
            )
            .optional()?
        };
        if let Some(w) = wrapped {
            let (k, _) = cipher::open(&self.account, &w, RECOVERY_KEY_AAD)?;
            let mut key = Zeroizing::new([0u8; KEY_LEN]);
            if k.len() != KEY_LEN {
                return Err(Error::Decrypt);
            }
            key.copy_from_slice(&k);
            return Ok(RecoveryKey(key));
        }
        let rk = RecoveryKey(Zeroizing::new(random::<KEY_LEN>()?));
        self.store_recovery_key(&rk)?;
        Ok(rk)
    }

    pub(crate) fn store_recovery_key(&self, rk: &RecoveryKey) -> Result<()> {
        let sealed = cipher::seal(self.cipher, &self.account, &rk.0[..], RECOVERY_KEY_AAD)?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES ('recovery', 1, ?1, 'account')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![sealed],
        )?;
        Ok(())
    }

    /// The recovery key, to show again, if syncing was ever started.
    pub fn recovery_key(&self) -> Result<Option<RecoveryKey>> {
        let has: bool = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM keys WHERE id = 'recovery')",
                [],
                |r| r.get(0),
            )?
        };
        if has {
            self.recovery_key_or_new().map(Some)
        } else {
            Ok(None)
        }
    }

    fn put(
        &self,
        kind: &str,
        key_field: (&str, &str),
        name: &str,
        fields: &[(&str, &str)],
    ) -> Result<()> {
        let id = match self.find(kind, key_field.0, key_field.1)? {
            Some((id, _)) => id,
            None => new_id()?,
        };
        let t = self.clock.tick();
        let mut f = std::collections::BTreeMap::new();
        for (k, v) in std::iter::once(&key_field).chain(fields) {
            f.insert(
                k.to_string(),
                Field {
                    val: v.to_string(),
                    t: t.clone(),
                },
            );
        }
        self.write(
            &id,
            &Record {
                v: 1,
                kind: kind.into(),
                name: name.into(),
                f,
            },
        )
    }

    /// The saved password for `user@host[:port]`.
    pub fn password(&self, target: &str) -> Result<Option<Zeroizing<String>>> {
        Ok(self
            .find("password", "target", target)?
            .and_then(|(_, r)| r.f.get("password").map(|f| Zeroizing::new(f.val.clone()))))
    }

    pub fn set_password(&self, target: &str, password: &str) -> Result<()> {
        self.put(
            "password",
            ("target", target),
            &format!("{target} password"),
            &[("password", password)],
        )
    }

    /// The saved passphrase of the key with this fingerprint.
    pub fn passphrase(&self, fingerprint: &str) -> Result<Option<Zeroizing<String>>> {
        Ok(self
            .find("key_passphrase", "fingerprint", fingerprint)?
            .and_then(|(_, r)| r.f.get("passphrase").map(|f| Zeroizing::new(f.val.clone()))))
    }

    pub fn set_passphrase(
        &self,
        fingerprint: &str,
        key_path: &str,
        passphrase: &str,
    ) -> Result<()> {
        self.put(
            "key_passphrase",
            ("fingerprint", fingerprint),
            &format!("{key_path} passphrase"),
            &[("key_path", key_path), ("passphrase", passphrase)],
        )
    }

    /// Every item, without the secrets.
    pub fn items(&self) -> Result<Vec<Item>> {
        let get = |r: &Record, k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
        Ok(self
            .records()?
            .into_iter()
            .filter_map(|(id, r)| match r.kind.as_str() {
                "password" => Some(Item::Password {
                    target: get(&r, "target"),
                    id,
                }),
                "key_passphrase" => Some(Item::Passphrase {
                    fingerprint: get(&r, "fingerprint"),
                    key_path: get(&r, "key_path"),
                    id,
                }),
                _ => None,
            })
            .collect())
    }

    /// Forgets an item. The row stays as a tombstone, as sync will need.
    pub fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "UPDATE records SET deleted = 1, data = X'', hlc = ?2 WHERE id = ?1",
            params![id, self.clock.tick()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("vault.db")
    }

    #[test]
    fn saves_and_finds_secrets_across_reopening() {
        let path = temp("reopen");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("deploy@10.0.0.5", "hunter2").expect("save");
        v.set_password("deploy@10.0.0.5", "hunter3")
            .expect("replace");
        v.set_passphrase("SHA256:abc", "~/.ssh/id_ed25519", "s3cret")
            .expect("save");
        drop(v);
        let v = Vault::open(&path, &keys).expect("reopen");
        assert_eq!(
            v.password("deploy@10.0.0.5")
                .expect("read")
                .as_deref()
                .map(String::as_str),
            Some("hunter3")
        );
        assert_eq!(v.password("other@host").expect("read"), None);
        assert_eq!(
            v.passphrase("SHA256:abc")
                .expect("read")
                .as_deref()
                .map(String::as_str),
            Some("s3cret")
        );
        assert_eq!(
            v.items().expect("items").len(),
            2,
            "replacing kept one record"
        );
    }

    #[test]
    fn nothing_readable_on_disk_and_a_wrong_key_fails() {
        let path = temp("at-rest");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("me@db.internal", "correct horse battery")
            .expect("save");
        drop(v);
        let raw = std::fs::read(&path).expect("read db");
        let has = |s: &str| raw.windows(s.len()).any(|w| w == s.as_bytes());
        assert!(
            !has("correct horse") && !has("db.internal"),
            "plaintext on disk"
        );
        // Another account key cannot open the space key.
        let other = MemoryKeys::default();
        other.save(&[7u8; KEY_LEN]).expect("save");
        assert!(matches!(Vault::open(&path, &other), Err(Error::Decrypt)));
        // A vault with records whose key vanished says so.
        assert!(matches!(
            Vault::open(&path, &MemoryKeys::default()),
            Err(Error::KeyMissing)
        ));
    }

    #[test]
    fn ciphertexts_cannot_be_swapped_between_records() {
        let path = temp("swap");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("a@x", "one").expect("save");
        v.set_password("b@x", "two").expect("save");
        {
            let db = v.db.lock().expect("db");
            let rows: Vec<(String, Vec<u8>)> = db
                .prepare("SELECT id, data FROM records ORDER BY id")
                .expect("prepare")
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .expect("query")
                .collect::<std::result::Result<_, _>>()
                .expect("rows");
            db.execute(
                "UPDATE records SET data = ?2 WHERE id = ?1",
                params![rows[0].0, rows[1].1],
            )
            .expect("swap");
        }
        assert!(matches!(v.password("a@x"), Err(Error::Decrypt)));
    }

    #[test]
    fn records_sealed_with_different_ciphers_live_side_by_side() {
        let path = temp("ciphers");
        let keys = MemoryKeys::default();
        let mut v = Vault::open(&path, &keys).expect("open");
        v.set_password("a@x", "one").expect("save");
        v.set_cipher(Cipher::Aes256Gcm);
        v.set_password("b@x", "two").expect("save");
        drop(v);
        let v = Vault::open(&path, &keys).expect("reopen");
        let get = |t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(
            (get("a@x"), get("b@x")),
            (Some("one".into()), Some("two".into()))
        );
        let mut ids: Vec<u8> = {
            let db = v.db.lock().expect("db");
            // Both may be saved within one millisecond, so ids do not give the order.
            db.prepare("SELECT data FROM records")
                .expect("prepare")
                .query_map([], |r| r.get::<_, Vec<u8>>(0))
                .expect("query")
                .map(|d| d.expect("row")[0])
                .collect()
        };
        ids.sort();
        assert_eq!(
            ids,
            [Cipher::XChaCha20Poly1305.id(), Cipher::Aes256Gcm.id()]
        );
    }

    /// A vault saved before values carried a cipher id (`nonce || ciphertext`).
    #[test]
    fn an_untagged_vault_opens_and_is_rewritten() {
        use chacha20poly1305::XChaCha20Poly1305;
        use chacha20poly1305::aead::{Aead, KeyInit, Payload};
        let untagged = |key: &[u8; KEY_LEN], plain: &[u8], aad: &[u8]| {
            let nonce = [3u8; 24];
            let mut out = nonce.to_vec();
            out.extend(
                XChaCha20Poly1305::new(key.into())
                    .encrypt((&nonce).into(), Payload { msg: plain, aad })
                    .expect("encrypt"),
            );
            out
        };
        let path = temp("untagged");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("old@host", "kept").expect("save");
        let account: [u8; KEY_LEN] = keys
            .load()
            .expect("load")
            .expect("key")
            .try_into()
            .expect("size");
        {
            // Turn everything back into the first format.
            let db = v.db.lock().expect("db");
            let (id, data): (String, Vec<u8>) = db
                .query_row("SELECT id, data FROM records", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .expect("row");
            let (plain, _) = cipher::open(&v.sk(), &data, &record_aad(&id)).expect("open");
            db.execute(
                "UPDATE records SET data = ?2 WHERE id = ?1",
                params![id, untagged(&v.sk(), &plain, &record_aad(&id))],
            )
            .expect("record");
            db.execute(
                "UPDATE keys SET wrapped = ?1",
                params![untagged(&account, &v.sk()[..], SPACE_AAD)],
            )
            .expect("key");
        }
        drop(v);
        let v = Vault::open(&path, &keys).expect("reopen");
        assert_eq!(
            v.password("old@host")
                .expect("read")
                .as_deref()
                .map(String::as_str),
            Some("kept")
        );
        let db = v.db.lock().expect("db");
        let first = |sql: &str| {
            db.query_row(sql, [], |r| r.get::<_, Vec<u8>>(0))
                .expect("row")[0]
        };
        assert_eq!(
            first("SELECT data FROM records"),
            Cipher::default().id(),
            "rewritten"
        );
        assert_eq!(
            first("SELECT wrapped FROM keys"),
            Cipher::default().id(),
            "rewritten"
        );
    }

    #[test]
    fn deleting_forgets() {
        let path = temp("delete");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("a@x", "one").expect("save");
        let Item::Password { id, .. } = v.items().expect("items").remove(0) else {
            panic!("a password");
        };
        v.delete(&id).expect("delete");
        assert_eq!(v.password("a@x").expect("read"), None);
        assert!(v.items().expect("items").is_empty());
    }
}

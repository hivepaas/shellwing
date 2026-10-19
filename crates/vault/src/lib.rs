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

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

mod cipher;
mod hlc;
mod keychain;
mod pairing;
mod recovery;
mod sync;

pub use cipher::Cipher;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use keychain::OsKeychain;
pub use keychain::{KeyStore, MemoryKeys};
pub use pairing::{Answerer, Ask, AskState, Request};
pub use recovery::RecoveryKey;
pub use sync::{Backend, Folder, FolderState, SyncReport};

const KEY_LEN: usize = 32;
const SPACE: &str = "personal";
const KEY_VERSION: i64 = 1;
/// The design keeps only "host" or "other" in the clear: hosts are counted for
/// plans, everything else (secrets, groups) looks alike.
pub(crate) fn clear_kind(record_type: &str) -> &'static str {
    if record_type == "host" {
        "host"
    } else {
        "other"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("vault database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("sync: {0}")]
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
    #[error("pairing: {0}")]
    Pairing(String),
    /// The vault cannot open without being told something: see [`Lock`].
    #[error("the vault is locked")]
    Locked(Lock),
    #[error("that passphrase does not open the vault")]
    WrongPassphrase,
    #[error("the passphrase is too short: at least {0} characters")]
    WeakPassphrase(usize),
    /// Too easy to guess for a folder others may read: `zxcvbn`'s score, 0–4.
    #[error("the passphrase is too easy to guess (strength {0} of 4; 3 is needed)")]
    GuessablePassphrase(u8),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Why a vault waits to be opened, and what opens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    /// No vault yet and no keychain to keep its key: a passphrase makes one.
    New,
    /// Its passphrase opens it (and so does the recovery key).
    Passphrase,
    /// The keychain has lost its key and there is no passphrase: only the
    /// recovery key opens it.
    Recovery,
}

/// What opens a locked vault (see [`Vault::unlock`]).
pub enum Unlock<'a> {
    Passphrase(&'a str),
    RecoveryKey(&'a str),
    /// Makes a new vault whose key this passphrase keeps ([`Lock::New`]).
    NewPassphrase(&'a str),
}

/// The shortest passphrase taken.
pub const MIN_PASSPHRASE: usize = 10;

/// How hard `passphrase` is to guess, `zxcvbn`'s 0 (very easy) to 4.
pub fn passphrase_strength(passphrase: &str) -> u8 {
    zxcvbn::zxcvbn(passphrase, &[]).score() as u8
}
const PASSPHRASE_AAD: &[u8] = b"shellwing account key wrapped by a passphrase v1";
const RECOVERY_WRAP_AAD: &[u8] = b"shellwing account key wrapped by the recovery key v1";

fn device_secret_aad(name: &str) -> Vec<u8> {
    format!("shellwing device secret v1 {name}").into_bytes()
}

/// How a passphrase becomes a key: Argon2id, about half a second and 64 MiB,
/// so a stolen vault.db is slow to guess at. Kept with the vault, so it can be
/// raised later.
#[derive(Serialize, Deserialize)]
pub(crate) struct Kdf {
    alg: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

impl Kdf {
    pub(crate) fn new() -> Result<Kdf> {
        use base64::Engine as _;
        // Tests run unoptimised: a small cost there, the real one otherwise.
        let (m_kib, t) = if cfg!(test) { (256, 1) } else { (64 * 1024, 3) };
        Ok(Kdf {
            alg: "argon2id".into(),
            m_kib,
            t,
            p: 1,
            salt: base64::engine::general_purpose::STANDARD.encode(random::<16>()?),
        })
    }

    pub(crate) fn key(&self, passphrase: &str) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        use base64::Engine as _;
        if self.alg != "argon2id" {
            return Err(Error::Keychain(format!(
                "unknown passphrase scheme {}",
                self.alg
            )));
        }
        let salt = base64::engine::general_purpose::STANDARD
            .decode(&self.salt)
            .map_err(|_| Error::Decrypt)?;
        let params = argon2::Params::new(self.m_kib, self.t, self.p, Some(KEY_LEN))
            .map_err(|e| Error::Keychain(e.to_string()))?;
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &salt, &mut key[..])
            .map_err(|e| Error::Keychain(e.to_string()))?;
        Ok(key)
    }
}

/// The database, and what it says about itself, before any key is known.
struct Opened {
    db: Connection,
    device: String,
    newest: Option<String>,
    has_records: bool,
}

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

/// A key kept in the vault: what the Keys page and signing in read.
pub struct SshKey {
    pub id: String,
    pub name: String,
    /// One line, as `authorized_keys` takes it.
    pub public: String,
    /// In OpenSSH's format, without a passphrase: the vault is encrypted.
    pub private: Zeroizing<String>,
}

/// A command kept to run in panes: what the Snippets page and the palette read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub command: String,
    /// Where the palette offers it: empty for every host, `group:<name>` or
    /// `host:<name>`.
    pub scope: String,
    /// Enter is pressed after it is typed.
    pub enter: bool,
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

fn record_aad(id: &str, kind: &str) -> Vec<u8> {
    format!("{id}\0{kind}\0{SPACE}\0{KEY_VERSION}").into_bytes()
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
    /// "host" or "other", part of the additional data.
    pub kind: String,
}

const RECOVERY_KEY_AAD: &[u8] = b"shellwing recovery key wrapped by the account key v1";

impl Vault {
    /// Opens (or creates) `vault.db` at `path`, unlocking it with the account key
    /// from `keys`; a first run creates both keys. Without a usable keychain,
    /// or with one that lost the key, it is [`Error::Locked`]: see
    /// [`Vault::unlock`].
    pub fn open(path: &Path, keys: &dyn KeyStore) -> Result<Vault> {
        let o = Self::open_db(path)?;
        let lock = |o: &Opened, err: Error| -> Error {
            match Self::wrapped_account(&o.db, "passphrase") {
                Ok(Some(_)) => Error::Locked(Lock::Passphrase),
                _ if !o.has_records => Error::Locked(Lock::New),
                _ => match Self::wrapped_account(&o.db, "recovery") {
                    Ok(Some(_)) => Error::Locked(Lock::Recovery),
                    _ => err,
                },
            }
        };
        let account = match keys.load() {
            Ok(Some(k)) if k.len() == KEY_LEN => {
                let mut a = Zeroizing::new([0u8; KEY_LEN]);
                a.copy_from_slice(&k);
                a
            }
            Ok(Some(_)) => return Err(Error::Keychain("the stored key has the wrong size".into())),
            Ok(None) if o.has_records || Self::wrapped_account(&o.db, "passphrase")?.is_some() => {
                return Err(lock(&o, Error::KeyMissing));
            }
            Ok(None) => {
                let a = Zeroizing::new(random::<KEY_LEN>()?);
                if let Err(e) = keys.save(&a[..]) {
                    return Err(lock(&o, Error::Keychain(e)));
                }
                a
            }
            Err(e) => return Err(lock(&o, Error::Keychain(e))),
        };
        Self::finish(o, account)
    }

    /// Opens a vault that [`Vault::open`] found locked. A recovery key also
    /// puts the account key back in `keys` when it can, so the next start
    /// needs nothing typed.
    pub fn unlock(path: &Path, keys: &dyn KeyStore, how: Unlock) -> Result<Vault> {
        let o = Self::open_db(path)?;
        let account = match how {
            Unlock::Passphrase(p) => {
                let (Some(wrapped), Some(kdf)) = (
                    Self::wrapped_account(&o.db, "passphrase")?,
                    Self::kdf(&o.db)?,
                ) else {
                    return Err(Error::WrongPassphrase);
                };
                let key = kdf.key(p)?;
                let (a, _) = cipher::open(&key, &wrapped, PASSPHRASE_AAD)
                    .map_err(|_| Error::WrongPassphrase)?;
                Self::account_from(&a)?
            }
            Unlock::RecoveryKey(text) => {
                let rk = RecoveryKey::parse(text).ok_or(Error::WrongRecoveryKey)?;
                let wrapped =
                    Self::wrapped_account(&o.db, "recovery")?.ok_or(Error::WrongRecoveryKey)?;
                let (a, _) = cipher::open(&rk.0, &wrapped, RECOVERY_WRAP_AAD)
                    .map_err(|_| Error::WrongRecoveryKey)?;
                let a = Self::account_from(&a)?;
                let _ = keys.save(&a[..]);
                a
            }
            Unlock::NewPassphrase(p) => {
                if o.has_records || Self::wrapped_account(&o.db, "passphrase")?.is_some() {
                    return Err(Error::Locked(Lock::Passphrase));
                }
                let a = Zeroizing::new(random::<KEY_LEN>()?);
                Self::wrap_with_passphrase(&o.db, &a, p)?;
                a
            }
        };
        Self::finish(o, account)
    }

    fn account_from(plain: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        if plain.len() != KEY_LEN {
            return Err(Error::Decrypt);
        }
        let mut a = Zeroizing::new([0u8; KEY_LEN]);
        a.copy_from_slice(plain);
        Ok(a)
    }

    /// The account key as kept by `by` (`passphrase`, `recovery`), sealed.
    fn wrapped_account(db: &Connection, by: &str) -> Result<Option<Vec<u8>>> {
        Ok(db
            .query_row(
                "SELECT wrapped FROM keys WHERE id = 'account' AND wrapped_by = ?1",
                params![by],
                |r| r.get(0),
            )
            .optional()?)
    }

    fn kdf(db: &Connection) -> Result<Option<Kdf>> {
        let text: Option<String> = db
            .query_row("SELECT value FROM meta WHERE key = 'passphrase'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
    }

    fn wrap_with_passphrase(
        db: &Connection,
        account: &[u8; KEY_LEN],
        passphrase: &str,
    ) -> Result<()> {
        let length = passphrase.chars().count();
        if length < MIN_PASSPHRASE {
            return Err(Error::WeakPassphrase(MIN_PASSPHRASE));
        }
        let kdf = Kdf::new()?;
        let key = kdf.key(passphrase)?;
        let sealed = cipher::seal(Cipher::default(), &key, &account[..], PASSPHRASE_AAD)?;
        db.execute(
            "INSERT INTO meta (key, value) VALUES ('passphrase', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![serde_json::to_string(&kdf)?],
        )?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES ('account', 1, ?1, 'passphrase')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![sealed],
        )?;
        Ok(())
    }

    /// Sets, changes or (with None) removes the passphrase that keeps the
    /// account key besides the keychain.
    pub fn set_vault_passphrase(&self, passphrase: Option<&str>) -> Result<()> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        match passphrase {
            Some(p) => Self::wrap_with_passphrase(&db, &self.account, p),
            None => {
                db.execute(
                    "DELETE FROM keys WHERE id = 'account' AND wrapped_by = 'passphrase'",
                    [],
                )?;
                db.execute("DELETE FROM meta WHERE key = 'passphrase'", [])?;
                Ok(())
            }
        }
    }

    /// Whether a passphrase keeps the account key too.
    pub fn has_vault_passphrase(&self) -> Result<bool> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        Ok(Self::wrapped_account(&db, "passphrase")?.is_some())
    }

    /// Puts the account key in `keys` (the keychain), so the vault opens
    /// without a passphrase.
    pub fn keep_in(&self, keys: &dyn KeyStore) -> Result<()> {
        keys.save(&self.account[..]).map_err(Error::Keychain)
    }

    /// Takes the account key out of `keys`, so every start asks for the
    /// passphrase. Only with a passphrase set: else nothing would open it
    /// but the recovery key.
    pub fn leave_keychain(&self, keys: &dyn KeyStore) -> Result<()> {
        if !self.has_vault_passphrase()? {
            return Err(Error::Locked(Lock::Recovery));
        }
        keys.forget().map_err(Error::Keychain)
    }

    /// Whether `keys` holds this vault's account key.
    pub fn kept_in(&self, keys: &dyn KeyStore) -> bool {
        keys.load()
            .ok()
            .flatten()
            .is_some_and(|k| k[..] == self.account[..])
    }

    fn open_db(path: &Path) -> Result<Opened> {
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
        Ok(Opened {
            db,
            device,
            newest,
            has_records,
        })
    }

    /// The vault, from its database and its account key.
    fn finish(o: Opened, account: Zeroizing<[u8; KEY_LEN]>) -> Result<Vault> {
        let Opened {
            db, device, newest, ..
        } = o;
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
        // A recovery key always opens it, synced or not: made now if there is
        // none, and kept able to open the account key.
        let rk = vault.recovery_key_or_new()?;
        let wraps = {
            let db = vault.db.lock().map_err(|_| Error::Decrypt)?;
            Self::wrapped_account(&db, "recovery")?.is_some()
        };
        if !wraps {
            vault.store_recovery_key(&rk)?;
        }
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
        let rows: Vec<(String, Vec<u8>, String)> = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            let mut q = db.prepare("SELECT id, data, kind FROM records WHERE deleted = 0")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<_, _>>()?
        };
        for (id, data, kind) in rows {
            let kind: String = kind;
            let (plain, legacy) = cipher::open(&self.sk(), &data, &record_aad(&id, &kind))?;
            if legacy {
                let sealed =
                    cipher::seal(self.cipher, &self.sk(), &plain, &record_aad(&id, &kind))?;
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
        let mut out = vec![];
        for row in self.raw_rows()? {
            if row.deleted {
                continue;
            }
            let (plain, _) = cipher::open(&self.sk(), &row.data, &record_aad(&row.id, &row.kind))?;
            out.push((row.id, serde_json::from_slice(&plain)?));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
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
        let kind = clear_kind(&record.kind);
        let data = cipher::seal(self.cipher, &self.sk(), &plain, &record_aad(id, kind))?;
        let newest = record.f.values().map(|f| f.t.as_str()).max().unwrap_or("");
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO records (id, kind, space, key_ver, data, hlc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data, hlc = excluded.hlc,
               kind = excluded.kind, deleted = 0",
            params![id, kind, SPACE, KEY_VERSION, data, newest],
        )?;
        Ok(())
    }

    /// Every row, deleted ones too.
    pub(crate) fn raw_rows(&self) -> Result<Vec<RawRow>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut q =
            db.prepare("SELECT id, data, hlc, deleted, kind FROM records WHERE space = ?1")?;
        let rows = q
            .query_map(params![SPACE], |r| {
                Ok(RawRow {
                    id: r.get(0)?,
                    data: r.get(1)?,
                    hlc: r.get(2)?,
                    deleted: r.get::<_, i64>(3)? != 0,
                    kind: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Stores a merged version from sync.
    pub(crate) fn store_version(&self, id: &str, kind: &str, v: &sync::Version) -> Result<()> {
        match v {
            sync::Version::Live(r) => self.write(id, r),
            sync::Version::Gone(t) => {
                let db = self.db.lock().map_err(|_| Error::Decrypt)?;
                db.execute(
                    "INSERT INTO records (id, kind, space, key_ver, data, hlc, deleted)
                     VALUES (?1, ?2, ?3, ?4, X'', ?5, 1)
                     ON CONFLICT(id) DO UPDATE SET data = X'', hlc = excluded.hlc, deleted = 1",
                    params![id, kind, SPACE, KEY_VERSION, t],
                )?;
                Ok(())
            }
        }
    }

    /// A secret of this device's own, such as the password of the place it
    /// syncs with: sealed with the account key, kept in vault.db, never
    /// synced (other devices sign in there with their own).
    pub fn device_secret(&self, name: &str) -> Result<Option<Zeroizing<String>>> {
        let wrapped: Option<Vec<u8>> = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            db.query_row(
                "SELECT wrapped FROM keys WHERE id = ?1 AND wrapped_by = 'account'",
                params![format!("device:{name}")],
                |r| r.get(0),
            )
            .optional()?
        };
        let Some(w) = wrapped else {
            return Ok(None);
        };
        let (plain, _) = cipher::open(&self.account, &w, &device_secret_aad(name))?;
        let text = String::from_utf8(plain.to_vec()).map_err(|_| Error::Decrypt)?;
        Ok(Some(Zeroizing::new(text)))
    }

    /// Keeps `secret` as this device's `name` (see [`Vault::device_secret`]);
    /// None forgets it.
    pub fn set_device_secret(&self, name: &str, secret: Option<&str>) -> Result<()> {
        let id = format!("device:{name}");
        let sealed = secret
            .map(|s| {
                cipher::seal(
                    self.cipher,
                    &self.account,
                    s.as_bytes(),
                    &device_secret_aad(name),
                )
            })
            .transpose()?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        match sealed {
            Some(w) => db.execute(
                "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES (?1, 1, ?2, 'account')
                 ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
                params![id, w],
            )?,
            None => db.execute(
                "DELETE FROM keys WHERE id = ?1 AND wrapped_by = 'account'",
                params![id],
            )?,
        };
        Ok(())
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

    /// Keeps `rk` as the recovery key: wrapped by the account key (to show
    /// it), and wrapping the account key (so it opens the vault).
    pub(crate) fn store_recovery_key(&self, rk: &RecoveryKey) -> Result<()> {
        let sealed = cipher::seal(self.cipher, &self.account, &rk.0[..], RECOVERY_KEY_AAD)?;
        let opens = cipher::seal(self.cipher, &rk.0, &self.account[..], RECOVERY_WRAP_AAD)?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES ('recovery', 1, ?1, 'account')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![sealed],
        )?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES ('account', 1, ?1, 'recovery')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![opens],
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

    /// The keys kept here, oldest first.
    pub fn ssh_keys(&self) -> Result<Vec<SshKey>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == "ssh_key")
            .map(|(id, r)| {
                let get = |k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
                SshKey {
                    name: get("name"),
                    public: get("public"),
                    private: Zeroizing::new(get("private")),
                    id,
                }
            })
            .collect())
    }

    /// Keeps a key under `name`; the same key kept again is renamed instead.
    /// Returns its id.
    pub fn add_ssh_key(&self, name: &str, public: &str, private: &str) -> Result<String> {
        self.put(
            "ssh_key",
            ("public", public),
            name,
            &[("name", name), ("private", private)],
        )?;
        Ok(self
            .find("ssh_key", "public", public)?
            .map(|(id, _)| id)
            .unwrap_or_default())
    }

    pub fn rename_ssh_key(&self, id: &str, name: &str) -> Result<()> {
        let Some(k) = self.ssh_keys()?.into_iter().find(|k| k.id == id) else {
            return Ok(());
        };
        let values = BTreeMap::from([
            ("name".to_string(), name.to_string()),
            ("public".to_string(), k.public.clone()),
            ("private".to_string(), k.private.to_string()),
        ]);
        self.update(id, "ssh_key", name, &values)?;
        Ok(())
    }

    /// The snippets kept here, oldest first.
    pub fn snippets(&self) -> Result<Vec<Snippet>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == "snippet")
            .map(|(id, r)| {
                let get = |k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
                Snippet {
                    name: get("name"),
                    command: get("command"),
                    scope: get("scope"),
                    // Unset means yes: running is what a snippet is for.
                    enter: get("enter") != "no",
                    id,
                }
            })
            .collect())
    }

    /// Keeps a snippet, new when its id is empty; returns its id.
    pub fn save_snippet(&self, s: &Snippet) -> Result<String> {
        let id = if s.id.is_empty() {
            new_id()?
        } else {
            s.id.clone()
        };
        let values = BTreeMap::from([
            ("name".to_string(), s.name.clone()),
            ("command".to_string(), s.command.clone()),
            ("scope".to_string(), s.scope.clone()),
            (
                "enter".to_string(),
                if s.enter { "yes" } else { "no" }.to_string(),
            ),
        ]);
        self.update(&id, "snippet", &s.name, &values)?;
        Ok(id)
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

    /// Records of one type (`"host"`, `"group"`) as plain values, without
    /// stamps: what a file like hosts.toml holds.
    pub fn entries(&self, record_type: &str) -> Result<Vec<(String, BTreeMap<String, String>)>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == record_type)
            .map(|(id, r)| {
                let values =
                    r.f.iter()
                        .filter(|(_, f)| !f.val.is_empty())
                        .map(|(k, f)| (k.clone(), f.val.clone()))
                        .collect();
                (id, values)
            })
            .collect())
    }

    /// Whether `id` was deleted, here or on another device.
    pub fn is_deleted(&self, id: &str) -> Result<bool> {
        Ok(self.raw_rows()?.iter().any(|r| r.id == id && r.deleted))
    }

    /// Sets a record's values, stamping only the fields that changed (a field no
    /// longer present becomes empty), so a merge keeps another device's edits to
    /// the other fields. Creates the record if it is new. Returns whether
    /// anything changed.
    pub fn update(
        &self,
        id: &str,
        record_type: &str,
        name: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<bool> {
        let old = self
            .records()?
            .into_iter()
            .find(|(i, _)| i == id)
            .map(|(_, r)| r);
        let mut f = old.as_ref().map(|r| r.f.clone()).unwrap_or_default();
        let mut changed = old.is_none();
        let t = self.clock.tick();
        for (k, v) in values {
            if f.get(k).is_none_or(|old| &old.val != v) {
                f.insert(
                    k.clone(),
                    Field {
                        val: v.clone(),
                        t: t.clone(),
                    },
                );
                changed = true;
            }
        }
        for (k, old) in f.iter_mut() {
            if !values.contains_key(k) && !old.val.is_empty() {
                *old = Field {
                    val: String::new(),
                    t: t.clone(),
                };
                changed = true;
            }
        }
        if changed {
            self.write(
                id,
                &Record {
                    v: 1,
                    kind: record_type.into(),
                    name: name.into(),
                    f,
                },
            )?;
        }
        Ok(changed)
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
    fn keeps_edits_and_forgets_snippets() {
        let path = temp("snippets");
        let v = Vault::open(&path, &MemoryKeys::default()).expect("open");
        let mut s = Snippet {
            id: String::new(),
            name: "Disk".into(),
            command: "df -h /".into(),
            scope: String::new(),
            enter: true,
        };
        s.id = v.save_snippet(&s).expect("save");
        s.command = "df -h / && free -h".into();
        s.scope = "group:Production".into();
        s.enter = false;
        assert_eq!(v.save_snippet(&s).expect("edit"), s.id);
        assert_eq!(v.snippets().expect("list"), [s.clone()]);
        assert!(v.items().expect("items").is_empty(), "not a secret");
        v.delete(&s.id).expect("delete");
        assert!(v.snippets().expect("list").is_empty());
    }

    #[test]
    fn keeps_renames_and_forgets_ssh_keys() {
        let path = temp("ssh-keys");
        let v = Vault::open(&path, &MemoryKeys::default()).expect("open");
        let id = v
            .add_ssh_key("laptop", "ssh-ed25519 AAAA me", "PRIVATE")
            .expect("add");
        // The same key again is the same record.
        assert_eq!(
            v.add_ssh_key("laptop 2", "ssh-ed25519 AAAA me", "PRIVATE")
                .expect("again"),
            id
        );
        v.rename_ssh_key(&id, "work").expect("rename");
        let keys = v.ssh_keys().expect("keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(
            (
                keys[0].name.as_str(),
                keys[0].public.as_str(),
                keys[0].private.as_str()
            ),
            ("work", "ssh-ed25519 AAAA me", "PRIVATE")
        );
        // Not a password or a passphrase.
        assert!(v.items().expect("items").is_empty());
        v.delete(&id).expect("delete");
        assert!(v.ssh_keys().expect("keys").is_empty());
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
        // A vault with records whose key vanished says so: the recovery key
        // can open it.
        assert!(matches!(
            Vault::open(&path, &MemoryKeys::default()),
            Err(Error::Locked(Lock::Recovery))
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
            let (plain, _) = cipher::open(&v.sk(), &data, &record_aad(&id, "other")).expect("open");
            db.execute(
                "UPDATE records SET data = ?2 WHERE id = ?1",
                params![id, untagged(&v.sk(), &plain, &record_aad(&id, "other"))],
            )
            .expect("record");
            db.execute(
                "UPDATE keys SET wrapped = ?1 WHERE id = ?2 AND wrapped_by = 'account'",
                params![untagged(&account, &v.sk()[..], SPACE_AAD), SPACE],
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
    fn updates_stamp_only_changed_fields() {
        let path = temp("update");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        let vals = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let stamps = |v: &Vault| -> BTreeMap<String, String> {
            v.records().expect("records")[0]
                .1
                .f
                .iter()
                .map(|(k, f)| (k.clone(), f.t.clone()))
                .collect()
        };
        let host = vals(&[("name", "web"), ("address", "10.0.0.5"), ("port", "22")]);
        assert!(v.update("h1", "host", "web", &host).expect("create"));
        let first = stamps(&v);
        assert!(
            !v.update("h1", "host", "web", &host).expect("same"),
            "nothing changed"
        );
        let edited = vals(&[("name", "web"), ("address", "10.0.0.6")]);
        assert!(v.update("h1", "host", "web", &edited).expect("edit"));
        let second = stamps(&v);
        assert_eq!(
            first["name"], second["name"],
            "untouched field keeps its stamp"
        );
        assert_ne!(first["address"], second["address"]);
        assert_ne!(
            first["port"], second["port"],
            "a removed field is stamped empty"
        );
        assert_eq!(v.entries("host").expect("entries")[0].1, edited);
        assert!(v.entries("group").expect("entries").is_empty());
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

    /// A keychain that is not there, as on Linux without the Secret Service.
    struct NoKeychain;

    impl KeyStore for NoKeychain {
        fn load(&self) -> std::result::Result<Option<Vec<u8>>, String> {
            Err("no secret service".into())
        }
        fn save(&self, _: &[u8]) -> std::result::Result<(), String> {
            Err("no secret service".into())
        }
    }

    fn locked(r: Result<Vault>) -> Lock {
        match r {
            Err(Error::Locked(l)) => l,
            Ok(_) => panic!("opened"),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn without_a_keychain_a_passphrase_keeps_the_key() {
        let dir = std::env::temp_dir().join(format!("shellwing-vault-pass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vault.db");
        assert_eq!(locked(Vault::open(&path, &NoKeychain)), Lock::New);
        assert!(matches!(
            Vault::unlock(&path, &NoKeychain, Unlock::NewPassphrase("short")),
            Err(Error::WeakPassphrase(_))
        ));
        let v = Vault::unlock(
            &path,
            &NoKeychain,
            Unlock::NewPassphrase("correct horse battery"),
        )
        .expect("made");
        v.set_password("u@h:22", "hunter2").expect("save");
        let rk = v.recovery_key().expect("rk").expect("always there");
        drop(v);

        assert_eq!(locked(Vault::open(&path, &NoKeychain)), Lock::Passphrase);
        assert!(matches!(
            Vault::unlock(
                &path,
                &NoKeychain,
                Unlock::Passphrase("wrong horse battery")
            ),
            Err(Error::WrongPassphrase)
        ));
        let v = Vault::unlock(
            &path,
            &NoKeychain,
            Unlock::Passphrase("correct horse battery"),
        )
        .expect("opened");
        assert_eq!(
            v.password("u@h:22")
                .expect("read")
                .as_deref()
                .map(|s| s.as_str()),
            Some("hunter2")
        );
        assert!(v.has_vault_passphrase().expect("has"));
        drop(v);

        // The recovery key opens it too, and puts the key in a keychain that works.
        let keys = MemoryKeys::default();
        let v = Vault::unlock(&path, &keys, Unlock::RecoveryKey(&rk.to_string())).expect("by rk");
        assert!(v.kept_in(&keys));
        drop(v);
        let v = Vault::open(&path, &keys).expect("from the keychain now");
        // Changed, the old passphrase no longer opens it; removed, none does.
        v.set_vault_passphrase(Some("another long passphrase"))
            .expect("change");
        drop(v);
        assert!(
            Vault::unlock(
                &path,
                &NoKeychain,
                Unlock::Passphrase("correct horse battery")
            )
            .is_err()
        );
        let v = Vault::unlock(
            &path,
            &NoKeychain,
            Unlock::Passphrase("another long passphrase"),
        )
        .expect("new one");
        v.set_vault_passphrase(None).expect("remove");
        assert!(!v.has_vault_passphrase().expect("has"));
        drop(v);
        assert_eq!(locked(Vault::open(&path, &NoKeychain)), Lock::Recovery);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_keychain_that_lost_the_key_leaves_the_recovery_key() {
        let dir = std::env::temp_dir().join(format!("shellwing-vault-lost-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vault.db");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        v.set_password("u@h:22", "hunter2").expect("save");
        let rk = v.recovery_key().expect("rk").expect("made on open");
        drop(v);
        // A reinstalled keychain: empty.
        let fresh = MemoryKeys::default();
        assert_eq!(locked(Vault::open(&path, &fresh)), Lock::Recovery);
        let v = Vault::unlock(&path, &fresh, Unlock::RecoveryKey(&rk.to_string())).expect("rk");
        assert_eq!(
            v.password("u@h:22")
                .expect("read")
                .as_deref()
                .map(|s| s.as_str()),
            Some("hunter2")
        );
        drop(v);
        Vault::open(&path, &fresh).expect("the key is back in the keychain");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaving_the_keychain_asks_for_the_passphrase_at_the_next_start() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-leave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vault.db");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("made");
        assert!(
            matches!(v.leave_keychain(&keys), Err(Error::Locked(Lock::Recovery))),
            "not without a passphrase"
        );
        assert!(v.kept_in(&keys));
        v.set_vault_passphrase(Some("correct horse battery"))
            .expect("passphrase");
        v.leave_keychain(&keys).expect("left");
        assert!(!v.kept_in(&keys));
        drop(v);

        assert_eq!(locked(Vault::open(&path, &keys)), Lock::Passphrase);
        let v = Vault::unlock(&path, &keys, Unlock::Passphrase("correct horse battery"))
            .expect("opened");
        assert!(!v.kept_in(&keys), "a passphrase does not put it back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_device_secret_is_kept_sealed_and_can_be_forgotten() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-device-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vault.db");
        let keys = MemoryKeys::default();
        let v = Vault::open(&path, &keys).expect("open");
        assert!(v.device_secret("sync").expect("read").is_none());
        v.set_device_secret("sync", Some("hunter2-webdav"))
            .expect("set");
        v.set_device_secret("sync", Some("hunter3-webdav"))
            .expect("replace");
        assert_eq!(
            v.device_secret("sync")
                .expect("read")
                .as_deref()
                .map(|s| s.as_str()),
            Some("hunter3-webdav")
        );
        assert!(
            v.items().expect("items").is_empty(),
            "not an item: never synced"
        );
        drop(v);
        let raw = std::fs::read(&path).expect("raw");
        assert!(!raw.windows(6).any(|w| w == b"hunter"), "sealed on disk");
        let v = Vault::open(&path, &keys).expect("again");
        v.set_device_secret("sync", None).expect("forget");
        assert!(v.device_secret("sync").expect("read").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

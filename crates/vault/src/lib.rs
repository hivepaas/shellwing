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

mod attachments;
mod cipher;
mod hlc;
mod keychain;
mod pairing;
mod presence;
mod recovery;
mod sync;

pub use attachments::{UNUSED_FOR_MS, attachment_line, attachment_refs, image_mime};
pub use cipher::Cipher;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use keychain::OsKeychain;
pub use keychain::{KeyStore, MemoryKeys};
pub use pairing::{Answerer, Ask, AskState, Request};
pub use presence::{MemoryPresence, Presence};
pub use recovery::RecoveryKey;
pub use sync::{Backend, Folder, FolderState, SyncReport, transact};

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
    #[error("{0}")]
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
    /// The user's presence was not given (cancelled), or no longer opens it.
    #[error("not confirmed")]
    NotPresent,
    #[error("the passphrase is too short: at least {0} characters")]
    WeakPassphrase(usize),
    /// Too easy to guess for a folder others may read: `zxcvbn`'s score, 0–4.
    #[error("the passphrase is too easy to guess (strength {0} of 4; 3 is needed)")]
    GuessablePassphrase(u8),
    #[error("{0}")]
    TooLarge(Oversize),
    /// Not a PNG or a JPEG: the images kept for notes.
    #[error("not an image that can be kept (PNG or JPEG)")]
    NotAnImage,
}

pub type Result<T> = std::result::Result<T, Error>;

/// How much a snippet or an environment holds at most. A shell takes an
/// exported variable of 128 KB at most (Linux's `MAX_ARG_STRLEN`), and
/// all of them with a command's arguments in about 2 MB (`ARG_MAX`):
/// past that, every program it starts fails. These stay well within, and
/// keep each record light to sync.
pub mod limits {
    /// A snippet's command, in bytes.
    pub const COMMAND: usize = 64 * 1024;
    /// A variable's name.
    pub const NAME: usize = 128;
    /// A variable's value, in bytes.
    pub const VALUE: usize = 32 * 1024;
    /// The variables of an environment.
    pub const VARS: usize = 100;
    /// An environment: its `NAME=value`s, in bytes.
    pub const ENVIRONMENT: usize = 64 * 1024;
    /// The environments loaded into one shell, together.
    pub const LOADED: usize = 256 * 1024;
    /// A config file, in bytes.
    pub const FILE: usize = 1024 * 1024;
    /// A note's text, in bytes.
    pub const NOTE: usize = 256 * 1024;
    /// An image kept for notes, in bytes.
    pub const ATTACHMENT: usize = 2 * 1024 * 1024;
    /// The images one note shows, together.
    pub const NOTE_ATTACHMENTS: usize = 20 * 1024 * 1024;
}

/// What is past one of the [`limits`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Oversize {
    /// A command of this many bytes.
    Command(usize),
    /// A variable's name, too long.
    Name(String),
    /// A variable whose value has this many bytes.
    Value(String, usize),
    /// This many variables.
    Vars(usize),
    /// An environment of this many bytes.
    Environment(usize),
    /// A config file of this many bytes.
    File(usize),
    /// A note of this many bytes.
    Note(usize),
    /// An image of this many bytes.
    Attachment(usize),
    /// A note whose images come to this many bytes.
    NoteAttachments(usize),
}

impl std::fmt::Display for Oversize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kb = |n: usize| n.div_ceil(1024);
        match self {
            Oversize::Command(n) => write!(
                f,
                "the command is {} KB; {} KB at most",
                kb(*n),
                kb(limits::COMMAND)
            ),
            Oversize::Name(n) => write!(f, "{n}: a name takes {} characters at most", limits::NAME),
            Oversize::Value(n, b) => write!(
                f,
                "{n} is {} KB; a value takes {} KB at most",
                kb(*b),
                kb(limits::VALUE)
            ),
            Oversize::Vars(n) => write!(f, "{n} variables; {} at most", limits::VARS),
            Oversize::Environment(n) => write!(
                f,
                "the variables come to {} KB; {} KB at most",
                kb(*n),
                kb(limits::ENVIRONMENT)
            ),
            Oversize::File(n) => write!(
                f,
                "the file is {} KB; {} KB at most",
                kb(*n),
                kb(limits::FILE)
            ),
            Oversize::Note(n) => write!(
                f,
                "the note is {} KB; {} KB at most",
                kb(*n),
                kb(limits::NOTE)
            ),
            Oversize::Attachment(n) => write!(
                f,
                "the image is {} KB; {} KB at most",
                kb(*n),
                kb(limits::ATTACHMENT)
            ),
            Oversize::NoteAttachments(n) => write!(
                f,
                "the note's images come to {} MB; {} MB at most",
                n.div_ceil(1024 * 1024),
                limits::NOTE_ATTACHMENTS / (1024 * 1024)
            ),
        }
    }
}

/// What of `vars` (names and values) is past the [`limits`], if any.
pub fn environment_oversize<'a>(
    vars: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<Oversize> {
    let (mut count, mut total) = (0, 0);
    for (name, value) in vars {
        if name.chars().count() > limits::NAME {
            return Some(Oversize::Name(name.to_string()));
        }
        if value.len() > limits::VALUE {
            return Some(Oversize::Value(name.to_string(), value.len()));
        }
        count += 1;
        total += name.len() + 1 + value.len();
    }
    if count > limits::VARS {
        return Some(Oversize::Vars(count));
    }
    (total > limits::ENVIRONMENT).then_some(Oversize::Environment(total))
}

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
    /// The user's presence, where it was set up ([`Vault::set_presence`]).
    Presence(&'a dyn Presence),
}

/// The shortest passphrase taken.
pub const MIN_PASSPHRASE: usize = 10;

/// How hard `passphrase` is to guess, `zxcvbn`'s 0 (very easy) to 4.
pub fn passphrase_strength(passphrase: &str) -> u8 {
    zxcvbn::zxcvbn(passphrase, &[]).score() as u8
}
const PASSPHRASE_AAD: &[u8] = b"shellwing account key wrapped by a passphrase v1";
const RECOVERY_WRAP_AAD: &[u8] = b"shellwing account key wrapped by the recovery key v1";
const PRESENCE_AAD: &[u8] = b"shellwing account key wrapped by the user's presence v1";
/// What the system shows when it asks.
const PRESENCE_REASON: &str = "open shellwing's vault";

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
    /// What it is filed under, to find it among many; empty: none.
    pub category: String,
    /// The environments (their ids) loaded for it alone: it runs in a
    /// subshell that has them, and they go when it ends.
    pub needs: Vec<String>,
    /// Where it came from, when not typed here: `library:<item>` for one
    /// added from the snippet library. Empty: made here.
    pub origin: String,
}

/// A file kept in the vault to place on hosts, such as `~/.kube/config`:
/// what it is, without its content ([`Vault::config_file_content`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigFile {
    pub id: String,
    pub name: String,
    /// What it is filed under; empty: none.
    pub category: String,
    /// Where it goes on a host, `~/` for the home directory.
    pub path: String,
    /// Its permissions there, as `chmod` takes them: 0o600.
    pub mode: u32,
    /// Where it is offered, as a snippet's scope.
    pub scope: String,
    /// Its content's bytes.
    pub size: usize,
    /// Not text (not UTF-8, or holding a NUL): replaced whole, not edited.
    pub binary: bool,
    /// Placed to stay, rather than until the tab closes: what placing it
    /// offers first.
    pub stays: bool,
}

/// A note's format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteFormat {
    #[default]
    Plain,
    Markdown,
}

/// Free text kept in the vault: what it is, without its text
/// ([`Vault::note_text`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub title: String,
    /// What it is filed under; empty: none.
    pub category: String,
    pub format: NoteFormat,
    /// The hosts it is about (their names): the palette offers it first in
    /// their panes.
    pub hosts: Vec<String>,
    /// Its text shows masked until asked for.
    pub sensitive: bool,
    /// When it was last changed, as the vault's clock stamps it.
    pub changed: String,
    /// Its text's bytes.
    pub size: usize,
}

/// Whether `bytes` are text to edit: UTF-8 without a NUL.
pub fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// An environment's variables: names and their values.
pub type EnvValues = Vec<(String, Zeroizing<String>)>;

/// Variables for the shell, kept in the vault, to load into a pane or
/// for a snippet: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub id: String,
    pub name: String,
    /// Where it is offered, as a snippet's scope.
    pub scope: String,
    /// The variables' names; their values only through
    /// [`Vault::environment_values`].
    pub names: Vec<String>,
}

/// A field and when it was set (a clock stamp), for merging edits. A note's
/// text also says what it was written from, so merging can tell an edit made
/// without seeing another (docs/design/note-conflicts.md).
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Field {
    val: String,
    t: String,
    /// The name of the device that wrote it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    by: String,
    /// For each device, the stamp of its newest edit this value builds on,
    /// its own included.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    seen: BTreeMap<String, String>,
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

/// This computer's name, as the system gives it; None when it gives none.
pub fn host_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let from_env = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"));
        let name = from_env.ok().or_else(|| {
            std::fs::read_to_string("/etc/hostname").ok().or_else(|| {
                std::process::Command::new("hostname")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
            })
        });
        name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())
    })
    .clone()
}

static COPY_TITLE: std::sync::OnceLock<fn(&str, Option<&str>) -> String> =
    std::sync::OnceLock::new();

/// How a note's text kept as a copy is titled, from the note's title and the
/// name of the device that wrote the text (None when unknown): set once by
/// the app, in its language (docs/design/note-conflicts.md).
pub fn title_copies(title: fn(&str, Option<&str>) -> String) {
    let _ = COPY_TITLE.set(title);
}

pub(crate) fn copy_title(title: &str, device: Option<&str>) -> String {
    match (COPY_TITLE.get(), device) {
        (Some(f), _) => f(title, device),
        (None, Some(d)) => format!("{title} (from {d})"),
        (None, None) => format!("{title} (from another device)"),
    }
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
    /// This device's name, kept with the notes' texts it writes.
    device_name: std::sync::RwLock<String>,
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
            Unlock::RecoveryKey(_) => {
                let a = Self::account_by(&o.db, &how)?;
                let _ = keys.save(&a[..]);
                a
            }
            Unlock::Passphrase(_) | Unlock::Presence(_) => Self::account_by(&o.db, &how)?,
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

    /// The account key, as the passphrase, recovery key or presence `how`
    /// unwraps it.
    fn account_by(db: &Connection, how: &Unlock) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        match how {
            Unlock::Passphrase(p) => {
                let (Some(wrapped), Some(kdf)) =
                    (Self::wrapped_account(db, "passphrase")?, Self::kdf(db)?)
                else {
                    return Err(Error::WrongPassphrase);
                };
                let key = kdf.key(p)?;
                let (a, _) = cipher::open(&key, &wrapped, PASSPHRASE_AAD)
                    .map_err(|_| Error::WrongPassphrase)?;
                Self::account_from(&a)
            }
            Unlock::RecoveryKey(text) => {
                let rk = RecoveryKey::parse(text).ok_or(Error::WrongRecoveryKey)?;
                let wrapped =
                    Self::wrapped_account(db, "recovery")?.ok_or(Error::WrongRecoveryKey)?;
                let (a, _) = cipher::open(&rk.0, &wrapped, RECOVERY_WRAP_AAD)
                    .map_err(|_| Error::WrongRecoveryKey)?;
                Self::account_from(&a)
            }
            Unlock::Presence(p) => {
                let (Some(wrapped), Some(id)) = (
                    Self::wrapped_account(db, "presence")?,
                    Self::presence_id(db)?,
                ) else {
                    return Err(Error::Locked(Lock::Passphrase));
                };
                let secret = p
                    .secret(&id, PRESENCE_REASON)
                    .map_err(Error::Keychain)?
                    .ok_or(Error::NotPresent)?;
                let (a, _) =
                    cipher::open(&secret, &wrapped, PRESENCE_AAD).map_err(|_| Error::NotPresent)?;
                Self::account_from(&a)
            }
            Unlock::NewPassphrase(_) => Err(Error::Locked(Lock::Passphrase)),
        }
    }

    /// Checks it is the user, at an open vault, before something shows a
    /// secret: their passphrase, recovery key or presence opens its key.
    pub fn confirm(&self, how: Unlock) -> Result<()> {
        let a = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            Self::account_by(&db, &how)?
        };
        if a[..] == self.account[..] {
            Ok(())
        } else {
            Err(match how {
                Unlock::Presence(_) => Error::NotPresent,
                Unlock::RecoveryKey(_) => Error::WrongRecoveryKey,
                _ => Error::WrongPassphrase,
            })
        }
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
    /// This device's id for its presence secret (made with it).
    fn presence_id(db: &Connection) -> Result<Option<String>> {
        Ok(db
            .query_row("SELECT value FROM meta WHERE key = 'presence'", [], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Lets `p` (Windows Hello, Touch ID) open the vault on this device: the
    /// system asks the user once now. Only with a passphrase set: a secret
    /// of this device's alone must not be all that opens it here.
    pub fn set_presence(&self, p: &dyn Presence) -> Result<()> {
        if !self.has_vault_passphrase()? {
            return Err(Error::Locked(Lock::Passphrase));
        }
        let id = {
            let db = self.db.lock().map_err(|_| Error::Decrypt)?;
            match Self::presence_id(&db)? {
                Some(id) => id,
                None => {
                    let id: String = random::<16>()?.iter().map(|b| format!("{b:02x}")).collect();
                    db.execute(
                        "INSERT INTO meta (key, value) VALUES ('presence', ?1)",
                        params![id],
                    )?;
                    id
                }
            }
        };
        let secret = Zeroizing::new(p.enroll(&id, PRESENCE_REASON).map_err(Error::Keychain)?);
        let sealed = cipher::seal(self.cipher, &secret, &self.account[..], PRESENCE_AAD)?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES ('account', 1, ?1, 'presence')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![sealed],
        )?;
        Ok(())
    }

    /// Stops `p` opening the vault, and has the system forget its secret.
    pub fn remove_presence(&self, p: &dyn Presence) -> Result<()> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "DELETE FROM keys WHERE id = 'account' AND wrapped_by = 'presence'",
            [],
        )?;
        if let Some(id) = Self::presence_id(&db)? {
            p.forget(&id).map_err(Error::Keychain)?;
        }
        Ok(())
    }

    /// Whether the user's presence opens the vault on this device.
    pub fn has_presence(&self) -> Result<bool> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        Ok(Self::wrapped_account(&db, "presence")?.is_some())
    }

    /// The same, of the vault at `path` before it is opened (to offer it at
    /// the lock).
    pub fn opens_with_presence(path: &Path) -> bool {
        let read_only = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
        Connection::open_with_flags(path, read_only).is_ok_and(|db| {
            Self::wrapped_account(&db, "presence")
                .ok()
                .flatten()
                .is_some()
        })
    }

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
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY, space TEXT NOT NULL, key_ver INTEGER NOT NULL,
                data BLOB NOT NULL, bytes INTEGER NOT NULL, unused_since INTEGER);",
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
            device_name: std::sync::RwLock::new(host_name().unwrap_or_default()),
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
                    ..Default::default()
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
                    category: get("category"),
                    needs: get("needs")
                        .split(',')
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                        .collect(),
                    origin: get("origin"),
                    id,
                }
            })
            .collect())
    }

    /// Keeps a snippet, new when its id is empty; returns its id.
    pub fn save_snippet(&self, s: &Snippet) -> Result<String> {
        if s.command.len() > limits::COMMAND {
            return Err(Error::TooLarge(Oversize::Command(s.command.len())));
        }
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
            ("category".to_string(), s.category.clone()),
            ("needs".to_string(), s.needs.join(",")),
            ("origin".to_string(), s.origin.clone()),
        ]);
        self.update(&id, "snippet", &s.name, &values)?;
        Ok(id)
    }

    /// The environments kept here, oldest first, without their values.
    pub fn environments(&self) -> Result<Vec<Environment>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == "environment")
            .map(|(id, r)| {
                let get = |k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
                Environment {
                    name: get("name"),
                    scope: get("scope"),
                    names: r
                        .f
                        .iter()
                        .filter(|(_, f)| !f.val.is_empty())
                        .filter_map(|(k, _)| k.strip_prefix("var:").map(str::to_string))
                        .collect(),
                    id,
                }
            })
            .collect())
    }

    /// An environment's variables, with their values, in name order.
    pub fn environment_values(&self, id: &str) -> Result<Option<EnvValues>> {
        Ok(self
            .records()?
            .into_iter()
            .find(|(i, r)| i == id && r.kind == "environment")
            .map(|(_, r)| {
                r.f.iter()
                    .filter(|(_, f)| !f.val.is_empty())
                    .filter_map(|(k, f)| {
                        k.strip_prefix("var:")
                            .map(|n| (n.to_string(), Zeroizing::new(f.val.clone())))
                    })
                    .collect()
            }))
    }

    /// The config files kept here, without their contents.
    pub fn config_files(&self) -> Result<Vec<ConfigFile>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == "config_file")
            .map(|(id, r)| {
                let get = |k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
                ConfigFile {
                    name: get("name"),
                    category: get("category"),
                    path: get("path"),
                    mode: u32::from_str_radix(&get("mode"), 8).unwrap_or(0o600),
                    scope: get("scope"),
                    size: get("size").parse().unwrap_or(0),
                    binary: get("binary") == "yes",
                    stays: get("stays") == "yes",
                    id,
                }
            })
            .collect())
    }

    /// A config file's content.
    pub fn config_file_content(&self, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        use base64::Engine as _;
        let Some((_, r)) = self
            .records()?
            .into_iter()
            .find(|(i, r)| i == id && r.kind == "config_file")
        else {
            return Ok(None);
        };
        let text = r.f.get("content").map(|f| f.val.as_str()).unwrap_or("");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|_| Error::Decrypt)?;
        Ok(Some(Zeroizing::new(bytes)))
    }

    /// Keeps a config file, new when `f.id` is empty; returns its id.
    /// `content` None keeps the one it had; `f.size` and `f.binary` are
    /// worked out from it.
    pub fn save_config_file(&self, f: &ConfigFile, content: Option<&[u8]>) -> Result<String> {
        use base64::Engine as _;
        let id = if f.id.is_empty() {
            new_id()?
        } else {
            f.id.clone()
        };
        let old;
        let content = match content {
            Some(c) => c,
            None => {
                old = self.config_file_content(&id)?.unwrap_or_default();
                &old[..]
            }
        };
        if content.len() > limits::FILE {
            return Err(Error::TooLarge(Oversize::File(content.len())));
        }
        let values = BTreeMap::from([
            ("name".to_string(), f.name.clone()),
            ("category".to_string(), f.category.clone()),
            ("path".to_string(), f.path.clone()),
            ("mode".to_string(), format!("{:o}", f.mode)),
            ("scope".to_string(), f.scope.clone()),
            ("size".to_string(), content.len().to_string()),
            (
                "binary".to_string(),
                if is_text(content) { "no" } else { "yes" }.to_string(),
            ),
            (
                "stays".to_string(),
                if f.stays { "yes" } else { "no" }.to_string(),
            ),
            (
                "content".to_string(),
                base64::engine::general_purpose::STANDARD.encode(content),
            ),
        ]);
        self.update(&id, "config_file", &f.name, &values)?;
        Ok(id)
    }

    /// The notes kept here, without their texts.
    pub fn notes(&self) -> Result<Vec<Note>> {
        Ok(self
            .records()?
            .into_iter()
            .filter(|(_, r)| r.kind == "note")
            .map(|(id, r)| {
                let get = |k: &str| r.f.get(k).map(|f| f.val.clone()).unwrap_or_default();
                let changed =
                    r.f.values()
                        .map(|f| f.t.as_str())
                        .max()
                        .unwrap_or("")
                        .to_string();
                Note {
                    title: get("title"),
                    category: get("category"),
                    format: if get("format") == "markdown" {
                        NoteFormat::Markdown
                    } else {
                        NoteFormat::Plain
                    },
                    hosts: get("hosts")
                        .split('\n')
                        .filter(|h| !h.is_empty())
                        .map(str::to_string)
                        .collect(),
                    sensitive: get("sensitive") == "yes",
                    size: r.f.get("text").map_or(0, |f| f.val.len()),
                    changed,
                    id,
                }
            })
            .collect())
    }

    /// The name this device's edits of a note's text carry.
    fn device_name(&self) -> String {
        self.device_name
            .read()
            .map(|n| n.clone())
            .unwrap_or_default()
    }

    /// This device's id: random, made once, the same as its clock stamps
    /// carry. Nothing about the computer or its user.
    pub fn device_id(&self) -> &str {
        self.clock.device()
    }

    /// Names this device otherwise than its host name does.
    pub fn set_device_name(&self, name: &str) {
        if let Ok(mut n) = self.device_name.write() {
            *n = name.to_string();
        }
    }

    /// A note's text.
    pub fn note_text(&self, id: &str) -> Result<Option<Zeroizing<String>>> {
        Ok(self
            .records()?
            .into_iter()
            .find(|(i, r)| i == id && r.kind == "note")
            .map(|(_, r)| {
                Zeroizing::new(r.f.get("text").map(|f| f.val.clone()).unwrap_or_default())
            }))
    }

    /// Keeps a note, new when `n.id` is empty; returns its id. `text` None
    /// keeps the one it had. Each part is its own field, so a title changed
    /// on one device and the text on another both stay.
    pub fn save_note(&self, n: &Note, text: Option<&str>) -> Result<String> {
        let id = if n.id.is_empty() {
            new_id()?
        } else {
            n.id.clone()
        };
        let old;
        let text = match text {
            Some(t) => t,
            None => {
                old = self.note_text(&id)?.unwrap_or_default();
                old.as_str()
            }
        };
        if text.len() > limits::NOTE {
            return Err(Error::TooLarge(Oversize::Note(text.len())));
        }
        let values = BTreeMap::from([
            ("title".to_string(), n.title.clone()),
            ("category".to_string(), n.category.clone()),
            (
                "format".to_string(),
                match n.format {
                    NoteFormat::Plain => "plain",
                    NoteFormat::Markdown => "markdown",
                }
                .to_string(),
            ),
            ("hosts".to_string(), n.hosts.join("\n")),
            (
                "sensitive".to_string(),
                if n.sensitive { "yes" } else { "no" }.to_string(),
            ),
            ("text".to_string(), text.to_string()),
        ]);
        self.update(&id, "note", &n.title, &values)?;
        Ok(id)
    }

    /// Keeps an environment, new when `id` is empty; returns its id. Each
    /// variable is its own field, so edits on two devices to two variables
    /// both stay. A variable given None keeps the value it had; one left
    /// out is removed.
    pub fn save_environment(
        &self,
        id: &str,
        name: &str,
        scope: &str,
        vars: &[(String, Option<String>)],
    ) -> Result<String> {
        let id = if id.is_empty() {
            new_id()?
        } else {
            id.to_string()
        };
        let old = self.environment_values(&id)?.unwrap_or_default();
        let mut values = BTreeMap::from([
            ("name".to_string(), name.to_string()),
            ("scope".to_string(), scope.to_string()),
        ]);
        for (var, value) in vars {
            let value = match value {
                Some(v) => v.clone(),
                None => old
                    .iter()
                    .find(|(n, _)| n == var)
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default(),
            };
            if !value.is_empty() {
                values.insert(format!("var:{var}"), value);
            }
        }
        let vars = values
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("var:")?, v.as_str())));
        if let Some(o) = environment_oversize(vars) {
            return Err(Error::TooLarge(o));
        }
        self.update(&id, "environment", name, &values)?;
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
                let mut field = Field {
                    val: v.clone(),
                    t: t.clone(),
                    ..Default::default()
                };
                // What a note's text was written from
                // (docs/design/note-conflicts.md).
                if record_type == "note" && k == "text" {
                    field.seen = f.get(k).map(Field::seen_all).unwrap_or_default();
                    field
                        .seen
                        .insert(self.clock.device().to_string(), t.clone());
                    field.by = self.device_name();
                }
                f.insert(k.clone(), field);
                changed = true;
            }
        }
        for (k, old) in f.iter_mut() {
            if !values.contains_key(k) && !old.val.is_empty() {
                *old = Field {
                    val: String::new(),
                    t: t.clone(),
                    ..Default::default()
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
            category: "Checks".into(),
            needs: vec!["e1".into(), "e2".into()],
            origin: "library:diagnostics/pg_ping".into(),
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

    #[test]
    fn the_users_presence_opens_the_vault_beside_its_passphrase() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-presence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vault.db");
        let keys = MemoryKeys::default();
        let hello = MemoryPresence::default();
        let v = Vault::open(&path, &keys).expect("made");
        v.set_password("u@h:22", "hunter2").expect("save");
        assert!(
            matches!(v.set_presence(&hello), Err(Error::Locked(Lock::Passphrase))),
            "not without a passphrase"
        );
        v.set_vault_passphrase(Some("correct horse battery"))
            .expect("passphrase");
        v.set_presence(&hello).expect("presence");
        assert!(v.has_presence().expect("has"));
        v.leave_keychain(&keys).expect("left");
        drop(v);

        assert!(Vault::opens_with_presence(&path));
        assert_eq!(locked(Vault::open(&path, &keys)), Lock::Passphrase);
        hello
            .absent
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(
            Vault::unlock(&path, &keys, Unlock::Presence(&hello)),
            Err(Error::NotPresent)
        ));
        hello
            .absent
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let v = Vault::unlock(&path, &keys, Unlock::Presence(&hello)).expect("present");
        assert_eq!(
            v.password("u@h:22")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("hunter2")
        );
        v.remove_presence(&hello).expect("removed");
        drop(v);
        assert!(!Vault::opens_with_presence(&path));
        assert!(matches!(
            Vault::unlock(&path, &keys, Unlock::Presence(&hello)),
            Err(Error::Locked(Lock::Passphrase))
        ));
        Vault::unlock(&path, &keys, Unlock::Passphrase("correct horse battery"))
            .expect("the passphrase still opens it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snippets_and_environments_stay_within_limits() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-limits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let s = Snippet {
            id: String::new(),
            name: "big".into(),
            command: "x".repeat(limits::COMMAND + 1),
            scope: String::new(),
            enter: true,
            category: String::new(),
            needs: vec![],
            origin: String::new(),
        };
        assert!(matches!(
            v.save_snippet(&s),
            Err(Error::TooLarge(Oversize::Command(_)))
        ));
        let big = "v".repeat(limits::VALUE + 1);
        assert!(matches!(
            v.save_environment("", "e", "", &[("A".into(), Some(big))]),
            Err(Error::TooLarge(Oversize::Value(n, _))) if n == "A"
        ));
        // Three values that fit alone, too much together.
        let third = "v".repeat(limits::VALUE - 10);
        let vars: Vec<(String, Option<String>)> = ["A", "B", "C"]
            .iter()
            .map(|n| (n.to_string(), Some(third.clone())))
            .collect();
        assert!(matches!(
            v.save_environment("", "e", "", &vars),
            Err(Error::TooLarge(Oversize::Environment(_)))
        ));
        // A kept value counts as it is.
        let id = v
            .save_environment("", "e", "", &vars[..1])
            .expect("one fits");
        let more = [(vars[0].0.clone(), None), vars[1].clone(), vars[2].clone()];
        assert!(v.save_environment(&id, "e", "", &more).is_err());
        let many: Vec<(String, Option<String>)> = (0..=limits::VARS)
            .map(|i| (format!("V{i}"), Some("1".into())))
            .collect();
        assert!(matches!(
            v.save_environment("", "e", "", &many),
            Err(Error::TooLarge(Oversize::Vars(_)))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_files_keep_their_bytes_and_stay_within_a_megabyte() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let mut f = ConfigFile {
            id: String::new(),
            name: "kube prod".into(),
            category: "Kubernetes".into(),
            path: "~/.kube/config".into(),
            mode: 0o600,
            scope: "group:prod".into(),
            size: 0,
            binary: false,
            stays: false,
        };
        let id = v
            .save_config_file(&f, Some(b"apiVersion: v1\n"))
            .expect("saved");
        let listed = v.config_files().expect("listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].size, listed[0].binary, listed[0].mode),
            (15, false, 0o600)
        );
        assert_eq!(listed[0].category, "Kubernetes");
        // Renamed, its content kept.
        f.id = id.clone();
        f.name = "kube".into();
        v.save_config_file(&f, None).expect("renamed");
        assert_eq!(
            v.config_file_content(&id)
                .expect("read")
                .as_deref()
                .map(|c| &c[..]),
            Some(&b"apiVersion: v1\n"[..])
        );
        // Placed to stay, as chosen.
        assert!(!listed[0].stays);
        f.stays = true;
        v.save_config_file(&f, None).expect("to stay");
        assert!(v.config_files().expect("listed")[0].stays);
        // Bytes that are not text.
        v.save_config_file(&f, Some(&[0x30, 0x82, 0, 1]))
            .expect("binary");
        assert!(v.config_files().expect("listed")[0].binary);
        let big = vec![b'x'; limits::FILE + 1];
        assert!(matches!(
            v.save_config_file(&f, Some(&big)),
            Err(Error::TooLarge(Oversize::File(_)))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_keep_their_parts_apart() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let mut n = Note {
            id: String::new(),
            title: "Runbook".into(),
            category: "Ops".into(),
            format: NoteFormat::Markdown,
            hosts: vec!["web-1".into(), "db".into()],
            sensitive: true,
            changed: String::new(),
            size: 0,
        };
        let id = v
            .save_note(&n, Some("# Restart\n\n- `systemctl restart app`"))
            .expect("saved");
        let listed = v.notes().expect("listed");
        assert_eq!(listed.len(), 1);
        let got = &listed[0];
        assert_eq!(
            (
                got.title.as_str(),
                got.format,
                got.sensitive,
                got.hosts.len()
            ),
            ("Runbook", NoteFormat::Markdown, true, 2)
        );
        assert!(!got.changed.is_empty());
        // Retitled, its text kept.
        n.id = id.clone();
        n.title = "Runbook: app".into();
        n.hosts.clear();
        v.save_note(&n, None).expect("retitled");
        assert_eq!(
            v.note_text(&id)
                .expect("read")
                .as_deref()
                .map(|t| t.as_str()),
            Some("# Restart\n\n- `systemctl restart app`")
        );
        assert!(v.notes().expect("listed")[0].hosts.is_empty());
        let big = "x".repeat(limits::NOTE + 1);
        assert!(matches!(
            v.save_note(&n, Some(&big)),
            Err(Error::TooLarge(Oversize::Note(_)))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_vault_confirms_its_passphrase_only() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-vault-confirm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        assert!(matches!(
            v.confirm(Unlock::Passphrase("anything at all")),
            Err(Error::WrongPassphrase)
        ));
        v.set_vault_passphrase(Some("correct horse battery"))
            .expect("passphrase");
        v.confirm(Unlock::Passphrase("correct horse battery"))
            .expect("confirmed");
        assert!(matches!(
            v.confirm(Unlock::Passphrase("wrong horse battery")),
            Err(Error::WrongPassphrase)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn environments_keep_values_apart_from_their_names() {
        let dir = std::env::temp_dir().join(format!("shellwing-vault-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let id = v
            .save_environment(
                "",
                "aws-prod",
                "group:prod",
                &[
                    ("AWS_ACCESS_KEY_ID".into(), Some("AKIA1".into())),
                    ("AWS_SECRET_ACCESS_KEY".into(), Some("s3cret".into())),
                ],
            )
            .expect("save");
        let listed = v.environments().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].names,
            ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
        );
        assert_eq!(listed[0].scope, "group:prod");
        // None keeps a value; a name left out goes.
        v.save_environment(
            &id,
            "aws-prod",
            "",
            &[
                ("AWS_SECRET_ACCESS_KEY".into(), None),
                ("AWS_REGION".into(), Some("eu-west-1".into())),
            ],
        )
        .expect("edit");
        let values = v.environment_values(&id).expect("read").expect("there");
        let said: Vec<(String, String)> = values
            .into_iter()
            .map(|(n, x)| (n, x.to_string()))
            .collect();
        assert_eq!(
            said,
            [
                ("AWS_REGION".to_string(), "eu-west-1".to_string()),
                ("AWS_SECRET_ACCESS_KEY".to_string(), "s3cret".to_string())
            ]
        );
        assert!(v.items().expect("items").is_empty(), "not a password");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

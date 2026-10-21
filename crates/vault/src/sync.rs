//! Sync through any store of files (docs/design/sync.md).
//!
//! The store holds `shellwing/format.json` (which key the records use, and that
//! key wrapped by the recovery key) and `shellwing/records/<id>.sw`, one sealed
//! record per file. Merging is per field, by clock stamp, so it gives the same
//! result in any order and on stores without locking: a write lost to a race is
//! merged back at the next sync by the device that still has it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::attachments::ATTACHMENTS;
use crate::recovery::RecoveryKey;
use crate::{Error, Field, KEY_LEN, KEY_VERSION, Record, Result, SPACE, Vault, cipher, clear_kind};

const FORMAT: &str = "format.json";
const RECORDS: &str = "records";
const RECOVERY_AAD: &[u8] = b"shellwing space key wrapped by the recovery key v1";

/// Where synced files live. Names are relative: `format.json`,
/// `records/<file>`, `pairing/<file>`.
pub trait Backend: Send + Sync {
    /// File names in the folder `dir` (`records`, `pairing`).
    fn list(&self, dir: &str) -> io::Result<Vec<String>>;
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn write(&self, name: &str, data: &[u8]) -> io::Result<()>;
    fn remove(&self, name: &str) -> io::Result<()>;
    /// Writes `name` only if it is still as this backend last saw it (in a
    /// listing or a read): unchanged since, or still absent. `Ok(false)`:
    /// another device wrote it meanwhile, and reading it now gives theirs.
    /// A backend that cannot tell (a folder) just writes.
    fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
        self.write(name, data).map(|()| true)
    }
    /// Takes in what other devices published (a Git fetch), dropping what
    /// this one wrote and did not publish. A folder or a server has nothing
    /// to do: what is written there is published.
    fn refresh(&self) -> io::Result<()> {
        Ok(())
    }
    /// Publishes what this device wrote since the last refresh (a Git commit
    /// and push). `Ok(false)`: another device published first; refresh and
    /// do it again.
    fn publish(&self) -> io::Result<bool> {
        Ok(true)
    }
}

/// Tries at publishing, each over what the others published just before.
const PUBLISH_TRIES: usize = 3;

/// Runs `op` against `b` as one change: over what others published, and
/// published at the end; done again over theirs if they published first.
/// Not nested: an inner one's refresh would drop the outer one's writes.
pub fn transact<T>(b: &dyn Backend, mut op: impl FnMut(&dyn Backend) -> Result<T>) -> Result<T> {
    for _ in 0..PUBLISH_TRIES {
        b.refresh()?;
        let done = op(b)?;
        if b.publish()? {
            return Ok(done);
        }
    }
    Err(Error::Io(io::Error::other(
        "other devices kept publishing first; trying again at the next sync",
    )))
}

/// Tries at writing a record that others keep writing meanwhile, in one
/// sync; the next one tries again.
const REPLACE_TRIES: usize = 3;

/// A folder on disk: iCloud Drive, Syncthing, a Dropbox or OneDrive folder, a
/// network share. Files go in `<folder>/shellwing/`.
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    pub fn new(folder: &Path) -> Self {
        Folder {
            root: folder.join("shellwing"),
        }
    }
}

impl Backend for Folder {
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        let dir = self.root.join(dir);
        match std::fs::read_dir(&dir) {
            Ok(entries) => Ok(entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                // Our own half-written files.
                .filter(|n| !n.starts_with('.'))
                .collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.root.join(name)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let path = self.root.join(name);
        let dir = path.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(dir)?;
        // A temp file renamed into place, so a sync tool never copies half of one.
        let tmp = dir.join(format!(
            ".{}.{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("f"),
            std::process::id()
        ));
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &path)
    }

    fn remove(&self, name: &str) -> io::Result<()> {
        match std::fs::remove_file(self.root.join(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Format {
    format: u32,
    space: String,
    /// Which space key the records use: a hash of it, safe to show.
    key_id: String,
    /// The space key, sealed with the recovery key.
    recovery: String,
    /// The space key and the recovery key, sealed with a passphrase, when the
    /// user lets a passphrase join devices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passphrase: Option<FolderPassphrase>,
}

#[derive(Serialize, Deserialize)]
struct FolderPassphrase {
    kdf: crate::Kdf,
    sealed: String,
}

const FOLDER_PASSPHRASE_AAD: &[u8] =
    b"shellwing space and recovery keys wrapped by a passphrase v1";

/// One record in the store: the clear part and the sealed payload (absent for
/// a deleted record).
#[derive(Serialize, Deserialize)]
struct Envelope {
    id: String,
    kind: String,
    space: String,
    key: i64,
    /// The newest stamp in it (the deletion's, for a tombstone).
    hlc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

/// A record's state on one side of a merge.
#[derive(Clone)]
pub(crate) enum Version {
    Live(Record),
    /// Deleted at this stamp.
    Gone(String),
}

impl Record {
    fn newest(&self) -> &str {
        self.f.values().map(|f| f.t.as_str()).max().unwrap_or("")
    }
}

/// The device a stamp was made on: its end.
fn stamp_device(t: &str) -> Option<&str> {
    t.splitn(3, '-').nth(2)
}

impl Field {
    /// What the value builds on, itself included. A value from before `seen`
    /// has seen only itself.
    pub(crate) fn seen_all(&self) -> BTreeMap<String, String> {
        let mut seen = self.seen.clone();
        if let Some(device) = stamp_device(&self.t) {
            let newest = seen.entry(device.to_string()).or_default();
            if *newest < self.t {
                newest.clone_from(&self.t);
            }
        }
        seen
    }

    /// Whether this value was written after seeing `other`.
    fn has_seen(&self, other: &Field) -> bool {
        stamp_device(&other.t)
            .and_then(|d| self.seen_all().remove(d))
            .is_some_and(|t| t >= other.t)
    }
}

impl Version {
    fn newest(&self) -> &str {
        match self {
            Version::Live(r) => r.newest(),
            Version::Gone(t) => t,
        }
    }

    fn same(&self, other: &Version) -> bool {
        match (self, other) {
            (Version::Live(a), Version::Live(b)) => {
                a.kind == b.kind
                    && a.name == b.name
                    && a.f.len() == b.f.len()
                    && a.f.iter().zip(&b.f).all(|((ka, fa), (kb, fb))| {
                        ka == kb && fa.val == fb.val && fa.t == fb.t && fa.seen == fb.seen
                    })
            }
            (Version::Gone(a), Version::Gone(b)) => a == b,
            _ => false,
        }
    }
}

/// Merges two versions of one record: each field keeps its newest value, and a
/// deletion wins over edits made before it (an edit after it brings it back).
/// A note's text that loses to one written without seeing it comes back too:
/// the losing side's version, to keep as a copy
/// (docs/design/note-conflicts.md).
pub(crate) fn merge(a: Version, b: Version) -> (Version, Option<Record>) {
    match (a, b) {
        (Version::Gone(x), Version::Gone(y)) => (Version::Gone(x.max(y)), None),
        (Version::Live(r), Version::Gone(t)) | (Version::Gone(t), Version::Live(r)) => {
            if t.as_str() > r.newest() {
                (Version::Gone(t), None)
            } else {
                (Version::Live(r), None)
            }
        }
        (Version::Live(a), Version::Live(b)) => {
            let newer_name = if b.newest() > a.newest() {
                b.name.clone()
            } else {
                a.name.clone()
            };
            let mut f: BTreeMap<String, Field> = a.f.clone();
            for (k, fb) in &b.f {
                match f.get(k) {
                    // Ties (the same stamp) keep the larger value, so every
                    // device picks the same one.
                    Some(fa)
                        if (fa.t.as_str(), fa.val.as_str()) >= (fb.t.as_str(), fb.val.as_str()) => {
                    }
                    _ => {
                        f.insert(k.clone(), fb.clone());
                    }
                }
            }
            let mut lost = None;
            if a.kind == "note"
                && let (Some(ta), Some(tb)) = (a.f.get("text"), b.f.get("text"))
                && let Some(text) = f.get_mut("text")
            {
                if ta.val != tb.val && !ta.has_seen(tb) && !tb.has_seen(ta) {
                    let a_won = (&text.t, &text.val) == (&ta.t, &ta.val);
                    lost = Some(if a_won { b.clone() } else { a.clone() });
                }
                // The result has seen both, so neither is found again.
                let mut seen = ta.seen_all();
                for (device, t) in tb.seen_all() {
                    let newest = seen.entry(device).or_default();
                    if *newest < t {
                        *newest = t;
                    }
                }
                text.seen = seen;
            }
            let merged = Record {
                v: a.v.max(b.v),
                kind: a.kind.clone(),
                name: newer_name,
                f,
            };
            (Version::Live(merged), lost)
        }
    }
}

/// A record merged, to keep here and in the store.
struct Pending {
    id: String,
    /// As it is here, and in the store's files.
    here: Option<Version>,
    there: Vec<(String, Version)>,
    merged: Version,
}

/// The note to keep a lost text in: `text` is the losing side's version of
/// note `note`. Its id and every stamp come from the losing edit, so each
/// device that finds the loss makes the same note, and a copy deleted since
/// stays deleted (docs/design/note-conflicts.md).
fn copy_of(note: &str, text: &Record) -> Option<(String, Record)> {
    let lost = text.f.get("text")?;
    let ms: u64 = lost.t.split('-').next()?.parse().ok()?;
    let hash = Sha256::new()
        .chain_update(b"shellwing note copy\0")
        .chain_update(note)
        .chain_update(b"\0")
        .chain_update(&lost.t)
        .finalize();
    let hex: String = hash[..10].iter().map(|b| format!("{b:02x}")).collect();
    let title = text.f.get("title").map_or("", |f| f.val.as_str());
    let by = Some(lost.by.as_str()).filter(|b| !b.is_empty());
    let title = crate::copy_title(title, by);
    let mut f = text.f.clone();
    for field in f.values_mut() {
        field.t.clone_from(&lost.t);
    }
    f.insert(
        "title".into(),
        Field {
            val: title.clone(),
            t: lost.t.clone(),
            ..Default::default()
        },
    );
    let copy = Record {
        v: text.v,
        kind: text.kind.clone(),
        name: title,
        f,
    };
    Some((format!("{ms:012x}{hex}"), copy))
}

/// What a sync did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Records changed here from the store.
    pub pulled: usize,
    /// Records written to the store.
    pub pushed: usize,
    /// Images sent to the store.
    pub images_pushed: usize,
    /// Images fetched from it.
    pub images_pulled: usize,
    /// Images a note shows that are not in the store yet.
    pub images_missing: usize,
    /// The titles of notes made here to keep a text another device's edit
    /// replaced without seeing it.
    pub copies: Vec<String>,
}

/// Whether a store already holds a vault, and whose.
#[derive(Debug, PartialEq, Eq)]
pub enum FolderState {
    Empty,
    /// This vault's key: syncing can start.
    Ours,
    /// Another vault's: join it with its recovery key.
    Other,
}

pub(crate) fn key_id(key: &[u8; KEY_LEN]) -> String {
    let hash = Sha256::digest(key);
    hash[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn file_id(name: &str) -> Option<&str> {
    // `<id>.sw`, or a copy a sync tool made of it (`<id> (conflicted copy).sw`,
    // `<id>.sync-conflict-….sw`): the id is the leading run of hex digits.
    let end = name
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(name.len());
    (end >= 16 && name.ends_with(".sw")).then(|| &name[..end])
}

impl Vault {
    fn format(&self, b: &dyn Backend) -> Result<Option<Format>> {
        match b.read(FORMAT)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn folder_state(&self, b: &dyn Backend) -> Result<FolderState> {
        Ok(match self.format(b)? {
            None => FolderState::Empty,
            Some(f) if f.key_id == key_id(&self.sk()) => FolderState::Ours,
            Some(_) => FolderState::Other,
        })
    }

    /// Starts syncing into an empty store (or one already holding this vault):
    /// writes `format.json` and returns the recovery key to show the user.
    pub fn start_sync(&self, b: &dyn Backend) -> Result<RecoveryKey> {
        let rk = self.recovery_key_or_new()?;
        match self.folder_state(b)? {
            FolderState::Other => return Err(Error::OtherVault),
            FolderState::Ours => return Ok(rk),
            FolderState::Empty => {}
        }
        let format = Format {
            format: 1,
            space: SPACE.into(),
            key_id: key_id(&self.sk()),
            recovery: B64.encode(cipher::seal(
                self.cipher,
                &rk.0,
                &self.sk()[..],
                RECOVERY_AAD,
            )?),
            passphrase: None,
        };
        b.write(FORMAT, &serde_json::to_vec_pretty(&format)?)?;
        Ok(rk)
    }

    /// Takes the space key of the vault in `b`, opened with its recovery key,
    /// so this device reads and writes the same records. Local records are
    /// sealed again with that key and merged at the next sync.
    pub fn join(&self, b: &dyn Backend, recovery: &str) -> Result<()> {
        let rk = RecoveryKey::parse(recovery).ok_or(Error::WrongRecoveryKey)?;
        let format = self.format(b)?.ok_or(Error::NotSynced)?;
        let sealed = B64
            .decode(&format.recovery)
            .map_err(|_| Error::WrongRecoveryKey)?;
        let (key, _) =
            cipher::open(&rk.0, &sealed, RECOVERY_AAD).map_err(|_| Error::WrongRecoveryKey)?;
        if key.len() != KEY_LEN {
            return Err(Error::WrongRecoveryKey);
        }
        let mut new_key = Zeroizing::new([0u8; KEY_LEN]);
        new_key.copy_from_slice(&key);
        if key_id(&new_key) != format.key_id {
            return Err(Error::WrongRecoveryKey);
        }
        self.adopt(new_key, &rk)
    }

    /// Lets `passphrase` join new devices to the vault in `b` (with None,
    /// stops). Anyone who can read the folder can guess at it, so it must be
    /// hard to guess: [`crate::passphrase_strength`] 3 or more.
    pub fn let_passphrase_join(&self, b: &dyn Backend, passphrase: Option<&str>) -> Result<()> {
        let mut format = self.format(b)?.ok_or(Error::NotSynced)?;
        if format.key_id != key_id(&self.sk()) {
            return Err(Error::OtherVault);
        }
        format.passphrase = match passphrase {
            None => None,
            Some(p) => {
                let strength = crate::passphrase_strength(p);
                if strength < 3 || p.chars().count() < crate::MIN_PASSPHRASE {
                    return Err(Error::GuessablePassphrase(strength));
                }
                let rk = self.recovery_key_or_new()?;
                let kdf = crate::Kdf::new()?;
                let mut both = Zeroizing::new(Vec::with_capacity(2 * KEY_LEN));
                both.extend_from_slice(&self.sk()[..]);
                both.extend_from_slice(&rk.0[..]);
                let key = kdf.key(p)?;
                let sealed = cipher::seal(self.cipher, &key, &both, FOLDER_PASSPHRASE_AAD)?;
                Some(FolderPassphrase {
                    kdf,
                    sealed: B64.encode(sealed),
                })
            }
        };
        b.write(FORMAT, &serde_json::to_vec_pretty(&format)?)?;
        Ok(())
    }

    /// Whether a passphrase joins devices to the vault in `b`.
    pub fn passphrase_joins(&self, b: &dyn Backend) -> Result<bool> {
        Ok(self.format(b)?.is_some_and(|f| f.passphrase.is_some()))
    }

    /// Joins the vault in `b` with the passphrase it lets join, as
    /// [`Vault::join`] does with its recovery key.
    pub fn join_with_passphrase(&self, b: &dyn Backend, passphrase: &str) -> Result<()> {
        let format = self.format(b)?.ok_or(Error::NotSynced)?;
        let fp = format.passphrase.as_ref().ok_or(Error::WrongPassphrase)?;
        let sealed = B64.decode(&fp.sealed).map_err(|_| Error::WrongPassphrase)?;
        let opener = fp.kdf.key(passphrase)?;
        let (both, _) = cipher::open(&opener, &sealed, FOLDER_PASSPHRASE_AAD)
            .map_err(|_| Error::WrongPassphrase)?;
        if both.len() != 2 * KEY_LEN {
            return Err(Error::WrongPassphrase);
        }
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        key.copy_from_slice(&both[..KEY_LEN]);
        let mut rk = Zeroizing::new([0u8; KEY_LEN]);
        rk.copy_from_slice(&both[KEY_LEN..]);
        if key_id(&key) != format.key_id {
            return Err(Error::WrongPassphrase);
        }
        self.adopt(key, &RecoveryKey(rk))
    }

    /// Whether `key` is the key of the vault in `b`.
    pub(crate) fn is_key_of(&self, b: &dyn Backend, key: &[u8; KEY_LEN]) -> Result<bool> {
        let format = self.format(b)?.ok_or(Error::NotSynced)?;
        Ok(key_id(key) == format.key_id)
    }

    /// Makes `new_key` this vault's key (another vault's, checked by the
    /// caller): local records are sealed again with it, to merge at the next
    /// sync, and `rk` is kept as the recovery key.
    pub(crate) fn adopt(&self, new_key: Zeroizing<[u8; KEY_LEN]>, rk: &RecoveryKey) -> Result<()> {
        let rows = self.raw_rows()?;
        let mut db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let tx = db.transaction()?;
        for crate::RawRow {
            id,
            data,
            deleted,
            kind,
            ..
        } in rows
        {
            if deleted {
                continue;
            }
            let aad = crate::record_aad(&id, &kind);
            let (plain, _) = cipher::open(&self.sk(), &data, &aad)?;
            let resealed = cipher::seal(self.cipher, &new_key, &plain, &aad)?;
            tx.execute(
                "UPDATE records SET data = ?2 WHERE id = ?1",
                rusqlite::params![id, resealed],
            )?;
        }
        self.reseal_attachments(&tx, &new_key)?;
        tx.execute(
            "UPDATE keys SET wrapped = ?1 WHERE id = ?2 AND ver = ?3 AND wrapped_by = 'account'",
            rusqlite::params![
                cipher::seal(self.cipher, &self.account, &new_key[..], crate::SPACE_AAD)?,
                SPACE,
                KEY_VERSION
            ],
        )?;
        tx.commit()?;
        drop(db);
        *self.space_key.write().unwrap_or_else(|e| e.into_inner()) = new_key;
        self.store_recovery_key(rk)?;
        Ok(())
    }

    /// Merges this vault with the store, both ways.
    pub fn sync(&self, b: &dyn Backend) -> Result<SyncReport> {
        self.sync_holding(b, &BTreeSet::new())
    }

    /// As [`Vault::sync`], leaving the records `held` alone both ways (a
    /// note open in its window: it syncs when the window closes).
    pub fn sync_holding(&self, b: &dyn Backend, held: &BTreeSet<String>) -> Result<SyncReport> {
        // A copy made in a round whose publishing was refused is kept here,
        // and goes out in the next round, which finds it made: it is still
        // said.
        let mut copies = Vec::new();
        let mut report = transact(b, |b| {
            let round = self.sync_round(b, held)?;
            copies.extend(round.copies.iter().cloned());
            Ok(round)
        })?;
        report.copies = copies;
        Ok(report)
    }

    /// One sync, over the files as they are now.
    fn sync_round(&self, b: &dyn Backend, held: &BTreeSet<String>) -> Result<SyncReport> {
        match self.folder_state(b)? {
            FolderState::Ours => {}
            FolderState::Empty => return Err(Error::NotSynced),
            FolderState::Other => return Err(Error::OtherVault),
        }
        // Each record's clear kind, from either side.
        let mut kinds: BTreeMap<String, String> = BTreeMap::new();
        // Remote versions of each record, with the files they came from.
        let mut remote: BTreeMap<String, Vec<(String, Version)>> = BTreeMap::new();
        for name in b.list(RECORDS)? {
            let Some(id) = file_id(&name).map(str::to_string) else {
                continue;
            };
            let Some(bytes) = b.read(&format!("{RECORDS}/{name}"))? else {
                continue;
            };
            let Some((kind, version)) = self.version_in(&id, &bytes)? else {
                continue;
            };
            kinds.entry(id.clone()).or_insert(kind);
            remote.entry(id).or_default().push((name, version));
        }
        let mut local: BTreeMap<String, Version> = BTreeMap::new();
        for crate::RawRow {
            id,
            data,
            hlc,
            deleted,
            kind,
        } in self.raw_rows()?
        {
            let v = self.row_version(&id, &kind, &data, hlc, deleted)?;
            kinds.insert(id.clone(), kind);
            local.insert(id, v);
        }
        let mut report = SyncReport::default();
        // Images first: those the notes here refer to that the store lacks,
        // so no device finds a note before its images
        // (docs/design/note-images.md). This also puts back one that went
        // missing.
        let listed: BTreeMap<String, String> = b
            .list(ATTACHMENTS)?
            .into_iter()
            .filter_map(|name| file_id(&name).map(|id| (id.to_string(), name.clone())))
            .collect();
        let refs_here: BTreeSet<String> = local
            .values()
            .filter_map(|v| match v {
                Version::Live(r) => Some(r.attachment_refs()),
                Version::Gone(_) => None,
            })
            .flatten()
            .collect();
        let stored: BTreeSet<String> = self
            .stored_attachments()?
            .into_iter()
            .map(|a| a.id)
            .collect();
        for id in refs_here.intersection(&stored) {
            if listed.contains_key(id) {
                continue;
            }
            if let Some(data) = self.sealed_attachment(id)? {
                // Refused: another device put it there since the listing.
                b.replace(&format!("{ATTACHMENTS}/{id}.sw"), &data)?;
                report.images_pushed += 1;
            }
        }
        // The images the notes refer to once merged, to fetch the missing.
        let mut used: BTreeSet<String> = BTreeSet::new();
        // Every record is merged before any is written, so the notes' texts
        // lost to an edit made without seeing them are kept as notes of
        // their own (docs/design/note-conflicts.md).
        let mut queue: VecDeque<Pending> = VecDeque::new();
        let mut lost: Vec<(String, Record)> = Vec::new();
        let ids: BTreeSet<String> = remote.keys().chain(local.keys()).cloned().collect();
        for id in ids {
            if held.contains(&id) {
                if let Some(Version::Live(r)) = local.get(&id) {
                    used.extend(r.attachment_refs());
                }
                continue;
            }
            let here = local.remove(&id);
            let there = remote.remove(&id).unwrap_or_default();
            let mut merged = here.clone();
            for (_, v) in &there {
                merged = Some(match merged {
                    Some(m) => {
                        let (m, text) = merge(m, v.clone());
                        lost.extend(text.map(|r| (id.clone(), r)));
                        m
                    }
                    None => v.clone(),
                });
            }
            let Some(merged) = merged else { continue };
            queue.push_back(Pending {
                id,
                here,
                there,
                merged,
            });
        }
        self.keep_copies(lost, held, &mut queue, &mut report)?;
        while let Some(p) = queue.pop_front() {
            let lost = self.settle(b, p, &kinds, &mut used, &mut report)?;
            self.keep_copies(lost, held, &mut queue, &mut report)?;
        }
        // Images the merged notes show that are not here yet, from the
        // store, checked to be what their ids say.
        for id in used.difference(&stored) {
            let data = match listed.get(id) {
                Some(name) => b.read(&format!("{ATTACHMENTS}/{name}"))?,
                None => None,
            };
            match data.and_then(|d| self.check_attachment(id, &d).map(|n| (d, n))) {
                Some((data, bytes)) => {
                    self.store_attachment(id, &data, bytes)?;
                    report.images_pulled += 1;
                }
                // Not synced yet (another device still sending it), or
                // damaged: the note shows a placeholder; tried again next time.
                None => report.images_missing += 1,
            }
        }
        // Those no note has shown for long go, here and there.
        for id in self.tend_attachments(&used, crate::now_ms())? {
            if let Some(name) = listed.get(&id) {
                b.remove(&format!("{ATTACHMENTS}/{name}"))?;
            }
        }
        Ok(report)
    }

    /// A record row's version.
    fn row_version(
        &self,
        id: &str,
        kind: &str,
        data: &[u8],
        hlc: String,
        deleted: bool,
    ) -> Result<Version> {
        Ok(if deleted {
            Version::Gone(hlc)
        } else {
            let (plain, _) = cipher::open(&self.sk(), data, &crate::record_aad(id, kind))?;
            Version::Live(serde_json::from_slice(&plain)?)
        })
    }

    /// The record `id` as kept here.
    fn version_here(&self, id: &str) -> Result<Option<Version>> {
        for row in self.raw_rows()? {
            if row.id == id {
                let v = self.row_version(&row.id, &row.kind, &row.data, row.hlc, row.deleted)?;
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    /// Keeps a merged record here and in the store. Gives back the notes'
    /// texts lost to another device's version written meanwhile.
    fn settle(
        &self,
        b: &dyn Backend,
        p: Pending,
        kinds: &BTreeMap<String, String>,
        used: &mut BTreeSet<String>,
        report: &mut SyncReport,
    ) -> Result<Vec<(String, Record)>> {
        let Pending {
            id,
            here,
            there,
            merged,
        } = p;
        let mut lost = Vec::new();
        if let Version::Live(r) = &merged {
            used.extend(r.attachment_refs());
        }
        let kind = match &merged {
            Version::Live(r) => clear_kind(&r.kind).to_string(),
            Version::Gone(_) => kinds.get(&id).cloned().unwrap_or_else(|| "other".into()),
        };
        if !here.as_ref().is_some_and(|h| h.same(&merged)) {
            self.store_version(&id, &kind, &merged)?;
            report.pulled += 1;
        }
        let canonical = format!("{id}.sw");
        let up_to_date = there.len() == 1 && there[0].0 == canonical && there[0].1.same(&merged);
        if !up_to_date {
            let path = format!("{RECORDS}/{canonical}");
            let mut merged = merged;
            for _ in 0..REPLACE_TRIES {
                if b.replace(&path, &self.envelope(&id, &kind, &merged)?)? {
                    break;
                }
                // Another device wrote it since the listing: theirs
                // goes in, and the write is tried again.
                if let Some(bytes) = b.read(&path)?
                    && let Some((_, theirs)) = self.version_in(&id, &bytes)?
                {
                    let (m, text) = merge(merged, theirs);
                    merged = m;
                    lost.extend(text.map(|r| (id.clone(), r)));
                    self.store_version(&id, &kind, &merged)?;
                    report.pulled += 1;
                }
            }
            for (name, _) in &there {
                if *name != canonical {
                    b.remove(&format!("{RECORDS}/{name}"))?;
                }
            }
            report.pushed += 1;
        }
        Ok(lost)
    }

    /// Keeps each text `lost` as a note of its own, to merge and write with
    /// the others. One already there (another device made it, or it was
    /// deleted since) is merged with, so it is made once.
    fn keep_copies(
        &self,
        lost: Vec<(String, Record)>,
        held: &BTreeSet<String>,
        queue: &mut VecDeque<Pending>,
        report: &mut SyncReport,
    ) -> Result<()> {
        for (note, text) in lost {
            let Some((id, copy)) = copy_of(&note, &text) else {
                continue;
            };
            if held.contains(&id) {
                continue;
            }
            let copy = Version::Live(copy);
            if let Some(p) = queue.iter_mut().find(|p| p.id == id) {
                p.merged = merge(p.merged.clone(), copy).0;
                continue;
            }
            let here = self.version_here(&id)?;
            let merged = match &here {
                Some(h) => merge(h.clone(), copy).0,
                None => copy,
            };
            if here.as_ref().is_some_and(|h| h.same(&merged)) {
                continue;
            }
            if here.is_none()
                && let Version::Live(r) = &merged
            {
                report.copies.push(r.name.clone());
            }
            queue.push_back(Pending {
                id,
                here,
                there: Vec::new(),
                merged,
            });
        }
        Ok(())
    }

    /// The record `id` in a synced file's `bytes`, with its clear kind; None
    /// for a file that is not one of this space's records.
    fn version_in(&self, id: &str, bytes: &[u8]) -> Result<Option<(String, Version)>> {
        let env: Envelope = serde_json::from_slice(bytes)?;
        if env.id != id || env.space != SPACE || !matches!(env.kind.as_str(), "host" | "other") {
            return Ok(None);
        }
        let version = match env.data {
            Some(d) => {
                let sealed = B64.decode(d).map_err(|_| Error::Decrypt)?;
                let (plain, _) =
                    cipher::open(&self.sk(), &sealed, &crate::record_aad(id, &env.kind))?;
                Version::Live(serde_json::from_slice(&plain)?)
            }
            None => Version::Gone(env.hlc.clone()),
        };
        self.clock.observe(version.newest());
        Ok(Some((env.kind, version)))
    }

    fn envelope(&self, id: &str, kind: &str, v: &Version) -> Result<Vec<u8>> {
        let data = match v {
            Version::Live(r) => {
                let plain = Zeroizing::new(serde_json::to_vec(r)?);
                Some(B64.encode(cipher::seal(
                    self.cipher,
                    &self.sk(),
                    &plain,
                    &crate::record_aad(id, kind),
                )?))
            }
            Version::Gone(_) => None,
        };
        Ok(serde_json::to_vec(&Envelope {
            id: id.into(),
            kind: kind.into(),
            space: SPACE.into(),
            key: KEY_VERSION,
            hlc: v.newest().into(),
            data,
        })?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryKeys;

    fn field(val: &str, t: &str) -> Field {
        Field {
            val: val.into(),
            t: t.into(),
            ..Default::default()
        }
    }

    fn rec(fields: &[(&str, &str, &str)]) -> Version {
        Version::Live(Record {
            v: 1,
            kind: "password".into(),
            name: "n".into(),
            f: fields
                .iter()
                .map(|(k, v, t)| (k.to_string(), field(v, t)))
                .collect(),
        })
    }

    fn vals(v: &Version) -> Vec<(String, String)> {
        match v {
            Version::Live(r) => {
                r.f.iter()
                    .map(|(k, f)| (k.clone(), f.val.clone()))
                    .collect()
            }
            Version::Gone(t) => vec![("gone".into(), t.clone())],
        }
    }

    #[test]
    fn merging_keeps_each_fields_newest_in_any_order() {
        let a = rec(&[("user", "deploy", "1-0-a"), ("port", "22", "5-0-a")]);
        let b = rec(&[("user", "root", "3-0-b"), ("port", "2222", "2-0-b")]);
        let ab = merge(a.clone(), b.clone()).0;
        let ba = merge(b, a).0;
        assert_eq!(vals(&ab), vals(&ba));
        assert_eq!(
            vals(&ab),
            [("port".into(), "22".into()), ("user".into(), "root".into())]
        );
    }

    #[test]
    fn deletion_wins_over_older_edits_but_not_newer_ones() {
        let r = rec(&[("password", "x", "3-0-a")]);
        assert!(matches!(
            merge(r.clone(), Version::Gone("4-0-b".into())).0,
            Version::Gone(_)
        ));
        assert!(matches!(
            merge(Version::Gone("2-0-b".into()), r).0,
            Version::Live(_)
        ));
    }

    #[test]
    fn conflict_copies_count_as_the_same_record() {
        assert_eq!(
            file_id("0189abcdef0123456789abcd.sw"),
            Some("0189abcdef0123456789abcd")
        );
        assert_eq!(
            file_id("0189abcdef0123456789abcd (conflicted copy).sw"),
            Some("0189abcdef0123456789abcd")
        );
        assert_eq!(file_id("notes.txt"), None);
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-sync-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_held_record_waits_until_let_go() {
        let dir = temp("held");
        let folder = Folder::new(&dir.join("shared"));
        let (keys_a, keys_b) = (MemoryKeys::default(), MemoryKeys::default());
        let a = Vault::open(&dir.join("a.db"), &keys_a).expect("a");
        let b = Vault::open(&dir.join("b.db"), &keys_b).expect("b");
        let rk = a.start_sync(&folder).expect("start").to_string();
        b.join(&folder, &rk).expect("join");
        let mut note = crate::Note {
            id: String::new(),
            title: "Runbook".into(),
            category: String::new(),
            format: crate::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        note.id = a.save_note(&note, Some("v1")).expect("save");
        a.sync(&folder).expect("sync a");
        b.sync(&folder).expect("sync b");
        let text = |v: &Vault| v.note_text(&note.id).expect("read").map(|t| t.to_string());
        assert_eq!(text(&b).as_deref(), Some("v1"));

        // Open in its window on A: edited, but held from syncs.
        a.save_note(&note, Some("v2")).expect("edit");
        let held: BTreeSet<String> = [note.id.clone()].into();
        a.sync_holding(&folder, &held).expect("sync a, held");
        b.sync(&folder).expect("sync b");
        assert_eq!(text(&b).as_deref(), Some("v1"), "not out while held");
        // Its window closed: let go, it goes.
        a.sync(&folder).expect("sync a");
        b.sync(&folder).expect("sync b");
        assert_eq!(text(&b).as_deref(), Some("v2"));
    }

    #[test]
    fn two_devices_share_secrets_through_a_folder() {
        let dir = temp("two");
        let folder = Folder::new(&dir.join("shared"));
        let (keys_a, keys_b) = (MemoryKeys::default(), MemoryKeys::default());
        let a = Vault::open(&dir.join("a.db"), &keys_a).expect("a");
        let b = Vault::open(&dir.join("b.db"), &keys_b).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        b.set_password("root@db", "from-b").expect("save");

        // A starts; B sees another vault and joins with the recovery key.
        let rk = a.start_sync(&folder).expect("start").to_string();
        a.sync(&folder).expect("sync a");
        assert_eq!(b.folder_state(&folder).expect("state"), FolderState::Other);
        assert!(matches!(
            b.join(&folder, "0000-0000"),
            Err(Error::WrongRecoveryKey)
        ));
        b.join(&folder, &rk).expect("join");
        assert_eq!(b.folder_state(&folder).expect("state"), FolderState::Ours);
        b.sync(&folder).expect("sync b");
        a.sync(&folder).expect("sync a again");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(pw(&a, "root@db").as_deref(), Some("from-b"));
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("from-a"));

        // An edit and a deletion travel too.
        b.set_password("deploy@web", "changed-on-b").expect("edit");
        let gone = a
            .items()
            .expect("items")
            .into_iter()
            .find_map(|i| match i {
                crate::Item::Password { id, target } if target == "root@db" => Some(id),
                _ => None,
            })
            .expect("id");
        a.delete(&gone).expect("delete");
        b.sync(&folder).expect("sync");
        a.sync(&folder).expect("sync");
        b.sync(&folder).expect("sync");
        assert_eq!(pw(&a, "deploy@web").as_deref(), Some("changed-on-b"));
        assert_eq!(pw(&b, "root@db"), None);

        // Nothing readable in the folder.
        for entry in std::fs::read_dir(dir.join("shared/shellwing/records")).expect("dir") {
            let raw = std::fs::read(entry.expect("entry").path()).expect("read");
            let has = |s: &str| raw.windows(s.len()).any(|w| w == s.as_bytes());
            assert!(!has("changed-on-b") && !has("deploy@web"));
        }
        // A second sync with nothing new writes nothing.
        assert_eq!(a.sync(&folder).expect("idle"), SyncReport::default());
    }

    #[test]
    fn a_strong_passphrase_joins_a_new_device() {
        let dir = temp("passphrase");
        let folder = Folder::new(&dir.join("shared"));
        let (keys_a, keys_b) = (MemoryKeys::default(), MemoryKeys::default());
        let a = Vault::open(&dir.join("a.db"), &keys_a).expect("a");
        let b = Vault::open(&dir.join("b.db"), &keys_b).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        assert!(matches!(
            a.let_passphrase_join(&folder, Some("correct horse battery")),
            Err(Error::NotSynced)
        ));
        let rk = a.start_sync(&folder).expect("start").to_string();
        a.sync(&folder).expect("sync a");
        assert!(!b.passphrase_joins(&folder).expect("read"));

        for weak in ["password123456", "qwertyuiop", "shellwing"] {
            assert!(
                matches!(
                    a.let_passphrase_join(&folder, Some(weak)),
                    Err(Error::GuessablePassphrase(_))
                ),
                "{weak}"
            );
        }
        let strong = "tangerine copper lantern 49";
        assert!(crate::passphrase_strength(strong) >= 3);
        a.let_passphrase_join(&folder, Some(strong)).expect("let");
        assert!(b.passphrase_joins(&folder).expect("read"));
        let format =
            std::fs::read_to_string(dir.join("shared/shellwing/format.json")).expect("format");
        assert!(!format.contains("tangerine"), "{format}");

        assert!(matches!(
            b.join_with_passphrase(&folder, "tangerine copper lantern 48"),
            Err(Error::WrongPassphrase)
        ));
        b.join_with_passphrase(&folder, strong).expect("join");
        assert_eq!(b.folder_state(&folder).expect("state"), FolderState::Ours);
        assert_eq!(
            b.recovery_key().expect("rk").map(|k| k.to_string()),
            Some(rk),
            "the same recovery key on both"
        );
        b.sync(&folder).expect("sync b");
        assert_eq!(
            b.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("from-a")
        );

        // Stopping takes it out of the folder; a sync keeps it out.
        a.let_passphrase_join(&folder, None).expect("stop");
        a.sync(&folder).expect("sync");
        assert!(!b.passphrase_joins(&folder).expect("read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder where another device writes between this one's listing and
    /// its write, as a server's conditional write would report.
    struct Racing<'a> {
        inner: Folder,
        meanwhile: std::sync::Mutex<Option<Box<dyn FnOnce() + Send + 'a>>>,
    }

    impl Backend for Racing<'_> {
        fn list(&self, dir: &str) -> io::Result<Vec<String>> {
            self.inner.list(dir)
        }
        fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
            self.inner.read(name)
        }
        fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
            self.inner.write(name, data)
        }
        fn remove(&self, name: &str) -> io::Result<()> {
            self.inner.remove(name)
        }
        fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
            let other = name
                .starts_with("records/")
                .then(|| self.meanwhile.lock().expect("lock").take())
                .flatten();
            match other {
                Some(write_theirs) => {
                    write_theirs();
                    Ok(false)
                }
                None => self.inner.replace(name, data),
            }
        }
    }

    #[test]
    fn a_record_written_meanwhile_is_taken_in_before_writing_again() {
        let dir = temp("racing");
        let folder = Folder::new(&dir.join("shared"));
        let (keys_a, keys_b) = (MemoryKeys::default(), MemoryKeys::default());
        let a = Vault::open(&dir.join("a.db"), &keys_a).expect("a");
        let b = Vault::open(&dir.join("b.db"), &keys_b).expect("b");
        a.set_password("deploy@web", "first").expect("save");
        let rk = a.start_sync(&folder).expect("start").to_string();
        a.sync(&folder).expect("sync a");
        b.join(&folder, &rk).expect("join");
        b.sync(&folder).expect("sync b");

        // A changes it; B changes it later, and writes it while A syncs.
        a.set_password("deploy@web", "from-a").expect("edit a");
        // Later by the clock: B's edit is the newer.
        std::thread::sleep(std::time::Duration::from_millis(5));
        b.set_password("deploy@web", "from-b").expect("edit b");
        let racing = Racing {
            inner: Folder::new(&dir.join("shared")),
            meanwhile: std::sync::Mutex::new(Some(Box::new(|| {
                b.sync(&Folder::new(&dir.join("shared")))
                    .expect("b meanwhile");
            }))),
        };
        let report = a.sync(&racing).expect("sync a");
        assert!(report.pulled >= 1, "B's write was taken in: {report:?}");
        let pw = |v: &Vault| {
            v.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
        };
        assert_eq!(pw(&a).as_deref(), Some("from-b"), "the newer edit, at once");
        b.sync(&folder).expect("sync b");
        assert_eq!(pw(&b).as_deref(), Some("from-b"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Records the names written, in order.
    struct Recording {
        inner: Folder,
        written: std::sync::Mutex<Vec<String>>,
    }

    impl Backend for Recording {
        fn list(&self, dir: &str) -> io::Result<Vec<String>> {
            self.inner.list(dir)
        }
        fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
            self.inner.read(name)
        }
        fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
            self.written.lock().expect("lock").push(name.to_string());
            self.inner.write(name, data)
        }
        fn remove(&self, name: &str) -> io::Result<()> {
            self.inner.remove(name)
        }
        fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
            self.written.lock().expect("lock").push(name.to_string());
            self.inner.replace(name, data)
        }
    }

    /// A small PNG: its signature, then bytes enough to stand for one.
    fn png(n: u8) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend(std::iter::repeat_n(n, 200));
        b
    }

    /// A Markdown note showing `images`, saved in `v`: its id.
    fn note_showing(v: &Vault, images: &[&str]) -> String {
        let text: Vec<String> = images.iter().map(|id| crate::attachment_line(id)).collect();
        let note = crate::Note {
            id: String::new(),
            title: "Disk usage".into(),
            category: String::new(),
            format: crate::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        v.save_note(&note, Some(&text.join("\n"))).expect("note")
    }

    /// Two devices of one vault, syncing through a folder in `dir`.
    fn two_devices(dir: &Path) -> (Folder, Vault, Vault) {
        let folder = Folder::new(&dir.join("shared"));
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        let rk = a.start_sync(&folder).expect("start").to_string();
        b.join(&folder, &rk).expect("join");
        (folder, a, b)
    }

    #[test]
    fn an_image_goes_with_its_note_and_before_it() {
        let dir = temp("image");
        let (folder, a, b) = two_devices(&dir);
        let id = a.add_attachment(&png(1)).expect("image");
        let note = note_showing(&a, &[&id]);
        let recording = Recording {
            inner: Folder::new(&dir.join("shared")),
            written: Default::default(),
        };
        let report = a.sync(&recording).expect("sync a");
        assert_eq!(report.images_pushed, 1);
        let written = recording.written.lock().expect("lock").clone();
        let image_at = written
            .iter()
            .position(|n| *n == format!("attachments/{id}.sw"));
        let note_at = written
            .iter()
            .position(|n| *n == format!("records/{note}.sw"));
        assert!(
            image_at.is_some() && image_at < note_at,
            "the image goes first: {written:?}"
        );

        let report = b.sync(&folder).expect("sync b");
        assert_eq!((report.images_pulled, report.images_missing), (1, 0));
        assert_eq!(
            b.attachment(&id).expect("read").map(|i| i.to_vec()),
            Some(png(1))
        );
        // Nothing new: nothing sent or fetched again.
        assert_eq!(a.sync(&folder).expect("again"), SyncReport::default());
        assert_eq!(b.sync(&folder).expect("again"), SyncReport::default());
    }

    #[test]
    fn a_missing_image_is_put_back() {
        let dir = temp("missing");
        let (folder, a, b) = two_devices(&dir);
        let id = a.add_attachment(&png(2)).expect("image");
        note_showing(&a, &[&id]);
        a.sync(&folder).expect("sync a");
        // Lost from the store (a sync tool's mistake, a cleanup by hand).
        std::fs::remove_file(dir.join(format!("shared/shellwing/attachments/{id}.sw")))
            .expect("remove");
        let report = b.sync(&folder).expect("sync b");
        assert_eq!(report.images_missing, 1, "the note came; its image not");
        assert!(b.attachment(&id).expect("read").is_none());
        assert_eq!(a.sync(&folder).expect("sync a").images_pushed, 1);
        assert_eq!(b.sync(&folder).expect("sync b").images_pulled, 1);
        assert!(b.attachment(&id).expect("read").is_some());
    }

    #[test]
    fn a_damaged_image_is_not_taken() {
        let dir = temp("damaged");
        let (folder, a, b) = two_devices(&dir);
        let id = a.add_attachment(&png(3)).expect("image");
        note_showing(&a, &[&id]);
        a.sync(&folder).expect("sync a");
        let path = dir.join(format!("shared/shellwing/attachments/{id}.sw"));
        let mut data = std::fs::read(&path).expect("read");
        let last = data.len() - 1;
        data[last] ^= 1;
        std::fs::write(&path, data).expect("write");
        assert_eq!(b.sync(&folder).expect("sync b").images_missing, 1);
        assert!(b.attachment(&id).expect("read").is_none());
    }

    /// Every file under `dir`, nested ones too.
    fn files_under(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("list") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                found.extend(files_under(&path));
            } else {
                found.push(path);
            }
        }
        found
    }

    #[test]
    fn nothing_of_an_image_is_in_the_clear() {
        let dir = temp("clear");
        let (folder, a, b) = two_devices(&dir);
        // A PNG holding a text chunk, then bytes no file holds by chance.
        let mut image = b"\x89PNG\r\n\x1a\n\0\0\0\x1btEXtComment\0".to_vec();
        image.extend_from_slice(b"shellwing-plaintext-canary");
        image.extend((0..200u32).map(|i| (i * 37 + 11) as u8));
        let id = a.add_attachment(&image).expect("image");
        note_showing(&a, &[&id]);
        a.sync(&folder).expect("sync a");
        assert_eq!(b.sync(&folder).expect("sync b").images_pulled, 1);
        assert_eq!(
            b.attachment(&id).expect("read").map(|i| i.to_vec()),
            Some(image.clone())
        );

        // The store, and each device's vault.db (and its journal).
        let files = files_under(&dir);
        assert!(
            files
                .iter()
                .any(|f| f.ends_with(format!("attachments/{id}.sw"))),
            "the image is in the store: {files:?}"
        );
        for file in &files {
            let data = std::fs::read(file).expect("read");
            for piece in image.windows(16) {
                assert!(
                    !data.windows(16).any(|w| w == piece),
                    "{} holds the image's bytes {piece:02x?}",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn joining_another_vault_keeps_the_images() {
        let dir = temp("join-images");
        let folder = Folder::new(&dir.join("shared"));
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        // A had notes with images before it joined B's vault.
        let id = a.add_attachment(&png(4)).expect("image");
        note_showing(&a, &[&id]);
        let rk = b.start_sync(&folder).expect("start").to_string();
        a.join(&folder, &rk).expect("join");
        assert_eq!(
            a.attachment(&id).expect("read").map(|i| i.to_vec()),
            Some(png(4)),
            "sealed again with the new key"
        );
        a.sync(&folder).expect("sync a");
        b.sync(&folder).expect("sync b");
        assert_eq!(
            b.attachment(&id).expect("read").map(|i| i.to_vec()),
            Some(png(4))
        );
    }

    #[test]
    fn an_image_no_note_shows_for_long_goes_from_the_store() {
        let dir = temp("unused-image");
        let (folder, a, _b) = two_devices(&dir);
        let id = a.add_attachment(&png(5)).expect("image");
        let note = note_showing(&a, &[&id]);
        a.sync(&folder).expect("sync a");
        let stored = dir.join(format!("shared/shellwing/attachments/{id}.sw"));
        assert!(stored.exists());
        a.delete(&note).expect("delete the note");
        a.sync(&folder).expect("sync a");
        assert!(stored.exists(), "kept for a while, for an undo");
        // As if that were 31 days ago.
        a.db.lock()
            .expect("db")
            .execute(
                "UPDATE attachments SET unused_since = 1 WHERE id = ?1",
                rusqlite::params![id],
            )
            .expect("age");
        a.sync(&folder).expect("sync a");
        assert!(!stored.exists());
        assert!(a.attachment(&id).expect("read").is_none());
    }

    /// A note's version whose text `val` was written at `t` on "laptop",
    /// after seeing the edits `seen` (stamps).
    fn note_v(val: &str, t: &str, seen: &[&str]) -> Version {
        let mut text = field(val, t);
        text.by = "laptop".into();
        for s in seen.iter().chain([&t]) {
            let device = stamp_device(s).expect("stamp");
            text.seen.insert(device.into(), s.to_string());
        }
        note_with(text)
    }

    /// A note's version from before `seen`.
    fn note_before_seen(val: &str, t: &str) -> Version {
        note_with(field(val, t))
    }

    fn note_with(text: Field) -> Version {
        let t = text.t.clone();
        Version::Live(Record {
            v: 1,
            kind: "note".into(),
            name: "Disk usage".into(),
            f: BTreeMap::from([
                ("title".to_string(), field("Disk usage", &t)),
                ("text".to_string(), text),
            ]),
        })
    }

    fn text_of(v: &Version) -> String {
        match v {
            Version::Live(r) => r.f["text"].val.clone(),
            Version::Gone(_) => "gone".into(),
        }
    }

    fn lost_text(lost: &Option<Record>) -> Option<String> {
        lost.as_ref().map(|r| r.f["text"].val.clone())
    }

    #[test]
    fn an_edit_made_after_another_makes_no_copy() {
        let first = note_v("df -h", "1-0-a", &[]);
        let then = note_v("df -h /var", "2-0-b", &["1-0-a"]);
        for (x, y) in [(first.clone(), then.clone()), (then.clone(), first)] {
            let (merged, lost) = merge(x, y);
            assert_eq!(text_of(&merged), "df -h /var");
            assert!(lost.is_none());
        }
        // A's next edit, made from B's: still one line of edits.
        let again = note_v("du -sh", "3-0-a", &["1-0-a", "2-0-b"]);
        assert!(merge(then, again).1.is_none());
    }

    #[test]
    fn edits_made_without_seeing_each_other_keep_both() {
        let a = note_v("df -h /var", "2-0-a", &["1-0-a"]);
        let b = note_v("du -sh /var/log", "3-0-b", &["1-0-a"]);
        let (ab, lost_ab) = merge(a.clone(), b.clone());
        let (ba, lost_ba) = merge(b.clone(), a.clone());
        assert_eq!(text_of(&ab), "du -sh /var/log", "the later stays the note");
        assert_eq!(lost_text(&lost_ab).as_deref(), Some("df -h /var"));
        assert_eq!(lost_text(&lost_ba), lost_text(&lost_ab));
        assert!(ab.same(&ba));
        // The result has seen both: merging either again finds nothing.
        assert!(merge(ab.clone(), a).1.is_none());
        assert!(merge(b, ab.clone()).1.is_none());
        assert!(merge(ab.clone(), ab).1.is_none());
    }

    #[test]
    fn the_same_text_written_on_two_devices_makes_no_copy() {
        let a = note_v("df -h", "2-0-a", &["1-0-a"]);
        let b = note_v("df -h", "3-0-b", &["1-0-a"]);
        assert!(merge(a, b).1.is_none());
    }

    #[test]
    fn three_concurrent_edits_keep_the_latest_and_two_copies_in_any_order() {
        let a = note_v("a", "2-0-a", &["1-0-a"]);
        let b = note_v("b", "3-0-b", &["1-0-a"]);
        let c = note_v("c", "4-0-c", &["1-0-a"]);
        for [x, y, z] in [[&a, &b, &c], [&c, &a, &b], [&b, &c, &a], [&c, &b, &a]] {
            let (merged, first) = merge(x.clone(), y.clone());
            let (merged, second) = merge(merged, z.clone());
            assert_eq!(text_of(&merged), "c");
            let mut lost: Vec<String> = [first, second].iter().filter_map(lost_text).collect();
            lost.sort();
            assert_eq!(lost, ["a", "b"]);
        }
    }

    #[test]
    fn texts_from_before_seen_are_never_lost() {
        // A device's later edit has seen its earlier one.
        let (x, y) = (
            note_before_seen("x", "1-0-a"),
            note_before_seen("y", "2-0-a"),
        );
        assert!(merge(x, y).1.is_none());
        // Two devices': nothing tells, so both stay.
        let (x, y) = (
            note_before_seen("x", "1-0-a"),
            note_before_seen("y", "2-0-b"),
        );
        assert_eq!(lost_text(&merge(x, y).1).as_deref(), Some("x"));
        // An edit made from one of them has seen it.
        let x = note_before_seen("x", "1-0-a");
        assert!(merge(x, note_v("y", "2-0-b", &["1-0-a"])).1.is_none());
    }

    #[test]
    fn a_lost_text_is_kept_the_same_way_on_every_device() {
        let Version::Live(lost) = note_v("df -h /var", "2-0-a", &["1-0-a"]) else {
            unreachable!()
        };
        let (id, copy) = copy_of("0190aa", &lost).expect("copy");
        let (again, copy_again) = copy_of("0190aa", &lost).expect("copy");
        assert_eq!(id, again);
        assert!(Version::Live(copy.clone()).same(&Version::Live(copy_again)));
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(copy_of("0190bb", &lost).expect("copy").0, id);
        assert_eq!(copy.name, "Disk usage (from laptop)");
        assert_eq!(copy.f["title"].val, "Disk usage (from laptop)");
        assert_eq!(copy.f["text"].val, "df -h /var");
        assert!(copy.f.values().all(|f| f.t == "2-0-a"));
        // Deleted since: it stays deleted.
        let deleted = Version::Gone("9-0-c".into());
        assert!(matches!(
            merge(deleted, Version::Live(copy)).0,
            Version::Gone(_)
        ));
    }

    /// Note `id` (new when empty), "Disk usage", holding `text`: its id.
    fn write_note(v: &Vault, id: &str, text: &str) -> String {
        let note = crate::Note {
            id: id.into(),
            title: "Disk usage".into(),
            category: String::new(),
            format: crate::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        v.save_note(&note, Some(text)).expect("note")
    }

    /// Each note's title and text, in order.
    fn notes_of(v: &Vault) -> Vec<(String, String)> {
        let mut notes: Vec<(String, String)> = v
            .notes()
            .expect("notes")
            .into_iter()
            .map(|n| {
                let text = v.note_text(&n.id).expect("text").expect("there");
                (n.title, text.to_string())
            })
            .collect();
        notes.sort();
        notes
    }

    /// Two devices that synced note "Disk usage", then changed its text
    /// before seeing each other's change, B later: the note's id.
    fn changed_on_both(folder: &Folder, a: &Vault, b: &Vault) -> String {
        a.set_device_name("desk");
        b.set_device_name("laptop");
        let note = write_note(a, "", "df -h");
        a.sync(folder).expect("sync a");
        b.sync(folder).expect("sync b");
        write_note(a, &note, "df -h /var");
        std::thread::sleep(std::time::Duration::from_millis(5));
        write_note(b, &note, "du -sh /var/log");
        note
    }

    fn both_kept() -> Vec<(String, String)> {
        vec![
            ("Disk usage".into(), "du -sh /var/log".into()),
            ("Disk usage (from desk)".into(), "df -h /var".into()),
        ]
    }

    #[test]
    fn a_note_changed_on_two_devices_keeps_both_texts() {
        let dir = temp("both-texts");
        let (folder, a, b) = two_devices(&dir);
        changed_on_both(&folder, &a, &b);
        assert!(a.sync(&folder).expect("sync a").copies.is_empty());
        let report = b.sync(&folder).expect("sync b");
        assert_eq!(report.copies, ["Disk usage (from desk)"]);
        assert!(
            a.sync(&folder).expect("sync a").copies.is_empty(),
            "made once"
        );
        assert_eq!(notes_of(&a), both_kept());
        assert_eq!(notes_of(&b), both_kept());
        // Nothing new.
        assert_eq!(a.sync(&folder).expect("again"), SyncReport::default());
        assert_eq!(b.sync(&folder).expect("again"), SyncReport::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Copies folder `from` to `to`, as a sync tool's copy on another device.
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("dir");
        for entry in std::fs::read_dir(from).expect("list") {
            let path = entry.expect("entry").path();
            let dest = to.join(path.file_name().expect("name"));
            if path.is_dir() {
                copy_dir(&path, &dest);
            } else {
                std::fs::copy(&path, &dest).expect("copy");
            }
        }
    }

    /// Both devices sync at once, each with its own copy of the folder; the
    /// sync tool then puts B's file of the note beside A's, as a
    /// "conflicted copy". Neither device has found the conflict yet.
    fn both_write_at_once(dir: &Path, a: &Vault, b: &Vault, note: &str) {
        let shared = dir.join("shared");
        copy_dir(&shared, &dir.join("shared-b"));
        assert!(
            a.sync(&Folder::new(&shared))
                .expect("sync a")
                .copies
                .is_empty()
        );
        let report = b.sync(&Folder::new(&dir.join("shared-b"))).expect("sync b");
        assert!(report.copies.is_empty());
        std::fs::copy(
            dir.join(format!("shared-b/shellwing/records/{note}.sw")),
            shared.join(format!("shellwing/records/{note} (conflicted copy).sw")),
        )
        .expect("conflicted copy");
    }

    #[test]
    fn two_devices_writing_at_once_through_a_sync_tool_make_one_copy() {
        let dir = temp("at-once");
        let (folder, a, b) = two_devices(&dir);
        let note = changed_on_both(&folder, &a, &b);
        both_write_at_once(&dir, &a, &b, &note);
        let report = a.sync(&folder).expect("sync a");
        assert_eq!(report.copies, ["Disk usage (from desk)"]);
        assert!(
            b.sync(&folder).expect("sync b").copies.is_empty(),
            "made once"
        );
        assert_eq!(notes_of(&a), both_kept());
        assert_eq!(notes_of(&b), both_kept());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deleted_copy_stays_deleted_when_another_device_finds_the_loss() {
        let dir = temp("deleted-copy");
        let (folder, a, b) = two_devices(&dir);
        let note = changed_on_both(&folder, &a, &b);
        both_write_at_once(&dir, &a, &b, &note);
        // B's sync tool is behind: its folder still holds both files.
        copy_dir(&dir.join("shared"), &dir.join("shared-late"));
        a.sync(&folder).expect("sync a");
        let copy = a
            .notes()
            .expect("notes")
            .into_iter()
            .find(|n| n.title == "Disk usage (from desk)")
            .expect("the copy");
        a.delete(&copy.id).expect("delete the copy");
        a.sync(&folder).expect("sync a");

        let late = Folder::new(&dir.join("shared-late"));
        assert_eq!(b.sync(&late).expect("sync b").copies.len(), 1);
        b.sync(&folder).expect("sync b");
        let note_only = vec![("Disk usage".to_string(), "du -sh /var/log".to_string())];
        assert_eq!(notes_of(&b), note_only);
        a.sync(&folder).expect("sync a");
        assert_eq!(notes_of(&a), note_only);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_open_in_its_window_merges_when_let_go() {
        let dir = temp("held-note");
        let (folder, a, b) = two_devices(&dir);
        let note = changed_on_both(&folder, &a, &b);
        a.sync(&folder).expect("sync a");
        let held = BTreeSet::from([note.clone()]);
        let report = b.sync_holding(&folder, &held).expect("sync b");
        assert!(report.copies.is_empty());
        assert_eq!(
            b.note_text(&note).expect("text").map(|t| t.to_string()),
            Some("du -sh /var/log".into())
        );
        assert_eq!(b.sync(&folder).expect("sync b").copies.len(), 1);
        assert_eq!(notes_of(&b), both_kept());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_text_lost_to_a_write_made_meanwhile_is_kept() {
        let dir = temp("lost-meanwhile");
        let (folder, a, b) = two_devices(&dir);
        changed_on_both(&folder, &a, &b);
        let racing = Racing {
            inner: Folder::new(&dir.join("shared")),
            meanwhile: std::sync::Mutex::new(Some(Box::new(|| {
                b.sync(&Folder::new(&dir.join("shared")))
                    .expect("b meanwhile");
            }))),
        };
        let report = a.sync(&racing).expect("sync a");
        assert_eq!(report.copies, ["Disk usage (from desk)"]);
        assert_eq!(notes_of(&a), both_kept());
        assert!(b.sync(&folder).expect("sync b").copies.is_empty());
        assert_eq!(notes_of(&b), both_kept());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_images_of_a_lost_text_are_kept() {
        let dir = temp("lost-images");
        let (folder, a, b) = two_devices(&dir);
        let note = changed_on_both(&folder, &a, &b);
        let image = a.add_attachment(&png(7)).expect("image");
        write_note(&a, &note, &crate::attachment_line(&image));
        std::thread::sleep(std::time::Duration::from_millis(5));
        write_note(&b, &note, "du -sh /var/log");
        a.sync(&folder).expect("sync a");
        let report = b.sync(&folder).expect("sync b");
        assert_eq!(report.copies.len(), 1);
        assert_eq!(report.images_pulled, 1, "the copy shows it");
        a.sync(&folder).expect("sync a");
        // As if no note had shown it for 31 days: the copy still does.
        a.db.lock()
            .expect("db")
            .execute(
                "UPDATE attachments SET unused_since = 1 WHERE id = ?1",
                rusqlite::params![image],
            )
            .expect("age");
        a.sync(&folder).expect("sync a");
        assert!(a.attachment(&image).expect("read").is_some());
        let stored = dir.join(format!("shared/shellwing/attachments/{image}.sw"));
        assert!(stored.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Sync through any store of files (docs/design/sync.md).
//!
//! The store holds `shellwing/format.json` (which key the records use, and that
//! key wrapped by the recovery key) and `shellwing/records/<id>.sw`, one sealed
//! record per file. Merging is per field, by clock stamp, so it gives the same
//! result in any order and on stores without locking: a write lost to a race is
//! merged back at the next sync by the device that still has it.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::recovery::RecoveryKey;
use crate::{Error, Field, KEY_LEN, KEY_VERSION, KIND, Record, Result, SPACE, Vault, cipher};

const FORMAT: &str = "format.json";
const RECORDS: &str = "records";
const RECOVERY_AAD: &[u8] = b"shellwing space key wrapped by the recovery key v1";

/// Where synced files live. Names are relative: `format.json`,
/// `records/<file>`.
pub trait Backend: Send + Sync {
    /// File names in `records/`.
    fn list(&self) -> io::Result<Vec<String>>;
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>>;
    fn write(&self, name: &str, data: &[u8]) -> io::Result<()>;
    fn remove(&self, name: &str) -> io::Result<()>;
}

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
    fn list(&self) -> io::Result<Vec<String>> {
        let dir = self.root.join(RECORDS);
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
}

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
                    && a.f
                        .iter()
                        .zip(&b.f)
                        .all(|((ka, fa), (kb, fb))| ka == kb && fa.val == fb.val && fa.t == fb.t)
            }
            (Version::Gone(a), Version::Gone(b)) => a == b,
            _ => false,
        }
    }
}

/// Merges two versions of one record: each field keeps its newest value, and a
/// deletion wins over edits made before it (an edit after it brings it back).
pub(crate) fn merge(a: Version, b: Version) -> Version {
    match (a, b) {
        (Version::Gone(x), Version::Gone(y)) => Version::Gone(x.max(y)),
        (Version::Live(r), Version::Gone(t)) | (Version::Gone(t), Version::Live(r)) => {
            if t.as_str() > r.newest() {
                Version::Gone(t)
            } else {
                Version::Live(r)
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
            Version::Live(Record {
                v: a.v.max(b.v),
                kind: a.kind.clone(),
                name: newer_name,
                f,
            })
        }
    }
}

/// What a sync did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Records changed here from the store.
    pub pulled: usize,
    /// Records written to the store.
    pub pushed: usize,
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
        let rows = self.raw_rows()?;
        let mut db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let tx = db.transaction()?;
        for crate::RawRow {
            id, data, deleted, ..
        } in rows
        {
            if deleted {
                continue;
            }
            let (plain, _) = cipher::open(&self.sk(), &data, &crate::record_aad(&id))?;
            let resealed = cipher::seal(self.cipher, &new_key, &plain, &crate::record_aad(&id))?;
            tx.execute(
                "UPDATE records SET data = ?2 WHERE id = ?1",
                rusqlite::params![id, resealed],
            )?;
        }
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
        self.store_recovery_key(&rk)?;
        Ok(())
    }

    /// Merges this vault with the store, both ways.
    pub fn sync(&self, b: &dyn Backend) -> Result<SyncReport> {
        match self.folder_state(b)? {
            FolderState::Ours => {}
            FolderState::Empty => return Err(Error::NotSynced),
            FolderState::Other => return Err(Error::OtherVault),
        }
        // Remote versions of each record, with the files they came from.
        let mut remote: BTreeMap<String, Vec<(String, Version)>> = BTreeMap::new();
        for name in b.list()? {
            let Some(id) = file_id(&name).map(str::to_string) else {
                continue;
            };
            let Some(bytes) = b.read(&format!("{RECORDS}/{name}"))? else {
                continue;
            };
            let env: Envelope = serde_json::from_slice(&bytes)?;
            if env.id != id || env.space != SPACE {
                continue;
            }
            let version = match env.data {
                Some(d) => {
                    let sealed = B64.decode(d).map_err(|_| Error::Decrypt)?;
                    let (plain, _) = cipher::open(&self.sk(), &sealed, &crate::record_aad(&id))?;
                    Version::Live(serde_json::from_slice(&plain)?)
                }
                None => Version::Gone(env.hlc.clone()),
            };
            self.clock.observe(version.newest());
            remote.entry(id).or_default().push((name, version));
        }
        let mut local: BTreeMap<String, Version> = BTreeMap::new();
        for crate::RawRow {
            id,
            data,
            hlc,
            deleted,
        } in self.raw_rows()?
        {
            let v = if deleted {
                Version::Gone(hlc)
            } else {
                let (plain, _) = cipher::open(&self.sk(), &data, &crate::record_aad(&id))?;
                Version::Live(serde_json::from_slice(&plain)?)
            };
            local.insert(id, v);
        }
        let ids: BTreeSet<String> = remote.keys().chain(local.keys()).cloned().collect();
        let mut report = SyncReport::default();
        for id in ids {
            let here = local.remove(&id);
            let there = remote.remove(&id).unwrap_or_default();
            let mut merged = here.clone();
            for (_, v) in &there {
                merged = Some(match merged {
                    Some(m) => merge(m, v.clone()),
                    None => v.clone(),
                });
            }
            let Some(merged) = merged else { continue };
            if !here.as_ref().is_some_and(|h| h.same(&merged)) {
                self.store_version(&id, &merged)?;
                report.pulled += 1;
            }
            let canonical = format!("{id}.sw");
            let up_to_date =
                there.len() == 1 && there[0].0 == canonical && there[0].1.same(&merged);
            if !up_to_date {
                b.write(
                    &format!("{RECORDS}/{canonical}"),
                    &self.envelope(&id, &merged)?,
                )?;
                for (name, _) in &there {
                    if *name != canonical {
                        b.remove(&format!("{RECORDS}/{name}"))?;
                    }
                }
                report.pushed += 1;
            }
        }
        Ok(report)
    }

    fn envelope(&self, id: &str, v: &Version) -> Result<Vec<u8>> {
        let data = match v {
            Version::Live(r) => {
                let plain = Zeroizing::new(serde_json::to_vec(r)?);
                Some(B64.encode(cipher::seal(
                    self.cipher,
                    &self.sk(),
                    &plain,
                    &crate::record_aad(id),
                )?))
            }
            Version::Gone(_) => None,
        };
        Ok(serde_json::to_vec(&Envelope {
            id: id.into(),
            kind: KIND.into(),
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
        let ab = merge(a.clone(), b.clone());
        let ba = merge(b, a);
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
            merge(r.clone(), Version::Gone("4-0-b".into())),
            Version::Gone(_)
        ));
        assert!(matches!(
            merge(Version::Gone("2-0-b".into()), r),
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
}

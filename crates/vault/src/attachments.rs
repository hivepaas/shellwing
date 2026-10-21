//! Attachments: images kept for notes (`docs/design/note-images.md`). Each is
//! an immutable blob, sealed with the space key, under a random id that a
//! note's Markdown refers to as `attachment:<id>`. They sync apart from the
//! records: the engine lists them, never merges or reads them twice
//! (`sync.rs`).

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};
use zeroize::Zeroizing;

use crate::{Error, KEY_VERSION, Oversize, Result, SPACE, Vault, cipher, limits};

/// Where attachments are on a backend, beside `records`.
pub(crate) const ATTACHMENTS: &str = "attachments";
/// How a note's Markdown refers to one.
const SCHEME: &str = "attachment:";
/// Unused this long (no live note refers to it), an attachment goes.
pub const UNUSED_FOR_MS: u128 = 30 * 24 * 60 * 60 * 1000;

pub(crate) fn attachment_aad(id: &str) -> Vec<u8> {
    format!("attachment\0{id}\0{SPACE}\0{KEY_VERSION}").into_bytes()
}

/// The kind of image `bytes` hold: `image/png` or `image/jpeg`, the two kept.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Whether `id` is an attachment's id: what [`crate::new_id`] makes.
fn is_id(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The attachments `text` (a note's Markdown) refers to, in order, once each.
pub fn attachment_refs(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    let mut rest = text;
    while let Some(at) = rest.find(SCHEME) {
        rest = &rest[at + SCHEME.len()..];
        let id: String = rest
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect::<String>()
            .to_ascii_lowercase();
        if is_id(&id) && seen.insert(id.clone()) {
            out.push(id);
        }
    }
    out
}

/// A note's Markdown line showing attachment `id`.
pub fn attachment_line(id: &str) -> String {
    format!("![]({SCHEME}{id})")
}

/// An attachment kept here: its id, and since when no note refers to it.
pub(crate) struct StoredAttachment {
    pub id: String,
    pub unused_since: Option<u128>,
}

impl Vault {
    /// Keeps `bytes` (a prepared PNG or JPEG) as an attachment; its id.
    pub fn add_attachment(&self, bytes: &[u8]) -> Result<String> {
        if bytes.len() > limits::ATTACHMENT {
            return Err(Error::TooLarge(Oversize::Attachment(bytes.len())));
        }
        if image_mime(bytes).is_none() {
            return Err(Error::NotAnImage);
        }
        let id = crate::new_id()?;
        let data = cipher::seal(self.cipher, &self.sk(), bytes, &attachment_aad(&id))?;
        self.store_attachment(&id, &data, bytes.len())?;
        Ok(id)
    }

    /// The image attachment `id` holds, if it is here.
    pub fn attachment(&self, id: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let Some(data) = self.sealed_attachment(id)? else {
            return Ok(None);
        };
        let (plain, _) = cipher::open(&self.sk(), &data, &attachment_aad(id))?;
        Ok(Some(plain))
    }

    /// The bytes of the attachments among `ids` that are here, together:
    /// for a note's limit.
    pub fn attachments_size(&self, ids: &[String]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut total = 0usize;
        for id in ids {
            let bytes: Option<i64> = db
                .query_row(
                    "SELECT bytes FROM attachments WHERE id = ?1 AND space = ?2",
                    params![id, SPACE],
                    |r| r.get(0),
                )
                .optional()?;
            total += bytes.map_or(0, |b| usize::try_from(b).unwrap_or(usize::MAX));
        }
        Ok(total)
    }

    /// Stores attachment `id`, sealed already (made here, or from a backend
    /// and checked); one already here is kept as it is.
    pub(crate) fn store_attachment(&self, id: &str, data: &[u8], bytes: usize) -> Result<()> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT OR IGNORE INTO attachments (id, space, key_ver, data, bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                SPACE,
                KEY_VERSION,
                data,
                i64::try_from(bytes).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// Every attachment here (not their data: each round of sync asks).
    pub(crate) fn stored_attachments(&self) -> Result<Vec<StoredAttachment>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut q = db.prepare("SELECT id, unused_since FROM attachments WHERE space = ?1")?;
        let rows = q
            .query_map(params![SPACE], |r| {
                Ok(StoredAttachment {
                    id: r.get(0)?,
                    unused_since: r
                        .get::<_, Option<i64>>(1)?
                        .and_then(|t| u128::try_from(t).ok()),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Attachment `id` as kept here, sealed: what goes to a backend.
    pub(crate) fn sealed_attachment(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        Ok(db
            .query_row(
                "SELECT data FROM attachments WHERE id = ?1 AND space = ?2",
                params![id, SPACE],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Opens attachment `id` from a backend's `data`, checking it is what its
    /// id says (sealed for it, with this vault's key) and an image of the
    /// kept size: its plaintext size if so.
    pub(crate) fn check_attachment(&self, id: &str, data: &[u8]) -> Option<usize> {
        if data.len() > limits::ATTACHMENT + 64 {
            return None;
        }
        let (plain, _) = cipher::open(&self.sk(), data, &attachment_aad(id)).ok()?;
        (plain.len() <= limits::ATTACHMENT && image_mime(&plain).is_some()).then_some(plain.len())
    }

    /// Marks the attachments no live note refers to (`used` lists those that
    /// one does) as unused from `now`, and those used again as used. The ids
    /// unused for longer than [`UNUSED_FOR_MS`] are removed here and
    /// returned, to remove where they sync.
    pub(crate) fn tend_attachments(
        &self,
        used: &BTreeSet<String>,
        now: u128,
    ) -> Result<Vec<String>> {
        let stored = self.stored_attachments()?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let mut gone = vec![];
        for a in stored {
            let since = i64::try_from(now).unwrap_or(i64::MAX);
            match (used.contains(&a.id), a.unused_since) {
                (true, Some(_)) => {
                    db.execute(
                        "UPDATE attachments SET unused_since = NULL WHERE id = ?1",
                        params![a.id],
                    )?;
                }
                (false, None) => {
                    db.execute(
                        "UPDATE attachments SET unused_since = ?2 WHERE id = ?1",
                        params![a.id, since],
                    )?;
                }
                (false, Some(t)) if now.saturating_sub(t) > UNUSED_FOR_MS => {
                    db.execute("DELETE FROM attachments WHERE id = ?1", params![a.id])?;
                    gone.push(a.id);
                }
                _ => {}
            }
        }
        Ok(gone)
    }

    /// The attachments the live notes refer to.
    pub(crate) fn used_attachments(&self) -> Result<BTreeSet<String>> {
        let mut used = BTreeSet::new();
        for (_, r) in self.records()? {
            used.extend(r.attachment_refs());
        }
        Ok(used)
    }

    /// Looks after the attachments here when the vault does not sync: those
    /// unused for long are removed.
    pub fn sweep_attachments(&self) -> Result<()> {
        let used = self.used_attachments()?;
        self.tend_attachments(&used, crate::now_ms())?;
        Ok(())
    }

    /// Seals every attachment again with `new_key` (joining another vault):
    /// within `tx`, beside the records.
    pub(crate) fn reseal_attachments(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new_key: &[u8; crate::KEY_LEN],
    ) -> Result<()> {
        let rows: Vec<(String, Vec<u8>)> = {
            let mut q = tx.prepare("SELECT id, data FROM attachments WHERE space = ?1")?;
            q.query_map(params![SPACE], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?
        };
        for (id, data) in rows {
            let aad = attachment_aad(&id);
            let (plain, _) = cipher::open(&self.sk(), &data, &aad)?;
            let resealed = cipher::seal(self.cipher, new_key, &plain, &aad)?;
            tx.execute(
                "UPDATE attachments SET data = ?2 WHERE id = ?1",
                params![id, resealed],
            )?;
        }
        Ok(())
    }
}

impl crate::Record {
    /// The attachments a live note refers to; none for other records.
    pub(crate) fn attachment_refs(&self) -> Vec<String> {
        if self.kind != "note" {
            return vec![];
        }
        self.f
            .get("text")
            .map(|t| attachment_refs(&t.val))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryKeys, Note, NoteFormat};

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shellwing-attachments-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// A small PNG: its signature, then bytes enough to stand for one.
    fn png(n: u8) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend(std::iter::repeat_n(n, 100));
        b
    }

    pub(crate) fn note(text: &str) -> (Note, String) {
        (
            Note {
                id: String::new(),
                title: "with images".into(),
                category: String::new(),
                format: NoteFormat::Markdown,
                hosts: vec![],
                sensitive: false,
                changed: String::new(),
                size: 0,
            },
            text.to_string(),
        )
    }

    #[test]
    fn refs_are_found_once_each_in_order() {
        let a = "0123456789abcdef0123456789abcdef";
        let b = "0189e7f3a2b1c4d5e6f708192a3b4c5d";
        let text = format!(
            "# Disk\n![]({SCHEME}{a}){{width=600}}\nsee ![x]({SCHEME}{b}) and\n![]({SCHEME}{a})\n\
             not one: attachment:xyz, attachment:12ab"
        );
        assert_eq!(attachment_refs(&text), [a, b]);
        assert_eq!(attachment_line(a), format!("![](attachment:{a})"));
    }

    #[test]
    fn kept_sealed_and_given_back() {
        let dir = temp("kept");
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let id = v.add_attachment(&png(7)).expect("add");
        assert_eq!(
            v.attachment(&id).expect("read").map(|b| b.to_vec()),
            Some(png(7))
        );
        assert_eq!(
            v.attachments_size(&[id.clone(), "ffff".into()])
                .expect("size"),
            108
        );
        // Sealed: the bytes are not in the database as they are.
        let raw = std::fs::read(dir.join("vault.db")).expect("db");
        assert!(!raw.windows(png(7).len()).any(|w| w == png(7)));
    }

    #[test]
    fn only_images_of_the_kept_size() {
        let dir = temp("limits");
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        assert!(matches!(
            v.add_attachment(b"#!/bin/sh\nrm -rf /\n"),
            Err(Error::NotAnImage)
        ));
        let mut big = png(1);
        big.resize(limits::ATTACHMENT + 1, 0);
        assert!(matches!(
            v.add_attachment(&big),
            Err(Error::TooLarge(Oversize::Attachment(_)))
        ));
    }

    #[test]
    fn a_blob_under_another_id_does_not_open() {
        let dir = temp("swap");
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let a = v.add_attachment(&png(1)).expect("a");
        let sealed = v.sealed_attachment(&a).expect("read").expect("there");
        assert_eq!(v.check_attachment(&a, &sealed), Some(108));
        assert_eq!(
            v.check_attachment("0189e7f3a2b1c4d5e6f708192a3b4c5d", &sealed),
            None
        );
    }

    #[test]
    fn unused_ones_go_after_thirty_days() {
        let dir = temp("unused");
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("open");
        let kept = v.add_attachment(&png(1)).expect("kept");
        let dropped = v.add_attachment(&png(2)).expect("dropped");
        let (n, text) = note(&format!("![]({SCHEME}{kept})"));
        v.save_note(&n, Some(text.as_str())).expect("note");
        let used = v.used_attachments().expect("used");
        assert_eq!(used, BTreeSet::from([kept.clone()]));
        let day = 24 * 60 * 60 * 1000;
        assert!(
            v.tend_attachments(&used, 1_000 * day)
                .expect("tend")
                .is_empty()
        );
        assert!(
            v.tend_attachments(&used, 1_029 * day)
                .expect("tend")
                .is_empty()
        );
        // Back in a note before its time: kept, and counted from scratch.
        let both = BTreeSet::from([kept.clone(), dropped.clone()]);
        assert!(
            v.tend_attachments(&both, 1_029 * day)
                .expect("tend")
                .is_empty()
        );
        assert!(
            v.tend_attachments(&used, 1_040 * day)
                .expect("tend")
                .is_empty()
        );
        assert_eq!(
            v.tend_attachments(&used, 1_071 * day).expect("tend"),
            std::slice::from_ref(&dropped)
        );
        assert!(v.attachment(&dropped).expect("read").is_none());
        assert!(v.attachment(&kept).expect("read").is_some());
    }
}

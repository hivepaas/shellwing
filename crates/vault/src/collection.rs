//! A collection's key, wrapped for each member (docs/design/collections.md).
//!
//! The wrapper agrees a key with the member's X25519 key from an ephemeral
//! pair, derives with HKDF-SHA256, seals the collection key with
//! XChaCha20-Poly1305, commits to it, and signs all of it with their Ed25519
//! key. shellwing-server's `pkg/collectionkey` checks the signature over the
//! same bytes.

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use curve25519_dalek::montgomery::MontgomeryPoint;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use std::collections::BTreeMap;
use std::io;

use rusqlite::{OptionalExtension, params};

use crate::identity::{Identity, Key, PublicIdentity};
use crate::sync::Backend;
use crate::{
    Error, KEY_LEN, Result, Space, Vault, cipher, collection_key_aad, collection_key_id, random,
};

/// What the wrapper signs, first.
const SIGNED: &[u8] = b"shellwing collection key wrap v1";
/// HKDF's info, and the seal's additional data, first.
const INFO: &[u8] = b"shellwing collection key v1";
const COMMITMENT: &[u8] = b"shellwing collection key commitment v1";
const NONCE_LEN: usize = 24;
/// The sealed key: nonce, then the key and its 16-byte tag.
pub const SEALED_LEN: usize = NONCE_LEN + KEY_LEN + 16;

/// A collection's key wrapped for one member, as the server keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrap {
    pub recipient_x25519: Key,
    pub ephemeral_public: Key,
    pub sealed: Vec<u8>,
    pub commitment: Key,
    pub signature: [u8; 64],
}

/// The collection id and version, as both the signature and the derivation
/// bind them: a 2-byte big-endian length, the id, a 4-byte big-endian version.
fn bound(collection: &str, version: u32) -> Result<Vec<u8>> {
    let len = u16::try_from(collection.len())
        .map_err(|_| Error::Collection("the collection's id is too long".into()))?;
    let mut out = Vec::with_capacity(2 + collection.len() + 4);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(collection.as_bytes());
    out.extend_from_slice(&version.to_be_bytes());
    Ok(out)
}

impl Wrap {
    /// What the wrapper's Ed25519 key signs.
    fn signed(&self, collection: &str, version: u32) -> Result<Vec<u8>> {
        Ok([
            SIGNED,
            &bound(collection, version)?,
            &self.recipient_x25519,
            &self.ephemeral_public,
            &self.sealed,
            &self.commitment,
        ]
        .concat())
    }

    /// Whether `wrapper` signed it, for this collection and version.
    pub fn signed_by(&self, collection: &str, version: u32, wrapper: &Key) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(wrapper) else {
            return false;
        };
        let Ok(signed) = self.signed(collection, version) else {
            return false;
        };
        key.verify_strict(&signed, &Signature::from_bytes(&self.signature))
            .is_ok()
    }
}

/// `SHA-256("shellwing collection key commitment v1" ‖ key)`: which key a wrap
/// holds, so that a key that opens it is the one meant.
pub fn commitment(key: &Key) -> Key {
    Sha256::new()
        .chain_update(COMMITMENT)
        .chain_update(key)
        .finalize()
        .into()
}

/// The key that seals the collection key for one member.
fn wrapping_key(
    shared: &Key,
    ephemeral: &Key,
    recipient: &Key,
    info: &[u8],
) -> Result<Zeroizing<Key>> {
    let salt = [&ephemeral[..], &recipient[..]].concat();
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(info, &mut key[..])
        .map_err(|_| Error::Collection("cannot derive a key".into()))?;
    Ok(key)
}

fn agree(secret: &Key, theirs: &Key) -> Result<Zeroizing<Key>> {
    let shared = Zeroizing::new(MontgomeryPoint(*theirs).mul_clamped(*secret).to_bytes());
    // A low-order key gives everyone the same result.
    if *shared == [0; KEY_LEN] {
        return Err(Error::Collection("that member's key is not usable".into()));
    }
    Ok(shared)
}

impl Identity {
    /// Wraps a collection's key for `to`, whose identity was checked: its
    /// signature here, its fingerprint by a person.
    pub fn wrap(
        &self,
        key: &Key,
        collection: &str,
        version: u32,
        to: &PublicIdentity,
    ) -> Result<Wrap> {
        self.wrap_with(
            key,
            collection,
            version,
            to,
            &Zeroizing::new(random::<KEY_LEN>()?),
            &random::<NONCE_LEN>()?,
        )
    }

    /// [`Identity::wrap`] with the ephemeral secret and nonce given: for the
    /// test vector only.
    pub(crate) fn wrap_with(
        &self,
        key: &Key,
        collection: &str,
        version: u32,
        to: &PublicIdentity,
        ephemeral: &Key,
        nonce: &[u8; NONCE_LEN],
    ) -> Result<Wrap> {
        if !to.verify() {
            return Err(Error::Collection(
                "that member's identity is not signed".into(),
            ));
        }
        let ephemeral_public = MontgomeryPoint::mul_base_clamped(*ephemeral).to_bytes();
        let shared = agree(ephemeral, &to.x25519)?;
        let info = [INFO, &bound(collection, version)?].concat();
        let sealing = wrapping_key(&shared, &ephemeral_public, &to.x25519, &info)?;
        let ciphertext = XChaCha20Poly1305::new((&*sealing).into())
            .encrypt(
                nonce.into(),
                Payload {
                    msg: key,
                    aad: &info,
                },
            )
            .map_err(|_| Error::Collection("cannot seal the key".into()))?;
        let mut wrap = Wrap {
            recipient_x25519: to.x25519,
            ephemeral_public,
            sealed: [&nonce[..], &ciphertext].concat(),
            commitment: commitment(key),
            signature: [0; 64],
        };
        let signing = SigningKey::from_bytes(self.ed25519_secret());
        wrap.signature = signing.sign(&wrap.signed(collection, version)?).to_bytes();
        Ok(wrap)
    }

    /// The collection's key in a wrap made for this identity and signed by
    /// `wrapper` (an Ed25519 key), checked against its commitment.
    pub fn unwrap(
        &self,
        wrap: &Wrap,
        collection: &str,
        version: u32,
        wrapper: &Key,
    ) -> Result<Zeroizing<Key>> {
        let refused = |why: &str| Error::Collection(why.into());
        if wrap.recipient_x25519 != self.public().x25519 {
            return Err(refused("the key was wrapped for another identity"));
        }
        if !wrap.signed_by(collection, version, wrapper) {
            return Err(refused("the key's wrap is not signed by who gave it"));
        }
        if wrap.sealed.len() != SEALED_LEN {
            return Err(refused("the key's wrap has the wrong size"));
        }
        let shared = agree(self.x25519_secret(), &wrap.ephemeral_public)?;
        let info = [INFO, &bound(collection, version)?].concat();
        let sealing = wrapping_key(
            &shared,
            &wrap.ephemeral_public,
            &wrap.recipient_x25519,
            &info,
        )?;
        let (nonce, ciphertext) = wrap.sealed.split_at(NONCE_LEN);
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new((&*sealing).into())
                .decrypt(
                    nonce.into(),
                    Payload {
                        msg: ciphertext,
                        aad: &info,
                    },
                )
                .map_err(|_| refused("the key's wrap does not open"))?,
        );
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        if plain.len() != KEY_LEN {
            return Err(refused("the key's wrap holds no key"));
        }
        key.copy_from_slice(&plain);
        if commitment(&key) != wrap.commitment {
            return Err(refused("the key is not the one its wrap names"));
        }
        Ok(key)
    }
}

/// A new collection key.
pub fn new_key() -> Result<Zeroizing<Key>> {
    Ok(Zeroizing::new(random::<KEY_LEN>()?))
}

/// A wrap the server gave, with what it is of and who wrapped it.
pub struct Given {
    pub version: u32,
    pub wrap: Wrap,
    /// The Ed25519 key of the person who wrapped it, which signed it.
    pub wrapper: Key,
}

impl Vault {
    pub(crate) fn load_collection_keys(
        db: &rusqlite::Connection,
        account: &Key,
    ) -> Result<crate::CollectionKeys> {
        let mut q = db.prepare(
            "SELECT id, ver, wrapped FROM keys WHERE id LIKE 'col:%' AND wrapped_by = 'account'",
        )?;
        let rows: Vec<(String, i64, Vec<u8>)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut keys = BTreeMap::new();
        for (row, ver, wrapped) in rows {
            let collection = row.trim_start_matches("col:").to_string();
            let (plain, _) =
                cipher::open(account, &wrapped, &collection_key_aad(&collection, ver))?;
            let key: Key = plain[..].try_into().map_err(|_| Error::Decrypt)?;
            keys.insert((collection, ver), Zeroizing::new(key));
        }
        Ok(keys)
    }

    /// The versions of a collection's key this person holds, oldest first.
    pub fn collection_versions(&self, collection: &str) -> Vec<i64> {
        self.collection_keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .filter(|(c, _)| c == collection)
            .map(|(_, v)| *v)
            .collect()
    }

    /// Keeps a collection's key, sealed with the account key.
    fn hold(&self, collection: &str, version: i64, key: &Key) -> Result<()> {
        let wrapped = cipher::seal(
            self.cipher,
            &self.account,
            key,
            &collection_key_aad(collection, version),
        )?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO keys (id, ver, wrapped, wrapped_by) VALUES (?1, ?2, ?3, 'account')
             ON CONFLICT(id, ver, wrapped_by) DO UPDATE SET wrapped = excluded.wrapped",
            params![collection_key_id(collection), version, wrapped],
        )?;
        drop(db);
        self.collection_keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert((collection.to_string(), version), Zeroizing::new(*key));
        Ok(())
    }

    /// The person's identity, which collection keys are wrapped for.
    fn identity_for_keys(&self) -> Result<Identity> {
        self.identity()?.ok_or_else(|| {
            Error::Collection("this vault has no identity yet: it makes one when it syncs".into())
        })
    }

    /// Opens the wraps the server gave for this person and keeps the keys
    /// not held yet. Gives the versions taken; a wrap that does not open is an
    /// error, and the others are kept.
    pub fn take_collection_keys(&self, collection: &str, given: &[Given]) -> Result<Vec<i64>> {
        let me = self.identity_for_keys()?;
        let held = self.collection_versions(collection);
        let mut taken = Vec::new();
        let mut failed = None;
        for g in given {
            let version = i64::from(g.version);
            if held.contains(&version) {
                continue;
            }
            match me.unwrap(&g.wrap, collection, g.version, &g.wrapper) {
                Ok(key) => {
                    self.hold(collection, version, &key)?;
                    taken.push(version);
                }
                Err(e) => failed = Some(e),
            }
        }
        match failed {
            Some(e) if taken.is_empty() => Err(e),
            _ => Ok(taken),
        }
    }

    /// A new key for a collection, wrapped for each identity given (the
    /// person's own among them, to get it back from the server once taken).
    /// Not kept here: the server may take another app's first.
    pub fn make_collection_key(
        &self,
        collection: &str,
        version: u32,
        to: &[PublicIdentity],
    ) -> Result<Vec<Wrap>> {
        let me = self.identity_for_keys()?;
        let key = new_key()?;
        to.iter()
            .map(|p| me.wrap(&key, collection, version, p))
            .collect()
    }

    /// The current key of a collection, wrapped for people waiting for it,
    /// whose fingerprints were compared.
    pub fn wrap_collection_key(
        &self,
        collection: &str,
        to: &[PublicIdentity],
    ) -> Result<(u32, Vec<Wrap>)> {
        let me = self.identity_for_keys()?;
        let space = Space::Collection(collection.to_string());
        let version = self.space_version(&space)?;
        let key = self.space_key(&space, version)?;
        let v = u32::try_from(version).map_err(|_| Error::Decrypt)?;
        let wraps = to
            .iter()
            .map(|p| me.wrap(&key, collection, v, p))
            .collect::<Result<_>>()?;
        Ok((v, wraps))
    }
}

/// A collection the server serves the person, as it lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CollectionInfo {
    pub id: String,
    pub org_id: String,
    pub name: String,
    /// The version of its key now: 0 before an app makes the first.
    pub key_version: u32,
    /// What the person may do: `connect`, `edit` or `manage`.
    pub permission: String,
}

impl CollectionInfo {
    pub fn manages(&self) -> bool {
        self.permission == "manage"
    }

    pub fn edits(&self) -> bool {
        matches!(self.permission.as_str(), "edit" | "manage")
    }
}

/// Somebody with access to a collection, for those who manage it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionMember {
    pub user_id: String,
    pub username: String,
    pub full_name: String,
    /// In the organization: `owner`, `admin` or `member`.
    pub role: String,
    pub permission: String,
    /// False while suspended: they keep access and get nothing.
    pub served: bool,
    pub has_key: bool,
    /// None until their app publishes one: nobody can give them the key before.
    pub identity: Option<PublicIdentity>,
}

/// What a shellwing server does for collections: the [`Backend`] of one is the
/// person's own space, and it serves the collections beside it.
pub trait Collections: Send + Sync {
    fn list_collections(&self) -> io::Result<Vec<CollectionInfo>>;
    /// Who has access, with their identities; for those who manage it.
    fn collection_members(&self, c: &CollectionInfo) -> io::Result<Vec<CollectionMember>>;
    /// The person's wraps of each version of the key.
    fn collection_keys(&self, c: &CollectionInfo) -> io::Result<Vec<Given>>;
    /// Gives the collection a new key, version current + 1, wrapped for each
    /// user id. `Ok(false)`: another app gave it one first.
    fn create_collection_key(
        &self,
        c: &CollectionInfo,
        version: u32,
        wraps: &[(String, Wrap)],
    ) -> io::Result<bool>;
    /// Gives the current key to people waiting for it.
    fn add_collection_wraps(
        &self,
        c: &CollectionInfo,
        version: u32,
        wraps: &[(String, Wrap)],
    ) -> io::Result<()>;
    /// The collection's store.
    fn collection_store(&self, c: &CollectionInfo) -> io::Result<Box<dyn Backend>>;
}

/// How syncing one collection went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectionState {
    /// Merged both ways.
    Synced {
        pulled: usize,
        pushed: usize,
    },
    /// Nobody gave this person its key yet.
    Waiting,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionOutcome {
    pub collection: CollectionInfo,
    pub state: CollectionState,
}

impl Vault {
    /// Syncs each collection the server serves the person: takes the keys
    /// given since, makes the first key of one with none if they may manage
    /// it, and merges its records. One that fails does not stop the others.
    pub fn sync_collections(&self, host: &dyn Collections) -> Result<Vec<CollectionOutcome>> {
        let listed = host.list_collections()?;
        self.keep_known_collections(&listed)?;
        let mut out = Vec::new();
        for c in listed {
            let state = match self.sync_one_collection(host, &c) {
                Ok(Some(r)) => CollectionState::Synced {
                    pulled: r.pulled,
                    pushed: r.pushed,
                },
                Ok(None) => CollectionState::Waiting,
                Err(e) => CollectionState::Failed(e.to_string()),
            };
            out.push(CollectionOutcome {
                collection: c,
                state,
            });
        }
        Ok(out)
    }

    fn sync_one_collection(
        &self,
        host: &dyn Collections,
        c: &CollectionInfo,
    ) -> Result<Option<crate::SyncReport>> {
        if c.key_version == 0 {
            if !c.manages() {
                return Ok(None);
            }
            self.make_first_key(host, c)?;
        }
        let newest = self.collection_versions(&c.id).last().copied().unwrap_or(0);
        if newest < i64::from(c.key_version.max(1)) {
            let given = host.collection_keys(c)?;
            if !given.is_empty() {
                self.take_collection_keys(&c.id, &given)?;
            }
        }
        if self.collection_versions(&c.id).is_empty() {
            return Ok(None);
        }
        let store = host.collection_store(c)?;
        self.sync_collection(&*store, &c.id, c.edits()).map(Some)
    }

    /// The collections the server served at the last sync: what the host list
    /// shows while offline. Kept on this device only.
    pub fn known_collections(&self) -> Result<Vec<CollectionInfo>> {
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        let text: Option<String> = db
            .query_row(
                "SELECT value FROM meta WHERE key = 'collections'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match text {
            Some(t) => serde_json::from_str(&t)?,
            None => Vec::new(),
        })
    }

    fn keep_known_collections(&self, listed: &[CollectionInfo]) -> Result<()> {
        let text = serde_json::to_string(listed)?;
        let db = self.db.lock().map_err(|_| Error::Decrypt)?;
        db.execute(
            "INSERT INTO meta (key, value) VALUES ('collections', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![text],
        )?;
        Ok(())
    }

    /// Version 1 of a collection made on the web, wrapped for the person and
    /// every owner with an identity, as the server requires. Another app
    /// making it first is fine: its wrap for this person comes next.
    fn make_first_key(&self, host: &dyn Collections, c: &CollectionInfo) -> Result<()> {
        let me = self.identity_for_keys()?.public();
        let members = host.collection_members(c)?;
        let mut to: Vec<(String, PublicIdentity)> = Vec::new();
        for m in members {
            let Some(identity) = m.identity else { continue };
            if identity == me || m.role == "owner" {
                to.push((m.user_id, identity));
            }
        }
        if !to.iter().any(|(_, p)| *p == me) {
            return Err(Error::Collection(
                "the server does not have this device's identity yet".into(),
            ));
        }
        let identities: Vec<PublicIdentity> = to.iter().map(|(_, p)| p.clone()).collect();
        let wraps = self.make_collection_key(&c.id, 1, &identities)?;
        let wraps: Vec<(String, Wrap)> = to.into_iter().map(|(u, _)| u).zip(wraps).collect();
        host.create_collection_key(c, 1, &wraps)?;
        Ok(())
    }

    /// Gives the current key to people waiting for it, whose fingerprints the
    /// person compared.
    pub fn give_collection_key(
        &self,
        host: &dyn Collections,
        c: &CollectionInfo,
        to: &[CollectionMember],
    ) -> Result<()> {
        let identities: Vec<PublicIdentity> = to
            .iter()
            .map(|m| {
                m.identity
                    .clone()
                    .ok_or_else(|| Error::Collection(format!("{} has no identity yet", m.username)))
            })
            .collect::<Result<_>>()?;
        let (version, wraps) = self.wrap_collection_key(&c.id, &identities)?;
        let wraps: Vec<(String, Wrap)> = to.iter().map(|m| m.user_id.clone()).zip(wraps).collect();
        host.add_collection_wraps(c, version, &wraps)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    fn counting(from: u8) -> Key {
        std::array::from_fn(|i| from + i as u8)
    }

    /// The identity vector's (identity.rs): secrets 1, 2, 3… and 33, 34, 35….
    fn vector_identity() -> Identity {
        Identity::from_secrets(counting(1), counting(33))
    }

    fn another() -> Identity {
        Identity::from_secrets(counting(101), counting(133))
    }

    #[test]
    fn a_member_opens_the_key_wrapped_for_them() {
        let (alice, bob) = (vector_identity(), another());
        let key = new_key().expect("key");
        let w = alice.wrap(&key, "col1", 3, &bob.public()).expect("wrap");
        let alice_ed = alice.public().ed25519;
        assert_eq!(*bob.unwrap(&w, "col1", 3, &alice_ed).expect("open"), *key);

        assert!(
            alice.unwrap(&w, "col1", 3, &alice_ed).is_err(),
            "not alice's"
        );
        assert!(
            bob.unwrap(&w, "col2", 3, &alice_ed).is_err(),
            "another collection"
        );
        assert!(
            bob.unwrap(&w, "col1", 4, &alice_ed).is_err(),
            "another version"
        );
        assert!(
            bob.unwrap(&w, "col1", 3, &bob.public().ed25519).is_err(),
            "not signed by bob"
        );
        let mut forged = w.clone();
        forged.sealed[30] ^= 1;
        assert!(
            bob.unwrap(&forged, "col1", 3, &alice_ed).is_err(),
            "changed"
        );
    }

    #[test]
    fn a_key_its_commitment_does_not_name_is_refused() {
        // A wrapper sealing one key and committing to another: re-signed, so
        // only the commitment can tell.
        let (alice, bob) = (vector_identity(), another());
        let mut w = alice
            .wrap(&counting(7), "col1", 1, &bob.public())
            .expect("wrap");
        w.commitment = commitment(&counting(8));
        let signing = SigningKey::from_bytes(alice.ed25519_secret());
        w.signature = signing
            .sign(&w.signed("col1", 1).expect("signed"))
            .to_bytes();
        assert!(bob.unwrap(&w, "col1", 1, &alice.public().ed25519).is_err());
    }

    /// The server's vector (shellwing-server's pkg/collectionkey): the parts
    /// count up from 65, 97 and 169, for the identity vector's X25519 key,
    /// signed by its Ed25519 key.
    #[test]
    fn the_signature_is_the_servers() {
        let w = Wrap {
            recipient_x25519: vector_identity().public().x25519,
            ephemeral_public: counting(65),
            sealed: (0..SEALED_LEN as u8).map(|i| 97 + i).collect(),
            commitment: counting(169),
            signature: [0; 64],
        };
        let signing = SigningKey::from_bytes(&counting(33));
        let signature = signing.sign(&w.signed("01JC0LLECT10N0000000000000", 1).expect("signed"));
        assert_eq!(
            B64.encode(signature.to_bytes()),
            "hV8F1bdORRJKFUVYFOyYGOBIeiybJv+zC3lm3p8xFzyZGcJ7Hz889YqDOoBYHSwhqRqO8J8JH4ExwMrNxSjkAw=="
        );
    }

    /// A whole wrap, made the same way every time: the format document's
    /// vector. The identity vector wraps the key 201, 202, 203… for itself,
    /// with the ephemeral secret 1, 1, 1… and the nonce 9, 9, 9….
    #[test]
    fn a_wrap_is_the_documented_one() {
        let me = vector_identity();
        let key = counting(201);
        let w = me
            .wrap_with(
                &key,
                "01JC0LLECT10N0000000000000",
                1,
                &me.public(),
                &[1; KEY_LEN],
                &[9; NONCE_LEN],
            )
            .expect("wrap");
        assert_eq!(
            B64.encode(w.ephemeral_public),
            "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
        );
        assert_eq!(
            B64.encode(&w.sealed),
            "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJD3nIsVTflOvUIw0qRWj6CBdhXmf5+zLW2+BQ21NL3D9QFo166bTPj7YqYRaS5Ymk"
        );
        assert_eq!(
            B64.encode(w.commitment),
            "ruUTPgQFKROEd2yEBlBz55dKRs3Hj/1Lnb8OrpqrBE0="
        );
        assert_eq!(
            B64.encode(w.signature),
            "2VxS/LEyomoRmfqlvvyPEQi0jSFkkUZU63a8E+FKTL6OECH8jAx9UHezgF668S0ISYBwxRcFXOYLNgxDtHqWCw=="
        );
        let opened = me
            .unwrap(&w, "01JC0LLECT10N0000000000000", 1, &me.public().ed25519)
            .expect("open");
        assert_eq!(*opened, key);
    }

    mod spaces {
        use super::super::*;
        use crate::{Folder, MemoryKeys};

        fn dir(name: &str) -> std::path::PathBuf {
            let d = std::env::temp_dir().join(format!(
                "shellwing-collection-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("dir");
            d
        }

        /// What the server hands back: each wrap with its version and who
        /// signed it.
        fn given(by: &Vault, version: u32, wraps: &[Wrap], to: &Vault) -> Vec<Given> {
            let mine = to
                .identity()
                .expect("read")
                .expect("identity")
                .public()
                .x25519;
            let wrapper = by
                .identity()
                .expect("read")
                .expect("identity")
                .public()
                .ed25519;
            wraps
                .iter()
                .filter(|w| w.recipient_x25519 == mine)
                .map(|w| Given {
                    version,
                    wrap: w.clone(),
                    wrapper,
                })
                .collect()
        }

        fn host(v: &Vault, space: &Space, id: &str, address: &str) {
            let values = [("address".to_string(), address.to_string())].into();
            v.update_in(space, id, "host", "web", &values)
                .expect("save");
        }

        #[test]
        fn two_people_share_a_collection_through_its_store() {
            let d = dir("share");
            let alice = Vault::open(&d.join("a.db"), &MemoryKeys::default()).expect("alice");
            let bob = Vault::open(&d.join("b.db"), &MemoryKeys::default()).expect("bob");
            let both = [
                alice.ensure_identity().expect("id").public(),
                bob.ensure_identity().expect("id").public(),
            ];
            let col = Space::Collection("col1".into());
            assert!(
                alice
                    .update_in(&col, "aa01000000000000", "host", "web", &BTreeMap::new())
                    .is_err(),
                "no key yet"
            );

            // Alice's app makes the key; the server gives each their wrap.
            let wraps = alice.make_collection_key("col1", 1, &both).expect("make");
            assert_eq!(
                alice
                    .take_collection_keys("col1", &given(&alice, 1, &wraps, &alice))
                    .expect("take"),
                [1]
            );
            assert_eq!(
                bob.take_collection_keys("col1", &given(&alice, 1, &wraps, &bob))
                    .expect("take"),
                [1]
            );
            assert!(
                bob.take_collection_keys("col1", &given(&alice, 1, &wraps, &bob))
                    .expect("again")
                    .is_empty()
            );

            let store = Folder::new(&d.join("col1"));
            host(&alice, &col, "aa01000000000000", "10.0.0.1");
            alice.sync_collection(&store, "col1", true).expect("sync a");
            bob.sync_collection(&store, "col1", true).expect("sync b");
            let entries = bob.entries_in(&col, "host").expect("entries");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].1["address"], "10.0.0.1");
            assert!(
                bob.entries("host").expect("personal").is_empty(),
                "not personal"
            );

            // Bob's edit comes back; the record lives in the collection only.
            host(&bob, &col, "aa01000000000000", "10.0.0.2");
            bob.sync_collection(&store, "col1", true).expect("sync b");
            alice.sync_collection(&store, "col1", true).expect("sync a");
            assert_eq!(
                alice.entries_in(&col, "host").expect("entries")[0].1["address"],
                "10.0.0.2"
            );
            let values = [("address".to_string(), "x".to_string())].into();
            assert!(
                alice
                    .update("aa01000000000000", "host", "web", &values)
                    .is_err(),
                "another space's id"
            );
        }

        #[test]
        fn a_record_under_a_key_not_held_yet_waits() {
            let d = dir("versions");
            let keys = MemoryKeys::default();
            let alice = Vault::open(&d.join("a.db"), &keys).expect("alice");
            let bob = Vault::open(&d.join("b.db"), &MemoryKeys::default()).expect("bob");
            let both = [
                alice.ensure_identity().expect("id").public(),
                bob.ensure_identity().expect("id").public(),
            ];
            let col = Space::Collection("col1".into());
            let v1 = alice.make_collection_key("col1", 1, &both).expect("v1");
            alice
                .take_collection_keys("col1", &given(&alice, 1, &v1, &alice))
                .expect("take");
            bob.take_collection_keys("col1", &given(&alice, 1, &v1, &bob))
                .expect("take");
            // Version 2, given to Alice only so far.
            let v2 = alice
                .make_collection_key("col1", 2, &both[..1])
                .expect("v2");
            alice
                .take_collection_keys("col1", &given(&alice, 2, &v2, &alice))
                .expect("take");
            assert_eq!(alice.collection_versions("col1"), [1, 2]);

            let store = Folder::new(&d.join("col1"));
            host(&alice, &col, "aa02000000000000", "10.0.0.3");
            alice.sync_collection(&store, "col1", true).expect("sync a");
            bob.sync_collection(&store, "col1", true).expect("sync b");
            assert!(
                bob.entries_in(&col, "host").expect("entries").is_empty(),
                "waits for v2"
            );

            // Alice's keys are still hers after opening the vault again.
            drop(alice);
            let alice = Vault::open(&d.join("a.db"), &keys).expect("again");
            assert_eq!(alice.collection_versions("col1"), [1, 2]);
            assert_eq!(alice.entries_in(&col, "host").expect("entries").len(), 1);
        }

        #[test]
        fn the_current_key_is_wrapped_for_who_waits() {
            let d = dir("waits");
            let alice = Vault::open(&d.join("a.db"), &MemoryKeys::default()).expect("alice");
            let carol = Vault::open(&d.join("c.db"), &MemoryKeys::default()).expect("carol");
            let a = alice.ensure_identity().expect("id").public();
            let c = carol.ensure_identity().expect("id").public();
            assert!(
                alice
                    .wrap_collection_key("col1", std::slice::from_ref(&c))
                    .is_err(),
                "no key held"
            );
            let wraps = alice.make_collection_key("col1", 1, &[a]).expect("make");
            alice
                .take_collection_keys("col1", &given(&alice, 1, &wraps, &alice))
                .expect("take");
            let (version, for_carol) = alice.wrap_collection_key("col1", &[c]).expect("wrap");
            assert_eq!(version, 1);
            assert_eq!(
                carol
                    .take_collection_keys("col1", &given(&alice, 1, &for_carol, &carol))
                    .expect("take"),
                [1]
            );
        }
    }

    mod host {
        use std::sync::Mutex;

        use super::super::*;
        use crate::{Folder, MemoryKeys};

        /// A server as far as collections go: one collection, its people and
        /// their wraps, and a folder as its store.
        struct Fake {
            dir: std::path::PathBuf,
            version: Mutex<u32>,
            people: Vec<(String, String, String, PublicIdentity)>,
            wraps: Mutex<Vec<(String, u32, Wrap, Key)>>,
        }

        /// One person's view of it.
        struct As<'a> {
            fake: &'a Fake,
            me: String,
        }

        impl As<'_> {
            fn info(&self) -> CollectionInfo {
                let (_, _, permission, _) = self
                    .fake
                    .people
                    .iter()
                    .find(|p| p.0 == self.me)
                    .expect("in it");
                CollectionInfo {
                    id: "col1".into(),
                    org_id: "org1".into(),
                    name: "Production".into(),
                    key_version: *self.fake.version.lock().expect("lock"),
                    permission: permission.clone(),
                }
            }

            fn identity_of(&self, user: &str) -> &PublicIdentity {
                &self
                    .fake
                    .people
                    .iter()
                    .find(|p| p.0 == user)
                    .expect("person")
                    .3
            }
        }

        impl Collections for As<'_> {
            fn list_collections(&self) -> io::Result<Vec<CollectionInfo>> {
                Ok(vec![self.info()])
            }

            fn collection_members(&self, _: &CollectionInfo) -> io::Result<Vec<CollectionMember>> {
                let version = *self.fake.version.lock().expect("lock");
                let wraps = self.fake.wraps.lock().expect("lock");
                Ok(self
                    .fake
                    .people
                    .iter()
                    .map(|(user, role, permission, identity)| CollectionMember {
                        user_id: user.clone(),
                        username: user.clone(),
                        full_name: String::new(),
                        role: role.clone(),
                        permission: permission.clone(),
                        served: true,
                        has_key: wraps.iter().any(|w| w.0 == *user && w.1 == version),
                        identity: Some(identity.clone()),
                    })
                    .collect())
            }

            fn collection_keys(&self, _: &CollectionInfo) -> io::Result<Vec<Given>> {
                Ok(self
                    .fake
                    .wraps
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|w| w.0 == self.me)
                    .map(|(_, version, wrap, wrapper)| Given {
                        version: *version,
                        wrap: wrap.clone(),
                        wrapper: *wrapper,
                    })
                    .collect())
            }

            fn create_collection_key(
                &self,
                _: &CollectionInfo,
                version: u32,
                wraps: &[(String, Wrap)],
            ) -> io::Result<bool> {
                let mut current = self.fake.version.lock().expect("lock");
                if version != *current + 1 {
                    return Ok(false);
                }
                *current = version;
                let wrapper = self.identity_of(&self.me).ed25519;
                let mut all = self.fake.wraps.lock().expect("lock");
                for (user, w) in wraps {
                    assert!(
                        w.signed_by("col1", version, &wrapper),
                        "the server checks it"
                    );
                    assert_eq!(
                        w.recipient_x25519,
                        self.identity_of(user).x25519,
                        "for their key"
                    );
                    all.push((user.clone(), version, w.clone(), wrapper));
                }
                Ok(true)
            }

            fn add_collection_wraps(
                &self,
                _: &CollectionInfo,
                version: u32,
                wraps: &[(String, Wrap)],
            ) -> io::Result<()> {
                let wrapper = self.identity_of(&self.me).ed25519;
                let mut all = self.fake.wraps.lock().expect("lock");
                for (user, w) in wraps {
                    assert!(w.signed_by("col1", version, &wrapper));
                    all.push((user.clone(), version, w.clone(), wrapper));
                }
                Ok(())
            }

            fn collection_store(&self, _: &CollectionInfo) -> io::Result<Box<dyn Backend>> {
                Ok(Box::new(Folder::new(&self.fake.dir)))
            }
        }

        fn vault(d: &std::path::Path, name: &str) -> Vault {
            let v =
                Vault::open(&d.join(format!("{name}.db")), &MemoryKeys::default()).expect("open");
            v.ensure_identity().expect("identity");
            v
        }

        fn public(v: &Vault) -> PublicIdentity {
            v.identity().expect("read").expect("identity").public()
        }

        #[test]
        fn a_manager_makes_the_key_and_gives_it_to_who_waits() {
            let d = std::env::temp_dir()
                .join(format!("shellwing-collection-host-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("dir");
            let (max, olga, ed, vic) = (
                vault(&d, "max"),
                vault(&d, "olga"),
                vault(&d, "ed"),
                vault(&d, "vic"),
            );
            let fake = Fake {
                dir: d.join("store"),
                version: Mutex::new(0),
                people: vec![
                    ("max".into(), "member".into(), "manage".into(), public(&max)),
                    (
                        "olga".into(),
                        "owner".into(),
                        "manage".into(),
                        public(&olga),
                    ),
                    ("ed".into(), "member".into(), "edit".into(), public(&ed)),
                    (
                        "vic".into(),
                        "member".into(),
                        "connect".into(),
                        public(&vic),
                    ),
                ],
                wraps: Mutex::new(Vec::new()),
            };
            let host = |me: &str| As {
                fake: &fake,
                me: me.into(),
            };
            let col = Space::Collection("col1".into());

            // Ed may not make the key: he waits.
            let out = ed.sync_collections(&host("ed")).expect("sync");
            assert_eq!(out[0].state, CollectionState::Waiting);
            // Max's app makes it, for himself and the owner.
            let out = max.sync_collections(&host("max")).expect("sync");
            assert_eq!(
                out[0].state,
                CollectionState::Synced {
                    pulled: 0,
                    pushed: 0
                }
            );
            assert_eq!(*fake.version.lock().expect("lock"), 1);
            let holders: Vec<String> = fake
                .wraps
                .lock()
                .expect("lock")
                .iter()
                .map(|w| w.0.clone())
                .collect();
            assert_eq!(holders, ["max", "olga"]);
            let values = [("address".to_string(), "10.0.0.9".to_string())].into();
            max.update_in(&col, "ab01000000000000", "host", "db", &values)
                .expect("save");
            max.sync_collections(&host("max")).expect("sync");
            olga.sync_collections(&host("olga")).expect("sync");
            assert_eq!(
                olga.entries_in(&col, "host").expect("entries").len(),
                1,
                "the owner has it"
            );

            // Max gives it to Ed and Vic, whose fingerprints he compared.
            let members = host("max")
                .collection_members(&host("max").info())
                .expect("members");
            let waiting: Vec<CollectionMember> =
                members.into_iter().filter(|m| !m.has_key).collect();
            assert_eq!(waiting.len(), 2);
            max.give_collection_key(&host("max"), &host("max").info(), &waiting)
                .expect("give");
            let out = ed.sync_collections(&host("ed")).expect("sync");
            assert!(matches!(
                out[0].state,
                CollectionState::Synced { pulled: 1, .. }
            ));
            assert_eq!(
                ed.entries_in(&col, "host").expect("entries")[0].1["address"],
                "10.0.0.9"
            );

            // Vic connects only: what he has here is not written there.
            vic.sync_collections(&host("vic")).expect("sync");
            let values = [("address".to_string(), "6.6.6.6".to_string())].into();
            vic.update_in(&col, "ab01000000000000", "host", "db", &values)
                .expect("local");
            let out = vic.sync_collections(&host("vic")).expect("sync");
            assert!(matches!(
                out[0].state,
                CollectionState::Synced { pushed: 0, .. }
            ));
            ed.sync_collections(&host("ed")).expect("sync");
            assert_eq!(
                ed.entries_in(&col, "host").expect("entries")[0].1["address"],
                "10.0.0.9"
            );
        }
    }
}

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

use rusqlite::params;

use crate::identity::{Identity, Key, PublicIdentity};
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
            alice.sync_collection(&store, "col1").expect("sync a");
            bob.sync_collection(&store, "col1").expect("sync b");
            let entries = bob.entries_in(&col, "host").expect("entries");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].1["address"], "10.0.0.1");
            assert!(
                bob.entries("host").expect("personal").is_empty(),
                "not personal"
            );

            // Bob's edit comes back; the record lives in the collection only.
            host(&bob, &col, "aa01000000000000", "10.0.0.2");
            bob.sync_collection(&store, "col1").expect("sync b");
            alice.sync_collection(&store, "col1").expect("sync a");
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
            alice.sync_collection(&store, "col1").expect("sync a");
            bob.sync_collection(&store, "col1").expect("sync b");
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
}

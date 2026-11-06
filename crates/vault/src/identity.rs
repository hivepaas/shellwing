//! A person's identity: an X25519 key pair, which collection keys are wrapped
//! for, and an Ed25519 one, which signs what they wrap
//! (docs/design/collections.md).
//!
//! It is a record of the personal space, so that every device of the person
//! has it through sync. Both secrets are one field: two devices making an
//! identity at once end up with one of them, whichever wins the merge, never a
//! mix.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use curve25519_dalek::montgomery::MontgomeryPoint;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{Error, KEY_LEN, Result, Vault, random};

/// The identity's record. Hex, as synced files' names must start with.
pub const IDENTITY_ID: &str = "1de0717e000000000000000000000000";
const RECORD_TYPE: &str = "identity";
const FIELD: &str = "keys";
/// What the Ed25519 key signs: this, then the two public keys.
const SIGNED: &[u8] = b"shellwing identity signature v1";
/// What the fingerprint hashes: this, then the two public keys.
const FINGERPRINT: &[u8] = b"shellwing identity v1";

pub type Key = [u8; KEY_LEN];

/// The secret halves. Never leaves the vault but sealed in its record.
pub struct Identity {
    x25519: Zeroizing<Key>,
    ed25519: Zeroizing<Key>,
}

/// What the server and the person's colleagues may know: the public keys, and
/// the X25519 one signed by the Ed25519 one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicIdentity {
    pub x25519: Key,
    pub ed25519: Key,
    pub signature: [u8; 64],
}

impl Identity {
    fn generate() -> Result<Identity> {
        Ok(Identity {
            x25519: Zeroizing::new(random::<KEY_LEN>()?),
            ed25519: Zeroizing::new(random::<KEY_LEN>()?),
        })
    }

    fn encode(&self) -> Zeroizing<String> {
        let mut both = Zeroizing::new([0u8; 2 * KEY_LEN]);
        both[..KEY_LEN].copy_from_slice(&self.x25519[..]);
        both[KEY_LEN..].copy_from_slice(&self.ed25519[..]);
        Zeroizing::new(B64.encode(&both[..]))
    }

    fn decode(text: &str) -> Option<Identity> {
        let both = Zeroizing::new(B64.decode(text).ok()?);
        if both.len() != 2 * KEY_LEN {
            return None;
        }
        let half = |range: std::ops::Range<usize>| {
            let mut key = Zeroizing::new([0u8; KEY_LEN]);
            key.copy_from_slice(&both[range]);
            key
        };
        Some(Identity {
            x25519: half(0..KEY_LEN),
            ed25519: half(KEY_LEN..2 * KEY_LEN),
        })
    }

    pub fn public(&self) -> PublicIdentity {
        let x25519 = MontgomeryPoint::mul_base_clamped(*self.x25519).to_bytes();
        let signing = SigningKey::from_bytes(&self.ed25519);
        let ed25519 = signing.verifying_key().to_bytes();
        let signature = signing.sign(&signed(&x25519, &ed25519)).to_bytes();
        PublicIdentity {
            x25519,
            ed25519,
            signature,
        }
    }
}

impl PublicIdentity {
    /// Whether the Ed25519 key signed the X25519 one: that one person holds both.
    pub fn verify(&self) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(&self.ed25519) else {
            return false;
        };
        key.verify_strict(
            &signed(&self.x25519, &self.ed25519),
            &Signature::from_bytes(&self.signature),
        )
        .is_ok()
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.x25519, &self.ed25519)
    }
}

fn signed(x25519: &Key, ed25519: &Key) -> Vec<u8> {
    [SIGNED, x25519, ed25519].concat()
}

/// What people compare to know they have each other's keys: six groups of
/// five digits, from the first 30 bytes of a hash of both public keys, each
/// group the big-endian number of five bytes modulo 100000.
pub fn fingerprint(x25519: &Key, ed25519: &Key) -> String {
    let hash = Sha256::new()
        .chain_update(FINGERPRINT)
        .chain_update(x25519)
        .chain_update(ed25519)
        .finalize();
    hash[..30]
        .chunks(5)
        .map(|c| {
            let n = c.iter().fold(0u64, |n, b| (n << 8) | u64::from(*b));
            format!("{:05}", n % 100_000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Vault {
    /// The person's identity, if a device of theirs made one.
    pub fn identity(&self) -> Result<Option<Identity>> {
        let Some((_, fields)) = self
            .entries(RECORD_TYPE)?
            .into_iter()
            .find(|(id, _)| id == IDENTITY_ID)
        else {
            return Ok(None);
        };
        match fields.get(FIELD) {
            Some(text) => Identity::decode(text)
                .map(Some)
                .ok_or_else(|| Error::Format(serde::de::Error::custom("the identity is damaged"))),
            None => Ok(None),
        }
    }

    /// The person's identity, made now if no device of theirs has one yet.
    pub fn ensure_identity(&self) -> Result<Identity> {
        if let Some(identity) = self.identity()? {
            return Ok(identity);
        }
        let identity = Identity::generate()?;
        let fields = [(FIELD.to_string(), identity.encode().to_string())].into();
        self.update(IDENTITY_ID, RECORD_TYPE, "Identity", &fields)?;
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryKeys;

    #[test]
    fn an_identity_is_made_once_and_signs_its_keys() {
        let dir = std::env::temp_dir().join(format!("shellwing-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let v = Vault::open(&dir.join("v.db"), &MemoryKeys::default()).expect("open");
        assert!(v.identity().expect("read").is_none());

        let first = v.ensure_identity().expect("make").public();
        let again = v.ensure_identity().expect("again").public();
        assert_eq!(first, again, "made once");
        assert!(first.verify());
        assert!(
            v.items().expect("items").is_empty(),
            "not an item people see"
        );

        let mut forged = first.clone();
        forged.x25519[0] ^= 1;
        assert!(!forged.verify(), "another X25519 key is not signed");
    }

    /// The test vector the server checks too (shellwing-server's
    /// pkg/identity): the keys of secrets 1, 2, 3… and 33, 34, 35….
    #[test]
    fn the_fingerprint_and_signature_are_the_documented_ones() {
        let secret =
            |from: u8| Zeroizing::new(std::array::from_fn::<u8, KEY_LEN, _>(|i| from + i as u8));
        let id = Identity {
            x25519: secret(1),
            ed25519: secret(33),
        };
        let public = id.public();
        assert!(public.verify());
        assert_eq!(
            B64.encode(public.x25519),
            "B6N8vBQgk8i3VdwbEOhstCY3StFqqFPtC9/AsrhtHHw="
        );
        assert_eq!(
            B64.encode(public.ed25519),
            "5/FioQvsVZr+oZXk3OhLaVaNXSywlj60RsBoXisX8vA="
        );
        // Ed25519 signs the same way every time: the signature is part of the vector.
        assert_eq!(
            B64.encode(public.signature),
            "I+mHldAuIZ4mZvZQbMr+4cdL6LdTZ7qA1Xz0egaIf0MvLbChGT55sA1gZQTCnuJV+hW3ZzeKafF1lHrxk2uNCA=="
        );
        assert_eq!(public.fingerprint(), "42092 42879 96924 77795 39558 81840");
    }
}

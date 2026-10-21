//! `release.signed.json`: `release.json` itself, base64, with its signatures
//! beside it in one file, so a CDN cannot serve a new list with an old
//! signature. `tools/releasesign` writes it, under the context
//! `shellwing-release-v1`.
//!
//! The list must carry a valid ed25519 (Ed25519ctx) and a valid ML-DSA-65
//! signature, each by a key compiled into the app: a forger has to break
//! both. A signature by a key this app does not know is passed over (a key
//! being rotated in); one by a known key that does not verify refuses the
//! whole list.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use fips204::ml_dsa_65;
use fips204::traits::{SerDes as _, Verifier as _};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};

pub const ED25519: &str = "ed25519";
pub const ML_DSA_65: &str = "ml-dsa-65";

/// At most this many signatures are looked at.
const MOST_SIGNATURES: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("the list of releases is not signed as it should be: {0}")]
    Invalid(String),
    #[error("a release key built into the app does not read: {0}")]
    Key(String),
}

fn invalid(why: impl Into<String>) -> SignatureError {
    SignatureError::Invalid(why.into())
}

/// A trusted public key.
#[derive(Clone)]
pub enum PublicKey {
    Ed25519([u8; 32]),
    MlDsa65(Box<ml_dsa_65::PublicKey>),
}

impl PublicKey {
    fn algorithm(&self) -> &'static str {
        match self {
            PublicKey::Ed25519(_) => ED25519,
            PublicKey::MlDsa65(_) => ML_DSA_65,
        }
    }

    fn verify(&self, data: &[u8], sig: &[u8], context: &[u8]) -> bool {
        match self {
            PublicKey::Ed25519(k) => {
                <&[u8; 64]>::try_from(sig).is_ok_and(|s| ed25519ctx_verify(k, data, s, context))
            }
            PublicKey::MlDsa65(k) => {
                <&[u8; ml_dsa_65::SIG_LEN]>::try_from(sig).is_ok_and(|s| k.verify(data, s, context))
            }
        }
    }
}

/// The trusted keys, by key id.
pub type PublicKeys = BTreeMap<String, PublicKey>;

/// Reads PKIX PEM public keys by key id; every one must read, and together
/// they must hold a key of each algorithm.
pub fn parse_keys(pems: &[(&str, &str)]) -> Result<PublicKeys, SignatureError> {
    if pems.is_empty() {
        return Err(SignatureError::Key("no release keys are built in".into()));
    }
    let mut keys = PublicKeys::new();
    for (id, pem) in pems {
        let key = parse_key(pem).map_err(|why| SignatureError::Key(format!("{id}: {why}")))?;
        keys.insert(id.to_string(), key);
    }
    for alg in [ED25519, ML_DSA_65] {
        if !keys.values().any(|k| k.algorithm() == alg) {
            return Err(SignatureError::Key(format!("no {alg} key")));
        }
    }
    Ok(keys)
}

/// ed25519 (1.3.101.112) and ML-DSA-65 (2.16.840.1.101.3.4.3.18).
const OID_ED25519: &[u8] = &[0x2b, 0x65, 0x70];
const OID_ML_DSA_65: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x03, 0x12];

fn parse_key(pem: &str) -> Result<PublicKey, &'static str> {
    let body: String = pem
        .lines()
        .skip_while(|l| l.trim() != "-----BEGIN PUBLIC KEY-----")
        .skip(1)
        .take_while(|l| l.trim() != "-----END PUBLIC KEY-----")
        .collect();
    let der = STANDARD.decode(body.trim()).map_err(|_| "not PEM")?;
    // SubjectPublicKeyInfo: SEQUENCE { SEQUENCE { OID }, BIT STRING }.
    let (spki, rest) = der_take(&der, 0x30).ok_or("not a public key")?;
    if !rest.is_empty() {
        return Err("not a public key");
    }
    let (algorithm, rest) = der_take(spki, 0x30).ok_or("not a public key")?;
    let (oid, params) = der_take(algorithm, 0x06).ok_or("not a public key")?;
    let (bits, rest) = der_take(rest, 0x03).ok_or("not a public key")?;
    if !rest.is_empty() || !params.is_empty() || bits.first() != Some(&0) {
        return Err("not a public key");
    }
    let raw = &bits[1..];
    match oid {
        OID_ED25519 => Ok(PublicKey::Ed25519(
            raw.try_into().map_err(|_| "an ed25519 key is 32 bytes")?,
        )),
        OID_ML_DSA_65 => {
            let raw: [u8; ml_dsa_65::PK_LEN] = raw
                .try_into()
                .map_err(|_| "an ML-DSA-65 key is 1952 bytes")?;
            let key =
                ml_dsa_65::PublicKey::try_from_bytes(raw).map_err(|_| "a bad ML-DSA-65 key")?;
            Ok(PublicKey::MlDsa65(Box::new(key)))
        }
        _ => Err("neither ed25519 nor ML-DSA-65"),
    }
}

/// One DER element of `tag` at the start of `der`: its content, and what
/// follows it.
fn der_take(der: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&t, rest) = der.split_first()?;
    if t != tag {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let len = rest[..n].iter().fold(0usize, |l, b| (l << 8) | *b as usize);
        (len, &rest[n..])
    };
    (rest.len() >= len).then(|| rest.split_at(len))
}

/// Ed25519ctx (RFC 8032, 5.1.7, with dom2(0, context)), as Go's
/// `ed25519.VerifyWithOptions` with a context: [S]B = R + [k]A, with
/// k = SHA-512(dom2 || R || A || M).
fn ed25519ctx_verify(key: &[u8; 32], data: &[u8], sig: &[u8; 64], context: &[u8]) -> bool {
    if context.is_empty() || context.len() > 255 {
        return false;
    }
    let Some(a) = CompressedEdwardsY(*key).decompress() else {
        return false;
    };
    let r: [u8; 32] = sig[..32].try_into().unwrap_or_default();
    let s: [u8; 32] = sig[32..].try_into().unwrap_or_default();
    let Some(s) = Option::<Scalar>::from(Scalar::from_canonical_bytes(s)) else {
        return false;
    };
    let mut h = Sha512::new();
    h.update(b"SigEd25519 no Ed25519 collisions");
    h.update([0]);
    h.update([context.len() as u8]);
    h.update(context);
    h.update(r);
    h.update(key);
    h.update(data);
    let k = Scalar::from_bytes_mod_order_wide(&h.finalize().into());
    let check = EdwardsPoint::vartime_double_scalar_mul_basepoint(&k, &-a, &s);
    check.compress().to_bytes() == r
}

#[derive(Deserialize)]
struct Envelope {
    payload: String,
    sha256: String,
    signatures: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "keyId")]
    key_id: String,
    alg: String,
    sig: String,
}

/// The payload `envelope` carries, if it carries a valid signature of each
/// algorithm under `context`, by a key of `keys`. Nothing of the payload is
/// to be read before.
pub fn open(keys: &PublicKeys, envelope: &[u8], context: &str) -> Result<Vec<u8>, SignatureError> {
    let env: Envelope = serde_json::from_slice(envelope).map_err(|_| invalid("malformed"))?;
    if env.signatures.len() > MOST_SIGNATURES {
        return Err(invalid("too many signatures"));
    }
    let data = STANDARD
        .decode(&env.payload)
        .ok()
        .filter(|d| !d.is_empty())
        .ok_or_else(|| invalid("malformed payload"))?;
    let sum: String = Sha256::digest(&data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if sum != env.sha256 {
        return Err(invalid("the sha256 does not match"));
    }
    let mut satisfied = vec![];
    for e in &env.signatures {
        let Some(key) = keys.get(&e.key_id) else {
            continue;
        };
        if e.alg != key.algorithm() {
            return Err(invalid(format!(
                "key {} is {}, the signature says {}",
                e.key_id,
                key.algorithm(),
                e.alg
            )));
        }
        let sig = STANDARD.decode(&e.sig).unwrap_or_default();
        if !key.verify(&data, &sig, context.as_bytes()) {
            return Err(invalid(format!(
                "the signature by {} does not verify",
                e.key_id
            )));
        }
        satisfied.push(key.algorithm());
    }
    for alg in [ED25519, ML_DSA_65] {
        if !satisfied.contains(&alg) {
            return Err(invalid(format!(
                "no valid {alg} signature by a trusted key"
            )));
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: &str = "shellwing-release-v1";

    const TEST_ED: &str = include_str!("../testdata/test-ed.pub.pem");
    const TEST_ML: &str = include_str!("../testdata/test-ml.pub.pem");

    fn test_keys() -> PublicKeys {
        parse_keys(&[("test-ed", TEST_ED), ("test-ml", TEST_ML)]).expect("keys")
    }

    #[test]
    fn a_list_signed_by_the_tool_opens() {
        let data = open(
            &test_keys(),
            include_bytes!("../testdata/release.signed.json"),
            CONTEXT,
        )
        .expect("open");
        assert_eq!(data, include_bytes!("../testdata/release.json"));
    }

    #[test]
    fn another_purpose_another_key_or_a_change_is_refused() {
        let signed = include_str!("../testdata/release.signed.json");
        // Opened for another purpose.
        assert!(open(&test_keys(), signed.as_bytes(), "another-purpose-v1").is_err());
        // By keys this app does not know: none is trusted.
        let others = parse_keys(&[("other-ed", TEST_ED), ("other-ml", TEST_ML)]).expect("keys");
        assert!(matches!(
            open(&others, signed.as_bytes(), CONTEXT),
            Err(SignatureError::Invalid(why)) if why.contains("no valid")
        ));
        // The payload changed: its hash, then its signatures, no longer hold.
        let mut env: serde_json::Value = serde_json::from_str(signed).expect("json");
        let other = STANDARD.encode(b"{\"channels\": {}}");
        env["payload"] = other.into();
        assert!(open(&test_keys(), env.to_string().as_bytes(), CONTEXT).is_err());
        let sum: String = Sha256::digest(b"{\"channels\": {}}")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        env["sha256"] = sum.into();
        assert!(matches!(
            open(&test_keys(), env.to_string().as_bytes(), CONTEXT),
            Err(SignatureError::Invalid(why)) if why.contains("does not verify")
        ));
        // One algorithm alone is not enough.
        let mut env: serde_json::Value = serde_json::from_str(signed).expect("json");
        env["signatures"].as_array_mut().expect("list").pop();
        assert!(open(&test_keys(), env.to_string().as_bytes(), CONTEXT).is_err());
    }

    #[test]
    fn the_keys_built_in_read_and_both_algorithms_are_needed() {
        // The real ones: a build without them would never tell of an update.
        assert!(crate::has_keys());
        assert!(crate::trusted_keys().is_ok());
        assert!(matches!(
            parse_keys(&[("test-ed", TEST_ED)]),
            Err(SignatureError::Key(why)) if why.contains("ml-dsa-65")
        ));
    }
}

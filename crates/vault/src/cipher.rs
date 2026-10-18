//! Authenticated encryption, tagged with the algorithm used.
//!
//! A sealed value is `[algorithm id][nonce][ciphertext + tag]`, so values sealed
//! with different algorithms can sit side by side and a new one can be added
//! without guessing what old data is.

use aes_gcm::Aes256Gcm;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use zeroize::Zeroizing;

use crate::{Error, KEY_LEN, Result, random};

/// The algorithms a value may be sealed with. The ids are stored and never reused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cipher {
    /// The default: a 192-bit nonce makes random nonces safe however many
    /// devices write, and it is fast without AES hardware.
    #[default]
    XChaCha20Poly1305,
    /// For where FIPS-approved algorithms are required. Random 96-bit nonces
    /// are fine for the few thousand values a vault seals under one key.
    Aes256Gcm,
}

impl Cipher {
    pub const ALL: [Cipher; 2] = [Cipher::XChaCha20Poly1305, Cipher::Aes256Gcm];

    pub fn id(self) -> u8 {
        match self {
            Cipher::XChaCha20Poly1305 => 1,
            Cipher::Aes256Gcm => 2,
        }
    }

    pub fn from_id(id: u8) -> Option<Cipher> {
        Cipher::ALL.into_iter().find(|c| c.id() == id)
    }

    fn nonce_len(self) -> usize {
        match self {
            Cipher::XChaCha20Poly1305 => 24,
            Cipher::Aes256Gcm => 12,
        }
    }

    fn encrypt(self, key: &[u8; KEY_LEN], nonce: &[u8], p: Payload) -> Option<Vec<u8>> {
        match self {
            Cipher::XChaCha20Poly1305 => XChaCha20Poly1305::new(key.into())
                .encrypt(nonce.into(), p)
                .ok(),
            Cipher::Aes256Gcm => Aes256Gcm::new(key.into()).encrypt(nonce.into(), p).ok(),
        }
    }

    fn decrypt(self, key: &[u8; KEY_LEN], nonce: &[u8], p: Payload) -> Option<Vec<u8>> {
        match self {
            Cipher::XChaCha20Poly1305 => XChaCha20Poly1305::new(key.into())
                .decrypt(nonce.into(), p)
                .ok(),
            Cipher::Aes256Gcm => Aes256Gcm::new(key.into()).decrypt(nonce.into(), p).ok(),
        }
    }
}

pub fn seal(cipher: Cipher, key: &[u8; KEY_LEN], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let nonce: [u8; 24] = random()?;
    let nonce = &nonce[..cipher.nonce_len()];
    let sealed = cipher
        .encrypt(key, nonce, Payload { msg: plain, aad })
        .ok_or(Error::Decrypt)?;
    let mut out = Vec::with_capacity(1 + nonce.len() + sealed.len());
    out.push(cipher.id());
    out.extend_from_slice(nonce);
    out.extend(sealed);
    Ok(out)
}

/// What was sealed, and whether it was in the first, untagged format (a nonce
/// and XChaCha20-Poly1305 ciphertext with no id), which callers rewrite.
pub fn open(key: &[u8; KEY_LEN], sealed: &[u8], aad: &[u8]) -> Result<(Zeroizing<Vec<u8>>, bool)> {
    if let Some((&id, rest)) = sealed.split_first() {
        let cipher = Cipher::from_id(id);
        if let Some(c) = cipher
            && rest.len() >= c.nonce_len()
        {
            let (nonce, data) = rest.split_at(c.nonce_len());
            if let Some(p) = c.decrypt(key, nonce, Payload { msg: data, aad }) {
                return Ok((Zeroizing::new(p), false));
            }
        }
    }
    // The untagged format came before the id byte. Its first byte is random, so
    // it may look like an id; the tag check above rejected it, and this decides.
    if sealed.len() >= 24 {
        let (nonce, data) = sealed.split_at(24);
        if let Some(p) = Cipher::XChaCha20Poly1305.decrypt(key, nonce, Payload { msg: data, aad }) {
            return Ok((Zeroizing::new(p), true));
        }
    }
    match sealed.first().and_then(|&id| Cipher::from_id(id)) {
        Some(_) => Err(Error::Decrypt),
        None => Err(Error::UnknownCipher(sealed.first().copied().unwrap_or(0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [9; KEY_LEN];

    #[test]
    fn every_cipher_round_trips_and_is_tagged() {
        for c in Cipher::ALL {
            let sealed = seal(c, &KEY, b"secret", b"aad").expect("seal");
            assert_eq!(sealed[0], c.id());
            let (plain, legacy) = open(&KEY, &sealed, b"aad").expect("open");
            assert_eq!((&plain[..], legacy), (&b"secret"[..], false));
            assert!(matches!(
                open(&KEY, &sealed, b"other aad"),
                Err(Error::Decrypt)
            ));
            assert!(matches!(
                open(&[1; KEY_LEN], &sealed, b"aad"),
                Err(Error::Decrypt)
            ));
        }
    }

    #[test]
    fn the_untagged_first_format_still_opens() {
        let nonce = [7u8; 24];
        let mut old = nonce.to_vec();
        old.extend(
            Cipher::XChaCha20Poly1305
                .encrypt(
                    &KEY,
                    &nonce,
                    Payload {
                        msg: b"old",
                        aad: b"aad",
                    },
                )
                .expect("encrypt"),
        );
        let (plain, legacy) = open(&KEY, &old, b"aad").expect("open");
        assert_eq!((&plain[..], legacy), (&b"old"[..], true));
    }

    #[test]
    fn an_unknown_id_says_so() {
        let mut sealed = seal(Cipher::default(), &KEY, b"x", b"").expect("seal");
        sealed[0] = 200;
        assert!(matches!(
            open(&KEY, &sealed, b""),
            Err(Error::UnknownCipher(200))
        ));
    }
}

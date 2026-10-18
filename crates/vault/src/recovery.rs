//! The recovery key: 32 random bytes shown as 13 groups of 4 characters
//! (Crockford base32), to write down. With it another device opens the synced
//! vault, and the vault survives losing every device.

use std::fmt;

use zeroize::Zeroizing;

use crate::KEY_LEN;

/// Crockford's alphabet: no I, L, O or U, so it reads aloud and copies well.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub struct RecoveryKey(pub(crate) Zeroizing<[u8; KEY_LEN]>);

impl RecoveryKey {
    /// Reads a key as typed: case, spaces and dashes do not matter, and the
    /// letters people confuse (O, I, L) count as the digits they look like.
    pub fn parse(text: &str) -> Option<RecoveryKey> {
        let mut bits: u64 = 0;
        let mut n = 0;
        let mut out = Vec::with_capacity(KEY_LEN);
        for c in text.chars().filter(|c| !c.is_whitespace() && *c != '-') {
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let v = ALPHABET.iter().position(|&a| a as char == c)? as u64;
            bits = (bits << 5) | v;
            n += 5;
            if n >= 8 {
                n -= 8;
                out.push((bits >> n) as u8);
                bits &= (1 << n) - 1;
            }
        }
        // 32 bytes are 256 bits: 52 characters, with 4 bits of padding left.
        if out.len() != KEY_LEN || n != 4 || bits != 0 {
            return None;
        }
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        key.copy_from_slice(&out);
        Some(RecoveryKey(key))
    }
}

impl fmt::Display for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = String::with_capacity(52);
        let (mut bits, mut n) = (0u64, 0);
        for &b in self.0.iter() {
            bits = (bits << 8) | b as u64;
            n += 8;
            while n >= 5 {
                n -= 5;
                chars.push(ALPHABET[((bits >> n) & 31) as usize] as char);
            }
        }
        if n > 0 {
            chars.push(ALPHABET[((bits << (5 - n)) & 31) as usize] as char);
        }
        let groups: Vec<&str> = chars
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap_or_default())
            .collect();
        f.write_str(&groups.join("-"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_forgives_typing() {
        let mut bytes = [0u8; KEY_LEN];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i * 37 + 11) as u8;
        }
        let key = RecoveryKey(Zeroizing::new(bytes));
        let text = key.to_string();
        assert_eq!(text.len(), 52 + 12, "13 groups of 4 with dashes");
        let back = RecoveryKey::parse(&text).expect("parse");
        assert_eq!(*back.0, bytes);
        let sloppy = text.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(*RecoveryKey::parse(&sloppy).expect("sloppy").0, bytes);
        assert!(RecoveryKey::parse(&text[..20]).is_none(), "too short");
        assert!(RecoveryKey::parse("not a key at all").is_none());
    }
}

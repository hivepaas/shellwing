use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// An sRGB color, written `#rrggbb` (or `#rgb`) in the files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("`{s}` is not a color; use #rrggbb");
        let hex = s.trim().strip_prefix('#').ok_or_else(bad)?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
        match hex.len() {
            6 => Ok(Color::new((v >> 16) as u8, (v >> 8) as u8, v as u8)),
            3 => {
                let d = |n: u32| (((v >> n) & 0xf) * 0x11) as u8;
                Ok(Color::new(d(8), d(4), d(0)))
            }
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_long_and_short_forms() {
        assert_eq!("#0b3a4f".parse(), Ok(Color::new(0x0b, 0x3a, 0x4f)));
        assert_eq!("#fa0".parse(), Ok(Color::new(0xff, 0xaa, 0x00)));
        assert!("0b3a4f".parse::<Color>().is_err());
        assert!("#0b3a4".parse::<Color>().is_err());
        assert!("#+b3a4f".parse::<Color>().is_err());
        assert_eq!(Color::new(1, 2, 255).to_string(), "#0102ff");
    }
}

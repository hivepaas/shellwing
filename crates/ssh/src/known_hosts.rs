//! known_hosts, read and edited: the Known hosts page lists and removes
//! entries, and a key that changed can be replaced, as `ssh-keygen -R`
//! followed by accepting the new key would.
//!
//! Lines are kept as they are, comments and markers (`@cert-authority`,
//! `@revoked`) included; only the lines removed go, and the file before the
//! change is kept as `known_hosts.old`, as `ssh-keygen` keeps it.

use std::io;
use std::path::{Path, PathBuf};

use hmac::{Hmac, KeyInit, Mac};
use russh::keys::ssh_key::PublicKey;
use sha1::Sha1;

use crate::keys;

/// One key line of known_hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its line in the file, from 1.
    pub line: usize,
    /// The names as written: `host`, `[host]:port`, a pattern, or a hashed
    /// `|1|salt|hash` that only a name can be checked against.
    pub hosts: Vec<String>,
    /// `@cert-authority` or `@revoked`, when the line has one.
    pub marker: Option<String>,
    pub key: keys::KeyInfo,
}

impl Entry {
    /// Whether every name is hashed: no name can be shown.
    pub fn hashed(&self) -> bool {
        self.hosts.iter().all(|h| h.starts_with("|1|"))
    }

    /// The names that can be shown.
    pub fn names(&self) -> Vec<&str> {
        self.hosts
            .iter()
            .filter(|h| !h.starts_with("|1|"))
            .map(String::as_str)
            .collect()
    }

    /// Whether the line is for `host` on `port`, hashed names included.
    pub fn is_for(&self, host: &str, port: u16) -> bool {
        let name = if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        };
        self.hosts.iter().any(|h| name_matches(h, &name))
    }
}

/// Whether a name of a known_hosts line is `name` (`host` or `[host]:port`).
fn name_matches(written: &str, name: &str) -> bool {
    let Some(hashed) = written.strip_prefix("|1|") else {
        return written.eq_ignore_ascii_case(name);
    };
    let mut parts = hashed.split('|');
    let (Some(salt), Some(hash)) = (parts.next(), parts.next()) else {
        return false;
    };
    let decode = |s: &str| data_encoding::BASE64.decode(s.as_bytes()).ok();
    let (Some(salt), Some(hash)) = (decode(salt), decode(hash)) else {
        return false;
    };
    Hmac::<Sha1>::new_from_slice(&salt)
        .map(|m| m.chain_update(name.as_bytes()).verify_slice(&hash).is_ok())
        .unwrap_or(false)
}

/// Reads one line: `[marker] hosts keytype base64 [comment]`.
fn parse(line: usize, text: &str) -> Option<Entry> {
    let text = text.trim();
    if text.is_empty() || text.starts_with('#') {
        return None;
    }
    let mut fields = text.split_whitespace();
    let mut first = fields.next()?;
    let marker = if first.starts_with('@') {
        let m = first.to_string();
        first = fields.next()?;
        Some(m)
    } else {
        None
    };
    let (kind, data) = (fields.next()?, fields.next()?);
    let public = PublicKey::from_openssh(&format!("{kind} {data}")).ok()?;
    Some(Entry {
        line,
        hosts: first.split(',').map(str::to_string).collect(),
        marker,
        key: keys::info(&public),
    })
}

/// The key lines of the file at `path`; none when it is not there.
pub fn entries(path: &Path) -> io::Result<Vec<Entry>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(text
        .lines()
        .enumerate()
        .filter_map(|(i, l)| parse(i + 1, l))
        .collect())
}

/// Rewrites the file without the lines `drop` (numbered from 1), keeping
/// the old one as `<path>.old`. Returns how many went.
pub fn remove_lines(path: &Path, drop: &[usize]) -> io::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    let mut kept = String::with_capacity(text.len());
    let mut gone = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if drop.contains(&(i + 1)) {
            gone += 1;
        } else {
            kept.push_str(line);
        }
    }
    if gone == 0 {
        return Ok(0);
    }
    let mut old = path.as_os_str().to_owned();
    old.push(".old");
    std::fs::copy(path, PathBuf::from(old))?;
    // Written beside, then moved over it: never half a file.
    let mut new = path.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    std::fs::write(&new, kept)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&new, meta.permissions());
    }
    std::fs::rename(&new, path)?;
    Ok(gone)
}

/// Removes every line for `host` on `port` but markers' (a `@revoked`
/// line stays revoked), as `ssh-keygen -R` does. Returns how many went.
pub fn forget(path: &Path, host: &str, port: u16) -> io::Result<usize> {
    let lines: Vec<usize> = entries(path)?
        .into_iter()
        .filter(|e| e.marker.is_none() && e.is_for(host, port))
        .map(|e| e.line)
        .collect();
    remove_lines(path, &lines)
}

/// The fingerprints known_hosts has for `host` on `port`.
pub fn fingerprints(path: &Path, host: &str, port: u16) -> Vec<String> {
    entries(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.marker.is_none() && e.is_for(host, port))
        .map(|e| e.key.fingerprint)
        .collect()
}

/// Trusts `key` for `host` on `port` in place of what was there: the old
/// lines go, the new key is written.
pub fn replace(path: &Path, host: &str, port: u16, key: &str) -> Result<(), crate::Error> {
    let key = PublicKey::from_openssh(key.trim()).map_err(|e| crate::Error::Key(e.to_string()))?;
    forget(path, host, port).map_err(|e| crate::Error::Key(e.to_string()))?;
    russh::keys::known_hosts::learn_known_hosts_path(host, port, &key, path)?;
    Ok(())
}

/// Where known_hosts is: `$SHELLWING_KNOWN_HOSTS`, else `~/.ssh/known_hosts`.
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("SHELLWING_KNOWN_HOSTS")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed(name: &str, salt: &[u8]) -> String {
        let hash = Hmac::<Sha1>::new_from_slice(salt)
            .expect("hmac")
            .chain_update(name.as_bytes())
            .finalize()
            .into_bytes();
        format!(
            "|1|{}|{}",
            data_encoding::BASE64.encode(salt),
            data_encoding::BASE64.encode(&hash)
        )
    }

    #[test]
    fn entries_are_read_found_and_replaced() {
        let dir = std::env::temp_dir().join(format!("shellwing-known-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("known_hosts");
        let old = keys::generate(keys::KeyKind::Ed25519, "").expect("key");
        let other = keys::generate(keys::KeyKind::Ed25519, "").expect("key");
        let new = keys::generate(keys::KeyKind::Ed25519, "").expect("key");
        let line = |k: &keys::ssh_key::PrivateKey| keys::info(k.public_key()).public;
        let text = format!(
            "# a comment\n\
             api.example.com,10.0.1.11 {}\n\
             {} {}\n\
             [db.example.com]:2222 {}\n\
             @revoked api.example.com {}\n",
            line(&old),
            hashed("api.example.com", b"0123456789abcdefghij"),
            line(&old),
            line(&other),
            line(&other),
        );
        std::fs::write(&path, &text).expect("write");

        let all = entries(&path).expect("entries");
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].line, 2, "comments count as lines");
        assert_eq!(all[0].names(), ["api.example.com", "10.0.1.11"]);
        assert!(all[1].hashed() && all[1].is_for("api.example.com", 22));
        assert!(!all[1].is_for("db.example.com", 22));
        assert!(all[2].is_for("db.example.com", 2222) && !all[2].is_for("db.example.com", 22));
        assert_eq!(all[3].marker.as_deref(), Some("@revoked"));
        assert_eq!(
            fingerprints(&path, "api.example.com", 22),
            vec![all[0].key.fingerprint.clone(); 2]
        );

        replace(&path, "api.example.com", 22, &line(&new)).expect("replace");
        let after = entries(&path).expect("entries");
        assert_eq!(after.len(), 3, "both old lines went, the new one came");
        assert!(
            after
                .iter()
                .any(|e| e.marker.as_deref() == Some("@revoked"))
        );
        assert!(after.iter().any(|e| e.is_for("db.example.com", 2222)));
        assert_eq!(
            fingerprints(&path, "api.example.com", 22),
            [keys::info(new.public_key()).fingerprint]
        );
        let old_file = std::fs::read_to_string(dir.join("known_hosts.old")).expect("old");
        assert_eq!(old_file, text);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .starts_with("# a comment\n")
        );

        assert_eq!(remove_lines(&path, &[999]).expect("none"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

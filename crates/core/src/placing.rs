//! Config files placed on a host over SFTP: written whole (to a file beside
//! the target, then renamed over it), with their permissions from the
//! start, and taken away again. Placed until the tab closes, what was there
//! before is set aside beside it and put back.

use sha2::{Digest, Sha256};
use shellwing_config::PlacedFile;
use shellwing_ssh::Sftp;
use shellwing_vault::limits;

/// A config file placed in a pane's host until its tab closes.
#[derive(Clone, Debug)]
pub struct Placed {
    /// The config file's id in the vault.
    pub id: String,
    pub name: String,
    /// Where it is, in full.
    pub path: String,
    /// Where what was there before was set aside, to put back.
    pub aside: Option<String>,
    pub sha256: String,
}

impl Placed {
    /// As kept in `state/placed.json`, for the host `host`.
    pub fn kept(&self, host: &str) -> PlacedFile {
        PlacedFile {
            host: host.to_string(),
            name: self.name.clone(),
            path: self.path.clone(),
            aside: self.aside.clone(),
            sha256: self.sha256.clone(),
        }
    }
}

/// What is at a path before a file is placed there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum There {
    Nothing,
    /// The same bytes as the file's.
    Same,
    /// Another file.
    Other,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Where what is at `path` goes while a file is placed there until its
/// tab closes.
pub fn aside_of(path: &str) -> String {
    format!("{path}.shellwing-saved")
}

/// Where what is at `path` goes when it is kept as a backup.
pub fn backup_of(path: &str) -> String {
    format!("{path}.bak")
}

/// SFTP, opened on the pane's connection.
pub async fn open(
    answer: tokio::sync::oneshot::Receiver<Result<Sftp, shellwing_ssh::Error>>,
) -> Result<Sftp, String> {
    match answer.await {
        Ok(Ok(sftp)) => Ok(sftp),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("the connection ended".into()),
    }
}

/// `path` in full (`~/` as the home directory), and what is there now
/// beside `content`.
pub async fn look(sftp: &Sftp, path: &str, content: &[u8]) -> Result<(String, There), String> {
    let full = sftp.expand(path).await.map_err(|e| e.to_string())?;
    // A file past the most a config file holds is not one of ours.
    let there = match sftp.read_file(&full, limits::FILE).await {
        Ok(None) => There::Nothing,
        Ok(Some(b)) if b == content => There::Same,
        Ok(Some(_)) => There::Other,
        Err(e) if e.to_string().contains("not a file") => return Err(e.to_string()),
        Err(_) => There::Other,
    };
    Ok((full, there))
}

/// Writes `content` at `path` (in full), what was there going to `aside`
/// when given.
pub async fn place(
    sftp: &Sftp,
    path: &str,
    content: &[u8],
    mode: u32,
    aside: Option<&str>,
) -> Result<(), String> {
    sftp.place(path, content, mode, aside)
        .await
        .map_err(|e| e.to_string())
}

/// Takes a placed file away, and puts back what it was set aside from.
pub async fn unplace(sftp: &Sftp, path: &str, aside: Option<&str>) -> Result<(), String> {
    sftp.unplace(path, aside).await.map_err(|e| e.to_string())
}

/// A file placed until its tab closes, left there for good: what it set
/// aside becomes its backup (`….bak`). That backup's path, if there is one.
pub async fn keep(sftp: &Sftp, path: &str, aside: Option<&str>) -> Result<Option<String>, String> {
    let Some(aside) = aside else {
        return Ok(None);
    };
    let backup = backup_of(path);
    let moved = sftp
        .keep_aside(aside, &backup)
        .await
        .map_err(|e| e.to_string())?;
    Ok(moved.then_some(backup))
}

/// A file left on the host from before: taken away (what it set aside put
/// back) if it is still as placed; left alone if it was changed since.
/// True when it was taken away.
pub async fn tidy(sftp: &Sftp, left: &PlacedFile) -> Result<bool, String> {
    let now = sftp
        .read_file(&left.path, limits::FILE)
        .await
        .map_err(|e| e.to_string())?;
    match now {
        Some(b) if sha256_hex(&b) == left.sha256 => {
            unplace(sftp, &left.path, left.aside.as_deref()).await?;
            Ok(true)
        }
        // Gone already: what was set aside still comes back.
        None => {
            unplace(sftp, &left.path, left.aside.as_deref()).await?;
            Ok(true)
        }
        Some(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_beside_and_digests() {
        assert_eq!(aside_of("/h/.npmrc"), "/h/.npmrc.shellwing-saved");
        assert_eq!(backup_of("/h/.npmrc"), "/h/.npmrc.bak");
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

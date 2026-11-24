//! What an AI does to a host's files, over SFTP: list a folder, read a file
//! (text as it is, anything else in base64, a large one a part at a time),
//! write one. What each says back is what the agent reads, and what AI
//! activity keeps.

use base64::Engine as _;
use shellwing_ssh::sftp::{self, Entry, Kind};
use shellwing_ssh::{Sftp, WriteHow};

/// A read gives this much at the most at a time; and this, when not told.
pub const MOST_READ: usize = 512 * 1024;
pub const READ: usize = 256 * 1024;
/// A write takes this much at the most.
pub const MOST_WRITE: usize = 8 * 1024 * 1024;

/// Why a write waits for the person, where risky ones do: its waiting
/// reason, as AI activity reads it.
pub const WRITES: &str = "\u{0}writes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOp {
    List {
        path: String,
    },
    Read {
        path: String,
        offset: u64,
        length: usize,
    },
    Write {
        path: String,
        bytes: Vec<u8>,
        how: WriteHow,
    },
}

impl FileOp {
    /// As AI activity lists it.
    pub fn label(&self) -> String {
        match self {
            FileOp::List { path } => format!("list {path}"),
            FileOp::Read {
                path, offset: 0, ..
            } => format!("read {path}"),
            FileOp::Read { path, offset, .. } => format!("read {path} from byte {offset}"),
            FileOp::Write { path, bytes, how } => {
                let how = match how {
                    WriteHow::Create => "new",
                    WriteHow::Overwrite => "overwrite",
                    WriteHow::Append => "append",
                };
                format!("write {path} ({}, {how})", size(bytes.len() as u64))
            }
        }
    }
}

/// `n` bytes, as people say it.
pub fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.),
        n => format!("{:.1} MB", n as f64 / 1024. / 1024.),
    }
}

/// `path` on the host: `~` its home, a relative one from the home.
async fn resolve(sftp: &Sftp, path: &str) -> Result<String, String> {
    let path = path.trim();
    let expanded = sftp.expand(path).await.map_err(|e| e.to_string())?;
    if expanded.starts_with('/') {
        return Ok(expanded);
    }
    let home = sftp.home().await.map_err(|e| e.to_string())?;
    Ok(if expanded.is_empty() || expanded == "." {
        home
    } else {
        sftp::join(&home, &expanded)
    })
}

/// Does `op`; what it says back.
pub async fn serve(sftp: &Sftp, op: FileOp) -> Result<String, String> {
    match op {
        FileOp::List { path } => {
            let path = resolve(sftp, &path).await?;
            let entries = sftp.list(&path).await.map_err(|e| e.to_string())?;
            Ok(listing(&path, &entries))
        }
        FileOp::Read {
            path,
            offset,
            length,
        } => {
            let path = resolve(sftp, &path).await?;
            let (bytes, total) = sftp
                .read_range(&path, offset, length.clamp(1, MOST_READ))
                .await
                .map_err(|e| e.to_string())?;
            Ok(read(&path, offset, &bytes, total))
        }
        FileOp::Write { path, bytes, how } => {
            let path = resolve(sftp, &path).await?;
            sftp.write_file(&path, &bytes, how)
                .await
                .map_err(|e| e.to_string())?;
            Ok(written(&path, &bytes, how))
        }
    }
}

/// A folder's entries, a line each: its kind, its name, its size and when
/// it changed.
pub fn listing(path: &str, entries: &[Entry]) -> String {
    let mut out = format!(
        "{path}: {} {}\n",
        entries.len(),
        if entries.len() == 1 {
            "entry"
        } else {
            "entries"
        }
    );
    for e in entries {
        let (mark, name) = match (e.kind, e.is_dir()) {
            (_, true) => ('d', format!("{}/", e.name)),
            (Kind::Link, false) => ('l', e.name.clone()),
            (Kind::File, _) => ('-', e.name.clone()),
            _ => ('?', e.name.clone()),
        };
        let when = e
            .modified
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let size = if e.is_dir() {
            String::new()
        } else {
            size(e.size)
        };
        out.push_str(&format!("{mark}  {name}  {size}  {when}\n"));
    }
    out
}

/// A part of a file: text as it is, else base64; where the rest begins.
pub fn read(path: &str, offset: u64, bytes: &[u8], total: u64) -> String {
    let end = offset + bytes.len() as u64;
    let text = std::str::from_utf8(bytes)
        .ok()
        .filter(|t| !t.contains('\0'));
    let mut out = format!(
        "{path}: bytes {offset} to {end} of {total} ({})",
        if text.is_some() {
            "text"
        } else {
            "binary, in base64"
        }
    );
    if end < total {
        out.push_str(&format!(
            "; the rest from offset {end}, with read_file's offset"
        ));
    }
    out.push_str("\n\n");
    match text {
        Some(t) => out.push_str(t),
        None => out.push_str(&base64::engine::general_purpose::STANDARD.encode(bytes)),
    }
    out
}

/// A write's report: its first line for the agent; what was written after
/// it, as AI activity keeps it.
pub fn written(path: &str, bytes: &[u8], how: WriteHow) -> String {
    let verb = match how {
        WriteHow::Create => "Made",
        WriteHow::Overwrite => "Wrote",
        WriteHow::Append => "Added",
    };
    let mut out = format!("{verb} {} at {path}.", size(bytes.len() as u64));
    out.push_str("\n\n");
    match std::str::from_utf8(bytes) {
        Ok(t) if !t.contains('\0') => out.push_str(t),
        _ => out.push_str("(binary)"),
    }
    out
}

/// What `content` is, as `encoding` says: `text` (or none), or `base64`.
pub fn decode(content: &str, encoding: Option<&str>) -> Result<Vec<u8>, String> {
    let bytes = match encoding.unwrap_or("text") {
        "text" => content.as_bytes().to_vec(),
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(content.trim())
            .map_err(|e| format!("not base64: {e}"))?,
        other => return Err(format!("encoding is text or base64, not {other}")),
    };
    if bytes.len() > MOST_WRITE {
        return Err(format!(
            "more than {} to write at once",
            size(MOST_WRITE as u64)
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn a_folder_a_line_each() {
        let e = |name: &str, kind, size| Entry {
            name: name.into(),
            kind,
            dir_link: false,
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        };
        let out = listing(
            "/etc/nginx",
            &[
                e("sites-enabled", Kind::Dir, 0),
                e("nginx.conf", Kind::File, 2048),
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "/etc/nginx: 2 entries");
        assert!(lines[1].starts_with("d  sites-enabled/  "), "{out}");
        assert!(
            lines[2].starts_with("-  nginx.conf  2.0 KB  2023-11-"),
            "{out}"
        );
    }

    #[test]
    fn text_as_it_is_the_rest_in_base64() {
        let out = read("/a", 0, b"hello", 12);
        assert!(
            out.starts_with("/a: bytes 0 to 5 of 12 (text); the rest from offset 5"),
            "{out}"
        );
        assert!(out.ends_with("\n\nhello"));
        let out = read("/b", 4, &[0, 159, 146, 150], 8);
        assert!(
            out.contains("(binary, in base64)") && !out.contains("the rest"),
            "{out}"
        );
        assert!(out.ends_with("AJ+Slg=="), "{out}");
    }

    #[test]
    fn what_is_written_and_how() {
        assert_eq!(decode("hi", None).expect("text"), b"hi");
        assert_eq!(
            decode("AJ+Slg==", Some("base64")).expect("b64"),
            [0, 159, 146, 150]
        );
        assert!(decode("x", Some("hex")).is_err());
        let w = written("/etc/x.conf", b"a = 1\n", WriteHow::Overwrite);
        assert_eq!(w.lines().next(), Some("Wrote 6 B at /etc/x.conf."));
        assert_eq!(
            FileOp::Write {
                path: "/x".into(),
                bytes: vec![0; 2048],
                how: WriteHow::Append
            }
            .label(),
            "write /x (2.0 KB, append)"
        );
    }
}

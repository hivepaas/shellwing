//! Files over SFTP, on a channel of its own beside the shell: a slow transfer
//! or a server without SFTP never gets in the terminal's way.
//!
//! Reads and writes are pipelined as OpenSSH's `sftp` does them (64 requests
//! of 32 KiB in flight), so a transfer is not held to one round trip per chunk.
//! Downloads go to a `.part` file that becomes the real one only when complete;
//! uploads write the target directly, as `sftp put` does.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use russh::ChannelMsg;
use russh::client::Handle;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileType;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::Error;
use crate::hostkey::Checker;

/// Seconds a single request may take before the transfer fails.
const REQUEST_TIMEOUT: u64 = 30;
const CHUNK: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Link,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    /// A link that points at a directory.
    pub dir_link: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Entry {
    /// Opens as a directory: one, or a link to one.
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir || self.dir_link
    }
}

/// How far a transfer is, in bytes; read from any thread while it runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    pub fn get(&self) -> (u64, u64) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }
}

/// An SFTP session on the connection. Cheap to clone; closed with the last clone
/// (or with the connection).
#[derive(Clone)]
pub struct Sftp(Arc<SftpSession>);

fn err(e: russh_sftp::client::error::Error) -> Error {
    use russh_sftp::client::error::Error as E;
    Error::Sftp(match e {
        E::Status(s) if !s.error_message.is_empty() => s.error_message,
        e => e.to_string(),
    })
}

fn io(e: std::io::Error) -> Error {
    Error::Sftp(e.to_string())
}

/// `dir/name` with SFTP's slashes.
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The last part of `path`.
pub fn name_of(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// Why `name` cannot name a file, if it cannot.
pub fn bad_name(name: &str) -> Option<&'static str> {
    match name {
        "" => Some("A name is needed."),
        "." | ".." => Some("That name is taken by the folder itself."),
        n if n.contains('/') => Some("A name cannot contain /."),
        n if n.contains('\0') => Some("A name cannot contain NUL."),
        _ => None,
    }
}

/// The directory holding `path`; `/` is its own.
pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

fn kind(t: FileType) -> Kind {
    match t {
        FileType::Dir => Kind::Dir,
        FileType::File => Kind::File,
        FileType::Symlink => Kind::Link,
        FileType::Other => Kind::Other,
    }
}

/// Opens the `sftp` subsystem on a channel of its own.
pub(crate) async fn open(handle: &Handle<Checker>) -> Result<Sftp, Error> {
    let mut channel = handle.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => break,
            Some(ChannelMsg::Failure) | Some(ChannelMsg::Close) | None => {
                return Err(Error::Sftp("the server does not offer SFTP".into()));
            }
            _ => {}
        }
    }
    let config = russh_sftp::client::Config {
        max_concurrent_reads: 64,
        max_concurrent_writes: 64,
        request_timeout_secs: REQUEST_TIMEOUT,
        ..Default::default()
    };
    let session = SftpSession::new_with_config(channel.into_stream(), config)
        .await
        .map_err(err)?;
    Ok(Sftp(Arc::new(session)))
}

/// Removes a half-written download when the transfer fails or is dropped.
struct Partial(Option<PathBuf>);

impl Drop for Partial {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

async fn copy(
    from: &mut (impl AsyncRead + Unpin),
    to: &mut (impl AsyncWrite + Unpin),
    progress: &Progress,
) -> std::io::Result<()> {
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = from.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n]).await?;
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
}

impl Sftp {
    /// The directory the server starts in, normally the home directory.
    pub async fn home(&self) -> Result<String, Error> {
        self.0.canonicalize(".").await.map_err(err)
    }

    /// The entries of `dir`: directories first, then by name.
    pub async fn list(&self, dir: &str) -> Result<Vec<Entry>, Error> {
        let mut out = vec![];
        for e in self.0.read_dir(dir).await.map_err(err)? {
            let meta = e.metadata();
            let mut entry = Entry {
                name: e.file_name(),
                kind: kind(e.file_type()),
                dir_link: false,
                size: meta.len(),
                modified: meta.modified().ok(),
            };
            if entry.kind == Kind::Link {
                // Where it points decides how it opens; a broken link is a file.
                entry.dir_link = self
                    .0
                    .metadata(join(dir, &entry.name))
                    .await
                    .is_ok_and(|m| m.is_dir());
            }
            out.push(entry);
        }
        out.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(out)
    }

    /// What is at `path`, following links; None when nothing is.
    pub async fn stat(&self, path: &str) -> Result<Option<Entry>, Error> {
        use russh_sftp::client::error::Error as E;
        use russh_sftp::protocol::StatusCode;
        match self.0.metadata(path).await {
            Ok(m) => Ok(Some(Entry {
                name: name_of(path).to_string(),
                kind: kind(m.file_type()),
                dir_link: false,
                size: m.len(),
                modified: m.modified().ok(),
            })),
            Err(E::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    /// Makes a directory.
    pub async fn mkdir(&self, path: &str) -> Result<(), Error> {
        if self.stat(path).await?.is_some() {
            return Err(Error::Sftp(format!("{} is already there", name_of(path))));
        }
        self.0.create_dir(path).await.map_err(err)
    }

    /// Renames or moves `from` to `to`, which must not exist.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        if self.lstat(to).await?.is_some() {
            return Err(Error::Sftp(format!("{} is already there", name_of(to))));
        }
        self.0.rename(from, to).await.map_err(err)
    }

    /// Removes a file, a link (not what it points at), or a directory with
    /// everything in it.
    pub async fn remove(&self, path: &str) -> Result<(), Error> {
        let Some(top) = self.lstat(path).await? else {
            return Ok(());
        };
        if top != Kind::Dir {
            return self.0.remove_file(path).await.map_err(err);
        }
        // Depth first: a directory goes once it is empty.
        let mut todo = vec![(path.to_string(), false)];
        while let Some((dir, emptied)) = todo.pop() {
            if emptied {
                self.0.remove_dir(dir.as_str()).await.map_err(err)?;
                continue;
            }
            todo.push((dir.clone(), true));
            for e in self.list(&dir).await? {
                let p = join(&dir, &e.name);
                if e.kind == Kind::Dir {
                    todo.push((p, false));
                } else {
                    self.0.remove_file(p.as_str()).await.map_err(err)?;
                }
            }
        }
        Ok(())
    }

    /// `path` with a leading `~` (or `~/`) as the home directory.
    pub async fn expand(&self, path: &str) -> Result<String, Error> {
        match path.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                let home = self.home().await?;
                Ok(format!("{}{rest}", home.trim_end_matches('/')))
            }
            _ => Ok(path.to_string()),
        }
    }

    /// The bytes of the file at `path`, None when nothing is there; a file
    /// of more than `max` bytes, or not a file, is an error.
    pub async fn read_file(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, Error> {
        let Some(e) = self.stat(path).await? else {
            return Ok(None);
        };
        if e.kind != Kind::File {
            return Err(Error::Sftp(format!("{} is not a file", name_of(path))));
        }
        if e.size > max as u64 {
            return Err(Error::Sftp(format!(
                "{} is {} KB; {} KB at most",
                name_of(path),
                e.size.div_ceil(1024),
                max / 1024
            )));
        }
        self.0.read(path).await.map(Some).map_err(err)
    }

    /// Makes the directories up to `dir` that are missing, `0700`: a place
    /// for secrets.
    async fn make_dirs(&self, dir: &str) -> Result<(), Error> {
        let mut missing = vec![];
        let mut at = dir.trim_end_matches('/').to_string();
        while !at.is_empty() && at != "/" && self.stat(&at).await?.is_none() {
            missing.push(at.clone());
            let up = parent(&at);
            if up == at {
                break;
            }
            at = up;
        }
        for d in missing.into_iter().rev() {
            self.0.create_dir(d.as_str()).await.map_err(err)?;
            self.set_mode(&d, 0o700).await?;
        }
        Ok(())
    }

    async fn set_mode(&self, path: &str, mode: u32) -> Result<(), Error> {
        let attrs = russh_sftp::protocol::FileAttributes {
            permissions: Some(mode),
            ..russh_sftp::protocol::FileAttributes::empty()
        };
        self.0.set_metadata(path, attrs).await.map_err(err)
    }

    /// Writes `data` to the file at `path`, whole or not at all: to a file
    /// beside it first, with `mode` from the start, then renamed over it.
    /// What was there goes to `aside` when given (kept), else is removed.
    /// The directories missing are made, `0700`.
    pub async fn place(
        &self,
        path: &str,
        data: &[u8],
        mode: u32,
        aside: Option<&str>,
    ) -> Result<(), Error> {
        use russh_sftp::protocol::{FileAttributes, OpenFlags};
        let dir = parent(path);
        self.make_dirs(&dir).await?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let tmp = join(&dir, &format!(".{}.shellwing-{stamp:x}", name_of(path)));
        let attrs = FileAttributes {
            permissions: Some(mode),
            ..FileAttributes::empty()
        };
        let written = async {
            let mut f = self
                .0
                .open_with_flags_and_attributes(
                    tmp.as_str(),
                    OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                    attrs,
                )
                .await
                .map_err(err)?;
            f.write_all(data).await.map_err(io)?;
            f.shutdown().await.map_err(io)?;
            // The umask may have taken bits off.
            self.set_mode(&tmp, mode).await
        }
        .await;
        if let Err(e) = written {
            let _ = self.0.remove_file(tmp.as_str()).await;
            return Err(e);
        }
        // SFTP's rename does not write over a file: what is there moves
        // out of the way first, and comes back if the rename fails.
        let there = self.lstat(path).await?.is_some();
        let old = match aside {
            Some(a) => a.to_string(),
            None => format!("{tmp}.old"),
        };
        if there {
            if aside.is_some() && self.lstat(&old).await?.is_some() {
                self.0.remove_file(old.as_str()).await.map_err(err)?;
            }
            if let Err(e) = self.0.rename(path, old.as_str()).await {
                let _ = self.0.remove_file(tmp.as_str()).await;
                return Err(err(e));
            }
        }
        if let Err(e) = self.0.rename(tmp.as_str(), path).await {
            if there {
                let _ = self.0.rename(old.as_str(), path).await;
            }
            let _ = self.0.remove_file(tmp.as_str()).await;
            return Err(err(e));
        }
        if there && aside.is_none() {
            let _ = self.0.remove_file(old.as_str()).await;
        }
        Ok(())
    }

    /// Takes the file at `path` away, and puts back what was set `aside`
    /// when it was placed, if anything.
    pub async fn unplace(&self, path: &str, aside: Option<&str>) -> Result<(), Error> {
        if self.lstat(path).await?.is_some() {
            self.0.remove_file(path).await.map_err(err)?;
        }
        if let Some(a) = aside
            && self.lstat(a).await?.is_some()
        {
            self.0.rename(a, path).await.map_err(err)?;
        }
        Ok(())
    }

    /// A file placed with what was there set `aside`, kept for good: what
    /// was set aside becomes `backup`, over what is there (as placing with
    /// a backup does). False when nothing was set aside any more.
    pub async fn keep_aside(&self, aside: &str, backup: &str) -> Result<bool, Error> {
        if self.lstat(aside).await?.is_none() {
            return Ok(false);
        }
        if self.lstat(backup).await?.is_some() {
            self.0.remove_file(backup).await.map_err(err)?;
        }
        self.0.rename(aside, backup).await.map_err(err)?;
        Ok(true)
    }

    /// The kind of what is at `path` itself (a link is a link); None when nothing is.
    async fn lstat(&self, path: &str) -> Result<Option<Kind>, Error> {
        use russh_sftp::client::error::Error as E;
        use russh_sftp::protocol::StatusCode;
        match self.0.symlink_metadata(path).await {
            Ok(m) => Ok(Some(kind(m.file_type()))),
            Err(E::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(None),
            Err(e) => Err(err(e)),
        }
    }

    /// Downloads a file or a whole directory to `local`. `progress.total` is
    /// known before the first byte moves.
    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        progress: &Progress,
    ) -> Result<(), Error> {
        let top = self
            .stat(remote)
            .await?
            .ok_or_else(|| Error::Sftp(format!("{remote} is gone")))?;
        // Files to fetch, after the directories they go in.
        let mut dirs = vec![];
        let mut files = vec![];
        if top.kind == Kind::Dir {
            let mut todo = vec![(remote.to_string(), local.to_path_buf())];
            while let Some((dir, to)) = todo.pop() {
                for e in self.list(&dir).await? {
                    let (from, into) = (join(&dir, &e.name), to.join(&e.name));
                    match e.kind {
                        Kind::Dir => todo.push((from, into)),
                        Kind::File => files.push((from, into, e.size, e.modified)),
                        // Links to directories could loop; ones to files are copied.
                        Kind::Link if !e.dir_link => {
                            let size = self.stat(&from).await?.map_or(0, |s| s.size);
                            files.push((from, into, size, e.modified));
                        }
                        _ => {}
                    }
                }
                dirs.push(to);
            }
        } else {
            files.push((
                remote.to_string(),
                local.to_path_buf(),
                top.size,
                top.modified,
            ));
        }
        let total = files.iter().map(|f| f.2).sum();
        progress.total.store(total, Ordering::Relaxed);
        for d in &dirs {
            tokio::fs::create_dir_all(d).await.map_err(io)?;
        }
        for (from, to, _, modified) in files {
            self.fetch(&from, &to, modified, progress).await?;
        }
        Ok(())
    }

    async fn fetch(
        &self,
        remote: &str,
        local: &Path,
        modified: Option<SystemTime>,
        progress: &Progress,
    ) -> Result<(), Error> {
        let mut part_name = local.file_name().unwrap_or_default().to_os_string();
        part_name.push(".part");
        let part = local.with_file_name(part_name);
        let mut src = self.0.open(remote).await.map_err(err)?;
        let mut guard = Partial(Some(part.clone()));
        let mut dst = tokio::fs::File::create(&part).await.map_err(io)?;
        copy(&mut src, &mut dst, progress).await.map_err(io)?;
        dst.flush().await.map_err(io)?;
        let dst = dst.into_std().await;
        if let Some(t) = modified {
            let _ = dst.set_modified(t);
        }
        drop(dst);
        tokio::fs::rename(&part, local).await.map_err(io)?;
        guard.0 = None;
        Ok(())
    }

    /// Uploads a file or a whole directory to `remote`, writing over a file
    /// already there and adding to a directory already there.
    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        progress: &Progress,
    ) -> Result<(), Error> {
        let top = tokio::fs::metadata(local).await.map_err(io)?;
        let mut dirs = vec![];
        let mut files = vec![];
        if top.is_dir() {
            let mut todo = vec![(local.to_path_buf(), remote.to_string())];
            while let Some((dir, to)) = todo.pop() {
                let mut read = tokio::fs::read_dir(&dir).await.map_err(io)?;
                while let Some(e) = read.next_entry().await.map_err(io)? {
                    let name = e.file_name().to_string_lossy().into_owned();
                    // Not following links: a link to a directory could loop.
                    let meta = tokio::fs::symlink_metadata(e.path()).await.map_err(io)?;
                    if meta.is_dir() {
                        todo.push((e.path(), join(&to, &name)));
                    } else if meta.is_file() {
                        files.push((e.path(), join(&to, &name), meta.len()));
                    }
                }
                dirs.push(to);
            }
        } else {
            files.push((local.to_path_buf(), remote.to_string(), top.len()));
        }
        let total = files.iter().map(|f| f.2).sum();
        progress.total.store(total, Ordering::Relaxed);
        for d in &dirs {
            if self.stat(d).await?.is_none() {
                self.0.create_dir(d.as_str()).await.map_err(err)?;
            }
        }
        for (from, to, _) in files {
            let mut src = tokio::fs::File::open(&from).await.map_err(io)?;
            let mut dst = self.0.create(to.as_str()).await.map_err(err)?;
            copy(&mut src, &mut dst, progress).await.map_err(io)?;
            // Waits for every write to be acknowledged, then closes.
            dst.shutdown().await.map_err(io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_and_go_up() {
        assert_eq!(join("/home/me", "a.txt"), "/home/me/a.txt");
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(parent("/home/me"), "/home");
        assert_eq!(parent("/home/me/"), "/home");
        assert_eq!(parent("/home"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(name_of("/home/me/a.txt"), "a.txt");
        assert_eq!(name_of("/home/me/"), "me");
    }

    #[test]
    fn names_are_checked() {
        assert!(bad_name("notes.txt").is_none());
        assert!(bad_name(".bashrc").is_none());
        for n in ["", ".", "..", "a/b", "a\0b"] {
            assert!(bad_name(n).is_some(), "{n:?}");
        }
    }
}

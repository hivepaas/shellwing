//! A session's files over SFTP, on a channel of its own beside the shell, as
//! the desktop's Files panel has them: listed, made, renamed, removed,
//! downloaded to the phone and uploaded from it.

use std::path::Path;

use shellwing_ssh::Sftp;
use shellwing_ssh::sftp::{Kind, Progress};

use crate::{MobileError, MobileSession};

/// An entry of a folder on the host.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FileEntry {
    pub name: String,
    /// A folder, or a link to one: it opens.
    pub dir: bool,
    pub link: bool,
    pub size: u64,
    /// Seconds since the epoch, when the server says.
    pub modified: Option<u64>,
}

#[uniffi::export]
impl MobileSession {
    /// The folder SFTP starts in: the home folder, usually.
    pub async fn files_home(&self) -> Result<String, MobileError> {
        let sftp = self.sftp().await?;
        self.keep(sftp.home().await)
    }

    /// The entries of `dir`: folders first, then by name.
    pub async fn files_list(&self, dir: String) -> Result<Vec<FileEntry>, MobileError> {
        let sftp = self.sftp().await?;
        let list = self.keep(sftp.list(&dir).await)?;
        Ok(list
            .into_iter()
            .map(|e| FileEntry {
                dir: e.is_dir(),
                link: e.kind == Kind::Link,
                name: e.name,
                size: e.size,
                modified: e
                    .modified
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
            })
            .collect())
    }

    pub async fn files_mkdir(&self, path: String) -> Result<(), MobileError> {
        let sftp = self.sftp().await?;
        self.keep(sftp.mkdir(&path).await)
    }

    pub async fn files_rename(&self, from: String, to: String) -> Result<(), MobileError> {
        let sftp = self.sftp().await?;
        self.keep(sftp.rename(&from, &to).await)
    }

    /// Removes a file, a link, or a folder with everything in it.
    pub async fn files_remove(&self, path: String) -> Result<(), MobileError> {
        let sftp = self.sftp().await?;
        self.keep(sftp.remove(&path).await)
    }

    /// Downloads `remote` (a file or a whole folder) to `local` on the phone.
    pub async fn files_download(&self, remote: String, local: String) -> Result<(), MobileError> {
        let sftp = self.sftp().await?;
        let progress = Progress::default();
        self.keep(sftp.download(&remote, Path::new(&local), &progress).await)
    }

    /// Uploads `local` (a file or a folder on the phone) to `remote`.
    pub async fn files_upload(&self, local: String, remote: String) -> Result<(), MobileError> {
        let sftp = self.sftp().await?;
        let progress = Progress::default();
        self.keep(sftp.upload(Path::new(&local), &remote, &progress).await)
    }
}

impl MobileSession {
    /// The session's SFTP, opened the first time it is asked for.
    pub(crate) async fn sftp_open(&self) -> Result<Sftp, MobileError> {
        self.sftp().await
    }

    async fn sftp(&self) -> Result<Sftp, MobileError> {
        if let Some(s) = self.sftp.lock().await.clone() {
            return Ok(s);
        }
        let answer = self
            .remote()
            .sftp()
            .ok_or_else(|| MobileError::Failed("Not connected.".into()))?;
        let sftp = answer
            .await
            .map_err(|_| MobileError::Failed("the connection ended".into()))?
            .map_err(MobileError::of)?;
        *self.sftp.lock().await = Some(sftp.clone());
        Ok(sftp)
    }

    /// `done`'s answer; an SFTP that failed is opened again next time (the
    /// connection may have been lost and made again).
    fn keep<T>(&self, done: Result<T, shellwing_ssh::Error>) -> Result<T, MobileError> {
        done.map_err(|e| {
            if let Ok(mut s) = self.sftp.try_lock() {
                *s = None;
            }
            MobileError::of(e)
        })
    }
}

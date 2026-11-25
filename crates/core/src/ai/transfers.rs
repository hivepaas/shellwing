//! Files copied between this computer and a host for an AI agent
//! (docs/design/ai-transfers.md), over SFTP, so their bytes never go through
//! the model: the agent is told the paths, the size and the time it took.
//!
//! A path here is the person's, so an agent reaches only the folders the
//! person gave its client, and never what keeps keys and secrets (`~/.ssh`,
//! the vault…), whatever the folders. A path is resolved (`..` and links
//! followed) before it is checked.
//!
//! A file is written beside its target first and then put in its place, so
//! a copy stopped or broken leaves nothing half-written. A folder is copied
//! only to a path where nothing is, the same way: what is there is never
//! written over or taken away.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use shellwing_config::{AiClient, AiMode};
use shellwing_ssh::WriteHow;
use shellwing_ssh::sftp::{self, Kind, Progress, Sftp};

use super::files::{resolve, size};
use super::{End, Hub, Job};

/// Why a copy waits for the person, where risky ones do: its waiting
/// reason, as AI activity reads it.
pub const COPIES: &str = "\u{0}copies";

/// Which way a copy goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    /// From this computer to the host.
    Up,
    /// From the host to this computer.
    Down,
}

/// A copy as an agent asks for it.
#[derive(Clone, Debug)]
pub struct Copy {
    pub way: Way,
    /// On this computer, as given: full, or from `~`.
    pub local: String,
    /// On the host, as `write_file` takes it.
    pub remote: String,
    /// [`WriteHow::Create`] or [`WriteHow::Overwrite`].
    pub how: WriteHow,
}

/// What an agent never reaches on this computer, whatever the folders: where
/// keys and secrets are kept. `config`: shellwing's own folder, its vault
/// with it.
pub fn never_reached(home: &Path, config: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = [
        ".ssh",
        ".gnupg",
        ".aws",
        ".kube",
        ".docker",
        ".netrc",
        ".git-credentials",
        ".password-store",
        ".local/share/keyrings",
        "Library/Keychains",
        "AppData/Roaming/Microsoft/Credentials",
        "AppData/Roaming/Microsoft/Protect",
    ]
    .iter()
    .map(|p| home.join(p))
    .collect();
    out.extend(config.map(Path::to_path_buf));
    out
}

/// `path` resolved: links followed, `..` gone; for one not there yet, its
/// folder resolved and its name kept.
fn resolved(path: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = path.canonicalize() {
        return Ok(p);
    }
    let name = path
        .file_name()
        .filter(|n| !matches!(Path::new(n).components().next(), Some(Component::ParentDir)))
        .ok_or_else(|| format!("{} is not a file's path", path.display()))?;
    let dir = path
        .parent()
        .ok_or_else(|| format!("{} has no folder", path.display()))?
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(dir.join(name))
}

/// `raw`, a path on this computer an agent gave, if it may reach it: inside
/// one of `folders`, and nothing of `never` in it or around it. `whole`: a
/// folder copied whole, whose insides go too.
pub fn local_path(
    raw: &str,
    home: &Path,
    folders: &[String],
    never: &[PathBuf],
    whole: bool,
) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let given = if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if raw == "~" {
        home.to_path_buf()
    } else {
        PathBuf::from(raw)
    };
    if !given.is_absolute() {
        return Err(format!(
            "{raw}: a path on this computer is a full one, or from ~"
        ));
    }
    let path = resolved(&given)?;
    let inside = folders
        .iter()
        .filter_map(|f| Path::new(f).canonicalize().ok())
        .any(|f| path.starts_with(&f));
    if !inside {
        return Err(format!(
            "{} is in none of the folders the user gave you here: {}",
            path.display(),
            if folders.is_empty() {
                "none yet (they add one in shellwing, under Settings › AI, Local folders)"
                    .to_string()
            } else {
                folders.join(", ")
            }
        ));
    }
    for n in never {
        let n = n.canonicalize().unwrap_or_else(|_| n.clone());
        if path.starts_with(&n) || (whole && n.starts_with(&path)) {
            return Err(format!(
                "{} is never reached: {} keeps keys or secrets",
                path.display(),
                n.display()
            ));
        }
    }
    Ok(path)
}

/// The free space where `path` is, if this system says.
#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    let s = nix::sys::statvfs::statvfs(path).ok()?;
    Some(s.blocks_available() as u64 * s.fragment_size() as u64)
}

#[cfg(not(unix))]
fn free_space(_: &Path) -> Option<u64> {
    None
}

/// The bytes a file holds, or a folder's files together (links not followed).
fn local_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| local_size(&e.path())).sum())
        .unwrap_or(0)
}

/// A copy checked: this computer's path, resolved.
struct Checked {
    way: Way,
    local: PathBuf,
    remote: String,
    how: WriteHow,
}

impl Hub {
    /// What an agent never reaches on this computer, as the app knows it
    /// (its configuration's folder among them).
    pub fn set_never_reached(&self, paths: Vec<PathBuf>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.never = paths;
        }
    }

    /// Copies a file or a folder between this computer and the host
    /// `client` calls `host`, if the grant lets it (writing on the host for
    /// an upload, reading for a download) and the path here is in a folder
    /// given to the client: a job, which waits as a write does.
    pub fn copy(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        c: Copy,
    ) -> Result<Arc<Job>, String> {
        let (g, target) = self.grant_for(client, host)?;
        match c.way {
            Way::Up if !g.grant.write => {
                return Err(format!(
                    "{} is not given to {} to write files",
                    g.name, client.name
                ));
            }
            Way::Down if !g.grant.read => {
                return Err(format!(
                    "{} is not given to {} to read files",
                    g.name, client.name
                ));
            }
            _ => {}
        }
        if !matches!(c.how, WriteHow::Create | WriteHow::Overwrite) {
            return Err("mode is create or overwrite".into());
        }
        let home = dirs::home_dir().ok_or("this computer has no home folder")?;
        let (folders, never) = {
            let inner = self.inner.lock().map_err(|_| "busy".to_string())?;
            let folders = inner
                .state
                .clients
                .iter()
                .find(|x| x.id == client.id)
                .map(|x| x.folders.clone())
                .unwrap_or_default();
            let never = if inner.never.is_empty() {
                never_reached(&home, None)
            } else {
                inner.never.clone()
            };
            (folders, never)
        };
        let whole = c.way == Way::Up;
        let local = local_path(&c.local, &home, &folders, &never, whole)?;
        if c.way == Way::Up && !local.exists() {
            return Err(format!("{} is not there", local.display()));
        }
        let label = match c.way {
            Way::Up => {
                let what = if local.is_dir() { "folder, " } else { "" };
                format!(
                    "upload {} → {} ({what}{})",
                    local.display(),
                    c.remote,
                    size(local_size(&local))
                )
            }
            Way::Down => format!("download {} → {}", c.remote, local.display()),
        };
        let asks = match g.grant.mode {
            _ if self.allowed_before(client, &g.spec, &label) => None,
            AiMode::Free => None,
            AiMode::Ask => Some(String::new()),
            // It adds or replaces files, here or there.
            AiMode::Risky => Some(COPIES.to_string()),
        };
        let (job, slot, answer) = self.begin(client, &g, &label, false, asks)?;
        let options = self.options(target, &job);
        let (hub, running) = (self.clone(), job.clone());
        let client_id = client.id.clone();
        let host_name = g.name.clone();
        let checked = Checked {
            way: c.way,
            local,
            remote: c.remote,
            how: c.how,
        };
        let task = self.rt.spawn(async move {
            if !Hub::wait_answer(&hub, &running, answer, client_id, &label).await {
                return;
            }
            let done = async {
                let conn = Hub::connection(&slot, options).await?;
                let sftp = conn.sftp().await.map_err(|e| e.to_string())?;
                serve(&sftp, checked, &host_name, &running).await
            }
            .await;
            let end = match done {
                Ok(text) => {
                    running.print(text.as_bytes());
                    End::Exited(Some(0))
                }
                Err(why) => End::Failed(why),
            };
            hub.finish(&running, end);
        });
        if let Ok(mut t) = job.task.lock() {
            *t = Some(task.abort_handle());
        }
        Ok(job)
    }
}

/// Says how far a copy is, every few seconds, until it ends.
fn follow(job: &Arc<Job>, progress: &Arc<Progress>) -> Arc<AtomicBool> {
    let done = Arc::new(AtomicBool::new(false));
    let (job, progress, stop) = (job.clone(), progress.clone(), done.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let (done, total) = progress.get();
            job.print(format!("copied {} of {}\n", size(done), size(total)).as_bytes());
        }
    });
    done
}

/// Does the copy; what it says back.
async fn serve(sftp: &Sftp, c: Checked, host: &str, job: &Arc<Job>) -> Result<String, String> {
    let started = Instant::now();
    let progress = Arc::new(Progress::default());
    let stop = follow(job, &progress);
    let done = match c.way {
        Way::Up => upload(sftp, &c, &progress).await,
        Way::Down => download(sftp, &c, &progress).await,
    };
    stop.store(true, Ordering::Relaxed);
    let target = done?;
    let (moved, _) = progress.get();
    let secs = started.elapsed().as_secs();
    Ok(match c.way {
        Way::Up => format!("Uploaded {} to {host}:{target} in {secs} s.\n", size(moved)),
        Way::Down => format!(
            "Downloaded {} from {host} to {target} in {secs} s.\n",
            size(moved)
        ),
    })
}

/// This computer's file or folder to the host: where it went.
async fn upload(sftp: &Sftp, c: &Checked, progress: &Progress) -> Result<String, String> {
    let e = |e: shellwing_ssh::Error| e.to_string();
    let folder = c.local.is_dir();
    let mut target = resolve(sftp, &c.remote).await?;
    let there = sftp.stat(&target).await.map_err(e)?;
    // Into a folder there, as cp puts a file.
    if !folder && there.as_ref().is_some_and(|t| t.is_dir()) {
        let name = c.local.file_name().unwrap_or_default().to_string_lossy();
        target = sftp::join(&target, &name);
    }
    let there = sftp.lstat(&target).await.map_err(e)?;
    if there.is_some() && (folder || c.how == WriteHow::Create) {
        return Err(format!(
            "{target} is already there: {}",
            if folder {
                "a folder is copied to a path where nothing is"
            } else {
                "mode create writes a new file only"
            }
        ));
    }
    if there == Some(Kind::Dir) {
        return Err(format!("{target} is a folder: name the file to write"));
    }
    let part = format!("{target}.shellwing-part");
    sftp.remove(&part).await.map_err(e)?;
    if let Err(why) = sftp.upload(&c.local, &part, progress).await {
        let _ = sftp.remove(&part).await;
        return Err(why.to_string());
    }
    if there.is_some() {
        sftp.remove_file(&target).await.map_err(e)?;
    }
    sftp.rename(&part, &target).await.map_err(e)?;
    Ok(target)
}

/// The host's file or folder to this computer: where it went.
async fn download(sftp: &Sftp, c: &Checked, progress: &Progress) -> Result<String, String> {
    let e = |e: shellwing_ssh::Error| e.to_string();
    let remote = resolve(sftp, &c.remote).await?;
    let top = sftp
        .stat(&remote)
        .await
        .map_err(e)?
        .ok_or_else(|| format!("{remote} is not there"))?;
    let folder = top.is_dir();
    let mut target = c.local.clone();
    // Into a folder here, as cp puts a file.
    if !folder && target.is_dir() {
        target = target.join(sftp::name_of(&remote));
    }
    if target.exists() && (folder || c.how == WriteHow::Create) {
        return Err(format!(
            "{} is already there: {}",
            target.display(),
            if folder {
                "a folder is copied to a path where nothing is"
            } else {
                "mode create writes a new file only"
            }
        ));
    }
    if !folder
        && let Some(free) = target.parent().and_then(free_space)
        && top.size > free
    {
        return Err(format!(
            "{} needs {}, and {} has {} free",
            remote,
            size(top.size),
            target.parent().unwrap_or(&target).display(),
            size(free)
        ));
    }
    if folder {
        // Whole, beside it, then in its place.
        let mut part = target.clone().into_os_string();
        part.push(".shellwing-part");
        let part = PathBuf::from(part);
        let _ = std::fs::remove_dir_all(&part);
        if let Err(why) = sftp.download(&remote, &part, progress).await {
            let _ = std::fs::remove_dir_all(&part);
            return Err(why.to_string());
        }
        std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    } else {
        // Written beside it, then renamed over it (Sftp::download does).
        sftp.download(&remote, &target, progress).await.map_err(e)?;
    }
    Ok(target.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("shellwing-transfers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d.canonicalize().expect("canonical")
    }

    #[test]
    fn only_inside_a_folder_given_and_never_a_secret() {
        let home = scratch("home");
        let project = home.join("project");
        std::fs::create_dir_all(project.join("dist")).expect("dirs");
        std::fs::write(project.join("dist/app.tar.gz"), "x").expect("file");
        std::fs::create_dir_all(home.join(".ssh")).expect(".ssh");
        std::fs::write(home.join(".ssh/id_ed25519"), "key").expect("key");
        let never = never_reached(&home, None);
        let folders = vec![project.display().to_string()];

        let ok = local_path("~/project/dist/app.tar.gz", &home, &folders, &never, false)
            .expect("inside");
        assert_eq!(ok, project.join("dist/app.tar.gz"));
        // Not there yet, for a download: its folder resolved.
        assert!(local_path("~/project/dist/new.log", &home, &folders, &never, false).is_ok());

        let e =
            local_path("~/.ssh/id_ed25519", &home, &folders, &never, false).expect_err("outside");
        assert!(e.contains("in none of the folders"), "{e}");
        // Up and out by `..`.
        let e = local_path(
            "~/project/../.ssh/id_ed25519",
            &home,
            &folders,
            &never,
            false,
        )
        .expect_err("..");
        assert!(e.contains("in none of the folders"), "{e}");
        // Out by a link inside a folder given.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.join(".ssh"), project.join("keys")).expect("link");
            let e = local_path("~/project/keys/id_ed25519", &home, &folders, &never, false)
                .expect_err("link");
            assert!(e.contains("in none of the folders"), "{e}");
        }
        // Home given whole: still never ~/.ssh, nor home copied whole.
        let all = vec![home.display().to_string()];
        let e = local_path("~/.ssh/id_ed25519", &home, &all, &never, false).expect_err("never");
        assert!(e.contains("is never reached"), "{e}");
        let e = local_path("~", &home, &all, &never, true).expect_err("holds .ssh");
        assert!(e.contains("is never reached"), "{e}");

        let e = local_path("project/x", &home, &folders, &never, false).expect_err("relative");
        assert!(e.contains("full one"), "{e}");
        let e = local_path("~/project/x", &home, &[], &never, false).expect_err("no folders");
        assert!(e.contains("none yet"), "{e}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_folder_counts_its_files() {
        let d = scratch("size");
        std::fs::write(d.join("a"), "12345").expect("a");
        std::fs::create_dir_all(d.join("sub")).expect("sub");
        std::fs::write(d.join("sub/b"), "123").expect("b");
        assert_eq!(local_size(&d), 8);
        assert_eq!(local_size(&d.join("a")), 5);
        let _ = std::fs::remove_dir_all(&d);
    }
}

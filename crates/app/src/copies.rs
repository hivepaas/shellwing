//! The local copies of remote files being edited. Each is in a folder of its
//! own, so its name (and the editor's syntax colors) stay as on the server,
//! under the user's cache folder (`~/.cache` and the like), which others
//! cannot write to, unlike /tmp. Beside it a note says what the server has,
//! so that a copy holding changes the server lacks is never thrown away:
//!
//! ```text
//! <cache>/shellwing/edit/<run>/lock         locked while that run of the app lasts
//! <cache>/shellwing/edit/<run>/<n>/<name>   the copy
//! <cache>/shellwing/edit/<run>/<n>.sent     its size and mtime as on the server, and where
//! ```
//!
//! A copy still as its note says holds nothing new: it is removed when its
//! edit ends, or at the next start when its run ended without that (it quit,
//! or crashed). Any other copy an ended run left is moved to the Downloads
//! folder, and the workspace says so.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use shellwing_i18n::{tr, tr_n};
use shellwing_ssh::sftp;

/// A file's size and mtime: what tells one save from another.
pub type Stamp = (u64, SystemTime);

pub fn stamp(p: &Path) -> Option<Stamp> {
    let m = fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

const LOCK: &str = "lock";

fn base() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("shellwing")
        .join("edit")
}

/// This run's folder, and its lock, held for as long as the app runs.
static RUN: Mutex<Option<(PathBuf, File)>> = Mutex::new(None);

fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn run_dir() -> io::Result<PathBuf> {
    let mut run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((dir, _)) = &*run {
        return Ok(dir.clone());
    }
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let dir = base().join(format!("{started:x}-{}", std::process::id()));
    private_dir(&dir)?;
    let lock = File::create(dir.join(LOCK))?;
    // Where the file system cannot lock, a sweep cannot tell either, and
    // leaves the run alone.
    let _ = lock.try_lock();
    *run = Some((dir.clone(), lock));
    Ok(dir)
}

fn nanos(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

/// A note's size and mtime, and the remote path.
fn read_note(note: &Path) -> Option<((u64, u128), String)> {
    let text = fs::read_to_string(note).ok()?;
    let (head, remote) = text.split_once('\n')?;
    let (size, mtime) = head.split_once(' ')?;
    Some((
        (size.parse().ok()?, mtime.parse().ok()?),
        remote.to_string(),
    ))
}

/// Moves `from` into the folder `into`, under a new name when its own is
/// taken; says where it went.
fn move_out(from: &Path, into: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(into)?;
    let name = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let to = crate::files::unique(into, &name, &[]);
    if fs::rename(from, &to).is_err() {
        // Another file system.
        fs::copy(from, &to)?;
    }
    Ok(to)
}

/// The copy of one remote file being edited.
pub struct EditCopy {
    /// Its folder of its own.
    dir: PathBuf,
    /// The copy itself.
    pub path: PathBuf,
    remote: String,
    /// The copy as the server has it, once it does.
    sent: Option<Stamp>,
}

impl EditCopy {
    /// A place for a copy of `remote`, under its name.
    pub fn new(remote: &str) -> io::Result<EditCopy> {
        Self::new_in(&run_dir()?, remote)
    }

    fn new_in(run: &Path, remote: &str) -> io::Result<EditCopy> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = run.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
        private_dir(&dir)?;
        Ok(EditCopy {
            path: dir.join(sftp::name_of(remote)),
            dir,
            remote: remote.to_string(),
            sent: None,
        })
    }

    fn note(&self) -> PathBuf {
        self.dir.with_extension("sent")
    }

    /// Takes `stamp` (the copy when it was read, or last written back) as
    /// what the server has.
    pub fn sent(&mut self, stamp: Option<Stamp>) {
        self.sent = stamp;
        let Some((size, mtime)) = stamp else {
            return;
        };
        // Written beside, then renamed over it, so no crash leaves half a note.
        let new = self.dir.with_extension("sent.new");
        let text = format!("{size} {}\n{}", nanos(mtime), self.remote);
        let _ = fs::write(&new, text).and_then(|()| fs::rename(&new, self.note()));
    }

    /// Whether the copy holds changes the server lacks.
    pub fn unsent(&self) -> bool {
        self.sent
            .is_some_and(|sent| stamp(&self.path).is_some_and(|now| now != sent))
    }

    /// Removes the copy and its note.
    pub fn remove(&self) {
        let _ = fs::remove_dir_all(&self.dir);
        // An editor holding the file can keep the folder (on Windows): then
        // the note stays, for the next start to clean up after.
        if !self.dir.exists() {
            let _ = fs::remove_file(self.note());
        }
    }

    /// Moves the copy to the folder `into`, and removes the rest; says where
    /// it went.
    pub fn keep(&self, into: &Path) -> io::Result<PathBuf> {
        let to = move_out(&self.path, into)?;
        self.remove();
        Ok(to)
    }

    /// The edit is over: the copy goes, unless it holds changes the server
    /// lacks. Those stay, for the next start to move to Downloads.
    pub fn end(&self) {
        if !self.unsent() {
            self.remove();
        }
    }
}

/// A copy an ended run left with changes the server lacks.
pub struct Kept {
    pub remote: String,
    /// Where it was moved to.
    pub path: PathBuf,
}

/// Cleans up after runs that ended (quit, or crashed) with copies still
/// there: removes those as the server has them, and moves the others into
/// `into`. Once per start.
pub fn sweep(into: &Path) -> Vec<Kept> {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) {
        return vec![];
    }
    sweep_in(&base(), into)
}

fn sweep_in(base: &Path, into: &Path) -> Vec<Kept> {
    let mut kept = vec![];
    for run in fs::read_dir(base).into_iter().flatten().flatten() {
        let dir = run.path();
        // A run still going holds its lock (this one's too); one without a
        // lock is not one of these, or is only being made.
        let Ok(lock) = File::open(dir.join(LOCK)) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let copy = entry.path();
            if !copy.is_dir() {
                continue;
            }
            let note = copy.with_extension("sent");
            // No note: never read in full, so nothing of the user's.
            if let Some((sent, remote)) = read_note(&note) {
                let file = copy.join(sftp::name_of(&remote));
                let changed =
                    stamp(&file).is_some_and(|(size, mtime)| (size, nanos(mtime)) != sent);
                if changed {
                    match move_out(&file, into) {
                        Ok(path) => kept.push(Kept { remote, path }),
                        // Left for the next start.
                        Err(_) => continue,
                    }
                }
            }
            let _ = fs::remove_dir_all(&copy);
            if !copy.exists() {
                let _ = fs::remove_file(&note);
            }
        }
        drop(lock);
        // Only the lock and stray notes left: the run is done with.
        let empty = fs::read_dir(&dir)
            .map(|mut d| d.all(|e| e.is_ok_and(|e| !e.path().is_dir())))
            .unwrap_or(false);
        if empty {
            let _ = fs::remove_dir_all(&dir);
        }
    }
    kept
}

/// What to tell the user of the copies a sweep kept.
pub fn kept_notice(kept: &[Kept]) -> Option<String> {
    match kept {
        [] => None,
        [k] => Some(tr!(
            "An edit of {remote} was not written back to the server; the file is kept as {path}.",
            remote = k.remote.as_str(),
            path = k.path.display().to_string()
        )),
        _ => Some(tr_n!(
            kept.len(),
            "{n} edited file was not written back to the server; it is kept in Downloads: {files}.",
            "{n} edited files were not written back to the server; they are kept in Downloads: {files}.",
            files = kept
                .iter()
                .map(|k| k.remote.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellwing-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn ended_run(base: &Path, name: &str) -> PathBuf {
        let run = base.join(name);
        fs::create_dir_all(&run).expect("run");
        File::create(run.join(LOCK)).expect("lock");
        run
    }

    /// A copy as read from the server, then saved with `saved` when Some.
    fn copy_in(run: &Path, remote: &str, saved: Option<&str>) -> EditCopy {
        let mut c = EditCopy::new_in(run, remote).expect("copy");
        fs::write(&c.path, "as on the server").expect("write");
        c.sent(stamp(&c.path));
        if let Some(text) = saved {
            fs::write(&c.path, text).expect("save");
        }
        c
    }

    #[test]
    fn a_copy_knows_when_it_holds_changes() {
        let base = scratch("copies-unsent");
        let run = ended_run(&base, "run");
        let c = copy_in(&run, "/etc/app.conf", None);
        assert!(!c.unsent());
        c.end();
        assert!(!c.path.exists() && !c.note().exists());

        let c = copy_in(&run, "/etc/app.conf", Some("changed here"));
        assert!(c.unsent());
        c.end();
        assert!(c.path.exists(), "an end keeps changes the server lacks");
        let kept = c.keep(&base.join("downloads")).expect("keep");
        assert_eq!(fs::read_to_string(kept).expect("kept"), "changed here");
        assert!(!c.path.exists() && !c.note().exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_sweep_keeps_only_changes() {
        let base = scratch("copies-sweep");
        let into = base.join("downloads");
        let run = ended_run(&base, "ended");
        copy_in(&run, "/srv/clean.txt", None);
        copy_in(&run, "/srv/changed.txt", Some("mine"));
        // Never read in full: no note.
        let partial = EditCopy::new_in(&run, "/srv/partial.bin").expect("copy");
        fs::write(&partial.path, "half").expect("write");

        let going = ended_run(&base, "going");
        let held = File::open(going.join(LOCK)).expect("lock");
        held.try_lock().expect("held");
        let theirs = copy_in(&going, "/srv/theirs.txt", Some("still editing"));

        let kept = sweep_in(&base, &into);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].remote, "/srv/changed.txt");
        assert_eq!(fs::read_to_string(&kept[0].path).expect("kept"), "mine");
        assert!(!run.exists(), "the ended run is gone");
        assert!(theirs.path.exists(), "a run still going is left alone");
        drop(held);
        let _ = fs::remove_dir_all(&base);
    }
}

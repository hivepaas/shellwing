//! Copies of notes for an editor of the user's own: plain text on this
//! computer's disk while it is open, so kept short-lived. Each run of the
//! app has a folder of its own, private (0700), holding a lock while it
//! runs; the copies go when the note's window is done with them, and a
//! later run removes those a run left behind (it crashed, or was killed).
//!
//! ```text
//! <cache>/shellwing/notes/<run>/lock
//! <cache>/shellwing/notes/<run>/<n>/<title>.md
//! ```

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCK: &str = "lock";

fn base() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("shellwing")
        .join("notes")
}

/// This run's folder, and its lock, held for as long as the app runs.
static RUN: Mutex<Option<(PathBuf, File)>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(0);

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
    let _ = lock.try_lock();
    *run = Some((dir.clone(), lock));
    Ok(dir)
}

/// A name for the file: the title without what a file name cannot hold.
fn file_name(title: &str, ext: &str) -> String {
    let mut name: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    name = name.trim().trim_matches('.').chars().take(80).collect();
    if name.is_empty() {
        name = "note".into();
    }
    format!("{name}.{ext}")
}

/// Writes `text` to a new copy named after `title`, readable by this user
/// alone.
pub fn write(title: &str, ext: &str, text: &str) -> io::Result<PathBuf> {
    let dir = run_dir()?.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    private_dir(&dir)?;
    let path = dir.join(file_name(title, ext));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(&path)?.write_all(text.as_bytes())?;
    Ok(path)
}

/// Removes a copy, and the folder it is alone in.
pub fn remove(path: &Path) {
    let _ = fs::remove_file(path);
    if let Some(dir) = path.parent() {
        let _ = fs::remove_dir(dir);
    }
}

/// Removes the copies runs that ended left behind; a run still going
/// holds its lock, and keeps them.
pub fn sweep() {
    sweep_in(&base());
}

fn sweep_in(base: &Path) {
    let Ok(runs) = fs::read_dir(base) else {
        return;
    };
    for run in runs.flatten() {
        let dir = run.path();
        let ended = File::options()
            .write(true)
            .open(dir.join(LOCK))
            .is_ok_and(|f| f.try_lock().is_ok());
        if ended {
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_hold_what_files_can() {
        assert_eq!(file_name("Runbook: app/db", "md"), "Runbook- app-db.md");
        assert_eq!(file_name("  ..  ", "txt"), "note.txt");
        assert_eq!(file_name("", "md"), "note.md");
    }

    #[test]
    fn a_sweep_takes_ended_runs_only() {
        let base = std::env::temp_dir().join(format!("shellwing-notes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (ended, live) = (base.join("a"), base.join("b"));
        for d in [&ended, &live] {
            fs::create_dir_all(d.join("0")).expect("dir");
            fs::write(d.join("0").join("n.md"), "secret").expect("copy");
        }
        File::create(ended.join(LOCK)).expect("lock");
        let held = File::create(live.join(LOCK)).expect("lock");
        held.try_lock().expect("held");
        sweep_in(&base);
        assert!(!ended.exists(), "an ended run's copies go");
        assert!(live.join("0").join("n.md").exists(), "a live run's stay");
        drop(held);
        let _ = fs::remove_dir_all(&base);
    }
}

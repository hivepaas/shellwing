//! AI activity's log on this computer: `ai-activity.jsonl`, a line for each
//! command that ended, and the parts it was before (`ai-activity.N.jsonl`,
//! the higher N the newer). A part is about [`PART`] long; once they are
//! more than the person chose, the oldest part goes whole. Nothing is ever
//! written again, so a large log costs no more than a small one: only the
//! last [`ACTIVITY`] commands are read back.

use std::path::{Path, PathBuf};

use super::{ACTIVITY, Activity};

/// How long a part of the log grows before a new one starts.
pub const PART: u64 = 20 * 1024 * 1024;

/// The least the person may keep, in MB.
pub const LEAST_MB: u32 = 32;

pub struct Log {
    /// The part written to: `ai-activity.jsonl`.
    path: PathBuf,
    /// How long a part grows.
    part: u64,
    /// How long the part written to is.
    now: u64,
    /// The parts before it, oldest first: their number and length.
    before: Vec<(u64, u64)>,
    /// The most all the parts may be.
    most: u64,
}

impl Log {
    /// The log at `path`, and the last [`ACTIVITY`] commands in it, oldest
    /// first. It keeps everything until [`Log::set_most`] says otherwise.
    pub fn open(path: PathBuf) -> (Log, Vec<Activity>) {
        Log::open_parts(path, PART)
    }

    fn open_parts(path: PathBuf, part: u64) -> (Log, Vec<Activity>) {
        let mut log = Log {
            now: std::fs::metadata(&path).map_or(0, |m| m.len()),
            before: vec![],
            path,
            part,
            most: u64::MAX,
        };
        log.before = log.find_parts();
        let mut newest_first: Vec<Activity> = vec![];
        let files = std::iter::once(log.path.clone())
            .chain(log.before.iter().rev().map(|(n, _)| log.part_path(*n)));
        for file in files {
            if newest_first.len() >= ACTIVITY {
                break;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let lines: Vec<Activity> = text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            newest_first.extend(lines.into_iter().rev());
        }
        newest_first.truncate(ACTIVITY);
        newest_first.reverse();
        (log, newest_first)
    }

    /// The parts before the one written to, found beside it.
    fn find_parts(&self) -> Vec<(u64, u64)> {
        let (Some(dir), Some(stem)) = (
            self.path.parent(),
            self.path.file_stem().and_then(|s| s.to_str()),
        ) else {
            return vec![];
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return vec![];
        };
        let mut parts: Vec<(u64, u64)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let n = name
                    .strip_prefix(stem)?
                    .strip_prefix('.')?
                    .strip_suffix(".jsonl")?
                    .parse()
                    .ok()?;
                Some((n, e.metadata().ok()?.len()))
            })
            .collect();
        parts.sort();
        parts
    }

    fn part_path(&self, n: u64) -> PathBuf {
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ai-activity");
        self.path.with_file_name(format!("{stem}.{n}.jsonl"))
    }

    /// The part written to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How long all the parts are.
    pub fn size(&self) -> u64 {
        self.now + self.before.iter().map(|(_, len)| len).sum::<u64>()
    }

    pub fn most(&self) -> u64 {
        self.most
    }

    /// The most all the parts may be: the oldest go at once if they are
    /// more.
    pub fn set_most(&mut self, most: u64) {
        self.most = most;
        self.let_go();
    }

    /// A command that ended, as its line. A part that would grow past
    /// [`PART`] is put by first, under the next number.
    pub fn add(&mut self, line: &str) {
        let n = line.len() as u64 + 1;
        if self.now > 0 && self.now + n > self.part {
            let next = self.before.last().map_or(1, |(k, _)| k + 1);
            if std::fs::rename(&self.path, self.part_path(next)).is_ok() {
                self.before.push((next, self.now));
                self.now = 0;
            }
        }
        if append(&self.path, line) {
            self.now += n;
        }
        self.let_go();
    }

    /// The person's Clear: every part goes.
    pub fn clear(&mut self) {
        for (n, _) in std::mem::take(&mut self.before) {
            let _ = std::fs::remove_file(self.part_path(n));
        }
        let _ = std::fs::remove_file(&self.path);
        self.now = 0;
    }

    /// The oldest parts go, whole, until all fit.
    fn let_go(&mut self) {
        while self.size() > self.most && !self.before.is_empty() {
            let (n, _) = self.before.remove(0);
            let _ = std::fs::remove_file(self.part_path(n));
        }
    }
}

/// Adds `line` to the file at `path`, made for its owner alone.
fn append(path: &Path, line: &str) -> bool {
    use std::io::Write;
    let mut open = std::fs::OpenOptions::new();
    open.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    open.open(path)
        .and_then(|mut f| writeln!(f, "{line}"))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::End;
    use super::*;

    fn command(id: usize, output: usize) -> String {
        serde_json::to_string(&Activity {
            id: format!("j-{id}"),
            client: "c".into(),
            host: "h".into(),
            command: "true".into(),
            started: 0,
            ended: Some(0),
            end: Some(End::Exited(Some(0))),
            output: "x".repeat(output),
            waiting: None,
            adding: None,
        })
        .expect("json")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-ai-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// The files in `dir`, by name, and how long they are on the disk.
    fn files(dir: &Path) -> Vec<(String, u64)> {
        let mut all: Vec<(String, u64)> = std::fs::read_dir(dir)
            .expect("dir")
            .flatten()
            .map(|e| {
                let name = e.file_name().into_string().expect("name");
                (name, e.metadata().expect("len").len())
            })
            .collect();
        all.sort();
        all
    }

    #[test]
    fn parts_follow_and_the_oldest_go_whole() {
        let dir = scratch("parts");
        let (mut log, _) = Log::open_parts(dir.join("ai-activity.jsonl"), 1000);
        log.set_most(3500);
        for i in 0..60 {
            log.add(&command(i, 100));
            let on_disk: u64 = files(&dir).iter().map(|(_, len)| len).sum();
            assert!(on_disk <= 3500, "{on_disk} bytes after {i}");
            assert_eq!(on_disk, log.size(), "after {i}");
        }
        let names: Vec<String> = files(&dir).into_iter().map(|(n, _)| n).collect();
        assert!(
            names.contains(&"ai-activity.jsonl".to_string()),
            "{names:?}"
        );
        assert!(
            !names.contains(&"ai-activity.1.jsonl".to_string()),
            "the oldest went: {names:?}"
        );
        assert!(names.len() >= 3, "parts kept: {names:?}");
        for (name, len) in files(&dir) {
            assert!(len <= 1000, "{name} is {len}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opened_again_it_reads_the_last_ones_only() {
        let dir = scratch("again");
        let path = dir.join("ai-activity.jsonl");
        let (mut log, _) = Log::open_parts(path.clone(), 4000);
        for i in 0..1500 {
            log.add(&command(i, 0));
        }
        let parts = files(&dir).len();
        assert!(parts > 10, "{parts} parts");
        let (log, read) = Log::open_parts(path, 4000);
        assert_eq!(read.len(), ACTIVITY);
        assert_eq!(read.first().map(|a| a.id.as_str()), Some("j-500"));
        assert_eq!(read.last().map(|a| a.id.as_str()), Some("j-1499"));
        let on_disk: u64 = files(&dir).iter().map(|(_, len)| len).sum();
        assert_eq!(log.size(), on_disk, "every part counted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn less_room_lets_the_oldest_go_at_once() {
        let dir = scratch("less");
        let (mut log, _) = Log::open_parts(dir.join("ai-activity.jsonl"), 1000);
        for i in 0..100 {
            log.add(&command(i, 100));
        }
        assert!(log.size() > 10_000);
        log.set_most(2500);
        let on_disk: u64 = files(&dir).iter().map(|(_, len)| len).sum();
        assert!(on_disk <= 2500, "{on_disk}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_log_from_before_is_put_by_as_a_part() {
        let dir = scratch("before");
        let path = dir.join("ai-activity.jsonl");
        let lines: String = (0..100).map(|i| command(i, 100) + "\n").collect();
        std::fs::write(&path, &lines).expect("old log");
        let (mut log, read) = Log::open_parts(path, 1000);
        assert_eq!(read.len(), 100, "read back");
        log.set_most(5000);
        log.add(&command(100, 100));
        let names: Vec<String> = files(&dir).into_iter().map(|(n, _)| n).collect();
        assert!(
            !names.contains(&"ai-activity.1.jsonl".to_string()),
            "too long to keep: {names:?}"
        );
        assert!(log.size() <= 5000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_takes_every_part() {
        let dir = scratch("clear");
        let (mut log, _) = Log::open_parts(dir.join("ai-activity.jsonl"), 1000);
        for i in 0..30 {
            log.add(&command(i, 100));
        }
        log.clear();
        assert!(files(&dir).is_empty(), "{:?}", files(&dir));
        assert_eq!(log.size(), 0);
        log.add(&command(30, 10));
        assert_eq!(files(&dir).len(), 1, "kept again after");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

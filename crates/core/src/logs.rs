//! Session logs: the folder they go to, what each file is called, and
//! opening one for a pane's terminal to write (see
//! [`shellwing_term::TermHandle::log`]).
//!
//! A log can hold whatever a session showed, so on Unix only its owner can
//! read it: the folder is made `0700` and each file `0600`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Local;
use shellwing_config::{Log as Settings, LogFormat};
use shellwing_term::log::{self, Log};

/// Where logs go when the settings name no folder: `shellwing/logs` in the
/// user's data folder (`~/.local/share` on Linux, `~/Library/Application
/// Support` on macOS, `%LOCALAPPDATA%` on Windows).
pub fn default_folder() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("shellwing").join("logs"))
}

/// The folder logs go to.
pub fn folder(settings: &Settings) -> Option<PathBuf> {
    settings.folder_path().or_else(default_folder)
}

/// `name` as it can be in a file name on every system.
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.@".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('.');
    if s.is_empty() {
        "session".into()
    } else {
        s.into()
    }
}

/// Starts a log of `name` (a host's name, `local`): a new file in the folder,
/// named for it and the time, as `api-1_2026-10-19_09-12-03.log`. `about`
/// heads a text log.
pub fn open(settings: &Settings, name: &str, about: &str) -> io::Result<(PathBuf, Log)> {
    let dir = folder(settings).ok_or_else(|| io::Error::other("no folder to put logs in"))?;
    make_folder(&dir)?;
    let now = Local::now();
    let stem = format!("{}_{}", file_safe(name), now.format("%Y-%m-%d_%H-%M-%S"));
    let (path, mut file) = create_new(&dir, &stem)?;
    let text = settings.format == LogFormat::Text;
    if text {
        writeln!(
            file,
            "# {about}, logged by shellwing from {}",
            now.format("%Y-%m-%d %H:%M:%S %:z")
        )?;
    }
    let stamp: Option<log::StampFn> = (text && settings.timestamps).then(|| {
        Box::new(|| Local::now().format("[%Y-%m-%d %H:%M:%S] ").to_string()) as log::StampFn
    });
    let format = if text {
        log::LogFormat::Text
    } else {
        log::LogFormat::Raw
    };
    Ok((path, Log::new(Box::new(file), format, stamp)))
}

/// A file no other log has: `stem.log`, else `stem-2.log`, and on.
fn create_new(dir: &Path, stem: &str) -> io::Result<(PathBuf, File)> {
    for n in 1..100 {
        let path = dir.join(if n == 1 {
            format!("{stem}.log")
        } else {
            format!("{stem}-{n}.log")
        });
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many logs of that name this second"))
}

/// Makes the folder logs go to, if it is not there, only its owner's.
pub fn make_folder(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_are_new_files_named_for_the_host_and_the_time() {
        let dir = std::env::temp_dir().join(format!("shellwing-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let settings = Settings {
            folder: Some(dir.display().to_string()),
            ..Settings::default()
        };
        let (path, mut log) = open(&settings, "prod/db 1", "prod/db 1 (ops@db:22)").expect("open");
        let file = path.file_name().and_then(|f| f.to_str()).expect("name");
        assert!(
            file.starts_with("prod_db_1_20") && file.ends_with(".log"),
            "{file}"
        );
        log.write(b"\x1b[1mhello\x1b[0m\r\n").expect("write");
        log.finish().expect("finish");
        let text = fs::read_to_string(&path).expect("read");
        let mut lines = text.lines();
        assert!(
            lines.next().is_some_and(
                |l| l.starts_with("# prod/db 1 (ops@db:22), logged by shellwing from ")
            )
        );
        let line = lines.next().expect("line");
        assert!(
            line.starts_with("[20") && line.ends_with("] hello"),
            "{line}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
            assert_eq!((mode(&dir), mode(&path)), (0o700, 0o600));
        }

        // The same name in the same second gets a file of its own.
        let (a, _) = create_new(&dir, "same").expect("first");
        let (b, _) = create_new(&dir, "same").expect("second");
        assert_eq!(b.file_name(), Some("same-2.log".as_ref()));
        assert_ne!(a, b);

        // A raw log has no heading: it replays as it came.
        let raw = Settings {
            format: LogFormat::Raw,
            ..settings
        };
        let (path, mut log) = open(&raw, "local", "zsh").expect("open raw");
        log.write(b"\x1b[31mred\x1b[0m\r\n").expect("write");
        log.finish().expect("finish");
        assert_eq!(fs::read(&path).expect("read"), b"\x1b[31mred\x1b[0m\r\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_made_safe_for_files() {
        assert_eq!(file_safe("api-1"), "api-1");
        assert_eq!(file_safe("deploy@10.0.1.5"), "deploy@10.0.1.5");
        assert_eq!(file_safe("a:b\\c"), "a_b_c");
        assert_eq!(file_safe(".."), "session");
    }
}

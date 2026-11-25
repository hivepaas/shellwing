//! What an AI does to a host's files, over SFTP: list a folder, read a file
//! (text as it is, anything else in base64, a large one a part at a time),
//! write one. What each says back is what the agent reads, and what AI
//! activity keeps.

use base64::Engine as _;
use shellwing_ssh::sftp::{self, Entry, Kind};
use shellwing_ssh::{Sftp, WriteHow};

/// A listing gives this many entries at the most at a time; and this, when
/// not told.
pub const MOST_LIST: usize = 1000;
pub const LIST: usize = 200;
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
        /// Only names like it: `*` any run of characters, `?` one.
        pattern: Option<String>,
        offset: usize,
        limit: usize,
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
            FileOp::List {
                path,
                pattern: Some(p),
                ..
            } => format!("list {path} ({p})"),
            FileOp::List { path, .. } => format!("list {path}"),
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
pub(crate) async fn resolve(sftp: &Sftp, path: &str) -> Result<String, String> {
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
        FileOp::List {
            path,
            pattern,
            offset,
            limit,
        } => {
            let path = resolve(sftp, &path).await?;
            let all = sftp.entries(&path).await.map_err(|e| e.to_string())?;
            let mut page = Page::of(all, pattern.as_deref(), offset, limit.clamp(1, MOST_LIST));
            // Only the links shown are asked where they point.
            for e in page.shown.iter_mut().filter(|e| e.kind == Kind::Link) {
                e.dir_link = sftp.points_to_dir(&sftp::join(&path, &e.name)).await;
            }
            Ok(listing(&path, &page))
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
            // The folders it goes in, made first where they are not, as
            // `mkdir -p` would.
            let made = missing_folders(&path, |dir| async move {
                sftp.stat(&dir)
                    .await
                    .map(|e| e.map(|e| e.is_dir()))
                    .map_err(|e| e.to_string())
            })
            .await?;
            for dir in &made {
                sftp.mkdir(dir)
                    .await
                    .map_err(|e| format!("the folder {dir} was not made: {e}"))?;
            }
            if let Err(e) = sftp.write_file(&path, &bytes, how).await {
                // The server's word for it is as plain as "Failure": what
                // is there says why, where it can be looked at.
                let there = sftp.lstat(&path).await;
                let holder = sftp.stat(&sftp::parent(&path)).await;
                let said = e.to_string();
                return Err(match (there, holder) {
                    (Ok(there), Ok(holder)) => {
                        not_written(&path, how, there, holder.map(|h| h.is_dir()), &said)
                    }
                    _ => format!("{path} was not written: {said}"),
                });
            }
            Ok(written(&path, &bytes, how, &made))
        }
    }
}

/// The folders `path` goes in that are not there, outermost first, from
/// what `is_dir` says of each (None: nothing is there); one that is a file
/// is an error.
async fn missing_folders<F, Fut>(path: &str, is_dir: F) -> Result<Vec<String>, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Option<bool>, String>>,
{
    let mut missing = vec![];
    let mut dir = sftp::parent(path);
    loop {
        match is_dir(dir.clone()).await? {
            Some(true) => break,
            Some(false) => return Err(format!("{dir} is not a folder")),
            None if dir == "/" => break,
            None => {
                let up = sftp::parent(&dir);
                missing.push(std::mem::replace(&mut dir, up));
            }
        }
    }
    missing.reverse();
    Ok(missing)
}

/// Why a write to `path` failed, from what is there: the file (`there`),
/// and whether its folder is one (`folder`: None when it is not there).
/// The server's own words (`said`) when neither tells.
fn not_written(
    path: &str,
    how: WriteHow,
    there: Option<Kind>,
    folder: Option<bool>,
    said: &str,
) -> String {
    let parent = sftp::parent(path);
    match (how, there, folder) {
        (WriteHow::Create, Some(_), _) => format!(
            "{path} is there already: mode create makes a new file only; overwrite or append write to one that is there"
        ),
        (_, Some(Kind::Dir), _) => format!("{path} is a folder, not a file"),
        (_, None, None) => format!("the folder {parent} is not there"),
        (_, None, Some(false)) => format!("{parent} is not a folder"),
        _ => format!("{path} was not written: {said}"),
    }
}

/// Whether `name` is like `pattern`: `*` any run of characters, `?` one.
pub fn like(name: &str, pattern: &str) -> bool {
    let (n, p): (Vec<char>, Vec<char>) = (name.chars().collect(), pattern.chars().collect());
    // Where a `*` was last, and where in the name it stood for.
    let (mut i, mut j, mut star, mut from) = (0, 0, None, 0);
    while i < n.len() {
        if j < p.len() && (p[j] == '?' || p[j] == n[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = Some(j);
            from = i;
            j += 1;
        } else if let Some(s) = star {
            j = s + 1;
            from += 1;
            i = from;
        } else {
            return false;
        }
    }
    p[j..].iter().all(|c| *c == '*')
}

/// A folder's entries, the part shown and how many there are.
pub struct Page {
    pub shown: Vec<Entry>,
    /// Those like the pattern (all, with none).
    pub matching: usize,
    pub folders: usize,
    pub files: usize,
    /// All of them, the pattern aside.
    pub total: usize,
    pub pattern: Option<String>,
    pub offset: usize,
}

impl Page {
    /// `limit` of `all` from `offset`, those like `pattern`: folders first,
    /// then by name.
    pub fn of(all: Vec<Entry>, pattern: Option<&str>, offset: usize, limit: usize) -> Page {
        let total = all.len();
        let mut kept: Vec<Entry> = all
            .into_iter()
            .filter(|e| pattern.is_none_or(|p| like(&e.name, p)))
            .collect();
        kept.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        let folders = kept.iter().filter(|e| e.is_dir()).count();
        let matching = kept.len();
        let shown = kept.into_iter().skip(offset).take(limit).collect();
        Page {
            shown,
            matching,
            folders,
            files: matching - folders,
            total,
            pattern: pattern.map(str::to_string),
            offset,
        }
    }
}

/// A folder's entries, a line each: its kind, its name, its size and when
/// it changed; first, how many there are, and where the rest begins.
pub fn listing(path: &str, page: &Page) -> String {
    let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    let mut out = match &page.pattern {
        Some(p) => format!(
            "{path}: {} like {p} ({}, {}), of {}",
            n(page.matching, "entry", "entries"),
            n(page.folders, "folder", "folders"),
            n(page.files, "file", "files"),
            page.total
        ),
        None => format!(
            "{path}: {} ({}, {})",
            n(page.matching, "entry", "entries"),
            n(page.folders, "folder", "folders"),
            n(page.files, "file", "files")
        ),
    };
    let end = page.offset + page.shown.len();
    if page.offset > 0 || end < page.matching {
        out.push_str(&format!("; showing {} to {end}", page.offset + 1));
    }
    if end < page.matching {
        out.push_str(&format!("; the rest from offset {end}"));
    }
    out.push('\n');
    for e in &page.shown {
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
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
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
/// What a write says back: what it did, with the folders it made for it
/// (`made`), then what it wrote.
pub fn written(path: &str, bytes: &[u8], how: WriteHow, made: &[String]) -> String {
    let verb = match how {
        WriteHow::Create => "Made",
        WriteHow::Overwrite => "Wrote",
        WriteHow::Append => "Added",
    };
    let mut out = format!("{verb} {} at {path}.", size(bytes.len() as u64));
    match made {
        [] => {}
        [one] => out.push_str(&format!(" It made the folder {one} for it.")),
        many => out.push_str(&format!(" It made the folders {} for it.", many.join(", "))),
    }
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
            "more than {} to write at once: write the rest with mode append, copy a file from this computer with upload (its bytes do not go through you), or make it there with a command",
            size(MOST_WRITE as u64)
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// More than a write takes says where to go instead: append, upload
    /// (its bytes do not go through the model), or a command there. The
    /// tools say so before it is tried, and read_file points to download.
    #[test]
    fn too_much_to_write_says_where_to_go() {
        let why = decode(&"x".repeat(MOST_WRITE + 1), None).expect_err("too much");
        assert!(why.contains("append") && why.contains("upload"), "{why}");
        assert_eq!(
            decode(&"x".repeat(MOST_WRITE), None).map(|b| b.len()),
            Ok(MOST_WRITE)
        );
        let tools = super::super::mcp::tools();
        let said = |name: &str| {
            tools
                .as_array()
                .and_then(|t| t.iter().find(|t| t["name"] == name))
                .and_then(|t| t["description"].as_str())
                .unwrap_or_default()
                .to_string()
        };
        assert!(
            said("write_file").contains("use upload"),
            "{}",
            said("write_file")
        );
        assert!(
            said("read_file").contains("use download"),
            "{}",
            said("read_file")
        );
    }

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
            &Page::of(
                vec![
                    e("nginx.conf", Kind::File, 2048),
                    e("sites-enabled", Kind::Dir, 0),
                ],
                None,
                0,
                LIST,
            ),
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "/etc/nginx: 2 entries (1 folder, 1 file)");
        assert!(lines[1].starts_with("d  sites-enabled/  "), "{out}");
        assert!(
            lines[2].starts_with("-  nginx.conf  2.0 KB  2023-11-") && lines[2].ends_with(" UTC"),
            "{out}"
        );
    }

    /// The folders a write goes in, those not there, outermost first, as
    /// `mkdir -p` makes them; a file on the way stops it.
    #[tokio::test]
    async fn the_folders_a_write_needs() {
        let host = |dirs: &'static [&'static str], files: &'static [&'static str]| {
            move |d: String| async move {
                Ok(if dirs.contains(&d.as_str()) {
                    Some(true)
                } else if files.contains(&d.as_str()) {
                    Some(false)
                } else {
                    None
                })
            }
        };
        let there = host(&["/", "/srv"], &["/srv/notes.txt"]);
        assert_eq!(
            missing_folders("/srv/app/conf/x.toml", there).await,
            Ok(vec!["/srv/app".to_string(), "/srv/app/conf".to_string()])
        );
        assert_eq!(missing_folders("/srv/x.toml", there).await, Ok(vec![]));
        assert_eq!(
            missing_folders("/srv/notes.txt/x", there).await,
            Err("/srv/notes.txt is not a folder".to_string())
        );
        // Nothing at all, not even /: it makes all but /.
        let bare = host(&[], &[]);
        assert_eq!(
            missing_folders("/a/b", bare).await,
            Ok(vec!["/a".to_string()])
        );
        // What cannot be looked at stops it, with why.
        let refused = |_: String| async { Err::<Option<bool>, _>("Permission denied".to_string()) };
        assert_eq!(
            missing_folders("/x/y", refused).await,
            Err("Permission denied".to_string())
        );
    }

    /// A write that failed says why, where the server says only "Failure"
    /// or "No such file".
    #[test]
    fn a_failed_write_says_why() {
        let why =
            |how, there, folder| not_written("/srv/app/x.conf", how, there, folder, "Failure");
        assert_eq!(
            why(WriteHow::Create, Some(Kind::File), Some(true)),
            "/srv/app/x.conf is there already: mode create makes a new file only; overwrite or append write to one that is there"
        );
        assert_eq!(
            why(WriteHow::Overwrite, None, None),
            "the folder /srv/app is not there"
        );
        assert_eq!(
            why(WriteHow::Append, None, Some(false)),
            "/srv/app is not a folder"
        );
        assert_eq!(
            why(WriteHow::Overwrite, Some(Kind::Dir), Some(true)),
            "/srv/app/x.conf is a folder, not a file"
        );
        // Nothing there explains it (a permission, a full disk): the
        // server's words.
        assert_eq!(
            why(WriteHow::Overwrite, Some(Kind::File), Some(true)),
            "/srv/app/x.conf was not written: Failure"
        );
    }

    #[test]
    fn a_large_folder_a_page_at_a_time() {
        let e = |name: String| Entry {
            name,
            kind: Kind::File,
            dir_link: false,
            size: 1,
            modified: None,
        };
        let all: Vec<Entry> = (0..2500)
            .map(|i| e(format!("f{i:04}.log")))
            .chain([e("x.txt".into())])
            .collect();
        let out = listing("/var/log", &Page::of(all.clone(), None, 0, LIST));
        assert_eq!(
            out.lines().next(),
            Some(
                "/var/log: 2501 entries (0 folders, 2501 files); showing 1 to 200; the rest from offset 200"
            )
        );
        assert_eq!(out.lines().count(), 201);
        assert_eq!(
            listing("/var/log", &Page::of(all.clone(), Some("*.txt"), 0, LIST))
                .lines()
                .next(),
            Some("/var/log: 1 entry like *.txt (0 folders, 1 file), of 2501")
        );
        let last = Page::of(all, Some("f24??.log"), 90, LIST);
        assert_eq!((last.matching, last.shown.len()), (100, 10));
        assert!(
            listing("/v", &last)
                .lines()
                .next()
                .expect("head")
                .ends_with("showing 91 to 100")
        );
    }

    #[test]
    fn names_like_a_pattern() {
        assert!(like("nginx.conf", "*.conf"));
        assert!(like("nginx.conf", "ng?nx*"));
        assert!(like("a", "*"));
        assert!(like("abcabd", "*abd"));
        assert!(!like("nginx.conf", "*.log"));
        assert!(!like("ab", "a?b"));
        assert!(like("", "*") && !like("", "?"));
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
        let w = written("/etc/x.conf", b"a = 1\n", WriteHow::Overwrite, &[]);
        assert_eq!(w.lines().next(), Some("Wrote 6 B at /etc/x.conf."));
        let w = written(
            "/srv/a/b/x",
            b"x",
            WriteHow::Create,
            &["/srv/a".into(), "/srv/a/b".into()],
        );
        assert_eq!(
            w.lines().next(),
            Some("Made 1 B at /srv/a/b/x. It made the folders /srv/a, /srv/a/b for it.")
        );
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

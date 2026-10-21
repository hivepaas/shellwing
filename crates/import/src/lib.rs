//! Hosts read from other SSH clients (`docs/design/import.md`): where each app
//! keeps its sessions on this computer, and what is in them. Nothing here
//! writes; the app turns what is found into `hosts.toml`'s hosts.
//!
//! Passwords are never read: each reader skips their fields.

use std::path::{Path, PathBuf};

pub use plan::{Plan, Row, State, plan};

mod csv;
mod mobaxterm;
mod plan;
mod putty;
mod securecrt;
mod ssh_config;
mod tabby;
mod xshell;

/// An app hosts can be read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum App {
    SshConfig,
    Csv,
    Putty,
    Kitty,
    MobaXterm,
    SecureCrt,
    Xshell,
    Tabby,
}

impl App {
    /// As people know it.
    pub fn name(self) -> &'static str {
        match self {
            App::SshConfig => "~/.ssh/config",
            App::Csv => "CSV",
            App::Putty => "PuTTY",
            App::Kitty => "KiTTY",
            App::MobaXterm => "MobaXterm",
            App::SecureCrt => "SecureCRT",
            App::Xshell => "Xshell",
            App::Tabby => "Tabby",
        }
    }

    /// The end of the `imported:<app>` tag imported hosts get.
    pub fn tag(self) -> &'static str {
        match self {
            App::SshConfig => "ssh-config",
            App::Csv => "csv",
            App::Putty => "putty",
            App::Kitty => "kitty",
            App::MobaXterm => "mobaxterm",
            App::SecureCrt => "securecrt",
            App::Xshell => "xshell",
            App::Tabby => "tabby",
        }
    }
}

/// A host found in another app's files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    /// Its folders, outermost first.
    pub folder: Vec<String>,
    pub address: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// A key file, its path as the app wrote it.
    pub key: Option<String>,
    pub forward_agent: bool,
    /// Hosts to go through first, outermost first.
    pub jump: Vec<Jump>,
    /// Forwards as `ssh` writes them: `-L 8080:localhost:80`, `-R …`, `-D 1080`.
    pub forward: Vec<String>,
    /// `#rrggbb`.
    pub color: Option<String>,
    pub description: Option<String>,
    /// What to look at once it is imported.
    pub look: Vec<Look>,
    /// What the app's other hosts call it in their jump hosts (a session
    /// path, an id), besides its name.
    pub known_as: Vec<String>,
}

/// A jump host, as the app names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
    /// Another host of the same app: its name, path or id.
    Host(String),
    /// An address of its own.
    Address {
        address: String,
        port: Option<u16>,
        user: Option<String>,
    },
}

/// Something about a host to look at once it is imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Look {
    /// Its key is PuTTY's `.ppk`, to convert to OpenSSH's format first.
    PpkKey,
    /// Its key is in the app's own key store, under this name.
    KeyInApp(String),
    /// A jump host the app names that is not among its hosts (left out).
    JumpNotFound(String),
}

/// A session left out, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftOut {
    pub name: String,
    pub why: Why,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// Not SSH: the protocol, as the app names it.
    NotSsh(String),
    NoAddress,
}

/// What a source holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub hosts: Vec<Found>,
    pub left_out: Vec<LeftOut>,
}

/// Why a source could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a file of an app hosts can be imported from")]
    Unknown,
    /// Only the app itself opens it (Tabby's encrypted config).
    #[error("encrypted: only {0} can open it")]
    Encrypted(&'static str),
    #[error("{0}")]
    Format(String),
}

/// Where an app keeps its sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    File(PathBuf),
    /// A folder of session files, its subfolders their folders.
    Folder(PathBuf),
    /// A key under `HKEY_CURRENT_USER` (Windows).
    Registry(&'static str),
}

/// A source on this computer, with the number of hosts it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub app: App,
    pub place: Place,
    pub hosts: usize,
}

/// The folders apps keep their files in, on this computer.
#[derive(Clone, Debug, Default)]
pub struct Dirs {
    pub home: PathBuf,
    /// Windows' `%APPDATA%`; macOS' `~/Library/Application Support`.
    pub app_data: Option<PathBuf>,
    /// Windows' Documents.
    pub documents: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, else `~/.config` (Linux).
    pub config: Option<PathBuf>,
}

impl Dirs {
    /// This computer's.
    pub fn here() -> Option<Dirs> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?;
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let (app_data, documents, config) = if cfg!(windows) {
            (env("APPDATA"), Some(home.join("Documents")), None)
        } else if cfg!(target_os = "macos") {
            (Some(home.join("Library/Application Support")), None, None)
        } else {
            let config = env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
            (None, None, Some(config))
        };
        Some(Dirs {
            home,
            app_data,
            documents,
            config,
        })
    }
}

/// The apps installed here that hold hosts, each with how many.
pub fn installed(dirs: &Dirs) -> Vec<Installed> {
    let mut places: Vec<(App, Place)> = vec![];
    let ssh = dirs.home.join(".ssh/config");
    if ssh.is_file() {
        places.push((App::SshConfig, Place::File(ssh)));
    }
    places.extend(putty::places(dirs));
    places.extend(mobaxterm::places(dirs));
    places.extend(securecrt::places(dirs));
    places.extend(xshell::places(dirs));
    places.extend(tabby::places(dirs));
    places
        .into_iter()
        .filter_map(|(app, place)| {
            let hosts = read(app, &place).ok()?.hosts.len();
            (hosts > 0).then_some(Installed { app, place, hosts })
        })
        .collect()
}

/// What `place` holds, read as `app` keeps it.
pub fn read(app: App, place: &Place) -> Result<Read, Error> {
    match (app, place) {
        (App::SshConfig, Place::File(p)) => Ok(ssh_config::read_file(p)),
        (App::Putty | App::Kitty, Place::Folder(p)) => putty::read_folder(app, p),
        (App::Putty | App::Kitty, Place::Registry(key)) => putty::read_registry(app, key),
        (App::SecureCrt, Place::Folder(p)) => securecrt::read_folder(p),
        (App::Xshell, Place::Folder(p)) => xshell::read_folder(p),
        (_, Place::File(p)) => read_bytes(app, &std::fs::read(p)?),
        _ => Err(Error::Unknown),
    }
}

/// What a file chosen by hand holds, its app told from its content.
pub fn read_file(path: &Path) -> Result<(App, Read), Error> {
    if path.is_dir() {
        // A Sessions folder, copied from another computer.
        for (app, read) in [
            (
                App::SecureCrt,
                securecrt::read_folder as fn(&Path) -> Result<Read, Error>,
            ),
            (App::Xshell, xshell::read_folder),
        ] {
            if let Ok(r) = read(path)
                && !(r.hosts.is_empty() && r.left_out.is_empty())
            {
                return Ok((app, r));
            }
        }
        return Err(Error::Unknown);
    }
    let bytes = std::fs::read(path)?;
    let app = identify(path, &bytes).ok_or(Error::Unknown)?;
    Ok((app, read_bytes(app, &bytes)?))
}

/// Which app wrote `bytes`, from its content (and its name, for CSV).
fn identify(path: &Path, bytes: &[u8]) -> Option<App> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Some(App::Xshell);
    }
    let text = text(bytes);
    let head: String = text.chars().take(4096).collect();
    if head.contains("Windows Registry Editor") || head.starts_with("REGEDIT4") {
        return Some(if head.contains("9bis.com\\KiTTY") {
            App::Kitty
        } else {
            App::Putty
        });
    }
    if head.contains("[Bookmarks") {
        return Some(App::MobaXterm);
    }
    if head.contains("<VanDyke") || head.contains("S:\"Hostname\"=") {
        return Some(App::SecureCrt);
    }
    if head.contains("[CONNECTION]") {
        return Some(App::Xshell);
    }
    if head.contains("profiles:") || head.contains("encrypted: true") {
        return Some(App::Tabby);
    }
    let lower = head.to_ascii_lowercase();
    if lower.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("host ") || l.starts_with("host\t") || l.starts_with("hostname ")
    }) {
        return Some(App::SshConfig);
    }
    let csv = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv"));
    (csv || csv::looks_like(&head)).then_some(App::Csv)
}

fn read_bytes(app: App, bytes: &[u8]) -> Result<Read, Error> {
    match app {
        App::SshConfig => Ok(ssh_config::read(&text(bytes), &|_| None)),
        App::Csv => csv::read(&text(bytes)),
        App::Putty | App::Kitty => Ok(putty::read_reg(app, &text(bytes))),
        App::MobaXterm => Ok(mobaxterm::read(&text(bytes))),
        App::SecureCrt => securecrt::read_bytes(bytes),
        App::Xshell => xshell::read_bytes(bytes),
        App::Tabby => tabby::read(&text(bytes)),
    }
}

/// Text in any of the encodings these apps write: UTF-16 (with its byte order
/// mark, or little-endian without, as `.reg` and Xshell files), UTF-8, else
/// GBK when it reads cleanly (a Chinese Windows), else Windows-1252.
pub(crate) fn text(bytes: &[u8]) -> String {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc
            .decode_without_bom_handling(&bytes[bom..])
            .0
            .into_owned();
    }
    let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 4 && odd_zeros * 2 >= bytes.len() / 2 {
        return encoding_rs::UTF_16LE
            .decode_without_bom_handling(bytes)
            .0
            .into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    if let Some(s) = encoding_rs::GBK.decode_without_bom_handling_and_without_replacement(bytes) {
        return s.into_owned();
    }
    encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

/// The sections of an INI file, in order, each with its `key=value` lines
/// (split at the first `=`, both trimmed). Lines before any section are in
/// the one named "".
pub(crate) fn ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![(String::new(), vec![])];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.trim().to_string(), vec![]));
        } else if let Some((k, v)) = line.split_once('=')
            && let Some((_, lines)) = out.last_mut()
        {
            lines.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// `#rrggbb`.
pub(crate) fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `[user@]host[:port]` (an IPv6 address in brackets).
pub(crate) fn address(s: &str) -> Jump {
    let s = s.trim().trim_start_matches("ssh://");
    let (user, rest) = match s.rsplit_once('@') {
        Some((u, r)) => (Some(u.to_string()), r),
        None => (None, s),
    };
    let (host, port) = if let Some(r) = rest.strip_prefix('[') {
        match r.split_once(']') {
            Some((h, p)) => (h, p.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (r, None),
        }
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h, p.parse().ok()),
            _ => (rest, None),
        }
    };
    Jump::Address {
        address: host.to_string(),
        port,
        user,
    }
}

/// Every file under `dir` with extension `ext` (no case), with its folder
/// path below `dir`, in name order.
pub(crate) fn files_in(dir: &Path, ext: &str) -> Vec<(Vec<String>, PathBuf)> {
    fn walk(
        dir: &Path,
        folder: &mut Vec<String>,
        ext: &str,
        out: &mut Vec<(Vec<String>, PathBuf)>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                folder.push(name.unwrap_or_default());
                walk(&path, folder, ext, out);
                folder.pop();
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
            {
                out.push((folder.clone(), path));
            }
        }
    }
    let mut out = vec![];
    walk(dir, &mut vec![], ext, &mut out);
    out
}

#[cfg(test)]
pub(crate) fn utf16le(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        out.extend(u.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_read_in_the_encodings_apps_write() {
        assert_eq!(text("héllo".as_bytes()), "héllo");
        assert_eq!(text(&utf16le("Host=web")), "Host=web");
        // UTF-16LE without its mark, as some .reg files are.
        let bare: Vec<u8> = "Host=web"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(text(&bare), "Host=web");
        let (gbk, _, _) = encoding_rs::GBK.encode("生产服务器");
        assert_eq!(text(&gbk), "生产服务器");
        assert_eq!(text(&[b'c', b'a', b'f', 0xE9]), "café");
    }

    #[test]
    fn addresses_are_split() {
        let a = |address: &str, port: Option<u16>, user: Option<&str>| Jump::Address {
            address: address.into(),
            port,
            user: user.map(str::to_string),
        };
        assert_eq!(
            address("ops@bastion:2222"),
            a("bastion", Some(2222), Some("ops"))
        );
        assert_eq!(address("bastion"), a("bastion", None, None));
        assert_eq!(address("[fe80::1]:22"), a("fe80::1", Some(22), None));
        assert_eq!(address("fe80::1"), a("fe80::1", None, None));
        assert_eq!(address("ssh://me@h:2"), a("h", Some(2), Some("me")));
    }

    #[test]
    fn files_are_told_apart_by_their_content() {
        let id = |name: &str, s: &str| identify(Path::new(name), s.as_bytes());
        assert_eq!(
            id(
                "x.reg",
                "Windows Registry Editor Version 5.00\r\n[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\a]"
            ),
            Some(App::Putty)
        );
        assert_eq!(
            id("x.mxtsessions", "[Bookmarks]\r\nSubRep=\r\n"),
            Some(App::MobaXterm)
        );
        assert_eq!(
            id("config", "Host web\n  HostName 10.0.0.1\n"),
            Some(App::SshConfig)
        );
        assert_eq!(
            id("config.yaml", "version: 3\nprofiles:\n  - type: ssh\n"),
            Some(App::Tabby)
        );
        assert_eq!(id("hosts.csv", "name,host\nweb,10.0.0.1\n"), Some(App::Csv));
        assert_eq!(id("notes.txt", "hello"), None);
    }
}

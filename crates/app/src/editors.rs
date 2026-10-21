//! Text editors: the ones this system has, the system's own for text files,
//! and starting one on a file. The Files panel's Edit uses them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use shellwing_core::command::{which, words};
use shellwing_i18n::tr;
use shellwing_spawn::Unblocked;

/// Editors worth offering, by the name shown and the command that starts one.
#[cfg(target_os = "macos")]
const KNOWN: &[(&str, &str)] = &[
    ("VS Code", "code"),
    ("VSCodium", "codium"),
    ("Zed", "zed"),
    ("Sublime Text", "subl"),
    ("BBEdit", "bbedit"),
    ("TextMate", "mate"),
];
#[cfg(target_os = "windows")]
const KNOWN: &[(&str, &str)] = &[
    ("VS Code", "code"),
    ("VSCodium", "codium"),
    ("Zed", "zed"),
    ("Sublime Text", "subl"),
    ("Notepad++", "notepad++"),
    ("Notepad", "notepad"),
];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const KNOWN: &[(&str, &str)] = &[
    ("VS Code", "code"),
    ("VSCodium", "codium"),
    ("Zed", "zed"),
    ("Sublime Text", "subl"),
    ("GNOME Text Editor", "gnome-text-editor"),
    ("gedit", "gedit"),
    ("Kate", "kate"),
    ("KWrite", "kwrite"),
    ("Mousepad", "mousepad"),
    ("Xed", "xed"),
    ("Pluma", "pluma"),
    ("Geany", "geany"),
];

/// The known editors this system has: (name, command).
pub fn installed() -> Vec<(String, String)> {
    KNOWN
        .iter()
        .filter(|(_, cmd)| which(cmd).is_some())
        .map(|(name, cmd)| (name.to_string(), cmd.to_string()))
        .collect()
}

/// The name of a known editor's command, else its program's file name.
pub fn name_of(cmd: &str) -> String {
    let first = words(cmd).into_iter().next().unwrap_or_default();
    KNOWN
        .iter()
        .find(|(_, c)| *c == first)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| {
            Path::new(&first)
                .file_name()
                .map_or(first.clone(), |f| f.to_string_lossy().into_owned())
        })
}

/// The desktop's app for plain text, as its desktop file id (Linux).
fn text_plain_app() -> Option<String> {
    Command::new("xdg-mime")
        .args(["query", "default", "text/plain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|id| !id.is_empty())
}

/// The `Name=` of a desktop file, looked for where desktops keep them.
fn desktop_name(id: &str) -> Option<String> {
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(home) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/share")))
    {
        dirs.push(home);
    }
    let shared =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(
        shared
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    let text = dirs
        .iter()
        .find_map(|d| std::fs::read_to_string(d.join("applications").join(id)).ok())?;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
            return Some(name.trim().to_string());
        }
    }
    None
}

/// What the system's text editor is called, for menus; worked out once.
pub fn system_name() -> Option<String> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        if cfg!(target_os = "windows") {
            Some("Notepad".into())
        } else if cfg!(target_os = "macos") {
            None
        } else {
            text_plain_app()
                .and_then(|id| desktop_name(&id))
                .or_else(|| installed().first().map(|(n, _)| n.clone()))
        }
    })
    .clone()
}

fn spawn(program: &str, args: &[&std::ffi::OsStr]) -> std::io::Result<()> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Not with the background thread's signals blocked: the editor
        // could not be stopped.
        .spawn_unblocked()
        .map(|_| ())
}

/// Starts `cmd` (the configured editor) on `path`, added at the end.
pub fn open_with(cmd: &str, path: &Path) -> std::io::Result<()> {
    let w = words(cmd);
    let Some((program, rest)) = w.split_first() else {
        return Err(std::io::Error::other(tr!("no editor command")));
    };
    let mut args: Vec<&std::ffi::OsStr> = rest.iter().map(|a| a.as_ref()).collect();
    args.push(path.as_os_str());
    spawn(program, &args)
}

/// Opens `path` in the system's text editor, whatever kind of file it is.
pub fn open_text(path: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "macos") {
        return spawn("open", &["-t".as_ref(), path.as_os_str()]);
    }
    if cfg!(target_os = "windows") {
        return spawn("notepad", &[path.as_os_str()]);
    }
    // The desktop's app for plain text, then any known editor there is.
    if let Some(id) = text_plain_app()
        && which("gtk-launch").is_some()
        && spawn("gtk-launch", &[id.as_ref(), path.as_os_str()]).is_ok()
    {
        return Ok(());
    }
    match installed().first() {
        Some((_, cmd)) => open_with(cmd, path),
        None => Err(std::io::Error::other(tr!("no text editor found"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_commands_have_names() {
        assert_eq!(name_of("code -n"), "VS Code");
        assert_eq!(name_of("/opt/x/edit --wait"), "edit");
    }
}

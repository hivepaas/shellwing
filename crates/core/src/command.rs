//! Commands as people type them: split into words as a shell would, and
//! programs found on `PATH`. For the editor and the local shell settings.

use std::path::PathBuf;

/// Where `cmd` is on `PATH`, if it is.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let pathext =
        cfg!(windows).then(|| std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()));
    which_in(cmd, &path, pathext.as_deref())
}

/// Where `cmd` is in the directories of `path`. With `pathext` (Windows), a
/// name without an extension is tried with each of them, as Windows finds
/// programs; a name with one (`pwsh.exe`) is looked for as it is.
fn which_in(cmd: &str, path: &std::ffi::OsStr, pathext: Option<&str>) -> Option<PathBuf> {
    let names: Vec<String> = match pathext {
        Some(exts) if std::path::Path::new(cmd).extension().is_none() => exts
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| format!("{cmd}{}", e.to_lowercase()))
            .collect(),
        _ => vec![cmd.to_string()],
    };
    std::env::split_paths(path)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// Splits a command as a shell would its words: spaces part them, quotes
/// keep them together.
pub fn words(cmd: &str) -> Vec<String> {
    let (mut out, mut cur, mut quote, mut any) = (vec![], String::new(), None, false);
    for c in cmd.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_split_like_a_shell() {
        assert_eq!(words("code -n"), ["code", "-n"]);
        assert_eq!(
            words(r#""/opt/My Editor/bin/edit" --wait"#),
            ["/opt/My Editor/bin/edit", "--wait"]
        );
        assert_eq!(words("  subl  "), ["subl"]);
        assert_eq!(words("vim -c ''"), ["vim", "-c", ""]);
    }

    /// A scratch directory holding empty files with these names.
    fn dir_with(name: &str, files: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-which-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for f in files {
            std::fs::write(dir.join(f), "").expect("file");
        }
        dir
    }

    #[test]
    fn windows_finds_a_name_given_with_its_extension() {
        // The local shell asks for "pwsh.exe": not "pwsh.exe.exe".
        let dir = dir_with("ext", &["pwsh.exe", "code.cmd"]);
        let path = dir.clone().into_os_string();
        let exts = Some(".COM;.EXE;.BAT;.CMD");
        assert_eq!(
            which_in("pwsh.exe", &path, exts),
            Some(dir.join("pwsh.exe"))
        );
        assert_eq!(which_in("pwsh", &path, exts), Some(dir.join("pwsh.exe")));
        assert_eq!(which_in("code", &path, exts), Some(dir.join("code.cmd")));
        assert_eq!(which_in("powershell.exe", &path, exts), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn elsewhere_names_are_looked_for_as_they_are() {
        let dir = dir_with("plain", &["zsh"]);
        let path = std::env::join_paths([dir_with("empty", &[]), dir.clone()]).expect("path");
        assert_eq!(which_in("zsh", &path, None), Some(dir.join("zsh")));
        assert_eq!(which_in("zsh.exe", &path, None), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}

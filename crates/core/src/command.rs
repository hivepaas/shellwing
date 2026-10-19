//! Commands as people type them: split into words as a shell would, and
//! programs found on `PATH`. For the editor and the local shell settings.

use std::path::PathBuf;

/// Where `cmd` is on `PATH`, if it is.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(str::to_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{cmd}{e}")))
            .find(|p| p.is_file())
    })
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
}

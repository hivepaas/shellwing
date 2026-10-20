//! Environments: variables for the shell, kept in the vault, loaded into a
//! pane's shell or into a snippet's own subshell, and unloaded.
//!
//! A value never shows: shellwing types a command that turns the
//! terminal's echo off and signals it is ready (an `OSC 777` shellwing's
//! terminal reads and never draws), and only then sends the values, one
//! per line, for `read` to take. The command itself holds the names only,
//! and starts with a space, which keeps it out of a bash history.

use shellwing_term::signal_sequence;

/// A variable's name, as a POSIX shell takes it.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// A variable read from `.env` text: its name, and its value, None when
/// left empty (keep the one it had).
pub type Parsed = Vec<(String, Option<String>)>;

/// `.env` text, as typed or pasted: `NAME=value` a line, `export ` before
/// it allowed, `#` comments, quotes around a value taken off. A line that
/// is not one, or a value of several lines, is said by its line number.
pub fn parse_dotenv(text: &str) -> Result<Parsed, String> {
    let mut vars: Parsed = vec![];
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {}: NAME=value", n + 1));
        };
        let name = name.trim();
        if !valid_name(name) {
            return Err(format!("line {}: {name}", n + 1));
        }
        let value = value.trim();
        let value = ['"', '\'']
            .iter()
            .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
            .unwrap_or(value);
        vars.retain(|(v, _)| v != name);
        vars.push((
            name.to_string(),
            (!value.is_empty()).then(|| value.to_string()),
        ));
    }
    Ok(vars)
}

/// What a set of names reads as `.env` text, for editing: the values left
/// out (they are kept unless one is typed).
pub fn names_as_dotenv(names: &[String]) -> String {
    names.iter().map(|n| format!("{n}=\n")).collect()
}

/// The command that reads `names` with the echo off, signals `ready`, and
/// exports them; then `done`. Typed into the pane, it waits for the values.
pub fn load(names: &[String], ready: &str, done: &str) -> String {
    format!(" {}; {}", reading(names, ready), finish(names, done))
}

/// A snippet's `command` in a subshell that has `names` first: they are
/// gone when it ends.
pub fn wrapped(names: &[String], ready: &str, command: &str) -> String {
    let export = format!("export {}", names.join(" "));
    format!(" ( {}; {export}\n{command}\n)", reading(names, ready))
}

fn reading(names: &[String], ready: &str) -> String {
    let reads: Vec<String> = names.iter().map(|n| format!("IFS= read -r {n}")).collect();
    format!(
        "stty -echo; printf '{}'; {}; stty echo",
        signal_sequence(ready),
        reads.join("; ")
    )
}

fn finish(names: &[String], done: &str) -> String {
    format!(
        "export {}; printf '{}'",
        names.join(" "),
        signal_sequence(done)
    )
}

/// The command that unloads `names` from the shell.
pub fn unload(names: &[String]) -> String {
    format!(" unset {}", names.join(" "))
}

/// A value as `read` takes it: one line. A value of several lines cannot
/// be one.
pub fn sendable(value: &str) -> bool {
    !value.contains(['\n', '\r'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_text_is_read_as_people_write_it() {
        let text = "# AWS\nexport AWS_ACCESS_KEY_ID=AKIA1\nAWS_SECRET_ACCESS_KEY=\"s3 cret\"\nAWS_REGION='eu-west-1'\nTOKEN=\n\n";
        assert_eq!(
            parse_dotenv(text).expect("read"),
            vec![
                ("AWS_ACCESS_KEY_ID".into(), Some("AKIA1".into())),
                ("AWS_SECRET_ACCESS_KEY".into(), Some("s3 cret".into())),
                ("AWS_REGION".into(), Some("eu-west-1".into())),
                ("TOKEN".into(), None),
            ]
        );
        assert_eq!(
            parse_dotenv("A=1\nA=2").expect("read"),
            vec![("A".into(), Some("2".into()))]
        );
        assert!(parse_dotenv("just words").is_err());
        assert!(parse_dotenv("2BAD=x").is_err());
        assert!(valid_name("_A1") && !valid_name("A-B") && !valid_name(""));
        assert_eq!(names_as_dotenv(&["A".into(), "B".into()]), "A=\nB=\n");
    }

    #[test]
    fn commands_hold_names_never_values() {
        let names = vec!["A".to_string(), "B".to_string()];
        let l = load(&names, "r1", "d1");
        assert!(
            l.starts_with(" stty -echo; printf '\\033]777;shellwing;r1\\007';"),
            "{l}"
        );
        assert!(
            l.contains("IFS= read -r A; IFS= read -r B; stty echo; export A B;"),
            "{l}"
        );
        assert!(l.ends_with("printf '\\033]777;shellwing;d1\\007'"), "{l}");
        let w = wrapped(&names, "r2", "aws s3 ls\naws sts get-caller-identity");
        assert!(w.starts_with(" ( stty -echo;"), "{w}");
        assert!(
            w.ends_with("export A B\naws s3 ls\naws sts get-caller-identity\n)"),
            "{w}"
        );
        assert_eq!(unload(&names), " unset A B");
        assert!(sendable("abc") && !sendable("a\nb"));
    }
}

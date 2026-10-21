//! Environments: variables for the shell, kept in the vault, loaded into a
//! pane's shell or into a snippet's own subshell, and unloaded.
//!
//! A value never shows: shellwing types a command that turns the
//! terminal's echo off and signals it is ready (an `OSC 777` shellwing's
//! terminal reads and never draws), and only then sends the values, as
//! lines of printable ASCII for `read` to take (see [`encode`]). The command itself holds the names only,
//! and starts with a space, which keeps it out of a bash history.

use shellwing_i18n::tr;
use shellwing_term::signal_sequence;
use shellwing_vault::{Oversize, limits};
use zeroize::Zeroizing;

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

/// What is past a limit, said in the language in use.
pub fn said_oversize(o: &Oversize) -> String {
    let kb = |n: usize| n.div_ceil(1024);
    match o {
        Oversize::Command(n) => tr!(
            "The command is {size} KB; {max} KB at most.",
            size = kb(*n),
            max = kb(limits::COMMAND)
        ),
        Oversize::Name(n) => tr!(
            "{name}: a name takes {max} characters at most.",
            name = n.as_str(),
            max = limits::NAME
        ),
        Oversize::Value(n, b) => tr!(
            "{name} is {size} KB; a value takes {max} KB at most.",
            name = n.as_str(),
            size = kb(*b),
            max = kb(limits::VALUE)
        ),
        Oversize::Vars(n) => tr!(
            "{count} variables; {max} at most.",
            count = *n,
            max = limits::VARS
        ),
        Oversize::Environment(n) => tr!(
            "The variables come to {size} KB; {max} KB at most.",
            size = kb(*n),
            max = kb(limits::ENVIRONMENT)
        ),
        Oversize::File(n) => tr!(
            "The file is {size} KB; {max} KB at most.",
            size = kb(*n),
            max = kb(limits::FILE)
        ),
        Oversize::Note(n) => tr!(
            "The note is {size} KB; {max} KB at most.",
            size = kb(*n),
            max = kb(limits::NOTE)
        ),
        Oversize::Attachment(n) => tr!(
            "The image is {size} KB; {max} KB at most.",
            size = kb(*n),
            max = kb(limits::ATTACHMENT)
        ),
        Oversize::NoteAttachments(n) => tr!(
            "The note's images come to {size} MB; {max} MB at most.",
            size = n.div_ceil(1024 * 1024),
            max = limits::NOTE_ATTACHMENTS / (1024 * 1024)
        ),
    }
}

/// A vault's error, said: a limit in the language in use.
pub fn said_error(e: &shellwing_vault::Error) -> String {
    match e {
        shellwing_vault::Error::TooLarge(o) => said_oversize(o),
        shellwing_vault::Error::NotAnImage => {
            tr!("Not an image that can be kept: a PNG or a JPEG.")
        }
        e => e.to_string(),
    }
}

/// A variable as sent: its name, and the lines its value goes in.
pub struct Sent {
    pub name: String,
    pub lines: Vec<Zeroizing<String>>,
}

/// The longest line a value is sent in: well under what a terminal's
/// line discipline takes (4095 bytes on Linux).
const LINE: usize = 512;

/// `value` as lines of printable ASCII for `read` to take: a newline, a
/// control character, a backslash or a byte past ASCII as `\0ooo`, which
/// the shell's `printf %b` turns back. Each line starts with `>`; a `.`
/// ends the value. Any value goes, of any length.
pub fn encode(name: &str, value: &str) -> Sent {
    let mut lines = vec![Zeroizing::new(String::new())];
    for b in value.bytes() {
        let piece = if (0x20..0x7f).contains(&b) && b != b'\\' {
            (b as char).to_string()
        } else {
            format!("\\0{b:03o}")
        };
        if lines.last().is_some_and(|l| l.len() + piece.len() > LINE) {
            lines.push(Zeroizing::new(String::new()));
        }
        if let Some(l) = lines.last_mut() {
            l.push_str(&piece);
        }
    }
    for l in &mut lines {
        l.insert(0, '>');
    }
    lines.push(Zeroizing::new(".".to_string()));
    Sent {
        name: name.to_string(),
        lines,
    }
}

/// The command that reads `vars` with the echo off, signals `ready`, and
/// exports them; then `done`. Typed into the pane, it waits for the values.
pub fn load(vars: &[Sent], ready: &str, done: &str) -> String {
    format!(" {}; {}", reading(vars, ready), finish(vars, done))
}

/// A snippet's `command` in a subshell that has `vars` first: they are
/// gone when it ends.
pub fn wrapped(vars: &[Sent], ready: &str, command: &str) -> String {
    format!(
        " ( {}; export {}\n{command}\n)",
        reading(vars, ready),
        names(vars)
    )
}

fn names(vars: &[Sent]) -> String {
    vars.iter()
        .map(|v| v.name.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each value's lines read up to its `.` and joined, then decoded: `X`
/// after it keeps the newlines it ends in from `$(…)`. A loop, so the
/// command is as long whatever the value's (busybox edits 1 KB at most).
fn reading(vars: &[Sent], ready: &str) -> String {
    let mut steps = vec![];
    for v in vars {
        steps.push(
            "__sw_v=; while IFS= read -r __sw_l && [ \"$__sw_l\" != . ]; do __sw_v=$__sw_v${__sw_l#>}; done"
                .to_string(),
        );
        steps.push(format!(
            "{n}=$(printf '%bX' \"$__sw_v\"); {n}=${{{n}%X}}",
            n = v.name
        ));
    }
    format!(
        "stty -echo; printf '{}'; {}; stty echo; unset __sw_v __sw_l",
        signal_sequence(ready),
        steps.join("; ")
    )
}

fn finish(vars: &[Sent], done: &str) -> String {
    format!("export {}; printf '{}'", names(vars), signal_sequence(done))
}

/// The command that unloads `names` from the shell.
pub fn unload(names: &[String]) -> String {
    format!(" unset {}", names.join(" "))
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
    }

    #[test]
    fn values_go_as_printable_lines() {
        let shown = |s: Sent| s.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shown(encode("K", "a\\b\nc\u{e9}")),
            [">a\\0134b\\0012c\\0303\\0251", "."]
        );
        let long = encode("K", &"x".repeat(1200));
        assert_eq!(
            long.lines.iter().map(|l| l.len()).collect::<Vec<_>>(),
            [513, 513, 177, 1]
        );
        assert_eq!(shown(encode("K", "")), [">", "."]);
    }

    #[test]
    fn commands_hold_names_never_values() {
        let vars = vec![encode("A", "secret one"), encode("B", &"y".repeat(600))];
        let l = load(&vars, "r1", "d1");
        assert!(
            l.starts_with(" stty -echo; printf '\\033]777;shellwing;r1\\007';"),
            "{l}"
        );
        assert!(!l.contains("secret") && !l.contains("yyy"), "{l}");
        assert_eq!(
            l.matches("IFS= read -r").count(),
            2,
            "one loop a variable: {l}"
        );
        assert!(l.contains("A=$(printf '%bX' \"$__sw_v\"); A=${A%X}"), "{l}");
        assert!(
            l.ends_with("export A B; printf '\\033]777;shellwing;d1\\007'"),
            "{l}"
        );
        let w = wrapped(&vars, "r2", "aws s3 ls\naws sts get-caller-identity");
        assert!(w.starts_with(" ( stty -echo;"), "{w}");
        assert!(
            w.ends_with("export A B\naws s3 ls\naws sts get-caller-identity\n)"),
            "{w}"
        );
        assert_eq!(unload(&["A".into(), "B".into()]), " unset A B");
    }
}

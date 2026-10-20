//! Shell code's parts, to color it as a snippet's editor does: commands,
//! keywords, flags, quoted text, `$` expansions, operators, comments, and
//! the snippet's own `{{name}}` holes over all of them. A reading for the
//! eye, line by line, not a parser: what it cannot place stays plain.

use std::ops::Range;

use crate::colors::colors;
use crate::input::Paint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    /// The word a command starts with: `docker`, `git`.
    Command,
    /// `if`, `then`, `for`, `done`.
    Keyword,
    /// `-f`, `--all`.
    Flag,
    /// Quoted: `'…'`, `"…"` (the expansions in it apart).
    Text,
    /// `$VAR`, `${…}`, `$(…)`, `` `…` ``.
    Expansion,
    /// `|`, `&&`, `;`, `>`, `(`.
    Operator,
    Comment,
    /// `{{name}}`, filled in when the snippet runs.
    Hole,
}

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "!", "{", "}", "[[", "]]",
];

/// After these a command still comes: `sudo systemctl …`.
const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "exec", "nohup", "nice", "command", "builtin", "xargs", "watch",
    "timeout", "time",
];

/// Ends a word: the rest of the line goes on in its own parts.
fn breaks(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t'
            | b'\r'
            | b'\n'
            | b'|'
            | b'&'
            | b';'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'\''
            | b'"'
            | b'`'
            | b'$'
            | b'\\'
    )
}

/// The length of the character at `i` (0 at the end).
fn char_len(src: &str, i: usize) -> usize {
    src[i..].chars().next().map_or(0, char::len_utf8)
}

/// Where the closing `close` of what opens at `open` (the opener itself)
/// is, counting those nested and skipping quoted text; past it.
fn closing(src: &str, open: usize, opener: u8, close: u8) -> usize {
    let b = src.as_bytes();
    let (mut depth, mut i) = (0usize, open);
    while i < b.len() {
        match b[i] {
            b'\\' => {
                i += 1 + char_len(src, (i + 1).min(src.len()));
                continue;
            }
            b'\'' if opener == b'(' => {
                i = src[i + 1..].find('\'').map_or(b.len(), |j| i + 1 + j + 1);
                continue;
            }
            c if c == opener => depth += 1,
            c if c == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// The `$` expansion at `i`: its end, or None for a lone `$`.
fn expansion(src: &str, i: usize) -> Option<usize> {
    let b = src.as_bytes();
    match b.get(i + 1)? {
        b'(' => Some(closing(src, i + 1, b'(', b')')),
        b'{' => Some(closing(src, i + 1, b'{', b'}')),
        c if c.is_ascii_alphabetic() || *c == b'_' => {
            let n = b[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                .count();
            Some(i + 1 + n)
        }
        c if c.is_ascii_digit() || b"@*#?$!-".contains(c) => Some(i + 2),
        _ => None,
    }
}

/// The parts of `src`, in order, apart from plain words and spaces.
pub fn tokens(src: &str) -> Vec<(Range<usize>, Token)> {
    let b = src.as_bytes();
    let mut out = vec![];
    // A command comes next: at a line's start, after `|`, `;`, `&&`, `then`.
    let mut command = true;
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match b[i] {
            b'\n' => {
                command = true;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            // The next character as it is; a line's end, the line goes on.
            b'\\' => i += 1 + char_len(src, (i + 1).min(src.len())),
            b'#' => {
                i = src[i..].find('\n').map_or(b.len(), |j| i + j);
                out.push((start..i, Token::Comment));
            }
            b'\'' => {
                i = src[i + 1..].find('\'').map_or(b.len(), |j| i + 1 + j + 1);
                out.push((start..i, Token::Text));
                command = false;
            }
            b'"' => {
                // Text, with its expansions apart.
                let mut from = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    match b[i] {
                        b'\\' => i += 1 + char_len(src, (i + 1).min(src.len())),
                        b'$' | b'`' => {
                            let end = if b[i] == b'`' {
                                src[i + 1..].find('`').map_or(b.len(), |j| i + 1 + j + 1)
                            } else {
                                match expansion(src, i) {
                                    Some(e) => e,
                                    None => {
                                        i += 1;
                                        continue;
                                    }
                                }
                            };
                            if from < i {
                                out.push((from..i, Token::Text));
                            }
                            out.push((i..end, Token::Expansion));
                            i = end;
                            from = end;
                        }
                        _ => i += 1,
                    }
                }
                i = (i + 1).min(b.len());
                if from < i {
                    out.push((from..i, Token::Text));
                }
                command = false;
            }
            b'`' => {
                i = src[i + 1..].find('`').map_or(b.len(), |j| i + 1 + j + 1);
                out.push((start..i, Token::Expansion));
                command = false;
            }
            b'$' => match expansion(src, i) {
                Some(end) => {
                    i = end;
                    out.push((start..i, Token::Expansion));
                    command = false;
                }
                None => i += 1,
            },
            b'(' | b')' => {
                i += 1;
                out.push((start..i, Token::Operator));
                command = b[start] == b'(';
            }
            b'|' | b'&' | b';' | b'<' | b'>' => {
                while i < b.len() && matches!(b[i], b'|' | b'&' | b';' | b'<' | b'>') {
                    i += 1;
                }
                out.push((start..i, Token::Operator));
                let op = &src[start..i];
                // `2>&1`, `> file`: what follows is not a command.
                command = !op.contains(['<', '>']);
            }
            _ => {
                while i < b.len() && !breaks(b[i]) {
                    i += char_len(src, i);
                }
                let word = &src[start..i];
                if command {
                    let assignment = word.split_once('=').is_some_and(|(name, _)| {
                        !name.is_empty()
                            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    });
                    if KEYWORDS.contains(&word) {
                        out.push((start..i, Token::Keyword));
                        // `for x in`, `case $a in`: a name, then words.
                        command = !matches!(word, "for" | "case" | "select" | "function" | "in")
                            && !matches!(word, "fi" | "done" | "esac" | "}" | "]]");
                    } else if assignment {
                        // `LANG=C sort`: still a command to come.
                    } else {
                        out.push((start..i, Token::Command));
                        command = WRAPPERS.contains(&word);
                    }
                } else if word.len() > 1 && word.starts_with('-') {
                    out.push((start..i, Token::Flag));
                } else if matches!(word, "then" | "do" | "else" | "elif") {
                    out.push((start..i, Token::Keyword));
                    command = true;
                } else if word == "in" {
                    // `for f in …`, `case $x in`.
                    out.push((start..i, Token::Keyword));
                }
            }
        }
    }
    out
}

/// The parts of `src` with the `{{name}}` holes over them, as one list in
/// order: a hole cuts what it sits in.
pub fn parts(src: &str) -> Vec<(Range<usize>, Token)> {
    let holes = crate::snippets::holes(src);
    if holes.is_empty() {
        return tokens(src);
    }
    let mut kind: Vec<Option<Token>> = vec![None; src.len()];
    for (r, t) in tokens(src) {
        kind[r].fill(Some(t));
    }
    for (r, _) in holes {
        kind[r].fill(Some(Token::Hole));
    }
    let mut out: Vec<(Range<usize>, Token)> = vec![];
    for (i, k) in kind.into_iter().enumerate() {
        let Some(k) = k else {
            continue;
        };
        match out.last_mut() {
            Some((r, last)) if r.end == i && *last == k => r.end = i + 1,
            _ => out.push((i..i + 1, k)),
        }
    }
    out
}

/// How `src` is painted, by part, in the colors in use.
pub fn paint(src: &str) -> Vec<(Range<usize>, Paint)> {
    let c = colors();
    parts(src)
        .into_iter()
        .map(|(r, t)| {
            let color = match t {
                Token::Command => c.code_command,
                Token::Keyword => c.code_keyword,
                Token::Flag => c.code_flag,
                Token::Text => c.code_text,
                Token::Expansion => c.code_expansion,
                Token::Operator => c.code_operator,
                Token::Comment => c.faint,
                Token::Hole => c.code_hole,
            };
            let ground = (t == Token::Hole).then_some(c.code_hole_ground);
            (r, Paint { color, ground })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each part's text and kind.
    fn said(src: &str) -> Vec<(&str, Token)> {
        parts(src).into_iter().map(|(r, t)| (&src[r], t)).collect()
    }

    use Token::*;

    #[test]
    fn commands_flags_and_operators() {
        assert_eq!(
            said("cd /srv && git pull --ff-only | tee -a log"),
            [
                ("cd", Command),
                ("&&", Operator),
                ("git", Command),
                ("--ff-only", Flag),
                ("|", Operator),
                ("tee", Command),
                ("-a", Flag),
            ]
        );
        // A wrapper's command is a command too; a redirect's target is not.
        assert_eq!(
            said("sudo systemctl restart nginx 2>&1 > out"),
            [
                ("sudo", Command),
                ("systemctl", Command),
                (">&", Operator),
                (">", Operator),
            ]
        );
        assert_eq!(said("LANG=C sort -u")[0], ("sort", Command));
    }

    #[test]
    fn quotes_expansions_and_comments() {
        assert_eq!(
            said(r#"echo "on $(hostname) at ${TIME}" 'as $is' # done"#),
            [
                ("echo", Command),
                ("\"on ", Text),
                ("$(hostname)", Expansion),
                (" at ", Text),
                ("${TIME}", Expansion),
                ("\"", Text),
                ("'as $is'", Text),
                ("# done", Comment),
            ]
        );
        // `#` in a word is not a comment; `$#` is an expansion.
        assert_eq!(said("echo a#b $#"), [("echo", Command), ("$#", Expansion)]);
    }

    #[test]
    fn keywords_start_commands_again() {
        assert_eq!(
            said("for f in *.log; do gzip \"$f\"; done"),
            [
                ("for", Keyword),
                ("in", Keyword),
                (";", Operator),
                ("do", Keyword),
                ("gzip", Command),
                ("\"", Text),
                ("$f", Expansion),
                ("\"", Text),
                (";", Operator),
                ("done", Keyword),
            ]
        );
        assert_eq!(
            said("if [ -f x ]; then\n  rm x\nfi"),
            [
                ("if", Keyword),
                ("[", Command),
                ("-f", Flag),
                (";", Operator),
                ("then", Keyword),
                ("rm", Command),
                ("fi", Keyword),
            ]
        );
    }

    #[test]
    fn holes_cut_what_they_sit_in() {
        assert_eq!(
            said("# Deploy {{app}}\ncd /srv/{{app}} && echo \"{{host}}!\""),
            [
                ("# Deploy ", Comment),
                ("{{app}}", Hole),
                ("cd", Command),
                ("{{app}}", Hole),
                ("&&", Operator),
                ("echo", Command),
                ("\"", Text),
                ("{{host}}", Hole),
                ("!\"", Text),
            ]
        );
    }

    #[test]
    fn what_does_not_close_runs_to_the_end_and_nothing_breaks() {
        for src in [
            "echo \"open",
            "echo 'open",
            "$(unclosed",
            "${",
            "a\\",
            "é$",
            "`x",
            "\\é",
        ] {
            let p = parts(src);
            assert!(
                p.iter()
                    .all(|(r, _)| r.end <= src.len() && src.is_char_boundary(r.start))
            );
        }
        assert_eq!(said("echo \"open"), [("echo", Command), ("\"open", Text)]);
    }
}

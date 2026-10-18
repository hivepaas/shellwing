//! Finding plain-text URLs in a terminal row.
//!
//! Programs can mark links explicitly with OSC 8; this covers the far more common
//! case of a URL that is just printed. Detection works on the row's cells so the
//! result is a column range the UI can underline.

/// Schemes recognised in plain text.
const SCHEMES: &[&str] = &["https", "http", "ftp", "ftps", "sftp", "ssh", "file", "git"];

/// Schemes the app will hand to the system to open, for both detected and OSC 8 links.
pub fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:", "ftp://"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Characters that can never be part of a detected URL.
fn stops(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '<' | '>' | '"' | '\'' | '`' | '{' | '}' | '|' | '\\' | '^'
        )
}

/// The URL under column `col`, given one entry per cell (empty for the second
/// half of a wide character). Returns the start column, the end column
/// (exclusive) and the URL.
pub fn url_at(cells: &[String], col: usize) -> Option<(usize, usize, String)> {
    // Flatten to characters, remembering which column each came from.
    let mut chars: Vec<(char, usize)> = Vec::new();
    for (c, g) in cells.iter().enumerate() {
        chars.extend(g.chars().map(|ch| (ch, c)));
    }
    let text: Vec<char> = chars.iter().map(|(c, _)| *c).collect();
    let mut i = 0;
    while i + 2 < text.len() {
        let is_sep = text[i] == ':' && text[i + 1] == '/' && text[i + 2] == '/';
        let is_mailto = text[i..].starts_with(&['m', 'a', 'i', 'l', 't', 'o', ':'])
            && (i == 0 || !text[i - 1].is_ascii_alphanumeric());
        if !is_sep && !is_mailto {
            i += 1;
            continue;
        }
        // Walk back over the scheme for "://"; mailto starts where it is.
        let mut start = i;
        if is_sep {
            while start > 0
                && (text[start - 1].is_ascii_alphanumeric()
                    || matches!(text[start - 1], '+' | '-' | '.'))
            {
                start -= 1;
            }
            let scheme: String = text[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            if !SCHEMES.contains(&scheme.as_str()) {
                i += 3;
                continue;
            }
        }
        let mut end = if is_sep { i + 3 } else { i + 7 };
        while end < text.len() && !stops(text[end]) {
            end += 1;
        }
        // Trailing punctuation usually ends the sentence, not the URL; a closing
        // bracket belongs to the URL only if it opened one.
        while end > start {
            let c = text[end - 1];
            let url = &text[start..end - 1];
            let unbalanced = |open: char, close: char| {
                c == close
                    && url.iter().filter(|&&x| x == open).count()
                        < url.iter().filter(|&&x| x == close).count() + 1
            };
            if matches!(c, '.' | ',' | ';' | ':' | '!' | '?')
                || unbalanced('(', ')')
                || unbalanced('[', ']')
            {
                end -= 1;
            } else {
                break;
            }
        }
        let min_len = if is_sep { i + 4 } else { i + 8 };
        if end >= min_len {
            let (c0, c1) = (chars[start].1, chars[end - 1].1);
            if (c0..=c1).contains(&col) {
                return Some((c0, c1 + 1, text[start..end].iter().collect()));
            }
        }
        i = end.max(i + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(s: &str) -> Vec<String> {
        s.chars().map(|c| c.to_string()).collect()
    }

    #[test]
    fn finds_the_url_under_the_column() {
        let row = cells("see https://example.com/a?b=1 now");
        assert_eq!(
            url_at(&row, 10),
            Some((4, 29, "https://example.com/a?b=1".into()))
        );
        assert_eq!(url_at(&row, 2), None);
        assert_eq!(url_at(&row, 31), None);
    }

    #[test]
    fn trims_sentence_punctuation_and_unbalanced_brackets() {
        let row = cells("(docs: https://ex.com/x_(y)).");
        let (_, _, url) = url_at(&row, 10).expect("url");
        assert_eq!(url, "https://ex.com/x_(y)");
        let row = cells("go to http://a.b/c, then");
        assert_eq!(url_at(&row, 8).map(|u| u.2), Some("http://a.b/c".into()));
    }

    #[test]
    fn ignores_unknown_schemes_and_handles_mailto() {
        assert_eq!(url_at(&cells("javascript://x"), 3), None);
        assert_eq!(
            url_at(&cells("mail mailto:me@ex.com"), 8).map(|u| u.2),
            Some("mailto:me@ex.com".into())
        );
    }

    #[test]
    fn maps_through_wide_cells() {
        // "日本" takes four cells (two wide glyphs), then a URL.
        let mut row = vec![
            "日".to_string(),
            String::new(),
            "本".to_string(),
            String::new(),
            " ".to_string(),
        ];
        row.extend(cells("https://a.jp"));
        assert_eq!(url_at(&row, 6), Some((5, 17, "https://a.jp".into())));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(openable("https://x") && openable("mailto:a@b") && openable("HTTP://x"));
        assert!(
            !openable("file:///etc/passwd")
                && !openable("ssh://h")
                && !openable("javascript:alert(1)")
        );
    }
}

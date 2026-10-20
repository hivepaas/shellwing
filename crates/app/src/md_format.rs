//! Markdown formatting of a selection, as a note's toolbar applies it:
//! marks around it (`**bold**`), or a prefix on each of its lines (`> `).
//! Applied again, each takes itself off. Each gives the new text and what
//! to select in it.

use std::ops::Range;

/// What the toolbar does to a selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bold,
    Italic,
    Strike,
    /// Inline code; a selection of several lines gets a fenced block.
    Code,
    Link,
    Heading,
    Quote,
    Bullet,
    Task,
}

/// The text with the selection `sel` formatted, and the new selection.
pub fn apply(text: &str, sel: Range<usize>, f: Format) -> (String, Range<usize>) {
    let sel = sel.start.min(text.len())..sel.end.min(text.len());
    match f {
        Format::Bold => wrap(text, sel, "**"),
        Format::Italic => wrap(text, sel, "_"),
        Format::Strike => wrap(text, sel, "~~"),
        Format::Code if text[sel.clone()].contains('\n') => fence(text, sel),
        Format::Code => wrap(text, sel, "`"),
        Format::Link => link(text, sel),
        Format::Heading => prefix(text, sel, "## "),
        Format::Quote => prefix(text, sel, "> "),
        Format::Bullet => prefix(text, sel, "- "),
        Format::Task => prefix(text, sel, "- [ ] "),
    }
}

/// `mark` around the selection, or off it when it has it (inside or just
/// outside the selection).
fn wrap(text: &str, sel: Range<usize>, mark: &str) -> (String, Range<usize>) {
    let inner = &text[sel.clone()];
    let n = mark.len();
    // Selected with its marks.
    if inner.len() >= 2 * n && inner.starts_with(mark) && inner.ends_with(mark) {
        let out = format!(
            "{}{}{}",
            &text[..sel.start],
            &inner[n..inner.len() - n],
            &text[sel.end..]
        );
        return (out, sel.start..sel.end - 2 * n);
    }
    // Marks just outside it.
    if sel.start >= n && text[..sel.start].ends_with(mark) && text[sel.end..].starts_with(mark) {
        let out = format!(
            "{}{}{}",
            &text[..sel.start - n],
            inner,
            &text[sel.end + n..]
        );
        return (out, sel.start - n..sel.end - n);
    }
    let out = format!(
        "{}{mark}{inner}{mark}{}",
        &text[..sel.start],
        &text[sel.end..]
    );
    (out, sel.start + n..sel.end + n)
}

/// A fenced block around the selection's lines, or off them: the fences
/// on the lines just around them, or among them.
fn fence(text: &str, sel: Range<usize>) -> (String, Range<usize>) {
    let lines = line_span(text, sel);
    let block = &text[lines.clone()];
    // Fences on the lines around the selection.
    let before = text[..lines.start.saturating_sub(1)]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let after_end = text[(lines.end + 1).min(text.len())..]
        .find('\n')
        .map_or(text.len(), |i| lines.end + 1 + i);
    if lines.start > 0
        && lines.end < text.len()
        && text[before..lines.start - 1].starts_with("```")
        && &text[lines.end + 1..after_end] == "```"
    {
        let out = format!("{}{block}{}", &text[..before], &text[after_end..]);
        return (out, before..before + block.len());
    }
    // The fences selected with it.
    if let Some(body) = block
        .strip_prefix("```")
        .and_then(|b| b.split_once('\n').map(|(_, rest)| rest))
        .and_then(|b| b.strip_suffix("```"))
    {
        let body = body.strip_suffix('\n').unwrap_or(body);
        let out = format!("{}{body}{}", &text[..lines.start], &text[lines.end..]);
        return (out, lines.start..lines.start + body.len());
    }
    let out = format!(
        "{}```\n{block}\n```{}",
        &text[..lines.start],
        &text[lines.end..]
    );
    (out, lines.start + 4..lines.start + 4 + block.len())
}

/// `[selection](url)`, with `url` selected to type over.
fn link(text: &str, sel: Range<usize>) -> (String, Range<usize>) {
    let inner = &text[sel.clone()];
    let out = format!("{}[{inner}](url){}", &text[..sel.start], &text[sel.end..]);
    let url = sel.start + inner.len() + 3;
    (out, url..url + 3)
}

/// The whole lines the selection touches.
fn line_span(text: &str, sel: Range<usize>) -> Range<usize> {
    let start = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    // A selection ending at a line's start does not take that line.
    let end_at = if sel.end > sel.start && text[..sel.end].ends_with('\n') {
        sel.end - 1
    } else {
        sel.end
    };
    let end = text[end_at..].find('\n').map_or(text.len(), |i| end_at + i);
    start..end.max(start)
}

/// `p` before each of the selection's lines, or off them all when every
/// one has it.
fn prefix(text: &str, sel: Range<usize>, p: &str) -> (String, Range<usize>) {
    let span = line_span(text, sel);
    let block = &text[span.clone()];
    let all = block.split('\n').all(|l| l.starts_with(p));
    let lines: Vec<String> = block
        .split('\n')
        .map(|l| {
            if all {
                l[p.len()..].to_string()
            } else if l.starts_with(p) {
                l.to_string()
            } else {
                format!("{p}{l}")
            }
        })
        .collect();
    let new = lines.join("\n");
    let out = format!("{}{new}{}", &text[..span.start], &text[span.end..]);
    (out, span.start..span.start + new.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, part: &str) -> Range<usize> {
        let i = text.find(part).expect("in the text");
        i..i + part.len()
    }

    #[test]
    fn marks_go_on_and_come_off() {
        let t = "restart the app now";
        let (b, s) = apply(t, at(t, "the app"), Format::Bold);
        assert_eq!(b, "restart **the app** now");
        assert_eq!(&b[s.clone()], "the app");
        // Again, on what it selected: off.
        let (off, s2) = apply(&b, s, Format::Bold);
        assert_eq!(off, t);
        assert_eq!(&off[s2], "the app");
        // Selected with the marks: off too.
        let (off, _) = apply(&b, at(&b, "**the app**"), Format::Bold);
        assert_eq!(off, t);
        assert_eq!(
            apply(t, at(t, "app"), Format::Code).0,
            "restart the `app` now"
        );
    }

    #[test]
    fn lines_take_prefixes_and_blocks() {
        let t = "intro\nsudo systemctl restart app\njournalctl -u app\nend";
        let sel = at(t, "systemctl restart app\njournal");
        let (q, s) = apply(t, sel.clone(), Format::Quote);
        assert_eq!(
            q,
            "intro\n> sudo systemctl restart app\n> journalctl -u app\nend"
        );
        let (back, _) = apply(&q, s, Format::Quote);
        assert_eq!(back, t);
        let (c, s) = apply(t, sel, Format::Code);
        assert_eq!(
            c,
            "intro\n```\nsudo systemctl restart app\njournalctl -u app\n```\nend"
        );
        assert_eq!(
            &c[s.clone()],
            "sudo systemctl restart app\njournalctl -u app"
        );
        // Again, on what it selected: the fences go.
        assert_eq!(apply(&c, s, Format::Code).0, t);
        // Selected with its fences: they go too.
        let all = at(&c, "```\nsudo");
        assert_eq!(apply(&c, all.start..c.len() - 4, Format::Code).0, t);
        let (task, _) = apply(t, at(t, "intro"), Format::Task);
        assert!(task.starts_with("- [ ] intro\n"));
    }

    #[test]
    fn a_link_selects_its_address() {
        let t = "see docs here";
        let (l, s) = apply(t, at(t, "docs"), Format::Link);
        assert_eq!(l, "see [docs](url) here");
        assert_eq!(&l[s], "url");
    }
}

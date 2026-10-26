//! A note's Markdown as the phone edits it: the source itself, styled as it
//! is typed (bold bold, headings large, code in a monospace face, links in
//! color, the marks `**`, `#`, `>` dimmed: the text is never changed for
//! show), and the edits its bar and its Return key make, as the desktop's
//! editor makes them (Return carries a list on, Tab moves an item in by four
//! spaces). Read with pulldown-cmark, as [`crate::markdown`] reads a note to
//! show it; what counts as an alert and a bare link is the desktop's
//! (`zorite-markdown`).
//!
//! Offsets in and out are UTF-16, as UIKit counts them; inside, bytes.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};
use zorite_markdown::syntax as zsyntax;

use crate::markdown::MdAlert;

/// What a run of the source is, for its color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum MdRole {
    #[default]
    Plain,
    /// A mark of the syntax (`**`, `#`, `>`, `](url)`, a fence): dimmed.
    Marker,
    /// Inline code.
    Code,
    /// A fenced or indented code block.
    CodeBlock,
    Link,
    Quote,
    /// A list item's `-` or `1.`.
    ListMarker,
    /// A task's `[ ]` or `[x]`: a tap ticks it.
    TaskBox,
    /// A ticked task's text.
    Done,
    /// An alert's `[!WARNING]`.
    Alert {
        kind: MdAlert,
    },
    Html,
}

/// A run of the source and how it shows.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MdRun {
    /// UTF-16 offset.
    pub start: u32,
    /// UTF-16 length.
    pub len: u32,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub mono: bool,
    /// 1 to 6 on a heading's line; 0 elsewhere.
    pub heading: u8,
    pub role: MdRole,
}

/// The text after an edit, and what is selected in it (UTF-16).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MdEdit {
    pub text: String,
    pub sel_start: u32,
    pub sel_end: u32,
}

/// An inline mark the bar puts around the selection, or takes away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MdWrap {
    Bold,
    Italic,
    Strike,
    Code,
}

/// What the bar makes of the selected lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MdLines {
    /// `#`, `##`, `###`, then none again.
    Heading,
    Bullet,
    Numbered,
    Task,
    Quote,
}

/// How far a Tab moves an item in, as the desktop's editor has it.
const INDENT: usize = 4;

// MARK: Styling

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    mono: bool,
    heading: u8,
    role: MdRole,
}

/// An element open while reading: its range, and what of it its children
/// cover; the rest is its marks.
struct Frame {
    tag: Kind,
    range: Range<usize>,
    covered: Vec<Range<usize>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Item,
    Quote,
    Other,
}

/// How `text` (a note's Markdown) shows as it is edited: runs in order,
/// covering it all.
#[uniffi::export]
pub fn markdown_runs(text: String) -> Vec<MdRun> {
    let styles = styles(&text);
    runs(&text, &styles)
}

fn styles(text: &str) -> Vec<Style> {
    let mut st = vec![Style::default(); text.len()];
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut stack: Vec<Frame> = vec![];
    let cover = |stack: &mut Vec<Frame>, r: &Range<usize>| {
        if let Some(f) = stack.last_mut() {
            f.covered.push(r.clone());
        }
    };
    for (ev, r) in Parser::new_ext(text, opts).into_offset_iter() {
        match ev {
            Event::Start(tag) => {
                cover(&mut stack, &r);
                let kind = match &tag {
                    Tag::Item => Kind::Item,
                    Tag::BlockQuote(_) => Kind::Quote,
                    _ => Kind::Other,
                };
                apply(&mut st, &r, |s| match &tag {
                    Tag::Strong => s.bold = true,
                    Tag::Emphasis => s.italic = true,
                    Tag::Strikethrough => s.strike = true,
                    Tag::Heading { level, .. } => s.heading = *level as u8,
                    Tag::BlockQuote(_) => s.role = MdRole::Quote,
                    Tag::CodeBlock(_) => {
                        s.mono = true;
                        s.role = MdRole::CodeBlock;
                    }
                    Tag::Link { .. } => s.role = MdRole::Link,
                    Tag::Table(_) => s.mono = true,
                    _ => {}
                });
                stack.push(Frame {
                    tag: kind,
                    range: r,
                    covered: vec![],
                });
            }
            Event::End(_) => {
                let Some(f) = stack.pop() else { continue };
                marks(text, &mut st, &f);
            }
            Event::Code(_) => {
                cover(&mut stack, &r);
                let ticks = text[r.clone()].bytes().take_while(|b| *b == b'`').count();
                let inner = (r.start + ticks)..r.end.saturating_sub(ticks).max(r.start + ticks);
                apply(&mut st, &r, |s| s.mono = true);
                apply(&mut st, &inner, |s| s.role = MdRole::Code);
                apply(&mut st, &(r.start..inner.start), |s| {
                    s.role = MdRole::Marker
                });
                apply(&mut st, &(inner.end..r.end), |s| s.role = MdRole::Marker);
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                cover(&mut stack, &r);
                apply(&mut st, &r, |s| s.role = MdRole::Html);
            }
            Event::Rule => {
                cover(&mut stack, &r);
                apply(&mut st, &r, |s| s.role = MdRole::Marker);
            }
            Event::TaskListMarker(done) => {
                // In a loose list the box comes inside the item's paragraph,
                // whose range starts after it: no frame open may take it for
                // a mark.
                for f in &mut stack {
                    f.covered.push(r.clone());
                }
                apply(&mut st, &r, |s| s.role = MdRole::TaskBox);
                if done {
                    let end = text[r.end..].find('\n').map_or(text.len(), |i| r.end + i);
                    apply(&mut st, &(r.end..end), |s| {
                        s.strike = true;
                        s.role = MdRole::Done;
                    });
                }
            }
            _ => cover(&mut stack, &r),
        }
    }
    links(text, &mut st);
    st
}

/// Sets what `f` sets on each byte of `r`.
fn apply(st: &mut [Style], r: &Range<usize>, f: impl Fn(&mut Style)) {
    let end = r.end.min(st.len());
    for s in &mut st[r.start.min(end)..end] {
        f(s);
    }
}

/// What of `f` its children did not cover: its marks. An item's first is its
/// `-` or `1.`; a quote's first line may hold an alert's `[!NOTE]`.
fn marks(text: &str, st: &mut [Style], f: &Frame) {
    let mut covered = f.covered.clone();
    covered.sort_by_key(|r| r.start);
    let mut gaps = vec![];
    let mut at = f.range.start;
    for c in &covered {
        if c.start > at {
            gaps.push(at..c.start);
        }
        at = at.max(c.end);
    }
    if at < f.range.end {
        gaps.push(at..f.range.end);
    }
    let mut first = true;
    for g in gaps {
        let Some(g) = trimmed(text, g) else { continue };
        let role = if f.tag == Kind::Item && first {
            MdRole::ListMarker
        } else {
            MdRole::Marker
        };
        first = false;
        // A mark is not struck with what it marks: it is dimmed.
        apply(st, &g, |s| {
            s.role = role;
            s.strike = false;
        });
    }
    if f.tag == Kind::Quote {
        alert_mark(text, st, &f.range);
    }
}

/// `g` without the white space at its ends; None when only that.
fn trimmed(text: &str, g: Range<usize>) -> Option<Range<usize>> {
    let s = &text[g.clone()];
    let start = g.start + (s.len() - s.trim_start().len());
    let end = g.end - (s.len() - s.trim_end().len());
    (start < end).then_some(start..end)
}

/// A quote whose first line is `> [!NOTE]` (alone, or before its text): the
/// marker in the alert's color.
fn alert_mark(text: &str, st: &mut [Style], quote: &Range<usize>) {
    let line_end = text[quote.start..]
        .find('\n')
        .map_or(text.len(), |i| quote.start + i);
    let line = &text[quote.start..line_end];
    let Some(gt) = line.find('>') else { return };
    let rest = &line[gt + 1..];
    let body = rest.trim_start();
    let at = quote.start + gt + 1 + (rest.len() - body.len());
    if let Some((kind, _, _)) = zsyntax::alert_marker(body)
        && let Some(close) = body.find(']')
    {
        let kind = crate::markdown::alert_of(kind);
        apply(st, &(at..at + close + 1), |s| {
            s.role = MdRole::Alert { kind }
        });
    }
}

/// Bare `https://` addresses in plain text: links, as the desktop's editor
/// has them.
fn links(text: &str, st: &mut [Style]) {
    let mut from = 0;
    while let Some(at) = ["https://", "http://"]
        .iter()
        .filter_map(|p| text[from..].find(p).map(|i| from + i))
        .min()
    {
        let end = zsyntax::url_end(text, at).max(at + 1);
        let plain = st
            .get(at)
            .is_some_and(|s| matches!(s.role, MdRole::Plain | MdRole::Quote | MdRole::Done));
        if plain && end - at > "https://".len() {
            apply(st, &(at..end), |s| s.role = MdRole::Link);
        }
        from = end;
    }
}

/// Styles per byte as runs, in UTF-16.
fn runs(text: &str, st: &[Style]) -> Vec<MdRun> {
    let u16s = utf16_at(text);
    let mut out: Vec<MdRun> = vec![];
    let mut start = 0;
    for i in 1..=st.len() {
        if i == st.len() || st[i] != st[start] {
            // Between two bytes of one character a style never changes: the
            // ranges read are on characters.
            let s = st[start];
            let (a, b) = (u16s[start], u16s[i]);
            if b > a {
                out.push(MdRun {
                    start: a,
                    len: b - a,
                    bold: s.bold,
                    italic: s.italic,
                    strike: s.strike,
                    mono: s.mono,
                    heading: s.heading,
                    role: s.role,
                });
            }
            start = i;
        }
    }
    out
}

/// For each byte of `text` (and its end), the UTF-16 offset of the
/// character it is in.
fn utf16_at(text: &str) -> Vec<u32> {
    let mut out = vec![0u32; text.len() + 1];
    let mut n = 0u32;
    for (i, c) in text.char_indices() {
        for slot in &mut out[i..i + c.len_utf8()] {
            *slot = n;
        }
        n += c.len_utf16() as u32;
    }
    out[text.len()] = n;
    out
}

// MARK: Offsets

/// The byte of `text` at UTF-16 offset `u` (clamped to the text, and to a
/// character's start).
fn byte_of(text: &str, u: u32) -> usize {
    let mut n = 0u32;
    for (i, c) in text.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16() as u32;
    }
    text.len()
}

fn u16_of(text: &str, b: usize) -> u32 {
    text[..b.min(text.len())].encode_utf16().count() as u32
}

fn edit(text: String, sel: Range<usize>) -> MdEdit {
    MdEdit {
        sel_start: u16_of(&text, sel.start),
        sel_end: u16_of(&text, sel.end),
        text,
    }
}

fn line_start(text: &str, b: usize) -> usize {
    text[..b].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, b: usize) -> usize {
    text[b..].find('\n').map_or(text.len(), |i| b + i)
}

// MARK: Inline marks

/// Puts `wrap`'s marks around the selection, or takes them away when they
/// are there (just inside it, or just outside); an empty selection gets an
/// empty pair, the caret in it.
#[uniffi::export]
pub fn md_toggle_wrap(text: String, start: u32, end: u32, wrap: MdWrap) -> MdEdit {
    let (c, n) = match wrap {
        MdWrap::Bold => ('*', 2),
        MdWrap::Italic => ('*', 1),
        MdWrap::Strike => ('~', 2),
        MdWrap::Code => ('`', 1),
    };
    let mark: String = std::iter::repeat_n(c, n).collect();
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let before = text[..s].chars().rev().take_while(|x| *x == c).count();
    let after = text[e..].chars().take_while(|x| *x == c).count();
    // `*` is italic on its own, bold twice and both three times: a run of
    // marks holds this one when it has it, odd for italic.
    let holds = |k: usize| if n == 1 { k % 2 == 1 } else { k >= n };
    let mut t = text.clone();
    if s == e {
        if holds(before) && holds(after) && before == after {
            t.replace_range(s - n..s + n, "");
            return edit(t, s - n..s - n);
        }
        t.insert_str(s, &mark.repeat(2));
        return edit(t, s + n..s + n);
    }
    let sel = &text[s..e];
    let inside_open = sel.chars().take_while(|x| *x == c).count();
    let inside_close = sel.chars().rev().take_while(|x| *x == c).count();
    if sel.len() > 2 * n && holds(inside_open) && holds(inside_close) {
        t.replace_range(e - n..e, "");
        t.replace_range(s..s + n, "");
        return edit(t, s..e - 2 * n);
    }
    if holds(before) && holds(after) {
        t.replace_range(e..e + n, "");
        t.replace_range(s - n..s, "");
        return edit(t, s - n..e - n);
    }
    t.insert_str(e, &mark);
    t.insert_str(s, &mark);
    edit(t, s + n..e + n)
}

/// `[selection](https://)`, the address selected to type over; with nothing
/// selected, `[](https://)` and the caret in the brackets.
#[uniffi::export]
pub fn md_link(text: String, start: u32, end: u32) -> MdEdit {
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let label = text[s..e].to_string();
    let link = format!("[{label}](https://)");
    let mut t = text;
    t.replace_range(s..e, &link);
    if label.is_empty() {
        return edit(t, s + 1..s + 1);
    }
    let url = s + label.len() + 3;
    edit(t, url..url + "https://".len())
}

// MARK: Lines

/// A line's list marker: its length with the space after it, its indent,
/// whether ordered, and its number; as the desktop's editor reads it.
fn list_prefix(line: &str) -> Option<(usize, usize, bool, u32)> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let indent = i;
    if i < b.len() && matches!(b[i], b'-' | b'*' | b'+') && b.get(i + 1) == Some(&b' ') {
        return Some((i + 2, indent, false, 0));
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > ds && matches!(b.get(i), Some(b'.') | Some(b')')) && b.get(i + 1) == Some(&b' ') {
        let num = line[ds..i].parse::<u32>().unwrap_or(1);
        return Some((i + 2, indent, true, num));
    }
    None
}

/// A task's marker, `- [ ] `: its length, and whether ticked.
fn task_prefix(line: &str) -> Option<(usize, bool)> {
    let (len, ..) = list_prefix(line)?;
    let rest = &line.as_bytes()[len..];
    (rest.len() >= 4
        && rest[0] == b'['
        && rest[2] == b']'
        && rest[3] == b' '
        && matches!(rest[1], b' ' | b'x' | b'X'))
    .then(|| (len + 4, matches!(rest[1], b'x' | b'X')))
}

fn heading_level(line: &str) -> usize {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
        hashes
    } else {
        0
    }
}

/// The lines a selection touches, as byte ranges without their newlines.
fn lines_of(text: &str, s: usize, e: usize) -> Range<usize> {
    let first = line_start(text, s);
    // A selection ending at a line's start leaves that line out.
    let e = if e > s && e == line_start(text, e) {
        e - 1
    } else {
        e
    };
    first..line_end(text, e.max(first))
}

/// Makes the selected lines headings, list items, tasks or a quote, or takes
/// that away when they all are already; headings go `#`, `##`, `###`, none.
#[uniffi::export]
pub fn md_toggle_lines(text: String, start: u32, end: u32, kind: MdLines) -> MdEdit {
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let block = lines_of(&text, s, e);
    let lines: Vec<&str> = text[block.clone()].split('\n').collect();
    let filled = |l: &str| !l.trim().is_empty();
    // The kind a line has, and the text after its marker.
    let content = |l: &str| -> (usize, &'static str) {
        if let Some((len, _)) = task_prefix(l) {
            return (len, "task");
        }
        if let Some((len, _, ordered, _)) = list_prefix(l) {
            return (len, if ordered { "numbered" } else { "bullet" });
        }
        (l.len() - l.trim_start().len(), "")
    };
    let indent = |l: &str| l.len() - l.trim_start().len();
    let all = |want: &str| {
        lines
            .iter()
            .filter(|l| filled(l))
            .all(|l| content(l).1 == want)
    };
    let mut n = 0;
    let new: Vec<String> = match kind {
        MdLines::Heading => {
            let next = (heading_level(lines[0]) + 1) % 4;
            lines
                .iter()
                .map(|l| {
                    let h = heading_level(l);
                    let body = if h > 0 { &l[h + 1..] } else { l.trim_start() };
                    if !filled(l) || next == 0 {
                        body.to_string()
                    } else {
                        format!("{} {body}", "#".repeat(next))
                    }
                })
                .collect()
        }
        MdLines::Quote => {
            let quoted = lines
                .iter()
                .filter(|l| filled(l))
                .all(|l| l.starts_with('>'));
            lines
                .iter()
                .map(|l| {
                    if quoted {
                        l.strip_prefix("> ")
                            .or_else(|| l.strip_prefix('>'))
                            .unwrap_or(l)
                            .to_string()
                    } else if filled(l) {
                        format!("> {l}")
                    } else {
                        l.to_string()
                    }
                })
                .collect()
        }
        MdLines::Bullet | MdLines::Numbered | MdLines::Task => {
            let name = match kind {
                MdLines::Bullet => "bullet",
                MdLines::Numbered => "numbered",
                _ => "task",
            };
            let off = all(name);
            lines
                .iter()
                .map(|l| {
                    if !filled(l) {
                        return l.to_string();
                    }
                    let (len, _) = content(l);
                    let (ws, body) = (&l[..indent(l)], &l[len..]);
                    if off {
                        return format!("{ws}{body}");
                    }
                    n += 1;
                    match kind {
                        MdLines::Bullet => format!("{ws}- {body}"),
                        MdLines::Numbered => format!("{ws}{n}. {body}"),
                        _ => format!("{ws}- [ ] {body}"),
                    }
                })
                .collect()
        }
    };
    let joined = new.join("\n");
    let mut t = text.clone();
    t.replace_range(block.clone(), &joined);
    if s == e {
        // The caret stays in its words: moved as its line's marker moved.
        let delta = new[0].len() as isize - lines[0].len() as isize;
        let at = (s as isize + delta).max(block.start as isize) as usize;
        let at = at.min(block.start + new[0].len());
        return edit(t, at..at);
    }
    edit(t, block.start..block.start + joined.len())
}

/// Puts the selected lines in a code block, or takes them out of the one
/// they are; with nothing selected, an empty block, the caret in it.
#[uniffi::export]
pub fn md_code_block(text: String, start: u32, end: u32) -> MdEdit {
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let mut t = text.clone();
    if s == e {
        let ls = line_start(&text, s);
        let le = line_end(&text, s);
        if text[ls..le].trim().is_empty() {
            t.replace_range(ls..le, "```\n\n```");
            return edit(t, ls + 4..ls + 4);
        }
        t.insert_str(le, "\n```\n\n```");
        let at = le + 5;
        return edit(t, at..at);
    }
    let block = lines_of(&text, s, e);
    let body = &text[block.clone()];
    if let Some(inner) = body
        .strip_prefix("```")
        .and_then(|b| b.split_once('\n'))
        .and_then(|(_, rest)| rest.strip_suffix("```"))
    {
        let inner = inner.strip_suffix('\n').unwrap_or(inner).to_string();
        t.replace_range(block.clone(), &inner);
        return edit(t, block.start..block.start + inner.len());
    }
    let fenced = format!("```\n{body}\n```");
    t.replace_range(block.clone(), &fenced);
    edit(t, block.start + 4..block.start + 4 + body.len())
}

/// Return at `at`, as the desktop's editor has it: an item carries its list
/// on (a task as a task, a number as the next), a quote its `> `; on an empty
/// item, or an empty quote line, the marker goes and the list ends. None:
/// a plain newline.
#[uniffi::export]
pub fn md_newline(text: String, at: u32) -> Option<MdEdit> {
    let c = byte_of(&text, at);
    let ls = line_start(&text, c);
    let le = line_end(&text, c);
    let line = &text[ls..le];
    let mut t = text.clone();
    if let Some((len, indent, ordered, num)) = list_prefix(line) {
        let task = task_prefix(line);
        let content = task.map_or(len, |(l, _)| l);
        if line[content.min(line.len())..].trim().is_empty() {
            t.replace_range(ls..le, "");
            return Some(edit(t, ls..ls));
        }
        let ws = &line[..indent];
        let bullet = line.as_bytes()[indent] as char;
        let next = if task.is_some() {
            format!("\n{ws}{bullet} [ ] ")
        } else if ordered {
            format!("\n{ws}{}. ", num + 1)
        } else {
            format!("\n{ws}{bullet} ")
        };
        t.insert_str(c, &next);
        return Some(edit(t, c + next.len()..c + next.len()));
    }
    if line.starts_with('>') {
        if line.trim_start_matches('>').trim().is_empty() {
            t.replace_range(ls..le, "");
            return Some(edit(t, ls..ls));
        }
        t.insert_str(c, "\n> ");
        return Some(edit(t, c + 3..c + 3));
    }
    None
}

/// Tab and its opposite on the selected lines: list items and quotes move
/// in by four spaces (an ordered item starts a nested list, so at 1), or
/// out by up to four (or a tab); elsewhere, Tab puts four spaces at the
/// caret.
#[uniffi::export]
pub fn md_indent(text: String, start: u32, end: u32, outdent: bool) -> MdEdit {
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let block = lines_of(&text, s, e);
    let lines: Vec<&str> = text[block.clone()].split('\n').collect();
    let pad = " ".repeat(INDENT);
    let single = lines.len() == 1;
    let item = |l: &str| list_prefix(l).is_some() || l.trim_start().starts_with('>');
    if !outdent && single && !item(lines[0]) {
        let mut t = text.clone();
        t.insert_str(s, &pad);
        return edit(t, s + INDENT..e + INDENT);
    }
    let new: Vec<String> = lines
        .iter()
        .map(|l| {
            if outdent {
                let gone = if l.starts_with('\t') {
                    1
                } else {
                    l.bytes().take(INDENT).take_while(|b| *b == b' ').count()
                };
                return l[gone..].to_string();
            }
            if l.trim().is_empty() {
                return l.to_string();
            }
            match list_prefix(l) {
                Some((_, ws, true, _)) => {
                    let digits = l[ws..].bytes().take_while(u8::is_ascii_digit).count();
                    format!("{pad}{}1{}", &l[..ws], &l[ws + digits..])
                }
                _ => format!("{pad}{l}"),
            }
        })
        .collect();
    let joined = new.join("\n");
    let mut t = text.clone();
    t.replace_range(block.clone(), &joined);
    if s == e {
        let delta = new[0].len() as isize - lines[0].len() as isize;
        let at = (s as isize + delta).max(block.start as isize) as usize;
        return edit(t, at..at);
    }
    edit(t, block.start..block.start + joined.len())
}

/// `block` (an image's line) on a line of its own where the selection is;
/// the caret after it.
#[uniffi::export]
pub fn md_insert_block(text: String, start: u32, end: u32, block: String) -> MdEdit {
    let (s, e) = (byte_of(&text, start), byte_of(&text, end.max(start)));
    let before = if s == 0 || text[..s].ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let after = if e == text.len() || text[e..].starts_with('\n') {
        ""
    } else {
        "\n"
    };
    let ins = format!("{before}{block}{after}");
    let mut t = text;
    t.replace_range(s..e, &ins);
    let at = s + before.len() + block.len();
    edit(t, at..at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The style of the text `what` (its first match) in `md`.
    fn style_of(md: &str, what: &str) -> MdRun {
        let at = u16_of(md, md.find(what).expect("in the text"));
        markdown_runs(md.into())
            .into_iter()
            .find(|r| r.start <= at && at < r.start + r.len)
            .expect("a run")
    }

    #[test]
    fn runs_cover_the_text_in_utf16() {
        let md = "# Chào 👋\n\n**đậm** và `mã`\n";
        let runs = markdown_runs(md.into());
        let total: u32 = runs.iter().map(|r| r.len).sum();
        assert_eq!(total as usize, md.encode_utf16().count());
        let mut at = 0;
        for r in &runs {
            assert_eq!(r.start, at, "in order, no gaps");
            at += r.len;
        }
        assert_eq!(style_of(md, "Chào").heading, 1);
        assert_eq!(style_of(md, "# ").role, MdRole::Marker);
        assert!(style_of(md, "đậm").bold);
        assert_eq!(style_of(md, "**").role, MdRole::Marker);
        let struck = "~~gone~~";
        assert!(style_of(struck, "gone").strike);
        assert!(!style_of(struck, "~~").strike, "its marks only dimmed");
        assert!(style_of(md, "mã").mono);
        assert_eq!(style_of(md, "mã").role, MdRole::Code);
    }

    #[test]
    fn links_lists_tasks_quotes_alerts_code_tables() {
        let md = "See [docs](https://a.example) or https://b.example/x_y.\n\n- one\n1. two\n- [ ] todo\n- [x] done\n\n> [!WARNING]\n> careful\n\n> [!TIP] tmux\n\n```bash\nls\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        assert_eq!(style_of(md, "docs").role, MdRole::Link);
        assert_eq!(style_of(md, "](https://a").role, MdRole::Marker);
        assert_eq!(style_of(md, "https://b").role, MdRole::Link);
        assert_eq!(style_of(md, "x_y").role, MdRole::Link);
        assert_eq!(style_of(md, "- one").role, MdRole::ListMarker);
        assert_eq!(style_of(md, "1. two").role, MdRole::ListMarker);
        assert_eq!(style_of(md, "[ ] todo").role, MdRole::TaskBox);
        let done = style_of(md, "done");
        assert!(done.strike && done.role == MdRole::Done);
        assert_eq!(
            style_of(md, "[!WARNING]").role,
            MdRole::Alert {
                kind: MdAlert::Warning
            }
        );
        assert_eq!(style_of(md, "careful").role, MdRole::Quote);
        assert_eq!(style_of(md, "> careful").role, MdRole::Marker);
        assert_eq!(
            style_of(md, "[!TIP]").role,
            MdRole::Alert { kind: MdAlert::Tip }
        );
        assert_eq!(style_of(md, "```bash").role, MdRole::Marker);
        let code = style_of(md, "ls\n");
        assert!(code.mono && code.role == MdRole::CodeBlock);
        assert!(style_of(md, "| 1").mono);
        assert_eq!(style_of(md, "|---|").role, MdRole::Marker);
    }

    #[test]
    fn a_loose_lists_tasks_are_boxes_too() {
        // A blank line between items: each is a paragraph.
        let md = "- [ ] one\n\n- [x] two\n";
        assert_eq!(style_of(md, "[ ] one").role, MdRole::TaskBox);
        assert_eq!(style_of(md, "[x] two").role, MdRole::TaskBox);
        assert_eq!(style_of(md, "- [ ]").role, MdRole::ListMarker);
        assert_eq!(style_of(md, "two").role, MdRole::Done);
    }

    fn sel(e: &MdEdit) -> &str {
        let s = byte_of(&e.text, e.sel_start);
        let t = byte_of(&e.text, e.sel_end);
        &e.text[s..t]
    }

    fn at(text: &str, what: &str) -> (u32, u32) {
        let s = text.find(what).expect("in the text");
        (u16_of(text, s), u16_of(text, s + what.len()))
    }

    #[test]
    fn marks_go_on_and_off() {
        let t = "make it loud now";
        let (s, e) = at(t, "loud");
        let on = md_toggle_wrap(t.into(), s, e, MdWrap::Bold);
        assert_eq!(on.text, "make it **loud** now");
        assert_eq!(sel(&on), "loud");
        let off = md_toggle_wrap(on.text.clone(), on.sel_start, on.sel_end, MdWrap::Bold);
        assert_eq!(off.text, t);
        // Italic beside bold leaves the bold be.
        let it = md_toggle_wrap(on.text.clone(), on.sel_start, on.sel_end, MdWrap::Italic);
        assert_eq!(it.text, "make it ***loud*** now");
        let back = md_toggle_wrap(it.text.clone(), it.sel_start, it.sel_end, MdWrap::Italic);
        assert_eq!(back.text, "make it **loud** now");
        // Selected with its marks: taken off too.
        let (s, e) = at(&on.text, "**loud**");
        assert_eq!(md_toggle_wrap(on.text.clone(), s, e, MdWrap::Bold).text, t);
        // Nothing selected: a pair, the caret in it; again, it goes.
        let empty = md_toggle_wrap("ab".into(), 1, 1, MdWrap::Code);
        assert_eq!((empty.text.as_str(), empty.sel_start), ("a``b", 2));
        assert_eq!(md_toggle_wrap(empty.text, 2, 2, MdWrap::Code).text, "ab");
        // UTF-16 offsets past an emoji and Vietnamese.
        let v = "👋 chữ đậm";
        let (s, e) = at(v, "đậm");
        let b = md_toggle_wrap(v.into(), s, e, MdWrap::Strike);
        assert_eq!(b.text, "👋 chữ ~~đậm~~");
        assert_eq!(sel(&b), "đậm");
    }

    #[test]
    fn links_wrap_the_selection() {
        let (s, e) = at("read the docs", "docs");
        let l = md_link("read the docs".into(), s, e);
        assert_eq!(l.text, "read the [docs](https://)");
        assert_eq!(sel(&l), "https://");
        let empty = md_link("x".into(), 1, 1);
        assert_eq!((empty.text.as_str(), empty.sel_start), ("x[](https://)", 2));
    }

    #[test]
    fn lines_become_lists_and_back() {
        let t = "alpha\nbeta\n\ngamma";
        let all = md_toggle_lines(
            t.into(),
            0,
            t.encode_utf16().count() as u32,
            MdLines::Numbered,
        );
        assert_eq!(all.text, "1. alpha\n2. beta\n\n3. gamma");
        let tasks = md_toggle_lines(all.text.clone(), all.sel_start, all.sel_end, MdLines::Task);
        assert_eq!(tasks.text, "- [ ] alpha\n- [ ] beta\n\n- [ ] gamma");
        let off = md_toggle_lines(
            tasks.text.clone(),
            tasks.sel_start,
            tasks.sel_end,
            MdLines::Task,
        );
        assert_eq!(off.text, t);
        // One line, the caret in it: it keeps its place in the words.
        let one = md_toggle_lines("hello".into(), 2, 2, MdLines::Bullet);
        assert_eq!((one.text.as_str(), one.sel_start), ("- hello", 4));
        let q = md_toggle_lines("a\nb".into(), 0, 3, MdLines::Quote);
        assert_eq!(q.text, "> a\n> b");
        assert_eq!(md_toggle_lines(q.text, 0, 7, MdLines::Quote).text, "a\nb");
    }

    #[test]
    fn headings_go_round() {
        let mut t = "Title".to_string();
        let mut seen = vec![];
        for _ in 0..4 {
            t = md_toggle_lines(t.clone(), 0, 0, MdLines::Heading).text;
            seen.push(t.clone());
        }
        assert_eq!(seen, ["# Title", "## Title", "### Title", "Title"]);
    }

    #[test]
    fn return_carries_a_list_on() {
        let t = "- [x] done";
        let n = md_newline(t.into(), t.len() as u32).expect("a task");
        assert_eq!(n.text, "- [x] done\n- [ ] ");
        let o = md_newline("  3. three".into(), 10).expect("ordered");
        assert_eq!(o.text, "  3. three\n  4. ");
        let q = md_newline("> said".into(), 6).expect("a quote");
        assert_eq!((q.text.as_str(), q.sel_start), ("> said\n> ", 9));
        // An empty item ends the list.
        let end = md_newline("- a\n- ".into(), 6).expect("ends");
        assert_eq!((end.text.as_str(), end.sel_start), ("- a\n", 4));
        assert!(md_newline("plain".into(), 5).is_none());
        // In the middle of an item: what follows goes to the next one.
        let split = md_newline("- one two".into(), 5).expect("split");
        assert_eq!(split.text, "- one\n-  two");
    }

    #[test]
    fn tab_moves_items_in_and_out() {
        let i = md_indent("1. a\n2. b".into(), 6, 6, false);
        assert_eq!(i.text, "1. a\n    1. b");
        let o = md_indent(i.text.clone(), i.sel_start, i.sel_end, true);
        assert_eq!(o.text, "1. a\n1. b");
        let plain = md_indent("ab".into(), 1, 1, false);
        assert_eq!((plain.text.as_str(), plain.sel_start), ("a    b", 5));
        let many = md_indent("- a\n- b".into(), 0, 7, false);
        assert_eq!(many.text, "    - a\n    - b");
    }

    #[test]
    fn code_blocks_and_images_go_on_their_lines() {
        let c = md_code_block("".into(), 0, 0);
        assert_eq!((c.text.as_str(), c.sel_start), ("```\n\n```", 4));
        let w = md_code_block("ls\npwd".into(), 0, 6);
        assert_eq!(w.text, "```\nls\npwd\n```");
        assert_eq!(sel(&w), "ls\npwd");
        let u = md_code_block(w.text.clone(), 0, w.text.len() as u32);
        assert_eq!(u.text, "ls\npwd");
        let img = md_insert_block("before after".into(), 6, 6, "![](attachment:ab)".into());
        assert_eq!(img.text, "before\n![](attachment:ab)\n after");
    }
}

#[cfg(test)]
mod timing {
    #[test]
    #[ignore = "a measure, not a check: cargo test -- --ignored --nocapture"]
    fn styling_a_long_note() {
        let one = include_str!("../../app/src/note_templates/en.md");
        for n in [10usize, 40, 160] {
            let text = one.repeat(n);
            let t = std::time::Instant::now();
            let runs = super::markdown_runs(text.clone());
            println!(
                "{} KB, {} UTF-16, {} runs: {:?}",
                text.len() / 1024,
                text.encode_utf16().count(),
                runs.len(),
                t.elapsed()
            );
        }
    }
}

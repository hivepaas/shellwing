//! A note's Markdown read into blocks the phone draws: headings, paragraphs,
//! list items and tasks, quotes and GitHub's alerts, code, tables, images,
//! rules. Parsed here (pulldown-cmark, GFM), with what counts as an alert, an
//! ordered marker and a bare link shared with the desktop's editor
//! (`zorite-markdown`), so the two read a note alike. Quotes and lists come
//! flat: each block says how deep it is in them, and the app draws the bars
//! and the indents.

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd,
};
use zorite_markdown::syntax::{self as zsyntax, AlertKind};

/// A run of text and how it looks.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MdSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// Where it goes, when it is a link.
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MdAlert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MdAlign {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MdCell {
    pub spans: Vec<MdSpan>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MdRow {
    pub cells: Vec<MdCell>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MdKind {
    Heading {
        level: u8,
        spans: Vec<MdSpan>,
    },
    Paragraph {
        spans: Vec<MdSpan>,
    },
    /// A list item's first lines: its marker (`•`, `3.`), or its task box.
    Item {
        marker: String,
        /// A task: ticked or not.
        task: Option<bool>,
        /// The task box's byte in the text (` ` or `x`), to tick it there.
        task_at: Option<u64>,
        spans: Vec<MdSpan>,
    },
    Code {
        lang: String,
        text: String,
    },
    Table {
        aligns: Vec<MdAlign>,
        head: Vec<MdCell>,
        rows: Vec<MdRow>,
    },
    Image {
        source: String,
        alt: String,
    },
    Rule,
    /// The title line of an alert (`> [!WARNING]`).
    AlertTitle {
        kind: MdAlert,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MdBlock {
    pub kind: MdKind,
    /// How many quotes it is in.
    pub quote: u32,
    /// Which quote (counted from 1, by the outermost) it is in; 0: none.
    /// Two quotes one after the other are two.
    pub group: u32,
    /// The innermost alert it is in.
    pub alert: Option<MdAlert>,
    /// How deep in lists it sits: an item at its list's depth less one, what
    /// continues an item at its depth.
    pub indent: u32,
}

/// The blocks `text` (a note's Markdown) shows as.
#[uniffi::export]
pub fn markdown_blocks(text: String) -> Vec<MdBlock> {
    Reader::default().read(&text)
}

/// `text` with the task box at `at` (from [`MdKind::Item`]) ticked or
/// unticked; None when no box is there (the text changed meanwhile).
#[uniffi::export]
pub fn toggle_task(text: String, at: u64) -> Option<String> {
    let at = at as usize;
    let b = text.as_bytes();
    if at == 0 || at + 1 >= b.len() || b[at - 1] != b'[' || b[at + 1] != b']' {
        return None;
    }
    let now = match b[at] {
        b' ' => "x",
        b'x' | b'X' => " ",
        _ => return None,
    };
    let mut out = text;
    out.replace_range(at..at + 1, now);
    Some(out)
}

/// A list being read: ordered from `next`, or not.
struct List {
    next: Option<u64>,
}

#[derive(Default)]
struct Reader {
    out: Vec<MdBlock>,
    spans: Vec<MdSpan>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    /// Each quote open: its alert, if one.
    quotes: Vec<Option<MdAlert>>,
    /// The outermost quotes opened so far: the last is the one open.
    groups: u32,
    /// A quote just opened: its first text may be an alert's marker.
    quote_fresh: bool,
    lists: Vec<List>,
    /// An item opened whose first lines are still to come: its marker, and
    /// its task box.
    item: Option<(String, Option<(bool, u64)>)>,
    heading: Option<u8>,
    code: Option<(String, String)>,
    image: Option<(String, String)>,
    table: Option<(Vec<MdAlign>, Vec<MdCell>, Vec<MdRow>)>,
    row: Vec<MdCell>,
}

impl Reader {
    fn read(mut self, text: &str) -> Vec<MdBlock> {
        let opts = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_GFM;
        for (ev, range) in Parser::new_ext(text, opts).into_offset_iter() {
            match ev {
                Event::Start(tag) => self.start(tag),
                Event::End(end) => self.end(end),
                Event::Text(t) => self.text(&t),
                Event::Code(t) => {
                    let mut s = self.style();
                    s.code = true;
                    s.text = t.to_string();
                    self.push_span(s);
                }
                Event::Html(t) | Event::InlineHtml(t) => {
                    if let Some(c) = &mut self.code {
                        c.1.push_str(&t);
                    } else {
                        self.text(t.trim_end_matches('\n'));
                    }
                }
                Event::SoftBreak | Event::HardBreak => self.text("\n"),
                Event::Rule => {
                    self.flush();
                    self.block(MdKind::Rule);
                }
                Event::TaskListMarker(done) => {
                    // `[ ]` or `[x]`: the box is the byte after `[`.
                    let at = text[range.clone()].find('[').map(|i| range.start + i + 1);
                    if let (Some((_, task)), Some(at)) = (&mut self.item, at) {
                        *task = Some((done, at as u64));
                    }
                }
                Event::FootnoteReference(t) => self.text(&format!("[^{t}]")),
                Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t),
            }
        }
        self.flush();
        self.out
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::BlockQuote(kind) => {
                self.flush();
                let alert = kind.map(|k| match k {
                    BlockQuoteKind::Note => MdAlert::Note,
                    BlockQuoteKind::Tip => MdAlert::Tip,
                    BlockQuoteKind::Important => MdAlert::Important,
                    BlockQuoteKind::Warning => MdAlert::Warning,
                    BlockQuoteKind::Caution => MdAlert::Caution,
                });
                if self.quotes.is_empty() {
                    self.groups += 1;
                }
                self.quotes.push(alert);
                self.quote_fresh = alert.is_none();
                if let Some(a) = alert {
                    self.block(MdKind::AlertTitle { kind: a });
                }
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => {
                        l.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(List { next: start });
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(List { next: Some(n) }) => {
                        let m = zsyntax::ordered_marker(depth, *n as u32);
                        *n += 1;
                        m
                    }
                    _ => ["•", "◦", "▪"][depth % 3].to_string(),
                };
                self.item = Some((marker, None));
            }
            Tag::Table(aligns) => {
                self.flush();
                let aligns = aligns
                    .into_iter()
                    .map(|a| match a {
                        Alignment::None => MdAlign::None,
                        Alignment::Left => MdAlign::Left,
                        Alignment::Center => MdAlign::Center,
                        Alignment::Right => MdAlign::Right,
                    })
                    .collect();
                self.table = Some((aligns, vec![], vec![]));
            }
            Tag::TableRow => self.row.clear(),
            Tag::TableCell => self.spans.clear(),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                let spans = linkify(std::mem::take(&mut self.spans));
                if let Some(level) = self.heading.take() {
                    self.block(MdKind::Heading { level, spans });
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quotes.pop();
                self.quote_fresh = false;
            }
            TagEnd::CodeBlock => {
                if let Some((lang, text)) = self.code.take() {
                    let text = text.strip_suffix('\n').unwrap_or(&text).to_string();
                    self.block(MdKind::Code { lang, text });
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => self.flush(),
            TagEnd::TableCell => {
                let spans = linkify(std::mem::take(&mut self.spans));
                self.row.push(MdCell { spans });
            }
            TagEnd::TableHead => {
                let cells = std::mem::take(&mut self.row);
                if let Some(t) = &mut self.table {
                    t.1 = cells;
                }
            }
            TagEnd::TableRow => {
                let cells = std::mem::take(&mut self.row);
                if let Some(t) = &mut self.table {
                    t.2.push(MdRow { cells });
                }
            }
            TagEnd::Table => {
                if let Some((aligns, head, rows)) = self.table.take() {
                    self.block(MdKind::Table { aligns, head, rows });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if let Some((source, alt)) = self.image.take() {
                    // On its own: what came before it is a block of its own.
                    self.flush();
                    self.block(MdKind::Image { source, alt });
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, t: &str) {
        if let Some(c) = &mut self.code {
            c.1.push_str(t);
            return;
        }
        if let Some(img) = &mut self.image {
            img.1.push_str(t);
            return;
        }
        let style = self.style();
        self.push_span(MdSpan {
            text: t.to_string(),
            ..style
        });
    }

    fn style(&self) -> MdSpan {
        MdSpan {
            text: String::new(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: false,
            link: self.links.last().cloned(),
        }
    }

    /// Adds `s`, joined to the last span when they look alike.
    fn push_span(&mut self, s: MdSpan) {
        if s.text.is_empty() {
            return;
        }
        let alike = |a: &MdSpan, b: &MdSpan| {
            (a.bold, a.italic, a.strike, a.code, &a.link)
                == (b.bold, b.italic, b.strike, b.code, &b.link)
        };
        if let Some(last) = self.spans.last_mut()
            && alike(last, &s)
        {
            last.text.push_str(&s.text);
            return;
        }
        self.spans.push(s);
    }

    /// What was read so far: an item's first lines, or a paragraph.
    fn flush(&mut self) {
        if self.table.is_some() || self.heading.is_some() {
            return;
        }
        let mut spans = linkify(std::mem::take(&mut self.spans));
        // `> [!NOTE] like so`: an alert too, as the desktop reads it.
        if std::mem::take(&mut self.quote_fresh) {
            let first: String = spans.iter().map(|s| s.text.as_str()).collect();
            if let Some((kind, strip, _)) = zsyntax::alert_marker(&first) {
                let a = alert_of(kind);
                if let Some(q) = self.quotes.last_mut() {
                    *q = Some(a);
                }
                self.block(MdKind::AlertTitle { kind: a });
                spans = drop_front(spans, strip);
            }
        }
        if let Some((marker, task)) = self.item.take() {
            let indent = self.lists.len().saturating_sub(1) as u32;
            self.out.push(MdBlock {
                kind: MdKind::Item {
                    marker,
                    task: task.map(|t| t.0),
                    task_at: task.map(|t| t.1),
                    spans,
                },
                quote: self.quotes.len() as u32,
                group: self.group(),
                alert: self.quotes.iter().rev().find_map(|a| *a),
                indent,
            });
            return;
        }
        if !spans.iter().all(|s| s.text.trim().is_empty()) {
            self.block(MdKind::Paragraph { spans });
        }
    }

    fn block(&mut self, kind: MdKind) {
        self.out.push(MdBlock {
            kind,
            quote: self.quotes.len() as u32,
            group: self.group(),
            alert: self.quotes.iter().rev().find_map(|a| *a),
            indent: self.lists.len() as u32,
        });
    }

    fn group(&self) -> u32 {
        if self.quotes.is_empty() {
            0
        } else {
            self.groups
        }
    }
}

/// `spans` with bare `https://` addresses made links, as the desktop's
/// editor has them; links and code are left as they are.
fn linkify(spans: Vec<MdSpan>) -> Vec<MdSpan> {
    let mut out = vec![];
    for s in spans {
        if s.link.is_some() || s.code {
            out.push(s);
            continue;
        }
        let mut rest = s.text.as_str();
        while let Some(at) = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min()
        {
            let end = zsyntax::url_end(rest, at).max(at);
            if end - at <= "https://".len() {
                break;
            }
            if at > 0 {
                out.push(MdSpan {
                    text: rest[..at].to_string(),
                    ..s.clone()
                });
            }
            out.push(MdSpan {
                text: rest[at..end].to_string(),
                link: Some(rest[at..end].to_string()),
                ..s.clone()
            });
            rest = &rest[end..];
        }
        if !rest.is_empty() {
            out.push(MdSpan {
                text: rest.to_string(),
                ..s
            });
        }
    }
    out
}

/// `spans` without their first `n` bytes of text.
fn drop_front(spans: Vec<MdSpan>, mut n: usize) -> Vec<MdSpan> {
    let mut out = vec![];
    for mut s in spans {
        if n >= s.text.len() {
            n -= s.text.len();
            continue;
        }
        if n > 0 {
            let cut = (n..=s.text.len())
                .find(|i| s.text.is_char_boundary(*i))
                .unwrap_or(s.text.len());
            s.text = s.text[cut..].to_string();
            n = 0;
        }
        out.push(s);
    }
    out
}

pub(crate) fn alert_of(kind: AlertKind) -> MdAlert {
    match kind {
        AlertKind::Note => MdAlert::Note,
        AlertKind::Tip => MdAlert::Tip,
        AlertKind::Important => MdAlert::Important,
        AlertKind::Warning => MdAlert::Warning,
        AlertKind::Caution => MdAlert::Caution,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(md: &str) -> Vec<MdKind> {
        markdown_blocks(md.into())
            .into_iter()
            .map(|b| b.kind)
            .collect()
    }

    fn plain(spans: &[MdSpan]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn the_desktop_template_reads_whole() {
        let md = include_str!("../../app/src/note_templates/en.md");
        let blocks = markdown_blocks(md.into());
        assert!(matches!(&blocks[0].kind, MdKind::Heading { level: 1, .. }));
        let task = blocks
            .iter()
            .find_map(|b| match &b.kind {
                MdKind::Item {
                    task: Some(false),
                    task_at: Some(at),
                    spans,
                    ..
                } => Some((*at, plain(spans))),
                _ => None,
            })
            .expect("a task");
        assert!(task.1.starts_with("Tasks:"), "{}", task.1);
        assert_eq!(&md[task.0 as usize - 1..task.0 as usize + 2], "[ ]");
        assert!(blocks.iter().any(|b| matches!(&b.kind, MdKind::Code { lang, text } if lang == "bash" && text.starts_with("sudo systemctl"))));
        assert!(
            blocks
                .iter()
                .any(|b| b.quote == 1 && matches!(b.kind, MdKind::Paragraph { .. }))
        );
        let link = blocks.iter().flat_map(|b| match &b.kind {
            MdKind::Item { spans, .. } | MdKind::Paragraph { spans } => spans.clone(),
            _ => vec![],
        });
        assert!(
            link.into_iter()
                .any(|s| s.link.as_deref() == Some("https://github.com/hivepaas/shellwing"))
        );
    }

    #[test]
    fn inline_styles_and_bare_links() {
        let k = kinds("**bold** *it* ~~gone~~ `ls -la` see https://example.com/a, then");
        let MdKind::Paragraph { spans } = &k[0] else {
            panic!("{k:?}")
        };
        assert!(spans.iter().any(|s| s.bold && s.text == "bold"));
        assert!(spans.iter().any(|s| s.italic && s.text == "it"));
        assert!(spans.iter().any(|s| s.strike && s.text == "gone"));
        assert!(spans.iter().any(|s| s.code && s.text == "ls -la"));
        let url = spans.iter().find(|s| s.link.is_some()).expect("a link");
        assert_eq!(url.text, "https://example.com/a");
        assert!(plain(spans).ends_with(", then"));
    }

    #[test]
    fn lists_nest_and_number() {
        let b = markdown_blocks("1. one\n2. two\n   - inner\n\n     more of inner\n".into());
        let items: Vec<(String, u32)> = b
            .iter()
            .filter_map(|b| match &b.kind {
                MdKind::Item { marker, .. } => Some((marker.clone(), b.indent)),
                _ => None,
            })
            .collect();
        assert_eq!(items, [("1.".into(), 0), ("2.".into(), 0), ("◦".into(), 1)]);
        let more = b.last().expect("more");
        assert!(
            matches!(&more.kind, MdKind::Paragraph { spans } if plain(spans) == "more of inner")
        );
        assert_eq!(more.indent, 2);
    }

    #[test]
    fn alerts_in_both_forms_and_quotes() {
        let b = markdown_blocks(
            "> [!WARNING]\n> Reboots the host.\n\n> [!TIP] Use tmux.\n\n> plain\n".into(),
        );
        assert_eq!(
            b[0].kind,
            MdKind::AlertTitle {
                kind: MdAlert::Warning
            }
        );
        assert_eq!(b[1].alert, Some(MdAlert::Warning));
        assert_eq!(b[2].kind, MdKind::AlertTitle { kind: MdAlert::Tip });
        assert!(matches!(&b[3].kind, MdKind::Paragraph { spans } if plain(spans) == "Use tmux."));
        assert_eq!((b[4].quote, b[4].alert), (1, None));
        // Each quote its own: the warning's two blocks one, the tip another.
        let groups: Vec<u32> = b.iter().map(|b| b.group).collect();
        assert_eq!(groups, [1, 1, 2, 2, 3]);
    }

    #[test]
    fn tables_and_images() {
        let b = kinds("| Host | Port |\n|:--|--:|\n| web | 22 |\n\n![](attachment:abc)\n");
        let MdKind::Table { aligns, head, rows } = &b[0] else {
            panic!("{b:?}")
        };
        assert_eq!(aligns, &[MdAlign::Left, MdAlign::Right]);
        assert_eq!(plain(&head[1].spans), "Port");
        assert_eq!(plain(&rows[0].cells[0].spans), "web");
        assert_eq!(
            b[1],
            MdKind::Image {
                source: "attachment:abc".into(),
                alt: String::new()
            }
        );
    }

    #[test]
    fn a_task_ticks_where_it_is() {
        let md = "- [ ] backup\n- [x] restore\n";
        let at: Vec<u64> = markdown_blocks(md.into())
            .into_iter()
            .filter_map(|b| match b.kind {
                MdKind::Item { task_at, .. } => task_at,
                _ => None,
            })
            .collect();
        let ticked = toggle_task(md.into(), at[0]).expect("ticked");
        assert_eq!(ticked, "- [x] backup\n- [x] restore\n");
        assert_eq!(
            toggle_task(ticked, at[1]).expect("unticked"),
            "- [x] backup\n- [ ] restore\n"
        );
        assert!(toggle_task(md.into(), 0).is_none());
        assert!(toggle_task(md.into(), 4).is_none(), "not a box");
    }

    #[test]
    fn code_keeps_its_text() {
        let b = kinds("```\nkill $$\n<b>x</b>\n```\n");
        assert_eq!(
            b[0],
            MdKind::Code {
                lang: String::new(),
                text: "kill $$\n<b>x</b>".into()
            }
        );
    }
}

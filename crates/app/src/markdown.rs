//! Markdown, as a note shows it: read with `pulldown-cmark` into blocks
//! (headings, paragraphs, lists with tasks, quotes, code, rules, tables),
//! then drawn with GPUI's text. What is not drawn (images, HTML) shows as
//! its text.

use std::ops::Range;

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString, StrikethroughStyle,
    StyledText, UnderlineStyle, Window, div, prelude::*, px, rgb,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::colors::colors;
use crate::look::ui;

/// How a stretch of inline text is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Strong,
    Em,
    Code,
    Strike,
    Link,
}

/// A line of text with its marks, and where its links go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub marks: Vec<(Range<usize>, Mark)>,
    pub links: Vec<(Range<usize>, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, Inline),
    Para(Inline),
    Code(String),
    Quote(Vec<Block>),
    /// `start`: the first number of an ordered list.
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Rule,
    Table {
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// A task's box: ticked or not.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// What blocks are being read into.
enum Open {
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Item),
    Table(Vec<Inline>, Vec<Vec<Inline>>, bool),
    Row(Vec<Inline>),
}

/// What inline text is being read into.
enum Text {
    Para,
    Heading(u8),
    Cell,
}

#[derive(Default)]
struct Reader {
    root: Vec<Block>,
    open: Vec<Open>,
    text: Option<(Text, Inline)>,
    /// Marks begun and not yet ended: where, and what (a link's address).
    started: Vec<(usize, Mark, Option<String>)>,
    code: Option<String>,
}

impl Reader {
    fn push(&mut self, b: Block) {
        match self.open.last_mut() {
            Some(Open::Quote(blocks)) => blocks.push(b),
            Some(Open::Item(item)) => item.blocks.push(b),
            _ => self.root.push(b),
        }
    }

    /// Text outside a paragraph (a tight list's item) gets one.
    fn inline(&mut self) -> &mut Inline {
        &mut self
            .text
            .get_or_insert_with(|| (Text::Para, Inline::default()))
            .1
    }

    fn end_text(&mut self) {
        let Some((kind, inline)) = self.text.take() else {
            return;
        };
        self.started.clear();
        match kind {
            Text::Para => self.push(Block::Para(inline)),
            Text::Heading(l) => self.push(Block::Heading(l, inline)),
            Text::Cell => {
                if let Some(Open::Row(cells)) = self.open.last_mut() {
                    cells.push(inline);
                } else if let Some(Open::Table(head, _, true)) = self.open.last_mut() {
                    head.push(inline);
                }
            }
        }
    }

    fn start_mark(&mut self, m: Mark, link: Option<String>) {
        let at = self.inline().text.len();
        self.started.push((at, m, link));
    }

    fn end_mark(&mut self, m: Mark) {
        let Some(i) = self.started.iter().rposition(|(_, x, _)| *x == m) else {
            return;
        };
        let (from, _, link) = self.started.remove(i);
        let inline = self.inline();
        let range = from..inline.text.len();
        if range.is_empty() {
            return;
        }
        inline.marks.push((range.clone(), m));
        if let Some(url) = link {
            inline.links.push((range, url));
        }
    }

    fn event(&mut self, e: Event) {
        if let Some(code) = &mut self.code {
            match e {
                Event::Text(t) => code.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let mut code = self.code.take().unwrap_or_default();
                    if code.ends_with('\n') {
                        code.pop();
                    }
                    self.push(Block::Code(code));
                }
                _ => {}
            }
            return;
        }
        match e {
            Event::Start(Tag::Paragraph) => {
                self.end_text();
                self.text = Some((Text::Para, Inline::default()));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                self.end_text();
                self.text = Some((Text::Heading(level as u8), Inline::default()));
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => self.end_text(),
            Event::Start(Tag::CodeBlock(_)) => {
                self.end_text();
                self.code = Some(String::new());
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.end_text();
                self.open.push(Open::Quote(vec![]));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.end_text();
                if let Some(Open::Quote(blocks)) = self.open.pop() {
                    self.push(Block::Quote(blocks));
                }
            }
            Event::Start(Tag::List(start)) => {
                self.end_text();
                self.open.push(Open::List(start, vec![]));
            }
            Event::End(TagEnd::List(_)) => {
                self.end_text();
                if let Some(Open::List(start, items)) = self.open.pop() {
                    self.push(Block::List { start, items });
                }
            }
            Event::Start(Tag::Item) => {
                self.end_text();
                self.open.push(Open::Item(Item::default()));
            }
            Event::End(TagEnd::Item) => {
                self.end_text();
                if let Some(Open::Item(item)) = self.open.pop()
                    && let Some(Open::List(_, items)) = self.open.last_mut()
                {
                    items.push(item);
                }
            }
            Event::TaskListMarker(done) => {
                if let Some(Open::Item(item)) = self.open.last_mut() {
                    item.task = Some(done);
                }
            }
            Event::Start(Tag::Table(_)) => {
                self.end_text();
                self.open.push(Open::Table(vec![], vec![], false));
            }
            Event::Start(Tag::TableHead) => {
                if let Some(Open::Table(_, _, head)) = self.open.last_mut() {
                    *head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(Open::Table(_, _, head)) = self.open.last_mut() {
                    *head = false;
                }
            }
            Event::Start(Tag::TableRow) => self.open.push(Open::Row(vec![])),
            Event::End(TagEnd::TableRow) => {
                if let Some(Open::Row(cells)) = self.open.pop()
                    && let Some(Open::Table(_, rows, _)) = self.open.last_mut()
                {
                    rows.push(cells);
                }
            }
            Event::Start(Tag::TableCell) => self.text = Some((Text::Cell, Inline::default())),
            Event::End(TagEnd::TableCell) => self.end_text(),
            Event::End(TagEnd::Table) => {
                if let Some(Open::Table(head, rows, _)) = self.open.pop() {
                    self.push(Block::Table { head, rows });
                }
            }
            Event::Start(Tag::Strong) => self.start_mark(Mark::Strong, None),
            Event::End(TagEnd::Strong) => self.end_mark(Mark::Strong),
            Event::Start(Tag::Emphasis) => self.start_mark(Mark::Em, None),
            Event::End(TagEnd::Emphasis) => self.end_mark(Mark::Em),
            Event::Start(Tag::Strikethrough) => self.start_mark(Mark::Strike, None),
            Event::End(TagEnd::Strikethrough) => self.end_mark(Mark::Strike),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.start_mark(Mark::Link, Some(dest_url.to_string()))
            }
            Event::End(TagEnd::Link) => self.end_mark(Mark::Link),
            Event::Code(t) => {
                let inline = self.inline();
                let from = inline.text.len();
                inline.text.push_str(&t);
                let to = inline.text.len();
                inline.marks.push((from..to, Mark::Code));
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                self.inline().text.push_str(&t)
            }
            Event::SoftBreak => self.inline().text.push(' '),
            Event::HardBreak => self.inline().text.push('\n'),
            Event::Rule => {
                self.end_text();
                self.push(Block::Rule);
            }
            _ => {}
        }
    }
}

/// `text` read as Markdown.
pub fn parse(text: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Reader::default();
    for e in Parser::new_ext(text, options) {
        r.event(e);
    }
    r.end_text();
    r.root
}

/// The stretches of `i` with the marks over each: in order, apart (GPUI's
/// highlights may not overlap, and marks nest).
pub fn segments(i: &Inline) -> Vec<(Range<usize>, Vec<Mark>)> {
    let mut cuts: Vec<usize> = i.marks.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|w| {
            let r = w[0]..w[1];
            let on: Vec<Mark> = i
                .marks
                .iter()
                .filter(|(m, _)| m.start <= r.start && r.end <= m.end)
                .map(|(_, k)| *k)
                .collect();
            (!on.is_empty()).then_some((r, on))
        })
        .collect()
}

fn styled(i: &Inline) -> StyledText {
    let accent = rgb(colors().accent).into();
    let code_bg = rgb(colors().chip).into();
    let highlights = segments(i).into_iter().map(move |(r, marks)| {
        let mut style = HighlightStyle::default();
        for m in marks {
            match m {
                Mark::Strong => style.font_weight = Some(FontWeight::BOLD),
                Mark::Em => style.font_style = Some(FontStyle::Italic),
                Mark::Code => {
                    style.background_color = Some(code_bg);
                    style.color = Some(accent);
                }
                Mark::Strike => {
                    style.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        ..Default::default()
                    })
                }
                Mark::Link => {
                    style.color = Some(accent);
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        ..Default::default()
                    });
                }
            }
        }
        (r, style)
    });
    StyledText::new(SharedString::from(i.text.clone())).with_highlights(highlights)
}

/// A line of text, its links opening in the browser.
fn line(id: impl Into<gpui::ElementId>, i: &Inline) -> gpui::AnyElement {
    if i.links.is_empty() {
        return styled(i).into_any_element();
    }
    let urls: Vec<String> = i.links.iter().map(|(_, u)| u.clone()).collect();
    InteractiveText::new(id, styled(i))
        .on_click(
            i.links.iter().map(|(r, _)| r.clone()).collect(),
            move |ix, _: &mut Window, cx: &mut App| {
                if let Some(u) = urls.get(ix) {
                    cx.open_url(u);
                }
            },
        )
        .into_any_element()
}

/// The blocks drawn, one under the other; `mono` for code. `id` keeps
/// the links' elements apart.
pub fn render(blocks: &[Block], mono: SharedString, id: &str) -> gpui::Div {
    let mut out = div().flex().flex_col().gap(px(10.));
    for (i, b) in blocks.iter().enumerate() {
        out = out.child(block(b, mono.clone(), &format!("{id}-{i}")));
    }
    out
}

fn block(b: &Block, mono: SharedString, id: &str) -> gpui::AnyElement {
    match b {
        Block::Heading(level, i) => div()
            .pt(px(if *level <= 2 { 6. } else { 2. }))
            .text_size(ui(match level {
                1 => 22.,
                2 => 18.,
                3 => 15.5,
                _ => 14.,
            }))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(colors().text))
            .when(*level <= 2, |d| {
                d.pb(px(4.)).border_b_1().border_color(rgb(colors().line))
            })
            .child(line(SharedString::from(id.to_string()), i))
            .into_any_element(),
        Block::Para(i) => div()
            .text_color(rgb(colors().text))
            .child(line(SharedString::from(id.to_string()), i))
            .into_any_element(),
        Block::Code(code) => div()
            .px(px(12.))
            .py(px(10.))
            .rounded(px(6.))
            .bg(rgb(colors().chip))
            .border_1()
            .border_color(rgb(colors().line))
            .font_family(mono)
            .text_size(ui(12.5))
            .text_color(rgb(colors().text))
            .child(StyledText::new(SharedString::from(code.clone())))
            .into_any_element(),
        Block::Quote(blocks) => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(rgb(colors().accent))
            .text_color(rgb(colors().soft))
            .child(render(blocks, mono, id))
            .into_any_element(),
        Block::List { start, items } => {
            let mut list = div().flex().flex_col().gap(px(4.));
            for (n, item) in items.iter().enumerate() {
                let marker = match (item.task, start) {
                    (Some(true), _) => "☑".to_string(),
                    (Some(false), _) => "☐".to_string(),
                    (None, Some(s)) => format!("{}.", s + n as u64),
                    (None, None) => "•".to_string(),
                };
                list = list.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(20.))
                                .flex_shrink_0()
                                .flex()
                                .justify_end()
                                .text_color(rgb(colors().muted))
                                .child(marker),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when(item.task == Some(true), |d| {
                                    d.text_color(rgb(colors().muted))
                                })
                                .child(render(&item.blocks, mono.clone(), &format!("{id}-{n}"))),
                        ),
                );
            }
            list.into_any_element()
        }
        Block::Rule => div()
            .my(px(4.))
            .h(px(1.))
            .bg(rgb(colors().line))
            .into_any_element(),
        Block::Table { head, rows } => {
            let cell = |i: &Inline, cid: String, header: bool| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(8.))
                    .py(px(5.))
                    .when(header, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(line(SharedString::from(cid), i))
            };
            let mut table = div()
                .flex()
                .flex_col()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(colors().line))
                .overflow_hidden();
            let mut head_row = div().flex().bg(rgb(colors().chip));
            for (c, h) in head.iter().enumerate() {
                head_row = head_row.child(cell(h, format!("{id}-h{c}"), true));
            }
            table = table.child(head_row);
            for (r, row) in rows.iter().enumerate() {
                let mut line_row = div().flex().border_t_1().border_color(rgb(colors().line));
                for (c, v) in row.iter().enumerate() {
                    line_row = line_row.child(cell(v, format!("{id}-{r}-{c}"), false));
                }
                table = table.child(line_row);
            }
            table.into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Block {
        Block::Para(Inline {
            text: text.into(),
            ..Default::default()
        })
    }

    #[test]
    fn blocks_are_read_as_written() {
        let doc = "# Restart\n\nRun **this** then `that`, see [docs](https://x.y).\n\n- [x] backed up\n- [ ] restarted\n\n1. one\n2. two\n\n> careful\n\n```\nsystemctl restart app\n```\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let b = parse(doc);
        assert!(matches!(&b[0], Block::Heading(1, i) if i.text == "Restart"));
        let Block::Para(p) = &b[1] else {
            panic!("{:?}", b[1])
        };
        assert_eq!(p.text, "Run this then that, see docs.");
        assert!(p.marks.contains(&(4..8, Mark::Strong)));
        assert!(p.marks.contains(&(14..18, Mark::Code)));
        assert_eq!(p.links, vec![(24..28, "https://x.y".to_string())]);
        let Block::List { start: None, items } = &b[2] else {
            panic!("{:?}", b[2])
        };
        assert_eq!(
            items.iter().map(|i| i.task).collect::<Vec<_>>(),
            [Some(true), Some(false)]
        );
        assert_eq!(items[0].blocks, vec![para("backed up")]);
        assert!(matches!(&b[3], Block::List { start: Some(1), items } if items.len() == 2));
        assert!(matches!(&b[4], Block::Quote(q) if q == &vec![para("careful")]));
        assert_eq!(b[5], Block::Code("systemctl restart app".into()));
        assert_eq!(b[6], Block::Rule);
        let Block::Table { head, rows } = &b[7] else {
            panic!("{:?}", b[7])
        };
        assert_eq!(
            head.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1].text, "2");
    }

    #[test]
    fn nested_marks_come_apart_in_order() {
        let b = parse("[**bold *both***](u) after");
        let Block::Para(p) = &b[0] else {
            panic!("{:?}", b[0])
        };
        assert_eq!(p.text, "bold both after");
        assert_eq!(
            segments(p),
            vec![
                (0..5, vec![Mark::Strong, Mark::Link]),
                (5..9, vec![Mark::Em, Mark::Strong, Mark::Link]),
            ]
        );
    }

    #[test]
    fn nested_lists_and_breaks() {
        let b = parse("- a\n  - b\n- c  \n  d\n");
        let Block::List { items, .. } = &b[0] else {
            panic!("{:?}", b[0])
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0].blocks[1], Block::List { items, .. } if items.len() == 1));
        assert!(matches!(&items[1].blocks[0], Block::Para(i) if i.text == "c\nd"));
    }
}

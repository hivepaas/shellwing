//! A one-line text field with input method support (preedit), selection and
//! the clipboard. A masked one (for passphrases) shows a dot per character
//! and gives nothing to the clipboard.
//!
//! Adapted from GPUI's `examples/input.rs` (Apache-2.0, Zed Industries): keys
//! are handled directly instead of through actions, and the look is shellwing's.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, div, fill,
    point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::colors::colors;

/// The text changed.
pub struct Changed;

pub struct TextInput {
    focus: FocusHandle,
    content: SharedString,
    placeholder: Placeholder,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How far the text is scrolled left, so the cursor stays in the field
    /// when the text is wider than it.
    scroll_x: Pixels,
    selecting: bool,
    masked: bool,
    /// Several lines, wrapped to the width: [`TextInput::multiline`].
    lines: Option<Lines>,
}

/// A field of several lines: how many it shows, and how they were drawn.
struct Lines {
    /// Rows shown at the least and at the most; beyond, it scrolls.
    rows: (usize, usize),
    /// Each line of the text, from the byte it starts at, as last drawn,
    /// with its top (before scrolling).
    drawn: Vec<(usize, gpui::WrappedLine, Pixels)>,
    line_height: Pixels,
    scroll_y: Pixels,
}

/// What an empty field shows.
enum Placeholder {
    Text(SharedString),
    /// Said again each time it is drawn: in the language in use now.
    Said(Box<dyn Fn() -> String>),
}

/// A field whose placeholder is said in the language in use when drawn,
/// so it follows a change of language: `said(cx, || tr!("Find"))`.
pub fn said<V: 'static>(
    cx: &mut Context<V>,
    placeholder: impl Fn() -> String + 'static,
) -> gpui::Entity<TextInput> {
    cx.new(|cx| TextInput::said(placeholder, cx))
}

/// What a masked field shows for each character.
const DOT: char = '•';

impl gpui::EventEmitter<Changed> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self::with(Placeholder::Text(placeholder.into()), cx)
    }

    /// See [`said`].
    pub fn said(placeholder: impl Fn() -> String + 'static, cx: &mut Context<Self>) -> Self {
        Self::with(Placeholder::Said(Box::new(placeholder)), cx)
    }

    fn with(placeholder: Placeholder, cx: &mut Context<Self>) -> Self {
        TextInput {
            focus: cx.focus_handle(),
            content: SharedString::default(),
            placeholder,
            selected: 0..0,
            reversed: false,
            marked: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            selecting: false,
            masked: false,
            lines: None,
        }
    }

    /// A field of several lines, wrapped to its width, `min` to `max` rows
    /// high as its text needs (then it scrolls). Enter starts a line; with
    /// Ctrl (Cmd on macOS) it goes to whoever holds the field, as a lone
    /// line's Enter does.
    pub fn multiline(mut self, min: usize, max: usize) -> Self {
        self.lines = Some(Lines {
            rows: (min.max(1), max.max(min.max(1))),
            drawn: vec![],
            line_height: px(0.),
            scroll_y: px(0.),
        });
        self
    }

    /// A field of several lines shows `min` to `max` rows from now on.
    pub fn set_rows(&mut self, min: usize, max: usize, cx: &mut Context<Self>) {
        if let Some(l) = self.lines.as_mut() {
            let rows = (min.max(1), max.max(min.max(1)));
            if l.rows != rows {
                l.rows = rows;
                cx.notify();
            }
        }
    }

    /// Where the line holding `offset` starts and ends.
    fn line_bounds(&self, offset: usize) -> (usize, usize) {
        let start = self.content[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.content[offset..]
            .find('\n')
            .map_or(self.content.len(), |i| offset + i);
        (start, end)
    }

    /// Where `offset` was drawn in a field of several lines, from the top of
    /// its text (before scrolling).
    fn point_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let lines = self.lines.as_ref()?;
        let (start, line, top) = lines
            .drawn
            .iter()
            .rev()
            .find(|(start, _, _)| *start <= offset)?;
        let p = line.position_for_index(offset - start, lines.line_height)?;
        Some(point(p.x, *top + p.y))
    }

    /// The offset nearest to `at` (from the top of the text, before
    /// scrolling) in a field of several lines.
    fn offset_at(&self, at: Point<Pixels>) -> usize {
        let Some(lines) = self.lines.as_ref() else {
            return 0;
        };
        let Some((start, line, top)) = lines
            .drawn
            .iter()
            .rev()
            .find(|(_, _, top)| *top <= at.y)
            .or(lines.drawn.first())
        else {
            return 0;
        };
        let local = point(at.x.max(px(0.)), (at.y - *top).max(px(0.)));
        let i = match line.closest_index_for_position(local, lines.line_height) {
            Ok(i) | Err(i) => i,
        };
        (start + i).min(self.content.len())
    }

    /// The cursor a row up (`-1`) or down (`1`), at the same place across.
    fn row_from(&self, offset: usize, rows: f32) -> Option<usize> {
        let lines = self.lines.as_ref()?;
        let p = self.point_for(offset)?;
        let y = p.y + lines.line_height * (0.5 + rows);
        let last = lines
            .drawn
            .last()
            .map(|(_, l, top)| *top + l.size(lines.line_height).height)?;
        if y < px(0.) {
            return Some(0);
        }
        if y > last {
            return Some(self.content.len());
        }
        Some(self.offset_at(point(p.x, y)))
    }

    fn placeholder_text(&self) -> SharedString {
        match &self.placeholder {
            Placeholder::Text(t) => t.clone(),
            Placeholder::Said(f) => f().into(),
        }
    }

    /// A field whose text is hidden, for passphrases.
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// What is drawn: the text, or a dot per character.
    fn shown(&self) -> SharedString {
        if self.masked {
            std::iter::repeat_n(DOT, self.content.graphemes(true).count())
                .collect::<String>()
                .into()
        } else {
            self.content.clone()
        }
    }

    /// An offset in the text as one in what is drawn.
    fn shown_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.content[..offset.min(self.content.len())]
                .graphemes(true)
                .count()
                * DOT.len_utf8()
        } else {
            offset
        }
    }

    /// An offset in what is drawn as one in the text.
    fn text_offset(&self, shown: usize) -> usize {
        if self.masked {
            let n = shown / DOT.len_utf8();
            self.content
                .grapheme_indices(true)
                .nth(n)
                .map_or(self.content.len(), |(i, _)| i)
        } else {
            shown
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replaces the text, with `selected` (a byte range) chosen, as a rename
    /// field picks the name without its extension.
    pub fn set_text(&mut self, text: &str, selected: Range<usize>, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        let end = selected.end.min(text.len());
        self.selected = selected.start.min(end)..end;
        self.reversed = false;
        self.marked = None;
        cx.emit(Changed);
        cx.notify();
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.content =
            (self.content[..range.start].to_owned() + text + &self.content[range.end..]).into();
        let at = range.start + text.len();
        self.selected = at..at;
        self.reversed = false;
        self.marked = None;
        cx.emit(Changed);
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        // The platform's shortcut modifier: Cmd on macOS, Ctrl elsewhere.
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        if self.lines.is_some() {
            let vertical = match k.key.as_str() {
                "up" => self.row_from(self.cursor(), -1.),
                "down" => self.row_from(self.cursor(), 1.),
                _ => None,
            };
            if let Some(to) = vertical {
                if m.shift {
                    self.select_to(to, cx);
                } else {
                    self.move_to(to, cx);
                }
                return cx.stop_propagation();
            }
            let (start, end) = self.line_bounds(self.cursor());
            match k.key.as_str() {
                "enter" if !cmd => self.replace(self.selected.clone(), "\n", cx),
                "home" if m.shift => self.select_to(start, cx),
                "end" if m.shift => self.select_to(end, cx),
                "home" => self.move_to(start, cx),
                "end" => self.move_to(end, cx),
                "v" if cmd => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        self.replace(self.selected.clone(), &text.replace("\r\n", "\n"), cx);
                    }
                }
                _ => return self.on_key_line(ev, cx),
            }
            return cx.stop_propagation();
        }
        self.on_key_line(ev, cx)
    }

    /// The keys a field of one line takes (those of several lines too).
    fn on_key_line(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        match k.key.as_str() {
            "backspace" => {
                if self.selected.is_empty() {
                    let prev = self.previous_boundary(self.cursor());
                    self.select_to(prev, cx);
                }
                self.replace(self.selected.clone(), "", cx);
            }
            "delete" => {
                if self.selected.is_empty() {
                    let next = self.next_boundary(self.cursor());
                    self.select_to(next, cx);
                }
                self.replace(self.selected.clone(), "", cx);
            }
            "left" if m.shift => self.select_to(self.previous_boundary(self.cursor()), cx),
            "right" if m.shift => self.select_to(self.next_boundary(self.cursor()), cx),
            "left" => {
                let to = if self.selected.is_empty() {
                    self.previous_boundary(self.cursor())
                } else {
                    self.selected.start
                };
                self.move_to(to, cx);
            }
            "right" => {
                let to = if self.selected.is_empty() {
                    self.next_boundary(self.cursor())
                } else {
                    self.selected.end
                };
                self.move_to(to, cx);
            }
            "home" => self.move_to(0, cx),
            "end" => self.move_to(self.content.len(), cx),
            "a" if cmd => {
                self.move_to(0, cx);
                self.select_to(self.content.len(), cx);
            }
            "v" if cmd => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.replace(self.selected.clone(), &text.replace(['\r', '\n'], " "), cx);
                }
            }
            "c" | "x" if cmd && !self.selected.is_empty() && !self.masked => {
                let text = self.content[self.selected.clone()].to_string();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                if k.key == "x" {
                    self.replace(self.selected.clone(), "", cx);
                }
            }
            "u" if m.control => self.replace(0..self.cursor(), "", cx),
            // Text arrives through the input handler; Enter, Escape and the arrows
            // up and down are for whoever holds this field.
            _ => return,
        }
        cx.stop_propagation();
    }

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        if let (Some(lines), Some(bounds)) = (self.lines.as_ref(), self.last_bounds.as_ref()) {
            let at = point(
                position.x - bounds.left(),
                position.y - bounds.top() + lines.scroll_y,
            );
            return self.offset_at(at);
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if self.content.is_empty() || position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        self.text_offset(line.closest_index_for_x(position.x - bounds.left() + self.scroll_x))
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let at = self.index_for_position(e.position);
        if e.modifiers.shift {
            self.select_to(at, cx);
        } else {
            self.move_to(at, cx);
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_position(e.position), cx);
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let (mut utf8, mut utf16) = (0, 0);
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let (mut utf16, mut utf8) = (0, 0);
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.replace(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.content =
            (self.content[..range.start].to_owned() + text + &self.content[range.end..]).into();
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        self.selected = new_selected_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + text.len()..range.start + text.len());
        cx.emit(Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        if let Some(lines) = self.lines.as_ref() {
            let p = self.point_for(range.start)?;
            let top = bounds.top() + p.y - lines.scroll_y;
            return Some(Bounds::new(
                point(bounds.left() + p.x, top),
                size(px(2.), lines.line_height),
            ));
        }
        let layout = self.last_layout.as_ref()?;
        let left = bounds.left() - self.scroll_x;
        Some(Bounds::from_corners(
            point(
                left + layout.x_for_index(self.shown_offset(range.start)),
                bounds.top(),
            ),
            point(
                left + layout.x_for_index(self.shown_offset(range.end)),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.last_bounds?.localize(&p)?;
        let layout = self.last_layout.as_ref()?;
        let utf8 = self.text_offset(layout.index_for_x(p.x - local.x)?);
        Some(self.offset_to_utf16(utf8))
    }
}

/// Paints the text, selection and caret, and registers the input handler.
struct TextElement {
    input: Entity<TextInput>,
}

struct Prepainted {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let input = self.input.read(cx);
        let style = window.text_style();
        let (text, color) = if input.content.is_empty() {
            (input.placeholder_text(), rgb(colors().faint).into())
        } else {
            (input.shown(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let marked = input
            .marked
            .as_ref()
            .map(|m| input.shown_offset(m.start)..input.shown_offset(m.end));
        let runs = match marked.as_ref() {
            Some(m) if !input.content.is_empty() => [
                TextRun {
                    len: m.start,
                    ..run.clone()
                },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: text.len() - m.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);
        let caret_at = if input.content.is_empty() {
            px(0.)
        } else {
            line.x_for_index(input.shown_offset(input.cursor()))
        };
        // Scrolled just enough to show the cursor; none for what fits (and
        // a placeholder, which is cut at the edge instead).
        let room = (bounds.size.width - px(2.)).max(px(0.));
        let scroll_x = if input.content.is_empty() || line.width <= room {
            px(0.)
        } else {
            let mut x = input.scroll_x;
            if caret_at - x > room {
                x = caret_at - room;
            }
            if caret_at < x {
                x = caret_at;
            }
            x.min(line.width - room).max(px(0.))
        };
        let left = bounds.left() - scroll_x;
        let (selection, cursor) = if input.selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + caret_at, bounds.top()),
                        size(px(2.), bounds.size.height),
                    ),
                    rgb(colors().accent),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            left + line.x_for_index(input.shown_offset(input.selected.start)),
                            bounds.top(),
                        ),
                        point(
                            left + line.x_for_index(input.shown_offset(input.selected.end)),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(crate::colors::with_alpha(colors().accent, 0x55)),
                )),
                None,
            )
        };
        Prepainted {
            line: Some(line),
            cursor,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let Some(line) = pre.line.take() else {
            return;
        };
        let scroll_x = pre.scroll_x;
        // Cut at the field's edges: a long text or placeholder stays inside.
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(sel) = pre.selection.take() {
                window.paint_quad(sel);
            }
            let _ = line.paint(
                point(bounds.left() - scroll_x, bounds.top()),
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
            if focused && let Some(c) = pre.cursor.take() {
                window.paint_quad(c);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .when(self.lines.is_none(), |d| {
                d.child(TextElement { input: cx.entity() })
            })
            .when(self.lines.is_some(), |d| {
                d.child(AreaElement { input: cx.entity() })
            })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A field of several lines, as [`TextElement`] draws one.
struct AreaElement {
    input: Entity<TextInput>,
}

impl IntoElement for AreaElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// The text of a field of several lines as it will be drawn.
struct Area {
    drawn: Vec<(usize, gpui::WrappedLine, Pixels)>,
    cursor: Option<PaintQuad>,
    scroll_y: Pixels,
}

/// The runs a field's text is drawn with: the selection on its own ground.
fn area_runs(
    input: &TextInput,
    text: &str,
    color: gpui::Hsla,
    style: &gpui::TextStyle,
) -> Vec<TextRun> {
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let sel = &input.selected;
    if input.content.is_empty() || sel.is_empty() {
        return vec![run];
    }
    let ground = rgba(crate::colors::with_alpha(colors().accent, 0x55)).into();
    [
        TextRun {
            len: sel.start,
            ..run.clone()
        },
        TextRun {
            len: sel.end - sel.start,
            background_color: Some(ground),
            ..run.clone()
        },
        TextRun {
            len: text.len() - sel.end,
            ..run
        },
    ]
    .into_iter()
    .filter(|r| r.len > 0)
    .collect()
}

impl Element for AreaElement {
    type RequestLayoutState = ();
    type PrepaintState = Area;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // As high as the text wraps to, within its rows.
        let input = self.input.read(cx);
        let text = if input.content.is_empty() {
            input.placeholder_text()
        } else {
            input.content.clone()
        };
        let (min, max) = input.lines.as_ref().map_or((1, 1), |l| l.rows);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let font = style.font();
        let mut layout = Style::default();
        layout.size.width = relative(1.).into();
        let id = window.request_measured_layout(layout, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let rows = window
                .text_system()
                .shape_text(text.clone(), font_size, &[run], width, None)
                .map(|lines| {
                    lines
                        .iter()
                        .map(|l| (l.size(line_height).height / line_height).round() as usize)
                        .sum::<usize>()
                })
                .unwrap_or(1)
                .clamp(min, max);
            gpui::size(width.unwrap_or(px(0.)), line_height * rows as f32)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Area {
        let input = self.input.read(cx);
        let style = window.text_style();
        let line_height = window.line_height();
        let (text, color) = if input.content.is_empty() {
            (input.placeholder_text(), rgb(colors().faint).into())
        } else {
            (input.content.clone(), style.color)
        };
        let runs = area_runs(input, &text, color, &style);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let shaped = window
            .text_system()
            .shape_text(text, font_size, &runs, Some(bounds.size.width), None)
            .unwrap_or_default();
        // Each line from the byte it starts at, one under the other.
        let mut drawn = vec![];
        let (mut start, mut top) = (0, px(0.));
        for line in shaped {
            let len = line.len();
            let height = line.size(line_height).height;
            drawn.push((start, line, top));
            start += len + 1;
            top += height;
        }
        // Scrolled just enough to show the cursor's row.
        let caret = if input.content.is_empty() {
            Some(point(px(0.), px(0.)))
        } else {
            let offset = input.cursor();
            drawn
                .iter()
                .rev()
                .find(|(s, _, _)| *s <= offset)
                .and_then(|(s, l, t)| {
                    l.position_for_index(offset - s, line_height)
                        .map(|p| point(p.x, *t + p.y))
                })
        };
        let mut scroll_y = input.lines.as_ref().map_or(px(0.), |l| l.scroll_y);
        if let Some(c) = caret {
            if c.y < scroll_y {
                scroll_y = c.y;
            }
            if c.y + line_height > scroll_y + bounds.size.height {
                scroll_y = c.y + line_height - bounds.size.height;
            }
        }
        scroll_y = scroll_y
            .min((top - bounds.size.height).max(px(0.)))
            .max(px(0.));
        let cursor = caret.filter(|_| input.selected.is_empty()).map(|c| {
            fill(
                Bounds::new(
                    point(bounds.left() + c.x, bounds.top() + c.y - scroll_y),
                    size(px(2.), line_height),
                ),
                rgb(colors().accent),
            )
        });
        Area {
            drawn,
            cursor,
            scroll_y,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        area: &mut Area,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        let focused = focus.is_focused(window);
        let scroll_y = area.scroll_y;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (_, line, top) in &area.drawn {
                let origin = point(bounds.left(), bounds.top() + *top - scroll_y);
                let _ = line.paint_background(
                    origin,
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                let _ = line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
            }
            if focused && let Some(c) = area.cursor.take() {
                window.paint_quad(c);
            }
        });
        let drawn = std::mem::take(&mut area.drawn);
        self.input.update(cx, |input, _| {
            input.last_bounds = Some(bounds);
            if let Some(lines) = input.lines.as_mut() {
                lines.drawn = drawn;
                lines.line_height = line_height;
                lines.scroll_y = scroll_y;
            }
        });
    }
}

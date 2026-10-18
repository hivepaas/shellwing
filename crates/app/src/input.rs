//! A one-line text field with input method support (preedit), selection and
//! the clipboard.
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

/// The text changed.
pub struct Changed;

pub struct TextInput {
    focus: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
}

impl gpui::EventEmitter<Changed> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        TextInput {
            focus: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected: 0..0,
            reversed: false,
            marked: None,
            last_layout: None,
            last_bounds: None,
            selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
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
            "c" | "x" if cmd && !self.selected.is_empty() => {
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
        line.closest_index_for_x(position.x - bounds.left())
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
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end),
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
        let utf8 = layout.index_for_x(p.x - local.x)?;
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
            (input.placeholder.clone(), rgb(0x6b7588).into())
        } else {
            (input.content.clone(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked.as_ref() {
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
            line.x_for_index(input.cursor())
        };
        let (selection, cursor) = if input.selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + caret_at, bounds.top()),
                        size(px(2.), bounds.size.height),
                    ),
                    rgb(0x38bdf8),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(input.selected.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(input.selected.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x38bdf855),
                )),
                None,
            )
        };
        Prepainted {
            line: Some(line),
            cursor,
            selection,
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
        if let Some(sel) = pre.selection.take() {
            window.paint_quad(sel);
        }
        let Some(line) = pre.line.take() else {
            return;
        };
        let _ = line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        if focus.is_focused(window)
            && let Some(c) = pre.cursor.take()
        {
            window.paint_quad(c);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
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
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

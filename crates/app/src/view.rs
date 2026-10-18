//! The terminal view: paints frames from the terminal thread, turns key presses
//! into terminal input, and implements GPUI's input handler so input methods
//! (ibus, fcitx, the macOS and Windows IMEs) can compose text with preedit.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, Bounds, Context, EntityInputHandler, FocusHandle, Focusable, Font, FontStyle, FontWeight,
    Hsla, KeyDownEvent, Pixels, Render, SharedString, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, canvas, div, font, point, prelude::*, px, rgb, size,
};
use shellwing_term::{Frame, Mods, NamedKey, Rgb, Row, TermHandle};

use crate::Writer;
use crate::bench::Probe;
use crate::chrome::{self, RESIZE_EDGE, TITLEBAR_H};

const FONT_FAMILY: &str = "JetBrains Mono";
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 1.3;

fn hsla(c: Rgb) -> Hsla {
    rgb(((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32).into()
}

/// A row converted to GPUI text runs, kept while the frame keeps sharing the row.
struct PaintRow {
    source: Arc<Row>,
    text: SharedString,
    runs: Vec<TextRun>,
}

pub struct TermView {
    focus: FocusHandle,
    term: TermHandle,
    writer: Writer,
    title: String,
    frame: Arc<Frame>,
    paint_rows: Vec<PaintRow>,
    grid: (u16, u16),
    cell: (Pixels, Pixels),
    /// Text the input method is composing (preedit), shown at the cursor.
    marked: Option<String>,
    probe: Arc<Probe>,
}

impl TermView {
    pub fn new(
        term: TermHandle,
        writer: Writer,
        title: String,
        probe: Arc<Probe>,
        cx: &mut Context<Self>,
    ) -> Self {
        TermView {
            focus: cx.focus_handle(),
            term,
            writer,
            title,
            frame: Arc::new(Frame::default()),
            paint_rows: vec![],
            grid: (0, 0),
            cell: (px(0.), px(0.)),
            marked: None,
            probe,
        }
    }

    fn base_font() -> Font {
        font(FONT_FAMILY)
    }

    fn measure_cell(&mut self, window: &mut Window) {
        let run = TextRun {
            len: 1,
            font: Self::base_font(),
            color: rgb(0xffffff).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line("M".into(), px(FONT_SIZE), &[run], None);
        self.cell = (line.width, px((FONT_SIZE * LINE_HEIGHT).round()));
    }

    /// Keeps the terminal grid and the remote PTY the size of the visible area.
    fn fit_grid(&mut self, window: &mut Window) {
        let mut vp = window.viewport_size();
        if matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        ) {
            vp.width -= px(RESIZE_EDGE * 2.);
            vp.height -= px(RESIZE_EDGE * 2. + TITLEBAR_H);
        }
        let cols = ((vp.width / self.cell.0).floor() as u16).max(2);
        let rows = ((vp.height / self.cell.1).floor() as u16).max(2);
        if (cols, rows) != self.grid {
            self.grid = (cols, rows);
            self.term.resize(
                cols,
                rows,
                f32::from(self.cell.0) as u32,
                f32::from(self.cell.1) as u32,
            );
            self.writer.resize(cols as u32, rows as u32);
        }
    }

    /// Converts changed rows of the latest frame into GPUI text runs.
    fn refresh_rows(&mut self) {
        if let Some(f) = self.term.take_frame() {
            self.frame = f;
        }
        let rows = &self.frame.rows;
        self.paint_rows.truncate(rows.len());
        for (i, row) in rows.iter().enumerate() {
            if self
                .paint_rows
                .get(i)
                .is_some_and(|p| Arc::ptr_eq(&p.source, row))
            {
                continue;
            }
            let base = Self::base_font();
            let runs = row
                .runs
                .iter()
                .map(|r| TextRun {
                    len: r.len,
                    font: Font {
                        weight: if r.bold {
                            FontWeight::BOLD
                        } else {
                            FontWeight::NORMAL
                        },
                        style: if r.italic {
                            FontStyle::Italic
                        } else {
                            FontStyle::Normal
                        },
                        ..base.clone()
                    },
                    color: hsla(r.fg),
                    background_color: r.bg.map(hsla),
                    underline: r.underline.then(|| UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    }),
                    strikethrough: None,
                })
                .collect();
            let p = PaintRow {
                source: row.clone(),
                text: row.text.clone().into(),
                runs,
            };
            if i < self.paint_rows.len() {
                self.paint_rows[i] = p;
            } else {
                self.paint_rows.push(p);
            }
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let mods = Mods {
            shift: k.modifiers.shift,
            ctrl: k.modifiers.control,
            alt: k.modifiers.alt,
        };
        let named = match k.key.as_str() {
            "enter" => NamedKey::Enter,
            "backspace" => NamedKey::Backspace,
            "tab" => NamedKey::Tab,
            "escape" => NamedKey::Escape,
            "up" => NamedKey::Up,
            "down" => NamedKey::Down,
            "left" => NamedKey::Left,
            "right" => NamedKey::Right,
            "home" => NamedKey::Home,
            "end" => NamedKey::End,
            "pageup" => NamedKey::PageUp,
            "pagedown" => NamedKey::PageDown,
            "delete" => NamedKey::Delete,
            key if (mods.ctrl || mods.alt)
                && key.len() == 1
                && key.as_bytes()[0].is_ascii_lowercase() =>
            {
                NamedKey::Letter(key.as_bytes()[0] as char)
            }
            // Printable text arrives through the input handler, after any IME.
            _ => return,
        };
        self.term.key(named, mods);
        cx.stop_propagation();
    }
}

impl Focusable for TermView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cell.0 == px(0.) {
            self.measure_cell(window);
        }
        self.fit_grid(window);
        self.refresh_rows();

        let rows: Vec<(SharedString, Vec<TextRun>)> = self
            .paint_rows
            .iter()
            .map(|r| (r.text.clone(), r.runs.clone()))
            .collect();
        let (cw, ch) = self.cell;
        let cursor = self.frame.cursor;
        let fg = hsla(self.frame.foreground);
        let bg = hsla(self.frame.background);
        let marked = self.marked.clone();
        let entity = cx.entity();
        let focus = self.focus.clone();
        let probe = self.probe.clone();
        let focus_click = self.focus.clone();

        let terminal = div()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .bg(bg)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                window.focus(&focus_click, cx)
            })
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus,
                            gpui::ElementInputHandler::new(bounds, entity),
                            cx,
                        );
                        for (y, (text, runs)) in rows.iter().enumerate() {
                            if text.is_empty() {
                                continue;
                            }
                            let origin = point(bounds.origin.x, bounds.origin.y + ch * y as f32);
                            // force_width keeps every glyph on the cell grid.
                            let line = window.text_system().shape_line(
                                text.clone(),
                                px(FONT_SIZE),
                                runs,
                                Some(cw),
                            );
                            let _ = line.paint_background(
                                origin,
                                ch,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            let _ = line.paint(origin, ch, TextAlign::Left, None, window, cx);
                        }
                        if let Some((x, y)) = cursor {
                            let o = point(
                                bounds.origin.x + cw * x as f32,
                                bounds.origin.y + ch * y as f32,
                            );
                            let mut c = fg;
                            c.a = 0.55;
                            window.paint_quad(gpui::fill(Bounds::new(o, size(cw, ch)), c));
                            if let Some(m) = &marked {
                                let run = TextRun {
                                    len: m.len(),
                                    font: TermView::base_font(),
                                    color: fg,
                                    background_color: Some(bg),
                                    underline: Some(UnderlineStyle {
                                        thickness: px(1.),
                                        color: Some(fg),
                                        wavy: false,
                                    }),
                                    strikethrough: None,
                                };
                                let line = window.text_system().shape_line(
                                    m.clone().into(),
                                    px(FONT_SIZE),
                                    &[run],
                                    Some(cw),
                                );
                                let _ =
                                    line.paint_background(o, ch, TextAlign::Left, None, window, cx);
                                let _ = line.paint(o, ch, TextAlign::Left, None, window, cx);
                            }
                        }
                        probe.painted();
                    },
                )
                .size_full(),
            );
        if matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        ) {
            chrome::window_chrome(self.title.clone(), terminal, bg)
        } else {
            div().size_full().flex().flex_col().child(terminal)
        }
    }
}

impl EntityInputHandler for TermView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let units: Vec<u16> = self.marked.as_ref()?.encode_utf16().collect();
        let r = range.start.min(units.len())..range.end.min(units.len());
        *adjusted = Some(r.clone());
        Some(String::from_utf16_lossy(&units[r]))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let n = self.marked.as_ref().map_or(0, |m| m.encode_utf16().count());
        Some(UTF16Selection {
            range: n..n,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    /// Committed text, from the IME or a plain key press, replaces any preedit.
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        self.writer.write(text.as_bytes().to_vec());
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    /// Where the IME should place its candidate window: the terminal cursor.
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (x, y) = self.frame.cursor.unwrap_or((0, 0));
        Some(Bounds::new(
            point(
                element_bounds.origin.x + self.cell.0 * x as f32,
                element_bounds.origin.y + self.cell.1 * y as f32,
            ),
            size(self.cell.0, self.cell.1),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

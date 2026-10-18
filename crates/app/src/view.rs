//! The terminal view: paints frames from the terminal thread, turns key presses,
//! mouse and wheel into terminal input, and implements GPUI's input handler so
//! input methods (ibus, fcitx, the macOS and Windows IMEs) can compose text with
//! preedit.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ClipboardItem, Context, Corners, EntityInputHandler, FocusHandle, Focusable, Font,
    FontFallbacks, FontStyle, FontWeight, Hsla, KeyDownEvent, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage,
    ScrollDelta, ScrollWheelEvent, SharedString, Size, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, canvas, div, font, point, prelude::*, px, rgb, size,
};
use shellwing_config::{CursorStyle, Gradient, Theme};
use shellwing_term::{
    Event as TermEvent, Frame, Mods, MouseAction, MouseButton as TermButton, MouseInput, NamedKey,
    Rgb, Row, TermHandle,
};

use crate::Writer;
use crate::bench::Probe;
use crate::chrome::{self, RESIZE_EDGE, TITLEBAR_H};
use crate::look::{Live, Look, rgb as theme_rgb};

/// Terminal font candidates; the first one installed is used ($SHELLWING_FONT wins).
#[cfg(target_os = "macos")]
const FONT_FAMILIES: &[&str] = &["JetBrains Mono", "SF Mono", "Menlo"];
#[cfg(target_os = "windows")]
const FONT_FAMILIES: &[&str] = &["JetBrains Mono", "Cascadia Mono", "Consolas"];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FONT_FAMILIES: &[&str] = &[
    "JetBrains Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
];

/// Tried in order for characters the terminal font lacks (CJK, emoji).
#[cfg(target_os = "macos")]
const FONT_FALLBACKS: &[&str] = &[
    "Menlo",
    "Apple Color Emoji",
    "PingFang SC",
    "Hiragino Sans",
    "Apple SD Gothic Neo",
];
#[cfg(target_os = "windows")]
const FONT_FALLBACKS: &[&str] = &[
    "Consolas",
    "Segoe UI Emoji",
    "Microsoft YaHei",
    "Yu Gothic",
    "Malgun Gothic",
];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FONT_FALLBACKS: &[&str] = &[
    "Noto Sans Mono CJK SC",
    "Noto Color Emoji",
    "Noto Sans CJK SC",
    "DejaVu Sans Mono",
];
/// Lines per wheel notch.
const WHEEL_LINES: f32 = 3.0;
/// How long a refused paste waits for the user to paste again to confirm.
const CONFIRM_PASTE: Duration = Duration::from_secs(5);
/// How long a problem with the config files stays on screen.
const CONFIG_NOTICE: Duration = Duration::from_secs(8);
/// Longest side of the gradient image. The GPU stretches it with linear
/// filtering, which keeps a gradient smooth, so only its aspect ratio matters.
const GRADIENT_SIZE: f32 = 256.0;

fn hsla(c: Rgb) -> Hsla {
    rgb(((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32).into()
}

fn mods(m: &Modifiers) -> Mods {
    Mods {
        shift: m.shift,
        ctrl: m.control,
        alt: m.alt,
        logo: m.platform,
    }
}

fn term_button(b: MouseButton) -> Option<TermButton> {
    match b {
        MouseButton::Left => Some(TermButton::Left),
        MouseButton::Middle => Some(TermButton::Middle),
        MouseButton::Right => Some(TermButton::Right),
        _ => None,
    }
}

/// A piece of a row painted with one shaped line: a stretch of narrow cells, or a
/// single wide glyph given two cells of advance.
#[derive(Clone)]
struct Segment {
    col: u16,
    wide: bool,
    text: SharedString,
    runs: Vec<TextRun>,
}

/// A row converted to GPUI text runs, kept while the frame keeps sharing the row.
struct PaintRow {
    source: Arc<Row>,
    segments: Vec<Segment>,
}

/// A theme's gradient rendered for one aspect ratio.
struct GradientImage {
    key: (String, u32, u32),
    image: Arc<RenderImage>,
}

/// The image for `gradient` at `size`, rendered again only when the theme or
/// the aspect ratio changed.
fn gradient_image(
    cache: &RefCell<Option<GradientImage>>,
    theme: &Theme,
    gradient: &Gradient,
    size: Size<Pixels>,
    window: &mut Window,
) -> Option<Arc<RenderImage>> {
    let (w, h) = (
        f32::from(size.width).max(1.),
        f32::from(size.height).max(1.),
    );
    let scale = GRADIENT_SIZE / w.max(h);
    // Rounded to 4 px so a window being resized does not render on every step.
    let dim = |v: f32| ((v * scale / 4.).round() as u32).max(1) * 4;
    let key = (theme.id.clone(), dim(w), dim(h));
    let mut cache = cache.borrow_mut();
    if let Some(c) = cache.as_ref()
        && c.key == key
    {
        return Some(c.image.clone());
    }
    let pixels = gradient.render_bgra(key.1, key.2);
    let buffer = image::RgbaImage::from_raw(key.1, key.2, pixels)?;
    let image = Arc::new(RenderImage::new([image::Frame::new(buffer)]));
    if let Some(old) = cache.take() {
        let _ = window.drop_image(old.image);
    }
    *cache = Some(GradientImage {
        key,
        image: image.clone(),
    });
    Some(image)
}

/// The modifier that turns a click into "open link": Cmd on macOS, Ctrl elsewhere.
fn link_mod(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    }
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
    /// Window position of the grid's top-left corner, recorded at paint time.
    grid_origin: Rc<Cell<Point<Pixels>>>,
    /// Text the input method is composing (preedit), shown at the cursor.
    marked: Option<String>,
    /// A paste the terminal refused as unsafe, waiting for confirmation.
    pending_paste: Option<(String, Instant)>,
    notice: Option<SharedString>,
    /// Last pointer position over the grid, to show links when the modifier goes down.
    last_mouse: Option<(f32, f32)>,
    font_family: SharedString,
    look: Look,
    live: Live,
    gradient: Rc<RefCell<Option<GradientImage>>>,
    probe: Arc<Probe>,
}

impl TermView {
    pub fn new(
        term: TermHandle,
        writer: Writer,
        title: String,
        probe: Arc<Probe>,
        live: Live,
        problems: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (look, problem) = live.look();
        term.set_colors(look.colors());
        let mut view = TermView {
            focus: cx.focus_handle(),
            term,
            writer,
            title,
            frame: Arc::new(Frame::default()),
            paint_rows: vec![],
            grid: (0, 0),
            cell: (px(0.), px(0.)),
            grid_origin: Rc::new(Cell::new(point(px(0.), px(0.)))),
            marked: None,
            pending_paste: None,
            notice: None,
            last_mouse: None,
            font_family: FONT_FAMILIES[0].into(),
            look,
            live,
            gradient: Rc::new(RefCell::new(None)),
            probe,
        };
        view.report(problems.into_iter().chain(problem).collect(), cx);
        view
    }

    /// Shows problems with the config files, if any.
    fn report(&mut self, problems: Vec<String>, cx: &mut Context<Self>) {
        if !problems.is_empty() {
            self.show_notice(problems.join("\n"), CONFIG_NOTICE, cx);
        }
    }

    /// Called periodically: picks up edits to the config files.
    pub fn poll_config(&mut self, cx: &mut Context<Self>) {
        if let Some(mut problems) = self.live.reload_if_changed() {
            let (look, problem) = self.live.look();
            problems.extend(problem);
            self.apply(look, cx);
            self.report(problems, cx);
        }
    }

    fn apply(&mut self, look: Look, cx: &mut Context<Self>) {
        if look == self.look {
            return;
        }
        if look.theme != self.look.theme {
            self.term.set_colors(look.colors());
        }
        if look.scrollback != self.look.scrollback {
            self.term.set_scrollback(look.scrollback);
        }
        if (&look.font_family, look.font_size, look.line_height)
            != (
                &self.look.font_family,
                self.look.font_size,
                self.look.line_height,
            )
        {
            // Measured again on the next render, which also resizes the grid.
            self.cell = (px(0.), px(0.));
        }
        self.look = look;
        // Text runs carry colors and fonts, so every row is converted again.
        self.paint_rows.clear();
        cx.notify();
    }

    fn step_theme(&mut self, step: isize, cx: &mut Context<Self>) {
        match self.live.step_theme(step) {
            Ok(theme) => {
                let (look, _) = self.live.look();
                self.apply(look, cx);
                self.show_notice(format!("Theme: {}", theme.name), CONFIRM_PASTE, cx);
            }
            Err(e) => self.show_notice(e, CONFIG_NOTICE, cx),
        }
    }

    /// The terminal font, with fallbacks for glyphs it lacks. Missing fallback
    /// families are skipped by the text system.
    fn base_font(&self) -> Font {
        Font {
            fallbacks: Some(FontFallbacks::from_fonts(
                FONT_FALLBACKS.iter().map(|f| f.to_string()).collect(),
            )),
            ..font(self.font_family.clone())
        }
    }

    /// Picks the terminal font: $SHELLWING_FONT, else the configured family if
    /// installed, else the first installed candidate.
    fn choose_font(window: &mut Window, configured: Option<&str>) -> SharedString {
        if let Ok(f) = std::env::var("SHELLWING_FONT") {
            return f.into();
        }
        let installed = window.text_system().all_font_names();
        configured
            .into_iter()
            .chain(FONT_FAMILIES.iter().copied())
            .find(|f| installed.iter().any(|i| i == f))
            .unwrap_or(FONT_FAMILIES[FONT_FAMILIES.len() - 1])
            .to_string()
            .into()
    }

    /// Events from the terminal thread (copied text, refused pastes).
    pub fn on_term_event(&mut self, ev: TermEvent, cx: &mut Context<Self>) {
        match ev {
            TermEvent::Copied(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            TermEvent::OpenLink(url) => cx.open_url(&url),
            TermEvent::PasteRefused(text) => {
                self.pending_paste = Some((text, Instant::now()));
                self.show_notice(
                    "The pasted text could run commands. Paste again within 5 s to send it anyway.",
                    CONFIRM_PASTE,
                    cx,
                );
            }
        }
    }

    fn show_notice(
        &mut self,
        msg: impl Into<SharedString>,
        shown: Duration,
        cx: &mut Context<Self>,
    ) {
        let msg: SharedString = msg.into();
        self.notice = Some(msg.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(shown).await;
            let _ = this.update(cx, |v, cx| {
                if v.notice.as_ref() == Some(&msg) {
                    v.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        // Pasting the same refused text again within the window confirms it.
        let confirmed = self
            .pending_paste
            .take()
            .is_some_and(|(pending, at)| pending == text && at.elapsed() < CONFIRM_PASTE);
        if confirmed {
            self.notice = None;
        }
        self.term.paste(text, confirmed);
        cx.notify();
    }

    fn measure_cell(&mut self, window: &mut Window) {
        self.font_family = Self::choose_font(window, self.look.font_family.as_deref());
        let run = TextRun {
            len: 1,
            font: self.base_font(),
            color: rgb(0xffffff).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line =
            window
                .text_system()
                .shape_line("M".into(), px(self.look.font_size), &[run], None);
        self.cell = (
            line.width,
            px((self.look.font_size * self.look.line_height).round()),
        );
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
        let theme = &self.look.theme;
        let selection_bg = hsla(theme_rgb(theme.selection_background));
        let selection_fg = theme.selection_foreground.map(|c| hsla(theme_rgb(c)));
        self.paint_rows.truncate(rows.len());
        for (i, row) in rows.iter().enumerate() {
            if self
                .paint_rows
                .get(i)
                .is_some_and(|p| Arc::ptr_eq(&p.source, row))
            {
                continue;
            }
            let base = self.base_font();
            let mut segments: Vec<Segment> = Vec::new();
            let mut narrow: Option<(u16, String, Vec<TextRun>)> = None;
            let mut offset = 0;
            for r in &row.runs {
                let text = &row.text[offset..offset + r.len];
                offset += r.len;
                let run = TextRun {
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
                    color: match selection_fg {
                        Some(c) if r.selected => c,
                        _ => hsla(r.fg),
                    },
                    background_color: if r.selected {
                        Some(selection_bg)
                    } else {
                        r.bg.map(hsla)
                    },
                    underline: r.underline.then(|| UnderlineStyle {
                        thickness: px(1.),
                        color: None,
                        wavy: false,
                    }),
                    strikethrough: None,
                };
                if r.wide {
                    if let Some((col, t, runs)) = narrow.take() {
                        segments.push(Segment {
                            col,
                            wide: false,
                            text: t.into(),
                            runs,
                        });
                    }
                    segments.push(Segment {
                        col: r.col,
                        wide: true,
                        text: text.to_string().into(),
                        runs: vec![run],
                    });
                } else {
                    let seg = narrow.get_or_insert_with(|| (r.col, String::new(), Vec::new()));
                    seg.1.push_str(text);
                    seg.2.push(run);
                }
            }
            if let Some((col, t, runs)) = narrow.take() {
                segments.push(Segment {
                    col,
                    wide: false,
                    text: t.into(),
                    runs,
                });
            }
            let p = PaintRow {
                source: row.clone(),
                segments,
            };
            if i < self.paint_rows.len() {
                self.paint_rows[i] = p;
            } else {
                self.paint_rows.push(p);
            }
        }
    }

    /// A window position relative to the grid's top-left corner.
    fn grid_pos(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.grid_origin.get();
        (f32::from(p.x - o.x), f32::from(p.y - o.y))
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (x, y) = self.grid_pos(e.position);
        let button = term_button(e.button);
        self.term.mouse(MouseInput {
            action: MouseAction::Press,
            button,
            x,
            y,
            mods: mods(&e.modifiers),
            link: link_mod(&e.modifiers),
        });
    }

    fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        let (x, y) = self.grid_pos(e.position);
        let button = term_button(e.button);
        self.term.mouse(MouseInput {
            action: MouseAction::Release,
            button,
            x,
            y,
            mods: mods(&e.modifiers),
            link: link_mod(&e.modifiers),
        });
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {
        let (x, y) = self.grid_pos(e.position);
        self.last_mouse = Some((x, y));
        if link_mod(&e.modifiers) {
            self.term.hover(Some((x, y)));
        } else if self.frame.hover_link.is_some() {
            self.term.hover(None);
        }
        let button = e.pressed_button.and_then(term_button);
        self.term.mouse(MouseInput {
            action: MouseAction::Motion,
            button,
            x,
            y,
            mods: mods(&e.modifiers),
            link: link_mod(&e.modifiers),
        });
    }

    fn modifiers_changed(
        &mut self,
        e: &ModifiersChangedEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        if link_mod(&e.modifiers) {
            if let Some(at) = self.last_mouse {
                self.term.hover(Some(at));
            }
        } else if self.frame.hover_link.is_some() {
            self.term.hover(None);
        }
    }

    fn scroll_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, _: &mut Context<Self>) {
        // GPUI reports positive y when scrolling up; the terminal wants negative for up.
        let lines = match e.delta {
            ScrollDelta::Lines(p) => -p.y * WHEEL_LINES,
            ScrollDelta::Pixels(p) => -f32::from(p.y) / f32::from(self.cell.1),
        };
        let (x, y) = self.grid_pos(e.position);
        self.term.wheel(lines, x, y, mods(&e.modifiers));
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let key = k.key.as_str();
        // Clipboard and scrollback shortcuts: Ctrl+Shift on Linux/Windows, Cmd on macOS.
        let app_chord = if cfg!(target_os = "macos") {
            m.platform && !m.control
        } else {
            m.control && m.shift
        };
        match key {
            // Next and previous theme, until the theme picker exists.
            "]" | "}" if app_chord => {
                self.step_theme(1, cx);
                return cx.stop_propagation();
            }
            "[" | "{" if app_chord => {
                self.step_theme(-1, cx);
                return cx.stop_propagation();
            }
            "c" | "C" if app_chord => {
                self.term.copy();
                return cx.stop_propagation();
            }
            "v" | "V" if app_chord => {
                self.paste_clipboard(cx);
                return cx.stop_propagation();
            }
            "insert" if m.shift => {
                self.paste_clipboard(cx);
                return cx.stop_propagation();
            }
            "pageup" | "pagedown" if m.shift => {
                self.term.scroll_pages(if key == "pageup" { -1 } else { 1 });
                return cx.stop_propagation();
            }
            "end" if m.shift => {
                self.term.scroll_to_bottom();
                return cx.stop_propagation();
            }
            _ => {}
        }
        let mods = mods(m);
        let named = match key {
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

        let rows: Vec<Vec<Segment>> = self.paint_rows.iter().map(|r| r.segments.clone()).collect();
        let hover = self.frame.hover_link;
        let (cw, ch) = self.cell;
        let cursor = self.frame.cursor;
        let fg = hsla(self.frame.foreground);
        let bg = hsla(self.frame.background);
        let theme = self.look.theme.clone();
        let cursor_style = self.look.cursor;
        let cursor_color = hsla(
            self.frame
                .cursor_color
                .unwrap_or_else(|| theme_rgb(theme.cursor)),
        );
        let font_size = px(self.look.font_size);
        // The gradient replaces the background unless a program set its own (OSC 11).
        let gradient = theme
            .gradient
            .clone()
            .filter(|_| self.frame.background == theme_rgb(theme.background));
        let gradient_cache = self.gradient.clone();
        let marked = self.marked.clone();
        let base_font = self.base_font();
        let entity = cx.entity();
        let focus = self.focus.clone();
        let probe = self.probe.clone();
        let grid_origin = self.grid_origin.clone();

        let mut terminal = div()
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .bg(bg)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            // A drag that ends outside the grid still has to finish the selection.
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .when(hover.is_some(), |d| d.cursor_pointer())
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        grid_origin.set(bounds.origin);
                        if let Some(g) = &gradient
                            && let Some(image) =
                                gradient_image(&gradient_cache, &theme, g, bounds.size, window)
                        {
                            let _ = window.paint_image(
                                bounds,
                                bounds,
                                Corners::default(),
                                image,
                                0,
                                false,
                            );
                        }
                        window.handle_input(
                            &focus,
                            gpui::ElementInputHandler::new(bounds, entity),
                            cx,
                        );
                        for (y, segments) in rows.iter().enumerate() {
                            for seg in segments {
                                let origin = point(
                                    bounds.origin.x + cw * seg.col as f32,
                                    bounds.origin.y + ch * y as f32,
                                );
                                // force_width keeps every glyph on the cell grid; a wide
                                // glyph advances two cells.
                                let advance = if seg.wide { cw * 2. } else { cw };
                                let line = window.text_system().shape_line(
                                    seg.text.clone(),
                                    font_size,
                                    &seg.runs,
                                    Some(advance),
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
                        }
                        if let Some(link) = hover {
                            let o = point(
                                bounds.origin.x + cw * link.start as f32,
                                bounds.origin.y + ch * (link.row as f32 + 1.) - px(2.),
                            );
                            let w = cw * (link.end - link.start) as f32;
                            window.paint_quad(gpui::fill(Bounds::new(o, size(w, px(1.))), fg));
                        }
                        if let Some((x, y)) = cursor {
                            let o = point(
                                bounds.origin.x + cw * x as f32,
                                bounds.origin.y + ch * y as f32,
                            );
                            let bar = px(2.);
                            let (at, area, alpha) = match cursor_style {
                                CursorStyle::Block => (o, size(cw, ch), 0.6),
                                CursorStyle::Bar => (o, size(bar, ch), 1.0),
                                CursorStyle::Underline => {
                                    (point(o.x, o.y + ch - bar), size(cw, bar), 1.0)
                                }
                            };
                            let mut c = cursor_color;
                            c.a *= alpha;
                            window.paint_quad(gpui::fill(Bounds::new(at, area), c));
                            if let Some(m) = &marked {
                                let run = TextRun {
                                    len: m.len(),
                                    font: base_font.clone(),
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
                                    font_size,
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

        // Overlays: a notice (e.g. refused paste) and a way back from the scrollback.
        let badge = || {
            div()
                .absolute()
                .right_3()
                .px_2()
                .py_1()
                .rounded_md()
                .text_sm()
                .bg(rgb(0x3f3f46))
                .text_color(rgb(0xe4e4e7))
        };
        if let Some(n) = &self.notice {
            terminal = terminal.child(badge().top_3().child(n.clone()));
        }
        if self.frame.scrolled_back {
            terminal = terminal.child(
                badge()
                    .id("to-bottom")
                    .bottom_3()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x52525b)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.term.scroll_to_bottom();
                        cx.notify();
                    }))
                    .child("↓ Back to bottom (Shift+End)"),
            );
        }

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
        self.term.text(text.to_string());
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
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

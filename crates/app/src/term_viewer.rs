//! A terminal that only shows: text handed to it, drawn as a tab's terminal
//! draws its own, in the person's look (theme and its gradient, font, line
//! height, padding, scrollbar). Nothing is typed into it and nothing reaches
//! a host: the wheel scrolls, a drag selects, a link opens with Ctrl+click
//! (Cmd on macOS), Ctrl+C or Ctrl+Shift+C (Cmd+C) copies, and the find
//! shortcut finds. AI activity shows a command and its output in one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ClipboardItem, Context, Corners, Entity, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, SharedString, Size, Window, canvas, div, point, prelude::*, px,
    rgb, size,
};
use shellwing_i18n::tr;
use shellwing_term::{
    Event as TermEvent, FindQuery, Frame, Mods, MouseAction, MouseButton as TermButton, MouseInput,
    TermHandle,
};

use crate::colors::colors;
use crate::input::{Changed, TextInput};
use crate::look::{Look, rgb as theme_rgb};
use crate::view::{
    DRAG_BEAT, Drag, FindOptions, GradientImage, PaintRow, SCROLLBAR_LINGER, SCROLLBAR_ZONE,
    Segment, TermView, WHEEL_LINES, cell_size, convert_rows, drag_scroll, gradient_image, hsla,
    paint_segments, row_for_thumb, term_font, thumb,
};

/// Hides the cursor: nothing is typed here.
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
/// Clears the screen and its history, and goes home.
const CLEAR: &[u8] = b"\x1bc\x1b[3J\x1b[2J\x1b[H";

/// What was fed, to feed only what comes after it.
struct Fed {
    key: String,
    /// Bytes of the output fed.
    len: usize,
    /// Its last bytes, to tell that the output still starts with what was fed.
    tail: Vec<u8>,
}

pub struct TermViewer {
    focus: FocusHandle,
    term: TermHandle,
    frame: Arc<Frame>,
    paint_rows: Vec<PaintRow>,
    look: Look,
    font_family: SharedString,
    cell: (Pixels, Pixels),
    grid: (u16, u16),
    grid_origin: Rc<Cell<Point<Pixels>>>,
    area: Rc<Cell<Size<Pixels>>>,
    gradient: Rc<RefCell<Option<GradientImage>>>,
    fed: Option<Fed>,
    scrollbar_until: Option<Instant>,
    scrollbar_hover: bool,
    scrollbar_grab: Option<f32>,
    find: Option<(Entity<TextInput>, gpui::Subscription)>,
    drag: Option<Drag>,
}

impl Focusable for TermViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for TermViewer {
    fn drop(&mut self) {
        self.term.close();
    }
}

/// `output`, then `errors` in red on a line of their own.
fn with_errors(output: &str, errors: &str) -> String {
    if errors.is_empty() {
        return output.to_string();
    }
    let gap = if output.is_empty() || output.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{output}{gap}\x1b[31m{errors}\x1b[0m")
}

/// `text` with each line ending as a terminal wants it: a lone `\n` moves
/// down a line but not back to its start.
fn lines_for_terminal(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 32);
    let mut last = 0u8;
    for &b in text.as_bytes() {
        if b == b'\n' && last != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        last = b;
    }
    out
}

fn mods(m: &gpui::Modifiers) -> Mods {
    Mods {
        shift: m.shift,
        ctrl: m.control,
        alt: m.alt,
        logo: m.platform,
    }
}

/// The modifier that turns a click into "open link": Cmd on macOS, Ctrl elsewhere.
fn link_mod(m: &gpui::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
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

impl TermViewer {
    /// A viewer in `look`, or None if its terminal could not start.
    pub fn spawn(look: Look, cx: &mut App) -> Option<Entity<TermViewer>> {
        // The terminal thread wakes the view through this; one pending wake is enough.
        let (wake_tx, wake) = async_channel::bounded::<()>(1);
        let (event_tx, events) = async_channel::unbounded::<TermEvent>();
        let term = TermHandle::spawn(
            80,
            24,
            look.scrollback,
            // Nothing it would answer goes anywhere.
            Box::new(|_| {}),
            Box::new(move || {
                let _ = wake_tx.try_send(());
            }),
            Box::new(move |ev| {
                let _ = event_tx.try_send(ev);
            }),
        )
        .map_err(|e| eprintln!("cannot start a terminal to show output: {e}"))
        .ok()?;
        term.set_colors(look.colors());
        term.feed(HIDE_CURSOR.to_vec());
        Some(cx.new(|cx| {
            cx.spawn(async move |this, cx| {
                while wake.recv().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn(async move |this, cx| {
                while let Ok(ev) = events.recv().await {
                    let alive = this.update(cx, |_, cx| match ev {
                        TermEvent::Copied(text) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(text))
                        }
                        TermEvent::OpenLink(url) => cx.open_url(&url),
                        _ => {}
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            })
            .detach();
            TermViewer {
                focus: cx.focus_handle(),
                term,
                frame: Arc::new(Frame::default()),
                paint_rows: vec![],
                look,
                font_family: SharedString::default(),
                cell: (px(0.), px(0.)),
                grid: (0, 0),
                grid_origin: Rc::new(Cell::new(point(px(0.), px(0.)))),
                area: Rc::new(Cell::new(Size::default())),
                gradient: Rc::new(RefCell::new(None)),
                fed: None,
                scrollbar_until: None,
                scrollbar_hover: false,
                scrollbar_grab: None,
                find: None,
                drag: None,
            }
        }))
    }

    /// Shows `command` at a prompt (if there is one), then `output`, then
    /// `errors` in red, as `key` (a job's id, say). Shown again with the same
    /// key, only what was added since is fed: a running command's output
    /// comes as it is printed.
    pub fn show(
        &mut self,
        key: &str,
        command: Option<&str>,
        output: &str,
        errors: &str,
        cx: &mut Context<Self>,
    ) {
        let output = with_errors(output, errors);
        let bytes = output.as_bytes();
        if let Some(f) = &mut self.fed
            && f.key == key
            && bytes.len() >= f.len
            && bytes[..f.len].ends_with(&f.tail)
        {
            if bytes.len() > f.len {
                if let Some(new) = output.get(f.len..) {
                    self.term.feed(lines_for_terminal(new));
                }
                f.len = bytes.len();
                f.tail = bytes[bytes.len().saturating_sub(16)..].to_vec();
                cx.notify();
            }
            return;
        }
        let mut all = CLEAR.to_vec();
        all.extend_from_slice(HIDE_CURSOR);
        // The prompt, bold; the lines of a script after it, indented.
        if let Some(command) = command {
            let mut lines = command.lines();
            all.extend_from_slice(b"\x1b[1m$\x1b[0m ");
            all.extend_from_slice(lines.next().unwrap_or("").as_bytes());
            for line in lines {
                all.extend_from_slice(b"\r\n  ");
                all.extend_from_slice(line.as_bytes());
            }
            all.extend_from_slice(b"\r\n");
        }
        all.extend(lines_for_terminal(&output));
        self.term.feed(all);
        self.term.scroll_to_bottom();
        self.fed = Some(Fed {
            key: key.to_string(),
            len: bytes.len(),
            tail: bytes[bytes.len().saturating_sub(16)..].to_vec(),
        });
        cx.notify();
    }

    /// How tall the viewer is to show `rows` rows, its padding included.
    pub fn height_for(&self, rows: usize) -> Pixels {
        let row = (self.look.font_size * self.look.line_height).round();
        // Half a pixel more, so the grid's rows fit however it rounds.
        px(row * rows as f32 + self.look.padding.1 * 2. + 0.5)
    }

    /// The person's look changed (a theme, a font).
    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if look == self.look {
            return;
        }
        if look.theme != self.look.theme {
            self.term.set_colors(look.colors());
        }
        if look.scrollback != self.look.scrollback {
            self.term.set_scrollback(look.scrollback);
        }
        // Measured again on the next render, which also resizes the grid.
        self.cell = (px(0.), px(0.));
        self.look = look;
        self.paint_rows.clear();
        cx.notify();
    }

    /// The text shown, the screen and its history: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn shown_text(&self) -> String {
        self.frame
            .rows
            .iter()
            .map(|r| r.text.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fit_grid(&mut self) {
        let vp = self.area.get();
        if vp.width <= px(0.) || vp.height <= px(0.) {
            return;
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
        }
    }

    /// A window position in the engine's pixels (see `TermView::grid_pos`).
    fn grid_pos(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.grid_origin.get();
        let (x, y) = (f32::from(p.x - o.x), f32::from(p.y - o.y));
        let scale = |at: f32, cell: Pixels| {
            let drawn = f32::from(cell);
            if drawn > 0. {
                at / drawn * (drawn as u32).max(1) as f32
            } else {
                at
            }
        };
        (scale(x, self.cell.0), scale(y, self.cell.1))
    }

    fn mouse(
        &self,
        action: MouseAction,
        button: Option<MouseButton>,
        at: Point<Pixels>,
        m: &gpui::Modifiers,
    ) {
        let (x, y) = self.grid_pos(at);
        self.term.mouse(MouseInput {
            action,
            button: button.and_then(term_button),
            x,
            y,
            mods: mods(m),
            link: link_mod(m),
        });
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.mouse(MouseAction::Press, Some(e.button), e.position, &e.modifiers);
        if e.button == MouseButton::Left && !link_mod(&e.modifiers) {
            self.drag = Some(Drag {
                at: e.position,
                mods: mods(&e.modifiers),
                scrolling: false,
            });
            cx.notify();
        }
    }

    /// The pointer moved, out of the grid, while a selection is dragged.
    fn drag_outside(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let Some(d) = &mut self.drag else {
            return;
        };
        d.at = e.position;
        self.mouse(
            MouseAction::Motion,
            Some(MouseButton::Left),
            e.position,
            &e.modifiers,
        );
        if !self.drag_step(false) {
            return;
        }
        if let Some(d) = &mut self.drag {
            d.scrolling = true;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAG_BEAT).await;
                if !this.update(cx, |v, _| v.drag_step(true)).unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One beat of scrolling while the dragging pointer is above or below
    /// the grid, the selection following: whether it goes on. `beat`: the
    /// scrolling's own, rather than whether one should start.
    fn drag_step(&mut self, beat: bool) -> bool {
        let Some(d) = &self.drag else {
            return false;
        };
        if d.scrolling != beat {
            return false;
        }
        let y = f32::from(d.at.y - self.grid_origin.get().y);
        let rows = drag_scroll(y, f32::from(self.area.get().height), f32::from(self.cell.1));
        if rows == 0 {
            if let Some(d) = &mut self.drag {
                d.scrolling = false;
            }
            return false;
        }
        if beat {
            let (at, m) = (d.at, d.mods);
            let (x, y) = self.grid_pos(at);
            self.term
                .wheel(rows as f32, x, y, Mods { shift: true, ..m });
            self.term.mouse(MouseInput {
                action: MouseAction::Motion,
                button: Some(TermButton::Left),
                x,
                y,
                mods: m,
                link: false,
            });
        }
        true
    }

    fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.scrollbar_grab.is_some() {
            self.let_go_of_thumb(cx);
            return;
        }
        if e.button == MouseButton::Left {
            self.drag = None;
        }
        self.mouse(
            MouseAction::Release,
            Some(e.button),
            e.position,
            &e.modifiers,
        );
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Held: [`crate::scrollbar::hold`] has it, in the element or out.
        if self.scrollbar_grab.is_some() {
            return;
        }
        let (x, y) = self.grid_pos(e.position);
        // In the window's pixels, as the bar is drawn: the grid's are fewer
        // where a cell is not a whole pixel wide, and never reach the edge.
        let edge = self.grid_origin.get().x + self.area.get().width;
        let hover = e.position.x >= edge - px(SCROLLBAR_ZONE);
        if hover != self.scrollbar_hover {
            self.scrollbar_hover = hover;
            cx.notify();
        }
        if let Some(d) = &mut self.drag {
            d.at = e.position;
        }
        if link_mod(&e.modifiers) {
            self.term.hover(Some((x, y)));
        } else if self.frame.hover_link.is_some() {
            self.term.hover(None);
        }
        self.mouse(
            MouseAction::Motion,
            e.pressed_button,
            e.position,
            &e.modifiers,
        );
    }

    fn scroll_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        // With nothing to scroll here, the page around it scrolls.
        if self.frame.scroll.total <= self.frame.scroll.len {
            return;
        }
        cx.stop_propagation();
        self.reveal_scrollbar(cx);
        // GPUI reports positive y when scrolling up; the terminal wants negative for up.
        let lines = match e.delta {
            ScrollDelta::Lines(p) => -p.y * WHEEL_LINES,
            ScrollDelta::Pixels(p) => -f32::from(p.y) / f32::from(self.cell.1),
        };
        let (x, y) = self.grid_pos(e.position);
        self.term.wheel(lines, x, y, mods(&e.modifiers));
    }

    /// The held thumb follows the pointer, at `at` in the window.
    fn drag_thumb(&mut self, at: Point<Pixels>) {
        let Some(grab) = self.scrollbar_grab else {
            return;
        };
        // The window's pixels, as the thumb was taken in (not the grid's).
        let y = f32::from(at.y - self.grid_origin.get().y);
        let track = f32::from(self.area.get().height);
        self.term
            .scroll_to(row_for_thumb(self.frame.scroll, track, y - grab));
    }

    fn let_go_of_thumb(&mut self, cx: &mut Context<Self>) {
        self.scrollbar_grab = None;
        self.reveal_scrollbar(cx);
    }

    /// Shows the scrollbar for a moment (it hides again by itself).
    fn reveal_scrollbar(&mut self, cx: &mut Context<Self>) {
        let until = Instant::now() + SCROLLBAR_LINGER;
        self.scrollbar_until = Some(until);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(SCROLLBAR_LINGER + Duration::from_millis(20))
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.scrollbar_until == Some(until) {
                    v.scrollbar_until = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Copy, and the scrollback's keys: nothing else is for this terminal.
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let finding = self
            .find
            .as_ref()
            .is_some_and(|(i, _)| i.read(cx).focus_handle(cx).is_focused(window));
        let copy = if cfg!(target_os = "macos") {
            m.platform && !m.control && k.key == "c"
        } else {
            m.control && !m.alt && k.key == "c"
        };
        match k.key.as_str() {
            _ if copy && !finding && self.frame.selected => self.term.copy(),
            "pageup" if m.shift => self.term.scroll_pages(-1),
            "pagedown" if m.shift => self.term.scroll_pages(1),
            "end" if m.shift && !finding => self.term.scroll_to_bottom(),
            "escape" if self.find.is_some() => self.close_find(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Opens the find bar, or takes the keyboard back to it.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.find {
            Some((input, _)) => {
                input.update(cx, |i, cx| {
                    let text = i.text().to_string();
                    i.set_text(&text, 0..text.len(), cx);
                });
                input.clone()
            }
            None => {
                let input = crate::input::said(cx, || tr!("Find"));
                let changed = cx.subscribe(&input, |v, _, _: &Changed, cx| v.send_find(cx));
                self.find = Some((input.clone(), changed));
                input
            }
        };
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    fn send_find(&mut self, cx: &mut Context<Self>) {
        let Some((input, _)) = &self.find else {
            return;
        };
        let o = cx.try_global::<FindOptions>().copied().unwrap_or_default();
        self.term.find(FindQuery {
            text: input.read(cx).text().to_string(),
            match_case: o.match_case,
            regex: o.regex,
        });
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.term.find(FindQuery::default());
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn find_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        match k.key.as_str() {
            "enter" | "f3" => self.term.find_next(!k.modifiers.shift),
            "up" => self.term.find_next(true),
            "down" => self.term.find_next(false),
            "escape" => self.close_find(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The find bar: the text, how many it found, older and newer, close.
    fn find_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let (input, _) = self.find.as_ref()?;
        let typed = !input.read(cx).text().is_empty();
        let count = match self.frame.find {
            _ if !typed => String::new(),
            None => "…".to_string(),
            Some(s) if s.invalid => tr!("Invalid pattern"),
            Some(s) => match (s.total, s.current) {
                (0, _) if !s.searching => tr!("No results"),
                (n, Some(i)) => tr!(
                    "{current} of {total}{more}",
                    current = i + 1,
                    total = n,
                    more = ""
                ),
                (n, None) => tr!("{total} found{more}", total = n, more = ""),
            },
        };
        let step = |id: &'static str, label: &'static str, older: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .w(px(24.))
                .h(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_color(rgb(colors().soft))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)))
                .on_click(cx.listener(move |v, _, _, _| v.term.find_next(older)))
                .child(label)
        };
        Some(
            div()
                .id("viewer-find")
                .debug_selector(|| "viewer-find".into())
                .absolute()
                .top_3()
                .right(px(SCROLLBAR_ZONE + 6.))
                .flex()
                .items_center()
                .gap_1()
                .pl_2()
                .pr_1()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(colors().edge))
                .bg(rgb(colors().chip))
                .shadow_lg()
                .text_sm()
                .text_color(rgb(colors().text))
                .cursor_default()
                .on_key_down(cx.listener(Self::find_key))
                // Clicks here are not selections in the terminal.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .w(px(180.))
                        .h(px(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().field))
                        .child(input.clone()),
                )
                .child(
                    div()
                        .min_w(px(60.))
                        .text_xs()
                        .text_color(rgb(colors().muted))
                        .child(count),
                )
                .child(step("viewer-find-older", "↑", true, cx))
                .child(step("viewer-find-newer", "↓", false, cx))
                .child(
                    div()
                        .id("viewer-find-close")
                        .w(px(24.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(|v, _, window, cx| v.close_find(window, cx)))
                        .child("✕"),
                )
                .into_any_element(),
        )
    }

    /// The overlay scrollbar, as a tab's: a thumb to drag, a track to page through.
    fn scrollbar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let pos = self.frame.scroll;
        let track = f32::from(self.area.get().height);
        let (top, h) = thumb(pos, track)?;
        let active = self.scrollbar_grab.is_some() || self.scrollbar_hover;
        let shown = match self.look.scrollbar {
            shellwing_config::Scrollbar::Never => false,
            shellwing_config::Scrollbar::Always => true,
            shellwing_config::Scrollbar::Auto => {
                active || self.scrollbar_until.is_some_and(|u| Instant::now() < u)
            }
        };
        if !shown {
            return None;
        }
        let origin_y = self.grid_origin.get().y;
        Some(
            div()
                .id("viewer-scrollbar")
                .debug_selector(|| "viewer-scrollbar".into())
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(SCROLLBAR_ZONE))
                .when(active, |d| d.bg(gpui::rgba(colors().tint)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |v, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let y = f32::from(e.position.y - origin_y);
                        if (top..top + h).contains(&y) {
                            v.scrollbar_grab = Some(y - top);
                        } else {
                            v.term.scroll_pages(if y < top { -1 } else { 1 });
                            v.reveal_scrollbar(cx);
                        }
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .debug_selector(|| "viewer-scrollbar-thumb".into())
                        .absolute()
                        .top(px(top + self.look.padding.1))
                        .right(px(2.))
                        .h(px(h))
                        .w(px(if active {
                            crate::scrollbar::WIDE
                        } else {
                            crate::scrollbar::THIN
                        }))
                        .rounded_full()
                        .bg(gpui::rgba(if active { 0xffffff8c } else { 0xffffff59 })),
                )
                .when(self.scrollbar_grab.is_some(), |d| {
                    d.child(crate::scrollbar::hold(
                        cx.entity().downgrade(),
                        |v: &mut Self, at, _| v.drag_thumb(at),
                        Self::let_go_of_thumb,
                    ))
                })
                .into_any_element(),
        )
    }
}

impl Render for TermViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cell.0 == px(0.) {
            self.font_family = TermView::choose_font(window, self.look.font_family.as_deref());
            self.cell = cell_size(
                window,
                term_font(self.font_family.clone()),
                self.look.font_size,
                self.look.line_height,
            );
        }
        self.fit_grid();
        if let Some(f) = self.term.take_frame() {
            self.frame = f;
        }
        convert_rows(
            &self.frame,
            &self.look.theme,
            &term_font(self.font_family.clone()),
            &mut self.paint_rows,
        );
        let rows: Vec<Vec<Segment>> = self.paint_rows.iter().map(|r| r.segments.clone()).collect();
        let cell = self.cell;
        let fg = hsla(self.frame.foreground);
        let bg = hsla(self.frame.background);
        let hover = self.frame.hover_link;
        let theme = self.look.theme.clone();
        let font_size = px(self.look.font_size);
        // The gradient replaces the background unless a program set its own.
        let gradient = theme
            .gradient
            .clone()
            .filter(|_| self.frame.background == theme_rgb(theme.background));
        let gradient_cache = self.gradient.clone();
        let grid_origin = self.grid_origin.clone();
        let area = self.area.clone();
        let (pad_x, pad_y) = (px(self.look.padding.0), px(self.look.padding.1));
        let dragging = self.drag.is_some().then(|| cx.entity());
        div()
            .id("term-viewer")
            .debug_selector(|| "term-viewer".into())
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .bg(bg)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .on_hover(cx.listener(|v, hovered: &bool, _, cx| {
                if !hovered && v.scrollbar_hover {
                    v.scrollbar_hover = false;
                    cx.notify();
                }
            }))
            .when(hover.is_some(), |d| d.cursor_pointer())
            .child(
                canvas(
                    |_, _, _| {},
                    move |full, _, window, cx| {
                        let bounds = Bounds::new(
                            point(full.origin.x + pad_x, full.origin.y + pad_y),
                            size(
                                (full.size.width - pad_x * 2.).max(px(0.)),
                                (full.size.height - pad_y * 2.).max(px(0.)),
                            ),
                        );
                        grid_origin.set(bounds.origin);
                        if area.get() != bounds.size {
                            // The grid follows on the next frame.
                            area.set(bounds.size);
                            window.refresh();
                        }
                        if let Some(g) = &gradient
                            && let Some(image) =
                                gradient_image(&gradient_cache, &theme, g, full.size, window)
                        {
                            let _ =
                                window.paint_image(full, full, Corners::default(), image, 0, false);
                        }
                        paint_segments(&rows, bounds.origin, cell, font_size, window, cx);
                        // A selection dragged out of the grid goes on following.
                        if let Some(view) = dragging {
                            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                                if phase == gpui::DispatchPhase::Bubble
                                    && !full.contains(&e.position)
                                {
                                    view.update(cx, |v, cx| v.drag_outside(e, cx));
                                }
                            });
                        }
                        if let Some(link) = hover {
                            let o = point(
                                bounds.origin.x + cell.0 * link.start as f32,
                                bounds.origin.y + cell.1 * (link.row as f32 + 1.) - px(2.),
                            );
                            let w = cell.0 * (link.end - link.start) as f32;
                            window.paint_quad(gpui::fill(Bounds::new(o, size(w, px(1.))), fg));
                        }
                    },
                )
                .size_full(),
            )
            .children(self.scrollbar(cx))
            .children(self.find_bar(cx))
            .when(self.frame.scrolled_back, |d| {
                d.child(
                    div()
                        .id("viewer-to-bottom")
                        .absolute()
                        .right_3()
                        .bottom_3()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .bg(rgb(colors().button))
                        .text_color(rgb(colors().text))
                        .font_weight(FontWeight::NORMAL)
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().button_hover)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.term.scroll_to_bottom();
                            cx.notify();
                        }))
                        .child(tr!("↓ Back to bottom (Shift+End)")),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{lines_for_terminal, with_errors};

    #[test]
    fn errors_come_after_in_red() {
        assert_eq!(with_errors("a\n", ""), "a\n");
        assert_eq!(with_errors("a", "b\n"), "a\n\x1b[31mb\n\x1b[0m");
        assert_eq!(with_errors("", "b"), "\x1b[31mb\x1b[0m");
    }

    #[test]
    fn each_line_starts_at_its_left() {
        assert_eq!(lines_for_terminal("a\nb\r\nc\rd\n"), b"a\r\nb\r\nc\rd\r\n");
        assert_eq!(lines_for_terminal(""), b"");
    }
}

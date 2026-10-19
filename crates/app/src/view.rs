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
    App, Bounds, ClipboardItem, Context, Corners, Entity, EntityInputHandler, FocusHandle,
    Focusable, Font, FontFallbacks, FontStyle, FontWeight, Hsla, KeyDownEvent, Modifiers,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Render, RenderImage, ScrollDelta, ScrollWheelEvent, SharedString, Size, TextAlign,
    TextRun, UTF16Selection, UnderlineStyle, Window, canvas, div, font, point, prelude::*, px, rgb,
    size,
};
use shellwing_config::{CursorStyle, Gradient, Theme};
use shellwing_term::{
    Event as TermEvent, FindQuery, Found, Frame, Mods, MouseAction, MouseButton as TermButton,
    MouseInput, NamedKey, Rgb, Row, ScrollPos, TermHandle,
};

use crate::Writer;
use crate::bench::Probe;
use crate::files::{self, FilesEvent, FilesPanel};
use crate::input::{Changed, TextInput};
use crate::look::{Look, rgb as theme_rgb};
use crate::session::{Session, Status};
use shellwing_config::{OnDisconnect, Scrollbar};

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

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
/// How long the font size stays on screen after zooming.
const ZOOM_NOTICE: Duration = Duration::from_secs(2);
/// How a key's install went.
const KEY_NOTICE: Duration = Duration::from_secs(8);
/// Font sizes zooming may reach, as `config.toml` allows.
const FONT_SIZES: (f32, f32) = (6.0, 72.0);
/// The scrollbar stays this long after scrolling stops (with `scrollbar = "auto"`).
const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);
/// The pointer this close to the right edge shows the scrollbar.
const SCROLLBAR_ZONE: f32 = 14.;
const SCROLLBAR_MIN_THUMB: f32 = 24.;

/// The scrollbar thumb's top and height in a track `track` px tall, or None
/// when everything fits (no history, or the alternate screen).
fn thumb(pos: ScrollPos, track: f32) -> Option<(f32, f32)> {
    if pos.total <= pos.len || track <= 0. {
        return None;
    }
    let h = (track * pos.len as f32 / pos.total as f32)
        .max(SCROLLBAR_MIN_THUMB)
        .min(track);
    let top = (track - h) * pos.offset as f32 / (pos.total - pos.len) as f32;
    Some((top, h))
}

/// The first history row to show with the thumb's top at `top`.
fn row_for_thumb(pos: ScrollPos, track: f32, top: f32) -> usize {
    let Some((_, h)) = thumb(pos, track) else {
        return 0;
    };
    let room = (track - h).max(1.);
    ((top.clamp(0., room) / room) * (pos.total - pos.len) as f32).round() as usize
}
/// Longest side of the gradient image. The GPU stretches it with linear
/// filtering, which keeps a gradient smooth, so only its aspect ratio matters.
const GRADIENT_SIZE: f32 = 256.0;

pub fn hsla(c: Rgb) -> Hsla {
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

/// The session's status changed (connected, closed, failed).
pub struct StatusChanged;

/// The session ended and the settings say to close the pane.
pub struct CloseRequested;

/// A secret was saved in the vault.
pub struct VaultChanged;

/// The Files panel's edge was dragged to a new width, to be remembered.
pub struct FilesResized;

/// What the pane's menu asks of the workspace, for this pane.
#[derive(Clone, Copy)]
pub enum PaneCommand {
    /// The palette: hosts, actions, themes, for those who do not know the keys.
    Palette,
    Split(gpui::Axis),
    Forward,
    Close,
}

/// Waits before each automatic reconnect: 1 s, 2 s, … then gives up.
const BACKOFF_SECS: [u64; 6] = [1, 2, 4, 8, 16, 30];

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
    session: Session,
    term: TermHandle,
    writer: Writer,
    frame: Arc<Frame>,
    paint_rows: Vec<PaintRow>,
    grid: (u16, u16),
    cell: (Pixels, Pixels),
    /// Window position of the grid's top-left corner, recorded at paint time.
    grid_origin: Rc<Cell<Point<Pixels>>>,
    /// The area the terminal had at the last paint, which the grid fills.
    area: Rc<Cell<Size<Pixels>>>,
    /// Text the input method is composing (preedit), shown at the cursor.
    marked: Option<String>,
    /// A paste the terminal refused as unsafe, waiting for confirmation.
    pending_paste: Option<(String, Instant)>,
    notice: Option<SharedString>,
    /// Last pointer position over the grid, to show links when the modifier goes down.
    last_mouse: Option<(f32, f32)>,
    font_family: SharedString,
    look: Look,
    /// Points added to the configured font size by zooming; not saved.
    zoom: f32,
    gradient: Rc<RefCell<Option<GradientImage>>>,
    /// The session's status change last handled.
    last_change: u64,
    /// Until when the scrollbar shows after scrolling.
    scrollbar_until: Option<Instant>,
    /// The pointer is at the right edge.
    scrollbar_hover: bool,
    /// Dragging the thumb: where in it the press was, in px from its top.
    scrollbar_grab: Option<f32>,
    /// A typed password or passphrase that worked, offered for saving.
    save_offer: Option<shellwing_ssh::Learned>,
    /// When the next automatic reconnect happens, if one is planned.
    retry_at: Option<Instant>,
    /// Automatic reconnects since the last successful connection.
    retries: usize,
    /// Bumped to cancel a planned reconnect.
    retry_plan: u64,
    probe: Arc<Probe>,
    /// The Files panel, kept while hidden so its folder and transfers stay.
    files: Option<Entity<FilesPanel>>,
    files_shown: bool,
    /// Dragging the panel's edge: where the drag began, and the width then.
    files_drag: Option<(f32, f32)>,
    find: Option<FindBar>,
    key_offer: Option<KeyOffer>,
    /// The tab types in every pane: None when not, else whether this one
    /// listens.
    broadcast: Option<bool>,
    /// The other panes that what is typed here goes to as well.
    echo: Vec<TermHandle>,
    /// The right-click menu, open where it was asked for.
    menu: Option<Point<Pixels>>,
}

/// Finding text on the screen and in its history.
struct FindBar {
    input: Entity<TextInput>,
    _changed: gpui::Subscription,
}

/// How text is found, the same in every pane: the last choice stays for the
/// next find bar.
#[derive(Clone, Copy, Default)]
struct FindOptions {
    match_case: bool,
    regex: bool,
}

impl gpui::Global for FindOptions {}

/// Matches of the text being found, as Ghostty shows them: yellow, and the
/// chosen one orange, under black text.
const MATCH_BG: u32 = 0xffe082;
const CURRENT_BG: u32 = 0xf2a57e;
const FOUND_TEXT: u32 = 0x000000;

/// The pane's host is to sign in with a key the vault keeps.
pub struct UseKey {
    /// The host's name in hosts.toml.
    pub host: String,
    pub vault_id: String,
    pub key_name: String,
}

/// A key just installed on this pane's host, offered as its way to sign in.
struct KeyOffer {
    host: String,
    vault_id: String,
    key_name: String,
}

/// The pane's broadcast badge was clicked: leave it out, or bring it back.
pub struct ListenToggled;

/// Typing in every pane of the tab: amber, as the host list's Edit badge.
const BROADCAST: u32 = 0xf59e0b;

impl gpui::EventEmitter<StatusChanged> for TermView {}
impl gpui::EventEmitter<ListenToggled> for TermView {}
impl gpui::EventEmitter<UseKey> for TermView {}
impl gpui::EventEmitter<CloseRequested> for TermView {}
impl gpui::EventEmitter<VaultChanged> for TermView {}
impl gpui::EventEmitter<FilesResized> for TermView {}
impl gpui::EventEmitter<PaneCommand> for TermView {}

impl TermView {
    pub fn new(session: Session, look: Look, probe: Arc<Probe>, cx: &mut Context<Self>) -> Self {
        session.term.set_colors(look.colors());
        // Repaint whenever the terminal thread has a new frame or the status changes.
        let wake = session.wake.clone();
        cx.spawn(async move |this, cx| {
            while wake.recv().await.is_ok() {
                let alive = this.update(cx, |v, cx| {
                    let (change, status) = v.session.status_change();
                    if change != v.last_change {
                        v.last_change = change;
                        v.status_changed(status, cx);
                        cx.emit(StatusChanged);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        let learned = session.learned.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(secret) = learned.recv().await {
                if this
                    .update(cx, |v, cx| {
                        v.save_offer = Some(secret);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let events = session.events.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |v, cx| v.on_term_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        TermView {
            focus: cx.focus_handle(),
            term: session.term.clone(),
            writer: session.writer.clone(),
            session,
            frame: Arc::new(Frame::default()),
            paint_rows: vec![],
            grid: (0, 0),
            cell: (px(0.), px(0.)),
            grid_origin: Rc::new(Cell::new(point(px(0.), px(0.)))),
            area: Rc::new(Cell::new(Size::default())),
            marked: None,
            pending_paste: None,
            notice: None,
            last_mouse: None,
            font_family: FONT_FAMILIES[0].into(),
            look,
            zoom: 0.,
            gradient: Rc::new(RefCell::new(None)),
            last_change: 0,
            scrollbar_until: None,
            scrollbar_hover: false,
            scrollbar_grab: None,
            save_offer: None,
            retry_at: None,
            retries: 0,
            retry_plan: 0,
            probe,
            files: None,
            files_shown: false,
            files_drag: None,
            find: None,
            key_offer: None,
            broadcast: None,
            echo: vec![],
            menu: None,
        }
    }

    /// The terminal, for other panes to type into (see [`Self::set_broadcast`]).
    pub fn term_handle(&self) -> TermHandle {
        self.term.clone()
    }

    /// Typing in every pane of the tab: whether this one listens, and the
    /// terminals what is typed here goes to as well.
    pub fn set_broadcast(
        &mut self,
        state: Option<bool>,
        echo: Vec<TermHandle>,
        cx: &mut Context<Self>,
    ) {
        if self.broadcast != state {
            cx.notify();
        }
        self.broadcast = state;
        self.echo = echo;
    }

    /// Types `text` into the shell as if typed here, and presses Enter when
    /// `enter`: what a snippet does. Only this pane gets it.
    pub fn type_text(&mut self, text: &str, enter: bool) {
        self.term.text(text.to_string());
        if enter {
            self.term.key(NamedKey::Enter, Mods::default());
        }
    }

    /// The badge on a pane of a tab that types in every pane.
    fn listen_badge(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let listening = self.broadcast?;
        let top = if self.find.is_some() {
            px(52.)
        } else {
            px(12.)
        };
        Some(
            div()
                .id("listen-badge")
                .absolute()
                .top(top)
                .right(px(SCROLLBAR_ZONE + 6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(8.))
                .rounded_full()
                .border_1()
                .border_color(rgb(if listening { 0xa16207 } else { 0x263042 }))
                .bg(rgb(if listening { 0x2a1d07 } else { 0x111a2a }))
                .text_xs()
                .text_color(rgb(if listening { 0xfde68a } else { 0x8a96aa }))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ListenToggled)))
                .child(div().size(px(6.)).rounded_full().bg(rgb(if listening {
                    BROADCAST
                } else {
                    0x5b6578
                })))
                .child(if listening { "listening" } else { "left out" })
                .into_any_element(),
        )
    }

    /// Adds `key` to the host's `authorized_keys` over this pane's
    /// connection, and says how it went; a key the vault keeps is then
    /// offered as the host's way to sign in.
    pub fn install_key(&mut self, key: crate::workspace::KeyToInstall, cx: &mut Context<Self>) {
        let host = self.session.title.clone();
        let Some(answer) = self.session.remote().exec(
            shellwing_ssh::keys::INSTALL,
            format!("{}\n", key.public.trim()).into_bytes(),
        ) else {
            return self.show_notice("Not connected.", CONFIRM_PASTE, cx);
        };
        self.show_notice(
            format!("Installing {} on {host}…", key.name),
            CONFIRM_PASTE,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |v, cx| {
                let done = match result {
                    Ok(Ok(out)) => {
                        shellwing_ssh::keys::installed(out.status, &out.stdout, &out.stderr)
                    }
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("the connection ended".into()),
                };
                let said = match &done {
                    Ok(shellwing_ssh::keys::Installed::Added) => {
                        format!("{} is installed on {host}.", key.name)
                    }
                    Ok(shellwing_ssh::keys::Installed::Present) => {
                        format!("{host} has {} already.", key.name)
                    }
                    Err(why) => format!("{} was not installed on {host}: {why}", key.name),
                };
                // Offered only for a host in hosts.toml not set to it already.
                let offer = key
                    .vault_id
                    .clone()
                    .filter(|id| done.is_ok() && v.session.vault_key() != Some(id.as_str()))
                    .zip(v.session.entry.clone());
                match offer {
                    Some((vault_id, host)) => {
                        v.notice = None;
                        v.key_offer = Some(KeyOffer {
                            host,
                            vault_id,
                            key_name: key.name.clone(),
                        });
                        cx.notify();
                    }
                    None => v.show_notice(said, KEY_NOTICE, cx),
                }
            });
        })
        .detach();
    }

    fn key_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let offer = self.key_offer.as_ref()?;
        let button = |id: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| d.bg(rgb(0x38bdf8)).text_color(rgb(0x04121c)))
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(0x3f4a5c))
                        .hover(|s| s.bg(rgb(0x1e2736)))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        Some(
            div()
                .absolute()
                .top_3()
                .left_3()
                .right_3()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x2b3648))
                .bg(gpui::rgba(0x080a12e6))
                .text_sm()
                .text_color(rgb(0xe2e8f0))
                .child(div().flex_1().min_w_0().child(format!(
                    "{} is installed on {}. Sign in to {} with it from now on?",
                    offer.key_name, offer.host, offer.host
                )))
                .child(
                    button("key-not-now", "Not now".into(), false).on_click(cx.listener(
                        |v, _, _, cx| {
                            v.key_offer = None;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    button("key-use", format!("Use it for {}", offer.host), true).on_click(
                        cx.listener(|v, _, _, cx| {
                            if let Some(o) = v.key_offer.take() {
                                cx.emit(UseKey {
                                    host: o.host,
                                    vault_id: o.vault_id,
                                    key_name: o.key_name,
                                });
                            }
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        )
    }

    /// Opens the find bar, or takes the keyboard back to it with its text
    /// chosen, to type over.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.find {
            Some(f) => {
                f.input.update(cx, |i, cx| {
                    let text = i.text().to_string();
                    i.set_text(&text, 0..text.len(), cx);
                });
                f.input.clone()
            }
            None => {
                let input = cx.new(|cx| TextInput::new("Find", cx));
                let changed = cx.subscribe(&input, |v, _, _: &Changed, cx| v.send_find(cx));
                self.find = Some(FindBar {
                    input: input.clone(),
                    _changed: changed,
                });
                input
            }
        };
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Hands the terminal what the find bar asks for now.
    fn send_find(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.find else {
            return;
        };
        let o = cx.try_global::<FindOptions>().copied().unwrap_or_default();
        self.term.find(FindQuery {
            text: f.input.read(cx).text().to_string(),
            match_case: o.match_case,
            regex: o.regex,
        });
    }

    fn toggle_find_option(&mut self, flip: fn(&mut FindOptions), cx: &mut Context<Self>) {
        let mut o = cx.try_global::<FindOptions>().copied().unwrap_or_default();
        flip(&mut o);
        cx.set_global(o);
        self.send_find(cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.term.find(FindQuery::default());
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Keys in the find bar: Enter (or F3, or Up) for the next match up the
    /// screen and into history, with Shift (or Down) for the next one down;
    /// Alt+C to match case, Alt+R for a regular expression, as in VS Code.
    /// Other keys go on: the terminal leaves them alone (see `on_key`), and
    /// only an unhandled key's text reaches the field.
    fn find_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let alt = m.alt && !m.control && !m.platform;
        match k.key.as_str() {
            "c" if alt => self.toggle_find_option(|o| o.match_case = !o.match_case, cx),
            "r" if alt => self.toggle_find_option(|o| o.regex = !o.regex, cx),
            "enter" | "f3" => self.term.find_next(!k.modifiers.shift),
            "up" => self.term.find_next(true),
            "down" => self.term.find_next(false),
            "escape" => self.close_find(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn find_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let f = self.find.as_ref()?;
        let typed = !f.input.read(cx).text().is_empty();
        let (count, none) = match self.frame.find {
            _ if !typed => (String::new(), false),
            None => ("…".to_string(), false),
            Some(s) if s.invalid => ("Invalid pattern".to_string(), true),
            Some(s) => {
                let more = if s.searching {
                    "…"
                } else if s.capped {
                    "+"
                } else {
                    ""
                };
                match (s.total, s.current) {
                    (0, _) if !s.searching => ("No results".to_string(), true),
                    (n, Some(i)) => (format!("{} of {n}{more}", i + 1), false),
                    (n, None) => (format!("{n} found{more}"), false),
                }
            }
        };
        let options = cx.try_global::<FindOptions>().copied().unwrap_or_default();
        let toggle = |id: &'static str,
                      label: &'static str,
                      tip: &'static str,
                      on: bool,
                      flip: fn(&mut FindOptions),
                      cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(22.))
                .min_w(px(24.))
                .px_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(if on { 0x38bdf8 } else { 0x111a2a }))
                .bg(rgb(if on { 0x17304a } else { 0x111a2a }))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(if on { 0xe2e8f0 } else { 0x8a96aa }))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x1a2333)))
                .tooltip(move |_, cx| {
                    cx.new(|_| crate::sidebar::Tip {
                        title: tip.into(),
                        detail: SharedString::default(),
                    })
                    .into()
                })
                .on_click(cx.listener(move |v, _, _, cx| v.toggle_find_option(flip, cx)))
                .child(label)
        };
        let step = |id: &'static str, label: &'static str, older: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .w(px(24.))
                .h(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_color(rgb(0xb6c0cf))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x1a2333)))
                .on_click(cx.listener(move |v, _, _, _| v.term.find_next(older)))
                .child(label)
        };
        Some(
            div()
                .id("find")
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
                .border_color(rgb(0x2b3648))
                .bg(rgb(0x111a2a))
                .shadow_lg()
                .text_sm()
                .text_color(rgb(0xe2e8f0))
                .cursor_default()
                .on_key_down(cx.listener(Self::find_key))
                // Clicks here are not selections in the terminal.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .w(px(200.))
                        .h(px(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(0x263042))
                        .bg(rgb(0x0b0f17))
                        .child(f.input.clone()),
                )
                .child(toggle(
                    "find-case",
                    "Aa",
                    "Match case (Alt+C)",
                    options.match_case,
                    |o| o.match_case = !o.match_case,
                    cx,
                ))
                .child(toggle(
                    "find-regex",
                    ".*",
                    "Regular expression (Alt+R)",
                    options.regex,
                    |o| o.regex = !o.regex,
                    cx,
                ))
                .child(
                    div()
                        .min_w(px(64.))
                        .px_1()
                        .text_xs()
                        .text_color(rgb(if none { 0xf87171 } else { 0x8a96aa }))
                        .child(count),
                )
                .child(step("find-older", "↑", true, cx))
                .child(step("find-newer", "↓", false, cx))
                .child(
                    div()
                        .id("find-close")
                        .w(px(24.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .text_color(rgb(0xb6c0cf))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(0x1a2333)))
                        .on_click(cx.listener(|v, _, window, cx| v.close_find(window, cx)))
                        .child("✕"),
                )
                .into_any_element(),
        )
    }

    /// Shows the Files panel (opening SFTP the first time) and gives it the
    /// keyboard, or hides it and gives the keyboard back to the terminal.
    pub fn toggle_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_local() {
            return self.show_notice(
                "The Files panel browses a server over SFTP; this pane is a shell on this computer.",
                CONFIRM_PASTE,
                cx,
            );
        }
        if self.files_shown {
            self.hide_files(window, cx);
            return;
        }
        let panel = match &self.files {
            Some(p) => p.clone(),
            None => {
                let remote = self.session.remote();
                let p = cx.new(|cx| FilesPanel::new(remote, cx));
                cx.subscribe_in(&p, window, |v, _, ev: &FilesEvent, window, cx| match ev {
                    FilesEvent::Close => v.hide_files(window, cx),
                })
                .detach();
                self.files = Some(p.clone());
                p
            }
        };
        self.files_shown = true;
        let focus = panel.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn hide_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.files_shown = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn files_shown(&self) -> bool {
        self.files_shown
    }

    fn status_changed(&mut self, status: Status, cx: &mut Context<Self>) {
        match status {
            Status::Connected => {
                self.retries = 0;
                self.retry_at = None;
                // A new connection needs a new SFTP session.
                if let Some(f) = &self.files {
                    f.update(cx, |f, cx| f.connect(cx));
                }
            }
            // Only a connection that once worked is retried without asking.
            Status::Lost
                if self.look.on_disconnect != OnDisconnect::Ask
                    && self.session.ever_connected() =>
            {
                self.plan_retry(cx)
            }
            Status::Closed if self.look.on_disconnect == OnDisconnect::Close => {
                cx.emit(CloseRequested)
            }
            _ => {}
        }
    }

    /// Reconnects after the next back-off delay, counting down on screen.
    fn plan_retry(&mut self, cx: &mut Context<Self>) {
        let Some(&secs) = BACKOFF_SECS.get(self.retries) else {
            // Given up; Enter still tries.
            self.retry_at = None;
            return;
        };
        self.retries += 1;
        self.retry_plan += 1;
        let plan = self.retry_plan;
        let due = Instant::now() + Duration::from_secs(secs);
        self.retry_at = Some(due);
        cx.spawn(async move |this, cx| {
            loop {
                let wait = due
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(1));
                cx.background_executor().timer(wait).await;
                let done = this.update(cx, |v, cx| {
                    if v.retry_plan != plan {
                        return true;
                    }
                    if Instant::now() >= due {
                        v.retry_at = None;
                        v.session.reconnect();
                        return true;
                    }
                    cx.notify();
                    false
                });
                if done.unwrap_or(true) {
                    break;
                }
            }
        })
        .detach();
    }

    fn reconnect_now(&mut self, cx: &mut Context<Self>) {
        self.retry_plan += 1;
        self.retry_at = None;
        self.retries = 0;
        self.session.reconnect();
        cx.notify();
    }

    fn stop_retrying(&mut self, cx: &mut Context<Self>) {
        self.retry_plan += 1;
        self.retry_at = None;
        cx.notify();
    }

    /// Asks whether to keep a password or passphrase that was just typed.
    fn save_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let offer = self.save_offer.as_ref()?;
        let what = match offer {
            shellwing_ssh::Learned::Password { target, .. } => {
                format!("Save the password for {target} in the vault?")
            }
            shellwing_ssh::Learned::Passphrase { key_path, .. } => {
                format!("Save the passphrase of {key_path} in the vault?")
            }
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| d.bg(rgb(0x38bdf8)).text_color(rgb(0x04121c)))
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(0x3f4a5c))
                        .hover(|s| s.bg(rgb(0x1e2736)))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        Some(
            div()
                .absolute()
                .top_3()
                .left_3()
                .right_3()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x2b3648))
                .bg(gpui::rgba(0x080a12e6))
                .text_sm()
                .text_color(rgb(0xe2e8f0))
                .child(div().flex_1().min_w_0().child(what))
                .child(
                    button("save-not-now", "Not now", false).on_click(cx.listener(
                        |v, _, _, cx| {
                            v.save_offer = None;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    button("save-secret", "Save", true).on_click(cx.listener(|v, _, _, cx| {
                        if let Some(secret) = v.save_offer.take() {
                            match v.session.save(&secret) {
                                Ok(()) => {
                                    v.show_notice("Saved in the vault.", CONFIRM_PASTE, cx);
                                    cx.emit(VaultChanged);
                                }
                                Err(e) => {
                                    v.show_notice(format!("Not saved: {e}"), CONFIRM_PASTE, cx)
                                }
                            }
                        }
                        cx.notify();
                    })),
                )
                .into_any_element(),
        )
    }

    /// The bar shown while the pane has no connection.
    /// The host's key changed: what was known and what the server shows,
    /// and whether to trust the new one. Not to be taken lightly: someone in
    /// the middle looks just like this.
    fn changed_key_banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.session.status() != Status::Failed {
            return None;
        }
        let c = self.session.changed_key()?;
        let button = |id: &'static str, label: &'static str, danger: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .whitespace_nowrap()
                .when(danger, |d| {
                    d.border_1()
                        .border_color(rgb(0xb45309))
                        .text_color(rgb(0xfde68a))
                        .hover(|s| s.bg(rgb(0x3a2a0c)))
                })
                .when(!danger, |d| {
                    d.border_1()
                        .border_color(rgb(0x3f4a5c))
                        .hover(|s| s.bg(rgb(0x1e2736)))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        let old = c.old.first().cloned().unwrap_or_default();
        let fact = |label: &str, value: String| {
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .text_color(rgb(0x8a96aa))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .min_w_0()
                        .font_family(self.font_family.clone())
                        .child(value),
                )
        };
        Some(
            div()
                .absolute()
                .left_3()
                .right_3()
                .bottom_3()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x7f1d1d))
                .bg(gpui::rgba(0x1a0b0ef0))
                .text_sm()
                .text_color(rgb(0xe2e8f0))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xfca5a5))
                        .child(format!("The host key for {} has changed.", c.label)),
                )
                .child(div().text_color(rgb(0xcbd5e1)).child(
                    "If the server was reinstalled or its key replaced, trust the new key. If you do not know why it changed, do not: someone may be between you and the server, reading what you send.",
                ))
                .child(fact(
                    "Known",
                    format!("{old}  (known_hosts line {})", c.line),
                ))
                .child(fact("Now", format!("{}  {}", c.fingerprint, c.kind)))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("key-changed-trust", "Trust the new key and connect", true)
                                .on_click(cx.listener(|v, _, _, cx| match v.session.trust_changed_key() {
                                    Ok(()) => v.reconnect_now(cx),
                                    Err(e) => v.show_notice(format!("Not trusted: {e}"), KEY_NOTICE, cx),
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if let Some(b) = self.changed_key_banner(cx) {
            return Some(b);
        }
        let status = self.session.status();
        let why = self.session.ended().unwrap_or_default();
        let text = match (status, self.retry_at) {
            (Status::Lost, Some(due)) => format!(
                "Connection lost. Trying again in {} s (attempt {} of {}).",
                due.saturating_duration_since(Instant::now()).as_secs() + 1,
                self.retries,
                BACKOFF_SECS.len()
            ),
            (Status::Lost, None) if self.retries >= BACKOFF_SECS.len() => {
                format!(
                    "Gave up after {} attempts. Enter tries again.",
                    self.retries
                )
            }
            (Status::Lost, None) => "Connection lost. Enter tries again.".to_string(),
            (Status::Closed, _) if self.session.is_local() => {
                format!("{}. Enter starts it again.", capitalize(&why))
            }
            (Status::Closed, _) => format!("{}. Enter reconnects.", capitalize(&why)),
            (Status::Failed, _) if self.session.ever_connected() || !why.is_empty() => {
                "Could not connect. Enter tries again.".to_string()
            }
            _ => return None,
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| d.bg(rgb(0x38bdf8)).text_color(rgb(0x04121c)))
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(0x3f4a5c))
                        .hover(|s| s.bg(rgb(0x1e2736)))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        Some(
            div()
                .absolute()
                .left_3()
                .right_3()
                .bottom_3()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x2b3648))
                .bg(gpui::rgba(0x080a12e6))
                .text_sm()
                .text_color(rgb(0xe2e8f0))
                .child(div().flex_1().min_w_0().child(text))
                .when(self.retry_at.is_some(), |d| {
                    d.child(
                        button("stop-retry", "Stop (Esc)", false)
                            .on_click(cx.listener(|v, _, _, cx| v.stop_retrying(cx))),
                    )
                })
                .child(
                    button(
                        "reconnect",
                        if self.session.is_local() {
                            "Start again (Enter)"
                        } else {
                            "Reconnect (Enter)"
                        },
                        true,
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.reconnect_now(cx))),
                )
                .into_any_element(),
        )
    }

    /// Where the terminal was last painted, in window coordinates.
    pub fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(self.grid_origin.get(), self.area.get())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn status(&self) -> Status {
        self.session.status()
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    /// Redraws the cached Files panel, as after the UI font changed.
    pub fn refresh_panels(&self, cx: &mut Context<Self>) {
        if let Some(f) = &self.files {
            f.update(cx, |_, cx| cx.notify());
        }
    }

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
        if (&look.font_family, look.font_size, look.line_height)
            != (
                &self.look.font_family,
                self.look.font_size,
                self.look.line_height,
            )
            || look.padding != self.look.padding
        {
            // Measured again on the next render, which also resizes the grid.
            self.cell = (px(0.), px(0.));
        }
        self.look = look;
        // Text runs carry colors and fonts, so every row is converted again.
        self.paint_rows.clear();
        cx.notify();
    }

    /// The configured font size plus the zoom.
    fn font_size(&self) -> f32 {
        (self.look.font_size + self.zoom).clamp(FONT_SIZES.0, FONT_SIZES.1)
    }

    /// Changes the font size of this window by `step` points, or back to the
    /// configured size with `None`.
    fn zoom(&mut self, step: Option<f32>, cx: &mut Context<Self>) {
        let before = self.font_size();
        self.zoom = step.map_or(0., |s| self.zoom + s);
        // Forget steps beyond the limits, so zooming back responds at once.
        self.zoom = self.font_size() - self.look.font_size;
        if self.font_size() != before {
            self.cell = (px(0.), px(0.));
            self.paint_rows.clear();
        }
        self.show_notice(format!("Font size {}", self.font_size()), ZOOM_NOTICE, cx);
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
    /// The font a terminal uses when none is set.
    pub fn default_font(window: &mut Window) -> String {
        Self::choose_font(window, None).to_string()
    }

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

    pub fn show_notice(
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

    /// The right-click menu: what can be done in this pane, with its keys.
    fn pane_menu(&self, at: Point<Pixels>, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mac = cfg!(target_os = "macos");
        let chord = |key: &str| {
            if mac {
                format!("Cmd+{key}")
            } else {
                format!("Ctrl+Shift+{key}")
            }
        };
        let item = |id: &'static str, label: &str, hint: String, enabled: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_3()
                .py(px(5.))
                .rounded(px(4.))
                .text_color(rgb(if enabled { 0xe2e8f0 } else { 0x5b6578 }))
                .whitespace_nowrap()
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(rgb(0x1e2a3d)))
                })
                .child(label.to_string())
                .child(div().text_xs().text_color(rgb(0x7d889b)).child(hint))
        };
        // Runs the item's command and closes the menu.
        let act = |f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            cx.listener(move |v: &mut Self, _: &gpui::ClickEvent, window, cx| {
                v.menu = None;
                f(v, window, cx);
                cx.notify();
            })
        };
        let line = || div().my_1().h(px(1.)).bg(rgb(0x1e2736));
        let status = self.session.status();
        let local = self.session.is_local();
        let menu = div()
            .id("pane-menu")
            // As wide as its longest item: the text grows with the UI font.
            .min_w(px(240.))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(0x111826))
            .border_1()
            .border_color(rgb(0x2b3648))
            .shadow_lg()
            .text_sm()
            // Another click anywhere closes it (a right click opens the next one).
            .on_mouse_down_out(cx.listener(|v, e: &MouseDownEvent, _, cx| {
                if e.button != MouseButton::Right {
                    v.menu = None;
                    cx.notify();
                }
            }))
            .child(
                item("menu-palette", "Command Palette…", chord("K"), true)
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::Palette))),
            )
            .child(line())
            .child(
                item("menu-copy", "Copy", chord("C"), self.frame.selected)
                    .when(self.frame.selected, |d| {
                        d.on_click(act(|v, _, _| v.term.copy()))
                    }),
            )
            .child(
                item("menu-paste", "Paste", chord("V"), true)
                    .on_click(act(|v, _, cx| v.paste_clipboard(cx))),
            )
            .child(line())
            .child(
                item("menu-find", "Find…", chord("F"), true)
                    .on_click(act(|v, window, cx| v.open_find(window, cx))),
            )
            // Files and forwards are SSH's.
            .when(!local, |d| {
                d.child(
                    item(
                        "menu-files",
                        if self.files_shown {
                            "Hide Files"
                        } else {
                            "Browse Files (SFTP)"
                        },
                        chord("O"),
                        true,
                    )
                    .on_click(act(|v, window, cx| v.toggle_files(window, cx))),
                )
            })
            .when(status == Status::Connected && !local, |d| {
                d.child(
                    item("menu-forward", "Forward a Port…", String::new(), true)
                        .on_click(act(|_, _, cx| cx.emit(PaneCommand::Forward))),
                )
            })
            .child(line())
            .child(
                item("menu-split-right", "Split Right", chord("D"), true).on_click(act(
                    |_, _, cx| cx.emit(PaneCommand::Split(gpui::Axis::Horizontal)),
                )),
            )
            .child(
                item(
                    "menu-split-down",
                    "Split Down",
                    if mac {
                        "Cmd+Shift+D".into()
                    } else {
                        chord("E")
                    },
                    true,
                )
                .on_click(act(|_, _, cx| {
                    cx.emit(PaneCommand::Split(gpui::Axis::Vertical))
                })),
            )
            .when(
                matches!(status, Status::Closed | Status::Lost | Status::Failed),
                |d| {
                    d.child(
                        item("menu-reconnect", "Reconnect", String::new(), true)
                            .on_click(act(|v, _, cx| v.reconnect_now(cx))),
                    )
                },
            )
            .child(line())
            .child(
                item("menu-close", "Close Pane", chord("W"), true)
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::Close))),
            );
        gpui::deferred(
            gpui::anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element()
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
        for t in &self.echo {
            t.paste(text.clone(), confirmed);
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
        let line = window
            .text_system()
            .shape_line("M".into(), px(self.font_size()), &[run], None);
        self.cell = (
            line.width,
            px((self.font_size() * self.look.line_height).round()),
        );
    }

    /// Keeps the terminal grid and the remote PTY the size of the terminal's area,
    /// as measured at the last paint.
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
                    color: match (selection_fg, r.found) {
                        (Some(c), _) if r.selected => c,
                        (_, Found::Match | Found::Current) if !r.selected => rgb(FOUND_TEXT).into(),
                        _ => hsla(r.fg),
                    },
                    background_color: match r.found {
                        _ if r.selected => Some(selection_bg),
                        Found::Current => Some(rgb(CURRENT_BG).into()),
                        Found::Match => Some(rgb(MATCH_BG).into()),
                        Found::No => r.bg.map(hsla),
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
        // A right click opens the menu, unless the program takes the mouse; Shift
        // opens it anyway, as Shift selects.
        if e.button == MouseButton::Right && (!self.frame.mouse_tracking || e.modifiers.shift) {
            self.menu = Some(e.position);
            cx.notify();
            return;
        }
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

    fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        // The release of the click that opened the menu.
        if e.button == MouseButton::Right && self.menu.is_some() {
            return;
        }
        if self.scrollbar_grab.take().is_some() {
            self.reveal_scrollbar(cx);
            return;
        }
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

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = self.grid_pos(e.position);
        if let Some(grab) = self.scrollbar_grab {
            let pos = self.frame.scroll;
            let track = f32::from(self.area.get().height);
            self.term.scroll_to(row_for_thumb(pos, track, y - grab));
            return;
        }
        let hover = x >= f32::from(self.area.get().width) - SCROLLBAR_ZONE;
        if hover != self.scrollbar_hover {
            self.scrollbar_hover = hover;
            cx.notify();
        }
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

    /// The overlay scrollbar: a thumb to drag, a track to page through.
    fn scrollbar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let pos = self.frame.scroll;
        let track = f32::from(self.area.get().height);
        let (top, h) = thumb(pos, track)?;
        let active = self.scrollbar_grab.is_some() || self.scrollbar_hover;
        let shown = match self.look.scrollbar {
            Scrollbar::Never => false,
            Scrollbar::Always => true,
            Scrollbar::Auto => active || self.scrollbar_until.is_some_and(|u| Instant::now() < u),
        };
        if !shown {
            return None;
        }
        let origin_y = self.grid_origin.get().y;
        Some(
            div()
                .id("scrollbar")
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(SCROLLBAR_ZONE))
                .when(active, |d| d.bg(gpui::rgba(0xffffff0d)))
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
                        .absolute()
                        .top(px(top + self.look.padding.1))
                        .right(px(3.))
                        .h(px(h))
                        .w(px(if active { 8. } else { 5. }))
                        .rounded_full()
                        .bg(gpui::rgba(if active { 0xffffff8c } else { 0xffffff59 })),
                )
                .into_any_element(),
        )
    }

    fn scroll_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Without history (the alternate screen) there is no scrollbar to show.
        self.reveal_scrollbar(cx);
        // GPUI reports positive y when scrolling up; the terminal wants negative for up.
        let lines = match e.delta {
            ScrollDelta::Lines(p) => -p.y * WHEEL_LINES,
            ScrollDelta::Pixels(p) => -f32::from(p.y) / f32::from(self.cell.1),
        };
        let (x, y) = self.grid_pos(e.position);
        self.term.wheel(lines, x, y, mods(&e.modifiers));
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let key = k.key.as_str();
        // Any key closes the menu; Escape does nothing else.
        if self.menu.take().is_some() {
            cx.notify();
            if key == "escape" {
                return cx.stop_propagation();
            }
        }
        // Typing in the find bar is not for the shell, though it passes by
        // here: GPUI gives a key's text to the field only if no one took the
        // key. Shift+PageUp / PageDown still scroll.
        let finding = self
            .find
            .as_ref()
            .is_some_and(|f| f.input.read(cx).focus_handle(cx).is_focused(window));
        if finding && !(m.shift && matches!(key, "pageup" | "pagedown")) {
            return;
        }
        // A hidden answer is typed past the input method, which is off meanwhile
        // (see `accepts_text_input`): Telex would turn "s" into a tone mark and
        // "dd" into "đ", and its preedit would show the password.
        if self.writer.is_secret_prompt()
            && !(m.control || m.alt || m.platform)
            && let Some(text) = k
                .key_char
                .as_ref()
                .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
        {
            self.term.text(text.clone());
            return cx.stop_propagation();
        }
        // Without a connection, Enter reconnects and Escape stops the countdown.
        if matches!(
            self.session.status(),
            Status::Closed | Status::Lost | Status::Failed
        ) && !(m.control || m.alt || m.platform)
        {
            match key {
                "enter" => {
                    self.reconnect_now(cx);
                    return cx.stop_propagation();
                }
                "escape" if self.retry_at.is_some() => {
                    self.stop_retrying(cx);
                    return cx.stop_propagation();
                }
                _ => {}
            }
        }
        // Clipboard and scrollback shortcuts: Ctrl+Shift on Linux/Windows, Cmd on macOS.
        let app_chord = if cfg!(target_os = "macos") {
            m.platform && !m.control
        } else {
            m.control && m.shift
        };
        // Font size for this window, as in GNOME Terminal and browsers: Ctrl+= (or
        // Ctrl++), Ctrl+- and Ctrl+0 (Cmd on macOS). Ctrl+Shift+- is left alone; it
        // is Ctrl+_ for nano and Emacs.
        let zoom_mod = if cfg!(target_os = "macos") {
            m.platform && !m.control && !m.alt
        } else {
            m.control && !m.alt && !m.platform
        };
        let zoom = match key {
            "=" | "+" | "add" if zoom_mod => Some(Some(1.)),
            "-" | "subtract" if zoom_mod => Some(Some(-1.)),
            "0" if zoom_mod => Some(None),
            _ => None,
        };
        if let Some(step) = zoom {
            self.zoom(step, cx);
            return cx.stop_propagation();
        }
        match key {
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
                self.reveal_scrollbar(cx);
                return cx.stop_propagation();
            }
            "end" if m.shift => {
                self.term.scroll_to_bottom();
                self.reveal_scrollbar(cx);
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
        for t in &self.echo {
            t.key(named, mods);
        }
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
        self.fit_grid();
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
        let font_size = px(self.font_size());
        // The gradient replaces the background unless a program set its own (OSC 11).
        let gradient = theme
            .gradient
            .clone()
            .filter(|_| self.frame.background == theme_rgb(theme.background));
        let gradient_cache = self.gradient.clone();
        let marked = self
            .marked
            .clone()
            .filter(|_| !self.writer.is_secret_prompt());
        let base_font = self.base_font();
        let entity = cx.entity();
        let focus = self.focus.clone();
        let probe = self.probe.clone();
        let grid_origin = self.grid_origin.clone();
        let area = self.area.clone();
        let (pad_x, pad_y) = (px(self.look.padding.0), px(self.look.padding.1));

        let mut terminal = div()
            .id("terminal")
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            // Leaving at the right edge must not leave the scrollbar shown.
            .on_hover(cx.listener(|v, hovered: &bool, _, cx| {
                if !hovered && v.scrollbar_hover {
                    v.scrollbar_hover = false;
                    cx.notify();
                }
            }))
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
                    move |full, _, window, cx| {
                        // The grid sits inside the padding; the background fills it all.
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
            // Below the find bar, when that is open.
            let top = if self.find.is_some() {
                px(52.)
            } else {
                px(12.)
            };
            terminal = terminal.child(badge().top(top).child(n.clone()));
        }
        if let Some(bar) = self.find_bar(cx) {
            terminal = terminal.child(bar);
        }
        if let Some(s) = self.scrollbar(cx) {
            terminal = terminal.child(s);
        }
        if let Some(s) = self.save_bar(cx) {
            terminal = terminal.child(s);
        }
        if let Some(k) = self.key_bar(cx) {
            terminal = terminal.child(k);
        }
        // Typing in every pane: an amber edge on those that listen, the
        // others dimmed; each with its badge.
        match self.broadcast {
            Some(true) => {
                terminal = terminal.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(rgb(BROADCAST)),
                )
            }
            Some(false) => {
                terminal = terminal.child(div().absolute().inset_0().bg(gpui::rgba(0x0b0f1766)))
            }
            None => {}
        }
        if let Some(b) = self.listen_badge(cx) {
            terminal = terminal.child(b);
        }
        if let Some(at) = self.menu {
            terminal = terminal.child(self.pane_menu(at, cx));
        }
        if let Some(b) = self.banner(cx) {
            terminal = terminal.child(b);
        } else if self.frame.scrolled_back {
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

        // The panel is its own cached view: terminal frames do not lay it out again.
        let files = self.files.clone().filter(|_| self.files_shown);
        let width = cx
            .try_global::<files::PanelWidth>()
            .map_or(files::WIDTH, |w| w.0);
        let dragging = self.files_drag.is_some();
        div()
            .relative()
            .flex()
            .flex_row()
            .flex_1()
            .w_full()
            .min_h_0()
            .child(terminal)
            .when_some(files, |d, f| {
                d.child(
                    f.cached(
                        gpui::StyleRefinement::default()
                            .w(px(width))
                            .h_full()
                            .flex_shrink_0(),
                    ),
                )
                // Its left edge: drag to resize, double-click for the default.
                .child(
                    div()
                        .id("files-edge")
                        .group("files-edge")
                        .absolute()
                        .top_0()
                        .h_full()
                        .right(px(width - 3.))
                        .w(px(6.))
                        .flex()
                        .justify_center()
                        .cursor(gpui::CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |v, e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if e.click_count >= 2 {
                                    v.files_drag = None;
                                    cx.set_global(files::PanelWidth(files::WIDTH));
                                    cx.emit(FilesResized);
                                } else {
                                    v.files_drag = Some((f32::from(e.position.x), width));
                                }
                                v.refresh_panels(cx);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .h_full()
                                .w(px(if dragging { 3. } else { 1. }))
                                .bg(rgb(if dragging { 0x38bdf8 } else { 0x1e2736 }))
                                .group_hover("files-edge", |s| s.w(px(2.)).bg(rgb(0x38bdf8))),
                        ),
                )
                .when(dragging, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(gpui::relative(0.45))
                            .right(px(width + 12.))
                            .px(px(9.))
                            .py(px(3.))
                            .rounded_md()
                            .bg(rgb(0x38bdf8))
                            .text_color(rgb(0x04121c))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} px", width.round())),
                    )
                })
            })
            .when(dragging, |d| {
                d.on_mouse_move(cx.listener(|v, e: &MouseMoveEvent, _, cx| {
                    if let Some((x0, w0)) = v.files_drag {
                        // Dragging left widens it.
                        let w = (w0 - (f32::from(e.position.x) - x0))
                            .clamp(files::MIN_WIDTH, files::MAX_WIDTH);
                        cx.set_global(files::PanelWidth(w));
                        v.refresh_panels(cx);
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|v, _, _, cx| {
                        v.files_drag = None;
                        cx.emit(FilesResized);
                        cx.notify();
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|v, _, _, cx| {
                        v.files_drag = None;
                        cx.emit(FilesResized);
                        cx.notify();
                    }),
                )
            })
    }
}

impl EntityInputHandler for TermView {
    /// Off while a hidden answer is typed, so the platform turns the input method
    /// off; keys then arrive in `on_key` as typed.
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.writer.is_secret_prompt()
    }

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
        for t in &self.echo {
            t.text(text.to_string());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(total: u64, offset: u64, len: u64) -> ScrollPos {
        ScrollPos { total, offset, len }
    }

    #[test]
    fn thumb_matches_the_viewport() {
        // 100 rows, 25 shown, at the bottom of a 400 px track.
        assert_eq!(thumb(pos(100, 75, 25), 400.), Some((300., 100.)));
        assert_eq!(thumb(pos(100, 0, 25), 400.), Some((0., 100.)));
        // Long history: the thumb keeps a size you can grab.
        assert_eq!(
            thumb(pos(100_000, 0, 25), 400.).map(|t| t.1),
            Some(SCROLLBAR_MIN_THUMB)
        );
        // Nothing to scroll.
        assert_eq!(thumb(pos(25, 0, 25), 400.), None);
    }

    #[test]
    fn dragging_the_thumb_picks_the_row() {
        let p = pos(100, 75, 25);
        assert_eq!(row_for_thumb(p, 400., 0.), 0);
        assert_eq!(row_for_thumb(p, 400., 150.), 38);
        assert_eq!(row_for_thumb(p, 400., 300.), 75);
        assert_eq!(row_for_thumb(p, 400., 900.), 75, "clamped at the bottom");
    }
}

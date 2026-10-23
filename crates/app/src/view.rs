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
use shellwing_i18n::{tr, tr_n};
use shellwing_term::{
    Event as TermEvent, FindQuery, Found, Frame, Mods, MouseAction, MouseButton as TermButton,
    MouseInput, NamedKey, Rgb, Row, ScrollPos, TermHandle,
};

use crate::Writer;
use crate::bench::Probe;
use crate::colors::colors;
use crate::files::{self, FilesEvent, FilesPanel};
use crate::input::{Changed, TextInput};
use crate::look::{Look, rgb as theme_rgb};
use crate::notice::{Notice, colored};
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
/// Where a log or the saved output went.
const LOG_NOTICE: Duration = Duration::from_secs(6);
/// Font sizes zooming may reach, as `config.toml` allows.
const FONT_SIZES: (f32, f32) = (6.0, 72.0);
/// The scrollbar stays this long after scrolling stops (with `scrollbar = "auto"`).
const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);
/// The pointer this close to the right edge shows the scrollbar.
const SCROLLBAR_ZONE: f32 = 14.;
const SCROLLBAR_MIN_THUMB: f32 = 24.;

/// A page for the terminal's HTML, in the pane's colors.
fn html_page(title: &str, body: &[u8], colors: &shellwing_term::Colors) -> Vec<u8> {
    let hex = |c: shellwing_term::Rgb| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut page = format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{title}</title></head>\n<body style=\"margin: 16px; background: {}; color: {}\">\n",
        hex(colors.background),
        hex(colors.foreground)
    )
    .into_bytes();
    page.extend_from_slice(body);
    page.extend_from_slice(b"\n</body></html>\n");
    page
}

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

/// An environment loaded into a pane's shell.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedEnv {
    pub id: String,
    pub name: String,
    pub names: Vec<String>,
    /// Its `NAME=value`s, in bytes.
    pub bytes: usize,
}

/// Values waiting to go to the shell: when it signals `ready` (its echo
/// off), and, for one loaded into the shell, `done`.
struct EnvLoad {
    ready: String,
    done: Option<String>,
    values: Vec<zeroize::Zeroizing<String>>,
    env: Option<LoadedEnv>,
}

/// How long the shell has to say it is ready before a load is given up:
/// not a POSIX shell (fish), or not at its prompt.
const ENV_WAIT: Duration = Duration::from_secs(15);

/// A word for one load's signals, unlike any other in the pane.
/// The lines that carry `sent`'s values, in turn.
fn lines(sent: Vec<crate::environment::Sent>) -> Vec<zeroize::Zeroizing<String>> {
    sent.into_iter().flat_map(|s| s.lines).collect()
}

fn env_nonce() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{t:x}{n:x}")
}

/// The session ended and the settings say to close the pane.
pub struct CloseRequested;

/// The program set the terminal's title (OSC 0/2).
pub struct TitleChanged;

/// A secret was saved in the vault.
pub struct VaultChanged;

/// Config files no longer placed until the tab closes on the pane's host
/// (their full paths), taken away or kept there: no longer to tidy up later.
pub struct Unplaced(pub Vec<String>);

/// The Files panel's edge was dragged to a new width, to be remembered.
pub struct FilesResized;

/// What the pane's menu asks of the workspace, for this pane.
#[derive(Clone, Copy)]
pub enum PaneCommand {
    /// The palette: hosts, actions, themes, for those who do not know the keys.
    Palette,
    /// The palette, at the snippets for this pane's host.
    Snippets,
    /// The palette, at the environments to load.
    Environments,
    /// The palette, at the config files to place.
    ConfigFiles,
    /// Keep a file of the host in the vault, as a config file.
    SaveFile,
    Split(gpui::Axis),
    Forward,
    /// Log the pane to a file, or stop: the workspace has the settings.
    Log,
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
    /// The window's bottom corners this pane fills, which it rounds; the
    /// workspace sets them as it lays the panes out.
    corners: Rc<Cell<crate::chrome::BottomCorners>>,
    /// What the program says the terminal is (OSC 0/2), such as the shell's
    /// `user@host: ~/dir`.
    shell_title: Option<SharedString>,
    /// The area the terminal had at the last paint, which the grid fills.
    area: Rc<Cell<Size<Pixels>>>,
    /// Text the input method is composing (preedit), shown at the cursor.
    marked: Option<String>,
    /// A paste the terminal refused as unsafe, waiting for confirmation.
    pending_paste: Option<(String, Instant)>,
    /// Environments typed for, waiting for the shell to say its echo is
    /// off before their values go.
    env_pending: Vec<EnvLoad>,
    /// Environments loaded into this pane's shell.
    env_loaded: Vec<LoadedEnv>,
    /// Config files placed on the host until the tab closes.
    placed: Vec<crate::placing::Placed>,
    notice: Option<Notice>,
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

impl gpui::EventEmitter<StatusChanged> for TermView {}
impl gpui::EventEmitter<ListenToggled> for TermView {}
impl gpui::EventEmitter<UseKey> for TermView {}
impl gpui::EventEmitter<CloseRequested> for TermView {}
impl gpui::EventEmitter<TitleChanged> for TermView {}
impl gpui::EventEmitter<VaultChanged> for TermView {}
impl gpui::EventEmitter<Unplaced> for TermView {}
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
            corners: Rc::default(),
            shell_title: None,
            area: Rc::new(Cell::new(Size::default())),
            marked: None,
            pending_paste: None,
            env_pending: vec![],
            env_loaded: vec![],
            placed: vec![],
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

    /// What the program says the terminal is (OSC 0/2), if it said.
    pub fn shell_title(&self) -> Option<&SharedString> {
        self.shell_title.as_ref()
    }

    /// What the shell's title adds to the host's name, for the tab: a
    /// `user@host:` start (bash's and zsh's own titles) is left out, since
    /// the tab names the host already.
    pub fn title_detail(&self) -> Option<String> {
        shell_detail(self.shell_title.as_deref()?, &self.session.title)
    }

    /// The window's bottom corners this pane fills (see `corners`).
    pub fn set_corners(&self, corners: crate::chrome::BottomCorners) {
        self.corners.set(corners);
    }

    /// Types `text` into the shell as if typed here, and presses Enter when
    /// `enter`: what a snippet does. Only this pane gets it.
    pub fn type_text(&mut self, text: &str, enter: bool) {
        // Several lines go as one paste: bracketed where the shell takes
        // that, so they arrive whole and run at the Enter, not line by line
        // as typed.
        let text = text.trim_end_matches('\n');
        if text.contains('\n') {
            self.term.paste(text.to_string(), true);
        } else {
            self.term.text(text.to_string());
        }
        if enter {
            self.term.key(NamedKey::Enter, Mods::default());
        }
    }

    /// Why nothing can be typed into the shell now, if so: not connected,
    /// or a full-screen program has the keys.
    pub fn shell_unready(&self) -> Option<String> {
        if self.session.status() != Status::Connected {
            return Some(tr!("Not connected."));
        }
        self.frame.alternate.then(|| {
            tr!("A full-screen program is running in this pane: leave it, then try again.")
        })
    }

    /// The config files placed on this pane's host until its tab closes.
    pub fn placed_files(&self) -> &[crate::placing::Placed] {
        &self.placed
    }

    pub fn add_placed(&mut self, p: crate::placing::Placed, cx: &mut Context<Self>) {
        self.placed.retain(|x| x.path != p.path);
        self.placed.push(p);
        cx.notify();
    }

    /// Takes the placed config files away from the host (all of them, or
    /// the one at `path`), on the network runtime; the ones gone are said
    /// with [`Unplaced`]. None when there are none, or no connection.
    pub fn take_down(
        &mut self,
        path: Option<&str>,
    ) -> Option<tokio::task::JoinHandle<(Vec<String>, Option<String>)>> {
        let todo: Vec<crate::placing::Placed> = self
            .placed
            .iter()
            .filter(|p| path.is_none_or(|x| p.path == x))
            .cloned()
            .collect();
        if todo.is_empty() || self.session.status() != Status::Connected {
            return None;
        }
        let remote = self.session.remote();
        let answer = remote.sftp()?;
        let job = async move {
            let sftp = match crate::placing::open(answer).await {
                Ok(s) => s,
                Err(e) => return (vec![], Some(e)),
            };
            let (mut gone, mut problem) = (vec![], None);
            for p in todo {
                match crate::placing::unplace(&sftp, &p.path, p.aside.as_deref()).await {
                    Ok(()) => gone.push(p.path),
                    Err(e) => problem = Some(e),
                }
            }
            (gone, problem)
        };
        // A host that does not answer does not hold the tab open long.
        Some(remote.rt.spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(10), job)
                .await
                .unwrap_or_else(|_| (vec![], Some("no answer".into())))
        }))
    }

    /// The placed files at `paths` are no longer this pane's to take away:
    /// gone from the host, or kept there.
    pub fn took_down(&mut self, paths: &[String], cx: &mut Context<Self>) {
        self.placed.retain(|p| !paths.contains(&p.path));
        if !paths.is_empty() {
            cx.emit(Unplaced(paths.to_vec()));
        }
        cx.notify();
    }

    /// Takes away the placed file at `path` (None: all), and says so.
    pub fn remove_placed(&mut self, path: Option<&str>, cx: &mut Context<Self>) {
        let Some(job) = self.take_down(path) else {
            return self.show_notice(Notice::warn(tr!("Not connected.")), LOG_NOTICE, cx);
        };
        cx.spawn(async move |this, cx| {
            let (gone, problem) = job.await.unwrap_or_else(|e| (vec![], Some(e.to_string())));
            let _ = this.update(cx, |v, cx| {
                v.took_down(&gone, cx);
                match problem {
                    Some(e) => {
                        v.show_notice(Notice::bad(tr!("Not removed: {e}", e = e)), LOG_NOTICE, cx)
                    }
                    None => v.show_notice(
                        Notice::good(tr_n!(gone.len(), "Removed {n} file.", "Removed {n} files.")),
                        LOG_NOTICE,
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    /// Leaves the placed file at `path` (None: all) on the host: closing the
    /// tab no longer takes it away. What it set aside becomes its backup,
    /// `….bak`, which needs the connection. Says so.
    pub fn keep_placed(&mut self, path: Option<&str>, cx: &mut Context<Self>) {
        let todo: Vec<crate::placing::Placed> = self
            .placed
            .iter()
            .filter(|p| path.is_none_or(|x| p.path == x))
            .cloned()
            .collect();
        if todo.is_empty() {
            return;
        }
        if todo.iter().all(|p| p.aside.is_none()) {
            let paths: Vec<String> = todo.into_iter().map(|p| p.path).collect();
            return self.kept_placed(paths, vec![], None, cx);
        }
        let remote = self.session.remote();
        let answer = (self.session.status() == Status::Connected)
            .then(|| remote.sftp())
            .flatten();
        let Some(answer) = answer else {
            return self.show_notice(Notice::warn(tr!("Not connected.")), LOG_NOTICE, cx);
        };
        let job = async move {
            let sftp = match crate::placing::open(answer).await {
                Ok(s) => s,
                Err(e) => return (vec![], vec![], Some(e)),
            };
            let (mut kept, mut backups, mut problem) = (vec![], vec![], None);
            for p in todo {
                match crate::placing::keep(&sftp, &p.path, p.aside.as_deref()).await {
                    Ok(backup) => {
                        kept.push(p.path);
                        backups.extend(backup);
                    }
                    Err(e) => problem = Some(e),
                }
            }
            (kept, backups, problem)
        };
        let job = remote.rt.spawn(async move {
            tokio::time::timeout(std::time::Duration::from_secs(10), job)
                .await
                .unwrap_or_else(|_| (vec![], vec![], Some("no answer".into())))
        });
        cx.spawn(async move |this, cx| {
            let (kept, backups, problem) = job
                .await
                .unwrap_or_else(|e| (vec![], vec![], Some(e.to_string())));
            let _ = this.update(cx, |v, cx| v.kept_placed(kept, backups, problem, cx));
        })
        .detach();
    }

    /// The placed files at `paths` stay on the host; what they set aside is
    /// at `backups`.
    fn kept_placed(
        &mut self,
        paths: Vec<String>,
        backups: Vec<String>,
        problem: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.took_down(&paths, cx);
        if let Some(e) = problem {
            let said = Notice::bad(tr!("Not kept: {e}", e = e));
            return self.show_notice(said, LOG_NOTICE, cx);
        }
        let mut said = vec![match paths.as_slice() {
            [one] => tr!("{path} stays when the tab closes.", path = one.as_str()),
            all => tr_n!(
                all.len(),
                "{n} file stays when the tab closes.",
                "{n} files stay when the tab closes."
            ),
        }];
        for b in &backups {
            said.push(tr!("What was there before is {path}.", path = b.as_str()));
        }
        self.show_notice(Notice::good(said.join(" ")), LOG_NOTICE, cx);
    }

    /// The environments loaded into this pane's shell.
    pub fn loaded_envs(&self) -> &[LoadedEnv] {
        &self.env_loaded
    }

    /// Loads `env` (its `values`) into the shell: a command that reads them
    /// with the echo off is typed, and they go once it is ready.
    pub fn load_env(
        &mut self,
        env: LoadedEnv,
        values: shellwing_vault::EnvValues,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(why) = self.shell_unready() {
            return Err(why);
        }
        let (names, sent, bytes) = Self::env_values(values)?;
        let others = self.env_bytes(Some(&env.id));
        Self::fits(others, bytes)?;
        let (ready, done) = (
            format!("ready-{}", env_nonce()),
            format!("done-{}", env_nonce()),
        );
        self.term
            .text(crate::environment::load(&sent, &ready, &done));
        self.term.key(NamedKey::Enter, Mods::default());
        self.wait_for(
            EnvLoad {
                ready,
                done: Some(done),
                values: lines(sent),
                env: Some(LoadedEnv {
                    names,
                    bytes,
                    ..env
                }),
            },
            cx,
        );
        Ok(())
    }

    /// Runs `command` in a subshell that has `values` (a snippet that needs
    /// them): they go when it ends.
    pub fn run_with_env(
        &mut self,
        values: shellwing_vault::EnvValues,
        command: &str,
        enter: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(why) = self.shell_unready() {
            return Err(why);
        }
        let (_, sent, bytes) = Self::env_values(values)?;
        // The subshell has the ones loaded too.
        Self::fits(self.env_bytes(None), bytes)?;
        let ready = format!("ready-{}", env_nonce());
        let text = crate::environment::wrapped(&sent, &ready, command.trim_end_matches('\n'));
        // Several lines: one paste, so it runs whole at the Enter.
        self.term.paste(text, true);
        if enter {
            self.term.key(NamedKey::Enter, Mods::default());
        }
        self.wait_for(
            EnvLoad {
                ready,
                done: None,
                values: lines(sent),
                env: None,
            },
            cx,
        );
        Ok(())
    }

    /// Unloads the environment `id` from the shell (None: all of them).
    pub fn unload_env(&mut self, id: Option<&str>, cx: &mut Context<Self>) -> Result<(), String> {
        if let Some(why) = self.shell_unready() {
            return Err(why);
        }
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.env_loaded)
            .into_iter()
            .partition(|e| id.is_none_or(|id| e.id == id));
        self.env_loaded = kept;
        let names: Vec<String> = gone.iter().flat_map(|e| e.names.clone()).collect();
        if !names.is_empty() {
            self.term.text(crate::environment::unload(&names));
            self.term.key(NamedKey::Enter, Mods::default());
        }
        cx.notify();
        Ok(())
    }

    /// The variables to send, each value as its lines; the names too.
    fn env_values(
        values: shellwing_vault::EnvValues,
    ) -> Result<(Vec<String>, Vec<crate::environment::Sent>, usize), String> {
        if values.is_empty() {
            return Err(tr!("It has no variables."));
        }
        // Kept within the limits, whatever device saved it.
        if let Some(o) = shellwing_vault::environment_oversize(
            values.iter().map(|(n, v)| (n.as_str(), v.as_str())),
        ) {
            return Err(crate::environment::said_oversize(&o));
        }
        let bytes = values.iter().map(|(n, v)| n.len() + 1 + v.len()).sum();
        if let Some((n, _)) = values
            .iter()
            .find(|(n, _)| !crate::environment::valid_name(n))
        {
            return Err(tr!("{name} is not a variable's name.", name = n.clone()));
        }
        let names = values.iter().map(|(n, _)| n.clone()).collect();
        let sent = values
            .iter()
            .map(|(n, v)| crate::environment::encode(n, v))
            .collect();
        Ok((names, sent, bytes))
    }

    /// The bytes of the environments loaded here, but `except`'s.
    fn env_bytes(&self, except: Option<&str>) -> usize {
        self.env_loaded
            .iter()
            .filter(|e| except.is_none_or(|x| e.id != x))
            .map(|e| e.bytes)
            .sum()
    }

    /// Whether `bytes` more fit in the shell beside `loaded`.
    fn fits(loaded: usize, bytes: usize) -> Result<(), String> {
        let max = shellwing_vault::limits::LOADED;
        if loaded + bytes <= max {
            return Ok(());
        }
        Err(tr!(
            "With the ones loaded, that comes to {size} KB; a shell takes {max} KB at most: unload one first.",
            size = (loaded + bytes).div_ceil(1024),
            max = max / 1024
        ))
    }

    /// Keeps `load` until the shell signals, or gives it up after a while.
    fn wait_for(&mut self, load: EnvLoad, cx: &mut Context<Self>) {
        let ready = load.ready.clone();
        self.env_pending.push(load);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENV_WAIT).await;
            let _ = this.update(cx, |v, cx| {
                let before = v.env_pending.len();
                v.env_pending.retain(|p| p.ready != ready);
                if v.env_pending.len() < before {
                    v.show_notice(
                        Notice::warn(tr!("The shell did not take the variables: it is not at its prompt, or not a POSIX shell (sh, bash, zsh).")),
                        LOG_NOTICE,
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// A signal from a command shellwing typed: the shell is ready for the
    /// values, or has them.
    fn env_signal(&mut self, signal: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.env_pending.iter().position(|p| p.ready == signal) {
            for value in std::mem::take(&mut self.env_pending[i].values) {
                self.term.text(value.to_string());
                self.term.key(NamedKey::Enter, Mods::default());
            }
            if self.env_pending[i].done.is_none() {
                self.env_pending.remove(i);
            }
            return;
        }
        if let Some(i) = self
            .env_pending
            .iter()
            .position(|p| p.done.as_deref() == Some(signal))
        {
            let load = self.env_pending.remove(i);
            if let Some(env) = load.env {
                let name = env.name.clone();
                self.env_loaded.retain(|e| e.id != env.id);
                self.env_loaded.push(env);
                self.show_notice(
                    Notice::good(tr!("{name} is loaded into this shell.", name = name)),
                    LOG_NOTICE,
                    cx,
                );
            }
            cx.notify();
        }
    }

    /// Starts logging the pane to a new file, or stops, and says where.
    pub fn toggle_log(&mut self, settings: &shellwing_config::Log, cx: &mut Context<Self>) {
        let said = match self.session.stop_log() {
            Some(path) => Notice::good(tr!("Log saved: {path}", path = path.display().to_string())),
            None => match self.session.start_log(settings) {
                Ok(path) => tr!("Logging to {path}", path = path.display().to_string()).into(),
                Err(e) => Notice::bad(tr!("Cannot log: {e}", e = e.to_string())),
            },
        };
        self.show_notice(said, LOG_NOTICE, cx);
    }

    /// Asks where, then saves all the text, the history included: as plain
    /// text, or as an HTML page with its colors.
    pub fn save_output(&mut self, html: bool, window: &mut Window, cx: &mut Context<Self>) {
        let name = format!(
            "{}_{}.{}",
            shellwing_core::logs::file_safe(&self.session.title),
            chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
            if html { "html" } else { "txt" }
        );
        let picked = cx.prompt_for_new_path(&crate::files::downloads_dir(), Some(&name));
        let (term, title, colors) = (
            self.term.clone(),
            self.session.title.clone(),
            self.look.colors(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let (tx, rx) = async_channel::bounded(1);
            term.format_all(
                html,
                Box::new(move |bytes| {
                    let _ = tx.try_send(bytes);
                }),
            );
            let Ok(mut bytes) = rx.recv().await else {
                return;
            };
            if html {
                bytes = html_page(&title, &bytes, &colors);
            }
            let to = path.clone();
            let written = cx
                .background_executor()
                .spawn(async move { std::fs::write(&to, bytes) })
                .await;
            let _ = this.update(cx, |v, cx| {
                let said = match written {
                    Ok(()) if html => Notice::good(tr!(
                        "Saved as HTML: {path}",
                        path = path.display().to_string()
                    )),
                    Ok(()) => Notice::good(tr!(
                        "Saved as text: {path}",
                        path = path.display().to_string()
                    )),
                    Err(e) => Notice::bad(tr!("Not saved: {e}", e = e.to_string())),
                };
                v.show_notice(said, LOG_NOTICE, cx);
            });
        })
        .detach();
    }

    /// The badges at the pane's top right: logging, and typing in every pane.
    fn badges(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let (log, listen) = (self.log_badge(cx), self.listen_badge(cx));
        let envs = self.env_badges(cx);
        let files = self.file_badges(cx);
        if log.is_none() && listen.is_none() && envs.is_empty() && files.is_empty() {
            return None;
        }
        let top = if self.find.is_some() {
            px(52.)
        } else {
            px(12.)
        };
        Some(
            div()
                .absolute()
                .top(top)
                .right(px(SCROLLBAR_ZONE + 6.))
                .flex()
                .gap(px(6.))
                // Faint over the text they cover; whole under the pointer.
                .opacity(0.45)
                .hover(|s| s.opacity(1.))
                .children(envs)
                .children(files)
                .children(log)
                .children(listen)
                .into_any_element(),
        )
    }

    /// An environment loaded into the shell: its name; a click unloads it.
    fn env_badges(&self, cx: &mut Context<Self>) -> Vec<gpui::Stateful<gpui::Div>> {
        self.env_loaded
            .iter()
            .map(|e| {
                let id = e.id.clone();
                div()
                    .id(SharedString::from(format!("env-badge-{}", e.id)))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(22.))
                    .px(px(8.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(colors().warn_edge))
                    .bg(rgb(colors().warn_bg))
                    .text_xs()
                    .text_color(rgb(colors().warn_text))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if let Err(e) = v.unload_env(Some(&id), cx) {
                            v.show_notice(Notice::bad(e), LOG_NOTICE, cx);
                        }
                    }))
                    .child(div().size(px(6.)).rounded_full().bg(rgb(colors().amber)))
                    .child(e.name.clone())
                    .child("✕")
            })
            .collect()
    }

    /// A config file placed until the tab closes: its name; Keep leaves it
    /// on the host, ✕ takes it away.
    fn file_badges(&self, cx: &mut Context<Self>) -> Vec<gpui::Stateful<gpui::Div>> {
        self.placed
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let (keep, remove) = (p.path.clone(), p.path.clone());
                let button = |id: &'static str| {
                    div()
                        .id((id, i))
                        .debug_selector(move || format!("{id}-{i}"))
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .text_color(rgb(colors().soft))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
                };
                div()
                    .id(SharedString::from(format!("file-badge-{}", p.path)))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(22.))
                    .pl(px(8.))
                    .pr(px(2.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .bg(rgb(colors().chip))
                    .text_xs()
                    .text_color(rgb(colors().text))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().size(px(6.)).rounded_full().bg(rgb(colors().accent)))
                    .child(div().pr(px(2.)).child(p.name.clone()))
                    .child(
                        button("file-keep")
                            .px(px(6.))
                            .on_click(
                                cx.listener(move |v, _, _, cx| v.keep_placed(Some(&keep), cx)),
                            )
                            .child(tr!("Keep")),
                    )
                    .child(
                        button("file-remove")
                            .w(px(18.))
                            .on_click(
                                cx.listener(move |v, _, _, cx| v.remove_placed(Some(&remove), cx)),
                            )
                            .child("✕"),
                    )
            })
            .collect()
    }

    /// While the pane is logged; a click opens the log.
    fn log_badge(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let path = self.session.log_path()?.to_path_buf();
        Some(
            div()
                .id("log-badge")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(8.))
                .rounded_full()
                .border_1()
                .border_color(rgb(colors().bad_edge))
                .bg(rgb(colors().bad_bg))
                .text_xs()
                .text_color(rgb(colors().bad_text))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.open_url(&format!("file://{}", path.display()))
                }))
                .child(div().size(px(6.)).rounded_full().bg(rgb(colors().danger)))
                .child(tr!("logging")),
        )
    }

    /// The badge on a pane of a tab that types in every pane.
    fn listen_badge(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        let listening = self.broadcast?;
        Some(
            div()
                .id("listen-badge")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(22.))
                .px(px(8.))
                .rounded_full()
                .border_1()
                .border_color(rgb(if listening {
                    colors().warn_edge
                } else {
                    colors().field_edge
                }))
                .bg(rgb(if listening {
                    colors().warn_bg
                } else {
                    colors().chip
                }))
                .text_xs()
                .text_color(rgb(if listening {
                    colors().warn_text
                } else {
                    colors().muted
                }))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ListenToggled)))
                .child(div().size(px(6.)).rounded_full().bg(rgb(if listening {
                    colors().amber
                } else {
                    colors().faint
                })))
                .child(if listening {
                    tr!("listening")
                } else {
                    tr!("left out")
                }),
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
            return self.show_notice(Notice::warn(tr!("Not connected.")), CONFIRM_PASTE, cx);
        };
        self.show_notice(
            tr!("Installing {key} on {host}…", key = &key.name, host = &host),
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
                    Err(_) => Err(tr!("the connection ended")),
                };
                let said = match &done {
                    Ok(shellwing_ssh::keys::Installed::Added) => Notice::good(tr!(
                        "{key} is installed on {host}.",
                        key = &key.name,
                        host = &host
                    )),
                    Ok(shellwing_ssh::keys::Installed::Present) => {
                        tr!("{host} has {key} already.", host = &host, key = &key.name).into()
                    }
                    Err(why) => Notice::bad(tr!(
                        "{key} was not installed on {host}: {why}",
                        key = &key.name,
                        host = &host,
                        why = why
                    )),
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
                .debug_selector(|| id.into())
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| {
                    d.bg(rgb(colors().accent))
                        .text_color(rgb(colors().on_accent))
                })
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(colors().edge))
                        .hover(|s| s.bg(rgb(colors().hover)))
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
                .border_color(rgb(colors().edge))
                .bg(gpui::rgba(colors().banner))
                .text_sm()
                .text_color(rgb(colors().text))
                .child(div().flex_1().min_w_0().child(tr!(
                    "{key} is installed on {host}. Sign in to {host} with it from now on?",
                    key = &offer.key_name,
                    host = &offer.host
                )))
                .child(
                    button("key-not-now", tr!("Not now"), false).on_click(cx.listener(
                        |v, _, _, cx| {
                            v.key_offer = None;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    button(
                        "key-use",
                        tr!("Use it for {host}", host = &offer.host),
                        true,
                    )
                    .on_click(cx.listener(|v, _, _, cx| {
                        if let Some(o) = v.key_offer.take() {
                            cx.emit(UseKey {
                                host: o.host,
                                vault_id: o.vault_id,
                                key_name: o.key_name,
                            });
                        }
                        cx.notify();
                    })),
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
                let input = crate::input::said(cx, || tr!("Find"));
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
            Some(s) if s.invalid => (tr!("Invalid pattern"), true),
            Some(s) => {
                let more = if s.searching {
                    "…"
                } else if s.capped {
                    "+"
                } else {
                    ""
                };
                match (s.total, s.current) {
                    (0, _) if !s.searching => (tr!("No results"), true),
                    (n, Some(i)) => (
                        tr!(
                            "{current} of {total}{more}",
                            current = i + 1,
                            total = n,
                            more = more
                        ),
                        false,
                    ),
                    (n, None) => (tr!("{total} found{more}", total = n, more = more), false),
                }
            }
        };
        let options = cx.try_global::<FindOptions>().copied().unwrap_or_default();
        let toggle = |id: &'static str,
                      label: &'static str,
                      tip: SharedString,
                      on: bool,
                      flip: fn(&mut FindOptions),
                      cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .h(px(22.))
                .min_w(px(24.))
                .px_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(if on { colors().accent } else { colors().chip }))
                .bg(rgb(if on { colors().selected } else { colors().chip }))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(if on { colors().text } else { colors().muted }))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)))
                .tooltip(move |_, cx| {
                    cx.new(|_| crate::sidebar::Tip {
                        title: tip.clone(),
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
                        .w(px(200.))
                        .h(px(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().field))
                        .child(f.input.clone()),
                )
                .child(toggle(
                    "find-case",
                    "Aa",
                    tr!("Match case (Alt+C)").into(),
                    options.match_case,
                    |o| o.match_case = !o.match_case,
                    cx,
                ))
                .child(toggle(
                    "find-regex",
                    ".*",
                    tr!("Regular expression (Alt+R)").into(),
                    options.regex,
                    |o| o.regex = !o.regex,
                    cx,
                ))
                .child(
                    div()
                        .min_w(px(64.))
                        .px_1()
                        .text_xs()
                        .text_color(rgb(if none { colors().bad } else { colors().muted }))
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
                        .text_color(rgb(colors().soft))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
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
                Notice::warn(tr!("The Files panel browses a server over SFTP; this pane is a shell on this computer.")),
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

    /// The bar offering to save a password or passphrase that worked: Save
    /// puts it in the vault, Not now drops it.
    pub fn answer_save_offer(&mut self, save: bool, cx: &mut Context<Self>) {
        if !save {
            self.save_offer = None;
            cx.notify();
            return;
        }
        if let Some(secret) = self.save_offer.take() {
            match self.session.save(&secret) {
                Ok(()) => {
                    self.show_notice(Notice::good(tr!("Saved in the vault.")), CONFIRM_PASTE, cx);
                    cx.emit(VaultChanged);
                }
                Err(e) => self.show_notice(
                    Notice::bad(tr!("Not saved: {e}", e = e.to_string())),
                    CONFIRM_PASTE,
                    cx,
                ),
            }
        }
        cx.notify();
    }

    /// Whether the bar offering to save a secret shows: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn save_offered(&self) -> bool {
        self.save_offer.is_some()
    }

    /// Whether the right-click menu is open: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The notice over the pane, if one shows: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn notice_shown(&self) -> Option<Notice> {
        self.notice.clone()
    }

    /// The Files panel, once opened: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn files_panel(&self) -> Option<Entity<FilesPanel>> {
        self.files.clone()
    }

    pub fn files_shown(&self) -> bool {
        self.files_shown
    }

    fn status_changed(&mut self, status: Status, cx: &mut Context<Self>) {
        // A shell that ended took its variables with it.
        if status != Status::Connected {
            self.env_pending.clear();
            self.env_loaded.clear();
            // Placed files stay on the host: back with the connection, or
            // taken away when the tab closes (or, failing that, when the
            // host connects next).
        }
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
                tr!(
                    "Save the password for {target} in the vault?",
                    target = target
                )
            }
            shellwing_ssh::Learned::Passphrase { key_path, .. } => {
                tr!(
                    "Save the passphrase of {key_path} in the vault?",
                    key_path = key_path
                )
            }
        };
        let button = |id: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| {
                    d.bg(rgb(colors().accent))
                        .text_color(rgb(colors().on_accent))
                })
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(colors().edge))
                        .hover(|s| s.bg(rgb(colors().hover)))
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
                .border_color(rgb(colors().edge))
                .bg(gpui::rgba(colors().banner))
                .text_sm()
                .text_color(rgb(colors().text))
                .child(div().flex_1().min_w_0().child(what))
                .child(
                    button("save-not-now", tr!("Not now"), false)
                        .on_click(cx.listener(|v, _, _, cx| v.answer_save_offer(false, cx))),
                )
                .child(
                    button("save-secret", tr!("Save"), true)
                        .on_click(cx.listener(|v, _, _, cx| v.answer_save_offer(true, cx))),
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
        let button = |id: &'static str, label: String, danger: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .whitespace_nowrap()
                .when(danger, |d| {
                    d.border_1()
                        .border_color(rgb(colors().warn_edge))
                        .text_color(rgb(colors().warn_text))
                        .hover(|s| s.bg(rgb(colors().warn_hover)))
                })
                .when(!danger, |d| {
                    d.border_1()
                        .border_color(rgb(colors().edge))
                        .hover(|s| s.bg(rgb(colors().hover)))
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
                        .text_color(rgb(colors().muted))
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
                .border_color(rgb(colors().bad_edge))
                .bg(gpui::rgba(colors().bad_banner))
                .text_sm()
                .text_color(rgb(colors().text))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().bad_text))
                        .child(tr!("The host key for {host} has changed.", host = &c.label)),
                )
                .child(div().text_color(rgb(colors().soft)).child(tr!(
                    "If the server was reinstalled or its key replaced, trust the new key. If you do not know why it changed, do not: someone may be between you and the server, reading what you send."
                )))
                .child(fact(
                    &tr!("Known"),
                    tr!("{old}  (known_hosts line {line})", old = &old, line = c.line),
                ))
                .child(fact(&tr!("Now"), format!("{}  {}", c.fingerprint, c.kind)))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("key-changed-trust", tr!("Trust the new key and connect"), true)
                                .on_click(cx.listener(|v, _, _, cx| match v.session.trust_changed_key() {
                                    Ok(()) => v.reconnect_now(cx),
                                    Err(e) => v.show_notice(Notice::bad(tr!("Not trusted: {e}", e = e.to_string())), KEY_NOTICE, cx),
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
            (Status::Lost, Some(due)) => tr!(
                "Connection lost. Trying again in {seconds} s (attempt {attempt} of {attempts}).",
                seconds = due.saturating_duration_since(Instant::now()).as_secs() + 1,
                attempt = self.retries,
                attempts = BACKOFF_SECS.len()
            ),
            (Status::Lost, None) if self.retries >= BACKOFF_SECS.len() => {
                tr_n!(
                    self.retries,
                    "Gave up after {n} attempt. Enter tries again.",
                    "Gave up after {n} attempts. Enter tries again."
                )
            }
            (Status::Lost, None) => tr!("Connection lost. Enter tries again."),
            (Status::Closed, _) if self.session.is_local() => {
                tr!("{why}. Enter starts it again.", why = capitalize(&why))
            }
            (Status::Closed, _) => tr!("{why}. Enter reconnects.", why = capitalize(&why)),
            (Status::Failed, _) if self.session.ever_connected() || !why.is_empty() => {
                tr!("Could not connect. Enter tries again.")
            }
            _ => return None,
        };
        let button = |id: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .when(primary, |d| {
                    d.bg(rgb(colors().accent))
                        .text_color(rgb(colors().on_accent))
                })
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(rgb(colors().edge))
                        .hover(|s| s.bg(rgb(colors().hover)))
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
                .border_color(rgb(colors().edge))
                .bg(gpui::rgba(colors().banner))
                .text_sm()
                .text_color(rgb(colors().text))
                .child(div().flex_1().min_w_0().child(text))
                .when(self.retry_at.is_some(), |d| {
                    d.child(
                        button("stop-retry", tr!("Stop (Esc)"), false)
                            .on_click(cx.listener(|v, _, _, cx| v.stop_retrying(cx))),
                    )
                })
                .child(
                    button(
                        "reconnect",
                        if self.session.is_local() {
                            tr!("Start again (Enter)")
                        } else {
                            tr!("Reconnect (Enter)")
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

    /// A cell's width and height as drawn: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn cell_size(&self) -> (Pixels, Pixels) {
        self.cell
    }

    /// The newest frame, taken as painting takes it: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn latest_frame(&mut self) -> Arc<Frame> {
        if let Some(frame) = self.term.take_frame() {
            self.frame = frame;
        }
        self.frame.clone()
    }

    /// The screen as text, one line per row, trailing spaces cut: for the UI
    /// tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn screen_text(&mut self) -> String {
        self.latest_frame()
            .rows
            .iter()
            .map(|r| r.text.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
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
        let size = self.font_size().to_string();
        self.show_notice(tr!("Font size {size}", size = size), ZOOM_NOTICE, cx);
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
                    Notice::warn(tr!("The pasted text could run commands. Paste again within 5 s to send it anyway.")),
                    CONFIRM_PASTE,
                    cx,
                );
            }
            TermEvent::Title(t) => {
                let t = t.trim();
                let title = (!t.is_empty()).then(|| SharedString::from(t.to_string()));
                if title != self.shell_title {
                    self.shell_title = title;
                    cx.emit(TitleChanged);
                }
                cx.notify();
            }
            TermEvent::Signal(s) => self.env_signal(&s, cx),
            TermEvent::LogFailed(e) => {
                self.session.stop_log();
                self.show_notice(
                    Notice::bad(tr!("Logging stopped: {e}", e = e.to_string())),
                    LOG_NOTICE,
                    cx,
                );
            }
        }
    }

    pub fn show_notice(&mut self, msg: impl Into<Notice>, shown: Duration, cx: &mut Context<Self>) {
        let msg: Notice = msg.into();
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
        let keys = crate::hotkeys::keymap(cx);
        let hint = |h: shellwing_config::Hotkey| keys.hint(h);
        let item = |id: &'static str, label: &str, hint: String, enabled: bool| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_3()
                .py(px(5.))
                .rounded(px(4.))
                .text_color(rgb(if enabled {
                    colors().text
                } else {
                    colors().faint
                }))
                .whitespace_nowrap()
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(rgb(colors().menu_hover)))
                })
                .child(label.to_string())
                .child(div().text_xs().text_color(rgb(colors().muted)).child(hint))
        };
        // Runs the item's command and closes the menu.
        let act = |f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            cx.listener(move |v: &mut Self, _: &gpui::ClickEvent, window, cx| {
                v.menu = None;
                f(v, window, cx);
                cx.notify();
            })
        };
        let line = || div().my_1().h(px(1.)).bg(rgb(colors().line));
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
            .bg(rgb(colors().raised))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_sm()
            // A press on it is its own: the terminal under it would take it as
            // a click, which ends the selection before Copy copies it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Another click anywhere closes it (a right click opens the next one).
            .on_mouse_down_out(cx.listener(|v, e: &MouseDownEvent, _, cx| {
                if e.button != MouseButton::Right {
                    v.menu = None;
                    cx.notify();
                }
            }))
            .child(
                item(
                    "menu-palette",
                    &tr!("Command Palette…"),
                    hint(shellwing_config::Hotkey::Palette),
                    true,
                )
                .on_click(act(|_, _, cx| cx.emit(PaneCommand::Palette))),
            )
            .child(
                item("menu-snippets", &tr!("Snippets…"), String::new(), true)
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::Snippets))),
            )
            .when(status == Status::Connected, |d| {
                d.child(
                    item(
                        "menu-load-env",
                        &tr!("Load Environment…"),
                        String::new(),
                        true,
                    )
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::Environments))),
                )
            })
            .when(status == Status::Connected && !local, |d| {
                d.child(
                    item(
                        "menu-place-file",
                        &tr!("Place Config File…"),
                        String::new(),
                        true,
                    )
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::ConfigFiles))),
                )
                .child(
                    item(
                        "menu-save-file",
                        &tr!("Keep a File in the Vault…"),
                        String::new(),
                        true,
                    )
                    .on_click(act(|_, _, cx| cx.emit(PaneCommand::SaveFile))),
                )
            })
            .when(!self.placed.is_empty(), |d| {
                let names = self
                    .placed
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                d.child(
                    item(
                        "menu-keep-files",
                        &tr!("Keep {names} on the host", names = names.as_str()),
                        String::new(),
                        true,
                    )
                    .on_click(act(|v, _, cx| v.keep_placed(None, cx))),
                )
                .child(
                    item(
                        "menu-remove-files",
                        &tr!("Remove {names}", names = names.as_str()),
                        String::new(),
                        true,
                    )
                    .on_click(act(|v, _, cx| v.remove_placed(None, cx))),
                )
            })
            .when(!self.env_loaded.is_empty(), |d| {
                let names: Vec<String> = self.env_loaded.iter().map(|e| e.name.clone()).collect();
                d.child(
                    item(
                        "menu-unload-env",
                        &tr!("Unload {names}", names = names.join(", ")),
                        String::new(),
                        true,
                    )
                    .on_click(act(|v, _, cx| {
                        if let Err(e) = v.unload_env(None, cx) {
                            v.show_notice(Notice::bad(e), LOG_NOTICE, cx);
                        }
                    })),
                )
            })
            .child(line())
            .child(
                item("menu-copy", &tr!("Copy"), chord("C"), self.frame.selected)
                    .when(self.frame.selected, |d| {
                        d.on_click(act(|v, _, _| v.term.copy()))
                    }),
            )
            .child(
                item("menu-paste", &tr!("Paste"), chord("V"), true)
                    .on_click(act(|v, _, cx| v.paste_clipboard(cx))),
            )
            .child(line())
            .child(
                item(
                    "menu-find",
                    &tr!("Find…"),
                    hint(shellwing_config::Hotkey::Find),
                    true,
                )
                .on_click(act(|v, window, cx| v.open_find(window, cx))),
            )
            // Files and forwards are SSH's.
            .when(!local, |d| {
                d.child(
                    item(
                        "menu-files",
                        &if self.files_shown {
                            tr!("Hide Files")
                        } else {
                            tr!("Browse Files (SFTP)")
                        },
                        hint(shellwing_config::Hotkey::Files),
                        true,
                    )
                    .on_click(act(|v, window, cx| v.toggle_files(window, cx))),
                )
            })
            .when(status == Status::Connected && !local, |d| {
                d.child(
                    item("menu-forward", &tr!("Forward a Port…"), String::new(), true)
                        .on_click(act(|_, _, cx| cx.emit(PaneCommand::Forward))),
                )
            })
            .child(line())
            // What the pane shows, kept: now (with its history), or from now on.
            .child(
                item(
                    "menu-save-text",
                    &tr!("Save Output as Text…"),
                    String::new(),
                    true,
                )
                .on_click(act(|v, window, cx| v.save_output(false, window, cx))),
            )
            .child(
                item(
                    "menu-save-html",
                    &tr!("Save Output as HTML…"),
                    tr!("with colors"),
                    true,
                )
                .on_click(act(|v, window, cx| v.save_output(true, window, cx))),
            )
            .child(
                item(
                    "menu-log",
                    &if self.session.log_path().is_some() {
                        tr!("Stop Logging")
                    } else {
                        tr!("Start Logging")
                    },
                    String::new(),
                    true,
                )
                .on_click(act(|_, _, cx| cx.emit(PaneCommand::Log))),
            )
            .child(line())
            .child(
                item(
                    "menu-split-right",
                    &tr!("Split Right"),
                    hint(shellwing_config::Hotkey::SplitRight),
                    true,
                )
                .on_click(act(|_, _, cx| {
                    cx.emit(PaneCommand::Split(gpui::Axis::Horizontal))
                })),
            )
            .child(
                item(
                    "menu-split-down",
                    &tr!("Split Down"),
                    hint(shellwing_config::Hotkey::SplitDown),
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
                        item("menu-reconnect", &tr!("Reconnect"), String::new(), true)
                            .on_click(act(|v, _, cx| v.reconnect_now(cx))),
                    )
                },
            )
            .child(line())
            .child(
                item(
                    "menu-close",
                    &tr!("Close Pane"),
                    hint(shellwing_config::Hotkey::ClosePane),
                    true,
                )
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
            // Measured, never drawn.
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
                        (_, Found::Match | Found::Current) if !r.selected => {
                            rgb(colors().find_text).into()
                        }
                        _ => hsla(r.fg),
                    },
                    background_color: match r.found {
                        _ if r.selected => Some(selection_bg),
                        Found::Current => Some(rgb(colors().find_current).into()),
                        Found::Match => Some(rgb(colors().find_match).into()),
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

    /// A window position relative to the grid's top-left corner, in the
    /// engine's pixels: its cells are whole pixels (`fit_grid`), and the
    /// cells drawn are as wide as the font's (8.4 px, say). Without the
    /// scaling, a click lands a column further right every few dozen.
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
        // With the Files panel open, the panel has the right corner.
        let corners = self.corners.get();
        let term_corners = if self.files.is_some() && self.files_shown {
            corners.left()
        } else {
            corners
        };

        let mut terminal = term_corners
            .round(div())
            .id("terminal")
            .debug_selector(|| "terminal".into())
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
                            let round = Corners {
                                bottom_left: term_corners.left,
                                bottom_right: term_corners.right,
                                ..Corners::default()
                            };
                            let _ = window.paint_image(full, full, round, image, 0, false);
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
                .bg(rgb(colors().button))
                .text_color(rgb(colors().text))
        };
        if let Some(n) = &self.notice {
            // Below the find bar, when that is open.
            let top = if self.find.is_some() {
                px(52.)
            } else {
                px(12.)
            };
            let n = colored(badge().top(top), n.tone).child(n.text.clone());
            terminal = terminal.child(n);
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
                    term_corners
                        .round(div())
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(rgb(colors().amber)),
                )
            }
            Some(false) => {
                terminal = terminal.child(
                    term_corners
                        .round(div())
                        .absolute()
                        .inset_0()
                        .bg(gpui::rgba(colors().dim_pane)),
                )
            }
            None => {}
        }
        if let Some(b) = self.badges(cx) {
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
                    .hover(|s| s.bg(rgb(colors().button_hover)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.term.scroll_to_bottom();
                        cx.notify();
                    }))
                    .child(tr!("↓ Back to bottom (Shift+End)")),
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
                // Its ground is drawn here, where the window's corner can be
                // rounded: a cached view is laid out by its parent, not painted.
                d.child(
                    corners
                        .right()
                        .round(
                            div()
                                .w(px(width))
                                .h_full()
                                .flex_shrink_0()
                                .bg(rgb(colors().panel)),
                        )
                        .child(f.cached(gpui::StyleRefinement::default().size_full())),
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
                                .bg(rgb(if dragging {
                                    colors().accent
                                } else {
                                    colors().line
                                }))
                                .group_hover("files-edge", |s| {
                                    s.w(px(2.)).bg(rgb(colors().accent))
                                }),
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
                            .bg(rgb(colors().accent))
                            .text_color(rgb(colors().on_accent))
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

/// See [`TermView::title_detail`].
fn shell_detail(shell: &str, host: &str) -> Option<String> {
    let shell = shell.trim();
    let rest = match shell.split_once(':') {
        Some((who, rest)) if who.contains('@') && !who.contains(char::is_whitespace) => rest.trim(),
        _ => shell,
    };
    (!rest.is_empty() && rest != host).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_leaves_out_what_names_the_host() {
        assert_eq!(
            shell_detail("deploy@api-1: ~/apps/billing", "api-1").as_deref(),
            Some("~/apps/billing")
        );
        assert_eq!(shell_detail("root@db:~", "db").as_deref(), Some("~"));
        assert_eq!(
            shell_detail("vim src/a:b.rs", "api-1").as_deref(),
            Some("vim src/a:b.rs")
        );
        assert_eq!(shell_detail("api-1", "api-1"), None);
        assert_eq!(shell_detail("deploy@api-1:", "api-1"), None);
    }

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

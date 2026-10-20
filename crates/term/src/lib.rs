//! Terminal core for shellwing.
//!
//! libghostty-vt's `Terminal` is not `Send`, so it lives on one dedicated thread
//! for its whole life. Everything else talks to it through [`TermHandle`]: bytes
//! from the host go in with [`TermHandle::feed`], user input (keys, text, paste,
//! mouse, wheel) through the other methods, and the UI takes immutable [`Frame`]s
//! out. Results the UI must act on (copied text, a refused paste) come back as
//! [`Event`]s.
//!
//! Frames are pulled, not pushed: after the UI takes one, the thread builds the
//! next only once something changed. While output floods in, parsing continues
//! at full speed and the UI simply receives the latest state each frame.
//!
//! Mouse input is routed here because only the terminal knows whether the
//! running program asked for mouse reporting: if it did, presses, motion and the
//! wheel are encoded for the program; otherwise they select text and scroll the
//! scrollback. Holding Shift always selects, as in other terminals.
//!
//! Finding text ([`TermHandle::find`], see [`find`]) runs here too, over the
//! screen and its history: a little at a time between batches, so a long
//! history never holds up the output. Frames mark the matches in view.

pub mod find;
pub mod links;
pub mod log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use libghostty_vt::render::{CellIterator, Dirty, RenderState, RowIterator};
use libghostty_vt::screen::{CellWide, Screen};
use libghostty_vt::selection::FormatOptions;
use libghostty_vt::selection::gesture::{DragEvent, Geometry, Gesture, PressEvent, ReleaseEvent};
use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::{ClipboardMime, Point, PointCoordinate, ScrollViewport};
use libghostty_vt::{Terminal, key, mouse, paste};

pub use find::FindQuery;
use find::Finder;

/// How many feed chunks may wait for the parser before `feed` blocks. Blocking
/// here is the backpressure that eventually slows the SSH server down.
const FEED_QUEUE: usize = 64;
/// Clicks closer together than this count as a double or triple click.
const MULTI_CLICK: Duration = Duration::from_millis(500);
/// Mouse reporting, bracketed paste, application cursor keys and keypad, the
/// kitty keyboard flags, cursor shape and visibility, and colors back to normal.
/// No soft reset (DECSTR): it moves the saved cursor to the top.
const RESET_MODES: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[?1l\x1b>\x1b[=0;1u\x1b[0 q\x1b[?25h\x1b[0m";
/// Longest stretch of parsing before a frame may be built (about one 120 Hz frame).
const BATCH_BUDGET: Duration = Duration::from_millis(8);
/// How long a search may go on between batches while it looks through history.
const SEARCH_BUDGET: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<RgbColor> for Rgb {
    fn from(c: RgbColor) -> Self {
        Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<Rgb> for RgbColor {
    fn from(c: Rgb) -> Self {
        RgbColor {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

/// The theme's colors, the defaults programs see. A program can still override
/// them (OSC 4/10/11/12); its overrides stay in place when the theme changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// ANSI colors 0–15.
    pub palette: [Rgb; 16],
}

/// A stretch of one row whose cells share a style. `len` is in bytes of `Row::text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub len: usize,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Inside the current selection; the UI chooses how to show it.
    pub selected: bool,
    /// Part of a match of the text being found.
    pub found: Found,
    /// First column of the run.
    pub col: u16,
    /// Columns covered: the number of cells, or 2 for a wide glyph.
    pub cells: u16,
    /// A single wide glyph (CJK, most emoji) that spans two cells.
    pub wide: bool,
}

/// How a cell stands to the text being found (see [`TermHandle::find`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Found {
    #[default]
    No,
    /// In a match.
    Match,
    /// In the match chosen, the one Enter moved to.
    Current,
}

/// How a search is getting on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindStatus {
    /// Matches found so far, on the screen shown (the alternate one for vim).
    pub total: usize,
    /// The chosen match, counting from the newest (the lowest on screen) as 0.
    pub current: Option<usize>,
    /// Still looking through the history.
    pub searching: bool,
    /// [`find::MAX_MATCHES`] found: the rest of the history is not looked at.
    pub capped: bool,
    /// The query is not a regular expression.
    pub invalid: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// One grapheme (or a space) per cell; wide glyphs occupy one entry.
    pub text: String,
    pub runs: Vec<Run>,
}

/// Where the viewport sits in the history, in rows: `offset` is the first row
/// shown, out of `total`, `len` at a time. On the alternate screen (vim, htop)
/// there is no history, so `total == len`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollPos {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub cols: u16,
    pub rows: Vec<Arc<Row>>,
    pub background: Rgb,
    pub foreground: Rgb,
    /// Cursor position in the viewport, when visible (hidden while scrolled back).
    pub cursor: Option<(u16, u16)>,
    /// The cursor color: a program's override, else the default from [`Colors`].
    pub cursor_color: Option<Rgb>,
    /// True while the viewport shows scrollback instead of the live screen.
    pub scrolled_back: bool,
    pub scroll: ScrollPos,
    /// The link under the pointer while the link modifier is held.
    pub hover_link: Option<LinkSpan>,
    /// The search, while text is being found.
    pub find: Option<FindStatus>,
    /// The program asked for mouse reports (Shift still selects).
    pub mouse_tracking: bool,
    /// A full-screen program (vim, htop, less) is on the alternate screen:
    /// what is typed goes to it, not to the shell.
    pub alternate: bool,
    /// Some text is selected, to copy.
    pub selected: bool,
    /// Increases with every frame built.
    pub seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    /// A letter `a`..=`z`, used with Ctrl or Alt.
    Letter(char),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Super / Command.
    pub logo: bool,
}

impl Mods {
    fn ghostty(self) -> key::Mods {
        let mut m = key::Mods::empty();
        if self.shift {
            m |= key::Mods::SHIFT;
        }
        if self.ctrl {
            m |= key::Mods::CTRL;
        }
        if self.alt {
            m |= key::Mods::ALT;
        }
        if self.logo {
            m |= key::Mods::SUPER;
        }
        m
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Columns `start..end` of viewport row `row`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkSpan {
    pub row: u16,
    pub start: u16,
    pub end: u16,
}

/// A mouse event in pixels relative to the top-left of the terminal grid.
#[derive(Clone, Copy, Debug)]
pub struct MouseInput {
    pub action: MouseAction,
    /// The button pressed or released; for motion, the button held, if any.
    pub button: Option<MouseButton>,
    pub x: f32,
    pub y: f32,
    pub mods: Mods,
    /// The platform's link modifier is held (Ctrl, or Cmd on macOS): a left
    /// press on a link opens it instead of selecting.
    pub link: bool,
}

/// Something the UI has to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Text to put on the clipboard.
    Copied(String),
    /// A paste refused because it could run commands (e.g. it contains a
    /// newline and the program did not enable bracketed paste). Paste it again
    /// with `allow_unsafe` once the user confirms.
    PasteRefused(String),
    /// A link the user clicked; already filtered by [`links::openable`].
    OpenLink(String),
    /// The log could not be written (the disk is full, the folder went):
    /// logging stopped.
    LogFailed(String),
    /// The program set the terminal's title (OSC 0 or 2), such as the shell's
    /// `user@host: ~/dir`; empty when it cleared it.
    Title(String),
    /// What a command shellwing typed printed to say where it stands
    /// (`OSC 777 ; shellwing ; <payload>`): see [`signal_sequence`].
    Signal(String),
}

/// What a command prints to signal shellwing, for a payload such as
/// `ready-1f3a`: printed with `printf`, it never shows.
/// Pasted text with every line end as a newline. A carriage return runs a
/// line as surely as a newline (Enter sends one), but libghostty's check for
/// a paste that could run commands looks for newlines only: lines ended by a
/// bare `\r`, as a web page can hide in text copied from it, would run with
/// no question. Windows' `\r\n` becomes one newline.
fn line_ends_as_newlines(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\r') {
        return text.into();
    }
    text.replace("\r\n", "\n").replace('\r', "\n").into()
}

pub fn signal_sequence(payload: &str) -> String {
    format!("\\033]777;shellwing;{payload}\\007")
}

/// Finds shellwing's signals in what the program sends, a sequence split
/// between two reads too.
#[derive(Default)]
struct Signals {
    /// The end of the last read, where a signal may have begun.
    tail: Vec<u8>,
}

const SIGNAL: &[u8] = b"\x1b]777;shellwing;";

impl Signals {
    fn scan(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut seen = std::mem::take(&mut self.tail);
        seen.extend_from_slice(bytes);
        let mut found = vec![];
        let mut at = 0;
        while let Some(i) = find(&seen[at..], SIGNAL) {
            let start = at + i + SIGNAL.len();
            let end = seen[start..]
                .iter()
                .position(|&b| b == 0x07 || b == 0x1b)
                .map(|e| start + e);
            match end {
                Some(e) => {
                    found.push(String::from_utf8_lossy(&seen[start..e]).into_owned());
                    at = e + 1;
                }
                None if seen.len() - start < 128 => {
                    // Its end is in the next read.
                    self.tail = seen[at + i..].to_vec();
                    return found;
                }
                None => at = start,
            }
        }
        // A signal's first bytes may end this read.
        let keep = (1..SIGNAL.len())
            .rev()
            .find(|&n| seen.len() >= n && seen.ends_with(&SIGNAL[..n]))
            .unwrap_or(0);
        self.tail = seen[seen.len() - keep..].to_vec();
        found
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

enum Cmd {
    Feed(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
        cell_w: u32,
        cell_h: u32,
    },
    Key(NamedKey, Mods),
    Text(String),
    Paste {
        text: String,
        allow_unsafe: bool,
    },
    Copy,
    Mouse(MouseInput),
    /// Wheel movement in lines; negative scrolls up into history.
    Wheel {
        lines: f32,
        x: f32,
        y: f32,
        mods: Mods,
    },
    /// Scroll the viewport by whole pages; negative goes up.
    ScrollPages(i32),
    ScrollToBottom,
    /// Scroll so that history row `n` is the first shown.
    ScrollTo(usize),
    /// Show the link under the pointer, or none.
    Hover(Option<(f32, f32)>),
    Colors(Box<Colors>),
    Scrollback(usize),
    /// Find this; empty text stops finding.
    Find(FindQuery),
    /// Choose the next match: older (up the screen and into history), or newer.
    FindNext {
        older: bool,
    },
    /// Log what comes from now on, or stop logging.
    Log(Option<Box<log::Log>>),
    /// All the text, history included, as plain text or HTML.
    Format {
        html: bool,
        reply: FormatFn,
    },
    /// Stop the thread, dropping the terminal and the write callback.
    Close,
    ResetModes,
    /// Wakes the thread after the UI took a frame (see [`Shared::taken`]).
    Wake,
}

/// State shared between the UI and the terminal thread.
struct Shared {
    latest: Mutex<Option<Arc<Frame>>>,
    /// The UI took the last frame, so the next change may build a new one. A flag
    /// rather than a queued message: when output floods in, the queue is full and a
    /// message could be dropped, which would stop frames for good.
    taken: AtomicBool,
}

/// Bytes the terminal wants written back to the host: key encodings, typed and
/// pasted text, mouse reports, and replies to queries such as device attributes.
pub type WriteFn = Box<dyn Fn(Vec<u8>) + Send>;
/// Called (from the terminal thread) when a new frame is ready.
pub type WakeFn = Box<dyn Fn() + Send>;
/// Called (from the terminal thread) with events for the UI.
pub type EventFn = Box<dyn Fn(Event) + Send>;
/// Called (from the terminal thread) with the text asked for.
pub type FormatFn = Box<dyn FnOnce(Vec<u8>) + Send>;

#[derive(Clone)]
pub struct TermHandle {
    tx: SyncSender<Cmd>,
    shared: Arc<Shared>,
}

impl TermHandle {
    /// Starts the terminal thread.
    pub fn spawn(
        cols: u16,
        rows: u16,
        scrollback: usize,
        write: WriteFn,
        wake: WakeFn,
        events: EventFn,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(FEED_QUEUE);
        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            taken: AtomicBool::new(true),
        });
        let out = shared.clone();
        thread::Builder::new()
            .name("shellwing-term".into())
            .spawn(move || {
                if let Err(e) = run(cols, rows, scrollback, rx, out, write, wake, events) {
                    eprintln!("terminal thread stopped: {e}");
                }
            })?;
        Ok(TermHandle { tx, shared })
    }

    /// Queues host output for parsing. Blocks while the parser is `FEED_QUEUE` chunks behind.
    pub fn feed(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Feed(bytes));
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        let _ = self.tx.send(Cmd::Resize {
            cols,
            rows,
            cell_w,
            cell_h,
        });
    }

    /// Encodes a non-text key with the terminal's current modes (application
    /// cursor keys, kitty keyboard protocol, ...) and writes it to the host.
    pub fn key(&self, key: NamedKey, mods: Mods) {
        let _ = self.tx.send(Cmd::Key(key, mods));
    }

    /// Typed (or IME-committed) text.
    pub fn text(&self, text: String) {
        let _ = self.tx.send(Cmd::Text(text));
    }

    /// Pastes text, bracketed if the program enabled bracketed paste.
    pub fn paste(&self, text: String, allow_unsafe: bool) {
        let _ = self.tx.send(Cmd::Paste { text, allow_unsafe });
    }

    /// Copies the selection; the text arrives as [`Event::Copied`].
    pub fn copy(&self) {
        let _ = self.tx.send(Cmd::Copy);
    }

    /// Writes what the terminal is sent from now on to `log`, in place of
    /// the log before, which ends; None stops logging.
    pub fn log(&self, log: Option<log::Log>) {
        let _ = self.tx.send(Cmd::Log(log.map(Box::new)));
    }

    /// Hands `reply` all the text, the history and the screen, as plain text
    /// or as HTML with its colors.
    pub fn format_all(&self, html: bool, reply: FormatFn) {
        let _ = self.tx.send(Cmd::Format { html, reply });
    }

    pub fn mouse(&self, input: MouseInput) {
        let _ = self.tx.send(Cmd::Mouse(input));
    }

    pub fn wheel(&self, lines: f32, x: f32, y: f32, mods: Mods) {
        let _ = self.tx.send(Cmd::Wheel { lines, x, y, mods });
    }

    pub fn scroll_pages(&self, pages: i32) {
        let _ = self.tx.send(Cmd::ScrollPages(pages));
    }

    /// Shows history from row `row` (see [`ScrollPos`]).
    pub fn scroll_to(&self, row: usize) {
        let _ = self.tx.send(Cmd::ScrollTo(row));
    }

    pub fn scroll_to_bottom(&self) {
        let _ = self.tx.send(Cmd::ScrollToBottom);
    }

    /// Highlights the link at a position (in grid pixels), or clears it with `None`.
    pub fn hover(&self, at: Option<(f32, f32)>) {
        let _ = self.tx.send(Cmd::Hover(at));
    }

    /// Sets the default colors, for example when the theme changes.
    pub fn set_colors(&self, colors: Colors) {
        let _ = self.tx.send(Cmd::Colors(Box::new(colors)));
    }

    /// Changes how many lines of history are kept.
    pub fn set_scrollback(&self, lines: usize) {
        let _ = self.tx.send(Cmd::Scrollback(lines));
    }

    /// Finds `query` on the screen and in its history, and chooses the newest
    /// match as soon as there is one, scrolling to it. Frames then mark the
    /// matches and say how many there are. The same query again keeps the
    /// matches and the choice; empty text stops finding.
    pub fn find(&self, query: FindQuery) {
        let _ = self.tx.send(Cmd::Find(query));
    }

    /// Chooses the next match, `older` or newer, wrapping round at either
    /// end, and scrolls to it when it is out of view.
    pub fn find_next(&self, older: bool) {
        let _ = self.tx.send(Cmd::FindNext { older });
    }

    /// Undoes modes a program may have left on when its connection died, before
    /// a new connection writes below: the alternate screen, mouse reporting,
    /// bracketed paste, application keys, a hidden cursor. Text and the cursor
    /// position stay.
    pub fn reset_modes(&self) {
        let _ = self.tx.send(Cmd::ResetModes);
    }

    /// Ends the terminal thread, for example when its tab closes. The callbacks
    /// given to [`TermHandle::spawn`] are dropped, which breaks any cycle through
    /// them; later calls on any clone of this handle do nothing.
    pub fn close(&self) {
        let _ = self.tx.send(Cmd::Close);
    }

    /// Takes the newest frame, if one was built since the last call.
    pub fn take_frame(&self) -> Option<Arc<Frame>> {
        let frame = self.shared.latest.lock().ok()?.take();
        if frame.is_some() {
            self.shared.taken.store(true, Ordering::Release);
            // Never block the UI thread. If the queue is full the terminal thread is
            // busy and checks `taken` after its current batch anyway.
            let _ = self.tx.try_send(Cmd::Wake);
        }
        frame
    }
}

struct Selecting {
    gesture: Gesture<'static>,
    press: PressEvent<'static>,
    release: ReleaseEvent<'static>,
    drag: DragEvent<'static>,
    /// The left button went down on the grid for a selection (not for the program).
    active: bool,
}

struct Engine {
    term: Terminal<'static, 'static>,
    rs: RenderState<'static>,
    rows_it: RowIterator<'static>,
    cells_it: CellIterator<'static>,
    enc: key::Encoder<'static>,
    kev: key::Event<'static>,
    mouse_enc: mouse::Encoder<'static>,
    mouse_ev: mouse::Event<'static>,
    sel: Selecting,
    started: Instant,
    rows: Vec<Arc<Row>>,
    cols: u16,
    grid_rows: u16,
    cell: (u32, u32),
    /// Fraction of a line left over from smooth (touchpad) scrolling.
    wheel_rest: f32,
    held: Option<MouseButton>,
    hover: Option<LinkSpan>,
    events: EventFn,
    seq: u64,
    /// Rebuild every row: default colors changed, and rows carry resolved colors.
    repaint_all: bool,
    find: Option<Finding>,
    /// The matches each row of the last frame showed, as column ranges.
    found: Vec<Vec<(u16, u16, Found)>>,
}

/// Something being found.
enum Finding {
    Going(Finder),
    /// Not a regular expression: nothing to look for.
    Invalid(FindQuery),
}

impl Finding {
    fn query(&self) -> &FindQuery {
        match self {
            Finding::Going(f) => f.query(),
            Finding::Invalid(q) => q,
        }
    }

    fn status(&self) -> FindStatus {
        match self {
            Finding::Going(f) => f.status(),
            Finding::Invalid(_) => FindStatus {
                invalid: true,
                ..FindStatus::default()
            },
        }
    }
}

/// Keeps `lines` of history. libghostty also caps history at 10 000 bytes
/// by default, about a thousand lines, which would win over any line count:
/// that cap goes.
fn set_scrollback(term: &mut Terminal, lines: usize) -> libghostty_vt::error::Result<()> {
    term.set_scrollback_max_bytes(None)?
        .set_scrollback_max_lines(Some(lines))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run(
    cols: u16,
    rows: u16,
    scrollback: usize,
    rx: Receiver<Cmd>,
    shared: Arc<Shared>,
    write: WriteFn,
    wake: WakeFn,
    events: EventFn,
) -> libghostty_vt::error::Result<()> {
    // Shared by the PTY-reply callback and input handling, both on this thread.
    let write = std::rc::Rc::new(write);
    let mut term = Terminal::new(cols, rows)?;
    set_scrollback(&mut term, scrollback)?;
    {
        let write = write.clone();
        term.on_pty_write(move |_t, data| write(data.to_vec()))?;
    }
    // The new title can be read only once the write that set it is done.
    let titled = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut signals = Signals::default();
    {
        let titled = titled.clone();
        term.on_title_changed(move |_| titled.set(true))?;
    }
    let mut press = PressEvent::new()?;
    press.set_repeat_interval(MULTI_CLICK)?;
    let mut e = Engine {
        term,
        rs: RenderState::new()?,
        rows_it: RowIterator::new()?,
        cells_it: CellIterator::new()?,
        enc: key::Encoder::new()?,
        kev: key::Event::new()?,
        mouse_enc: mouse::Encoder::new()?,
        mouse_ev: mouse::Event::new()?,
        sel: Selecting {
            gesture: Gesture::new()?,
            press,
            release: ReleaseEvent::new()?,
            drag: DragEvent::new()?,
            active: false,
        },
        started: Instant::now(),
        rows: vec![],
        cols,
        grid_rows: rows,
        cell: (8, 16),
        wheel_rest: 0.0,
        held: None,
        hover: None,
        events,
        seq: 0,
        repaint_all: false,
        find: None,
        found: vec![],
    };
    let mut logging = Logging(None);
    let mut changed = true;
    loop {
        // A search still going through history goes on between commands.
        let first = if matches!(&e.find, Some(Finding::Going(f)) if f.searching()) {
            match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };
        let mut next = first;
        let batch_start = Instant::now();
        // Apply what is queued before building a frame, but for at most BATCH_BUDGET:
        // when input arrives faster than it parses, the queue never drains, and the
        // screen must still update.
        while let Some(cmd) = next.take().or_else(|| {
            (batch_start.elapsed() < BATCH_BUDGET)
                .then(|| rx.try_recv().ok())
                .flatten()
        }) {
            match cmd {
                Cmd::Feed(b) => {
                    for signal in signals.scan(&b) {
                        (e.events)(Event::Signal(signal));
                    }
                    logging.write(&b, &e.events);
                    e.term.vt_write(&b);
                    if titled.replace(false)
                        && let Ok(title) = e.term.title()
                    {
                        (e.events)(Event::Title(title.to_string()));
                    }
                }
                Cmd::Resize {
                    cols,
                    rows,
                    cell_w,
                    cell_h,
                } => {
                    if let Some(l) = &mut logging.0 {
                        l.resize(cols);
                    }
                    e.term.resize(cols, rows, cell_w, cell_h)?;
                    (e.cols, e.grid_rows, e.cell) = (cols, rows, (cell_w.max(1), cell_h.max(1)));
                }
                Cmd::Key(k, m) => {
                    let mut out = Vec::new();
                    e.encode_key(k, m, &mut out)?;
                    if !out.is_empty() {
                        e.term.scroll_viewport(ScrollViewport::Bottom);
                        write(out);
                    }
                }
                Cmd::Text(t) => {
                    e.term.scroll_viewport(ScrollViewport::Bottom);
                    write(t.into_bytes());
                }
                Cmd::Paste { text, allow_unsafe } => {
                    let mime = ClipboardMime::new("text/plain");
                    let opts = paste::Options::new().with_allow_unsafe(allow_unsafe);
                    let sent = line_ends_as_newlines(&text);
                    match e
                        .term
                        .paste(opts, &[mime], |_, w| w.write_all(sent.as_bytes()))
                    {
                        Ok(_) => e.term.scroll_viewport(ScrollViewport::Bottom),
                        Err(libghostty_vt::Error::Rejected) => {
                            (e.events)(Event::PasteRefused(text))
                        }
                        Err(err) => return Err(err),
                    }
                }
                Cmd::Copy => {
                    let opts = FormatOptions::new().with_unwrap(true).with_trim(true);
                    if let Some(bytes) = e.term.format_selection_alloc(None, opts)? {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        if !text.is_empty() {
                            (e.events)(Event::Copied(text));
                        }
                    }
                }
                Cmd::Mouse(m) => {
                    let mut out = Vec::new();
                    e.mouse(m, &mut out)?;
                    if !out.is_empty() {
                        write(out);
                    }
                }
                Cmd::Wheel { lines, x, y, mods } => {
                    let mut out = Vec::new();
                    e.wheel(lines, x, y, mods, &mut out)?;
                    if !out.is_empty() {
                        write(out);
                    }
                }
                Cmd::ScrollPages(p) => {
                    let page = e.grid_rows.saturating_sub(1).max(1) as isize;
                    e.term
                        .scroll_viewport(ScrollViewport::Delta(p as isize * page));
                }
                Cmd::ScrollToBottom => e.term.scroll_viewport(ScrollViewport::Bottom),
                Cmd::ScrollTo(row) => e.term.scroll_viewport(ScrollViewport::Row(row)),
                Cmd::Hover(at) => {
                    let span = match at {
                        Some((x, y)) => e.link_at(x, y)?.map(|(span, _)| span),
                        None => None,
                    };
                    if span == e.hover {
                        continue;
                    }
                    e.hover = span;
                }
                Cmd::Colors(c) => {
                    let mut palette = e.term.default_color_palette()?;
                    for (slot, c) in palette.0.iter_mut().zip(c.palette) {
                        *slot = c.into();
                    }
                    e.term
                        .set_default_fg_color(Some(c.foreground.into()))?
                        .set_default_bg_color(Some(c.background.into()))?
                        .set_default_cursor_color(Some(c.cursor.into()))?
                        .set_default_color_palette(Some(palette))?;
                    e.repaint_all = true;
                }
                Cmd::Scrollback(lines) => set_scrollback(&mut e.term, lines)?,
                Cmd::Find(query) => e.set_find(query),
                Cmd::FindNext { older } => {
                    if let Some(Finding::Going(f)) = &mut e.find
                        && f.step(&mut e.term, older).is_err()
                    {
                        e.find_failed();
                    }
                }
                Cmd::Log(log) => {
                    logging.end(&e.events);
                    logging.0 = log.map(|mut l| {
                        l.resize(e.cols);
                        l
                    });
                    continue;
                }
                Cmd::Format { html, reply } => {
                    // Nothing, rather than the thread stopping.
                    reply(e.format_all(html).unwrap_or_default());
                    continue;
                }
                Cmd::Close => return Ok(()),
                Cmd::ResetModes => {
                    // Only leave the alternate screen if a program left it on:
                    // 1049 also restores the cursor saved on entry, which outside
                    // the alternate screen would be wherever (often the top).
                    if matches!(e.term.active_screen(), Ok(Screen::Alternate)) {
                        e.term.vt_write(b"\x1b[?1049l");
                    }
                    e.term.vt_write(RESET_MODES);
                }
                Cmd::Wake => continue,
            }
            changed = true;
        }
        logging.flush(&e.events);
        if e.advance_find(changed) {
            changed = true;
        }
        if changed && shared.taken.swap(false, Ordering::AcqRel) {
            let frame = e.build_frame()?;
            if let Ok(mut slot) = shared.latest.lock() {
                *slot = Some(Arc::new(frame));
            }
            changed = false;
            wake();
        }
    }
    Ok(())
}

/// The log being written, if any. When the thread stops, it ends: its last
/// line is written.
struct Logging(Option<Box<log::Log>>);

impl Logging {
    fn write(&mut self, bytes: &[u8], events: &EventFn) {
        if let Some(l) = &mut self.0
            && let Err(err) = l.write(bytes)
        {
            self.failed(&err, events);
        }
    }

    fn flush(&mut self, events: &EventFn) {
        if let Some(l) = &mut self.0
            && let Err(err) = l.flush()
        {
            self.failed(&err, events);
        }
    }

    fn end(&mut self, events: &EventFn) {
        if let Some(l) = self.0.take()
            && let Err(err) = l.finish()
        {
            self.failed(&err, events);
        }
    }

    fn failed(&mut self, err: &std::io::Error, events: &EventFn) {
        self.0 = None;
        events(Event::LogFailed(err.to_string()));
    }
}

impl Drop for Logging {
    fn drop(&mut self) {
        if let Some(l) = self.0.take() {
            let _ = l.finish();
        }
    }
}

impl Engine {
    /// The text of the history and the screen.
    fn format_all(&mut self, html: bool) -> libghostty_vt::error::Result<Vec<u8>> {
        let Some(all) = self.term.select_all()? else {
            return Ok(Vec::new());
        };
        let format = if html {
            libghostty_vt::fmt::Format::Html
        } else {
            libghostty_vt::fmt::Format::Plain
        };
        let opts = FormatOptions::new()
            .with_emit_format(format)
            .with_unwrap(true)
            .with_trim(true)
            .with_selection(&all);
        Ok(self
            .term
            .format_selection_alloc(None, opts)?
            .map(|b| b.to_vec())
            .unwrap_or_default())
    }

    fn encode_key(
        &mut self,
        k: NamedKey,
        m: Mods,
        out: &mut Vec<u8>,
    ) -> libghostty_vt::error::Result<()> {
        let (gk, unshifted) = match k {
            NamedKey::Enter => (key::Key::Enter, '\0'),
            NamedKey::Backspace => (key::Key::Backspace, '\0'),
            NamedKey::Tab => (key::Key::Tab, '\0'),
            NamedKey::Escape => (key::Key::Escape, '\0'),
            NamedKey::Up => (key::Key::ArrowUp, '\0'),
            NamedKey::Down => (key::Key::ArrowDown, '\0'),
            NamedKey::Left => (key::Key::ArrowLeft, '\0'),
            NamedKey::Right => (key::Key::ArrowRight, '\0'),
            NamedKey::Home => (key::Key::Home, '\0'),
            NamedKey::End => (key::Key::End, '\0'),
            NamedKey::PageUp => (key::Key::PageUp, '\0'),
            NamedKey::PageDown => (key::Key::PageDown, '\0'),
            NamedKey::Delete => (key::Key::Delete, '\0'),
            NamedKey::Letter(c) if c.is_ascii_lowercase() => {
                // Key::A..=Key::Z are contiguous in libghostty's key enum.
                let Ok(gk) = key::Key::try_from(key::Key::A as i32 + (c as u8 - b'a') as i32)
                else {
                    return Ok(());
                };
                (gk, c)
            }
            NamedKey::Letter(_) => return Ok(()),
        };
        self.kev
            .set_action(key::Action::Press)
            .set_key(gk)
            .set_mods(m.ghostty())
            .set_consumed_mods(key::Mods::empty())
            .set_unshifted_codepoint(unshifted)
            .set_utf8(None::<String>);
        self.enc
            .set_options_from_terminal(&self.term)
            .encode_to_vec(&self.kev, out)
    }

    /// The viewport cell under a pixel position, clamped to the grid.
    fn point(&self, x: f32, y: f32) -> Point {
        let cx = (x / self.cell.0 as f32).clamp(0.0, self.cols.saturating_sub(1) as f32) as u16;
        let cy =
            (y / self.cell.1 as f32).clamp(0.0, self.grid_rows.saturating_sub(1) as f32) as u32;
        Point::Viewport(PointCoordinate { x: cx, y: cy })
    }

    fn sync_mouse_encoder(&mut self) {
        let size = mouse::EncoderSize {
            screen_width: self.cols as u32 * self.cell.0,
            screen_height: self.grid_rows as u32 * self.cell.1,
            cell_width: self.cell.0,
            cell_height: self.cell.1,
            padding_top: 0,
            padding_bottom: 0,
            padding_left: 0,
            padding_right: 0,
        };
        self.mouse_enc
            .set_options_from_terminal(&self.term)
            .set_size(size)
            .set_any_button_pressed(self.held.is_some())
            .set_track_last_cell(true);
    }

    /// The OSC 8 URI of a viewport cell, if it has one.
    fn cell_uri(&self, x: u16, y: u32, buf: &mut Vec<u8>) -> Option<String> {
        let gref = self
            .term
            .grid_ref(Point::Viewport(PointCoordinate { x, y }))
            .ok()?;
        buf.resize(256, 0);
        let n = match gref.hyperlink_uri(buf) {
            Ok(n) => n,
            Err(libghostty_vt::Error::OutOfSpace { required }) => {
                buf.resize(required, 0);
                gref.hyperlink_uri(buf).ok()?
            }
            Err(_) => return None,
        };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n]).into_owned())
    }

    /// The link at a position in grid pixels: an OSC 8 hyperlink if the program
    /// set one, otherwise a URL found in the row's text.
    fn link_at(&self, x: f32, y: f32) -> libghostty_vt::error::Result<Option<(LinkSpan, String)>> {
        let Point::Viewport(PointCoordinate { x: cx, y: cy }) = self.point(x, y) else {
            return Ok(None);
        };
        let mut buf = Vec::new();
        if let Some(uri) = self.cell_uri(cx, cy, &mut buf) {
            // Extend over neighbours that carry the same link.
            let (mut start, mut end) = (cx, cx + 1);
            while start > 0
                && self.cell_uri(start - 1, cy, &mut buf).as_deref() == Some(uri.as_str())
            {
                start -= 1;
            }
            while end < self.cols
                && self.cell_uri(end, cy, &mut buf).as_deref() == Some(uri.as_str())
            {
                end += 1;
            }
            return Ok(Some((
                LinkSpan {
                    row: cy as u16,
                    start,
                    end,
                },
                uri,
            )));
        }
        let mut cells = Vec::with_capacity(self.cols as usize);
        let mut g = ['\0'; 8];
        for col in 0..self.cols {
            let gref = self
                .term
                .grid_ref(Point::Viewport(PointCoordinate { x: col, y: cy }))?;
            let n = gref.graphemes(&mut g).unwrap_or(0);
            let wide_tail = matches!(gref.cell().and_then(|c| c.wide()), Ok(CellWide::SpacerTail));
            cells.push(if wide_tail {
                String::new()
            } else if n == 0 {
                " ".into()
            } else {
                g[..n].iter().collect()
            });
        }
        Ok(links::url_at(&cells, cx as usize).map(|(start, end, url)| {
            (
                LinkSpan {
                    row: cy as u16,
                    start: start as u16,
                    end: end as u16,
                },
                url,
            )
        }))
    }

    /// Whether a gesture selects text (true) or goes to the program (false).
    fn selects(&self, mods: Mods) -> libghostty_vt::error::Result<bool> {
        Ok(!self.term.is_mouse_tracking()? || mods.shift)
    }

    fn mouse(&mut self, m: MouseInput, out: &mut Vec<u8>) -> libghostty_vt::error::Result<()> {
        match m.action {
            MouseAction::Press => self.held = m.button,
            MouseAction::Release => self.held = None,
            MouseAction::Motion => {}
        }
        let left = m.button == Some(MouseButton::Left);
        if left
            && m.link
            && m.action == MouseAction::Press
            && let Some((_, url)) = self.link_at(m.x, m.y)?
        {
            if links::openable(&url) {
                (self.events)(Event::OpenLink(url));
            }
            return Ok(());
        }
        // Selection: left button presses and drags, unless the program wants the mouse.
        if left && m.action == MouseAction::Press && self.selects(m.mods)? {
            let grid = self.term.grid_ref(self.point(m.x, m.y))?;
            let distance = self.cell.0 as f64;
            let sel = self
                .sel
                .press
                .set_repeat_distance(distance)?
                .set_time(self.started.elapsed())?
                .set_position(m.x as f64, m.y as f64)?
                .apply(&mut self.sel.gesture, &self.term, grid)?;
            self.term.set_selection(sel.as_ref())?;
            self.sel.active = true;
            return Ok(());
        }
        if self.sel.active {
            match m.action {
                MouseAction::Motion if left => {
                    let grid = self.term.grid_ref(self.point(m.x, m.y))?;
                    let geometry = Geometry {
                        columns: self.cols.into(),
                        cell_width: self.cell.0,
                        padding_left: 0,
                        screen_height: self.grid_rows as u32 * self.cell.1,
                    };
                    let rect = m.mods.alt && (m.mods.ctrl || m.mods.logo);
                    let sel = self
                        .sel
                        .drag
                        .set_rectangle(rect)?
                        .set_position(m.x as f64, m.y as f64)?
                        .apply(&mut self.sel.gesture, &self.term, grid, geometry)?;
                    self.term.set_selection(sel.as_ref())?;
                }
                MouseAction::Release if left => {
                    let grid = self.term.grid_ref(self.point(m.x, m.y)).ok();
                    self.sel
                        .release
                        .apply(&mut self.sel.gesture, &self.term, grid)?;
                    self.sel.active = false;
                }
                _ => {}
            }
            return Ok(());
        }
        if !self.term.is_mouse_tracking()? {
            return Ok(());
        }
        // The program asked for mouse reports.
        self.sync_mouse_encoder();
        let button = m.button.map(|b| match b {
            MouseButton::Left => mouse::Button::Left,
            MouseButton::Middle => mouse::Button::Middle,
            MouseButton::Right => mouse::Button::Right,
        });
        let action = match m.action {
            MouseAction::Press => mouse::Action::Press,
            MouseAction::Release => mouse::Action::Release,
            MouseAction::Motion => mouse::Action::Motion,
        };
        self.mouse_ev
            .set_action(action)
            .set_button(button)
            .set_mods(m.mods.ghostty())
            .set_position(mouse::Position { x: m.x, y: m.y });
        self.mouse_enc.encode_to_vec(&self.mouse_ev, out)
    }

    fn wheel(
        &mut self,
        lines: f32,
        x: f32,
        y: f32,
        mods: Mods,
        out: &mut Vec<u8>,
    ) -> libghostty_vt::error::Result<()> {
        self.wheel_rest += lines;
        let whole = self.wheel_rest.trunc();
        self.wheel_rest -= whole;
        if whole == 0.0 {
            return Ok(());
        }
        if self.term.is_mouse_tracking()? && !mods.shift {
            // Wheel up is button 4, down is button 5; one press/release per line.
            self.sync_mouse_encoder();
            let button = if whole < 0.0 {
                mouse::Button::Four
            } else {
                mouse::Button::Five
            };
            for _ in 0..(whole.abs() as usize).min(10) {
                for action in [mouse::Action::Press, mouse::Action::Release] {
                    self.mouse_ev
                        .set_action(action)
                        .set_button(Some(button))
                        .set_mods(mods.ghostty())
                        .set_position(mouse::Position { x, y });
                    self.mouse_enc.encode_to_vec(&self.mouse_ev, out)?;
                }
            }
        } else {
            self.term
                .scroll_viewport(ScrollViewport::Delta(whole as isize));
        }
        Ok(())
    }

    fn set_find(&mut self, query: FindQuery) {
        if query.text.is_empty() {
            self.find = None;
            return;
        }
        // The same query keeps its matches and the one chosen.
        if self.find.as_ref().is_some_and(|f| *f.query() == query) {
            return;
        }
        self.find = match query.compile() {
            Ok(re) => Finder::new(query, re, &mut self.term)
                .ok()
                .map(Finding::Going),
            Err(_) => Some(Finding::Invalid(query)),
        };
    }

    /// A search that hit a terminal error starts over, so that finding never
    /// stops the terminal; one that cannot start over ends.
    fn find_failed(&mut self) {
        let query = self.find.take().map(|f| f.query().clone());
        if let Some(q) = query
            && let Ok(re) = q.compile()
        {
            self.find = Finder::new(q, re, &mut self.term).ok().map(Finding::Going);
        }
    }

    /// Moves the search along for a while (see [`Finder::advance`]). Says
    /// whether there is news to show.
    fn advance_find(&mut self, changed: bool) -> bool {
        let Some(Finding::Going(f)) = &mut self.find else {
            return false;
        };
        if !changed && !f.searching() {
            return false;
        }
        match f.advance(&mut self.term, changed, SEARCH_BUDGET) {
            Ok(news) => news,
            Err(_) => {
                self.find_failed();
                true
            }
        }
    }

    /// The matches in view, as column ranges for each row.
    fn found_rows(&mut self) -> Vec<Vec<(u16, u16, Found)>> {
        let rows = self.grid_rows as usize;
        let mut out = match &self.find {
            Some(Finding::Going(f)) => f.spans(&self.term).unwrap_or_default(),
            _ => vec![],
        };
        out.resize_with(rows, Vec::new);
        out
    }

    /// Copies the rows libghostty marked dirty into `self.rows` and returns a frame
    /// sharing the unchanged rows with the previous one.
    fn build_frame(&mut self) -> libghostty_vt::error::Result<Frame> {
        // Rows whose matches changed are built again, as dirty ones are.
        let found = self.found_rows();
        self.found.resize_with(found.len(), Vec::new);
        let snap = self.rs.update(&self.term)?;
        let full = std::mem::take(&mut self.repaint_all) || matches!(snap.dirty()?, Dirty::Full);
        let colors = snap.colors()?;
        let (bg, fg): (Rgb, Rgb) = (colors.background.into(), colors.foreground.into());
        let cursor = snap
            .cursor()
            .ok()
            .filter(|c| c.visible)
            .and_then(|c| c.viewport.map(|v| (v.x, v.y)));
        let cursor_color = snap.cursor_color()?.map(Rgb::from);
        let rows = snap.rows()? as usize;
        if self.rows.len() != rows {
            self.rows.resize_with(rows, Default::default);
        }
        let mut row_it = self.rows_it.update(&snap)?;
        let mut y = 0usize;
        let mut buf = String::new();
        while let Some(row) = row_it.next() {
            if y >= rows {
                break;
            }
            let spans = found.get(y).map_or(&[][..], Vec::as_slice);
            if full || row.dirty()? || self.found.get(y).map(Vec::as_slice) != Some(spans) {
                let mut out = Row::default();
                let mut cells = self.cells_it.update(row)?;
                let mut col: u16 = 0;
                while let Some(cell) = cells.next() {
                    let width = cell.raw_cell()?.wide();
                    if matches!(width, Ok(CellWide::SpacerTail)) {
                        // Second half of a wide glyph, already covered by its run.
                        col += 1;
                        continue;
                    }
                    let wide = matches!(width, Ok(CellWide::Wide));
                    buf.clear();
                    if cell.graphemes_len()? == 0 {
                        buf.push(' ');
                    } else {
                        cell.graphemes_utf8(&mut buf)?;
                    }
                    let mut run = Run {
                        len: buf.len(),
                        fg: cell.fg_color()?.map(Rgb::from).unwrap_or(fg),
                        bg: cell.bg_color()?.map(Rgb::from),
                        bold: false,
                        italic: false,
                        underline: false,
                        selected: cell.is_selected()?,
                        found: spans
                            .iter()
                            .find(|(start, end, _)| (*start..*end).contains(&col))
                            .map_or(Found::No, |s| s.2),
                        col,
                        cells: if wide { 2 } else { 1 },
                        wide,
                    };
                    col += 1;
                    if cell.has_styling()? {
                        let st = cell.style()?;
                        run.bold = st.bold;
                        run.italic = st.italic;
                        run.underline =
                            !matches!(st.underline, libghostty_vt::style::Underline::None);
                        if st.inverse {
                            let b = run.bg.unwrap_or(bg);
                            run.bg = Some(run.fg);
                            run.fg = b;
                        }
                    }
                    out.text.push_str(&buf);
                    match out.runs.last_mut() {
                        Some(last)
                            if last.fg == run.fg
                                && last.bg == run.bg
                                && last.bold == run.bold
                                && last.italic == run.italic
                                && last.underline == run.underline
                                && last.selected == run.selected
                                && last.found == run.found
                                && !last.wide
                                && !run.wide =>
                        {
                            last.len += run.len;
                            last.cells += 1;
                        }
                        _ => out.runs.push(run),
                    }
                }
                self.rows[y] = Arc::new(out);
                row.set_dirty(false)?;
            }
            y += 1;
        }
        snap.set_dirty(Dirty::Clean)?;
        self.found = found;
        let scrolled_back = !self.term.viewport_active().unwrap_or(true);
        let scroll = self
            .term
            .scrollbar()
            .map(|s| ScrollPos {
                total: s.total,
                offset: s.offset,
                len: s.len,
            })
            .unwrap_or_default();
        self.seq += 1;
        Ok(Frame {
            cols: self.cols,
            rows: self.rows.clone(),
            background: bg,
            foreground: fg,
            cursor: if scrolled_back { None } else { cursor },
            cursor_color,
            scrolled_back,
            scroll,
            hover_link: self.hover,
            find: self.find.as_ref().map(Finding::status),
            mouse_tracking: self.term.is_mouse_tracking().unwrap_or(false),
            alternate: matches!(self.term.active_screen(), Ok(Screen::Alternate)),
            selected: self.term.selection().is_ok_and(|s| s.is_some()),
            seq: self.seq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_are_found_whole_or_split_between_reads() {
        let mut s = Signals::default();
        assert_eq!(
            s.scan(b"ls\r\n\x1b]777;shellwing;ready-1f\x07$ "),
            vec!["ready-1f".to_string()]
        );
        assert!(s.scan(b"out \x1b]777;shell").is_empty());
        assert!(s.scan(b"wing;done").is_empty());
        assert_eq!(s.scan(b"-1f\x1b\\ rest"), vec!["done-1f".to_string()]);
        assert!(
            s.scan(b"\x1b]2;title\x07 plain").is_empty(),
            "others are not"
        );
        assert_eq!(signal_sequence("x"), "\\033]777;shellwing;x\\007");
    }
}

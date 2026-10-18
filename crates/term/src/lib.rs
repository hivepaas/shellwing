//! Terminal core for shellwing.
//!
//! libghostty-vt's `Terminal` is not `Send`, so it lives on one dedicated thread
//! for its whole life. Everything else talks to it through [`TermHandle`]: bytes
//! from the host go in with [`TermHandle::feed`], key presses with
//! [`TermHandle::key`], and the UI takes immutable [`Frame`]s out.
//!
//! Frames are pulled, not pushed: after the UI takes one, the thread builds the
//! next only once something changed. While output floods in, parsing continues
//! at full speed and the UI simply receives the latest state each frame.

use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use libghostty_vt::render::{CellIterator, Dirty, RenderState, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::RgbColor;
use libghostty_vt::{Terminal, key};

/// How many feed chunks may wait for the parser before `feed` blocks. Blocking
/// here is the backpressure that eventually slows the SSH server down.
const FEED_QUEUE: usize = 64;

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

/// A stretch of one row whose cells share a style. `len` is in bytes of `Row::text`.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub len: usize,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// One grapheme (or a space) per cell; wide glyphs occupy one entry.
    pub text: String,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub cols: u16,
    pub rows: Vec<Arc<Row>>,
    pub background: Rgb,
    pub foreground: Rgb,
    /// Cursor position in the viewport, when visible.
    pub cursor: Option<(u16, u16)>,
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
    /// The UI took the latest frame; build the next one when there are changes.
    FrameTaken,
}

/// Bytes the terminal wants written back to the host: key encodings and replies
/// to queries such as device attributes.
pub type WriteFn = Box<dyn Fn(Vec<u8>) + Send>;
/// Called (from the terminal thread) when a new frame is ready.
pub type WakeFn = Box<dyn Fn() + Send>;

#[derive(Clone)]
pub struct TermHandle {
    tx: SyncSender<Cmd>,
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
}

impl TermHandle {
    /// Starts the terminal thread.
    pub fn spawn(
        cols: u16,
        rows: u16,
        scrollback: usize,
        write: WriteFn,
        wake: WakeFn,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(FEED_QUEUE);
        let latest = Arc::new(Mutex::new(None));
        let out = latest.clone();
        thread::Builder::new()
            .name("shellwing-term".into())
            .spawn(move || {
                if let Err(e) = run(cols, rows, scrollback, rx, out, write, wake) {
                    eprintln!("terminal thread stopped: {e}");
                }
            })?;
        Ok(TermHandle { tx, latest })
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

    /// Takes the newest frame, if one was built since the last call.
    pub fn take_frame(&self) -> Option<Arc<Frame>> {
        let frame = self.latest.lock().ok()?.take();
        if frame.is_some() {
            // Never block the UI thread: if the queue is full the terminal thread is
            // busy anyway and will see changes on its next pass.
            if let Err(TrySendError::Full(_)) = self.tx.try_send(Cmd::FrameTaken) {}
        }
        frame
    }
}

struct Engine {
    term: Terminal<'static, 'static>,
    rs: RenderState<'static>,
    rows_it: RowIterator<'static>,
    cells_it: CellIterator<'static>,
    enc: key::Encoder<'static>,
    kev: key::Event<'static>,
    rows: Vec<Arc<Row>>,
    cols: u16,
    seq: u64,
}

fn run(
    cols: u16,
    rows: u16,
    scrollback: usize,
    rx: Receiver<Cmd>,
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    write: WriteFn,
    wake: WakeFn,
) -> libghostty_vt::error::Result<()> {
    // Shared by the PTY-reply callback and key encoding, both on this thread.
    let write = std::rc::Rc::new(write);
    let mut term = Terminal::new(cols, rows)?;
    term.set_scrollback_max_lines(Some(scrollback))?;
    {
        let write = write.clone();
        term.on_pty_write(move |_t, data| write(data.to_vec()))?;
    }
    let mut e = Engine {
        term,
        rs: RenderState::new()?,
        rows_it: RowIterator::new()?,
        cells_it: CellIterator::new()?,
        enc: key::Encoder::new()?,
        kev: key::Event::new()?,
        rows: vec![],
        cols,
        seq: 0,
    };
    // The UI holds no frame yet, so the first change produces one.
    let mut ui_ready = true;
    let mut changed = true;
    while let Ok(first) = rx.recv() {
        let mut next = Some(first);
        // Apply everything already queued before building a frame.
        while let Some(cmd) = next.take().or_else(|| rx.try_recv().ok()) {
            match cmd {
                Cmd::Feed(b) => {
                    e.term.vt_write(&b);
                    changed = true;
                }
                Cmd::Resize {
                    cols,
                    rows,
                    cell_w,
                    cell_h,
                } => {
                    e.term.resize(cols, rows, cell_w, cell_h)?;
                    e.cols = cols;
                    changed = true;
                }
                Cmd::Key(k, m) => {
                    let mut out = Vec::new();
                    e.encode_key(k, m, &mut out)?;
                    if !out.is_empty() {
                        write(out);
                    }
                }
                Cmd::FrameTaken => ui_ready = true,
            }
        }
        if changed && ui_ready {
            let frame = e.build_frame()?;
            if let Ok(mut slot) = latest.lock() {
                *slot = Some(Arc::new(frame));
            }
            changed = false;
            ui_ready = false;
            wake();
        }
    }
    Ok(())
}

impl Engine {
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
        let mut mods = key::Mods::empty();
        if m.shift {
            mods |= key::Mods::SHIFT;
        }
        if m.ctrl {
            mods |= key::Mods::CTRL;
        }
        if m.alt {
            mods |= key::Mods::ALT;
        }
        self.kev
            .set_action(key::Action::Press)
            .set_key(gk)
            .set_mods(mods)
            .set_consumed_mods(key::Mods::empty())
            .set_unshifted_codepoint(unshifted)
            .set_utf8(None::<String>);
        self.enc
            .set_options_from_terminal(&self.term)
            .encode_to_vec(&self.kev, out)
    }

    /// Copies the rows libghostty marked dirty into `self.rows` and returns a frame
    /// sharing the unchanged rows with the previous one.
    fn build_frame(&mut self) -> libghostty_vt::error::Result<Frame> {
        let snap = self.rs.update(&self.term)?;
        let full = matches!(snap.dirty()?, Dirty::Full);
        let colors = snap.colors()?;
        let (bg, fg): (Rgb, Rgb) = (colors.background.into(), colors.foreground.into());
        let cursor = snap
            .cursor()
            .ok()
            .filter(|c| c.visible)
            .and_then(|c| c.viewport.map(|v| (v.x, v.y)));
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
            if full || row.dirty()? {
                let mut out = Row::default();
                let mut cells = self.cells_it.update(row)?;
                while let Some(cell) = cells.next() {
                    if matches!(cell.raw_cell()?.wide(), Ok(CellWide::SpacerTail)) {
                        continue;
                    }
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
                    };
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
                                && last.underline == run.underline =>
                        {
                            last.len += run.len
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
        self.seq += 1;
        Ok(Frame {
            cols: self.cols,
            rows: self.rows.clone(),
            background: bg,
            foreground: fg,
            cursor,
            seq: self.seq,
        })
    }
}

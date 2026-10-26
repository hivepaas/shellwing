//! A session as the phone sees it: what it is doing, the terminal's latest
//! frame as rows of styled text, and the keys and text sent to it.

use std::sync::{Arc, Mutex, Weak};

use shellwing_core::session::{Session, Status};
use shellwing_ssh::Learned;
use shellwing_ssh::keys::{self, Installed};
use shellwing_term::{Event, Frame, Mods, NamedKey, Rgb};

use crate::MobileError;

/// Told when a session has something new to show: a frame, its status, its
/// title. Called off the main thread; the app reads them on its own.
#[uniffi::export(with_foreign)]
pub trait SessionListener: Send + Sync {
    fn changed(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SessionStatus {
    Connecting,
    Connected,
    /// The shell ended (`exit`), or the server closed the session.
    Closed,
    /// The network broke; connecting again may work.
    Lost,
    /// It could not connect: refused sign-in, a host key, a bad address.
    Failed,
}

impl From<Status> for SessionStatus {
    fn from(s: Status) -> Self {
        match s {
            Status::Connecting => SessionStatus::Connecting,
            Status::Connected => SessionStatus::Connected,
            Status::Closed => SessionStatus::Closed,
            Status::Lost => SessionStatus::Lost,
            Status::Failed => SessionStatus::Failed,
        }
    }
}

/// A key that is not text: what the row above the keyboard sends, and the
/// keyboard's return and delete.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Key {
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
    /// A letter with Ctrl or Alt held: Ctrl+C.
    Letter {
        letter: String,
    },
}

/// Cells of one row that share a style, and their text.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RunData {
    pub text: String,
    /// First column.
    pub col: u16,
    /// Columns covered: 2 for a wide glyph.
    pub cells: u16,
    /// `0xrrggbb`.
    pub fg: u32,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub selected: bool,
    /// One glyph over two cells (CJK, most emoji).
    pub wide: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RowData {
    pub runs: Vec<RunData>,
}

/// What the terminal shows now.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FrameData {
    pub cols: u16,
    pub rows: Vec<RowData>,
    pub background: u32,
    pub foreground: u32,
    /// The cursor's cell, when it shows.
    pub cursor_col: Option<u16>,
    pub cursor_row: Option<u16>,
    pub cursor_color: u32,
    /// A full-screen program (vim, htop) is on.
    pub alternate: bool,
    /// The history is shown, not the live screen.
    pub scrolled_back: bool,
    /// Grows with every frame.
    pub seq: u64,
}

#[derive(uniffi::Object)]
pub struct MobileSession {
    session: Mutex<Session>,
    /// What the shell set as the terminal's title, else the host's name.
    title: Mutex<String>,
    /// The last frame taken: a new one only replaces it.
    frame: Mutex<Option<FrameData>>,
    /// A password or passphrase typed at a prompt that worked: offered to
    /// keep in the vault.
    offer: Mutex<Option<Learned>>,
}

impl MobileSession {
    pub(crate) fn new(
        session: Session,
        listener: Arc<dyn SessionListener>,
        rt: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let (wake, events, learned) = (
            session.wake.clone(),
            session.events.clone(),
            session.learned.clone(),
        );
        let me = Arc::new(MobileSession {
            title: Mutex::new(session.title.clone()),
            session: Mutex::new(session),
            frame: Mutex::new(None),
            offer: Mutex::new(None),
        });
        let weak: Weak<MobileSession> = Arc::downgrade(&me);
        rt.spawn(async move {
            loop {
                tokio::select! {
                    woke = wake.recv() => {
                        if woke.is_err() {
                            break;
                        }
                    }
                    event = events.recv() => match event {
                        Ok(Event::Title(t)) => {
                            let Some(me) = weak.upgrade() else { break };
                            if !t.is_empty() {
                                *lock(&me.title) = t;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    },
                    typed = learned.recv() => match typed {
                        Ok(l) => {
                            let Some(me) = weak.upgrade() else { break };
                            *lock(&me.offer) = Some(l);
                        }
                        Err(_) => break,
                    },
                }
                if weak.strong_count() == 0 {
                    break;
                }
                listener.changed();
            }
        });
        me
    }
}

#[uniffi::export]
impl MobileSession {
    pub fn title(&self) -> String {
        lock(&self.title).clone()
    }

    pub fn spec(&self) -> String {
        lock(&self.session).spec.clone()
    }

    pub fn status(&self) -> SessionStatus {
        lock(&self.session).status().into()
    }

    /// How the last connection ended, when it did.
    pub fn ended(&self) -> Option<String> {
        lock(&self.session).ended()
    }

    /// The terminal as it is now: a new frame when there is one, else the
    /// last one again (None before the first).
    pub fn frame(&self) -> Option<FrameData> {
        let mut last = lock(&self.frame);
        if let Some(f) = lock(&self.session).term.take_frame() {
            *last = Some(frame_data(&f));
        }
        last.clone()
    }

    /// Typed text, as the keyboard gives it.
    pub fn text(&self, text: String) {
        lock(&self.session).term.text(text);
    }

    pub fn paste(&self, text: String) {
        lock(&self.session).term.paste(text, false);
    }

    pub fn key(&self, key: Key, ctrl: bool, alt: bool, shift: bool) {
        let named = match key {
            Key::Enter => NamedKey::Enter,
            Key::Backspace => NamedKey::Backspace,
            Key::Tab => NamedKey::Tab,
            Key::Escape => NamedKey::Escape,
            Key::Up => NamedKey::Up,
            Key::Down => NamedKey::Down,
            Key::Left => NamedKey::Left,
            Key::Right => NamedKey::Right,
            Key::Home => NamedKey::Home,
            Key::End => NamedKey::End,
            Key::PageUp => NamedKey::PageUp,
            Key::PageDown => NamedKey::PageDown,
            Key::Delete => NamedKey::Delete,
            Key::Letter { letter } => match letter.chars().next() {
                Some(c) if c.is_ascii_alphabetic() => NamedKey::Letter(c.to_ascii_lowercase()),
                _ => return,
            },
        };
        let mods = Mods {
            shift,
            ctrl,
            alt,
            logo: false,
        };
        lock(&self.session).term.key(named, mods);
    }

    /// The terminal's size in cells, and a cell's in pixels.
    pub fn resize(&self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        lock(&self.session)
            .term
            .resize(cols.max(1), rows.max(1), cell_w, cell_h);
    }

    /// Scrolls the history by `lines` (up: positive), as a wheel does.
    pub fn scroll(&self, lines: f32) {
        lock(&self.session)
            .term
            .wheel(lines, 0., 0., Mods::default());
    }

    pub fn scroll_to_bottom(&self) {
        lock(&self.session).term.scroll_to_bottom();
    }

    /// Connects again, after it was lost or closed.
    pub fn reconnect(&self) {
        lock(&self.session).reconnect();
    }

    /// What a prompt that worked would keep in the vault, said for the
    /// question: `the password for deploy@web-1`.
    pub fn offer(&self) -> Option<String> {
        lock(&self.offer).as_ref().map(|l| match l {
            Learned::Password { target, .. } => format!("the password for {target}"),
            Learned::Passphrase { key_path, .. } => format!("the passphrase of {key_path}"),
        })
    }

    /// Keeps what was offered in the vault: tried first next time.
    pub fn save_offer(&self) -> Result<(), MobileError> {
        let Some(l) = lock(&self.offer).take() else {
            return Ok(());
        };
        lock(&self.session).save(&l).map_err(MobileError::Failed)
    }

    pub fn dismiss_offer(&self) {
        lock(&self.offer).take();
    }

    /// Adds `public` (one line, as `authorized_keys` takes it) to the host's
    /// `~/.ssh/authorized_keys`, over this session's connection. True when
    /// it was added, false when it was there already.
    pub async fn install_key(&self, public: String) -> Result<bool, MobileError> {
        let answer = lock(&self.session)
            .remote()
            .exec(keys::INSTALL, format!("{}\n", public.trim()).into_bytes())
            .ok_or_else(|| MobileError::Failed("Not connected.".into()))?;
        let ran = answer
            .await
            .map_err(|_| MobileError::Failed("the connection ended".into()))?
            .map_err(MobileError::of)?;
        match keys::installed(ran.status, &ran.stdout, &ran.stderr) {
            Ok(Installed::Added) => Ok(true),
            Ok(Installed::Present) => Ok(false),
            Err(e) => Err(MobileError::Failed(e)),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn hex(c: Rgb) -> u32 {
    (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

fn frame_data(f: &Frame) -> FrameData {
    let rows = f
        .rows
        .iter()
        .map(|row| {
            let mut at = 0;
            let runs = row
                .runs
                .iter()
                .map(|r| {
                    let end = (at + r.len).min(row.text.len());
                    let text = row.text.get(at..end).unwrap_or_default().to_string();
                    at = end;
                    RunData {
                        text,
                        col: r.col,
                        cells: r.cells,
                        fg: hex(r.fg),
                        bg: r.bg.map(hex),
                        bold: r.bold,
                        italic: r.italic,
                        underline: r.underline,
                        selected: r.selected,
                        wide: r.wide,
                    }
                })
                .collect();
            RowData { runs }
        })
        .collect();
    FrameData {
        cols: f.cols,
        rows,
        background: hex(f.background),
        foreground: hex(f.foreground),
        cursor_col: f.cursor.map(|(x, _)| x),
        cursor_row: f.cursor.map(|(_, y)| y),
        cursor_color: hex(f.cursor_color.unwrap_or(f.foreground)),
        alternate: f.alternate,
        scrolled_back: f.scrolled_back,
        seq: f.seq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_term::{Found, Row, Run};

    fn run(len: usize, col: u16, cells: u16, fg: Rgb) -> Run {
        Run {
            len,
            fg,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
            selected: false,
            found: Found::No,
            col,
            cells,
            wide: false,
        }
    }

    #[test]
    fn a_frame_becomes_runs_of_text() {
        let green = Rgb {
            r: 0x4a,
            g: 0xde,
            b: 0x80,
        };
        let white = Rgb {
            r: 0xff,
            g: 0xff,
            b: 0xff,
        };
        let row = Row {
            text: "deploy@web-1:~$ ".into(),
            runs: vec![run(12, 0, 12, green), run(4, 12, 4, white)],
        };
        let f = Frame {
            cols: 80,
            rows: vec![Arc::new(row)],
            cursor: Some((16, 0)),
            ..Frame::default()
        };
        let d = frame_data(&f);
        assert_eq!(d.rows[0].runs[0].text, "deploy@web-1");
        assert_eq!(d.rows[0].runs[0].fg, 0x4ade80);
        assert_eq!(d.rows[0].runs[1].text, ":~$ ");
        assert_eq!((d.cursor_col, d.cursor_row), (Some(16), Some(0)));
    }
}

//! A session as the phone sees it: what it is doing, the terminal's latest
//! frame as rows of styled text, and the keys and text sent to it.

use shellwing_i18n::tr;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use shellwing_config::{OnDisconnect, Store};
use shellwing_core::envs::{self, Envs, LoadedEnv};
use shellwing_core::session::{RETRY_BACKOFF_SECS, Session, Status};
use shellwing_ssh::Learned;
use shellwing_ssh::keys::{self, Installed};
use shellwing_term::{Event, Frame, Mods, MouseAction, MouseButton, MouseInput, NamedKey, Rgb};

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

/// What a finger does to select text: a long press takes the word under it,
/// dragging takes more, word by word, and lifting it ends the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Selecting {
    Start,
    Extend,
    End,
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
    /// Some text is selected: there is something to copy.
    pub selected: bool,
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
    /// Environments loaded into the shell, and those waiting for it.
    envs: Mutex<Envs>,
    /// What to say once: an environment loaded, a load given up.
    notice: Mutex<Option<String>>,
    /// The text of the last selection copied, until taken.
    copied: Mutex<Option<String>>,
    /// The session's SFTP, opened the first time Files asks.
    pub(crate) sftp: tokio::sync::Mutex<Option<shellwing_ssh::Sftp>>,
    /// The app's runtime: what Swift awaits runs there (`on_runtime`).
    pub(crate) rt: tokio::runtime::Handle,
    /// The configuration: what a lost connection does (`[terminal]
    /// on_disconnect`, the desktop's setting).
    store: Store,
    /// Reconnecting by itself, after a connection that worked was lost.
    retry: Mutex<Retry>,
    listener: Arc<dyn SessionListener>,
    /// Config files placed on the host until the session ends.
    placed: Mutex<Vec<shellwing_core::placing::Placed>>,
}

/// An environment loaded into a session's shell.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LoadedEnvRow {
    pub id: String,
    pub name: String,
}

impl MobileSession {
    /// The size the shell on the host is given.
    #[cfg(test)]
    pub(crate) fn shell_size(&self) -> Option<(u32, u32)> {
        lock(&self.session).writer.size()
    }

    pub(crate) fn new(
        session: Session,
        listener: Arc<dyn SessionListener>,
        rt: &tokio::runtime::Handle,
        store: Store,
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
            envs: Mutex::default(),
            notice: Mutex::new(None),
            copied: Mutex::new(None),
            sftp: tokio::sync::Mutex::new(None),
            placed: Mutex::default(),
            rt: rt.clone(),
            store,
            retry: Mutex::default(),
            listener: listener.clone(),
        });
        let weak: Weak<MobileSession> = Arc::downgrade(&me);
        rt.spawn(async move {
            // The last status change acted on: each is, once.
            let mut seen = 0;
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
                        Ok(Event::Copied(text)) => {
                            let Some(me) = weak.upgrade() else { break };
                            *lock(&me.copied) = Some(text);
                        }
                        Ok(Event::Signal(signal)) => {
                            let Some(me) = weak.upgrade() else { break };
                            let term = lock(&me.session).term.clone();
                            if let Some(name) = lock(&me.envs).signal(&term, &signal) {
                                *lock(&me.notice) = Some(tr!("{name} is loaded into this shell.", name = name));
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
                let Some(me) = weak.upgrade() else { break };
                let (n, status) = lock(&me.session).status_change();
                if n != seen {
                    seen = n;
                    me.status_moved(status);
                }
                drop(me);
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

    /// The terminal's size in cells, and a cell's in pixels: the grid's,
    /// and the shell's on the host, as the desktop's pane does.
    pub fn resize(&self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let session = lock(&self.session);
        session.term.resize(cols, rows, cell_w, cell_h);
        session.writer.resize(u32::from(cols), u32::from(rows));
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

    /// Selects with a finger at (`x`, `y`), in grid pixels (see
    /// [`Selecting`]). It selects under a program that takes the mouse too
    /// (htop, vim with the mouse on), as Shift does with a mouse.
    pub fn select(&self, how: Selecting, x: f32, y: f32) {
        let term = lock(&self.session).term.clone();
        let at = |action| MouseInput {
            action,
            button: Some(MouseButton::Left),
            x,
            y,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
            link: false,
        };
        match how {
            // Pressed twice, as a double click is: the word.
            Selecting::Start => {
                term.mouse(at(MouseAction::Press));
                term.mouse(at(MouseAction::Release));
                term.mouse(at(MouseAction::Press));
            }
            Selecting::Extend => term.mouse(at(MouseAction::Motion)),
            Selecting::End => term.mouse(at(MouseAction::Release)),
        }
    }

    pub fn clear_selection(&self) {
        lock(&self.session).term.clear_selection();
    }

    /// Copies the selection: its text comes from [`Self::copied`] once the
    /// terminal has it, and the listener is told.
    pub fn copy_selection(&self) {
        lock(&self.session).term.copy();
    }

    /// The text copied last, once.
    pub fn copied(&self) -> Option<String> {
        lock(&self.copied).take()
    }

    /// Connects again, after it was lost or closed; trying by itself starts
    /// over.
    pub fn reconnect(&self) {
        self.start_retrying_over();
        lock(&self.session).reconnect();
    }

    /// While it is going to reconnect by itself: in how long, and which
    /// try of how many that is.
    pub fn retrying(&self) -> Option<Retrying> {
        let r = lock(&self.retry);
        let due = r.due?;
        Some(Retrying {
            seconds: due
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                .ceil() as u32,
            attempt: r.tries as u32,
            attempts: RETRY_BACKOFF_SECS.len() as u32,
        })
    }

    /// Stops reconnecting by itself, coming back to the app included, until
    /// Connect again.
    pub fn stop_retrying(&self) {
        let mut r = lock(&self.retry);
        r.plan += 1;
        r.due = None;
        r.tries = RETRY_BACKOFF_SECS.len();
        r.stopped = true;
    }

    /// The app is back from the background, or the network changed. A phone
    /// asleep does not hear its connections die, so one that looks connected
    /// is asked whether it answers; one that does not is shown lost. With
    /// the setting Reconnect, a lost one is also made again at once, unless
    /// trying by itself was stopped.
    pub fn resumed(self: Arc<Self>) {
        let by_itself =
            self.on_disconnect() == OnDisconnect::Reconnect && !lock(&self.retry).stopped;
        let (status, ever) = {
            let s = lock(&self.session);
            (s.status(), s.ever_connected())
        };
        match status {
            Status::Lost if ever && by_itself => self.reconnect(),
            Status::Connected => {
                let Some(answer) = lock(&self.session).remote().ping() else {
                    return;
                };
                let me = self.clone();
                self.rt.spawn(async move {
                    if answer.await.unwrap_or(false) {
                        return;
                    }
                    if by_itself {
                        me.start_retrying_over();
                        lock(&me.session).reconnect_unanswered();
                    } else {
                        lock(&me.session).drop_unanswered();
                    }
                });
            }
            _ => {}
        }
    }

    /// Something to say once (an environment loaded, a load given up); taken.
    pub fn notice(&self) -> Option<String> {
        lock(&self.notice).take()
    }

    /// The environments loaded into the shell.
    pub fn loaded_envs(&self) -> Vec<LoadedEnvRow> {
        lock(&self.envs)
            .loaded()
            .iter()
            .map(|e| LoadedEnvRow {
                id: e.id.clone(),
                name: e.name.clone(),
            })
            .collect()
    }

    /// Unloads environment `id` from the shell (None: all of them).
    pub fn unload_env(&self, id: Option<String>) -> Result<(), MobileError> {
        self.ready()?;
        let term = lock(&self.session).term.clone();
        lock(&self.envs).unload(&term, id.as_deref());
        Ok(())
    }

    /// What a prompt that worked would keep in the vault, said for the
    /// question: `the password for deploy@web-1`.
    pub fn offer(&self) -> Option<String> {
        lock(&self.offer).as_ref().map(|l| match l {
            Learned::Password { target, .. } => {
                tr!("the password for {target}", target = target.to_string())
            }
            Learned::Passphrase { key_path, .. } => {
                tr!("the passphrase of {path}", path = key_path.to_string())
            }
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
    pub async fn install_key(self: Arc<Self>, public: String) -> Result<bool, MobileError> {
        let rt = self.rt.clone();
        crate::on_runtime(&rt, async move { self.install_key_now(public).await }).await
    }
}

impl MobileSession {
    async fn install_key_now(&self, public: String) -> Result<bool, MobileError> {
        let answer = lock(&self.session)
            .remote()
            .exec(keys::INSTALL, format!("{}\n", public.trim()).into_bytes())
            .ok_or_else(|| MobileError::Failed(tr!("Not connected.")))?;
        let ran = answer
            .await
            .map_err(|_| MobileError::Failed(tr!("the connection ended")))?
            .map_err(MobileError::of)?;
        match keys::installed(ran.status, &ran.stdout, &ran.stderr) {
            Ok(Installed::Added) => Ok(true),
            Ok(Installed::Present) => Ok(false),
            Err(e) => Err(MobileError::Failed(e)),
        }
    }
}

/// A lost connection about to be tried again: in `seconds`, try `attempt`
/// of `attempts`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Retrying {
    pub seconds: u32,
    pub attempt: u32,
    pub attempts: u32,
}

/// Reconnecting by itself: tries made since the last connection, when the
/// next is due, which plan is current (one called off is not made), and
/// whether the user stopped it.
#[derive(Default)]
struct Retry {
    tries: usize,
    due: Option<Instant>,
    plan: u64,
    stopped: bool,
}

/// The wait before try `tries` (from 0) at reconnecting; None: no more
/// tries, the user connects again.
fn retry_wait(tries: usize) -> Option<Duration> {
    RETRY_BACKOFF_SECS
        .get(tries)
        .map(|s| Duration::from_secs(*s))
}

impl MobileSession {
    fn on_disconnect(&self) -> OnDisconnect {
        self.store
            .settings()
            .map(|s| s.terminal.on_disconnect)
            .unwrap_or_default()
    }

    /// The connection's status moved: connected, trying starts over; lost,
    /// after it had worked, it is tried again, waiting longer each time, as
    /// the desktop does.
    fn status_moved(self: &Arc<Self>, status: Status) {
        match status {
            Status::Connected => self.start_retrying_over(),
            Status::Lost
                if self.on_disconnect() == OnDisconnect::Reconnect
                    && lock(&self.session).ever_connected()
                    && lock(&self.retry).due.is_none() =>
            {
                self.plan_retry();
            }
            _ => {}
        }
    }

    /// Tries made start from none, one planned is called off, and a stop
    /// is forgotten.
    fn start_retrying_over(&self) {
        let mut r = lock(&self.retry);
        r.tries = 0;
        r.due = None;
        r.plan += 1;
        r.stopped = false;
    }

    /// Reconnects after the next wait, saying each second how long is left.
    fn plan_retry(self: &Arc<Self>) {
        let (plan, due) = {
            let mut r = lock(&self.retry);
            let Some(wait) = retry_wait(r.tries) else {
                r.due = None;
                return;
            };
            r.tries += 1;
            r.plan += 1;
            let due = Instant::now() + wait;
            r.due = Some(due);
            (r.plan, due)
        };
        let weak = Arc::downgrade(self);
        self.rt.spawn(async move {
            loop {
                let wait = due
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(1));
                tokio::time::sleep(wait).await;
                let Some(me) = weak.upgrade() else { return };
                if lock(&me.retry).plan != plan {
                    return;
                }
                if Instant::now() >= due {
                    lock(&me.retry).due = None;
                    lock(&me.session).reconnect();
                    me.listener.changed();
                    return;
                }
                me.listener.changed();
            }
        });
    }

    /// Why nothing can be typed into the shell now: not connected, or a
    /// full-screen program has the keys.
    fn ready(&self) -> Result<(), MobileError> {
        if lock(&self.session).status() != Status::Connected {
            return Err(MobileError::Failed(tr!("Not connected.")));
        }
        if lock(&self.frame).as_ref().is_some_and(|f| f.alternate) {
            return Err(MobileError::Failed(tr!(
                "A full-screen program is running: leave it, then try again."
            )));
        }
        Ok(())
    }

    pub(crate) fn placed_now(&self) -> Vec<shellwing_core::placing::Placed> {
        lock(&self.placed).clone()
    }

    /// The placed file at `path`, or all of them.
    pub(crate) fn placed_matching(
        &self,
        path: Option<&str>,
    ) -> Vec<shellwing_core::placing::Placed> {
        lock(&self.placed)
            .iter()
            .filter(|p| path.is_none_or(|x| p.path == x))
            .cloned()
            .collect()
    }

    pub(crate) fn add_placed(&self, p: shellwing_core::placing::Placed) {
        let mut all = lock(&self.placed);
        all.retain(|x| x.path != p.path);
        all.push(p);
    }

    /// No longer placed until the session ends: taken away, or kept.
    pub(crate) fn drop_placed(&self, paths: &[String]) {
        lock(&self.placed).retain(|p| !paths.contains(&p.path));
    }

    /// What runs beside the shell: SFTP, commands.
    pub(crate) fn remote(&self) -> shellwing_core::session::Remote {
        lock(&self.session).remote()
    }

    /// `host`, `user` and `address`, as snippets name them.
    pub(crate) fn built_ins(&self) -> Vec<(String, String)> {
        let s = lock(&self.session);
        let mut out = vec![(
            "host".to_string(),
            s.entry.clone().unwrap_or_else(|| s.title.clone()),
        )];
        if let Some(t) = s.reached() {
            out.push(("user".into(), t.user.clone()));
            out.push(("address".into(), t.host.clone()));
        }
        out
    }

    /// Types `text` (several lines as one paste), and Enter if `enter`.
    pub(crate) fn type_text(&self, text: &str, enter: bool) -> Result<(), MobileError> {
        self.ready()?;
        let term = lock(&self.session).term.clone();
        let text = text.trim_end_matches('\n');
        if text.contains('\n') {
            term.paste(text.to_string(), true);
        } else {
            term.text(text.to_string());
        }
        if enter {
            term.key(NamedKey::Enter, Mods::default());
        }
        Ok(())
    }

    pub(crate) fn run_with_env(
        self: &Arc<Self>,
        values: shellwing_vault::EnvValues,
        command: &str,
        enter: bool,
        rt: &tokio::runtime::Runtime,
    ) -> Result<(), MobileError> {
        self.ready()?;
        let term = lock(&self.session).term.clone();
        let ready = lock(&self.envs)
            .run_with(&term, values, command, enter)
            .map_err(MobileError::Failed)?;
        self.give_up_later(ready, rt);
        Ok(())
    }

    pub(crate) fn load_env(
        self: &Arc<Self>,
        env: LoadedEnv,
        values: shellwing_vault::EnvValues,
        rt: &tokio::runtime::Runtime,
    ) -> Result<(), MobileError> {
        self.ready()?;
        let term = lock(&self.session).term.clone();
        let ready = lock(&self.envs)
            .load(&term, env, values)
            .map_err(MobileError::Failed)?;
        self.give_up_later(ready, rt);
        Ok(())
    }

    /// Gives up on `ready` if the shell has not signalled it by then.
    fn give_up_later(self: &Arc<Self>, ready: String, rt: &tokio::runtime::Runtime) {
        let me = Arc::downgrade(self);
        rt.spawn(async move {
            tokio::time::sleep(envs::WAIT).await;
            if let Some(me) = me.upgrade()
                && lock(&me.envs).give_up(&ready)
            {
                *lock(&me.notice) = Some(envs::gave_up());
            }
        });
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
        selected: f.selected,
        seq: f.seq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_term::{Found, Row, Run};

    #[test]
    fn a_lost_connection_is_tried_again_waiting_longer_then_left() {
        let waits: Vec<u64> = (0..).map_while(retry_wait).map(|d| d.as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 30], "the desktop's");
        assert!(retry_wait(6).is_none(), "then the user connects again");
    }

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

//! The Files panel: the remote side of a pane's connection, over SFTP, beside
//! the terminal. Click or the arrow keys choose; a double click or Enter opens a
//! folder or downloads a file; Backspace goes up; F2 renames and Delete deletes
//! (after asking). Dropping files or folders on the panel uploads them into the
//! folder shown. Downloads go to the Downloads folder, under a new name when one
//! is taken.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    ClickEvent, Context, ElementId, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable,
    FontWeight, KeyDownEvent, MouseButton, Render, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, div, prelude::*, px, relative, rgb, uniform_list,
};
use shellwing_ssh::sftp::{self, Entry, Progress};
use shellwing_ssh::{Error, Sftp};

use crate::input::TextInput;
use crate::session::Remote;
use crate::sidebar::{LINE, MUTED, TEXT};

pub const WIDTH: f32 = 340.;
const ROW_H: f32 = 26.;
const PANEL: u32 = 0x0d121c;
const HOVER: u32 = 0x141c2b;
const SELECTED: u32 = 0x17233a;
const DROP: u32 = 0x132036;
const FOLDER: u32 = 0x7aa2f7;
const BAD: u32 = 0xf87171;
const BAR: u32 = 0x3b82f6;
/// How often running transfers redraw their progress.
const TICK: Duration = Duration::from_millis(250);
/// Finished transfers kept in the list.
const KEEP: usize = 6;

pub enum FilesEvent {
    Close,
}

#[derive(Clone, PartialEq)]
enum State {
    Opening,
    Loading,
    Ready,
    Failed(SharedString),
}

enum Step {
    Running,
    /// An upload whose target is already there, waiting for an answer.
    Exists {
        local: PathBuf,
        remote: String,
    },
    /// Downloads say where they went.
    Done(Option<PathBuf>),
    Failed(String),
}

/// A name being typed: for a new folder, or a new name for an entry.
#[derive(Clone, PartialEq)]
enum Edit {
    NewFolder,
    Rename(String),
}

/// How a transfer job ended.
enum Outcome {
    Done(Option<PathBuf>),
    /// An upload found its target there and wrote nothing.
    Exists {
        local: PathBuf,
        remote: String,
    },
}

struct Transfer {
    id: u64,
    upload: bool,
    name: String,
    progress: Arc<Progress>,
    started: Instant,
    step: Step,
    task: Option<tokio::task::AbortHandle>,
    /// Uploads: the folder they go into, shown again when they finish.
    into: Option<String>,
    /// Downloads: the local path, kept from other downloads of the same name.
    target: Option<PathBuf>,
}

pub struct FilesPanel {
    remote: Remote,
    focus: FocusHandle,
    sftp: Option<Sftp>,
    /// Bumped with each SFTP session, so answers meant for an old one are dropped.
    era: u64,
    home: Option<String>,
    dir: Option<String>,
    entries: Vec<Entry>,
    selected: Option<usize>,
    state: State,
    transfers: Vec<Transfer>,
    next_id: u64,
    ticking: bool,
    scroll: UniformListScrollHandle,
    downloads: PathBuf,
    input: Entity<TextInput>,
    editing: Option<Edit>,
    /// Why the name typed was not taken.
    edit_problem: Option<String>,
    /// An entry waiting for its deletion to be confirmed.
    deleting: Option<Entry>,
}

impl EventEmitter<FilesEvent> for FilesPanel {}

/// The user's Downloads folder, else their home.
fn downloads_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `name` in `dir`, or `name (1).ext`, `name (2).ext`… when taken.
fn unique(dir: &Path, name: &str, taken: &[&Path]) -> PathBuf {
    let free = |p: &Path| !p.exists() && !taken.contains(&p);
    let first = dir.join(name);
    if free(&first) {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| free(p))
        .unwrap_or(first)
}

/// A size as people read it: 512 B, 1.5 KB, 23 MB.
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024. && unit < UNITS.len() - 1 {
        v /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        return format!("{n} B");
    }
    let round = |v: f64, d: i32| (v * 10f64.powi(d)).round() / 10f64.powi(d);
    let mut decimals = if v < 10. { 1 } else { 0 };
    let mut shown = round(v, decimals);
    // 9.99 rounds to 10.0, shown as 10; 1023.97 KB rounds to 1024 KB: 1.0 MB.
    if shown >= 10. && decimals == 1 {
        decimals = 0;
        shown = round(v, 0);
    }
    if shown >= 1024. && unit < UNITS.len() - 1 {
        return format!("1.0 {}", UNITS[unit + 1]);
    }
    format!("{shown:.*} {}", decimals as usize, UNITS[unit])
}

/// When a file changed: the time today, the day this year, else the date.
fn when(t: SystemTime) -> String {
    use chrono::{DateTime, Datelike, Local};
    let t: DateTime<Local> = t.into();
    let now = Local::now();
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M").to_string()
    } else if t.year() == now.year() {
        t.format("%b %e").to_string()
    } else {
        t.format("%Y-%m-%d").to_string()
    }
}

fn button(id: impl Into<ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py(px(2.))
        .rounded_md()
        .text_xs()
        .text_color(rgb(TEXT))
        .bg(rgb(0x1a2333))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x243047)))
        .child(label.to_string())
}

impl FilesPanel {
    pub fn new(remote: Remote, cx: &mut Context<Self>) -> Self {
        let mut panel = FilesPanel {
            remote,
            focus: cx.focus_handle(),
            sftp: None,
            era: 0,
            home: None,
            dir: None,
            entries: vec![],
            selected: None,
            state: State::Opening,
            transfers: vec![],
            next_id: 0,
            ticking: false,
            scroll: UniformListScrollHandle::new(),
            downloads: downloads_dir(),
            input: cx.new(|cx| TextInput::new("Name", cx)),
            editing: None,
            edit_problem: None,
            deleting: None,
        };
        panel.connect(cx);
        panel
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Opens SFTP on the connection (again, after a reconnect), staying in the
    /// folder shown.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        self.era += 1;
        let era = self.era;
        self.sftp = None;
        let Some(answer) = self.remote.sftp() else {
            return self.fail("Not connected.".into(), cx);
        };
        self.state = State::Opening;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let opened = answer.await;
            let _ = this.update(cx, |p, cx| {
                if p.era != era {
                    return;
                }
                match opened {
                    Ok(Ok(sftp)) => {
                        p.sftp = Some(sftp);
                        let dir = p.dir.clone();
                        let chosen = p.chosen_name();
                        p.open_dir(dir, chosen, cx);
                    }
                    Ok(Err(e)) => p.fail(e.to_string(), cx),
                    Err(_) => p.fail("The connection ended.".into(), cx),
                }
            });
        })
        .detach();
    }

    fn fail(&mut self, msg: String, cx: &mut Context<Self>) {
        self.state = State::Failed(msg.into());
        cx.notify();
    }

    fn chosen_name(&self) -> Option<String> {
        self.selected
            .and_then(|i| self.entries.get(i))
            .map(|e| e.name.clone())
    }

    /// Runs `job` with the session on the network runtime, then `done` here.
    fn with_sftp<T, F>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Sftp) -> F,
        done: impl FnOnce(&mut Self, Result<T, Error>, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = Result<T, Error>> + Send + 'static,
    {
        let Some(sftp) = self.sftp.clone() else {
            return;
        };
        let era = self.era;
        let task = self.remote.rt.spawn(job(sftp));
        cx.spawn(async move |this, cx| {
            let result = task
                .await
                .unwrap_or_else(|e| Err(Error::Sftp(e.to_string())));
            let _ = this.update(cx, |p, cx| {
                if p.era == era {
                    done(p, result, cx);
                }
            });
        })
        .detach();
    }

    /// Lists `dir` (the home folder when None) and shows it, choosing the entry
    /// named `choose` when there is one.
    fn open_dir(&mut self, dir: Option<String>, choose: Option<String>, cx: &mut Context<Self>) {
        self.state = State::Loading;
        cx.notify();
        self.with_sftp(
            cx,
            move |sftp| async move {
                let (dir, home) = match dir {
                    Some(d) => (d, false),
                    None => (sftp.home().await?, true),
                };
                let entries = sftp.list(&dir).await?;
                Ok((dir, home, entries))
            },
            move |p, result, cx| match result {
                Ok((dir, home, entries)) => {
                    if home {
                        p.home = Some(dir.clone());
                    }
                    p.selected = choose
                        .and_then(|n| entries.iter().position(|e| e.name == n))
                        .or((!entries.is_empty()).then_some(0));
                    p.entries = entries;
                    p.dir = Some(dir);
                    p.state = State::Ready;
                    if let Some(i) = p.selected {
                        p.scroll.scroll_to_item(i, ScrollStrategy::Nearest);
                    }
                    cx.notify();
                }
                Err(e) => p.fail(e.to_string(), cx),
            },
        );
    }

    fn enter(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(dir), Some(e)) = (self.dir.clone(), self.entries.get(ix).cloned()) else {
            return;
        };
        if e.is_dir() {
            self.open_dir(Some(sftp::join(&dir, &e.name)), None, cx);
        } else {
            self.download(ix, cx);
        }
    }

    fn up(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone().filter(|d| d != "/") else {
            return;
        };
        // Back in the parent, the folder just left stays chosen.
        let came_from = dir
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .map(String::from);
        self.open_dir(Some(sftp::parent(&dir)), came_from, cx);
    }

    fn go_home(&mut self, cx: &mut Context<Self>) {
        self.open_dir(None, None, cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.sftp.is_none() {
            return self.connect(cx);
        }
        let chosen = self.chosen_name();
        self.open_dir(self.dir.clone(), chosen, cx);
    }

    fn add_transfer(
        &mut self,
        upload: bool,
        name: String,
        into: Option<String>,
        target: Option<PathBuf>,
    ) -> (u64, Arc<Progress>) {
        self.next_id += 1;
        let progress = Arc::new(Progress::default());
        self.transfers.push(Transfer {
            id: self.next_id,
            upload,
            name,
            progress: progress.clone(),
            started: Instant::now(),
            step: Step::Running,
            task: None,
            into,
            target,
        });
        (self.next_id, progress)
    }

    fn transfer(&mut self, id: u64) -> Option<&mut Transfer> {
        self.transfers.iter_mut().find(|t| t.id == id)
    }

    /// Runs a transfer job; its row follows it.
    fn start<F>(&mut self, id: u64, cx: &mut Context<Self>, job: impl FnOnce(Sftp) -> F)
    where
        F: Future<Output = Result<Outcome, Error>> + Send + 'static,
    {
        let Some(sftp) = self.sftp.clone() else {
            if let Some(t) = self.transfer(id) {
                t.step = Step::Failed("Not connected.".into());
            }
            return cx.notify();
        };
        let task = self.remote.rt.spawn(job(sftp));
        if let Some(t) = self.transfer(id) {
            t.task = Some(task.abort_handle());
            t.started = Instant::now();
            t.step = Step::Running;
        }
        self.tick(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |p, cx| p.finished(id, result, cx));
        })
        .detach();
    }

    fn finished(
        &mut self,
        id: u64,
        result: Result<Result<Outcome, Error>, tokio::task::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let Some(t) = self.transfer(id) else {
            return;
        };
        t.task = None;
        let mut refresh = None;
        match result {
            Ok(Ok(Outcome::Done(local))) => {
                t.step = Step::Done(local);
                if t.upload {
                    refresh = t.into.clone();
                }
            }
            Ok(Ok(Outcome::Exists { local, remote })) => t.step = Step::Exists { local, remote },
            Ok(Err(e)) => t.step = Step::Failed(e.to_string()),
            // Cancelled: the row goes.
            Err(_) => self.transfers.retain(|t| t.id != id),
        }
        let finished: Vec<u64> = self
            .transfers
            .iter()
            .filter(|t| matches!(t.step, Step::Done(_) | Step::Failed(_)))
            .map(|t| t.id)
            .collect();
        if finished.len() > KEEP {
            let drop = &finished[..finished.len() - KEEP];
            self.transfers.retain(|t| !drop.contains(&t.id));
        }
        if refresh.is_some() && refresh == self.dir {
            self.refresh(cx);
        }
        cx.notify();
    }

    /// Redraws while transfers run.
    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let running = this
                    .update(cx, |p, cx| {
                        let running = p.transfers.iter().any(|t| t.task.is_some());
                        p.ticking = running;
                        cx.notify();
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
    }

    fn download(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(dir), Some(e)) = (self.dir.clone(), self.entries.get(ix).cloned()) else {
            return;
        };
        let taken: Vec<&Path> = self
            .transfers
            .iter()
            .filter_map(|t| t.target.as_deref())
            .collect();
        let local = unique(&self.downloads, &e.name, &taken);
        let remote = sftp::join(&dir, &e.name);
        let (id, progress) = self.add_transfer(false, e.name, None, Some(local.clone()));
        let downloads = self.downloads.clone();
        self.start(id, cx, move |sftp| async move {
            tokio::fs::create_dir_all(&downloads)
                .await
                .map_err(|e| Error::Sftp(format!("{}: {e}", downloads.display())))?;
            sftp.download(&remote, &local, &progress).await?;
            Ok(Outcome::Done(Some(local)))
        });
    }

    fn download_chosen(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.selected {
            self.download(i, cx);
        }
    }

    /// Uploads `paths` into the folder shown, asking before writing over
    /// anything already there.
    pub fn upload(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        for local in paths {
            let Some(name) = local.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let remote = sftp::join(&dir, &name);
            let (id, progress) = self.add_transfer(true, name, Some(dir.clone()), None);
            self.start(id, cx, move |sftp| async move {
                if sftp.stat(&remote).await?.is_some() {
                    return Ok(Outcome::Exists { local, remote });
                }
                sftp.upload(&local, &remote, &progress).await?;
                Ok(Outcome::Done(None))
            });
        }
        cx.notify();
    }

    fn replace(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.transfer(id) else {
            return;
        };
        let Step::Exists { local, remote } = std::mem::replace(&mut t.step, Step::Running) else {
            return;
        };
        let progress = t.progress.clone();
        self.start(id, cx, move |sftp| async move {
            sftp.upload(&local, &remote, &progress).await?;
            Ok(Outcome::Done(None))
        });
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(t) = self.transfer(id)
            && let Some(task) = t.task.take()
        {
            task.abort();
        }
        self.transfers.retain(|t| t.id != id);
        cx.notify();
    }

    fn pick_uploads(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Upload".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |p, cx| p.upload(paths, cx));
            }
        })
        .detach();
    }

    fn select(&mut self, by: isize, cx: &mut Context<Self>) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let i = match self.selected {
            Some(i) => (i as isize + by).clamp(0, n as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(i);
        self.scroll.scroll_to_item(i, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn start_edit(&mut self, edit: Edit, window: &mut Window, cx: &mut Context<Self>) {
        if self.dir.is_none() || self.sftp.is_none() {
            return;
        }
        self.deleting = None;
        self.edit_problem = None;
        let (text, chosen) = match &edit {
            Edit::NewFolder => (String::new(), 0..0),
            Edit::Rename(name) => {
                // The name without its extension is chosen, as file managers do.
                let is_dir = self.entries.iter().any(|e| e.name == *name && e.is_dir());
                let stem = match name.rfind('.') {
                    Some(i) if i > 0 && !is_dir => i,
                    _ => name.len(),
                };
                (name.clone(), 0..stem)
            }
        };
        self.input.update(cx, |i, cx| i.set_text(&text, chosen, cx));
        self.editing = Some(edit);
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.chosen_name() {
            self.start_edit(Edit::Rename(name), window, cx);
        }
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.edit_problem = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Makes the folder or renames the entry, then shows it chosen.
    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(edit), Some(dir)) = (self.editing.clone(), self.dir.clone()) else {
            return;
        };
        let name = self.input.read(cx).text().trim().to_string();
        if let Some(why) = sftp::bad_name(&name) {
            self.edit_problem = Some(why.into());
            return cx.notify();
        }
        if edit == Edit::Rename(name.clone()) {
            return self.cancel_edit(window, cx);
        }
        let Some(sftp) = self.sftp.clone() else {
            return;
        };
        let to = sftp::join(&dir, &name);
        let task = self.remote.rt.spawn(async move {
            match edit {
                Edit::NewFolder => sftp.mkdir(&to).await,
                Edit::Rename(old) => sftp.rename(&sftp::join(&dir, &old), &to).await,
            }
        });
        let era = self.era;
        // In the window: the keyboard goes back to the list once it worked.
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .unwrap_or_else(|e| Err(Error::Sftp(e.to_string())));
            let _ = this.update_in(cx, |p, window, cx| {
                if p.era != era {
                    return;
                }
                match result {
                    Ok(()) => {
                        p.cancel_edit(window, cx);
                        let dir = p.dir.clone();
                        p.open_dir(dir, Some(name), cx);
                    }
                    Err(e) => {
                        p.edit_problem = Some(e.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn ask_delete(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.selected.and_then(|i| self.entries.get(i)).cloned() {
            self.editing = None;
            self.deleting = Some(e);
            cx.notify();
        }
    }

    /// Deletes the entry asked about, then chooses its neighbour.
    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let (Some(e), Some(dir)) = (self.deleting.take(), self.dir.clone()) else {
            return;
        };
        let at = self.entries.iter().position(|x| x.name == e.name);
        let neighbour = at.and_then(|i| {
            self.entries
                .get(i + 1)
                .or_else(|| i.checked_sub(1).and_then(|j| self.entries.get(j)))
                .map(|n| n.name.clone())
        });
        self.state = State::Loading;
        cx.notify();
        let path = sftp::join(&dir, &e.name);
        self.with_sftp(
            cx,
            move |sftp| async move { sftp.remove(&path).await },
            move |p, result, cx| match result {
                Ok(()) => {
                    let dir = p.dir.clone();
                    p.open_dir(dir, neighbour, cx);
                }
                Err(err) => p.fail(format!("Not deleted: {err}"), cx),
            },
        );
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        // Typing a name: the field has the other keys.
        if self.editing.is_some() {
            match k.key.as_str() {
                "enter" => self.commit_edit(window, cx),
                "escape" => self.cancel_edit(window, cx),
                _ => return,
            }
            return cx.stop_propagation();
        }
        if self.deleting.is_some() {
            match k.key.as_str() {
                "enter" => self.confirm_delete(cx),
                "escape" => {
                    self.deleting = None;
                    cx.notify();
                }
                _ => return,
            }
            return cx.stop_propagation();
        }
        // Cmd+Backspace deletes on macOS, as in Finder.
        if cfg!(target_os = "macos") && k.modifiers.platform && k.key == "backspace" {
            self.ask_delete(cx);
            return cx.stop_propagation();
        }
        if k.modifiers.control || k.modifiers.platform || k.modifiers.alt {
            return;
        }
        match k.key.as_str() {
            "f2" => self.start_rename(window, cx),
            "delete" => self.ask_delete(cx),
            "up" => self.select(-1, cx),
            "down" => self.select(1, cx),
            "pageup" => self.select(-10, cx),
            "pagedown" => self.select(10, cx),
            "enter" => {
                if let Some(i) = self.selected {
                    self.enter(i, cx);
                }
            }
            "backspace" => self.up(cx),
            "escape" => cx.emit(FilesEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The folder shown, `~` standing for home, shortened from the left.
    fn shown_path(&self) -> String {
        let Some(dir) = &self.dir else {
            return String::new();
        };
        let path = match &self.home {
            Some(h) if dir == h => "~".to_string(),
            Some(h) if dir.starts_with(&format!("{h}/")) => format!("~{}", &dir[h.len()..]),
            _ => dir.clone(),
        };
        let chars: Vec<char> = path.chars().collect();
        if chars.len() > 44 {
            format!("…{}", chars[chars.len() - 43..].iter().collect::<String>())
        } else {
            path
        }
    }

    fn row(&self, i: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let e = &self.entries[i];
        let selected = self.selected == Some(i);
        let name = if e.is_dir() {
            format!("{}/", e.name)
        } else {
            e.name.clone()
        };
        div()
            .id(("file", i))
            .h(px(ROW_H))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(selected, |d| d.bg(rgb(SELECTED)))
            .when(!selected, |d| d.hover(|s| s.bg(rgb(HOVER))))
            .on_click(cx.listener(move |p, ev: &ClickEvent, window, cx| {
                window.focus(&p.focus, cx);
                p.selected = Some(i);
                if ev.click_count() >= 2 {
                    p.enter(i, cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(rgb(if e.is_dir() { FOLDER } else { TEXT }))
                    .child(name),
            )
            .when(e.kind == sftp::Kind::Link, |d| {
                d.child(div().text_xs().text_color(rgb(MUTED)).child("link"))
            })
            .child(
                div()
                    .w(px(58.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_right()
                    .text_color(rgb(MUTED))
                    .child(if e.is_dir() {
                        String::new()
                    } else {
                        human_size(e.size)
                    }),
            )
            .child(
                div()
                    .w(px(78.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_right()
                    .text_color(rgb(MUTED))
                    .child(e.modified.map(when).unwrap_or_default()),
            )
            .into_any_element()
    }

    fn transfer_row(&self, t: &Transfer, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = t.id;
        let (done, total) = t.progress.get();
        let arrow = if t.upload { "↑" } else { "↓" };
        let status: String;
        let mut actions = div().flex().gap_1().flex_shrink_0();
        match &t.step {
            Step::Running => {
                let secs = t.started.elapsed().as_secs_f64().max(0.001);
                let rate = (done as f64 / secs) as u64;
                status = if total > 0 {
                    format!(
                        "{}% · {}/s",
                        done.saturating_mul(100) / total.max(1),
                        human_size(rate)
                    )
                } else {
                    "starting…".into()
                };
                actions = actions.child(
                    button(("cancel", id), "Cancel")
                        .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                );
            }
            Step::Exists { local, .. } => {
                status = "already there".into();
                let label = if local.is_dir() { "Merge" } else { "Replace" };
                actions = actions
                    .child(
                        button(("replace", id), label)
                            .on_click(cx.listener(move |p, _, _, cx| p.replace(id, cx))),
                    )
                    .child(
                        button(("skip", id), "Skip")
                            .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                    );
            }
            Step::Done(local) => {
                status = format!("done · {}", human_size(total));
                if let Some(path) = local.clone() {
                    actions = actions.child(
                        button(("show", id), "Show")
                            .on_click(cx.listener(move |_, _, _, cx| cx.reveal_path(&path))),
                    );
                }
                actions = actions.child(
                    button(("dismiss", id), "×")
                        .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                );
            }
            Step::Failed(why) => {
                status = why.clone();
                actions = actions.child(
                    button(("dismiss", id), "×")
                        .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                );
            }
        }
        let failed = matches!(t.step, Step::Failed(_));
        let fraction = if total > 0 {
            done as f32 / total as f32
        } else {
            0.
        };
        div()
            .px_3()
            .py_1()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(format!("{arrow} {}", t.name)),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .text_xs()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(rgb(if failed { BAD } else { MUTED }))
                    .child(status),
            )
            .when(matches!(t.step, Step::Running), |d| {
                d.child(
                    div().h(px(3.)).w_full().rounded_full().bg(rgb(LINE)).child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(rgb(BAR))
                            .w(relative(fraction.clamp(0., 1.))),
                    ),
                )
            })
            .into_any_element()
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(34.))
            .px_3()
            .border_b_1()
            .border_color(rgb(LINE))
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Files"))
            .child(
                button("files-close", "×")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FilesEvent::Close))),
            );
        let tools = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(self.shown_path()),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(button("files-up", "Up").on_click(cx.listener(|p, _, _, cx| p.up(cx))))
                    .child(
                        button("files-home", "Home")
                            .on_click(cx.listener(|p, _, _, cx| p.go_home(cx))),
                    )
                    .child(
                        button("files-refresh", "Refresh")
                            .on_click(cx.listener(|p, _, _, cx| p.refresh(cx))),
                    )
                    .child(button("files-mkdir", "New folder").on_click(
                        cx.listener(|p, _, window, cx| p.start_edit(Edit::NewFolder, window, cx)),
                    ))
                    .child(
                        button("files-upload", "Upload…")
                            .on_click(cx.listener(|p, _, _, cx| p.pick_uploads(cx))),
                    ),
            )
            .when(self.selected.is_some() && self.editing.is_none(), |d| {
                d.child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            button("files-download", "Download")
                                .on_click(cx.listener(|p, _, _, cx| p.download_chosen(cx))),
                        )
                        .child(
                            button("files-rename", "Rename").on_click(
                                cx.listener(|p, _, window, cx| p.start_rename(window, cx)),
                            ),
                        )
                        .child(
                            button("files-delete", "Delete")
                                .on_click(cx.listener(|p, _, _, cx| p.ask_delete(cx))),
                        ),
                )
            });

        // Typing a name, or confirming a deletion, above the list.
        let mut asking = div().flex().flex_col();
        if let Some(edit) = &self.editing {
            let label = match edit {
                Edit::NewFolder => "New folder".to_string(),
                Edit::Rename(old) => format!("Rename {old}"),
            };
            asking = asking.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(label))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x2b3a55))
                            .bg(rgb(0x0a0f18))
                            .child(self.input.clone()),
                    )
                    .child(match &self.edit_problem {
                        Some(p) => div().text_xs().text_color(rgb(BAD)).child(p.clone()),
                        None => div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("Enter to save · Esc to cancel"),
                    }),
            );
        }
        if let Some(e) = &self.deleting {
            let question = if e.kind == sftp::Kind::Dir {
                format!("Delete the folder {} and everything in it?", e.name)
            } else {
                format!("Delete {}?", e.name)
            };
            asking = asking.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(div().text_xs().child(question))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                button("files-delete-yes", "Delete")
                                    .bg(rgb(0x7f1d1d))
                                    .hover(|s| s.bg(rgb(0x991b1b)))
                                    .on_click(cx.listener(|p, _, _, cx| p.confirm_delete(cx))),
                            )
                            .child(button("files-delete-no", "Cancel").on_click(cx.listener(
                                |p, _, _, cx| {
                                    p.deleting = None;
                                    cx.notify();
                                },
                            ))),
                    ),
            );
        }

        let note = |text: String, color: u32| {
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(rgb(color))
                .child(text)
        };
        let mut body = div().flex_1().min_h_0().flex().flex_col();
        match &self.state {
            State::Opening => body = body.child(note("Opening SFTP…".into(), MUTED)),
            State::Loading if self.entries.is_empty() => {
                body = body.child(note("Loading…".into(), MUTED))
            }
            State::Failed(why) => {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pr_3()
                        .child(note(why.to_string(), BAD).flex_1())
                        .child(
                            button("files-retry", "Try again")
                                .on_click(cx.listener(|p, _, _, cx| p.refresh(cx))),
                        ),
                )
            }
            _ => {}
        }
        if self.dir.is_some() && self.entries.is_empty() && self.state == State::Ready {
            body = body.child(note("Empty folder.".into(), MUTED));
        }
        if !self.entries.is_empty() {
            body = body.child(
                uniform_list(
                    "files-list",
                    self.entries.len(),
                    cx.processor(|p, range: std::ops::Range<usize>, _, cx| {
                        range.map(|i| p.row(i, cx)).collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .when(self.state == State::Loading, |d| d.opacity(0.6)),
            );
        }

        let mut transfers = div()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(LINE))
            .py_1();
        if self.transfers.is_empty() {
            transfers = transfers.child(note(
                "Drop files or folders here to upload them.".into(),
                MUTED,
            ));
        }
        for t in &self.transfers {
            transfers = transfers.child(self.transfer_row(t, cx));
        }

        div()
            .id("files")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            // A click anywhere takes the keyboard, unless the name field just did.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, _, window, cx| {
                    if !p.focus.contains_focused(window, cx) {
                        window.focus(&p.focus, cx);
                    }
                }),
            )
            .on_drop(cx.listener(|p, paths: &ExternalPaths, _, cx| {
                p.upload(paths.paths().to_vec(), cx);
            }))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgb(DROP)))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(LINE))
            .text_sm()
            .text_color(rgb(TEXT))
            .child(header)
            .child(tools)
            .child(asking)
            .child(body)
            .child(transfers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(20 * 1024 * 1024), "20 MB");
        // Just under a unit is the next unit, not "1024 KB".
        assert_eq!(human_size(1024 * 1024 - 1), "1.0 MB");
        assert_eq!(human_size(10 * 1024 - 1), "10 KB");
    }

    #[test]
    fn taken_names_get_a_number() {
        let dir = std::env::temp_dir().join(format!("shellwing-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("a.txt"), "").expect("a");
        assert_eq!(unique(&dir, "b.txt", &[]), dir.join("b.txt"));
        assert_eq!(unique(&dir, "a.txt", &[]), dir.join("a (1).txt"));
        let pending = dir.join("a (1).txt");
        assert_eq!(unique(&dir, "a.txt", &[&pending]), dir.join("a (2).txt"));
        assert_eq!(unique(&dir, ".bashrc", &[]), dir.join(".bashrc"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

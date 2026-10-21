//! The Files panel: the remote side of a pane's connection, over SFTP, beside
//! the terminal. Click or the arrow keys choose; a double click or Enter opens a
//! folder or downloads a file; Backspace goes up; F2 renames and Delete deletes
//! (after asking). Dropping files or folders on the panel uploads them into the
//! folder shown. Downloads go to the Downloads folder, under a new name when one
//! is taken. Edit opens a file in an editor and writes it back each time it is
//! saved there, unless someone else changed it on the server meanwhile;
//! stopping it removes the copy, after asking when it holds changes the
//! server lacks (see `copies`).

use std::cell::RefCell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    ClickEvent, Context, ElementId, Entity, EventEmitter, ExternalDragPayload, ExternalPaths,
    FileDragPaths, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Render,
    ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px, relative,
    rgb, svg, uniform_list,
};
use shellwing_i18n::tr;
use shellwing_ssh::sftp::{self, Entry, Progress};
use shellwing_ssh::{Error, Sftp};

use crate::colors::colors;
use crate::copies::{self, EditCopy};
use crate::editors;
use crate::input::TextInput;
use crate::session::Remote;
use crate::sidebar::Tip;

/// The panel's width by default, and how far its edge can be dragged.
pub const WIDTH: f32 = 480.;
pub const MIN_WIDTH: f32 = 280.;
pub const MAX_WIDTH: f32 = 960.;

/// The panel's width, the same in every pane; set from the saved layout and
/// by dragging any panel's edge.
pub struct PanelWidth(pub f32);

impl gpui::Global for PanelWidth {}

impl Default for PanelWidth {
    fn default() -> Self {
        PanelWidth(WIDTH)
    }
}
const ROW_H: f32 = 26.;
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

/// An entry being dragged. Out of the window it is a file of this computer's
/// for the system: fetched into a folder of its own as the drag starts.
struct DragOut {
    name: String,
    dir: bool,
    /// Where it is being fetched to, once the drag has started.
    staged: Rc<RefCell<Option<PathBuf>>>,
}

/// What follows the pointer while an entry is dragged in the window.
/// What follows the pointer while something is dragged: its name.
pub(crate) struct DragGhost(pub SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(colors().edge))
            .bg(rgb(colors().raised))
            .text_sm()
            .text_color(rgb(colors().text))
            .child(self.0.clone())
    }
}

/// Where dragged entries are fetched to, each drag in a folder of its own.
fn drag_folder() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("shellwing").join("drag"))
}

/// Forgets drags of more than a day ago: whatever took them has its copy.
fn clean_drags(root: &Path) {
    let Ok(read) = std::fs::read_dir(root) else {
        return;
    };
    let day = Duration::from_secs(24 * 3600);
    for e in read.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// The editor command (`[files] editor`), set by the workspace from the settings.
#[derive(Default)]
pub struct EditorCommand(pub Option<String>);

impl gpui::Global for EditorCommand {}

/// What a context menu is for: an entry of the list, or the folder shown.
#[derive(Clone, Copy, PartialEq)]
enum MenuFor {
    Entry(usize),
    Folder,
}

/// How often edited files are looked at for a save.
const WATCH: Duration = Duration::from_secs(1);

#[derive(Clone, PartialEq)]
enum EditStep {
    /// Being downloaded.
    Opening,
    /// Not downloaded, so there is nothing to write back.
    NotOpened(String),
    /// In the editor; each save is written back. When the last one was.
    Watching(Option<String>),
    Saving,
    /// Changed on the server since it was opened: overwrite or stop.
    Conflict,
    /// The last write-back failed: tried again on the next save, on Retry,
    /// and once connected again.
    Unsent(String),
    /// Stopped, its changes moved to the Downloads folder.
    Kept(PathBuf),
}

/// A remote file being edited: a copy of its own, written back to `remote`
/// each time the editor saves it.
struct RemoteEdit {
    id: u64,
    name: String,
    remote: String,
    copy: EditCopy,
    /// The server's file as last read or written (size, mtime): anything else
    /// there means someone else wrote it.
    server: (u64, Option<SystemTime>),
    /// The copy when last written back (or tried, or downloaded).
    written: Option<copies::Stamp>,
    step: EditStep,
    /// Opened in a text editor, else in the system's app for the file.
    text: bool,
    /// Why the editor did not start.
    problem: Option<String>,
    /// Stop was asked for while the copy holds changes the server lacks:
    /// what to do with them.
    stopping: bool,
}

impl RemoteEdit {
    /// Still being edited: not failed to open, nor stopped.
    fn live(&self) -> bool {
        !matches!(self.step, EditStep::NotOpened(_) | EditStep::Kept(_))
    }
}

/// How a write-back went.
enum Wrote {
    Saved((u64, Option<SystemTime>)),
    Changed,
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
    /// Remote files open in an editor.
    edits: Vec<RemoteEdit>,
    watching: bool,
    /// The context menu, where it was asked for.
    menu: Option<(gpui::Point<gpui::Pixels>, MenuFor)>,
}

impl EventEmitter<FilesEvent> for FilesPanel {}

/// The user's Downloads folder, else their home.
pub(crate) fn downloads_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `name` in `dir`, or `name (1).ext`, `name (2).ext`… when taken.
pub(crate) fn unique(dir: &Path, name: &str, taken: &[&Path]) -> PathBuf {
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
    button_in(id, label, rgb(colors().button), rgb(colors().button_hover))
}

/// A button in other colors, such as a red Delete. (A second `hover` on a
/// [`button`] would not do: GPUI takes one.)
fn button_in(
    id: impl Into<ElementId>,
    label: &str,
    bg: gpui::Rgba,
    hover: gpui::Rgba,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py(px(2.))
        .rounded_md()
        .text_xs()
        .text_color(rgb(colors().text))
        .bg(bg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
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
            input: crate::input::said(cx, || tr!("Name")),
            editing: None,
            edit_problem: None,
            deleting: None,
            edits: vec![],
            watching: false,
            menu: None,
        };
        // Copies the server has as they are go with the panel or the app;
        // the others stay, for the next start to move to Downloads.
        cx.on_release(|p, _| p.end_edits()).detach();
        cx.on_app_quit(|p, _| {
            p.end_edits();
            async {}
        })
        .detach();
        panel.connect(cx);
        panel
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Opens SFTP on the connection (again, after a reconnect), staying in the
    /// folder shown.
    /// What the panel shows: the folder, its entries (folders end in `/`),
    /// and "opening", "loading", "ready" or the failure: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn shown(&self) -> (Option<String>, Vec<String>, String) {
        let state = match &self.state {
            State::Opening => "opening".to_string(),
            State::Loading => "loading".to_string(),
            State::Ready => "ready".to_string(),
            State::Failed(why) => format!("failed: {why}"),
        };
        let names = self
            .entries
            .iter()
            .map(|e| {
                if e.is_dir() {
                    format!("{}/", e.name)
                } else {
                    e.name.clone()
                }
            })
            .collect();
        (self.dir.clone(), names, state)
    }

    /// Selects the entry named `name`: for the UI tests, in place of the
    /// arrow keys or a click.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn select_named(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let found = self.entries.iter().position(|e| e.name == name);
        if found.is_some() {
            self.selected = found;
            cx.notify();
        }
        found.is_some()
    }

    /// The files being edited: each one's name, its local copy, and where it
    /// stands (`opening`, `watching`, `saving`, `conflict`, `unsent: …`,
    /// `kept`, `not opened: …`): for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn edits_shown(&self) -> Vec<(String, PathBuf, String)> {
        self.edits
            .iter()
            .map(|e| {
                let step = match &e.step {
                    EditStep::Opening => "opening".to_string(),
                    EditStep::NotOpened(why) => format!("not opened: {why}"),
                    EditStep::Watching(_) => "watching".to_string(),
                    EditStep::Saving => "saving".to_string(),
                    EditStep::Conflict => "conflict".to_string(),
                    EditStep::Unsent(why) => format!("unsent: {why}"),
                    EditStep::Kept(_) => "kept".to_string(),
                };
                (e.name.clone(), e.copy.path.clone(), step)
            })
            .collect()
    }

    /// Downloads go here instead of the user's Downloads: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn set_downloads(&mut self, dir: PathBuf) {
        self.downloads = dir;
    }

    /// Starts typing a new folder's name, as the header's button does: for
    /// the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_edit(Edit::NewFolder, window, cx);
    }

    pub fn connect(&mut self, cx: &mut Context<Self>) {
        self.era += 1;
        let era = self.era;
        self.sftp = None;
        // Answers to the old session are dropped: nothing more comes for these.
        for ed in &mut self.edits {
            match ed.step {
                EditStep::Opening => {
                    ed.copy.remove();
                    ed.step = EditStep::NotOpened(tr!("The connection ended."));
                }
                EditStep::Saving => {
                    ed.step = EditStep::Unsent(tr!("The connection ended while writing back."));
                }
                _ => {}
            }
        }
        let Some(answer) = self.remote.sftp() else {
            return self.fail(tr!("Not connected."), cx);
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
                        let unsent: Vec<u64> = p
                            .edits
                            .iter()
                            .filter(|e| matches!(e.step, EditStep::Unsent(_)))
                            .map(|e| e.id)
                            .collect();
                        for id in unsent {
                            p.write_back(id, false, cx);
                        }
                        let dir = p.dir.clone();
                        let chosen = p.chosen_name();
                        p.open_dir(dir, chosen, cx);
                    }
                    Ok(Err(e)) => p.fail(e.to_string(), cx),
                    Err(_) => p.fail(tr!("The connection ended."), cx),
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
                t.step = Step::Failed(tr!("Not connected."));
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

    /// Downloads an entry into the Downloads folder, under a free name.
    fn download(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(e) = self.entries.get(ix).cloned() else {
            return;
        };
        let taken: Vec<&Path> = self
            .transfers
            .iter()
            .filter_map(|t| t.target.as_deref())
            .collect();
        let local = unique(&self.downloads, &e.name, &taken);
        self.download_to(&e.name, local, cx);
    }

    /// Asks where (and under what name) to download an entry, then does.
    fn download_as(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(e) = self.entries.get(ix).cloned() else {
            return;
        };
        let picked = cx.prompt_for_new_path(&self.downloads, Some(&e.name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(local))) = picked.await {
                let _ = this.update(cx, |p, cx| p.download_to(&e.name, local, cx));
            }
        })
        .detach();
    }

    /// Fetches the entry `name` of the folder shown for a drag out of the
    /// window, into a folder of its own in the cache. It is fetched beside its
    /// place and moved there only once whole, so a drop never takes half of
    /// it. Returns its place.
    fn stage(&mut self, name: &str, cx: &mut Context<Self>) -> Option<PathBuf> {
        let dir = self.dir.clone()?;
        let root = drag_folder()?;
        clean_drags(&root);
        self.next_id += 1;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let folder = root.join(format!("{stamp}-{}", self.next_id));
        let target = folder.join(name);
        let fetching = folder.join(".fetching").join(name);
        let remote = sftp::join(&dir, name);
        let (id, progress) = self.add_transfer(false, name.to_string(), None, None);
        let place = target.clone();
        self.start(id, cx, move |sftp| async move {
            let io = |e: std::io::Error| Error::Sftp(format!("{}: {e}", place.display()));
            if let Some(into) = fetching.parent() {
                tokio::fs::create_dir_all(into).await.map_err(io)?;
            }
            sftp.download(&remote, &fetching, &progress).await?;
            tokio::fs::rename(&fetching, &place).await.map_err(io)?;
            Ok(Outcome::Done(None))
        });
        Some(target)
    }

    /// Downloads the entry `name` of the folder shown to `local`.
    fn download_to(&mut self, name: &str, local: PathBuf, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let remote = sftp::join(&dir, name);
        let (id, progress) = self.add_transfer(false, name.to_string(), None, Some(local.clone()));
        let into = local.parent().map(Path::to_path_buf);
        self.start(id, cx, move |sftp| async move {
            if let Some(into) = &into {
                tokio::fs::create_dir_all(into)
                    .await
                    .map_err(|e| Error::Sftp(format!("{}: {e}", into.display())))?;
            }
            sftp.download(&remote, &local, &progress).await?;
            Ok(Outcome::Done(Some(local)))
        });
    }

    /// Opens the chosen file (in a text editor, or the system's app for its
    /// kind), writing it back on each save.
    fn edit_chosen(&mut self, text: bool, cx: &mut Context<Self>) {
        let (Some(dir), Some(e)) = (
            self.dir.clone(),
            self.selected.and_then(|i| self.entries.get(i)).cloned(),
        ) else {
            return;
        };
        if e.is_dir() {
            return;
        }
        let remote = sftp::join(&dir, &e.name);
        // Already open: just to the editor again.
        if let Some(open) = self.edits.iter().find(|x| x.remote == remote && x.live()) {
            let local = open.copy.path.clone();
            return self.open_editor(&local, text, cx);
        }
        if self.sftp.is_none() {
            return self.fail(tr!("Not connected."), cx);
        }
        let copy = match EditCopy::new(&remote) {
            Ok(c) => c,
            Err(err) => {
                return self.fail(
                    tr!(
                        "Cannot make a folder to edit in: {err}",
                        err = err.to_string()
                    ),
                    cx,
                );
            }
        };
        let into = copy.path.clone();
        self.next_id += 1;
        let id = self.next_id;
        self.edits.push(RemoteEdit {
            id,
            name: e.name.clone(),
            remote: remote.clone(),
            copy,
            server: (e.size, e.modified),
            written: None,
            step: EditStep::Opening,
            text,
            problem: None,
            stopping: false,
        });
        cx.notify();
        self.with_sftp(
            cx,
            move |sftp| async move { sftp.download(&remote, &into, &Progress::default()).await },
            move |p, result, cx| {
                let Some(ed) = p.edits.iter_mut().find(|x| x.id == id) else {
                    return;
                };
                match result {
                    Ok(()) => {
                        let now = copies::stamp(&ed.copy.path);
                        ed.written = now;
                        ed.copy.sent(now);
                        ed.step = EditStep::Watching(None);
                        let (local, text) = (ed.copy.path.clone(), ed.text);
                        p.open_editor(&local, text, cx);
                        p.watch(cx);
                    }
                    Err(e) => {
                        ed.copy.remove();
                        ed.step = EditStep::NotOpened(e.to_string());
                    }
                }
                cx.notify();
            },
        );
    }

    /// A text editor (the configured one, else the system's for text), or
    /// the system's app for the file's kind.
    fn open_editor(&mut self, path: &Path, text: bool, cx: &mut Context<Self>) {
        if !text {
            return cx.open_with_system(path);
        }
        let cmd = cx
            .try_global::<EditorCommand>()
            .and_then(|c| c.0.clone())
            .filter(|c| !c.trim().is_empty());
        let started = match &cmd {
            Some(cmd) => editors::open_with(cmd, path),
            None => editors::open_text(path),
        };
        let problem = started.err().map(|e| {
            let who = cmd
                .as_deref()
                .map_or(tr!("the text editor"), editors::name_of);
            tr!("Cannot start {who}: {e}", who = who, e = e.to_string())
        });
        if let Some(ed) = self.edits.iter_mut().find(|x| x.copy.path == path) {
            ed.problem = problem;
        }
        cx.notify();
    }

    /// Looks at the edited copies every second while any is open.
    fn watch(&mut self, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                let go_on = this
                    .update(cx, |p, cx| {
                        let saved: Vec<u64> = p
                            .edits
                            .iter()
                            .filter(|e| {
                                matches!(e.step, EditStep::Watching(_) | EditStep::Unsent(_))
                            })
                            .filter(|e| copies::stamp(&e.copy.path) != e.written)
                            .map(|e| e.id)
                            .collect();
                        for id in saved {
                            p.write_back(id, false, cx);
                        }
                        p.watching = p.edits.iter().any(RemoteEdit::live);
                        p.watching
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        })
        .detach();
    }

    /// Writes an edited copy back, unless the server's file changed since it
    /// was read (`force`: write anyway).
    fn write_back(&mut self, id: u64, force: bool, cx: &mut Context<Self>) {
        let connected = self.sftp.is_some();
        let Some(ed) = self.edits.iter_mut().find(|x| x.id == id) else {
            return;
        };
        let (remote, local, server) = (ed.remote.clone(), ed.copy.path.clone(), ed.server);
        // The copy as it is now; a save while writing comes round again.
        let written = copies::stamp(&local);
        if !connected {
            ed.written = written;
            ed.step = EditStep::Unsent(tr!("Not connected: written back once it is."));
            return cx.notify();
        }
        ed.step = EditStep::Saving;
        cx.notify();
        self.with_sftp(
            cx,
            move |sftp| async move {
                let now = sftp.stat(&remote).await?.map(|e| (e.size, e.modified));
                if !force && now.is_some_and(|n| n != server) {
                    return Ok(Wrote::Changed);
                }
                sftp.upload(&local, &remote, &Progress::default()).await?;
                let after = sftp.stat(&remote).await?.map(|e| (e.size, e.modified));
                Ok(Wrote::Saved(after.unwrap_or(server)))
            },
            move |p, result, cx| {
                let Some(ed) = p.edits.iter_mut().find(|x| x.id == id) else {
                    return;
                };
                match result {
                    Ok(Wrote::Saved(stamp)) => {
                        ed.server = stamp;
                        ed.written = written;
                        ed.copy.sent(written);
                        ed.problem = None;
                        ed.step = EditStep::Watching(Some(
                            chrono::Local::now().format("%H:%M:%S").to_string(),
                        ));
                    }
                    Ok(Wrote::Changed) => ed.step = EditStep::Conflict,
                    Err(e) => {
                        // Tried again on the next save, or on Retry.
                        ed.written = written;
                        ed.step = EditStep::Unsent(tr!("Not written back: {e}", e = e.to_string()));
                    }
                }
                let refresh = p.dir.as_deref() == Some(&*sftp::parent(&ed.remote));
                if refresh {
                    p.refresh(cx);
                }
                cx.notify();
            },
        );
    }

    /// Stops watching an edited file and removes the copy; asks first when
    /// the copy holds changes the server lacks.
    fn stop_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(ed) = self.edits.iter_mut().find(|x| x.id == id) else {
            return;
        };
        if ed.live() && ed.copy.unsent() {
            ed.stopping = true;
            return cx.notify();
        }
        self.drop_edit(id, cx);
    }

    /// Forgets an edit, and removes its copy whatever it holds.
    fn drop_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(i) = self.edits.iter().position(|x| x.id == id) {
            self.edits.remove(i).copy.remove();
        }
        cx.notify();
    }

    /// Stops an edit, moving the copy to the Downloads folder.
    fn keep_edit(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(ed) = self.edits.iter_mut().find(|x| x.id == id) else {
            return;
        };
        match ed.copy.keep(&self.downloads) {
            Ok(path) => {
                ed.step = EditStep::Kept(path);
                ed.stopping = false;
                ed.problem = None;
            }
            Err(e) => ed.problem = Some(tr!("Cannot move it to Downloads: {e}", e = e.to_string())),
        }
        cx.notify();
    }

    /// Ends the edits as the panel goes: copies as the server has them are
    /// removed, the others left for the next start to move to Downloads.
    fn end_edits(&mut self) {
        for ed in self.edits.drain(..) {
            ed.copy.end();
        }
    }

    fn edit_row(&self, ed: &RemoteEdit, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = ed.id;
        let (status, bad) = match &ed.step {
            _ if ed.stopping => (
                tr!("not written back to the server: discard your changes?"),
                true,
            ),
            _ if ed.problem.is_some() => (ed.problem.clone().unwrap_or_default(), true),
            EditStep::Opening => (tr!("opening…"), false),
            EditStep::NotOpened(why) => (why.clone(), true),
            EditStep::Watching(None) => (tr!("in the editor; each save is written back"), false),
            EditStep::Watching(Some(t)) => (tr!("written back at {t}", t = t), false),
            EditStep::Saving => (tr!("writing back…"), false),
            EditStep::Conflict => (tr!("changed on the server since you opened it"), true),
            EditStep::Unsent(why) => (why.clone(), true),
            EditStep::Kept(path) => (
                tr!(
                    "not on the server; moved to Downloads as {name}",
                    name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                ),
                false,
            ),
        };
        let mut actions = div().flex().gap_1().flex_shrink_0();
        let open = |actions: gpui::Div, cx: &mut Context<Self>| {
            let (local, text) = (ed.copy.path.clone(), ed.text);
            actions.child(
                button(("edit-open", id), &tr!("Open"))
                    .on_click(cx.listener(move |p, _, _, cx| p.open_editor(&local, text, cx))),
            )
        };
        let stop = |actions: gpui::Div, cx: &mut Context<Self>| {
            actions.child(
                button(("edit-stop", id), &tr!("Stop"))
                    .on_click(cx.listener(move |p, _, _, cx| p.stop_edit(id, cx))),
            )
        };
        let dismiss = |actions: gpui::Div, cx: &mut Context<Self>| {
            actions.child(
                button(("edit-dismiss", id), "×")
                    .on_click(cx.listener(move |p, _, _, cx| p.drop_edit(id, cx))),
            )
        };
        actions = match &ed.step {
            _ if ed.stopping => actions
                .child(
                    button(("edit-discard", id), &tr!("Discard"))
                        .on_click(cx.listener(move |p, _, _, cx| p.drop_edit(id, cx))),
                )
                .child(
                    button(("edit-keep", id), &tr!("Keep in Downloads"))
                        .on_click(cx.listener(move |p, _, _, cx| p.keep_edit(id, cx))),
                )
                .child(
                    button(("edit-cancel", id), &tr!("Cancel")).on_click(cx.listener(
                        move |p, _, _, cx| {
                            if let Some(ed) = p.edits.iter_mut().find(|x| x.id == id) {
                                ed.stopping = false;
                            }
                            cx.notify();
                        },
                    )),
                ),
            EditStep::Opening => stop(actions, cx),
            EditStep::NotOpened(_) => dismiss(actions, cx),
            EditStep::Kept(path) => {
                let path = path.clone();
                let actions = actions.child(
                    button(("edit-show", id), &tr!("Show"))
                        .on_click(cx.listener(move |_, _, _, cx| cx.reveal_path(&path))),
                );
                dismiss(actions, cx)
            }
            EditStep::Conflict => {
                let actions = actions.child(
                    button(("edit-overwrite", id), &tr!("Overwrite"))
                        .on_click(cx.listener(move |p, _, _, cx| p.write_back(id, true, cx))),
                );
                stop(actions, cx)
            }
            EditStep::Unsent(_) => {
                let actions = actions.child(
                    button(("edit-retry", id), &tr!("Retry"))
                        .on_click(cx.listener(move |p, _, _, cx| p.write_back(id, false, cx))),
                );
                stop(open(actions, cx), cx)
            }
            EditStep::Watching(_) | EditStep::Saving => stop(open(actions, cx), cx),
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
                            .child(format!("✎ {}", ed.name)),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .text_xs()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(rgb(if bad { colors().bad } else { colors().muted }))
                    .child(status),
            )
            .into_any_element()
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
            prompt: Some(tr!("Upload").into()),
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
                Err(err) => p.fail(tr!("Not deleted: {err}", err = err.to_string()), cx),
            },
        );
    }

    /// The context menu for an entry (its actions) or the folder (new, upload).
    fn context_menu(
        &self,
        at: gpui::Point<gpui::Pixels>,
        target: MenuFor,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let item = |id: &'static str, label: &str, hint: &str, danger: bool| {
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
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().selected)))
                .text_color(rgb(if danger { colors().bad } else { colors().text }))
                .child(label.to_string())
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(colors().muted))
                        .child(hint.to_string()),
                )
        };
        let line = || div().my_1().h(px(1.)).bg(rgb(colors().line));
        // Named for the editor it opens: the chosen one, else the system's.
        let edit_label = cx
            .try_global::<EditorCommand>()
            .and_then(|c| c.0.clone())
            .filter(|c| !c.trim().is_empty())
            .map(|c| editors::name_of(&c))
            .or_else(editors::system_name)
            .map_or_else(
                || tr!("Edit in text editor"),
                |n| tr!("Edit in {editor}", editor = n),
            );
        let mut menu = div()
            .id("files-menu")
            // As wide as its longest item: the text grows with the UI font.
            .min_w(px(230.))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(colors().raised))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_sm()
            // A press on it is its own, not the list's under it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Another click anywhere closes it (a right click opens the next one).
            .on_mouse_down_out(cx.listener(|p, e: &gpui::MouseDownEvent, _, cx| {
                if e.button != MouseButton::Right {
                    p.menu = None;
                    cx.notify();
                }
            }));
        let path = match target {
            MenuFor::Entry(i) => self
                .dir
                .as_ref()
                .zip(self.entries.get(i))
                .map(|(d, e)| sftp::join(d, &e.name)),
            MenuFor::Folder => self.dir.clone(),
        };
        match target {
            MenuFor::Entry(i) => {
                let is_dir = self.entries.get(i).is_some_and(|e| e.is_dir());
                menu = if is_dir {
                    menu.child(item("menu-open", &tr!("Open"), "Enter", false).on_click(
                        cx.listener(move |p, _, _, cx| {
                            p.menu = None;
                            p.enter(i, cx);
                        }),
                    ))
                } else {
                    menu.child(
                        item("menu-edit", &edit_label, "", false).on_click(cx.listener(
                            |p, _, _, cx| {
                                p.menu = None;
                                p.edit_chosen(true, cx);
                            },
                        )),
                    )
                    .child(
                        item("menu-open-app", &tr!("Open with default app"), "", false).on_click(
                            cx.listener(|p, _, _, cx| {
                                p.menu = None;
                                p.edit_chosen(false, cx);
                            }),
                        ),
                    )
                };
                menu = menu
                    .child(item("menu-download", &tr!("Download"), "", false).on_click(
                        cx.listener(move |p, _, _, cx| {
                            p.menu = None;
                            p.download(i, cx);
                        }),
                    ))
                    .child(
                        item("menu-download-as", &tr!("Download As…"), "", false).on_click(
                            cx.listener(move |p, _, _, cx| {
                                p.menu = None;
                                p.download_as(i, cx);
                            }),
                        ),
                    )
                    .child(line());
            }
            MenuFor::Folder => {
                menu = menu
                    .child(item("menu-mkdir", &tr!("New folder…"), "", false).on_click(
                        cx.listener(|p, _, window, cx| {
                            p.menu = None;
                            p.start_edit(Edit::NewFolder, window, cx);
                        }),
                    ))
                    .child(
                        item("menu-upload", &tr!("Upload…"), "", false).on_click(cx.listener(
                            |p, _, _, cx| {
                                p.menu = None;
                                p.pick_uploads(cx);
                            },
                        )),
                    )
                    .child(
                        item("menu-refresh", &tr!("Refresh"), "", false).on_click(cx.listener(
                            |p, _, _, cx| {
                                p.menu = None;
                                p.refresh(cx);
                            },
                        )),
                    )
                    .child(line());
            }
        }
        if let Some(path) = path {
            menu = menu.child(item("menu-copy", &tr!("Copy path"), "", false).on_click(
                cx.listener(move |p, _, _, cx| {
                    p.menu = None;
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
                    cx.notify();
                }),
            ));
        }
        if let MenuFor::Entry(_) = target {
            menu =
                menu.child(item("menu-rename", &tr!("Rename…"), "F2", false).on_click(
                    cx.listener(|p, _, window, cx| {
                        p.menu = None;
                        p.start_rename(window, cx);
                    }),
                ))
                .child(line())
                .child(
                    item("menu-delete", &tr!("Delete…"), "Del", true).on_click(cx.listener(
                        |p, _, _, cx| {
                            p.menu = None;
                            p.ask_delete(cx);
                        },
                    )),
                );
        }
        gpui::deferred(
            gpui::anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if self.menu.is_some() && k.key == "escape" {
            self.menu = None;
            cx.notify();
            return cx.stop_propagation();
        }
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
        let panel = cx.entity().downgrade();
        let selected = self.selected == Some(i);
        let name = if e.is_dir() {
            format!("{}/", e.name)
        } else {
            e.name.clone()
        };
        div()
            .id(("file", i))
            .debug_selector(|| format!("file:{}", e.name))
            // The whole width: the size and date columns line up.
            .w_full()
            .h(px(ROW_H))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(selected, |d| d.bg(rgb(colors().selected)))
            .when(!selected, |d| d.hover(|s| s.bg(rgb(colors().hover))))
            // Dragged out of the window, it is fetched for the system to take.
            .on_drag(
                DragOut {
                    name: e.name.clone(),
                    dir: e.is_dir(),
                    staged: Rc::default(),
                },
                move |d: &DragOut, _, _, cx| {
                    if let Some(panel) = panel.upgrade() {
                        let staged = panel.update(cx, |p, cx| p.stage(&d.name, cx));
                        *d.staged.borrow_mut() = staged;
                    }
                    cx.new(|_| DragGhost(d.name.clone().into()))
                },
            )
            .external_drag_payload(|d: &DragOut, _, _| {
                let path = d.staged.borrow().clone()?;
                Some(ExternalDragPayload::Files(FileDragPaths::new([(
                    path, d.dir,
                )])))
            })
            // Right click: chosen, with its context menu.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |p, e: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&p.focus, cx);
                    p.selected = Some(i);
                    p.menu = Some((e.position, MenuFor::Entry(i)));
                    cx.notify();
                }),
            )
            .on_click(cx.listener(move |p, ev: &ClickEvent, window, cx| {
                window.focus(&p.focus, cx);
                p.selected = Some(i);
                if ev.click_count() >= 2 {
                    p.enter(i, cx);
                }
                cx.notify();
            }))
            .child(
                svg()
                    .path(if e.is_dir() {
                        "icons/folder.svg"
                    } else {
                        "icons/file.svg"
                    })
                    .size(px(15.))
                    .flex_shrink_0()
                    .text_color(rgb(if e.is_dir() {
                        colors().folder
                    } else {
                        colors().muted
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(rgb(if e.is_dir() {
                        colors().folder
                    } else {
                        colors().text
                    }))
                    .child(name),
            )
            .when(e.kind == sftp::Kind::Link, |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(rgb(colors().muted))
                        .child(tr!("link")),
                )
            })
            .child(
                div()
                    .w(px(58.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_right()
                    .text_color(rgb(colors().muted))
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
                    .text_color(rgb(colors().muted))
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
                    tr!(
                        "{percent}% · {rate}/s",
                        percent = done.saturating_mul(100) / total.max(1),
                        rate = human_size(rate)
                    )
                } else {
                    tr!("starting…")
                };
                actions = actions.child(
                    button(("cancel", id), &tr!("Cancel"))
                        .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                );
            }
            Step::Exists { local, .. } => {
                status = tr!("already there");
                let label = if local.is_dir() {
                    tr!("Merge")
                } else {
                    tr!("Replace")
                };
                actions = actions
                    .child(
                        button(("replace", id), &label)
                            .on_click(cx.listener(move |p, _, _, cx| p.replace(id, cx))),
                    )
                    .child(
                        button(("skip", id), &tr!("Skip"))
                            .on_click(cx.listener(move |p, _, _, cx| p.dismiss(id, cx))),
                    );
            }
            Step::Done(local) => {
                status = tr!("done · {size}", size = human_size(total));
                if let Some(path) = local.clone() {
                    actions = actions.child(
                        button(("show", id), &tr!("Show"))
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
                    .text_color(rgb(if failed { colors().bad } else { colors().muted }))
                    .child(status),
            )
            .when(matches!(t.step, Step::Running), |d| {
                d.child(
                    div()
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(rgb(colors().line))
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(rgb(colors().progress))
                                .w(relative(fraction.clamp(0., 1.))),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The title and the folder's tools as icons; the rest is in the
        // context menu (right click) and on the keys.
        let tool = |id: &'static str, icon: &'static str, tip: SharedString| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)))
                .tooltip(move |_, cx| {
                    cx.new(|_| Tip {
                        title: tip.clone(),
                        detail: SharedString::default(),
                    })
                    .into()
                })
                .child(
                    svg()
                        .path(icon)
                        .size(px(16.))
                        .text_color(rgb(colors().text)),
                )
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .h(px(40.))
            .pl_3()
            .pr_1()
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("Files")),
            )
            .child(
                tool("files-home", "icons/home.svg", tr!("Home").into())
                    .on_click(cx.listener(|p, _, _, cx| p.go_home(cx))),
            )
            .child(
                tool("files-refresh", "icons/refresh.svg", tr!("Refresh").into())
                    .on_click(cx.listener(|p, _, _, cx| p.refresh(cx))),
            )
            .child(
                tool(
                    "files-mkdir",
                    "icons/folder-plus.svg",
                    tr!("New folder").into(),
                )
                .on_click(
                    cx.listener(|p, _, window, cx| p.start_edit(Edit::NewFolder, window, cx)),
                ),
            )
            .child(
                tool("files-upload", "icons/upload.svg", tr!("Upload…").into())
                    .on_click(cx.listener(|p, _, _, cx| p.pick_uploads(cx))),
            )
            .child(div().w(px(6.)))
            .child(
                button("files-close", "×")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FilesEvent::Close))),
            );
        let tools = div()
            .px_3()
            .py(px(6.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(colors().muted))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(self.shown_path()),
            );

        // Typing a name, or confirming a deletion, above the list.
        let mut asking = div().flex().flex_col();
        if let Some(edit) = &self.editing {
            let label = match edit {
                Edit::NewFolder => tr!("New folder"),
                Edit::Rename(old) => tr!("Rename {old}", old = old),
            };
            asking = asking.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(div().text_xs().text_color(rgb(colors().muted)).child(label))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(colors().field_edge))
                            .bg(rgb(colors().field))
                            .child(self.input.clone()),
                    )
                    .child(match &self.edit_problem {
                        Some(p) => div()
                            .text_xs()
                            .text_color(rgb(colors().bad))
                            .child(p.clone()),
                        None => div()
                            .text_xs()
                            .text_color(rgb(colors().muted))
                            .child(tr!("Enter to save · Esc to cancel")),
                    }),
            );
        }
        if let Some(e) = &self.deleting {
            let question = if e.kind == sftp::Kind::Dir {
                tr!(
                    "Delete the folder {name} and everything in it?",
                    name = &e.name
                )
            } else {
                tr!("Delete {name}?", name = &e.name)
            };
            asking = asking.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(div().text_xs().child(question))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                button_in(
                                    "files-delete-yes",
                                    &tr!("Delete"),
                                    rgb(colors().bad_edge),
                                    rgb(colors().bad_hover),
                                )
                                .on_click(cx.listener(|p, _, _, cx| p.confirm_delete(cx))),
                            )
                            .child(button("files-delete-no", &tr!("Cancel")).on_click(
                                cx.listener(|p, _, _, cx| {
                                    p.deleting = None;
                                    cx.notify();
                                }),
                            )),
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
        // A right click beside the entries: the folder's own menu.
        let mut body = div()
            .id("files-body")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|p, e: &gpui::MouseDownEvent, _, cx| {
                    if p.dir.is_some() {
                        p.menu = Some((e.position, MenuFor::Folder));
                        cx.notify();
                    }
                }),
            );
        match &self.state {
            State::Opening => body = body.child(note(tr!("Opening SFTP…"), colors().muted)),
            State::Loading if self.entries.is_empty() => {
                body = body.child(note(tr!("Loading…"), colors().muted))
            }
            State::Failed(why) => {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pr_3()
                        .child(note(why.to_string(), colors().bad).flex_1())
                        .child(
                            button("files-retry", &tr!("Try again"))
                                .on_click(cx.listener(|p, _, _, cx| p.refresh(cx))),
                        ),
                )
            }
            _ => {}
        }
        // The folder above, as a first row that stays while the list scrolls.
        if self.dir.as_deref().is_some_and(|d| d != "/") {
            body = body.child(
                div()
                    .id("files-parent")
                    .debug_selector(|| "files-parent".into())
                    .w_full()
                    .h(px(ROW_H))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(|p, ev: &ClickEvent, window, cx| {
                        window.focus(&p.focus, cx);
                        if ev.click_count() >= 2 {
                            p.up(cx);
                        }
                    }))
                    .child(
                        svg()
                            .path("icons/folder.svg")
                            .size(px(15.))
                            .flex_shrink_0()
                            .text_color(rgb(colors().folder)),
                    )
                    .child(div().flex_1().text_color(rgb(colors().folder)).child(".."))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(colors().muted))
                            .child("Backspace"),
                    ),
            );
        }
        if self.dir.is_some() && self.entries.is_empty() && self.state == State::Ready {
            body = body.child(note(tr!("Empty folder."), colors().muted));
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
            .border_color(rgb(colors().line))
            .py_1();
        for ed in &self.edits {
            transfers = transfers.child(self.edit_row(ed, cx));
        }
        if self.transfers.is_empty() && self.edits.is_empty() {
            transfers = transfers.child(note(
                tr!(
                    "Right-click a file or folder for its actions; drop files here to upload them."
                ),
                colors().muted,
            ));
        }
        for t in &self.transfers {
            transfers = transfers.child(self.transfer_row(t, cx));
        }

        div()
            .id("files")
            .debug_selector(|| "files-panel".into())
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
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgb(colors().drop)))
            .size_full()
            .flex()
            .flex_col()
            // The ground is the pane's, which rounds the window's corner.
            .border_l_1()
            .border_color(rgb(colors().line))
            .text_sm()
            .text_color(rgb(colors().text))
            .when_some(self.menu, |d, (at, target)| {
                d.child(self.context_menu(at, target, cx))
            })
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

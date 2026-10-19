//! The window: tabs in the title bar, the host list on the left, the active
//! tab's terminal, and what concerns every tab (settings, the window's place).

use std::sync::Arc;
use std::time::Duration;

use std::rc::Rc;

use gpui::{
    AnyElement, Axis, Context, CursorStyle, Div, ElementId, Entity, FocusHandle, Focusable, Hsla,
    KeyDownEvent, MouseButton, MouseMoveEvent, PromptLevel, Render, SharedString, Subscription,
    Window, canvas, div, prelude::*, px, relative, rgb, rgba, svg,
};
use shellwing_config::{EditFile, Host, PaneState, SessionState, TabState, WindowState};
use shellwing_ssh::config as ssh_config;
use tokio::runtime::Handle;

use crate::bench::Probe;
use crate::chrome::{self, TITLEBAR_H};
use crate::colors::colors;
use crate::files::EditorCommand;
use crate::group_editor::{GroupEditor, GroupEvent};
use crate::host_editor::{self, EditorContext, EditorEvent, HostEditor};
use crate::look::{self, Live, rgb as theme_rgb};
use crate::palette::{Action, Entry, Kind, Palette, PaletteEvent};
use crate::pane::{Dir, Pane, SplitState, neighbor};
use crate::secrets::VaultSlot;
use crate::session::{Opens, Session, Status};
use crate::settings_view::{self, Page, SettingsEvent, SettingsView, Snap};
use crate::sidebar::{self, Row, Section, Sidebar, dot};
use crate::snippets;
use crate::syncing::{SyncEvent, SyncWorker};
use crate::target;
use crate::view::{
    CloseRequested, FilesResized, ListenToggled, PaneCommand, StatusChanged, TermView, UseKey,
    VaultChanged, hsla,
};
use shellwing_vault::{Folder, FolderState};

/// The window's size and place are saved this long after they stop changing.
const SAVE_BOUNDS: Duration = Duration::from_millis(400);
const CONFIG_NOTICE: Duration = Duration::from_secs(8);
/// How long the name of a newly chosen theme stays on screen.
const THEME_NOTICE: Duration = Duration::from_secs(3);
/// Edited files moved to Downloads: worth longer than other notices.
const KEPT_NOTICE: Duration = Duration::from_secs(20);

/// Keyboard shortcuts that concern the whole window.
#[derive(Debug, PartialEq, Eq)]
enum Shortcut {
    Palette,
    NewTab,
    /// The focused pane, or the tab with its last pane.
    Close,
    Split(Axis),
    Focus(Dir),
    Step(isize),
    Goto(usize),
    Sidebar,
    /// The focused pane's Files panel.
    Files,
    /// Find text in the focused pane.
    Find,
    /// Type in every pane of the tab, or stop.
    Broadcast,
    Settings,
    Theme(isize),
    /// Cmd+Q on macOS.
    Quit,
}

/// A tab: its panes, and which one has the keyboard.
struct Tab {
    root: Option<Pane<Entity<TermView>>>,
    focused: Entity<TermView>,
    /// What is typed in one pane goes to every pane that listens.
    broadcast: bool,
    /// Panes left out of that.
    left_out: Vec<gpui::EntityId>,
}

impl Tab {
    fn views(&self) -> Vec<Entity<TermView>> {
        self.root.as_ref().map(Pane::leaves).unwrap_or_default()
    }
}

pub struct Workspace {
    focus: FocusHandle,
    tabs: Vec<Tab>,
    active: usize,
    sidebar: Entity<Sidebar>,
    show_sidebar: bool,
    /// The host list's width when expanded, as dragged.
    sidebar_width: f32,
    /// Folded to a rail of host tiles.
    sidebar_collapsed: bool,
    /// Dragging the host list's edge: where the drag began, and the width then.
    sidebar_drag: Option<(f32, f32)>,
    live: Live,
    /// Hosts named in `~/.ssh/config`, for the sidebar.
    ssh_aliases: Vec<String>,
    rt: Handle,
    /// `$SHELLWING_BENCH`: measured on the first tab opened.
    bench: Option<(String, Arc<Probe>)>,
    /// Shown in place of the terminal while no tab is open.
    notice: Option<SharedString>,
    /// A quit or close question is showing (GPUI allows one prompt at a time).
    prompting: bool,
    /// The open palette, and whether it is picking a theme (whose preview must
    /// be undone if nothing is picked).
    palette: Option<(Entity<Palette>, Subscription, bool)>,
    /// The split whose divider is being dragged.
    dragging: Option<Rc<SplitState>>,
    /// The window's size and place, waiting to be saved, and which change it is.
    unsaved_bounds: Option<WindowState>,
    bounds_change: u64,
    /// Counts changes to the tabs, to save them shortly after the last one.
    layout_change: u64,
    vault: VaultSlot,
    /// A vault that failed to open was reported.
    vault_reported: bool,
    sync: SyncWorker,
    /// The last sync error shown, so it is not repeated every minute.
    sync_error: Option<String>,
    /// What the palette's question is for, while one is asked.
    asking: Option<Asking>,
    /// The host editor drawer, while open.
    editor: Option<(Entity<HostEditor>, Subscription)>,
    /// The group editor drawer, while open.
    group_editor: Option<(Entity<GroupEditor>, Subscription)>,
    /// The settings page, while open, and the fonts it offers.
    settings: Option<(Entity<SettingsView>, Subscription)>,
    /// Code fonts, the default terminal font, and every installed font.
    fonts: (Vec<String>, String, Vec<String>),
    /// Text editors found on this system, looked for when Settings opens.
    editors: Vec<(String, String)>,
    /// A key waiting for the palette's choice of a host to install it on.
    choosing: Option<KeyToInstall>,
    /// Keys to install on panes once they are connected.
    installs: Vec<(gpui::WeakEntity<TermView>, KeyToInstall)>,
}

/// A public key to put in a host's `authorized_keys`.
#[derive(Clone, Debug)]
pub struct KeyToInstall {
    /// How the key is named to the user.
    pub name: String,
    /// One line, as `authorized_keys` takes it.
    pub public: String,
    /// A key the vault keeps: the host can be set to sign in with it after.
    pub vault_id: Option<String>,
}

/// What the host editor opens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTarget {
    New,
    /// A host in hosts.toml, by name.
    Saved(String),
    /// A host from `~/.ssh/config`, by alias: shown read only.
    Ssh(String),
    /// This computer: its shell is a setting.
    Local,
}

#[derive(Clone)]
enum Asking {
    /// A folder holding another vault, waiting for its recovery key.
    RecoveryKey(std::path::PathBuf),
    /// A port to forward on the active tab's connection.
    Forward,
    /// A value a snippet needs before it runs.
    Snippet(SnippetRun),
}

/// A snippet on its way to running.
#[derive(Clone)]
struct SnippetRun {
    snippet: snippets::Snippet,
    /// In every pane of the tab (that listens), not only the focused one.
    all: bool,
    values: std::collections::BTreeMap<String, String>,
    /// The names still to ask for.
    left: Vec<String>,
}

/// What `state/session.json` keeps of a pane tree: hosts and splits.
fn pane_state(pane: &Pane<Entity<TermView>>, cx: &gpui::App) -> PaneState {
    match pane {
        Pane::Leaf(v) => PaneState::Leaf {
            spec: v.read(cx).session().spec.clone(),
        },
        Pane::Split(s, a, b) => PaneState::Split {
            horizontal: s.axis == Axis::Horizontal,
            ratio: s.ratio.get(),
            first: Box::new(pane_state(a, cx)),
            second: Box::new(pane_state(b, cx)),
        },
    }
}

impl Workspace {
    pub fn new(
        live: Live,
        problems: Vec<String>,
        rt: Handle,
        bench: Option<(String, Arc<Probe>)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let vault = match live.store() {
            Some(store) => VaultSlot::open(store.dir().join("vault.db")),
            None => VaultSlot::none(),
        };
        let (sync_tx, sync_rx) = async_channel::unbounded();
        let sync = SyncWorker::start(vault.clone(), live.store().cloned(), sync_tx);
        sync.set_folder(live.sync_folder());
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = sync_rx.recv().await {
                if this.update(cx, |ws, cx| ws.on_sync(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut ws = Workspace {
            focus: cx.focus_handle(),
            tabs: vec![],
            active: 0,
            sidebar: {
                let me = cx.weak_entity();
                cx.new(|cx| Sidebar::new(me, cx))
            },
            show_sidebar: true,
            sidebar_width: sidebar::WIDTH,
            sidebar_collapsed: false,
            sidebar_drag: None,
            live,
            ssh_aliases: ssh_config::aliases(ssh_config::default_config_path().as_deref()),
            rt,
            bench,
            notice: None,
            prompting: false,
            palette: None,
            dragging: None,
            unsaved_bounds: None,
            bounds_change: 0,
            layout_change: 0,
            vault,
            vault_reported: false,
            sync,
            sync_error: None,
            asking: None,
            editor: None,
            group_editor: None,
            settings: None,
            fonts: (vec![], String::new(), vec![]),
            editors: vec![],
            choosing: None,
            installs: vec![],
        };
        if let Some(s) = ws.live.store().and_then(|s| s.window_state()) {
            ws.sidebar_width = s
                .sidebar_width
                .unwrap_or(sidebar::WIDTH)
                .clamp(sidebar::MIN_WIDTH, sidebar::MAX_WIDTH);
            ws.sidebar_collapsed = s.sidebar_collapsed;
            if let Some(w) = s.files_width {
                cx.set_global(crate::files::PanelWidth(
                    w.clamp(crate::files::MIN_WIDTH, crate::files::MAX_WIDTH),
                ));
            }
        }
        ws.share_settings(cx);
        // The system editor's name is read from the desktop: once, off the UI thread.
        std::thread::spawn(crate::editors::system_name);
        // Edited copies an earlier run left behind: those holding changes
        // the server lacks go to Downloads, and the user is told.
        cx.spawn(async move |this, cx| {
            let into = crate::files::downloads_dir();
            let kept = cx
                .background_executor()
                .spawn(async move { crate::copies::sweep(&into) })
                .await;
            if let Some(msg) = crate::copies::kept_notice(&kept) {
                let _ = this.update(cx, |ws, cx| ws.say(msg, KEPT_NOTICE, cx));
            }
        })
        .detach();
        ws.report(problems, cx);
        ws.refresh_sidebar(cx);
        ws
    }

    /// The rail's search button: the palette, to find a host by typing.
    pub fn open_palette_from_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(window, cx);
    }

    /// Folds the host list to a rail of host tiles, or unfolds it.
    pub fn set_sidebar_collapsed(
        &mut self,
        collapsed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_collapsed = collapsed;
        self.show_sidebar = true;
        self.refresh_sidebar(cx);
        self.save_sidebar(window, cx);
    }

    /// Remembers the host list's width and fold with the window's place.
    fn save_sidebar(&mut self, window: &Window, cx: &gpui::App) {
        let mut state = crate::placement::state(window.window_bounds());
        state.sidebar_width = Some(self.sidebar_width);
        state.sidebar_collapsed = self.sidebar_collapsed;
        state.files_width = cx.try_global::<crate::files::PanelWidth>().map(|w| w.0);
        if let Some(store) = self.live.store()
            && let Err(e) = store.save_window_state(&state)
        {
            eprintln!("cannot save the window layout: {e}");
        }
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The focused pane of the active tab.
    fn active_tab(&self) -> Option<&Entity<TermView>> {
        self.tabs.get(self.active).map(|t| &t.focused)
    }

    /// Every pane of every tab.
    fn all_views(&self) -> Vec<Entity<TermView>> {
        self.tabs.iter().flat_map(Tab::views).collect()
    }

    /// Opens a tab connected to `spec`: a hosts.toml name, an ssh config alias or
    /// `[user@]host[:port]`.
    pub fn open(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_forwarding(spec, &[], window, cx);
    }

    fn open_forwarding(
        &mut self,
        spec: &str,
        forwards: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.make_view_forwarding(spec, forwards, window, cx) {
            self.tabs.push(Tab {
                root: Some(Pane::Leaf(view.clone())),
                focused: view,
                broadcast: false,
                left_out: vec![],
            });
            self.activate(self.tabs.len() - 1, window, cx);
            self.refresh_sidebar(cx);
        }
    }

    /// Splits the focused pane, the new pane connected to the same host.
    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = self.active_tab().cloned() else {
            return;
        };
        let spec = focused.read(cx).session().spec.clone();
        let Some(view) = self.make_view(&spec, window, cx) else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.root = tab
                .root
                .take()
                .map(|r| r.with_split(&focused, view.clone(), axis));
            tab.focused = view;
        }
        self.refresh_broadcast(self.active, cx);
        self.activate(self.active, window, cx);
        self.refresh_sidebar(cx);
    }

    /// The panes a snippet goes to: the focused one, or (`all`) every pane of
    /// the tab that listens.
    fn snippet_targets(&self, all: bool) -> Vec<Entity<TermView>> {
        let Some(tab) = self.tabs.get(self.active) else {
            return vec![];
        };
        if !all {
            return vec![tab.focused.clone()];
        }
        tab.views()
            .into_iter()
            .filter(|v| !(tab.broadcast && tab.left_out.contains(&v.entity_id())))
            .collect()
    }

    /// The snippets the palette offers for the focused pane's host.
    fn snippet_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let Some(view) = self.active_tab() else {
            return vec![];
        };
        let s = view.read(cx).session();
        let host = s.entry.clone().unwrap_or_else(|| s.spec.clone());
        let group = s
            .entry
            .as_deref()
            .and_then(|n| self.live.hosts().find(n))
            .and_then(|h| h.group.clone());
        let panes = self.snippet_targets(true).len();
        self.vault
            .ready()
            .and_then(|v| v.snippets().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|sn| snippets::Scope::parse(&sn.scope).fits(Some(&host), group.as_deref()))
            .map(|sn| {
                Entry::new(Kind::Snippet(sn.id.clone()), "Snippets", sn.name.clone())
                    .detail(sn.command.clone())
                    .hint(if panes > 1 {
                        format!("Enter · Alt+Enter: {panes} panes")
                    } else {
                        "Enter".to_string()
                    })
            })
            .collect()
    }

    fn run_snippet(&mut self, id: &str, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snippet) = self
            .vault
            .ready()
            .and_then(|v| v.snippets().ok())
            .and_then(|l| l.into_iter().find(|s| s.id == id))
        else {
            return;
        };
        let left = snippets::asked(&snippet.command);
        self.ask_snippet(
            SnippetRun {
                snippet,
                all,
                values: Default::default(),
                left,
            },
            window,
            cx,
        );
    }

    /// Asks for the next value the snippet needs, or, with none left, types
    /// it into its panes.
    fn ask_snippet(&mut self, run: SnippetRun, window: &mut Window, cx: &mut Context<Self>) {
        let targets = self.snippet_targets(run.all);
        let Some(name) = run.left.first().cloned() else {
            self.asking = None;
            for v in targets {
                let text = {
                    let s = v.read(cx).session();
                    let mut values = run.values.clone();
                    values.insert(
                        "host".into(),
                        s.entry.clone().unwrap_or_else(|| s.title.clone()),
                    );
                    if let Some(t) = s.reached() {
                        values.insert("user".into(), t.user.clone());
                        values.insert("address".into(), t.host.clone());
                    }
                    snippets::fill(&run.snippet.command, &values)
                };
                v.update(cx, |v, _| v.type_text(&text, run.snippet.enter));
            }
            return;
        };
        let total = snippets::asked(&run.snippet.command).len();
        let at = total - run.left.len() + 1;
        let panes = match targets.len() {
            1 => "in this pane".to_string(),
            n => format!("in {n} panes"),
        };
        let notes = vec![
            Entry::new(
                Kind::Note,
                "",
                format!("{} · {panes} · {name} ({at} of {total})", run.snippet.name),
            ),
            Entry::new(Kind::Note, "", "Will run")
                .detail(snippets::fill(&run.snippet.command, &run.values)),
        ];
        self.asking = Some(Asking::Snippet(run));
        let palette = cx.new(|cx| Palette::ask(&name, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// Types in every pane of the active tab, or stops.
    fn toggle_broadcast(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if !tab.broadcast && tab.views().len() < 2 {
            return self.say(
                "Split the tab first: there is one pane to type in.".into(),
                CONFIG_NOTICE,
                cx,
            );
        }
        tab.broadcast = !tab.broadcast;
        tab.left_out.clear();
        self.refresh_broadcast(self.active, cx);
        cx.notify();
    }

    /// Hands each pane of tab `ix` whether it listens, and the terminals
    /// what is typed in it goes to as well.
    fn refresh_broadcast(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let views = tab.views();
        // Panes gone from the tab are forgotten.
        tab.left_out
            .retain(|id| views.iter().any(|v| v.entity_id() == *id));
        let (on, left_out) = (tab.broadcast, tab.left_out.clone());
        let handles: Vec<(gpui::EntityId, shellwing_term::TermHandle)> = views
            .iter()
            .filter(|v| !left_out.contains(&v.entity_id()))
            .map(|v| (v.entity_id(), v.read(cx).term_handle()))
            .collect();
        for v in &views {
            let id = v.entity_id();
            let (state, echo) = if !on {
                (None, vec![])
            } else if left_out.contains(&id) {
                (Some(false), vec![])
            } else {
                let others = handles
                    .iter()
                    .filter(|(o, _)| *o != id)
                    .map(|(_, h)| h.clone())
                    .collect();
                (Some(true), others)
            };
            v.update(cx, |v, cx| v.set_broadcast(state, echo, cx));
        }
    }

    /// A pane's badge: leave it out of typing in every pane, or bring it back.
    fn toggle_listen(&mut self, view: &Entity<TermView>, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.views().contains(view)) else {
            return;
        };
        let id = view.entity_id();
        let tab = &mut self.tabs[ix];
        if let Some(at) = tab.left_out.iter().position(|x| *x == id) {
            tab.left_out.remove(at);
        } else {
            tab.left_out.push(id);
        }
        self.refresh_broadcast(ix, cx);
        cx.notify();
    }

    /// Closes the focused pane; the last one closes its tab.
    fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.active_tab().cloned() {
            self.request_close_view(&view, window, cx);
        }
    }

    /// Closes the tab at `ix`, asking first while any of its panes is connected.
    fn request_close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        // Found again by a pane on Close: tabs may have moved meanwhile.
        let (views, key) = (tab.views(), tab.focused.clone());
        self.confirm_close(
            &views,
            "Close the tab?",
            window,
            cx,
            move |ws, window, cx| {
                if let Some(ix) = ws.tabs.iter().position(|t| t.views().contains(&key)) {
                    ws.close(ix, window, cx);
                }
            },
        );
    }

    /// Closes the pane showing `view`, asking first while it is connected.
    fn request_close_view(
        &mut self,
        view: &Entity<TermView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let last = self
            .tabs
            .iter()
            .find(|t| t.views().contains(view))
            .is_some_and(|t| t.views().len() < 2);
        let question = if last {
            "Close the tab?"
        } else {
            "Close the pane?"
        };
        let v = view.clone();
        self.confirm_close(
            std::slice::from_ref(view),
            question,
            window,
            cx,
            move |ws, window, cx| ws.close_view(&v, window, cx),
        );
    }

    /// Runs `close` at once if none of `views` is connected; else asks, and runs
    /// it on Close.
    fn confirm_close(
        &mut self,
        views: &[Entity<TermView>],
        question: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        close: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let open: Vec<String> = views
            .iter()
            .map(|v| v.read(cx).session())
            .filter(|s| s.status() == Status::Connected)
            .map(|s| s.title.clone())
            .collect();
        if open.is_empty() {
            return close(self, window, cx);
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let detail = match open.as_slice() {
            [one] => format!("The connection to {one} is open; closing ends it."),
            _ => format!("{} connections are open; closing ends them.", open.len()),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            question,
            Some(&detail),
            &["Close", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let yes = matches!(answer.await, Ok(0));
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.prompting = false;
                if yes {
                    close(ws, window, cx);
                }
            });
        })
        .detach();
    }

    /// Closes the pane showing `view`, wherever it is; the last pane of a tab
    /// closes the tab.
    fn close_view(&mut self, view: &Entity<TermView>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.views().contains(view)) else {
            return;
        };
        let tab = &mut self.tabs[ix];
        if tab.views().len() < 2 {
            return self.close(ix, window, cx);
        }
        tab.root = tab.root.take().and_then(|r| r.without(view));
        if &tab.focused == view
            && let Some(r) = &tab.root
        {
            tab.focused = r.first();
        }
        // One pane left: nothing to type in every pane of.
        if tab.views().len() < 2 {
            tab.broadcast = false;
        }
        self.refresh_broadcast(ix, cx);
        if ix == self.active {
            self.activate(ix, window, cx);
        }
        self.refresh_sidebar(cx);
    }

    /// Moves the keyboard to the pane beside the focused one; false if there is
    /// no other pane, so the key can go to the terminal instead.
    fn focus_pane(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        let views = tab.views();
        if views.len() < 2 {
            return false;
        }
        let from = tab.focused.read(cx).bounds();
        let others: Vec<_> = views
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != tab.focused)
            .map(|(i, v)| (i, v.read(cx).bounds()))
            .collect();
        if let Some(i) = neighbor(from, &others, dir) {
            self.set_focused(views[i].clone(), window, cx);
        }
        true
    }

    fn set_focused(&mut self, view: Entity<TermView>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.focused != view
        {
            tab.focused = view;
            self.activate(self.active, window, cx);
        }
    }

    /// A terminal connected to `spec`, ready for a tab or a split.
    fn make_view(
        &mut self,
        spec: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TermView>> {
        self.make_view_forwarding(spec, &[], window, cx)
    }

    /// As [`Self::make_view`], also forwarding `forwards` (from the command line).
    fn make_view_forwarding(
        &mut self,
        spec: &str,
        forwards: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TermView>> {
        let local = spec == crate::local::SPEC;
        let opens = if local {
            Opens::Local(crate::local::shell(
                self.live.settings().terminal.local_shell.as_deref(),
            ))
        } else {
            Opens::Host(Box::new(
                target::resolve(
                    spec,
                    self.live.hosts(),
                    ssh_config::default_config_path().as_deref(),
                )
                .map(|(mut r, h)| {
                    r.forwards.extend_from_slice(forwards);
                    (r, h)
                }),
            ))
        };
        let entry = match &opens {
            Opens::Host(t) => (**t)
                .as_ref()
                .ok()
                .and_then(|(_, h)| h.as_ref().map(|h| h.name.clone())),
            Opens::Local(_) => None,
        };
        let (look, problem) = self.live.look_for(entry.as_deref());
        let bench = self.bench.take();
        let probe = bench
            .as_ref()
            .map_or_else(|| Arc::new(Probe::new()), |(_, p)| p.clone());
        let session = match Session::start(
            spec,
            opens,
            self.live.settings(),
            &self.rt,
            bench,
            self.vault.clone(),
        ) {
            Ok(s) => s,
            Err(e) => {
                self.notice = Some(format!("Cannot open a terminal: {e}").into());
                cx.notify();
                return None;
            }
        };
        // A local shell is one of the host list's own, not a recent host.
        if !local
            && let Some(store) = self.live.store()
            && let Err(e) = store.add_recent(spec)
        {
            eprintln!("cannot save the recent hosts: {e}");
        }
        let view = cx.new(|cx| TermView::new(session, look, probe, cx));
        // Tab and host list dots follow the session's status; a key waiting
        // for the connection is installed once it is up.
        cx.subscribe(&view, |ws, _, _: &StatusChanged, cx| {
            ws.refresh_sidebar(cx);
            ws.run_installs(cx);
        })
        .detach();
        cx.subscribe(&view, |ws, _, ev: &UseKey, cx| ws.use_key(ev, cx))
            .detach();
        cx.subscribe(&view, |ws, view, _: &ListenToggled, cx| {
            ws.toggle_listen(&view, cx)
        })
        .detach();
        cx.subscribe(&view, |ws, _, _: &VaultChanged, _| ws.sync.now())
            .detach();
        cx.subscribe_in(&view, window, |ws, _, _: &FilesResized, window, cx| {
            ws.save_sidebar(window, cx);
            // Every pane's panel takes the new width.
            for v in ws.all_views() {
                v.update(cx, |v, cx| v.refresh_panels(cx));
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&view, window, |ws, view, c: &PaneCommand, window, cx| {
            // The menu's pane has the keyboard already; make it the tab's too.
            ws.set_focused(view.clone(), window, cx);
            match *c {
                PaneCommand::Palette => ws.open_palette(window, cx),
                PaneCommand::Split(axis) => ws.split(axis, window, cx),
                PaneCommand::Forward => ws.ask_forward(None, window, cx),
                PaneCommand::Log => ws.toggle_log(cx),
                PaneCommand::Close => ws.request_close_view(view, window, cx),
            }
        })
        .detach();
        cx.subscribe_in(&view, window, |ws, view, _: &CloseRequested, window, cx| {
            ws.close_view(view, window, cx)
        })
        .detach();
        if let Some(p) = problem {
            view.update(cx, |v, cx| v.show_notice(p, CONFIG_NOTICE, cx));
        }
        Some(view)
    }

    /// Opens `spec` if given, with `forwards` from the command line; otherwise
    /// the last run's tabs, if the settings say so.
    pub fn start(
        &mut self,
        spec: Option<&str>,
        forwards: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match spec {
            Some(spec) => self.open_forwarding(spec, forwards, window, cx),
            None if self.live.restore_tabs() => self.restore(window, cx),
            None => {}
        }
        cx.on_release(|ws, cx| ws.save_layout(cx)).detach();
    }

    /// Opens the tabs and panes saved in `state/session.json`.
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.live.store().and_then(|s| s.session_state()) else {
            return;
        };
        for tab in &saved.tabs {
            if let Some(root) = self.build_pane(&tab.root, window, cx) {
                let views = root.leaves();
                let focused = views
                    .get(tab.focused)
                    .cloned()
                    .unwrap_or_else(|| root.first());
                self.tabs.push(Tab {
                    root: Some(root),
                    focused,
                    broadcast: false,
                    left_out: vec![],
                });
            }
        }
        let active = saved.active.min(self.tabs.len().saturating_sub(1));
        self.activate(active, window, cx);
        self.refresh_sidebar(cx);
    }

    fn build_pane(
        &mut self,
        saved: &PaneState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane<Entity<TermView>>> {
        match saved {
            PaneState::Leaf { spec } => self.make_view(spec, window, cx).map(Pane::Leaf),
            PaneState::Split {
                horizontal,
                ratio,
                first,
                second,
            } => {
                let a = self.build_pane(first, window, cx);
                let b = self.build_pane(second, window, cx);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let s = SplitState::new(if *horizontal {
                            Axis::Horizontal
                        } else {
                            Axis::Vertical
                        });
                        s.ratio.set(ratio.clamp(0.1, 0.9));
                        Some(Pane::Split(s, Box::new(a), Box::new(b)))
                    }
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    fn snapshot(&self, cx: &gpui::App) -> SessionState {
        SessionState {
            tabs: self
                .tabs
                .iter()
                .filter_map(|t| {
                    let root = t.root.as_ref()?;
                    Some(TabState {
                        root: pane_state(root, cx),
                        focused: root
                            .leaves()
                            .iter()
                            .position(|v| *v == t.focused)
                            .unwrap_or(0),
                    })
                })
                .collect(),
            active: self.active,
        }
    }

    fn save_layout(&self, cx: &gpui::App) {
        if let Some(store) = self.live.store()
            && let Err(e) = store.save_session_state(&self.snapshot(cx))
        {
            eprintln!("cannot save the open tabs: {e}");
        }
    }

    /// Saves the tabs shortly after they stop changing.
    fn layout_changed(&mut self, cx: &mut Context<Self>) {
        self.layout_change += 1;
        let change = self.layout_change;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_BOUNDS).await;
            let _ = this.update(cx, |ws, cx| {
                if ws.layout_change == change {
                    ws.save_layout(cx);
                }
            });
        })
        .detach();
    }

    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_changed(cx);
        if let Some(tab) = self.tabs.get(ix).map(|t| t.focused.clone()) {
            self.active = ix;
            let title = tab.read(cx).session().title.clone();
            window.set_window_title(&format!("{title} — shellwing"));
            window.focus(&tab.focus_handle(cx), cx);
        } else {
            window.set_window_title("shellwing");
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        // Dropping the view drops its session, which disconnects.
        self.tabs.remove(ix);
        if self.active > ix || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.activate(self.active, window, cx);
        self.refresh_sidebar(cx);
    }

    /// Connections open now, in every tab and pane.
    fn connections(&self, cx: &gpui::App) -> usize {
        self.tabs
            .iter()
            .flat_map(Tab::views)
            .filter(|v| v.read(cx).session().status() == Status::Connected)
            .count()
    }

    /// Whether quitting, or closing the window (which quits), may go ahead now.
    /// While connections are open it asks instead, quits if told to, and says no.
    pub fn may_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let open = self.connections(cx);
        if open == 0 {
            return true;
        }
        if !self.prompting {
            self.prompting = true;
            let detail = if open == 1 {
                "A connection is open; quitting closes it.".to_string()
            } else {
                format!("{open} connections are open; quitting closes them.")
            };
            let answer = window.prompt(
                PromptLevel::Warning,
                "Quit shellwing?",
                Some(&detail),
                &["Quit", "Cancel"],
                cx,
            );
            cx.spawn(async move |this, cx| {
                let quit = matches!(answer.await, Ok(0));
                let _ = this.update(cx, |ws, cx| {
                    ws.prompting = false;
                    if quit {
                        cx.quit();
                    }
                });
            })
            .detach();
        }
        false
    }

    /// Closing the window quits, so the system's close button asks first too.
    pub fn guard_close(&self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            ws.update(cx, |ws, cx| ws.may_quit(window, cx))
                .unwrap_or(true)
        });
    }

    fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            let ix = (self.active as isize + by).rem_euclid(n) as usize;
            self.activate(ix, window, cx);
        }
    }

    /// Opens another tab to the active tab's host.
    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_tab().map(|t| t.read(cx).session().spec.clone()) {
            Some(spec) => self.open(&spec, window, cx),
            None => {
                self.notice = Some("Choose a host on the left to connect.".into());
                cx.notify();
            }
        }
    }

    /// What the palette offers: hosts, recent connections, then actions.
    fn palette_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let mut entries = vec![];
        let mut specs = vec![];
        for section in self.sidebar_sections(cx) {
            for r in section.rows {
                specs.push(r.spec.clone());
                entries.push(
                    Entry::new(Kind::Host(r.spec), "Hosts", r.name)
                        .detail(if r.detail.is_empty() {
                            section.title.clone()
                        } else {
                            format!("{} · {}", r.detail, section.title).into()
                        })
                        .status(r.status)
                        .hint("Enter"),
                );
            }
        }
        for spec in self.live.store().map(|s| s.recent()).unwrap_or_default() {
            if !specs.contains(&spec) {
                entries.push(Entry::new(Kind::Recent(spec.clone()), "Recent", spec).hint("Enter"));
            }
        }
        entries.extend(self.snippet_entries(cx));
        let (chord, alt) = if cfg!(target_os = "macos") {
            ("Cmd", "Cmd")
        } else {
            ("Ctrl+Shift", "Ctrl+Shift")
        };
        let mut action = |a: Action, title: String, keys: String| {
            entries.push(Entry::new(Kind::Action(a), "Actions", title).hint(keys));
        };
        if let Some(t) = self.active_tab() {
            let title = t.read(cx).session().title.clone();
            action(
                Action::Duplicate,
                format!("Another tab to {title}"),
                String::new(),
            );
            action(
                Action::SplitRight,
                format!("Split right, to {title}"),
                format!("{chord}+D"),
            );
            action(
                Action::SplitDown,
                format!("Split down, to {title}"),
                if cfg!(target_os = "macos") {
                    "Cmd+Shift+D".into()
                } else {
                    "Ctrl+Shift+E".into()
                },
            );
            action(Action::CloseTab, "Close the tab".into(), String::new());
            if let Some(tab) = self.tabs.get(self.active)
                && tab.views().len() > 1
            {
                action(
                    Action::Broadcast,
                    if tab.broadcast {
                        "Stop typing in every pane".into()
                    } else {
                        "Type in every pane of this tab".into()
                    },
                    format!("{chord}+I"),
                );
            }
            action(
                Action::Find,
                format!("Find in {title}…"),
                format!("{chord}+F"),
            );
            action(
                Action::SaveOutput { html: false },
                format!("Save the output of {title} as text…"),
                String::new(),
            );
            action(
                Action::SaveOutput { html: true },
                format!("Save the output of {title} as HTML, with colors…"),
                String::new(),
            );
            action(
                Action::Log,
                if t.read(cx).session().log_path().is_some() {
                    format!("Stop logging {title}")
                } else {
                    format!("Log {title} to a file")
                },
                String::new(),
            );
            action(
                Action::Files,
                if t.read(cx).files_shown() {
                    "Hide the files".into()
                } else {
                    format!("Files on {title}")
                },
                format!("{chord}+O"),
            );
            if t.read(cx).session().status() == Status::Connected {
                action(
                    Action::Forward,
                    format!("Forward a port through {title}…"),
                    "-L / -R / -D".into(),
                );
            }
        }
        action(
            Action::ChangeTheme,
            "Change theme…".into(),
            format!("{alt}+] / ["),
        );
        action(
            Action::ToggleHosts,
            if self.show_sidebar {
                "Hide the host list".into()
            } else {
                "Show the host list".into()
            },
            format!("{chord}+B"),
        );
        action(
            Action::ManageSecrets,
            "Saved passwords and passphrases…".into(),
            String::new(),
        );
        match self.sync.folder() {
            Some(f) => {
                action(
                    Action::SyncNow,
                    format!("Sync now with {}", f.display()),
                    String::new(),
                );
                action(
                    Action::ShowRecoveryKey,
                    "Show the recovery key".into(),
                    String::new(),
                );
                action(Action::StopSync, "Stop syncing".into(), String::new());
            }
            None => action(
                Action::SyncFolder,
                "Sync passwords with a folder…".into(),
                "iCloud Drive, Syncthing, Dropbox…".into(),
            ),
        }
        action(
            Action::Settings,
            "Settings…".into(),
            if cfg!(target_os = "macos") {
                "Cmd+,".into()
            } else {
                "Ctrl+,".into()
            },
        );
        action(Action::NewHost, "New host…".into(), String::new());
        action(Action::NewGroup, "New group…".into(), String::new());
        if let Some(target) = self.active_tab().map(|t| {
            let s = t.read(cx).session();
            match &s.entry {
                Some(name) => EditTarget::Saved(name.clone()),
                None => EditTarget::Ssh(s.spec.clone()),
            }
        }) && (matches!(target, EditTarget::Saved(_))
            || matches!(&target, EditTarget::Ssh(a) if self.ssh_aliases.contains(a)))
        {
            let name = match &target {
                EditTarget::Saved(n) | EditTarget::Ssh(n) => n.clone(),
                EditTarget::New | EditTarget::Local => String::new(),
            };
            action(
                Action::EditHost(target),
                format!("Edit host {name}…"),
                String::new(),
            );
        }
        action(Action::EditHosts, "Edit hosts.toml".into(), String::new());
        action(
            Action::EditSettings,
            "Edit config.toml".into(),
            String::new(),
        );
        entries
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.palette_entries(cx);
        let palette = cx.new(|cx| Palette::connect(entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    fn open_theme_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self
            .active_tab()
            .and_then(|t| t.read(cx).session().entry.clone());
        if let Some(t) = self.live.host_theme(entry.as_deref()) {
            let msg = format!("This host uses the theme `{t}` from hosts.toml; change it there.");
            self.say(msg, CONFIG_NOTICE, cx);
            return;
        }
        let current = self.live.look_for(None).0.theme.id.clone();
        let entries = self
            .live
            .themes()
            .all()
            .iter()
            .map(|t| {
                Entry::new(Kind::Theme(t.id.clone()), "Themes", t.name.clone()).detail(
                    if t.gradient.is_some() {
                        "gradient"
                    } else {
                        "solid"
                    },
                )
            })
            .collect();
        let palette = cx.new(|cx| Palette::themes(entries, &current, cx));
        self.show_palette(palette, true, window, cx);
    }

    fn on_sync(&mut self, ev: SyncEvent, cx: &mut Context<Self>) {
        match ev {
            SyncEvent::Done(r) => {
                self.sync_error = None;
                if r.pulled > 0 {
                    let n = r.pulled;
                    let s = if n == 1 { "" } else { "s" };
                    self.say(
                        format!("Synced {n} change{s} from other devices."),
                        THEME_NOTICE,
                        cx,
                    );
                }
            }
            SyncEvent::Failed(e) => {
                if self.sync_error.as_ref() != Some(&e) {
                    self.say(format!("Sync: {e}"), CONFIG_NOTICE, cx);
                    self.sync_error = Some(e);
                }
            }
        }
    }

    /// Asks for a folder, then starts syncing with it, or joins the vault in it.
    fn choose_sync_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Sync with this folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |ws, window, cx| ws.sync_with(folder, window, cx));
            }
        })
        .detach();
    }

    fn sync_with(
        &mut self,
        folder: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = self.vault.ready() else {
            let why = self
                .vault
                .problem()
                .unwrap_or_else(|| "the vault is still opening".into());
            return self.say(format!("Cannot sync: {why}."), CONFIG_NOTICE, cx);
        };
        let store = Folder::new(&folder);
        match vault.folder_state(&store) {
            Ok(FolderState::Empty) | Ok(FolderState::Ours) => match vault.start_sync(&store) {
                Ok(rk) => {
                    self.keep_sync_folder(Some(&folder), cx);
                    self.show_recovery_key(rk.to_string(), true, window, cx);
                }
                Err(e) => self.say(format!("Cannot sync: {e}"), CONFIG_NOTICE, cx),
            },
            Ok(FolderState::Other) => {
                self.asking = Some(Asking::RecoveryKey(folder));
                self.ask_recovery_key(false, window, cx);
            }
            Err(e) => self.say(format!("Cannot sync: {e}"), CONFIG_NOTICE, cx),
        }
    }

    fn keep_sync_folder(&mut self, folder: Option<&std::path::Path>, cx: &mut Context<Self>) {
        if let Err(e) = self.live.set_sync_folder(folder) {
            self.say(
                format!("Cannot save the sync folder: {e}"),
                CONFIG_NOTICE,
                cx,
            );
        }
        self.sync.set_folder(folder.map(|f| f.to_path_buf()));
        self.sync.now();
        self.refresh_settings(cx);
    }

    fn ask_recovery_key(&mut self, again: bool, window: &mut Window, cx: &mut Context<Self>) {
        let folder = match &self.asking {
            Some(Asking::RecoveryKey(f)) => f.display().to_string(),
            _ => String::new(),
        };
        let mut notes = vec![
            Entry::new(
                Kind::Note,
                "",
                format!("{folder} holds a vault from another device."),
            )
            .detail("Type its recovery key to share it. Secrets saved here are kept and merged."),
        ];
        if again {
            notes.insert(
                0,
                Entry::new(Kind::Note, "", "That recovery key does not open it.")
                    .detail("Check each group of four; letters and case do not need to be exact."),
            );
        }
        let palette = cx.new(|cx| Palette::ask("Recovery key: XXXX-XXXX-…", notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// What the settings page shows.
    fn settings_snap(&self) -> Snap {
        Snap {
            settings: self.live.settings().clone(),
            themes: self.live.themes().all().to_vec(),
            fonts: self.fonts.0.clone(),
            default_font: self.fonts.1.clone(),
            all_fonts: self.fonts.2.clone(),
            editors: self.editors.clone(),
            sync_folder: self.sync.folder(),
            config_dir: self.live.store().map(|s| s.dir().to_path_buf()),
            known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS")
                .map(Into::into)
                .or_else(|| std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts"))),
            ssh_config: ssh_config::default_config_path(),
            secrets: self
                .vault
                .ready()
                .and_then(|v| v.items().ok())
                .map(|i| i.len()),
        }
    }

    pub fn open_settings(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        self.editors = crate::editors::installed();
        let installed = window.text_system().all_font_names();
        self.fonts = (
            settings_view::mono_fonts(&installed),
            TermView::default_font(window),
            installed,
        );
        let snap = self.settings_snap();
        let ws = cx.entity().downgrade();
        let view = cx.new(|cx| SettingsView::new(ws, page, snap, cx));
        let sub = cx.subscribe_in(
            &view,
            window,
            |ws, _, ev: &SettingsEvent, window, cx| match ev {
                SettingsEvent::Close => ws.close_settings(window, cx),
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.settings = Some((view, sub));
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.take().is_some() {
            self.activate(self.active, window, cx);
            cx.notify();
        }
    }

    /// From the settings page: writes a setting, applies it to the open tabs,
    /// and returns what the page shows now.
    /// Hands the settings other views read on their own (the Files panel's
    /// editor) the values just loaded. Run after every load: a change made
    /// here reloads at once, so the periodic look finds nothing new after it.
    fn share_settings(&self, cx: &mut Context<Self>) {
        cx.set_global(EditorCommand(self.live.settings().files.editor.clone()));
        let font = self.live.settings().appearance.ui_font.family.clone();
        cx.set_global(crate::prompt::PromptFont(font.map(Into::into)));
    }

    pub fn change_setting(
        &mut self,
        path: &[&str],
        value: Option<shellwing_config::SettingValue>,
        cx: &mut Context<Self>,
    ) -> Option<Snap> {
        if let Err(e) = self.live.set_setting(path, value) {
            self.say(e, CONFIG_NOTICE, cx);
        }
        self.share_settings(cx);
        let problems = self.restyle(cx);
        self.report(problems, cx);
        Some(self.settings_snap())
    }

    pub fn set_app_theme(&mut self, id: &str, cx: &mut Context<Self>) -> Option<Snap> {
        match self.live.set_theme(id) {
            Ok(_) => {
                self.restyle(cx);
            }
            Err(e) => self.say(e, CONFIG_NOTICE, cx),
        }
        Some(self.settings_snap())
    }

    /// From the settings page: the same actions as the palette's.
    /// The vault, and the network runtime, for the Keys page.
    pub(crate) fn keys_context(&self) -> (VaultSlot, Handle) {
        (self.vault.clone(), self.rt.clone())
    }

    /// What a snippet can be offered for: the groups, and the hosts (from
    /// hosts.toml, then ~/.ssh/config).
    pub(crate) fn snippet_scopes(&self) -> (Vec<String>, Vec<String>) {
        let hosts = self.live.hosts();
        let groups = hosts.groups.iter().map(|g| g.name.clone()).collect();
        let mut names: Vec<String> = hosts.hosts.iter().map(|h| h.name.clone()).collect();
        for a in &self.ssh_aliases {
            if !names.contains(a) {
                names.push(a.clone());
            }
        }
        (groups, names)
    }

    /// The vault changed here: sync it.
    pub(crate) fn vault_changed(&self) {
        self.sync.now();
    }

    /// The keys a host can be set to sign in with: the vault's, then the
    /// files in `~/.ssh`.
    fn key_choices(&self) -> Vec<host_editor::KeyChoice> {
        use shellwing_ssh::keys;
        let mut choices: Vec<host_editor::KeyChoice> = self
            .vault
            .ready()
            .and_then(|v| v.ssh_keys().ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|k| {
                let public = keys::ssh_key::PublicKey::from_openssh(&k.public).ok()?;
                Some(host_editor::KeyChoice {
                    value: format!("{}{}", target::VAULT_KEY, k.id),
                    name: k.name,
                    kind: keys::info(&public).kind,
                    vault: true,
                })
            })
            .collect();
        let home = std::env::home_dir();
        if let Some(dir) = home.as_ref().map(|h| h.join(".ssh")) {
            for f in keys::key_files(&dir) {
                let value = match home.as_ref().and_then(|h| f.path.strip_prefix(h).ok()) {
                    Some(rest) => format!("~/{}", rest.display()),
                    None => f.path.display().to_string(),
                };
                choices.push(host_editor::KeyChoice {
                    name: value.clone(),
                    value,
                    kind: f.info.kind,
                    vault: false,
                });
            }
        }
        choices
    }

    /// The hosts that sign in with each key the vault keeps, by its id.
    pub(crate) fn key_users(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut users: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for h in &self.live.hosts().hosts {
            if h.auth.method == shellwing_config::AuthMethod::Key
                && let Some(id) = h
                    .auth
                    .key
                    .as_deref()
                    .and_then(|k| k.strip_prefix(target::VAULT_KEY))
            {
                users
                    .entry(id.to_string())
                    .or_default()
                    .push(h.name.clone());
            }
        }
        users
    }

    /// Asks which host to install `key` on: connected ones first.
    pub(crate) fn choose_host_for_key(
        &mut self,
        key: KeyToInstall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (mut connected, mut others) = (vec![], vec![]);
        for section in self.sidebar_sections(cx) {
            for r in section.rows {
                let up = r.status == Some(Status::Connected);
                let entry = Entry::new(
                    Kind::Choice(r.spec),
                    if up {
                        "Connected"
                    } else {
                        "Other hosts · connected first"
                    },
                    r.name,
                )
                .detail(if r.detail.is_empty() {
                    section.title.clone()
                } else {
                    r.detail
                })
                .status(r.status)
                .hint("Enter");
                if up {
                    connected.push(entry)
                } else {
                    others.push(entry)
                }
            }
        }
        connected.extend(others);
        let title = format!("Install {} on…", key.name);
        self.choosing = Some(key);
        let palette = cx.new(|cx| Palette::chooser(&title, connected, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// Installs `key` on the host `spec`: on a pane connected to it, else on
    /// a new tab once that connects.
    fn install_on(
        &mut self,
        spec: &str,
        key: KeyToInstall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_settings(window, cx);
        let up = self.tabs.iter().enumerate().find_map(|(i, t)| {
            t.views().into_iter().find_map(|v| {
                let s = v.read(cx).session();
                (s.spec == spec && s.status() == Status::Connected).then_some((i, v.clone()))
            })
        });
        match up {
            Some((tab, view)) => {
                self.activate(tab, window, cx);
                view.update(cx, |v, cx| v.install_key(key, cx));
            }
            None => {
                let before = self.tabs.len();
                self.open(spec, window, cx);
                if self.tabs.len() > before
                    && let Some(view) = self.tabs.last().map(|t| t.focused.clone())
                {
                    view.update(cx, |v, cx| {
                        v.show_notice(
                            format!("{} goes on here once connected.", key.name),
                            CONFIG_NOTICE,
                            cx,
                        )
                    });
                    self.installs.push((view.downgrade(), key));
                }
            }
        }
    }

    /// Installs the keys waiting for panes that are connected now.
    fn run_installs(&mut self, cx: &mut Context<Self>) {
        let waiting = std::mem::take(&mut self.installs);
        for (view, key) in waiting {
            let Some(v) = view.upgrade() else {
                continue;
            };
            if v.read(cx).session().status() == Status::Connected {
                v.update(cx, |v, cx| v.install_key(key, cx));
            } else {
                self.installs.push((view, key));
            }
        }
    }

    /// From a pane: sign in to its host with a key the vault keeps.
    fn use_key(&mut self, ev: &UseKey, cx: &mut Context<Self>) {
        let Some(mut host) = self.live.hosts().find(&ev.host).cloned() else {
            return;
        };
        host.auth.method = shellwing_config::AuthMethod::Key;
        host.auth.key = Some(format!("{}{}", target::VAULT_KEY, ev.vault_id));
        let saved = match self.live.store() {
            Some(store) => store
                .save_host(&host)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            None => Err("no config directory".into()),
        };
        match saved {
            Ok(()) => {
                self.poll_config(cx);
                self.sync.now();
                self.say(
                    format!("{} signs in with {} from now on.", ev.host, ev.key_name),
                    CONFIG_NOTICE,
                    cx,
                );
            }
            Err(e) => self.say(format!("Cannot edit hosts.toml: {e}"), CONFIG_NOTICE, cx),
        }
    }

    pub fn run_from_settings(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run(action, window, cx);
        self.refresh_settings(cx);
    }

    /// Shows the settings page what is true now. Deferred: this may run while
    /// the page itself is handling a click.
    fn refresh_settings(&self, cx: &mut Context<Self>) {
        if let Some((view, _)) = &self.settings {
            let (view, snap) = (view.clone(), self.settings_snap());
            cx.defer(move |cx| view.update(cx, |v, cx| v.set_snap(snap, cx)));
        }
    }

    /// Opens the host editor drawer on `target`.
    pub fn edit_host(&mut self, target: EditTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        // The local terminal's one choice, its shell, is on the Terminal page.
        if target == EditTarget::Local {
            return self.open_settings(Page::Terminal, window, cx);
        }
        // Every host gets an id first, so the one edited is found again on saving.
        if let Some(store) = self.live.store()
            && let Err(e) = store.assign_host_ids()
        {
            self.say(format!("Cannot edit hosts.toml: {e}"), CONFIG_NOTICE, cx);
            return;
        }
        self.poll_config(cx);
        let ctx = EditorContext {
            store: self.live.store().cloned(),
            hosts: self.live.hosts().clone(),
            ssh_aliases: self.ssh_aliases.clone(),
            themes: self
                .live
                .themes()
                .all()
                .iter()
                .map(|t| (t.id.clone(), t.name.clone()))
                .collect(),
            app_ssh: self.live.settings().ssh.clone(),
            app_logs: self.live.settings().log.always,
            keys: self.key_choices(),
            vault: self.vault.clone(),
        };
        let found = match &target {
            EditTarget::Saved(name) => self.live.hosts().find(name).cloned(),
            _ => None,
        };
        let editor = cx.new(|cx| match (target, found) {
            (EditTarget::Saved(_), Some(host)) => HostEditor::edit(ctx, host, cx),
            (EditTarget::Ssh(alias), _) => HostEditor::ssh_config(ctx, alias, cx),
            _ => HostEditor::new_host(ctx, cx),
        });
        let sub = cx.subscribe_in(
            &editor,
            window,
            |ws, _, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::Close => ws.close_editor(window, cx),
                EditorEvent::Saved {
                    name,
                    connect,
                    install,
                } => {
                    let (name, connect, install) = (name.clone(), *connect, install.clone());
                    ws.close_editor(window, cx);
                    ws.poll_config(cx);
                    ws.sync.now();
                    if let Some(key) = install {
                        ws.install_on(&name, key, window, cx);
                    } else if connect {
                        ws.open(&name, window, cx);
                    } else {
                        ws.say(format!("Saved {name}."), THEME_NOTICE, cx);
                    }
                }
                EditorEvent::Deleted { name } => {
                    let name = name.clone();
                    ws.close_editor(window, cx);
                    ws.poll_config(cx);
                    ws.sync.now();
                    ws.say(format!("Deleted {name}."), THEME_NOTICE, cx);
                }
                EditorEvent::OpenSshConfig => {
                    if let Some(path) = ssh_config::default_config_path() {
                        cx.open_url(&format!("file://{}", path.display()));
                    }
                }
            },
        );
        let first = editor.read(cx).first_focus(cx);
        self.editor = Some((editor, sub));
        window.focus(&first, cx);
        cx.notify();
    }

    /// Opens the group editor on the group named `name`, or on a new one.
    pub fn edit_group(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if let Some(store) = self.live.store()
            && let Err(e) = store.assign_host_ids()
        {
            self.say(format!("Cannot edit hosts.toml: {e}"), CONFIG_NOTICE, cx);
            return;
        }
        self.poll_config(cx);
        let hosts = self.live.hosts().clone();
        let group = name.and_then(|n| hosts.groups.iter().find(|g| g.name == n).cloned());
        let themes = self
            .live
            .themes()
            .all()
            .iter()
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();
        let store = self.live.store().cloned();
        let editor = cx.new(|cx| GroupEditor::new(store, hosts, themes, group, cx));
        let sub = cx.subscribe_in(&editor, window, |ws, _, ev: &GroupEvent, window, cx| {
            let said = match ev {
                GroupEvent::Close => None,
                GroupEvent::Saved { name } => Some(format!("Saved the group {name}.")),
                GroupEvent::Deleted { name } => Some(format!("Deleted the group {name}.")),
            };
            ws.close_group_editor(window, cx);
            if let Some(msg) = said {
                ws.poll_config(cx);
                ws.sync.now();
                ws.say(msg, THEME_NOTICE, cx);
            }
        });
        let first = editor.read(cx).first_focus(cx);
        self.group_editor = Some((editor, sub));
        window.focus(&first, cx);
        cx.notify();
    }

    fn close_group_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.group_editor.take().is_some() {
            self.activate(self.active, window, cx);
            cx.notify();
        }
    }

    fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.take().is_some() {
            self.activate(self.active, window, cx);
            cx.notify();
        }
    }

    fn toggle_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab().cloned() {
            tab.update(cx, |v, cx| v.toggle_files(window, cx));
        }
    }

    fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab().cloned() {
            tab.update(cx, |v, cx| v.open_find(window, cx));
        }
    }

    /// Logs the focused pane to a file, or stops.
    fn toggle_log(&mut self, cx: &mut Context<Self>) {
        let settings = self.live.settings().log.clone();
        if let Some(tab) = self.active_tab().cloned() {
            tab.update(cx, |v, cx| v.toggle_log(&settings, cx));
        }
    }

    fn save_output(&mut self, html: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab().cloned() {
            tab.update(cx, |v, cx| v.save_output(html, window, cx));
        }
    }

    fn ask_forward(
        &mut self,
        problem: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.asking = Some(Asking::Forward);
        let note = |title: &str, detail: &str| {
            Entry::new(Kind::Note, "", title.to_string()).detail(detail.to_string())
        };
        let mut notes = vec![
            note(
                "-L 8080:localhost:80",
                "Port 8080 here reaches localhost:80 as the server sees it.",
            ),
            note(
                "-R 9000:localhost:3000",
                "Port 9000 on the server reaches localhost:3000 here.",
            ),
            note(
                "-D 1080",
                "A SOCKS proxy on port 1080 here; connections go out from the server.",
            ),
            note(
                "For this tab, until it is closed",
                "To forward on every connection, add it to the host's forward list in hosts.toml.",
            ),
        ];
        if let Some(p) = problem {
            notes.insert(0, Entry::new(Kind::Note, "", p));
        }
        let palette = cx.new(|cx| Palette::ask("-L 8080:localhost:80", notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// Starts the forward typed into the palette on the active tab's connection.
    fn answer_forward(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(window, cx);
        let forward = match text.parse::<shellwing_ssh::Forward>() {
            Ok(f) => f,
            // Asked again, with what was wrong.
            Err(e) => return self.ask_forward(Some(e.to_string()), window, cx),
        };
        self.asking = None;
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        let Some(answer) = tab.read(cx).session().forward(forward) else {
            return self.say("Not connected.".into(), CONFIG_NOTICE, cx);
        };
        cx.spawn(async move |_, cx| {
            let result = answer.await;
            tab.update(cx, |v, cx| {
                let msg = match result {
                    Ok(Ok(active)) => {
                        v.session_mut().keep_forward(&active);
                        let text = active.to_string();
                        let mut chars = text.chars();
                        chars
                            .next()
                            .map(|c| c.to_uppercase().chain(chars).collect())
                            .unwrap_or(text)
                    }
                    Ok(Err(e)) => e.to_string(),
                    Err(_) => "The connection ended.".into(),
                };
                v.show_notice(msg, CONFIG_NOTICE, cx);
            });
        })
        .detach();
    }

    fn show_recovery_key(
        &mut self,
        key: String,
        new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = vec![
            Entry::new(Kind::Copy(key.clone()), "Recovery key", key).hint("Enter copies"),
            Entry::new(
                Kind::Note,
                "Recovery key",
                if new {
                    "Syncing. Write this key down and keep it safe."
                } else {
                    "The key of the synced vault."
                },
            )
            .detail(
                "Another device needs it to join; it also opens the vault if this device is lost.",
            ),
        ];
        let palette = cx.new(|cx| Palette::show(entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    fn open_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            let why = self
                .vault
                .problem()
                .unwrap_or_else(|| "the vault is still opening".into());
            self.say(format!("No saved passwords: {why}."), CONFIG_NOTICE, cx);
            return;
        };
        let entries: Vec<Entry> = match vault.items() {
            Ok(items) => items
                .into_iter()
                .map(|item| match item {
                    shellwing_vault::Item::Password { id, target } => {
                        Entry::new(Kind::Secret(id), "Saved", format!("Password for {target}"))
                            .hint("Enter to forget")
                    }
                    shellwing_vault::Item::Passphrase {
                        id,
                        fingerprint,
                        key_path,
                    } => Entry::new(
                        Kind::Secret(id),
                        "Saved",
                        format!("Passphrase of {key_path}"),
                    )
                    .detail(fingerprint)
                    .hint("Enter to forget"),
                })
                .collect(),
            Err(e) => {
                self.say(format!("Cannot read the vault: {e}"), CONFIG_NOTICE, cx);
                return;
            }
        };
        if entries.is_empty() {
            self.say(
                "Nothing saved yet: after a password works, the pane offers to save it.".into(),
                CONFIG_NOTICE,
                cx,
            );
            return;
        }
        let palette = cx.new(|cx| Palette::secrets(entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    fn show_palette(
        &mut self,
        palette: Entity<Palette>,
        themes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sub = cx.subscribe_in(&palette, window, Self::on_palette);
        window.focus(&palette.focus_handle(cx), cx);
        self.palette = Some((palette, sub, themes));
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choosing = None;
        if let Some((_, _, themes)) = self.palette.take()
            && themes
        {
            // Undo the preview.
            self.restyle(cx);
        }
        // Back to the settings page if the palette was opened from it.
        match &self.settings {
            Some((view, _)) => {
                let view = view.clone();
                self.refresh_settings(cx);
                window.focus(&view.focus_handle(cx), cx);
            }
            None => self.activate(self.active, window, cx),
        }
    }

    fn on_palette(
        &mut self,
        _: &Entity<Palette>,
        ev: &PaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            PaletteEvent::Dismiss => self.close_palette(window, cx),
            PaletteEvent::Open(spec) => {
                let spec = spec.clone();
                self.close_palette(window, cx);
                self.open(&spec, window, cx);
            }
            PaletteEvent::Snippet { id, all } => {
                let (id, all) = (id.clone(), *all);
                self.close_palette(window, cx);
                self.run_snippet(&id, all, window, cx);
            }
            PaletteEvent::Chose(spec) => {
                let (spec, key) = (spec.clone(), self.choosing.take());
                self.close_palette(window, cx);
                if let Some(key) = key {
                    self.install_on(&spec, key, window, cx);
                }
            }
            PaletteEvent::Act(a) => {
                self.close_palette(window, cx);
                self.run(a.clone(), window, cx);
            }
            PaletteEvent::Preview(id) => {
                if let (Some(tab), Some(theme)) = (
                    self.active_tab().cloned(),
                    self.live.themes().find(id).cloned(),
                ) {
                    tab.update(cx, |v, cx| {
                        let mut look = v.look().clone();
                        look.theme = theme;
                        v.set_look(look, cx);
                    });
                }
            }
            PaletteEvent::Copy(text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                self.close_palette(window, cx);
                self.say("Copied.".into(), THEME_NOTICE, cx);
            }
            PaletteEvent::Answer(text) => {
                let folder = match self.asking.clone() {
                    Some(Asking::Forward) => return self.answer_forward(text, window, cx),
                    Some(Asking::Snippet(mut run)) => {
                        if !run.left.is_empty() {
                            let name = run.left.remove(0);
                            run.values.insert(name, text.clone());
                        }
                        self.close_palette(window, cx);
                        return self.ask_snippet(run, window, cx);
                    }
                    Some(Asking::RecoveryKey(folder)) => folder,
                    None => return self.close_palette(window, cx),
                };
                let Some(vault) = self.vault.ready() else {
                    return self.close_palette(window, cx);
                };
                match vault.join(&Folder::new(&folder), text) {
                    Ok(()) => {
                        self.asking = None;
                        self.close_palette(window, cx);
                        self.keep_sync_folder(Some(&folder), cx);
                        self.say(
                            format!("Joined: this device syncs with {}.", folder.display()),
                            CONFIG_NOTICE,
                            cx,
                        );
                    }
                    Err(shellwing_vault::Error::WrongRecoveryKey) => {
                        self.close_palette(window, cx);
                        self.ask_recovery_key(true, window, cx);
                    }
                    Err(e) => {
                        self.close_palette(window, cx);
                        self.say(format!("Cannot join: {e}"), CONFIG_NOTICE, cx);
                    }
                }
            }
            PaletteEvent::Forget(id) => {
                let forgotten = self.vault.ready().map(|v| v.delete(id));
                self.close_palette(window, cx);
                match forgotten {
                    Some(Ok(())) => {
                        self.sync.now();
                        self.say("Forgotten.".into(), THEME_NOTICE, cx);
                        // Back to the list for the next one, if any is left.
                        let left = self
                            .vault
                            .ready()
                            .and_then(|v| v.items().ok())
                            .is_some_and(|i| !i.is_empty());
                        if left {
                            self.open_secrets(window, cx);
                        }
                    }
                    Some(Err(e)) => self.say(format!("Not forgotten: {e}"), CONFIG_NOTICE, cx),
                    None => {}
                }
            }
            PaletteEvent::Pick(id) => {
                let picked = self.live.set_theme(id);
                self.close_palette(window, cx);
                match picked {
                    Ok(t) => {
                        self.restyle(cx);
                        self.say(format!("Theme: {}", t.name), THEME_NOTICE, cx);
                    }
                    Err(e) => self.say(e, CONFIG_NOTICE, cx),
                }
            }
        }
    }

    fn run(&mut self, a: Action, window: &mut Window, cx: &mut Context<Self>) {
        match a {
            Action::Duplicate => self.duplicate(window, cx),
            Action::SplitRight => self.split(Axis::Horizontal, window, cx),
            Action::SplitDown => self.split(Axis::Vertical, window, cx),
            Action::CloseTab => self.request_close_tab(self.active, window, cx),
            Action::ChangeTheme => self.open_theme_picker(window, cx),
            Action::Forward => self.ask_forward(None, window, cx),
            Action::Files => self.toggle_files(window, cx),
            Action::Broadcast => self.toggle_broadcast(cx),
            Action::Find => self.find(window, cx),
            Action::Log => self.toggle_log(cx),
            Action::SaveOutput { html } => self.save_output(html, window, cx),
            Action::NewHost => self.edit_host(EditTarget::New, window, cx),
            Action::NewGroup => self.edit_group(None, window, cx),
            Action::Settings => self.open_settings(Page::Appearance, window, cx),
            Action::EditHost(target) => self.edit_host(target, window, cx),
            Action::ManageSecrets => self.open_secrets(window, cx),
            Action::SyncFolder => self.choose_sync_folder(window, cx),
            Action::SyncNow => {
                self.sync_error = None;
                self.sync.now();
                self.say("Syncing…".into(), THEME_NOTICE, cx);
            }
            Action::ShowRecoveryKey => match self.vault.ready().map(|v| v.recovery_key()) {
                Some(Ok(Some(rk))) => self.show_recovery_key(rk.to_string(), false, window, cx),
                Some(Err(e)) => self.say(format!("Cannot read the key: {e}"), CONFIG_NOTICE, cx),
                _ => self.say("Not syncing yet.".into(), CONFIG_NOTICE, cx),
            },
            Action::StopSync => {
                self.keep_sync_folder(None, cx);
                self.say(
                    "Stopped syncing. The folder and the secrets here are kept.".into(),
                    CONFIG_NOTICE,
                    cx,
                );
            }
            Action::ToggleHosts => {
                self.show_sidebar = !self.show_sidebar;
                cx.notify();
            }
            Action::EditHosts | Action::EditSettings => {
                let which = if a == Action::EditHosts {
                    EditFile::Hosts
                } else {
                    EditFile::Settings
                };
                match self.live.store().map(|s| s.file_for_editing(which)) {
                    Some(Ok(path)) => cx.open_url(&format!("file://{}", path.display())),
                    Some(Err(e)) => {
                        self.say(format!("Cannot create the file: {e}"), CONFIG_NOTICE, cx)
                    }
                    None => self.say("No config directory.".into(), CONFIG_NOTICE, cx),
                }
            }
        }
    }

    /// Shows problems with the config files, if any.
    fn report(&mut self, problems: Vec<String>, cx: &mut Context<Self>) {
        if problems.is_empty() {
            return;
        }
        self.say(problems.join("\n"), CONFIG_NOTICE, cx);
    }

    fn say(&mut self, msg: String, shown: Duration, cx: &mut Context<Self>) {
        match self.active_tab().cloned() {
            Some(tab) => tab.update(cx, |v, cx| v.show_notice(msg, shown, cx)),
            None => {
                self.notice = Some(msg.into());
                cx.notify();
            }
        }
    }

    /// Gives every tab the look its host should have now.
    fn restyle(&mut self, cx: &mut Context<Self>) -> Vec<String> {
        let mut problems = vec![];
        for tab in self.all_views() {
            let entry = tab.read(cx).session().entry.clone();
            let (look, problem) = self.live.look_for(entry.as_deref());
            problems.extend(problem);
            tab.update(cx, |v, cx| {
                v.set_look(look, cx);
                v.refresh_panels(cx);
            });
        }
        // Cached views keep their old layout until told: the UI font may have changed.
        self.sidebar.update(cx, |_, cx| cx.notify());
        problems.dedup();
        cx.notify();
        problems
    }

    /// Called periodically: picks up edits to the config files.
    pub fn poll_config(&mut self, cx: &mut Context<Self>) {
        if !self.vault_reported
            && self.live.store().is_some()
            && let Some(p) = self.vault.problem()
        {
            self.vault_reported = true;
            self.say(format!("Passwords cannot be saved: {p}"), CONFIG_NOTICE, cx);
        }
        if let Some(mut problems) = self.live.reload_if_changed() {
            self.ssh_aliases = ssh_config::aliases(ssh_config::default_config_path().as_deref());
            self.share_settings(cx);
            problems.extend(self.restyle(cx));
            self.report(problems, cx);
            self.refresh_settings(cx);
            self.refresh_sidebar(cx);
            self.sync.set_folder(self.live.sync_folder());
        }
    }

    fn step_theme(&mut self, step: isize, cx: &mut Context<Self>) {
        let entry = self
            .active_tab()
            .and_then(|t| t.read(cx).session().entry.clone());
        match self.live.step_theme(step, entry.as_deref()) {
            Ok(theme) => {
                self.restyle(cx);
                self.say(format!("Theme: {}", theme.name), THEME_NOTICE, cx);
            }
            Err(e) => self.say(e, CONFIG_NOTICE, cx),
        }
    }

    /// Remembers the window's size and place for the next start: saved shortly
    /// after they change, and when the window closes.
    pub fn track_bounds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds_changed(window, cx);
        cx.observe_window_bounds(window, Self::bounds_changed)
            .detach();
        cx.on_release(|ws, cx| ws.save_bounds(cx)).detach();
    }

    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unsaved_bounds = Some(crate::placement::state(window.window_bounds()));
        self.bounds_change += 1;
        let change = self.bounds_change;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_BOUNDS).await;
            let _ = this.update(cx, |ws, cx| {
                if ws.bounds_change == change {
                    ws.save_bounds(cx);
                }
            });
        })
        .detach();
    }

    fn save_bounds(&mut self, cx: &gpui::App) {
        if let (Some(mut state), Some(store)) = (self.unsaved_bounds.take(), self.live.store()) {
            state.sidebar_width = Some(self.sidebar_width);
            state.sidebar_collapsed = self.sidebar_collapsed;
            state.files_width = cx.try_global::<crate::files::PanelWidth>().map(|w| w.0);
            if let Err(e) = store.save_window_state(&state) {
                eprintln!("cannot save the window size: {e}");
            }
        }
    }

    fn shortcut_for(ev: &KeyDownEvent) -> Option<Shortcut> {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let key = k.key.as_str();
        let mac = cfg!(target_os = "macos");
        // Ctrl+Shift on Linux and Windows, Cmd on macOS, as for copy and paste.
        let chord = if mac {
            m.platform && !m.control
        } else {
            m.control && m.shift
        };
        // GPUI reports Shift+] as "}" and drops the Shift flag for symbols.
        let braced = !mac && m.control && !m.alt;
        let numbered = if mac {
            m.platform && !m.control && !m.alt
        } else {
            m.alt && !m.control && !m.platform
        };
        // Moving between panes: Alt+arrows, as in Windows Terminal (Cmd+Alt on macOS).
        let panes = if mac {
            m.platform && m.alt && !m.control
        } else {
            m.alt && !m.control && !m.shift && !m.platform
        };
        // Ctrl+K alone is the shell's kill-line, so the palette takes Ctrl+Shift+K.
        Some(match key {
            "k" if chord => Shortcut::Palette,
            "t" if chord => Shortcut::NewTab,
            "w" if chord => Shortcut::Close,
            "q" if mac && m.platform && !m.control && !m.alt && !m.shift => Shortcut::Quit,
            // Cmd+D and Cmd+Shift+D on macOS, as in iTerm.
            "d" if mac && chord => Shortcut::Split(if m.shift {
                Axis::Vertical
            } else {
                Axis::Horizontal
            }),
            "d" if chord => Shortcut::Split(Axis::Horizontal),
            "e" if !mac && chord => Shortcut::Split(Axis::Vertical),
            "left" | "right" | "up" | "down" if panes => Shortcut::Focus(match key {
                "left" => Dir::Left,
                "right" => Dir::Right,
                "up" => Dir::Up,
                _ => Dir::Down,
            }),
            "b" if chord => Shortcut::Sidebar,
            "o" if chord => Shortcut::Files,
            "f" if chord => Shortcut::Find,
            "i" if chord => Shortcut::Broadcast,
            // Ctrl+, sends nothing to a terminal, so it is free, as in most apps.
            "," if (mac && m.platform) || (!mac && m.control && !m.alt) => Shortcut::Settings,
            "tab" if m.control => Shortcut::Step(if m.shift { -1 } else { 1 }),
            "pagedown" if m.control && !m.shift => Shortcut::Step(1),
            "pageup" if m.control && !m.shift => Shortcut::Step(-1),
            "}" if mac && chord => Shortcut::Step(1),
            "{" if mac && chord => Shortcut::Step(-1),
            "]" if mac && chord => Shortcut::Theme(1),
            "[" if mac && chord => Shortcut::Theme(-1),
            "}" if braced => Shortcut::Theme(1),
            "{" if braced => Shortcut::Theme(-1),
            d if numbered && d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9') => {
                Shortcut::Goto((d.as_bytes()[0] - b'1') as usize)
            }
            _ => return None,
        })
    }

    /// Runs before the terminal sees the key, so these never reach the host.
    fn shortcut(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = Self::shortcut_for(ev) else {
            return;
        };
        // Without a second pane, Alt+arrows belong to the shell (word movement).
        if let Shortcut::Focus(dir) = s {
            if self.palette.is_none() && self.focus_pane(dir, window, cx) {
                cx.stop_propagation();
            }
            return;
        }
        cx.stop_propagation();
        // Any other window shortcut leaves the palette first.
        if self.palette.is_some() && !matches!(s, Shortcut::Palette | Shortcut::NewTab) {
            self.close_palette(window, cx);
        }
        match s {
            Shortcut::Palette | Shortcut::NewTab => {
                if self.palette.is_some() {
                    self.close_palette(window, cx);
                } else {
                    self.open_palette(window, cx);
                }
            }
            Shortcut::Close => self.close_pane(window, cx),
            Shortcut::Quit => {
                if self.may_quit(window, cx) {
                    cx.quit();
                }
            }
            Shortcut::Split(axis) => self.split(axis, window, cx),
            Shortcut::Focus(_) => {}
            Shortcut::Step(by) => self.step(by, window, cx),
            Shortcut::Goto(ix) => {
                if ix < self.tabs.len() {
                    self.activate(ix, window, cx)
                }
            }
            Shortcut::Sidebar => {
                self.show_sidebar = !self.show_sidebar;
                cx.notify();
            }
            Shortcut::Files => self.toggle_files(window, cx),
            Shortcut::Find => self.find(window, cx),
            Shortcut::Broadcast => self.toggle_broadcast(cx),
            Shortcut::Settings => {
                if self.settings.is_some() {
                    self.close_settings(window, cx);
                } else {
                    self.open_settings(Page::Appearance, window, cx);
                }
            }
            Shortcut::Theme(step) => self.step_theme(step, cx),
        }
    }

    /// The best status among tabs matching `is_it`, for a sidebar dot.
    fn host_status(&self, cx: &Context<Self>, is_it: impl Fn(&Session) -> bool) -> Option<Status> {
        self.all_views()
            .iter()
            .map(|t| t.read(cx))
            .filter(|v| is_it(v.session()))
            .map(|v| v.status())
            .min_by_key(|s| match s {
                Status::Connected => 0,
                Status::Connecting => 1,
                Status::Lost => 2,
                Status::Failed => 3,
                Status::Closed => 4,
            })
    }

    /// The tabs, then empty header bar (which moves the window), then the
    /// Settings button.
    /// Above the panes of a tab that types in every pane.
    fn broadcast_strip(&self, listening: usize, panes: usize, cx: &mut Context<Self>) -> Div {
        let keys = if cfg!(target_os = "macos") {
            "Cmd+Shift+I"
        } else {
            "Ctrl+Shift+I"
        };
        div()
            .h(px(34.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .bg(rgb(colors().warn_bg))
            .border_b_1()
            .border_color(rgb(colors().warn_edge))
            .text_sm()
            .text_color(rgb(colors().warn_text))
            .child(div().size(px(8.)).rounded_full().bg(rgb(colors().amber)))
            .child(div().flex_1().min_w_0().child(format!(
                "Typing goes to {listening} of {panes} panes. Click a pane's badge to leave it out or bring it back."
            )))
            .child(
                div()
                    .id("stop-broadcast")
                    .h(px(24.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(colors().warn_edge))
                    .text_xs()
                    .text_color(rgb(colors().warn_text))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().warn_hover)))
                    .on_click(cx.listener(|ws, _, _, cx| ws.toggle_broadcast(cx)))
                    .child(format!("Stop ({keys})")),
            )
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let mut strip = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h_full()
            .min_w_0()
            .overflow_hidden();
        for (i, tab) in self.tabs.iter().enumerate() {
            let v = tab.focused.read(cx);
            let active = i == self.active;
            let title = v.session().title.clone();
            let color = self.host_color(v.session().entry.as_deref());
            strip = strip.child(
                div()
                    .id(ElementId::NamedInteger("tab".into(), i as u64))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .pl(px(9.))
                    .pr(px(4.))
                    .max_w(px(220.))
                    .rounded(px(7.))
                    .border_1()
                    .cursor_pointer()
                    .text_color(rgb(if active { colors().text } else { colors().soft }))
                    // The active one joins the pane under it; amber while it
                    // types in every pane.
                    .when(active, |d| {
                        d.bg(rgb(colors().tab_on))
                            .border_color(rgb(if tab.broadcast {
                                colors().amber
                            } else {
                                colors().edge
                            }))
                    })
                    .when(!active, |d| {
                        d.border_color(gpui::transparent_black())
                            .hover(|s| s.bg(rgb(colors().hover)))
                    })
                    // The host's color, as a mark at the start; a host without
                    // one keeps the place, so the titles line up.
                    .child(
                        div()
                            .w(px(3.))
                            .h(px(14.))
                            .flex_shrink_0()
                            .rounded(px(2.))
                            .when_some(color, |d, c| d.bg(rgb(c))),
                    )
                    // A press on a tab must not start moving the window.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |ws, _, window, cx| ws.activate(i, window, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |ws, _, window, cx| ws.request_close_tab(i, window, cx)),
                    )
                    .child(dot(Some(v.status())))
                    // Typing in every pane: said on the tab too.
                    .when(tab.broadcast, |d| {
                        d.child(div().size(px(8.)).rounded_full().bg(rgb(colors().amber)))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .child(
                        div()
                            .id(ElementId::NamedInteger("close-tab".into(), i as u64))
                            .px_1()
                            .rounded_sm()
                            .text_color(rgb(colors().muted))
                            .hover(|s| {
                                s.bg(rgb(colors().button_hover))
                                    .text_color(rgb(colors().text))
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                cx.stop_propagation();
                                ws.request_close_tab(i, window, cx);
                            }))
                            .child("✕"),
                    ),
            );
        }
        let strip = strip.child(
            div()
                .id("new-tab")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(px(6.))
                .text_color(rgb(colors().soft))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|ws, _, window, cx| ws.open_palette(window, cx)))
                .child("+"),
        );
        let open = self.settings.is_some();
        div()
            .flex()
            .items_center()
            .h_full()
            .w_full()
            .min_w_0()
            // The logo, as the sketch puts it before the tabs.
            .child(
                div().flex_shrink_0().pl(px(12.)).pr(px(8.)).child(
                    svg()
                        .path("icons/logo.svg")
                        .size(px(18.))
                        .text_color(rgb(colors().accent)),
                ),
            )
            .child(strip)
            .child(div().flex_1().h_full())
            .child(
                div()
                    .id("settings-button")
                    .flex_shrink_0()
                    .size(px(30.))
                    .mr(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(open, |d| d.bg(rgb(colors().selected)))
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|ws, _, window, cx| {
                        if ws.settings.is_some() {
                            ws.close_settings(window, cx);
                        } else {
                            ws.open_settings(Page::Appearance, window, cx);
                        }
                    }))
                    .child(
                        svg()
                            .path("icons/settings.svg")
                            .size(px(16.))
                            .text_color(rgb(if open { colors().text } else { colors().soft })),
                    ),
            )
    }

    /// The color a hosts.toml host is marked with, by name.
    fn host_color(&self, entry: Option<&str>) -> Option<u32> {
        self.live.hosts().find(entry?).and_then(|h| h.color_rgb())
    }

    /// What the host list shows now: hosts.toml by group, then ssh config
    /// aliases it does not already name, each with its tabs' status.
    fn sidebar_sections(&self, cx: &Context<Self>) -> Vec<Section> {
        let hosts = self.live.hosts();
        // The active tab's host, marked on its tile.
        let (active_entry, active_spec) = match self.active_tab() {
            Some(t) => {
                let s = t.read(cx).session();
                (s.entry.clone(), Some(s.spec.clone()))
            }
            None => (None, None),
        };
        let row = |h: &Host| Row {
            name: h.name.clone().into(),
            detail: match &h.user {
                Some(u) => format!("{u}@{}", h.address).into(),
                None => h.address.clone().into(),
            },
            spec: h.alias.clone().unwrap_or_else(|| h.name.clone()),
            status: self.host_status(cx, |s| s.entry.as_deref() == Some(h.name.as_str())),
            edit: EditTarget::Saved(h.name.clone()),
            color: h.color_rgb(),
            active: active_entry.as_deref() == Some(h.name.as_str()),
            search: [
                h.name.as_str(),
                h.alias.as_deref().unwrap_or(""),
                h.address.as_str(),
                h.user.as_deref().unwrap_or(""),
                &h.tags.join(" "),
            ]
            .join(" ")
            .to_lowercase(),
        };
        let mut sections = vec![];
        let shell = crate::local::shell(self.live.settings().terminal.local_shell.as_deref());
        let shell_name = crate::local::shell_name(&shell);
        sections.push(Section {
            title: "This computer".into(),
            note: SharedString::default(),
            group: None,
            color: None,
            rows: vec![Row {
                name: "Local terminal".into(),
                detail: shell_name.clone().into(),
                spec: crate::local::SPEC.into(),
                status: self.host_status(cx, Session::is_local),
                edit: EditTarget::Local,
                color: None,
                active: active_spec.as_deref() == Some(crate::local::SPEC),
                search: format!("local terminal this computer shell {shell_name}"),
            }],
        });
        let favorites: Vec<Row> = hosts.hosts.iter().filter(|h| h.favorite).map(row).collect();
        if !favorites.is_empty() {
            sections.push(Section {
                title: "Favorites".into(),
                note: SharedString::default(),
                group: None,
                color: None,
                rows: favorites,
            });
        }
        for g in &hosts.groups {
            let rows: Vec<Row> = hosts
                .hosts
                .iter()
                .filter(|h| h.group.as_deref() == Some(g.name.as_str()))
                .map(row)
                .collect();
            // Shown even when empty: a new group is there to put hosts in.
            sections.push(Section {
                title: g.name.clone().into(),
                note: rows.len().to_string().into(),
                group: Some(g.name.clone()),
                color: g.color.as_deref().and_then(shellwing_config::color_rgb),
                rows,
            });
        }
        let loose: Vec<Row> = hosts
            .hosts
            .iter()
            .filter(|h| hosts.group_of(h).is_none())
            .map(row)
            .collect();
        if !loose.is_empty() {
            sections.push(Section {
                title: "Hosts".into(),
                note: loose.len().to_string().into(),
                group: None,
                color: None,
                rows: loose,
            });
        }
        let aliases: Vec<Row> = self
            .ssh_aliases
            .iter()
            .filter(|a| hosts.find(a).is_none())
            .map(|a| Row {
                name: a.clone().into(),
                detail: SharedString::default(),
                spec: a.clone(),
                status: self.host_status(cx, |s| s.entry.is_none() && &s.spec == a),
                edit: EditTarget::Ssh(a.clone()),
                color: None,
                active: active_entry.is_none() && active_spec.as_deref() == Some(a.as_str()),
                search: a.to_lowercase(),
            })
            .collect();
        if !aliases.is_empty() {
            sections.push(Section {
                title: "~/.ssh/config".into(),
                note: "read only".into(),
                group: None,
                color: None,
                rows: aliases,
            });
        }
        sections
    }

    /// Hands the host list a fresh snapshot (it redraws only if it differs).
    fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        let sections = self.sidebar_sections(cx);
        let collapsed = self.sidebar_collapsed;
        self.sidebar.update(cx, |s, cx| {
            s.set(sections, cx);
            s.set_collapsed(collapsed, cx);
        });
        cx.notify();
    }

    /// A pane tree: terminals, and between the two sides of each split a divider
    /// that can be dragged. With several panes, the focused one is marked.
    /// A pane, or a split of two; `corners` are the window's bottom corners
    /// it fills, which it rounds.
    fn render_pane(
        &self,
        pane: &Pane<Entity<TermView>>,
        focused: &Entity<TermView>,
        multi: bool,
        accent: Hsla,
        corners: chrome::BottomCorners,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match pane {
            Pane::Leaf(view) => {
                view.read(cx).set_corners(corners);
                let v = view.clone();
                // The focused pane's border: its host's color, else the theme's accent.
                let accent = self
                    .host_color(view.read(cx).session().entry.as_deref())
                    .map(|c| rgb(c).into())
                    .unwrap_or(accent);
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .size_full()
                    .flex()
                    .flex_col()
                    .when(multi, |d| {
                        d.border_t_2().border_color(if view == focused {
                            accent
                        } else {
                            gpui::transparent_black()
                        })
                    })
                    // A click in a pane gives it the keyboard; the terminal sees the
                    // press too, for selection.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _, window, cx| ws.set_focused(v.clone(), window, cx)),
                    )
                    .child(view.clone())
                    .into_any_element()
            }
            Pane::Split(state, a, b) => {
                let horizontal = state.axis == Axis::Horizontal;
                let ratio = state.ratio.get();
                // Side by side, each has one corner; one over the other, the
                // lower has both.
                let (corners_a, corners_b) = if horizontal {
                    (corners.left(), corners.right())
                } else {
                    (chrome::BottomCorners::none(), corners)
                };
                let (s_bounds, s_drag) = (state.clone(), state.clone());
                let first = div()
                    .flex()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(relative(ratio))
                    .flex_grow(0.)
                    .child(self.render_pane(a, focused, multi, accent, corners_a, cx));
                let second = div()
                    .flex()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(relative(1. - ratio))
                    .flex_grow(0.)
                    .child(self.render_pane(b, focused, multi, accent, corners_b, cx));
                let divider = div()
                    .id(ElementId::Name(
                        format!("divider-{:p}", Rc::as_ptr(state)).into(),
                    ))
                    .flex_shrink_0()
                    .bg(rgb(colors().line))
                    .when(horizontal, |d| {
                        d.w(px(3.)).h_full().cursor(CursorStyle::ResizeLeftRight)
                    })
                    .when(!horizontal, |d| {
                        d.h(px(3.)).w_full().cursor(CursorStyle::ResizeUpDown)
                    })
                    .hover(|s| s.bg(accent))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, _, _, cx| {
                            ws.dragging = Some(s_drag.clone());
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    );
                div()
                    .relative()
                    .flex()
                    .when(horizontal, |d| d.flex_row())
                    .when(!horizontal, |d| d.flex_col())
                    .flex_1()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        canvas(
                            move |bounds, _, _| s_bounds.bounds.set(bounds),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(first)
                    .child(divider)
                    .child(second)
                    .into_any_element()
            }
        }
    }

    fn welcome(&self, bg: Hsla, corners: chrome::BottomCorners) -> impl IntoElement {
        let key = |k: &str, what: &str| {
            div()
                .flex()
                .gap_3()
                .child(
                    div()
                        .w(px(150.))
                        .text_color(rgb(colors().soft))
                        .child(k.to_string()),
                )
                .child(
                    div()
                        .text_color(rgb(colors().muted))
                        .child(what.to_string()),
                )
        };
        let (chord, alt) = if cfg!(target_os = "macos") {
            ("Cmd", "Cmd")
        } else {
            ("Ctrl+Shift", "Alt")
        };
        corners
            .round(div())
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(bg)
            .text_sm()
            .child(
                svg()
                    .path("icons/logo.svg")
                    .size(px(56.))
                    .text_color(rgb(colors().accent)),
            )
            .child(
                div()
                    .text_color(rgb(colors().text))
                    .text_lg()
                    .child("Choose a host on the left to connect"),
            )
            .child(div().text_color(rgb(colors().muted)).child(format!(
                "or press {} to find one, or to connect to user@host",
                if cfg!(target_os = "macos") {
                    "Cmd+K"
                } else {
                    "Ctrl+Shift+K"
                }
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pt_2()
                    .child(key(
                        &format!("{chord}+K"),
                        "find a host, connect, run an action",
                    ))
                    .child(key(&format!("{chord}+T"), "a new tab"))
                    .child(key(&format!("{chord}+W"), "close the tab"))
                    .child(key("Ctrl+Tab", "next tab"))
                    .child(key(&format!("{alt}+1…9"), "go to a tab"))
                    .child(key(&format!("{chord}+B"), "show or hide the host list")),
            )
            .when_some(self.notice.clone(), |d, n| {
                d.child(div().pt_2().text_color(rgb(colors().warn)).child(n))
            })
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The UI font: every rem-sized text follows its size; the terminal has its own.
        let ui_font = self.live.settings().appearance.ui_font.clone();
        window.set_rem_size(look::rem_for(ui_font.size));
        let look = match self.active_tab() {
            Some(t) => t.read(cx).look().clone(),
            None => self.live.look_for(None).0,
        };
        let bg = hsla(theme_rgb(look.theme.background));
        let accent = hsla(theme_rgb(look.theme.accent));
        // What fills the window's bottom corners rounds them.
        let bottom = chrome::BottomCorners::of(window);
        if chrome::BottomCorners::current(cx) != bottom {
            cx.set_global(bottom);
        }
        let main_corners = if self.show_sidebar {
            bottom.right()
        } else {
            bottom
        };
        let tabs = self.tab_strip(cx);
        let main: AnyElement = match self.tabs.get(self.active) {
            Some(tab) => {
                let multi = tab.views().len() > 1;
                let pane = match &tab.root {
                    Some(root) => {
                        self.render_pane(root, &tab.focused, multi, accent, main_corners, cx)
                    }
                    None => div().into_any_element(),
                };
                let strip = tab.broadcast.then(|| {
                    let n = tab.views().len();
                    self.broadcast_strip(n - tab.left_out.len(), n, cx)
                });
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(strip)
                    .child(pane)
                    .into_any_element()
            }
            None => self.welcome(bg, main_corners).into_any_element(),
        };
        let palette = self.palette.as_ref().map(|(p, ..)| p.clone());
        let side_w = if self.sidebar_collapsed {
            sidebar::RAIL_WIDTH
        } else {
            self.sidebar_width
        };
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .when(self.show_sidebar, |d| {
                // Its ground is drawn here, where the window's corner can be
                // rounded: a cached view is laid out by its parent, not painted.
                d.child(
                    bottom
                        .left()
                        .round(
                            div()
                                .w(px(side_w))
                                .h_full()
                                .flex_shrink_0()
                                .bg(rgb(colors().panel)),
                        )
                        .child(
                            self.sidebar
                                .clone()
                                .cached(gpui::StyleRefinement::default().size_full()),
                        ),
                )
            })
            .child(main)
            // The host list's edge: drag to resize, double-click for the default.
            .when(self.show_sidebar && !self.sidebar_collapsed, |d| {
                let dragging = self.sidebar_drag.is_some();
                d.child(
                    div()
                        .id("sidebar-edge")
                        .group("sidebar-edge")
                        .absolute()
                        .top_0()
                        .h_full()
                        .left(px(side_w - 3.))
                        .w(px(6.))
                        .flex()
                        .justify_center()
                        .cursor(CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, e: &gpui::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                if e.click_count >= 2 {
                                    ws.sidebar_drag = None;
                                    ws.sidebar_width = sidebar::WIDTH;
                                    ws.save_sidebar(window, cx);
                                } else {
                                    ws.sidebar_drag =
                                        Some((f32::from(e.position.x), ws.sidebar_width));
                                }
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
                                .group_hover("sidebar-edge", |s| {
                                    s.w(px(2.)).bg(rgb(colors().accent))
                                }),
                        ),
                )
            })
            .when(self.sidebar_drag.is_some(), |d| {
                d.child(
                    div()
                        .absolute()
                        .top(relative(0.45))
                        .left(px(side_w + 12.))
                        .px(px(9.))
                        .py(px(3.))
                        .rounded_md()
                        .bg(rgb(colors().accent))
                        .text_color(rgb(colors().on_accent))
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(format!("{} px", side_w.round())),
                )
            })
            .when_some(self.editor.as_ref().map(|(e, _)| e.clone()), |d, e| {
                let quiet = e.clone();
                d.child(
                    bottom
                        .round(div())
                        .id("editor-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(rgba(colors().scrim))
                        // Beside the drawer: closes it, unless that would lose edits.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |ws, _, window, cx| {
                                if quiet.read(cx).can_close_quietly(cx) {
                                    ws.close_editor(window, cx);
                                }
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .h_full()
                                .w(px(host_editor::WIDTH))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(e),
                        ),
                )
            })
            .when_some(
                self.group_editor.as_ref().map(|(e, _)| e.clone()),
                |d, e| {
                    let quiet = e.clone();
                    d.child(
                        bottom
                            .round(div())
                            .id("group-editor-backdrop")
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .bg(rgba(colors().scrim))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |ws, _, window, cx| {
                                    if quiet.read(cx).can_close_quietly(cx) {
                                        ws.close_group_editor(window, cx);
                                    }
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .h_full()
                                    .w(px(host_editor::WIDTH))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(e),
                            ),
                    )
                },
            )
            .when_some(self.settings.as_ref().map(|(s, _)| s.clone()), |d, s| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(s),
                )
            })
            .when_some(palette, |d, p| {
                d.child(
                    bottom
                        .round(div())
                        .id("palette-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .justify_center()
                        // At its own height: a flex row would stretch it to the bottom.
                        .items_start()
                        .pt(px(56.))
                        .bg(rgba(colors().scrim))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _, window, cx| ws.close_palette(window, cx)),
                        )
                        .child(p),
                )
            });
        let root = if matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        ) {
            chrome::window_chrome(tabs, body, bg, window)
        } else {
            // A native title bar: the tabs get a row of their own.
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(bg)
                .child(
                    div()
                        .h(px(TITLEBAR_H))
                        .flex_shrink_0()
                        .bg(rgb(colors().chrome))
                        .border_b_1()
                        .border_color(rgb(colors().line))
                        .child(tabs),
                )
                .child(body)
        };
        root.when_some(ui_font.family, |d, f| d.font_family(f))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::shortcut))
            .when(self.sidebar_drag.is_some(), |d| {
                d.on_mouse_move(cx.listener(|ws, e: &MouseMoveEvent, _, cx| {
                    if let Some((x0, w0)) = ws.sidebar_drag {
                        ws.sidebar_width = (w0 + f32::from(e.position.x) - x0)
                            .clamp(sidebar::MIN_WIDTH, sidebar::MAX_WIDTH);
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|ws, _, window, cx| {
                        ws.sidebar_drag = None;
                        ws.save_sidebar(window, cx);
                        cx.notify();
                    }),
                )
            })
            .when(self.dragging.is_some(), |d| {
                d.on_mouse_move(cx.listener(|ws, e: &MouseMoveEvent, _, cx| {
                    if let Some(s) = &ws.dragging {
                        s.drag_to(e.position);
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|ws, _, _, cx| {
                        ws.dragging = None;
                        ws.layout_changed(cx);
                        cx.notify();
                    }),
                )
            })
            .text_color(rgb(colors().text))
    }
}

/// The drawn title bar's close button: as the system's, asking first while connected.
pub fn close_window(window: &mut Window, cx: &mut gpui::App) {
    let ws = window.root::<Workspace>().flatten();
    if ws.is_none_or(|ws| ws.update(cx, |ws, cx| ws.may_quit(window, cx))) {
        window.remove_window();
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};

    fn key(key: &str, control: bool, shift: bool, alt: bool) -> Option<Shortcut> {
        Workspace::shortcut_for(&KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers {
                    control,
                    shift,
                    alt,
                    ..Modifiers::default()
                },
                key: key.into(),
                key_char: None,
            },
            is_held: false,
            prefer_character_input: false,
        })
    }

    #[test]
    fn window_shortcuts() {
        assert_eq!(key("t", true, true, false), Some(Shortcut::NewTab));
        assert_eq!(key("w", true, true, false), Some(Shortcut::Close));
        assert_eq!(key("b", true, true, false), Some(Shortcut::Sidebar));
        assert_eq!(key("tab", true, false, false), Some(Shortcut::Step(1)));
        assert_eq!(key("tab", true, true, false), Some(Shortcut::Step(-1)));
        assert_eq!(key("pagedown", true, false, false), Some(Shortcut::Step(1)));
        // Ctrl+Shift+] arrives as "}" with only Ctrl.
        assert_eq!(key("}", true, false, false), Some(Shortcut::Theme(1)));
        assert_eq!(key("3", false, false, true), Some(Shortcut::Goto(2)));
        assert_eq!(
            key("d", true, true, false),
            Some(Shortcut::Split(Axis::Horizontal))
        );
        assert_eq!(
            key("e", true, true, false),
            Some(Shortcut::Split(Axis::Vertical))
        );
        assert_eq!(
            key("left", false, false, true),
            Some(Shortcut::Focus(Dir::Left))
        );
        assert_eq!(key("k", true, true, false), Some(Shortcut::Palette));
    }

    #[test]
    fn terminal_keys_pass_through() {
        assert_eq!(
            key("t", true, false, false),
            None,
            "Ctrl+T is for the shell"
        );
        assert_eq!(key("w", true, false, false), None, "Ctrl+W deletes a word");
        assert_eq!(key("tab", false, false, false), None);
        assert_eq!(
            key("pageup", false, true, false),
            None,
            "Shift+PageUp scrolls"
        );
        assert_eq!(key("0", false, false, true), None);
        assert_eq!(key("k", true, false, false), None, "Ctrl+K kills the line");
        assert_eq!(key("d", true, false, false), None, "Ctrl+D ends the shell");
        assert_eq!(
            key("left", false, true, true),
            None,
            "Alt+Shift+Left selects"
        );
    }
}

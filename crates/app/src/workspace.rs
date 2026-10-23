//! The window: tabs in the title bar, the host list on the left, the active
//! tab's terminal, and what concerns every tab (settings, the window's place).

use std::sync::Arc;
use std::time::Duration;

use std::rc::Rc;

use gpui::{
    AnyElement, Axis, Context, CursorStyle, Div, ElementId, Entity, FocusHandle, Focusable, Hsla,
    KeyDownEvent, MouseButton, MouseMoveEvent, PromptLevel, Render, SharedString, Subscription,
    Window, WindowControlArea, canvas, div, prelude::*, px, relative, rgb, rgba, svg,
};
use shellwing_config::{
    EditFile, Host, Hotkey, Keymap, PaneState, SessionState, TabState, WindowState,
};
use shellwing_ssh::config as ssh_config;
use tokio::runtime::Handle;

use crate::bench::Probe;
use crate::chrome::{self, TITLEBAR_H};
use crate::colors::colors;
use crate::files::EditorCommand;
use crate::group_editor::{GroupEditor, GroupEvent};
use crate::host_editor::{self, EditorContext, EditorEvent, HostEditor};
use crate::import_view::{ImportEvent, ImportView, Imported};
use crate::look::{self, Live, rgb as theme_rgb};
use crate::notice::{Notice, colored};
use crate::palette::{Action, Entry, Kind, Palette, PaletteEvent, Part};
use crate::pane::{Dir, Pane, SplitState, neighbor};
use crate::secrets::VaultSlot;
use crate::session::{Opens, Session, Status};
use crate::settings_view::{self, Page, SettingsEvent, SettingsView, Snap};
use crate::sidebar::{self, Row, Section, Sidebar, dot};
use crate::snippets;
use crate::sync_panel::{Standing, SyncPanel, SyncPanelEvent, SyncSnap};
use crate::syncing::{SyncEvent, SyncWorker};
use crate::tab_overview::{OverviewEvent, TabOverview, TabRow};
use crate::target;
mod config_files;
mod notes;
mod snippet_windows;
mod updates;
pub(crate) use updates::{Stage as UpdateStage, UpdateSnap, by_said};

use crate::view::{
    CloseRequested, FilesResized, ListenToggled, PaneCommand, StatusChanged, TermView,
    TitleChanged, Unplaced, UseKey, VaultChanged, hsla,
};
use shellwing_config::SyncPlace;
use shellwing_i18n::{tr, tr_n};
use shellwing_vault::FolderState;
use shellwing_vault::{AskState, Request};

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
    /// Every open tab, by host.
    Tabs,
}

/// A tab being dragged to another place in the strip.
struct DraggedTab {
    from: usize,
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
    notice: Option<Notice>,
    /// A quit or close question is showing (GPUI allows one prompt at a time).
    prompting: bool,
    /// The open palette, and whether it is picking a theme (whose preview must
    /// be undone if nothing is picked).
    palette: Option<(Entity<Palette>, Subscription, bool)>,
    /// Every open tab, by host, under the header's tab button.
    overview: Option<(Entity<TabOverview>, Subscription)>,
    /// Where the tab button is, for the overview to hang from.
    overview_button: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    /// The sync panel, while open, and where its cloud button is.
    sync_panel: Option<(Entity<SyncPanel>, Subscription)>,
    sync_button: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    /// When this run last synced, and whether a sync runs now.
    last_synced: Option<std::time::SystemTime>,
    syncing: bool,
    /// The split whose divider is being dragged.
    dragging: Option<Rc<SplitState>>,
    /// The window's size and place, waiting to be saved, and which change it is.
    unsaved_bounds: Option<WindowState>,
    bounds_change: u64,
    /// The system is light (with `mode = "system"`, so is the app).
    system_light: bool,
    /// The window, for questions that come from the sync thread.
    window_handle: Option<gpui::AnyWindowHandle>,
    /// Pairing requests already put to the user.
    prompted: std::collections::HashSet<String>,
    /// Devices asking to join while shellwing is locked: asked about once
    /// it is the user again.
    held_requests: Vec<Request>,
    /// Counts changes to the tabs, to save them shortly after the last one.
    layout_change: u64,
    vault: VaultSlot,
    /// A vault that failed to open was reported.
    vault_reported: bool,
    /// The locked vault was asked about at start.
    vault_asked: bool,
    /// Looking for a newer shellwing, and what was found.
    updates: updates::Updates,
    /// The last key pressed or click, for locking the vault, and shellwing,
    /// when idle.
    last_input: std::time::Instant,
    /// shellwing locked (`[lock]`): what covers the window until the system
    /// says it is the user.
    lock: Option<(Entity<crate::app_lock::LockView>, Subscription)>,
    /// Windows Hello or Touch ID is asking now, to open the vault.
    presence_asking: bool,
    sync: SyncWorker,
    /// The last sync error shown, so it is not repeated every minute.
    sync_error: Option<String>,
    /// A passphrase joins new devices where the vault syncs, as the sync
    /// thread last found.
    passphrase_joins: Option<bool>,
    /// What the palette's question is for, while one is asked.
    asking: Option<Asking>,
    /// The host editor drawer, while open.
    editor: Option<(Entity<HostEditor>, Subscription)>,
    /// The group editor drawer, while open.
    group_editor: Option<(Entity<GroupEditor>, Subscription)>,
    /// Importing hosts from another app.
    import: Option<(Entity<ImportView>, Subscription)>,
    /// What the last import put in, with its notes' ids: *Undo import* takes
    /// it out, until the app quits.
    last_import: Option<(Imported, Vec<String>)>,
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
    /// A place holding another vault, waiting for its recovery key.
    RecoveryKey(SyncPlace),
    /// A place holding another vault: join it by pairing, with its key, or
    /// (if it lets one) with its passphrase?
    JoinHow { place: SyncPlace, passphrase: bool },
    /// Asking a device that syncs there to let this one in.
    Pairing(SyncPlace),
    /// A locked vault's passphrase or recovery key.
    VaultUnlock(shellwing_vault::Lock),
    /// A passphrase for the vault, typed once (`None`) or a second time to
    /// check it; `new` makes the vault, else sets it on the open one.
    VaultPassphrase { first: Option<String>, new: bool },
    /// A passphrase that lets new devices join the vault in the sync folder,
    /// typed once or a second time.
    FolderPassphrase { first: Option<String> },
    /// A place whose vault a passphrase joins, waiting for it.
    JoinPassphrase(SyncPlace),
    /// A port to forward on the active tab's connection.
    Forward,
    /// A value a snippet needs before it runs.
    Snippet(SnippetRun),
    /// Where on the host to place a config file.
    PlacePath(shellwing_vault::ConfigFile),
    /// How to place it there (`full`), in `view`'s host.
    PlaceHow {
        file: shellwing_vault::ConfigFile,
        full: String,
        view: gpui::WeakEntity<TermView>,
    },
    /// A file of the host to keep in the vault.
    SaveFile,
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
            // Tests keep the vault's key in memory: the system keychain's
            // entry is the user's own app's, the same service and account.
            #[cfg(test)]
            Some(store) => VaultSlot::open_with(
                store.dir().join("vault.db"),
                shellwing_vault::MemoryKeys::default(),
            ),
            #[cfg(not(test))]
            Some(store) => VaultSlot::open(store.dir().join("vault.db")),
            None => VaultSlot::none(),
        };
        let (sync_tx, sync_rx) = async_channel::unbounded();
        let sync = SyncWorker::start(vault.clone(), live.store().cloned(), sync_tx);
        sync.set_place(live.sync_place());
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
            overview: None,
            overview_button: Rc::default(),
            sync_panel: None,
            sync_button: Rc::default(),
            last_synced: None,
            syncing: false,
            dragging: None,
            unsaved_bounds: None,
            bounds_change: 0,
            system_light: false,
            window_handle: None,
            prompted: Default::default(),
            held_requests: Vec::new(),
            layout_change: 0,
            vault,
            vault_reported: false,
            vault_asked: false,
            updates: Default::default(),
            last_input: std::time::Instant::now(),
            lock: None,
            presence_asking: false,
            sync,
            sync_error: None,
            passphrase_joins: None,
            asking: None,
            editor: None,
            group_editor: None,
            import: None,
            last_import: None,
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
        ws.start_update_checks(cx);
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
                let _ = this.update(cx, |ws, cx| ws.say(Notice::warn(msg), KEPT_NOTICE, cx));
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

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Syncs at once, as *Sync now* does, without saying so.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn sync_now(&self) {
        self.sync.now();
    }

    /// The vault, for the UI tests to wait on and look into.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn vault_slot(&self) -> VaultSlot {
        self.vault.clone()
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn pane_count(&self) -> usize {
        self.tabs.get(self.active).map_or(0, |t| t.views().len())
    }

    /// The notice at the bottom of the window, if one shows.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn notice_shown(&self) -> Option<Notice> {
        self.notice.clone()
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn overview_open(&self) -> bool {
        self.overview.is_some()
    }

    /// The Settings page showing, if Settings is open.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn settings_page(&self, cx: &gpui::App) -> Option<Page> {
        self.settings.as_ref().map(|(v, _)| v.read(cx).page())
    }

    /// Whether shellwing is locked: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn app_locked(&self) -> bool {
        self.lock.is_some()
    }

    /// Devices asking to join, held while locked: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn held_requests(&self) -> usize {
        self.held_requests.len()
    }

    /// What the lock says now: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn lock_said(&self, cx: &gpui::App) -> Option<String> {
        self.lock.as_ref().and_then(|(v, _)| v.read(cx).said())
    }

    /// As if nothing was pressed or clicked for `d`: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn away_for(&mut self, d: std::time::Duration) {
        if let Some(then) = std::time::Instant::now().checked_sub(d) {
            self.last_input = then;
        }
    }

    /// Settings, if open: for the UI tests to read.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn settings_view(&self) -> Option<Entity<crate::settings_view::SettingsView>> {
        self.settings.as_ref().map(|(v, _)| v.clone())
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn host_editor_open(&self) -> bool {
        self.editor.is_some()
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn group_editor_open(&self) -> bool {
        self.group_editor.is_some()
    }

    /// The import drawer, while open.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn import_view(&self) -> Option<Entity<ImportView>> {
        self.import.as_ref().map(|(v, _)| v.clone())
    }

    /// The host list as shown: each section's title and its hosts' names.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn host_list(&self, cx: &gpui::App) -> Vec<(String, Vec<String>)> {
        self.sidebar.read(cx).shown_names(cx)
    }

    /// The focused pane of the active tab: for the UI tests to read.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn active_view(&self) -> Entity<TermView> {
        self.tabs[self.active].focused.clone()
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

    /// The focused pane's host and its group, which snippets and
    /// environments are offered for.
    fn pane_scope(&self, cx: &Context<Self>) -> Option<(String, Option<String>)> {
        let s = self.active_tab()?.read(cx).session();
        let host = s.entry.clone().unwrap_or_else(|| s.spec.clone());
        let group = s
            .entry
            .as_deref()
            .and_then(|n| self.live.hosts().find(n))
            .and_then(|h| h.group.clone());
        Some((host, group))
    }

    /// The environments that can be loaded in the focused pane.
    fn pane_envs(&self, cx: &Context<Self>) -> Vec<shellwing_vault::Environment> {
        let mut envs: Vec<_> = self
            .vault
            .ready()
            .and_then(|v| v.environments().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|e| self.fits_pane(&e.scope, cx))
            .collect();
        envs.sort_by_key(|e| e.name.to_lowercase());
        envs
    }

    /// Whether something offered for `scope` is for the focused pane's
    /// host; with no pane, everything is (listed, to say what it needs).
    pub(super) fn fits_pane(&self, scope: &str, cx: &Context<Self>) -> bool {
        match self.pane_scope(cx) {
            Some((host, group)) => {
                snippets::Scope::parse(scope).fits(Some(&host), group.as_deref())
            }
            None => true,
        }
    }

    /// Why the focused pane cannot take an environment or a config file
    /// (`ssh`: over SSH only) now, if so: said in the palette, and on Enter.
    pub(super) fn pane_unready(&self, ssh: bool, cx: &Context<Self>) -> Option<String> {
        let Some(view) = self.active_tab() else {
            return Some(tr!("Open a tab first"));
        };
        let s = view.read(cx).session();
        if s.status() != Status::Connected {
            return Some(tr!("Connect first"));
        }
        (ssh && s.is_local()).then(|| tr!("Over SSH only"))
    }

    /// The snippets the palette offers for the focused pane's host.
    fn snippet_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let panes = self.snippet_targets(true).len();
        let no_tab = self.active_tab().is_none();
        let mut list: Vec<_> = self
            .vault
            .ready()
            .and_then(|v| v.snippets().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|sn| self.fits_pane(&sn.scope, cx))
            .collect();
        // By category, those without first, as in Settings.
        list.sort_by_key(|sn| (sn.category.trim().to_lowercase(), sn.name.to_lowercase()));
        let first = (list.is_empty() && self.vault.ready().is_some()).then(|| {
            Entry::new(
                Kind::Action(Action::NewSnippet),
                tr!("Snippets"),
                tr!("New snippet…"),
            )
            .hint("Enter")
            .section(Part::Snippets)
        });
        list.into_iter()
            .map(|sn| {
                let category = sn.category.trim();
                Entry::new(
                    Kind::Snippet(sn.id.clone()),
                    if category.is_empty() {
                        tr!("Snippets")
                    } else {
                        tr!("Snippets · {category}", category = category)
                    },
                    sn.name.clone(),
                )
                .detail(snippets::one_line(&sn.command))
                .section(Part::Snippets)
                .hint(if no_tab {
                    tr!("Open a tab first")
                } else if panes > 1 {
                    tr_n!(
                        panes,
                        "Enter · Alt+Enter: {n} pane",
                        "Enter · Alt+Enter: {n} panes"
                    )
                } else {
                    "Enter".to_string()
                })
            })
            .chain(first)
            .chain(std::iter::once(
                Entry::new(
                    Kind::Action(Action::SnippetLibrary),
                    tr!("Snippets"),
                    tr!("Snippet library…"),
                )
                .detail(tr_n!(
                    crate::library::items().len(),
                    "{n} ready command to add",
                    "{n} ready commands to add"
                ))
                .hint("Enter")
                .section(Part::Snippets),
            ))
            .collect()
    }

    fn run_snippet(&mut self, id: &str, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_none() {
            return self.say(
                Notice::warn(tr!("Open a tab first: a snippet runs in the focused pane.")),
                CONFIG_NOTICE,
                cx,
            );
        }
        let Some(snippet) = self
            .vault
            .ready()
            .and_then(|v| v.snippets().ok())
            .and_then(|l| l.into_iter().find(|s| s.id == id))
        else {
            return;
        };
        // Kept within the limit, whatever device saved it.
        if snippet.command.len() > shellwing_vault::limits::COMMAND {
            let o = shellwing_vault::Oversize::Command(snippet.command.len());
            return self.say(
                Notice::warn(crate::environment::said_oversize(&o)),
                CONFIG_NOTICE,
                cx,
            );
        }
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
                if run.snippet.needs.is_empty() {
                    v.update(cx, |v, _| v.type_text(&text, run.snippet.enter));
                    continue;
                }
                // In a subshell that has the environments, run at once: it
                // waits for their values, sent once it is ready.
                let values = match self.env_values(&run.snippet.needs) {
                    Ok(values) => values,
                    Err(e) => return self.say(Notice::bad(e), CONFIG_NOTICE, cx),
                };
                if let Err(e) = v.update(cx, |v, cx| v.run_with_env(values, &text, true, cx)) {
                    v.update(cx, |v, cx| v.show_notice(Notice::bad(e), CONFIG_NOTICE, cx));
                }
            }
            return;
        };
        let total = snippets::asked(&run.snippet.command).len();
        let at = total - run.left.len() + 1;
        let panes = match targets.len() {
            1 => tr!("in this pane"),
            n => tr_n!(n, "in {n} pane", "in {n} panes"),
        };
        let notes = vec![
            Entry::new(
                Kind::Note,
                "",
                tr!(
                    "{snippet} · {panes} · {name} ({at} of {total})",
                    snippet = run.snippet.name.clone(),
                    panes = panes,
                    name = name.clone(),
                    at = at,
                    total = total
                ),
            ),
            Entry::new(Kind::Note, "", tr!("Will run")).detail(snippets::one_line(
                &snippets::fill(&run.snippet.command, &run.values),
            )),
        ];
        self.asking = Some(Asking::Snippet(run));
        let palette = cx.new(|cx| Palette::ask(&name, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// The variables of the environments `ids`, together (a later one's
    /// value wins).
    fn env_values(&self, ids: &[String]) -> Result<shellwing_vault::EnvValues, String> {
        let vault = self
            .vault
            .ready()
            .ok_or_else(|| tr!("The vault is not open."))?;
        let mut all: shellwing_vault::EnvValues = vec![];
        for id in ids {
            let values = vault
                .environment_values(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| tr!("An environment this needs was deleted: edit the snippet."))?;
            for (name, value) in values {
                all.retain(|(n, _)| *n != name);
                all.push((name, value));
            }
        }
        Ok(all)
    }

    /// The environments the palette offers to load into the focused pane,
    /// while it is connected.
    fn env_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let unready = self.pane_unready(false, cx);
        let loaded: Vec<String> = self
            .active_tab()
            .map(|v| {
                v.read(cx)
                    .loaded_envs()
                    .iter()
                    .map(|e| e.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.pane_envs(cx)
            .into_iter()
            .map(|e| {
                let on = loaded.contains(&e.id);
                Entry::new(Kind::Env(e.id.clone()), tr!("Environments"), e.name.clone())
                    .detail(e.names.join(" "))
                    .hint(if let Some(why) = &unready {
                        why.clone()
                    } else if on {
                        tr!("loaded: Enter loads it again")
                    } else {
                        "Enter".into()
                    })
                    .section(Part::Environments)
            })
            .collect()
    }

    /// Loads environment `id` into the focused pane.
    fn load_env(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(why) = self.pane_unready(false, cx) {
            return self.say(Notice::warn(why), CONFIG_NOTICE, cx);
        }
        let Some(view) = self.active_tab().cloned() else {
            return;
        };
        let Some(env) = self.pane_envs(cx).into_iter().find(|e| e.id == id) else {
            return;
        };
        let done = self
            .env_values(std::slice::from_ref(&env.id))
            .and_then(|values| {
                view.update(cx, |v, cx| {
                    v.load_env(
                        crate::view::LoadedEnv {
                            id: env.id.clone(),
                            name: env.name.clone(),
                            names: vec![],
                            bytes: 0,
                        },
                        values,
                        cx,
                    )
                })
            });
        if let Err(e) = done {
            self.say(Notice::bad(e), CONFIG_NOTICE, cx);
        }
    }

    /// Types in every pane of the active tab, or stops.
    fn toggle_broadcast(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if !tab.broadcast && tab.views().len() < 2 {
            return self.say(
                Notice::warn(tr!("Split the tab first: there is one pane to type in.")),
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
            &tr!("Close the tab?"),
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
            &tr!("Close the tab?")
        } else {
            &tr!("Close the pane?")
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
        question: &str,
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
        // The files placed until the tab closes go first.
        let views = views.to_vec();
        if open.is_empty() {
            return self.take_down_then(views, window, cx, close);
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let detail = match open.as_slice() {
            [one] => tr!(
                "The connection to {one} is open; closing ends it.",
                one = one
            ),
            _ => tr_n!(
                open.len(),
                "{n} connection is open; closing ends it.",
                "{n} connections are open; closing ends them."
            ),
        };
        let (close_label, cancel_label) = (tr!("Close"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            question,
            Some(&detail),
            &[close_label.as_str(), cancel_label.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let yes = matches!(answer.await, Ok(0));
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.prompting = false;
                if yes {
                    ws.take_down_then(views, window, cx, close);
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
                self.notice = Some(Notice::bad(tr!(
                    "Cannot open a terminal: {e}",
                    e = e.to_string()
                )));
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
        cx.subscribe(&view, |ws, view, _: &StatusChanged, cx| {
            ws.refresh_sidebar(cx);
            ws.run_installs(cx);
            if view.read(cx).session().status() == Status::Connected {
                ws.tidy_placed(&view, cx);
            }
        })
        .detach();
        cx.subscribe(&view, |ws, view, ev: &Unplaced, cx| {
            let spec = view.read(cx).session().spec.clone();
            ws.forget_placed(&spec, &ev.0);
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
        // The tab, and the window while it is the active one, show the
        // shell's title.
        cx.subscribe_in(&view, window, |ws, _, _: &TitleChanged, window, cx| {
            ws.title_window(window, cx);
            cx.notify();
        })
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
                PaneCommand::Snippets => ws.open_palette_at(Some(Part::Snippets), window, cx),
                PaneCommand::Environments => {
                    ws.open_palette_at(Some(Part::Environments), window, cx)
                }
                PaneCommand::ConfigFiles => ws.open_palette_at(Some(Part::ConfigFiles), window, cx),
                PaneCommand::SaveFile => ws.ask_save_file(window, cx),
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
            view.update(cx, |v, cx| {
                v.show_notice(Notice::warn(p), CONFIG_NOTICE, cx)
            });
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
        // Locked from the start: whoever opens it is asked, at once.
        if self.lock_after() > 0 {
            self.lock_app(true, window, cx);
        }
    }

    /// Minutes away before shellwing locks itself (`[lock] after`); 0:
    /// never by itself.
    fn lock_after(&self) -> u32 {
        self.live.settings().lock.after.unwrap_or(0)
    }

    /// Locks shellwing: its windows hidden, and the keyboard kept from them,
    /// until the system says it is the user. With `ask_now` the system asks
    /// at once; else when the user comes back and asks.
    pub fn lock_app(&mut self, ask_now: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.lock.is_some() {
            return;
        }
        let confirmer = crate::app_lock::confirmer(cx);
        let Some(how) = confirmer.how() else {
            return self.say(
                Notice::warn(tr!(
                    "shellwing cannot lock here: this system has no way to ask that it is you."
                )),
                CONFIG_NOTICE,
                cx,
            );
        };
        // A question drawn over the lock would still take an answer. The
        // palette, under it, stays: what it asks (the vault's passphrase) is
        // asked again after.
        crate::prompt::dismiss_all(cx);
        crate::note_window::before_lock(cx);
        crate::snippet_window::before_lock(cx);
        cx.set_global(crate::app_lock::Locked(true));
        let view = cx.new(|cx| crate::app_lock::LockView::new(confirmer, how, ask_now, window, cx));
        let events = cx.subscribe_in(
            &view,
            window,
            |ws, _, e: &crate::app_lock::LockEvent, window, cx| match e {
                crate::app_lock::LockEvent::Unlocked => ws.unlock_app(window, cx),
                // As any quit: asking first while connected.
                crate::app_lock::LockEvent::Quit => {
                    if ws.may_quit(window, cx) {
                        cx.quit();
                    }
                }
            },
        );
        self.lock = Some((view, events));
        self.title_window(window, cx);
        cx.refresh_windows();
        cx.notify();
    }

    /// It is the user: the windows show again.
    fn unlock_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lock = None;
        cx.set_global(crate::app_lock::Locked(false));
        self.last_input = std::time::Instant::now();
        let active = self.active;
        self.activate(active, window, cx);
        if let Some((palette, ..)) = &self.palette {
            window.focus(&palette.focus_handle(cx), cx);
        }
        for r in std::mem::take(&mut self.held_requests) {
            if self.prompted.insert(r.id.clone()) {
                self.in_window(cx, move |ws, window, cx| ws.prompt_pairing(r, window, cx));
            }
        }
        cx.refresh_windows();
    }

    /// While locked, the lock has the keyboard, whatever took it meanwhile;
    /// not a question asked over it (quitting), drawn outside the window's
    /// tree.
    fn keep_keys_locked(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((lock, _)) = &self.lock else {
            return;
        };
        let lock = lock.clone();
        let here = self.focus.contains_focused(window, cx) || window.focused(cx).is_none();
        if here && !lock.read(cx).focus_handle(cx).contains_focused(window, cx) {
            lock.update(cx, |v, cx| v.take_keys(window, cx));
        }
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
            window.focus(&tab.focus_handle(cx), cx);
        } else {
            window.focus(&self.focus, cx);
        }
        self.keep_keys_locked(window, cx);
        self.title_window(window, cx);
        cx.notify();
    }

    /// Moves a tab to `to`, the others making room; the active one stays
    /// active.
    fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let active = self.active_tab().map(|t| t.entity_id());
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if let Some(id) = active {
            self.active = self
                .tabs
                .iter()
                .position(|t| t.focused.entity_id() == id)
                .unwrap_or(self.active);
        }
        self.layout_changed(cx);
        cx.notify();
    }

    /// The window's title: the active tab's host, and what its shell says.
    fn title_window(&self, window: &mut Window, cx: &mut Context<Self>) {
        // Locked, it says no host.
        if self.lock.is_some() {
            return window.set_window_title("shellwing");
        }
        let title = match self.active_tab() {
            Some(t) => {
                let v = t.read(cx);
                match v.title_detail() {
                    Some(d) => format!("{}: {d} — shellwing", v.session().title),
                    None => format!("{} — shellwing", v.session().title),
                }
            }
            None => "shellwing".into(),
        };
        window.set_window_title(&title);
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
            // Notes changed in their windows sync first.
            self.finish_then_quit(cx);
            return false;
        }
        if !self.prompting {
            self.prompting = true;
            let detail = if open == 1 {
                tr!("A connection is open; quitting closes it.")
            } else {
                tr_n!(
                    open,
                    "{n} connection is open; quitting closes it.",
                    "{n} connections are open; quitting closes them."
                )
            };
            // Restarting to install an update says so.
            let restart = self.updates.relaunch;
            let (quit_label, cancel_label) = if restart {
                (tr!("Restart"), tr!("Cancel"))
            } else {
                (tr!("Quit"), tr!("Cancel"))
            };
            let title = if restart {
                tr!("Restart to install the update?")
            } else {
                tr!("Quit shellwing?")
            };
            let answer = window.prompt(
                PromptLevel::Warning,
                &title,
                Some(&detail),
                &[quit_label.as_str(), cancel_label.as_str()],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let quit = matches!(answer.await, Ok(0));
                let _ = this.update_in(cx, |ws, window, cx| {
                    ws.prompting = false;
                    if !quit {
                        ws.updates.relaunch = false;
                    }
                    if quit {
                        // The files placed until their tab closes go first.
                        let views = ws.tabs.iter().flat_map(|t| t.views()).collect();
                        ws.take_down_then(views, window, cx, |ws, _, cx| ws.finish_then_quit(cx));
                    }
                });
            })
            .detach();
        }
        false
    }

    /// Quits once the notes and snippets open in their windows are saved
    /// and, when any changed, synced (10 s at most: a place that does not
    /// answer does not hold the app).
    fn finish_then_quit(&mut self, cx: &mut Context<Self>) {
        let notes = crate::note_window::finish_all(cx);
        let snippets = crate::snippet_window::finish_all(cx);
        if !(notes || snippets) || self.sync.place().is_none() {
            return cx.quit();
        }
        self.say(tr!("Syncing the notes before quitting…"), THEME_NOTICE, cx);
        let sync = self.sync.clone();
        let job =
            cx.background_spawn(
                async move { sync.sync_and_wait(std::time::Duration::from_secs(10)) },
            );
        cx.spawn(async move |_, cx| {
            let _ = job.await;
            cx.update(|cx| cx.quit());
        })
        .detach();
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
                self.notice = Some(tr!("Choose a host on the left to connect.").into());
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
                    Entry::new(Kind::Host(r.spec), tr!("Hosts"), r.name)
                        .detail(if r.detail.is_empty() {
                            section.title.clone()
                        } else {
                            format!("{} · {}", r.detail, section.title).into()
                        })
                        .status(r.status)
                        .hint("Enter")
                        .section(Part::Hosts),
                );
            }
        }
        if entries.is_empty() {
            entries.push(
                Entry::new(
                    Kind::Action(Action::NewHost),
                    tr!("Hosts"),
                    tr!("New host…"),
                )
                .hint("Enter")
                .section(Part::Hosts),
            );
        }
        for spec in self.live.store().map(|s| s.recent()).unwrap_or_default() {
            if !specs.contains(&spec) {
                entries.push(
                    Entry::new(Kind::Recent(spec.clone()), tr!("Recent"), spec)
                        .hint("Enter")
                        .section(Part::Recent),
                );
            }
        }
        if self.vault.locked().is_some() {
            // Not left out without a word: they are in the locked vault.
            entries.push(
                Entry::new(
                    Kind::Action(Action::UnlockVault),
                    tr!("Snippets"),
                    tr!("Unlock the vault…"),
                )
                .detail(tr!("Snippets, environments and config files are in it."))
                .hint("Enter")
                .section(Part::Snippets),
            );
        }
        entries.extend(self.snippet_entries(cx));
        // A section with nothing in it offers to make the first one (not
        // while the vault they are kept in is locked).
        let open = self.vault.ready().is_some();
        let first = |a: Action, group: String, title: String, s: Part| {
            Entry::new(Kind::Action(a), group, title)
                .hint("Enter")
                .section(s)
        };
        let envs = self.env_entries(cx);
        if envs.is_empty() && open {
            entries.push(first(
                Action::NewEnvironment,
                tr!("Environments"),
                tr!("New environment…"),
                Part::Environments,
            ));
        }
        entries.extend(envs);
        let files = self.file_entries(cx);
        if files.is_empty() && open {
            entries.push(first(
                Action::NewConfigFile,
                tr!("Config files"),
                tr!("New config file…"),
                Part::ConfigFiles,
            ));
        }
        entries.extend(files);
        entries.extend(self.note_entries(cx));
        let keys = crate::hotkeys::keymap(cx);
        // What is done in the pane or the tab, then what is set.
        let (mut acts, mut sets) = (vec![], vec![]);
        let mut action = |a: Action, title: String, keys: String| {
            acts.push(
                Entry::new(Kind::Action(a), tr!("Actions"), title)
                    .hint(keys)
                    .section(Part::Actions),
            );
        };
        let mut setting = |a: Action, title: String, keys: String| {
            sets.push(
                Entry::new(Kind::Action(a), tr!("Settings"), title)
                    .hint(keys)
                    .section(Part::Settings),
            );
        };
        if let Some(t) = self.active_tab() {
            let title = t.read(cx).session().title.clone();
            action(
                Action::Duplicate,
                tr!("Another tab to {title}", title = title.clone()),
                String::new(),
            );
            action(
                Action::SplitRight,
                tr!("Split right, to {title}", title = title.clone()),
                keys.hint(Hotkey::SplitRight),
            );
            action(
                Action::SplitDown,
                tr!("Split down, to {title}", title = title.clone()),
                keys.hint(Hotkey::SplitDown),
            );
            action(Action::CloseTab, tr!("Close the tab"), String::new());
            if let Some(tab) = self.tabs.get(self.active)
                && tab.views().len() > 1
            {
                action(
                    Action::Broadcast,
                    if tab.broadcast {
                        tr!("Stop typing in every pane")
                    } else {
                        tr!("Type in every pane of this tab")
                    },
                    keys.hint(Hotkey::Broadcast),
                );
            }
            if self.tabs.len() >= 2 {
                action(
                    Action::Tabs,
                    tr!("All {count} tabs, by host…", count = self.tabs.len()),
                    keys.hint(Hotkey::Tabs),
                );
            }
            action(
                Action::Find,
                tr!("Find in {title}…", title = title.clone()),
                keys.hint(Hotkey::Find),
            );
            action(
                Action::SaveOutput { html: false },
                tr!("Save the output of {title} as text…", title = title.clone()),
                String::new(),
            );
            action(
                Action::SaveOutput { html: true },
                tr!(
                    "Save the output of {title} as HTML, with colors…",
                    title = title.clone()
                ),
                String::new(),
            );
            action(
                Action::Log,
                if t.read(cx).session().log_path().is_some() {
                    tr!("Stop logging {title}", title = title.clone())
                } else {
                    tr!("Log {title} to a file", title = title.clone())
                },
                String::new(),
            );
            action(
                Action::Files,
                if t.read(cx).files_shown() {
                    tr!("Hide the files")
                } else {
                    tr!("Files on {title}", title = title.clone())
                },
                keys.hint(Hotkey::Files),
            );
            if t.read(cx).session().status() == Status::Connected {
                action(
                    Action::Forward,
                    tr!("Forward a port through {title}…", title = title.clone()),
                    "-L / -R / -D".into(),
                );
                let loaded: Vec<String> = t
                    .read(cx)
                    .loaded_envs()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect();
                if !loaded.is_empty() {
                    action(
                        Action::UnloadEnv,
                        tr!("Unload environment: {names}", names = loaded.join(", ")),
                        String::new(),
                    );
                }
            }
        }
        setting(
            Action::ChangeTheme,
            tr!("Change theme…"),
            [Hotkey::NextTheme, Hotkey::PreviousTheme]
                .map(|h| keys.hint(h))
                .into_iter()
                .filter(|k| !k.is_empty())
                .collect::<Vec<_>>()
                .join(" / "),
        );
        setting(
            Action::ToggleHosts,
            if self.show_sidebar {
                tr!("Hide the host list")
            } else {
                tr!("Show the host list")
            },
            keys.hint(Hotkey::HostList),
        );
        setting(
            Action::ManageSecrets,
            tr!("Saved passwords and passphrases…"),
            String::new(),
        );
        if self.vault.locked().is_some() {
            setting(Action::UnlockVault, tr!("Unlock the vault…"), String::new());
        }
        if self.vault.can_lock() {
            setting(Action::LockVault, tr!("Lock the vault"), String::new());
        }
        if crate::app_lock::confirmer(cx).how().is_some() {
            setting(Action::LockApp, tr!("Lock shellwing"), String::new());
        }
        match self.sync.place() {
            Some(f) => {
                setting(
                    Action::SyncNow,
                    tr!(
                        "Sync now with {place}",
                        place = crate::syncing::describe(&f)
                    ),
                    String::new(),
                );
                setting(
                    Action::ShowRecoveryKey,
                    tr!("Show the recovery key"),
                    String::new(),
                );
                setting(Action::StopSync, tr!("Stop syncing"), String::new());
            }
            None => {
                setting(
                    Action::SyncFolder,
                    tr!("Sync passwords with a folder…"),
                    "iCloud Drive, Syncthing, Dropbox…".into(),
                );
                setting(
                    Action::SyncServer,
                    tr!("Sync passwords with a server…"),
                    "WebDAV (Nextcloud, Synology), S3 (AWS, R2, B2), Git".into(),
                );
            }
        }
        setting(
            Action::Settings,
            tr!("Settings…"),
            keys.hint(Hotkey::Settings),
        );
        if let updates::Stage::Ready(v) = &self.updates.stage {
            setting(
                Action::RestartToUpdate,
                tr!(
                    "Restart to install shellwing {version}",
                    version = v.to_string()
                ),
                String::new(),
            );
        } else if let Some(v) = self.update_offered() {
            setting(
                Action::Update,
                tr!("shellwing {version} is out…", version = v.to_string()),
                String::new(),
            );
        }
        setting(
            Action::CheckUpdates,
            tr!("Check for updates"),
            String::new(),
        );
        action(Action::NewHost, tr!("New host…"), String::new());
        action(Action::NewGroup, tr!("New group…"), String::new());
        action(
            Action::ImportHosts,
            tr!("Import hosts from another app…"),
            "PuTTY, MobaXterm, SecureCRT, Xshell, Tabby, CSV".into(),
        );
        if let Some((done, _)) = &self.last_import {
            action(
                Action::UndoImport,
                tr!("Undo import from {app}", app = done.app),
                String::new(),
            );
        }
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
                tr!("Edit host {name}…", name = name),
                String::new(),
            );
        }
        setting(Action::EditHosts, tr!("Edit hosts.toml"), String::new());
        setting(Action::EditSettings, tr!("Edit config.toml"), String::new());
        entries.extend(acts);
        entries.extend(sets);
        entries
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette_at(None, window, cx);
    }

    /// The palette, opened at section `at` (from the pane's menu); a word
    /// instead where the focused pane has none of it.
    fn open_palette_at(&mut self, at: Option<Part>, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.palette_entries(cx);
        if let Some(s) = at
            && !entries.iter().any(|e| e.section == s)
        {
            let said = match s {
                Part::Snippets => Notice::warn(tr!(
                    "No snippet for this host: add one in Settings › Snippets."
                )),
                Part::ConfigFiles => Notice::warn(tr!(
                    "No config file for this host: add one in Settings › Config files."
                )),
                _ => Notice::warn(tr!(
                    "No environment for this host: add one in Settings › Environments."
                )),
            };
            return self.say(said, CONFIG_NOTICE, cx);
        }
        let palette = cx.new(|cx| Palette::connect(entries, at, cx));
        self.show_palette(palette, false, window, cx);
    }

    fn open_theme_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self
            .active_tab()
            .and_then(|t| t.read(cx).session().entry.clone());
        if let Some(t) = self.live.host_theme(entry.as_deref()) {
            let msg = Notice::warn(tr!(
                "This host uses the theme `{t}` from hosts.toml; change it there.",
                t = t
            ));
            self.say(msg, CONFIG_NOTICE, cx);
            return;
        }
        let current = self.live.look_for(None).0.theme.id.clone();
        // By group, as in Settings; while light, the light ones first.
        let mut themes: Vec<_> = self.live.themes().all().iter().collect();
        if self.live.light() {
            themes.sort_by_key(|t| !t.group.as_deref().is_some_and(|g| g.contains("light")));
        }
        let entries = themes
            .into_iter()
            .map(|t| {
                let known = shellwing_config::GROUPS
                    .iter()
                    .any(|(g, _)| Some(*g) == t.group.as_deref());
                let group = crate::look::group_title(t.group.as_deref().filter(|_| known));
                Entry::new(Kind::Theme(t.id.clone()), group, t.name.clone())
            })
            .collect();
        let palette = cx.new(|cx| Palette::themes(entries, &current, cx));
        self.show_palette(palette, true, window, cx);
    }

    fn on_sync(&mut self, ev: SyncEvent, cx: &mut Context<Self>) {
        // The cloud, and its panel, follow.
        match &ev {
            SyncEvent::Started => self.syncing = true,
            SyncEvent::Done(_) => {
                self.syncing = false;
                self.last_synced = Some(std::time::SystemTime::now());
            }
            SyncEvent::Failed(_) => self.syncing = false,
            _ => {}
        }
        self.on_sync_event(ev, cx);
        self.refresh_sync_panel(cx);
        // Settings › Sync shows the same.
        self.refresh_settings(cx);
        cx.notify();
    }

    fn on_sync_event(&mut self, ev: SyncEvent, cx: &mut Context<Self>) {
        match ev {
            SyncEvent::Started => {}
            SyncEvent::Done(r) => {
                self.sync_error = None;
                if r.pulled > 0 {
                    let n = r.pulled;
                    self.say(
                        Notice::good(tr_n!(
                            n,
                            "Synced {n} change from other devices.",
                            "Synced {n} changes from other devices."
                        )),
                        THEME_NOTICE,
                        cx,
                    );
                }
                // Texts kept from edits made on two devices
                // (docs/design/note-conflicts.md): said last, and longer.
                match r.copies.as_slice() {
                    [] => {}
                    [title] => self.say(
                        Notice::warn(tr!(
                            "A note was changed on two devices: the other version is kept as {title}.",
                            title = title.clone()
                        )),
                        KEPT_NOTICE,
                        cx,
                    ),
                    copies => {
                        let n = copies.len();
                        self.say(
                            Notice::warn(tr_n!(
                                n,
                                "{n} note was changed on two devices: the other version is kept beside it.",
                                "{n} notes were changed on two devices: the other versions are kept beside them."
                            )),
                            KEPT_NOTICE,
                            cx,
                        );
                    }
                }
            }
            SyncEvent::Failed(e) => {
                if self.sync_error.as_ref() != Some(&e) {
                    self.say(
                        Notice::bad(tr!("Sync: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    );
                    self.sync_error = Some(e);
                }
            }
            SyncEvent::Asking(state) => {
                // Cancelled meanwhile.
                let Some(Asking::Pairing(place)) = self.asking.clone() else {
                    return;
                };
                match state {
                    AskState::Waiting | AskState::Compare { .. } => {
                        self.in_window(cx, move |ws, window, cx| {
                            ws.show_pairing(&state, window, cx)
                        });
                    }
                    AskState::Joined => {
                        self.asking = None;
                        self.in_window(cx, |ws, window, cx| ws.close_palette(window, cx));
                        self.joined(place, cx);
                    }
                    AskState::Refused | AskState::Expired => {
                        self.asking = None;
                        self.in_window(cx, |ws, window, cx| ws.close_palette(window, cx));
                        let msg = if state == AskState::Refused {
                            Notice::bad(tr!("The other device refused to let this one in."))
                        } else {
                            Notice::bad(tr!(
                                "No device let this one in within 10 minutes. Ask again, or use the recovery key."
                            ))
                        };
                        self.say(msg, CONFIG_NOTICE, cx);
                    }
                }
            }
            SyncEvent::Requests(requests) => {
                for r in requests {
                    // Nobody lets a device in from under the lock.
                    if self.lock.is_some() {
                        if !self.held_requests.iter().any(|h| h.id == r.id) {
                            self.held_requests.push(r);
                        }
                        continue;
                    }
                    if self.prompted.insert(r.id.clone()) {
                        self.in_window(cx, move |ws, window, cx| ws.prompt_pairing(r, window, cx));
                    }
                }
            }
            SyncEvent::PairingFailed(e) => self.say(
                Notice::bad(tr!("Pairing: {e}", e = e.to_string())),
                CONFIG_NOTICE,
                cx,
            ),
            SyncEvent::PassphraseJoins(joins) => {
                self.passphrase_joins = Some(joins);
                self.refresh_settings(cx);
            }
        }
    }

    /// Runs `f` with the window, from where there is none (the sync thread's
    /// events).
    fn in_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(handle) = self.window_handle else {
            return;
        };
        let me = cx.entity();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                me.update(cx, |ws, cx| f(ws, window, cx))
            });
        });
    }

    /// Asks the user whether a new device may join: they check it shows the
    /// same code.
    fn prompt_pairing(&mut self, r: Request, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("Let {device} into this vault?", device = r.device.clone()),
            Some(&tr!(
                "It gets everything the vault holds: passwords, keys, snippets and hosts. Approve only if {device} shows the code {code}.",
                device = r.device.clone(),
                code = r.code.clone()
            )),
            &[
                gpui::PromptButton::Ok(tr!("Approve").into()),
                gpui::PromptButton::Cancel(tr!("Refuse").into()),
            ],
            cx,
        );
        let sync = self.sync.clone();
        cx.spawn(async move |_, _| match answer.await {
            Ok(0) => sync.approve(r.id),
            _ => sync.refuse(r.id),
        })
        .detach();
    }

    /// Asks for what opens the locked vault; `problem` says why again.
    fn ask_vault(
        &mut self,
        lock: shellwing_vault::Lock,
        problem: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use shellwing_vault::Lock;
        if lock == Lock::New {
            self.asking = Some(Asking::VaultPassphrase {
                first: None,
                new: true,
            });
            return self.ask_passphrase(problem, window, cx);
        }
        self.asking = Some(Asking::VaultUnlock(lock));
        let mut notes = vec![if lock == Lock::Passphrase {
            Entry::new(Kind::Note, "", tr!("The vault is locked")).detail(tr!(
                "Its passphrase opens it, or its recovery key (13 groups of four). Esc leaves it locked: Unlock the vault, in the palette, asks again."
            ))
        } else {
            Entry::new(Kind::Note, "", tr!("The system keychain lost the vault's key")).detail(tr!(
                "Its recovery key (13 groups of four) opens it, and puts the key back in the keychain."
            ))
        }];
        if let Some(p) = problem {
            notes.insert(0, Entry::new(Kind::Note, "", p.to_string()));
        }
        // Windows Hello or Touch ID asks too, where it opens the vault; the
        // passphrase can be typed meanwhile. Not again after a wrong one.
        let presence = (lock == Lock::Passphrase && problem.is_none())
            .then(|| {
                self.vault
                    .presence()
                    .filter(|_| self.vault.opens_with_presence())
            })
            .flatten();
        if let Some(p) = &presence {
            use shellwing_vault::Presence as _;
            notes.insert(
                0,
                Entry::new(Kind::Note, "", tr!("{name} is asking", name = p.name()))
                    .detail(tr!("Or type the passphrase here.")),
            );
        }
        let placeholder = if lock == Lock::Passphrase {
            tr!("The vault's passphrase, or its recovery key")
        } else {
            tr!("The recovery key")
        };
        let palette = cx.new(|cx| Palette::ask_secret(&placeholder, notes, cx));
        self.show_palette(palette, false, window, cx);
        if presence.is_some() && !self.presence_asking {
            self.unlock_with_presence(window, cx);
        }
    }

    /// Opens the locked vault with Windows Hello or Touch ID, off the UI
    /// thread (the system's prompt waits for the user).
    fn unlock_with_presence(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.presence_asking = true;
        let slot = self.vault.clone();
        let job = cx.background_spawn(async move { slot.unlock_with_presence() });
        cx.spawn_in(window, async move |this, cx| {
            let done = job.await;
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.presence_asking = false;
                match done {
                    Ok(_) => {
                        if matches!(ws.asking, Some(Asking::VaultUnlock(_))) {
                            ws.asking = None;
                            ws.close_palette(window, cx);
                        }
                        ws.sync.now();
                        ws.say(Notice::good(tr!("The vault is open.")), THEME_NOTICE, cx);
                        ws.refresh_settings(cx);
                    }
                    // Cancelled: the passphrase is still asked for.
                    Err(shellwing_vault::Error::NotPresent) => {}
                    Err(e) => ws.say(
                        Notice::bad(tr!("Not opened that way: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    /// Asks for a new passphrase, or for it again to check it.
    fn ask_passphrase(
        &mut self,
        problem: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Asking::VaultPassphrase { first, new }) = &self.asking else {
            return;
        };
        let min = shellwing_vault::MIN_PASSPHRASE;
        let mut notes = vec![if first.is_some() {
            Entry::new(Kind::Note, "", tr!("Type it again")).detail(tr!("To be sure of it."))
        } else if *new {
            Entry::new(Kind::Note, "", tr!("There is no system keychain to keep the vault's key")).detail(tr!(
                "A passphrase keeps it instead: you type it when shellwing starts. At least {min} characters. Esc: no saved passwords, keys or sync this time.",
                min = min
            ))
        } else {
            Entry::new(Kind::Note, "", tr!("A passphrase for the vault")).detail(tr!(
                "It opens the vault where the system keychain cannot. At least {min} characters.",
                min = min
            ))
        }];
        if let Some(p) = problem {
            notes.insert(0, Entry::new(Kind::Note, "", p.to_string()));
        }
        let placeholder = if first.is_some() {
            tr!("The passphrase again")
        } else {
            tr!("A passphrase")
        };
        let palette = cx.new(|cx| Palette::ask_secret(&placeholder, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// An answer to [`Workspace::ask_vault`] or [`Workspace::ask_passphrase`].
    fn answer_vault(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        use shellwing_vault::{Error as VaultError, Lock, Unlock};
        match self.asking.take() {
            Some(Asking::VaultUnlock(lock)) => {
                let tried = if lock == Lock::Passphrase {
                    match self.vault.unlock(Unlock::Passphrase(&text)) {
                        // Not the passphrase: perhaps the recovery key.
                        Err(VaultError::WrongPassphrase)
                            if shellwing_vault::RecoveryKey::parse(&text).is_some() =>
                        {
                            self.vault.unlock(Unlock::RecoveryKey(&text))
                        }
                        other => other,
                    }
                } else {
                    self.vault.unlock(Unlock::RecoveryKey(&text))
                };
                self.close_palette(window, cx);
                match tried {
                    Ok(_) => {
                        self.sync.now();
                        self.say(Notice::good(tr!("The vault is open.")), THEME_NOTICE, cx);
                        self.refresh_settings(cx);
                    }
                    Err(e @ (VaultError::WrongPassphrase | VaultError::WrongRecoveryKey)) => self
                        .ask_vault(
                            lock,
                            Some(&tr!("That does not open it: {e}.", e = e.to_string())),
                            window,
                            cx,
                        ),
                    Err(e) => self.say(
                        Notice::bad(tr!("The vault stays locked: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
            }
            Some(Asking::VaultPassphrase { first: None, new }) => {
                let min = shellwing_vault::MIN_PASSPHRASE;
                self.close_palette(window, cx);
                self.asking = Some(Asking::VaultPassphrase {
                    first: (text.chars().count() >= min).then(|| text.clone()),
                    new,
                });
                let short = tr!("Too short: at least {min} characters.", min = min);
                let problem = (text.chars().count() < min).then_some(short.as_str());
                self.ask_passphrase(problem, window, cx);
            }
            Some(Asking::VaultPassphrase {
                first: Some(first),
                new,
            }) => {
                self.close_palette(window, cx);
                if first != text {
                    self.asking = Some(Asking::VaultPassphrase { first: None, new });
                    return self.ask_passphrase(
                        Some(&tr!("The two were not the same.")),
                        window,
                        cx,
                    );
                }
                let done = if new {
                    self.vault.unlock(Unlock::NewPassphrase(&text)).map(|_| ())
                } else {
                    match self.vault.ready() {
                        Some(v) => v.set_vault_passphrase(Some(&text)),
                        None => Err(VaultError::Locked(Lock::Passphrase)),
                    }
                };
                match done {
                    Ok(()) if new => {
                        // Its recovery key, for when the passphrase is forgotten.
                        if let Some(Ok(Some(rk))) = self.vault.ready().map(|v| v.recovery_key()) {
                            let key = rk.to_string();
                            let entries = vec![
                                Entry::new(Kind::Copy(key.clone()), tr!("Recovery key"), key).hint(tr!("Enter copies")),
                                Entry::new(Kind::Note, tr!("Recovery key"), tr!("Write this key down and keep it safe.")).detail(tr!(
                                    "It opens the vault if the passphrase is forgotten; Settings › Vault shows it again."
                                )),
                            ];
                            let palette = cx.new(|cx| Palette::show(entries, cx));
                            self.show_palette(palette, false, window, cx);
                        }
                        self.say(
                            Notice::good(tr!("The vault is ready; its passphrase keeps it.")),
                            CONFIG_NOTICE,
                            cx,
                        );
                    }
                    Ok(()) => self.say(
                        Notice::good(tr!("The passphrase opens the vault too now.")),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    Err(e) => self.say(
                        Notice::bad(tr!("No passphrase set: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
                self.refresh_settings(cx);
            }
            other => self.asking = other,
        }
    }

    /// Asks for a passphrase that lets new devices join the vault in the
    /// sync folder, or for it again to check it.
    fn ask_folder_passphrase(
        &mut self,
        problem: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Asking::FolderPassphrase { first }) = &self.asking else {
            return;
        };
        let min = shellwing_vault::MIN_PASSPHRASE;
        let mut notes = vec![if first.is_some() {
            Entry::new(Kind::Note, "", tr!("Type it again")).detail(tr!("To be sure of it."))
        } else {
            Entry::new(Kind::Note, "", tr!("A passphrase that joins new devices")).detail(tr!(
                "Typed on a new device that syncs the same folder, instead of the recovery key. Anyone who can read the folder can try to guess it: four or more unrelated words, at least {min} characters.",
                min = min
            ))
        }];
        if let Some(p) = problem {
            notes.insert(0, Entry::new(Kind::Note, "", p.to_string()));
        }
        let placeholder = if first.is_some() {
            tr!("The passphrase again")
        } else {
            tr!("A sync passphrase")
        };
        let palette = cx.new(|cx| Palette::ask_secret(&placeholder, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// Asks for the passphrase that joins the vault in a folder.
    fn ask_join_passphrase(
        &mut self,
        problem: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Asking::JoinPassphrase(place)) = &self.asking else {
            return;
        };
        let mut notes = vec![
            Entry::new(
                Kind::Note,
                "",
                tr!(
                    "{place} lets a passphrase join its vault",
                    place = crate::syncing::describe(place)
                ),
            )
            .detail(tr!("The one set on a device that syncs (Settings › Sync). Secrets saved here are kept and merged.")),
        ];
        if let Some(p) = problem {
            notes.insert(0, Entry::new(Kind::Note, "", p.to_string()));
        }
        let placeholder = tr!("The sync passphrase");
        let palette = cx.new(|cx| Palette::ask_secret(&placeholder, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// An answer to [`Workspace::ask_folder_passphrase`] or
    /// [`Workspace::ask_join_passphrase`].
    fn answer_folder_passphrase(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use shellwing_vault::Error as VaultError;
        self.close_palette(window, cx);
        match self.asking.take() {
            Some(Asking::FolderPassphrase { first: None }) => {
                let min = shellwing_vault::MIN_PASSPHRASE;
                let strength = shellwing_vault::passphrase_strength(&text);
                let problem = if text.chars().count() < min {
                    Some(tr!("Too short: at least {min} characters.", min = min))
                } else if strength < 3 {
                    Some(tr!(
                        "Too easy to guess (strength {strength} of 4; 3 is needed). Several unrelated words work well.",
                        strength = strength
                    ))
                } else {
                    None
                };
                self.asking = Some(Asking::FolderPassphrase {
                    first: problem.is_none().then_some(text),
                });
                self.ask_folder_passphrase(problem.as_deref(), window, cx);
            }
            Some(Asking::FolderPassphrase { first: Some(first) }) => {
                if first != text {
                    self.asking = Some(Asking::FolderPassphrase { first: None });
                    return self.ask_folder_passphrase(
                        Some(&tr!("The two were not the same.")),
                        window,
                        cx,
                    );
                }
                let Some(place) = self.sync.place() else {
                    return self.say(
                        Notice::warn(tr!("Not set: this device does not sync.")),
                        CONFIG_NOTICE,
                        cx,
                    );
                };
                self.say(tr!("Setting the sync passphrase…"), THEME_NOTICE, cx);
                self.at_place(
                    place,
                    move |vault, b| vault.let_passphrase_join(b, Some(&text)),
                    |ws, done, _, cx| {
                        match done {
                            Ok(()) => {
                                ws.passphrase_joins = Some(true);
                                ws.say(
                                    Notice::good(tr!(
                                        "New devices can join with the sync passphrase now."
                                    )),
                                    CONFIG_NOTICE,
                                    cx,
                                )
                            }
                            Err(e) => ws.say(
                                Notice::bad(tr!("Not set: {e}", e = e.to_string())),
                                CONFIG_NOTICE,
                                cx,
                            ),
                        }
                        ws.refresh_settings(cx);
                    },
                    window,
                    cx,
                );
            }
            Some(Asking::JoinPassphrase(place)) => {
                let at = place.clone();
                self.say(tr!("Joining…"), THEME_NOTICE, cx);
                self.at_place(
                    place,
                    move |vault, b| vault.join_with_passphrase(b, &text),
                    move |ws, joined, window, cx| match joined {
                        Ok(()) => ws.joined(at, cx),
                        Err(VaultError::WrongPassphrase) => {
                            ws.asking = Some(Asking::JoinPassphrase(at));
                            ws.ask_join_passphrase(
                                Some(&tr!("That passphrase does not open it.")),
                                window,
                                cx,
                            );
                        }
                        Err(e) => ws.say(
                            Notice::bad(tr!("Cannot join: {e}", e = e.to_string())),
                            CONFIG_NOTICE,
                            cx,
                        ),
                    },
                    window,
                    cx,
                );
            }
            other => self.asking = other,
        }
    }

    /// Joining a vault found in a folder: by approving on a device that
    /// syncs it, or with its recovery key.
    fn ask_join_how(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (folder, joins) = match &self.asking {
            Some(Asking::JoinHow { place, passphrase }) => {
                (crate::syncing::describe(place), *passphrase)
            }
            _ => return,
        };
        let mut entries =
            vec![
            Entry::new(
                Kind::Choice("pair".into()),
                "",
                tr!("Approve it on a device that syncs"),
            )
            .detail(tr!(
                "That device asks whether to let this one in; you check both show the same code."
            )),
            Entry::new(Kind::Choice("recovery".into()), "", tr!("Type the recovery key"))
                .detail(tr!("The 13 groups of four from when syncing started.")),
        ];
        if joins {
            entries.push(
                Entry::new(
                    Kind::Choice("passphrase".into()),
                    "",
                    tr!("Type the sync passphrase"),
                )
                .detail(tr!(
                    "The one a device that syncs set to let new devices join."
                )),
            );
        }
        let palette = cx.new(|cx| {
            Palette::chooser(
                &tr!(
                    "{folder} holds a vault from another device: join it",
                    folder = folder
                ),
                entries,
                cx,
            )
        });
        self.show_palette(palette, false, window, cx);
    }

    /// Where this device's request to join stands.
    fn show_pairing(&mut self, state: &AskState, window: &mut Window, cx: &mut Context<Self>) {
        let entries = match state {
            AskState::Compare { device, code } => vec![
                Entry::new(Kind::Note, "", tr!("Code {code}", code = code)).detail(tr!(
                    "{device} asks whether to let this device in. Approve there if it shows this same code.",
                    device = device
                )),
            ],
            _ => vec![
                Entry::new(Kind::Note, "", tr!("Waiting for a device that syncs")).detail(tr!(
                    "Open shellwing on a device that syncs this folder: it asks whether to let this one in. Esc cancels."
                )),
            ],
        };
        let palette = cx.new(|cx| Palette::show(entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// Asks for a folder, then starts syncing with it, or joins the vault in it.
    fn choose_sync_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("Sync with this folder").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |ws, window, cx| {
                    ws.sync_with(SyncPlace::Folder(folder), window, cx)
                });
            }
        })
        .detach();
    }

    /// Saves the secret of a WebDAV server or S3 storage (from Settings ›
    /// Sync) as this device's, then syncs there.
    pub fn connect_sync(
        &mut self,
        place: SyncPlace,
        secret: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = self.vault.ready() else {
            let why = self
                .vault
                .problem()
                .unwrap_or_else(|| tr!("it is still opening"));
            return self.say(
                Notice::bad(tr!("Cannot sync: the vault: {why}.", why = why)),
                CONFIG_NOTICE,
                cx,
            );
        };
        // None: none (a Git repository git signs in to by itself), not the
        // last place's.
        if let Err(e) = vault.set_device_secret(crate::syncing::PLACE_SECRET, secret.as_deref()) {
            return self.say(
                Notice::bad(tr!("Cannot keep the password: {e}", e = e.to_string())),
                CONFIG_NOTICE,
                cx,
            );
        }
        match &place {
            SyncPlace::Git(g) => {
                let url = g.url.clone();
                self.sync_with_git(place, url, window, cx)
            }
            _ => self.sync_with(place, window, cx),
        }
    }

    /// Starts syncing with a Git repository, asking first when anyone can
    /// read it: the vault is encrypted there, but how many hosts it holds
    /// and when devices sync are not. The check asks the server, so it is
    /// made off the UI thread.
    fn sync_with_git(
        &mut self,
        place: SyncPlace,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.say(
            tr!(
                "Connecting to {place}…",
                place = crate::syncing::describe(&place)
            ),
            THEME_NOTICE,
            cx,
        );
        let check = cx.background_spawn(async move {
            shellwing_remote::Git::find().is_ok_and(|git| git.readable_by_anyone(&url))
        });
        cx.spawn_in(window, async move |this, cx| {
            let public = check.await;
            let _ = this.update_in(cx, |ws, window, cx| {
                if public {
                    ws.ask_about_public_repository(place, window, cx)
                } else {
                    ws.sync_with(place, window, cx)
                }
            });
        })
        .detach();
    }

    /// Anyone can read the repository: synced there only if the user says
    /// so, another one being the first answer.
    fn ask_about_public_repository(
        &mut self,
        place: SyncPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prompting {
            return;
        }
        self.prompting = true;
        let detail = tr!(
            "What the vault keeps there is encrypted, but anyone can see how many hosts it holds and when your devices sync. A private repository is safer."
        );
        let (another, anyway) = (tr!("Use another repository"), tr!("Use it anyway"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("Anyone can read this repository"),
            Some(&detail),
            &[another.as_str(), anyway.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let anyway = matches!(answer.await, Ok(1));
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.prompting = false;
                if anyway {
                    ws.sync_with(place, window, cx)
                } else {
                    ws.sync_failed(tr!("Not connected: anyone can read this repository."), cx)
                }
            });
        })
        .detach();
    }

    /// Starts syncing with `place`, or joins the vault there.
    fn sync_with(&mut self, place: SyncPlace, window: &mut Window, cx: &mut Context<Self>) {
        // Empty (or ours): started, with the recovery key to show. Another
        // vault's: whether its passphrase lets this device in.
        enum Found {
            Started(String),
            Other { passphrase: bool },
        }
        self.say(
            tr!(
                "Connecting to {place}…",
                place = crate::syncing::describe(&place)
            ),
            THEME_NOTICE,
            cx,
        );
        let at = place.clone();
        self.at_place(
            place,
            |vault, b| match vault.folder_state(b)? {
                FolderState::Empty | FolderState::Ours => {
                    Ok(Found::Started(vault.start_sync(b)?.to_string()))
                }
                FolderState::Other => Ok(Found::Other {
                    passphrase: vault.passphrase_joins(b)?,
                }),
            },
            move |ws, found, window, cx| match found {
                Ok(Found::Started(rk)) => {
                    ws.keep_sync_place(Some(&at), cx);
                    ws.show_recovery_key(rk, true, window, cx);
                }
                Ok(Found::Other { passphrase }) => {
                    ws.asking = Some(Asking::JoinHow {
                        place: at,
                        passphrase,
                    });
                    ws.ask_join_how(window, cx);
                }
                Err(e) => ws.sync_failed(tr!("Cannot sync: {e}", e = e.to_string()), cx),
            },
            window,
            cx,
        );
    }

    /// Connecting to a sync place failed: said, and kept beside Settings'
    /// Connect while it is open.
    fn sync_failed(&mut self, why: String, cx: &mut Context<Self>) {
        if let Some((s, _)) = &self.settings {
            let (s, kept) = (s.clone(), why.clone());
            cx.defer(move |cx| s.update(cx, |v, cx| v.sync_failed(kept, cx)));
        }
        self.say(Notice::bad(why), CONFIG_NOTICE, cx);
    }

    /// Runs `work` against `place` off the UI thread (a server may be slow to
    /// answer), then `then` with what it gave.
    fn at_place<T: Send + 'static>(
        &mut self,
        place: SyncPlace,
        work: impl FnMut(
            &shellwing_vault::Vault,
            &dyn shellwing_vault::Backend,
        ) -> shellwing_vault::Result<T>
        + Send
        + 'static,
        then: impl FnOnce(&mut Self, shellwing_vault::Result<T>, &mut Window, &mut Context<Self>)
        + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault) = self.vault.ready() else {
            let why = self
                .vault
                .problem()
                .unwrap_or_else(|| tr!("it is still opening"));
            return self.say(
                Notice::bad(tr!("The vault: {why}.", why = why)),
                CONFIG_NOTICE,
                cx,
            );
        };
        let job = cx.background_spawn(async move {
            let backend = crate::syncing::open(&place, &vault)?;
            // Over what others published, and published after (Git); done
            // again over theirs if they published first.
            let mut work = work;
            shellwing_vault::transact(backend.as_ref(), |b| work(&vault, b))
        });
        cx.spawn_in(window, async move |this, cx| {
            let done = job.await;
            let _ = this.update_in(cx, |ws, window, cx| then(ws, done, window, cx));
        })
        .detach();
    }

    /// This device joined the vault at `place`: it syncs there from now on.
    fn joined(&mut self, place: SyncPlace, cx: &mut Context<Self>) {
        self.asking = None;
        let msg = tr!(
            "Joined: this device syncs with {place}.",
            place = crate::syncing::describe(&place)
        );
        self.keep_sync_place(Some(&place), cx);
        self.say(Notice::good(msg), CONFIG_NOTICE, cx);
    }

    fn keep_sync_place(&mut self, place: Option<&SyncPlace>, cx: &mut Context<Self>) {
        if let Err(e) = self.live.set_sync_place(place) {
            self.say(
                Notice::bad(tr!(
                    "Cannot save where the vault syncs: {e}",
                    e = e.to_string()
                )),
                CONFIG_NOTICE,
                cx,
            );
        }
        self.passphrase_joins = None;
        self.sync_error = None;
        self.last_synced = None;
        self.sync.set_place(place.cloned());
        self.sync.now();
        self.refresh_settings(cx);
        // Saving the place reloads the config files, hosts.toml with them,
        // which a sync may have just written (a device let in by pairing
        // syncs at once): the next poll sees nothing changed, so the host
        // list is shown again here.
        self.refresh_sidebar(cx);
    }

    fn ask_recovery_key(&mut self, again: bool, window: &mut Window, cx: &mut Context<Self>) {
        let folder = match &self.asking {
            Some(Asking::RecoveryKey(f)) => crate::syncing::describe(f),
            _ => String::new(),
        };
        let mut notes = vec![
            Entry::new(
                Kind::Note,
                "",
                tr!(
                    "{folder} holds a vault from another device.",
                    folder = folder
                ),
            )
            .detail(tr!(
                "Type its recovery key to share it. Secrets saved here are kept and merged."
            )),
        ];
        if again {
            notes.insert(
                0,
                Entry::new(Kind::Note, "", tr!("That recovery key does not open it.")).detail(tr!(
                    "Check each group of four; letters and case do not need to be exact."
                )),
            );
        }
        let placeholder = tr!("Recovery key: XXXX-XXXX-…");
        let palette = cx.new(|cx| Palette::ask(&placeholder, notes, cx));
        self.show_palette(palette, false, window, cx);
    }

    /// What the settings page shows.
    fn settings_snap(&self) -> Snap {
        Snap {
            settings: self.live.settings().clone(),
            light: self.live.light(),
            themes: self.live.themes().all().to_vec(),
            fonts: self.fonts.0.clone(),
            default_font: self.fonts.1.clone(),
            all_fonts: self.fonts.2.clone(),
            editors: self.editors.clone(),
            sync_place: self.sync.place(),
            config_dir: self.live.store().map(|s| s.dir().to_path_buf()),
            known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS")
                .map(Into::into)
                .or_else(|| std::env::home_dir().map(|h| h.join(".ssh").join("known_hosts"))),
            ssh_config: ssh_config::default_config_path(),
            vault_locked: self.vault.locked().is_some(),
            vault_passphrase: self
                .vault
                .ready()
                .and_then(|v| v.has_vault_passphrase().ok()),
            vault_keychain: self.vault.in_keychain(),
            vault_can_lock: self.vault.can_lock(),
            presence: {
                use shellwing_vault::Presence as _;
                self.vault.presence().map(|p| {
                    let on = self
                        .vault
                        .ready()
                        .and_then(|v| v.has_presence().ok())
                        .unwrap_or(false);
                    (p.name(), on)
                })
            },
            passphrase_joins: self.sync.place().and(self.passphrase_joins),
            sync: self.sync_snap(),
            update: self.update_snap(),
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
        cx.set_global(crate::hotkeys::Bound(self.live.keymap()));
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
            self.say(Notice::bad(e), CONFIG_NOTICE, cx);
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
            Err(e) => self.say(Notice::bad(e), CONFIG_NOTICE, cx),
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
                        tr!("Connected")
                    } else {
                        tr!("Other hosts · connected first")
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
        let title = tr!("Install {key} on…", key = key.name.clone());
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
                            tr!("{key} goes on here once connected.", key = key.name.clone()),
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
            None => Err(tr!("no config directory")),
        };
        match saved {
            Ok(()) => {
                self.poll_config(cx);
                self.sync.now();
                self.say(
                    Notice::good(tr!(
                        "{host} signs in with {key} from now on.",
                        host = ev.host.clone(),
                        key = ev.key_name.clone()
                    )),
                    CONFIG_NOTICE,
                    cx,
                );
            }
            Err(e) => self.say(
                Notice::bad(tr!("Cannot edit hosts.toml: {e}", e = e)),
                CONFIG_NOTICE,
                cx,
            ),
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
            self.say(
                Notice::bad(tr!("Cannot edit hosts.toml: {e}", e = e.to_string())),
                CONFIG_NOTICE,
                cx,
            );
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
                        ws.say(
                            Notice::good(tr!("Saved {name}.", name = name)),
                            THEME_NOTICE,
                            cx,
                        );
                    }
                }
                EditorEvent::Deleted { name } => {
                    let name = name.clone();
                    ws.close_editor(window, cx);
                    ws.poll_config(cx);
                    ws.sync.now();
                    ws.say(
                        Notice::good(tr!("Deleted {name}.", name = name)),
                        THEME_NOTICE,
                        cx,
                    );
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
            self.say(
                Notice::bad(tr!("Cannot edit hosts.toml: {e}", e = e.to_string())),
                CONFIG_NOTICE,
                cx,
            );
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
                GroupEvent::Saved { name } => {
                    Some(Notice::good(tr!("Saved the group {name}.", name = name)))
                }
                GroupEvent::Deleted { name } => {
                    Some(Notice::good(tr!("Deleted the group {name}.", name = name)))
                }
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

    /// The drawer that imports hosts from another app.
    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        self.poll_config(cx);
        let (store, hosts) = (self.live.store().cloned(), self.live.hosts().clone());
        let view = cx.new(|cx| ImportView::new(store, hosts, cx));
        let sub = cx.subscribe_in(&view, window, |ws, _, ev: &ImportEvent, window, cx| {
            if let ImportEvent::Imported(done) = ev {
                ws.imported(done.clone(), cx);
            }
            ws.close_import(window, cx);
        });
        let first = view.read(cx).first_focus();
        self.import = Some((view, sub));
        window.focus(&first, cx);
        cx.notify();
    }

    fn close_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import.take().is_some() {
            // Back to Settings, when it was opened from there.
            match &self.settings {
                Some((s, _)) => window.focus(&s.focus_handle(cx), cx),
                None => self.activate(self.active, window, cx),
            }
            cx.notify();
        }
    }

    /// Hosts were imported: their descriptions kept as notes, and said.
    fn imported(&mut self, done: Imported, cx: &mut Context<Self>) {
        let mut notes = vec![];
        if let Some(vault) = self.vault.ready() {
            for (host, text) in &done.notes {
                let note = shellwing_vault::Note {
                    id: String::new(),
                    title: host.clone(),
                    category: String::new(),
                    format: shellwing_vault::NoteFormat::Markdown,
                    hosts: vec![host.clone()],
                    sensitive: false,
                    changed: String::new(),
                    size: 0,
                };
                if let Ok(id) = vault.save_note(&note, Some(text)) {
                    notes.push(id);
                }
            }
        }
        self.poll_config(cx);
        self.sync.now();
        let n = done.hosts.len();
        let mut said = tr_n!(
            n,
            "Imported {n} host from {app}.",
            "Imported {n} hosts from {app}.",
            app = done.app
        );
        if done.left_out > 0 {
            said.push(' ');
            said.push_str(&tr_n!(done.left_out, "{n} left out.", "{n} left out."));
        }
        said.push(' ');
        said.push_str(&tr!("The palette can undo it."));
        self.last_import = Some((done, notes));
        self.say(Notice::good(said), CONFIG_NOTICE, cx);
    }

    /// Takes out what the last import put in: its hosts, groups and notes.
    fn undo_import(&mut self, cx: &mut Context<Self>) {
        let Some((done, notes)) = self.last_import.take() else {
            return;
        };
        let Some(store) = self.live.store().cloned() else {
            return;
        };
        if let Err(e) = store.unimport(&done.groups, &done.hosts) {
            self.last_import = Some((done, notes));
            self.say(
                Notice::bad(tr!("Cannot edit hosts.toml: {e}", e = e.to_string())),
                CONFIG_NOTICE,
                cx,
            );
            return;
        }
        if let Some(vault) = self.vault.ready() {
            for id in &notes {
                let _ = vault.delete(id);
            }
        }
        self.poll_config(cx);
        self.sync.now();
        self.say(
            Notice::good(tr!("The import from {app} is undone.", app = done.app)),
            CONFIG_NOTICE,
            cx,
        );
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
                &tr!("Port 8080 here reaches localhost:80 as the server sees it."),
            ),
            note(
                "-R 9000:localhost:3000",
                &tr!("Port 9000 on the server reaches localhost:3000 here."),
            ),
            note(
                "-D 1080",
                &tr!("A SOCKS proxy on port 1080 here; connections go out from the server."),
            ),
            note(
                &tr!("For this tab, until it is closed"),
                &tr!(
                    "To forward on every connection, add it to the host's forward list in hosts.toml."
                ),
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
            return self.say(Notice::warn(tr!("Not connected.")), CONFIG_NOTICE, cx);
        };
        cx.spawn(async move |_, cx| {
            let result = answer.await;
            tab.update(cx, |v, cx| {
                let msg = match result {
                    Ok(Ok(active)) => {
                        v.session_mut().keep_forward(&active);
                        let text = active.to_string();
                        let mut chars = text.chars();
                        let said: String = chars
                            .next()
                            .map(|c| c.to_uppercase().chain(chars).collect())
                            .unwrap_or(text);
                        Notice::good(said)
                    }
                    Ok(Err(e)) => Notice::bad(e.to_string()),
                    Err(_) => Notice::bad(tr!("The connection ended.")),
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
            Entry::new(Kind::Copy(key.clone()), tr!("Recovery key"), key).hint(tr!("Enter copies")),
            Entry::new(
                Kind::Note,
                tr!("Recovery key"),
                if new {
                    tr!("Syncing. Write this key down and keep it safe.")
                } else {
                    tr!("The key of the synced vault.")
                },
            )
            .detail(tr!(
                "Another device needs it to join; it also opens the vault if this device is lost."
            )),
        ];
        let palette = cx.new(|cx| Palette::show(entries, cx));
        self.show_palette(palette, false, window, cx);
    }

    fn open_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            let why = self
                .vault
                .problem()
                .unwrap_or_else(|| tr!("the vault is still opening"));
            self.say(
                Notice::bad(tr!("No saved passwords: {why}.", why = why)),
                CONFIG_NOTICE,
                cx,
            );
            return;
        };
        let entries: Vec<Entry> = match vault.items() {
            Ok(items) => items
                .into_iter()
                .map(|item| match item {
                    shellwing_vault::Item::Password { id, target } => Entry::new(
                        Kind::Secret(id),
                        tr!("Saved"),
                        tr!("Password for {target}", target = target),
                    )
                    .hint(tr!("Enter to forget")),
                    shellwing_vault::Item::Passphrase {
                        id,
                        fingerprint,
                        key_path,
                    } => Entry::new(
                        Kind::Secret(id),
                        tr!("Saved"),
                        tr!("Passphrase of {key_path}", key_path = key_path),
                    )
                    .detail(fingerprint)
                    .hint(tr!("Enter to forget")),
                })
                .collect(),
            Err(e) => {
                self.say(
                    Notice::bad(tr!("Cannot read the vault: {e}", e = e.to_string())),
                    CONFIG_NOTICE,
                    cx,
                );
                return;
            }
        };
        if entries.is_empty() {
            self.say(
                tr!("Nothing saved yet: after a password works, the pane offers to save it."),
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
            PaletteEvent::Dismiss => {
                if let Some(Asking::Pairing(_)) = self.asking.take() {
                    self.sync.cancel_ask();
                    self.say(tr!("Pairing cancelled."), THEME_NOTICE, cx);
                }
                self.close_palette(window, cx)
            }
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
            PaletteEvent::Chose(how) if matches!(self.asking, Some(Asking::JoinHow { .. })) => {
                let Some(Asking::JoinHow { place: folder, .. }) = self.asking.take() else {
                    return;
                };
                self.close_palette(window, cx);
                if how == "pair" {
                    self.asking = Some(Asking::Pairing(folder.clone()));
                    self.sync.ask_to_join(folder);
                    self.show_pairing(&AskState::Waiting, window, cx);
                } else if how == "passphrase" {
                    self.asking = Some(Asking::JoinPassphrase(folder));
                    self.ask_join_passphrase(None, window, cx);
                } else {
                    self.asking = Some(Asking::RecoveryKey(folder));
                    self.ask_recovery_key(false, window, cx);
                }
            }
            PaletteEvent::LoadEnv(id) => {
                let id = id.clone();
                self.close_palette(window, cx);
                self.load_env(&id, cx);
            }
            PaletteEvent::OpenNote(id) => {
                let id = id.clone();
                self.close_palette(window, cx);
                self.open_note_id(&id, cx);
            }
            PaletteEvent::PlaceFile(id) => {
                let id = id.clone();
                self.close_palette(window, cx);
                self.ask_place(&id, window, cx);
            }
            PaletteEvent::Chose(how) if matches!(self.asking, Some(Asking::PlaceHow { .. })) => {
                let how = how.clone();
                self.chose_place(&how, window, cx);
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
                self.say(Notice::good(tr!("Copied.")), THEME_NOTICE, cx);
            }
            PaletteEvent::Answer(text) => {
                let folder = match self.asking.clone() {
                    Some(Asking::Forward) => return self.answer_forward(text, window, cx),
                    Some(Asking::PlacePath(_)) => {
                        return self.answer_place_path(text.clone(), window, cx);
                    }
                    Some(Asking::SaveFile) => {
                        return self.answer_save_file(text.clone(), window, cx);
                    }
                    Some(Asking::Snippet(mut run)) => {
                        if !run.left.is_empty() {
                            let name = run.left.remove(0);
                            run.values.insert(name, text.clone());
                        }
                        self.close_palette(window, cx);
                        return self.ask_snippet(run, window, cx);
                    }
                    Some(Asking::RecoveryKey(folder)) => folder,
                    Some(Asking::VaultUnlock(_) | Asking::VaultPassphrase { .. }) => {
                        return self.answer_vault(text.clone(), window, cx);
                    }
                    Some(Asking::FolderPassphrase { .. } | Asking::JoinPassphrase(_)) => {
                        return self.answer_folder_passphrase(text.clone(), window, cx);
                    }
                    Some(Asking::JoinHow { .. } | Asking::Pairing(_) | Asking::PlaceHow { .. })
                    | None => {
                        return self.close_palette(window, cx);
                    }
                };
                let text = text.clone();
                let at = folder.clone();
                self.close_palette(window, cx);
                self.say(tr!("Joining…"), THEME_NOTICE, cx);
                self.at_place(
                    folder,
                    move |vault, b| vault.join(b, &text),
                    move |ws, joined, window, cx| match joined {
                        Ok(()) => ws.joined(at, cx),
                        Err(shellwing_vault::Error::WrongRecoveryKey) => {
                            ws.asking = Some(Asking::RecoveryKey(at));
                            ws.ask_recovery_key(true, window, cx);
                        }
                        Err(e) => ws.say(
                            Notice::bad(tr!("Cannot join: {e}", e = e.to_string())),
                            CONFIG_NOTICE,
                            cx,
                        ),
                    },
                    window,
                    cx,
                );
            }
            PaletteEvent::Forget(id) => {
                let forgotten = self.vault.ready().map(|v| v.delete(id));
                self.close_palette(window, cx);
                match forgotten {
                    Some(Ok(())) => {
                        self.sync.now();
                        self.say(Notice::good(tr!("Forgotten.")), THEME_NOTICE, cx);
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
                    Some(Err(e)) => self.say(
                        Notice::bad(tr!("Not forgotten: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    None => {}
                }
            }
            PaletteEvent::Pick(id) => {
                let picked = self.live.set_theme(id);
                self.close_palette(window, cx);
                match picked {
                    Ok(t) => {
                        self.restyle(cx);
                        self.say(
                            tr!("Theme: {name}", name = t.name.clone()),
                            THEME_NOTICE,
                            cx,
                        );
                    }
                    Err(e) => self.say(Notice::bad(e), CONFIG_NOTICE, cx),
                }
            }
        }
    }

    fn run(&mut self, a: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.lock.is_some() {
            return;
        }
        match a {
            Action::LockApp => self.lock_app(false, window, cx),
            Action::Duplicate => self.duplicate(window, cx),
            Action::SplitRight => self.split(Axis::Horizontal, window, cx),
            Action::SplitDown => self.split(Axis::Vertical, window, cx),
            Action::CloseTab => self.request_close_tab(self.active, window, cx),
            Action::ChangeTheme => self.open_theme_picker(window, cx),
            Action::Forward => self.ask_forward(None, window, cx),
            Action::NewNote => self.open_note(None, cx),
            Action::NewSnippet => self.open_snippet(None, cx),
            Action::NewEnvironment | Action::NewConfigFile => {
                let page = if a == Action::NewEnvironment {
                    Page::Environments
                } else {
                    Page::ConfigFiles
                };
                self.open_settings(page, window, cx);
                // Once this update is done: making the page's view reads it.
                if let Some((view, _)) = &self.settings {
                    let view = view.clone();
                    window.defer(cx, move |window, cx| {
                        view.update(cx, |v, cx| v.new_item(window, cx))
                    });
                }
            }
            Action::Update => self.get_update(cx),
            Action::RestartToUpdate => self.restart_to_update(window, cx),
            Action::CheckUpdates => {
                self.check_updates(cx);
                self.open_settings(Page::About, window, cx);
            }
            Action::SnippetLibrary => self.open_library(cx),
            Action::UnloadEnv => {
                if let Some(view) = self.active_tab().cloned()
                    && let Err(e) = view.update(cx, |v, cx| v.unload_env(None, cx))
                {
                    self.say(Notice::bad(e), CONFIG_NOTICE, cx);
                }
            }
            Action::Files => self.toggle_files(window, cx),
            Action::Broadcast => self.toggle_broadcast(cx),
            Action::Find => self.find(window, cx),
            Action::Tabs => self.toggle_overview(window, cx),
            Action::Log => self.toggle_log(cx),
            Action::SaveOutput { html } => self.save_output(html, window, cx),
            Action::NewHost => self.edit_host(EditTarget::New, window, cx),
            Action::NewGroup => self.edit_group(None, window, cx),
            Action::ImportHosts => self.open_import(window, cx),
            Action::UndoImport => self.undo_import(cx),
            Action::Settings => self.open_settings(Page::Appearance, window, cx),
            Action::EditHost(target) => self.edit_host(target, window, cx),
            Action::ManageSecrets => self.open_secrets(window, cx),
            Action::SyncFolder => self.choose_sync_folder(window, cx),
            Action::SyncServer => self.open_settings(Page::Sync, window, cx),
            Action::SyncNow => {
                self.sync_error = None;
                self.sync.now();
                self.say(tr!("Syncing…"), THEME_NOTICE, cx);
            }
            Action::ShowRecoveryKey => match self.vault.ready().map(|v| v.recovery_key()) {
                Some(Ok(Some(rk))) => self.show_recovery_key(rk.to_string(), false, window, cx),
                Some(Err(e)) => self.say(
                    Notice::bad(tr!("Cannot read the key: {e}", e = e.to_string())),
                    CONFIG_NOTICE,
                    cx,
                ),
                _ => self.say(
                    Notice::warn(tr!("The vault is not open.")),
                    CONFIG_NOTICE,
                    cx,
                ),
            },
            Action::UnlockVault => match self.vault.locked() {
                Some(lock) => self.ask_vault(lock, None, window, cx),
                None => self.say(Notice::good(tr!("The vault is open.")), THEME_NOTICE, cx),
            },
            Action::SetVaultPassphrase => {
                self.asking = Some(Asking::VaultPassphrase {
                    first: None,
                    new: false,
                });
                self.ask_passphrase(None, window, cx);
            }
            Action::RemoveVaultPassphrase => {
                let done = match self.vault.ready() {
                    Some(v) if self.vault.in_keychain() => {
                        v.set_vault_passphrase(None).map_err(|e| e.to_string())
                    }
                    _ => Err(tr!(
                        "the system keychain does not hold the vault's key: without a passphrase, only the recovery key would open it"
                    )),
                };
                match done {
                    Ok(()) => self.say(
                        Notice::good(tr!("The vault opens with the system keychain alone now.")),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    Err(e) => self.say(
                        Notice::bad(tr!("Passphrase kept: {e}.", e = e)),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
                self.refresh_settings(cx);
            }
            Action::TogglePresence => {
                let slot = self.vault.clone();
                let on = !self
                    .vault
                    .ready()
                    .and_then(|v| v.has_presence().ok())
                    .unwrap_or(false);
                let name = {
                    use shellwing_vault::Presence as _;
                    self.vault
                        .presence()
                        .map(|p| p.name().to_string())
                        .unwrap_or_else(|| tr!("Presence"))
                };
                let job = cx.background_spawn(async move { slot.set_presence(on) });
                cx.spawn(async move |this, cx| {
                    let done = job.await;
                    let _ = this.update(cx, |ws, cx| {
                        match done {
                            Ok(()) if on => ws.say(
                                Notice::good(tr!(
                                    "{name} opens the vault now, as its passphrase does.",
                                    name = name.as_str()
                                )),
                                CONFIG_NOTICE,
                                cx,
                            ),
                            Ok(()) => ws.say(
                                tr!("{name} no longer opens the vault.", name = name.as_str()),
                                CONFIG_NOTICE,
                                cx,
                            ),
                            Err(e) => {
                                ws.say(Notice::bad(format!("{name}: {e}")), CONFIG_NOTICE, cx)
                            }
                        }
                        ws.refresh_settings(cx);
                    });
                })
                .detach();
            }
            Action::LockVault => {
                crate::note_window::before_lock(cx);
                crate::snippet_window::before_lock(cx);
                if self.vault.lock() {
                    self.say(tr!("The vault is locked."), THEME_NOTICE, cx);
                } else {
                    self.say(
                        Notice::warn(tr!("The vault cannot lock: the system keychain opens it, or it has no passphrase.")),
                        CONFIG_NOTICE,
                        cx,
                    );
                }
                self.refresh_settings(cx);
            }
            Action::LeaveKeychain => {
                match self.vault.leave_keychain() {
                    Ok(()) => self.say(
                        tr!("The vault's key is out of the system keychain: each start asks for the passphrase."),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    Err(e) => self.say(
                        Notice::bad(tr!("The key stays in the keychain: {e}.", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
                self.refresh_settings(cx);
            }
            Action::LetPassphraseJoin => {
                self.asking = Some(Asking::FolderPassphrase { first: None });
                self.ask_folder_passphrase(None, window, cx);
            }
            Action::StopPassphraseJoin => {
                let Some(place) = self.sync.place() else {
                    return;
                };
                self.at_place(
                    place,
                    |vault, b| vault.let_passphrase_join(b, None),
                    |ws, done, _, cx| {
                        match done {
                            Ok(()) => {
                                ws.passphrase_joins = Some(false);
                                ws.say(
                                    tr!("New devices join with the recovery key, or by approving them."),
                                    CONFIG_NOTICE,
                                    cx,
                                )
                            }
                            Err(e) => ws.say(Notice::bad(tr!("Not stopped: {e}", e = e.to_string())), CONFIG_NOTICE, cx),
                        }
                        ws.refresh_settings(cx);
                    },
                    window,
                    cx,
                );
            }
            Action::KeepVaultKey => {
                let done = self.vault.keep_in_keychain();
                match done {
                    Ok(()) => self.say(
                        Notice::good(tr!("The system keychain keeps the vault's key now.")),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    Err(e) => self.say(
                        Notice::bad(tr!("Not kept: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                }
                self.refresh_settings(cx);
            }
            Action::StopSync => {
                self.keep_sync_place(None, cx);
                // The place's password goes with it.
                if let Some(v) = self.vault.ready() {
                    let _ = v.set_device_secret(crate::syncing::PLACE_SECRET, None);
                }
                self.say(
                    tr!("Stopped syncing. What is there and the secrets here are kept."),
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
                    Some(Err(e)) => self.say(
                        Notice::bad(tr!("Cannot create the file: {e}", e = e.to_string())),
                        CONFIG_NOTICE,
                        cx,
                    ),
                    None => self.say(tr!("No config directory."), CONFIG_NOTICE, cx),
                }
            }
        }
    }

    /// Shows problems with the config files, if any.
    fn report(&mut self, problems: Vec<String>, cx: &mut Context<Self>) {
        if problems.is_empty() {
            return;
        }
        self.say(Notice::warn(problems.join("\n")), CONFIG_NOTICE, cx);
    }

    fn say(&mut self, msg: impl Into<Notice>, shown: Duration, cx: &mut Context<Self>) {
        let msg: Notice = msg.into();
        // Settings covers the tabs: said there, or nobody sees it. Deferred,
        // as Settings may be what is asking, in the middle of its update.
        if let Some((s, _)) = &self.settings {
            let s = s.clone();
            cx.defer(move |cx| s.update(cx, |v, cx| v.show_notice(msg, shown, cx)));
            return;
        }
        match self.active_tab().cloned() {
            Some(tab) => tab.update(cx, |v, cx| v.show_notice(msg, shown, cx)),
            None => {
                self.notice = Some(msg);
                cx.notify();
            }
        }
    }

    /// Gives every tab the look its host should have now, in the mode the
    /// app is in: light or dark, by the settings or the system.
    fn restyle(&mut self, cx: &mut Context<Self>) -> Vec<String> {
        // The language, as config.toml now says (a reload, or Settings).
        let was = shellwing_i18n::language();
        shellwing_i18n::set_language(self.live.settings().general.language.as_deref());
        if shellwing_i18n::language() != was {
            // What was said once and kept: the host list's sections, the
            // window's title, the panels that are open.
            self.refresh_sidebar(cx);
            self.refresh_sync_panel(cx);
            self.refresh_overview(cx);
            self.in_window(cx, |ws, window, cx| ws.title_window(window, cx));
        }
        let light = self.live.light_for(self.system_light);
        crate::colors::set_light(light);
        self.live.set_light(light);
        let accent = self.live.settings().appearance.theme_accent.then(|| {
            let c = self.live.look_for(None).0.theme.accent;
            (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
        });
        crate::colors::set_accent(accent);
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
        let away = self.lock_after();
        if away > 0
            && self.lock.is_none()
            && self.last_input.elapsed() >= std::time::Duration::from_secs(u64::from(away) * 60)
        {
            self.in_window(cx, |ws, window, cx| ws.lock_app(false, window, cx));
        }
        let idle = self.live.settings().vault.lock_after.unwrap_or(0);
        let idle_now = idle > 0
            && self.last_input.elapsed() >= std::time::Duration::from_secs(u64::from(idle) * 60)
            && self.vault.locked().is_none()
            && self.vault.can_lock();
        if idle_now && {
            crate::note_window::before_lock(cx);
            crate::snippet_window::before_lock(cx);
            self.vault.lock()
        } {
            self.say(
                tr_n!(
                    idle,
                    "The vault locked: {n} minute without use.",
                    "The vault locked: {n} minutes without use."
                ),
                CONFIG_NOTICE,
                cx,
            );
            self.refresh_settings(cx);
            // Its passphrase is asked for, waiting there for the user's return.
            if self.palette.is_none() {
                self.in_window(cx, |ws, window, cx| {
                    ws.ask_vault(shellwing_vault::Lock::Passphrase, None, window, cx)
                });
            }
        }
        if !self.vault_asked
            && let Some(lock) = self.vault.locked()
        {
            self.vault_asked = true;
            self.in_window(cx, move |ws, window, cx| {
                ws.ask_vault(lock, None, window, cx)
            });
        }
        if !self.vault_reported
            && self.live.store().is_some()
            && self.vault.locked().is_none()
            && let Some(p) = self.vault.problem()
        {
            self.vault_reported = true;
            self.say(
                Notice::bad(tr!("Passwords cannot be saved: {p}", p = p)),
                CONFIG_NOTICE,
                cx,
            );
        }
        if let Some(mut problems) = self.live.reload_if_changed() {
            self.ssh_aliases = ssh_config::aliases(ssh_config::default_config_path().as_deref());
            self.share_settings(cx);
            problems.extend(self.restyle(cx));
            self.report(problems, cx);
            self.refresh_settings(cx);
            self.refresh_sidebar(cx);
            self.sync.set_place(self.live.sync_place());
        }
    }

    fn step_theme(&mut self, step: isize, cx: &mut Context<Self>) {
        let entry = self
            .active_tab()
            .and_then(|t| t.read(cx).session().entry.clone());
        match self.live.step_theme(step, entry.as_deref()) {
            Ok(theme) => {
                self.restyle(cx);
                self.say(
                    tr!("Theme: {name}", name = theme.name.clone()),
                    THEME_NOTICE,
                    cx,
                );
            }
            Err(e) => self.say(Notice::bad(e), CONFIG_NOTICE, cx),
        }
    }

    /// Follows the system's light or dark appearance, which the app takes
    /// with `mode = "system"`.
    pub fn track_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window_handle = Some(window.window_handle());
        self.system_light = Self::looks_light(window);
        // Applies the mode at the start, whatever the system is.
        let problems = self.restyle(cx);
        self.report(problems, cx);
        cx.observe_window_appearance(window, Self::appearance_changed)
            .detach();
    }

    fn looks_light(window: &Window) -> bool {
        matches!(
            window.appearance(),
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
        )
    }

    fn appearance_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let light = Self::looks_light(window);
        if light != self.system_light {
            self.system_light = light;
            let problems = self.restyle(cx);
            self.report(problems, cx);
            self.refresh_settings(cx);
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

    /// The window's shortcut a key is, as `[shortcuts]` sets them over the
    /// defaults (see [`shellwing_config::Hotkey::defaults`]).
    fn shortcut_for(keys: &Keymap, ev: &KeyDownEvent) -> Option<Shortcut> {
        let (h, digit) = keys.find(&crate::hotkeys::pressed(&ev.keystroke)?)?;
        Some(match h {
            Hotkey::Palette => Shortcut::Palette,
            Hotkey::NewTab => Shortcut::NewTab,
            Hotkey::ClosePane => Shortcut::Close,
            Hotkey::SplitRight => Shortcut::Split(Axis::Horizontal),
            Hotkey::SplitDown => Shortcut::Split(Axis::Vertical),
            Hotkey::PaneLeft => Shortcut::Focus(Dir::Left),
            Hotkey::PaneRight => Shortcut::Focus(Dir::Right),
            Hotkey::PaneUp => Shortcut::Focus(Dir::Up),
            Hotkey::PaneDown => Shortcut::Focus(Dir::Down),
            Hotkey::NextTab => Shortcut::Step(1),
            Hotkey::PreviousTab => Shortcut::Step(-1),
            Hotkey::GoToTab => Shortcut::Goto(digit?),
            Hotkey::Tabs => Shortcut::Tabs,
            Hotkey::HostList => Shortcut::Sidebar,
            Hotkey::Files => Shortcut::Files,
            Hotkey::Find => Shortcut::Find,
            Hotkey::Broadcast => Shortcut::Broadcast,
            Hotkey::Settings => Shortcut::Settings,
            Hotkey::NextTheme => Shortcut::Theme(1),
            Hotkey::PreviousTheme => Shortcut::Theme(-1),
            Hotkey::Quit => Shortcut::Quit,
        })
    }

    /// Runs before the terminal sees the key, so these never reach the host.
    fn shortcut(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.last_input = std::time::Instant::now();
        // Locked: keys go to the lock (its password), no shortcut to
        // anything, and none to what took the focus from it meanwhile.
        if let Some((lock, _)) = &self.lock {
            if !lock.read(cx).focus_handle(cx).contains_focused(window, cx) {
                cx.stop_propagation();
                self.keep_keys_locked(window, cx);
            }
            return;
        }
        // The Keyboard page is taking a key for a shortcut.
        if self
            .settings
            .as_ref()
            .is_some_and(|(v, _)| v.read(cx).recording())
        {
            return;
        }
        let Some(s) = Self::shortcut_for(&self.live.keymap(), ev) else {
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
        if self.overview.is_some() && !matches!(s, Shortcut::Tabs) {
            self.close_overview(window, cx);
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
            Shortcut::Tabs => self.toggle_overview(window, cx),
        }
    }

    /// The open tabs, for the overview.
    fn overview_rows(&self, cx: &gpui::App) -> Vec<TabRow> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let v = tab.focused.read(cx);
                let s = v.session();
                let local = s.is_local();
                let address: SharedString = match s.reached() {
                    _ if local => tr!("this computer").into(),
                    Some(t) => format!("{}@{}", t.user, t.host).into(),
                    None => s.spec.clone().into(),
                };
                TabRow {
                    tab: i,
                    host_key: if local {
                        crate::local::SPEC.to_string()
                    } else {
                        s.entry.clone().unwrap_or_else(|| s.title.clone())
                    },
                    host: if local {
                        tr!("Local terminal").into()
                    } else {
                        s.title.clone().into()
                    },
                    local,
                    mark: self.host_color(s.entry.as_deref()),
                    title: v.shell_title().cloned().unwrap_or_else(|| address.clone()),
                    address,
                    status: v.status(),
                    panes: tab.views().len(),
                    here: i == self.active,
                }
            })
            .collect()
    }

    /// Shows every open tab by host, or hides them.
    fn toggle_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overview.is_some() {
            return self.close_overview(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        let rows = self.overview_rows(cx);
        let view = cx.new(|cx| TabOverview::new(rows, cx));
        let sub = cx.subscribe_in(&view, window, |ws, _, ev: &OverviewEvent, window, cx| {
            match ev {
                OverviewEvent::Dismiss => ws.close_overview(window, cx),
                OverviewEvent::Go(ix) => {
                    let ix = *ix;
                    ws.close_overview(window, cx);
                    ws.activate(ix, window, cx);
                }
                OverviewEvent::Close(ix) => {
                    ws.request_close_tab(*ix, window, cx);
                    ws.refresh_overview(cx);
                    // The list keeps the keyboard, unless a question took it.
                    if let Some((v, _)) = &ws.overview
                        && ws.tabs.len() > 1
                        && !ws.prompting
                    {
                        window.focus(&v.focus_handle(cx), cx);
                    }
                }
            }
        });
        window.focus(&view.focus_handle(cx), cx);
        self.overview = Some((view, sub));
        cx.notify();
    }

    /// How sync stands, for the cloud and its panel.
    fn sync_standing(&self) -> Standing {
        if self.sync.place().is_none() {
            Standing::Off
        } else if self.vault.locked().is_some() {
            Standing::Paused
        } else if self.syncing {
            Standing::Syncing
        } else if let Some(e) = &self.sync_error {
            Standing::Failed(e.clone())
        } else {
            Standing::Synced(self.last_synced)
        }
    }

    fn sync_snap(&self) -> SyncSnap {
        let counts = self.vault.ready().map(|v| crate::sync_panel::Counts {
            hosts: self.live.hosts().hosts.len(),
            passwords: v
                .items()
                .map(|i| {
                    i.iter()
                        .filter(|i| matches!(i, shellwing_vault::Item::Password { .. }))
                        .count()
                })
                .unwrap_or(0),
            keys: v.ssh_keys().map(|k| k.len()).unwrap_or(0),
            snippets: v.snippets().map(|s| s.len()).unwrap_or(0),
            environments: v.environments().map(|e| e.len()).unwrap_or(0),
            files: v.config_files().map(|f| f.len()).unwrap_or(0),
            notes: v.notes().map(|n| n.len()).unwrap_or(0),
        });
        SyncSnap {
            standing: self.sync_standing(),
            place: self.sync.place().map(|p| crate::syncing::describe(&p)),
            counts,
        }
    }

    /// Shows how sync stands, hanging from the cloud, or hides it.
    fn toggle_sync_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sync_panel.is_some() {
            return self.close_sync_panel(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.overview.is_some() {
            self.close_overview(window, cx);
        }
        let snap = self.sync_snap();
        let view = cx.new(|cx| SyncPanel::new(snap, cx));
        let sub = cx.subscribe_in(
            &view,
            window,
            |ws, _, ev: &SyncPanelEvent, window, cx| match ev {
                SyncPanelEvent::Dismiss => ws.close_sync_panel(window, cx),
                SyncPanelEvent::SyncNow => ws.run(Action::SyncNow, window, cx),
                SyncPanelEvent::OpenSettings => {
                    ws.close_sync_panel(window, cx);
                    ws.open_settings(Page::Sync, window, cx);
                }
                SyncPanelEvent::Unlock => {
                    ws.close_sync_panel(window, cx);
                    ws.run(Action::UnlockVault, window, cx);
                }
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.sync_panel = Some((view, sub));
        cx.notify();
    }

    fn close_sync_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sync_panel.take().is_some() {
            self.activate(self.active, window, cx);
        }
    }

    fn refresh_sync_panel(&mut self, cx: &mut Context<Self>) {
        if let Some((view, _)) = &self.sync_panel {
            let (view, snap) = (view.clone(), self.sync_snap());
            view.update(cx, |p, cx| p.set_snap(snap, cx));
        }
    }

    fn close_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overview.take().is_some() {
            self.activate(self.active, window, cx);
        }
    }

    /// The overview follows the tabs while it is open.
    fn refresh_overview(&mut self, cx: &mut Context<Self>) {
        if let Some((view, _)) = &self.overview {
            if self.tabs.len() < 2 {
                self.overview = None;
            } else {
                let (view, rows) = (view.clone(), self.overview_rows(cx));
                view.update(cx, |o, cx| o.set_rows(rows, cx));
            }
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
        let keys = crate::hotkeys::hint(cx, Hotkey::Broadcast);
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
            .child(div().flex_1().min_w_0().child(tr_n!(
                panes,
                "Typing goes to {listening} of {n} pane. Click a pane's badge to leave it out or bring it back.",
                "Typing goes to {listening} of {n} panes. Click a pane's badge to leave it out or bring it back.",
                listening = listening
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
                    .child(tr!("Stop ({keys})", keys = keys)),
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
            let detail = v.title_detail();
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
                    .max_w(px(if detail.is_some() { 280. } else { 220. }))
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
                    // Dragged onto another tab, it takes that one's place.
                    .on_drag(DraggedTab { from: i }, {
                        let name = SharedString::from(title.clone());
                        move |_, _, _, cx| cx.new(|_| crate::files::DragGhost(name.clone()))
                    })
                    .drag_over::<DraggedTab>(|s, _, _, _| s.border_color(rgb(colors().accent)))
                    .on_drop(
                        cx.listener(move |ws, d: &DraggedTab, _, cx| ws.move_tab(d.from, i, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |ws, _, window, cx| ws.request_close_tab(i, window, cx)),
                    )
                    .child(dot(Some(v.status())))
                    // Typing in every pane: said on the tab too.
                    .when(tab.broadcast, |d| {
                        d.child(div().size(px(8.)).rounded_full().bg(rgb(colors().amber)))
                    })
                    // The host's name keeps its place; what the shell says
                    // after it gives way first.
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(150.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(title),
                    )
                    .when_some(detail, |d, detail| {
                        d.child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(rgb(colors().muted))
                                .child(detail),
                        )
                    })
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
        let listing = self.overview.is_some();
        let bounds = self.overview_button.clone();
        div()
            .flex()
            .items_center()
            .h_full()
            .w_full()
            .min_w_0()
            // Its own size: under a native title bar (Windows, X11) nothing
            // above it sets one, and the rem default is bigger than the sidebar's.
            .text_sm()
            // The logo, as the sketch puts it before the tabs. It and the room
            // after the tabs move the window: on Windows only what is marked
            // so does (see chrome::window_chrome).
            .child(
                div()
                    .flex_shrink_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(px(12.))
                    .pr(px(8.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        svg()
                            .path("icons/logo.svg")
                            .size(px(18.))
                            .text_color(rgb(colors().accent)),
                    ),
            )
            .child(strip)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            // Every tab by host: for when the strip has more than it shows.
            .when(self.tabs.len() >= 2, |d| {
                d.child(
                    div()
                        .id("tabs-button")
                        .relative()
                        .flex_shrink_0()
                        .h(px(30.))
                        .mr(px(6.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(7.))
                        .border_1()
                        .cursor_pointer()
                        .text_size(look::ui(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .when(listing, |d| {
                            d.bg(rgb(colors().selected))
                                .border_color(rgb(colors().accent))
                                .text_color(rgb(colors().text))
                        })
                        .when(!listing, |d| {
                            d.border_color(gpui::transparent_black())
                                .text_color(rgb(colors().soft))
                                .hover(|s| s.bg(rgb(colors().hover)))
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|ws, _, window, cx| ws.toggle_overview(window, cx)))
                        .child(
                            canvas(|_, _, _| {}, move |b, _, _, _| bounds.set(b))
                                .absolute()
                                .size_full()
                                .top_0()
                                .left_0(),
                        )
                        .child(svg().path("icons/tabs.svg").size(px(15.)).text_color(rgb(
                            if listing {
                                colors().text
                            } else {
                                colors().soft
                            },
                        )))
                        .child(self.tabs.len().to_string()),
                )
            })
            .child(
                div()
                    .id("settings-button")
                    .relative()
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
                    )
                    // A newer shellwing is out (Settings › About says which).
                    .when(
                        self.update_offered().is_some()
                            || matches!(self.updates.stage, updates::Stage::Ready(_)),
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top(px(5.))
                                    .right(px(5.))
                                    .size(px(7.))
                                    .rounded_full()
                                    .bg(rgb(colors().accent)),
                            )
                        },
                    ),
            )
            .child(self.sync_button(cx))
    }

    /// The cloud: its color says how sync stands; it opens the sync panel.
    fn sync_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let standing = self.sync_standing();
        let showing = self.sync_panel.is_some();
        let bounds = self.sync_button.clone();
        div()
            .id("sync-button")
            .relative()
            .flex_shrink_0()
            .size(px(30.))
            .mr(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .when(showing, |d| d.bg(rgb(colors().selected)))
            .hover(|s| s.bg(rgb(colors().hover)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|ws, _, window, cx| ws.toggle_sync_panel(window, cx)))
            .child(
                canvas(|_, _, _| {}, move |b, _, _, _| bounds.set(b))
                    .absolute()
                    .size_full()
                    .top_0()
                    .left_0(),
            )
            .child(
                svg()
                    .path("icons/cloud.svg")
                    .size(px(18.))
                    .text_color(rgb(standing.color())),
            )
            .when(standing.dotted(), |d| {
                d.child(
                    div()
                        .absolute()
                        .right(px(4.))
                        .bottom(px(5.))
                        .size(px(8.))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(colors().chrome))
                        .bg(rgb(standing.color())),
                )
            })
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
            title: tr!("This computer").into(),
            note: SharedString::default(),
            group: None,
            color: None,
            rows: vec![Row {
                name: tr!("Local terminal").into(),
                detail: shell_name.clone().into(),
                spec: crate::local::SPEC.into(),
                status: self.host_status(cx, Session::is_local),
                edit: EditTarget::Local,
                color: None,
                active: active_spec.as_deref() == Some(crate::local::SPEC),
                search: format!(
                    "{} {} local terminal this computer shell {shell_name}",
                    tr!("Local terminal"),
                    tr!("this computer")
                ),
            }],
        });
        let favorites: Vec<Row> = hosts.hosts.iter().filter(|h| h.favorite).map(row).collect();
        if !favorites.is_empty() {
            sections.push(Section {
                title: tr!("Favorites").into(),
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
                title: tr!("Hosts").into(),
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
                note: tr!("read only").into(),
                group: None,
                color: None,
                rows: aliases,
            });
        }
        sections
    }

    /// Hands the host list a fresh snapshot (it redraws only if it differs).
    fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        self.refresh_overview(cx);
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
        let keys = self.live.keymap();
        let palette = keys.hint(Hotkey::Palette);
        let mut list = div().flex().flex_col().gap_1().pt_2();
        for (h, what) in [
            (Hotkey::Palette, tr!("find a host, connect, run an action")),
            (Hotkey::NewTab, tr!("a new tab")),
            (Hotkey::ClosePane, tr!("close the tab")),
            (Hotkey::NextTab, tr!("next tab")),
            (Hotkey::GoToTab, tr!("go to a tab")),
            (Hotkey::HostList, tr!("show or hide the host list")),
        ] {
            let k = keys.hint(h);
            if !k.is_empty() {
                list = list.child(key(&k, &what));
            }
        }
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
                    .child(tr!("Choose a host on the left to connect")),
            )
            .when(!palette.is_empty(), |d| {
                d.child(div().text_color(rgb(colors().muted)).child(tr!(
                    "or press {keys} to find one, or to connect to user@host",
                    keys = palette
                )))
            })
            .child(list)
            .when_some(self.notice.clone(), |d, n| {
                let notice = div().mt_2().px_3().py_2().rounded_md();
                d.child(colored(notice, n.tone).child(n.text))
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
        // Note windows are drawn in the same font.
        let family = ui_font.family.clone().map(SharedString::from);
        if cx
            .try_global::<crate::note_window::NoteLook>()
            .is_none_or(|l| l.size != ui_font.size || l.family != family)
        {
            let mono = TermView::default_font(window).into();
            cx.set_global(crate::note_window::NoteLook {
                size: ui_font.size,
                family,
                mono,
            });
        }
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
                        .occlude()
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
                            .occlude()
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
                        // Nothing under it takes the mouse: a right click
                        // would open the terminal's menu.
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(s),
                )
            })
            // Over Settings too, which can open it.
            .when_some(self.import.as_ref().map(|(v, _)| v.clone()), |d, v| {
                d.child(
                    bottom
                        .round(div())
                        .id("import-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .bg(rgba(colors().scrim))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _, window, cx| ws.close_import(window, cx)),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .h_full()
                                .w(px(host_editor::WIDTH))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(v),
                        ),
                )
            })
            // Hangs from the tab button; a press elsewhere puts it away.
            .when_some(self.overview.as_ref().map(|(o, _)| o.clone()), |d, o| {
                let button = self.overview_button.clone();
                let b = button.get();
                d.child(
                    gpui::deferred(
                        gpui::anchored()
                            .anchor(gpui::Anchor::TopRight)
                            .position(gpui::point(b.right(), b.bottom() + px(6.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(div().child(o).on_mouse_down_out(cx.listener(
                                move |ws, e: &gpui::MouseDownEvent, window, cx| {
                                    // The button toggles it itself.
                                    if !button.get().contains(&e.position) {
                                        ws.close_overview(window, cx);
                                    }
                                },
                            ))),
                    )
                    .with_priority(1),
                )
            })
            // Hangs from the cloud, as the overview from its button.
            .when_some(self.sync_panel.as_ref().map(|(p, _)| p.clone()), |d, p| {
                let button = self.sync_button.clone();
                let b = button.get();
                d.child(
                    gpui::deferred(
                        gpui::anchored()
                            .anchor(gpui::Anchor::TopRight)
                            .position(gpui::point(b.right(), b.bottom() + px(6.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(div().child(p).on_mouse_down_out(cx.listener(
                                move |ws, e: &gpui::MouseDownEvent, window, cx| {
                                    if !button.get().contains(&e.position) {
                                        ws.close_sync_panel(window, cx);
                                    }
                                },
                            ))),
                    )
                    .with_priority(1),
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
                        .occlude()
                        .flex()
                        .justify_center()
                        // At its own height: a flex row would stretch it to the bottom.
                        .items_start()
                        // The same margin below as above.
                        .pt(px(56.))
                        .pb(px(56.))
                        .bg(rgba(colors().scrim))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, _, window, cx| ws.close_palette(window, cx)),
                        )
                        .child(p),
                )
            });
        let root = if chrome::draws_title_bar(window) {
            chrome::window_chrome(tabs, body, bg, window)
        } else {
            // A native title bar (X11): the tabs get a row of their own.
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
        self.keep_keys_locked(window, cx);
        let lock = self.lock.as_ref().map(|(v, _)| v.clone());
        root.when_some(ui_font.family, |d, f| d.font_family(f))
            .when_some(lock, |d, lock| {
                d.child(
                    gpui::deferred(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .occlude()
                            .child(lock),
                    )
                    .with_priority(100),
                )
            })
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::shortcut))
            .capture_any_mouse_down(cx.listener(|ws, _, _, _| {
                ws.last_input = std::time::Instant::now();
            }))
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
        let keys = Keymap::defaults(shellwing_config::Os::this());
        Workspace::shortcut_for(
            &keys,
            &KeyDownEvent {
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
            },
        )
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

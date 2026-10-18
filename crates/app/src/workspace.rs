//! The window: tabs in the title bar, the host list on the left, the active
//! tab's terminal, and what concerns every tab (settings, the window's place).

use std::sync::Arc;
use std::time::Duration;

use std::rc::Rc;

use gpui::{
    AnyElement, Axis, Context, CursorStyle, Div, ElementId, Entity, FocusHandle, Focusable, Hsla,
    KeyDownEvent, MouseButton, MouseMoveEvent, Render, SharedString, Subscription, Window, canvas,
    div, prelude::*, px, relative, rgb, rgba,
};
use shellwing_config::{EditFile, Host, PaneState, SessionState, TabState, WindowState};
use shellwing_ssh::config as ssh_config;
use tokio::runtime::Handle;

use crate::bench::Probe;
use crate::chrome::{self, TITLEBAR_H};
use crate::host_editor::{self, EditorContext, EditorEvent, HostEditor};
use crate::look::{Live, rgb as theme_rgb};
use crate::palette::{Action, Entry, Kind, Palette, PaletteEvent};
use crate::pane::{Dir, Pane, SplitState, neighbor};
use crate::secrets::VaultSlot;
use crate::session::{Session, Status};
use crate::settings_view::{self, Page, SettingsEvent, SettingsView, Snap};
use crate::sidebar::{self, LINE, MUTED, Row, Section, Sidebar, TEXT, dot};
use crate::syncing::{SyncEvent, SyncWorker};
use crate::target;
use crate::view::{CloseRequested, StatusChanged, TermView, VaultChanged, hsla};
use shellwing_vault::{Folder, FolderState};

/// The window's size and place are saved this long after they stop changing.
const SAVE_BOUNDS: Duration = Duration::from_millis(400);
/// How long a problem with the config files stays on screen.
const CONFIG_NOTICE: Duration = Duration::from_secs(8);
/// How long the name of a newly chosen theme stays on screen.
const THEME_NOTICE: Duration = Duration::from_secs(3);
const DIM: u32 = 0xb6c0cf;

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
    Settings,
    Theme(isize),
}

/// A tab: its panes, and which one has the keyboard.
struct Tab {
    root: Option<Pane<Entity<TermView>>>,
    focused: Entity<TermView>,
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
    live: Live,
    /// Hosts named in `~/.ssh/config`, for the sidebar.
    ssh_aliases: Vec<String>,
    rt: Handle,
    /// `$SHELLWING_BENCH`: measured on the first tab opened.
    bench: Option<(String, Arc<Probe>)>,
    /// Shown in place of the terminal while no tab is open.
    notice: Option<SharedString>,
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
    /// The settings page, while open, and the fonts it offers.
    settings: Option<(Entity<SettingsView>, Subscription)>,
    fonts: (Vec<String>, String),
}

/// What the host editor opens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTarget {
    New,
    /// A host in hosts.toml, by name.
    Saved(String),
    /// A host from `~/.ssh/config`, by alias: shown read only.
    Ssh(String),
}

#[derive(Clone)]
enum Asking {
    /// A folder holding another vault, waiting for its recovery key.
    RecoveryKey(std::path::PathBuf),
    /// A port to forward on the active tab's connection.
    Forward,
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
                cx.new(|_| Sidebar::new(me))
            },
            show_sidebar: true,
            live,
            ssh_aliases: ssh_config::aliases(ssh_config::default_config_path().as_deref()),
            rt,
            bench,
            notice: None,
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
            settings: None,
            fonts: (vec![], String::new()),
        };
        ws.report(problems, cx);
        ws.refresh_sidebar(cx);
        ws
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
        self.activate(self.active, window, cx);
        self.refresh_sidebar(cx);
    }

    /// Closes the focused pane; the last one closes its tab.
    fn close_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.active_tab().cloned() {
            self.close_view(&view, window, cx);
        }
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
        let target = target::resolve(
            spec,
            self.live.hosts(),
            ssh_config::default_config_path().as_deref(),
        )
        .map(|(mut r, h)| {
            r.forwards.extend_from_slice(forwards);
            (r, h)
        });
        let entry = target
            .as_ref()
            .ok()
            .and_then(|(_, h)| h.as_ref().map(|h| h.name.clone()));
        let (look, problem) = self.live.look_for(entry.as_deref());
        let bench = self.bench.take();
        let probe = bench
            .as_ref()
            .map_or_else(|| Arc::new(Probe::new()), |(_, p)| p.clone());
        let session = match Session::start(
            spec,
            target,
            look.scrollback,
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
        if let Some(store) = self.live.store()
            && let Err(e) = store.add_recent(spec)
        {
            eprintln!("cannot save the recent hosts: {e}");
        }
        let view = cx.new(|cx| TermView::new(session, look, probe, cx));
        // Tab and host list dots follow the session's status.
        cx.subscribe(&view, |ws, _, _: &StatusChanged, cx| ws.refresh_sidebar(cx))
            .detach();
        cx.subscribe(&view, |ws, _, _: &VaultChanged, _| ws.sync.now())
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
                EditTarget::New => String::new(),
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
        self.fonts = (
            settings_view::mono_fonts(&window.text_system().all_font_names()),
            TermView::default_font(window),
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
    pub fn change_setting(
        &mut self,
        path: &[&str],
        value: Option<shellwing_config::SettingValue>,
        cx: &mut Context<Self>,
    ) -> Option<Snap> {
        if let Err(e) = self.live.set_setting(path, value) {
            self.say(e, CONFIG_NOTICE, cx);
        }
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
                EditorEvent::Saved { name, connect } => {
                    let (name, connect) = (name.clone(), *connect);
                    ws.close_editor(window, cx);
                    ws.poll_config(cx);
                    ws.sync.now();
                    if connect {
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
            Action::CloseTab => self.close(self.active, window, cx),
            Action::ChangeTheme => self.open_theme_picker(window, cx),
            Action::Forward => self.ask_forward(None, window, cx),
            Action::Files => self.toggle_files(window, cx),
            Action::NewHost => self.edit_host(EditTarget::New, window, cx),
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
            tab.update(cx, |v, cx| v.set_look(look, cx));
        }
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
        cx.on_release(|ws, _| ws.save_bounds()).detach();
    }

    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.unsaved_bounds = Some(crate::placement::state(window.window_bounds()));
        self.bounds_change += 1;
        let change = self.bounds_change;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_BOUNDS).await;
            let _ = this.update(cx, |ws, _| {
                if ws.bounds_change == change {
                    ws.save_bounds();
                }
            });
        })
        .detach();
    }

    fn save_bounds(&mut self) {
        if let (Some(state), Some(store)) = (self.unsaved_bounds.take(), self.live.store())
            && let Err(e) = store.save_window_state(&state)
        {
            eprintln!("cannot save the window size: {e}");
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

    fn tab_strip(&self, accent: Hsla, cx: &mut Context<Self>) -> Div {
        let mut strip = div().flex().h_full().min_w_0().overflow_hidden();
        for (i, tab) in self.tabs.iter().enumerate() {
            let v = tab.focused.read(cx);
            let active = i == self.active;
            let title = v.session().title.clone();
            strip = strip.child(
                div()
                    .id(ElementId::NamedInteger("tab".into(), i as u64))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h_full()
                    .pl_3()
                    .pr_1()
                    .max_w(px(220.))
                    .border_b_2()
                    .cursor_pointer()
                    .text_color(rgb(if active { TEXT } else { DIM }))
                    .when(active, |d| d.bg(rgb(0x0b0f17)).border_color(accent))
                    .when(!active, |d| {
                        d.border_color(gpui::transparent_black())
                            .hover(|s| s.bg(rgb(0x141c2b)))
                    })
                    // A press on a tab must not start moving the window.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |ws, _, window, cx| ws.activate(i, window, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |ws, _, window, cx| ws.close(i, window, cx)),
                    )
                    .child(dot(Some(v.status())))
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
                            .text_color(rgb(MUTED))
                            .hover(|s| s.bg(rgb(0x2a3446)).text_color(rgb(TEXT)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                cx.stop_propagation();
                                ws.close(i, window, cx);
                            }))
                            .child("✕"),
                    ),
            );
        }
        strip.child(
            div()
                .id("new-tab")
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h_full()
                .text_color(rgb(MUTED))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(TEXT)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|ws, _, window, cx| ws.open_palette(window, cx)))
                .child("+"),
        )
    }

    /// What the host list shows now: hosts.toml by group, then ssh config
    /// aliases it does not already name, each with its tabs' status.
    fn sidebar_sections(&self, cx: &Context<Self>) -> Vec<Section> {
        let hosts = self.live.hosts();
        let row = |h: &Host| Row {
            name: h.name.clone().into(),
            detail: match &h.user {
                Some(u) => format!("{u}@{}", h.address).into(),
                None => h.address.clone().into(),
            },
            spec: h.alias.clone().unwrap_or_else(|| h.name.clone()),
            status: self.host_status(cx, |s| s.entry.as_deref() == Some(h.name.as_str())),
            edit: EditTarget::Saved(h.name.clone()),
        };
        let mut sections = vec![];
        for g in &hosts.groups {
            let rows: Vec<Row> = hosts
                .hosts
                .iter()
                .filter(|h| h.group.as_deref() == Some(g.name.as_str()))
                .map(row)
                .collect();
            if !rows.is_empty() {
                sections.push(Section {
                    title: g.name.clone().into(),
                    rows,
                });
            }
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
            })
            .collect();
        if !aliases.is_empty() {
            sections.push(Section {
                title: "~/.ssh/config".into(),
                rows: aliases,
            });
        }
        sections
    }

    /// Hands the host list a fresh snapshot (it redraws only if it differs).
    fn refresh_sidebar(&mut self, cx: &mut Context<Self>) {
        let sections = self.sidebar_sections(cx);
        self.sidebar.update(cx, |s, cx| s.set(sections, cx));
        cx.notify();
    }

    /// A pane tree: terminals, and between the two sides of each split a divider
    /// that can be dragged. With several panes, the focused one is marked.
    fn render_pane(
        &self,
        pane: &Pane<Entity<TermView>>,
        focused: &Entity<TermView>,
        multi: bool,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match pane {
            Pane::Leaf(view) => {
                let v = view.clone();
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
                let (s_bounds, s_drag) = (state.clone(), state.clone());
                let first = div()
                    .flex()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(relative(ratio))
                    .flex_grow(0.)
                    .child(self.render_pane(a, focused, multi, accent, cx));
                let second = div()
                    .flex()
                    .min_w_0()
                    .min_h_0()
                    .flex_basis(relative(1. - ratio))
                    .flex_grow(0.)
                    .child(self.render_pane(b, focused, multi, accent, cx));
                let divider = div()
                    .id(ElementId::Name(
                        format!("divider-{:p}", Rc::as_ptr(state)).into(),
                    ))
                    .flex_shrink_0()
                    .bg(rgb(LINE))
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

    fn welcome(&self, bg: Hsla) -> impl IntoElement {
        let key = |k: &str, what: &str| {
            div()
                .flex()
                .gap_3()
                .child(div().w(px(150.)).text_color(rgb(DIM)).child(k.to_string()))
                .child(div().text_color(rgb(MUTED)).child(what.to_string()))
        };
        let (chord, alt) = if cfg!(target_os = "macos") {
            ("Cmd", "Cmd")
        } else {
            ("Ctrl+Shift", "Alt")
        };
        div()
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
                div()
                    .text_color(rgb(TEXT))
                    .text_lg()
                    .child("Choose a host on the left to connect"),
            )
            .child(div().text_color(rgb(MUTED)).child(format!(
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
                d.child(div().pt_2().text_color(rgb(0xfbbf24)).child(n))
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
        let look = match self.active_tab() {
            Some(t) => t.read(cx).look().clone(),
            None => self.live.look_for(None).0,
        };
        let bg = hsla(theme_rgb(look.theme.background));
        let accent = hsla(theme_rgb(look.theme.accent));
        let tabs = self.tab_strip(accent, cx);
        let main: AnyElement = match self.tabs.get(self.active) {
            Some(tab) => {
                let multi = tab.views().len() > 1;
                let pane = match &tab.root {
                    Some(root) => self.render_pane(root, &tab.focused, multi, accent, cx),
                    None => div().into_any_element(),
                };
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(pane)
                    .into_any_element()
            }
            None => self.welcome(bg).into_any_element(),
        };
        let palette = self.palette.as_ref().map(|(p, ..)| p.clone());
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .when(self.show_sidebar, |d| {
                d.child(
                    self.sidebar.clone().cached(
                        gpui::StyleRefinement::default()
                            .w(px(sidebar::WIDTH))
                            .h_full()
                            .flex_shrink_0(),
                    ),
                )
            })
            .child(main)
            .when_some(self.editor.as_ref().map(|(e, _)| e.clone()), |d, e| {
                let quiet = e.clone();
                d.child(
                    div()
                        .id("editor-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(rgba(0x03060c8c))
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
                    div()
                        .id("palette-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .justify_center()
                        .pt(px(56.))
                        .bg(rgba(0x03060c99))
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
                        .bg(rgb(0x0f1522))
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(tabs),
                )
                .child(body)
        };
        root.track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::shortcut))
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
            .text_color(rgb(TEXT))
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

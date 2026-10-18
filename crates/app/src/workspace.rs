//! The window: tabs in the title bar, the host list on the left, the active
//! tab's terminal, and what concerns every tab (settings, the window's place).

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, Div, ElementId, Entity, FocusHandle, Focusable, Hsla, KeyDownEvent,
    MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px, rgb, rgba,
};
use shellwing_config::{EditFile, Host, WindowState};
use shellwing_ssh::config as ssh_config;
use tokio::runtime::Handle;

use crate::bench::Probe;
use crate::chrome::{self, TITLEBAR_H};
use crate::look::{Live, rgb as theme_rgb};
use crate::palette::{Action, Entry, Kind, Palette, PaletteEvent};
use crate::session::{Session, Status};
use crate::sidebar::{self, LINE, MUTED, Row, Section, Sidebar, TEXT, dot};
use crate::target;
use crate::view::{StatusChanged, TermView, hsla};

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
    CloseTab,
    Step(isize),
    Goto(usize),
    Sidebar,
    Theme(isize),
}

pub struct Workspace {
    focus: FocusHandle,
    tabs: Vec<Entity<TermView>>,
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
    /// The window's size and place, waiting to be saved, and which change it is.
    unsaved_bounds: Option<WindowState>,
    bounds_change: u64,
}

impl Workspace {
    pub fn new(
        live: Live,
        problems: Vec<String>,
        rt: Handle,
        bench: Option<(String, Arc<Probe>)>,
        cx: &mut Context<Self>,
    ) -> Self {
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
            unsaved_bounds: None,
            bounds_change: 0,
        };
        ws.report(problems, cx);
        ws.refresh_sidebar(cx);
        ws
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    fn active_tab(&self) -> Option<&Entity<TermView>> {
        self.tabs.get(self.active)
    }

    /// Opens a tab connected to `spec`: a hosts.toml name, an ssh config alias or
    /// `[user@]host[:port]`.
    pub fn open(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        let target = target::resolve(
            spec,
            self.live.hosts(),
            ssh_config::default_config_path().as_deref(),
        );
        let entry = target
            .as_ref()
            .ok()
            .and_then(|(_, h)| h.as_ref().map(|h| h.name.clone()));
        let (look, problem) = self.live.look_for(entry.as_deref());
        let bench = self.bench.take();
        let probe = bench
            .as_ref()
            .map_or_else(|| Arc::new(Probe::new()), |(_, p)| p.clone());
        let session = match Session::start(spec, target, look.scrollback, &self.rt, bench) {
            Ok(s) => s,
            Err(e) => {
                self.notice = Some(format!("Cannot open a terminal: {e}").into());
                cx.notify();
                return;
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
        self.tabs.push(view.clone());
        self.activate(self.tabs.len() - 1, window, cx);
        self.refresh_sidebar(cx);
        if let Some(p) = problem {
            view.update(cx, |v, cx| v.show_notice(p, CONFIG_NOTICE, cx));
        }
    }

    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(ix) {
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
                Action::CloseTab,
                "Close the tab".into(),
                format!("{chord}+W"),
            );
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
        self.activate(self.active, window, cx);
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
            Action::CloseTab => self.close(self.active, window, cx),
            Action::ChangeTheme => self.open_theme_picker(window, cx),
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
        for tab in self.tabs.clone() {
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
        if let Some(mut problems) = self.live.reload_if_changed() {
            self.ssh_aliases = ssh_config::aliases(ssh_config::default_config_path().as_deref());
            problems.extend(self.restyle(cx));
            self.report(problems, cx);
            self.refresh_sidebar(cx);
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
        // Ctrl+K alone is the shell's kill-line, so the palette takes Ctrl+Shift+K.
        Some(match key {
            "k" if chord => Shortcut::Palette,
            "t" if chord => Shortcut::NewTab,
            "w" if chord => Shortcut::CloseTab,
            "b" if chord => Shortcut::Sidebar,
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
            Shortcut::CloseTab => self.close(self.active, window, cx),
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
            Shortcut::Theme(step) => self.step_theme(step, cx),
        }
    }

    /// The best status among tabs matching `is_it`, for a sidebar dot.
    fn host_status(&self, cx: &Context<Self>, is_it: impl Fn(&Session) -> bool) -> Option<Status> {
        self.tabs
            .iter()
            .map(|t| t.read(cx))
            .filter(|v| is_it(v.session()))
            .map(|v| v.status())
            .min_by_key(|s| match s {
                Status::Connected => 0,
                Status::Connecting => 1,
                Status::Failed => 2,
                Status::Closed => 3,
            })
    }

    fn tab_strip(&self, accent: Hsla, cx: &mut Context<Self>) -> Div {
        let mut strip = div().flex().h_full().min_w_0().overflow_hidden();
        for (i, tab) in self.tabs.iter().enumerate() {
            let v = tab.read(cx);
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
        let main: AnyElement = match self.active_tab() {
            Some(t) => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(t.clone())
                .into_any_element(),
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
        assert_eq!(key("w", true, true, false), Some(Shortcut::CloseTab));
        assert_eq!(key("b", true, true, false), Some(Shortcut::Sidebar));
        assert_eq!(key("tab", true, false, false), Some(Shortcut::Step(1)));
        assert_eq!(key("tab", true, true, false), Some(Shortcut::Step(-1)));
        assert_eq!(key("pagedown", true, false, false), Some(Shortcut::Step(1)));
        // Ctrl+Shift+] arrives as "}" with only Ctrl.
        assert_eq!(key("}", true, false, false), Some(Shortcut::Theme(1)));
        assert_eq!(key("3", false, false, true), Some(Shortcut::Goto(2)));
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
    }
}

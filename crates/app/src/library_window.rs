//! The snippet library in a window of its own: ready commands by tool and
//! task, to pick one or more of and add to one's snippets, each adjusted
//! first (its options, on the host or in a container, the environment that
//! has its password). What was added shows as such; it can be opened, or
//! added again.

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight,
    HighlightStyle, KeyDownEvent, MouseButton, Render, StyledText, Subscription, Window,
    WindowBounds, WindowDecorations, WindowHandle, WindowOptions, anchored, deferred, div,
    prelude::*, px, rgb, rgba, size,
};
use shellwing_i18n::{tr, tr_n};
use shellwing_vault::Environment;

use crate::chrome;
use crate::colors::colors;
use crate::host_editor::{checkbox, hint_text, problem_text};
use crate::input::{Changed, TextInput};
use crate::library::{self, Choice, Item};
use crate::look::{self, ui};
use crate::note_window::NoteLook;
use crate::secrets::VaultSlot;
use crate::snippets::{self, Snippet};

const SIZE: (f32, f32) = (1240., 780.);

/// What the library's window tells the rest of the app.
#[derive(Clone)]
pub struct Hooks {
    /// Snippets were added: lists show them, and they sync.
    pub added: Rc<dyn Fn(&mut App)>,
    /// Opens one of the user's snippets in its window.
    pub open: Open,
    /// A key or a click: the vault is in use.
    pub touched: Rc<dyn Fn(&mut App)>,
}

/// See [`Hooks::open`].
pub type Open = Rc<dyn Fn(Snippet, &mut App)>;

/// The library's window, while open.
#[derive(Default)]
struct Opened(Option<WindowHandle<LibraryWindow>>);

impl gpui::Global for Opened {}

/// Opens the library's window, or brings it forward.
pub fn open(vault: VaultSlot, hooks: Hooks, cx: &mut App) {
    if let Some(h) = cx.default_global::<Opened>().0
        && h.update(cx, |v, window, cx| {
            v.refresh(cx);
            window.activate_window();
        })
        .is_ok()
    {
        return;
    }
    let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(SIZE.0), px(SIZE.1)),
            cx,
        ))),
        window_decorations: client_side.then_some(WindowDecorations::Client),
        titlebar: (!client_side).then_some(gpui::TitlebarOptions {
            title: Some(tr!("Snippet library").into()),
            appears_transparent: true,
            traffic_light_position: Some(chrome::TRAFFIC_LIGHTS),
        }),
        app_id: Some("shellwing".into()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| LibraryWindow::new(vault, hooks, window, cx))
    });
    match opened {
        Ok(h) => {
            cx.default_global::<Opened>().0 = Some(h);
            let _ = h.update(cx, |_, window, _| window.activate_window());
        }
        Err(e) => eprintln!("cannot open the snippet library: {e}"),
    }
}

/// Which items the list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shelf {
    All,
    Added,
    /// A tool's, by its place in [`library::tools`].
    Tool(usize),
}

/// The category snippets are added under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileUnder {
    Tool,
    Task,
    Both,
}

impl FileUnder {
    fn category(self, i: &Item) -> String {
        match self {
            FileUnder::Tool => i.tool.clone(),
            FileUnder::Task => i.task.clone(),
            FileUnder::Both => format!("{} · {}", i.tool, i.task),
        }
    }

    /// Under its task alone, a name says its tool too: "PostgreSQL: Ping".
    fn name(self, i: &Item) -> String {
        match self {
            FileUnder::Task => format!("{}: {}", i.tool, i.name),
            _ => i.name.clone(),
        }
    }

    fn said(self) -> String {
        match self {
            FileUnder::Tool => tr!("Its tool (PostgreSQL)"),
            FileUnder::Task => tr!("Its task (Backup)"),
            FileUnder::Both => tr!("Both (PostgreSQL · Backup)"),
        }
    }
}

pub struct LibraryWindow {
    vault: VaultSlot,
    hooks: Hooks,
    focus: FocusHandle,
    query: Entity<TextInput>,
    shelf: Shelf,
    task: Option<&'static str>,
    /// Only what was not added yet.
    not_added: bool,
    /// Picked to add, by index in [`library::items`].
    checked: BTreeSet<usize>,
    /// Shown on the right.
    focused: Option<usize>,
    /// How each is to be added, once changed from as it comes.
    choices: HashMap<usize, Choice>,
    /// The environment (id) an item is given for its password.
    env_of: HashMap<usize, String>,
    /// The list of environments, open.
    picking_env: bool,
    file_under: FileUnder,
    picking_file: bool,
    /// Where the File under button is drawn: its list opens from there.
    file_button: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    /// The user's snippets made from an item, by its origin.
    added: HashMap<String, Vec<Snippet>>,
    envs: Vec<Environment>,
    notice: Option<String>,
    problem: Option<String>,
    list: gpui::ScrollHandle,
    list_drag: crate::scrollbar::Drag,
    _subs: Vec<Subscription>,
}

impl LibraryWindow {
    fn new(vault: VaultSlot, hooks: Hooks, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| TextInput::said(|| tr!("Find a command: a tool, a task, a word…"), cx));
        let subs = vec![cx.subscribe(&query, |v, _, _: &Changed, cx| {
            v.notice = None;
            cx.notify();
        })];
        cx.on_release(|_, cx| cx.default_global::<Opened>().0 = None)
            .detach();
        let f = query.read(cx).focus_handle(cx);
        window.focus(&f, cx);
        let mut v = LibraryWindow {
            vault,
            hooks,
            focus: cx.focus_handle(),
            query,
            shelf: Shelf::All,
            task: None,
            not_added: false,
            checked: BTreeSet::new(),
            focused: None,
            choices: HashMap::new(),
            env_of: HashMap::new(),
            picking_env: false,
            file_under: FileUnder::Tool,
            picking_file: false,
            file_button: Rc::default(),
            added: HashMap::new(),
            envs: vec![],
            notice: None,
            problem: None,
            list: gpui::ScrollHandle::new(),
            list_drag: Default::default(),
            _subs: subs,
        };
        v.refresh(cx);
        v
    }

    /// What was added, and the environments, as the vault has them now.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.added.clear();
        let Some(vault) = self.vault.ready() else {
            return cx.notify();
        };
        for s in vault.snippets().unwrap_or_default() {
            if s.origin.starts_with(library::ORIGIN) {
                self.added.entry(s.origin.clone()).or_default().push(s);
            }
        }
        self.envs = vault.environments().unwrap_or_default();
        cx.notify();
    }

    fn added_count(&self, i: &Item) -> usize {
        self.added.get(&i.origin()).map_or(0, Vec::len)
    }

    /// The items the list shows, by index, as the shelf, the task, the
    /// words typed and "not added yet" have it.
    fn shown(&self, cx: &App) -> Vec<usize> {
        let words: Vec<String> = self
            .query
            .read(cx)
            .text()
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let tools = library::tools();
        library::items()
            .iter()
            .enumerate()
            .filter(|(_, i)| match self.shelf {
                Shelf::All => true,
                Shelf::Added => self.added_count(i) > 0,
                Shelf::Tool(t) => tools.get(t).is_some_and(|(name, _)| *name == i.tool),
            })
            .filter(|(_, i)| self.task.is_none_or(|t| i.task == t))
            .filter(|(_, i)| !self.not_added || self.added_count(i) == 0)
            .filter(|(_, i)| {
                let all = format!("{} {} {} {} {}", i.tool, i.task, i.name, i.about, i.command)
                    .to_lowercase();
                words.iter().all(|w| all.contains(w.as_str()))
            })
            .map(|(n, _)| n)
            .collect()
    }

    fn choice(&self, n: usize) -> Choice {
        self.choices.get(&n).cloned().unwrap_or_default()
    }

    /// The environment an item would be given: the one picked for it, else
    /// the only one that has its password's variable.
    fn env_for(&self, n: usize) -> Option<String> {
        if let Some(e) = self.env_of.get(&n) {
            return (!e.is_empty()).then(|| e.clone());
        }
        let var = library::items().get(n)?.password.as_deref()?;
        let mut having = self
            .envs
            .iter()
            .filter(|e| e.names.iter().any(|v| v == var));
        match (having.next(), having.next()) {
            (Some(e), None) => Some(e.id.clone()),
            _ => None,
        }
    }

    /// The snippet an item becomes, as chosen.
    fn snippet(&self, n: usize) -> Option<Snippet> {
        let i = library::items().get(n)?;
        Some(Snippet {
            id: String::new(),
            name: self.file_under.name(i),
            command: i.command(&self.choice(n)),
            scope: String::new(),
            // What changes data is typed, and waits for Enter.
            enter: !i.changes_data,
            category: self.file_under.category(i),
            needs: self.env_for(n).into_iter().collect(),
            origin: i.origin(),
        })
    }

    /// Adds what is picked to the user's snippets.
    fn add(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            self.problem = Some(tr!("The vault is locked: unlock it in the main window."));
            return cx.notify();
        };
        let picked: Vec<usize> = self.checked.iter().copied().collect();
        let mut done = 0;
        for n in &picked {
            let Some(s) = self.snippet(*n) else {
                continue;
            };
            match vault.save_snippet(&s) {
                Ok(_) => done += 1,
                Err(e) => {
                    self.problem = Some(crate::environment::said_error(&e));
                    break;
                }
            }
        }
        if done > 0 {
            self.problem = None;
            self.notice = Some(tr_n!(
                done,
                "{n} snippet added: run it from the palette.",
                "{n} snippets added: run them from the palette."
            ));
            self.checked.clear();
            (self.hooks.added)(cx);
        }
        self.refresh(cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        (self.hooks.touched)(cx);
        if ev.keystroke.key == "escape" && (self.picking_env || self.picking_file) {
            self.picking_env = false;
            self.picking_file = false;
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn heading(s: &str) -> gpui::Div {
        div()
            .text_size(ui(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(colors().muted))
            .child(s.to_uppercase())
    }

    fn badge(text: String, ground: u32, color: u32) -> gpui::Div {
        div()
            .flex_shrink_0()
            .h(px(20.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(10.))
            .bg(rgb(ground))
            .text_color(rgb(color))
            .text_size(ui(11.))
            .child(text)
    }

    fn button(id: impl Into<gpui::ElementId>, label: String) -> gpui::Stateful<gpui::Div> {
        Self::styled_button(id, label, false)
    }

    /// A button, or the window's main one in the accent color. Its hover is
    /// set once: GPUI refuses a second (a panic in debug builds).
    fn styled_button(
        id: impl Into<gpui::ElementId>,
        label: String,
        primary: bool,
    ) -> gpui::Stateful<gpui::Div> {
        let c = colors();
        let (bg, hover, edge) = if primary {
            (c.accent, c.accent_hover, c.accent)
        } else {
            (c.button, c.button_hover, c.field_edge)
        };
        let id = id.into();
        let selector = id.to_string();
        div()
            .id(id)
            .debug_selector(|| selector)
            .h(px(30.))
            .px(px(12.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(edge))
            .bg(rgb(bg))
            .text_size(ui(12.5))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(hover)))
            .when(primary, |d| {
                d.text_color(rgb(c.on_accent))
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .child(label)
    }

    /// The search field, the tasks, and "not added yet".
    fn top(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let on = self.query.read(cx).focus_handle(cx).is_focused(window);
        let mut tasks = div()
            .flex()
            .gap(px(3.))
            .p(px(3.))
            .rounded(px(8.))
            .bg(rgb(colors().field));
        let counts = |t: &str| library::items().iter().filter(|i| i.task == t).count();
        let mut pill = |n: usize, label: String, chosen: bool, task: Option<&'static str>| {
            tasks = std::mem::replace(&mut tasks, div()).child(
                div()
                    .id(("library-task", n))
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .text_size(ui(12.))
                    .cursor_pointer()
                    .when(chosen, |d| {
                        d.bg(rgb(colors().selected)).text_color(rgb(colors().text))
                    })
                    .when(!chosen, |d| {
                        d.text_color(rgb(colors().soft))
                            .hover(|s| s.bg(rgb(colors().hover)))
                    })
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.task = task;
                        cx.notify();
                    }))
                    .child(label),
            );
        };
        pill(0, tr!("Every task"), self.task.is_none(), None);
        for (n, t) in library::TASKS.iter().enumerate() {
            pill(
                n + 1,
                format!("{t} {}", counts(t)),
                self.task == Some(*t),
                Some(*t),
            );
        }
        let not_added = self.not_added;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex_1()
                    .min_w(px(240.))
                    .h(px(34.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(if on {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().field))
                    .child(self.query.clone()),
            )
            .child(tasks)
            .child(
                div()
                    .id("library-not-added")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(ui(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.not_added = !v.not_added;
                        cx.notify();
                    }))
                    .child(checkbox(not_added))
                    .child(tr!("Not added yet")),
            )
    }

    /// The tools on the left.
    fn shelves(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let added: usize = self.added.values().map(Vec::len).sum();
        let row = |id: usize, label: String, count: usize, shelf: Shelf, on: bool| {
            div()
                .id(("library-shelf", id))
                .px(px(10.))
                .py(px(6.))
                .flex()
                .justify_between()
                .rounded(px(6.))
                .text_size(ui(12.5))
                .cursor_pointer()
                .when(on, |d| {
                    d.bg(rgb(colors().selected))
                        .text_color(rgb(colors().text))
                        .font_weight(FontWeight::MEDIUM)
                })
                .when(!on, |d| {
                    d.text_color(rgb(colors().soft))
                        .hover(|s| s.bg(rgb(colors().hover)))
                })
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.shelf = shelf;
                    cx.notify();
                }))
                .child(label)
                .child(
                    div()
                        .text_color(rgb(if shelf == Shelf::Added && count > 0 {
                            colors().ok
                        } else {
                            colors().faint
                        }))
                        .child(count.to_string()),
                )
        };
        let mut nav = div()
            .id("library-shelves")
            .w(px(200.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(px(8.))
            .py(px(10.))
            .overflow_y_scroll()
            .bg(rgb(colors().panel))
            .border_r_1()
            .border_color(rgb(colors().line))
            .child(row(
                0,
                tr!("Every tool"),
                library::items().len(),
                Shelf::All,
                self.shelf == Shelf::All,
            ))
            .child(row(
                1,
                tr!("Added"),
                added,
                Shelf::Added,
                self.shelf == Shelf::Added,
            ))
            .child(div().h(px(1.)).mx(px(4.)).my(px(6.)).bg(rgb(colors().line)));
        for (t, (name, count)) in library::tools().into_iter().enumerate() {
            nav = nav.child(row(
                t + 2,
                name.to_string(),
                count,
                Shelf::Tool(t),
                self.shelf == Shelf::Tool(t),
            ));
        }
        nav
    }

    /// The items shown, under their tools.
    fn items(&self, cx: &mut Context<Self>) -> gpui::Div {
        let shown = self.shown(cx);
        let mut list = div()
            .id("library-list")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.list);
        if shown.is_empty() {
            list = list.child(
                div()
                    .p(px(24.))
                    .text_color(rgb(colors().muted))
                    .child(tr!("Nothing here: try other words, or every task.")),
            );
        }
        let mut tool = "";
        for n in shown {
            let i = &library::items()[n];
            if i.tool != tool {
                tool = &i.tool;
                let count = library::items().iter().filter(|x| x.tool == tool).count();
                list = list.child(
                    div()
                        .px(px(16.))
                        .pt(px(12.))
                        .pb(px(6.))
                        .child(Self::heading(&format!("{tool} · {count}"))),
                );
            }
            let checked = self.checked.contains(&n);
            let focused = self.focused == Some(n);
            let added = self.added_count(i);
            list = list.child(
                div()
                    .id(("library-item", n))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(9.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .cursor_pointer()
                    .when(focused, |d| d.bg(rgb(colors().raised)))
                    .when(!focused, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.focused = Some(n);
                        v.picking_env = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id(("library-check", n))
                            .debug_selector(|| format!("library-check:{}", i.id))
                            .mt(px(2.))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                cx.stop_propagation();
                                if !v.checked.remove(&n) {
                                    v.checked.insert(n);
                                }
                                v.notice = None;
                                cx.notify();
                            }))
                            .child(checkbox(checked)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().font_weight(FontWeight::MEDIUM).child(i.name.clone()))
                            .child(
                                div()
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().muted))
                                    .child(i.about.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(ui(11.))
                            .text_color(rgb(colors().muted))
                            .child(i.task.clone()),
                    )
                    .when(i.changes_data, |d| {
                        d.child(Self::badge(
                            tr!("Makes changes"),
                            colors().warn_bg,
                            colors().warn_text,
                        ))
                    })
                    .when(added > 0, |d| {
                        d.child(Self::badge(
                            if added == 1 {
                                tr!("Added ✓")
                            } else {
                                tr!("Added ×{n}", n = added)
                            },
                            colors().good_bg,
                            colors().good,
                        ))
                    }),
            );
        }
        div()
            .flex_1()
            .min_w_0()
            .relative()
            .child(list)
            .child(crate::scrollbar::overlay(
                self.list.clone(),
                self.list_drag.clone(),
            ))
    }

    /// A command, colored as the snippet editor colors it.
    fn code(command: &str, mono: gpui::SharedString) -> gpui::Div {
        let highlights = crate::shell_syntax::paint(command)
            .into_iter()
            .map(|(r, p)| {
                (
                    r,
                    HighlightStyle {
                        color: Some(rgb(p.color).into()),
                        background_color: p.ground.map(|g| rgba(g).into()),
                        ..Default::default()
                    },
                )
            });
        div()
            .px(px(12.))
            .py(px(10.))
            .rounded(px(8.))
            .bg(rgb(colors().field))
            .font_family(mono)
            .text_size(ui(12.))
            .line_height(px(20.))
            .child(StyledText::new(command.to_string()).with_highlights(highlights))
    }

    /// The item shown on the right: what it does, its command as chosen,
    /// its options, where it runs, its password.
    fn detail(
        &self,
        mono: gpui::SharedString,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let pane = div()
            .id("library-detail")
            .w(px(420.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(18.))
            .py(px(16.))
            .overflow_y_scroll()
            .bg(rgb(colors().panel))
            .border_l_1()
            .border_color(rgb(colors().line));
        let Some(n) = self.focused else {
            return pane.child(hint_text(&tr!(
                "Click a command to see it, and adjust it before adding. Tick those to add."
            )));
        };
        let i = &library::items()[n];
        let choice = self.choice(n);
        let command = i.command(&choice);
        let mut head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .text_size(ui(12.))
            .text_color(rgb(colors().muted))
            .child(format!("{} · {}", i.tool, i.task));
        if let Some(url) = i.docs.clone() {
            head = head.child(
                div()
                    .id("library-docs")
                    .text_color(rgb(colors().accent))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(colors().accent_hover)))
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .child(tr!("Docs ↗")),
            );
        }
        let mut b = pane
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(ui(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(i.name.clone()),
                    )
                    .child(head)
                    .child(
                        div()
                            .text_size(ui(12.5))
                            .text_color(rgb(colors().soft))
                            .child(i.about.clone()),
                    ),
            )
            .child(Self::code(&command, mono.clone()));
        if !i.options.is_empty() {
            let mut grid = div().flex().flex_wrap().gap(px(8.));
            for (o, flag) in i.options.iter().enumerate() {
                let on = choice.options.contains(&o);
                grid = grid.child(
                    div()
                        .id(("library-option", o))
                        .w(px(180.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .font_family(mono.clone())
                        .text_size(ui(12.))
                        .text_color(rgb(if on { colors().text } else { colors().soft }))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            let c = v.choices.entry(n).or_default();
                            if !c.options.remove(&o) {
                                c.options.insert(o);
                            }
                            cx.notify();
                        }))
                        .child(checkbox(on))
                        .child(div().min_w_0().overflow_hidden().child(flag.clone())),
                );
            }
            b = b.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::heading(&tr!("Options")))
                    .child(grid),
            );
        }
        let sudo = i.sudo(&choice);
        b = b.child(
            div()
                .id("library-sudo")
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .text_size(ui(12.5))
                .on_click(cx.listener(move |v, _, _, cx| {
                    let c = v.choices.entry(n).or_default();
                    c.sudo = Some(!sudo);
                    cx.notify();
                }))
                .child(checkbox(sudo))
                .child(tr!("Run with sudo"))
                .when(i.sudo, |d| {
                    d.child(
                        div()
                            .text_size(ui(11.5))
                            .text_color(rgb(colors().muted))
                            .child(tr!("it needs root")),
                    )
                }),
        );
        if i.can_contain() {
            let radio = |id: &'static str, label: String, on: bool, container: bool| {
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(ui(12.5))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.choices.entry(n).or_default().container = container;
                        cx.notify();
                    }))
                    .child(checkbox(on))
                    .child(label)
            };
            b = b.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(Self::heading(&tr!("Runs")))
                    .child(radio(
                        "library-host",
                        tr!("On the host"),
                        !choice.container,
                        false,
                    ))
                    .child(radio(
                        "library-container",
                        tr!("In a Docker container: asks which"),
                        choice.container,
                        true,
                    ))
                    .when(choice.container, |d| {
                        d.child(hint_text(&tr!(
                            "The files it reads or writes are the container's."
                        )))
                    }),
            );
        }
        if let Some(var) = i.password.clone() {
            let picked = self.env_for(n);
            let shown = picked
                .as_ref()
                .and_then(|id| self.envs.iter().find(|e| e.id == *id))
                .map(|e| e.name.clone())
                .unwrap_or_else(|| tr!("None: it asks, or goes without"));
            let mut pick = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(Self::heading(&tr!("Password")))
                .child(hint_text(&tr!(
                    "It reads {var}: give it an environment that has it.",
                    var = var.clone()
                )))
                .child(
                    Self::button("library-env", format!("{shown} ▾"))
                        .bg(rgb(colors().field))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.picking_env = !v.picking_env;
                            cx.notify();
                        })),
                );
            if self.picking_env {
                let mut list = div()
                    .flex()
                    .flex_col()
                    .p(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(colors().edge))
                    .bg(rgb(colors().raised));
                let mut choices: Vec<(String, String, bool)> =
                    vec![(String::new(), tr!("None: it asks, or goes without"), false)];
                for e in &self.envs {
                    let has = e.names.contains(&var);
                    choices.push((e.id.clone(), e.name.clone(), has));
                }
                for (k, (id, name, has)) in choices.into_iter().enumerate() {
                    let chosen = picked.as_deref().unwrap_or("") == id;
                    list = list.child(
                        div()
                            .id(("library-env-choice", k))
                            .px_2()
                            .py(px(5.))
                            .flex()
                            .justify_between()
                            .rounded(px(4.))
                            .text_size(ui(12.5))
                            .cursor_pointer()
                            .when(chosen, |d| d.bg(rgb(colors().selected)))
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.env_of.insert(n, id.clone());
                                v.picking_env = false;
                                cx.notify();
                            }))
                            .child(name)
                            .when(has, |d| {
                                d.child(
                                    div()
                                        .text_color(rgb(colors().ok))
                                        .child(tr!("has {var}", var = var.clone())),
                                )
                            }),
                    );
                }
                if self.envs.is_empty() {
                    list = list.child(hint_text(&tr!(
                        "None yet: add one in Settings › Environments."
                    )));
                }
                pick = pick.child(list);
            }
            b = b.child(pick);
        }
        let asked = snippets::asked(&command);
        if !asked.is_empty() {
            b = b.child(hint_text(&tr!(
                "Asked when it runs: {names}. Once added, it is yours: edit it in its window.",
                names = asked.join(", ")
            )));
        }
        if i.changes_data {
            b = b.child(hint_text(&tr!(
                "It makes changes: added, it is typed and waits for you to press Enter."
            )));
        }
        let mine = self.added.get(&i.origin()).cloned().unwrap_or_default();
        if let Some(first) = mine.first().cloned() {
            b = b.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(ui(12.))
                            .text_color(rgb(colors().ok))
                            .child(tr_n!(
                                mine.len(),
                                "In your snippets once.",
                                "In your snippets {n} times."
                            )),
                    )
                    .child(Self::button("library-open", tr!("Open")).on_click(
                        cx.listener(move |v, _, _, cx| (v.hooks.open)(first.clone(), cx)),
                    )),
            );
        }
        b
    }

    /// Below: what is picked, where it goes, and adding it.
    fn footer(&self, cx: &mut Context<Self>) -> gpui::Div {
        let picked = self.checked.len();
        let mut b = div()
            .h(px(54.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .bg(rgb(colors().panel))
            .border_t_1()
            .border_color(rgb(colors().line))
            .text_size(ui(12.5));
        if let Some(n) = &self.notice {
            b = b.child(div().text_color(rgb(colors().ok)).child(n.clone()));
        } else if let Some(p) = &self.problem {
            b = b.child(problem_text(p));
        } else if picked > 0 {
            b = b
                .child(div().font_weight(FontWeight::SEMIBOLD).child(tr_n!(
                    picked,
                    "{n} picked",
                    "{n} picked"
                )))
                .child(
                    div()
                        .id("library-clear")
                        .text_color(rgb(colors().muted))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(colors().text)))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.checked.clear();
                            cx.notify();
                        }))
                        .child(tr!("Clear")),
                );
        } else {
            b = b.child(div().text_color(rgb(colors().muted)).child(tr!(
                "Tick the commands to add; each becomes a snippet of your own."
            )));
        }
        let file = self.file_under;
        let at = self.file_button.clone();
        let mut under = div().relative().child(
            Self::button("library-file", format!("{} ▾", file.said()))
                .relative()
                .bg(rgb(colors().field))
                .on_click(cx.listener(|v, _, _, cx| {
                    v.picking_file = !v.picking_file;
                    cx.notify();
                }))
                .child(
                    gpui::canvas(move |b, _, _| at.set(b), |_, _, _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                ),
        );
        if self.picking_file {
            let mut list = div()
                .debug_selector(|| "library-file-list".into())
                .w(px(260.))
                .flex()
                .flex_col()
                .p(px(4.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(colors().edge))
                .bg(rgb(colors().raised))
                .shadow_lg();
            for (k, f) in [FileUnder::Tool, FileUnder::Task, FileUnder::Both]
                .into_iter()
                .enumerate()
            {
                list = list.child(
                    div()
                        .id(("library-file-choice", k))
                        .px_2()
                        .py(px(6.))
                        .rounded(px(4.))
                        .text_size(ui(12.5))
                        .cursor_pointer()
                        .when(f == file, |d| d.bg(rgb(colors().selected)))
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.file_under = f;
                            v.picking_file = false;
                            cx.notify();
                        }))
                        .child(f.said()),
                );
            }
            // Right above the button: the window's foot is below it.
            let b = self.file_button.get();
            under = under.child(
                deferred(
                    anchored()
                        .anchor(gpui::Anchor::BottomLeft)
                        .position(gpui::point(b.left(), b.top() - px(4.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(list),
                )
                .with_priority(1),
            );
        }
        let add = Self::styled_button(
            "library-add",
            if picked > 0 {
                tr_n!(picked, "Add {n} to my snippets", "Add {n} to my snippets")
            } else {
                tr!("Add to my snippets")
            },
            picked > 0,
        );
        let add = if picked > 0 {
            add.on_click(cx.listener(|v, _, _, cx| v.add(cx)))
        } else {
            add.opacity(0.5).cursor_default()
        };
        b.child(div().flex_1())
            .child(
                div()
                    .text_color(rgb(colors().muted))
                    .child(tr!("File under")),
            )
            .child(under)
            .child(add)
    }
}

impl Focusable for LibraryWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LibraryWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = cx.try_global::<NoteLook>().cloned();
        let ui_size = look.as_ref().map_or(13., |l| l.size);
        window.set_rem_size(look::rem_for(ui_size));
        window.set_window_title(&tr!("Snippet library"));
        let mono = look
            .as_ref()
            .map(|l| l.mono.clone())
            .unwrap_or_else(|| "monospace".into());
        let body: gpui::AnyElement = if self.vault.ready().is_none() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(colors().muted))
                .child(tr!("The vault is locked: unlock it in the main window."))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.shelves(cx))
                .child(self.items(cx))
                .child(self.detail(mono, cx))
                .into_any_element()
        };
        let content = div()
            .id("library")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| (v.hooks.touched)(cx)),
            )
            // What is left under the title bar, not the window's height.
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .child(self.top(window, cx))
            .child(body)
            .child(self.footer(cx));
        let content = div().flex_1().min_h_0().flex().flex_col().child(content);
        let bar = chrome::title_bar(tr!("Snippet library"));
        let root = chrome::window_chrome(bar, content, rgb(colors().surface).into(), window);
        root.when_some(look.and_then(|l| l.family), |d, f| d.font_family(f))
            .children(crate::app_lock::cover(cx))
    }
}

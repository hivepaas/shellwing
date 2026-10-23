//! A snippet in a window of its own, as a note is: its name and category
//! above; its command in a code editor (colored as shell, its lines
//! numbered); beside it where the palette offers it, the environments it
//! needs, its `{{name}}` values and whether Enter follows; below, running it
//! (or typing it) in the main window's pane. Saved a moment after typing
//! stops, once it has a name and a command, and when the window closes or
//! the app quits. While the vault is locked its command is not in the
//! window.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, MouseButton, PromptLevel, Render, Subscription, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};
use shellwing_i18n::{tr, tr_n};
use shellwing_vault::Environment;

use crate::chrome;
use crate::colors::colors;
use crate::host_editor::{checkbox, hint_text, label_text, problem_text};
use crate::input::{Changed, TextInput};
use crate::look::{self, ui};
use crate::note_window::{NoteLook, PaneTitle, Release, SizeKept};
use crate::secrets::VaultSlot;
use crate::snippets::{self, BUILT_IN, Scope, Snippet};

/// A snippet's window when none was opened before, and the least it opens at.
const DEFAULT_SIZE: (f32, f32) = (1000., 680.);
const LEAST_SIZE: (f32, f32) = (700., 480.);

/// How long after the last key a snippet is saved.
const SAVE_AFTER: Duration = Duration::from_secs(3);
/// How often a window looks whether the vault locked or opened.
const WATCH: Duration = Duration::from_secs(1);

/// What a snippet window tells the rest of the app.
#[derive(Clone)]
pub struct Hooks {
    /// A snippet was saved or deleted (on this device only): lists show it
    /// again.
    pub saved: Rc<dyn Fn(&mut App)>,
    /// A snippet is open in its window: left out of syncs until let go.
    pub hold: crate::note_window::Hold,
    /// Its window is done with it: it syncs again, at once when it changed.
    pub release: Release,
    /// A key or a click: the vault is in use (it locks when idle).
    pub touched: Rc<dyn Fn(&mut App)>,
    /// The groups and the hosts it can be offered for.
    pub scopes: Scopes,
    /// The main window's focused pane, when its shell can take text.
    pub pane: PaneTitle,
    /// Runs it there as the palette does (asking for its values there),
    /// with Enter after it or not.
    pub run: Run,
    /// The window's size as it closes, for the next to open so.
    pub sized: SizeKept,
}

/// See [`Hooks::scopes`]: the groups' names, and the hosts'.
pub type Scopes = Rc<dyn Fn(&App) -> (Vec<String>, Vec<String>)>;
/// See [`Hooks::run`].
pub type Run = Rc<dyn Fn(Snippet, bool, &mut App) -> Result<(), String>>;

/// The snippet windows open, by snippet id.
#[derive(Default)]
struct Opened(HashMap<String, WindowHandle<SnippetWindow>>);

impl gpui::Global for Opened {}

/// The size a snippet's window opens at: the last one's (`last`), never
/// less than 700 × 480; 1000 × 680 the first time.
pub fn opening_size(last: Option<(f32, f32)>) -> (f32, f32) {
    let (w, h) = last.unwrap_or(DEFAULT_SIZE);
    (w.max(LEAST_SIZE.0), h.max(LEAST_SIZE.1))
}

/// Opens `snippet` (None: a new one) in a window, or brings forward the
/// one it is open in.
pub fn open(
    snippet: Option<Snippet>,
    vault: VaultSlot,
    hooks: Hooks,
    last: Option<(f32, f32)>,
    cx: &mut App,
) {
    if let Some(id) = snippet.as_ref().map(|s| s.id.clone())
        && let Some(h) = cx.default_global::<Opened>().0.get(&id).copied()
        && h.update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
    let title = snippet
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| tr!("New snippet"));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            {
                let (w, h) = opening_size(last);
                size(px(w), px(h))
            },
            cx,
        ))),
        window_decorations: client_side.then_some(WindowDecorations::Client),
        titlebar: (!client_side).then_some(gpui::TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(chrome::TRAFFIC_LIGHTS),
        }),
        app_id: Some("shellwing".into()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| SnippetWindow::new(snippet, vault, hooks, window, cx))
    });
    match opened {
        Ok(h) => {
            let _ = h.update(cx, |v, window, cx| {
                v.handle = Some(h);
                v.held(cx);
                window.activate_window();
            });
        }
        Err(e) => eprintln!("cannot open a snippet's window: {e}"),
    }
}

/// Quitting: every snippet window saves, and lets its snippet sync again.
/// True when any changed while open, for the app to sync before it goes.
pub fn finish_all(cx: &mut App) -> bool {
    let windows: Vec<_> = cx.default_global::<Opened>().0.values().copied().collect();
    let mut changed = false;
    for h in windows {
        let _ = h.update(cx, |v, _, cx| {
            v.save(cx);
            changed |= v.unsynced;
            v.let_go(cx);
        });
    }
    changed
}

/// The vault is about to lock: every snippet window saves, and lets go of
/// its command.
pub fn before_lock(cx: &mut App) {
    let windows: Vec<_> = cx.default_global::<Opened>().0.values().copied().collect();
    for h in windows {
        let _ = h.update(cx, |v, _, cx| {
            v.save(cx);
            v.unload(cx);
        });
    }
}

/// Which list is open beside the snippet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Picking {
    Group,
    Host,
    Environment,
}

pub struct SnippetWindow {
    /// Its window, noted under the snippet's id once it has one.
    handle: Option<WindowHandle<SnippetWindow>>,
    vault: VaultSlot,
    hooks: Hooks,
    /// As last saved (or loaded); what the fields do not hold, as set here.
    snippet: Snippet,
    focus: FocusHandle,
    name: Entity<TextInput>,
    category: Entity<TextInput>,
    command: Entity<TextInput>,
    scope: Scope,
    picking: Option<Picking>,
    /// The command is in the field (not while the vault is locked).
    loaded: bool,
    /// Counts the edits, so a save waits for the last one.
    edits: u64,
    saved_at: u64,
    /// What was last saved (or loaded), to save only what differs.
    last_saved: Option<u64>,
    /// Saved here since the window held its snippet: it syncs when let go.
    unsynced: bool,
    problem: Option<String>,
    /// The window's size, not maximized, as last drawn: kept as it closes.
    size: Option<(f32, f32)>,
    _subs: Vec<Subscription>,
}

impl SnippetWindow {
    fn new(
        snippet: Option<Snippet>,
        vault: VaultSlot,
        hooks: Hooks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let new = snippet.is_none();
        let snippet = snippet.unwrap_or(Snippet {
            id: String::new(),
            name: String::new(),
            command: String::new(),
            scope: String::new(),
            enter: true,
            category: String::new(),
            needs: vec![],
            origin: String::new(),
        });
        let field = |placeholder: fn() -> String, text: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut i = TextInput::said(placeholder, cx);
                i.set_text(text, text.len()..text.len(), cx);
                i
            })
        };
        let name = field(
            || tr!("What it does, to find it in the palette"),
            &snippet.name,
            cx,
        );
        let category = field(|| tr!("No category"), &snippet.category, cx);
        let command = cx.new(|cx| {
            TextInput::said(|| "df -h / && free -h".to_string(), cx)
                .code(crate::shell_syntax::paint)
        });
        let mut subs = vec![];
        for i in [&name, &category, &command] {
            subs.push(cx.subscribe(i, |v, _, _: &Changed, cx| v.edited(cx)));
        }
        // Saved as it goes, so leaving keeps the last of it.
        cx.on_release(|v, cx| {
            v.save(cx);
            v.let_go(cx);
            if let Some((w, h)) = v.size {
                (v.hooks.sized)(w, h, cx);
            }
            let id = v.snippet.id.clone();
            let opened = &mut cx.default_global::<Opened>().0;
            if opened.get(&id).is_some_and(|h| Some(*h) == v.handle) {
                opened.remove(&id);
            }
        })
        .detach();
        cx.on_app_quit(|v, cx| {
            v.save(cx);
            async {}
        })
        .detach();
        // The vault locking or opening shows here within a moment.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                if this.update(cx, |v, cx| v.watch(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut v = SnippetWindow {
            handle: None,
            vault,
            hooks,
            scope: Scope::parse(&snippet.scope),
            snippet,
            focus: cx.focus_handle(),
            name,
            category,
            command,
            picking: None,
            loaded: false,
            edits: 0,
            saved_at: 0,
            last_saved: None,
            unsynced: false,
            problem: None,
            size: None,
            _subs: subs,
        };
        v.load(cx);
        let f = if new {
            v.name.read(cx).focus_handle(cx)
        } else {
            v.command.read(cx).focus_handle(cx)
        };
        window.focus(&f, cx);
        v
    }

    /// The command into the field, as the vault has it now.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            return;
        };
        if self.loaded {
            return;
        }
        if !self.snippet.id.is_empty() {
            match vault
                .snippets()
                .map(|l| l.into_iter().find(|s| s.id == self.snippet.id))
            {
                Ok(Some(s)) => {
                    self.scope = Scope::parse(&s.scope);
                    self.snippet = s;
                }
                Ok(None) => {
                    self.problem = Some(tr!("It is not in the vault any more."));
                    return cx.notify();
                }
                Err(e) => {
                    self.problem = Some(e.to_string());
                    return cx.notify();
                }
            }
        }
        let text = self.snippet.command.clone();
        self.command
            .update(cx, |i, cx| i.set_text(&text, text.len()..text.len(), cx));
        // Loading is not an edit.
        self.edits += 1;
        self.saved_at = self.edits;
        self.loaded = true;
        self.last_saved = (!self.snippet.id.is_empty()).then(|| self.snapshot(cx));
        cx.notify();
    }

    /// Lets go of the command (the vault locked).
    fn unload(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        self.command.update(cx, |i, cx| i.set_text("", 0..0, cx));
        self.snippet.command.clear();
        self.edits += 1;
        self.saved_at = self.edits;
        self.loaded = false;
        cx.notify();
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        // Locked, shellwing shows nothing: as with the vault shut.
        let open = self.vault.ready().is_some() && !crate::app_lock::is_locked(cx);
        if !open && self.loaded {
            self.unload(cx);
        } else if open && !self.loaded {
            self.load(cx);
        }
    }

    /// Notes the window under the snippet's id, once it has one, and holds
    /// it out of syncs.
    fn held(&self, cx: &mut App) {
        if let Some(h) = self.handle
            && !self.snippet.id.is_empty()
        {
            cx.default_global::<Opened>()
                .0
                .insert(self.snippet.id.clone(), h);
            (self.hooks.hold)(&self.snippet.id, cx);
        }
    }

    /// The window is done with its snippet: it syncs again (at once when it
    /// changed meanwhile).
    fn let_go(&mut self, cx: &mut App) {
        if !self.snippet.id.is_empty() {
            (self.hooks.release)(&self.snippet.id, self.unsynced, cx);
        }
        self.unsynced = false;
    }

    /// What the window holds, as a snippet.
    fn current(&self, cx: &App) -> Snippet {
        let text = |i: &Entity<TextInput>| i.read(cx).text().trim().to_string();
        // A group or a host not yet picked: every host.
        let scope = match &self.scope {
            Scope::Group(g) | Scope::Host(g) if g.is_empty() => String::new(),
            s => s.field(),
        };
        Snippet {
            name: text(&self.name),
            category: text(&self.category),
            command: if self.loaded {
                self.command.read(cx).text().trim_end().to_string()
            } else {
                self.snippet.command.clone()
            },
            scope,
            ..self.snippet.clone()
        }
    }

    /// What a save would keep, hashed.
    fn snapshot(&self, cx: &App) -> u64 {
        use std::hash::{Hash, Hasher};
        let s = self.current(cx);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            &s.name,
            &s.category,
            &s.command,
            &s.scope,
            s.enter,
            &s.needs,
        )
            .hash(&mut h);
        h.finish()
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        self.edits += 1;
        (self.hooks.touched)(cx);
        let at = self.edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let _ = this.update(cx, |v, cx| {
                if v.edits == at {
                    v.save(cx);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// What it lacks to be kept, if anything.
    fn lacking(&self, cx: &App) -> Option<String> {
        let s = self.current(cx);
        (s.name.is_empty() || s.command.trim().is_empty())
            .then(|| tr!("A name and a command are needed."))
    }

    /// Saves what changed since the last save, once it has a name and a
    /// command.
    fn save(&mut self, cx: &mut App) {
        if self.saved_at == self.edits || !self.loaded {
            return;
        }
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let snap = self.snapshot(cx);
        if self.last_saved == Some(snap) {
            self.saved_at = self.edits;
            return;
        }
        // Not yet: it waits for the rest.
        if self.lacking(cx).is_some() {
            return;
        }
        let s = self.current(cx);
        match vault.save_snippet(&s) {
            Ok(id) => {
                self.saved_at = self.edits;
                self.last_saved = Some(snap);
                self.unsynced = true;
                self.problem = None;
                let new = self.snippet.id.is_empty();
                self.snippet = Snippet { id, ..s };
                if new {
                    self.held(cx);
                }
                (self.hooks.saved)(cx);
            }
            Err(e) => self.problem = Some(crate::environment::said_error(&e)),
        }
    }

    /// A change to what is not typed (where, needs, Enter): saved at once.
    fn changed_meta(&mut self, cx: &mut Context<Self>) {
        self.edits += 1;
        (self.hooks.touched)(cx);
        self.save(cx);
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.current(cx).name;
        let (yes, no) = (tr!("Delete"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!(
                "Delete {name}?",
                name = if name.is_empty() {
                    tr!("this snippet")
                } else {
                    name
                }
            ),
            Some(&tr!(
                "It goes from the vault, and from your other devices once they sync."
            )),
            &[yes.as_str(), no.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let _ = this.update_in(cx, |v, window, cx| {
                let id = v.snippet.id.clone();
                if !id.is_empty() {
                    match v.vault.ready().map(|vault| vault.delete(&id)) {
                        Some(Ok(())) => {}
                        Some(Err(e)) => {
                            v.problem = Some(e.to_string());
                            return cx.notify();
                        }
                        None => return,
                    }
                    cx.default_global::<Opened>().0.remove(&id);
                    (v.hooks.saved)(cx);
                    // Gone: that goes out at once.
                    (v.hooks.release)(&id, true, cx);
                    v.snippet.id.clear();
                }
                // Nothing to save on the way out.
                v.saved_at = v.edits;
                window.remove_window();
            });
        })
        .detach();
    }

    /// Runs it (or types it, `enter` false) in the main window's pane, saved
    /// first.
    fn run(&mut self, enter: bool, cx: &mut Context<Self>) {
        self.save(cx);
        let s = self.current(cx);
        if s.command.trim().is_empty() {
            return;
        }
        self.problem = (self.hooks.run)(s, enter, cx).err();
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        (self.hooks.touched)(cx);
        let k = &ev.keystroke;
        let cmd = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        match k.key.as_str() {
            // Saved at once, as editors do.
            "s" if cmd => {
                self.save(cx);
                cx.notify();
            }
            "escape" if self.picking.is_some() => {
                self.picking = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn field_box(input: &Entity<TextInput>, on: bool) -> gpui::Div {
        div()
            .min_h(px(34.))
            .py(px(6.))
            .px(px(10.))
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
            .text_size(ui(13.))
            .child(input.clone())
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
            .gap(px(6.))
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

    fn heading(s: &str) -> gpui::Div {
        div()
            .text_size(ui(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(colors().muted))
            .child(s.to_uppercase())
    }

    /// A list to pick from, under what opened it.
    fn list(
        names: Vec<(String, String, bool)>,
        empty: String,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let pick = Rc::new(pick);
        let mut list = div()
            .id("snippet-pick")
            .flex()
            .flex_col()
            .max_h(px(180.))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(colors().edge))
            .bg(rgb(colors().raised));
        if names.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py(px(5.))
                    .text_size(ui(12.5))
                    .text_color(rgb(colors().muted))
                    .child(empty),
            );
        }
        for (i, (key, shown, chosen)) in names.into_iter().enumerate() {
            let pick = pick.clone();
            list = list.child(
                div()
                    .id(("snippet-choice", i))
                    .px_2()
                    .py(px(5.))
                    .rounded(px(4.))
                    .text_size(ui(13.))
                    .cursor_pointer()
                    .when(chosen, |d| d.bg(rgb(colors().selected)))
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(move |v, _, _, cx| pick(v, key.clone(), cx)))
                    .child(shown),
            );
        }
        list
    }

    /// Where the palette offers it.
    fn offered(&self, cx: &mut Context<Self>) -> gpui::Div {
        let (groups, hosts) = (self.hooks.scopes)(cx);
        let mut seg = div()
            .flex()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(colors().field_edge))
            .overflow_hidden();
        let choices = [
            (
                0usize,
                tr!("Every host"),
                matches!(self.scope, Scope::Every),
            ),
            (
                1,
                match &self.scope {
                    Scope::Group(g) if !g.is_empty() => format!("{g} ▾"),
                    _ => tr!("A group ▾"),
                },
                matches!(self.scope, Scope::Group(_)),
            ),
            (
                2,
                match &self.scope {
                    Scope::Host(h) if !h.is_empty() => format!("{h} ▾"),
                    _ => tr!("One host ▾"),
                },
                matches!(self.scope, Scope::Host(_)),
            ),
        ];
        for (n, label, on) in choices {
            seg = seg.child(
                div()
                    .id(("snippet-scope", n))
                    .flex_1()
                    .min_w_0()
                    .h(px(30.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .text_size(ui(12.))
                    .whitespace_nowrap()
                    .text_color(rgb(if on { colors().text } else { colors().soft }))
                    .cursor_pointer()
                    .when(n > 0, |d| {
                        d.border_l_1().border_color(rgb(colors().field_edge))
                    })
                    .when(on, |d| d.bg(rgb(colors().selected)))
                    .when(!on, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        match n {
                            0 => {
                                v.scope = Scope::Every;
                                v.picking = None;
                                return v.changed_meta(cx);
                            }
                            1 => {
                                v.picking =
                                    (v.picking != Some(Picking::Group)).then_some(Picking::Group)
                            }
                            _ => {
                                v.picking =
                                    (v.picking != Some(Picking::Host)).then_some(Picking::Host)
                            }
                        }
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let mut b = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Self::heading(&tr!("Offered for")))
            .child(seg);
        if let Some(p @ (Picking::Group | Picking::Host)) = self.picking {
            let (names, empty) = if p == Picking::Group {
                (groups, tr!("No groups yet: make one in the host list."))
            } else {
                (hosts, tr!("No hosts yet."))
            };
            let names = names
                .into_iter()
                .map(|n| {
                    let chosen = matches!(&self.scope, Scope::Group(g) | Scope::Host(g) if *g == n);
                    (n.clone(), n, chosen)
                })
                .collect();
            b = b.child(Self::list(names, empty, cx, move |v, name, cx| {
                v.scope = if p == Picking::Group {
                    Scope::Group(name)
                } else {
                    Scope::Host(name)
                };
                v.picking = None;
                v.changed_meta(cx);
            }));
        }
        b.child(hint_text(&tr!(
            "The palette lists it in panes of those hosts only."
        )))
    }

    /// The environments it runs with.
    fn needs(&self, envs: &[Environment], cx: &mut Context<Self>) -> gpui::Div {
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for (i, id) in self.snippet.needs.iter().enumerate() {
            let shown = envs
                .iter()
                .find(|e| e.id == *id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| tr!("(deleted)"));
            let gone = id.clone();
            chips = chips.child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .rounded(px(5.))
                    .bg(rgb(colors().chip))
                    .border_1()
                    .border_color(rgb(colors().edge))
                    .text_size(ui(12.))
                    .child(shown)
                    .child(
                        div()
                            .id(("snippet-need-drop", i))
                            .px(px(5.))
                            .rounded(px(3.))
                            .text_color(rgb(colors().muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.snippet.needs.retain(|n| *n != gone);
                                v.changed_meta(cx);
                            }))
                            .child("×"),
                    ),
            );
        }
        let left: Vec<&Environment> = envs
            .iter()
            .filter(|e| !self.snippet.needs.contains(&e.id))
            .collect();
        if !left.is_empty() {
            chips = chips.child(
                div()
                    .id("snippet-need-add")
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(colors().edge))
                    .text_size(ui(12.))
                    .text_color(rgb(colors().soft))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.picking = (v.picking != Some(Picking::Environment))
                            .then_some(Picking::Environment);
                        cx.notify();
                    }))
                    .child(tr!("+ Add")),
            );
        }
        let mut b = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Self::heading(&tr!("Environments it needs")));
        if envs.is_empty() {
            return b.child(hint_text(&tr!(
                "None yet: add one in Settings › Environments."
            )));
        }
        b = b.child(chips);
        if self.picking == Some(Picking::Environment) {
            let names = left
                .iter()
                .map(|e| {
                    (
                        e.id.clone(),
                        format!(
                            "{} · {}",
                            e.name,
                            tr_n!(e.names.len(), "{n} variable", "{n} variables")
                        ),
                        false,
                    )
                })
                .collect();
            b = b.child(Self::list(names, String::new(), cx, |v, id, cx| {
                v.snippet.needs.push(id);
                v.picking = None;
                v.changed_meta(cx);
            }));
        }
        b.child(hint_text(&tr!(
            "It runs in a subshell that has their variables; they go when it ends."
        )))
    }

    /// The `{{name}}` values the command takes, and where each comes from.
    fn values(&self, mono: gpui::SharedString, cx: &App) -> gpui::Div {
        let vars = snippets::vars(self.command.read(cx).text());
        let mut b = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(Self::heading(&tr!("Values")));
        for v in &vars {
            let from_host = BUILT_IN.contains(&v.as_str());
            b = b.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .text_size(ui(12.))
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_color(rgb(colors().code_hole))
                            .child(format!("{{{{{v}}}}}")),
                    )
                    .child(
                        div()
                            .text_color(rgb(if from_host {
                                colors().muted
                            } else {
                                colors().soft
                            }))
                            .child(if from_host {
                                tr!("from the pane's host")
                            } else {
                                tr!("asked when it runs")
                            }),
                    ),
            );
        }
        b.child(hint_text(&tr!(
            "{{host}}, {{user}} and {{address}} are filled in; any other {{name}} is asked for when it runs."
        )))
    }

    /// Beside the command: where it is offered, what it needs, its values,
    /// and Enter.
    fn side(&self, mono: gpui::SharedString, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let envs = self
            .vault
            .ready()
            .and_then(|v| v.environments().ok())
            .unwrap_or_default();
        let enter = self.snippet.enter;
        div()
            .id("snippet-side")
            .w(px(300.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(20.))
            .p(px(16.))
            .overflow_y_scroll()
            .bg(rgb(colors().panel))
            .border_l_1()
            .border_color(rgb(colors().line))
            .child(self.offered(cx))
            .child(self.needs(&envs, cx))
            .child(self.values(mono, cx))
            .child(
                div()
                    .id("snippet-enter")
                    .debug_selector(|| "snippet-enter".into())
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .cursor_pointer()
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.snippet.enter = !v.snippet.enter;
                        v.changed_meta(cx);
                    }))
                    .child(div().mt(px(1.)).child(checkbox(enter)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(label_text(&tr!("Run it: press Enter after typing it")))
                            .child(hint_text(&tr!(
                                "Off: it is typed, and waits for you to read it."
                            ))),
                    ),
            )
    }

    /// Below: how it stands, and what to do with it.
    fn footer(&self, cx: &mut Context<Self>) -> gpui::Div {
        let status = if !self.loaded {
            None
        } else if let Some(l) = self.lacking(cx) {
            Some(l)
        } else if self.saved_at != self.edits {
            Some(tr!("Saving…"))
        } else if !self.snippet.id.is_empty() {
            Some(tr!("Saved"))
        } else {
            None
        };
        let lines = self.command.read(cx).text().lines().count();
        let pane = (self.hooks.pane)(cx);
        let can_run = self.loaded && !self.command.read(cx).text().trim().is_empty();
        let mut b = div()
            .h(px(52.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .bg(rgb(colors().panel))
            .border_t_1()
            .border_color(rgb(colors().line))
            .text_size(ui(12.))
            .when_some(status, |d, s| {
                d.child(div().text_color(rgb(colors().muted)).child(s))
            })
            .when(lines > 0, |d| {
                d.child(div().text_color(rgb(colors().faint)).child(tr_n!(
                    lines,
                    "{n} line",
                    "{n} lines"
                )))
            })
            .child(div().flex_1());
        if !self.snippet.id.is_empty() {
            b = b.child(
                Self::button("snippet-delete", tr!("Delete"))
                    .bg(rgb(colors().panel))
                    .border_color(rgb(colors().bad_edge))
                    .text_color(rgb(colors().bad_text))
                    .on_click(cx.listener(|v, _, w, cx| v.delete(w, cx))),
            );
        }
        match pane {
            Some(p) if can_run => {
                // Typed alone it cannot have its environments: those it
                // runs with.
                if self.snippet.needs.is_empty() {
                    b = b.child(
                        Self::button(
                            "snippet-insert",
                            tr!("Insert into {pane}", pane = p.clone()),
                        )
                        .on_click(cx.listener(|v, _, _, cx| v.run(false, cx))),
                    );
                }
                b.child(
                    Self::styled_button("snippet-run", tr!("Run in {pane}", pane = p), true)
                        .on_click(cx.listener(|v, _, _, cx| v.run(true, cx))),
                )
            }
            Some(_) => b,
            None => b.child(
                div()
                    .text_color(rgb(colors().muted))
                    .child(tr!("Connect a tab in the main window to run it there.")),
            ),
        }
    }
}

impl Focusable for SnippetWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SnippetWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = cx.try_global::<NoteLook>().cloned();
        let ui_size = look.as_ref().map_or(13., |l| l.size);
        window.set_rem_size(look::rem_for(ui_size));
        let mono = look
            .as_ref()
            .map(|l| l.mono.clone())
            .unwrap_or_else(|| "monospace".into());
        let name = self.name.read(cx).text().trim().to_string();
        let shown_name = if name.is_empty() {
            tr!("New snippet")
        } else {
            name
        };
        window.set_window_title(&shown_name);
        // Its size as it would be restored (a maximized window's too).
        let b = window.window_bounds().get_bounds().size;
        self.size = Some((f32::from(b.width), f32::from(b.height)));
        let focused = |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
        let (name_on, cat_on) = (focused(&self.name), focused(&self.category));
        let code: gpui::AnyElement = if self.vault.ready().is_none() {
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
                .debug_selector(|| "snippet-command".into())
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .py(px(10.))
                .bg(rgb(colors().field))
                .font_family(mono.clone())
                .text_size(ui(13.))
                .line_height(px(22.))
                .child(self.command.clone())
                .into_any_element()
        };
        let content = div()
            .id("snippet")
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
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .p(px(14.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(
                        Self::field_box(&self.name, name_on)
                            .flex_1()
                            .text_size(ui(15.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(Self::field_box(&self.category, cat_on).w(px(200.))),
            )
            .when_some(self.problem.clone(), |d, p| {
                d.child(div().px(px(14.)).py(px(6.)).child(problem_text(&p)))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(code)
                    .child(self.side(mono, cx)),
            )
            .child(self.footer(cx));
        let bar = chrome::title_bar(shown_name);
        let content = div().flex_1().min_h_0().flex().flex_col().child(content);
        let root = chrome::window_chrome(bar, content, rgb(colors().surface).into(), window);
        root.when_some(look.and_then(|l| l.family), |d, f| d.font_family(f))
            .children(crate::app_lock::cover(cx))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_snippet_window_opens_as_the_last_one_closed_but_not_smaller() {
        use super::opening_size;
        assert_eq!(opening_size(None), (1000., 680.));
        assert_eq!(opening_size(Some((1200., 900.))), (1200., 900.));
        assert_eq!(opening_size(Some((500., 300.))), (700., 480.));
    }
}

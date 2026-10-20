//! A note in a window of its own: its title, category, hosts and format
//! above, its text below (edited, or shown as Markdown). Saved a moment
//! after typing stops, and when the window closes or the app quits. While
//! the vault is locked the text is not in the window; a sensitive note's
//! text shows only once the user is confirmed (as an environment's values).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, MouseButton, PromptLevel, Render, SharedString, Subscription, Window,
    WindowBounds, WindowDecorations, WindowHandle, WindowOptions, anchored, deferred, div,
    prelude::*, px, rgb, size,
};
use shellwing_i18n::tr;
use shellwing_vault::{Error as VaultError, Note, NoteFormat};

use crate::chrome;
use crate::colors::colors;
use crate::host_editor::{checkbox, hint_text, primary_button, problem_text};
use crate::input::{Changed, TextInput};
use crate::look::{self, ui};
use crate::secrets::VaultSlot;
use shellwing_note_editor::{EditorEvent, EditorState, SyntaxStyle};

/// How long after the last key a note is saved.
const SAVE_AFTER: Duration = Duration::from_secs(1);
/// How often a window looks whether the vault locked or opened.
const WATCH: Duration = Duration::from_secs(1);

/// The UI font the windows are drawn in, as the main window has it.
#[derive(Clone, PartialEq)]
pub struct NoteLook {
    pub size: f32,
    pub family: Option<SharedString>,
    /// The terminal's font, for code.
    pub mono: SharedString,
}

impl gpui::Global for NoteLook {}

/// What a note window tells the rest of the app.
#[derive(Clone)]
pub struct Hooks {
    /// The vault changed: sync it, refresh lists.
    pub saved: Rc<dyn Fn(&mut App)>,
    /// A key or a click: the vault is in use (it locks when idle).
    pub touched: Rc<dyn Fn(&mut App)>,
    /// The hosts a note can be about.
    pub hosts: HostNames,
    /// The main window's focused pane, when its shell can take text: its
    /// title.
    pub pane: PaneTitle,
    /// Types text into that pane, without Enter.
    pub insert: Insert,
}

/// See [`Hooks::pane`].
pub type PaneTitle = Rc<dyn Fn(&App) -> Option<String>>;
/// See [`Hooks::insert`].
pub type Insert = Rc<dyn Fn(String, &mut App) -> Result<(), String>>;

/// The names of the hosts, as the app has them now.
pub type HostNames = Rc<dyn Fn(&App) -> Vec<String>>;

/// The note windows open, by note id.
#[derive(Default)]
struct Opened(HashMap<String, WindowHandle<NoteWindow>>);

impl gpui::Global for Opened {}

/// Opens `note` (None: a new one) in a window, or brings forward the one
/// it is open in.
pub fn open(note: Option<Note>, vault: VaultSlot, hooks: Hooks, cx: &mut App) {
    if let Some(id) = note.as_ref().map(|n| n.id.clone())
        && let Some(h) = cx.default_global::<Opened>().0.get(&id).copied()
        && h.update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let client_side = cfg!(any(target_os = "linux", target_os = "freebsd"));
    let title: SharedString = note
        .as_ref()
        .map(|n| n.title.clone())
        .unwrap_or_else(|| tr!("New note"))
        .into();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(800.), px(700.)),
            cx,
        ))),
        window_decorations: client_side.then_some(WindowDecorations::Client),
        titlebar: (!client_side).then_some(gpui::TitlebarOptions {
            title: Some(title),
            appears_transparent: true,
            traffic_light_position: Some(chrome::TRAFFIC_LIGHTS),
        }),
        app_id: Some("shellwing".into()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| NoteWindow::new(note, vault, hooks, window, cx))
    });
    match opened {
        Ok(h) => {
            let _ = h.update(cx, |v, window, cx| {
                v.handle = Some(h);
                v.note_open(cx);
                window.activate_window();
            });
        }
        Err(e) => eprintln!("cannot open a note's window: {e}"),
    }
}

/// The vault is about to lock: every note window saves, and lets go of its
/// text.
pub fn before_lock(cx: &mut App) {
    let windows: Vec<_> = cx.default_global::<Opened>().0.values().copied().collect();
    for h in windows {
        let _ = h.update(cx, |v, _, cx| {
            v.save(cx);
            v.unload(cx);
        });
    }
}

/// Asking it is the user before a sensitive note's text shows.
enum Ask {
    /// Windows Hello or Touch ID is asking.
    Waiting,
    /// The vault's passphrase, as typed; what was wrong with the last one.
    Passphrase(Entity<TextInput>, Option<String>),
    /// No passphrase guards the vault: a yes.
    Yes,
}

pub struct NoteWindow {
    /// Its window, noted under the note's id once it has one.
    handle: Option<WindowHandle<NoteWindow>>,
    vault: VaultSlot,
    hooks: Hooks,
    note: Note,
    focus: FocusHandle,
    title: Entity<TextInput>,
    category: Entity<TextInput>,
    /// The text's editor (shellwing-note-editor: zorite-editor, taken in).
    text: Entity<EditorState>,
    /// The editor auto-grows: it scrolls in here, with a bar.
    scroll: gpui::ScrollHandle,
    drag: crate::scrollbar::Drag,
    /// Frames left in which the view brings the caret into sight (it moved).
    reveal: u8,
    /// The pane "Insert into" names in the editor's menu, as last set.
    insert_into: Option<Option<String>>,
    /// The text is in the field (not while locked, nor a sensitive note's
    /// before it is shown).
    loaded: bool,
    /// A sensitive note's text, shown.
    shown: bool,
    /// Markdown, drawn rather than edited.
    preview: bool,
    /// The list of hosts to add, open.
    picking: bool,
    ask: Option<Ask>,
    /// Counts the edits, so a save waits for the last one.
    edits: u64,
    saved_at: u64,
    problem: Option<String>,
    _subs: Vec<Subscription>,
}

impl NoteWindow {
    fn new(
        note: Option<Note>,
        vault: VaultSlot,
        hooks: Hooks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let new = note.is_none();
        let note = note.unwrap_or(Note {
            id: String::new(),
            title: String::new(),
            category: String::new(),
            format: NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        });
        let field = |placeholder: fn() -> String, text: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut i = TextInput::said(placeholder, cx);
                i.set_text(text, text.len()..text.len(), cx);
                i
            })
        };
        let title = field(|| tr!("Title"), &note.title, cx);
        let category = field(|| tr!("No category"), &note.category, cx);
        let text = cx.new(|cx| {
            let mut e = EditorState::new(window, cx)
                .with_placeholder(tr!("Write here; Markdown shows as you type."));
            e.set_labels(editor_labels(), cx);
            // A selection made with the mouse opens the menu, with the
            // app's items first.
            e.set_menu_on_select(true);
            e
        });
        let mut subs = vec![];
        for i in [&title, &category] {
            subs.push(cx.subscribe(i, |v, _, _: &Changed, cx| v.edited(cx)));
        }
        subs.push(cx.subscribe(&text, |v, _, e: &EditorEvent, cx| match e {
            EditorEvent::Changed => {
                v.reveal = 2;
                v.edited(cx);
            }
            EditorEvent::SelectionChanged => {
                v.reveal = 2;
                cx.notify();
            }
            EditorEvent::HostItem(id) if id.as_ref() == "insert" => v.insert(cx),
            EditorEvent::InsertCode(code) => v.insert_text(code.to_string(), cx),
            EditorEvent::OpenLink(url) if zorite_markdown::syntax::is_safe_external_url(url) => {
                cx.open_url(url)
            }
            _ => {}
        }));
        // Saved as it goes, so leaving keeps the last of it.
        cx.on_release(|v, cx| {
            v.save(cx);
            let id = v.note.id.clone();
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
        let preview = !new && note.format == NoteFormat::Markdown;
        let mut v = NoteWindow {
            handle: None,
            vault,
            hooks,
            note,
            focus: cx.focus_handle(),
            title,
            category,
            text,
            loaded: false,
            shown: false,
            preview,
            picking: false,
            ask: None,
            edits: 0,
            saved_at: 0,
            problem: None,
            scroll: gpui::ScrollHandle::new(),
            drag: Default::default(),
            reveal: 0,
            insert_into: None,
            _subs: subs,
        };
        v.load(cx);
        let f = if new {
            v.title.read(cx).focus_handle(cx)
        } else {
            v.text.read(cx).focus_handle(cx)
        };
        window.focus(&f, cx);
        v
    }

    /// The text into the field, when it may be there.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            return;
        };
        if self.loaded || (self.note.sensitive && !self.shown) {
            return;
        }
        let text = if self.note.id.is_empty() {
            // A new note starts with a guide to write over (not an edit:
            // closed untouched, nothing is kept).
            zeroize::Zeroizing::new(template().to_string())
        } else {
            match vault.note_text(&self.note.id) {
                Ok(Some(t)) => t,
                Ok(None) => {
                    self.problem = Some(tr!("It is not in the vault any more."));
                    return cx.notify();
                }
                Err(e) => {
                    self.problem = Some(e.to_string());
                    return cx.notify();
                }
            }
        };
        let md = self.note.format == NoteFormat::Markdown;
        let ro = self.preview && md;
        self.text.update(cx, |e, cx| {
            e.set_text(text.to_string(), cx);
            style_editor(e, md, cx);
            e.set_read_only(ro, cx);
        });
        // Loading is not an edit.
        self.saved_at = self.edits + 1;
        self.edits += 1;
        self.loaded = true;
        cx.notify();
    }

    /// Notes the window under the note's id, once it has one.
    fn note_open(&self, cx: &mut App) {
        if let Some(h) = self.handle
            && !self.note.id.is_empty()
        {
            cx.default_global::<Opened>()
                .0
                .insert(self.note.id.clone(), h);
        }
    }

    /// Lets go of the text (the vault locked).
    fn unload(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        self.text.update(cx, |e, cx| e.set_text(String::new(), cx));
        self.edits += 1;
        self.saved_at = self.edits;
        self.loaded = false;
        self.shown = false;
        self.ask = None;
        cx.notify();
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        let open = self.vault.ready().is_some();
        if !open && self.loaded {
            self.unload(cx);
        } else if open && !self.loaded {
            self.load(cx);
        }
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
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// What the window holds, as a note.
    fn current(&self, cx: &App) -> Note {
        let text = |i: &Entity<TextInput>| i.read(cx).text().trim().to_string();
        Note {
            title: text(&self.title),
            category: text(&self.category),
            ..self.note.clone()
        }
    }

    /// Saves what changed since the last save; a new note only once it has
    /// a title or some text.
    fn save(&mut self, cx: &mut App) {
        if self.saved_at == self.edits {
            return;
        }
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let note = self.current(cx);
        let text = self.loaded.then(|| self.text.read(cx).text().to_string());
        if note.id.is_empty() && note.title.is_empty() && text.as_deref().unwrap_or("").is_empty() {
            // Nothing yet: nothing to keep.
            self.saved_at = self.edits;
            return;
        }
        match vault.save_note(&note, text.as_deref()) {
            Ok(id) => {
                self.saved_at = self.edits;
                self.problem = None;
                if self.note.id.is_empty() {
                    self.note.id = id;
                    self.note_open(cx);
                }
                self.note.title = note.title;
                self.note.category = note.category;
                (self.hooks.saved)(cx);
            }
            Err(e) => self.problem = Some(crate::environment::said_error(&e)),
        }
    }

    /// Saves now.
    fn save_here(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
        cx.notify();
    }

    /// A change to what the note is (format, hosts, sensitive): saved at
    /// once.
    fn changed_meta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edits += 1;
        (self.hooks.touched)(cx);
        self.save_here(window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.current(cx).title;
        let (yes, no) = (tr!("Delete"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!(
                "Delete {name}?",
                name = if title.is_empty() {
                    tr!("this note")
                } else {
                    title
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
                let id = v.note.id.clone();
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
                }
                // Nothing to save on the way out.
                v.saved_at = v.edits;
                window.remove_window();
            });
        })
        .detach();
    }

    /// The selection as a link, `[it](url)`, `url` selected to type over
    /// (the editor has no link of its own).
    fn link(&mut self, cx: &mut Context<Self>) {
        let sel = self.text.read(cx).selected_range();
        let text = self
            .text
            .read(cx)
            .text()
            .get(sel.clone())
            .unwrap_or("")
            .to_string();
        let url = sel.start + text.len() + 3;
        self.text.update(cx, |e, cx| {
            e.replace_range(sel, &format!("[{text}](url)"), cx);
            e.select(url..url + 3, cx);
        });
        // A replace says nothing: an edit all the same.
        self.edited(cx);
    }

    fn selected_text(&self, cx: &App) -> String {
        let i = self.text.read(cx);
        i.text()
            .get(i.selected_range())
            .unwrap_or_default()
            .to_string()
    }

    /// The selection into the main window's pane, to run from there.
    fn insert(&mut self, cx: &mut Context<Self>) {
        let text = self.selected_text(cx);
        self.insert_text(text, cx);
    }

    /// `text` into the main window's pane (a selection, a code block).
    fn insert_text(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        match (self.hooks.insert)(text, cx) {
            Ok(()) => self.problem = None,
            Err(e) => self.problem = Some(e),
        }
        cx.notify();
    }

    /// A sensitive note's eye: asks it is the user.
    fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let slot = self.vault.clone();
        let Some(vault) = slot.ready() else {
            return;
        };
        let presence = slot.presence().is_some() && vault.has_presence().unwrap_or(false);
        let passphrase = vault.has_vault_passphrase().unwrap_or(false);
        let ask_passphrase =
            move |v: &mut Self, problem, window: &mut Window, cx: &mut Context<Self>| {
                if !passphrase {
                    v.ask = Some(Ask::Yes);
                    return cx.notify();
                }
                let input =
                    cx.new(|cx| TextInput::said(|| tr!("The vault's passphrase"), cx).masked());
                let f = input.read(cx).focus_handle(cx);
                v.ask = Some(Ask::Passphrase(input, problem));
                window.focus(&f, cx);
                cx.notify();
            };
        if !presence {
            return ask_passphrase(self, None, window, cx);
        }
        self.ask = Some(Ask::Waiting);
        cx.notify();
        let job = cx.background_spawn(async move { slot.confirm(None) });
        cx.spawn_in(window, async move |this, cx| {
            let done = job.await;
            let _ = this.update_in(cx, |v, window, cx| match done {
                Ok(()) => v.confirmed(cx),
                Err(e) => {
                    let problem = (!matches!(e, VaultError::NotPresent)).then(|| e.to_string());
                    ask_passphrase(v, problem, window, cx)
                }
            });
        })
        .detach();
    }

    fn answer(&mut self, cx: &mut Context<Self>) {
        match &self.ask {
            Some(Ask::Yes) => self.confirmed(cx),
            Some(Ask::Passphrase(input, _)) => {
                let typed = zeroize::Zeroizing::new(input.read(cx).text().to_string());
                input.update(cx, |i, cx| i.set_text("", 0..0, cx));
                let slot = self.vault.clone();
                let job = cx.background_spawn(async move { slot.confirm(Some(typed.as_str())) });
                cx.spawn(async move |this, cx| {
                    let done = job.await;
                    let _ = this.update(cx, |v, cx| match done {
                        Ok(()) => v.confirmed(cx),
                        Err(e) => {
                            if let Some(Ask::Passphrase(_, problem)) = &mut v.ask {
                                *problem = Some(match e {
                                    VaultError::WrongPassphrase => tr!("Wrong passphrase."),
                                    e => e.to_string(),
                                });
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            _ => {}
        }
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        self.ask = None;
        self.shown = true;
        self.load(cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        (self.hooks.touched)(cx);
        let k = &ev.keystroke;
        let cmd = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        let md = self.note.format == NoteFormat::Markdown
            && self.text.read(cx).focus_handle(cx).is_focused(window);
        // The editor binds B, I, E and the like itself; K (a link) is ours.
        if md && cmd && !k.modifiers.shift && k.key == "k" {
            self.link(cx);
            return cx.stop_propagation();
        }
        match k.key.as_str() {
            "enter" if self.ask.is_some() => self.answer(cx),
            // Saved at once, as editors do.
            "s" if cmd => self.save_here(window, cx),
            "escape" if self.picking => {
                self.picking = false;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn chip(id: impl Into<gpui::ElementId>, label: String, on: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id.into())
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .text_size(ui(12.))
            .cursor_pointer()
            .when(on, |d| {
                d.border_color(rgb(colors().accent))
                    .bg(rgb(colors().selected))
                    .text_color(rgb(colors().text))
            })
            .when(!on, |d| {
                d.border_color(rgb(colors().field_edge))
                    .text_color(rgb(colors().soft))
                    .hover(|s| s.bg(rgb(colors().hover)))
            })
            .child(label)
    }

    fn field_box(input: &Entity<TextInput>, on: bool) -> gpui::Div {
        div()
            .min_h(px(32.))
            .py(px(6.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
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

    /// Format, preview, sensitive, hosts, and delete.
    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let md = self.note.format == NoteFormat::Markdown;
        let mut hosts = div().flex().flex_wrap().items_center().gap(px(6.));
        for (i, h) in self.note.hosts.iter().enumerate() {
            let name = h.clone();
            hosts = hosts.child(
                div()
                    .id(("note-host", i))
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_full()
                    .bg(rgb(colors().chip))
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .text_size(ui(12.))
                    .cursor_pointer()
                    .hover(|s| s.border_color(rgb(colors().bad)))
                    .on_click(cx.listener(move |v, _, w, cx| {
                        v.note.hosts.retain(|x| *x != name);
                        v.changed_meta(w, cx);
                    }))
                    .child(h.clone())
                    .child(div().text_color(rgb(colors().muted)).child("✕")),
            );
        }
        let picker = div()
            .relative()
            .child(
                Self::chip("note-add-host", tr!("For a host ▾"), self.picking).on_click(
                    cx.listener(|v, _, _, cx| {
                        v.picking = !v.picking;
                        cx.notify();
                    }),
                ),
            )
            .when(self.picking, |d| {
                let choices: Vec<String> = (self.hooks.hosts)(cx)
                    .into_iter()
                    .filter(|h| !self.note.hosts.contains(h))
                    .collect();
                let mut list = div()
                    .id("note-host-list")
                    .min_w(px(220.))
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(colors().edge))
                    .bg(rgb(colors().raised))
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                        v.picking = false;
                        cx.notify();
                    }));
                if choices.is_empty() {
                    list = list.child(
                        div()
                            .px_2()
                            .py(px(5.))
                            .text_size(ui(12.5))
                            .text_color(rgb(colors().muted))
                            .child(tr!("No hosts yet.")),
                    );
                }
                for (i, h) in choices.into_iter().enumerate() {
                    let name = h.clone();
                    list = list.child(
                        div()
                            .id(("note-host-choice", i))
                            .px_2()
                            .py(px(5.))
                            .rounded(px(4.))
                            .text_size(ui(13.))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                v.note.hosts.push(name.clone());
                                v.picking = false;
                                v.changed_meta(w, cx);
                            }))
                            .child(h),
                    );
                }
                d.child(deferred(anchored().child(div().mt(px(30.)).child(list))).with_priority(2))
            });
        let _ = window;
        let status = if self.saved_at != self.edits {
            Some(tr!("Saving…"))
        } else if !self.note.id.is_empty() {
            Some(tr!("Saved"))
        } else {
            None
        };
        let left =
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .child(Self::chip("note-plain", tr!("Plain text"), !md).on_click(
                            cx.listener(|v, _, w, cx| {
                                v.note.format = NoteFormat::Plain;
                                v.preview = false;
                                v.text.update(cx, |e, cx| {
                                    style_editor(e, false, cx);
                                    e.set_read_only(false, cx);
                                });
                                v.changed_meta(w, cx);
                            }),
                        ))
                        .child(Self::chip("note-md", "Markdown".to_string(), md).on_click(
                            cx.listener(|v, _, w, cx| {
                                v.note.format = NoteFormat::Markdown;
                                v.text.update(cx, |e, cx| style_editor(e, true, cx));
                                v.changed_meta(w, cx);
                            }),
                        )),
                )
                .when(md, |d| {
                    d.child(
                        Self::chip(
                            "note-preview",
                            if self.preview {
                                tr!("Edit")
                            } else {
                                tr!("Preview")
                            },
                            false,
                        )
                        .on_click(cx.listener(|v, _, w, cx| {
                            v.preview = !v.preview;
                            // The same editor, read-only: the text stays where
                            // it is between the two.
                            let ro = v.preview;
                            v.text.update(cx, |e, cx| e.set_read_only(ro, cx));
                            let f = v.text.read(cx).focus_handle(cx);
                            w.focus(&f, cx);
                            cx.notify();
                        })),
                    )
                })
                .child(
                    div()
                        .id("note-sensitive")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .text_size(ui(12.5))
                        .on_click(cx.listener(|v, _, w, cx| {
                            v.note.sensitive = !v.note.sensitive;
                            // Who turns it on sees the text already.
                            v.shown = true;
                            v.changed_meta(w, cx);
                        }))
                        .child(checkbox(self.note.sensitive))
                        .child(tr!("Sensitive")),
                )
                .child(hosts)
                .child(picker);
        let right = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .when_some(status, |d, s| {
                d.child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(colors().muted))
                        .child(s),
                )
            })
            .child(
                Self::chip("note-delete", tr!("Delete"), false)
                    .text_color(rgb(colors().bad))
                    .on_click(cx.listener(|v, _, w, cx| v.delete(w, cx))),
            );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(left)
            .child(right)
    }

    /// The text: edited, drawn as Markdown, or held back (locked, or
    /// sensitive and not shown).
    fn body(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let look = cx.try_global::<NoteLook>().cloned();
        let mono = look
            .as_ref()
            .map(|l| l.mono.clone())
            .unwrap_or_else(|| "monospace".into());
        let panel = |msg: String| {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .text_color(rgb(colors().muted))
                .child(msg)
        };
        if self.vault.ready().is_none() {
            return panel(tr!("The vault is locked: unlock it in the main window."))
                .into_any_element();
        }
        if self.note.sensitive && !self.shown {
            let mut p = panel(tr!("Sensitive: its text shows once it is you."));
            p = match &self.ask {
                None => p.child(
                    primary_button("note-show", &tr!("Show the text"))
                        .on_click(cx.listener(|v, _, w, cx| v.reveal(w, cx))),
                ),
                Some(Ask::Waiting) => {
                    p.child(hint_text(&tr!("Confirm it is you in the system's prompt…")))
                }
                Some(Ask::Yes) => p
                    .child(hint_text(&tr!(
                        "No passphrase guards the vault, so anyone at this device can show it."
                    )))
                    .child(
                        primary_button("note-yes", &tr!("Show"))
                            .on_click(cx.listener(|v, _, _, cx| v.answer(cx))),
                    ),
                Some(Ask::Passphrase(input, problem)) => {
                    let on = input.read(cx).focus_handle(cx).is_focused(window);
                    p.child(
                        div()
                            .w(px(320.))
                            .flex()
                            .gap(px(8.))
                            .child(Self::field_box(input, on).flex_1())
                            .child(
                                primary_button("note-yes", &tr!("Show"))
                                    .on_click(cx.listener(|v, _, _, cx| v.answer(cx))),
                            ),
                    )
                    .when_some(problem.clone(), |d, e| d.child(problem_text(&e)))
                }
            };
            return p.into_any_element();
        }
        let on = self.text.read(cx).focus_handle(cx).is_focused(window);
        let md = self.note.format == NoteFormat::Markdown;
        let family = look.and_then(|l| l.family);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(if on {
                colors().accent
            } else {
                colors().field_edge
            }))
            .bg(rgb(colors().field))
            .overflow_hidden()
            .child(
                div()
                    .id("note-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .text_size(ui(13.5))
                            .text_color(rgb(colors().text))
                            // Markdown in the app's font (code in the terminal's);
                            // plain text in the terminal's.
                            .when(!md, |d| d.font_family(mono.clone()))
                            .when_some(family.filter(|_| md), |d, f| d.font_family(f))
                            .child(self.text.clone()),
                    ),
            )
            .child(crate::scrollbar::overlay(
                self.scroll.clone(),
                self.drag.clone(),
            ))
            .into_any_element()
    }

    /// The app's item in the editor's menu: Insert into the main window's
    /// pane, named; set again only when that changes.
    fn menu_items(&mut self, cx: &mut Context<Self>) {
        let pane = (self.hooks.pane)(cx);
        if self.insert_into.as_ref() == Some(&pane) {
            return;
        }
        let item = shellwing_note_editor::HostItem {
            id: "insert".into(),
            label: match &pane {
                Some(p) => tr!("Insert into {pane}", pane = p.clone()).into(),
                None => tr!("No shell to insert into").into(),
            },
            enabled: pane.is_some(),
        };
        // A code block's Insert too, while a pane can take it.
        let code = pane.as_ref().map(|_| tr!("Insert").into());
        self.insert_into = Some(pane);
        self.text.update(cx, |e, cx| {
            e.set_host_items(vec![item], cx);
            e.set_code_insert(code, cx);
        });
    }

    /// The caret back in sight after it moved (the editor grows; the view
    /// around it scrolls): from where it was drawn last.
    fn reveal_caret(&mut self, cx: &mut Context<Self>) {
        if self.reveal == 0 {
            return;
        }
        self.reveal -= 1;
        let Some(caret) = self.text.read(cx).caret_screen_bounds() else {
            return;
        };
        let view = self.scroll.bounds();
        let mut off = self.scroll.offset();
        let margin = px(8.);
        if caret.top() < view.top() + margin {
            off.y += view.top() + margin - caret.top();
        } else if caret.bottom() > view.bottom() - margin {
            off.y -= caret.bottom() - (view.bottom() - margin);
        } else {
            return;
        }
        off.y = off.y.min(px(0.)).max(-self.scroll.max_offset().y);
        self.scroll.set_offset(off);
        cx.notify();
    }
}

/// What a new note starts with, in the app's language: how to write one,
/// a code block, and that it goes into the shell.
fn template() -> &'static str {
    match shellwing_i18n::language() {
        "vi" => include_str!("note_templates/vi.md"),
        "de" => include_str!("note_templates/de.md"),
        "es" => include_str!("note_templates/es.md"),
        "fr" => include_str!("note_templates/fr.md"),
        "ja" => include_str!("note_templates/ja.md"),
        "ko" => include_str!("note_templates/ko.md"),
        "pt" => include_str!("note_templates/pt.md"),
        "ru" => include_str!("note_templates/ru.md"),
        "zh" => include_str!("note_templates/zh.md"),
        _ => include_str!("note_templates/en.md"),
    }
}

/// The editor in the app's colors: Markdown shown as it is typed, or plain
/// text.
fn style_editor(e: &mut EditorState, markdown: bool, cx: &mut Context<EditorState>) {
    if !markdown {
        return e.clear_markdown_style(cx);
    }
    let c = colors();
    let hsla = |v: u32| -> gpui::Hsla { rgb(v).into() };
    let mono = cx
        .try_global::<NoteLook>()
        .map_or_else(|| SharedString::from("monospace"), |l| l.mono.clone());
    e.set_markdown_style(
        SyntaxStyle {
            marker: hsla(c.faint),
            code: hsla(c.accent),
            code_bg: hsla(c.chip),
            link: hsla(c.accent),
            tag: hsla(c.accent),
            quote: hsla(c.soft),
            alert_note: hsla(c.accent),
            alert_tip: hsla(c.good),
            alert_important: hsla(c.accent),
            alert_warning: hsla(c.warn),
            alert_caution: hsla(c.bad),
            alert_icons: None,
            rule: hsla(c.line),
            mark_bg: hsla(c.warn_bg),
            block_label: None,
            block_label_gen: 0,
            block_ref_count: None,
            popover_bg: hsla(c.raised),
            popover_border: hsla(c.edge),
            popover_fg: hsla(c.text),
            popover_hover: hsla(c.hover),
            popover_divider: hsla(c.line),
            popover_danger: hsla(c.bad),
            mono: gpui::font(mono),
            property_icon: None,
        },
        cx,
    );
}

/// The editor's menus, in the app's language.
fn editor_labels() -> shellwing_note_editor::Labels {
    shellwing_note_editor::Labels {
        cut: tr!("Cut").into(),
        copy: tr!("Copy").into(),
        copy_as_markdown: tr!("Copy as Markdown").into(),
        paste: tr!("Paste").into(),
        code_copy: tr!("Copy").into(),
        turn_into: tr!("Turn into").into(),
        text: tr!("Text").into(),
        heading_1: tr!("Heading 1").into(),
        heading_2: tr!("Heading 2").into(),
        heading_3: tr!("Heading 3").into(),
        bulleted_list: tr!("Bulleted list").into(),
        numbered_list: tr!("Numbered list").into(),
        todo: tr!("To-do").into(),
        quote: tr!("Quote").into(),
        callout: tr!("Callout").into(),
        code_block: tr!("Code block").into(),
        insert_row_above: tr!("Insert row above").into(),
        insert_row_below: tr!("Insert row below").into(),
        duplicate_row: tr!("Duplicate row").into(),
        insert_column_left: tr!("Insert column left").into(),
        insert_column_right: tr!("Insert column right").into(),
        align_left: tr!("Align left").into(),
        align_center: tr!("Align center").into(),
        align_right: tr!("Align right").into(),
        grid_style: tr!("Grid style").into(),
        striped_style: tr!("Striped style").into(),
        header_style: tr!("Header style").into(),
        minimal_style: tr!("Minimal style").into(),
        delete_row: tr!("Delete row").into(),
        delete_column: tr!("Delete column").into(),
        delete_table: tr!("Delete table").into(),
        ..Default::default()
    }
}

impl Focusable for NoteWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NoteWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = cx.try_global::<NoteLook>().cloned();
        let ui_size = look.as_ref().map_or(13., |l| l.size);
        window.set_rem_size(look::rem_for(ui_size));
        let title = self.title.read(cx).text().trim().to_string();
        let shown_title = if title.is_empty() {
            tr!("Untitled note")
        } else {
            title
        };
        window.set_window_title(&shown_title);
        self.reveal_caret(cx);
        self.menu_items(cx);
        let focused = |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
        let (title_on, cat_on) = (focused(&self.title), focused(&self.category));
        let content = div()
            .id("note")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| (v.hooks.touched)(cx)),
            )
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        Self::field_box(&self.title, title_on)
                            .flex_1()
                            .text_size(ui(16.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(Self::field_box(&self.category, cat_on).w(px(200.))),
            )
            .child(self.toolbar(window, cx))
            .when_some(self.problem.clone(), |d, p| d.child(problem_text(&p)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(self.body(window, cx)),
            );
        let bar = div()
            .h_full()
            .flex()
            .items_center()
            .px(px(14.))
            .text_size(ui(13.))
            .child(shown_title);
        let content = div().size_full().flex().flex_col().child(content);
        let root = chrome::window_chrome(bar, content, rgb(colors().surface).into(), window);
        root.when_some(look.and_then(|l| l.family), |d, f| d.font_family(f))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_template_shows_a_command_to_insert() {
        for t in [
            include_str!("note_templates/en.md"),
            include_str!("note_templates/vi.md"),
            include_str!("note_templates/de.md"),
            include_str!("note_templates/es.md"),
            include_str!("note_templates/fr.md"),
            include_str!("note_templates/ja.md"),
            include_str!("note_templates/ko.md"),
            include_str!("note_templates/pt.md"),
            include_str!("note_templates/ru.md"),
            include_str!("note_templates/zh.md"),
        ] {
            assert!(t.starts_with("# "), "a heading first");
            assert_eq!(t.matches("```").count(), 2, "one code block");
            assert!(t.contains("sudo systemctl restart nginx"));
            assert!(t.contains("- [ ] "), "a task");
        }
    }
}

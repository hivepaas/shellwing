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
use crate::host_editor::{checkbox, hint_text, outline_button, primary_button, problem_text};
use crate::input::{Changed, TextInput};
use crate::look::{self, ui};
use crate::secrets::VaultSlot;

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
}

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
            size(px(760.), px(680.)),
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
    text: Entity<TextInput>,
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
            TextInput::said(|| tr!("Write here; Markdown shows when previewed."), cx)
                .multiline(10, 10)
        });
        let mut subs = vec![];
        for i in [&title, &category, &text] {
            subs.push(cx.subscribe(i, |v, _, _: &Changed, cx| v.edited(cx)));
        }
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
            _subs: subs,
        };
        v.load(cx);
        let first = if new { &v.title } else { &v.text };
        let f = first.read(cx).focus_handle(cx);
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
            Default::default()
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
        self.text.update(cx, |i, cx| i.set_text(&text, 0..0, cx));
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
        self.text.update(cx, |i, cx| i.set_text("", 0..0, cx));
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
                    .child(
                        Self::chip("note-plain", tr!("Plain text"), !md).on_click(cx.listener(
                            |v, _, w, cx| {
                                v.note.format = NoteFormat::Plain;
                                v.preview = false;
                                v.changed_meta(w, cx);
                            },
                        )),
                    )
                    .child(Self::chip("note-md", "Markdown".to_string(), md).on_click(
                        cx.listener(|v, _, w, cx| {
                            v.note.format = NoteFormat::Markdown;
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
                        if !v.preview {
                            let f = v.text.read(cx).focus_handle(cx);
                            w.focus(&f, cx);
                        }
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
            .child(picker)
            .child(div().flex_1())
            .child(
                div()
                    .text_size(ui(11.5))
                    .text_color(rgb(colors().muted))
                    .child(if self.saved_at == self.edits {
                        tr!("Saved")
                    } else {
                        tr!("Editing…")
                    }),
            )
            .child(
                outline_button("note-delete", &tr!("Delete"))
                    .text_color(rgb(colors().bad))
                    .on_click(cx.listener(|v, _, w, cx| v.delete(w, cx))),
            )
    }

    /// The text: edited, drawn as Markdown, or held back (locked, or
    /// sensitive and not shown).
    fn body(&self, rows: usize, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let look = cx.try_global::<NoteLook>().cloned();
        let mono = look.map(|l| l.mono).unwrap_or_else(|| "monospace".into());
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
        if self.preview && self.note.format == NoteFormat::Markdown {
            let text = self.text.read(cx).text().to_string();
            let blocks = crate::markdown::parse(&text);
            return div()
                .id("note-preview")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(4.))
                .text_size(ui(13.5))
                .child(if blocks.is_empty() {
                    div()
                        .text_color(rgb(colors().muted))
                        .child(tr!("Nothing written yet: Edit to write."))
                } else {
                    crate::markdown::render(&blocks, mono, "md")
                })
                .into_any_element();
        }
        self.text.update(cx, |i, cx| i.set_rows(rows, rows, cx));
        let on = self.text.read(cx).focus_handle(cx).is_focused(window);
        Self::field_box(&self.text, on)
            .font_family(mono)
            .items_start()
            .into_any_element()
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
        // The editor's rows: what is left of the window under the bars.
        let line = ui_size * 1.6;
        let left = f32::from(window.viewport_size().height) - chrome::TITLEBAR_H - 150.;
        let rows = ((left / line).floor() as usize).max(4);
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
            .child(self.body(rows, window, cx));
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

//! A note in a window of its own: its title, category, hosts and format
//! above, its text below (edited, or shown as Markdown). Saved a moment
//! after typing stops, and when the window closes or the app quits. While
//! the vault is locked the text is not in the window; a sensitive note's
//! text shows only once the user is confirmed (as an environment's values).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, ClipboardEntry, Context, Entity, ExternalPaths, FocusHandle,
    Focusable, FontWeight, KeyDownEvent, MouseButton, PathPromptOptions, PromptLevel, Render,
    SharedString, Subscription, Window, WindowBounds, WindowDecorations, WindowHandle,
    WindowOptions, anchored, deferred, div, prelude::*, px, rgb, size,
};
use shellwing_i18n::{tr, tr_n};
use shellwing_vault::{Error as VaultError, Note, NoteFormat};

use crate::chrome;
use crate::colors::colors;
use crate::host_editor::{checkbox, hint_text, primary_button, problem_text};
use crate::input::{Changed, TextInput};
use crate::look::{self, ui};
use crate::note_images::{self, Images, SharedImages, Source};
use crate::secrets::VaultSlot;
use shellwing_note_editor::{EditorEvent, EditorState, SyntaxStyle};

/// A note's window when none was opened before, and the least it opens at.
const DEFAULT_SIZE: (f32, f32) = (800., 700.);
const LEAST_SIZE: (f32, f32) = (600., 500.);

/// How long after the last key a note is saved.
const SAVE_AFTER: Duration = Duration::from_secs(3);
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
    /// A note was saved (on this device only): lists show it again.
    pub saved: Rc<dyn Fn(&mut App)>,
    /// A note is open in its window: left out of syncs until let go.
    pub hold: Hold,
    /// Its window is done with it: it syncs again, at once when it
    /// changed meanwhile.
    pub release: Release,
    /// A key or a click: the vault is in use (it locks when idle).
    pub touched: Rc<dyn Fn(&mut App)>,
    /// The hosts a note can be about.
    pub hosts: HostNames,
    /// The main window's focused pane, when its shell can take text: its
    /// title.
    pub pane: PaneTitle,
    /// Types text into that pane, without Enter.
    pub insert: Insert,
    /// The window's size as it closes, for the next to open so.
    pub sized: SizeKept,
}

/// See [`Hooks::sized`].
pub type SizeKept = Rc<dyn Fn(f32, f32, &mut App)>;

/// See [`Hooks::hold`].
pub type Hold = Rc<dyn Fn(&str, &mut App)>;
/// See [`Hooks::release`].
pub type Release = Rc<dyn Fn(&str, bool, &mut App)>;
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
/// The size a note's window opens at: the last one's (`last`), never less
/// than 600 × 500; 800 × 700 the first time.
pub fn opening_size(last: Option<(f32, f32)>) -> (f32, f32) {
    let (w, h) = last.unwrap_or(DEFAULT_SIZE);
    (w.max(LEAST_SIZE.0), h.max(LEAST_SIZE.1))
}

pub fn open(
    note: Option<Note>,
    vault: VaultSlot,
    hooks: Hooks,
    last: Option<(f32, f32)>,
    cx: &mut App,
) {
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
            {
                let (w, h) = opening_size(last);
                size(px(w), px(h))
            },
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

/// Quitting: every note window saves, and lets its note sync again. True
/// when any changed while open, for the app to sync before it goes.
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
    /// The window's size, not maximized, as last drawn: kept as it closes.
    size: Option<(f32, f32)>,
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
    /// Where the For a host chip is drawn: its list opens below it.
    host_chip: Rc<std::cell::Cell<Bounds<gpui::Pixels>>>,
    ask: Option<Ask>,
    /// Counts the edits, so a save waits for the last one.
    edits: u64,
    saved_at: u64,
    /// What was last saved (or loaded), to save only what differs.
    last_saved: Option<u64>,
    /// Saved here since the window held its note: it syncs when let go.
    unsynced: bool,
    problem: Option<String>,
    /// The images the text shows, as loaded (note_images.rs).
    images: SharedImages,
    /// What "Insert Image…" in the editor's menu was last set for: whether
    /// images can go in.
    images_item: Option<bool>,
    /// `data:` images being made attachments: no second round meanwhile.
    taking_in: bool,
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
        let images: SharedImages = Rc::new(RefCell::new(Images::default()));
        let (wanted, to_load) = async_channel::unbounded::<String>();
        let text = cx.new(|cx| {
            let mut e = EditorState::new(window, cx)
                .with_placeholder(tr!("Write here; Markdown shows as you type."));
            e.set_labels(editor_labels(), cx);
            // A selection made with the mouse opens the menu, with the
            // app's items first.
            e.set_menu_on_select(true);
            note_images::show_in(&mut e, images.clone(), wanted);
            // An image's own menu: copy it, save it as a file.
            e.set_image_items(
                vec![
                    shellwing_note_editor::HostItem {
                        id: "copy-image".into(),
                        label: tr!("Copy Image").into(),
                        enabled: true,
                        selection: false,
                    },
                    shellwing_note_editor::HostItem {
                        id: "save-image".into(),
                        label: tr!("Save Image As…").into(),
                        enabled: true,
                        selection: false,
                    },
                ],
                cx,
            );
            e
        });
        // The images the editor asks for, read and decoded off the UI thread.
        cx.spawn(async move |this, cx| {
            while let Ok(id) = to_load.recv().await {
                let Ok(vault) = this.update(cx, |v, _| v.vault.ready()) else {
                    break;
                };
                let Some(vault) = vault else {
                    // Locked: asked for again once the text is back.
                    let _ = this.update(cx, |v, _| v.images.borrow_mut().forget(&id));
                    continue;
                };
                let loaded = cx
                    .background_spawn({
                        let id = id.clone();
                        async move { note_images::load(&vault, &id) }
                    })
                    .await;
                let shown = this.update(cx, |v, cx| {
                    v.images.borrow_mut().put(id, loaded);
                    v.text.update(cx, |_, cx| cx.notify());
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut subs = vec![];
        for i in [&title, &category] {
            subs.push(cx.subscribe(i, |v, _, _: &Changed, cx| v.edited(cx)));
        }
        subs.push(cx.subscribe(&text, |v, _, e: &EditorEvent, cx| match e {
            EditorEvent::Changed => {
                v.reveal = 2;
                v.edited(cx);
                v.take_in_data_images(cx);
            }
            EditorEvent::ImageItem { item, src } => match item.as_ref() {
                "copy-image" => v.copy_image(src, cx),
                "save-image" => v.save_image(src, cx),
                _ => {}
            },
            EditorEvent::SelectionChanged => {
                v.reveal = 2;
                cx.notify();
            }
            EditorEvent::HostItem(id) if id.as_ref() == "insert" => v.insert(cx),
            EditorEvent::HostItem(id) if id.as_ref() == "image" => v.pick_images(cx),
            EditorEvent::InsertCode(code) => v.insert_text(code.to_string(), cx),
            EditorEvent::OpenLink(url) if zorite_markdown::syntax::is_safe_external_url(url) => {
                cx.open_url(url)
            }
            _ => {}
        }));
        // Saved as it goes, so leaving keeps the last of it.
        cx.on_release(|v, cx| {
            v.save(cx);
            v.let_go(cx);
            if let Some((w, h)) = v.size {
                (v.hooks.sized)(w, h, cx);
            }
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
            host_chip: Rc::default(),
            ask: None,
            edits: 0,
            saved_at: 0,
            last_saved: None,
            unsynced: false,
            problem: None,
            images,
            images_item: None,
            taking_in: false,
            scroll: gpui::ScrollHandle::new(),
            drag: Default::default(),
            reveal: 0,
            size: None,
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
        // As it is in the vault: a save of the same is no save. A new
        // note's guide is not kept unless written over.
        self.last_saved = (!self.note.id.is_empty()).then(|| self.snapshot(cx));
        cx.notify();
    }

    /// What a save would keep, hashed: the note's parts and its text.
    fn snapshot(&self, cx: &App) -> u64 {
        use std::hash::{Hash, Hasher};
        let n = self.current(cx);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            &n.title,
            &n.category,
            n.format == NoteFormat::Markdown,
            &n.hosts,
            n.sensitive,
        )
            .hash(&mut h);
        if self.loaded {
            self.text.read(cx).text().hash(&mut h);
        }
        h.finish()
    }

    /// The window is done with its note: it syncs again (at once when it
    /// changed meanwhile).
    fn let_go(&mut self, cx: &mut App) {
        if !self.note.id.is_empty() {
            (self.hooks.release)(&self.note.id, self.unsynced, cx);
        }
        self.unsynced = false;
    }

    /// Between Preview and editing a Markdown note.
    fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = !self.preview;
        // The same editor, read-only: the text stays where it is between the
        // two.
        let ro = self.preview;
        self.text.update(cx, |e, cx| e.set_read_only(ro, cx));
        let f = self.text.read(cx).focus_handle(cx);
        window.focus(&f, cx);
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
            // Open here: it syncs when the window closes.
            (self.hooks.hold)(&self.note.id, cx);
        }
    }

    /// Lets go of the text (the vault locked).
    fn unload(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        self.text.update(cx, |e, cx| e.set_text(String::new(), cx));
        self.images.borrow_mut().clear();
        self.edits += 1;
        self.saved_at = self.edits;
        self.loaded = false;
        self.shown = false;
        self.ask = None;
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
        self.look_for_missing_images(cx);
    }

    /// Images not synced yet are asked for again once a sync brought them.
    fn look_for_missing_images(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let missing = self.images.borrow().missing();
        let mut came = false;
        for id in missing {
            if vault
                .attachments_size(std::slice::from_ref(&id))
                .unwrap_or(0)
                > 0
            {
                self.images.borrow_mut().forget(&id);
                came = true;
            }
        }
        if came {
            self.text.update(cx, |_, cx| cx.notify());
        }
    }

    /// Why images cannot go into the note now, if they cannot.
    fn images_refused(&self) -> Option<String> {
        if self.note.format != NoteFormat::Markdown {
            return Some(tr!(
                "Images show in Markdown notes; this note is plain text."
            ));
        }
        if !self.loaded || self.preview || self.vault.ready().is_none() {
            return Some(tr!("Images go in while the note is edited."));
        }
        None
    }

    /// Pasted with an image (a screenshot) or files on the clipboard: the
    /// editor pastes text itself and passes on what it does not take.
    fn paste_images(
        &mut self,
        _: &shellwing_note_editor::Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .text
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
            cx.propagate();
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let mut sources = vec![];
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) => sources.push(Source::Bytes(image.bytes.clone())),
                ClipboardEntry::ExternalPaths(paths) => {
                    sources.extend(paths.paths().iter().cloned().map(Source::File))
                }
                _ => {}
            }
        }
        if sources.is_empty() {
            cx.propagate();
            return;
        }
        self.add_images(sources, cx);
    }

    /// Files dropped on the window: where they were dropped, at a line's
    /// start or end.
    fn drop_images(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.text.read(cx);
        let at = note_images::drop_offset(
            editor.text(),
            editor.offset_for_position(window.mouse_position()),
        );
        self.add_images_at(
            paths.paths().iter().cloned().map(Source::File).collect(),
            Some(at),
            cx,
        );
    }

    /// The image `src` names, to the clipboard.
    fn copy_image(&mut self, src: &str, cx: &mut Context<Self>) {
        let image = note_images::attachment_id(src)
            .zip(self.vault.ready())
            .and_then(|(id, vault)| note_images::image_bytes(&vault, id));
        match image {
            Some((bytes, ext)) => {
                let format = if ext == "png" {
                    gpui::ImageFormat::Png
                } else {
                    gpui::ImageFormat::Jpeg
                };
                cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                    format, bytes,
                )));
                self.problem = None;
            }
            None => self.problem = Some(tr!("The image cannot be shown")),
        }
        cx.notify();
    }

    /// The image `src` names, saved where the user says.
    fn save_image(&mut self, src: &str, cx: &mut Context<Self>) {
        let image = note_images::attachment_id(src)
            .zip(self.vault.ready())
            .and_then(|(id, vault)| {
                note_images::image_bytes(&vault, id).map(|b| (id.to_string(), b))
            });
        let Some((id, (bytes, ext))) = image else {
            self.problem = Some(tr!("The image cannot be shown"));
            cx.notify();
            return;
        };
        let dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let title = self.title.read(cx).text().trim().to_string();
        let stem = if title.is_empty() {
            format!(
                "image-{}",
                id.get(id.len().saturating_sub(6)..).unwrap_or(&id)
            )
        } else {
            title
        };
        let name = format!("{stem}.{ext}");
        let chosen = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = cx
                .background_spawn(async move { std::fs::write(&path, bytes) })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.problem = written
                    .err()
                    .map(|e| tr!("Not saved: {e}", e = e.to_string()));
                cx.notify();
            });
        })
        .detach();
    }

    /// Images pasted as `data:` URIs (Markdown from elsewhere) become
    /// attachments, as pasted images do.
    fn take_in_data_images(&mut self, cx: &mut Context<Self>) {
        if self.taking_in
            || !self.text.read(cx).text().contains("](data:image/")
            || self.images_refused().is_some()
        {
            return;
        }
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let text = self.text.read(cx).text().to_string();
        self.taking_in = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let mut kept = vec![];
                    for (uri, bytes) in note_images::data_images(&text) {
                        let (ids, _) = note_images::add(&vault, &text, vec![Source::Bytes(bytes)]);
                        if let Some((_, id)) = ids.into_iter().next() {
                            kept.push((uri, id));
                        }
                    }
                    kept
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.taking_in = false;
                if found.is_empty() {
                    return;
                }
                v.text.update(cx, |e, cx| {
                    for (uri, id) in &found {
                        if let Some(at) = e.text().find(uri.as_str()) {
                            e.replace_range(at..at + uri.len(), &format!("attachment:{id}"), cx);
                        }
                    }
                });
                v.edited(cx);
            });
        })
        .detach();
    }

    /// "Download Images": the note's web images, downloaded once and kept
    /// as attachments, their links replaced. Asked first: the hosts learn
    /// the user's address, and plain http can be altered on the way.
    fn download_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(why) = self.images_refused() {
            self.problem = Some(why);
            cx.notify();
            return;
        }
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let text = self.text.read(cx).text().to_string();
        let urls = note_images::remote_images(&text);
        if urls.is_empty() {
            return;
        }
        let mut hosts: Vec<String> = urls
            .iter()
            .filter_map(|u| note_images::host_of(u))
            .collect();
        hosts.sort();
        hosts.dedup();
        let mut detail = tr!(
            "shellwing downloads them from {hosts}, which learn your address. They are then kept in the vault, encrypted, and the links in the note are replaced.",
            hosts = hosts.join(", ")
        );
        if urls.iter().any(|u| u.starts_with("http://")) {
            detail.push(' ');
            detail.push_str(&tr!(
                "Some come over plain http: they could be altered on the way."
            ));
        }
        let (yes, no) = (tr!("Download"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Info,
            &tr_n!(
                urls.len(),
                "Download the note's image from the web?",
                "Download the note's {n} images from the web?"
            ),
            Some(&detail),
            &[yes.as_str(), no.as_str()],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let (kept, problems) = cx
                .background_spawn(async move {
                    let (mut sources, mut from, mut problems) = (vec![], vec![], vec![]);
                    for url in urls {
                        match note_images::fetch(&url) {
                            Ok(bytes) => {
                                from.push(url.clone());
                                sources.push(Source::Downloaded { url, bytes });
                            }
                            Err(e) => problems.push(e.said(&url)),
                        }
                    }
                    let (ids, more) = note_images::add(&vault, &text, sources);
                    problems.extend(more);
                    let kept: Vec<(String, String)> = ids
                        .into_iter()
                        .filter_map(|(i, id)| from.get(i).map(|u| (u.clone(), id)))
                        .collect();
                    (kept, problems)
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                if !kept.is_empty() {
                    v.text.update(cx, |e, cx| {
                        for (url, id) in &kept {
                            let link = format!("]({url})");
                            while let Some(at) = e.text().find(&link) {
                                e.replace_range(
                                    at..at + link.len(),
                                    &format!("](attachment:{id})"),
                                    cx,
                                );
                            }
                        }
                    });
                    v.edited(cx);
                }
                v.problem = (!problems.is_empty()).then(|| problems.join(" "));
                cx.notify();
            });
        })
        .detach();
    }

    /// "Export…": the note and its images, as files, in a folder the user
    /// chooses. They leave the vault unencrypted, so it asks first.
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.ready() else {
            return;
        };
        if !self.loaded {
            return;
        }
        let (title, text) = (
            self.title.read(cx).text().trim().to_string(),
            self.text.read(cx).text().to_string(),
        );
        let (yes, no) = (tr!("Export"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("Export the note?"),
            Some(&tr!(
                "The note and its images are written as files, not encrypted: anyone who can read the folder can read them."
            )),
            &[yes.as_str(), no.as_str()],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(chosen) = this.update(cx, |_, cx| {
                cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(tr!("Export").into()),
                })
            }) else {
                return;
            };
            let Ok(Ok(Some(dirs))) = chosen.await else {
                return;
            };
            let Some(dir) = dirs.into_iter().next() else {
                return;
            };
            let written = cx
                .background_spawn(async move { note_images::export(&vault, &title, &text, &dir) })
                .await;
            let _ = this.update(cx, |v, cx| {
                match written {
                    Ok((folder, missing)) => {
                        v.problem = (missing > 0).then(|| {
                            tr_n!(
                                missing,
                                "Exported without an image not synced yet.",
                                "Exported without {n} images not synced yet."
                            )
                        });
                        // Shown in the file manager (GPUI's test platform has
                        // none).
                        if !cfg!(all(test, feature = "ui-tests")) {
                            cx.reveal_path(&folder);
                        }
                    }
                    Err(e) => v.problem = Some(tr!("Not exported: {e}", e = e.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Insert Image…": files chosen in the system's dialog.
    fn pick_images(&mut self, cx: &mut Context<Self>) {
        if let Some(why) = self.images_refused() {
            self.problem = Some(why);
            cx.notify();
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(tr!("Insert").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                let _ = this.update(cx, |v, cx| {
                    v.add_images(paths.into_iter().map(Source::File).collect(), cx)
                });
            }
        })
        .detach();
    }

    /// Prepares and keeps `sources` (off the UI thread), then puts a line
    /// for each at the caret.
    fn add_images(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        self.add_images_at(sources, None, cx);
    }

    /// As [`Self::add_images`], at offset `at` if given.
    fn add_images_at(&mut self, sources: Vec<Source>, at: Option<usize>, cx: &mut Context<Self>) {
        if sources.is_empty() {
            return;
        }
        if let Some(why) = self.images_refused() {
            self.problem = Some(why);
            cx.notify();
            return;
        }
        let Some(vault) = self.vault.ready() else {
            return;
        };
        let text = self.text.read(cx).text().to_string();
        cx.spawn(async move |this, cx| {
            let (ids, problems) = cx
                .background_spawn(async move { note_images::add(&vault, &text, sources) })
                .await;
            let _ = this.update(cx, |v, cx| {
                let ids: Vec<String> = ids.into_iter().map(|(_, id)| id).collect();
                if !ids.is_empty() {
                    v.text.update(cx, |e, cx| {
                        let range = match at {
                            Some(at) => {
                                let at = at.min(e.text().len());
                                at..at
                            }
                            None => e.selected_range(),
                        };
                        let lines = note_images::insertion(e.text(), range.clone(), &ids);
                        e.replace_range(range, &lines, cx);
                    });
                    v.edited(cx);
                }
                v.problem = (!problems.is_empty()).then(|| problems.join(" "));
                cx.notify();
            });
        })
        .detach();
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
        // The same as last saved: nothing to write.
        let snap = self.snapshot(cx);
        if self.last_saved == Some(snap) {
            self.saved_at = self.edits;
            return;
        }
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
                self.last_saved = Some(snap);
                self.unsynced = true;
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
                    // Gone: that goes out at once.
                    (v.hooks.release)(&id, true, cx);
                    v.note.id.clear();
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
        let at = self.host_chip.clone();
        let picker = div()
            .relative()
            .child(
                Self::chip("note-add-host", tr!("For a host ▾"), self.picking)
                    .debug_selector(|| "note-add-host".into())
                    .relative()
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.picking = !v.picking;
                        cx.notify();
                    }))
                    .child(
                        gpui::canvas(move |b, _, _| at.set(b), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                    ),
            )
            .when(self.picking, |d| {
                let choices: Vec<String> = (self.hooks.hosts)(cx)
                    .into_iter()
                    .filter(|h| !self.note.hosts.contains(h))
                    .collect();
                let mut list = div()
                    .id("note-host-list")
                    .debug_selector(|| "note-host-list".into())
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
                // Right below the chip.
                let b = self.host_chip.get();
                d.child(
                    deferred(
                        anchored()
                            .position(gpui::point(b.left(), b.bottom() + px(4.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(list),
                    )
                    .with_priority(2),
                )
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
                        .on_click(cx.listener(|v, _, w, cx| v.toggle_preview(w, cx))),
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
            .when(self.loaded && self.images_refused().is_none(), |d| {
                let web = note_images::remote_images(self.text.read(cx).text()).len();
                d.when(web > 0, |d| {
                    d.child(
                        Self::chip(
                            "note-download",
                            tr_n!(web, "Download Image", "Download {n} Images"),
                            false,
                        )
                        .on_click(cx.listener(|v, _, w, cx| v.download_images(w, cx))),
                    )
                })
            })
            .when(self.loaded, |d| {
                d.child(
                    Self::chip("note-export", tr!("Export…"), false)
                        .on_click(cx.listener(|v, _, w, cx| v.export(w, cx))),
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
                            // While Markdown is edited, room on the left for
                            // the editor's grip (22 px out from the text), to
                            // drag a block, an image's among them, elsewhere.
                            .pl(px(if md && !self.preview { 30. } else { 12. }))
                            .pr(px(12.))
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
        let images = self.images_refused().is_none();
        if self.insert_into.as_ref() == Some(&pane) && self.images_item == Some(images) {
            return;
        }
        let item = shellwing_note_editor::HostItem {
            id: "insert".into(),
            label: match &pane {
                Some(p) => tr!("Insert into {pane}", pane = p.clone()).into(),
                None => tr!("No shell to insert into").into(),
            },
            enabled: pane.is_some(),
            selection: true,
        };
        let image = shellwing_note_editor::HostItem {
            id: "image".into(),
            label: tr!("Insert Image…").into(),
            enabled: images,
            selection: false,
        };
        // A code block's Insert too, while a pane can take it.
        let code = pane.as_ref().map(|_| tr!("Insert").into());
        self.insert_into = Some(pane);
        self.images_item = Some(images);
        self.text.update(cx, |e, cx| {
            e.set_host_items(vec![item, image], cx);
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

/// For the UI tests: what a user would see, and a way in where they would
/// click.
#[cfg(all(test, feature = "ui-tests"))]
impl NoteWindow {
    pub fn editor_text(&self, cx: &App) -> String {
        self.text.read(cx).text().to_string()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn images_ready(&self) -> usize {
        self.images.borrow().ready_count()
    }

    pub fn problem(&self) -> Option<String> {
        self.problem.clone()
    }

    pub fn note_id(&self) -> String {
        self.note.id.clone()
    }

    pub fn title_text(&self, cx: &App) -> String {
        self.title.read(cx).text().to_string()
    }

    /// The title's field, clicked.
    pub fn focus_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.title.read(cx).focus_handle(cx), cx);
    }

    /// The caret at the end of the text, in the editor.
    pub fn focus_text_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let end = self.text.read(cx).text().len();
        self.text.update(cx, |e, cx| {
            e.set_cursor(end, cx);
            e.focus(window, cx);
        });
    }

    /// "Edit" (or "Preview"), clicked.
    pub fn click_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_preview(window, cx);
    }

    /// The format's chip, clicked.
    pub fn make_plain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.note.format = NoteFormat::Plain;
        self.preview = false;
        self.text.update(cx, |e, cx| {
            style_editor(e, false, cx);
            e.set_read_only(false, cx);
        });
        self.changed_meta(window, cx);
    }

    /// "Insert Image…" in the editor's menu, chosen.
    pub fn choose_insert_image(&mut self, cx: &mut Context<Self>) {
        self.pick_images(cx);
    }

    /// Saved now, as Ctrl+S does.
    pub fn save_now(&mut self, cx: &mut Context<Self>) {
        self.save(cx);
    }

    /// An image's menu item, chosen: "copy-image" or "save-image".
    pub fn image_item(&mut self, item: &str, src: &str, cx: &mut Context<Self>) {
        match item {
            "copy-image" => self.copy_image(src, cx),
            "save-image" => self.save_image(src, cx),
            _ => {}
        }
    }

    /// "Export…", clicked.
    pub fn click_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.export(window, cx);
    }

    /// "Download Images", clicked.
    pub fn click_download_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.download_images(window, cx);
    }

    /// The text, replaced (as if typed or pasted).
    pub fn set_text_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text.update(cx, |e, cx| {
            let all = 0..e.text().len();
            e.replace_range(all, text, cx);
        });
        self.edited(cx);
    }

    /// Files dropped at byte `at` of the text (where the pointer was).
    pub fn drop_at(&mut self, paths: Vec<std::path::PathBuf>, at: usize, cx: &mut Context<Self>) {
        let at = note_images::drop_offset(self.text.read(cx).text(), at);
        self.add_images_at(paths.into_iter().map(Source::File).collect(), Some(at), cx);
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
        // Its size as it would be restored (a maximized window's too).
        let b = window.window_bounds().get_bounds().size;
        self.size = Some((f32::from(b.width), f32::from(b.height)));
        self.menu_items(cx);
        let focused = |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
        let (title_on, cat_on) = (focused(&self.title), focused(&self.category));
        // Images the window let go of leave the GPU's atlas too.
        for image in self.images.borrow_mut().take_evicted() {
            let _ = window.drop_image(image);
        }
        let content = div()
            .id("note")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::paste_images))
            .on_drop(cx.listener(Self::drop_images))
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
        let bar = chrome::title_bar(shown_title);
        let content = div().size_full().flex().flex_col().child(content);
        let root = chrome::window_chrome(bar, content, rgb(colors().surface).into(), window);
        root.when_some(look.and_then(|l| l.family), |d, f| d.font_family(f))
            .children(crate::app_lock::cover(cx))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_note_window_opens_as_the_last_one_closed_but_not_smaller() {
        use super::opening_size;
        assert_eq!(opening_size(None), (800., 700.));
        assert_eq!(opening_size(Some((1000., 820.))), (1000., 820.));
        assert_eq!(opening_size(Some((420., 900.))), (600., 900.));
        assert_eq!(opening_size(Some((300., 200.))), (600., 500.));
    }

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

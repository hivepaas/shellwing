//! Notes from the palette and Settings, each in a window of its own (see
//! `note_window`).

use std::rc::Rc;

use super::*;
use crate::note_window::{self, Hooks};
use shellwing_vault::{Note, NoteFormat};

impl Workspace {
    /// What a note window tells this one: saved (sync, refresh lists), in
    /// use (the vault does not lock as idle), and the hosts to choose from.
    fn note_hooks(&self, cx: &Context<Self>) -> Hooks {
        let ws = cx.entity().downgrade();
        let (saved, touched, hosts, pane, insert) =
            (ws.clone(), ws.clone(), ws.clone(), ws.clone(), ws.clone());
        let (hold, release) = (self.sync.clone(), self.sync.clone());
        let store = self.live.store().cloned();
        Hooks {
            saved: Rc::new(move |cx| {
                // Saved here only: it syncs when its window lets it go.
                let _ = saved.update(cx, |ws, cx| {
                    if let Some((view, _)) = &ws.settings {
                        let view = view.clone();
                        cx.defer(move |cx| view.update(cx, |v, cx| v.notes_changed(cx)));
                    }
                });
            }),
            sized: Rc::new(move |width, height, _| {
                if let Some(s) = &store
                    && let Err(e) =
                        s.save_note_window(&shellwing_config::NoteWindowState { width, height })
                {
                    eprintln!("cannot keep the note window's size: {e}");
                }
            }),
            hold: Rc::new(move |id, _| hold.hold(id)),
            release: Rc::new(move |id, changed, _| release.release(id, changed)),
            touched: Rc::new(move |cx| {
                let _ = touched.update(cx, |ws, _| ws.last_input = std::time::Instant::now());
            }),
            hosts: Rc::new(move |cx| {
                hosts
                    .upgrade()
                    .map(|ws| ws.read(cx).snippet_scopes().1)
                    .unwrap_or_default()
            }),
            pane: Rc::new(move |cx| {
                let ws = pane.upgrade()?;
                let view = ws.read(cx).active_tab()?.clone();
                let s = view.read(cx).session();
                (s.status() == Status::Connected).then(|| s.title.clone())
            }),
            insert: Rc::new(move |text, cx| {
                let ws = insert.upgrade().ok_or_else(|| tr!("Not connected."))?;
                ws.update(cx, |ws, cx| ws.insert_text(&text, cx))
            }),
        }
    }

    /// Types `text` into the focused pane (no Enter: it is read first), and
    /// brings the window forward to run it.
    fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let view = self
            .active_tab()
            .cloned()
            .ok_or_else(|| tr!("Open a tab first"))?;
        if view.read(cx).session().status() != Status::Connected {
            return Err(tr!("Connect first"));
        }
        view.update(cx, |v, _| v.type_text(text, false));
        let focus = view.read(cx).focus_handle(cx);
        self.in_window(cx, move |_, window, cx| {
            window.activate_window();
            window.focus(&focus, cx);
        });
        Ok(())
    }

    /// Opens `note` (None: a new one) in its window.
    pub(crate) fn open_note(&mut self, note: Option<Note>, cx: &mut Context<Self>) {
        let hooks = self.note_hooks(cx);
        let vault = self.vault.clone();
        let last = self
            .live
            .store()
            .and_then(|s| s.note_window())
            .map(|s| (s.width, s.height));
        cx.defer(move |cx| note_window::open(note, vault, hooks, last, cx));
    }

    pub(super) fn open_note_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let note = self
            .vault
            .ready()
            .and_then(|v| v.notes().ok())
            .and_then(|l| l.into_iter().find(|n| n.id == id));
        match note {
            Some(n) => self.open_note(Some(n), cx),
            None => self.say(tr!("It is not in the vault any more."), CONFIG_NOTICE, cx),
        }
    }

    /// The notes the palette offers: those about the focused pane's host
    /// first, then the others by category; and a new one.
    pub(super) fn note_entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let Some(vault) = self.vault.ready() else {
            return vec![];
        };
        let mut notes = vault.notes().unwrap_or_default();
        notes.sort_by_key(|n| (n.category.trim().to_lowercase(), n.title.to_lowercase()));
        let host = self.active_tab().map(|v| {
            let s = v.read(cx).session();
            s.entry.clone().unwrap_or_else(|| s.spec.clone())
        });
        let entry = |n: &Note, group: String| {
            let mut detail = vec![];
            if n.format == NoteFormat::Markdown {
                detail.push("Markdown".to_string());
            }
            if !n.hosts.is_empty() {
                detail.push(n.hosts.join(", "));
            }
            if n.sensitive {
                detail.push(tr!("sensitive"));
            }
            let title = if n.title.is_empty() {
                tr!("Untitled note")
            } else {
                n.title.clone()
            };
            Entry::new(Kind::OpenNote(n.id.clone()), group, title)
                .detail(detail.join(" · "))
                .hint("Enter")
                .section(Part::Notes)
        };
        let (about, others): (Vec<&Note>, Vec<&Note>) = notes
            .iter()
            .partition(|n| host.as_ref().is_some_and(|h| n.hosts.contains(h)));
        let mut out = vec![];
        if let Some(h) = &host {
            for n in about {
                out.push(entry(n, tr!("Notes for {host}", host = h.as_str())));
            }
        }
        out.push(
            Entry::new(
                Kind::Action(Action::NewNote),
                tr!("Notes"),
                tr!("New note…"),
            )
            .hint("Enter")
            .section(Part::Notes),
        );
        for n in others {
            let category = n.category.trim();
            let group = if category.is_empty() {
                tr!("Notes")
            } else {
                tr!("Notes · {category}", category = category)
            };
            out.push(entry(n, group));
        }
        // The ones without a category under "Notes", beside "New note…".
        let (plain, filed): (Vec<Entry>, Vec<Entry>) = out
            .split_off(
                out.iter()
                    .position(|e| e.kind == Kind::Action(Action::NewNote))
                    .unwrap_or(0)
                    + 1,
            )
            .into_iter()
            .partition(|e| e.group.as_ref() == tr!("Notes"));
        out.extend(plain);
        out.extend(filed);
        out
    }
}

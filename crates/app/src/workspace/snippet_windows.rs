//! Snippets edited each in a window of its own (see `snippet_window`), and
//! run from there in the focused pane.

use std::rc::Rc;

use super::*;
use crate::snippet_window::{self, Hooks};

impl Workspace {
    /// What a snippet window tells this one: saved (lists show it again),
    /// in use (the vault does not lock as idle), where it can be offered,
    /// and running it.
    fn snippet_hooks(&self, cx: &Context<Self>) -> Hooks {
        let ws = cx.entity().downgrade();
        let (saved, touched, scopes, pane, run) =
            (ws.clone(), ws.clone(), ws.clone(), ws.clone(), ws.clone());
        let (hold, release) = (self.sync.clone(), self.sync.clone());
        let store = self.live.store().cloned();
        Hooks {
            saved: Rc::new(move |cx| {
                // Saved here only: it syncs when its window lets it go.
                let _ = saved.update(cx, |ws, cx| {
                    if let Some((view, _)) = &ws.settings {
                        let view = view.clone();
                        cx.defer(move |cx| view.update(cx, |v, cx| v.snippets_changed(cx)));
                    }
                });
            }),
            sized: Rc::new(move |width, height, _| {
                if let Some(s) = &store
                    && let Err(e) =
                        s.save_snippet_window(&shellwing_config::NoteWindowState { width, height })
                {
                    eprintln!("cannot keep the snippet window's size: {e}");
                }
            }),
            hold: Rc::new(move |id, _| hold.hold(id)),
            release: Rc::new(move |id, changed, _| release.release(id, changed)),
            touched: Rc::new(move |cx| {
                let _ = touched.update(cx, |ws, _| ws.last_input = std::time::Instant::now());
            }),
            scopes: Rc::new(move |cx| {
                scopes
                    .upgrade()
                    .map(|ws| ws.read(cx).snippet_scopes())
                    .unwrap_or_default()
            }),
            pane: Rc::new(move |cx| {
                let ws = pane.upgrade()?;
                let view = ws.read(cx).active_tab()?.clone();
                let s = view.read(cx).session();
                (s.status() == Status::Connected).then(|| s.title.clone())
            }),
            run: Rc::new(move |snippet, enter, cx| {
                let ws = run.upgrade().ok_or_else(|| tr!("Connect first"))?;
                ws.update(cx, |ws, cx| ws.run_snippet_from_window(snippet, enter, cx))
            }),
        }
    }

    /// Opens the snippet library's window.
    pub(crate) fn open_library(&mut self, cx: &mut Context<Self>) {
        let ws = cx.entity().downgrade();
        let (added, open, touched) = (ws.clone(), ws.clone(), ws);
        let hooks = crate::library_window::Hooks {
            added: Rc::new(move |cx| {
                let _ = added.update(cx, |ws, cx| {
                    ws.vault_changed();
                    if let Some((view, _)) = &ws.settings {
                        let view = view.clone();
                        cx.defer(move |cx| view.update(cx, |v, cx| v.snippets_changed(cx)));
                    }
                });
            }),
            open: Rc::new(move |snippet, cx| {
                let _ = open.update(cx, |ws, cx| ws.open_snippet(Some(snippet), cx));
            }),
            touched: Rc::new(move |cx| {
                let _ = touched.update(cx, |ws, _| ws.last_input = std::time::Instant::now());
            }),
        };
        let vault = self.vault.clone();
        cx.defer(move |cx| crate::library_window::open(vault, hooks, cx));
    }

    /// Opens `snippet` (None: a new one) in its window.
    pub(crate) fn open_snippet(
        &mut self,
        snippet: Option<crate::snippets::Snippet>,
        cx: &mut Context<Self>,
    ) {
        let hooks = self.snippet_hooks(cx);
        let vault = self.vault.clone();
        let last = self
            .live
            .store()
            .and_then(|s| s.snippet_window())
            .map(|s| (s.width, s.height));
        cx.defer(move |cx| snippet_window::open(snippet, vault, hooks, last, cx));
    }

    /// Runs `snippet` as its window has it (Enter after it, or not) in the
    /// focused pane: its values asked for in the palette here, as when run
    /// from there. This window comes forward.
    fn run_snippet_from_window(
        &mut self,
        mut snippet: crate::snippets::Snippet,
        enter: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(why) = self.pane_unready(false, cx) {
            return Err(why);
        }
        if snippet.command.len() > shellwing_vault::limits::COMMAND {
            let o = shellwing_vault::Oversize::Command(snippet.command.len());
            return Err(crate::environment::said_oversize(&o));
        }
        snippet.enter = enter;
        let left = snippets::asked(&snippet.command);
        let asks = !left.is_empty();
        self.in_window(cx, move |ws, window, cx| {
            window.activate_window();
            ws.ask_snippet(
                SnippetRun {
                    snippet,
                    all: false,
                    values: Default::default(),
                    left,
                },
                window,
                cx,
            );
            // Typed: the pane has the keys, to go on from there.
            if !asks {
                ws.activate(ws.active, window, cx);
            }
        });
        Ok(())
    }
}

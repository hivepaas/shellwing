//! Run on hosts tabs: opened from the host list's Actions, a group's ▶, the
//! hosts picked with Ctrl+click, or the palette.

use super::*;

impl Workspace {
    /// The host list's sections as a Run tab chooses from them.
    pub(super) fn run_picks(&self, cx: &Context<Self>) -> Vec<Pick> {
        super::ai::picks_of(&self.sidebar_sections(cx))
    }

    /// Opens a Run on hosts tab with `chosen` (specs) chosen.
    pub fn open_run(&mut self, chosen: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let picks = self.run_picks(cx);
        let fleet = shellwing_core::run::Fleet::new(
            self.rt.clone(),
            shellwing_core::run::secrets(self.vault.clone(), self.live.settings()),
            &self.live.settings().ssh,
        );
        let ws = cx.entity().downgrade();
        let resolve: crate::run_view::Resolve = Rc::new(move |spec, cx| {
            let ws = ws.upgrade().ok_or_else(|| tr!("The window closed."))?;
            ws.read(cx).resolve_for_run(spec)
        });
        let vault = self.vault.clone();
        let snippets: crate::run_view::Snippets = Rc::new(move |_| {
            let mut list = vault
                .ready()
                .and_then(|v| v.snippets().ok())
                .unwrap_or_default();
            list.sort_by_key(|sn| (sn.category.trim().to_lowercase(), sn.name.to_lowercase()));
            list.into_iter().map(|sn| (sn.name, sn.command)).collect()
        });
        let run = cx.new(|cx| RunView::new(picks, chosen, fleet, resolve, snippets, cx));
        cx.subscribe_in(&run, window, |ws, _, ev: &RunEvent, window, cx| match ev {
            RunEvent::Open(spec) => ws.open(spec, window, cx),
        })
        .detach();
        // Its title and progress show on its tab.
        cx.observe(&run, |_, _, cx| cx.notify()).detach();
        self.tabs.push(Tab {
            root: None,
            focused: None,
            run: Some(run),
            broadcast: false,
            left_out: vec![],
        });
        self.activate(self.tabs.len() - 1, window, cx);
        self.sidebar.update(cx, |s, cx| s.clear_picked(cx));
    }

    /// The active tab's Run view, if it is a Run on hosts tab.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn active_run(&self) -> Option<Entity<RunView>> {
        self.tabs.get(self.active).and_then(|t| t.run.clone())
    }

    /// The tabs `state/session.json` would keep, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn saved_tab_count(&self, cx: &gpui::App) -> usize {
        self.snapshot(cx).tabs.len()
    }

    /// The hosts picked with Ctrl+click, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn picked_hosts(&self, cx: &gpui::App) -> Vec<String> {
        self.sidebar.read(cx).picked()
    }

    /// Where a Run tab connects to `spec`, or why it is left out: a
    /// collection's host whose settings are held back is not run on.
    pub(super) fn resolve_for_run(&self, spec: &str) -> Result<ssh_config::Resolved, String> {
        if let Some((_, held)) = self.held_back(spec) {
            let names: Vec<String> = held.into_iter().map(|u| u.name).collect();
            return Err(tr!(
                "its collection's settings for {names} wait until you allow them, on a tab to it",
                names = names.join(", ")
            ));
        }
        target::resolve(
            spec,
            self.live.hosts(),
            ssh_config::default_config_path().as_deref(),
        )
        .map(|(r, _)| r)
        .map_err(|e| e.to_string())
    }

    /// The host list changed: the Run tabs choose from it as it is now.
    pub(super) fn refresh_runs(&mut self, picks: &[Pick], cx: &mut Context<Self>) {
        let runs: Vec<Entity<RunView>> = self.tabs.iter().filter_map(|t| t.run.clone()).collect();
        if runs.is_empty() {
            return;
        }
        for r in runs {
            r.update(cx, |r, cx| r.set_picks(picks.to_vec(), cx));
        }
    }

    /// Closing a Run tab that is signing in or running stops it: asked
    /// first.
    pub(super) fn confirm_close_run(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prompting {
            return;
        }
        let Some(key) = self.tabs.get(ix).and_then(Tab::key) else {
            return;
        };
        self.prompting = true;
        let (close_label, cancel_label) = (tr!("Stop and close"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("Close the tab?"),
            Some(&tr!(
                "It is still running: closing stops it on the hosts it has not ended on."
            )),
            &[close_label.as_str(), cancel_label.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let yes = matches!(answer.await, Ok(0));
            let _ = this.update_in(cx, |ws, window, cx| {
                ws.prompting = false;
                if yes && let Some(ix) = ws.tabs.iter().position(|t| t.key() == Some(key)) {
                    ws.close(ix, window, cx);
                }
            });
        })
        .detach();
    }
}

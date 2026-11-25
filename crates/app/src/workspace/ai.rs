//! AI access: the hub the MCP server works through, the clients and hosts
//! given them (state/ai.json), and the socket AI clients reach the app at.

use std::collections::{BTreeMap, BTreeSet};

/// How long the notice that a command waits for the person shows.
const AI_NOTICE: Duration = Duration::from_secs(20);

use super::*;
use shellwing_config::AiState;
use shellwing_core::ai::{self, Hub, HubEvent};

/// The app's side of AI access.
pub(super) struct AiWork {
    pub hub: Arc<Hub>,
    pub state: AiState,
    pub connected: BTreeSet<String>,
    /// What AI clients ran, oldest first, as the hub says it.
    pub activity: Vec<Arc<ai::Activity>>,
    /// Commands for hosts given through a tab, not typed yet: no tab to
    /// the host is free.
    queued: Vec<TabJob>,
    /// The panes an AI's command is typed in now: the job's id.
    typing: BTreeMap<gpui::EntityId, String>,
    /// Panes where the person paused AI.
    pub paused: BTreeSet<gpui::EntityId>,
    /// Panes AI typed in, and whose: what their strip says.
    pub used: BTreeMap<gpui::EntityId, (String, usize)>,
    /// What the hub was last told about hosts: the host list's, and the
    /// grants', so it is told again only when either changes.
    told: Option<(Vec<Pick>, AiState)>,
}

impl Workspace {
    /// Starts AI access: the saved clients and grants (those for the last
    /// session dropped), and the socket, on the network runtime.
    pub(super) fn start_ai(&mut self, cx: &mut Context<Self>) {
        let store = self.live.store();
        let mut state = store.map(|s| s.ai_state()).unwrap_or_default();
        let before = state.grants.len();
        state.grants.retain(|g| !g.session);
        if state.grants.len() != before
            && let Some(s) = store
        {
            let _ = s.save_ai_state(&state);
        }
        let (events, inbox) = async_channel::unbounded();
        let hub = Hub::new(
            self.rt.clone(),
            shellwing_core::run::secrets(self.vault.clone(), self.live.settings()),
            &self.live.settings().ssh,
            store.map(|s| s.dir().join("state").join("ai-activity.jsonl")),
            events,
        );
        hub.set_state(state.clone());
        let vault = Arc::new(ai::notes::VaultNotes {
            vault: self.vault.clone(),
            sync: Some(self.sync.clone()),
        });
        hub.set_notes(Some(vault.clone()));
        hub.set_snippets(Some(vault));
        hub.set_locked(
            cx.try_global::<crate::app_lock::Locked>()
                .is_some_and(|l| l.0),
        );
        let activity = hub.activity().into_iter().map(Arc::new).collect();
        if let Some(dir) = store.map(|s| s.dir().to_path_buf()) {
            let at = ai::socket::Endpoint::for_config(&dir);
            let listening = hub.clone();
            self.rt.spawn(async move {
                if let Err(e) = ai::socket::listen(listening, at).await {
                    eprintln!("AI clients cannot reach shellwing: {e}");
                }
            });
        }
        cx.spawn(async move |ws, cx| {
            while let Ok(ev) = inbox.recv().await {
                if ws.update(cx, |ws, cx| ws.on_ai(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.ai = Some(AiWork {
            hub,
            state,
            connected: BTreeSet::new(),
            activity,
            queued: vec![],
            typing: BTreeMap::new(),
            paused: BTreeSet::new(),
            used: BTreeMap::new(),
            told: None,
        });
    }

    fn on_ai(&mut self, ev: HubEvent, cx: &mut Context<Self>) {
        if let HubEvent::Show { args } = ev {
            self.show_again(args, cx);
            return;
        }
        let Some(ai) = &mut self.ai else {
            return;
        };
        match ev {
            HubEvent::Client { id, connected } => {
                if connected {
                    ai.connected.insert(id);
                } else {
                    ai.connected.remove(&id);
                }
                cx.notify();
            }
            HubEvent::Tab {
                id,
                spec,
                command,
                client,
                needs,
            } => {
                ai.queued.push(TabJob {
                    id,
                    spec,
                    command,
                    client,
                    needs,
                    opened: false,
                });
                self.type_next(cx);
            }
            HubEvent::TabStop { id } => {
                let view = ai.typing.iter().find(|(_, j)| **j == id).map(|(v, _)| *v);
                ai.queued.retain(|j| j.id != id);
                if let Some(v) =
                    view.and_then(|v| self.all_views().into_iter().find(|x| x.entity_id() == v))
                {
                    // Ctrl+C, as the person would press it.
                    v.read(cx).session().writer.write(vec![0x03]);
                }
            }
            HubEvent::Cleared => {
                ai.activity.retain(|a| a.end.is_none());
                self.refresh_sidebar(cx);
                cx.notify();
            }
            HubEvent::SnippetsChanged => {
                if let Some((view, _)) = &self.settings {
                    let view = view.clone();
                    cx.defer(move |cx| view.update(cx, |v, cx| v.snippets_changed(cx)));
                }
            }
            // Taken before the AI's own.
            HubEvent::Show { .. } => {}
            HubEvent::NotesChanged => {
                if let Some((view, _)) = &self.settings {
                    let view = view.clone();
                    cx.defer(move |cx| view.update(cx, |v, cx| v.notes_changed(cx)));
                }
            }
            HubEvent::Changed(a) => {
                let asks = a.waiting.is_some();
                let new_ask = asks
                    && !ai
                        .activity
                        .iter()
                        .any(|x| x.id == a.id && x.waiting.is_some());
                let key = a
                    .waiting
                    .as_deref()
                    .is_some_and(|w| w.starts_with(ai::HOST_KEY));
                let note = if key {
                    tr!(
                        "{client} wants to reach {host}, whose host key is not in known_hosts: AI activity has it, waiting for you.",
                        client = a.client.clone(),
                        host = a.host.clone()
                    )
                } else {
                    tr!(
                        "{client} wants to run a command on {host}: AI activity has it, waiting for you.",
                        client = a.client.clone(),
                        host = a.host.clone()
                    )
                };
                match ai.activity.iter().position(|x| x.id == a.id) {
                    Some(i) => ai.activity[i] = Arc::new(a),
                    None => ai.activity.push(Arc::new(a)),
                }
                let over = ai.activity.len().saturating_sub(ai::ACTIVITY);
                ai.activity.drain(..over);
                if new_ask {
                    self.say(Notice::warn(note), AI_NOTICE, cx);
                }
                self.refresh_sidebar(cx);
                cx.notify();
            }
        }
    }

    /// A second shellwing was started on this config: this window comes
    /// forward, and opens the host it was given, as at a start (not while
    /// locked: the lock comes forward alone).
    fn show_again(&mut self, args: Vec<String>, cx: &mut Context<Self>) {
        let (spec, forwards) = crate::parse_args(args.into_iter()).unwrap_or_default();
        self.in_window(cx, move |ws, window, cx| {
            window.activate_window();
            cx.activate(true);
            if let Some(spec) = spec
                && !crate::app_lock::is_locked(cx)
            {
                ws.open_forwarding(&spec, &forwards, window, cx);
            }
        });
    }

    /// What AI clients ran, for the activity tab.
    pub fn ai_activity(&self) -> Vec<Arc<ai::Activity>> {
        self.ai
            .as_ref()
            .map(|a| a.activity.clone())
            .unwrap_or_default()
    }

    /// Commands waiting for the person.
    pub(super) fn ai_waiting(&self) -> usize {
        self.ai.as_ref().map_or(0, |a| {
            a.activity.iter().filter(|x| x.waiting.is_some()).count()
        })
    }

    /// The person's answer to a command that waits.
    pub fn ai_answer(&self, id: &str, approval: ai::Approval) {
        if let Some(a) = &self.ai {
            a.hub.answer(id, approval);
        }
    }

    /// How much the activity's log keeps on this computer, and the most it
    /// may (bytes).
    pub fn ai_kept(&self) -> Option<(u64, u64)> {
        self.ai.as_ref().and_then(|a| a.hub.kept())
    }

    /// Where the activity's log is written now.
    pub fn ai_log_path(&self) -> Option<std::path::PathBuf> {
        self.ai.as_ref().and_then(|a| a.hub.log_path())
    }

    /// Clears the activity: what ended goes, on this computer too.
    pub fn ai_clear(&self) {
        if let Some(a) = &self.ai {
            a.hub.clear_activity();
        }
    }

    /// Stops one AI command, or (None) every one going.
    pub fn ai_stop(&self, id: Option<&str>) {
        if let Some(a) = &self.ai {
            match id {
                Some(id) => a.hub.stop(id),
                None => a.hub.stop_all(),
            }
        }
    }

    /// Opens the AI activity tab, or goes to it: there is one.
    pub fn open_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(t.page, Some(TabPage::Activity(_))))
        {
            return self.activate(i, window, cx);
        }
        let ws = cx.entity().downgrade();
        let view = cx.new(|cx| crate::activity_view::ActivityView::new(ws, cx));
        // Its count of commands waiting shows on its tab.
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        self.tabs.push(Tab::page(TabPage::Activity(view)));
        self.activate(self.tabs.len() - 1, window, cx);
    }

    /// The output AI activity shows now, as its terminal has it: for the UI
    /// tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn activity_text(&self, cx: &gpui::App) -> String {
        self.tabs
            .iter()
            .find_map(|t| match &t.page {
                Some(TabPage::Activity(v)) => Some(v.read(cx).shown_text(cx)),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The look of a tab to the host the host list names `host`: what AI
    /// activity draws a command's output in.
    pub fn look_of_host(&self, host: &str) -> crate::look::Look {
        self.look_of(Some(host))
    }

    /// The look of a tab to `host` (its name in the host list), or (None) the
    /// usual one.
    pub fn look_of(&self, host: Option<&str>) -> crate::look::Look {
        self.live.look_for(host).0
    }

    /// What Settings › AI shows.
    pub fn ai_snapshot(&self, _: &gpui::App) -> crate::ai_view::AiSnap {
        let (state, connected) = match &self.ai {
            Some(a) => (a.state.clone(), a.connected.clone()),
            None => Default::default(),
        };
        crate::ai_view::AiSnap {
            state,
            connected,
            picks: self
                .ai
                .as_ref()
                .and_then(|a| a.told.as_ref())
                .map(|(p, _)| p.clone())
                .unwrap_or_default(),
            exe: std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "shellwing".into()),
        }
    }

    /// Changes the clients or grants: saved, and the hub told.
    pub fn change_ai(&mut self, f: impl FnOnce(&mut AiState), cx: &mut Context<Self>) {
        let Some(ai) = &mut self.ai else {
            return;
        };
        f(&mut ai.state);
        if let Some(store) = self.live.store()
            && let Err(e) = store.save_ai_state(&ai.state)
        {
            eprintln!("cannot save AI access: {e}");
        }
        ai.hub.set_state(ai.state.clone());
        self.refresh_sidebar(cx);
    }

    /// Tells the hub where the hosts given are, when they or the grants
    /// changed.
    pub(super) fn refresh_ai(&mut self, picks: &[Pick]) {
        let Some(ai) = &self.ai else {
            return;
        };
        if ai
            .told
            .as_ref()
            .is_some_and(|(p, s)| p == picks && *s == ai.state)
        {
            return;
        }
        let given: BTreeSet<&str> = ai.state.grants.iter().map(|g| g.host.as_str()).collect();
        let hosts: BTreeMap<String, ai::Host> = picks
            .iter()
            .flat_map(|p| &p.hosts)
            .filter(|h| given.contains(h.spec.as_str()))
            .map(|h| {
                (
                    h.spec.clone(),
                    ai::Host {
                        name: h.name.clone(),
                        detail: h.detail.clone(),
                        group: self
                            .live
                            .hosts()
                            .find(&h.name)
                            .and_then(|x| x.group.clone()),
                        target: self.resolve_for_run(&h.spec),
                    },
                )
            })
            .collect();
        let told = (picks.to_vec(), ai.state.clone());
        ai.hub.set_hosts(hosts);
        if let Some(ai) = &mut self.ai {
            ai.told = Some(told);
        }
    }

    /// The hosts an AI client may use now, by spec: their tiles say so. One
    /// whose every grant is paused, or its client, is not.
    pub(super) fn ai_given(&self) -> BTreeSet<String> {
        let mut given = BTreeSet::new();
        if let Some(a) = &self.ai {
            for g in a
                .state
                .grants
                .iter()
                .filter(|g| g.until.is_none_or(|u| ai::now() < u))
            {
                let client_paused = a.state.clients.iter().any(|c| c.id == g.client && c.paused);
                if !g.paused && !client_paused {
                    given.insert(g.host.clone());
                }
            }
        }
        given
    }

    /// Opens Settings › AI to give AI clients `hosts` (specs): the hosts
    /// picked in the host list.
    pub fn open_give(&mut self, hosts: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(crate::settings_view::Page::Ai, window, cx);
        self.sidebar.update(cx, |s, cx| s.clear_picked(cx));
        let Some((view, _)) = &self.settings else {
            return;
        };
        let view = view.clone();
        // Once this update is done: the page reads the workspace.
        window.defer(cx, move |window, cx| {
            view.update(cx, |v, cx| v.give_hosts(hosts, window, cx));
        });
    }
}

/// A command for a host given through a tab.
pub(super) struct TabJob {
    id: String,
    spec: String,
    command: String,
    client: String,
    /// The environments it runs with, in a subshell: their values sent as
    /// a pane sends them.
    needs: Vec<String>,
    /// A tab was opened for it.
    opened: bool,
}

impl Workspace {
    /// Types the commands queued for tabs into the tabs free for them: a
    /// pane to the host, connected, with nothing of AI's running nor paused.
    /// A host with no tab gets one; a tab that will not connect ends its
    /// commands.
    pub(super) fn type_next(&mut self, cx: &mut Context<Self>) {
        let Some(ai) = &self.ai else {
            return;
        };
        if ai.queued.is_empty() {
            return;
        }
        let views = self.all_views();
        let mut start = vec![];
        let mut open = vec![];
        let mut failed = vec![];
        let mut busy: BTreeSet<gpui::EntityId> = ai.typing.keys().copied().collect();
        busy.extend(ai.paused.iter().copied());
        for (i, job) in ai.queued.iter().enumerate() {
            let to_host: Vec<&Entity<TermView>> = views
                .iter()
                .filter(|v| v.read(cx).session().spec == job.spec)
                .collect();
            let up = |v: &&Entity<TermView>| v.read(cx).session().status() == Status::Connected;
            // A pane AI used before, else any.
            let free = to_host
                .iter()
                .filter(|v| up(v) && !busy.contains(&v.entity_id()))
                .max_by_key(|v| ai.used.contains_key(&v.entity_id()));
            if let Some(v) = free {
                busy.insert(v.entity_id());
                start.push((i, (*v).clone()));
            } else if to_host.is_empty() && !job.opened {
                open.push(i);
            } else if job.opened
                && !to_host.is_empty()
                && to_host.iter().all(|v| {
                    matches!(
                        v.read(cx).session().status(),
                        Status::Failed | Status::Closed
                    )
                })
            {
                failed.push(i);
            }
        }
        let hub = ai.hub.clone();
        let Some(ai) = &mut self.ai else {
            return;
        };
        for i in &open {
            ai.queued[*i].opened = true;
        }
        let specs: Vec<String> = open.iter().map(|i| ai.queued[*i].spec.clone()).collect();
        let mut gone = vec![];
        for i in failed {
            let job = &ai.queued[i];
            hub.tab_end(&job.id, ai::End::Failed("its tab did not connect".into()));
            gone.push(i);
        }
        let mut jobs = vec![];
        for (i, view) in start {
            gone.push(i);
            let j = &ai.queued[i];
            jobs.push((
                view,
                TabJob {
                    id: j.id.clone(),
                    spec: j.spec.clone(),
                    command: j.command.clone(),
                    client: j.client.clone(),
                    needs: j.needs.clone(),
                    opened: j.opened,
                },
            ));
        }
        gone.sort_unstable();
        for i in gone.into_iter().rev() {
            ai.queued.remove(i);
        }
        for (view, job) in jobs {
            self.type_in(view, job, cx);
        }
        for spec in specs {
            self.in_window(cx, move |ws, window, cx| ws.open(&spec, window, cx));
        }
    }

    /// Types `job` into `view`, and hands the hub what comes back between
    /// its marks.
    fn type_in(&mut self, view: Entity<TermView>, job: TabJob, cx: &mut Context<Self>) {
        let Some(ai) = &mut self.ai else {
            return;
        };
        let id = view.entity_id();
        ai.typing.insert(id, job.id.clone());
        let n = ai.used.get(&id).map_or(0, |(_, n)| *n) + 1;
        ai.used.insert(id, (job.client.clone(), n));
        let hub = ai.hub.clone();
        let session = view.read(cx).session();
        let output = session.tap();
        if job.needs.is_empty() {
            session
                .writer
                .write(ai::tab::wrap(&job.id, &job.command).into_bytes());
        } else {
            // In a subshell that reads the values with the echo off, as a
            // pane runs a snippet that needs them.
            let marked = ai::tab::marked(&job.id, &job.command);
            let typed = self.env_values(&job.needs).and_then(|values| {
                view.update(cx, |v, cx| v.run_with_env(values, &marked, true, cx))
            });
            if let Err(why) = typed {
                hub.tab_end(&job.id, ai::End::Failed(why));
                return;
            }
        }
        let job_id = job.id.clone();
        cx.spawn(async move |ws, cx| {
            let mut catch = ai::tab::Catch::new(&job_id);
            // Whole lines only: a line end split across reads stays whole.
            let mut pending: Vec<u8> = vec![];
            let mut ended = None;
            while let Ok(bytes) = output.recv().await {
                let got = catch.feed(&bytes);
                pending.extend_from_slice(&got.output);
                if let Some(at) = pending.iter().rposition(|b| *b == b'\n') {
                    let lines: Vec<u8> = pending.drain(..=at).collect();
                    hub.tab_print(&job_id, &ai::tab::plain(&lines));
                }
                if got.ended.is_some() {
                    ended = got.ended;
                    break;
                }
            }
            if !pending.is_empty() {
                hub.tab_print(&job_id, &ai::tab::plain(&pending));
            }
            hub.tab_end(
                &job_id,
                match ended {
                    Some(code) => ai::End::Exited(code),
                    None => ai::End::Failed("its tab closed".into()),
                },
            );
            let _ = ws.update(cx, |ws, cx| {
                if let Some(v) = ws.all_views().into_iter().find(|v| v.entity_id() == id) {
                    v.read(cx).session().untap();
                }
                if let Some(ai) = &mut ws.ai {
                    ai.typing.remove(&id);
                }
                ws.type_next(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The strip over a pane AI typed in: whose, how many, and the person's
    /// say.
    pub(super) fn ai_strip(&self, view: &Entity<TermView>, cx: &mut Context<Self>) -> Option<Div> {
        let ai = self.ai.as_ref()?;
        let id = view.entity_id();
        let (client, n) = ai.used.get(&id)?;
        let spec = view.read(cx).session().spec.clone();
        // Only while the grant holds.
        let given = ai.state.grants.iter().any(|g| {
            g.host == spec
                && g.how == shellwing_config::AiHow::Tab
                && g.until.is_none_or(|u| ai::now() < u)
        });
        if !given {
            return None;
        }
        let paused = ai.paused.contains(&id);
        let text = if paused {
            tr!(
                "Paused: {client} waits until you let it go on.",
                client = client.clone()
            )
        } else {
            tr_n!(
                *n,
                "{client} is using this tab: {n} command so far.",
                "{client} is using this tab: {n} commands so far.",
                client = client.clone()
            )
        };
        let button = |sel: &'static str, label: String| {
            div()
                .id(sel)
                .debug_selector(move || sel.into())
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(crate::sidebar::AI_MARK))
                .bg(rgb(colors().surface))
                .text_xs()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)))
                .child(label)
        };
        let (client_name, spec2) = (client.clone(), spec.clone());
        Some(
            div()
                .debug_selector(|| "ai-strip".into())
                .h(px(34.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .bg(rgb(colors().info_bg))
                .border_b_1()
                .border_color(rgb(colors().info_edge))
                .text_sm()
                .text_color(rgb(colors().info_text))
                .child(
                    div()
                        .px(px(6.))
                        .rounded_full()
                        .bg(rgb(crate::sidebar::AI_MARK))
                        .text_color(rgb(colors().on_accent))
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("AI"),
                )
                .child(div().flex_1().min_w_0().child(text))
                .child(if paused {
                    button("ai-resume", tr!("Let it go on")).on_click(cx.listener(
                        move |ws, _, _, cx| {
                            if let Some(ai) = &mut ws.ai {
                                ai.paused.remove(&id);
                            }
                            ws.type_next(cx);
                            cx.notify();
                        },
                    ))
                } else {
                    button("ai-pause", tr!("Pause")).on_click(cx.listener(move |ws, _, _, cx| {
                        if let Some(ai) = &mut ws.ai {
                            ai.paused.insert(id);
                        }
                        cx.notify();
                    }))
                })
                .child(
                    button("ai-take-tab", tr!("Take access back")).on_click(cx.listener(
                        move |ws, _, _, cx| {
                            let (client, spec) = (client_name.clone(), spec2.clone());
                            ws.change_ai(
                                |s| {
                                    let ids: Vec<String> = s
                                        .clients
                                        .iter()
                                        .filter(|c| c.name == client)
                                        .map(|c| c.id.clone())
                                        .collect();
                                    s.grants
                                        .retain(|g| !(g.host == spec && ids.contains(&g.client)));
                                },
                                cx,
                            );
                        },
                    )),
                ),
        )
    }
}

/// The host list's sections as runs and AI access choose from them: hosts
/// only, not this computer's terminal.
pub(super) fn picks_of(sections: &[Section]) -> Vec<Pick> {
    sections
        .iter()
        .map(|s| Pick {
            group: s.title.to_string(),
            hosts: s
                .rows
                .iter()
                .filter(|r| !matches!(r.edit, EditTarget::Local))
                .map(|r| Candidate {
                    name: r.name.to_string(),
                    spec: r.spec.clone(),
                    detail: r.detail.to_string(),
                    icon: r.icon,
                    color: r.color,
                })
                .collect(),
        })
        .filter(|p| !p.hosts.is_empty())
        .collect()
}

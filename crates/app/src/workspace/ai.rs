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
            told: None,
        });
    }

    fn on_ai(&mut self, ev: HubEvent, cx: &mut Context<Self>) {
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
            HubEvent::Changed(a) => {
                let asks = a.waiting.is_some();
                let new_ask = asks
                    && !ai
                        .activity
                        .iter()
                        .any(|x| x.id == a.id && x.waiting.is_some());
                let note = tr!(
                    "{client} wants to run a command on {host}: AI activity has it, waiting for you.",
                    client = a.client.clone(),
                    host = a.host.clone()
                );
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

    /// The hosts given to an AI client now, by spec: their tiles say so.
    pub(super) fn ai_given(&self) -> BTreeSet<String> {
        self.ai
            .as_ref()
            .map(|a| {
                a.state
                    .grants
                    .iter()
                    .filter(|g| g.until.is_none_or(|u| ai::now() < u))
                    .map(|g| g.host.clone())
                    .collect()
            })
            .unwrap_or_default()
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

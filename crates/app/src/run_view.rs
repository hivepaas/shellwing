//! A Run on hosts tab: the hosts, a command or a script, and under them
//! what each host said (docs/configuration.md, Run on hosts). The work is
//! shellwing_core::run's; this is the tab around it.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Task, Window, div, prelude::FluentBuilder, px, rgb,
};
use shellwing_core::run::{self, Event, Fleet, Interpreter, Job, Outcome, Said};
use shellwing_i18n::{tr, tr_n};
use shellwing_ssh::Prompt;
use shellwing_ssh::config::Resolved;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::colors::colors;
use crate::host_editor::{
    checkbox, hint_text, label_text, outline_button, primary_button, problem_text,
};
use crate::host_icons::HostIcon;
use crate::input::{Changed, TextInput};
use crate::look::ui;
use crate::term_viewer::TermViewer;

/// A host the tab can run on.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub name: String,
    /// What connecting to it resolves.
    pub spec: String,
    pub detail: String,
    pub icon: Option<&'static HostIcon>,
    pub color: Option<u32>,
}

/// The hosts of one section of the host list.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub group: String,
    pub hosts: Vec<Candidate>,
}

/// What the tab asks of the workspace.
pub enum RunEvent {
    /// Open a tab to this host.
    Open(String),
}

/// Where a host is, to connect to it, or why it is left out.
pub type Resolve = Rc<dyn Fn(&str, &App) -> Result<Resolved, String>>;
/// The snippets to start from: their names and commands.
pub type Snippets = Rc<dyn Fn(&App) -> Vec<(String, String)>>;
/// The look of a tab to a host (by its name in the host list), or the
/// usual one (None): what a host's output is drawn in.
pub type Looks = Rc<dyn Fn(Option<&str>, &App) -> Option<crate::look::Look>>;

/// The most rows an output shows before it scrolls within itself.
const OUTPUT_ROWS: usize = 16;

/// At most this many hosts at once, unless the tab says otherwise.
const AT_ONCE: usize = 8;
/// Seconds a host may run before it is given up on, likewise.
const GIVE_UP: u64 = 60;
/// What a field of the script shows at the most before it scrolls.
const SCRIPT_ROWS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Choosing and typing.
    Edit,
    /// Signing in, and saying what will run where.
    Confirm,
    Running,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Connecting,
    Asking,
    Ready,
    NotConnected(String),
    LeftOut(String),
    Waiting,
    Running(Instant),
    Ended(Outcome),
    Stopped,
}

/// A question one or more hosts ask, answered once for all of them.
struct Ask {
    hosts: Vec<String>,
    prompt: Prompt,
    replies: Vec<oneshot::Sender<Option<String>>>,
    input: Entity<TextInput>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Interpreter,
    Snippets,
}

pub struct RunView {
    focus: FocusHandle,
    picks: Vec<Pick>,
    /// The hosts chosen, by spec, in the host list's order.
    chosen: Vec<String>,
    picker: bool,
    filter: Entity<TextInput>,
    script: Entity<TextInput>,
    /// The interpreter chosen here; None: the script's `#!`, else sh.
    interpreter: Option<Interpreter>,
    menu: Option<Menu>,
    at_once: Entity<TextInput>,
    give_up: Entity<TextInput>,
    one_first: bool,
    stage: Stage,
    sudo: Entity<TextInput>,
    sudo_each: bool,
    sudo_hosts: BTreeMap<String, Entity<TextInput>>,
    asks: Vec<Ask>,
    /// The hosts of what is going on, in order, and how each is.
    order: Vec<String>,
    states: BTreeMap<String, State>,
    /// One host first: the job the others wait for, and who they are.
    rest: Vec<(Job, Vec<String>)>,
    /// What last ran: its script, its interpreter, when.
    ran: Option<(String, Interpreter, String, Instant)>,
    by_host: bool,
    open_row: Option<String>,
    problem: Option<String>,
    /// The terminal's font, for the script and what hosts print.
    mono: SharedString,
    fleet: Fleet,
    resolve: Resolve,
    snippets: Snippets,
    looks: Looks,
    /// The outputs shown, each drawn as a terminal, by what they show.
    viewers: BTreeMap<String, Entity<TermViewer>>,
    events: async_channel::Sender<Event>,
    _pump: Task<()>,
}

impl EventEmitter<RunEvent> for RunView {}

impl Focusable for RunView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A host's spec as the name the host list shows.
fn name_in(picks: &[Pick], spec: &str) -> String {
    picks
        .iter()
        .flat_map(|p| &p.hosts)
        .find(|h| h.spec == spec)
        .map_or_else(|| spec.to_string(), |h| h.name.clone())
}

impl RunView {
    pub fn new(
        picks: Vec<Pick>,
        chosen: Vec<String>,
        fleet: Fleet,
        resolve: Resolve,
        snippets: Snippets,
        looks: Looks,
        cx: &mut Context<Self>,
    ) -> RunView {
        let script = cx.new(|cx| {
            TextInput::said(|| "systemctl is-active nginx".to_string(), cx)
                .code(crate::shell_syntax::paint)
        });
        let number = |n: String, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut i = TextInput::new("", cx);
                i.set_text(&n, 0..0, cx);
                i
            })
        };
        let at_once = number(AT_ONCE.to_string(), cx);
        let give_up = number(GIVE_UP.to_string(), cx);
        let filter = cx.new(|cx| TextInput::said(|| tr!("Find hosts"), cx));
        let sudo = cx.new(|cx| TextInput::said(|| tr!("sudo password"), cx).masked());
        for i in [&script, &filter] {
            cx.subscribe(i, |_, _, _: &Changed, cx| cx.notify())
                .detach();
        }
        let (events, inbox) = async_channel::unbounded();
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(ev) = inbox.recv().await {
                if this.update(cx, |v, cx| v.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        });
        let mut v = RunView {
            focus: cx.focus_handle(),
            picks,
            chosen: vec![],
            picker: chosen.is_empty(),
            filter,
            script,
            interpreter: None,
            menu: None,
            at_once,
            give_up,
            one_first: false,
            stage: Stage::Edit,
            sudo,
            sudo_each: false,
            sudo_hosts: BTreeMap::new(),
            asks: vec![],
            order: vec![],
            states: BTreeMap::new(),
            rest: vec![],
            ran: None,
            by_host: false,
            open_row: None,
            problem: None,
            mono: SharedString::default(),
            fleet,
            resolve,
            snippets,
            looks,
            viewers: BTreeMap::new(),
            events,
            _pump: pump,
        };
        v.set_chosen(chosen);
        v
    }

    /// The outputs on the page now: their key, the host whose look they take
    /// (None: the usual one, for a group of hosts), the output and the
    /// errors.
    fn outputs(&self) -> Vec<(String, Option<String>, String, String)> {
        if !matches!(self.stage, Stage::Running | Stage::Done) {
            return vec![];
        }
        if self.by_host {
            let Some(h) = &self.open_row else {
                return vec![];
            };
            let state = self.states.get(h).cloned().unwrap_or(State::Waiting);
            let (text, errors) = host_output(&state);
            return vec![(
                format!("host:{h}"),
                Some(name_in(&self.picks, h)),
                text,
                errors,
            )];
        }
        self.ended()
            .iter()
            .map(|(said, _)| {
                let (_, text, errors) = said_text(said);
                (output_key(&text, &errors), None, text, errors)
            })
            .filter(|(_, _, text, errors)| !text.is_empty() || !errors.is_empty())
            .collect()
    }

    /// A terminal for each output on the page, showing it; those no longer
    /// on it let go.
    fn refresh_viewers(&mut self, cx: &mut Context<Self>) {
        let outputs = self.outputs();
        self.viewers
            .retain(|k, _| outputs.iter().any(|(key, ..)| key == k));
        for (key, host, text, errors) in outputs {
            let Some(look) = (self.looks)(host.as_deref(), cx) else {
                continue;
            };
            if !self.viewers.contains_key(&key)
                && let Some(v) = TermViewer::spawn(look.clone(), cx)
            {
                self.viewers.insert(key.clone(), v);
            }
            if let Some(v) = self.viewers.get(&key) {
                v.update(cx, |v, cx| {
                    v.set_look(look, cx);
                    // Its last line ending left out: the box ends with the text.
                    v.show(&key, None, text.trim_end(), errors.trim_end(), cx);
                });
            }
        }
    }

    /// An output, drawn as a terminal as tall as its lines (up to
    /// [`OUTPUT_ROWS`], the rest scrolled to); as text if it has no terminal.
    fn output_view(&self, key: &str, text: &str, errors: &str, cx: &App) -> gpui::Div {
        let Some(v) = self.viewers.get(key) else {
            return output_box(self.mono.clone(), text, errors);
        };
        let rows = text.trim_end().lines().count() + errors.trim_end().lines().count();
        div()
            .debug_selector(|| "run-output".into())
            .w_full()
            .h(v.read(cx).height_for(rows.clamp(1, OUTPUT_ROWS)))
            .flex()
            .border_1()
            .border_color(rgb(colors().line))
            .overflow_hidden()
            .child(v.clone())
    }

    /// What the outputs on the page show, as their terminals have them, for
    /// the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn shown_outputs(&self, cx: &App) -> Vec<String> {
        self.viewers
            .values()
            .map(|v| v.read(cx).shown_text())
            .collect()
    }

    /// How each host of the last run ended, by name, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn outcomes(&self) -> Vec<(String, Outcome)> {
        self.order
            .iter()
            .filter_map(|h| match self.states.get(h) {
                Some(State::Ended(o)) => Some((name_in(&self.picks, h), o.clone())),
                _ => None,
            })
            .collect()
    }

    /// The hosts chosen, by name, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn chosen_names(&self) -> Vec<String> {
        self.chosen
            .iter()
            .map(|h| name_in(&self.picks, h))
            .collect()
    }

    /// Whether the confirm step would run now, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn ready_to_go(&self, cx: &App) -> bool {
        self.stage == Stage::Confirm && self.can_go(cx)
    }

    /// Whether a run is over, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// What the tab says is wrong, and its script, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn said(&self, cx: &App) -> (Option<String>, String) {
        (
            self.problem.clone(),
            self.script.read(cx).text().to_string(),
        )
    }

    /// The questions asked now, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn asked(&self) -> usize {
        self.asks.len()
    }

    /// The host list's sections, again (hosts were added or changed).
    pub fn set_picks(&mut self, picks: Vec<Pick>, cx: &mut Context<Self>) {
        if picks != self.picks {
            self.picks = picks;
            cx.notify();
        }
    }

    /// Keeps the chosen ones the list has, in its order.
    fn set_chosen(&mut self, chosen: Vec<String>) {
        self.chosen = self
            .picks
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.spec.clone())
            .filter(|s| chosen.contains(s))
            .fold(vec![], |mut out, s| {
                if !out.contains(&s) {
                    out.push(s);
                }
                out
            });
    }

    /// Where the keyboard starts: the script.
    pub fn script_focus(&self, cx: &App) -> FocusHandle {
        self.script.focus_handle(cx)
    }

    /// The tab's title: the script's first line that says something, once
    /// one is typed.
    pub fn title(&self, cx: &App) -> String {
        let script = self.ran.as_ref().map_or_else(
            || self.script.read(cx).text().to_string(),
            |(s, ..)| s.clone(),
        );
        let line = script
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("set "));
        match line {
            Some(l) if l.chars().count() > 28 => {
                format!("{}…", l.chars().take(27).collect::<String>())
            }
            Some(l) => l.to_string(),
            None => tr!("Run on hosts"),
        }
    }

    /// How far along a run is ("3/7"), while it goes.
    pub fn progress(&self) -> Option<String> {
        (self.stage == Stage::Running).then(|| {
            let ended = self
                .order
                .iter()
                .filter(|h| matches!(self.states.get(*h), Some(State::Ended(_))))
                .count();
            format!("{ended}/{}", self.order.len())
        })
    }

    /// Whether it is signing in or running: closing it stops that.
    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Confirm | Stage::Running)
    }

    fn interpreter(&self, cx: &App) -> Interpreter {
        self.interpreter
            .clone()
            .unwrap_or_else(|| Interpreter::of(self.script.read(cx).text()))
    }

    fn number(input: &Entity<TextInput>, default: u64, most: u64, cx: &App) -> u64 {
        input
            .read(cx)
            .text()
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .map_or(default, |n| n.min(most))
    }

    // What the tab does.

    /// Run: sign in to the chosen hosts, and say what will run where.
    fn ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.problem = None;
        let script = self.script.read(cx).text().to_string();
        if self.chosen.is_empty() {
            self.problem = Some(tr!("Choose at least one host."));
        } else if script.trim().is_empty() {
            self.problem = Some(tr!("Type a command or a script first."));
        } else if script.len() > run::MOST_SCRIPT {
            self.problem = Some(tr!(
                "The script is larger than {kb} KB.",
                kb = (run::MOST_SCRIPT / 1024).to_string()
            ));
        }
        if self.problem.is_some() || self.busy() {
            return cx.notify();
        }
        self.stage = Stage::Confirm;
        self.menu = None;
        self.picker = false;
        self.rest.clear();
        self.asks.clear();
        self.order = self.chosen.clone();
        self.states.clear();
        let mut connect = vec![];
        for spec in self.chosen.clone() {
            if self.fleet.connected(&spec) {
                self.states.insert(spec, State::Ready);
                continue;
            }
            match (self.resolve)(&spec, cx) {
                Ok(target) => {
                    self.states.insert(spec.clone(), State::Connecting);
                    connect.push((spec, target));
                }
                Err(why) => {
                    self.states.insert(spec, State::LeftOut(why));
                }
            }
        }
        let limit = Self::number(&self.at_once, AT_ONCE as u64, 64, cx) as usize;
        self.fleet.connect(connect, limit, self.events.clone());
        self.sudo_hosts = self
            .chosen
            .iter()
            .map(|h| {
                let input = cx.new(|cx| TextInput::said(|| tr!("sudo password"), cx).masked());
                (h.clone(), input)
            })
            .collect();
        let _ = window;
        cx.notify();
    }

    /// Back from the confirm step: the hosts stay connected for the next
    /// time.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        for ask in self.asks.drain(..) {
            for r in ask.replies {
                let _ = r.send(None);
            }
        }
        self.fleet.stop();
        self.stage = Stage::Edit;
        self.order.clear();
        self.states.clear();
        cx.notify();
    }

    fn uses_sudo(&self, cx: &App) -> bool {
        run::uses_sudo(self.script.read(cx).text(), &self.interpreter(cx))
    }

    /// The hosts signed in to, to run on.
    fn ready(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|h| self.states.get(*h) == Some(&State::Ready))
            .cloned()
            .collect()
    }

    /// Whether the confirm step has all it needs: every host signed in or
    /// given up on, nothing asked, the sudo password typed.
    fn can_go(&self, cx: &App) -> bool {
        let settled = self
            .states
            .values()
            .all(|s| !matches!(s, State::Connecting | State::Asking));
        let sudo = !self.uses_sudo(cx)
            || if self.sudo_each {
                self.ready().iter().all(|h| {
                    self.sudo_hosts
                        .get(h)
                        .is_some_and(|i| !i.read(cx).text().is_empty())
                })
            } else {
                !self.sudo.read(cx).text().is_empty()
            };
        settled && self.asks.is_empty() && !self.ready().is_empty() && sudo
    }

    /// Runs it on the hosts signed in to (the first of them alone, for one
    /// host first). The sudo password goes with the run, and the fields
    /// are emptied.
    fn go(&mut self, cx: &mut Context<Self>) {
        if self.stage != Stage::Confirm || !self.can_go(cx) {
            return;
        }
        let script = self.script.read(cx).text().to_string();
        let interpreter = self.interpreter(cx);
        let give_up = Duration::from_secs(Self::number(&self.give_up, GIVE_UP, 24 * 3600, cx));
        let uses_sudo = self.uses_sudo(cx);
        let ready = self.ready();
        // Each host's own password, or one for all: a run per password.
        let mut jobs: Vec<(Job, Vec<String>)> = vec![];
        let job = |sudo: Option<Zeroizing<String>>| Job {
            script: script.clone(),
            interpreter: interpreter.clone(),
            sudo,
            give_up,
        };
        if uses_sudo && self.sudo_each {
            for h in &ready {
                let pw = self
                    .sudo_hosts
                    .get(h)
                    .map(|i| Zeroizing::new(i.read(cx).text().to_string()));
                jobs.push((job(pw), vec![h.clone()]));
            }
        } else {
            let pw = uses_sudo.then(|| Zeroizing::new(self.sudo.read(cx).text().to_string()));
            jobs.push((job(pw), ready.clone()));
        }
        self.forget_sudo(cx);
        // Those not signed in to say why, as what they ended with.
        for h in self.order.clone() {
            match self.states.get(&h).cloned() {
                Some(State::NotConnected(why)) | Some(State::LeftOut(why)) => {
                    self.states
                        .insert(h, State::Ended(Outcome::NotConnected(why)));
                }
                Some(State::Ready) => {
                    self.states.insert(h, State::Waiting);
                }
                _ => {}
            }
        }
        if self.one_first && ready.len() > 1 {
            let first = ready[0].clone();
            let rest: Vec<String> = ready[1..].to_vec();
            for h in &rest {
                self.states.remove(h);
            }
            self.order.retain(|h| !rest.contains(h));
            // Each job's other hosts wait, with their own password if so.
            let mut first_job = None;
            for (j, hosts) in jobs {
                let others: Vec<String> = hosts.iter().filter(|h| **h != first).cloned().collect();
                if hosts.contains(&first) {
                    first_job = Some(j.clone());
                }
                if !others.is_empty() {
                    self.rest.push((j, others));
                }
            }
            if let Some(j) = first_job {
                self.start(j, vec![first], cx);
            }
        } else {
            for (j, hosts) in jobs {
                self.start(j, hosts, cx);
            }
        }
        self.stage = Stage::Running;
        self.settle(cx);
        cx.notify();
    }

    /// After one host first: the others.
    fn go_on(&mut self, cx: &mut Context<Self>) {
        if self.rest.is_empty() {
            return;
        }
        for (job, hosts) in std::mem::take(&mut self.rest) {
            for h in &hosts {
                self.order.push(h.clone());
                self.states.insert(h.clone(), State::Waiting);
            }
            self.start(job, hosts, cx);
        }
        self.stage = Stage::Running;
        cx.notify();
    }

    fn start(&mut self, job: Job, hosts: Vec<String>, cx: &mut Context<Self>) {
        let mut targets = vec![];
        for h in hosts {
            match (self.resolve)(&h, cx) {
                Ok(t) => targets.push((h, t)),
                Err(why) => {
                    self.states
                        .insert(h, State::Ended(Outcome::NotConnected(why)));
                }
            }
        }
        let when = chrono::Local::now().format("%H:%M").to_string();
        self.ran = Some((
            job.script.clone(),
            job.interpreter.clone(),
            when,
            Instant::now(),
        ));
        let limit = Self::number(&self.at_once, AT_ONCE as u64, 64, cx) as usize;
        self.fleet.run(targets, job, limit, self.events.clone());
    }

    /// Stop: the hosts not started are left alone, those running cut off.
    fn stop(&mut self, cx: &mut Context<Self>) {
        self.fleet.stop();
        for s in self.states.values_mut() {
            if matches!(s, State::Waiting | State::Running(_)) {
                *s = State::Stopped;
            }
        }
        self.rest.clear();
        self.stage = Stage::Done;
        cx.notify();
    }

    /// Again, on the hosts that did not end well.
    fn again_on_failed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let failed: Vec<String> = self
            .order
            .iter()
            .filter(|h| !matches!(self.states.get(*h), Some(State::Ended(o)) if o.ok()))
            .cloned()
            .collect();
        if failed.is_empty() {
            return;
        }
        self.stage = Stage::Done;
        self.set_chosen(failed);
        self.ask(window, cx);
    }

    fn again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage == Stage::Running {
            return;
        }
        self.stage = Stage::Done;
        self.ask(window, cx);
    }

    fn forget_sudo(&mut self, cx: &mut Context<Self>) {
        for i in std::iter::once(&self.sudo).chain(self.sudo_hosts.values()) {
            i.update(cx, |i, cx| i.set_text("", 0..0, cx));
        }
    }

    /// Whether the run is over: every host ended.
    fn settle(&mut self, cx: &mut Context<Self>) {
        if self.stage == Stage::Running
            && self
                .order
                .iter()
                .all(|h| matches!(self.states.get(h), Some(State::Ended(_) | State::Stopped)))
        {
            self.stage = Stage::Done;
            cx.notify();
        }
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        match ev {
            Event::Asks {
                host,
                prompt,
                reply,
            } => {
                // A run's questions after the confirm step: answered there too.
                self.states.insert(host.clone(), State::Asking);
                // The same question (a key's passphrase) is answered once.
                let same = match &prompt {
                    Prompt::Passphrase { .. } => self.asks.iter_mut().find(|a| a.prompt == prompt),
                    _ => None,
                };
                match same {
                    Some(a) => {
                        a.hosts.push(host);
                        a.replies.push(reply);
                    }
                    None => {
                        let masked = !prompt.echo();
                        let input = cx.new(|cx| {
                            let i = TextInput::new("", cx);
                            if masked { i.masked() } else { i }
                        });
                        self.asks.push(Ask {
                            hosts: vec![host],
                            prompt,
                            replies: vec![reply],
                            input,
                        });
                    }
                }
            }
            Event::Connected { host } => {
                if self.stage == Stage::Confirm {
                    self.states.insert(host, State::Ready);
                }
            }
            Event::NotConnected { host, why } => {
                if self.stage == Stage::Confirm {
                    self.states.insert(host, State::NotConnected(why));
                }
            }
            Event::Started { host } => {
                self.states.insert(host, State::Running(Instant::now()));
            }
            Event::Ended { host, outcome } => {
                self.states.insert(host, State::Ended(outcome));
                self.settle(cx);
            }
        }
        cx.notify();
    }

    /// Answers the question at `ix` for every host that asked it; None
    /// gives up on them.
    fn answer(&mut self, ix: usize, answer: Option<String>, cx: &mut Context<Self>) {
        if ix >= self.asks.len() {
            return;
        }
        let ask = self.asks.remove(ix);
        for host in &ask.hosts {
            if answer.is_some() {
                self.states.insert(host.clone(), State::Connecting);
            }
        }
        for r in ask.replies {
            let _ = r.send(answer.clone());
        }
        cx.notify();
    }

    fn toggle(&mut self, spec: &str, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        match chosen.iter().position(|s| s == spec) {
            Some(i) => {
                chosen.remove(i);
            }
            None => chosen.push(spec.to_string()),
        }
        self.set_chosen(chosen);
        cx.notify();
    }

    fn toggle_group(&mut self, group: usize, cx: &mut Context<Self>) {
        let Some(pick) = self.picks.get(group) else {
            return;
        };
        let specs: Vec<String> = pick.hosts.iter().map(|h| h.spec.clone()).collect();
        let all = specs.iter().all(|s| self.chosen.contains(s));
        let mut chosen = self.chosen.clone();
        if all {
            chosen.retain(|s| !specs.contains(s));
        } else {
            chosen.extend(specs);
        }
        self.set_chosen(chosen);
        cx.notify();
    }

    fn open_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr!("Open").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first() else {
                return;
            };
            let read = std::fs::metadata(path)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    if m.len() as usize > run::MOST_SCRIPT {
                        Err(tr!(
                            "The script is larger than {kb} KB.",
                            kb = (run::MOST_SCRIPT / 1024).to_string()
                        ))
                    } else {
                        std::fs::read_to_string(path).map_err(|e| e.to_string())
                    }
                });
            let _ = this.update(cx, |v, cx| {
                match read {
                    Ok(text) => {
                        let text = text.replace("\r\n", "\n");
                        v.script.update(cx, |i, cx| i.set_text(&text, 0..0, cx));
                        v.problem = None;
                    }
                    Err(e) => v.problem = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.key == "enter" && k.modifiers.secondary() {
            match self.stage {
                Stage::Confirm => self.go(cx),
                _ => self.ask(window, cx),
            }
            return cx.stop_propagation();
        }
        if k.key == "enter" && !k.modifiers.modified() {
            // Enter in a question's field answers it; in the sudo field, runs.
            if let Some(ix) = self
                .asks
                .iter()
                .position(|a| a.input.focus_handle(cx).is_focused(window))
            {
                let text = self.asks[ix].input.read(cx).text().to_string();
                self.answer(ix, Some(text), cx);
                return cx.stop_propagation();
            }
            if self.sudo.focus_handle(cx).is_focused(window)
                || self
                    .sudo_hosts
                    .values()
                    .any(|i| i.focus_handle(cx).is_focused(window))
            {
                self.go(cx);
                return cx.stop_propagation();
            }
        }
        if k.key == "escape" && self.menu.is_some() {
            self.menu = None;
            cx.notify();
            cx.stop_propagation();
        }
    }

    // How it looks.

    fn field(&self, input: &Entity<TextInput>, w: f32, window: &Window, cx: &App) -> gpui::Div {
        let focused = input.focus_handle(cx).is_focused(window);
        div()
            .w(px(w))
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(if focused {
                colors().accent
            } else {
                colors().field_edge
            }))
            .bg(rgb(colors().field))
            .text_size(ui(12.5))
            .child(input.clone())
    }

    fn hosts_part(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let editing = matches!(self.stage, Stage::Edit | Stage::Done);
        let names: BTreeMap<&str, &Candidate> = self
            .picks
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| (h.spec.as_str(), h))
            .collect();
        let mut by_group: Vec<(String, Vec<&Candidate>)> = vec![];
        for p in &self.picks {
            let hosts: Vec<&Candidate> = p
                .hosts
                .iter()
                .filter(|h| self.chosen.contains(&h.spec))
                .collect();
            if !hosts.is_empty() {
                by_group.push((p.group.clone(), hosts));
            }
        }
        let count = self.chosen.len();
        let summary = if count == 0 {
            tr!("none chosen")
        } else {
            let groups: Vec<String> = by_group
                .iter()
                .map(|(g, h)| format!("{g} {}", h.len()))
                .collect();
            format!(
                "{} · {}",
                tr_n!(count, "{n} host", "{n} hosts", n = count.to_string()),
                groups.join(", ")
            )
        };
        let mut part = div().flex().flex_col().gap(px(8.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(ui(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().label))
                        .child(tr!("Hosts")),
                )
                .child(
                    div()
                        .debug_selector(|| "run-hosts".into())
                        .text_size(ui(12.))
                        .text_color(rgb(colors().muted))
                        .child(summary),
                )
                .when(editing, |d| {
                    d.child(
                        outline_button("run-change-hosts", &tr!("Change"))
                            .h(px(24.))
                            .px(px(8.))
                            .text_size(ui(12.))
                            .when(self.picker, |d| d.bg(rgb(colors().selected)))
                            .on_click(cx.listener(|v, _, window, cx| {
                                v.picker = !v.picker;
                                if v.picker {
                                    window.focus(&v.filter.focus_handle(cx), cx);
                                }
                                cx.notify();
                            })),
                    )
                }),
        );
        for (group, hosts) in &by_group {
            let mut chips = div().flex().flex_wrap().gap(px(6.));
            for h in hosts {
                let spec = h.spec.clone();
                chips = chips.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(26.))
                        .pl(px(3.))
                        .pr(px(if editing { 2. } else { 9. }))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(colors().line))
                        .bg(rgb(colors().surface))
                        .text_size(ui(12.))
                        .child(div().size(px(20.)).child(crate::sidebar::tile_face_sized(
                            h.icon, &h.name, h.color, false, 20.,
                        )))
                        .child(h.name.clone())
                        .when(editing, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("run-drop:{}", h.name)))
                                    .debug_selector({
                                        let n = h.name.clone();
                                        move || format!("run-drop:{n}")
                                    })
                                    .px(px(5.))
                                    .rounded(px(4.))
                                    .text_color(rgb(colors().muted))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(colors().hover)))
                                    .on_click(cx.listener(move |v, _, _, cx| v.toggle(&spec, cx)))
                                    .child("✕"),
                            )
                        }),
                );
            }
            part = part.child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(90.))
                            .flex_shrink_0()
                            .pt(px(5.))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().muted))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(group.clone()),
                    )
                    .child(chips.flex_1()),
            );
        }
        let _ = names;
        if self.picker && editing {
            part = part.child(self.picker_part(window, cx));
        }
        part
    }

    fn picker_part(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let query = self.filter.read(cx).text().trim().to_lowercase();
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(colors().field_edge))
            .bg(rgb(colors().surface))
            .child(
                div()
                    .pb(px(6.))
                    .child(self.field(&self.filter, 260., window, cx)),
            );
        for (gi, p) in self.picks.iter().enumerate() {
            let shown: Vec<&Candidate> = p
                .hosts
                .iter()
                .filter(|h| {
                    query.is_empty()
                        || h.name.to_lowercase().contains(&query)
                        || h.detail.to_lowercase().contains(&query)
                })
                .collect();
            if shown.is_empty() {
                continue;
            }
            let all = p.hosts.iter().all(|h| self.chosen.contains(&h.spec));
            let picked = p
                .hosts
                .iter()
                .filter(|h| self.chosen.contains(&h.spec))
                .count();
            let group = p.group.clone();
            list = list.child(
                div()
                    .id(("run-pick-group", gi))
                    .debug_selector(move || format!("run-pick-group:{group}"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(cx.listener(move |v, _, _, cx| v.toggle_group(gi, cx)))
                    .child(checkbox(all))
                    .child(p.group.clone())
                    .child(
                        div()
                            .ml_auto()
                            .font_weight(FontWeight::NORMAL)
                            .text_size(ui(12.))
                            .text_color(rgb(colors().muted))
                            .child(tr!(
                                "{picked} of {all}",
                                picked = picked.to_string(),
                                all = p.hosts.len().to_string()
                            )),
                    ),
            );
            for h in shown {
                let on = self.chosen.contains(&h.spec);
                let (spec, name) = (h.spec.clone(), h.name.clone());
                list = list.child(
                    div()
                        .id(SharedString::from(format!(
                            "run-pick:{}:{}",
                            p.group, h.spec
                        )))
                        .debug_selector(move || format!("run-pick:{name}"))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .h(px(34.))
                        .pl(px(22.))
                        .pr(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(rgb(colors().selected)))
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| v.toggle(&spec, cx)))
                        .child(checkbox(on))
                        .child(div().font_weight(FontWeight::MEDIUM).child(h.name.clone()))
                        .child(
                            div()
                                .text_size(ui(11.5))
                                .text_color(rgb(colors().muted))
                                .child(h.detail.clone()),
                        ),
                );
            }
        }
        if self.picks.iter().all(|p| p.hosts.is_empty()) {
            list = list.child(hint_text(&tr!("No hosts yet: add one in the host list.")));
        }
        list
    }

    fn script_part(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let editing = matches!(self.stage, Stage::Edit | Stage::Done);
        let interpreter = self.interpreter(cx);
        let why = if self.interpreter.is_some() {
            tr!("chosen")
        } else if Interpreter::from_shebang(self.script.read(cx).text()).is_some() {
            tr!("from #!")
        } else {
            tr!("default")
        };
        let lines = self
            .script
            .read(cx)
            .text()
            .lines()
            .count()
            .clamp(1, SCRIPT_ROWS);
        let focused = self.script.focus_handle(cx).is_focused(window);
        let menu_button = |id: &'static str, label: String, menu: Menu, cx: &mut Context<Self>| {
            outline_button(id, &label)
                .h(px(24.))
                .px(px(8.))
                .text_size(ui(12.))
                .when(self.menu == Some(menu), |d| d.bg(rgb(colors().selected)))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.menu = if v.menu == Some(menu) {
                        None
                    } else {
                        Some(menu)
                    };
                    cx.notify();
                }))
        };
        let mut part = div().flex().flex_col().gap(px(6.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .text_size(ui(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().label))
                        .child(tr!("Command or script")),
                )
                .when(editing, |d| {
                    d.child(
                        div()
                            .id("run-interpreter")
                            .debug_selector(|| "run-interpreter".into())
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .h(px(24.))
                            .px(px(9.))
                            .rounded_full()
                            .border_1()
                            .border_color(rgb(colors().info_edge))
                            .bg(rgb(colors().info_bg))
                            .text_color(rgb(colors().info_text))
                            .text_size(ui(12.))
                            .cursor_pointer()
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.menu = if v.menu == Some(Menu::Interpreter) {
                                    None
                                } else {
                                    Some(Menu::Interpreter)
                                };
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .font_family(self.mono.clone())
                                    .child(interpreter.name().to_string()),
                            )
                            .child(div().text_size(ui(11.)).child(why)),
                    )
                    .child(
                        outline_button("run-open-file", &tr!("Open file…"))
                            .h(px(24.))
                            .px(px(8.))
                            .text_size(ui(12.))
                            .on_click(cx.listener(|v, _, _, cx| v.open_file(cx))),
                    )
                    .child(menu_button(
                        "run-from-snippet",
                        tr!("From a snippet…"),
                        Menu::Snippets,
                        cx,
                    ))
                }),
        );
        match self.menu {
            Some(Menu::Interpreter) if editing => {
                let mut row = div().flex().flex_wrap().gap(px(6.));
                let auto = self.interpreter.is_none();
                row = row.child(
                    outline_button("run-interpreter:auto", &tr!("From the first line"))
                        .h(px(26.))
                        .text_size(ui(12.))
                        .when(auto, |d| d.bg(rgb(colors().selected)))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.interpreter = None;
                            v.menu = None;
                            cx.notify();
                        })),
                );
                for i in Interpreter::offered() {
                    let on = self.interpreter.as_ref() == Some(&i);
                    let name = i.name().to_string();
                    row = row.child(
                        outline_button(&format!("run-interpreter:{name}"), &name)
                            .h(px(26.))
                            .text_size(ui(12.))
                            .font_family(self.mono.clone())
                            .when(on, |d| d.bg(rgb(colors().selected)))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.interpreter = Some(i.clone());
                                v.menu = None;
                                cx.notify();
                            })),
                    );
                }
                part = part.child(row);
            }
            Some(Menu::Snippets) if editing => {
                let snippets = (self.snippets)(cx);
                let mut list = div()
                    .flex()
                    .flex_col()
                    .max_h(px(220.))
                    .p(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .bg(rgb(colors().surface));
                if snippets.is_empty() {
                    list = list.child(hint_text(&tr!("No snippets yet.")));
                }
                for (i, (name, command)) in snippets.into_iter().enumerate() {
                    list = list.child(
                        div()
                            .id(("run-snippet", i))
                            .flex()
                            .gap(px(10.))
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click({
                                let command = command.clone();
                                cx.listener(move |v, _, window, cx| {
                                    v.script.update(cx, |i, cx| {
                                        i.set_text(&command, command.len()..command.len(), cx)
                                    });
                                    v.menu = None;
                                    window.focus(&v.script.focus_handle(cx), cx);
                                    cx.notify();
                                })
                            })
                            .child(div().font_weight(FontWeight::MEDIUM).child(name))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .font_family(self.mono.clone())
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().muted))
                                    .child(command.lines().next().unwrap_or("").to_string()),
                            ),
                    );
                }
                part = part.child(list);
            }
            _ => {}
        }
        part.child(
            div()
                .id("run-script")
                .debug_selector(|| "run-script".into())
                .on_click(cx.listener(|v, _, window, cx| {
                    window.focus(&v.script.focus_handle(cx), cx);
                }))
                // The field is as high as this box, which has room for its
                // lines, up to SCRIPT_ROWS.
                .h(px(lines as f32 * 22. + 22.))
                .flex()
                .min_w_0()
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(if focused {
                    colors().accent
                } else {
                    colors().field_edge
                }))
                .bg(rgb(colors().field))
                .overflow_hidden()
                .font_family(self.mono.clone())
                .text_size(ui(13.))
                .line_height(px(22.))
                .child(self.script.clone()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .text_size(ui(12.))
                .text_color(rgb(colors().soft))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(tr!("At most"))
                        .child(self.field(&self.at_once, 44., window, cx))
                        .child(tr!("at once")),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(tr!("Give up after"))
                        .child(self.field(&self.give_up, 52., window, cx))
                        .child(tr!("seconds")),
                )
                .child(
                    div()
                        .id("run-one-first")
                        .debug_selector(|| "run-one-first".into())
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.one_first = !v.one_first;
                            cx.notify();
                        }))
                        .child(checkbox(self.one_first))
                        .child(tr!("One host first, the rest when you say")),
                )
                .child(div().flex_1())
                .child(hint_text(&if cfg!(target_os = "macos") {
                    tr!("Cmd+Enter runs")
                } else {
                    tr!("Ctrl+Enter runs")
                }))
                .when(editing, |d| {
                    let label = if self.chosen.is_empty() {
                        tr!("Run")
                    } else {
                        tr_n!(
                            self.chosen.len(),
                            "Run on {n} host",
                            "Run on {n} hosts",
                            n = self.chosen.len().to_string()
                        )
                    };
                    d.child(
                        primary_button("run-go", &format!("▶  {label}"))
                            .on_click(cx.listener(|v, _, window, cx| v.ask(window, cx))),
                    )
                }),
        )
    }

    fn confirm_part(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let interpreter = self.interpreter(cx);
        let script = self.script.read(cx).text().to_string();
        let lines = script.lines().count();
        let ready = self.ready();
        let total = self.order.len();
        let signing = self
            .states
            .values()
            .filter(|s| matches!(s, State::Connecting | State::Asking))
            .count();
        let one_first = self.one_first && ready.len() > 1;
        let title = if one_first {
            tr!(
                "Run it on {first} first, then the other {n}?",
                first = name_in(&self.picks, &ready[0]),
                n = (ready.len() - 1).to_string()
            )
        } else {
            tr_n!(
                total,
                "Run it on {n} host?",
                "Run it on {n} hosts?",
                n = total.to_string()
            )
        };
        let sub = format!(
            "{} · {} · {}",
            tr_n!(
                lines,
                "{interpreter}, {n} line",
                "{interpreter}, {n} lines",
                interpreter = interpreter.name().to_string(),
                n = lines.to_string()
            ),
            if signing > 0 {
                tr!(
                    "signing in to {n} of {all}…",
                    n = signing.to_string(),
                    all = total.to_string()
                )
            } else {
                tr!(
                    "{n} of {all} signed in",
                    n = ready.len().to_string(),
                    all = total.to_string()
                )
            },
            tr!(
                "at most {n} at once",
                n = Self::number(&self.at_once, AT_ONCE as u64, 64, cx).to_string()
            )
        );
        let mut body = div().flex().flex_col().gap(px(10.)).px(px(18.)).py(px(12.));
        // What the hosts ask: each once.
        if !self.asks.is_empty() {
            body = body.child(label_text(&tr!("Signing in: asked now, once")));
        }
        for (ix, ask) in self.asks.iter().enumerate() {
            let hosts: Vec<String> = ask.hosts.iter().map(|h| name_in(&self.picks, h)).collect();
            let what = match &ask.prompt {
                Prompt::Password { target, retry } => {
                    if *retry {
                        tr!(
                            "{target}'s password, again: that one was refused",
                            target = target.clone()
                        )
                    } else {
                        tr!("{target}'s password", target = target.clone())
                    }
                }
                Prompt::Passphrase { key, .. } => {
                    tr!("The passphrase of {key}", key = key.clone())
                }
                Prompt::HostKey {
                    algorithm,
                    fingerprint,
                    ..
                } => tr!(
                    "A host key never seen before: {algorithm} {fingerprint}",
                    algorithm = algorithm.clone(),
                    fingerprint = fingerprint.clone()
                ),
                Prompt::Question { text, .. } => text.trim().to_string(),
            };
            let row = div()
                .debug_selector(move || format!("run-ask:{ix}"))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(hosts.join(", ")),
                        )
                        .child(hint_text(&what)),
                );
            let row = if matches!(ask.prompt, Prompt::HostKey { .. }) {
                row.child(
                    outline_button(&format!("run-ask-no:{ix}"), &tr!("Leave it out")).on_click(
                        cx.listener(move |v, _, _, cx| v.answer(ix, Some("no".into()), cx)),
                    ),
                )
                .child(
                    primary_button(&format!("run-ask-yes:{ix}"), &tr!("Trust it")).on_click(
                        cx.listener(move |v, _, _, cx| v.answer(ix, Some("yes".into()), cx)),
                    ),
                )
            } else {
                row.child(
                    div()
                        .debug_selector(move || format!("run-ask-input:{ix}"))
                        .child(self.field(&ask.input, 220., window, cx)),
                )
                .child(
                    outline_button(&format!("run-ask-ok:{ix}"), &tr!("OK"))
                        .h(px(28.))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            let text = v.asks.get(ix).map(|a| a.input.read(cx).text().to_string());
                            v.answer(ix, text, cx);
                        })),
                )
            };
            body = body.child(row);
        }
        // Those left out, and why.
        for h in &self.order {
            let why = match self.states.get(h) {
                Some(State::LeftOut(w)) | Some(State::NotConnected(w)) => w.clone(),
                _ => continue,
            };
            let name = name_in(&self.picks, h);
            body = body.child(
                div()
                    .debug_selector({
                        let n = name.clone();
                        move || format!("run-left-out:{n}")
                    })
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(rgb(colors().warn_bg))
                    .text_color(rgb(colors().warn_text))
                    .text_size(ui(12.))
                    .child(tr!("{host} is left out: {why}", host = name, why = why)),
            );
        }
        if self.uses_sudo(cx) {
            let mut sudo = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(10.))
                .border_t_1()
                .border_color(rgb(colors().line))
                .child(label_text("sudo"));
            if self.sudo_each {
                for h in &ready {
                    if let Some(input) = self.sudo_hosts.get(h) {
                        sudo = sudo.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(div().w(px(120.)).child(name_in(&self.picks, h)))
                                .child(self.field(input, 220., window, cx)),
                        );
                    }
                }
            } else {
                sudo = sudo.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div().flex_1().child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(tr!("sudo password, for every host")),
                            ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "run-sudo".into())
                                .child(self.field(&self.sudo, 220., window, cx)),
                        ),
                );
            }
            sudo = sudo
                .child(
                    div()
                        .id("run-sudo-each")
                        .debug_selector(|| "run-sudo-each".into())
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .text_size(ui(12.))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.sudo_each = !v.sudo_each;
                            cx.notify();
                        }))
                        .child(checkbox(!self.sudo_each))
                        .child(tr!("The same on every host")),
                )
                .child(hint_text(&tr!(
                    "The script uses sudo: the password goes to each host with it, for sudo there to read, never on a command line, in a file or in what is shown. It is forgotten once the run starts."
                )));
            body = body.child(sudo);
        }
        let go = self.can_go(cx);
        let go_label = if one_first {
            tr!("Run on {host}", host = name_in(&self.picks, &ready[0]))
        } else {
            tr_n!(
                ready.len(),
                "Run on {n} host",
                "Run on {n} hosts",
                n = ready.len().to_string()
            )
        };
        div()
            .debug_selector(|| "run-confirm".into())
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(colors().info_edge))
            .bg(rgb(colors().surface))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(18.))
                    .pt(px(14.))
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(
                        div()
                            .text_size(ui(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(hint_text(&sub)),
            )
            .child(body)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(18.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .child(
                        outline_button("run-cancel", &tr!("Cancel"))
                            .on_click(cx.listener(|v, _, _, cx| v.cancel(cx))),
                    )
                    .child(if go {
                        primary_button("run-confirm-go", &go_label)
                            .on_click(cx.listener(|v, _, _, cx| v.go(cx)))
                    } else {
                        div()
                            .id("run-confirm-go")
                            .debug_selector(|| "run-confirm-go".into())
                            .h(px(32.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(rgb(colors().button))
                            .text_color(rgb(colors().muted))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(go_label)
                    }),
            )
    }

    /// The hosts' ends, as [`run::groups`] puts them.
    fn ended(&self) -> Vec<(Said, Vec<String>)> {
        let ended: Vec<(&str, &Outcome)> = self
            .order
            .iter()
            .filter_map(|h| match self.states.get(h) {
                Some(State::Ended(o)) => Some((h.as_str(), o)),
                _ => None,
            })
            .collect();
        run::groups(ended)
    }

    fn results_part(&self, cx: &mut Context<Self>) -> gpui::Div {
        let total = self.order.len();
        let ok = self
            .states
            .values()
            .filter(|s| matches!(s, State::Ended(o) if o.ok()))
            .count();
        let failed = self
            .states
            .values()
            .filter(|s| matches!(s, State::Ended(o) if !o.ok()))
            .count();
        let stopped = self
            .states
            .values()
            .filter(|s| matches!(s, State::Stopped))
            .count();
        let to_go = total.saturating_sub(ok + failed + stopped);
        let running = self.stage == Stage::Running;
        let (script, interpreter, when) = match &self.ran {
            Some((s, i, w, _)) => (s.clone(), i.name().to_string(), w.clone()),
            None => (String::new(), String::new(), String::new()),
        };
        let first_line = script
            .lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with("#!"))
            .unwrap_or("")
            .to_string();
        let bar = |n: usize, color: u32| {
            div()
                .h_full()
                .w(px(if total == 0 {
                    0.
                } else {
                    220. * n as f32 / total as f32
                }))
                .bg(rgb(color))
        };
        let toggle =
            |id: &'static str, label: String, on: bool, by_host: bool, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .h(px(26.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .text_size(ui(12.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(rgb(colors().surface)).shadow_sm())
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.by_host = by_host;
                        cx.notify();
                    }))
                    .child(label)
            };
        let mut head = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .px(px(7.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(rgb(colors().info_bg))
                            .text_color(rgb(colors().info_text))
                            .font_family(self.mono.clone())
                            .text_size(ui(11.5))
                            .child(interpreter),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(self.mono.clone())
                            .text_size(ui(12.5))
                            .child(first_line),
                    )
                    .child(hint_text(&if running {
                        tr!("started {when}", when = when.clone())
                    } else {
                        tr!("ran at {when}", when = when.clone())
                    }))
                    .when(running, |d| {
                        d.child(
                            outline_button("run-stop", &tr!("Stop"))
                                .text_color(rgb(colors().bad))
                                .on_click(cx.listener(|v, _, _, cx| v.stop(cx))),
                        )
                    })
                    .when(!running && failed > 0, |d| {
                        d.child(
                            outline_button(
                                "run-again-failed",
                                &tr_n!(
                                    failed,
                                    "Again on the {n} that failed",
                                    "Again on the {n} that failed",
                                    n = failed.to_string()
                                ),
                            )
                            .on_click(
                                cx.listener(|v, _, window, cx| v.again_on_failed(window, cx)),
                            ),
                        )
                    })
                    .when(!running, |d| {
                        d.child(
                            primary_button("run-again", &tr!("Run again"))
                                .on_click(cx.listener(|v, _, window, cx| v.again(window, cx))),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .w(px(220.))
                            .h(px(6.))
                            .flex()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(rgb(colors().button))
                            .child(bar(ok, colors().ok))
                            .child(bar(failed, colors().bad)),
                    )
                    .child(
                        div()
                            .debug_selector(|| "run-counts".into())
                            .flex()
                            .gap(px(14.))
                            .text_size(ui(12.5))
                            .child(
                                div()
                                    .text_color(rgb(colors().good_text))
                                    .child(tr!("{n} ok", n = ok.to_string())),
                            )
                            .child(
                                div()
                                    .text_color(rgb(colors().bad_text))
                                    .child(tr!("{n} failed", n = failed.to_string())),
                            )
                            .when(to_go > 0 && running, |d| {
                                d.child(
                                    div()
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("{n} to go", n = to_go.to_string())),
                                )
                            })
                            .when(stopped > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("{n} stopped", n = stopped.to_string())),
                                )
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(rgb(colors().button))
                            .child(toggle(
                                "run-by-output",
                                tr!("By output"),
                                !self.by_host,
                                false,
                                cx,
                            ))
                            .child(toggle(
                                "run-by-host",
                                tr!("By host"),
                                self.by_host,
                                true,
                                cx,
                            )),
                    ),
            );
        if !self.rest.is_empty() && !running {
            let n: usize = self.rest.iter().map(|(_, h)| h.len()).sum();
            head = head.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(rgb(colors().info_bg))
                    .text_color(rgb(colors().info_text))
                    .child(div().flex_1().child(tr!(
                        "The first host is done. The other {n} wait for you.",
                        n = n.to_string()
                    )))
                    .child(
                        primary_button(
                            "run-go-on",
                            &tr_n!(
                                n,
                                "Run on the other {n}",
                                "Run on the other {n}",
                                n = n.to_string()
                            ),
                        )
                        .on_click(cx.listener(|v, _, _, cx| v.go_on(cx))),
                    ),
            );
        }
        let list = if self.by_host {
            self.by_host_part(cx)
        } else {
            self.by_output_part(cx)
        };
        div().flex().flex_col().gap(px(14.)).child(head).child(list)
    }

    fn by_output_part(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut out = div().flex().flex_col().gap(px(10.));
        let groups = self.ended();
        for (gi, (said, hosts)) in groups.iter().enumerate() {
            let good = matches!(
                said,
                Said::Ran {
                    status: Some(0),
                    ..
                }
            );
            let (status, text, errors) = said_text(said);
            let mut chips = div().flex().flex_wrap().gap(px(6.));
            for h in hosts {
                let name = name_in(&self.picks, h);
                let took = match self.states.get(h) {
                    Some(State::Ended(Outcome::Ran(r))) => format!("{:.1} s", r.took.as_secs_f32()),
                    _ => String::new(),
                };
                let spec = h.clone();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("run-host:{gi}:{h}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(26.))
                        .px(px(9.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(colors().line))
                        .bg(rgb(colors().surface))
                        .text_size(ui(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.by_host = true;
                            v.open_row = Some(spec.clone());
                            cx.notify();
                        }))
                        .child(name)
                        .child(
                            div()
                                .text_size(ui(11.))
                                .text_color(rgb(colors().muted))
                                .child(took),
                        ),
                );
            }
            let count = hosts.len();
            out = out.child(
                div()
                    .debug_selector(move || format!("run-group-out:{gi}"))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(if good {
                        colors().line
                    } else {
                        colors().bad_edge
                    }))
                    .bg(rgb(if good {
                        colors().surface
                    } else {
                        colors().bad_bg
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(20.))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(if good { colors().ok } else { colors().bad }))
                                    .text_color(rgb(colors().on_accent))
                                    .text_size(ui(11.))
                                    .child(if good { "✓" } else { "✕" }),
                            )
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(tr_n!(
                                count,
                                "{n} host",
                                "{n} hosts",
                                n = count.to_string()
                            )))
                            .child(hint_text(&status)),
                    )
                    .when(!text.is_empty() || !errors.is_empty(), |d| {
                        d.child(self.output_view(&output_key(&text, &errors), &text, &errors, cx))
                    })
                    .child(chips),
            );
        }
        // Those still going.
        let going: Vec<String> = self
            .order
            .iter()
            .filter(|h| !matches!(self.states.get(*h), Some(State::Ended(_))))
            .map(|h| name_in(&self.picks, h))
            .collect();
        if !going.is_empty() {
            out = out.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .text_color(rgb(colors().muted))
                    .child(if self.stage == Stage::Running {
                        tr!("Still going: {hosts}", hosts = going.join(", "))
                    } else {
                        tr!(
                            "Stopped before they ended: {hosts}",
                            hosts = going.join(", ")
                        )
                    }),
            );
        }
        out
    }

    fn by_host_part(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut table = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().surface))
            .overflow_hidden();
        for h in &self.order {
            let name = name_in(&self.picks, h);
            let state = self.states.get(h).cloned().unwrap_or(State::Waiting);
            let (status, color, last, took) = state_text(&state);
            let open = self.open_row.as_ref() == Some(h);
            let spec = h.clone();
            table = table.child(
                div()
                    .id(SharedString::from(format!("run-row:{h}")))
                    .debug_selector({
                        let n = name.clone();
                        move || format!("run-row:{n}")
                    })
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(40.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .cursor_pointer()
                    .when(open, |d| d.bg(rgb(colors().selected)))
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.open_row = if v.open_row.as_ref() == Some(&spec) {
                            None
                        } else {
                            Some(spec.clone())
                        };
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(160.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_color(rgb(color))
                            .child(div().size(px(8.)).rounded_full().bg(rgb(color)))
                            .child(status),
                    )
                    .child(div().w(px(70.)).text_color(rgb(colors().muted)).child(took))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(self.mono.clone())
                            .text_size(ui(12.))
                            .child(last),
                    ),
            );
            if open {
                let (text, errors) = host_output(&state);
                let copy = format!("{text}{errors}");
                let spec = h.clone();
                table = table.child(
                    div()
                        .debug_selector({
                            let n = name.clone();
                            move || format!("run-detail:{n}")
                        })
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .px(px(14.))
                        .py(px(10.))
                        .border_b_1()
                        .border_color(rgb(colors().line))
                        .child(self.output_view(&format!("host:{h}"), &text, &errors, cx))
                        .when(
                            matches!(&state, State::Ended(Outcome::Ran(r)) if r.cut),
                            |d| {
                                d.child(hint_text(&tr!(
                                    "It printed more than {mb} MB: the rest is not kept.",
                                    mb = (run::KEEP / 1024 / 1024).to_string()
                                )))
                            },
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    outline_button("run-open-tab", &tr!("Open a tab"))
                                        .h(px(28.))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(RunEvent::Open(spec.clone()))
                                        })),
                                )
                                .child(
                                    outline_button("run-copy", &tr!("Copy output"))
                                        .h(px(28.))
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        }),
                                ),
                        ),
                );
            }
        }
        table
    }
}

/// What a host's open row shows: its output and its errors (or why it has
/// none).
fn host_output(state: &State) -> (String, String) {
    match state {
        State::Ended(Outcome::Ran(r)) => (r.stdout.clone(), r.stderr.clone()),
        State::Ended(Outcome::NotConnected(w))
        | State::Ended(Outcome::Failed(w))
        | State::NotConnected(w)
        | State::LeftOut(w) => (String::new(), w.clone()),
        State::Ended(Outcome::GaveUp(d)) => (
            String::new(),
            tr!(
                "Given up on after {s} seconds.",
                s = d.as_secs().to_string()
            ),
        ),
        _ => (String::new(), String::new()),
    }
}

/// A group's output, named by what it is: the same output keeps its viewer
/// as more hosts end and the groups move.
fn output_key(text: &str, errors: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    (text, errors).hash(&mut h);
    format!("group:{:x}", h.finish())
}

/// What a group of hosts said: its status line, its output and its errors.
fn said_text(said: &Said) -> (String, String, String) {
    match said {
        Said::Ran {
            status,
            stdout,
            stderr,
        } => {
            let code = match status {
                Some(c) => tr!("exit {code}", code = c.to_string()),
                None => tr!("ended by a signal"),
            };
            let sudo = if stderr.contains("incorrect password attempt") {
                format!("{} · ", tr!("sudo did not take the password"))
            } else if stderr.contains("sudo: a terminal is required")
                || stderr.contains("sudo: a password is required")
            {
                format!("{} · ", tr!("sudo asked for a password"))
            } else {
                String::new()
            };
            (format!("{sudo}{code}"), stdout.clone(), stderr.clone())
        }
        Said::NotConnected(why) => (tr!("did not connect"), String::new(), why.clone()),
        Said::GaveUp => (tr!("given up on"), String::new(), String::new()),
        Said::Failed(why) => (tr!("did not run"), String::new(), why.clone()),
    }
}

/// A host's line in the table: its status, its color, its last line, its
/// time.
fn state_text(s: &State) -> (String, u32, String, String) {
    let c = colors();
    let last = |text: &str| text.trim_end().lines().last().unwrap_or("").to_string();
    match s {
        State::Connecting => (tr!("signing in"), c.muted, String::new(), String::new()),
        State::Asking => (tr!("asks something"), c.warn, String::new(), String::new()),
        State::Ready => (tr!("signed in"), c.ok, String::new(), String::new()),
        State::NotConnected(w) => (tr!("did not connect"), c.bad, w.clone(), String::new()),
        State::LeftOut(w) => (tr!("left out"), c.warn, w.clone(), String::new()),
        State::Waiting => (tr!("waiting"), c.muted, String::new(), String::new()),
        State::Running(since) => (
            tr!("running"),
            c.accent,
            String::new(),
            format!("{} s", since.elapsed().as_secs()),
        ),
        State::Stopped => (tr!("stopped"), c.muted, String::new(), String::new()),
        State::Ended(o) => match o {
            Outcome::Ran(r) => {
                let took = format!("{:.1} s", r.took.as_secs_f32());
                let line = if r.stdout.trim().is_empty() {
                    last(&r.stderr)
                } else {
                    last(&r.stdout)
                };
                if r.ok() {
                    (tr!("exit 0"), c.ok, line, took)
                } else if r.sudo_refused() {
                    (tr!("sudo refused"), c.bad, last(&r.stderr), took)
                } else {
                    let code = r.status.map_or_else(
                        || tr!("ended by a signal"),
                        |s| tr!("exit {code}", code = s.to_string()),
                    );
                    (code, c.bad, line, took)
                }
            }
            Outcome::NotConnected(w) => (tr!("did not connect"), c.bad, w.clone(), String::new()),
            Outcome::GaveUp(d) => (
                tr!("given up on"),
                c.bad,
                String::new(),
                format!("{} s", d.as_secs()),
            ),
            Outcome::Failed(w) => (tr!("did not run"), c.bad, w.clone(), String::new()),
        },
    }
}

/// What a host printed: its output, then its errors in red.
fn output_box(mono: SharedString, text: &str, errors: &str) -> gpui::Div {
    let shown = |s: &str| {
        // A long output shows its end, which is where what went wrong is.
        let lines: Vec<&str> = s.trim_end().lines().collect();
        let from = lines.len().saturating_sub(200);
        lines[from..].join("\n")
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(6.))
        .bg(rgb(colors().field))
        .border_1()
        .border_color(rgb(colors().line))
        .font_family(mono)
        .text_size(ui(12.5))
        .when(!text.trim().is_empty(), |d| {
            d.child(div().child(shown(text)))
        })
        .when(!errors.trim().is_empty(), |d| {
            d.child(
                div()
                    .text_color(rgb(colors().bad_text))
                    .child(shown(errors)),
            )
        })
}

impl Render for RunView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        self.refresh_viewers(cx);
        let mut page = div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .w_full()
            .px(px(32.))
            .py(px(22.))
            .child(self.hosts_part(window, cx))
            .child(self.script_part(window, cx));
        if let Some(p) = &self.problem {
            page = page.child(problem_text(p));
        }
        page = match self.stage {
            Stage::Edit => page.child(
                div()
                    .debug_selector(|| "run-empty".into())
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .min_h(px(200.))
                    .px(px(24.))
                    .rounded(px(12.))
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(colors().field_edge))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(tr!("What each host says shows here, grouped by output.")),
                    )
                    .child(hint_text(&tr!(
                        "It runs as ssh host 'command' would, with no terminal, read whole before it starts, so nothing in it reads the rest of it. sudo's password is asked once, before it runs; anything else that asks (a y/n) gets no answer."
                    ))),
            ),
            Stage::Confirm => page.child(self.confirm_part(window, cx)),
            Stage::Running | Stage::Done => page.child(self.results_part(cx)),
        };
        div()
            .id("run-view")
            .debug_selector(|| "run-view".into())
            .key_context("RunView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(colors().page))
            .text_color(rgb(colors().text))
            .text_size(ui(13.))
            .flex()
            .flex_col()
            .child(page)
    }
}

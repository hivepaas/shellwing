//! AI access to hosts, over MCP (docs/configuration.md, AI access): the
//! clients let in, the hosts given them, and what they run there.
//!
//! An AI client never sees a key, a password or a passphrase: shellwing
//! signs in for it, with what the person saved, and a host that would ask
//! is refused. What it runs is a job, whose output it reads as it comes
//! ([`Job::wait`]), so a long command does not keep it waiting for its end.

pub mod files;
pub mod history;
pub mod log;
pub mod mcp;
pub mod notes;
pub mod snippets;
pub mod socket;
pub mod sudo;
pub mod tab;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shellwing_config::{AiClient, AiGrant, AiHow, AiMode, AiState};
use shellwing_ssh::config::Resolved;
use shellwing_ssh::{Auth, ExecConnection, HostKeyPolicy, Options, Prompt, Prompter, Secrets};
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::AbortHandle;

/// What a job keeps of its output.
pub const KEEP: usize = 1024 * 1024;
/// What the activity log keeps of each command's output.
pub const LOGGED: usize = 64 * 1024;
/// Commands the activity shows, the oldest let go first: the log on this
/// computer keeps more, as much as the person chose ([`log`]).
pub const ACTIVITY: usize = 1000;
/// How often the app is told what a running command printed.
const FOLLOW: Duration = Duration::from_millis(250);
/// How long a host may take to answer when its settings say nothing.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// A new client's token, to hand it once: `shw_` and 32 random bytes in hex.
pub fn new_token() -> String {
    let mut b = [0u8; 32];
    let _ = getrandom::fill(&mut b);
    format!("shw_{}", hex(&b))
}

/// What a token is kept as.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.trim().as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A host a client may be given: as the host list names it, and where it
/// is (or why it cannot be reached through shellwing).
#[derive(Clone, Debug)]
pub struct Host {
    pub name: String,
    /// Its address, as the host list shows it.
    pub detail: String,
    /// Its group in hosts.toml, which snippets are offered for.
    pub group: Option<String>,
    pub target: Result<Resolved, String>,
}

/// Why a job waits for the person when its host, or a jump on the way,
/// shows a key not in known_hosts: then the one showing it, as the
/// connection names it, a line, and the key, as `ED25519 SHA256:…`.
/// Trusted, the key is kept and the job goes on.
pub const HOST_KEY: &str = "\u{0}host-key:";

/// One command an AI ran, as the activity keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// The job's id.
    pub id: String,
    /// The client's name.
    pub client: String,
    /// The host's name.
    pub host: String,
    pub command: String,
    /// Seconds since 1970.
    pub started: u64,
    #[serde(default)]
    pub ended: Option<u64>,
    /// How it ended: `exit 0`, a reason it did not run, `stopped`; None
    /// while it runs.
    #[serde(default)]
    pub end: Option<End>,
    /// What it printed, the first [`LOGGED`] bytes of it once logged.
    #[serde(default)]
    pub output: String,
    /// It waits for the person to allow it: the risky words it has, or
    /// nothing when its grant asks for every command.
    #[serde(default)]
    pub waiting: Option<String>,
    /// What it adds to a note, as it will go in: shown while it waits for
    /// the person, never kept in the log (the vault keeps notes encrypted).
    #[serde(skip)]
    pub adding: Option<String>,
    /// The snippet it ran, by name, when it was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// The client's id, which reads back its own ([`history`]); None in
    /// what was logged before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The host's spec, as its grant names it; None for what was on no one
    /// host (reading notes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Seconds it ran, once it ended: the time it waited for the person
    /// aside. None in what was logged before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum End {
    /// Its exit status; None when a signal ended it.
    Exited(Option<u32>),
    /// It did not run, or its end was not seen: why.
    Failed(String),
    /// Stopped, by the AI or the person.
    Stopped,
    /// The person would not have it run.
    Refused,
}

impl End {
    pub fn ok(&self) -> bool {
        *self == End::Exited(Some(0))
    }

    /// As the AI is told it.
    pub fn said(&self) -> String {
        match self {
            End::Exited(Some(c)) => format!("exit {c}"),
            End::Exited(None) => "ended by a signal".into(),
            End::Failed(why) => format!("did not run: {why}"),
            End::Stopped => "stopped".into(),
            End::Refused => "refused by the user, who would not have it run".into(),
        }
    }
}

pub use files::{FileOp, WRITES};

/// What the app hears from the hub.
#[derive(Clone, Debug)]
pub enum HubEvent {
    /// A command started, or printed, or ended: the activity's entry now.
    Changed(Activity),
    /// A client connected (true) or went (false).
    Client { id: String, connected: bool },
    /// A command for a host given through a tab, allowed: the app types it
    /// there ([`tab::wrap`]) and hands back what it prints
    /// ([`Hub::tab_print`], [`Hub::tab_end`]).
    Tab {
        id: String,
        /// The host, as the host list opens it.
        spec: String,
        command: String,
        client: String,
        /// The environments (their ids) it runs with, in a subshell, as a
        /// pane runs a snippet that needs them: the app sends their values.
        needs: Vec<String>,
    },
    /// A command typed into a tab was stopped: the app interrupts it there.
    TabStop { id: String },
    /// An AI added to the notes: lists showing them show them again.
    NotesChanged,
    /// An AI added a snippet: lists showing them show them again.
    SnippetsChanged,
    /// The person cleared the activity: only what runs or waits is left.
    Cleared,
    /// A second shellwing was started on this config: this one comes
    /// forward, and opens what it was given (`args`, as typed: a host and
    /// its forwards), while the second ends.
    Show { args: Vec<String> },
}

/// A command an AI started: its output so far, and its end once it came.
pub struct Job {
    pub id: String,
    /// The client's id, which alone may read it.
    pub client: String,
    pub host: String,
    began: Instant,
    state: Mutex<JobState>,
    changed: Notify,
    task: Mutex<Option<AbortHandle>>,
    /// Typed into a tab, rather than run on a connection of its own.
    pub tab: bool,
    /// While it waits for the person: where their answer goes.
    asking: Mutex<Option<tokio::sync::oneshot::Sender<Approval>>>,
    /// sudo's password, as the person gave it for this run (sudo.rs).
    sudo: Mutex<Option<zeroize::Zeroizing<String>>>,
}

/// The person's answer to a command that waits for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approval {
    /// This time.
    Once,
    /// This command, from this client on this host, until shellwing quits.
    Session,
    Refuse,
}

struct JobState {
    activity: Activity,
    out: Vec<u8>,
    cut: bool,
    /// How much of `out` the AI has read.
    read: usize,
    /// The time it waited for the person, which is not running; and since
    /// when it waits now.
    waited: Duration,
    waiting_since: Option<Instant>,
    /// How long it ran, once it ended.
    took: Option<Duration>,
}

/// What a job has to say now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    /// Output the AI had not read yet.
    pub new: String,
    pub end: Option<End>,
    pub seconds: u64,
    /// It printed more than [`KEEP`]: the rest was not kept.
    pub cut: bool,
    /// It waits for the person to allow it.
    pub waiting: bool,
}

impl Job {
    fn new(id: String, client: &AiClient, spec: &str, host: &str, command: &str) -> Job {
        Job {
            id: id.clone(),
            client: client.id.clone(),
            host: spec.to_string(),
            began: Instant::now(),
            state: Mutex::new(JobState {
                activity: Activity {
                    id,
                    client: client.name.clone(),
                    host: host.to_string(),
                    command: command.to_string(),
                    started: now(),
                    ended: None,
                    end: None,
                    output: String::new(),
                    waiting: None,
                    adding: None,
                    snippet: None,
                    client_id: Some(client.id.clone()),
                    spec: (!spec.is_empty()).then(|| spec.to_string()),
                    ran: None,
                },
                out: vec![],
                cut: false,
                read: 0,
                waited: Duration::ZERO,
                waiting_since: None,
                took: None,
            }),
            changed: Notify::new(),
            task: Mutex::new(None),
            tab: false,
            asking: Mutex::new(None),
            sudo: Mutex::new(None),
        }
    }

    pub fn waiting(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|s| s.activity.waiting.is_some())
    }

    fn set_snippet(&self, name: String) {
        if let Ok(mut st) = self.state.lock() {
            st.activity.snippet = Some(name);
        }
    }

    fn set_adding(&self, text: String) {
        if let Ok(mut st) = self.state.lock() {
            st.activity.adding = Some(text);
        }
    }

    fn set_waiting(&self, why: Option<String>) {
        if let Ok(mut st) = self.state.lock() {
            match (&why, st.waiting_since) {
                (Some(_), None) => st.waiting_since = Some(Instant::now()),
                (None, Some(since)) => {
                    st.waited += since.elapsed();
                    st.waiting_since = None;
                }
                _ => {}
            }
            st.activity.waiting = why;
        }
        self.changed.notify_waiters();
    }

    /// How long it ran so far, or in all once it ended: the time it waited
    /// for the person aside.
    fn ran(&self, st: &JobState) -> Duration {
        st.took.unwrap_or_else(|| {
            let waiting = st.waiting_since.map_or(Duration::ZERO, |s| s.elapsed());
            self.began.elapsed().saturating_sub(st.waited + waiting)
        })
    }

    pub fn activity(&self) -> Activity {
        let st = self.state.lock().expect("job");
        let mut a = st.activity.clone();
        a.output = String::from_utf8_lossy(&st.out).into_owned();
        a
    }

    pub fn ended(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.activity.end.is_some())
    }

    /// How much it printed so far.
    fn printed(&self) -> usize {
        self.state.lock().map_or(0, |s| s.out.len())
    }

    fn print(&self, data: &[u8]) {
        if let Ok(mut st) = self.state.lock() {
            let room = KEEP.saturating_sub(st.out.len());
            st.cut |= data.len() > room;
            st.out.extend_from_slice(&data[..data.len().min(room)]);
        }
        self.changed.notify_waiters();
    }

    /// Its end, told at once: a job on its own, in tests ([`Hub::finish`]
    /// ends one the activity keeps).
    #[cfg(test)]
    fn end(&self, end: End) {
        self.set_end(end);
        self.changed.notify_waiters();
    }

    /// Its end, kept, with how long it ran; told to no one yet.
    fn set_end(&self, end: End) {
        if let Ok(mut st) = self.state.lock()
            && st.activity.end.is_none()
        {
            let took = self.ran(&st);
            st.took = Some(took);
            st.activity.ran = Some(took.as_secs());
            st.activity.end = Some(end);
            st.activity.ended = Some(now());
        }
    }

    /// Waits up to `most` for its end (`for_end`), or else for output not
    /// read yet, then says what it has: the new output, read now.
    pub async fn wait(&self, most: Duration, for_end: bool) -> Said {
        let deadline = tokio::time::Instant::now() + most;
        loop {
            let notified = self.changed.notified();
            let ready = self
                .state
                .lock()
                .is_ok_and(|st| st.activity.end.is_some() || (!for_end && st.out.len() > st.read));
            if ready {
                break;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                break;
            }
        }
        self.take()
    }

    /// The output not read yet, read now: whole characters only, until it
    /// ends.
    fn take(&self) -> Said {
        let Ok(mut st) = self.state.lock() else {
            return Said {
                new: String::new(),
                end: Some(End::Failed("lost".into())),
                seconds: 0,
                cut: false,
                waiting: false,
            };
        };
        let ended = st.activity.end.is_some();
        let tail = &st.out[st.read..];
        let upto = match std::str::from_utf8(tail) {
            Ok(_) => tail.len(),
            Err(e) if e.error_len().is_none() && !ended => e.valid_up_to(),
            Err(_) => tail.len(),
        };
        let new = String::from_utf8_lossy(&tail[..upto]).into_owned();
        st.read += upto;
        Said {
            new,
            end: st.activity.end.clone(),
            seconds: self.ran(&st).as_secs(),
            cut: st.cut,
            waiting: st.activity.waiting.is_some(),
        }
    }

    /// Stops what runs it, and what waits for the person; its end is
    /// [`Hub::stop_job`]'s.
    fn halt(&self) {
        if let Some(t) = self.task.lock().ok().and_then(|mut t| t.take()) {
            t.abort();
        }
        // Whatever waited for the person's answer hears none.
        if let Ok(mut a) = self.asking.lock() {
            a.take();
        }
        self.set_waiting(None);
    }
}

/// One connection per host, made once and shared by the jobs on it.
type Slot = Arc<tokio::sync::Mutex<Option<Arc<ExecConnection>>>>;

struct Inner {
    state: AiState,
    /// By spec.
    hosts: BTreeMap<String, Host>,
    jobs: BTreeMap<String, Arc<Job>>,
    slots: BTreeMap<String, Slot>,
    next: u64,
    /// The last ended commands, oldest first.
    activity: Vec<Activity>,
    /// Where they are kept on this computer, if anywhere.
    log: Option<log::Log>,
    /// Commands the person allowed for the session: client, host, command.
    allowed: Vec<(String, String, String)>,
    /// Where notes are read from (the vault), if anywhere.
    notes: Option<Arc<dyn notes::NoteStore>>,
    /// shellwing is locked: no note goes out.
    locked: bool,
    /// Where snippets are read from (the vault), if anywhere.
    snippets: Option<Arc<dyn snippets::SnippetStore>>,
    /// sudo's passwords kept a while, by client and host (sudo.rs).
    sudo: BTreeMap<(String, String), sudo::Kept>,
}

/// The MCP server's side in the app: who may reach what, and the work.
pub struct Hub {
    rt: Handle,
    secrets: Arc<dyn Secrets>,
    policy: HostKeyPolicy,
    keepalive: Duration,
    inner: Mutex<Inner>,
    events: async_channel::Sender<HubEvent>,
}

/// A grant as a client sees it: the host's name and how it may use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Given {
    pub spec: String,
    pub name: String,
    pub detail: String,
    pub grant: AiGrant,
}

impl Hub {
    /// `log`: the activity's file (`state/ai-activity.jsonl`), read back now.
    pub fn new(
        rt: Handle,
        secrets: Arc<dyn Secrets>,
        ssh: &shellwing_config::Ssh,
        log: Option<PathBuf>,
        events: async_channel::Sender<HubEvent>,
    ) -> Arc<Hub> {
        let (log, activity) = match log.map(log::Log::open) {
            Some((log, activity)) => (Some(log), activity),
            None => (None, vec![]),
        };
        let next = next_after(&activity);
        Arc::new(Hub {
            rt,
            secrets,
            policy: crate::target::policy(ssh.host_key),
            keepalive: Duration::from_secs(ssh.keepalive),
            inner: Mutex::new(Inner {
                state: AiState::default(),
                hosts: BTreeMap::new(),
                jobs: BTreeMap::new(),
                slots: BTreeMap::new(),
                next,
                activity,
                log,
                allowed: vec![],
                notes: None,
                locked: false,
                snippets: None,
                sudo: BTreeMap::new(),
            }),
            events,
        })
    }

    /// The clients and grants, as saved or changed: work on a host no
    /// longer given, or paused, stops (what waits for the person too), and
    /// its connection closes.
    pub fn set_state(&self, state: AiState) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.state = state;
        let mb = inner
            .state
            .activity_mb
            .unwrap_or(shellwing_config::AI_ACTIVITY_MB)
            .max(log::LEAST_MB);
        if let Some(log) = &mut inner.log {
            log.set_most(u64::from(mb) * 1024 * 1024);
        }
        let paused: Vec<&str> = inner
            .state
            .clients
            .iter()
            .filter(|c| c.paused)
            .map(|c| c.id.as_str())
            .collect();
        let given: Vec<(String, String)> = inner
            .state
            .grants
            .iter()
            .filter(|g| live(g) && !g.paused && !paused.contains(&g.client.as_str()))
            .map(|g| (g.client.clone(), g.host.clone()))
            .collect();
        let stopping: Vec<Arc<Job>> = inner
            .jobs
            .values()
            .filter(|j| !given.contains(&(j.client.clone(), j.host.clone())) && !j.ended())
            .cloned()
            .collect();
        inner
            .slots
            .retain(|spec, _| given.iter().any(|(_, h)| h == spec));
        drop(inner);
        for job in stopping {
            self.stop_job(&job);
        }
        self.keep_sudo_for_grants();
    }

    /// The hosts that can be given, by spec.
    pub fn set_hosts(&self, hosts: BTreeMap<String, Host>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.hosts = hosts;
        }
    }

    /// The client `token` belongs to.
    pub fn client(&self, token: &str) -> Option<AiClient> {
        let hash = token_hash(token);
        let inner = self.inner.lock().ok()?;
        inner
            .state
            .clients
            .iter()
            .find(|c| c.token_sha256 == hash)
            .cloned()
    }

    /// Whether the person paused the client `id`: it may use no host.
    pub fn client_paused(&self, id: &str) -> bool {
        self.inner
            .lock()
            .is_ok_and(|i| i.state.clients.iter().any(|c| c.id == id && c.paused))
    }

    /// The hosts given to `client` now, paused ones too: the grant says so.
    pub fn given(&self, client: &str) -> Vec<Given> {
        let Ok(inner) = self.inner.lock() else {
            return vec![];
        };
        inner
            .state
            .grants
            .iter()
            .filter(|g| g.client == client && live(g))
            .filter_map(|g| {
                let h = inner.hosts.get(&g.host)?;
                Some(Given {
                    spec: g.host.clone(),
                    name: h.name.clone(),
                    detail: h.detail.clone(),
                    grant: g.clone(),
                })
            })
            .collect()
    }

    /// The grant `client` holds for the host it calls `host` (its name, as
    /// [`Hub::given`] says it), and where the host is.
    fn grant_for(&self, client: &AiClient, host: &str) -> Result<(Given, Resolved), String> {
        let g = self.given_for(client, host)?;
        let target = {
            let inner = self.inner.lock().map_err(|_| "busy".to_string())?;
            inner
                .hosts
                .get(&g.spec)
                .map(|h| h.target.clone())
                .ok_or_else(|| format!("{host} is no longer in the host list"))?
        }?;
        Ok((g, target))
    }

    /// The grant `client` holds for `host`, in force: [`Hub::grant_for`]
    /// without reaching the host.
    fn given_for(&self, client: &AiClient, host: &str) -> Result<Given, String> {
        let given = self.given(&client.id);
        let Some(g) = given
            .iter()
            .find(|g| g.name == host)
            .or_else(|| given.iter().find(|g| g.spec == host))
        else {
            return Err(format!(
                "{host} is not one of the hosts given to {}: list_hosts says which are",
                client.name
            ));
        };
        if self.client_paused(&client.id) {
            return Err(paused_client(&client.name));
        }
        if g.grant.paused {
            return Err(format!(
                "{} is paused for {} in shellwing. Nothing ran. Ask the user to resume it in Settings › AI.",
                g.name, client.name
            ));
        }
        Ok(g.clone())
    }

    /// A new job `label` on `g`'s host, waiting for the person if `asks`;
    /// its host's connection slot, and where the person's answer comes.
    #[allow(clippy::type_complexity)]
    fn begin(
        &self,
        client: &AiClient,
        g: &Given,
        label: &str,
        tab: bool,
        asks: Option<String>,
    ) -> Result<
        (
            Arc<Job>,
            Slot,
            Option<tokio::sync::oneshot::Receiver<Approval>>,
        ),
        String,
    > {
        let (job, slot) = {
            let mut inner = self.inner.lock().map_err(|_| "busy".to_string())?;
            let id = format!("j-{}", inner.next);
            inner.next += 1;
            let mut job = Job::new(id.clone(), client, &g.spec, &g.name, label);
            job.tab = tab;
            let job = Arc::new(job);
            inner.jobs.insert(id, job.clone());
            // The ended ones beyond the activity's length are let go.
            if inner.jobs.len() > ACTIVITY {
                let old: Vec<String> = inner
                    .jobs
                    .iter()
                    .filter(|(_, j)| j.ended())
                    .map(|(id, _)| id.clone())
                    .take(inner.jobs.len() - ACTIVITY)
                    .collect();
                for id in old {
                    inner.jobs.remove(&id);
                }
            }
            let slot = inner.slots.entry(g.spec.clone()).or_default().clone();
            (job, slot)
        };
        let answer = asks.map(|why| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Ok(mut a) = job.asking.lock() {
                *a = Some(tx);
            }
            job.set_waiting(Some(why));
            rx
        });
        let _ = self.events.try_send(HubEvent::Changed(job.activity()));
        Ok((job, slot, answer))
    }

    /// Whether `label`, allowed for the session, needs no asking.
    fn allowed_before(&self, client: &AiClient, spec: &str, label: &str) -> bool {
        self.inner.lock().is_ok_and(|i| {
            i.allowed
                .iter()
                .any(|(c, h, x)| *c == client.id && h == spec && x == label)
        })
    }

    /// Waits for the person's answer to `job`, if it asks; whether it goes
    /// on. Refused, it ends so.
    async fn wait_answer(
        hub: &Arc<Hub>,
        job: &Arc<Job>,
        answer: Option<tokio::sync::oneshot::Receiver<Approval>>,
        client: String,
        label: &str,
    ) -> bool {
        let Some(answer) = answer else {
            return true;
        };
        let said = answer.await.unwrap_or(Approval::Refuse);
        job.set_waiting(None);
        match said {
            Approval::Refuse => {
                hub.finish(job, End::Refused);
                return false;
            }
            Approval::Session => {
                if let Ok(mut inner) = hub.inner.lock() {
                    inner
                        .allowed
                        .push((client, job.host.clone(), label.to_string()));
                }
            }
            Approval::Once => {}
        }
        let _ = hub.events.try_send(HubEvent::Changed(job.activity()));
        true
    }

    /// Tells the app what `job` printed, a few times a second while it runs:
    /// what came last too, so a command that prints a line and then works a
    /// while shows that line at once.
    fn follow_output(&self, job: &Arc<Job>) {
        let (job, events) = (job.clone(), self.events.clone());
        self.rt.spawn(async move {
            let mut sent = 0;
            while !job.ended() {
                tokio::time::sleep(FOLLOW).await;
                let printed = job.printed();
                if printed != sent && !job.ended() {
                    sent = printed;
                    let _ = events.try_send(HubEvent::Changed(job.activity()));
                }
            }
        });
    }

    /// Where `job`'s connection asks about a host key not in known_hosts:
    /// `job` waits for the person in AI activity, with the key, and the
    /// key is trusted if they say so. Anything else asked is refused.
    fn ask_host_key(self: &Arc<Self>, job: &Arc<Job>) -> Prompter {
        let (prompter, mut asked) = Prompter::new();
        let (hub, job) = (Arc::downgrade(self), job.clone());
        self.rt.spawn(async move {
            while let Some(q) = asked.recv().await {
                let answer = match &q.prompt {
                    Prompt::HostKey {
                        host,
                        algorithm,
                        fingerprint,
                        again: false,
                    } => {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        if let Ok(mut a) = job.asking.lock() {
                            *a = Some(tx);
                        }
                        job.set_waiting(Some(format!(
                            "{HOST_KEY}{host}\n{algorithm} {fingerprint}"
                        )));
                        let tell = |job: &Job| {
                            if let Some(hub) = hub.upgrade() {
                                let _ = hub.events.try_send(HubEvent::Changed(job.activity()));
                            }
                        };
                        tell(&job);
                        let said = rx.await.unwrap_or(Approval::Refuse);
                        job.set_waiting(None);
                        tell(&job);
                        Some(
                            if said == Approval::Refuse {
                                "no"
                            } else {
                                "yes"
                            }
                            .to_string(),
                        )
                    }
                    // A password, a code: not asked, as without a prompter.
                    _ => None,
                };
                let _ = q.reply.send(answer);
            }
        });
        prompter
    }

    /// The host's connection, made if it is not.
    async fn connection(slot: &Slot, options: Options) -> Result<Arc<ExecConnection>, String> {
        let mut slot = slot.lock().await;
        match slot.clone() {
            Some(c) => Ok(c),
            None => match shellwing_ssh::connect_exec(options).await {
                Ok(c) => {
                    let c = Arc::new(c);
                    *slot = Some(c.clone());
                    Ok(c)
                }
                Err(e) => Err(e.to_string()),
            },
        }
    }

    /// Starts `command` on the host `client` calls `host` (its name, as
    /// [`Hub::given`] says it), if it was given that host to run commands.
    pub fn run(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        command: &str,
    ) -> Result<Arc<Job>, String> {
        self.run_with(client, host, command, RunWith::default())
    }

    /// [`Hub::run`]; waiting for the person whatever the mode when
    /// `always` says why, kept as the run of `snippet` when it is one, and
    /// run with `envs` when it needs environments.
    fn run_with(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        command: &str,
        with: RunWith,
    ) -> Result<Arc<Job>, String> {
        let RunWith {
            always,
            snippet,
            envs,
            sudo,
        } = with;
        let (g, target) = self.grant_for(client, host)?;
        if !g.grant.run {
            return Err(format!(
                "{} is given to {} to read files, not to run commands",
                g.name, client.name
            ));
        }
        let asks = match g.grant.mode {
            _ if always.is_some() => always,
            _ if self.allowed_before(client, &g.spec, command) => None,
            AiMode::Free => None,
            AiMode::Ask => Some(String::new()),
            AiMode::Risky => {
                let words = self
                    .inner
                    .lock()
                    .map(|i| i.state.risky())
                    .unwrap_or_default();
                risky(command, &words)
            }
        };
        let (job, slot, answer) =
            self.begin(client, &g, command, g.grant.how == AiHow::Tab, asks)?;
        if let Some(name) = snippet {
            job.set_snippet(name);
            let _ = self.events.try_send(HubEvent::Changed(job.activity()));
        }
        let options = self.options(target, &job);
        let (hub, running, command) = (self.clone(), job.clone(), command.to_string());
        let client_id = client.id.clone();
        let task = self.rt.spawn(async move {
            if !Hub::wait_answer(&hub, &running, answer, client_id, &command).await {
                return;
            }
            // Through a tab: the app types it, and says what came of it.
            if running.tab {
                let _ = hub.events.try_send(HubEvent::Tab {
                    id: running.id.clone(),
                    spec: running.host.clone(),
                    command: command.clone(),
                    client: running.activity().client,
                    needs: envs.map(|e| e.ids).unwrap_or_default(),
                });
                return;
            }
            // With environments: the values on the input, the names alone
            // on the command line. With sudo's password: on the input too,
            // read by sudo as its askpass.
            let with_sudo = sudo.is_some();
            let (run, input) = match (&envs, sudo) {
                (Some(e), _) => (e.command.clone(), e.input.clone()),
                (None, Some(s)) => {
                    let password = match s {
                        sudo::SudoRun::Kept(p) => Some(p),
                        sudo::SudoRun::Asked => running.sudo.lock().ok().and_then(|mut g| g.take()),
                    };
                    let Some(password) = password else {
                        hub.finish(
                            &running,
                            End::Failed("no password was given for sudo".into()),
                        );
                        return;
                    };
                    crate::run::invocation(
                        &command,
                        &crate::run::Interpreter::Shell {
                            program: "sh".into(),
                            flags: vec![],
                        },
                        Some(&password),
                    )
                }
                (None, None) => (command.clone(), Default::default()),
            };
            let end = match Hub::connection(&slot, options).await {
                Err(why) => End::Failed(why),
                Ok(c) => {
                    let printing = running.clone();
                    hub.follow_output(&running);
                    let status = c.stream(&run, &input, |_, data| printing.print(data)).await;
                    // A password sudo turned down is kept no longer.
                    if with_sudo && sudo::refused(&running.activity().output) {
                        hub.sudo_refused(&running.client, &running.host);
                    }
                    match status {
                        Ok(s) => End::Exited(s),
                        Err(e) => {
                            // The connection is likely gone: the next connects again.
                            *slot.lock().await = None;
                            End::Failed(e.to_string())
                        }
                    }
                }
            };
            hub.finish(&running, end);
        });
        if let Ok(mut t) = job.task.lock() {
            *t = Some(task.abort_handle());
        }
        Ok(job)
    }

    /// Starts `op` on the host `client` calls `host`, over SFTP on its own
    /// connection (a host given through a tab too), if the grant lets it:
    /// reading for a list or a read, writing for a write. Its output is what
    /// the agent reads.
    pub fn file(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        op: FileOp,
    ) -> Result<Arc<Job>, String> {
        let (g, target) = self.grant_for(client, host)?;
        let writes = matches!(op, FileOp::Write { .. });
        if writes && !g.grant.write {
            return Err(format!(
                "{} is not given to {} to write files",
                g.name, client.name
            ));
        }
        if !writes && !g.grant.read {
            return Err(format!(
                "{} is not given to {} to read files",
                g.name, client.name
            ));
        }
        let label = op.label();
        let asks = match g.grant.mode {
            _ if self.allowed_before(client, &g.spec, &label) => None,
            AiMode::Free => None,
            AiMode::Ask => Some(String::new()),
            // A write is risky: it changes the host, and is not undone.
            AiMode::Risky if writes => Some(WRITES.to_string()),
            AiMode::Risky => None,
        };
        let (job, slot, answer) = self.begin(client, &g, &label, false, asks)?;
        let options = self.options(target, &job);
        let (hub, running) = (self.clone(), job.clone());
        let client_id = client.id.clone();
        let task = self.rt.spawn(async move {
            if !Hub::wait_answer(&hub, &running, answer, client_id, &label).await {
                return;
            }
            let done = async {
                let c = Hub::connection(&slot, options).await?;
                let sftp = c.sftp().await.map_err(|e| e.to_string())?;
                files::serve(&sftp, op).await
            }
            .await;
            let end = match done {
                Ok(text) => {
                    running.print(text.as_bytes());
                    End::Exited(Some(0))
                }
                Err(why) => End::Failed(why),
            };
            hub.finish(&running, end);
        });
        if let Ok(mut t) = job.task.lock() {
            *t = Some(task.abort_handle());
        }
        Ok(job)
    }

    /// The person's answer to job `id`, which waits for them.
    pub fn answer(&self, id: &str, approval: Approval) {
        let job = self.inner.lock().ok().and_then(|i| i.jobs.get(id).cloned());
        if let Some(tx) = job.and_then(|j| j.asking.lock().ok().and_then(|mut a| a.take())) {
            let _ = tx.send(approval);
        }
    }

    /// Stops job `id`, whoever's it is: the person's Stop.
    pub fn stop(&self, id: &str) {
        let job = self.inner.lock().ok().and_then(|i| i.jobs.get(id).cloned());
        if let Some(j) = job.filter(|j| !j.ended()) {
            self.stop_job(&j);
        }
    }

    /// Stops `job`: in its tab too, if it was typed there.
    pub fn stop_job(&self, job: &Job) {
        let was_typed = job.tab && !job.waiting();
        job.halt();
        if was_typed {
            let _ = self
                .events
                .try_send(HubEvent::TabStop { id: job.id.clone() });
        }
        self.finish(job, End::Stopped);
    }

    /// What job `id`, typed into a tab, printed there: plain text.
    pub fn tab_print(&self, id: &str, text: &str) {
        let job = self.inner.lock().ok().and_then(|i| i.jobs.get(id).cloned());
        if let Some(j) = job.filter(|j| !j.ended()) {
            j.print(text.as_bytes());
            let _ = self.events.try_send(HubEvent::Changed(j.activity()));
        }
    }

    /// Job `id`, typed into a tab, ended so.
    pub fn tab_end(&self, id: &str, end: End) {
        let job = self.inner.lock().ok().and_then(|i| i.jobs.get(id).cloned());
        if let Some(j) = job.filter(|j| !j.ended()) {
            self.finish(&j, end);
        }
    }

    /// `client`'s job `id`.
    pub fn job(&self, client: &str, id: &str) -> Option<Arc<Job>> {
        let inner = self.inner.lock().ok()?;
        inner.jobs.get(id).filter(|j| j.client == client).cloned()
    }

    /// Stops every job going: the person's Stop all.
    pub fn stop_all(&self) {
        let jobs: Vec<Arc<Job>> = self
            .inner
            .lock()
            .map(|i| i.jobs.values().filter(|j| !j.ended()).cloned().collect())
            .unwrap_or_default();
        for j in jobs {
            self.stop_job(&j);
        }
    }

    /// The activity, newest last: what was logged and what runs now.
    pub fn activity(&self) -> Vec<Activity> {
        let Ok(inner) = self.inner.lock() else {
            return vec![];
        };
        let mut all = inner.activity.clone();
        all.extend(
            inner
                .jobs
                .values()
                .filter(|j| !j.ended())
                .map(|j| j.activity()),
        );
        all
    }

    /// A second shellwing asks this one to come forward, with its `args`.
    pub fn show(&self, args: Vec<String>) {
        let _ = self.events.try_send(HubEvent::Show { args });
    }

    pub fn client_connected(&self, id: &str, connected: bool) {
        let _ = self.events.try_send(HubEvent::Client {
            id: id.to_string(),
            connected,
        });
    }

    /// Ends `job` so: kept in the activity first, then told to whoever
    /// waits for it, so that what they ask next finds it there.
    fn finish(&self, job: &Job, end: End) {
        job.set_end(end);
        self.ended(job);
        job.changed.notify_waiters();
    }

    /// A job ended: kept in the activity, and the log.
    fn ended(&self, job: &Job) {
        let mut a = job.activity();
        let _ = self.events.try_send(HubEvent::Changed(a.clone()));
        if a.output.len() > LOGGED {
            let mut at = LOGGED;
            while !a.output.is_char_boundary(at) {
                at -= 1;
            }
            a.output.truncate(at);
        }
        if let Ok(mut inner) = self.inner.lock() {
            if inner.activity.iter().any(|x| x.id == a.id) {
                return;
            }
            if let (Some(log), Ok(line)) = (&mut inner.log, serde_json::to_string(&a)) {
                log.add(&line);
            }
            inner.activity.push(a);
            let over = inner.activity.len().saturating_sub(ACTIVITY);
            inner.activity.drain(..over);
        }
    }

    /// How much the log keeps on this computer, and the most it may (bytes);
    /// None when it keeps nothing there.
    pub fn kept(&self) -> Option<(u64, u64)> {
        let inner = self.inner.lock().ok()?;
        inner.log.as_ref().map(|l| (l.size(), l.most()))
    }

    /// Where the log is written now, to show it in its folder.
    pub fn log_path(&self) -> Option<PathBuf> {
        let inner = self.inner.lock().ok()?;
        inner.log.as_ref().map(|l| l.path().to_path_buf())
    }

    /// The person's Clear: the ended commands go, the log on this computer
    /// too; what runs or waits stays.
    pub fn clear_activity(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.activity.clear();
            if let Some(log) = &mut inner.log {
                log.clear();
            }
        }
        let _ = self.events.try_send(HubEvent::Cleared);
    }

    /// How a host is reached for an AI: as a tab would, its forwards, agent
    /// and X11 left out. Nothing is asked but a key not in known_hosts, and
    /// that of the person, through `job` (where the host key setting asks):
    /// what would ask for a password or a code is refused.
    fn options(self: &Arc<Self>, mut target: Resolved, job: &Arc<Job>) -> Options {
        target.forwards.clear();
        target.forward_agent = false;
        for hop in target.jumps.iter_mut() {
            hop.connect_timeout.get_or_insert(CONNECT_TIMEOUT);
        }
        target.connect_timeout.get_or_insert(CONNECT_TIMEOUT);
        Options {
            target,
            auth: Auth {
                agent: true,
                password: None,
                agent_socket: None,
            },
            // A host never seen is trusted by the person, never the AI.
            policy: self.policy,
            known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS").map(Into::into),
            prompter: None,
            host_key_prompter: (self.policy == HostKeyPolicy::Ask).then(|| self.ask_host_key(job)),
            secrets: Some(self.secrets.clone()),
            forwarded: None,
            keepalive: Some(self.keepalive),
            size: None,
            cols: 80,
            rows: 24,
            term: "dumb".into(),
        }
    }
}

/// The words of `patterns` that make `command` risky: the first whose
/// words come in a row in it, case aside; a word of a pattern ending in
/// `/` begins one of the command's.
pub fn risky(command: &str, patterns: &[String]) -> Option<String> {
    let words: Vec<String> = command
        .split(|c: char| c.is_whitespace() || ";&|()`".contains(c))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches(|c| c == '"' || c == '\'').to_lowercase())
        .collect();
    patterns.iter().find_map(|p| {
        let want: Vec<String> = p.split_whitespace().map(str::to_lowercase).collect();
        if want.is_empty() || want.len() > words.len() {
            return None;
        }
        let fits = |w: &str, p: &str| w == p || (p.ends_with('/') && w.starts_with(p));
        words
            .windows(want.len())
            .any(|win| win.iter().zip(&want).all(|(w, p)| fits(w, p)))
            .then(|| p.trim().to_string())
    })
}

/// Whether a grant holds now.
/// What a paused client is told, whatever it asks.
/// What a run may come with, besides its command (see [`Hub::run`]).
#[derive(Default)]
pub(crate) struct RunWith {
    /// It waits for the person whatever the grant's mode: why.
    pub always: Option<String>,
    /// The snippet it is, by name.
    pub snippet: Option<String>,
    /// The environments it runs with, their values on its input.
    pub envs: Option<snippets::EnvRun>,
    /// sudo's password, kept or to be given.
    pub sudo: Option<sudo::SudoRun>,
}

/// The number of the next job after those in `activity`: the activity
/// read back from the log goes on from where it was, so a new job's id is
/// never one it has already, which would put it in that one's place and
/// leave it out of the log.
fn next_after(activity: &[Activity]) -> u64 {
    activity
        .iter()
        .filter_map(|a| a.id.strip_prefix("j-")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1)
}

pub fn paused_client(name: &str) -> String {
    format!(
        "{name} is paused in shellwing: no host can be used until the user resumes it in Settings › AI."
    )
}

fn live(g: &AiGrant) -> bool {
    g.until.is_none_or(|u| now() < u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> AiClient {
        AiClient {
            id: "c1".into(),
            kind: "claude-code".into(),
            name: "Claude Code".into(),
            token_sha256: token_hash("shw_x"),
            added_at: 0,
            paused: false,
        }
    }

    /// Opened again, the hub goes on numbering its jobs after those its log
    /// kept: a new job is kept beside them, last, and logged.
    #[tokio::test]
    async fn jobs_go_on_from_the_log() {
        let dir = std::env::temp_dir().join(format!("shellwing-ai-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("ai-activity.jsonl");
        let lines: String = ["j-1", "j-7", "j-2"]
            .iter()
            .map(|id| {
                let a = Activity {
                    id: id.to_string(),
                    client: "c".into(),
                    host: "h".into(),
                    command: "true".into(),
                    started: 0,
                    ended: Some(0),
                    end: Some(End::Exited(Some(0))),
                    output: String::new(),
                    waiting: None,
                    adding: None,
                    snippet: None,
                    client_id: None,
                    spec: None,
                    ran: None,
                };
                serde_json::to_string(&a).expect("json") + "\n"
            })
            .collect();
        std::fs::write(&path, lines).expect("log");
        struct None_;
        impl Secrets for None_ {
            fn password(&self, _: &str) -> Option<String> {
                None
            }
            fn passphrase(&self, _: &str) -> Option<String> {
                None
            }
            fn learned(&self, _: shellwing_ssh::Learned) {}
        }
        let (tx, _rx) = async_channel::unbounded();
        let hub = Hub::new(
            Handle::current(),
            Arc::new(None_),
            &shellwing_config::Ssh::default(),
            Some(path.clone()),
            tx,
        );
        let id = {
            let mut inner = hub.inner.lock().expect("lock");
            let id = format!("j-{}", inner.next);
            inner.next += 1;
            id
        };
        assert_eq!(id, "j-8", "after the highest kept");
        let job = Job::new(id, &client(), "h", "h", "echo new");
        hub.finish(&job, End::Exited(Some(0)));
        let ids: Vec<String> = hub.activity().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["j-1", "j-7", "j-2", "j-8"], "kept, last");
        let logged = std::fs::read_to_string(&path).expect("log");
        assert!(logged.contains("\"echo new\""), "logged: {logged}");
        assert_eq!(next_after(&[]), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_token_is_kept_as_its_hash() {
        let t = new_token();
        assert!(t.starts_with("shw_") && t.len() == 4 + 64, "{t}");
        assert_ne!(t, new_token());
        assert_eq!(token_hash(&t), token_hash(&format!(" {t}\n")));
        assert_ne!(token_hash(&t), t);
    }

    #[tokio::test]
    async fn a_job_says_what_is_new_and_waits_for_more() {
        let job = Job::new("j-1".into(), &client(), "web", "web", "x");
        job.print(b"one\n");
        let s = job.wait(Duration::from_secs(5), false).await;
        assert_eq!((s.new.as_str(), s.end.clone()), ("one\n", None));
        // Nothing new: it waits, and says nothing new.
        let s = job.wait(Duration::from_millis(50), false).await;
        assert_eq!(s.new, "");
        // A character cut in two waits for its other half.
        job.print(&"é".as_bytes()[..1]);
        assert_eq!(job.wait(Duration::from_millis(20), false).await.new, "");
        job.print(&"é".as_bytes()[1..]);
        job.print(b"!");
        assert_eq!(job.wait(Duration::from_secs(1), false).await.new, "é!");
        job.end(End::Exited(Some(3)));
        let s = job.wait(Duration::from_secs(1), true).await;
        assert_eq!(s.end, Some(End::Exited(Some(3))));
        assert_eq!(job.activity().output, "one\né!");
    }

    #[tokio::test]
    async fn waiting_for_the_end_takes_what_came_meanwhile() {
        let job = Arc::new(Job::new("j-1".into(), &client(), "web", "web", "x"));
        let later = job.clone();
        tokio::spawn(async move {
            later.print(b"a");
            tokio::time::sleep(Duration::from_millis(30)).await;
            later.print(b"b");
            later.end(End::Exited(Some(0)));
        });
        let s = job.wait(Duration::from_secs(5), true).await;
        assert_eq!((s.new.as_str(), s.end), ("ab", Some(End::Exited(Some(0)))));
    }

    #[test]
    fn what_makes_a_command_risky() {
        let p = shellwing_config::AiState::default().risky();
        assert_eq!(
            risky("sudo rm -rf /var/cache/app", &p).as_deref(),
            Some("rm -rf")
        );
        assert_eq!(risky("cd /x && RM -RF tmp", &p).as_deref(), Some("rm -rf"));
        assert_eq!(risky("echo hi > /etc/motd", &p).as_deref(), Some("> /etc/"));
        assert_eq!(risky("psql -c 'DROP TABLE x'", &p).as_deref(), Some("drop"));
        assert_eq!(
            risky("sudo systemctl stop nginx", &p).as_deref(),
            Some("systemctl stop")
        );
        assert_eq!(risky("df -h / && free -m", &p), None);
        assert_eq!(
            risky("cat ddrescue.log", &p),
            None,
            "dd is a word, not a part of one"
        );
        assert_eq!(risky("systemctl status nginx", &p), None);
        assert_eq!(risky("anything", &[]), None);
    }

    /// How long it ran stops growing once it ended: asked later, it says
    /// the same.
    #[tokio::test]
    async fn its_time_is_kept_at_its_end() {
        let job = Job::new("j-1".into(), &client(), "web", "web", "x");
        tokio::time::sleep(Duration::from_millis(30)).await;
        job.end(End::Exited(Some(0)));
        let took = job.ran(&job.state.lock().expect("state"));
        assert!(took >= Duration::from_millis(30), "{took:?}");
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(job.ran(&job.state.lock().expect("state")), took);
    }

    #[tokio::test]
    async fn a_job_that_waits_says_so() {
        let job = Job::new("j-1".into(), &client(), "web", "web", "x");
        job.set_waiting(Some("it asks".into()));
        let s = job.wait(Duration::from_millis(20), true).await;
        assert!(s.waiting && s.end.is_none());
        job.set_waiting(None);
        job.end(End::Refused);
        let s = job.wait(Duration::from_millis(20), true).await;
        assert_eq!((s.waiting, s.end), (false, Some(End::Refused)));
    }

    #[tokio::test]
    async fn the_log_keeps_what_the_person_chose_at_least_its_least() {
        struct None_;
        impl Secrets for None_ {
            fn password(&self, _: &str) -> Option<String> {
                None
            }
            fn passphrase(&self, _: &str) -> Option<String> {
                None
            }
            fn learned(&self, _: shellwing_ssh::Learned) {}
        }
        let dir = std::env::temp_dir().join(format!("shellwing-ai-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = async_channel::unbounded();
        let hub = Hub::new(
            Handle::current(),
            Arc::new(None_),
            &shellwing_config::Ssh::default(),
            Some(dir.join("ai-activity.jsonl")),
            tx,
        );
        let mb = 1024 * 1024;
        hub.set_state(AiState::default());
        assert_eq!(hub.kept(), Some((0, 32 * mb)), "32 MB unless chosen");
        hub.set_state(AiState {
            activity_mb: Some(1),
            ..AiState::default()
        });
        assert_eq!(hub.kept(), Some((0, 32 * mb)), "never less than 32");
        hub.set_state(AiState {
            activity_mb: Some(500),
            ..AiState::default()
        });
        assert_eq!(hub.kept(), Some((0, 500 * mb)), "as much as chosen");
        assert_eq!(hub.log_path(), Some(dir.join("ai-activity.jsonl")));

        hub.clear_activity();
        assert!(hub.activity().is_empty());
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, HubEvent::Cleared)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

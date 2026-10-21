//! One connection, shown in one pane: its terminal, where typed input goes, and
//! the network task. The terminal outlives connections: reconnecting starts a
//! new network task that writes below what the old one left. Dropping a
//! `Session` disconnects and frees all of it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use shellwing_config::Host;
use shellwing_ssh::{
    Active, Auth, Ended, Error, Forward, Input, Options, Prompter, Resolved, Sftp,
};
use shellwing_term::{Event, TermHandle};
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

use crate::Writer;
use crate::bench::{self, Probe};
use crate::secrets::{SessionSecrets, VaultSlot};
use shellwing_ssh::{Learned, Secrets};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    /// The shell ended (`exit`), or the server closed the session.
    Closed,
    /// The network broke; trying again may work.
    Lost,
    /// It could not connect for a reason retrying will not fix: refused
    /// sign-in, a host key, a bad address, a cancelled prompt.
    Failed,
}

/// Errors worth retrying on their own: the network, not the person.
/// A changed host key, the target's or a jump host's.
fn changed_key(e: &Error) -> Option<&shellwing_ssh::ChangedKey> {
    match e {
        Error::HostKeyChanged(c) => Some(c),
        Error::Jump { source, .. } => changed_key(source),
        _ => None,
    }
}

fn retryable(e: &Error) -> bool {
    match e {
        Error::Ssh(_) | Error::Disconnected => true,
        Error::Jump { source, .. } => retryable(source),
        _ => false,
    }
}

/// Status and history, shared with the network task.
#[derive(Default)]
struct State {
    status: Option<Status>,
    /// Counts status changes, so a watcher that wakes late still sees that one
    /// happened (Connecting then Lost again looks like no change by value).
    changes: u64,
    /// Some connection succeeded at least once.
    ever_connected: bool,
    /// How the last connection ended, for the banner.
    ended: Option<String>,
    /// The last attempt found the host's key changed: what was known, and
    /// what the server showed.
    changed_key: Option<shellwing_ssh::ChangedKey>,
}

pub struct Session {
    pub term: TermHandle,
    pub writer: Writer,
    /// For the tab: the host's name or address.
    pub title: String,
    /// What was asked for (a hosts.toml name, an alias, `user@host`), to open again.
    pub spec: String,
    /// The hosts.toml entry, whose theme wins over the app's.
    pub entry: Option<String>,
    target: Option<Resolved>,
    rt: Handle,
    state: Arc<Mutex<State>>,
    task: Option<AbortHandle>,
    wake_tx: async_channel::Sender<()>,
    /// A new frame (or a status change) is ready.
    pub wake: async_channel::Receiver<()>,
    pub events: async_channel::Receiver<Event>,
    /// Passwords and passphrases typed at a prompt that worked, to offer saving.
    pub learned: async_channel::Receiver<Learned>,
    secrets: Arc<dyn Secrets>,
    vault: VaultSlot,
    /// The forwards the current connection runs.
    held: Weak<Held>,
    /// The app's defaults for hosts that set none.
    ssh: shellwing_config::Ssh,
    /// Typed into the shell once it is ready (the host's `run`).
    run: Option<String>,
    /// A shell on this computer instead of a host: its program and arguments.
    local: Option<Vec<String>>,
    /// The file the terminal is logged to.
    log: Option<PathBuf>,
}

/// What a Files panel needs from its pane's connection.
#[derive(Clone)]
pub struct Remote {
    writer: Writer,
    /// The network runtime, where SFTP work runs.
    pub rt: Handle,
}

impl Remote {
    /// Opens SFTP on the running connection. None when nothing is connected.
    pub fn sftp(&self) -> Option<tokio::sync::oneshot::Receiver<Result<Sftp, Error>>> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.writer.send(Input::Sftp { reply }).then_some(answer)
    }

    /// Runs `command` on the running connection, beside the shell. None when
    /// nothing is connected.
    pub fn exec(
        &self,
        command: &str,
        stdin: Vec<u8>,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<shellwing_ssh::Exec, Error>>> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.writer
            .send(Input::Exec {
                command: command.into(),
                stdin,
                reply,
            })
            .then_some(answer)
    }
}

/// Forwards running in this app, as specs. A second tab to a host finds the
/// ports the first one forwards taken; that is said quietly, not as an error.
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The forwards of one connection, in [`RUNNING`] until it is dropped with the
/// connection.
#[derive(Default)]
struct Held(Mutex<Vec<String>>);

impl Held {
    fn add(&self, active: &Active) {
        let spec = active.running().to_string();
        if let Ok(mut r) = RUNNING.lock() {
            r.push(spec.clone());
        }
        if let Ok(mut h) = self.0.lock() {
            h.push(spec);
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let mine = self.0.get_mut().map(std::mem::take).unwrap_or_default();
        if let Ok(mut r) = RUNNING.lock() {
            for spec in mine {
                if let Some(i) = r.iter().position(|s| *s == spec) {
                    r.remove(i);
                }
            }
        }
    }
}

fn running_elsewhere(spec: &str) -> bool {
    RUNNING.lock().is_ok_and(|r| r.iter().any(|s| s == spec))
}

/// How a forward went, on a line of its own below the connecting line.
fn forwarded(term: &TermHandle, held: &Held, result: Result<Active, Error>) {
    match result {
        Ok(active) => {
            term.feed(format!("\x1b[2m[{active}]\x1b[0m\r\n").into_bytes());
            held.add(&active);
        }
        Err(Error::Forward { spec, .. }) if running_elsewhere(&spec) => {
            term.feed(format!("\x1b[2m[{spec}: forwarded by another tab]\x1b[0m\r\n").into_bytes())
        }
        Err(e) => term.feed(format!("\x1b[31m{e}\x1b[0m\r\n").into_bytes()),
    }
}

/// A dimmed status line written into the terminal itself.
fn status_line(term: &TermHandle, msg: &str) {
    term.feed(format!("\r\n\x1b[2m[{msg}]\x1b[0m\r\n").into_bytes());
}

/// An error line, in red.
fn failure(term: &TermHandle, msg: &str) {
    term.feed(format!("\r\n\x1b[31m{msg}\x1b[0m\r\n").into_bytes());
}

/// What a pane opens.
pub enum Opens {
    /// An SSH host, as resolved from the spec (or why it could not be).
    Host(Box<anyhow::Result<(Resolved, Option<Host>)>>),
    /// A shell on this computer: the program and its arguments.
    Local(Vec<String>),
}

impl Session {
    /// Starts connecting to `target` (resolved from `spec`) on `rt`. A target that
    /// failed to resolve still gets a session, which shows the error. The
    /// session is logged from the start when the settings or the host say so.
    pub fn start(
        spec: &str,
        opens: Opens,
        settings: &shellwing_config::Settings,
        rt: &Handle,
        bench: Option<(String, Arc<Probe>)>,
        vault: VaultSlot,
    ) -> std::io::Result<Session> {
        let (writer, shown) = Writer::new();
        let (learned_tx, learned) = async_channel::unbounded();
        // The terminal thread wakes the UI through this; one pending wake is enough.
        let (wake_tx, wake) = async_channel::bounded::<()>(1);
        // Events the UI has to act on, such as copied text.
        let (event_tx, events) = async_channel::unbounded::<Event>();
        let term = {
            let (w, wake_tx) = (writer.clone(), wake_tx.clone());
            TermHandle::spawn(
                80,
                24,
                settings.terminal.scrollback,
                Box::new(move |bytes| w.write(bytes)),
                Box::new(move || {
                    let _ = wake_tx.try_send(());
                }),
                Box::new(move |ev| {
                    let _ = event_tx.try_send(ev);
                }),
            )?
        };
        // Prompt text and echo, shown from their own thread (see Writer). It ends
        // when the last Writer is dropped.
        {
            let term = term.clone();
            std::thread::Builder::new()
                .name("shellwing-echo".into())
                .spawn(move || {
                    for bytes in shown {
                        term.feed(bytes);
                    }
                })?;
        }
        let mut session = Session {
            term,
            writer,
            title: spec.to_string(),
            spec: spec.to_string(),
            entry: None,
            target: None,
            rt: rt.clone(),
            state: Arc::default(),
            task: None,
            wake_tx,
            wake,
            events,
            learned,
            secrets: Arc::new(SessionSecrets {
                vault: vault.clone(),
                learned: learned_tx,
            }),
            vault,
            held: Weak::new(),
            ssh: settings.ssh.clone(),
            run: None,
            local: None,
            log: None,
        };
        let target = match opens {
            Opens::Host(target) => *target,
            Opens::Local(argv) => {
                session.title = crate::local::TITLE.into();
                session.local = Some(argv);
                if settings.log.always {
                    session.log_from_start(&settings.log);
                }
                session.connect(bench);
                return Ok(session);
            }
        };
        match target {
            Ok((resolved, entry)) => {
                session.run = entry
                    .as_ref()
                    .and_then(|h| h.run.clone())
                    .filter(|r| !r.trim().is_empty());
                session.title = resolved.alias.clone();
                let logged = entry.as_ref().and_then(|h| h.log);
                session.entry = entry.map(|h| h.name);
                session.target = Some(resolved);
                if logged.unwrap_or(settings.log.always) {
                    session.log_from_start(&settings.log);
                }
                session.connect(bench);
            }
            Err(e) => {
                failure(&session.term, &e.to_string());
                session.set_status(Status::Failed, Some(e.to_string()));
            }
        }
        Ok(session)
    }

    fn set_status(&self, status: Status, ended: Option<String>) {
        set_status(&self.state, &self.wake_tx, status, ended);
    }

    /// Logs the terminal from now on, to a new file in the settings' folder.
    /// Returns the file.
    pub fn start_log(&mut self, settings: &shellwing_config::Log) -> std::io::Result<PathBuf> {
        let (name, about) = match &self.local {
            Some(argv) => (
                crate::local::TITLE.to_string(),
                format!("{} on this computer", crate::local::shell_name(argv)),
            ),
            None => {
                let name = self.entry.clone().unwrap_or_else(|| self.title.clone());
                let about = match self.reached() {
                    Some(t) => format!("{name} ({}@{}:{})", t.user, t.host, t.port),
                    None => name.clone(),
                };
                (name, about)
            }
        };
        let (path, log) = crate::logs::open(settings, &name, &about)?;
        self.term.log(Some(log));
        self.log = Some(path.clone());
        Ok(path)
    }

    /// Before connecting: a log that cannot start says why in the pane.
    fn log_from_start(&mut self, settings: &shellwing_config::Log) {
        if let Err(e) = self.start_log(settings) {
            failure(&self.term, &format!("Not logged: {e}"));
        }
    }

    /// Stops logging; the file it was.
    pub fn stop_log(&mut self) -> Option<PathBuf> {
        self.term.log(None);
        self.log.take()
    }

    /// The file the terminal is logged to.
    pub fn log_path(&self) -> Option<&Path> {
        self.log.as_deref()
    }

    /// Saves a typed password or passphrase in the vault.
    pub fn save(&self, secret: &Learned) -> Result<(), String> {
        self.vault.save(secret)
    }

    pub fn status(&self) -> Status {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.status)
            .unwrap_or(Status::Failed)
    }

    /// The status and how many changes led to it.
    pub fn status_change(&self) -> (u64, Status) {
        self.state
            .lock()
            .map(|s| (s.changes, s.status.unwrap_or(Status::Failed)))
            .unwrap_or((0, Status::Failed))
    }

    /// Whether a connection ever succeeded: only then is a lost one retried
    /// without asking.
    pub fn ever_connected(&self) -> bool {
        self.state.lock().is_ok_and(|s| s.ever_connected)
    }

    /// How the last connection ended or failed.
    pub fn ended(&self) -> Option<String> {
        self.state.lock().ok().and_then(|s| s.ended.clone())
    }

    /// The host's key changed at the last attempt: what was known and what
    /// the server showed.
    pub fn changed_key(&self) -> Option<shellwing_ssh::ChangedKey> {
        self.state.lock().ok().and_then(|s| s.changed_key.clone())
    }

    /// Trusts the key the server showed in place of the old one (as
    /// `ssh-keygen -R` and accepting it would), for the next attempt.
    pub fn trust_changed_key(&self) -> Result<(), String> {
        let c = self.changed_key().ok_or("no key changed")?;
        let path = shellwing_ssh::known_hosts::default_path().ok_or("no home folder")?;
        shellwing_ssh::known_hosts::replace(&path, &c.host, c.port, &c.key)
            .map_err(|e| e.to_string())
    }

    /// Starts forwarding on the running connection; the answer comes on the
    /// receiver. None when nothing is connected.
    pub fn forward(
        &self,
        forward: Forward,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<Active, Error>>> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.writer
            .send(Input::Forward { forward, reply })
            .then_some(answer)
    }

    /// Where the pane connects: user, host and port.
    pub fn reached(&self) -> Option<&shellwing_ssh::Target> {
        self.target.as_ref().map(|r| &r.target)
    }

    /// The key the vault keeps that this host signs in with, if it names one.
    pub fn vault_key(&self) -> Option<&str> {
        self.target.as_ref()?.vault_key.as_deref()
    }

    pub fn remote(&self) -> Remote {
        Remote {
            writer: self.writer.clone(),
            rt: self.rt.clone(),
        }
    }

    /// Keeps a forward started by hand: it is set up again on reconnecting.
    pub fn keep_forward(&mut self, active: &Active) {
        if let Some(t) = &mut self.target {
            t.forwards.push(active.running().to_string());
        }
        if let Some(held) = self.held.upgrade() {
            held.add(active);
        }
    }

    /// A shell on this computer, not an SSH connection.
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }

    /// Connects again to the same target, below what the terminal shows.
    pub fn reconnect(&mut self) {
        if (self.target.is_none() && self.local.is_none())
            || matches!(self.status(), Status::Connecting | Status::Connected)
        {
            return;
        }
        if let Some(t) = self.task.take() {
            t.abort();
        }
        self.writer.stop();
        self.term.reset_modes();
        self.connect(None);
    }

    /// Starts the shell on this computer, in a PTY the pane's input goes to.
    fn connect_local(&mut self, argv: Vec<String>) {
        self.set_status(Status::Connecting, None);
        self.writer.connecting();
        let (term, writer, state, wake_tx) = (
            self.term.clone(),
            self.writer.clone(),
            self.state.clone(),
            self.wake_tx.clone(),
        );
        let task = self.rt.spawn(async move {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let out = term.clone();
            let started = crate::local::start(
                argv.clone(),
                (80, 24),
                rx,
                Box::new(move |bytes| out.feed(bytes.to_vec())),
            );
            match started {
                Ok(done) => {
                    // Sends the pane's size first, then the keyboard.
                    writer.start_session(tx);
                    if let Ok(mut st) = state.lock() {
                        st.ever_connected = true;
                    }
                    set_status(&state, &wake_tx, Status::Connected, None);
                    let status = done.await.ok().flatten();
                    writer.stop();
                    let why = match status {
                        Some(s) => format!("{} ended (exit {s})", crate::local::shell_name(&argv)),
                        None => format!("{} ended", crate::local::shell_name(&argv)),
                    };
                    status_line(&term, &why);
                    set_status(&state, &wake_tx, Status::Closed, Some(why));
                }
                Err(e) => {
                    failure(&term, &e);
                    set_status(&state, &wake_tx, Status::Failed, Some(e));
                }
            }
        });
        self.task = Some(task.abort_handle());
    }

    fn connect(&mut self, bench: Option<(String, Arc<Probe>)>) {
        if let Some(argv) = self.local.clone() {
            return self.connect_local(argv);
        }
        let Some(resolved) = self.target.clone() else {
            return;
        };
        self.set_status(Status::Connecting, None);
        self.writer.connecting();
        let (term, writer, state, wake_tx) = (
            self.term.clone(),
            self.writer.clone(),
            self.state.clone(),
            self.wake_tx.clone(),
        );
        let secrets = Some(self.secrets.clone());
        let policy = crate::target::policy(self.ssh.host_key);
        let run = self.run.clone();
        let keepalive = std::time::Duration::from_secs(self.ssh.keepalive);
        // Lives as long as the connection: the forwarded callback and the ended
        // one hold it.
        let held = Arc::new(Held::default());
        self.held = Arc::downgrade(&held);
        let task = self.rt.spawn(async move {
            let via: Vec<&str> = resolved.jumps.iter().map(|j| j.alias.as_str()).collect();
            let via = if via.is_empty() {
                String::new()
            } else {
                format!(" via {}", via.join(", "))
            };
            status_line(&term, &format!("connecting to {}{via}…", resolved.target));

            // Questions from the connection are asked in the terminal.
            let (prompter, mut questions) = Prompter::new();
            {
                let writer = writer.clone();
                tokio::spawn(async move {
                    while let Some(req) = questions.recv().await {
                        writer.ask(req);
                    }
                });
            }
            let (fwd_term, fwd_held) = (term.clone(), held.clone());
            let opts = Options {
                target: resolved,
                auth: Auth {
                    agent: true,
                    // For scripts and benchmarks on a build of one's own:
                    // a release never takes a password from the environment.
                    password: cfg!(debug_assertions)
                        .then(|| std::env::var("SHELLWING_PASSWORD").ok())
                        .flatten(),
                    agent_socket: None,
                },
                policy,
                keepalive: Some(keepalive),
                known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS").map(Into::into),
                prompter: Some(prompter),
                secrets: secrets.clone(),
                forwarded: Some(Box::new(move |r| forwarded(&fwd_term, &fwd_held, r))),
                cols: 80,
                rows: 24,
                term: "xterm-256color".into(),
            };
            let probe = bench.as_ref().map(|(_, p)| p.clone());
            let out_term = term.clone();
            let (end_term, end_writer, end_state, end_wake) =
                (term.clone(), writer.clone(), state.clone(), wake_tx.clone());
            let (done_tx, done_rx) = tokio::sync::oneshot::channel();
            let session = shellwing_ssh::connect(
                opts,
                Box::new(move |bytes| {
                    if let Some(p) = &probe {
                        p.saw_output(bytes);
                    }
                    out_term.feed(bytes.to_vec());
                }),
                Box::new(move |ended| {
                    // The ports are closed now.
                    drop(held);
                    end_writer.stop();
                    let (status, why) = match ended {
                        Ended::Closed {
                            exit_status: Some(s),
                        } => (Status::Closed, format!("session closed (exit {s})")),
                        Ended::Closed { exit_status: None } => {
                            (Status::Closed, "session closed".to_string())
                        }
                        Ended::Error(e) => (Status::Lost, format!("connection lost: {e}")),
                    };
                    if status == Status::Lost {
                        failure(&end_term, &why);
                    } else {
                        status_line(&end_term, &why);
                    }
                    set_status(&end_state, &end_wake, status, Some(why));
                    let _ = done_tx.send(());
                }),
            )
            .await;
            match session {
                Ok(s) => {
                    writer.start_session(s.sender());
                    // Typed ahead: the shell reads it once it is ready.
                    if let Some(cmd) = &run {
                        writer.write(format!("{cmd}\r").into_bytes());
                    }
                    if let Ok(mut st) = state.lock() {
                        st.ever_connected = true;
                    }
                    set_status(&state, &wake_tx, Status::Connected, None);
                    if let Some((mode, probe)) = bench {
                        tokio::spawn(bench::run(mode, writer.clone(), probe));
                    }
                    let _ = done_rx.await;
                }
                Err(e) => {
                    writer.stop();
                    failure(&term, &e.to_string());
                    if let (Some(c), Ok(mut st)) = (changed_key(&e), state.lock()) {
                        st.changed_key = Some(c.clone());
                    }
                    let status = if retryable(&e) {
                        Status::Lost
                    } else {
                        Status::Failed
                    };
                    set_status(&state, &wake_tx, status, Some(e.to_string()));
                }
            }
        });
        self.task = Some(task.abort_handle());
    }
}

fn set_status(
    state: &Mutex<State>,
    wake: &async_channel::Sender<()>,
    status: Status,
    ended: Option<String>,
) {
    if let Ok(mut st) = state.lock() {
        st.status = Some(status);
        st.changes += 1;
        if ended.is_some() {
            st.ended = ended;
        }
        // A new attempt looks at the key again.
        if status == Status::Connecting {
            st.changed_key = None;
        }
    }
    let _ = wake.try_send(());
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(t) = &self.task {
            t.abort();
        }
        // Drops the SSH channel's sender, which closes the session, and ends the
        // terminal thread; the echo thread follows once the last Writer is gone.
        self.writer.stop();
        self.term.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_network_errors_are_retried() {
        assert!(!retryable(&Error::AuthFailed));
        assert!(retryable(&Error::Disconnected));
        assert!(!retryable(&Error::Cancelled));
        assert!(!retryable(&Error::HostKeyRejected { host: "h".into() }));
        let io = Error::Ssh(russh::Error::IO(std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused,
        )));
        assert!(retryable(&io));
        assert!(retryable(&Error::Jump {
            host: "bastion".into(),
            source: Box::new(io),
        }));
    }

    /// A session the settings log from its start: what the shell says, and
    /// how it ended, are in the file.
    #[cfg(unix)]
    #[test]
    fn a_session_is_logged_from_its_start() {
        use std::time::Duration;
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let dir =
            std::env::temp_dir().join(format!("shellwing-session-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut settings = shellwing_config::Settings::default();
        settings.log.always = true;
        settings.log.timestamps = false;
        settings.log.folder = Some(dir.display().to_string());
        let shell = ["/bin/sh", "-c", "echo logged-$((40+2))"];
        let mut session = Session::start(
            crate::local::SPEC,
            Opens::Local(shell.map(String::from).to_vec()),
            &settings,
            rt.handle(),
            None,
            VaultSlot::none(),
        )
        .expect("start");
        let path = session.log_path().expect("logged").to_path_buf();
        // Written as the terminal thread gets to it.
        let mut text = String::new();
        for _ in 0..250 {
            text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("ended") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            text.starts_with("# sh on this computer, logged by shellwing from "),
            "{text}"
        );
        assert!(text.contains("\nlogged-42\n"), "{text}");
        assert!(text.contains("[sh ended (exit 0)]"), "{text}");
        assert_eq!(session.stop_log(), Some(path));
        assert!(session.log_path().is_none());
        drop(session);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

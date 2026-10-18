//! One connection, shown in one pane: its terminal, where typed input goes, and
//! the network task. The terminal outlives connections: reconnecting starts a
//! new network task that writes below what the old one left. Dropping a
//! `Session` disconnects and frees all of it.

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

impl Session {
    /// Starts connecting to `target` (resolved from `spec`) on `rt`. A target that
    /// failed to resolve still gets a session, which shows the error.
    pub fn start(
        spec: &str,
        target: anyhow::Result<(Resolved, Option<Host>)>,
        scrollback: usize,
        ssh: shellwing_config::Ssh,
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
                scrollback,
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
            ssh,
        };
        match target {
            Ok((resolved, entry)) => {
                session.title = resolved.alias.clone();
                session.entry = entry.map(|h| h.name);
                session.target = Some(resolved);
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

    /// Connects again to the same target, below what the terminal shows.
    pub fn reconnect(&mut self) {
        if self.target.is_none() || matches!(self.status(), Status::Connecting | Status::Connected)
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

    fn connect(&mut self, bench: Option<(String, Arc<Probe>)>) {
        let Some(resolved) = self.target.clone() else {
            return;
        };
        self.set_status(Status::Connecting, None);
        let (term, writer, state, wake_tx) = (
            self.term.clone(),
            self.writer.clone(),
            self.state.clone(),
            self.wake_tx.clone(),
        );
        let secrets = Some(self.secrets.clone());
        let policy = crate::target::policy(self.ssh.host_key);
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
                    password: std::env::var("SHELLWING_PASSWORD").ok(),
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
}

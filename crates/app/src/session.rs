//! One connection, shown in one tab: its terminal, where typed input goes, and
//! the network task. Dropping a `Session` disconnects and frees all of it.

use std::sync::{Arc, Mutex};

use shellwing_config::Host;
use shellwing_ssh::{Auth, Ended, HostKeyPolicy, Options, Prompter, Resolved};
use shellwing_term::{Event, TermHandle};
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

use crate::Writer;
use crate::bench::{self, Probe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    /// The server closed the session, or it was closed from here.
    Closed,
    /// It never connected, or broke.
    Failed,
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
    status: Arc<Mutex<Status>>,
    task: Option<AbortHandle>,
    /// A new frame (or a status change) is ready.
    pub wake: async_channel::Receiver<()>,
    pub events: async_channel::Receiver<Event>,
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
        rt: &Handle,
        bench: Option<(String, Arc<Probe>)>,
    ) -> std::io::Result<Session> {
        let (writer, shown) = Writer::new();
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
        let status = Arc::new(Mutex::new(Status::Connecting));
        let set_status = {
            let (status, wake_tx) = (status.clone(), wake_tx);
            move |s: Status| {
                if let Ok(mut st) = status.lock() {
                    *st = s;
                }
                let _ = wake_tx.try_send(());
            }
        };
        let mut session = Session {
            term: term.clone(),
            writer: writer.clone(),
            title: spec.to_string(),
            spec: spec.to_string(),
            entry: None,
            status,
            task: None,
            wake,
            events,
        };
        let (resolved, entry) = match target {
            Ok(t) => t,
            Err(e) => {
                failure(&term, &e.to_string());
                set_status(Status::Failed);
                return Ok(session);
            }
        };
        session.title = resolved.alias.clone();
        session.entry = entry.map(|h| h.name);
        let task = rt.spawn(async move {
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
            let opts = Options {
                target: resolved,
                auth: Auth {
                    agent: true,
                    password: std::env::var("SHELLWING_PASSWORD").ok(),
                },
                policy: HostKeyPolicy::Ask,
                known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS").map(Into::into),
                prompter: Some(prompter),
                cols: 80,
                rows: 24,
                term: "xterm-256color".into(),
            };
            let probe = bench.as_ref().map(|(_, p)| p.clone());
            let out_term = term.clone();
            let (end_term, end_writer, end_status) =
                (term.clone(), writer.clone(), set_status.clone());
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
                    end_writer.stop();
                    let broke = matches!(ended, Ended::Error(_));
                    match ended {
                        Ended::Closed {
                            exit_status: Some(s),
                        } => status_line(&end_term, &format!("session closed (exit {s})")),
                        Ended::Closed { exit_status: None } => {
                            status_line(&end_term, "session closed")
                        }
                        Ended::Error(e) => failure(&end_term, &format!("session error: {e}")),
                    }
                    end_status(if broke {
                        Status::Failed
                    } else {
                        Status::Closed
                    });
                    let _ = done_tx.send(());
                }),
            )
            .await;
            match session {
                Ok(s) => {
                    writer.start_session(s.sender());
                    set_status(Status::Connected);
                    if let Some((mode, probe)) = bench {
                        tokio::spawn(bench::run(mode, writer.clone(), probe));
                    }
                    let _ = done_rx.await;
                }
                Err(e) => {
                    writer.stop();
                    failure(&term, &e.to_string());
                    set_status(Status::Failed);
                }
            }
        });
        session.task = Some(task.abort_handle());
        Ok(session)
    }

    pub fn status(&self) -> Status {
        self.status.lock().map_or(Status::Failed, |s| *s)
    }
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

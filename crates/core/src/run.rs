//! Running a command or a script on many hosts at once: each host on a
//! connection of its own for it, with no shell, a few at a time; what each
//! printed and how it ended, to put side by side.
//!
//! The script goes in on the command's input and is read whole before any
//! of it runs, so nothing in it can read the rest of it. sudo's password,
//! when the script uses sudo, goes in on the same input, ahead of it: sudo
//! gets it from `printenv` as its askpass program, so it is never on a
//! command line, in a file or typed into a terminal.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shellwing_ssh::config::Resolved;
use shellwing_ssh::{Auth, ExecConnection, HostKeyPolicy, Options, Prompt, Prompter, Secrets};
use tokio::runtime::Handle;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::AbortHandle;
use zeroize::Zeroizing;

/// The largest script that is sent.
pub const MOST_SCRIPT: usize = 256 * 1024;

/// How much of each stream a host's run keeps.
pub const KEEP: usize = 1024 * 1024;

/// How long a host may take to answer when its own settings say nothing
/// (`ConnectTimeout`).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What runs the script on the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interpreter {
    /// A POSIX shell (`sh`, `bash`, `zsh`…), by its name or path on the
    /// host, with the flags its `#!` line gave.
    Shell { program: String, flags: Vec<String> },
    /// Python, by its name or path on the host.
    Python(String),
    /// Windows PowerShell.
    PowerShell,
}

impl Interpreter {
    /// The ones offered to choose from.
    pub fn offered() -> Vec<Interpreter> {
        ["sh", "bash", "zsh", "python3", "powershell"]
            .iter()
            .filter_map(|n| Interpreter::named(n))
            .collect()
    }

    /// The one called `name` (a program's name or path on the host), if it
    /// is one the app knows how to give a script to.
    pub fn named(name: &str) -> Option<Interpreter> {
        // A name or path only: it is put in the command as it is.
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
        {
            return None;
        }
        let base = name.rsplit('/').next().unwrap_or(name);
        let shell =
            |b: &str| ["sh", "bash", "dash", "zsh", "ksh", "mksh", "ash", "busybox"].contains(&b);
        Some(if shell(base) {
            Interpreter::Shell {
                program: name.to_string(),
                flags: vec![],
            }
        } else if base.starts_with("python") {
            Interpreter::Python(name.to_string())
        } else if matches!(base, "powershell" | "pwsh" | "powershell.exe" | "pwsh.exe") {
            Interpreter::PowerShell
        } else {
            return None;
        })
    }

    /// What the script's own first line asks for (`#!/bin/bash`,
    /// `#!/usr/bin/env python3`), else `sh`. A `#!` the app cannot give a
    /// script to is run by `sh`, as the shell would.
    pub fn of(script: &str) -> Interpreter {
        Self::from_shebang(script).unwrap_or_else(Self::sh)
    }

    /// The one the script's `#!` line names, if it names one.
    pub fn from_shebang(script: &str) -> Option<Interpreter> {
        let line = script.lines().next()?.strip_prefix("#!")?;
        let mut words = line.split_whitespace();
        let mut program = words.next()?;
        if program.rsplit('/').next() == Some("env") {
            program = words.find(|w| !w.starts_with('-'))?;
        }
        let mut found = Interpreter::named(program)?;
        if let Interpreter::Shell { flags, .. } = &mut found {
            // `-e`, `-eu` and the like; nothing that takes a value, nor
            // what changes where the script is read from (`-c`, `-s`).
            *flags = words
                .filter(|w| {
                    w.len() > 1
                        && w.starts_with('-')
                        && w[1..]
                            .chars()
                            .all(|c| c.is_ascii_alphabetic() && !"cs".contains(c))
                })
                .map(str::to_string)
                .collect();
        }
        Some(found)
    }

    pub fn sh() -> Interpreter {
        Interpreter::Shell {
            program: "sh".into(),
            flags: vec![],
        }
    }

    /// Its name, as shown.
    pub fn name(&self) -> &str {
        match self {
            Interpreter::Shell { program, .. } | Interpreter::Python(program) => {
                program.rsplit('/').next().unwrap_or(program)
            }
            Interpreter::PowerShell => "powershell",
        }
    }

    /// Whether sudo's password can be given to the script: a shell's.
    pub fn takes_sudo(&self) -> bool {
        matches!(self, Interpreter::Shell { .. })
    }
}

/// Whether `script` calls sudo itself, run by `interpreter`: a word `sudo`
/// where a command starts.
pub fn uses_sudo(script: &str, interpreter: &Interpreter) -> bool {
    interpreter.takes_sudo()
        && script.lines().any(|line| {
            let line = line.trim_start();
            !line.starts_with('#')
                && line
                    .split(|c: char| c.is_whitespace() || ";&|()`{}".contains(c))
                    .any(|w| w == "sudo")
        })
}

/// What the host is sent: the command its shell runs, and the command's
/// input. `sudo`: the password sudo is given, for a shell's script.
pub fn invocation(
    script: &str,
    interpreter: &Interpreter,
    sudo: Option<&str>,
) -> (String, Zeroizing<Vec<u8>>) {
    let mut input = Zeroizing::new(Vec::with_capacity(script.len() + 64));
    match interpreter {
        Interpreter::Shell { program, flags } => {
            // Read whole first; then run in this shell, as the script.
            let mut body = String::new();
            if let Some(password) = sudo {
                input.extend_from_slice(password.as_bytes());
                input.push(b'\n');
                // sudo asks `printenv SHELLWING_SUDO` (its askpass, with
                // the prompt as its argument) and gets the password; only
                // sudo's own process has it in its environment.
                body.push_str(concat!(
                    "IFS= read -r __shellwing_sudo; ",
                    "__shellwing_askpass=$(command -v printenv) || { ",
                    "echo 'shellwing: no printenv on this host, for sudo to get the password from' >&2; ",
                    "exit 127; }; ",
                    "sudo() ( SHELLWING_SUDO=$__shellwing_sudo; SUDO_ASKPASS=$__shellwing_askpass; ",
                    "export SHELLWING_SUDO SUDO_ASKPASS; ",
                    "command sudo -A -p SHELLWING_SUDO \"$@\" ); ",
                ));
            }
            body.push_str("__shellwing_script=$(cat); eval \"$__shellwing_script\"");
            input.extend_from_slice(script.as_bytes());
            let mut command = quote(program);
            for f in flags {
                command.push(' ');
                command.push_str(&quote(f));
            }
            command.push_str(" -c ");
            command.push_str(&quote(&body));
            (command, input)
        }
        Interpreter::Python(program) => {
            input.extend_from_slice(script.as_bytes());
            let body = "import sys; exec(compile(sys.stdin.read(), '<script>', 'exec'))";
            (format!("{} -c {}", quote(program), quote(body)), input)
        }
        Interpreter::PowerShell => {
            input.extend_from_slice(script.as_bytes());
            // Run by cmd, Windows' own shell for commands: no `'` quoting;
            // `$s` is PowerShell's, which cmd leaves alone.
            (
                "powershell -NoProfile -NonInteractive -Command \
                 \"$s = [Console]::In.ReadToEnd(); Invoke-Expression $s\""
                    .into(),
                input,
            )
        }
    }
}

/// `s` in single quotes for a POSIX shell (and fish), a `'` in it closed,
/// escaped and opened again.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// How a host's run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It ran, and ended so.
    Ran(Ran),
    /// It could not connect, or sign in: why.
    NotConnected(String),
    /// It was still running after this long, and was given up on.
    GaveUp(Duration),
    /// Connected, but the command did not start or its end was not seen:
    /// why.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    /// None when the host sent none (a signal ended it).
    pub status: Option<u32>,
    pub stdout: String,
    pub stderr: String,
    pub took: Duration,
    /// It printed more than [`KEEP`] on a stream: the rest is not kept.
    pub cut: bool,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }

    /// Whether sudo turned the password down.
    pub fn sudo_refused(&self) -> bool {
        self.stderr.contains("incorrect password attempt")
    }

    /// Whether sudo wanted a password the script was not given, or a
    /// terminal to ask on.
    pub fn sudo_asked(&self) -> bool {
        self.stderr.contains("sudo: a terminal is required")
            || self.stderr.contains("sudo: a password is required")
            || self.stderr.contains("you must have a tty to run sudo")
    }
}

impl Outcome {
    pub fn ok(&self) -> bool {
        matches!(self, Outcome::Ran(r) if r.ok())
    }
}

/// What hosts that said the same thing have in common: an outcome without
/// its time.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Said {
    Ran {
        status: Option<u32>,
        stdout: String,
        stderr: String,
    },
    NotConnected(String),
    GaveUp,
    Failed(String),
}

impl From<&Outcome> for Said {
    fn from(o: &Outcome) -> Said {
        match o {
            Outcome::Ran(r) => Said::Ran {
                status: r.status,
                stdout: r.stdout.trim_end().to_string(),
                stderr: r.stderr.trim_end().to_string(),
            },
            Outcome::NotConnected(why) => Said::NotConnected(why.clone()),
            Outcome::GaveUp(_) => Said::GaveUp,
            Outcome::Failed(why) => Said::Failed(why.clone()),
        }
    }
}

/// The hosts that ended the same way, together: those that ran and ended
/// well first, then the larger groups, else as they came.
pub fn groups<'a>(
    ended: impl IntoIterator<Item = (&'a str, &'a Outcome)>,
) -> Vec<(Said, Vec<String>)> {
    let mut out: Vec<(Said, Vec<String>)> = vec![];
    for (host, outcome) in ended {
        let said = Said::from(outcome);
        match out.iter_mut().find(|(s, _)| *s == said) {
            Some((_, hosts)) => hosts.push(host.to_string()),
            None => out.push((said, vec![host.to_string()])),
        }
    }
    let ok = |s: &Said| {
        matches!(
            s,
            Said::Ran {
                status: Some(0),
                ..
            }
        )
    };
    // Stable: equal ones keep the order they came in.
    out.sort_by_key(|(s, hosts)| (!ok(s), std::cmp::Reverse(hosts.len())));
    out
}

/// What a run tells the app, host by host.
#[derive(Debug)]
pub enum Event {
    /// The host's connection asks something (a password, a passphrase, a
    /// new host key): answer on `reply`, None to give up on the host.
    Asks {
        host: String,
        prompt: Prompt,
        reply: oneshot::Sender<Option<String>>,
    },
    Connected {
        host: String,
    },
    NotConnected {
        host: String,
        why: String,
    },
    /// The script started on the host.
    Started {
        host: String,
    },
    Ended {
        host: String,
        outcome: Outcome,
    },
}

/// What is run, and how.
#[derive(Clone)]
pub struct Job {
    pub script: String,
    pub interpreter: Interpreter,
    /// sudo's password, given to a shell's script when it uses sudo.
    pub sudo: Option<Zeroizing<String>>,
    /// How long a host may run it before it is given up on.
    pub give_up: Duration,
}

/// The hosts of one Run tab: their connections, kept between runs, and the
/// work going on. Dropping it stops the work and closes the connections.
pub struct Fleet {
    rt: Handle,
    secrets: Arc<dyn Secrets>,
    policy: HostKeyPolicy,
    keepalive: Duration,
    connections: Arc<Mutex<BTreeMap<String, Arc<ExecConnection>>>>,
    work: Mutex<Vec<AbortHandle>>,
}

impl Fleet {
    pub fn new(rt: Handle, secrets: Arc<dyn Secrets>, settings: &shellwing_config::Ssh) -> Fleet {
        Fleet {
            rt,
            secrets,
            policy: crate::target::policy(settings.host_key),
            keepalive: Duration::from_secs(settings.keepalive),
            connections: Arc::default(),
            work: Mutex::default(),
        }
    }

    /// Whether `host` is connected now.
    pub fn connected(&self, host: &str) -> bool {
        self.connections.lock().is_ok_and(|c| c.contains_key(host))
    }

    /// Connects to each of `hosts` (its name, and where it is) not
    /// connected yet, `limit` at a time, telling `events` how each went.
    pub fn connect(
        &self,
        hosts: Vec<(String, Resolved)>,
        limit: usize,
        events: async_channel::Sender<Event>,
    ) {
        self.each(hosts, limit, events, None);
    }

    /// Runs `job` on `hosts`, `limit` at a time, connecting first to those
    /// not connected yet.
    pub fn run(
        &self,
        hosts: Vec<(String, Resolved)>,
        job: Job,
        limit: usize,
        events: async_channel::Sender<Event>,
    ) {
        self.each(hosts, limit, events, Some(Arc::new(job)));
    }

    /// Stops what is going on: hosts not yet started are left alone, and
    /// those running are cut off. The connections stay.
    pub fn stop(&self) {
        if let Ok(mut work) = self.work.lock() {
            for task in work.drain(..) {
                task.abort();
            }
        }
    }

    fn each(
        &self,
        hosts: Vec<(String, Resolved)>,
        limit: usize,
        events: async_channel::Sender<Event>,
        job: Option<Arc<Job>>,
    ) {
        let turns = Arc::new(Semaphore::new(limit.max(1)));
        let mut tasks = vec![];
        for (host, target) in hosts {
            let (turns, events, job) = (turns.clone(), events.clone(), job.clone());
            let connections = self.connections.clone();
            let options = self.options(&host, target, events.clone());
            tasks.push(
                self.rt
                    .spawn(async move {
                        let Ok(_turn) = turns.acquire_owned().await else {
                            return;
                        };
                        one(host, options, job, connections, events).await;
                    })
                    .abort_handle(),
            );
        }
        if let Ok(mut work) = self.work.lock() {
            work.retain(|t| !t.is_finished());
            work.extend(tasks);
        }
    }

    /// How `host` is connected to: as a tab would, its questions sent to
    /// the app, its forwards, agent and X11 left out.
    fn options(
        &self,
        host: &str,
        mut target: Resolved,
        events: async_channel::Sender<Event>,
    ) -> Options {
        target.forwards.clear();
        target.forward_agent = false;
        for hop in target.jumps.iter_mut() {
            hop.connect_timeout.get_or_insert(CONNECT_TIMEOUT);
        }
        target.connect_timeout.get_or_insert(CONNECT_TIMEOUT);
        let (prompter, mut questions) = Prompter::new();
        let asking = host.to_string();
        self.rt.spawn(async move {
            while let Some(req) = questions.recv().await {
                let (reply, answer) = oneshot::channel();
                let asked = Event::Asks {
                    host: asking.clone(),
                    prompt: req.prompt,
                    reply,
                };
                if events.send(asked).await.is_err() {
                    let _ = req.reply.send(None);
                    continue;
                }
                let _ = req.reply.send(answer.await.ok().flatten());
            }
        });
        Options {
            target,
            auth: Auth {
                agent: true,
                password: cfg!(debug_assertions)
                    .then(|| std::env::var("SHELLWING_PASSWORD").ok())
                    .flatten(),
                agent_socket: None,
            },
            policy: self.policy,
            known_hosts: std::env::var_os("SHELLWING_KNOWN_HOSTS").map(Into::into),
            prompter: Some(prompter),
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

impl Drop for Fleet {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One host: connected if it is not, then `job` run on it, if there is one.
async fn one(
    host: String,
    options: Options,
    job: Option<Arc<Job>>,
    connections: Arc<Mutex<BTreeMap<String, Arc<ExecConnection>>>>,
    events: async_channel::Sender<Event>,
) {
    let existing = connections.lock().ok().and_then(|c| c.get(&host).cloned());
    let connection = match existing {
        Some(c) => c,
        None => match shellwing_ssh::connect_exec(options).await {
            Ok(c) => {
                let c = Arc::new(c);
                if let Ok(mut all) = connections.lock() {
                    all.insert(host.clone(), c.clone());
                }
                let _ = events.send(Event::Connected { host: host.clone() }).await;
                c
            }
            Err(e) => {
                let why = e.to_string();
                let _ = events
                    .send(Event::NotConnected {
                        host: host.clone(),
                        why: why.clone(),
                    })
                    .await;
                if job.is_some() {
                    let outcome = Outcome::NotConnected(why);
                    let _ = events.send(Event::Ended { host, outcome }).await;
                }
                return;
            }
        },
    };
    let Some(job) = job else {
        return;
    };
    let sudo = job
        .sudo
        .as_deref()
        .filter(|_| uses_sudo(&job.script, &job.interpreter))
        .map(String::as_str);
    let (command, input) = invocation(&job.script, &job.interpreter, sudo);
    let _ = events.send(Event::Started { host: host.clone() }).await;
    let began = Instant::now();
    let outcome =
        match tokio::time::timeout(job.give_up, connection.run(&command, &input, KEEP)).await {
            Err(_) => Outcome::GaveUp(job.give_up),
            Ok(Ok(out)) => Outcome::Ran(Ran {
                status: out.status,
                cut: out.stdout.len() >= KEEP || out.stderr.len() >= KEEP,
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                took: began.elapsed(),
            }),
            Ok(Err(e)) => {
                // The connection is likely gone: the next run connects again.
                if let Ok(mut all) = connections.lock() {
                    all.remove(&host);
                }
                Outcome::Failed(e.to_string())
            }
        };
    let _ = events.send(Event::Ended { host, outcome }).await;
}

/// The person's saved passwords and passphrases, keys and certificates, for
/// a run's connections as for a tab's; what is typed there is not offered
/// for saving.
pub fn secrets(
    vault: crate::secrets::VaultSlot,
    settings: &shellwing_config::Settings,
) -> Arc<dyn Secrets> {
    let (learned, _) = async_channel::unbounded();
    Arc::new(crate::secrets::SessionSecrets {
        vault,
        learned,
        server: match settings.sync.place() {
            Some(shellwing_config::SyncPlace::Server(s)) => Some(s.url),
            _ => None,
        },
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Runs what [`invocation`] makes as a host's shell would: `sh -c`.
    fn run(
        script: &str,
        interpreter: &Interpreter,
        sudo: Option<&str>,
        path: Option<&str>,
    ) -> (Option<i32>, String, String) {
        let (command, input) = invocation(script, interpreter, sudo);
        let mut c = Command::new("sh");
        c.arg("-c").arg(&command);
        if let Some(dir) = path {
            let old = std::env::var("PATH").unwrap_or_default();
            c.env("PATH", format!("{dir}:{old}"));
        }
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("sh");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&input)
            .expect("input");
        let out = child.wait_with_output().expect("ran");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    #[test]
    fn the_first_line_names_the_interpreter() {
        assert_eq!(Interpreter::of("echo hi"), Interpreter::sh());
        assert_eq!(
            Interpreter::of("#!/bin/bash -eu\nx"),
            Interpreter::Shell {
                program: "/bin/bash".into(),
                flags: vec!["-eu".into()]
            }
        );
        assert_eq!(Interpreter::of("#!/usr/bin/env bash\n").name(), "bash");
        assert_eq!(
            Interpreter::of("#!/usr/bin/env python3\nprint(1)"),
            Interpreter::Python("python3".into())
        );
        assert_eq!(
            Interpreter::of("#!/usr/bin/env -S ruby\n"),
            Interpreter::sh()
        );
        // Nothing that could be read as more than a program.
        assert_eq!(Interpreter::named("bash; rm -rf ~"), None);
        assert_eq!(
            Interpreter::of("#!/bin/sh -c 'x'\n"),
            Interpreter::Shell {
                program: "/bin/sh".into(),
                flags: vec![]
            }
        );
    }

    #[test]
    fn sudo_where_a_command_starts() {
        let sh = Interpreter::sh();
        assert!(uses_sudo("sudo systemctl reload nginx", &sh));
        assert!(uses_sudo("set -e\nnginx -t && sudo nginx -s reload", &sh));
        assert!(uses_sudo("x=$(sudo cat /etc/shadow)", &sh));
        assert!(!uses_sudo("# sudo is not needed\necho pseudo", &sh));
        assert!(!uses_sudo("echo sudoers", &sh));
        assert!(!uses_sudo("sudo x", &Interpreter::Python("python3".into())));
    }

    #[test]
    fn the_script_arrives_as_it_was_written() {
        let script = "#!/bin/sh\nx='it'\\''s'\necho \"$x\" '$HOME' \\\\ \"é\"\necho \"$0\" >/dev/null\nexit 3";
        let (code, out, _) = run(script, &Interpreter::of(script), None, None);
        assert_eq!(code, Some(3));
        assert_eq!(out, "it's $HOME \\ é\n");
    }

    /// What reads its input gets nothing: not the rest of the script.
    #[test]
    fn nothing_in_it_reads_the_rest_of_it() {
        let script = "read line\necho \"read: [$line]\"\necho after";
        let (code, out, _) = run(script, &Interpreter::sh(), None, None);
        assert_eq!(code, Some(0));
        assert_eq!(out, "read: []\nafter\n");
    }

    /// A stand-in for sudo: asks its askpass with its prompt, as sudo -A
    /// does, and runs the command for the right password.
    fn fake_sudo() -> tempdir::Dir {
        let dir = tempdir::Dir::new("fake-sudo");
        let sudo = dir.path.join("sudo");
        std::fs::write(
            &sudo,
            "#!/bin/sh\n\
             [ \"$1\" = -A ] || { echo 'sudo: a terminal is required to read the password' >&2; exit 1; }\n\
             shift; [ \"$1\" = -p ] && { prompt=$2; shift 2; }\n\
             pw=$(\"$SUDO_ASKPASS\" \"$prompt\")\n\
             [ \"$pw\" = 'pa ss' ] && exec \"$@\"\n\
             echo 'sudo: 3 incorrect password attempts' >&2; exit 1\n",
        )
        .expect("sudo");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sudo, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        dir
    }

    mod tempdir {
        pub struct Dir {
            pub path: std::path::PathBuf,
        }
        impl Dir {
            pub fn new(name: &str) -> Dir {
                let path = std::env::temp_dir().join(format!(
                    "shellwing-run-{name}-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).expect("dir");
                Dir { path }
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    #[test]
    fn sudo_gets_the_password_and_nothing_else_does() {
        let dir = fake_sudo();
        let path = dir.path.to_string_lossy().into_owned();
        let script = "sudo echo \"as root: $1\" ok\necho \"seen: [$SHELLWING_SUDO]\"\nread x; echo \"read: [$x]\"";
        let (code, out, err) = run(script, &Interpreter::sh(), Some("pa ss"), Some(&path));
        assert_eq!(code, Some(0), "{err}");
        assert_eq!(out, "as root:  ok\nseen: []\nread: []\n");

        let (code, _, err) = run("sudo true", &Interpreter::sh(), Some("wrong"), Some(&path));
        assert_eq!(code, Some(1));
        let ran = Ran {
            status: Some(1),
            stdout: String::new(),
            stderr: err,
            took: Duration::ZERO,
            cut: false,
        };
        assert!(ran.sudo_refused());
    }

    #[test]
    fn python_reads_its_script_whole() {
        if Command::new("python3").arg("-V").output().is_err() {
            eprintln!("skipped: no python3");
            return;
        }
        let script = "#!/usr/bin/env python3\nimport sys\nprint('it\\'s', repr(sys.stdin.read()))\nsys.exit(4)";
        let (code, out, err) = run(script, &Interpreter::of(script), None, None);
        assert_eq!(code, Some(4), "{err}");
        assert_eq!(out, "it's ''\n");
    }

    #[test]
    fn hosts_that_said_the_same_go_together() {
        let ran = |status, out: &str| {
            Outcome::Ran(Ran {
                status: Some(status),
                stdout: out.into(),
                stderr: String::new(),
                took: Duration::from_millis(status as u64 * 10 + out.len() as u64),
                cut: false,
            })
        };
        let ended = [
            ("web-3", ran(3, "inactive\n")),
            ("api-1", ran(0, "active\n")),
            ("stg-web", Outcome::NotConnected("timed out".into())),
            ("api-2", ran(0, "active")),
            ("web-4", ran(3, "inactive")),
            ("web-5", ran(3, "inactive")),
        ];
        let groups = groups(ended.iter().map(|(h, o)| (*h, o)));
        let hosts: Vec<Vec<String>> = groups.into_iter().map(|(_, h)| h).collect();
        assert_eq!(
            hosts,
            [
                vec!["api-1", "api-2"],
                vec!["web-3", "web-4", "web-5"],
                vec!["stg-web"]
            ]
            .map(|v| v.into_iter().map(String::from).collect::<Vec<_>>())
        );
    }
}

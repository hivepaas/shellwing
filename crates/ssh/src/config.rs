//! Resolving a target through `~/.ssh/config`, as `ssh` would.
//!
//! `shellwing web` uses the `Host web` block: its `HostName`, `User`, `Port`,
//! `IdentityFile`, `IdentityAgent`, `IdentitiesOnly`, `ProxyJump`, `ProxyCommand` and
//! `ServerAliveInterval`. Values typed on the
//! command line (`user@host:port`) win over the file, and the file wins over the
//! defaults, matching OpenSSH's precedence.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Error, Target};

/// ProxyJump chains longer than this are refused (OpenSSH has no limit, but a
/// config that loops would otherwise never resolve).
const MAX_JUMPS: usize = 8;

/// Which ssh-agent signs in and is forwarded (`IdentityAgent`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum IdentityAgent {
    /// `$SSH_AUTH_SOCK`; on Windows, OpenSSH's agent, then Pageant.
    #[default]
    Usual,
    /// `none`: no agent.
    Off,
    /// One of its own: a socket, or on Windows a named pipe. 1Password,
    /// Secretive and Bitwarden have theirs named so.
    At(PathBuf),
}

/// X11 forwarding for a host (`ForwardX11`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct X11 {
    /// The display's own cookie (`ForwardX11Trusted yes`): what the host's
    /// programs do on the display has no limits, reading the keyboard and the
    /// screen included. Otherwise a limited cookie made for the session.
    pub trusted: bool,
    /// No new connections after this long (`ForwardX11Timeout`); zero: no
    /// limit. Only for a limited cookie, which stops working then too.
    pub timeout: Duration,
    /// The `xauth` to run (`XAuthLocation`).
    pub xauth: Option<PathBuf>,
}

impl Default for X11 {
    fn default() -> Self {
        X11 {
            trusted: false,
            timeout: Duration::from_secs(20 * 60),
            xauth: None,
        }
    }
}

/// Everything needed to reach one host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The name as given (an alias or a host name), shown to the user.
    pub alias: String,
    /// The real host name, port and user.
    pub target: Target,
    /// Keys to try, in order. Empty means the OpenSSH defaults, then every key
    /// the app keeps.
    pub identity_files: Vec<PathBuf>,
    /// Certificates to sign in with (`CertificateFile`), beside the
    /// `<key>-cert.pub` found next to each key file.
    pub certificate_files: Vec<PathBuf>,
    /// The one key the app keeps to sign in with (its id), instead of any file.
    pub vault_key: Option<String>,
    pub identity_agent: IdentityAgent,
    /// Only the keys named (`IdentitiesOnly`): none of the agent's others,
    /// which otherwise come before the key files and can use up the
    /// server's tries (`MaxAuthTries`).
    pub identities_only: bool,
    /// Hosts to go through first, outermost first; each has no jumps of its own.
    pub jumps: Vec<Resolved>,
    /// The command whose input and output carry the connection
    /// (`ProxyCommand`), its `%` tokens not expanded yet. Used when this host
    /// is the first one connected to, not through a jump host (as `ssh -J`
    /// skips it there).
    pub proxy_command: Option<String>,
    pub keepalive: Option<Duration>,
    /// Ports to forward, as `-L`, `-R` and `-D` options are written. Read when
    /// connecting, so one that is wrong is reported without stopping the rest.
    pub forwards: Vec<String>,
    /// Let the host use the local ssh-agent (`ForwardAgent`). Off unless asked:
    /// whoever is root there can then sign with your keys while you are connected.
    pub forward_agent: bool,
    /// How an unknown host key is handled here (`StrictHostKeyChecking`, or
    /// hosts.toml's `host_key`); None: the app's setting.
    pub host_key: Option<crate::HostKeyPolicy>,
    /// The known_hosts file for this host (`UserKnownHostsFile`, its first);
    /// None: `~/.ssh/known_hosts`. The null device for `/dev/null` or `none`:
    /// nothing known, nothing kept.
    pub known_hosts: Option<PathBuf>,
    /// How long to wait for the host to answer, up to showing its key
    /// (`ConnectTimeout`); None: as long as the system waits.
    pub connect_timeout: Option<Duration>,
    /// Let the host's programs open windows here (`ForwardX11`). Off unless
    /// asked; jump hosts never get it.
    pub x11: Option<X11>,
    /// Run instead of the login shell (`RemoteCommand`), its `%` tokens not
    /// expanded yet.
    pub remote_command: Option<String>,
    /// A terminal (PTY) for the session: off only for `RequestTTY no`.
    pub tty: bool,
    /// Which of this machine's variables to send the host (`SendEnv`
    /// patterns, with `*` and `?`), read when connecting.
    pub send_env: Vec<String>,
    /// Variables sent as they are (`SetEnv`). The host takes only those its
    /// `AcceptEnv` names, these and `SendEnv`'s alike.
    pub set_env: Vec<(String, String)>,
}

impl Resolved {
    /// A host used as is, without consulting any config file.
    pub fn direct(target: Target) -> Self {
        Resolved {
            alias: target.host.clone(),
            target,
            identity_files: vec![],
            certificate_files: vec![],
            vault_key: None,
            identity_agent: IdentityAgent::Usual,
            identities_only: false,
            jumps: vec![],
            proxy_command: None,
            keepalive: None,
            forwards: vec![],
            forward_agent: false,
            host_key: None,
            known_hosts: None,
            connect_timeout: None,
            x11: None,
            remote_command: None,
            tty: true,
            send_env: vec![],
            set_env: vec![],
        }
    }

    /// The key files to offer: none for a key the app keeps, the configured
    /// ones, else the OpenSSH defaults that exist.
    pub fn keys_to_try(&self) -> Vec<PathBuf> {
        if self.vault_key.is_some() {
            return vec![];
        }
        if !self.identity_files.is_empty() {
            return self.identity_files.clone();
        }
        let Some(ssh) = std::env::home_dir().map(|h| h.join(".ssh")) else {
            return vec![];
        };
        // Security keys too, for the agent's to go first; the token signs there.
        [
            "id_ed25519",
            "id_ed25519_sk",
            "id_ecdsa",
            "id_ecdsa_sk",
            "id_rsa",
        ]
        .iter()
        .map(|n| ssh.join(n))
        .filter(|p| p.exists())
        .collect()
    }
}

/// The ssh config file in effect: `$SHELLWING_SSH_CONFIG`, else `~/.ssh/config`.
pub fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("SHELLWING_SSH_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".ssh").join("config")))
}

/// The names `Host` lines give, in file order, leaving out patterns (`*`, `?`)
/// and negations: the hosts a person can connect to by name.
pub fn aliases(config: Option<&Path>) -> Vec<String> {
    let Some(path) = config.filter(|p| p.exists()) else {
        return vec![];
    };
    let mut out: Vec<String> = vec![];
    for (blocks, line) in scan(path) {
        let (Line::Starts, Some(Block::Hosts(patterns))) = (line, blocks.last()) else {
            continue;
        };
        for p in patterns {
            if !p.contains(['*', '?', '!']) && !out.contains(p) {
                out.push(p.clone());
            }
        }
    }
    out
}

pub(crate) fn local_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".into())
}

/// Splits `[user@]host[:port]` without applying defaults.
pub fn split(spec: &str) -> Result<(Option<String>, String, Option<u16>), Error> {
    let (user, rest) = match spec.rsplit_once('@') {
        Some((u, r)) => (Some(u.to_string()), r),
        None => (None, spec),
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (
            h.to_string(),
            Some(p.parse().map_err(|_| Error::BadTarget(spec.into()))?),
        ),
        _ => (rest.to_string(), None),
    };
    if host.is_empty() || user.as_deref() == Some("") {
        return Err(Error::BadTarget(spec.into()));
    }
    Ok((user, host, port))
}

/// Expands the `%` tokens OpenSSH allows in `IdentityFile`.
fn expand_tokens(path: &Path, host: &str, user: &str) -> PathBuf {
    let s = path.to_string_lossy();
    if !s.contains('%') {
        return path.to_path_buf();
    }
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str(&home),
            Some('h') => out.push_str(host),
            Some('r') => out.push_str(user),
            Some('u') => out.push_str(&local_user()),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    PathBuf::from(out)
}

/// What an `IdentityAgent` value names, as ssh reads it: `none`,
/// `SSH_AUTH_SOCK` (the usual agent), `$NAME` (a variable holding the
/// socket's path; no agent if it is not set), or a path, with `~`,
/// `${NAME}` and the `%` tokens of `IdentityFile`.
fn identity_agent(value: &str, host: &str, user: &str) -> IdentityAgent {
    match value {
        "none" => IdentityAgent::Off,
        "SSH_AUTH_SOCK" => IdentityAgent::Usual,
        v if v.starts_with('$') && !v.starts_with("${") => match std::env::var_os(&v[1..]) {
            Some(path) if !path.is_empty() => IdentityAgent::At(path.into()),
            _ => IdentityAgent::Off,
        },
        v => IdentityAgent::At(config_path(v, host, user)),
    }
}

/// A path from the config, as ssh reads `IdentityAgent` and
/// `UserKnownHostsFile`: `${NAME}` from the environment, `~` for the home
/// folder, and the `%` tokens of `IdentityFile`.
fn config_path(value: &str, host: &str, user: &str) -> PathBuf {
    let mut path = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        path.push_str(&rest[..start]);
        path.push_str(&std::env::var(&rest[start + 2..start + len]).unwrap_or_default());
        rest = &rest[start + len + 1..];
    }
    path.push_str(rest);
    let path = match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(under), Some(home)) => home.join(under),
        _ => PathBuf::from(path),
    };
    expand_tokens(&path, host, user)
}

/// The null device, which `UserKnownHostsFile /dev/null` means everywhere.
pub(crate) fn null_device() -> PathBuf {
    PathBuf::from(if cfg!(windows) { "NUL" } else { "/dev/null" })
}

/// How long a `Match exec` command may take; past that it counts as false.
/// A host is often resolved on the app's main thread.
const EXEC_LIMIT: Duration = Duration::from_secs(3);

/// Whether a `Match exec` runs its command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exec {
    /// Connecting: as ssh does.
    Run,
    /// Only looking: nothing runs, and it counts as false.
    Skip,
}

/// Resolves `[user@]host[:port]` through the config file at `config` (none:
/// defaults only), to connect: a `Match exec` runs its command.
pub fn resolve(spec: &str, config: Option<&Path>) -> Result<Resolved, Error> {
    resolve_as(spec, config, Exec::Run)
}

/// As [`resolve`], to show what a host would be: no `Match exec` command runs.
pub fn resolve_without_commands(spec: &str, config: Option<&Path>) -> Result<Resolved, Error> {
    resolve_as(spec, config, Exec::Skip)
}

fn resolve_as(spec: &str, config: Option<&Path>, exec: Exec) -> Result<Resolved, Error> {
    let lines = match config.filter(|p| p.exists()) {
        Some(path) => {
            std::fs::read(path).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
            scan(path)
        }
        None => vec![],
    };
    let context = Reading {
        lines: &lines,
        exec,
        ran: Default::default(),
    };
    let mut r = resolve_with(&context, spec, &mut vec![])?;
    // The host's own forwards; jump hosts forward nothing, as with ssh -J.
    r.forwards = forwards_in(&context.applying(&r.alias, split(spec)?.0.as_deref()));
    Ok(r)
}

/// How deep `Include`s may nest, as in OpenSSH.
const MAX_INCLUDE: usize = 16;

/// Which hosts the lines of a config file are for.
#[derive(Clone, Debug)]
enum Block {
    /// Before any `Host`: every host.
    Every,
    /// `Host` patterns.
    Hosts(Vec<String>),
    /// `Match` conditions, all of which must hold, weighed once per reading
    /// where the line stands; the id tells one `Match` line from another.
    Match(usize, Vec<Condition>),
}

/// One condition of a `Match` line.
#[derive(Clone, Debug)]
struct Condition {
    negated: bool,
    test: Test,
}

#[derive(Clone, Debug)]
enum Test {
    All,
    /// `final`: holds in a second reading, once the rest is known.
    Final,
    Host(String),
    OriginalHost(String),
    User(String),
    LocalUser(String),
    Exec(String),
    Tagged(String),
    Command(String),
    /// What is not weighed here: `canonical` (no canonicalization),
    /// `localnetwork`, `version`, and anything unknown. It holds for nobody,
    /// negated or not, so such a block applies to no host.
    Never,
}

/// A line of a config file.
enum Line {
    /// A `Host` or `Match` line: what starts a block.
    Starts,
    HostName(String),
    User(String),
    Port(String),
    /// `IdentityFile`, as written; every one counts.
    IdentityFile(String),
    /// `CertificateFile`, as written; every one counts.
    CertificateFile(String),
    ServerAliveInterval(String),
    ConnectTimeout(String),
    ForwardAgent(bool),
    Tag(String),
    /// `LocalForward`, `RemoteForward` or `DynamicForward`, as a `-L`/`-R`/`-D` spec.
    Forward(String),
    /// `ClearAllForwardings`.
    Clear(bool),
    /// `ProxyCommand`, the rest of the line as written: it goes to a shell.
    ProxyCommand(String),
    /// `ProxyJump`: its hosts, or `none`.
    ProxyJump(String),
    /// `IdentityAgent`, as written (its quotes taken off).
    IdentityAgent(String),
    IdentitiesOnly(bool),
    /// `SendEnv`'s patterns, `-pattern` taking back earlier ones.
    SendEnv(Vec<String>),
    SetEnv(Vec<(String, String)>),
    /// `RemoteCommand`, the rest of the line as written.
    RemoteCommand(String),
    /// `RequestTTY`: false for `no`.
    RequestTty(bool),
    StrictHostKeyChecking(String),
    /// `UserKnownHostsFile`'s first file, as written.
    UserKnownHostsFile(String),
    ForwardX11(bool),
    ForwardX11Trusted(bool),
    /// `ForwardX11Timeout`, as written.
    ForwardX11Timeout(String),
    XAuthLocation(String),
}

/// The lines of a config file, each with the blocks it comes under: its own,
/// after those of the `Include`s that brought its file in. It applies only
/// if all of them do: an `Include` in a block that does not apply brings
/// nothing, whatever `Host` lines its file has.
type Lines = Vec<(Vec<Block>, Line)>;

/// One reading of a config file, which may run `Match exec` commands.
struct Reading<'a> {
    lines: &'a Lines,
    exec: Exec,
    /// The answers of the commands run so far.
    ran: std::cell::RefCell<std::collections::HashMap<String, bool>>,
}

/// What ssh has settled so far while it reads a config file for one host.
struct Settled<'a> {
    alias: &'a str,
    host: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    tag: Option<String>,
}

impl Settled<'_> {
    fn take(&mut self, line: &Line) {
        match line {
            Line::HostName(h) if self.host.is_none() => self.host = Some(host_name(h, self.alias)),
            Line::User(u) if self.user.is_none() => self.user = Some(u.clone()),
            Line::Port(p) if self.port.is_none() => self.port = p.parse().ok(),
            Line::Tag(t) if self.tag.is_none() => self.tag = Some(t.clone()),
            _ => {}
        }
    }
}

/// `HostName`, with `%h` the name given.
fn host_name(value: &str, alias: &str) -> String {
    value
        .replace("%%", "\0")
        .replace("%h", alias)
        .replace('\0', "%")
}

/// Whether `text` matches a comma-separated list of patterns: one of them
/// does and no negated one.
fn matches_list(list: &str, text: &str) -> bool {
    let patterns: Vec<String> = list.split(',').map(str::to_string).collect();
    let mut hit = false;
    for p in &patterns {
        match p.strip_prefix('!') {
            Some(neg) if wildcard(neg, text) => return false,
            Some(_) => {}
            None => hit |= wildcard(p, text),
        }
    }
    hit
}

/// Runs a `Match exec` command under the user's shell: whether it exits with
/// zero within [`EXEC_LIMIT`].
fn exec_holds(command: &str) -> bool {
    use shellwing_spawn::Unblocked;
    #[cfg(unix)]
    let mut c = {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut c = std::process::Command::new(shell);
        c.arg("-c").arg(format!("exec {command}"));
        c
    };
    #[cfg(windows)]
    let mut c = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = std::process::Command::new(
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
        );
        c.raw_arg(format!("/d /c {command}"));
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(not(any(unix, windows)))]
    let mut c = std::process::Command::new(command);
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let Ok(mut child) = c.spawn_unblocked() else {
        return false;
    };
    let deadline = std::time::Instant::now() + EXEC_LIMIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

impl<'a> Reading<'a> {
    fn condition_holds(&self, c: &Condition, now: &Settled, last_pass: bool) -> bool {
        let host = now.host.clone().unwrap_or_else(|| now.alias.to_string());
        let user = now.user.clone().unwrap_or_else(local_user);
        let holds = match &c.test {
            Test::Never => return false,
            Test::All => true,
            Test::Final => last_pass,
            Test::Host(list) => matches_list(&list.to_lowercase(), &host.to_lowercase()),
            Test::OriginalHost(list) => {
                matches_list(&list.to_lowercase(), &now.alias.to_lowercase())
            }
            Test::User(list) => matches_list(list, &user),
            Test::LocalUser(list) => matches_list(list, &local_user()),
            Test::Tagged(list) => matches_list(list, now.tag.as_deref().unwrap_or("")),
            // The command on ssh's command line, not RemoteCommand: none here.
            Test::Command(list) => matches_list(list, ""),
            Test::Exec(command) => {
                if self.exec == Exec::Skip {
                    return false;
                }
                let mut hop = Resolved::direct(Target {
                    user,
                    host,
                    port: now.port.unwrap_or(22),
                });
                hop.alias = now.alias.to_string();
                let Ok(command) = crate::tokens::expand(command, &hop, "Match exec", "hprndu")
                else {
                    return false;
                };
                if let Some(known) = self.ran.borrow().get(&command) {
                    *known
                } else {
                    let answer = exec_holds(&command);
                    self.ran.borrow_mut().insert(command, answer);
                    answer
                }
            }
        };
        holds != c.negated
    }

    /// The lines that apply to `alias`, in the order ssh takes them. As in
    /// ssh, a `Match` sees what the lines before it settled. A `Match final`
    /// holds in a second reading, whose lines come after the first's: what
    /// the first settled stays.
    fn applying(&self, alias: &str, given_user: Option<&str>) -> Vec<&'a Line> {
        let mut now = Settled {
            alias,
            host: None,
            user: given_user.map(str::to_string),
            port: None,
            tag: None,
        };
        let wants_final = self.lines.iter().any(|(blocks, _)| {
            blocks.iter().any(|block| {
                matches!(block, Block::Match(_, conditions)
                    if conditions.iter().any(|c| matches!(c.test, Test::Final)))
            })
        });
        let mut taken: Vec<&'a Line> = vec![];
        let mut seen = vec![false; self.lines.len()];
        for last_pass in [false, true] {
            if last_pass && !wants_final {
                break;
            }
            let mut weighed = std::collections::HashMap::new();
            for (i, (blocks, line)) in self.lines.iter().enumerate() {
                let on = blocks.iter().all(|block| match block {
                    Block::Every => true,
                    Block::Hosts(patterns) => host_matches(patterns, alias),
                    Block::Match(id, conditions) => *weighed.entry(*id).or_insert_with(|| {
                        conditions
                            .iter()
                            .all(|c| self.condition_holds(c, &now, last_pass))
                    }),
                });
                if on && !seen[i] {
                    seen[i] = true;
                    now.take(line);
                    taken.push(line);
                }
            }
        }
        taken
    }
}

/// The conditions of a `Match` line: `keyword value` or `keyword=value`, each
/// perhaps negated with `!`. A line with none holds for nobody.
fn match_conditions(words: &[String]) -> Vec<Condition> {
    let mut out = vec![];
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let (negated, word) = match word.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, word.as_str()),
        };
        let (key, inline) = match word.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (word, None),
        };
        let key = key.to_lowercase();
        let test = match key.as_str() {
            "all" => Test::All,
            "final" => Test::Final,
            "canonical" => Test::Never,
            _ => {
                let Some(value) = inline.or_else(|| words.next().cloned()) else {
                    out.push(Condition {
                        negated,
                        test: Test::Never,
                    });
                    break;
                };
                match key.as_str() {
                    "host" => Test::Host(value),
                    "originalhost" => Test::OriginalHost(value),
                    "user" => Test::User(value),
                    "localuser" => Test::LocalUser(value),
                    "exec" => Test::Exec(value),
                    "tagged" | "tag" => Test::Tagged(value),
                    "command" => Test::Command(value),
                    _ => Test::Never,
                }
            }
        };
        out.push(Condition { negated, test });
    }
    if out.is_empty() {
        out.push(Condition {
            negated: false,
            test: Test::Never,
        });
    }
    out
}

/// Whether `host` matches a `Host` line's patterns: one of them does and
/// none of the negated (`!`) ones, ignoring case.
fn host_matches(patterns: &[String], host: &str) -> bool {
    let host = host.to_lowercase();
    let mut hit = false;
    for p in patterns {
        let p = p.to_lowercase();
        match p.strip_prefix('!') {
            Some(neg) if wildcard(neg, &host) => return false,
            Some(_) => {}
            None => hit |= wildcard(&p, &host),
        }
    }
    hit
}

/// `*` for any run of characters, `?` for one.
pub(crate) fn wildcard(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// The files an `Include` names: `~` expanded, relative to `~/.ssh`, and a
/// `*` or `?` in the file name matched, in name order.
fn include_paths(arg: &str) -> Vec<PathBuf> {
    let home = std::env::home_dir().unwrap_or_default();
    let path = match arg.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if Path::new(arg).is_absolute() => PathBuf::from(arg),
        None => home.join(".ssh").join(arg),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return vec![];
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| wildcard(&name, &e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// A yes/no value, as ssh reads it.
fn flag(args: &[String]) -> bool {
    args.first()
        .is_some_and(|a| a.eq_ignore_ascii_case("yes") || a.eq_ignore_ascii_case("true"))
}

/// A time as ssh_config writes it: `300`, `20m`, `1h30m`, with `s`, `m`, `h`,
/// `d` and `w`.
fn time_value(text: &str) -> Option<Duration> {
    let (mut total, mut number) = (0u64, None::<u64>);
    for c in text.to_ascii_lowercase().chars() {
        if let Some(d) = c.to_digit(10) {
            number = Some(number.unwrap_or(0).checked_mul(10)?.checked_add(d as u64)?);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            'w' => 604800,
            _ => return None,
        };
        total = total.checked_add(number.take()?.checked_mul(unit)?)?;
    }
    total = total.checked_add(number.unwrap_or(0))?;
    Some(Duration::from_secs(total))
}

/// Splits a line's values as ssh does: at spaces outside quotes (`"` or
/// `'`), a `\` outside single quotes taking the next character as it is.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quote, mut started) = (vec![], String::new(), None, false);
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (Some(q), c) if c == q => quote = None,
            (q, '\\') if q != Some('\'') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
                started = true;
            }
            (_, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// The [`Line`]s of the config file at `path` and the files it includes.
fn scan(path: &Path) -> Lines {
    let mut lines = vec![];
    scan_into(path, &[], 0, &mut lines);
    lines
}

fn scan_into(path: &Path, within: &[Block], depth: usize, out: &mut Lines) {
    let mut block = Block::Every;
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `Keyword value…` or `Keyword=value…`.
        let (key, rest) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (
                &line[..i],
                line[i..].trim_start_matches(|c: char| c.is_whitespace() || c == '='),
            ),
            None => (line, ""),
        };
        let args = words(rest);
        let key = key.to_lowercase();
        let first = args.first();
        let parsed = match (key.as_str(), first) {
            ("host", _) => {
                block = Block::Hosts(args);
                Some(Line::Starts)
            }
            ("match", _) => {
                block = Block::Match(out.len(), match_conditions(&args));
                Some(Line::Starts)
            }
            ("include", _) if depth < MAX_INCLUDE => {
                let mut blocks = within.to_vec();
                blocks.push(block.clone());
                for a in &args {
                    for file in include_paths(a) {
                        scan_into(&file, &blocks, depth + 1, out);
                    }
                }
                None
            }
            ("hostname", Some(v)) => Some(Line::HostName(v.clone())),
            ("user", Some(v)) => Some(Line::User(v.clone())),
            ("port", Some(v)) => Some(Line::Port(v.clone())),
            ("identityfile", Some(v)) => Some(Line::IdentityFile(v.clone())),
            ("certificatefile", Some(v)) => Some(Line::CertificateFile(v.clone())),
            ("serveraliveinterval", Some(v)) => Some(Line::ServerAliveInterval(v.clone())),
            ("connecttimeout", Some(v)) => Some(Line::ConnectTimeout(v.clone())),
            ("forwardagent", Some(_)) => Some(Line::ForwardAgent(flag(&args))),
            ("tag", Some(v)) => Some(Line::Tag(v.clone())),
            ("localforward", Some(_)) => Some(Line::Forward(format!("-L {}", args.join(":")))),
            ("remoteforward", Some(_)) => Some(Line::Forward(format!("-R {}", args.join(":")))),
            ("dynamicforward", Some(_)) => Some(Line::Forward(format!("-D {}", args.join(":")))),
            ("clearallforwardings", _) => Some(Line::Clear(flag(&args))),
            ("proxycommand", Some(_)) => Some(Line::ProxyCommand(rest.to_string())),
            ("proxyjump", Some(v)) => Some(Line::ProxyJump(v.clone())),
            ("identityagent", Some(_)) => {
                Some(Line::IdentityAgent(rest.trim_matches('"').to_string()))
            }
            ("sendenv", _) => Some(Line::SendEnv(args)),
            ("setenv", _) => Some(Line::SetEnv(
                args.iter()
                    .filter_map(|w| w.split_once('=').map(|(k, v)| (k.into(), v.into())))
                    .collect(),
            )),
            ("remotecommand", Some(_)) => Some(Line::RemoteCommand(rest.to_string())),
            ("requesttty", _) => {
                Some(Line::RequestTty(!first.is_some_and(|a| {
                    a.eq_ignore_ascii_case("no") || a.eq_ignore_ascii_case("false")
                })))
            }
            ("stricthostkeychecking", Some(v)) => {
                Some(Line::StrictHostKeyChecking(v.to_lowercase()))
            }
            ("userknownhostsfile", Some(v)) => Some(Line::UserKnownHostsFile(v.clone())),
            ("forwardx11", _) => Some(Line::ForwardX11(flag(&args))),
            ("forwardx11trusted", _) => Some(Line::ForwardX11Trusted(flag(&args))),
            ("forwardx11timeout", Some(v)) => Some(Line::ForwardX11Timeout(v.clone())),
            ("xauthlocation", Some(v)) => Some(Line::XAuthLocation(v.clone())),
            ("identitiesonly", _) => Some(Line::IdentitiesOnly(flag(&args))),
            _ => None,
        };
        if let Some(parsed) = parsed {
            let mut blocks = within.to_vec();
            blocks.push(block.clone());
            out.push((blocks, parsed));
        }
    }
}

/// The forwards the config file at `path` gives `alias`, as ssh reads them:
/// every `LocalForward`, `RemoteForward` and `DynamicForward` of every block
/// that applies, in order, unless `ClearAllForwardings yes` comes first.
pub fn forwards_for(path: &Path, alias: &str) -> Vec<String> {
    let lines = scan(path);
    let reading = Reading {
        lines: &lines,
        exec: Exec::Skip,
        ran: Default::default(),
    };
    forwards_in(&reading.applying(alias, None))
}

fn forwards_in(mine: &[&Line]) -> Vec<String> {
    let clear = mine.iter().find_map(|l| match l {
        Line::Clear(on) => Some(*on),
        _ => None,
    });
    if clear == Some(true) {
        return vec![];
    }
    let mut out: Vec<String> = vec![];
    for l in mine {
        if let Line::Forward(spec) = l
            && !out.contains(spec)
        {
            out.push(spec.clone());
        }
    }
    out
}

fn resolve_with(reading: &Reading, spec: &str, chain: &mut Vec<String>) -> Result<Resolved, Error> {
    let (user, alias, port) = split(spec)?;
    if chain.contains(&alias) || chain.len() > MAX_JUMPS {
        return Err(Error::Config(format!(
            "ProxyJump loops or is too long at {alias}"
        )));
    }
    let mine = reading.applying(&alias, user.as_deref());
    // The first value wins, as in ssh, but for what adds up.
    macro_rules! first {
        ($pattern:pat => $value:expr) => {
            mine.iter().find_map(|l| match *l {
                $pattern => Some($value),
                _ => None,
            })
        };
    }
    let host = first!(Line::HostName(h) => host_name(h, &alias)).unwrap_or_else(|| alias.clone());
    let user = user
        .or(first!(Line::User(u) => u.clone()))
        .unwrap_or_else(local_user);
    let port = match (port, first!(Line::Port(p) => p)) {
        (Some(given), _) => given,
        (None, Some(p)) => p
            .parse()
            .map_err(|_| Error::Config(format!("bad Port {p:?} for {alias}")))?,
        (None, None) => 22,
    };
    let identity_files = mine
        .iter()
        .filter_map(|l| match l {
            Line::IdentityFile(f) if f != "none" => Some(config_path(f, &host, &user)),
            _ => None,
        })
        .collect();
    let certificate_files = mine
        .iter()
        .filter_map(|l| match l {
            Line::CertificateFile(f) if f != "none" => Some(config_path(f, &host, &user)),
            _ => None,
        })
        .collect();

    // The first ProxyJump or ProxyCommand for the host wins, as in OpenSSH;
    // either one set to `none` turns both off (`ProxyCommand none` on a
    // bastion that `Host *` would send through itself).
    let proxy = mine
        .iter()
        .find(|l| matches!(l, Line::ProxyCommand(_) | Line::ProxyJump(_)));
    let proxy_command = match proxy {
        Some(Line::ProxyCommand(c)) if c != "none" => Some(c.clone()),
        _ => None,
    };
    let identity_agent =
        first!(Line::IdentityAgent(v) => identity_agent(v, &host, &user)).unwrap_or_default();
    let identities_only = first!(Line::IdentitiesOnly(on) => *on).unwrap_or(false);
    // SendEnv adds up over every block that applies; `-pattern` takes back
    // the patterns it matches. The others: the first value wins.
    let mut send_env: Vec<String> = vec![];
    for l in &mine {
        if let Line::SendEnv(patterns) = l {
            for p in patterns {
                match p.strip_prefix('-') {
                    Some(taken) => send_env.retain(|had| !wildcard(taken, had)),
                    None if !send_env.contains(p) => send_env.push(p.clone()),
                    None => {}
                }
            }
        }
    }
    let mut set_env: Vec<(String, String)> = vec![];
    if let Some(vars) = first!(Line::SetEnv(vars) => vars) {
        for (name, value) in vars {
            if !set_env.iter().any(|(had, _)| had == name) {
                set_env.push((name.clone(), value.clone()));
            }
        }
    }
    let remote_command = first!(Line::RemoteCommand(c) => c)
        .filter(|c| *c != "none")
        .cloned();
    let tty = first!(Line::RequestTty(on) => *on).unwrap_or(true);
    // `no` and `off` would let even a changed key in; shellwing takes them
    // as `accept-new`, which is what they are wanted for (hosts made and
    // thrown away, often with UserKnownHostsFile /dev/null).
    let host_key = first!(Line::StrictHostKeyChecking(v) => v).and_then(|v| match v.as_str() {
        "yes" | "true" => Some(crate::HostKeyPolicy::Strict),
        "ask" => Some(crate::HostKeyPolicy::Ask),
        "accept-new" | "no" | "off" | "false" => Some(crate::HostKeyPolicy::AcceptNew),
        _ => None,
    });
    let known_hosts = first!(Line::UserKnownHostsFile(v) => match v.as_str() {
        "/dev/null" | "none" => null_device(),
        v => config_path(v, &host, &user),
    });
    let x11 = first!(Line::ForwardX11(on) => *on)
        .unwrap_or(false)
        .then(|| {
            let defaults = X11::default();
            X11 {
                trusted: first!(Line::ForwardX11Trusted(on) => *on).unwrap_or(defaults.trusted),
                timeout: first!(Line::ForwardX11Timeout(v) => v)
                    .and_then(|v| time_value(v))
                    .unwrap_or(defaults.timeout),
                xauth: first!(Line::XAuthLocation(v) => config_path(v, &host, &user)),
            }
        });
    let mut jumps = vec![];
    if let Some(Line::ProxyJump(list)) = proxy
        && !list.eq_ignore_ascii_case("none")
    {
        chain.push(alias.clone());
        for hop in list.split(',').map(str::trim).filter(|h| !h.is_empty()) {
            // A jump host may itself be configured with a ProxyJump: flatten it.
            let mut j = resolve_with(reading, hop, chain)?;
            jumps.append(&mut j.jumps);
            jumps.push(j);
        }
        chain.pop();
    }
    Ok(Resolved {
        keepalive: first!(Line::ServerAliveInterval(v) => v).and_then(|v| time_value(v)),
        forward_agent: first!(Line::ForwardAgent(on) => *on).unwrap_or(false),
        connect_timeout: first!(Line::ConnectTimeout(v) => v)
            .and_then(|v| time_value(v))
            .filter(|t| !t.is_zero()),
        alias,
        target: Target { user, host, port },
        identity_files,
        certificate_files,
        vault_key: None,
        identity_agent,
        identities_only,
        jumps,
        proxy_command,
        forwards: vec![],
        host_key,
        known_hosts,
        x11,
        remote_command,
        tty,
        send_env,
        set_env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_add_up_as_in_ssh() {
        let dir = std::env::temp_dir().join(format!("shellwing-fwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("conf.d")).expect("dir");
        std::fs::write(
            dir.join("conf.d/10-extra.conf"),
            "Host web\n  LocalForward 7000 y:7\n",
        )
        .expect("include");
        let main = dir.join("config");
        std::fs::write(
            &main,
            format!(
                "LocalForward 1111 localhost:1\n\
                 Host web\n  LocalForward 8080 localhost:80\n  LocalForward=127.0.0.1:8443 localhost:443\n  DynamicForward 1080\n\
                 Host web*\n  RemoteForward 9000 localhost:3000\n  LocalForward 8080 localhost:80\n\
                 Host * !web\n  LocalForward 5555 x:5\n\
                 Host quiet\n  ClearAllForwardings yes\n\
                 Match all\n  Include {}/conf.d/*.conf\n\
                 Match user root\n  LocalForward 6666 z:6\n",
                dir.display()
            ),
        )
        .expect("config");
        assert_eq!(
            forwards_for(&main, "WEB"),
            [
                "-L 1111:localhost:1",
                "-L 8080:localhost:80",
                "-L 127.0.0.1:8443:localhost:443",
                "-D 1080",
                "-R 9000:localhost:3000",
                "-L 7000:y:7",
            ]
        );
        assert_eq!(forwards_for(&main, "quiet"), Vec::<String>::new());
        assert_eq!(
            forwards_for(&main, "db"),
            ["-L 1111:localhost:1", "-L 5555:x:5"]
        );
        // Each one reads as a forward.
        for spec in forwards_for(&main, "web") {
            spec.parse::<crate::Forward>().expect("a forward");
        }
        // Resolving gives the host its forwards, and a jump host none.
        let r = resolve("web", Some(&main)).expect("resolve");
        assert_eq!(r.forwards.len(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_patterns_match_like_ssh() {
        let p = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(host_matches(&p(&["*.example.com"]), "api.EXAMPLE.com"));
        assert!(host_matches(&p(&["web?"]), "web1"));
        assert!(!host_matches(&p(&["web?"]), "web12"));
        assert!(!host_matches(&p(&["*", "!bastion"]), "bastion"));
        assert!(host_matches(&p(&["*", "!bastion"]), "api"));
        assert!(
            !host_matches(&p(&["!api"]), "web"),
            "a negation alone matches nothing"
        );
    }

    fn config(text: &str) -> PathBuf {
        // A folder of its own for each, as tests run side by side.
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "shellwing-cfg-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("config");
        std::fs::write(&path, text).expect("write");
        path
    }

    #[test]
    fn uses_the_host_block_and_command_line_wins() {
        let cfg = config(
            "Host web\n  HostName 10.0.0.5\n  User deploy\n  Port 2200\n  IdentityFile ~/.ssh/web_%r\n  ServerAliveInterval 15\n",
        );
        let r = resolve("web", Some(&cfg)).expect("resolve");
        assert_eq!(
            r.target,
            Target {
                user: "deploy".into(),
                host: "10.0.0.5".into(),
                port: 2200
            }
        );
        assert_eq!(r.keepalive, Some(Duration::from_secs(15)));
        assert!(
            r.identity_files[0]
                .to_string_lossy()
                .ends_with(".ssh/web_deploy")
        );
        let r = resolve("root@web:22", Some(&cfg)).expect("resolve");
        assert_eq!((r.target.user.as_str(), r.target.port), ("root", 22));
    }

    #[test]
    fn proxy_command_is_the_rest_of_the_line_as_written() {
        let cfg = config(
            "Host i-*\n  User ec2-user\n  ProxyCommand sh -c \"aws ssm start-session --target %h --parameters 'portNumber=%p'\"\nHost tunnel\n  ProxyCommand=cloudflared access ssh --hostname %h\n",
        );
        let r = resolve("i-0abc", Some(&cfg)).expect("resolve");
        assert_eq!(
            r.proxy_command.as_deref(),
            Some("sh -c \"aws ssm start-session --target %h --parameters 'portNumber=%p'\"")
        );
        assert_eq!(r.target.user, "ec2-user");
        assert!(r.jumps.is_empty());
        let r = resolve("tunnel", Some(&cfg)).expect("resolve");
        assert_eq!(
            r.proxy_command.as_deref(),
            Some("cloudflared access ssh --hostname %h")
        );
        assert_eq!(
            resolve("other", Some(&cfg)).expect("resolve").proxy_command,
            None
        );
    }

    #[test]
    fn the_first_of_proxy_jump_and_proxy_command_wins() {
        let cfg = config(
            "Host bastion\n  HostName bastion.example.com\n  ProxyCommand none\nHost web\n  ProxyCommand cloudflared access ssh --hostname %h\nHost app\n  ProxyJump edge\nHost edge\n  ProxyCommand nc %h %p\nHost *\n  ProxyJump bastion\n",
        );
        // `none` first: neither, though `Host *` would jump through itself.
        let r = resolve("bastion", Some(&cfg)).expect("resolve");
        assert_eq!((r.proxy_command, r.jumps.len()), (None, 0));
        let r = resolve("web", Some(&cfg)).expect("resolve");
        assert_eq!(
            (r.proxy_command.as_deref(), r.jumps.len()),
            (Some("cloudflared access ssh --hostname %h"), 0)
        );
        let r = resolve("db", Some(&cfg)).expect("resolve");
        assert_eq!(r.proxy_command, None);
        let hops: Vec<&str> = r.jumps.iter().map(|j| j.alias.as_str()).collect();
        assert_eq!(hops, ["bastion"]);
        assert_eq!(r.jumps[0].proxy_command, None);
        // A jump host has its own: edge's command comes before `Host *`'s
        // ProxyJump, which it skips.
        let r = resolve("edge", Some(&cfg)).expect("resolve");
        assert_eq!(
            (r.proxy_command.as_deref(), r.jumps.len()),
            (Some("nc %h %p"), 0)
        );
        let r = resolve("app", Some(&cfg)).expect("resolve");
        assert_eq!(r.proxy_command, None);
        let hops: Vec<&str> = r.jumps.iter().map(|j| j.alias.as_str()).collect();
        assert_eq!(hops, ["edge"]);
        assert_eq!(r.jumps[0].proxy_command.as_deref(), Some("nc %h %p"));
    }

    #[test]
    fn identity_agent_and_identities_only_as_ssh_reads_them() {
        let home = std::env::home_dir().expect("home");
        let path = std::env::var("PATH").expect("PATH");
        let cfg = config(
            "Host op\n  IdentityAgent \"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock\"\n  IdentitiesOnly yes\nHost off\n  IdentityAgent none\nHost env\n  IdentityAgent $PATH\nHost unset\n  IdentityAgent $SHELLWING_NO_SUCH_AGENT\nHost braces\n  IdentityAgent ${PATH}/agent-%r.sock\nHost usual\n  IdentityAgent SSH_AUTH_SOCK\nHost *\n  IdentityAgent /run/second.sock\n  IdentitiesOnly no\n",
        );
        let r = resolve("op", Some(&cfg)).expect("resolve");
        assert_eq!(
            r.identity_agent,
            IdentityAgent::At(
                home.join("Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock")
            )
        );
        assert!(r.identities_only);
        let agent = |name: &str| resolve(name, Some(&cfg)).expect("resolve").identity_agent;
        assert_eq!(agent("off"), IdentityAgent::Off);
        assert_eq!(agent("env"), IdentityAgent::At(PathBuf::from(&path)));
        assert_eq!(agent("unset"), IdentityAgent::Off);
        assert_eq!(
            agent("me@braces"),
            IdentityAgent::At(PathBuf::from(format!("{path}/agent-me.sock")))
        );
        assert_eq!(agent("usual"), IdentityAgent::Usual);
        // The first value wins: Host *'s comes after the others'.
        assert_eq!(agent("other"), IdentityAgent::At("/run/second.sock".into()));
        assert!(
            !resolve("other", Some(&cfg))
                .expect("resolve")
                .identities_only
        );
        assert_eq!(
            resolve("x", None).expect("resolve").identity_agent,
            IdentityAgent::Usual
        );
    }

    #[test]
    fn session_options_as_ssh_reads_them() {
        let cfg = config(
            "Host box\n  SendEnv LANG LC_*\n  SetEnv GREETING=\"hello there\" EMPTY= 'QUOTED=a b'\n  RemoteCommand tmux new -A -s %r\n  RequestTTY force\nHost plain\n  RemoteCommand none\n  RequestTTY no\nHost *\n  SendEnv -LC_* EDITOR\n  SetEnv LATER=never\n  RemoteCommand echo never\n",
        );
        let r = resolve("box", Some(&cfg)).expect("resolve");
        // Every block adds; `-LC_*` takes back what came before it.
        assert_eq!(r.send_env, ["LANG", "EDITOR"]);
        // The first SetEnv that applies, all of it; later ones not at all.
        assert_eq!(
            r.set_env,
            [
                ("GREETING".to_string(), "hello there".to_string()),
                ("EMPTY".into(), "".into()),
                ("QUOTED".into(), "a b".into())
            ]
        );
        assert_eq!(r.remote_command.as_deref(), Some("tmux new -A -s %r"));
        assert!(r.tty);
        let r = resolve("plain", Some(&cfg)).expect("resolve");
        assert_eq!(r.remote_command, None, "none, before Host *'s");
        assert!(!r.tty);
        assert_eq!(r.send_env, ["EDITOR"]);
        assert_eq!(r.set_env, [("LATER".to_string(), "never".to_string())]);
        let r = resolve("x", None).expect("resolve");
        assert!(r.tty && r.remote_command.is_none() && r.send_env.is_empty());
    }

    #[test]
    fn host_keys_and_timeouts_as_ssh_reads_them() {
        let home = std::env::home_dir().expect("home");
        let cfg = config(
            "Host throwaway-*\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  ConnectTimeout 5\nHost work\n  StrictHostKeyChecking yes\n  UserKnownHostsFile ~/.ssh/known_hosts_work ~/.ssh/known_hosts_other\nHost per-host\n  UserKnownHostsFile ~/.ssh/known/%h\n  StrictHostKeyChecking accept-new\nHost *\n  StrictHostKeyChecking ask\n  ConnectTimeout 0\n",
        );
        let r = resolve("throwaway-1", Some(&cfg)).expect("resolve");
        // `no` is taken as accept-new: a changed key still stops it.
        assert_eq!(r.host_key, Some(crate::HostKeyPolicy::AcceptNew));
        assert_eq!(r.known_hosts, Some(null_device()));
        assert_eq!(r.connect_timeout, Some(Duration::from_secs(5)));
        let r = resolve("work", Some(&cfg)).expect("resolve");
        assert_eq!(r.host_key, Some(crate::HostKeyPolicy::Strict));
        assert_eq!(r.known_hosts, Some(home.join(".ssh/known_hosts_work")));
        // ConnectTimeout 0: none, as in ssh.
        assert_eq!(r.connect_timeout, None);
        let r = resolve("per-host", Some(&cfg)).expect("resolve");
        assert_eq!(r.known_hosts, Some(home.join(".ssh/known/per-host")));
        assert_eq!(r.host_key, Some(crate::HostKeyPolicy::AcceptNew));
        let r = resolve("other", Some(&cfg)).expect("resolve");
        assert_eq!(r.host_key, Some(crate::HostKeyPolicy::Ask));
        assert_eq!(r.known_hosts, None);
    }

    #[test]
    fn x11_as_ssh_reads_it() {
        let cfg = config(
            "Host limited\n  ForwardX11 yes\nHost full\n  ForwardX11 yes\n  ForwardX11Trusted yes\n  ForwardX11Timeout 1h30m\n  XAuthLocation /opt/X11/bin/xauth\nHost never\n  ForwardX11Timeout 0\n  ForwardX11 yes\nHost off\n  ForwardX11 no\nHost *\n  ForwardX11 yes\n",
        );
        let x11 = |name: &str| resolve(name, Some(&cfg)).expect("resolve").x11;
        assert_eq!(x11("limited"), Some(X11::default()));
        assert_eq!(
            x11("full"),
            Some(X11 {
                trusted: true,
                timeout: Duration::from_secs(5400),
                xauth: Some("/opt/X11/bin/xauth".into()),
            })
        );
        assert_eq!(x11("never").map(|x| x.timeout), Some(Duration::ZERO));
        assert_eq!(x11("off"), None, "no, before Host *'s yes");
        assert_eq!(resolve("x", None).expect("resolve").x11, None);
        assert_eq!(time_value("300"), Some(Duration::from_secs(300)));
        assert_eq!(time_value("20m"), Some(Duration::from_secs(1200)));
        assert_eq!(time_value("soon"), None);
    }

    #[cfg(unix)]
    #[test]
    fn match_exec_runs_only_when_connecting_and_not_for_long() {
        let dir = std::env::temp_dir().join(format!("shellwing-exec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let ran = dir.join("ran");
        let cfg = config(&format!(
            "Match exec \"touch {}\"\n  User yes\n",
            ran.display()
        ));
        // Only looking at a host runs nothing, and the condition is false.
        let r = resolve_without_commands("web", Some(&cfg)).expect("resolve");
        assert!(!ran.exists(), "the command ran");
        assert_ne!(r.target.user, "yes");
        let r = resolve("web", Some(&cfg)).expect("resolve");
        assert!(ran.exists(), "the command did not run");
        assert_eq!(r.target.user, "yes");
        // A command that takes too long counts as false, and does not hold
        // the app up for long.
        let cfg = config("Match exec \"sleep 30\"\n  User yes\n");
        let started = std::time::Instant::now();
        let r = resolve("web", Some(&cfg)).expect("resolve");
        assert_ne!(r.target.user, "yes");
        assert!(started.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn match_conditions_not_weighed_hold_for_nobody() {
        let cfg = config(
            "Match localnetwork 10.0.0.0/8\n  User a\nMatch !localnetwork 10.0.0.0/8\n  User b\nMatch canonical all\n  User c\nMatch something-new x\n  User d\nMatch\n  User e\nMatch host\n  User f\n",
        );
        let r = resolve("web", Some(&cfg)).expect("resolve");
        assert!(!["a", "b", "c", "d", "e", "f"].contains(&r.target.user.as_str()));
    }

    #[test]
    fn a_bad_port_is_refused() {
        let cfg = config("Host web\n  Port many\n");
        assert!(matches!(resolve("web", Some(&cfg)), Err(Error::Config(_))));
        assert_eq!(
            resolve("me@web:22", Some(&cfg))
                .expect("resolve")
                .target
                .port,
            22
        );
    }

    #[test]
    fn flattens_nested_proxy_jumps() {
        let cfg = config(
            "Host db\n  HostName db.internal\n  ProxyJump bastion\nHost bastion\n  HostName bastion.example.com\n  User jump\n  ProxyJump edge\nHost edge\n  HostName edge.example.com\n",
        );
        let r = resolve("db", Some(&cfg)).expect("resolve");
        let hops: Vec<&str> = r.jumps.iter().map(|j| j.target.host.as_str()).collect();
        assert_eq!(hops, ["edge.example.com", "bastion.example.com"]);
        assert_eq!(r.jumps[1].target.user, "jump");
        assert!(r.jumps.iter().all(|j| j.jumps.is_empty()));
    }

    #[test]
    fn lists_plain_aliases_only() {
        let cfg = config("Host web db\n  User x\nHost *.internal !bad\nHost web nas\n");
        assert_eq!(aliases(Some(&cfg)), ["web", "db", "nas"]);
        assert!(aliases(None).is_empty());
    }

    #[test]
    fn refuses_jump_loops() {
        let cfg = config("Host a\n  ProxyJump b\nHost b\n  ProxyJump a\n");
        assert!(matches!(resolve("a", Some(&cfg)), Err(Error::Config(_))));
    }

    #[test]
    fn hosts_without_a_block_use_defaults() {
        let r = resolve("me@plain.example:2022", None).expect("resolve");
        assert_eq!(
            r.target,
            Target {
                user: "me".into(),
                host: "plain.example".into(),
                port: 2022
            }
        );
        assert!(r.jumps.is_empty() && r.identity_files.is_empty());
    }
}

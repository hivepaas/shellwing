//! Resolving a target through `~/.ssh/config`, as `ssh` would.
//!
//! `shellwing web` uses the `Host web` block: its `HostName`, `User`, `Port`,
//! `IdentityFile`, `IdentityAgent`, `IdentitiesOnly`, `ProxyJump`, `ProxyCommand` and
//! `ServerAliveInterval`. Values typed on the
//! command line (`user@host:port`) win over the file, and the file wins over the
//! defaults, matching OpenSSH's precedence.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ssh2_config::{ParseRule, SshConfig};

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
    /// How an unknown host key is handled here; None: the app's setting.
    pub host_key: Option<crate::HostKeyPolicy>,
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
        ["id_ed25519", "id_ecdsa", "id_rsa"]
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

fn load(path: Option<&Path>) -> Result<SshConfig, Error> {
    let Some(path) = path.filter(|p| p.exists()) else {
        return Ok(SshConfig::default());
    };
    let file = File::open(path).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
    SshConfig::default()
        .parse(
            &mut BufReader::new(file),
            ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
        )
        .map_err(|e| Error::Config(format!("{}: {e}", path.display())))
}

/// The names `Host` lines give, in file order, leaving out patterns (`*`, `?`)
/// and negations: the hosts a person can connect to by name.
pub fn aliases(config: Option<&Path>) -> Vec<String> {
    let Ok(cfg) = load(config) else {
        return vec![];
    };
    let mut out: Vec<String> = vec![];
    for clause in cfg.get_hosts().iter().flat_map(|h| &h.pattern) {
        if !clause.negated
            && !clause.pattern.contains(['*', '?', '!'])
            && !out.contains(&clause.pattern)
        {
            out.push(clause.pattern.clone());
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
        v => {
            let mut path = String::new();
            let mut rest = v;
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
            IdentityAgent::At(expand_tokens(&path, host, user))
        }
    }
}

/// Resolves `[user@]host[:port]` through the config file at `config` (none: defaults only).
pub fn resolve(spec: &str, config: Option<&Path>) -> Result<Resolved, Error> {
    let cfg = load(config)?;
    let lines = config.filter(|p| p.exists()).map(scan).unwrap_or_default();
    let mut r = resolve_with(&cfg, &lines, spec, &mut vec![])?;
    // The host's own forwards; jump hosts forward nothing, as with ssh -J.
    r.forwards = forwards_in(&lines, &r.alias);
    Ok(r)
}

/// How deep `Include`s may nest, as in OpenSSH.
const MAX_INCLUDE: usize = 16;

/// A line of a config file that the parsed config does not keep as ssh
/// reads it.
enum Line {
    /// `LocalForward`, `RemoteForward` or `DynamicForward`, as a `-L`/`-R`/`-D` spec.
    Forward(String),
    /// `ClearAllForwardings`.
    Clear(bool),
    /// `ProxyCommand`, the rest of the line as written: it goes to a shell.
    ProxyCommand(String),
    /// `ProxyJump`: only where it comes, before or after a `ProxyCommand`.
    ProxyJump,
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
}

/// The lines of a config file, each with the `Host` patterns it comes under:
/// None before any `Host` (every host), Some(empty) under a `Match` (not
/// read: no host).
type Lines = Vec<(Option<Vec<String>>, Line)>;

/// Whether a line under `under` applies to `alias`.
fn applies(under: &Option<Vec<String>>, alias: &str) -> bool {
    under.as_ref().is_none_or(|p| host_matches(p, alias))
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
    scan_into(path, None, 0, &mut lines);
    lines
}

fn scan_into(path: &Path, mut under: Option<Vec<String>>, depth: usize, out: &mut Lines) {
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
        let args: Vec<String> = rest
            .split_whitespace()
            .map(|a| a.trim_matches('"').to_string())
            .collect();
        match key.to_lowercase().as_str() {
            "host" => under = Some(args),
            "match" => under = Some(vec![]),
            "include" if depth < MAX_INCLUDE => {
                for a in &args {
                    for file in include_paths(a) {
                        scan_into(&file, under.clone(), depth + 1, out);
                    }
                }
            }
            "localforward" | "remoteforward" | "dynamicforward" if !args.is_empty() => {
                let flag = match key.to_lowercase().as_str() {
                    "localforward" => "-L",
                    "remoteforward" => "-R",
                    _ => "-D",
                };
                out.push((
                    under.clone(),
                    Line::Forward(format!("{flag} {}", args.join(":"))),
                ));
            }
            "clearallforwardings" => out.push((
                under.clone(),
                Line::Clear(args.first().is_some_and(|a| a.eq_ignore_ascii_case("yes"))),
            )),
            "proxycommand" if !rest.is_empty() => {
                out.push((under.clone(), Line::ProxyCommand(rest.to_string())));
            }
            "proxyjump" => out.push((under.clone(), Line::ProxyJump)),
            "identityagent" if !rest.is_empty() => out.push((
                under.clone(),
                Line::IdentityAgent(rest.trim_matches('"').to_string()),
            )),
            "sendenv" => out.push((under.clone(), Line::SendEnv(words(rest)))),
            "setenv" => out.push((
                under.clone(),
                Line::SetEnv(
                    words(rest)
                        .into_iter()
                        .filter_map(|w| w.split_once('=').map(|(k, v)| (k.into(), v.into())))
                        .collect(),
                ),
            )),
            "remotecommand" if !rest.is_empty() => {
                out.push((under.clone(), Line::RemoteCommand(rest.to_string())));
            }
            "requesttty" => out.push((
                under.clone(),
                Line::RequestTty(!args.first().is_some_and(|a| {
                    a.eq_ignore_ascii_case("no") || a.eq_ignore_ascii_case("false")
                })),
            )),
            "identitiesonly" => out.push((
                under.clone(),
                Line::IdentitiesOnly(args.first().is_some_and(|a| {
                    a.eq_ignore_ascii_case("yes") || a.eq_ignore_ascii_case("true")
                })),
            )),
            _ => {}
        }
    }
}

/// The forwards the config file at `path` gives `alias`, as ssh reads them:
/// every `LocalForward`, `RemoteForward` and `DynamicForward` of every block
/// that matches, in order, unless `ClearAllForwardings yes` comes first.
/// (Not taken from the parsed config, which keeps only one of each.)
pub fn forwards_for(path: &Path, alias: &str) -> Vec<String> {
    forwards_in(&scan(path), alias)
}

fn forwards_in(lines: &Lines, alias: &str) -> Vec<String> {
    let clear = lines.iter().find_map(|(under, l)| match l {
        Line::Clear(on) if applies(under, alias) => Some(*on),
        _ => None,
    });
    if clear == Some(true) {
        return vec![];
    }
    let mut out: Vec<String> = vec![];
    for (under, l) in lines {
        if let Line::Forward(spec) = l
            && applies(under, alias)
            && !out.contains(spec)
        {
            out.push(spec.clone());
        }
    }
    out
}

fn resolve_with(
    cfg: &SshConfig,
    lines: &Lines,
    spec: &str,
    chain: &mut Vec<String>,
) -> Result<Resolved, Error> {
    let (user, alias, port) = split(spec)?;
    if chain.contains(&alias) || chain.len() > MAX_JUMPS {
        return Err(Error::Config(format!(
            "ProxyJump loops or is too long at {alias}"
        )));
    }
    let p = cfg.query(&alias);
    let host = p
        .host_name
        .clone()
        .map(|h| h.replace("%h", &alias))
        .unwrap_or_else(|| alias.clone());
    let user = user.or(p.user.clone()).unwrap_or_else(local_user);
    let port = port.or(p.port).unwrap_or(22);
    let identity_files = p
        .identity_file
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|f| expand_tokens(f, &host, &user))
        .collect();
    let certificate_files = p
        .certificate_file
        .iter()
        .map(|f| expand_tokens(f, &host, &user))
        .collect();

    // The first ProxyJump or ProxyCommand for the host wins, as in OpenSSH;
    // either one set to `none` turns both off (`ProxyCommand none` on a
    // bastion that `Host *` would send through itself).
    let first = lines.iter().find_map(|(under, l)| match l {
        Line::ProxyCommand(_) | Line::ProxyJump if applies(under, &alias) => Some(l),
        _ => None,
    });
    let proxy_command = match first {
        Some(Line::ProxyCommand(c)) if c != "none" => Some(c.clone()),
        _ => None,
    };
    let identity_agent = lines
        .iter()
        .find_map(|(under, l)| match l {
            Line::IdentityAgent(v) if applies(under, &alias) => {
                Some(identity_agent(v, &host, &user))
            }
            _ => None,
        })
        .unwrap_or_default();
    let identities_only = lines
        .iter()
        .find_map(|(under, l)| match l {
            Line::IdentitiesOnly(on) if applies(under, &alias) => Some(*on),
            _ => None,
        })
        .unwrap_or(false);
    // SendEnv adds up over every block that applies; `-pattern` takes back
    // the patterns it matches. The others: the first value wins.
    let mut send_env: Vec<String> = vec![];
    for (under, l) in lines {
        if let Line::SendEnv(patterns) = l
            && applies(under, &alias)
        {
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
    if let Some(vars) = lines.iter().find_map(|(under, l)| match l {
        Line::SetEnv(vars) if applies(under, &alias) => Some(vars),
        _ => None,
    }) {
        for (name, value) in vars {
            if !set_env.iter().any(|(had, _)| had == name) {
                set_env.push((name.clone(), value.clone()));
            }
        }
    }
    let remote_command = lines
        .iter()
        .find_map(|(under, l)| match l {
            Line::RemoteCommand(c) if applies(under, &alias) => Some(c),
            _ => None,
        })
        .filter(|c| *c != "none")
        .cloned();
    let tty = lines
        .iter()
        .find_map(|(under, l)| match l {
            Line::RequestTty(on) if applies(under, &alias) => Some(*on),
            _ => None,
        })
        .unwrap_or(true);
    let mut jumps = vec![];
    if !matches!(first, Some(Line::ProxyCommand(_)))
        && let Some(list) = &p.proxy_jump
        && !(list.len() == 1 && list[0].eq_ignore_ascii_case("none"))
    {
        chain.push(alias.clone());
        for hop in list {
            // A jump host may itself be configured with a ProxyJump: flatten it.
            let mut j = resolve_with(cfg, lines, hop, chain)?;
            jumps.append(&mut j.jumps);
            jumps.push(j);
        }
        chain.pop();
    }
    Ok(Resolved {
        alias,
        target: Target { user, host, port },
        identity_files,
        certificate_files,
        vault_key: None,
        identity_agent,
        identities_only,
        jumps,
        proxy_command,
        keepalive: p.server_alive_interval,
        forwards: vec![],
        forward_agent: p.forward_agent.unwrap_or(false),
        host_key: None,
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
                 Match user root\n  LocalForward 6666 z:6\n\
                 Include {}/conf.d/*.conf\n",
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

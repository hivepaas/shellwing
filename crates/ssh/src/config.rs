//! Resolving a target through `~/.ssh/config`, as `ssh` would.
//!
//! `shellwing web` uses the `Host web` block: its `HostName`, `User`, `Port`,
//! `IdentityFile`, `ProxyJump` and `ServerAliveInterval`. Values typed on the
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

/// Everything needed to reach one host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The name as given (an alias or a host name), shown to the user.
    pub alias: String,
    /// The real host name, port and user.
    pub target: Target,
    /// Keys to try, in order. Empty means the OpenSSH defaults.
    pub identity_files: Vec<PathBuf>,
    /// Hosts to go through first, outermost first; each has no jumps of its own.
    pub jumps: Vec<Resolved>,
    pub keepalive: Option<Duration>,
    /// Ports to forward, as `-L`, `-R` and `-D` options are written. Read when
    /// connecting, so one that is wrong is reported without stopping the rest.
    pub forwards: Vec<String>,
    /// Let the host use the local ssh-agent (`ForwardAgent`). Off unless asked:
    /// whoever is root there can then sign with your keys while you are connected.
    pub forward_agent: bool,
    /// How an unknown host key is handled here; None: the app's setting.
    pub host_key: Option<crate::HostKeyPolicy>,
}

impl Resolved {
    /// A host used as is, without consulting any config file.
    pub fn direct(target: Target) -> Self {
        Resolved {
            alias: target.host.clone(),
            target,
            identity_files: vec![],
            jumps: vec![],
            keepalive: None,
            forwards: vec![],
            forward_agent: false,
            host_key: None,
        }
    }

    /// The keys to offer: the configured ones, else the OpenSSH defaults that exist.
    pub fn keys_to_try(&self) -> Vec<PathBuf> {
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

fn local_user() -> String {
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

/// Resolves `[user@]host[:port]` through the config file at `config` (none: defaults only).
pub fn resolve(spec: &str, config: Option<&Path>) -> Result<Resolved, Error> {
    let cfg = load(config)?;
    resolve_with(&cfg, spec, &mut vec![])
}

fn resolve_with(cfg: &SshConfig, spec: &str, chain: &mut Vec<String>) -> Result<Resolved, Error> {
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

    let mut jumps = vec![];
    if let Some(list) = &p.proxy_jump
        && !(list.len() == 1 && list[0].eq_ignore_ascii_case("none"))
    {
        chain.push(alias.clone());
        for hop in list {
            // A jump host may itself be configured with a ProxyJump: flatten it.
            let mut j = resolve_with(cfg, hop, chain)?;
            jumps.append(&mut j.jumps);
            jumps.push(j);
        }
        chain.pop();
    }
    Ok(Resolved {
        alias,
        target: Target { user, host, port },
        identity_files,
        jumps,
        keepalive: p.server_alive_interval,
        forwards: vec![],
        forward_agent: p.forward_agent.unwrap_or(false),
        host_key: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shellwing-cfg-{}-{}",
            std::process::id(),
            text.len()
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

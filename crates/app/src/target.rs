//! Which host a name means: an entry in `hosts.toml`, else `~/.ssh/config`, else
//! the name itself as a host name.

use std::path::{Path, PathBuf};

use anyhow::bail;
use shellwing_config::{AuthMethod, Host, Hosts};
use shellwing_ssh::config::{self, Resolved};

/// Jump chains longer than this are refused, so a loop cannot recurse forever.
const MAX_JUMPS: usize = 8;

/// Resolves `[user@]name[:port]`, returning the `hosts.toml` entry when the name
/// was found there. User and port typed on the command line win over the entry.
pub fn resolve(
    spec: &str,
    hosts: &Hosts,
    ssh_config: Option<&Path>,
) -> anyhow::Result<(Resolved, Option<Host>)> {
    let (user, name, port) = config::split(spec)?;
    match hosts.find(&name) {
        Some(h) => Ok((
            from_host(h, user, port, hosts, ssh_config, &mut vec![])?,
            Some(h.clone()),
        )),
        None => Ok((config::resolve(spec, ssh_config)?, None)),
    }
}

fn hop(
    name: &str,
    hosts: &Hosts,
    ssh_config: Option<&Path>,
    chain: &mut Vec<String>,
) -> anyhow::Result<Resolved> {
    match hosts.find(name) {
        Some(h) => from_host(h, None, None, hosts, ssh_config, chain),
        None => Ok(config::resolve(name, ssh_config)?),
    }
}

fn from_host(
    h: &Host,
    user: Option<String>,
    port: Option<u16>,
    hosts: &Hosts,
    ssh_config: Option<&Path>,
    chain: &mut Vec<String>,
) -> anyhow::Result<Resolved> {
    if chain.contains(&h.name) || chain.len() > MAX_JUMPS {
        bail!("hosts.toml: jump hosts loop or go too deep at {}", h.name);
    }
    // Start from what ssh would use for the address, then apply the entry.
    let mut r = config::resolve(&h.address, ssh_config)?;
    r.alias = h.name.clone();
    if let Some(u) = user.or_else(|| h.user.clone()) {
        r.target.user = u;
    }
    if let Some(p) = port.or(h.port) {
        r.target.port = p;
    }
    r.forwards = h.forward.clone();
    if h.auth.method == AuthMethod::Key
        && let Some(key) = &h.auth.key
    {
        r.identity_files = vec![expand_home(key)];
    }
    if !h.jump.is_empty() {
        chain.push(h.name.clone());
        let mut jumps = vec![];
        for name in &h.jump {
            // A jump host may have jumps of its own: flatten, outermost first.
            let mut j = hop(name, hosts, ssh_config, chain)?;
            jumps.append(&mut j.jumps);
            jumps.push(j);
        }
        chain.pop();
        r.jumps = jumps;
    }
    Ok(r)
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(text: &str) -> Hosts {
        toml::from_str(text).expect("hosts")
    }

    const SAMPLE: &str = r#"
        [[host]]
        name = "api-1"
        alias = "api1"
        address = "10.0.1.11"
        user = "deploy"
        port = 2222
        jump = ["bastion", "edge.example.com"]
        forward = ["-L 8080:localhost:80"]
        auth = { method = "key", key = "/keys/deploy" }

        [[host]]
        name = "bastion"
        address = "bastion.example.com"
        user = "jump"
        jump = ["outer"]

        [[host]]
        name = "outer"
        address = "outer.example.com"
    "#;

    #[test]
    fn uses_the_entry_and_its_jumps() {
        let h = hosts(SAMPLE);
        let (r, entry) = resolve("api1", &h, None).expect("resolve");
        assert_eq!(entry.map(|e| e.name), Some("api-1".into()));
        assert_eq!(r.alias, "api-1");
        assert_eq!(r.target.to_string(), "deploy@10.0.1.11:2222");
        assert_eq!(r.identity_files, [PathBuf::from("/keys/deploy")]);
        assert_eq!(r.forwards, ["-L 8080:localhost:80"]);
        let hops: Vec<String> = r.jumps.iter().map(|j| j.target.to_string()).collect();
        let me = config::resolve("x", None).expect("x").target.user;
        assert_eq!(
            hops,
            [
                format!("{me}@outer.example.com"),
                "jump@bastion.example.com".to_string(),
                format!("{me}@edge.example.com"),
            ]
        );
        assert!(r.jumps.iter().all(|j| j.jumps.is_empty()));
    }

    #[test]
    fn the_command_line_wins_over_the_entry() {
        let (r, _) = resolve("root@api-1:22", &hosts(SAMPLE), None).expect("resolve");
        assert_eq!(r.target.to_string(), "root@10.0.1.11");
    }

    #[test]
    fn unknown_names_fall_through_to_ssh() {
        let (r, entry) = resolve("me@plain.example:2022", &hosts(SAMPLE), None).expect("resolve");
        assert!(entry.is_none());
        assert_eq!(r.target.to_string(), "me@plain.example:2022");
    }

    #[test]
    fn refuses_jump_loops() {
        let h = hosts(
            "[[host]]\nname = \"a\"\naddress = \"a\"\njump = [\"b\"]\n[[host]]\nname = \"b\"\naddress = \"b\"\njump = [\"a\"]\n",
        );
        let err = resolve("a", &h, None).expect_err("loop");
        assert!(err.to_string().contains("loop"), "{err}");
    }
}

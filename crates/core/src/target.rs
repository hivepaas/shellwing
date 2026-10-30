//! Which host a name means: an entry in `hosts.toml`, else `~/.ssh/config`, else
//! the name itself as a host name.

use std::path::{Path, PathBuf};

use anyhow::bail;
use shellwing_config::{AuthMethod, Host, HostKey, Hosts};
use shellwing_ssh::HostKeyPolicy;
use shellwing_ssh::config::{self, Resolved};

/// The connection's policy for a host key setting.
pub fn policy(k: HostKey) -> HostKeyPolicy {
    match k {
        HostKey::Ask => HostKeyPolicy::Ask,
        HostKey::AcceptNew => HostKeyPolicy::AcceptNew,
        HostKey::Strict => HostKeyPolicy::Strict,
    }
}

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
    // The entry's forwards add to those ~/.ssh/config gives the address.
    for f in &h.forward {
        if !r.forwards.contains(f) {
            r.forwards.push(f.clone());
        }
    }
    // On here, or `ForwardAgent yes` for the address in ~/.ssh/config.
    r.forward_agent |= h.forward_agent;
    if let Some(k) = h.keepalive {
        r.keepalive = Some(std::time::Duration::from_secs(k));
    }
    // Set here, it wins over StrictHostKeyChecking for the address.
    if let Some(k) = h.host_key {
        r.host_key = Some(policy(k));
    }
    if h.auth.method == AuthMethod::Key
        && let Some(key) = &h.auth.key
    {
        // That key only: an agent's others first could use up the server's
        // tries (MaxAuthTries) before it, as IdentitiesOnly keeps them out.
        r.identities_only = true;
        // `vault:<id>`: a key the app keeps; else a key file.
        match key.strip_prefix(VAULT_KEY) {
            Some(id) => r.vault_key = Some(id.to_string()),
            None => r.identity_files = vec![expand_home(key)],
        }
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
        // The entry's jump hosts replace how ~/.ssh/config reaches the
        // address, a ProxyCommand too.
        r.jumps = jumps;
        r.proxy_command = None;
    } else if let Some(command) = h.proxy_command.as_deref().map(str::trim)
        && !command.is_empty()
    {
        // Or its own command, which replaces them as well.
        r.proxy_command = Some(command.to_string());
        r.jumps.clear();
    }
    Ok(r)
}

/// How hosts.toml names a key the app keeps: `key = "vault:<id>"`.
pub const VAULT_KEY: &str = "vault:";

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
        forward_agent = true
        keepalive = 10
        host_key = "strict"
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
        assert!(r.identities_only);
        assert_eq!(r.forwards, ["-L 8080:localhost:80"]);
        assert!(r.forward_agent);
        assert_eq!(r.keepalive, Some(std::time::Duration::from_secs(10)));
        assert_eq!(r.host_key, Some(HostKeyPolicy::Strict));
        assert!(
            r.jumps.iter().all(|j| !j.forward_agent),
            "jump hosts get no agent"
        );
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
    fn a_key_the_app_keeps_is_named_by_its_id() {
        let h = hosts(
            "[[host]]\nname = \"v\"\naddress = \"v.example\"\n[host.auth]\nmethod = \"key\"\nkey = \"vault:0192ab\"\n",
        );
        let (r, _) = resolve("v", &h, None).expect("resolve");
        assert_eq!(r.vault_key.as_deref(), Some("0192ab"));
        assert!(r.identities_only, "the agent's other keys first");
        assert!(r.identity_files.is_empty() && r.keys_to_try().is_empty());
    }

    #[test]
    fn the_entrys_jumps_replace_a_proxy_command() {
        let dir = std::env::temp_dir().join(format!("shellwing-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let ssh = dir.join("config");
        std::fs::write(
            &ssh,
            "Host *.internal\n  ProxyCommand cloudflared access ssh --hostname %h\n",
        )
        .expect("config");
        let h = hosts(
            "[[host]]\nname = \"db\"\naddress = \"db.internal\"\n[[host]]\nname = \"via\"\naddress = \"db.internal\"\njump = [\"bastion.example.com\"]\n",
        );
        let (r, _) = resolve("db", &h, Some(&ssh)).expect("resolve");
        assert_eq!(
            r.proxy_command.as_deref(),
            Some("cloudflared access ssh --hostname %h")
        );
        let (r, _) = resolve("via", &h, Some(&ssh)).expect("resolve");
        assert_eq!(r.proxy_command, None);
        assert_eq!(r.jumps.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_entrys_host_key_wins_over_the_ssh_config() {
        let dir = std::env::temp_dir().join(format!("shellwing-hostkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let ssh = dir.join("config");
        std::fs::write(&ssh, "Host *.lab\n  StrictHostKeyChecking no\n").expect("config");
        let h = hosts(
            "[[host]]\nname = \"a\"\naddress = \"a.lab\"\n[[host]]\nname = \"b\"\naddress = \"b.lab\"\nhost_key = \"strict\"\n",
        );
        let (r, _) = resolve("a", &h, Some(&ssh)).expect("resolve");
        assert_eq!(
            r.host_key,
            Some(HostKeyPolicy::AcceptNew),
            "the ssh config's"
        );
        let (r, _) = resolve("b", &h, Some(&ssh)).expect("resolve");
        assert_eq!(r.host_key, Some(HostKeyPolicy::Strict));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_entrys_own_command_replaces_the_route_and_jumps_win() {
        let dir = std::env::temp_dir().join(format!("shellwing-proxyentry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let ssh = dir.join("config");
        std::fs::write(&ssh, "Host *.internal\n  ProxyJump bastion.example.com\n").expect("config");
        let h = hosts(
            "[[host]]\nname = \"db\"\naddress = \"db.internal\"\nproxy_command = \"cloudflared access ssh --hostname %h\"\n[[host]]\nname = \"both\"\naddress = \"x.example\"\njump = [\"edge.example\"]\nproxy_command = \"nc %h %p\"\n[[host]]\nname = \"blank\"\naddress = \"y.internal\"\nproxy_command = \"  \"\n",
        );
        let (r, _) = resolve("db", &h, Some(&ssh)).expect("resolve");
        assert_eq!(
            r.proxy_command.as_deref(),
            Some("cloudflared access ssh --hostname %h")
        );
        assert!(r.jumps.is_empty(), "the ssh config's ProxyJump is replaced");
        let (r, _) = resolve("both", &h, Some(&ssh)).expect("resolve");
        assert_eq!((r.proxy_command, r.jumps.len()), (None, 1), "jump wins");
        let (r, _) = resolve("blank", &h, Some(&ssh)).expect("resolve");
        assert_eq!((r.proxy_command, r.jumps.len()), (None, 1), "blank: none");
        let _ = std::fs::remove_dir_all(&dir);
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

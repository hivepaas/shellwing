//! `~/.ssh/config` read as OpenSSH reads it: for each config below, what
//! shellwing resolves is compared with what `ssh -G` prints, field by field.
//! Nothing connects anywhere, so these run with `cargo test`, where there is
//! an OpenSSH client.
#![cfg(unix)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use shellwing_ssh::Resolved;
use shellwing_ssh::config::resolve;

/// `ssh -G`'s answer: each keyword's values.
fn openssh(config: &Path, target: &str) -> Option<HashMap<String, Vec<String>>> {
    let out = Command::new("ssh")
        .arg("-F")
        .arg(config)
        .arg("-G")
        .arg(target)
        .output()
        .ok()?;
    assert!(
        out.status.success(),
        "ssh -G {target}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut fields: HashMap<String, Vec<String>> = HashMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((key, value)) = line.split_once(' ') {
            fields.entry(key.into()).or_default().push(value.into());
        }
    }
    Some(fields)
}

/// The same fields, from what shellwing resolved.
fn ours(r: &Resolved) -> Vec<(&'static str, Vec<String>)> {
    let yes_no = |on: bool| vec![if on { "yes" } else { "no" }.to_string()];
    let paths = |list: &[PathBuf]| list.iter().map(|p| p.display().to_string()).collect();
    let mut fields = vec![
        ("hostname", vec![r.target.host.clone()]),
        ("user", vec![r.target.user.clone()]),
        ("port", vec![r.target.port.to_string()]),
        ("forwardagent", yes_no(r.forward_agent)),
        ("identitiesonly", yes_no(r.identities_only)),
        ("forwardx11", yes_no(r.x11.is_some())),
    ];
    // Where the config names none, ssh lists its defaults.
    if !r.identity_files.is_empty() {
        fields.push(("identityfile", paths(&r.identity_files)));
    }
    // Only `yes`: where the config says nothing, Debian's ssh differs from
    // OpenSSH's own default, `no`, which shellwing keeps.
    if r.x11.as_ref().is_some_and(|x| x.trusted) {
        fields.push(("forwardx11trusted", yes_no(true)));
    }
    if !r.jumps.is_empty() {
        let hops: Vec<&str> = r.jumps.iter().map(|j| j.alias.as_str()).collect();
        fields.push(("proxyjump", vec![hops.join(",")]));
    }
    if let Some(c) = &r.proxy_command {
        fields.push(("proxycommand", vec![c.clone()]));
    }
    if let Some(c) = &r.remote_command {
        fields.push(("remotecommand", vec![c.clone()]));
    }
    if !r.tty {
        fields.push(("requesttty", vec!["false".to_string()]));
    }
    if let Some(t) = r.connect_timeout {
        fields.push(("connecttimeout", vec![t.as_secs().to_string()]));
    }
    if let Some(t) = r.keepalive {
        fields.push(("serveraliveinterval", vec![t.as_secs().to_string()]));
    }
    if !r.send_env.is_empty() {
        fields.push(("sendenv", r.send_env.clone()));
    }
    if !r.set_env.is_empty() {
        let vars = r.set_env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        fields.push(("setenv", vars));
    }
    fields
}

/// Compares every target with `ssh -G`, on a config in a folder of its own.
/// `{dir}` in the text stands for that folder.
fn same_as_openssh(name: &str, text: &str, targets: &[&str]) {
    let dir = std::env::temp_dir().join(format!("shellwing-ssh-g-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let config = dir.join("config");
    std::fs::write(&config, text.replace("{dir}", &dir.display().to_string())).expect("config");
    for target in targets {
        let Some(theirs) = openssh(&config, target) else {
            eprintln!("skipped: no OpenSSH client");
            return;
        };
        let resolved = resolve(target, Some(&config)).expect("resolve");
        for (field, ours) in ours(&resolved) {
            let empty = vec![];
            let theirs = theirs.get(field).unwrap_or(&empty);
            assert_eq!(&ours, theirs, "{name}: {field} of {target}");
        }
        // What ssh has and shellwing left out would not show above.
        for (field, has) in [
            ("proxyjump", !resolved.jumps.is_empty()),
            ("proxycommand", resolved.proxy_command.is_some()),
            ("remotecommand", resolved.remote_command.is_some()),
            ("sendenv", !resolved.send_env.is_empty()),
            ("setenv", !resolved.set_env.is_empty()),
        ] {
            assert_eq!(
                theirs.contains_key(field),
                has,
                "{name}: {field} of {target}: {:?}",
                theirs.get(field)
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn me() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".into())
}

#[test]
fn the_common_ways_of_writing_a_config() {
    same_as_openssh(
        "common",
        "# A comment\nUser everyone\nHOST web db\n  HostName %h.example.com\n  Port=2200\n  IdentityFile \"/k/my key\"\n  IdentityFile /k/two\n  ForwardAgent yes\n  ConnectTimeout 5\n  ServerAliveInterval 1m\n  ForwardX11 yes\n  ForwardX11Trusted yes\n  RemoteCommand tmux new -A -s main\n  RequestTTY no\n  SendEnv LANG LC_*\n  SetEnv A=1 B=\"two words\"\n  IdentitiesOnly yes\nHost *.example.com !skip.example.com\n  User wild\n  ProxyJump edge,bastion\nHost edge bastion\n  ProxyJump none\n  User hop\n",
        &[
            "web",
            "db",
            "x.example.com",
            "skip.example.com",
            "me@web",
            "plain",
        ],
    );
}

#[test]
fn proxy_jump_or_proxy_command_whichever_comes_first() {
    same_as_openssh(
        "proxy",
        "Host bastion\n  ProxyCommand none\nHost tunnel\n  ProxyCommand sh -c \"nc %h %p\"\nHost *\n  ProxyJump bastion\n",
        &["bastion", "tunnel", "other"],
    );
}

#[test]
fn a_match_that_does_not_hold_gives_nothing() {
    // Its lines used to land on the Host block above.
    same_as_openssh(
        "not-held",
        "Host web\n  HostName web.example.com\nMatch host nothing-matches-this\n  User wrong\n  Port 2222\n  IdentityFile /wrong/key\n  ProxyJump wrong\n  ForwardAgent yes\nHost *\n  Port 2205\n",
        &["web", "other"],
    );
}

#[test]
fn match_host_sees_the_host_name_and_originalhost_the_name_given() {
    same_as_openssh(
        "host",
        "Host web\n  HostName web.example.com\nMatch host *.example.com\n  User by-host-name\nMatch originalhost web\n  Port 2200\nMatch originalhost web.example.com\n  ForwardAgent yes\nMatch host WEB.example.COM,!other.example.com\n  IdentitiesOnly yes\n",
        &["web", "web.example.com", "other.example.com", "plain"],
    );
}

#[test]
fn match_user_sees_the_user_so_far() {
    let me = me();
    same_as_openssh(
        "user",
        &format!(
            "Match user deploy\n  Port 2201\nHost web\n  User deploy\nMatch user deploy\n  ForwardAgent yes\nMatch localuser {me}\n  IdentitiesOnly yes\nMatch localuser nobody-here\n  Port 1\n"
        ),
        &["web", "deploy@other", "plain"],
    );
}

#[test]
fn match_all_negations_and_several_conditions() {
    let me = me();
    same_as_openssh(
        "conditions",
        &format!(
            "Host a\n  User first\nMatch all\n  User second\n  Port 2202\nMatch !host a\n  ForwardAgent yes\nMatch host a,b user {me},first\n  IdentitiesOnly yes\nMatch host=b\n  ForwardX11 yes\nMatch\tHOST b !user nobody-here\n  RemoteCommand echo b\n"
        ),
        &["a", "b", "c"],
    );
}

#[test]
fn match_tagged_and_command() {
    same_as_openssh(
        "tagged",
        "Host a\n  Tag work\nMatch tagged work\n  Port 2210\nMatch tagged other\n  User wrong\nHost b\n  RemoteCommand tmux new\nMatch command \"tmux new\"\n  Port 2211\nMatch !tagged work host c\n  ForwardAgent yes\n",
        &["a", "b", "c"],
    );
}

#[test]
fn match_exec_runs_with_its_tokens() {
    same_as_openssh(
        "exec",
        "Match exec true\n  User from-true\nMatch exec false\n  Port 1\nMatch host web exec \"test %h = web\"\n  Port 2203\nMatch !exec false\n  ForwardAgent yes\nMatch host nothing exec \"exit 0\"\n  IdentitiesOnly yes\n",
        &["web", "other"],
    );
}

#[test]
fn match_final_comes_after_everything_else() {
    same_as_openssh(
        "final",
        "Host a\n  HostName one.example.com\nMatch final host one.example.com\n  User from-final\n  Port 2204\nHost *\n  Port 2206\nMatch final all\n  ForwardAgent yes\n",
        &["a", "b"],
    );
}

#[test]
fn include_goes_by_the_block_it_stands_in() {
    let dir = std::env::temp_dir().join(format!("shellwing-ssh-g-inc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let inc = dir.join("included");
    std::fs::write(&inc, "User included\nHost inner\n  Port 2207\n").expect("include");
    same_as_openssh(
        "include",
        &format!(
            "Match host nothing\n  Include {inc}\nHost web\n  Include {inc}\n  Port 2208\n",
            inc = inc.display()
        ),
        &["web", "inner", "other"],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

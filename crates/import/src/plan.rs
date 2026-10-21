//! What an import adds to `hosts.toml`: each host found, named apart from
//! those there, in its group (`Parent / Child`), its jump hosts by the names
//! they get, and whether it is there already (`docs/design/import.md`).

use std::collections::HashSet;

use shellwing_config::{Auth, AuthMethod, HOST_COLORS, Host, Hosts};

use crate::{App, Found, Jump, LeftOut, Look, Read};

/// A host as it would be imported.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub host: Host,
    /// Imported when checked; one already here starts unchecked.
    pub checked: bool,
    pub state: State,
    pub look: Vec<Look>,
    /// Kept as a note on the host.
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    New,
    /// Another host has its name: it gets this one, its name in the app given.
    Renamed(String),
    /// A host in `hosts.toml` has its address, port and user: that one's name.
    Here(String),
    /// Not one of the app's hosts: a jump host the app gives by its address.
    ForJump,
}

/// What importing a source would do.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub app: App,
    pub rows: Vec<Row>,
    pub left_out: Vec<LeftOut>,
}

impl Plan {
    /// The groups the checked hosts need that `here` lacks, in order.
    pub fn new_groups(&self, here: &Hosts) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for row in self.rows.iter().filter(|r| r.checked) {
            if let Some(g) = &row.host.group
                && !here.groups.iter().any(|h| &h.name == g)
                && !out.contains(g)
            {
                out.push(g.clone());
            }
        }
        out
    }

    /// The checked hosts, their jump hosts kept to those that will be there
    /// (a checked host, or one already here).
    pub fn hosts(&self, here: &Hosts) -> Vec<Host> {
        let there: HashSet<&str> = self
            .rows
            .iter()
            .filter(|r| r.checked)
            .map(|r| r.host.name.as_str())
            .chain(here.hosts.iter().map(|h| h.name.as_str()))
            .collect();
        self.rows
            .iter()
            .filter(|r| r.checked)
            .map(|r| {
                let mut host = r.host.clone();
                host.jump.retain(|j| there.contains(j.as_str()));
                host
            })
            .collect()
    }
}

/// What importing `read` (from `app`) into `here` would do.
pub fn plan(app: App, read: Read, here: &Hosts) -> Plan {
    let mut taken: HashSet<String> = here.hosts.iter().map(|h| h.name.clone()).collect();
    let aliases: HashSet<&str> = here
        .hosts
        .iter()
        .filter_map(|h| h.alias.as_deref())
        .collect();
    let tag = format!("imported:{}", app.tag());
    let mut rows: Vec<Row> = vec![];
    // What the app's hosts call each row (its name, path, id), by row.
    let mut known: Vec<Vec<String>> = vec![];
    let found = read.hosts;
    for f in &found {
        let here_already = here
            .hosts
            .iter()
            .find(|h| same_place(h, &f.address, f.port, f.user.as_deref()));
        let (name, state) = match here_already {
            Some(h) => (h.name.clone(), State::Here(h.name.clone())),
            None => {
                let name = unique(&f.name, &mut taken);
                let state = if name == f.name {
                    State::New
                } else {
                    State::Renamed(f.name.clone())
                };
                (name, state)
            }
        };
        let mut host = host_of(f, &name, &tag);
        if app == App::SshConfig && !aliases.contains(f.name.as_str()) {
            // `shellwing <name>` keeps working as it did through ssh.
            host.alias = Some(f.name.clone());
        }
        let mut names = vec![f.name.clone()];
        names.extend(f.known_as.iter().cloned());
        known.push(names);
        rows.push(Row {
            checked: !matches!(state, State::Here(_)),
            host,
            state,
            look: f.look.clone(),
            description: f.description.clone(),
        });
    }
    for (i, f) in found.iter().enumerate() {
        let mut jump = vec![];
        for j in &f.jump {
            match j {
                Jump::Host(r) => match known.iter().position(|k| k.contains(r)) {
                    Some(at) => jump.push(rows[at].host.name.clone()),
                    None => rows[i].look.push(Look::JumpNotFound(r.clone())),
                },
                Jump::Address {
                    address,
                    port,
                    user,
                } => {
                    let user = user.as_deref();
                    let name = here
                        .hosts
                        .iter()
                        .find(|h| same_place(h, address, *port, user))
                        .map(|h| h.name.clone())
                        .or_else(|| {
                            rows.iter()
                                .find(|r| same_place(&r.host, address, *port, user))
                                .map(|r| r.host.name.clone())
                        });
                    let name = name.unwrap_or_else(|| {
                        let jump_host = Found {
                            name: address.clone(),
                            address: address.clone(),
                            port: *port,
                            user: user.map(str::to_string),
                            ..Found::default()
                        };
                        let name = unique(address, &mut taken);
                        rows.push(Row {
                            host: host_of(&jump_host, &name, &tag),
                            checked: true,
                            state: State::ForJump,
                            look: vec![],
                            description: None,
                        });
                        known.push(vec![]);
                        name
                    });
                    jump.push(name);
                }
            }
        }
        rows[i].host.jump = jump;
    }
    Plan {
        app,
        rows,
        left_out: read.left_out,
    }
}

fn host_of(f: &Found, name: &str, tag: &str) -> Host {
    Host {
        id: None,
        name: name.to_string(),
        alias: None,
        address: f.address.clone(),
        port: f.port.filter(|p| *p != 22),
        user: f.user.clone(),
        group: (!f.folder.is_empty()).then(|| f.folder.join(" / ")),
        tags: vec![tag.to_string()],
        jump: vec![],
        forward: f.forward.clone(),
        auth: match &f.key {
            Some(k) => Auth {
                method: AuthMethod::Key,
                key: Some(k.clone()),
            },
            None => Auth::default(),
        },
        theme: None,
        local_only: false,
        forward_agent: f.forward_agent,
        keepalive: None,
        host_key: None,
        color: f.color.as_deref().map(color),
        favorite: false,
        run: None,
        log: None,
    }
}

/// Whether `h` reaches `address`, `port` (none: 22) as `user`.
fn same_place(h: &Host, address: &str, port: Option<u16>, user: Option<&str>) -> bool {
    h.address.eq_ignore_ascii_case(address)
        && h.port.unwrap_or(22) == port.unwrap_or(22)
        && h.user.as_deref() == user
}

/// `name`, or `name (2)`, `name (3)`... whichever is free; taken from then on.
fn unique(name: &str, taken: &mut HashSet<String>) -> String {
    let name = if name.trim().is_empty() {
        "host"
    } else {
        name.trim()
    };
    let mut out = name.to_string();
    let mut n = 2;
    while taken.contains(&out) {
        out = format!("{name} ({n})");
        n += 1;
    }
    taken.insert(out.clone());
    out
}

/// One of shellwing's color names when close to it, else `#rrggbb`.
fn color(hex: &str) -> String {
    let Some(rgb) = hex
        .strip_prefix('#')
        .and_then(|h| u32::from_str_radix(h, 16).ok())
    else {
        return hex.to_string();
    };
    let channels = |c: u32| [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| v as i64);
    let distance = |c: u32| {
        channels(c)
            .iter()
            .zip(channels(rgb))
            .map(|(a, b)| (a - b).pow(2))
            .sum::<i64>()
    };
    HOST_COLORS
        .iter()
        .min_by_key(|(_, c)| distance(*c))
        .filter(|(_, c)| distance(*c) <= 60 * 60)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| hex.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, address: &str) -> Found {
        Found {
            name: name.into(),
            address: address.into(),
            ..Found::default()
        }
    }

    fn here() -> Hosts {
        toml::from_str(
            r#"
[[group]]
name = "Prod / EU"

[[host]]
name = "web"
address = "10.0.0.1"
user = "deploy"
"#,
        )
        .expect("hosts")
    }

    #[test]
    fn hosts_are_named_apart_grouped_and_marked_when_here() {
        let mut a = found("web", "10.0.0.1");
        a.user = Some("deploy".into());
        a.port = Some(22);
        let mut b = found("web", "10.0.0.2");
        b.folder = vec!["Prod".into(), "EU".into()];
        b.color = Some("#f87070".into());
        let mut c = found("web", "10.0.0.3");
        c.folder = vec!["Lab".into()];
        c.port = Some(2222);
        let read = Read {
            hosts: vec![a, b, c],
            left_out: vec![],
        };
        let plan = plan(App::MobaXterm, read, &here());
        let states: Vec<&State> = plan.rows.iter().map(|r| &r.state).collect();
        assert_eq!(
            states,
            [
                &State::Here("web".into()),
                &State::Renamed("web".into()),
                &State::Renamed("web".into())
            ]
        );
        let names: Vec<&str> = plan.rows.iter().map(|r| r.host.name.as_str()).collect();
        assert_eq!(names, ["web", "web (2)", "web (3)"]);
        assert!(!plan.rows[0].checked && plan.rows[1].checked);
        assert_eq!(plan.rows[1].host.group.as_deref(), Some("Prod / EU"));
        assert_eq!(plan.rows[1].host.color.as_deref(), Some("red"));
        assert_eq!(plan.rows[2].host.port, Some(2222));
        assert_eq!(plan.rows[1].host.tags, ["imported:mobaxterm"]);
        // Prod / EU is here already; Lab is made.
        assert_eq!(plan.new_groups(&here()), ["Lab"]);
    }

    #[test]
    fn jump_hosts_get_the_names_their_hosts_get() {
        let mut bastion = found("bastion", "bastion.example.com");
        bastion.known_as = vec!["ssh:bastion".into()];
        let mut app = found("app", "10.0.1.5");
        app.jump = vec![
            Jump::Host("ssh:bastion".into()),
            Jump::Host("gone".into()),
            Jump::Address {
                address: "10.0.0.1".into(),
                port: None,
                user: Some("deploy".into()),
            },
            Jump::Address {
                address: "edge.example.com".into(),
                port: Some(2200),
                user: None,
            },
        ];
        let mut db = found("db", "10.0.1.6");
        db.jump = vec![Jump::Address {
            address: "edge.example.com".into(),
            port: Some(2200),
            user: None,
        }];
        let read = Read {
            hosts: vec![bastion, app, db],
            left_out: vec![],
        };
        let plan = plan(App::Tabby, read, &here());
        let app = &plan.rows[1];
        assert_eq!(app.host.jump, ["bastion", "web", "edge.example.com"]);
        assert_eq!(app.look, [Look::JumpNotFound("gone".into())]);
        assert_eq!(plan.rows[2].host.jump, ["edge.example.com"], "made once");
        let edge = &plan.rows[3];
        assert_eq!(
            (edge.state.clone(), edge.host.port),
            (State::ForJump, Some(2200))
        );
        assert_eq!(plan.rows.len(), 4);
        // Unchecked, a jump host is left out of the hosts that use it.
        let mut plan = plan;
        plan.rows[0].checked = false;
        let hosts = plan.hosts(&here());
        let app = hosts.iter().find(|h| h.name == "app").expect("app");
        assert_eq!(app.jump, ["web", "edge.example.com"]);
    }

    #[test]
    fn ssh_config_hosts_keep_their_alias() {
        let read = Read {
            hosts: vec![found("db", "10.0.2.2")],
            left_out: vec![],
        };
        let plan = plan(App::SshConfig, read, &here());
        assert_eq!(plan.rows[0].host.alias.as_deref(), Some("db"));
    }
}

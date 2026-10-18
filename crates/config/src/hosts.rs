use serde::Deserialize;

/// `hosts.toml`: `[[group]]` and `[[host]]` tables.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Hosts {
    #[serde(rename = "group")]
    pub groups: Vec<Group>,
    #[serde(rename = "host")]
    pub hosts: Vec<Host>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    /// Theme for the group's hosts, unless a host sets its own.
    #[serde(default)]
    pub theme: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Host {
    /// Stable id for sync; written by the app.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// Short name for the command line: `shellwing <alias>`.
    #[serde(default)]
    pub alias: Option<String>,
    pub address: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: Option<String>,
    /// The group's name.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Hosts to go through first, outermost first: names from this file or
    /// aliases from `~/.ssh/config`.
    #[serde(default)]
    pub jump: Vec<String>,
    /// Ports to forward while connected, written as ssh's options:
    /// `-L 8080:localhost:80`, `-R 9000:localhost:3000`, `-D 1080`.
    #[serde(default)]
    pub forward: Vec<String>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub theme: Option<String>,
    /// Never synced.
    #[serde(default)]
    pub local_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Auth {
    pub method: AuthMethod,
    /// With `method = "key"`: a key file path (`~/` allowed).
    pub key: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    /// As `ssh` does: agent, key files, then a password.
    #[default]
    Auto,
    Key,
    Password,
    Agent,
}

impl Hosts {
    /// The host whose alias, or else whose name, is `name`.
    pub fn find(&self, name: &str) -> Option<&Host> {
        self.hosts
            .iter()
            .find(|h| h.alias.as_deref() == Some(name))
            .or_else(|| self.hosts.iter().find(|h| h.name == name))
    }

    pub fn group_of(&self, host: &Host) -> Option<&Group> {
        let name = host.group.as_deref()?;
        self.groups.iter().find(|g| g.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        # Written by shellwing. Editing by hand is fine.
        [[group]]
        id = "01JAQ7"
        name = "Production"
        color = "pink"

        [[host]]
        id = "01JAQ8"
        name = "api-1"
        alias = "api1"
        address = "10.0.1.11"
        port = 2222
        user = "deploy"
        group = "Production"
        tags = ["api", "docker"]
        jump = ["edge", "bastion"]
        auth = { method = "key", key = "~/.ssh/deploy_ed25519" }
        theme = "crimson-ember"
        local_only = true

        [[host]]
        name = "web"
        address = "10.0.0.5"
    "#;

    #[test]
    fn reads_the_documented_shape() {
        let h: Hosts = toml::from_str(SAMPLE).expect("parse");
        assert_eq!(h.groups.len(), 1);
        let api = h.find("api1").expect("by alias");
        assert_eq!(api.port, Some(2222));
        assert_eq!(api.jump, ["edge", "bastion"]);
        assert_eq!(api.auth.method, AuthMethod::Key);
        assert!(api.local_only);
        assert_eq!(h.group_of(api).map(|g| g.name.as_str()), Some("Production"));
        let web = h.find("web").expect("by name");
        assert_eq!((web.port, web.auth.method), (None, AuthMethod::Auto));
        assert!(h.find("nope").is_none());
    }

    #[test]
    fn alias_wins_over_another_hosts_name() {
        let h: Hosts = toml::from_str(
            "[[host]]\nname = \"db\"\naddress = \"a\"\n[[host]]\nname = \"x\"\nalias = \"db\"\naddress = \"b\"\n",
        )
        .expect("parse");
        assert_eq!(h.find("db").map(|h| h.address.as_str()), Some("b"));
    }
}

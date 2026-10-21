//! Tabby: `config.yaml`, its `profiles` of `type: ssh`. A profile's group is
//! an id into `groups` (with parents) since its fifth config version, a
//! name before.

use yaml_rust2::{Yaml, YamlLoader};

use crate::{App, Dirs, Error, Found, Jump, LeftOut, Place, Read, Why};

pub(crate) fn places(dirs: &Dirs) -> Vec<(App, Place)> {
    let mut roots = vec![];
    roots.extend(dirs.app_data.clone());
    roots.extend(dirs.config.clone());
    roots
        .iter()
        .flat_map(|r| [r.join("tabby/config.yaml"), r.join("terminus/config.yaml")])
        .find(|p| p.is_file())
        .map(|p| (App::Tabby, Place::File(p)))
        .into_iter()
        .collect()
}

pub(crate) fn read(text: &str) -> Result<Read, Error> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| Error::Format(e.to_string()))?;
    let Some(doc) = docs.first() else {
        return Ok(Read::default());
    };
    if doc["encrypted"].as_bool() == Some(true) {
        return Err(Error::Encrypted("Tabby"));
    }
    let groups = doc["groups"].as_vec().cloned().unwrap_or_default();
    let mut out = Read::default();
    for p in doc["profiles"].as_vec().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default().to_string();
        match p["type"].as_str() {
            Some("ssh") => {}
            // Local shells are not hosts; these are, of another kind.
            Some(kind @ ("telnet" | "serial")) => {
                out.left_out.push(LeftOut {
                    name,
                    why: Why::NotSsh(kind.into()),
                });
                continue;
            }
            _ => continue,
        }
        let o = &p["options"];
        let Some(address) = o["host"].as_str().filter(|h| !h.is_empty()) else {
            out.left_out.push(LeftOut {
                name,
                why: Why::NoAddress,
            });
            continue;
        };
        let key = o["privateKeys"]
            .as_vec()
            .into_iter()
            .flatten()
            .filter_map(Yaml::as_str)
            .find_map(|k| k.strip_prefix("file://"))
            .map(|k| {
                // `file:///C:/keys/id` on Windows.
                if k.get(2..3) == Some(":") && k.starts_with('/') {
                    k[1..].to_string()
                } else {
                    k.to_string()
                }
            });
        let forward = o["forwardedPorts"]
            .as_vec()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                let bind = format!(
                    "{}:{}",
                    f["host"].as_str().unwrap_or("127.0.0.1"),
                    number(&f["port"])?
                );
                let target = || {
                    Some(format!(
                        "{}:{}",
                        f["targetAddress"].as_str()?,
                        number(&f["targetPort"])?
                    ))
                };
                match f["type"].as_str()? {
                    "Local" => Some(format!("-L {bind}:{}", target()?)),
                    "Remote" => Some(format!("-R {bind}:{}", target()?)),
                    "Dynamic" => Some(format!("-D {bind}")),
                    _ => None,
                }
            })
            .collect();
        out.hosts.push(Found {
            name,
            folder: folder(&p["group"], &groups),
            address: address.to_string(),
            port: number(&o["port"]).and_then(|p| u16::try_from(p).ok()),
            user: o["user"]
                .as_str()
                .filter(|u| !u.is_empty())
                .map(str::to_string),
            key,
            forward_agent: o["agentForward"].as_bool() == Some(true),
            jump: o["jumpHost"]
                .as_str()
                .map(|id| vec![Jump::Host(id.to_string())])
                .unwrap_or_default(),
            forward,
            color: p["color"]
                .as_str()
                .filter(|c| c.starts_with('#') && c.len() == 7)
                .map(str::to_lowercase),
            known_as: p["id"].as_str().map(str::to_string).into_iter().collect(),
            ..Found::default()
        });
    }
    Ok(out)
}

fn number(y: &Yaml) -> Option<i64> {
    y.as_i64().or_else(|| y.as_str()?.parse().ok())
}

/// A profile's folders: its group's name and its parents', outermost first.
fn folder(group: &Yaml, groups: &[Yaml]) -> Vec<String> {
    let Some(id) = group.as_str().filter(|g| !g.is_empty()) else {
        return vec![];
    };
    let by_id = |id: &str| groups.iter().find(|g| g["id"].as_str() == Some(id));
    if by_id(id).is_none() {
        // An older file: the group's name itself.
        return vec![id.to_string()];
    }
    let mut out = vec![];
    let mut at = by_id(id);
    while let Some(g) = at {
        out.push(g["name"].as_str().unwrap_or_default().to_string());
        at = g["parentGroupId"].as_str().and_then(by_id);
        if out.len() > 16 {
            break;
        }
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r##"
version: 5
groups:
  - id: g-prod
    name: Prod
  - id: g-eu
    name: EU
    parentGroupId: g-prod
profiles:
  - type: local
    name: Bash
  - type: ssh
    id: ssh:bastion
    name: bastion
    group: g-prod
    options:
      host: bastion.example.com
      port: 2222
      user: ops
  - type: ssh
    id: ssh:web
    name: web
    group: g-eu
    color: "#3B82F6"
    options:
      host: 10.0.1.11
      user: deploy
      auth: publicKey
      privateKeys:
        - file:///C:/keys/deploy
      jumpHost: ssh:bastion
      agentForward: true
      forwardedPorts:
        - type: Local
          host: 127.0.0.1
          port: 8080
          targetAddress: localhost
          targetPort: 80
        - type: Dynamic
          host: 127.0.0.1
          port: 1080
  - type: serial
    name: console
"##;

    #[test]
    fn its_ssh_profiles_are_read() {
        let read = read(CONFIG).expect("read");
        let names: Vec<&str> = read.hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["bastion", "web"]);
        assert_eq!(read.hosts[0].folder, ["Prod"]);
        assert_eq!(read.hosts[0].port, Some(2222));
        let web = &read.hosts[1];
        assert_eq!(web.folder, ["Prod", "EU"]);
        assert_eq!(web.key.as_deref(), Some("C:/keys/deploy"));
        assert_eq!(web.jump, [Jump::Host("ssh:bastion".into())]);
        assert_eq!(read.hosts[0].known_as, ["ssh:bastion"]);
        assert!(web.forward_agent);
        assert_eq!(
            web.forward,
            ["-L 127.0.0.1:8080:localhost:80", "-D 127.0.0.1:1080"]
        );
        assert_eq!(web.color.as_deref(), Some("#3b82f6"));
        assert_eq!(read.left_out[0].why, Why::NotSsh("serial".into()));
    }

    #[test]
    fn an_encrypted_config_is_said_to_be() {
        assert!(matches!(
            read("encrypted: true\nvault: x\n"),
            Err(Error::Encrypted("Tabby"))
        ));
    }
}

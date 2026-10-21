//! SecureCRT: `Config/Sessions/`, one `.ini` per session, its folders the
//! session's; or the XML file *Export Settings* writes (9.6 and later).
//!
//! Lines are `S:"Name"=text`, `D:"Name"=hex number`, and `Z:"Name"=count`
//! followed by that many lines of text, each after a space.

use std::collections::HashMap;
use std::path::Path;

use crate::{App, Dirs, Error, Found, Jump, LeftOut, Place, Read, Why};

pub(crate) fn places(dirs: &Dirs) -> Vec<(App, Place)> {
    [
        dirs.app_data
            .as_ref()
            .map(|d| d.join("VanDyke/Config/Sessions")),
        dirs.app_data
            .as_ref()
            .map(|d| d.join("VanDyke/SecureCRT/Config/Sessions")),
        Some(dirs.home.join(".vandyke/SecureCRT/Config/Sessions")),
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_dir())
    .map(|p| (App::SecureCrt, Place::Folder(p)))
    .collect()
}

/// A `Sessions` folder.
pub(crate) fn read_folder(dir: &Path) -> Result<Read, Error> {
    let mut sessions = vec![];
    for (folder, path) in crate::files_in(dir, "ini") {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if stem == "__FolderData__" || (folder.is_empty() && stem == "Default") {
            continue;
        }
        let values = values(&crate::text(&std::fs::read(&path)?));
        sessions.push((folder, stem, values));
    }
    Ok(from_sessions(sessions))
}

/// A file chosen by hand: the XML export, or one session's `.ini`.
pub(crate) fn read_bytes(bytes: &[u8]) -> Result<Read, Error> {
    let text = crate::text(bytes);
    if text.contains("<VanDyke") {
        return xml(&text);
    }
    let values = values(&text);
    let name = values.get("Hostname").cloned().unwrap_or_default();
    Ok(from_sessions(vec![(vec![], name, values)]))
}

/// A session `.ini`'s values by name: numbers in decimal, text of several
/// lines joined.
fn values(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        let Some((name, value)) = rest.strip_prefix('"').and_then(|r| r.split_once("\"=")) else {
            continue;
        };
        let value = match kind {
            "S" => value.to_string(),
            "D" => u32::from_str_radix(value.trim(), 16)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            "Z" => {
                let count = usize::from_str_radix(value.trim(), 16).unwrap_or(0);
                lines
                    .by_ref()
                    .take(count)
                    .map(|l| l.strip_prefix(' ').unwrap_or(l))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => continue,
        };
        out.insert(name.to_string(), value);
    }
    out
}

/// The XML export: `<key name="Sessions">`, its sub-keys the folders and
/// sessions, a session's values in `<string>` and `<dword>` (decimal).
fn xml(text: &str) -> Result<Read, Error> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut path: Vec<String> = vec![];
    // The value being read: its name, and its text so far; an array's
    // strings are its lines.
    let mut value: Option<(String, String)> = None;
    let mut in_array = false;
    let mut found: Vec<(Vec<String>, HashMap<String, String>)> = vec![];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e
                    .try_get_attribute("name")
                    .ok()
                    .flatten()
                    .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
                    .map(|v| v.into_owned())
                    .unwrap_or_default();
                match e.name().as_ref() {
                    "key" => path.push(name),
                    "array" => {
                        value = Some((name, String::new()));
                        in_array = true;
                    }
                    "string" | "dword" if in_array => {
                        if let Some((_, text)) = &mut value
                            && !text.is_empty()
                        {
                            text.push('\n');
                        }
                    }
                    "string" | "dword" => value = Some((name, String::new())),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((_, text)) = &mut value {
                    text.push_str(&t.xml10_content());
                }
            }
            // `&amp;` and the like, and `&#233;`.
            Ok(Event::GeneralRef(r)) => {
                if let Some((_, text)) = &mut value {
                    let entity = r.xml10_content();
                    let c = match entity.as_ref() {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        _ => r.resolve_char_ref().ok().flatten(),
                    };
                    text.extend(c);
                }
            }
            Ok(Event::End(e)) => {
                let ends_value = match e.name().as_ref() {
                    "key" => {
                        path.pop();
                        false
                    }
                    "array" => {
                        in_array = false;
                        true
                    }
                    "string" | "dword" => !in_array,
                    _ => false,
                };
                if ends_value && let Some((name, text)) = value.take() {
                    match found.iter_mut().find(|(p, _)| *p == path) {
                        Some((_, values)) => {
                            values.insert(name, text);
                        }
                        None => found.push((path.clone(), HashMap::from([(name, text)]))),
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Format(e.to_string())),
            _ => {}
        }
    }
    let sessions = found
        .into_iter()
        .filter_map(|(path, values)| {
            let at = path.iter().position(|k| k == "Sessions")?;
            let (name, folder) = path[at + 1..].split_last()?;
            values
                .contains_key("Hostname")
                .then(|| (folder.to_vec(), name.clone(), values))
        })
        .collect();
    Ok(from_sessions(sessions))
}

fn from_sessions(sessions: Vec<(Vec<String>, String, HashMap<String, String>)>) -> Read {
    let mut out = Read::default();
    for (folder, name, v) in sessions {
        let get = |k: &str| v.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let protocol = get("Protocol Name").unwrap_or("SSH2");
        if !protocol.to_ascii_uppercase().starts_with("SSH") {
            out.left_out.push(LeftOut {
                name,
                why: Why::NotSsh(protocol.to_string()),
            });
            continue;
        }
        let Some(address) = get("Hostname") else {
            out.left_out.push(LeftOut {
                name,
                why: Why::NoAddress,
            });
            continue;
        };
        // `Session:<folder>/<name>`: another saved session in front.
        let jump = get("Firewall Name")
            .and_then(|f| f.strip_prefix("Session:"))
            .map(|s| vec![Jump::Host(s.trim_matches('/').to_string())])
            .unwrap_or_default();
        // The path may end in `::rawkey` or the like.
        let key = get("Identity Filename V2")
            .or(get("Identity Filename"))
            .map(|k| k.split("::").next().unwrap_or(k).to_string())
            .filter(|k| !k.is_empty());
        let mut path = folder.clone();
        path.push(name.clone());
        out.hosts.push(Found {
            known_as: vec![path.join("/")],
            name,
            folder,
            address: address.to_string(),
            port: get("[SSH2] Port").and_then(|p| p.parse().ok()),
            user: get("Username").map(str::to_string),
            key,
            forward_agent: get("Enable Agent Forwarding") == Some("1"),
            jump,
            description: get("Description").map(str::to_string),
            ..Found::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEB: &str = "S:\"Protocol Name\"=SSH2\r\n\
S:\"Hostname\"=10.0.1.11\r\n\
D:\"[SSH2] Port\"=000008ae\r\n\
S:\"Username\"=deploy\r\n\
S:\"Identity Filename V2\"=C:\\keys\\deploy::rawkey\r\n\
D:\"Enable Agent Forwarding\"=00000001\r\n\
S:\"Firewall Name\"=Session:Infra/bastion\r\n\
Z:\"Description\"=00000002\r\n Front end\r\n second line\r\n\
B:\"Color Scheme Overrides\"=00000002\r\n 00 00\r\n";

    #[test]
    fn a_sessions_folder_gives_its_sessions() {
        let dir = std::env::temp_dir().join(format!("shellwing-import-crt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Prod/EU")).expect("dir");
        std::fs::create_dir_all(dir.join("Infra")).expect("dir");
        std::fs::write(dir.join("Default.ini"), "S:\"Hostname\"=\r\n").expect("default");
        std::fs::write(
            dir.join("Prod/__FolderData__.ini"),
            "S:\"Folder List\"=\r\n",
        )
        .expect("folder");
        std::fs::write(dir.join("Prod/EU/web.ini"), WEB).expect("web");
        std::fs::write(
            dir.join("Infra/bastion.ini"),
            "S:\"Hostname\"=bastion.example.com\r\n",
        )
        .expect("bastion");
        std::fs::write(
            dir.join("Infra/console.ini"),
            "S:\"Protocol Name\"=Serial\r\n",
        )
        .expect("serial");
        let read = read_folder(&dir).expect("read");
        let names: Vec<&str> = read.hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["bastion", "web"]);
        assert_eq!(read.hosts[0].known_as, ["Infra/bastion"]);
        let web = &read.hosts[1];
        assert_eq!(web.folder, ["Prod", "EU"]);
        assert_eq!(
            (web.port, web.user.as_deref()),
            (Some(2222), Some("deploy"))
        );
        assert_eq!(web.key.as_deref(), Some("C:\\keys\\deploy"));
        assert!(web.forward_agent);
        assert_eq!(web.jump, [Jump::Host("Infra/bastion".into())]);
        assert_eq!(web.description.as_deref(), Some("Front end\nsecond line"));
        assert_eq!(read.left_out[0].why, Why::NotSsh("Serial".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_xml_export_gives_the_same() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<VanDyke version="3.0">
  <key name="Sessions">
    <key name="Prod">
      <key name="web">
        <string name="Protocol Name">SSH2</string>
        <string name="Hostname">10.0.1.11</string>
        <dword name="[SSH2] Port">2222</dword>
        <string name="Username">deploy</string>
        <array name="Description"><string>one</string><string>two</string></array>
      </key>
    </key>
  </key>
</VanDyke>"#;
        let read = read_bytes(xml.as_bytes()).expect("read");
        let web = &read.hosts[0];
        assert_eq!(
            (web.name.as_str(), web.folder.as_slice()),
            ("web", &["Prod".to_string()][..])
        );
        assert_eq!((web.address.as_str(), web.port), ("10.0.1.11", Some(2222)));
        assert_eq!(web.description.as_deref(), Some("one\ntwo"));
    }
}

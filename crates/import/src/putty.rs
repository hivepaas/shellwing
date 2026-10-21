//! PuTTY and KiTTY: one registry key (or file) of values per session.
//!
//! - Windows: `HKCU\Software\SimonTatham\PuTTY\Sessions\<name>` (KiTTY:
//!   `Software\9bis.com\KiTTY\Sessions`), or a `.reg` export of it.
//! - Elsewhere: `~/.putty/sessions/<name>`, one `Key=Value` per line.
//!
//! Session names are `%XX`-escaped in both.

use std::collections::HashMap;
use std::path::Path;

use crate::{App, Dirs, Error, Found, Jump, LeftOut, Look, Place, Read, Why};

const PUTTY_KEY: &str = r"Software\SimonTatham\PuTTY\Sessions";
const KITTY_KEY: &str = r"Software\9bis.com\KiTTY\Sessions";

/// Proxy kinds that go through another SSH server (PuTTY's `ProxyMethod`).
const SSH_PROXIES: [&str; 3] = ["6", "7", "8"];

pub(crate) fn places(dirs: &Dirs) -> Vec<(App, Place)> {
    if cfg!(windows) {
        return vec![
            (App::Putty, Place::Registry(PUTTY_KEY)),
            (App::Kitty, Place::Registry(KITTY_KEY)),
        ];
    }
    let mut out = vec![];
    for dir in [
        Some(dirs.home.join(".putty/sessions")),
        dirs.config.as_ref().map(|c| c.join("putty/sessions")),
    ]
    .into_iter()
    .flatten()
    {
        if dir.is_dir() {
            out.push((App::Putty, Place::Folder(dir)));
        }
    }
    out
}

/// A `.reg` export: the sessions under a `...\Sessions\<name>` key.
pub(crate) fn read_reg(app: App, text: &str) -> Read {
    let mut sessions: Vec<(String, HashMap<String, String>)> = vec![];
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(key) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = key
                .to_ascii_lowercase()
                .find("\\sessions\\")
                .map(|at| &key[at + "\\sessions\\".len()..])
                .filter(|n| !n.contains('\\'));
            inside = name.is_some();
            if let Some(name) = name {
                sessions.push((unescape(name), HashMap::new()));
            }
        } else if inside
            && let Some((name, value)) = line.split_once('=')
            && let Some((_, values)) = sessions.last_mut()
        {
            let name = name.trim().trim_matches('"').to_string();
            let value = value.trim();
            let value = if let Some(hex) = value.strip_prefix("dword:") {
                u32::from_str_radix(hex, 16)
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            } else if let Some(s) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                s.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                continue;
            };
            values.insert(name, value);
        }
    }
    from_sessions(app, sessions)
}

/// A folder of session files: `Key=Value` lines (PuTTY), or `Key\Value\`
/// (KiTTY's own folder).
pub(crate) fn read_folder(app: App, dir: &Path) -> Result<Read, Error> {
    let mut sessions = vec![];
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for path in files {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let text = crate::text(&std::fs::read(&path)?);
        let values = text
            .lines()
            .filter_map(|l| {
                let l = l.trim_end_matches(['\r', '\n']);
                match l.strip_suffix('\\') {
                    Some(kitty) => kitty.split_once('\\'),
                    None => l.split_once('='),
                }
                .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        sessions.push((unescape(&name), values));
    }
    Ok(from_sessions(app, sessions))
}

#[cfg(windows)]
pub(crate) fn read_registry(app: App, key: &str) -> Result<Read, Error> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, RegType};
    let root = RegKey::predef(HKEY_CURRENT_USER).open_subkey(key)?;
    let mut sessions = vec![];
    for name in root.enum_keys().filter_map(Result::ok) {
        let Ok(session) = root.open_subkey(&name) else {
            continue;
        };
        let mut values = HashMap::new();
        for (value_name, value) in session.enum_values().filter_map(Result::ok) {
            let text = match value.vtype {
                RegType::REG_DWORD => session
                    .get_value::<u32, _>(&value_name)
                    .map(|n| n.to_string())
                    .ok(),
                RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
                    session.get_value::<String, _>(&value_name).ok()
                }
                _ => None,
            };
            if let Some(text) = text {
                values.insert(value_name, text);
            }
        }
        sessions.push((unescape(&name), values));
    }
    Ok(from_sessions(app, sessions))
}

#[cfg(not(windows))]
pub(crate) fn read_registry(_: App, _: &str) -> Result<Read, Error> {
    Err(Error::Unknown)
}

fn from_sessions(app: App, sessions: Vec<(String, HashMap<String, String>)>) -> Read {
    let mut out = Read::default();
    let names: Vec<String> = sessions.iter().map(|(n, _)| n.clone()).collect();
    for (name, v) in sessions {
        if name == "Default Settings" {
            continue;
        }
        let get = |k: &str| v.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let protocol = get("Protocol").unwrap_or("ssh");
        if !protocol.eq_ignore_ascii_case("ssh") {
            out.left_out.push(LeftOut {
                name,
                why: Why::NotSsh(protocol.to_string()),
            });
            continue;
        }
        let Some(host) = get("HostName") else {
            out.left_out.push(LeftOut {
                name,
                why: Why::NoAddress,
            });
            continue;
        };
        // The host name may carry the user: `deploy@web`.
        let (user, address) = match host.rsplit_once('@') {
            Some((u, h)) => (Some(u.to_string()), h.to_string()),
            None => (None, host.to_string()),
        };
        let key = get("PublicKeyFile").map(str::to_string);
        let mut look = vec![];
        if key
            .as_deref()
            .is_some_and(|k| k.to_ascii_lowercase().ends_with(".ppk"))
        {
            look.push(Look::PpkKey);
        }
        let jump = match (get("ProxyMethod"), get("ProxyHost")) {
            (Some(m), Some(proxy)) if SSH_PROXIES.contains(&m) => {
                if names.iter().any(|n| n == proxy) {
                    vec![Jump::Host(proxy.to_string())]
                } else {
                    vec![Jump::Address {
                        address: proxy.to_string(),
                        port: get("ProxyPort").and_then(|p| p.parse().ok()),
                        user: get("ProxyUsername").map(str::to_string),
                    }]
                }
            }
            _ => vec![],
        };
        let folder = match (app, get("Folder")) {
            (App::Kitty, Some(f)) => f
                .split(['\\', '/'])
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
            _ => vec![],
        };
        out.hosts.push(Found {
            name,
            folder,
            address,
            port: get("PortNumber").and_then(|p| p.parse().ok()),
            user: get("UserName").map(str::to_string).or(user),
            key,
            forward_agent: get("AgentFwd") == Some("1"),
            jump,
            forward: get("PortForwardings").map(forwards).unwrap_or_default(),
            look,
            ..Found::default()
        });
    }
    out
}

/// `PortForwardings`: `[4|6]{L|R|D}[addr:]port=dest:port`, comma separated,
/// `\` escaping a comma, `=` or `\`.
fn forwards(text: &str) -> Vec<String> {
    let mut entries = vec![];
    let mut entry = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => entry.extend(chars.next()),
            ',' => entries.push(std::mem::take(&mut entry)),
            c => entry.push(c),
        }
    }
    entries.push(entry);
    entries
        .iter()
        .filter_map(|e| {
            let e = e.trim_start_matches(['4', '6']);
            let mut chars = e.chars();
            let kind = chars.next()?;
            let (from, to) = chars
                .as_str()
                .split_once('=')
                .unwrap_or((chars.as_str(), ""));
            match kind {
                'L' => Some(format!("-L {from}:{to}")),
                'R' => Some(format!("-R {from}:{to}")),
                'D' => Some(format!("-D {from}")),
                _ => None,
            }
        })
        .collect()
}

/// `%XX` escapes undone.
fn unescape(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = name.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: &str = "Windows Registry Editor Version 5.00\r\n\r\n\
[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions]\r\n\r\n\
[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\Default%20Settings]\r\n\
\"HostName\"=\"\"\r\n\r\n\
[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\bastion]\r\n\
\"HostName\"=\"ops@bastion.example.com\"\r\n\
\"PortNumber\"=dword:000008ae\r\n\
\"Protocol\"=\"ssh\"\r\n\r\n\
[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\Web%20%231]\r\n\
\"HostName\"=\"10.0.1.11\"\r\n\
\"PortNumber\"=dword:00000016\r\n\
\"Protocol\"=\"ssh\"\r\n\
\"UserName\"=\"deploy\"\r\n\
\"PublicKeyFile\"=\"C:\\\\Users\\\\me\\\\deploy.ppk\"\r\n\
\"AgentFwd\"=dword:00000001\r\n\
\"PortForwardings\"=\"L8080=localhost:80,4R9000=localhost:3000,D1080=,L127.0.0.1:5432=db\\,x:5432\"\r\n\
\"ProxyMethod\"=dword:00000006\r\n\
\"ProxyHost\"=\"bastion\"\r\n\r\n\
[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\router]\r\n\
\"HostName\"=\"192.168.1.1\"\r\n\
\"Protocol\"=\"telnet\"\r\n";

    #[test]
    fn a_reg_export_gives_its_ssh_sessions() {
        let read = read_reg(App::Putty, &crate::text(&crate::utf16le(REG)));
        let names: Vec<&str> = read.hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["bastion", "Web #1"]);
        let bastion = &read.hosts[0];
        assert_eq!(bastion.address, "bastion.example.com");
        assert_eq!(
            (bastion.port, bastion.user.as_deref()),
            (Some(2222), Some("ops"))
        );
        let web = &read.hosts[1];
        assert_eq!((web.port, web.user.as_deref()), (Some(22), Some("deploy")));
        assert_eq!(web.key.as_deref(), Some("C:\\Users\\me\\deploy.ppk"));
        assert_eq!(web.look, [Look::PpkKey]);
        assert!(web.forward_agent);
        assert_eq!(web.jump, [Jump::Host("bastion".into())]);
        assert_eq!(
            web.forward,
            [
                "-L 8080:localhost:80",
                "-R 9000:localhost:3000",
                "-D 1080",
                "-L 127.0.0.1:5432:db,x:5432"
            ]
        );
        assert_eq!(
            read.left_out,
            [LeftOut {
                name: "router".into(),
                why: Why::NotSsh("telnet".into())
            }]
        );
    }

    #[test]
    fn a_session_folder_is_read_too() {
        let dir =
            std::env::temp_dir().join(format!("shellwing-import-putty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("db%20main"),
            "HostName=10.0.0.5\nPortNumber=2200\nProtocol=ssh\nUserName=pg\n",
        )
        .expect("putty");
        std::fs::write(
            dir.join("kitty"),
            "HostName\\10.0.0.6\\\nFolder\\Prod\\EU\\\n",
        )
        .expect("kitty");
        let read = read_folder(App::Kitty, &dir).expect("read");
        let db = &read.hosts[0];
        assert_eq!(
            (db.name.as_str(), db.address.as_str(), db.port),
            ("db main", "10.0.0.5", Some(2200))
        );
        assert_eq!(read.hosts[1].folder, ["Prod", "EU"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

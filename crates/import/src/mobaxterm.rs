//! MobaXterm: `MobaXterm.ini`, or its export (`.mxtsessions`, `.moba`).
//!
//! Folders are the `[Bookmarks]` and `[Bookmarks_N]` sections, named by their
//! `SubRep` (`Parent\Child`). Each session is a line
//! `Name=<flag>#<icon>#<params>#<terminal>#<start>#<comment>#<color>`, its
//! params separated by `%`, the first being the type (0: SSH).

use crate::{App, Dirs, Found, Jump, LeftOut, Place, Read, Why};

/// The params read, by their place.
const TYPE: usize = 0;
const HOST: usize = 1;
const PORT: usize = 2;
const USER: usize = 3;
const JUMP_HOSTS: usize = 8;
const JUMP_PORTS: usize = 9;
const JUMP_USERS: usize = 10;
const KEY: usize = 14;
const AGENT_FORWARD: usize = 34;

pub(crate) fn places(dirs: &Dirs) -> Vec<(App, Place)> {
    [
        dirs.app_data
            .as_ref()
            .map(|d| d.join("MobaXterm/MobaXterm.ini")),
        dirs.documents
            .as_ref()
            .map(|d| d.join("MobaXterm/MobaXterm.ini")),
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_file())
    .map(|p| (App::MobaXterm, Place::File(p)))
    .collect()
}

pub(crate) fn read(text: &str) -> Read {
    let mut out = Read::default();
    for (section, lines) in crate::ini(text) {
        if !section.starts_with("Bookmarks") {
            continue;
        }
        let folder: Vec<String> = lines
            .iter()
            .find(|(k, _)| k == "SubRep")
            .map(|(_, v)| {
                v.split('\\')
                    .filter(|p| !p.is_empty())
                    .map(unescape)
                    .collect()
            })
            .unwrap_or_default();
        for (name, value) in &lines {
            if name == "SubRep" || name == "ImgNum" || !value.contains('#') {
                continue;
            }
            let name = unescape(name);
            let fields: Vec<&str> = value.split('#').collect();
            let params: Vec<&str> = fields
                .get(2)
                .map(|p| p.split('%').collect())
                .unwrap_or_default();
            let param = |i: usize| {
                params
                    .get(i)
                    .map(|p| unescape(p.trim()))
                    .filter(|p| !p.is_empty())
            };
            match param(TYPE).as_deref() {
                Some("0") => {}
                kind => {
                    let protocol = match kind {
                        Some("1") => "Telnet",
                        Some("4") => "RDP",
                        Some("5") => "VNC",
                        Some("7") => "SFTP",
                        Some("8") => "Serial",
                        _ => "other",
                    };
                    out.left_out.push(LeftOut {
                        name,
                        why: Why::NotSsh(protocol.into()),
                    });
                    continue;
                }
            }
            let Some(address) = param(HOST) else {
                out.left_out.push(LeftOut {
                    name,
                    why: Why::NoAddress,
                });
                continue;
            };
            out.hosts.push(Found {
                name,
                folder: folder.clone(),
                address,
                port: param(PORT).and_then(|p| p.parse().ok()),
                user: param(USER).filter(|u| u != "<default>"),
                key: param(KEY),
                forward_agent: param(AGENT_FORWARD).as_deref() == Some("-1"),
                jump: jumps(&params),
                description: fields
                    .get(5)
                    .map(|c| unescape(c.trim()))
                    .filter(|c| !c.is_empty()),
                color: fields.get(6).and_then(|c| color(c.trim())),
                ..Found::default()
            });
        }
    }
    out
}

/// The jump hosts, their ports and users, several joined by `__PIPE__`.
fn jumps(params: &[&str]) -> Vec<Jump> {
    let list = |i: usize| -> Vec<String> {
        params
            .get(i)
            .map(|p| p.split("__PIPE__").map(|s| unescape(s.trim())).collect())
            .unwrap_or_default()
    };
    let (hosts, ports, users) = (list(JUMP_HOSTS), list(JUMP_PORTS), list(JUMP_USERS));
    hosts
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty())
        .map(|(i, h)| Jump::Address {
            address: h.clone(),
            port: ports.get(i).and_then(|p| p.parse().ok()),
            user: users
                .get(i)
                .filter(|u| !u.is_empty() && *u != "<default>")
                .cloned(),
        })
        .collect()
}

/// A Windows color number, `0x00BBGGRR`; -1 is none.
fn color(text: &str) -> Option<String> {
    let n: i64 = text.parse().ok().filter(|n| *n >= 0)?;
    let n = n as u32;
    Some(crate::hex(n as u8, (n >> 8) as u8, (n >> 16) as u8))
}

fn unescape(s: &str) -> String {
    s.replace("__PERCENT__", "%")
        .replace("__DIEZE__", "#")
        .replace("__PTVIRG__", ";")
        .replace("__DBLQUO__", "\"")
        .replace("__PIPE__", "|")
        .replace("_ProfileDir_", "~")
        .replace("_CurrentDrive_", "C:")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session line's params, those given set at their place.
    fn params(set: &[(usize, &str)]) -> String {
        let mut p = vec![""; 40];
        for (i, v) in set {
            p[*i] = v;
        }
        p.join("%")
    }

    fn export() -> String {
        let bastion = params(&[
            (TYPE, "0"),
            (HOST, "bastion.example.com"),
            (PORT, "2222"),
            (USER, "ops"),
            (21, "1080"),
        ]);
        let web = params(&[
            (TYPE, "0"),
            (HOST, "10.0.1.11"),
            (PORT, "22"),
            (USER, "<default>"),
            (JUMP_HOSTS, "jump1.example.com__PIPE__jump2"),
            (JUMP_PORTS, "22__PIPE__2200"),
            (JUMP_USERS, "u1__PIPE__"),
            (KEY, "_ProfileDir_\\.ssh\\id_ed25519"),
            (AGENT_FORWARD, "-1"),
        ]);
        let desktop = params(&[(TYPE, "4"), (HOST, "10.0.2.5"), (PORT, "3389")]);
        format!(
            "[Bookmarks]\r\nSubRep=\r\nImgNum=42\r\n\
             bastion=#109#{bastion}#MobaFont%10#0# #-1\r\n\r\n\
             [Bookmarks_1]\r\nSubRep=Prod\\EU\r\nImgNum=41\r\n\
             web __DIEZE__1=#109#{web}#MobaFont%10#0#Front 100__PERCENT__ up#255\r\n\
             desktop=#91#{desktop}#MobaFont%10#0# #-1\r\n"
        )
    }

    #[test]
    fn an_export_gives_its_ssh_sessions_in_their_folders() {
        let read = read(&export());
        assert_eq!(read.hosts.len(), 2);
        let bastion = &read.hosts[0];
        assert!(bastion.folder.is_empty());
        assert_eq!(
            (
                bastion.address.as_str(),
                bastion.port,
                bastion.user.as_deref()
            ),
            ("bastion.example.com", Some(2222), Some("ops"))
        );
        assert_eq!(
            (bastion.description.as_deref(), bastion.color.as_deref()),
            (None, None)
        );
        let web = &read.hosts[1];
        assert_eq!(web.name, "web #1");
        assert_eq!(web.folder, ["Prod", "EU"]);
        assert_eq!(web.user, None);
        assert_eq!(web.key.as_deref(), Some("~\\.ssh\\id_ed25519"));
        assert!(web.forward_agent);
        assert_eq!(
            web.jump,
            [
                Jump::Address {
                    address: "jump1.example.com".into(),
                    port: Some(22),
                    user: Some("u1".into())
                },
                Jump::Address {
                    address: "jump2".into(),
                    port: Some(2200),
                    user: None
                }
            ]
        );
        assert_eq!(web.description.as_deref(), Some("Front 100% up"));
        assert_eq!(web.color.as_deref(), Some("#ff0000"));
        assert_eq!(
            read.left_out,
            [LeftOut {
                name: "desktop".into(),
                why: Why::NotSsh("RDP".into())
            }]
        );
    }
}

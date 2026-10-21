//! Xshell: a `Sessions` folder of `.xsh` files (INI, UTF-16LE from Xshell 7),
//! its subfolders the sessions' folders; or an `.xts` export, a zip of them.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

use crate::{App, Dirs, Error, Found, LeftOut, Look, Place, Read, Why};

pub(crate) fn places(dirs: &Dirs) -> Vec<(App, Place)> {
    let Some(docs) = &dirs.documents else {
        return vec![];
    };
    let mut out = vec![];
    for version in ["8", "7", "6"] {
        let dir = docs.join(format!("NetSarang Computer/{version}/Xshell/Sessions"));
        if dir.is_dir() {
            out.push((App::Xshell, Place::Folder(dir)));
            // The newest Xshell's sessions: older ones were copied into it.
            return out;
        }
    }
    let old = docs.join("NetSarang/Xshell/Sessions");
    if old.is_dir() {
        out.push((App::Xshell, Place::Folder(old)));
    }
    out
}

pub(crate) fn read_folder(dir: &Path) -> Result<Read, Error> {
    let mut out = Read::default();
    for (folder, path) in crate::files_in(dir, "xsh") {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        session(&mut out, folder, name, &crate::text(&std::fs::read(&path)?));
    }
    Ok(out)
}

/// An `.xts` export (a zip), or one `.xsh` file.
pub(crate) fn read_bytes(bytes: &[u8]) -> Result<Read, Error> {
    if !bytes.starts_with(b"PK") {
        let mut out = Read::default();
        let text = crate::text(bytes);
        let name = section(&text, "CONNECTION")
            .get("Host")
            .cloned()
            .unwrap_or_default();
        session(&mut out, vec![], name, &text);
        return Ok(out);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Format(e.to_string()))?;
    let mut entries = vec![];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Error::Format(e.to_string()))?;
        // Names are in the Windows code page on a Chinese Windows.
        let path = crate::text(entry.name_raw()).replace('\\', "/");
        if entry.is_dir() || !path.to_ascii_lowercase().ends_with(".xsh") {
            continue;
        }
        let mut data = vec![];
        entry.read_to_end(&mut data)?;
        entries.push((path, data));
    }
    entries.sort();
    let mut out = Read::default();
    for (path, data) in entries {
        let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let file = parts.pop().unwrap_or_default();
        let folder = match parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("Sessions"))
        {
            Some(at) => &parts[at + 1..],
            None => &parts[..],
        };
        let name = file
            .strip_suffix(".xsh")
            .or(file.strip_suffix(".XSH"))
            .unwrap_or(file);
        session(
            &mut out,
            folder.iter().map(|p| p.to_string()).collect(),
            name.to_string(),
            &crate::text(&data),
        );
    }
    Ok(out)
}

fn section(text: &str, name: &str) -> HashMap<String, String> {
    crate::ini(text)
        .into_iter()
        .filter(|(s, _)| s.eq_ignore_ascii_case(name))
        .flat_map(|(_, lines)| lines)
        .collect()
}

fn session(out: &mut Read, folder: Vec<String>, name: String, text: &str) {
    let connection = section(text, "CONNECTION");
    let auth = section(text, "CONNECTION:AUTHENTICATION");
    let info = section(text, "SessionInfo");
    let get = |m: &HashMap<String, String>, k: &str| {
        m.get(k)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let protocol = get(&connection, "Protocol").unwrap_or_else(|| "SSH".into());
    if !protocol.eq_ignore_ascii_case("SSH") {
        out.left_out.push(LeftOut {
            name,
            why: Why::NotSsh(protocol),
        });
        return;
    }
    let Some(address) = get(&connection, "Host") else {
        out.left_out.push(LeftOut {
            name,
            why: Why::NoAddress,
        });
        return;
    };
    // A key in Xshell's own store, by its name: to export from Xshell.
    let look = get(&auth, "UserKey")
        .map(Look::KeyInApp)
        .into_iter()
        .collect();
    out.hosts.push(Found {
        name,
        folder,
        address,
        port: get(&connection, "Port").and_then(|p| p.parse().ok()),
        user: get(&auth, "UserName"),
        description: get(&info, "Description"),
        look,
        ..Found::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const WEB: &str = "[SessionInfo]\r\nVersion=8.0\r\nDescription=Front end\r\n\
[CONNECTION]\r\nHost=10.0.1.11\r\nPort=2222\r\nProtocol=SSH\r\n\
[CONNECTION:AUTHENTICATION]\r\nUserName=deploy\r\nUserKey=id_rsa_2048\r\n";

    #[test]
    fn an_export_gives_its_sessions_in_their_folders() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        let options = zip::write::SimpleFileOptions::default();
        for (path, text) in [
            ("Xshell/Sessions/生产/web.xsh", WEB),
            (
                "Xshell/Sessions/console.xsh",
                "[CONNECTION]\r\nHost=10.0.0.9\r\nProtocol=TELNET\r\n",
            ),
            ("Xshell/Sessions/notes.txt", "not a session"),
        ] {
            zip.start_file(path, options).expect("entry");
            zip.write_all(&crate::utf16le(text)).expect("write");
        }
        let bytes = zip.finish().expect("zip").into_inner();
        let read = read_bytes(&bytes).expect("read");
        assert_eq!(read.hosts.len(), 1);
        let web = &read.hosts[0];
        assert_eq!(web.name, "web");
        assert_eq!(web.folder, ["生产"]);
        assert_eq!((web.address.as_str(), web.port), ("10.0.1.11", Some(2222)));
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.description.as_deref(), Some("Front end"));
        assert_eq!(web.look, [Look::KeyInApp("id_rsa_2048".into())]);
        assert_eq!(read.left_out[0].why, Why::NotSsh("TELNET".into()));
    }
}

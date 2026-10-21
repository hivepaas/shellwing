//! A CSV file: a header naming the columns (in any order, without case), then
//! a host per row. Commas, semicolons or tabs, whichever the header uses;
//! fields quoted as RFC 4180 says.

use crate::{Error, Found, Jump, LeftOut, Read, Why};

/// The names a column may have, by what it holds.
const COLUMNS: &[(&str, &[&str])] = &[
    ("name", &["name", "label", "title", "session"]),
    ("host", &["host", "hostname", "address", "ip", "server"]),
    ("port", &["port"]),
    ("user", &["user", "username", "login"]),
    ("group", &["group", "folder"]),
    ("tags", &["tags"]),
    ("key", &["key", "identity", "identity file", "private key"]),
    ("jump", &["jump", "jump host", "proxyjump"]),
    ("description", &["description", "notes", "note", "comment"]),
];

/// Whether `head` (the start of a file) looks like a CSV of hosts: a header
/// naming a host column.
pub(crate) fn looks_like(head: &str) -> bool {
    let Some(first) = head.lines().next() else {
        return false;
    };
    let sep = separator(first);
    row(first, sep).iter().any(|c| column(c) == Some("host"))
}

pub(crate) fn read(text: &str) -> Result<Read, Error> {
    let mut lines = records(text);
    let Some(header) = lines.first().cloned() else {
        return Ok(Read::default());
    };
    let sep = separator(&header);
    let names: Vec<Option<&str>> = row(&header, sep).iter().map(|c| column(c)).collect();
    if !names.contains(&Some("host")) {
        return Err(Error::Format(
            "no column names the host (host, hostname, address or ip)".into(),
        ));
    }
    lines.remove(0);
    let mut out = Read::default();
    for line in lines {
        let fields = row(&line, sep);
        let get = |what: &str| {
            names
                .iter()
                .position(|n| *n == Some(what))
                .and_then(|i| fields.get(i))
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty())
        };
        let address = get("host");
        let name = get("name").or_else(|| address.clone()).unwrap_or_default();
        let Some(address) = address else {
            if !name.is_empty() {
                out.left_out.push(LeftOut {
                    name,
                    why: Why::NoAddress,
                });
            }
            continue;
        };
        let jump = get("jump")
            .map(|j| {
                j.split(',')
                    .map(|j| Jump::Host(j.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let folder = get("group")
            .map(|g| {
                g.split(['/', '\\', '>'])
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        out.hosts.push(Found {
            name,
            folder,
            address,
            port: get("port").and_then(|p| p.parse().ok()),
            user: get("user"),
            key: get("key"),
            jump,
            description: get("description"),
            ..Found::default()
        });
    }
    Ok(out)
}

fn column(name: &str) -> Option<&'static str> {
    let name = name.trim().trim_matches('"').to_lowercase();
    COLUMNS
        .iter()
        .find(|(_, names)| names.contains(&name.as_str()))
        .map(|(what, _)| *what)
}

/// The separator the header uses most: a comma, a semicolon or a tab.
fn separator(header: &str) -> char {
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|c| header.matches(*c).count())
        .unwrap_or(',')
}

/// The file's records: lines, but a quoted field may hold line breaks.
fn records(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut current = String::new();
    let mut quoted = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        if !current.is_empty() || quoted {
            current.push('\n');
        }
        current.push_str(line);
        quoted ^= line.matches('"').count() % 2 == 1;
        if !quoted {
            if !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// A record's fields: `"` quotes a field, `""` in one is a quote.
fn row(line: &str, sep: char) -> Vec<String> {
    let mut fields = vec![];
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => fields.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_read_by_their_header() {
        let text = "\u{feff}Label;Hostname;Port;Username;Folder;Description\n\
                    web;10.0.0.1;2222;deploy;Prod/EU;\"Front, \"\"blue\"\"\nand green\"\n\
                    ;;;;;\n\
                    no address;;22;;;\n\
                    db;10.0.0.2;;;;\n";
        let read = read(text).expect("read");
        assert_eq!(read.hosts.len(), 2);
        let web = &read.hosts[0];
        assert_eq!(web.name, "web");
        assert_eq!((web.address.as_str(), web.port), ("10.0.0.1", Some(2222)));
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.folder, ["Prod", "EU"]);
        assert_eq!(
            web.description.as_deref(),
            Some("Front, \"blue\"\nand green")
        );
        assert_eq!(read.hosts[1].port, None);
        assert_eq!(
            read.left_out,
            [LeftOut {
                name: "no address".into(),
                why: Why::NoAddress
            }]
        );
    }

    #[test]
    fn a_file_without_a_host_column_is_refused() {
        assert!(read("name,port\nweb,22\n").is_err());
        assert!(!looks_like("name,port\n"));
        assert!(looks_like("Name\tIP\tUser\n"));
    }
}

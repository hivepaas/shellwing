//! `~/.ssh/config`: each `Host` name without a pattern, with what OpenSSH
//! would use to reach it (first value wins; keys and forwards add up).

use std::collections::HashMap;
use std::path::Path;

use crate::{Found, Jump, Read};

/// Includes nested deeper than this are not followed (a file that includes
/// itself would never end).
const MAX_DEPTH: usize = 8;

/// Which hosts the lines that follow are for.
#[derive(Clone)]
enum For {
    All,
    Hosts(Vec<String>),
    /// A `Match` block: its conditions are not weighed, so it counts for none.
    None,
}

struct Line {
    within: For,
    key: String,
    value: String,
}

/// The config at `path`, its `Include`s read from beside it.
pub(crate) fn read_file(path: &Path) -> Read {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Read::default();
    };
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    read(&text, &|pattern| included(&dir, pattern))
}

/// The files an `Include` names, relative to `dir` (`~/.ssh`), a `*` or `?`
/// in the file name matching several.
fn included(dir: &Path, pattern: &str) -> Option<Vec<String>> {
    let home = dir.parent().unwrap_or(dir);
    let path = match pattern.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => dir.join(pattern),
    };
    let name = path.file_name()?.to_string_lossy().into_owned();
    if !name.contains(['*', '?']) {
        return std::fs::read_to_string(&path).ok().map(|t| vec![t]);
    }
    let mut files: Vec<_> = std::fs::read_dir(path.parent()?)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| glob(&name, &n.to_string_lossy()))
        })
        .collect();
    files.sort();
    Some(
        files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect(),
    )
}

/// The hosts `text` names, `include` giving the text of the files an
/// `Include` line names.
pub(crate) fn read(text: &str, include: &dyn Fn(&str) -> Option<Vec<String>>) -> Read {
    let mut lines = vec![];
    parse(text, For::All, include, 0, &mut lines);
    let mut names: Vec<String> = vec![];
    for line in &lines {
        if line.key == "host" {
            for p in line.value.split_whitespace() {
                if !p.contains(['*', '?', '!']) && !names.iter().any(|n| n == p) {
                    names.push(p.to_string());
                }
            }
        }
    }
    let hosts = names.iter().map(|n| host(&lines, n, &names)).collect();
    Read {
        hosts,
        left_out: vec![],
    }
}

fn parse(
    text: &str,
    mut within: For,
    include: &dyn Fn(&str) -> Option<Vec<String>>,
    depth: usize,
    out: &mut Vec<Line>,
) {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split = line
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(line.len());
        let key = line[..split].to_ascii_lowercase();
        let value = line[split..]
            .trim_start()
            .trim_start_matches('=')
            .trim()
            .trim_matches('"')
            .to_string();
        match key.as_str() {
            "host" => {
                within = For::Hosts(value.split_whitespace().map(str::to_string).collect());
                out.push(Line {
                    within: For::None,
                    key,
                    value,
                });
            }
            "match" => within = For::None,
            "include" if depth < MAX_DEPTH => {
                for pattern in value.split_whitespace() {
                    for text in include(pattern).unwrap_or_default() {
                        parse(&text, within.clone(), include, depth + 1, out);
                    }
                }
            }
            _ => out.push(Line {
                within: within.clone(),
                key,
                value,
            }),
        }
    }
}

/// What OpenSSH would use for `name`.
fn host(lines: &[Line], name: &str, names: &[String]) -> Found {
    let mut first: HashMap<&str, &str> = HashMap::new();
    let mut keys = vec![];
    let mut forward = vec![];
    for line in lines.iter().filter(|l| applies(&l.within, name)) {
        let v = line.value.as_str();
        match line.key.as_str() {
            "identityfile" => keys.push(v),
            "localforward" => forward.push(format!(
                "-L {}",
                v.split_whitespace().collect::<Vec<_>>().join(":")
            )),
            "remoteforward" => forward.push(format!(
                "-R {}",
                v.split_whitespace().collect::<Vec<_>>().join(":")
            )),
            "dynamicforward" => forward.push(format!("-D {v}")),
            key => {
                first.entry(key).or_insert(v);
            }
        }
    }
    let address = first
        .get("hostname")
        .map(|h| h.replace("%h", name).replace("%%", "%"))
        .unwrap_or_else(|| name.to_string());
    let jump = match first.get("proxyjump") {
        Some(j) if !j.eq_ignore_ascii_case("none") => j
            .split(',')
            .map(str::trim)
            .filter(|j| !j.is_empty())
            .map(|j| {
                if names.iter().any(|n| n == j) {
                    Jump::Host(j.to_string())
                } else {
                    crate::address(j)
                }
            })
            .collect(),
        _ => vec![],
    };
    Found {
        name: name.to_string(),
        address,
        port: first.get("port").and_then(|p| p.parse().ok()),
        user: first.get("user").map(|u| u.to_string()),
        key: keys.first().map(|k| {
            k.strip_prefix("%d/")
                .map_or(k.to_string(), |r| format!("~/{r}"))
        }),
        forward_agent: first
            .get("forwardagent")
            .is_some_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "no" | "false")),
        jump,
        forward,
        ..Found::default()
    }
}

fn applies(within: &For, name: &str) -> bool {
    match within {
        For::All => true,
        For::None => false,
        For::Hosts(patterns) => {
            let mut hit = false;
            for p in patterns {
                if let Some(not) = p.strip_prefix('!') {
                    if glob(not, name) {
                        return false;
                    }
                } else if glob(p, name) {
                    hit = true;
                }
            }
            hit
        }
    }
}

/// `*` and `?` as ssh_config's patterns, without case.
fn glob(pattern: &str, s: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let s: Vec<char> = s.to_lowercase().chars().collect();
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
# Everything
User me

Host bastion
  HostName bastion.example.com
  Port 2222
  User ops

Host web web-alias
  HostName 10.0.1.11
  ProxyJump bastion
  IdentityFile ~/.ssh/deploy_ed25519
  IdentityFile ~/.ssh/other
  LocalForward 5432 localhost:5432
  LocalForward 127.0.0.1:6379 cache:6379
  DynamicForward 1080
  RemoteForward 9000 localhost:3000
  ForwardAgent yes

Host *.prod !skip.prod
  User deployer
  ProxyJump admin@jump.example.com:2200

Host db.prod skip.prod
  Port=2201

Match host foo
  User nobody

Host *
  User fallback
  ServerAliveInterval 30
"#;

    fn hosts(text: &str) -> Vec<Found> {
        read(text, &|_| None).hosts
    }

    #[test]
    fn each_host_gets_what_openssh_would_use() {
        let h = hosts(CONFIG);
        let names: Vec<&str> = h.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            ["bastion", "web", "web-alias", "db.prod", "skip.prod"]
        );
        let bastion = &h[0];
        assert_eq!(bastion.address, "bastion.example.com");
        assert_eq!(
            (bastion.port, bastion.user.as_deref()),
            (Some(2222), Some("me"))
        );
        let web = &h[1];
        assert_eq!(web.address, "10.0.1.11");
        assert_eq!(web.jump, [Jump::Host("bastion".into())]);
        assert_eq!(web.key.as_deref(), Some("~/.ssh/deploy_ed25519"));
        assert!(web.forward_agent);
        assert_eq!(
            web.forward,
            [
                "-L 5432:localhost:5432",
                "-L 127.0.0.1:6379:cache:6379",
                "-D 1080",
                "-R 9000:localhost:3000"
            ]
        );
        let db = &h[3];
        assert_eq!((db.address.as_str(), db.port), ("db.prod", Some(2201)));
        assert_eq!(db.user.as_deref(), Some("me"), "the first User wins");
        assert_eq!(
            db.jump,
            [Jump::Address {
                address: "jump.example.com".into(),
                port: Some(2200),
                user: Some("admin".into())
            }]
        );
        assert!(h[4].jump.is_empty(), "negated: *.prod is not for it");
    }

    #[test]
    fn includes_are_read_where_they_stand() {
        let text = "Include conf.d/*\nHost main\n  HostName m\n";
        let read = read(text, &|p| {
            (p == "conf.d/*").then(|| vec!["Host inc\n  HostName 10.0.0.9\n  User x\n".to_string()])
        });
        let names: Vec<&str> = read.hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["inc", "main"]);
        assert_eq!(read.hosts[0].user.as_deref(), Some("x"));
        assert_eq!(
            read.hosts[1].user, None,
            "the included Host block ends with its file"
        );
    }

    #[test]
    fn patterns_match_as_ssh_does() {
        assert!(glob("*.prod", "db.prod"));
        assert!(glob("web-?", "WEB-1"));
        assert!(!glob("web-?", "web-10"));
        assert!(glob("*", ""));
    }
}

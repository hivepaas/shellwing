//! Snippets: commands kept in the vault, typed into panes from the palette.
//! A `{{name}}` in one is filled in when it runs: `host`, `user` and
//! `address` from the pane's host, any other asked for first.

use std::collections::BTreeMap;

use shellwing_i18n::tr;

pub use shellwing_vault::Snippet;

/// Filled in from the pane's host.
pub const BUILT_IN: [&str; 3] = ["host", "user", "address"];

/// Where the palette offers a snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Every,
    Group(String),
    Host(String),
}

impl Scope {
    /// As the vault keeps it: empty, `group:<name>` or `host:<name>`.
    pub fn parse(s: &str) -> Scope {
        if let Some(g) = s.strip_prefix("group:") {
            Scope::Group(g.to_string())
        } else if let Some(h) = s.strip_prefix("host:") {
            Scope::Host(h.to_string())
        } else {
            Scope::Every
        }
    }

    pub fn field(&self) -> String {
        match self {
            Scope::Every => String::new(),
            Scope::Group(g) => format!("group:{g}"),
            Scope::Host(h) => format!("host:{h}"),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Scope::Every => tr!("every host"),
            Scope::Group(g) | Scope::Host(g) => g.clone(),
        }
    }

    /// Whether a pane of `host` (its name in hosts.toml or as typed), in
    /// `group`, is offered it.
    pub fn fits(&self, host: Option<&str>, group: Option<&str>) -> bool {
        match self {
            Scope::Every => true,
            Scope::Group(g) => group == Some(g.as_str()),
            Scope::Host(h) => host == Some(h.as_str()),
        }
    }
}

fn var_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// Each `{{name}}` in `command`: where it is (the braces included), and the name.
/// A name starts with a letter or `_`: Go templates as Docker and kubectl
/// take them (`{{.Names}}`, `{{json .State}}`) stay as they are.
pub fn holes(command: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = vec![];
    let mut from = 0;
    while let Some(open) = command[from..].find("{{").map(|i| from + i) {
        let Some(close) = command[open + 2..].find("}}").map(|i| open + 2 + i) else {
            break;
        };
        let name = command[open + 2..close].trim();
        let starts = name.starts_with(|c: char| c.is_alphabetic() || c == '_');
        if starts && name.chars().all(var_char) {
            out.push((open..close + 2, name.to_string()));
            from = close + 2;
        } else {
            from = open + 2;
        }
    }
    out
}

/// The names in `command`, each once, in order.
pub fn vars(command: &str) -> Vec<String> {
    let mut seen: Vec<String> = vec![];
    for (_, name) in holes(command) {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// The names to ask for: those not filled in from the host.
pub fn asked(command: &str) -> Vec<String> {
    vars(command)
        .into_iter()
        .filter(|n| !BUILT_IN.contains(&n.as_str()))
        .collect()
}

/// `command` with each `{{name}}` that `values` has replaced; the others
/// stay as they are.
/// A command of several lines on one, for a list: ` ⏎ ` between them.
pub fn one_line(command: &str) -> String {
    command
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ⏎ ")
}

pub fn fill(command: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (range, name) in holes(command) {
        if let Some(v) = values.get(&name) {
            out.push_str(&command[at..range.start]);
            out.push_str(v);
            at = range.end;
        }
    }
    out.push_str(&command[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_lines_show_on_one() {
        assert_eq!(
            one_line("cd /srv\n  make deploy\n\n"),
            "cd /srv ⏎   make deploy"
        );
        assert_eq!(one_line("df -h"), "df -h");
    }

    #[test]
    fn names_are_found_once_and_filled() {
        let cmd = "sudo systemctl restart {{service}} && systemctl status {{ service }} -H {{host}} {{not a name}} {{.Names}} {{-x}} {{";
        assert_eq!(vars(cmd), ["service", "host"]);
        assert_eq!(asked(cmd), ["service"]);
        let values = BTreeMap::from([
            ("service".to_string(), "nginx".to_string()),
            ("host".to_string(), "api-1".to_string()),
        ]);
        assert_eq!(
            fill(cmd, &values),
            "sudo systemctl restart nginx && systemctl status nginx -H api-1 {{not a name}} {{.Names}} {{-x}} {{"
        );
        // A value is not read again for names.
        let odd = BTreeMap::from([("a".to_string(), "{{b}}".to_string())]);
        assert_eq!(fill("{{a}} {{b}}", &odd), "{{b}} {{b}}");
    }

    #[test]
    fn scopes_read_back_and_fit() {
        for s in [
            Scope::Every,
            Scope::Group("Production".into()),
            Scope::Host("api-1".into()),
        ] {
            assert_eq!(Scope::parse(&s.field()), s);
        }
        assert!(Scope::Every.fits(None, None));
        assert!(Scope::Group("Production".into()).fits(Some("api-1"), Some("Production")));
        assert!(!Scope::Group("Production".into()).fits(Some("api-1"), None));
        assert!(Scope::Host("api-1".into()).fits(Some("api-1"), None));
        assert!(!Scope::Host("api-1".into()).fits(Some("api-2"), Some("Production")));
    }
}

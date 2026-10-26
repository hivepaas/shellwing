//! The snippet library: ready commands for servers, in two files. Those for
//! the system, services, the network, Docker, Kubernetes and the like are
//! written here (`library/extra.toml`); those for databases come from
//! HivePaaS's command templates (`scripts/library_from_hivepaas.py` makes
//! `library/items.json`). Picked in the library's window, each becomes a
//! snippet of the user's own, noted as coming from here. In English only:
//! commands and their tools' words are.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use serde::Deserialize;

/// A snippet's origin, for one added from here: this, then the item's id.
pub const ORIGIN: &str = "library:";

#[derive(Debug, Deserialize)]
pub struct Item {
    // An item of extra.toml may leave out what it does not have.
    /// Its template's: `diagnostics/pg_ping`.
    pub id: String,
    /// `PostgreSQL`.
    pub tool: String,
    /// `Diagnostics`, `Backup`…
    pub task: String,
    pub name: String,
    pub about: String,
    /// With `<options>` where those chosen go, if it has any; and `<sudo>`
    /// before each command of it that sudo goes before, when not only the
    /// first (`<sudo>nginx -t && <sudo>systemctl reload nginx`).
    pub command: String,
    #[serde(default)]
    pub options: Vec<String>,
    /// The variable the tool takes the password from (`PGPASSWORD`), or
    /// `DB_PASSWORD` that the command names; None: it needs none.
    #[serde(default)]
    pub password: Option<String>,
    /// It changes things (data, a service, a deployment) or takes them
    /// away: typed, not run, until told.
    #[serde(default)]
    pub changes_data: bool,
    #[serde(default)]
    pub docs: Option<String>,
    /// It needs root: added with sudo unless the user says not.
    #[serde(default)]
    pub sudo: bool,
}

#[derive(Deserialize)]
struct File {
    items: Vec<Item>,
}

/// `library/extra.toml`: the tools' order, and the items written there.
#[derive(Deserialize)]
struct Extra {
    tools: Vec<String>,
    item: Vec<Item>,
}

static ITEMS: LazyLock<Vec<Item>> = LazyLock::new(|| {
    let extra: Extra = toml::from_str(include_str!("library/extra.toml")).unwrap_or(Extra {
        tools: vec![],
        item: vec![],
    });
    let hivepaas = serde_json::from_str::<File>(include_str!("library/items.json"))
        .map(|f| f.items)
        .unwrap_or_default();
    let mut items: Vec<Item> = extra.item.into_iter().chain(hivepaas).collect();
    for i in &mut items {
        i.command = i.command.trim().to_string();
    }
    // Tools as extra.toml orders them, the others after, as they come; in a
    // tool, by task.
    let mut order = extra.tools;
    for i in &items {
        if !order.contains(&i.tool) {
            order.push(i.tool.clone());
        }
    }
    let rank = |i: &Item| {
        (
            order.iter().position(|t| *t == i.tool),
            TASKS.iter().position(|t| *t == i.task),
        )
    };
    items.sort_by_key(rank);
    items
});

/// Every item, by tool, then task.
pub fn items() -> &'static [Item] {
    &ITEMS
}

/// The tools, in the library's order, and how many items each has.
pub fn tools() -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = vec![];
    for i in items() {
        match out.iter_mut().find(|(t, _)| *t == i.tool) {
            Some((_, n)) => *n += 1,
            None => out.push((i.tool.as_str(), 1)),
        }
    }
    out
}

/// The tasks, in the order the library lists them.
pub const TASKS: [&str; 8] = [
    "Diagnostics",
    "Logs",
    "Maintenance",
    "Deploy",
    "Backup",
    "Database",
    "Data",
    "Testing",
];

/// How an item is to become a snippet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    /// Its options taken, by index.
    pub options: BTreeSet<usize>,
    /// Run in a Docker container (`{{container}}`) rather than on the host.
    pub container: bool,
    /// With sudo, or not; None: as the item has it.
    pub sudo: Option<bool>,
}

impl Item {
    /// Where a snippet made from it came from.
    pub fn origin(&self) -> String {
        format!("{ORIGIN}{}", self.id)
    }

    /// Docker's own, and those that start a shell of their own, run on the
    /// host only.
    pub fn can_contain(&self) -> bool {
        self.tool != "Docker" && !self.command.starts_with("sh -c")
    }

    /// Run with sudo, as `choice` has it.
    pub fn sudo(&self, choice: &Choice) -> bool {
        choice.sudo.unwrap_or(self.sudo)
    }

    /// The command as `choice` has it.
    pub fn command(&self, choice: &Choice) -> String {
        let options: Vec<&str> = choice
            .options
            .iter()
            .filter_map(|i| self.options.get(*i).map(String::as_str))
            .collect();
        let taken = if options.is_empty() {
            String::new()
        } else {
            format!(" {}", options.join(" "))
        };
        let command = self.command.replace("<options>", &taken);
        // sudo forgets the environment: the password's variable is kept.
        let sudo = match (self.sudo(choice), self.password.as_deref()) {
            (false, _) => String::new(),
            (true, Some(v)) => format!("sudo --preserve-env={v} "),
            (true, None) => "sudo ".to_string(),
        };
        if !(choice.container && self.can_contain()) {
            return if command.contains("<sudo>") {
                command.replace("<sudo>", &sudo)
            } else {
                format!("{sudo}{command}")
            };
        }
        // In a container, sudo is for docker; the password's variable goes
        // in with it: the container has its own environment.
        let env = self
            .password
            .as_deref()
            .map(|v| format!("-e {v} "))
            .unwrap_or_default();
        let command = command.replace("<sudo>", "");
        format!("{sudo}docker exec -it {env}{{{{container}}}} {command}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> &'static Item {
        items().iter().find(|i| i.id == id).expect("item")
    }

    #[test]
    fn the_library_reads_and_each_item_is_whole() {
        assert!(items().len() >= 90, "{}", items().len());
        for i in items() {
            assert!(!i.name.is_empty() && !i.command.is_empty(), "{}", i.id);
            assert!(TASKS.contains(&i.task.as_str()), "{}: {}", i.id, i.task);
            // Options where they go, or none.
            assert_eq!(
                i.command.contains("<options>"),
                !i.options.is_empty(),
                "{}",
                i.id
            );
            assert!(!i.command.contains("${"), "{}", i.id);
            // A password is never in a command, only its variable's name.
            assert!(!i.command.to_lowercase().contains("hivepaas"), "{}", i.id);
        }
        let tools = tools();
        assert_eq!(tools[0].0, "System");
        assert!(tools.contains(&("PostgreSQL", 16)));
        assert_eq!(tools.iter().map(|t| t.1).sum::<usize>(), items().len());
        // A tool's items together, and no id twice.
        let mut seen = std::collections::HashSet::new();
        let mut done: Vec<&str> = vec![];
        for (n, i) in items().iter().enumerate() {
            assert!(seen.insert(i.id.as_str()), "{} twice", i.id);
            if n > 0 && items()[n - 1].tool != i.tool {
                assert!(!done.contains(&i.tool.as_str()), "{} apart", i.tool);
                done.push(&items()[n - 1].tool);
            }
            // The snippet's values are its own: no Go template is one.
            for (_, name) in crate::snippets::holes(&i.command) {
                assert!(
                    !crate::snippets::BUILT_IN.contains(&name.as_str()),
                    "{}: {name}",
                    i.id
                );
            }
        }
    }

    #[test]
    fn options_and_a_container_make_the_command() {
        let dump = item("backup/pg_dump");
        assert_eq!(
            dump.command(&Choice::default()),
            "pg_dump --host=localhost --port=5432 --username={{db_user}} --dbname={{database}} --format=custom --no-owner --verbose --file=/tmp/{{database}}.dump"
        );
        let clean = dump
            .options
            .iter()
            .position(|o| o == "--clean")
            .expect("--clean");
        let choice = Choice {
            options: [clean].into(),
            container: true,
            sudo: None,
        };
        assert_eq!(
            dump.command(&choice),
            "docker exec -it -e PGPASSWORD {{container}} pg_dump --host=localhost --port=5432 --username={{db_user}} --dbname={{database}} --format=custom --no-owner --verbose --clean --file=/tmp/{{database}}.dump"
        );
        // Docker's own run on the host whatever is chosen.
        let prune = item("maintenance/docker_prune_all");
        assert!(prune.changes_data && !prune.can_contain());
        assert_eq!(
            prune.command(&Choice {
                container: true,
                ..Choice::default()
            }),
            "docker system prune -a -f --volumes"
        );
    }

    #[test]
    fn sudo_goes_where_it_is_needed() {
        let reload = item("web/nginx_test_reload");
        assert!(reload.sudo && reload.changes_data);
        assert_eq!(
            reload.command(&Choice::default()),
            "sudo nginx -t && sudo systemctl reload nginx"
        );
        assert_eq!(
            reload.command(&Choice {
                sudo: Some(false),
                ..Choice::default()
            }),
            "nginx -t && systemctl reload nginx"
        );
        // One command: before it. With a password: kept through sudo.
        let ports = item("network/listening");
        assert_eq!(ports.command(&Choice::default()), "sudo ss -tulpn");
        let ping = item("diagnostics/pg_ping");
        assert!(
            ping.command(&Choice {
                sudo: Some(true),
                ..Choice::default()
            })
            .starts_with("sudo --preserve-env=PGPASSWORD psql ")
        );
    }
}

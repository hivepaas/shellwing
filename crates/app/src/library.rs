//! The snippet library: ready commands for databases and servers, from
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
    /// Its template's: `diagnostics/pg_ping`.
    pub id: String,
    /// `PostgreSQL`.
    pub tool: String,
    /// `Diagnostics`, `Backup`…
    pub task: String,
    pub name: String,
    pub about: String,
    /// With `<options>` where those chosen go, if it has any.
    pub command: String,
    pub options: Vec<String>,
    /// The variable the tool takes the password from (`PGPASSWORD`), or
    /// `DB_PASSWORD` that the command names; None: it needs none.
    pub password: Option<String>,
    /// It changes data or takes things away: typed, not run, until told.
    pub changes_data: bool,
    pub docs: Option<String>,
}

#[derive(Deserialize)]
struct File {
    items: Vec<Item>,
}

static ITEMS: LazyLock<Vec<Item>> = LazyLock::new(|| {
    serde_json::from_str::<File>(include_str!("library/items.json"))
        .map(|f| f.items)
        .unwrap_or_default()
});

/// Every item, by tool, then task, then name.
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
pub const TASKS: [&str; 6] = [
    "Diagnostics",
    "Maintenance",
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
        if !(choice.container && self.can_contain()) {
            return command;
        }
        // The password's variable goes in with it: the container has its
        // own environment.
        let env = self
            .password
            .as_deref()
            .map(|v| format!("-e {v} "))
            .unwrap_or_default();
        format!("docker exec -it {env}{{{{container}}}} {command}")
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
        assert_eq!(tools[0], ("PostgreSQL", 16));
        assert_eq!(tools.iter().map(|t| t.1).sum::<usize>(), items().len());
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
}

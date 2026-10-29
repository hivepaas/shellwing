//! What the vault keeps besides secrets, as the desktop's Library has it:
//! snippets (run in a session, their `{{holes}}` asked for), environments
//! (loaded into a session's shell, or given to one snippet) and notes.

use shellwing_i18n::tr;
use std::collections::HashMap;
use std::sync::Arc;

use shellwing_core::envs::LoadedEnv;
use shellwing_core::{environment, snippets};
use shellwing_vault::{Note, NoteFormat, Snippet};

use crate::{MobileError, MobileSession, Shellwing};

/// A snippet as the list shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SnippetRow {
    pub id: String,
    pub name: String,
    pub category: String,
    pub command: String,
    /// On one line, for the list.
    pub one_line: String,
    /// The holes asked for when it runs, in order (not host, user, address:
    /// those come from the session).
    pub asked: Vec<String>,
    /// The environments it runs with, by id.
    pub needs: Vec<String>,
    /// Enter pressed after it is typed.
    pub enter: bool,
    /// Where it is offered: empty, `group:<name>` or `host:<name>`.
    pub scope: String,
}

/// A snippet as its editor saves it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SnippetEdit {
    /// Empty: a new one.
    pub id: String,
    pub name: String,
    pub category: String,
    pub command: String,
    pub enter: bool,
    pub scope: String,
    /// The environments it runs with, by id.
    pub needs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EnvRow {
    pub id: String,
    pub name: String,
    pub scope: String,
    /// Its variables' names; their values stay in the vault.
    pub names: Vec<String>,
}

/// An environment's `.env` text with its values shown (see
/// [`Shellwing::env_text_shown`]).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EnvShown {
    pub text: String,
    /// Variables whose value is of several lines, which a line of the text
    /// cannot hold: they stay kept (`NAME=`), as the desktop shows them.
    pub not_shown: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NoteRow {
    pub id: String,
    pub title: String,
    pub category: String,
    pub markdown: bool,
    /// Kept hidden until the owner says it is them.
    pub sensitive: bool,
    /// The text's bytes.
    pub size: u64,
    /// The hosts it is about, by name.
    pub hosts: Vec<String>,
    /// About the session's host: offered first there.
    pub here: bool,
}

#[uniffi::export]
impl Shellwing {
    /// The snippets offered in a session to `spec` (None: all of them), by
    /// category then name.
    pub fn snippets(&self, spec: Option<String>) -> Vec<SnippetRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        let (host, group) = spec.map(|s| self.host_and_group(&s)).unwrap_or_default();
        let mut out: Vec<SnippetRow> = v
            .snippets()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| {
                host.is_none() && group.is_none()
                    || snippets::Scope::parse(&s.scope).fits(host.as_deref(), group.as_deref())
            })
            .map(|s| SnippetRow {
                one_line: snippets::one_line(&s.command),
                asked: snippets::asked(&s.command),
                id: s.id,
                name: s.name,
                category: s.category,
                command: s.command,
                needs: s.needs,
                enter: s.enter,
                scope: s.scope,
            })
            .collect();
        out.sort_by(|a, b| {
            (a.category.to_lowercase(), a.name.to_lowercase())
                .cmp(&(b.category.to_lowercase(), b.name.to_lowercase()))
        });
        out
    }

    pub fn save_snippet(&self, edit: SnippetEdit) -> Result<String, MobileError> {
        let vault = self.open_vault()?;
        let old = vault
            .snippets()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|s| !edit.id.is_empty() && s.id == edit.id);
        let mut s = old.unwrap_or(Snippet {
            id: String::new(),
            name: String::new(),
            command: String::new(),
            scope: String::new(),
            enter: true,
            category: String::new(),
            needs: vec![],
            origin: String::new(),
        });
        if edit.name.trim().is_empty() || edit.command.trim().is_empty() {
            return Err(MobileError::Failed(tr!("A name and a command are needed.")));
        }
        s.name = edit.name.trim().to_string();
        s.category = edit.category.trim().to_string();
        s.command = edit.command;
        s.enter = edit.enter;
        s.scope = edit.scope;
        s.needs = edit.needs;
        vault
            .save_snippet(&s)
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))
    }

    /// Types snippet `id` into `session`, its holes filled from `values`
    /// (and host, user and address from the session); with the environments
    /// it needs, in a subshell that has them.
    pub fn run_snippet(
        &self,
        session: Arc<MobileSession>,
        id: String,
        values: HashMap<String, String>,
    ) -> Result<(), MobileError> {
        let vault = self.open_vault()?;
        let s = vault
            .snippets()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| MobileError::Failed(tr!("It is not in the vault any more.")))?;
        if s.command.len() > shellwing_vault::limits::COMMAND {
            let o = shellwing_vault::Oversize::Command(s.command.len());
            return Err(MobileError::Failed(environment::said_oversize(&o)));
        }
        let mut all: std::collections::BTreeMap<String, String> = values.into_iter().collect();
        for (k, v) in session.built_ins() {
            all.insert(k, v);
        }
        let text = snippets::fill(&s.command, &all);
        if s.needs.is_empty() {
            return session.type_text(&text, s.enter);
        }
        let mut values: shellwing_vault::EnvValues = vec![];
        for env in &s.needs {
            for (n, v) in vault
                .environment_values(env)
                .map_err(MobileError::of)?
                .unwrap_or_default()
            {
                values.retain(|(m, _)| m != &n);
                values.push((n, v));
            }
        }
        session.run_with_env(values, &text, s.enter, &self.rt)
    }

    pub fn environments(&self) -> Vec<EnvRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        let mut out: Vec<EnvRow> = v
            .environments()
            .unwrap_or_default()
            .into_iter()
            .map(|e| EnvRow {
                id: e.id,
                name: e.name,
                scope: e.scope,
                names: e.names,
            })
            .collect();
        out.sort_by_key(|e| e.name.to_lowercase());
        out
    }

    /// Saves an environment from `.env` text (`NAME=value` a line); a name
    /// left without a value keeps the one it had.
    pub fn save_environment(
        &self,
        id: String,
        name: String,
        scope: String,
        dotenv: String,
    ) -> Result<String, MobileError> {
        if name.trim().is_empty() {
            return Err(MobileError::Failed(tr!("A name is needed.")));
        }
        let vars = environment::parse_dotenv(&dotenv).map_err(MobileError::Failed)?;
        self.open_vault()?
            .save_environment(&id, name.trim(), &scope, &vars)
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))
    }

    /// `text`, as the editor holds it, with each variable left to keep its
    /// value (`NAME=`) given the one environment `id` keeps. The app asks
    /// it is the user first, as the desktop does before its eyes show them.
    pub fn env_text_shown(&self, id: String, text: String) -> Result<EnvShown, MobileError> {
        let values = self.env_values(&id)?;
        let mut not_shown = vec![];
        let lines = text.lines().map(|line| {
            let (Some((name, None)), Some(eq)) = (dotenv_line(line), line.find('=')) else {
                return line.to_string();
            };
            match values.iter().find(|(n, _)| *n == name) {
                Some((_, v)) if v.contains(['\n', '\r']) => {
                    not_shown.push(name);
                    line.to_string()
                }
                Some((_, v)) => format!("{}{}", &line[..=eq], dotenv_value(v)),
                None => line.to_string(),
            }
        });
        let text = same_end(lines.collect(), &text);
        Ok(EnvShown { text, not_shown })
    }

    /// `text` with each value shown and not changed since kept again
    /// (`NAME=`): what was typed stays, to replace the value when saved.
    pub fn env_text_hidden(&self, id: String, text: String) -> String {
        let Ok(values) = self.env_values(&id) else {
            return text;
        };
        let lines = text
            .lines()
            .map(|line| match (dotenv_line(line), line.find('=')) {
                (Some((name, Some(value))), Some(eq))
                    if values
                        .iter()
                        .any(|(n, v)| *n == name && v.as_str() == value) =>
                {
                    line[..=eq].to_string()
                }
                _ => line.to_string(),
            });
        same_end(lines.collect(), &text)
    }

    /// Loads environment `id` into `session`'s shell.
    pub fn load_env(&self, session: Arc<MobileSession>, id: String) -> Result<(), MobileError> {
        let vault = self.open_vault()?;
        let env = vault
            .environments()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| MobileError::Failed(tr!("It is not in the vault any more.")))?;
        let values = vault
            .environment_values(&id)
            .map_err(MobileError::of)?
            .unwrap_or_default();
        session.load_env(
            LoadedEnv {
                id: env.id,
                name: env.name,
                names: vec![],
                bytes: 0,
            },
            values,
            &self.rt,
        )
    }

    /// The notes by category then title; with `spec`, those about its host
    /// first, as the desktop's palette offers them in a pane.
    pub fn notes(&self, spec: Option<String>) -> Vec<NoteRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        let host = spec.and_then(|s| self.host_and_group(&s).0);
        let mut out: Vec<NoteRow> = v
            .notes()
            .unwrap_or_default()
            .into_iter()
            .map(|n| NoteRow {
                id: n.id,
                title: n.title,
                category: n.category,
                markdown: n.format == NoteFormat::Markdown,
                sensitive: n.sensitive,
                size: n.size as u64,
                here: host.as_ref().is_some_and(|h| n.hosts.contains(h)),
                hosts: n.hosts,
            })
            .collect();
        out.sort_by(|a, b| {
            (!a.here, a.category.to_lowercase(), a.title.to_lowercase()).cmp(&(
                !b.here,
                b.category.to_lowercase(),
                b.title.to_lowercase(),
            ))
        });
        out
    }

    /// The image a note shows as `attachment:<id>`: PNG or JPEG bytes.
    pub fn note_image(&self, id: String) -> Option<Vec<u8>> {
        let bytes = self.vault.get()?.attachment(&id).ok()??;
        Some(bytes.to_vec())
    }

    /// Keeps an image for a note (made smaller if need be, as the desktop
    /// does); returns the Markdown line that shows it.
    pub fn add_note_image(&self, bytes: Vec<u8>) -> Result<String, MobileError> {
        let prepared = shellwing_core::images::prepare(&bytes).map_err(|e| match e {
            shellwing_core::images::PrepareError::TooLarge(n) => MobileError::Failed(
                environment::said_oversize(&shellwing_vault::Oversize::Attachment(n)),
            ),
            e => MobileError::of(e),
        })?;
        let id = self
            .open_vault()?
            .add_attachment(&prepared.bytes)
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))?;
        Ok(shellwing_vault::attachment_line(&id))
    }

    pub fn note_text(&self, id: String) -> Result<String, MobileError> {
        Ok(self
            .open_vault()?
            .note_text(&id)
            .map_err(MobileError::of)?
            .map(|t| t.to_string())
            .unwrap_or_default())
    }

    /// Saves a note: a new one when `id` is empty. `text` None keeps the
    /// one it has (a sensitive note not shown).
    #[allow(clippy::too_many_arguments)]
    pub fn save_note(
        &self,
        id: String,
        title: String,
        category: String,
        markdown: bool,
        hosts: Vec<String>,
        sensitive: bool,
        text: Option<String>,
    ) -> Result<String, MobileError> {
        let vault = self.open_vault()?;
        let old = vault
            .notes()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|n| !id.is_empty() && n.id == id);
        let mut n = old.unwrap_or(Note {
            id: String::new(),
            title: String::new(),
            category: String::new(),
            format: NoteFormat::Plain,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        });
        n.title = title.trim().to_string();
        n.category = category.trim().to_string();
        n.format = if markdown {
            NoteFormat::Markdown
        } else {
            NoteFormat::Plain
        };
        n.hosts = hosts;
        n.sensitive = sensitive;
        vault
            .save_note(&n, text.as_deref())
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))
    }
}

/// What a new note starts with, in the app's language, as the desktop's
/// does: how to write one, a code block, and that a session inserts it. Its
/// words are a phone's (a tap, Preview, a session's Notes…). Left as it is,
/// with nothing else given, the note is not kept.
#[uniffi::export]
pub fn note_template() -> String {
    match shellwing_i18n::language() {
        "vi" => include_str!("note_templates/vi.md"),
        "de" => include_str!("note_templates/de.md"),
        "es" => include_str!("note_templates/es.md"),
        "fr" => include_str!("note_templates/fr.md"),
        "ja" => include_str!("note_templates/ja.md"),
        "ko" => include_str!("note_templates/ko.md"),
        "pt" => include_str!("note_templates/pt.md"),
        "ru" => include_str!("note_templates/ru.md"),
        "zh" => include_str!("note_templates/zh.md"),
        "zh-Hant" => include_str!("note_templates/zh-Hant.md"),
        "id" => include_str!("note_templates/id.md"),
        "tr" => include_str!("note_templates/tr.md"),
        _ => include_str!("note_templates/en.md"),
    }
    .to_string()
}

impl Shellwing {
    /// The values environment `id` keeps; none for one not saved yet.
    fn env_values(&self, id: &str) -> Result<shellwing_vault::EnvValues, MobileError> {
        self.open_vault()?
            .environment_values(id)
            .map(Option::unwrap_or_default)
            .map_err(MobileError::of)
    }

    /// The hosts.toml entry `spec` names (its name) and its group, for the
    /// snippets offered there.
    pub(crate) fn host_and_group(&self, spec: &str) -> (Option<String>, Option<String>) {
        let Ok(hosts) = self.store.hosts() else {
            return (None, None);
        };
        let name = shellwing_ssh::config::split(spec)
            .map(|(_, n, _)| n)
            .unwrap_or_default();
        match hosts.find(&name) {
            Some(h) => (Some(h.name.clone()), h.group.clone()),
            None => (Some(name), None),
        }
    }
}

/// A ready command of the snippet library (`shellwing_core::library`), as
/// the desktop's library window lists it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LibraryItem {
    /// Its place in the library: what [`Shellwing::add_from_library`] takes.
    pub index: u32,
    pub id: String,
    /// `PostgreSQL`.
    pub tool: String,
    /// `Backup`.
    pub task: String,
    pub name: String,
    pub about: String,
    /// Options to add to its command.
    pub options: Vec<String>,
    /// The variable it reads its password from (`PGPASSWORD`).
    pub password: Option<String>,
    /// It changes things: added, it is typed and waits for Enter.
    pub changes_data: bool,
    /// It needs root: added with sudo unless told not.
    pub sudo: bool,
    /// It can run in a Docker container rather than on the host.
    pub can_contain: bool,
    pub docs: Option<String>,
    /// How many of the user's snippets were made from it.
    pub added: u32,
}

/// How an item becomes a snippet.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LibraryPick {
    pub index: u32,
    /// The options taken, by index.
    pub options: Vec<u32>,
    /// In a Docker container (`{{container}}`) rather than on the host.
    pub container: bool,
    /// None: as the item has it.
    pub sudo: Option<bool>,
    /// The environment that has its password: None, the one found for it
    /// ([`Shellwing::library_env`]); `Some("")`: none.
    pub env: Option<String>,
}

/// The category snippets from the library go under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FileUnder {
    /// `PostgreSQL`.
    Tool,
    /// `Backup`; the name says its tool: `PostgreSQL: Ping`.
    Task,
    /// `PostgreSQL · Backup`.
    Both,
}

/// The library's tasks, in its order.
#[uniffi::export]
pub fn library_tasks() -> Vec<String> {
    shellwing_core::library::TASKS
        .iter()
        .map(|t| t.to_string())
        .collect()
}

/// The command `pick` makes of its item, as it would be added.
#[uniffi::export]
pub fn library_command(pick: LibraryPick) -> String {
    let Some(i) = shellwing_core::library::items().get(pick.index as usize) else {
        return String::new();
    };
    i.command(&choice(&pick))
}

#[uniffi::export]
impl Shellwing {
    /// The library's items, by tool then task, with how many times each was
    /// added.
    pub fn library_items(&self) -> Vec<LibraryItem> {
        let mine: Vec<String> = self
            .vault
            .get()
            .and_then(|v| v.snippets().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.origin)
            .collect();
        shellwing_core::library::items()
            .iter()
            .enumerate()
            .map(|(n, i)| LibraryItem {
                index: n as u32,
                added: mine.iter().filter(|o| **o == i.origin()).count() as u32,
                id: i.id.clone(),
                tool: i.tool.clone(),
                task: i.task.clone(),
                name: i.name.clone(),
                about: i.about.clone(),
                options: i.options.clone(),
                password: i.password.clone(),
                changes_data: i.changes_data,
                sudo: i.sudo,
                can_contain: i.can_contain(),
                docs: i.docs.clone(),
            })
            .collect()
    }

    /// The environment item `index` is given for its password, unless told
    /// otherwise: the only one that has its variable.
    pub fn library_env(&self, index: u32) -> Option<String> {
        let var = shellwing_core::library::items()
            .get(index as usize)?
            .password
            .as_deref()?;
        let envs = self.vault.get()?.environments().ok()?;
        let mut having = envs.iter().filter(|e| e.names.iter().any(|v| v == var));
        match (having.next(), having.next()) {
            (Some(e), None) => Some(e.id.clone()),
            _ => None,
        }
    }

    /// Adds `picks` to the user's snippets, filed `under`. Returns how many.
    pub fn add_from_library(
        &self,
        picks: Vec<LibraryPick>,
        under: FileUnder,
    ) -> Result<u32, MobileError> {
        let vault = self.open_vault()?;
        let mut done = 0;
        for pick in picks {
            let Some(i) = shellwing_core::library::items().get(pick.index as usize) else {
                continue;
            };
            let env = match &pick.env {
                Some(e) => (!e.is_empty()).then(|| e.clone()),
                None => self.library_env(pick.index),
            };
            let s = Snippet {
                id: String::new(),
                name: match under {
                    FileUnder::Task => format!("{}: {}", i.tool, i.name),
                    _ => i.name.clone(),
                },
                command: i.command(&choice(&pick)),
                scope: String::new(),
                // What changes data is typed, and waits for Enter.
                enter: !i.changes_data,
                category: match under {
                    FileUnder::Tool => i.tool.clone(),
                    FileUnder::Task => i.task.clone(),
                    FileUnder::Both => format!("{} · {}", i.tool, i.task),
                },
                needs: env.into_iter().collect(),
                origin: i.origin(),
            };
            vault
                .save_snippet(&s)
                .map_err(|e| MobileError::Failed(environment::said_error(&e)))?;
            done += 1;
        }
        Ok(done)
    }
}

fn choice(pick: &LibraryPick) -> shellwing_core::library::Choice {
    shellwing_core::library::Choice {
        options: pick.options.iter().map(|o| *o as usize).collect(),
        container: pick.container,
        sudo: pick.sudo,
    }
}

/// The values `command` asks for when it runs, in order (not host, user,
/// address: those come from the session).
#[uniffi::export]
pub fn snippet_asked(command: String) -> Vec<String> {
    snippets::asked(&command)
}

/// The variable a line of `.env` text holds, as saving reads it: its name,
/// and its value (None: left to keep the one it has).
fn dotenv_line(line: &str) -> Option<(String, Option<String>)> {
    let mut vars = environment::parse_dotenv(line).ok()?;
    (vars.len() == 1).then(|| vars.remove(0))
}

/// `value` as a line of `.env` text gives it back: in quotes where trimming
/// the line, or taking quotes off, would change it.
fn dotenv_value(value: &str) -> String {
    if value.trim() != value || value.starts_with(['"', '\'']) {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// `lines` as text, ending as `text` did.
fn same_end(lines: Vec<String>, text: &str) -> String {
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_shown_reads_back_the_same() {
        for value in [
            "plain", "a=b #c", " lead", "trail\t", "  ", "\"both\"", "'single'", "\"open",
            "close\"", "'mixed\"",
        ] {
            let line = format!("N={}", dotenv_value(value));
            assert_eq!(
                dotenv_line(&line),
                Some(("N".into(), Some(value.into()))),
                "{line}"
            );
        }
        assert_eq!(dotenv_value("plain"), "plain", "quoted only when needed");
    }

    #[test]
    fn every_template_has_a_heading_a_task_and_a_command() {
        use crate::markdown::{MdKind, markdown_blocks};
        for md in [
            include_str!("note_templates/en.md"),
            include_str!("note_templates/vi.md"),
            include_str!("note_templates/de.md"),
            include_str!("note_templates/es.md"),
            include_str!("note_templates/fr.md"),
            include_str!("note_templates/ja.md"),
            include_str!("note_templates/ko.md"),
            include_str!("note_templates/pt.md"),
            include_str!("note_templates/ru.md"),
            include_str!("note_templates/zh.md"),
            include_str!("note_templates/zh-Hant.md"),
            include_str!("note_templates/id.md"),
            include_str!("note_templates/tr.md"),
        ] {
            let blocks = markdown_blocks(md.into());
            let head = md.lines().next().unwrap_or_default();
            assert!(
                matches!(blocks[0].kind, MdKind::Heading { level: 1, .. }),
                "{head}"
            );
            assert!(
                blocks.iter().any(|b| matches!(
                    b.kind,
                    MdKind::Item {
                        task: Some(false),
                        task_at: Some(_),
                        ..
                    }
                )),
                "{head}: a task to tick"
            );
            assert!(
                blocks.iter().any(|b| matches!(
                    &b.kind,
                    MdKind::Code { lang, text } if lang == "bash" && text.contains("systemctl restart")
                )),
                "{head}: a command to insert"
            );
        }
    }
}

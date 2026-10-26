//! What the vault keeps besides secrets, as the desktop's Library has it:
//! snippets (run in a session, their `{{holes}}` asked for), environments
//! (loaded into a session's shell, or given to one snippet) and notes.

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
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EnvRow {
    pub id: String,
    pub name: String,
    pub scope: String,
    /// Its variables' names; their values stay in the vault.
    pub names: Vec<String>,
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
            return Err(MobileError::Failed(
                "A name and a command are needed.".into(),
            ));
        }
        s.name = edit.name.trim().to_string();
        s.category = edit.category.trim().to_string();
        s.command = edit.command;
        s.enter = edit.enter;
        s.scope = edit.scope;
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
            .ok_or_else(|| MobileError::Failed("It is not in the vault any more.".into()))?;
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
            return Err(MobileError::Failed("A name is needed.".into()));
        }
        let vars = environment::parse_dotenv(&dotenv).map_err(MobileError::Failed)?;
        self.open_vault()?
            .save_environment(&id, name.trim(), &scope, &vars)
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))
    }

    /// Loads environment `id` into `session`'s shell.
    pub fn load_env(&self, session: Arc<MobileSession>, id: String) -> Result<(), MobileError> {
        let vault = self.open_vault()?;
        let env = vault
            .environments()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| MobileError::Failed("It is not in the vault any more.".into()))?;
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

    pub fn notes(&self) -> Vec<NoteRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
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
            })
            .collect();
        out.sort_by(|a, b| {
            (a.category.to_lowercase(), a.title.to_lowercase())
                .cmp(&(b.category.to_lowercase(), b.title.to_lowercase()))
        });
        out
    }

    pub fn note_text(&self, id: String) -> Result<String, MobileError> {
        Ok(self
            .open_vault()?
            .note_text(&id)
            .map_err(MobileError::of)?
            .map(|t| t.to_string())
            .unwrap_or_default())
    }

    /// Saves a note: a new one when `id` is empty. Its other fields (hosts,
    /// sensitive) stay as they were.
    pub fn save_note(
        &self,
        id: String,
        title: String,
        category: String,
        markdown: bool,
        text: String,
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
        vault
            .save_note(&n, Some(&text))
            .map_err(|e| MobileError::Failed(environment::said_error(&e)))
    }
}

impl Shellwing {
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

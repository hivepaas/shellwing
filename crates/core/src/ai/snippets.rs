//! What an AI does with the person's snippets (docs/design/ai-snippets.md):
//! where a grant has *Use snippets*, it lists those offered for the host,
//! as a pane of it is offered them, and runs one through [`Hub::run`]'s
//! path: the grant's mode and the risky list, on the command as filled in.
//!
//! The values an agent gives fill a snippet's `{{name}}`s only if they
//! cannot change what the command is: one word of a set that means the same
//! unquoted, in double quotes and in single quotes. A snippet typed without
//! Enter waits for the person every time. One that needs environments is
//! not run yet.

use std::collections::BTreeMap;
use std::sync::Arc;

use shellwing_config::AiClient;
use shellwing_vault::Snippet;

use super::notes::{VAULT_LOCKED, VaultNotes};
use super::{End, Hub, Job, paused_client};
use crate::snippets::{Scope, asked, fill};

/// Why a snippet typed without Enter waits for the person, under every
/// mode: its waiting reason, as AI activity reads it.
pub const LOOK: &str = "\u{0}look";
/// The longest value an agent gives, in characters.
pub const MOST_VALUE: usize = 256;

/// Where the snippets are: the vault, as the app has it.
pub trait SnippetStore: Send + Sync {
    fn snippets(&self) -> Result<Vec<Snippet>, String>;
}

impl SnippetStore for VaultNotes {
    fn snippets(&self) -> Result<Vec<Snippet>, String> {
        let vault = self.vault.get().ok_or(VAULT_LOCKED)?;
        vault.snippets().map_err(|e| e.to_string())
    }
}

/// Why `value`, for `{{name}}`, is not taken from an agent, if it is not.
pub fn value_refused(name: &str, value: &str) -> Option<String> {
    let safe = |c: char| c.is_alphanumeric() || "_./:@=+,%-".contains(c);
    if value.is_empty() {
        return Some(format!("{name} is empty"));
    }
    if value.chars().count() > MOST_VALUE {
        return Some(format!("{name} takes {MOST_VALUE} characters at the most"));
    }
    if value.starts_with('-') {
        return Some(format!(
            "{name} starts with -, which a program would take as an option"
        ));
    }
    if let Some(c) = value.chars().find(|c| !safe(*c)) {
        return Some(format!(
            "{name} has {c:?}: a value takes letters, digits and _ . / : @ = + , % - only, so that it stays one word and cannot change the command. Use run for this one, where the user sees the whole command."
        ));
    }
    None
}

/// Of `all`, those offered for a pane of the host `name` (in hosts.toml,
/// or as typed) in `group`, by category then name.
pub fn offered(all: Vec<Snippet>, name: &str, group: Option<&str>) -> Vec<Snippet> {
    let mut out: Vec<Snippet> = all
        .into_iter()
        .filter(|s| Scope::parse(&s.scope).fits(Some(name), group))
        .collect();
    out.sort_by_key(|s| (s.category.to_lowercase(), s.name.to_lowercase()));
    out
}

impl Hub {
    /// Where the snippets are read from; None: nowhere (no vault).
    pub fn set_snippets(&self, store: Option<Arc<dyn SnippetStore>>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.snippets = store;
        }
    }

    /// The snippets offered for `host` (as `client` names it), if it holds
    /// the host with *Use snippets*: the host's name too.
    fn offered_for(&self, client: &AiClient, host: &str) -> Result<(String, Vec<Snippet>), String> {
        if self.client_paused(&client.id) {
            return Err(paused_client(&client.name));
        }
        let g = self.given_for(client, host)?;
        if !g.grant.snippets {
            return Err(format!(
                "{} is not given to {} to use snippets: the user gives that in shellwing, under Settings › AI (Use snippets).",
                g.name, client.name
            ));
        }
        let (store, group) = {
            let inner = self.inner.lock().map_err(|_| "busy".to_string())?;
            if inner.locked {
                return Err(
                    "shellwing is locked: no snippet is listed or run until the user unlocks it."
                        .into(),
                );
            }
            let group = inner.hosts.get(&g.spec).and_then(|h| h.group.clone());
            (inner.snippets.clone().ok_or(VAULT_LOCKED)?, group)
        };
        Ok((
            g.name.clone(),
            offered(store.snippets()?, &g.name, group.as_deref()),
        ))
    }

    /// The snippets offered for `host`, as the model reads them.
    pub fn list_snippets(&self, client: &AiClient, host: &str) -> Result<String, String> {
        let (name, found) = self.offered_for(client, host)?;
        self.snippet_activity(client, &name, &format!("{} snippets", found.len()));
        if found.is_empty() {
            return Ok(format!("No snippet is offered for {name}."));
        }
        let mut out = format!(
            "{} {} offered for {name}, by category then name: id, name, category, the values it asks for, how it runs; then its command, indented. run_snippet runs one, host, user and address filled in from the host.\n",
            found.len(),
            if found.len() == 1 {
                "snippet"
            } else {
                "snippets"
            },
        );
        for s in &found {
            let values = asked(&s.command);
            let how = if !s.needs.is_empty() {
                "needs environments, which run_snippet does not load yet"
            } else if !s.enter {
                "typed without Enter: each run waits for the user"
            } else {
                "runs as a command"
            };
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{how}\n",
                s.id,
                s.name,
                if s.category.is_empty() {
                    "-"
                } else {
                    &s.category
                },
                if values.is_empty() {
                    "no values".to_string()
                } else {
                    format!("values: {}", values.join(", "))
                },
            ));
            for line in s.command.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        }
        Ok(out.trim_end().to_string())
    }

    /// Runs snippet `id` on `host`, its `{{name}}`s filled in from the host
    /// and `values`: a job, as [`Hub::run`] gives.
    pub fn run_snippet(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Arc<Job>, String> {
        let (name, found) = self.offered_for(client, host)?;
        let s = found.into_iter().find(|s| s.id == id).ok_or_else(|| {
            format!("No snippet {id} is offered for {name}: list_snippets says which.")
        })?;
        if !s.needs.is_empty() {
            return Err(format!(
                "{:?} needs environments, which run_snippet does not load yet: the user can run it from a pane.",
                s.name
            ));
        }
        let mut filled = BTreeMap::new();
        let mut missing = vec![];
        for v in asked(&s.command) {
            match values.get(&v) {
                Some(value) => {
                    if let Some(why) = value_refused(&v, value) {
                        return Err(why);
                    }
                    filled.insert(v, value.clone());
                }
                None => missing.push(v),
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "{:?} asks for {}: give them in values.",
                s.name,
                missing.join(", ")
            ));
        }
        let (_, target) = self.grant_for(client, host)?;
        filled.insert("host".into(), name.clone());
        filled.insert("user".into(), target.target.user.clone());
        filled.insert("address".into(), target.target.host.clone());
        let command = fill(&s.command, &filled);
        if command.len() > shellwing_vault::limits::COMMAND {
            return Err(format!(
                "{:?} filled in is {} long: a command takes {} at the most.",
                s.name,
                super::files::size(command.len() as u64),
                super::files::size(shellwing_vault::limits::COMMAND as u64)
            ));
        }
        let always = (!s.enter).then(|| LOOK.to_string());
        self.run_with(client, host, &command, always, Some(s.name.clone()))
    }

    /// Keeps a listing in the activity, done at once.
    fn snippet_activity(&self, client: &AiClient, host: &str, what: &str) {
        let job = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            let id = format!("j-{}", inner.next);
            inner.next += 1;
            Job::new(id, client, "", host, "list_snippets")
        };
        job.print(what.as_bytes());
        job.end(End::Exited(Some(0)));
        self.ended(&job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_safe_word_is_taken() {
        for ok in [
            "nginx",
            "app-1.log",
            "/var/log/syslog",
            "user@host:22",
            "a=b,c+d%",
        ] {
            assert_eq!(value_refused("v", ok), None, "{ok}");
        }
        for bad in [
            "a b", "$(id)", "`id`", "x;y", "x|y", "x&y", "'", "\"", "x\ny", "*", "~/x", "a>b", "#",
            "!", "{x}", "(x)", "a\\b",
        ] {
            assert!(value_refused("v", bad).is_some(), "{bad:?}");
        }
        assert!(
            value_refused("v", "-rf")
                .expect("an option")
                .contains("option")
        );
        assert!(value_refused("v", "").is_some());
        assert!(value_refused("v", &"a".repeat(MOST_VALUE + 1)).is_some());
    }

    fn snippet(id: &str, scope: &str, category: &str) -> Snippet {
        Snippet {
            id: id.into(),
            name: format!("name {id}"),
            command: "true".into(),
            scope: scope.into(),
            enter: true,
            category: category.into(),
            needs: vec![],
            origin: String::new(),
        }
    }

    struct Kept(Vec<Snippet>);

    impl SnippetStore for Kept {
        fn snippets(&self) -> Result<Vec<Snippet>, String> {
            Ok(self.0.clone())
        }
    }

    fn client() -> AiClient {
        AiClient {
            id: "c1".into(),
            kind: "stdio".into(),
            name: "Some agent".into(),
            token_sha256: String::new(),
            added_at: 0,
            paused: false,
        }
    }

    /// A hub giving `web` (group Prod, at 127.0.0.1:1, where nothing
    /// listens) with Use snippets in `mode`, and `db` without.
    fn hub(mode: shellwing_config::AiMode) -> Arc<Hub> {
        struct None_;
        impl shellwing_ssh::Secrets for None_ {
            fn password(&self, _: &str) -> Option<String> {
                None
            }
            fn passphrase(&self, _: &str) -> Option<String> {
                None
            }
            fn learned(&self, _: shellwing_ssh::Learned) {}
        }
        let (tx, _rx) = async_channel::unbounded();
        let hub = Hub::new(
            tokio::runtime::Handle::current(),
            Arc::new(None_),
            &shellwing_config::Ssh::default(),
            None,
            tx,
        );
        let grant = |spec: &str, snippets: bool| shellwing_config::AiGrant {
            client: "c1".into(),
            host: spec.into(),
            how: shellwing_config::AiHow::Background,
            mode,
            until: None,
            session: false,
            run: true,
            read: true,
            write: false,
            notes_read: false,
            notes_add: false,
            snippets,
            paused: false,
        };
        hub.set_state(shellwing_config::AiState {
            clients: vec![client()],
            grants: vec![grant("web", true), grant("db", false)],
            ..Default::default()
        });
        let target = shellwing_ssh::config::resolve_without_commands("deploy@127.0.0.1:1", None)
            .map_err(|e| e.to_string());
        hub.set_hosts(
            [("web", Some("Prod")), ("db", None)]
                .into_iter()
                .map(|(name, group)| {
                    (
                        name.to_string(),
                        super::super::Host {
                            name: name.into(),
                            detail: String::new(),
                            group: group.map(str::to_string),
                            target: target.clone(),
                        },
                    )
                })
                .collect(),
        );
        let mut tail = snippet("tail", "group:Prod", "Logs");
        tail.command = "tail -n {{lines}} /var/log/{{file}} # on {{host}} as {{user}}".into();
        let mut look = snippet("look", "host:web", "Ops");
        look.enter = false;
        look.command = "sudo systemctl restart app".into();
        let mut envs = snippet("envs", "", "Ops");
        envs.needs = vec!["env1".into()];
        hub.set_snippets(Some(Arc::new(Kept(vec![
            tail,
            look,
            envs,
            snippet("other", "host:db", "Ops"),
        ]))));
        hub
    }

    #[tokio::test]
    async fn an_agent_lists_the_snippets_of_its_host() {
        let hub = hub(shellwing_config::AiMode::Free);
        let c = client();
        let listed = hub.list_snippets(&c, "web").expect("listed");
        assert!(listed.starts_with("3 snippets offered for web"), "{listed}");
        assert!(listed.contains("tail\tname tail\tLogs\tvalues: lines, file\truns as a command\n    tail -n {{lines}}"), "{listed}");
        assert!(
            listed.contains("look\tname look\tOps\tno values\ttyped without Enter"),
            "{listed}"
        );
        assert!(
            listed.contains("envs\tname envs\tOps\tno values\tneeds environments"),
            "{listed}"
        );
        assert!(!listed.contains("name other"), "db's own: {listed}");
        let e = hub.list_snippets(&c, "db").expect_err("db");
        assert!(e.contains("not given to Some agent to use snippets"), "{e}");
        hub.set_locked(true);
        assert!(
            hub.list_snippets(&c, "web")
                .expect_err("locked")
                .contains("locked")
        );
    }

    #[tokio::test]
    async fn a_run_is_filled_in_and_goes_through_run() {
        let hub = hub(shellwing_config::AiMode::Free);
        let c = client();
        let values = |v: &[(&str, &str)]| -> BTreeMap<String, String> {
            v.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let e = hub
            .run_snippet(&c, "web", "tail", &values(&[("lines", "50")]))
            .err()
            .expect("missing");
        assert!(e.contains("asks for file"), "{e}");
        let e = hub
            .run_snippet(
                &c,
                "web",
                "tail",
                &values(&[("lines", "50"), ("file", "syslog; id")]),
            )
            .err()
            .expect("unsafe");
        assert!(e.contains("file has ';'"), "{e}");
        let e = hub
            .run_snippet(&c, "web", "envs", &values(&[]))
            .err()
            .expect("envs");
        assert!(e.contains("needs environments"), "{e}");
        let e = hub
            .run_snippet(&c, "web", "other", &values(&[]))
            .err()
            .expect("not offered");
        assert!(e.starts_with("No snippet other is offered for web"), "{e}");

        let job = hub
            .run_snippet(
                &c,
                "web",
                "tail",
                &values(&[("lines", "50"), ("file", "syslog")]),
            )
            .expect("run");
        let a = job.activity();
        assert_eq!(a.command, "tail -n 50 /var/log/syslog # on web as deploy");
        assert_eq!(a.snippet.as_deref(), Some("name tail"));
        assert_eq!(a.waiting, None, "runs freely");
        // Nothing listens there: it fails to connect, as run would.
        let end = job.wait(std::time::Duration::from_secs(10), true).await.end;
        assert!(matches!(end, Some(End::Failed(_))), "{end:?}");
    }

    #[tokio::test]
    async fn one_typed_without_enter_always_waits() {
        let hub = hub(shellwing_config::AiMode::Free);
        let c = client();
        let job = hub
            .run_snippet(&c, "web", "look", &BTreeMap::new())
            .expect("asked");
        let said = job.wait(std::time::Duration::from_millis(50), true).await;
        assert!(
            said.waiting && said.end.is_none(),
            "waits, even running freely"
        );
        assert_eq!(job.activity().waiting.as_deref(), Some(LOOK));
        hub.answer(&job.id, super::super::Approval::Refuse);
        let end = job.wait(std::time::Duration::from_secs(5), true).await.end;
        assert_eq!(end, Some(End::Refused));
    }

    #[test]
    fn those_a_pane_of_the_host_is_offered() {
        let all = vec![
            snippet("every", "", "b"),
            snippet("group", "group:Prod", "a"),
            snippet("host", "host:web", "a"),
            snippet("other", "host:db", "a"),
            snippet("othergroup", "group:Dev", "a"),
        ];
        let ids: Vec<String> = offered(all, "web", Some("Prod"))
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["group", "host", "every"], "by category, then name");
    }
}

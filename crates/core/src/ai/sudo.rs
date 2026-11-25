//! sudo's password for an agent's command on a connection of its own
//! (docs/design/ai-sudo.md): asked of the person each time, typed or the
//! host's saved one, and given to the command as Run on hosts gives it (on
//! its input, read by sudo through `printenv` as its askpass). The agent
//! never sees it. Kept a while for that client on that host when the person
//! says so, never for good.

use std::sync::Arc;
use std::time::{Duration, Instant};

use shellwing_config::{AiClient, AiHow};
use zeroize::Zeroizing;

use super::{Approval, Hub, Job};

/// Why a command waits for sudo's password, under every mode: its waiting
/// reason, as AI activity reads it. [`SAVED`] after it: the vault keeps a
/// password for the host, to offer.
pub const SUDO: &str = "\u{0}sudo";
pub const SAVED: &str = ":saved";

/// What the agent is told when sudo wanted a password it was not given.
pub const HINT: &str = "sudo needs a password on this host: run the command again with sudo: true, and the user is asked for it (you never see it).";

/// Whether `output` says sudo wanted a password, or a terminal to ask on.
pub fn wanted_password(output: &str) -> bool {
    output.contains("sudo: a password is required")
        || output.contains("sudo: a terminal is required")
        || output.contains("you must have a tty to run sudo")
}

/// Whether `output` says sudo turned the password down.
pub fn refused(output: &str) -> bool {
    output.contains("incorrect password attempt")
}

/// sudo's password for a run: one kept for the host, or the person's answer.
pub(crate) enum SudoRun {
    Kept(Zeroizing<String>),
    Asked,
}

/// A password kept for a client on a host, and until when.
pub(crate) struct Kept {
    pub password: Zeroizing<String>,
    pub until: Instant,
}

impl Hub {
    /// Runs `command` on `host` with sudo's password: through a tab, as any
    /// command (sudo asks there, of the person); else with the one kept for
    /// the host, or waiting for the person to give it.
    pub fn run_sudo(
        self: &Arc<Self>,
        client: &AiClient,
        host: &str,
        command: &str,
    ) -> Result<Arc<Job>, String> {
        let g = self.given_for(client, host)?;
        if g.grant.how == AiHow::Tab {
            return self.run(client, host, command);
        }
        let kept = self.kept_sudo(&client.id, &g.spec);
        let (always, sudo) = match kept {
            Some(password) => (None, SudoRun::Kept(password)),
            None => {
                let saved = self.saved_password(&g.spec).is_some();
                let why = format!("{SUDO}{}", if saved { SAVED } else { "" });
                (Some(why), SudoRun::Asked)
            }
        };
        self.run_with(
            client,
            host,
            command,
            super::RunWith {
                always,
                sudo: Some(sudo),
                ..Default::default()
            },
        )
    }

    /// The person's answer to `id`, which waits for sudo's password: `typed`,
    /// or the host's saved one when None; kept for `remember` (the client on
    /// that host) as long as the person allows.
    pub fn answer_sudo(&self, id: &str, typed: Option<Zeroizing<String>>, remember: bool) {
        let Some(job) = self.inner.lock().ok().and_then(|i| i.jobs.get(id).cloned()) else {
            return;
        };
        let Some(password) = typed.or_else(|| self.saved_password(&job.host)) else {
            return;
        };
        let minutes = self
            .inner
            .lock()
            .map(|i| i.state.sudo_minutes())
            .unwrap_or(0);
        if remember && minutes > 0 {
            let until = Instant::now() + Duration::from_secs(u64::from(minutes) * 60);
            if let Ok(mut inner) = self.inner.lock() {
                inner.sudo.insert(
                    (job.client.clone(), job.host.clone()),
                    Kept {
                        password: password.clone(),
                        until,
                    },
                );
            }
        }
        if let Ok(mut given) = job.sudo.lock() {
            *given = Some(password);
        }
        self.answer(id, Approval::Once);
    }

    /// Every password kept for sudo, forgotten: shellwing locks or quits.
    pub fn forget_sudo(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.sudo.clear();
        }
    }

    /// The password kept for `client` on `spec`, if it still is.
    fn kept_sudo(&self, client: &str, spec: &str) -> Option<Zeroizing<String>> {
        let mut inner = self.inner.lock().ok()?;
        let now = Instant::now();
        inner.sudo.retain(|_, k| k.until > now);
        inner
            .sudo
            .get(&(client.to_string(), spec.to_string()))
            .map(|k| k.password.clone())
    }

    /// The password sudo turned down: no longer kept for `client` on `spec`.
    pub(crate) fn sudo_refused(&self, client: &str, spec: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.sudo.remove(&(client.to_string(), spec.to_string()));
        }
    }

    /// The password the vault keeps for the host `spec` (the one shellwing
    /// signs in with), if any.
    fn saved_password(&self, spec: &str) -> Option<Zeroizing<String>> {
        let target = {
            let inner = self.inner.lock().ok()?;
            inner.hosts.get(spec)?.target.clone().ok()?
        };
        self.secrets
            .password(&target.target.to_string())
            .map(Zeroizing::new)
    }

    /// Keeps only what grants in force still reach: a grant paused or taken
    /// back, or its client paused, forgets the password kept for it.
    pub(crate) fn keep_sudo_for_grants(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            let live: Vec<(String, String)> = inner
                .state
                .grants
                .iter()
                .filter(|g| super::live(g) && !g.paused)
                .filter(|g| {
                    !inner
                        .state
                        .clients
                        .iter()
                        .any(|c| c.id == g.client && c.paused)
                })
                .map(|g| (g.client.clone(), g.host.clone()))
                .collect();
            inner.sudo.retain(|k, _| live.contains(k));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::End;
    use shellwing_config::{AiGrant, AiMode, AiState};

    /// The vault: a password for the host, or none.
    struct Saved(Option<String>);

    impl shellwing_ssh::Secrets for Saved {
        fn password(&self, target: &str) -> Option<String> {
            (target == "deploy@127.0.0.1:1")
                .then(|| self.0.clone())
                .flatten()
        }
        fn passphrase(&self, _: &str) -> Option<String> {
            None
        }
        fn learned(&self, _: shellwing_ssh::Learned) {}
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

    fn grant(how: AiHow, paused: bool) -> AiGrant {
        AiGrant {
            client: "c1".into(),
            host: "web".into(),
            how,
            mode: AiMode::Free,
            until: None,
            session: false,
            run: true,
            read: true,
            write: false,
            notes_read: false,
            notes_add: false,
            snippets: false,
            paused,
        }
    }

    /// A hub giving `web` (at 127.0.0.1:1, where nothing listens), freely,
    /// `how` it is reached; the vault keeping `saved` for it.
    fn hub(how: AiHow, saved: Option<&str>, minutes: Option<u32>) -> Arc<Hub> {
        let (tx, _rx) = async_channel::unbounded();
        let hub = Hub::new(
            tokio::runtime::Handle::current(),
            Arc::new(Saved(saved.map(str::to_string))),
            &shellwing_config::Ssh::default(),
            None,
            tx,
        );
        hub.set_state(AiState {
            clients: vec![client()],
            grants: vec![grant(how, false)],
            sudo_minutes: minutes,
            ..Default::default()
        });
        let target = shellwing_ssh::config::resolve_without_commands("deploy@127.0.0.1:1", None)
            .map_err(|e| e.to_string());
        hub.set_hosts(
            [(
                "web".to_string(),
                super::super::Host {
                    name: "web".into(),
                    detail: String::new(),
                    group: None,
                    target,
                },
            )]
            .into_iter()
            .collect(),
        );
        hub
    }

    async fn waits(job: &Job) -> bool {
        job.wait(Duration::from_millis(50), true).await.waiting
    }

    async fn ended(job: &Job) -> Option<End> {
        job.wait(Duration::from_secs(10), true).await.end
    }

    /// Asked for, sudo's password is waited for whatever the mode; given and
    /// kept, the next sudo run on the host goes as any command does.
    #[tokio::test]
    async fn a_sudo_run_waits_for_the_password_and_may_keep_it() {
        let hub = hub(AiHow::Background, None, None);
        let c = client();
        let job = hub
            .run_sudo(&c, "web", "sudo systemctl restart app")
            .expect("asked");
        assert!(waits(&job).await, "waits though the grant runs freely");
        assert_eq!(
            job.activity().waiting.as_deref(),
            Some(SUDO),
            "no saved one to offer"
        );
        hub.answer_sudo(&job.id, Some(Zeroizing::new("pa ss".into())), true);
        assert!(
            matches!(ended(&job).await, Some(End::Failed(_))),
            "ran: nothing listens there"
        );
        assert!(
            !format!("{:?}", job.activity()).contains("pa ss"),
            "the password is nowhere kept in it"
        );

        let job = hub
            .run_sudo(&c, "web", "sudo systemctl status app")
            .expect("ran");
        assert!(!waits(&job).await, "kept: it goes as any command");
        ended(&job).await;

        hub.set_locked(true);
        hub.set_locked(false);
        let job = hub.run_sudo(&c, "web", "sudo true").expect("asked");
        assert!(waits(&job).await, "locked: forgotten");
        hub.answer(&job.id, Approval::Refuse);
        assert_eq!(ended(&job).await, Some(End::Refused));
    }

    #[tokio::test]
    async fn kept_no_longer_than_allowed() {
        // Not remembered: asked again.
        let hub = hub(AiHow::Background, None, None);
        let c = client();
        let job = hub.run_sudo(&c, "web", "sudo true").expect("asked");
        hub.answer_sudo(&job.id, Some(Zeroizing::new("pw".into())), false);
        ended(&job).await;
        assert!(waits(&hub.run_sudo(&c, "web", "sudo true").expect("asked")).await);

        // 0 minutes: never kept, even asked to.
        let hub = self::hub(AiHow::Background, None, Some(0));
        let job = hub.run_sudo(&c, "web", "sudo true").expect("asked");
        hub.answer_sudo(&job.id, Some(Zeroizing::new("pw".into())), true);
        ended(&job).await;
        assert!(waits(&hub.run_sudo(&c, "web", "sudo true").expect("asked")).await);

        // The grant paused: forgotten.
        let hub = self::hub(AiHow::Background, None, None);
        let job = hub.run_sudo(&c, "web", "sudo true").expect("asked");
        hub.answer_sudo(&job.id, Some(Zeroizing::new("pw".into())), true);
        ended(&job).await;
        let mut state = hub.inner.lock().expect("lock").state.clone();
        state.grants = vec![grant(AiHow::Background, true)];
        hub.set_state(state.clone());
        state.grants = vec![grant(AiHow::Background, false)];
        hub.set_state(state);
        assert!(waits(&hub.run_sudo(&c, "web", "sudo true").expect("asked")).await);
    }

    #[tokio::test]
    async fn the_saved_password_is_offered_and_a_tab_asks_there() {
        let hub = hub(AiHow::Background, Some("saved pw"), None);
        let c = client();
        let job = hub.run_sudo(&c, "web", "sudo true").expect("asked");
        assert_eq!(job.activity().waiting.as_deref(), Some("\u{0}sudo:saved"));

        // Through a tab, sudo asks the person there: a command as any.
        let hub = self::hub(AiHow::Tab, None, None);
        let job = hub.run_sudo(&c, "web", "sudo true").expect("typed");
        assert!(job.tab && job.activity().waiting.is_none());
    }

    #[test]
    fn what_sudo_says() {
        assert!(wanted_password("sudo: a password is required\n"));
        assert!(wanted_password(
            "sudo: a terminal is required to read the password"
        ));
        assert!(!wanted_password("permission denied"));
        assert!(refused("sudo: 3 incorrect password attempts"));
    }
}

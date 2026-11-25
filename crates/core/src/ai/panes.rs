//! Panes an AI client may read (docs/design/ai-read-tab.md): each allowed by
//! the person for one client, for [`MINUTES`], and no longer. shellwing
//! locked, the client paused or removed, or the pane closed ends it sooner.
//! What a pane is, the app says: an id of its own.

use std::time::{Duration, Instant};

use super::Hub;

/// How long a pane stays readable once allowed.
pub const MINUTES: u64 = 15;

impl Hub {
    /// Lets `client` read `pane` for [`MINUTES`] from now: until when.
    pub fn allow_pane(&self, client: &str, pane: &str) -> Instant {
        let until = Instant::now() + Duration::from_secs(MINUTES * 60);
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .panes
                .insert((client.to_string(), pane.to_string()), until);
        }
        until
    }

    /// No client reads `pane` any more: the person said so, or it closed.
    pub fn stop_pane(&self, pane: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.panes.retain(|(_, p), _| p != pane);
        }
    }

    /// Who may read which pane now, and until when: client's id, pane.
    pub fn pane_readers(&self) -> Vec<(String, String, Instant)> {
        let Ok(mut inner) = self.inner.lock() else {
            return vec![];
        };
        let now = Instant::now();
        inner.panes.retain(|_, until| *until > now);
        inner
            .panes
            .iter()
            .map(|((c, p), until)| (c.clone(), p.clone(), *until))
            .collect()
    }

    /// Whether `client` may read `pane` now.
    pub fn may_read_pane(&self, client: &str, pane: &str) -> bool {
        self.inner.lock().is_ok_and(|i| {
            i.panes
                .get(&(client.to_string(), pane.to_string()))
                .is_some_and(|until| *until > Instant::now())
        })
    }

    /// Only clients still let in, and not paused, keep what they may read.
    pub(crate) fn keep_panes_for_clients(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            let live: Vec<String> = inner
                .state
                .clients
                .iter()
                .filter(|c| !c.paused)
                .map(|c| c.id.clone())
                .collect();
            inner.panes.retain(|(c, _), _| live.contains(c));
        }
    }

    /// shellwing locked: no pane is read any more.
    pub(crate) fn forget_panes(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.panes.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_config::{AiClient, AiState};
    use std::sync::Arc;

    fn client(id: &str, paused: bool) -> AiClient {
        AiClient {
            id: id.into(),
            kind: "stdio".into(),
            name: id.into(),
            token_sha256: String::new(),
            added_at: 0,
            paused,
            folders: vec![],
        }
    }

    fn hub() -> Arc<Hub> {
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
        hub.set_state(AiState {
            clients: vec![client("c1", false), client("c2", false)],
            ..Default::default()
        });
        hub
    }

    #[tokio::test]
    async fn a_pane_is_read_by_the_client_allowed_for_a_while() {
        let hub = hub();
        let until = hub.allow_pane("c1", "p7");
        let left = until.duration_since(Instant::now());
        assert!(
            left > Duration::from_secs(MINUTES * 60 - 5)
                && left <= Duration::from_secs(MINUTES * 60)
        );
        assert!(hub.may_read_pane("c1", "p7"));
        assert!(!hub.may_read_pane("c2", "p7"), "another client: not");
        assert!(!hub.may_read_pane("c1", "p8"), "another pane: not");
        assert_eq!(hub.pane_readers().len(), 1);

        hub.stop_pane("p7");
        assert!(!hub.may_read_pane("c1", "p7"), "stopped");
    }

    #[tokio::test]
    async fn locked_paused_or_removed_ends_it() {
        let hub = hub();
        hub.allow_pane("c1", "p1");
        hub.allow_pane("c2", "p2");
        hub.set_locked(true);
        assert!(hub.pane_readers().is_empty(), "locked: none");
        hub.set_locked(false);

        hub.allow_pane("c1", "p1");
        hub.allow_pane("c2", "p2");
        hub.set_state(AiState {
            clients: vec![client("c1", true)],
            ..Default::default()
        });
        assert!(hub.pane_readers().is_empty(), "c1 paused, c2 removed");
    }

    #[tokio::test]
    async fn it_ends_by_itself() {
        let hub = hub();
        hub.allow_pane("c1", "p1");
        if let Ok(mut inner) = hub.inner.lock() {
            for until in inner.panes.values_mut() {
                *until = Instant::now() - Duration::from_secs(1);
            }
        }
        assert!(!hub.may_read_pane("c1", "p1"));
        assert!(hub.pane_readers().is_empty(), "gone once past");
    }
}

//! The system a host runs, for its tile: asked once the host is connected,
//! at most once a day, only while its icon is left to it; what was seen is
//! kept in `state/systems.json`, on this computer only.

use super::*;
use crate::host_icons::{self, HostIcon};
use shellwing_core::systems;

impl Workspace {
    /// The icon a host's tile shows: the one it names, else the system it
    /// was seen running, opened by `spec`.
    pub(super) fn host_icon(&self, named: Option<&str>, spec: &str) -> Option<&'static HostIcon> {
        match named {
            Some(id) => host_icons::find(id),
            None => self
                .systems
                .get(spec)
                .and_then(|s| s.system.as_deref())
                .and_then(host_icons::find),
        }
    }

    /// Asks the newly connected pane's host what it runs, unless its icon
    /// is chosen, it was asked less than a day ago, or another of its tabs
    /// is asking now. What it answers, or that it would not, is kept with
    /// the time; a connection that ended first is asked the next time.
    pub(super) fn look_for_system(&mut self, view: &Entity<TermView>, cx: &mut Context<Self>) {
        let session = view.read(cx).session();
        if session.is_local() {
            return;
        }
        let spec = session.spec.clone();
        let chosen = session
            .entry
            .as_deref()
            .and_then(|name| self.live.hosts().find(name))
            .is_some_and(|h| h.icon.is_some());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        if chosen
            || !systems::due(self.systems.get(&spec), now)
            || self.asking_system.contains(&spec)
        {
            return;
        }
        let remote = session.remote();
        let Some(answer) = remote.exec(systems::COMMAND, vec![]) else {
            return;
        };
        self.asking_system.insert(spec.clone());
        cx.spawn(async move |ws, cx| {
            // None: the connection ended before it answered, so it is asked
            // the next time.
            let mut system = match answer.await {
                Ok(Ok(out)) => Some(systems::parse(&String::from_utf8_lossy(&out.stdout))),
                // The host would not run it: as good as an answer.
                Ok(Err(_)) => Some(None),
                Err(_) => None,
            };
            // Nothing a POSIX shell would say: maybe Windows.
            if system == Some(None)
                && let Some(answer) = remote.exec(systems::WINDOWS_COMMAND, vec![])
                && let Ok(Ok(out)) = answer.await
            {
                system = Some(systems::parse_windows(&String::from_utf8_lossy(
                    &out.stdout,
                )));
            }
            let _ = ws.update(cx, |ws, cx| {
                ws.asking_system.remove(&spec);
                let Some(system) = system else {
                    return;
                };
                ws.systems.insert(
                    spec,
                    SystemSeen {
                        system: system.map(str::to_string),
                        checked_at: now,
                    },
                );
                if let Some(store) = ws.live.store() {
                    let _ = store.save_systems(&ws.systems);
                }
                ws.refresh_sidebar(cx);
            });
        })
        .detach();
    }
}

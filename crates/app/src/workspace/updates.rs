//! Telling that a newer shellwing is out (see `shellwing_update`): a look
//! once a day while `[updates] check` is on, or when asked in Settings ›
//! About. What is offered shows as a dot on the Settings button, an entry of
//! the palette and a line in About; it never interrupts.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use shellwing_config::UpdateState;
use shellwing_update::{Install, Version};

/// How often a look is due, and how often whether one is due is asked.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const WAKE_EVERY: Duration = Duration::from_secs(60 * 60);
/// The first look waits for the app to settle.
const FIRST_AFTER: Duration = Duration::from_secs(20);

/// How looking for updates stands.
#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) state: UpdateState,
    pub(crate) checking: bool,
    /// What went wrong the last time, if it did.
    pub(crate) problem: Option<String>,
}

/// What Settings › About shows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateSnap {
    /// The build has release keys: it can tell of updates.
    pub keys: bool,
    pub current: String,
    pub channel: &'static str,
    pub check: bool,
    pub checking: bool,
    pub checked_at: Option<SystemTime>,
    pub offered: Option<String>,
    pub problem: Option<String>,
    /// What updates this copy, when something else installed it.
    pub managed: Option<(&'static str, Option<&'static str>)>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What updates this copy, in the language in use: names stay as they are,
/// the others are said.
pub(crate) fn by_said(by: &str) -> String {
    match by {
        "the system's package manager" => tr!("the system's package manager"),
        "the App Store" => tr!("the App Store"),
        "the Microsoft Store" => tr!("the Microsoft Store"),
        name => name.to_string(),
    }
}

fn install() -> Install {
    let exe = std::env::current_exe().unwrap_or_default();
    shellwing_update::install_of(&exe, |k| std::env::var(k).ok())
}

impl Workspace {
    /// What was found before, and the looks to come.
    pub(super) fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        if let Some(store) = self.live.store() {
            self.updates.state = store.update_state();
        }
        // A build without release keys could believe no list: it does not look.
        if !shellwing_update::has_keys() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_AFTER).await;
            loop {
                let go_on = this.update(cx, |ws, cx| {
                    let due = ws
                        .updates
                        .state
                        .checked_at
                        .is_none_or(|t| now_secs().saturating_sub(t) >= CHECK_EVERY.as_secs());
                    if ws.live.settings().updates.check && due {
                        ws.check_updates(cx);
                    }
                });
                if go_on.is_err() {
                    break;
                }
                cx.background_executor().timer(WAKE_EVERY).await;
            }
        })
        .detach();
    }

    /// Reads the signed list now, and says what it offers.
    pub(crate) fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.updates.checking || !shellwing_update::has_keys() {
            return;
        }
        self.updates.checking = true;
        self.refresh_settings(cx);
        let job = self
            .rt
            .spawn(shellwing_update::fetch(shellwing_update::LIST_URL));
        cx.spawn(async move |this, cx| {
            let found = match job.await {
                Ok(Ok(list)) => shellwing_update::offer(&list, &shellwing_update::current())
                    .map_err(|e| e.to_string()),
                // Before the first release: up to date.
                Ok(Err(shellwing_update::CheckError::NoList)) => Ok(None),
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = this.update(cx, |ws, cx| {
                ws.updates.checking = false;
                match found {
                    Ok(offered) => {
                        ws.updates.problem = None;
                        ws.updates.state.checked_at = Some(now_secs());
                        ws.updates.state.offered = offered.map(|v| v.to_string());
                    }
                    Err(e) => ws.updates.problem = Some(e),
                }
                if let Some(store) = ws.live.store()
                    && let Err(e) = store.save_update_state(&ws.updates.state)
                {
                    eprintln!("cannot keep what the update check found: {e}");
                }
                ws.refresh_settings(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The release offered, unless skipped or already running.
    pub(crate) fn update_offered(&self) -> Option<Version> {
        let s = &self.updates.state;
        let v = Version::parse(s.offered.as_deref()?)?;
        (v > shellwing_update::current() && s.skipped.as_deref() != s.offered.as_deref())
            .then_some(v)
    }

    /// Opens the offered release's page, or says what updates this copy.
    pub(crate) fn get_update(&mut self, cx: &mut Context<Self>) {
        let Some(v) = self.update_offered() else {
            return;
        };
        match install() {
            Install::Direct => cx.open_url(&shellwing_update::release_page(&v)),
            Install::Managed { by, command } => {
                let msg = match command {
                    Some(c) => tr!(
                        "shellwing {version} is out: {by} updates it ({command}).",
                        version = v.to_string(),
                        by = by_said(by),
                        command = c
                    ),
                    None => tr!(
                        "shellwing {version} is out: {by} updates it.",
                        version = v.to_string(),
                        by = by_said(by)
                    ),
                };
                self.say(msg, CONFIG_NOTICE, cx);
            }
        }
    }

    /// Not offered again until a newer one comes.
    pub(crate) fn skip_update(&mut self, cx: &mut Context<Self>) {
        self.updates.state.skipped = self.updates.state.offered.clone();
        if let Some(store) = self.live.store()
            && let Err(e) = store.save_update_state(&self.updates.state)
        {
            eprintln!("cannot keep the skipped release: {e}");
        }
        self.refresh_settings(cx);
        cx.notify();
    }

    pub(super) fn update_snap(&self) -> UpdateSnap {
        let current = shellwing_update::current();
        let s = &self.updates.state;
        UpdateSnap {
            keys: shellwing_update::has_keys(),
            channel: current.channel().name(),
            current: current.to_string(),
            check: self.live.settings().updates.check,
            checking: self.updates.checking,
            checked_at: s.checked_at.map(|t| UNIX_EPOCH + Duration::from_secs(t)),
            offered: self.update_offered().map(|v| v.to_string()),
            problem: self.updates.problem.clone(),
            managed: match install() {
                Install::Direct => None,
                Install::Managed { by, command } => Some((by, command)),
            },
        }
    }
}

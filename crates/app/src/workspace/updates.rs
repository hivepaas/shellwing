//! Telling that a newer shellwing is out, and installing it (see
//! `shellwing_update`): a look once a day while `[updates] check` is on, or
//! when asked in Settings › About. What is offered shows as a dot on the
//! Settings button, an entry of the palette and a line in About; it never
//! interrupts.
//!
//! A copy the app can replace downloads the release when told to, checks it
//! against the signed list, and installs it as it quits: at *Restart now*
//! (asked first while connections are open), or whenever the user quits.
//! Any other copy opens the release's page, or says what updates it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use shellwing_config::UpdateState;
use shellwing_update::{Install, Version};

/// How often a look is due, and how often whether one is due is asked.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const WAKE_EVERY: Duration = Duration::from_secs(60 * 60);
/// The first look waits for the app to settle.
const FIRST_AFTER: Duration = Duration::from_secs(20);
/// How often a download's progress is shown.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Where a release's file is downloaded.
fn staging_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("shellwing").join("updates"))
}

/// Getting a release in.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Stage {
    #[default]
    Idle,
    /// Bytes so far, and of how many when known.
    Downloading(u64, Option<u64>),
    /// Downloaded and checked: installed as the app quits.
    Ready(Version),
    Failed(String),
}

/// How looking for updates stands.
#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) state: UpdateState,
    pub(crate) checking: bool,
    /// What went wrong the last time, if it did.
    pub(crate) problem: Option<String>,
    pub(crate) stage: Stage,
    /// Quitting to install the update starts the app again.
    pub(crate) relaunch: bool,
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
    /// The app can download a release and install it itself.
    pub updates_itself: bool,
    pub stage: Stage,
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

/// "12.3 MB".
fn megabytes(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_000_000.)
}

impl Workspace {
    /// What was found before, the looks to come, and the update to install
    /// as the app quits.
    pub(super) fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        if let Some(store) = self.live.store() {
            self.updates.state = store.update_state();
        }
        self.restore_staged();
        cx.on_app_quit(|ws, _| {
            ws.install_staged();
            async {}
        })
        .detach();
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

    fn save_update_state(&self) {
        if let Some(store) = self.live.store()
            && let Err(e) = store.save_update_state(&self.updates.state)
        {
            eprintln!("cannot keep the update's state: {e}");
        }
    }

    /// A release downloaded by an earlier run: ready still, unless what runs
    /// is it (or newer), whose file then goes.
    fn restore_staged(&mut self) {
        let Some((version, file)) = self.updates.state.staged.clone() else {
            return;
        };
        let path = PathBuf::from(&file);
        match Version::parse(&version) {
            Some(v) if v > shellwing_update::current() && path.exists() => {
                self.updates.stage = Stage::Ready(v);
            }
            _ => {
                let _ = std::fs::remove_file(&path);
                self.updates.state.staged = None;
                self.save_update_state();
            }
        }
    }

    /// Reads the signed list now, and says what it offers.
    pub(crate) fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.updates.checking || !shellwing_update::has_keys() {
            return;
        }
        self.updates.checking = true;
        self.refresh_settings(cx);
        let url = shellwing_update::list_url();
        let job = self
            .rt
            .spawn(async move { shellwing_update::fetch(&url).await });
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
                ws.save_update_state();
                ws.refresh_settings(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Why the last look for updates failed, if it did: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn update_problem(&self) -> Option<String> {
        self.updates.problem.clone()
    }

    /// A look for updates is under way: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn update_checking(&self) -> bool {
        self.updates.checking
    }

    /// The release offered, unless skipped or already running.
    pub(crate) fn update_offered(&self) -> Option<Version> {
        let s = &self.updates.state;
        let v = Version::parse(s.offered.as_deref()?)?;
        (v > shellwing_update::current() && s.skipped.as_deref() != s.offered.as_deref())
            .then_some(v)
    }

    /// The offered release: downloaded to install, when this copy can take
    /// it; else its page, or what updates this copy.
    pub(crate) fn get_update(&mut self, cx: &mut Context<Self>) {
        let Some(v) = self.update_offered() else {
            return;
        };
        match install() {
            i if i.updates_itself() => self.download_update(v, cx),
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
            _ => cx.open_url(&shellwing_update::release_page(&v)),
        }
    }

    /// Downloads `v`'s file for this platform, as the signed list (read
    /// again, and checked again) names it.
    fn download_update(&mut self, v: Version, cx: &mut Context<Self>) {
        if matches!(self.updates.stage, Stage::Downloading(..) | Stage::Ready(_)) {
            return;
        }
        let Some(dir) = staging_dir() else {
            self.updates.stage = Stage::Failed(tr!("No folder to download to."));
            return self.refresh_settings(cx);
        };
        let (got, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let (job_got, job_total, wanted) = (got.clone(), total.clone(), v.clone());
        let job = self.rt.spawn(async move {
            let list = shellwing_update::fetch(&shellwing_update::list_url()).await?;
            let file = shellwing_update::release_file(&list, &wanted)?;
            let dest = dir.join(&file.file);
            shellwing_update::download::download(&file.url, &file.sha256, &dest, |n, of| {
                job_got.store(n, Ordering::Relaxed);
                job_total.store(of.unwrap_or(0), Ordering::Relaxed);
            })
            .await?;
            Ok::<PathBuf, shellwing_update::CheckError>(dest)
        });
        self.updates.stage = Stage::Downloading(0, None);
        self.refresh_settings(cx);
        cx.spawn(async move |this, cx| {
            // The progress, shown as it comes, until the download ends.
            let mut job = job;
            let done = loop {
                let tick = cx.background_executor().timer(PROGRESS_EVERY);
                let (n, of) = (got.load(Ordering::Relaxed), total.load(Ordering::Relaxed));
                let shown = this.update(cx, |ws, cx| {
                    ws.updates.stage = Stage::Downloading(n, (of > 0).then_some(of));
                    ws.refresh_settings(cx);
                });
                if shown.is_err() {
                    return;
                }
                if job.is_finished() {
                    break (&mut job).await;
                }
                tick.await;
            };
            let _ = this.update(cx, |ws, cx| {
                match done {
                    Ok(Ok(path)) => {
                        ws.updates.state.staged =
                            Some((v.to_string(), path.to_string_lossy().to_string()));
                        ws.save_update_state();
                        ws.updates.stage = Stage::Ready(v.clone());
                        ws.say(
                            tr!(
                                "shellwing {version} is ready: it is installed when you quit, or now with Restart.",
                                version = v.to_string()
                            ),
                            CONFIG_NOTICE,
                            cx,
                        );
                    }
                    Ok(Err(e)) => ws.updates.stage = Stage::Failed(e.to_string()),
                    Err(e) => ws.updates.stage = Stage::Failed(e.to_string()),
                }
                ws.refresh_settings(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Quits to install the downloaded release, and starts it: asked first
    /// while connections are open, as quitting is.
    pub(crate) fn restart_to_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.updates.stage, Stage::Ready(_)) {
            return;
        }
        self.updates.relaunch = true;
        if self.may_quit(window, cx) {
            cx.quit();
        }
    }

    /// As the app quits: the downloaded release in place of this copy, and
    /// started again after a Restart.
    fn install_staged(&mut self) {
        let Stage::Ready(v) = &self.updates.stage else {
            return;
        };
        let Some((_, file)) = self.updates.state.staged.clone() else {
            return;
        };
        let how = install();
        match shellwing_update::apply::apply(&how, &PathBuf::from(&file), self.updates.relaunch) {
            Ok(()) => {
                // The installer on Windows runs once this app has gone: its
                // file stays until the next run sees the new version.
                if how != Install::WindowsInstaller {
                    let _ = std::fs::remove_file(&file);
                    self.updates.state.staged = None;
                }
            }
            Err(e) => {
                eprintln!("cannot install shellwing {v}: {e}");
                // Not tried again on every quit: the next look offers it anew.
                let _ = std::fs::remove_file(&file);
                self.updates.state.staged = None;
            }
        }
        self.save_update_state();
    }

    /// Not offered again until a newer one comes.
    pub(crate) fn skip_update(&mut self, cx: &mut Context<Self>) {
        self.updates.state.skipped = self.updates.state.offered.clone();
        self.save_update_state();
        self.refresh_settings(cx);
        cx.notify();
    }

    /// Said as a download goes: "12.3 MB of 45.6 MB".
    pub(crate) fn download_said(got: u64, of: Option<u64>) -> String {
        match of {
            Some(of) => tr!(
                "Downloading… {got} of {total}",
                got = megabytes(got),
                total = megabytes(of)
            ),
            None => tr!("Downloading… {got}", got = megabytes(got)),
        }
    }

    pub(super) fn update_snap(&self) -> UpdateSnap {
        let current = shellwing_update::current();
        let s = &self.updates.state;
        let how = install();
        UpdateSnap {
            keys: shellwing_update::has_keys(),
            channel: current.channel().name(),
            current: current.to_string(),
            check: self.live.settings().updates.check,
            checking: self.updates.checking,
            checked_at: s.checked_at.map(|t| UNIX_EPOCH + Duration::from_secs(t)),
            offered: self.update_offered().map(|v| v.to_string()),
            problem: self.updates.problem.clone(),
            managed: match how {
                Install::Managed { by, command } => Some((by, command)),
                _ => None,
            },
            updates_itself: how.updates_itself(),
            stage: self.updates.stage.clone(),
        }
    }
}

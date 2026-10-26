//! Syncing on a phone: the desktop's own worker (`shellwing_core::sync`),
//! with a place set up here (a WebDAV folder, S3-compatible storage, a Git
//! repository over HTTPS, or a folder the app was given, such as one in
//! iCloud Drive), or joined from the desktop with the recovery key, a
//! passphrase, or pairing. iOS runs no `git`: there, libgit2 does
//! (`shellwing_remote::LibGitRepo`), the desktop's repository as it is.

use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use shellwing_config::{GitSync, S3Sync, SyncPlace, SyncPlaceText, WebDavSync};
use shellwing_core::sync::{self as worker, SyncEvent, SyncWorker};
use shellwing_vault::{AskState, FolderState, Request};

use crate::{MobileError, Shellwing};

/// Told when the sync's state moved: a sync done (hosts may have changed),
/// a device asking to join, pairing going on. Off the main thread.
#[uniffi::export(with_foreign)]
pub trait SyncListener: Send + Sync {
    fn changed(&self);
}

/// Where the vault syncs.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum Place {
    WebDav {
        /// The folder's address: `https://cloud.example.com/remote.php/dav/files/me/`.
        url: String,
        user: String,
    },
    S3 {
        endpoint: String,
        /// Empty: `auto`, as R2 takes.
        region: String,
        bucket: String,
        /// A folder in the bucket; empty: its top.
        prefix: String,
        access_key: String,
    },
    /// A folder on the phone the app may use (one in iCloud Drive, chosen
    /// in Files).
    Folder { path: String },
    /// A Git repository, by its `https://` address, signed in to with a
    /// token (the place's secret).
    Git {
        url: String,
        /// Empty: the repository's default.
        branch: String,
        /// Who the token signs in as, where the host needs it; empty: any.
        user: String,
    },
}

impl Place {
    fn config(&self) -> SyncPlace {
        match self.clone() {
            Place::WebDav { url, user } => SyncPlace::WebDav(WebDavSync { url, user }),
            Place::S3 {
                endpoint,
                region,
                bucket,
                prefix,
                access_key,
            } => SyncPlace::S3(S3Sync {
                endpoint,
                region,
                bucket,
                prefix,
                access_key,
            }),
            Place::Folder { path } => SyncPlace::Folder(path.into()),
            Place::Git { url, branch, user } => SyncPlace::Git(GitSync { url, branch, user }),
        }
    }

    fn of(place: &SyncPlace) -> Place {
        match place.clone() {
            SyncPlace::WebDav(w) => Place::WebDav {
                url: w.url,
                user: w.user,
            },
            SyncPlace::S3(s) => Place::S3 {
                endpoint: s.endpoint,
                region: s.region,
                bucket: s.bucket,
                prefix: s.prefix,
                access_key: s.access_key,
            },
            SyncPlace::Folder(f) => Place::Folder {
                path: f.to_string_lossy().into_owned(),
            },
            SyncPlace::Git(g) => Place::Git {
                url: g.url,
                branch: g.branch,
                user: g.user,
            },
        }
    }
}

/// What connecting to a place found.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum Connected {
    /// It was empty (or this vault's): it syncs there now. The recovery key
    /// opens it on other devices: shown to write down.
    Started { recovery_key: String },
    /// Another device's vault is there: joined with its recovery key, its
    /// passphrase (`passphrase`: one is set), or by pairing.
    Other { passphrase: bool },
}

/// A device asking to join the vault, from another one that syncs there.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct JoinRequest {
    pub id: String,
    pub device: String,
    /// What both devices show: approved only when they match.
    pub code: String,
}

/// This device's request to join, while pairing.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum Pairing {
    /// No device that syncs has answered yet.
    Waiting,
    /// `device` answered: approve there if it shows `code`.
    Compare {
        device: String,
        code: String,
    },
    Joined,
    Refused,
    /// Nobody approved in 10 minutes.
    Expired,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SyncStatus {
    pub place: Option<Place>,
    /// The place, said: `WebDAV: cloud.example.com`.
    pub said: Option<String>,
    pub syncing: bool,
    /// Seconds since the epoch of the last sync done.
    pub last: Option<u64>,
    /// Changes the last sync brought from other devices.
    pub pulled: u32,
    pub error: Option<String>,
    pub pairing: Option<Pairing>,
    pub requests: Vec<JoinRequest>,
}

/// What the worker said, kept for the app to read.
#[derive(Default)]
pub(crate) struct State {
    syncing: bool,
    last: Option<u64>,
    pulled: u32,
    error: Option<String>,
    pairing: Option<Pairing>,
    /// Where this device asked to join, to sync at once it is in.
    asking_at: Option<SyncPlace>,
    requests: Vec<Request>,
    listener: Option<Arc<dyn SyncListener>>,
}

pub(crate) struct Syncing {
    pub worker: SyncWorker,
    pub state: Arc<Mutex<State>>,
}

impl Syncing {
    /// The worker, started on the vault and the configuration; its events
    /// kept in `state` by a task on `rt`.
    pub fn start(
        vault: shellwing_core::secrets::VaultSlot,
        store: shellwing_config::Store,
        rt: &tokio::runtime::Handle,
    ) -> Syncing {
        let place = store.settings().ok().and_then(|s| s.sync.place());
        let (tx, rx) = async_channel::unbounded::<SyncEvent>();
        let worker = SyncWorker::start(vault, Some(store.clone()), tx);
        worker.set_place(place);
        let state: Arc<Mutex<State>> = Arc::default();
        let kept = state.clone();
        let (w, joined_store) = (worker.clone(), store);
        rt.spawn(async move {
            while let Ok(ev) = rx.recv().await {
                let listener = {
                    let mut s = lock(&kept);
                    match ev {
                        SyncEvent::Started => s.syncing = true,
                        SyncEvent::Done(r) => {
                            s.syncing = false;
                            s.error = None;
                            s.pulled = u32::try_from(r.pulled).unwrap_or(u32::MAX);
                            s.last = SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_secs());
                        }
                        SyncEvent::Failed(e) => {
                            s.syncing = false;
                            s.error = Some(e);
                        }
                        SyncEvent::Asking(a) => {
                            let p = pairing(&a);
                            if p != Pairing::Waiting && !matches!(p, Pairing::Compare { .. }) {
                                // Over: in, or not. In, the place asked at is
                                // this device's now.
                                let at = s.asking_at.take();
                                if let (Pairing::Joined, Some(place)) = (&p, at) {
                                    if let Err(e) = keep_place(&joined_store, Some(&place)) {
                                        s.error = Some(e);
                                    }
                                    w.set_place(Some(place));
                                    w.now();
                                }
                            }
                            s.pairing = Some(p);
                        }
                        SyncEvent::Requests(r) => s.requests = r,
                        SyncEvent::PairingFailed(e) => s.error = Some(e),
                        SyncEvent::PassphraseJoins(_) => {}
                    }
                    s.listener.clone()
                };
                if let Some(l) = listener {
                    l.changed();
                }
            }
        });
        Syncing { worker, state }
    }
}

fn pairing(a: &AskState) -> Pairing {
    match a {
        AskState::Waiting => Pairing::Waiting,
        AskState::Compare { device, code } => Pairing::Compare {
            device: device.clone(),
            code: code.clone(),
        },
        AskState::Joined => Pairing::Joined,
        AskState::Refused => Pairing::Refused,
        AskState::Expired => Pairing::Expired,
    }
}

/// `[sync]` in config.toml: where this device syncs.
fn keep_place(store: &shellwing_config::Store, place: Option<&SyncPlace>) -> Result<(), String> {
    let text = place.map(|p| match p {
        SyncPlace::Folder(f) => SyncPlaceText::Folder(f.to_string_lossy().into_owned()),
        SyncPlace::WebDav(w) => SyncPlaceText::WebDav(w.clone()),
        SyncPlace::S3(s) => SyncPlaceText::S3(s.clone()),
        SyncPlace::Git(g) => SyncPlaceText::Git(g.clone()),
    });
    store
        .set_sync_place(text.as_ref())
        .map_err(|e| e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[uniffi::export]
impl Shellwing {
    pub fn set_sync_listener(&self, listener: Arc<dyn SyncListener>) {
        lock(&self.sync.state).listener = Some(listener);
    }

    pub fn sync_status(&self) -> SyncStatus {
        let place = self.sync.worker.place();
        let s = lock(&self.sync.state);
        SyncStatus {
            said: place.as_ref().map(worker::describe),
            place: place.as_ref().map(Place::of),
            syncing: s.syncing,
            last: s.last,
            pulled: s.pulled,
            error: s.error.clone(),
            pairing: s.pairing.clone(),
            requests: s
                .requests
                .iter()
                .map(|r| JoinRequest {
                    id: r.id.clone(),
                    device: r.device.clone(),
                    code: r.code.clone(),
                })
                .collect(),
        }
    }

    /// Connects to `place` (`secret`: its password or secret key, kept as
    /// this device's own in the vault). Blocks on the network: off the main
    /// thread.
    pub fn connect_sync(
        &self,
        place: Place,
        secret: Option<String>,
    ) -> Result<Connected, MobileError> {
        let vault = self.open_vault()?;
        vault
            .set_device_secret(worker::PLACE_SECRET, secret.as_deref())
            .map_err(MobileError::of)?;
        let config = place.config();
        let backend = worker::open(&config, &vault).map_err(MobileError::of)?;
        let found =
            shellwing_vault::transact(backend.as_ref(), |b| match vault.folder_state(b)? {
                FolderState::Empty | FolderState::Ours => Ok(Connected::Started {
                    recovery_key: vault.start_sync(b)?.to_string(),
                }),
                FolderState::Other => Ok(Connected::Other {
                    passphrase: vault.passphrase_joins(b)?,
                }),
            })
            .map_err(MobileError::of)?;
        if matches!(found, Connected::Started { .. }) {
            self.sync_at(config)?;
        }
        Ok(found)
    }

    /// Joins the vault at `place` with its recovery key (13 groups of 4).
    pub fn join_with_recovery_key(&self, place: Place, key: String) -> Result<(), MobileError> {
        let vault = self.open_vault()?;
        let config = place.config();
        let backend = worker::open(&config, &vault).map_err(MobileError::of)?;
        shellwing_vault::transact(backend.as_ref(), |b| vault.join(b, &key))
            .map_err(MobileError::of)?;
        self.sync_at(config)
    }

    /// Joins the vault at `place` with the passphrase another device set.
    pub fn join_with_passphrase(
        &self,
        place: Place,
        passphrase: String,
    ) -> Result<(), MobileError> {
        let vault = self.open_vault()?;
        let config = place.config();
        let backend = worker::open(&config, &vault).map_err(MobileError::of)?;
        shellwing_vault::transact(backend.as_ref(), |b| {
            vault.join_with_passphrase(b, &passphrase)
        })
        .map_err(MobileError::of)?;
        self.sync_at(config)
    }

    /// Asks the devices that sync at `place` to let this one in: they show a
    /// code, this one too (see [`SyncStatus::pairing`]).
    pub fn ask_to_join(&self, place: Place) {
        let config = place.config();
        {
            let mut s = lock(&self.sync.state);
            s.pairing = Some(Pairing::Waiting);
            s.asking_at = Some(config.clone());
        }
        self.sync.worker.ask_to_join(config);
    }

    pub fn cancel_join(&self) {
        {
            let mut s = lock(&self.sync.state);
            s.pairing = None;
            s.asking_at = None;
        }
        self.sync.worker.cancel_ask();
    }

    /// Lets in the device asking with `id`: it gets the vault's keys.
    pub fn approve(&self, id: String) {
        lock(&self.sync.state).requests.retain(|r| r.id != id);
        self.sync.worker.approve(id);
    }

    pub fn refuse(&self, id: String) {
        lock(&self.sync.state).requests.retain(|r| r.id != id);
        self.sync.worker.refuse(id);
    }

    pub fn sync_now(&self) {
        self.sync.worker.now();
    }

    /// Stops syncing here: the place forgotten, its secret too. The vault
    /// stays, and so do the copies elsewhere.
    pub fn stop_sync(&self) -> Result<(), MobileError> {
        keep_place(&self.store, None).map_err(MobileError::Failed)?;
        self.sync.worker.set_place(None);
        if let Some(v) = self.vault.get() {
            let _ = v.set_device_secret(worker::PLACE_SECRET, None);
        }
        let mut s = lock(&self.sync.state);
        s.error = None;
        s.last = None;
        s.pairing = None;
        Ok(())
    }
}

impl Shellwing {
    /// Syncs at `place` from now on.
    fn sync_at(&self, place: SyncPlace) -> Result<(), MobileError> {
        keep_place(&self.store, Some(&place)).map_err(MobileError::Failed)?;
        self.sync.worker.set_place(Some(place));
        self.sync.worker.now();
        Ok(())
    }
}

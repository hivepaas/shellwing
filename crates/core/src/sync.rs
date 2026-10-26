//! Syncing the vault with a folder, a WebDAV server or S3-compatible storage
//! (the place, [`SyncPlace`]), on a thread of its own: every minute, and
//! right after something asks (a secret saved or forgotten, Sync now).
//!
//! Pairing goes through the place too: a new device's request to join (see
//! `shellwing_vault::Ask`), and the answers of a device that syncs. While one
//! is open the place is looked at every two seconds.
//!
//! Hosts and groups from hosts.toml ride along as vault records: before each
//! sync, edits in the file are recorded (only the fields that changed get new
//! stamps); after it, the merged result is written back into the file. So do
//! the settings of config.toml that are not this device's own (with `[sync]
//! settings`, on by default), as one record with a field per setting.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::collections::BTreeSet;

use shellwing_config::{Store, SyncPlace};
use shellwing_i18n::tr;
use shellwing_remote::Place;
use shellwing_vault::{
    Answerer, Ask, AskState, Backend, FolderState, Request, SyncReport, Vault, transact,
};

use crate::secrets::VaultSlot;

const EVERY: Duration = Duration::from_secs(60);
/// The settings' record: the same on every device, so they merge into one.
/// Hex, as synced files' names must start with.
const SETTINGS_ID: &str = "5e771a65000000000000000000000000";
/// How often the place is looked at while pairing.
const PAIRING: Duration = Duration::from_secs(2);
/// The same over Git, where each look is a fetch.
const PAIRING_GIT: Duration = Duration::from_secs(5);

pub enum SyncEvent {
    /// A sync starts.
    Started,
    Done(SyncReport),
    Failed(String),
    /// This device's request to join: where it stands.
    Asking(AskState),
    /// Requests from new devices, ready to approve or refuse.
    Requests(Vec<Request>),
    /// A step of pairing failed.
    PairingFailed(String),
    /// Whether a passphrase joins new devices to the vault there.
    PassphraseJoins(bool),
}

/// The vault's own secret of the place (a WebDAV password, an S3 secret
/// key), kept as this device's.
pub const PLACE_SECRET: &str = "sync";

/// The place with its secret from the vault, ready to open.
pub fn resolve(place: &SyncPlace, vault: &Vault) -> shellwing_vault::Result<Place> {
    // A token for Git is not needed: git signs in as it does for the user.
    let token = || -> shellwing_vault::Result<String> {
        Ok(vault
            .device_secret(PLACE_SECRET)?
            .map(|s| s.to_string())
            .unwrap_or_default())
    };
    let secret = || -> shellwing_vault::Result<String> {
        match vault.device_secret(PLACE_SECRET)? {
            Some(s) => Ok(s.to_string()),
            None => Err(shellwing_vault::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                tr!("its password is not saved on this device: connect again in Settings › Sync"),
            ))),
        }
    };
    Ok(match place {
        SyncPlace::Folder(f) => Place::Folder(f.clone()),
        SyncPlace::WebDav(w) => Place::WebDav {
            url: w.url.clone(),
            user: w.user.clone(),
            password: secret()?,
        },
        SyncPlace::S3(s3) => Place::S3 {
            endpoint: s3.endpoint.clone(),
            region: s3.region.clone(),
            bucket: s3.bucket.clone(),
            prefix: s3.prefix.clone(),
            access_key: s3.access_key.clone(),
            secret_key: secret()?,
        },
        SyncPlace::Git(g) => Place::Git {
            url: g.url.clone(),
            branch: g.branch.clone(),
            user: g.user.clone(),
            token: token()?,
            // In every commit: the random id, not the computer's name.
            device: vault.device_id().to_string(),
        },
    })
}

/// How `place` is named to the user.
pub fn describe(place: &SyncPlace) -> String {
    let unsealed = match place {
        SyncPlace::Folder(f) => Place::Folder(f.clone()),
        SyncPlace::WebDav(w) => Place::WebDav {
            url: w.url.clone(),
            user: w.user.clone(),
            password: String::new(),
        },
        SyncPlace::S3(s3) => Place::S3 {
            endpoint: s3.endpoint.clone(),
            region: s3.region.clone(),
            bucket: s3.bucket.clone(),
            prefix: s3.prefix.clone(),
            access_key: String::new(),
            secret_key: String::new(),
        },
        SyncPlace::Git(g) => Place::Git {
            url: g.url.clone(),
            branch: g.branch.clone(),
            user: g.user.clone(),
            token: String::new(),
            device: String::new(),
        },
    };
    unsealed.describe()
}

/// Opens the place's backend with its secret.
pub fn open(place: &SyncPlace, vault: &Vault) -> shellwing_vault::Result<Arc<dyn Backend>> {
    Ok(resolve(place, vault)?.open()?)
}

/// The backend of the place now, opened again only when the place (or its
/// secret) changed: a remote one keeps what it read.
#[derive(Default)]
struct Opened(Option<(Place, Arc<dyn Backend>)>);

impl Opened {
    fn get(
        &mut self,
        place: &SyncPlace,
        vault: &Vault,
    ) -> shellwing_vault::Result<Arc<dyn Backend>> {
        let resolved = resolve(place, vault)?;
        if let Some((p, b)) = &self.0
            && *p == resolved
        {
            return Ok(b.clone());
        }
        let backend = resolved.open()?;
        self.0 = Some((resolved, backend.clone()));
        Ok(backend)
    }
}

enum Cmd {
    Sync,
    /// A sync, then a word back once it is over (or nothing could sync).
    SyncThen(mpsc::Sender<()>),
    /// Ask to join the vault there, as this device.
    Ask(SyncPlace),
    CancelAsk,
    Approve(String),
    Refuse(String),
}

#[derive(Clone)]
pub struct SyncWorker {
    wake: mpsc::Sender<Cmd>,
    place: Arc<Mutex<Option<SyncPlace>>>,
    /// Records left out of every sync for now (notes open in their windows):
    /// they sync when let go.
    held: Arc<Mutex<BTreeSet<String>>>,
}

/// This device's name, shown to the other when pairing.
pub fn device_name() -> String {
    shellwing_vault::host_name().unwrap_or_else(|| "another device".into())
}

/// The title of a note made to keep a text that another device's edit
/// replaced without seeing it; `device` wrote that text
/// (docs/design/note-conflicts.md).
pub fn copy_title(title: &str, device: Option<&str>) -> String {
    match device {
        Some(d) => tr!(
            "{title} (from {device})",
            title = title.to_string(),
            device = d.to_string()
        ),
        None => tr!("{title} (from another device)", title = title.to_string()),
    }
}

impl SyncWorker {
    pub fn start(
        vault: VaultSlot,
        store: Option<Store>,
        events: async_channel::Sender<SyncEvent>,
    ) -> SyncWorker {
        let (wake, woken) = mpsc::channel::<Cmd>();
        let place: Arc<Mutex<Option<SyncPlace>>> = Arc::default();
        let watched = place.clone();
        let held: Arc<Mutex<BTreeSet<String>>> = Arc::default();
        let holding = held.clone();
        let config = store;
        let spawned = std::thread::Builder::new()
            .name("shellwing-sync".into())
            .spawn(move || {
                let mut answerer = Answerer::new(&device_name());
                let mut asking: Option<(Ask, SyncPlace, Arc<dyn Backend>)> = None;
                let mut opened = Opened::default();
                // What the app was last told of the passphrase that joins.
                let mut joins: Option<bool> = None;
                let mut offered: Vec<Request> = vec![];
                // What the app was last told of this device's request.
                let mut told: Option<AskState> = None;
                // Due at once: the first sync runs at start.
                let mut last_sync: Option<Instant> = None;
                // Those waiting for a sync to be over: told when the round
                // that took their wake is done.
                let mut acks: Vec<mpsc::Sender<()>> = vec![];
                loop {
                    for a in acks.drain(..) {
                        let _ = a.send(());
                    }
                    let pairing = asking.is_some() || answerer.busy();
                    let over_git = matches!(
                        watched.lock().ok().and_then(|f| f.clone()),
                        Some(SyncPlace::Git(_))
                    ) || matches!(&asking, Some((_, SyncPlace::Git(_), _)));
                    let wait = match (pairing, over_git) {
                        (false, _) => EVERY,
                        (true, false) => PAIRING,
                        (true, true) => PAIRING_GIT,
                    };
                    let mut sync = match woken.recv_timeout(wait) {
                        Ok(Cmd::SyncThen(a)) => {
                            acks.push(a);
                            true
                        }
                        Ok(cmd) => Self::run(
                            cmd,
                            &vault,
                            &mut asking,
                            &mut answerer,
                            &mut opened,
                            &watched,
                            &events,
                        ),
                        Err(RecvTimeoutError::Timeout) => false,
                        Err(RecvTimeoutError::Disconnected) => return,
                    };
                    // Several wakes in a row are one sync.
                    while let Ok(cmd) = woken.try_recv() {
                        if let Cmd::SyncThen(a) = cmd {
                            acks.push(a);
                            sync = true;
                            continue;
                        }
                        sync |= Self::run(
                            cmd,
                            &vault,
                            &mut asking,
                            &mut answerer,
                            &mut opened,
                            &watched,
                            &events,
                        );
                    }
                    let Some(vault) = vault.get() else {
                        continue;
                    };
                    if asking.is_none() {
                        told = None;
                    }
                    if let Some((ask, at, backend)) = &mut asking {
                        let state = transact(backend.as_ref(), |b| ask.step(&vault, b));
                        // Over once in, refused or expired, or when the keys sent
                        // are no good; a folder that cannot be read is tried again.
                        let done = matches!(
                            state,
                            Ok(AskState::Joined | AskState::Refused | AskState::Expired)
                                | Err(shellwing_vault::Error::Pairing(_))
                        );
                        if matches!(state, Ok(AskState::Joined)) {
                            if let Ok(mut f) = watched.lock() {
                                *f = Some(at.clone());
                            }
                            sync = true;
                        }
                        // Told again only when it changed.
                        let event = match state {
                            Ok(s) if told.as_ref() == Some(&s) => None,
                            Ok(s) => {
                                told = Some(s.clone());
                                Some(SyncEvent::Asking(s))
                            }
                            Err(e) => Some(SyncEvent::PairingFailed(e.to_string())),
                        };
                        if let Some(event) = event
                            && events.send_blocking(event).is_err()
                        {
                            return;
                        }
                        if done {
                            asking = None;
                        }
                    }
                    let Some(place) = watched.lock().ok().and_then(|f| f.clone()) else {
                        joins = None;
                        continue;
                    };
                    let store = match opened.get(&place, &vault) {
                        Ok(b) => b,
                        Err(e) => {
                            if events
                                .send_blocking(SyncEvent::Failed(e.to_string()))
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                    };
                    let store = store.as_ref();
                    match transact(store, |b| answerer.poll(&vault, b)) {
                        Ok(ready) if ready != offered => {
                            offered = ready.clone();
                            if events.send_blocking(SyncEvent::Requests(ready)).is_err() {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            let _ = events.send_blocking(SyncEvent::PairingFailed(e.to_string()));
                        }
                    }
                    let due = last_sync.is_none_or(|t| t.elapsed() >= EVERY);
                    if !sync && !due {
                        continue;
                    }
                    last_sync = Some(Instant::now());
                    if events.send_blocking(SyncEvent::Started).is_err() {
                        return;
                    }
                    let now_held = holding.lock().map(|h| h.clone()).unwrap_or_default();
                    let event = sync_once(&vault, store, config.as_ref(), &now_held);
                    if events.send_blocking(event).is_err() {
                        return;
                    }
                    if let Ok(now) = vault.passphrase_joins(store)
                        && joins != Some(now)
                    {
                        joins = Some(now);
                        if events
                            .send_blocking(SyncEvent::PassphraseJoins(now))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("cannot start syncing: {e}");
        }
        SyncWorker { wake, place, held }
    }

    /// Syncs with `place` from now on (None: stops), starting right away.
    pub fn set_place(&self, place: Option<SyncPlace>) {
        let changed = self
            .place
            .lock()
            .map(|mut f| {
                let changed = *f != place;
                *f = place;
                changed
            })
            .unwrap_or(false);
        if changed {
            self.now();
        }
    }

    pub fn place(&self) -> Option<SyncPlace> {
        self.place.lock().ok().and_then(|f| f.clone())
    }

    pub fn now(&self) {
        let _ = self.wake.send(Cmd::Sync);
    }

    /// Leaves record `id` out of syncs (a note open in its window).
    pub fn hold(&self, id: &str) {
        if let Ok(mut h) = self.held.lock() {
            h.insert(id.to_string());
        }
    }

    /// Lets record `id` sync again; at once when it `changed` meanwhile.
    pub fn release(&self, id: &str, changed: bool) {
        if let Ok(mut h) = self.held.lock() {
            h.remove(id);
        }
        if changed {
            self.now();
        }
    }

    /// Asks for a sync and waits for it to be over, `most` at the longest
    /// (quitting: the last edits go out first). False when it took longer.
    pub fn sync_and_wait(&self, most: Duration) -> bool {
        let (tx, rx) = mpsc::channel();
        if self.wake.send(Cmd::SyncThen(tx)).is_err() {
            return true;
        }
        rx.recv_timeout(most).is_ok()
    }

    /// Asks to join the vault at `place` from a device that syncs it; how it
    /// goes comes as [`SyncEvent::Asking`].
    pub fn ask_to_join(&self, place: SyncPlace) {
        let _ = self.wake.send(Cmd::Ask(place));
    }

    pub fn cancel_ask(&self) {
        let _ = self.wake.send(Cmd::CancelAsk);
    }

    /// Lets the device of request `id` in.
    pub fn approve(&self, id: String) {
        let _ = self.wake.send(Cmd::Approve(id));
    }

    pub fn refuse(&self, id: String) {
        let _ = self.wake.send(Cmd::Refuse(id));
    }

    /// Carries out a command on the sync thread; whether a sync should follow.
    fn run(
        cmd: Cmd,
        vault: &VaultSlot,
        asking: &mut Option<(Ask, SyncPlace, Arc<dyn Backend>)>,
        answerer: &mut Answerer,
        opened: &mut Opened,
        watched: &Mutex<Option<SyncPlace>>,
        events: &async_channel::Sender<SyncEvent>,
    ) -> bool {
        let fail = |e: shellwing_vault::Error| {
            let _ = events.send_blocking(SyncEvent::PairingFailed(e.to_string()));
            false
        };
        // The place synced now, opened.
        let mut current = |vault: &Vault| {
            let place = watched.lock().ok().and_then(|f| f.clone())?;
            opened.get(&place, vault).ok()
        };
        match cmd {
            // (Taken before run, with its word back.)
            Cmd::Sync | Cmd::SyncThen(_) => true,
            Cmd::Ask(place) => {
                if let Some((old, _, at)) = asking.take() {
                    let _ = transact(at.as_ref(), |b| old.cancel(b));
                }
                let Some(v) = vault.get() else {
                    return false;
                };
                let backend = match open(&place, &v) {
                    Ok(b) => b,
                    Err(e) => return fail(e),
                };
                match transact(backend.as_ref(), |b| Ask::new(b, &device_name())) {
                    Ok(ask) => *asking = Some((ask, place, backend)),
                    Err(e) => return fail(e),
                }
                false
            }
            Cmd::CancelAsk => {
                if let Some((ask, _, at)) = asking.take() {
                    let _ = transact(at.as_ref(), |b| ask.cancel(b));
                }
                false
            }
            Cmd::Approve(id) => {
                let Some(v) = vault.get() else {
                    return false;
                };
                let Some(store) = current(&v) else {
                    return false;
                };
                match transact(store.as_ref(), |b| answerer.approve(&v, b, &id)) {
                    Ok(()) => false,
                    Err(e) => fail(e),
                }
            }
            Cmd::Refuse(id) => {
                let Some(v) = vault.get() else {
                    return false;
                };
                let Some(store) = current(&v) else {
                    return false;
                };
                match transact(store.as_ref(), |b| answerer.refuse(b, &id)) {
                    Ok(()) => false,
                    Err(e) => fail(e),
                }
            }
        }
    }
}

/// One round: records the edits made in the files, syncs, and writes the
/// merged result back into them.
fn sync_once(
    vault: &Vault,
    store: &dyn Backend,
    config: Option<&Store>,
    held: &BTreeSet<String>,
) -> SyncEvent {
    // A folder set by hand in config.toml starts empty: begin
    // there (the recovery key can be shown from the palette).
    let started = transact(store, |b| match vault.folder_state(b)? {
        FolderState::Empty => vault.start_sync(b).map(|_| ()),
        _ => Ok(()),
    });
    // Hosts problems (a broken hosts.toml) do not stop secrets from syncing.
    let hosts_problem = config.and_then(|c| record_hosts(c, vault).err());
    let settings = config.filter(|c| c.settings().is_ok_and(|s| s.sync.settings));
    // A device whose settings have never synced takes the
    // others' first: recording its own now would stamp them
    // newer than the ones it has not seen yet.
    let settings_known = settings.is_some_and(|_| has_settings(vault));
    let settings_problem = settings
        .filter(|_| settings_known)
        .and_then(|c| record_settings(c, vault).err());
    let synced = started.and_then(|()| vault.sync_holding(store, held));
    let written = match (&synced, config) {
        (Ok(_), Some(c)) if hosts_problem.is_none() => write_hosts(c, vault).err(),
        _ => None,
    };
    let settings_written = match (&synced, settings) {
        (Ok(_), Some(c)) if settings_problem.is_none() => {
            let joined = write_settings(c, vault, !settings_known);
            // Its own settings go out now, merged with the others'.
            if joined.as_ref().is_ok_and(|&new| new) {
                let _ = vault.sync_holding(store, held);
            }
            joined.err()
        }
        _ => None,
    };
    let hosts_problem = hosts_problem.or(settings_problem);
    let written = written.or(settings_written);
    match (synced, hosts_problem.or(written)) {
        (Ok(_), Some(p)) => SyncEvent::Failed(p),
        (Ok(r), None) => SyncEvent::Done(r),
        (Err(e), _) => SyncEvent::Failed(e.to_string()),
    }
}

/// Records edits made in hosts.toml since the last sync.
fn record_hosts(store: &Store, vault: &Vault) -> Result<(), String> {
    let file_exists = store.dir().join("hosts.toml").exists();
    store
        .assign_host_ids()
        .map_err(|e| tr!("hosts not synced: {e}", e = e.to_string()))?;
    let hosts = store
        .hosts()
        .map_err(|e| tr!("hosts not synced: {e}", e = e.to_string()))?;
    let err = |e: shellwing_vault::Error| tr!("hosts not synced: {e}", e = e.to_string());
    let mut present = BTreeSet::new();
    let mut kept_here = BTreeSet::new();
    for h in &hosts.hosts {
        let Some(id) = &h.id else { continue };
        if h.local_only {
            kept_here.insert(id.clone());
            continue;
        }
        present.insert(id.clone());
        // Deleted on another device: the write-back removes it here.
        if !vault.is_deleted(id).map_err(err)? {
            vault
                .update(id, "host", &h.name, &h.sync_fields())
                .map_err(err)?;
        }
    }
    for g in &hosts.groups {
        let Some(id) = &g.id else { continue };
        present.insert(id.clone());
        if !vault.is_deleted(id).map_err(err)? {
            vault
                .update(id, "group", &g.name, &g.sync_fields())
                .map_err(err)?;
        }
    }
    // A host gone from the file was deleted here; a missing file is not taken
    // to mean every host was (it is written again instead).
    if file_exists {
        for kind in ["host", "group"] {
            for (id, _) in vault.entries(kind).map_err(err)? {
                if !present.contains(&id) && !kept_here.contains(&id) {
                    vault.delete(&id).map_err(err)?;
                }
            }
        }
    }
    Ok(())
}

fn has_settings(vault: &Vault) -> bool {
    vault
        .entries("settings")
        .is_ok_and(|e| e.iter().any(|(id, _)| id == SETTINGS_ID))
}

/// Records the synced settings config.toml has now.
fn record_settings(store: &Store, vault: &Vault) -> Result<(), String> {
    let fields = store
        .synced_settings()
        .map_err(|e| tr!("settings not synced: {e}", e = e.to_string()))?;
    vault
        .update(SETTINGS_ID, "settings", "Settings", &fields)
        .map_err(|e| tr!("settings not synced: {e}", e = e.to_string()))?;
    Ok(())
}

/// Writes the merged settings into config.toml. The first time (`first`),
/// the others' are taken where they set a value and this device's kept
/// where they set none, then recorded, to go out: whether that happened.
fn write_settings(store: &Store, vault: &Vault, first: bool) -> Result<bool, String> {
    let err = |e: String| tr!("settings not written: {e}", e = e);
    let theirs = vault
        .entries("settings")
        .map_err(|e| err(e.to_string()))?
        .into_iter()
        .find(|(id, _)| id == SETTINGS_ID)
        .map(|(_, f)| f);
    let fields = match (theirs, first) {
        (Some(theirs), false) => theirs,
        (Some(theirs), true) => {
            let mut mine = store.synced_settings().map_err(|e| err(e.to_string()))?;
            mine.extend(theirs);
            mine
        }
        // Nobody's yet: this device's are the first.
        (None, _) => {
            record_settings(store, vault)?;
            return Ok(true);
        }
    };
    store
        .apply_synced_settings(&fields)
        .map_err(|e| err(e.to_string()))?;
    if first {
        record_settings(store, vault)?;
    }
    Ok(first)
}

/// Writes the merged hosts and groups back into hosts.toml.
fn write_hosts(store: &Store, vault: &Vault) -> Result<(), String> {
    let err = |e: shellwing_vault::Error| tr!("hosts not written: {e}", e = e.to_string());
    let hosts = vault.entries("host").map_err(err)?;
    let groups = vault.entries("group").map_err(err)?;
    store
        .apply_synced_hosts(&hosts, &groups)
        .map_err(|e| tr!("hosts not written: {e}", e = e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::{Folder, MemoryKeys};

    struct Device {
        store: Store,
        vault: Vault,
        _keys: MemoryKeys,
    }

    impl Device {
        fn new(root: &std::path::Path, name: &str, hosts: &str) -> Device {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join("hosts.toml"), hosts).expect("hosts");
            let keys = MemoryKeys::default();
            let vault = Vault::open(&dir.join("vault.db"), &keys).expect("vault");
            Device {
                store: Store::new(dir),
                vault,
                _keys: keys,
            }
        }

        /// One round, as the worker does it.
        fn sync(&self, folder: &Folder) {
            match sync_once(&self.vault, folder, Some(&self.store), &BTreeSet::new()) {
                SyncEvent::Done(_) => {}
                SyncEvent::Failed(e) => panic!("sync: {e}"),
                _ => panic!("not a sync"),
            }
        }

        fn config(&self) -> String {
            std::fs::read_to_string(self.store.dir().join("config.toml")).unwrap_or_default()
        }

        fn set_config(&self, text: &str) {
            std::fs::write(self.store.dir().join("config.toml"), text).expect("config");
        }

        fn hosts(&self) -> shellwing_config::Hosts {
            self.store.hosts().expect("hosts")
        }

        fn edit(&self, from: &str, to: &str) {
            let path = self.store.dir().join("hosts.toml");
            let text = std::fs::read_to_string(&path).expect("read");
            assert!(text.contains(from), "{from:?} in\n{text}");
            std::fs::write(&path, text.replacen(from, to, 1)).expect("write");
        }
    }

    #[test]
    fn settings_travel_but_each_device_keeps_its_own() {
        let root =
            std::env::temp_dir().join(format!("shellwing-settings-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = Folder::new(&root.join("sync"));
        let a = Device::new(&root, "a", "");
        a.set_config("[appearance]\ntheme = \"nord\"\nfont = { size = 16 }\n");
        a.sync(&folder);
        let rk = a.vault.recovery_key().expect("rk").expect("started");

        // The second device had its own theme and a UI font, and a shell of its own.
        let b = Device::new(&root, "b", "");
        b.set_config("# b's\n[appearance]\ntheme = \"dracula\"\nui_font = { size = 15 }\n\n[terminal]\nlocal_shell = \"fish\"\n");
        b.vault.join(&folder, &rk.to_string()).expect("join");
        b.sync(&folder);
        let bs = b.store.settings().expect("b");
        assert_eq!(
            bs.appearance.theme, "nord",
            "the synced ones win, the first time"
        );
        assert_eq!(bs.appearance.font.size, 16.0);
        assert_eq!(bs.appearance.ui_font.size, 15.0, "what only it set stays");
        assert_eq!(bs.terminal.local_shell.as_deref(), Some("fish"));
        assert!(
            b.config().starts_with("# b's"),
            "its file keeps its comments"
        );

        // And goes the other way; edits then travel both ways.
        a.sync(&folder);
        assert_eq!(a.store.settings().expect("a").appearance.ui_font.size, 15.0);
        assert!(
            a.store
                .settings()
                .expect("a")
                .terminal
                .local_shell
                .is_none(),
            "b's own shell stays on b"
        );
        a.set_config(&a.config().replace("\"nord\"", "\"tokyo-night\""));
        a.sync(&folder);
        b.sync(&folder);
        assert_eq!(
            b.store.settings().expect("b").appearance.theme,
            "tokyo-night"
        );

        // Turned off on a device, nothing more comes in.
        b.set_config(&format!("{}\n[sync]\nsettings = false\n", b.config()));
        a.set_config(&a.config().replace("\"tokyo-night\"", "\"nord\""));
        a.sync(&folder);
        b.sync(&folder);
        assert_eq!(
            b.store.settings().expect("b").appearance.theme,
            "tokyo-night"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hosts_travel_between_devices_and_merge_by_field() {
        let root = std::env::temp_dir().join(format!("shellwing-hostsync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = Folder::new(&root.join("shared"));
        let a = Device::new(
            &root,
            "a",
            "# A's hosts\n[[group]]\nname = \"Production\"\n\n[[host]]\nname = \"web\"\naddress = \"10.0.0.5\"\ngroup = \"Production\"\n\n[[host]]\nname = \"db\"\naddress = \"10.0.0.6\"\n",
        );
        let b = Device::new(
            &root,
            "b",
            "[[host]]\nname = \"nas\"\naddress = \"192.168.1.4\"\n\n[[host]]\nname = \"mine\"\naddress = \"127.0.0.1\"\nlocal_only = true\n",
        );
        let rk = a.vault.start_sync(&folder).expect("start").to_string();
        a.sync(&folder);
        b.vault.join(&folder, &rk).expect("join");
        b.sync(&folder);
        a.sync(&folder);
        let names = |d: &Device| {
            let mut n: Vec<String> = d.hosts().hosts.iter().map(|h| h.name.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(
            names(&a),
            ["db", "nas", "web"],
            "local-only hosts stay home"
        );
        assert_eq!(names(&b), ["db", "mine", "nas", "web"]);
        assert_eq!(b.hosts().groups[0].name, "Production");
        assert!(
            std::fs::read_to_string(a.store.dir().join("hosts.toml"))
                .expect("read")
                .starts_with("# A's hosts"),
            "the comment stays"
        );

        // Both edit "web" offline, different fields: both edits survive.
        a.edit("address = \"10.0.0.5\"", "address = \"10.0.0.50\"");
        b.edit("name = \"web\"", "name = \"web\"\nuser = \"deploy\"");
        a.sync(&folder);
        b.sync(&folder);
        a.sync(&folder);
        for d in [&a, &b] {
            let hosts = d.hosts();
            let web = hosts.find("web").expect("web");
            assert_eq!(
                (web.address.as_str(), web.user.as_deref()),
                ("10.0.0.50", Some("deploy"))
            );
        }

        // A deletes db: gone from B too.
        let text = std::fs::read_to_string(a.store.dir().join("hosts.toml")).expect("read");
        let start = text.find("name = \"db\"").expect("db");
        let table = text[..start].rfind("[[host]]").expect("table");
        let end = text[start..]
            .find("[[host]]")
            .map_or(text.len(), |e| start + e);
        std::fs::write(
            a.store.dir().join("hosts.toml"),
            format!("{}{}", &text[..table], &text[end..]),
        )
        .expect("write");
        a.sync(&folder);
        b.sync(&folder);
        assert!(b.hosts().find("db").is_none());
        assert!(b.hosts().find("mine").is_some());
    }

    #[test]
    fn a_missing_hosts_file_is_written_again_not_taken_as_deleting_all() {
        let root =
            std::env::temp_dir().join(format!("shellwing-hostsync-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = Folder::new(&root.join("shared"));
        let a = Device::new(
            &root,
            "a",
            "[[host]]\nname = \"web\"\naddress = \"10.0.0.5\"\n",
        );
        a.vault.start_sync(&folder).expect("start");
        a.sync(&folder);
        std::fs::remove_file(a.store.dir().join("hosts.toml")).expect("remove");
        a.sync(&folder);
        assert!(a.hosts().find("web").is_some());
    }
}

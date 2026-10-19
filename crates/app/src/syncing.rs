//! Syncing the vault with a folder, on a thread of its own: every minute, and
//! right after something asks (a secret saved or forgotten, Sync now).
//!
//! Pairing goes through the folder too: a new device's request to join (see
//! `shellwing_vault::Ask`), and the answers of a device that syncs. While one
//! is open the folder is looked at every two seconds.
//!
//! Hosts and groups from hosts.toml ride along as vault records: before each
//! sync, edits in the file are recorded (only the fields that changed get new
//! stamps); after it, the merged result is written back into the file.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::collections::BTreeSet;

use shellwing_config::Store;
use shellwing_vault::{Answerer, Ask, AskState, Folder, FolderState, Request, SyncReport, Vault};

use crate::secrets::VaultSlot;

const EVERY: Duration = Duration::from_secs(60);
/// How often the folder is looked at while pairing.
const PAIRING: Duration = Duration::from_secs(2);

pub enum SyncEvent {
    Done(SyncReport),
    Failed(String),
    /// This device's request to join: where it stands.
    Asking(AskState),
    /// Requests from new devices, ready to approve or refuse.
    Requests(Vec<Request>),
    /// A step of pairing failed.
    PairingFailed(String),
}

enum Cmd {
    Sync,
    /// Ask to join the vault in the folder, as this device.
    Ask(PathBuf),
    CancelAsk,
    Approve(String),
    Refuse(String),
}

#[derive(Clone)]
pub struct SyncWorker {
    wake: mpsc::Sender<Cmd>,
    folder: Arc<Mutex<Option<PathBuf>>>,
}

/// This device's name, shown to the other when pairing.
pub fn device_name() -> String {
    let from_env = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"));
    let name = from_env.ok().or_else(|| {
        std::fs::read_to_string("/etc/hostname").ok().or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
    });
    match name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => "another device".into(),
    }
}

impl SyncWorker {
    pub fn start(
        vault: VaultSlot,
        store: Option<Store>,
        events: async_channel::Sender<SyncEvent>,
    ) -> SyncWorker {
        let (wake, woken) = mpsc::channel::<Cmd>();
        let folder: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        let watched = folder.clone();
        let config = store;
        let spawned = std::thread::Builder::new()
            .name("shellwing-sync".into())
            .spawn(move || {
                let mut answerer = Answerer::new(&device_name());
                let mut asking: Option<(Ask, PathBuf)> = None;
                let mut offered: Vec<Request> = vec![];
                // What the app was last told of this device's request.
                let mut told: Option<AskState> = None;
                // Due at once: the first sync runs at start.
                let mut last_sync: Option<Instant> = None;
                loop {
                    let pairing = asking.is_some() || answerer.busy();
                    let wait = if pairing { PAIRING } else { EVERY };
                    let mut sync = match woken.recv_timeout(wait) {
                        Ok(cmd) => {
                            Self::run(cmd, &vault, &mut asking, &mut answerer, &watched, &events)
                        }
                        Err(RecvTimeoutError::Timeout) => false,
                        Err(RecvTimeoutError::Disconnected) => return,
                    };
                    // Several wakes in a row are one sync.
                    while let Ok(cmd) = woken.try_recv() {
                        sync |=
                            Self::run(cmd, &vault, &mut asking, &mut answerer, &watched, &events);
                    }
                    let Some(vault) = vault.get() else {
                        continue;
                    };
                    if asking.is_none() {
                        told = None;
                    }
                    if let Some((ask, path)) = &mut asking {
                        let state = ask.step(&vault, &Folder::new(path));
                        // Over once in, refused or expired, or when the keys sent
                        // are no good; a folder that cannot be read is tried again.
                        let done = matches!(
                            state,
                            Ok(AskState::Joined | AskState::Refused | AskState::Expired)
                                | Err(shellwing_vault::Error::Pairing(_))
                        );
                        if matches!(state, Ok(AskState::Joined)) {
                            if let Ok(mut f) = watched.lock() {
                                *f = Some(path.clone());
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
                    let Some(path) = watched.lock().ok().and_then(|f| f.clone()) else {
                        continue;
                    };
                    let store = Folder::new(&path);
                    match answerer.poll(&vault, &store) {
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
                    // A folder set by hand in config.toml starts empty: begin
                    // there (the recovery key can be shown from the palette).
                    let started = match vault.folder_state(&store) {
                        Ok(FolderState::Empty) => vault.start_sync(&store).map(|_| ()),
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    };
                    // Hosts problems (a broken hosts.toml) do not stop secrets from syncing.
                    let hosts_problem = config.as_ref().and_then(|c| record_hosts(c, &vault).err());
                    let synced = started.and_then(|()| vault.sync(&store));
                    let written = match (&synced, &config) {
                        (Ok(_), Some(c)) if hosts_problem.is_none() => write_hosts(c, &vault).err(),
                        _ => None,
                    };
                    let event = match (synced, hosts_problem.or(written)) {
                        (Ok(_), Some(p)) => SyncEvent::Failed(p),
                        (Ok(r), None) => SyncEvent::Done(r),
                        (Err(e), _) => SyncEvent::Failed(e.to_string()),
                    };
                    if events.send_blocking(event).is_err() {
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("cannot start syncing: {e}");
        }
        SyncWorker { wake, folder }
    }

    /// Syncs with `folder` from now on (None: stops), starting right away.
    pub fn set_folder(&self, folder: Option<PathBuf>) {
        let changed = self
            .folder
            .lock()
            .map(|mut f| {
                let changed = *f != folder;
                *f = folder;
                changed
            })
            .unwrap_or(false);
        if changed {
            self.now();
        }
    }

    pub fn folder(&self) -> Option<PathBuf> {
        self.folder.lock().ok().and_then(|f| f.clone())
    }

    pub fn now(&self) {
        let _ = self.wake.send(Cmd::Sync);
    }

    /// Asks to join the vault in `folder` from a device that syncs it; how it
    /// goes comes as [`SyncEvent::Asking`].
    pub fn ask_to_join(&self, folder: PathBuf) {
        let _ = self.wake.send(Cmd::Ask(folder));
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
        asking: &mut Option<(Ask, PathBuf)>,
        answerer: &mut Answerer,
        watched: &Mutex<Option<PathBuf>>,
        events: &async_channel::Sender<SyncEvent>,
    ) -> bool {
        let fail = |e: shellwing_vault::Error| {
            let _ = events.send_blocking(SyncEvent::PairingFailed(e.to_string()));
            false
        };
        match cmd {
            Cmd::Sync => true,
            Cmd::Ask(path) => {
                if let Some((old, at)) = asking.take() {
                    let _ = old.cancel(&Folder::new(&at));
                }
                match Ask::new(&Folder::new(&path), &device_name()) {
                    Ok(ask) => *asking = Some((ask, path)),
                    Err(e) => return fail(e),
                }
                false
            }
            Cmd::CancelAsk => {
                if let Some((ask, at)) = asking.take() {
                    let _ = ask.cancel(&Folder::new(&at));
                }
                false
            }
            Cmd::Approve(id) => {
                let (Some(vault), Some(path)) =
                    (vault.get(), watched.lock().ok().and_then(|f| f.clone()))
                else {
                    return false;
                };
                match answerer.approve(&vault, &Folder::new(&path), &id) {
                    Ok(()) => false,
                    Err(e) => fail(e),
                }
            }
            Cmd::Refuse(id) => {
                let Some(path) = watched.lock().ok().and_then(|f| f.clone()) else {
                    return false;
                };
                match answerer.refuse(&Folder::new(&path), &id) {
                    Ok(()) => false,
                    Err(e) => fail(e),
                }
            }
        }
    }
}

/// Records edits made in hosts.toml since the last sync.
fn record_hosts(store: &Store, vault: &Vault) -> Result<(), String> {
    let file_exists = store.dir().join("hosts.toml").exists();
    store
        .assign_host_ids()
        .map_err(|e| format!("hosts not synced: {e}"))?;
    let hosts = store
        .hosts()
        .map_err(|e| format!("hosts not synced: {e}"))?;
    let err = |e: shellwing_vault::Error| format!("hosts not synced: {e}");
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

/// Writes the merged hosts and groups back into hosts.toml.
fn write_hosts(store: &Store, vault: &Vault) -> Result<(), String> {
    let err = |e: shellwing_vault::Error| format!("hosts not written: {e}");
    let hosts = vault.entries("host").map_err(err)?;
    let groups = vault.entries("group").map_err(err)?;
    store
        .apply_synced_hosts(&hosts, &groups)
        .map_err(|e| format!("hosts not written: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::MemoryKeys;

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
            record_hosts(&self.store, &self.vault).expect("record");
            self.vault.sync(folder).expect("sync");
            write_hosts(&self.store, &self.vault).expect("write");
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

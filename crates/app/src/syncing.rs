//! Syncing the vault with a folder, on a thread of its own: every minute, and
//! right after something asks (a secret saved or forgotten, Sync now).
//!
//! Hosts and groups from hosts.toml ride along as vault records: before each
//! sync, edits in the file are recorded (only the fields that changed get new
//! stamps); after it, the merged result is written back into the file.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::collections::BTreeSet;

use shellwing_config::Store;
use shellwing_vault::{Folder, FolderState, SyncReport, Vault};

use crate::secrets::VaultSlot;

const EVERY: Duration = Duration::from_secs(60);

pub enum SyncEvent {
    Done(SyncReport),
    Failed(String),
}

#[derive(Clone)]
pub struct SyncWorker {
    wake: mpsc::Sender<()>,
    folder: Arc<Mutex<Option<PathBuf>>>,
}

impl SyncWorker {
    pub fn start(
        vault: VaultSlot,
        store: Option<Store>,
        events: async_channel::Sender<SyncEvent>,
    ) -> SyncWorker {
        let (wake, woken) = mpsc::channel::<()>();
        let folder: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        let watched = folder.clone();
        let config = store;
        let spawned = std::thread::Builder::new()
            .name("shellwing-sync".into())
            .spawn(move || {
                loop {
                    match woken.recv_timeout(EVERY) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    // Several wakes in a row are one sync.
                    while woken.try_recv().is_ok() {}
                    let Some(path) = watched.lock().ok().and_then(|f| f.clone()) else {
                        continue;
                    };
                    let Some(vault) = vault.get() else {
                        continue;
                    };
                    let store = Folder::new(&path);
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
        let _ = self.wake.send(());
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

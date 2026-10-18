//! Syncing the vault with a folder, on a thread of its own: every minute, and
//! right after something asks (a secret saved or forgotten, Sync now).

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use shellwing_vault::{Folder, FolderState, SyncReport};

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
    pub fn start(vault: VaultSlot, events: async_channel::Sender<SyncEvent>) -> SyncWorker {
        let (wake, woken) = mpsc::channel::<()>();
        let folder: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        let watched = folder.clone();
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
                    let event = match started.and_then(|()| vault.sync(&store)) {
                        Ok(r) => SyncEvent::Done(r),
                        Err(e) => SyncEvent::Failed(e.to_string()),
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

//! Config files kept in the vault, placed on a session's host as the
//! desktop places them (`shellwing_core::placing`): until the session ends,
//! or to stay; over a file there, set aside or kept as `….bak`. Those placed
//! until the session ends are noted in `state/placed.json`, so a session
//! that could not take them away (the app was stopped) does when its host
//! connects next.

use std::sync::Arc;

use shellwing_core::placing::{self, Placed, There};
use shellwing_core::snippets;
use shellwing_vault::ConfigFile;

use crate::{MobileError, MobileSession, Shellwing};

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ConfigFileRow {
    pub id: String,
    pub name: String,
    pub category: String,
    /// Where it goes on a host, `~/` for the home folder.
    pub path: String,
    /// Its permissions there: 0o600.
    pub mode: u32,
    pub size: u64,
    pub binary: bool,
    /// Placed to stay by default, rather than until the session ends.
    pub stays: bool,
    pub scope: String,
}

/// What is at a path on the host, before a file goes there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Found {
    Nothing,
    /// The same bytes as the file's.
    Same,
    Other,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Looked {
    /// The path in full.
    pub full: String,
    pub found: Found,
    /// This session placed it there already, until it ends.
    pub ours: bool,
}

/// How a file goes on the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PlaceHow {
    /// Taken away when the session ends.
    Until,
    /// Left there.
    Stay,
    /// Over the file there until the session ends: that one is set aside,
    /// and comes back.
    UntilOver,
    /// Over it, the old one kept as `….bak`.
    Backup,
    /// Over it, the old one gone.
    Over,
}

/// A file placed until the session ends.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PlacedRow {
    pub name: String,
    pub path: String,
}

#[uniffi::export]
impl Shellwing {
    /// The config files offered in a session to `spec` (None: all of them).
    pub fn config_files(&self, spec: Option<String>) -> Vec<ConfigFileRow> {
        let Some(v) = self.vault.get() else {
            return vec![];
        };
        let (host, group) = spec.map(|s| self.host_and_group(&s)).unwrap_or_default();
        let mut out: Vec<ConfigFileRow> = v
            .config_files()
            .unwrap_or_default()
            .into_iter()
            .filter(|f| {
                host.is_none() && group.is_none()
                    || snippets::Scope::parse(&f.scope).fits(host.as_deref(), group.as_deref())
            })
            .map(|f| ConfigFileRow {
                id: f.id,
                name: f.name,
                category: f.category,
                path: f.path,
                mode: f.mode,
                size: f.size as u64,
                binary: f.binary,
                stays: f.stays,
                scope: f.scope,
            })
            .collect();
        out.sort_by(|a, b| {
            (a.category.to_lowercase(), a.name.to_lowercase())
                .cmp(&(b.category.to_lowercase(), b.name.to_lowercase()))
        });
        out
    }

    /// Saves a config file; `content` None keeps the one it has. Returns its id.
    pub fn save_config_file(
        &self,
        row: ConfigFileRow,
        content: Option<String>,
    ) -> Result<String, MobileError> {
        let path = row.path.trim().to_string();
        if row.name.trim().is_empty() {
            return Err(MobileError::Failed("A name is needed.".into()));
        }
        if !(path.starts_with('/') || path.starts_with("~/")) || path.ends_with('/') {
            return Err(MobileError::Failed(
                "Where it goes: a file's path from / or from ~/, such as ~/.kube/config.".into(),
            ));
        }
        let f = ConfigFile {
            id: row.id,
            name: row.name.trim().to_string(),
            category: row.category.trim().to_string(),
            path,
            mode: row.mode,
            scope: row.scope,
            size: 0,
            binary: false,
            stays: row.stays,
        };
        self.open_vault()?
            .save_config_file(&f, content.as_deref().map(str::as_bytes))
            .map_err(|e| MobileError::Failed(shellwing_core::environment::said_error(&e)))
    }

    /// The text of config file `id`, when it is text.
    pub fn config_file_text(&self, id: String) -> Result<Option<String>, MobileError> {
        let bytes = self
            .open_vault()?
            .config_file_content(&id)
            .map_err(MobileError::of)?
            .unwrap_or_default();
        Ok(shellwing_vault::is_text(&bytes).then(|| String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// What is at `path` on `session`'s host, for config file `id`.
    pub async fn look_to_place(
        &self,
        session: Arc<MobileSession>,
        id: String,
        path: String,
    ) -> Result<Looked, MobileError> {
        let content = self.content(&id)?;
        let sftp = session.sftp_open().await?;
        let (full, there) = placing::look(&sftp, &path, &content)
            .await
            .map_err(MobileError::Failed)?;
        let ours = session.placed_now().iter().any(|p| p.path == full);
        Ok(Looked {
            found: match there {
                There::Nothing => Found::Nothing,
                There::Same => Found::Same,
                There::Other => Found::Other,
            },
            full,
            ours,
        })
    }

    /// Places config file `id` at `full` (as [`Self::look_to_place`] said it)
    /// on `session`'s host, the way `how` says.
    pub async fn place(
        &self,
        session: Arc<MobileSession>,
        id: String,
        full: String,
        how: PlaceHow,
    ) -> Result<(), MobileError> {
        let file = self
            .open_vault()?
            .config_files()
            .map_err(MobileError::of)?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| MobileError::Failed("It is not in the vault any more.".into()))?;
        let content = self.content(&id)?;
        let (aside, until) = match how {
            PlaceHow::Until => (None, true),
            PlaceHow::Stay | PlaceHow::Over => (None, false),
            PlaceHow::UntilOver => (Some(placing::aside_of(&full)), true),
            PlaceHow::Backup => (Some(placing::backup_of(&full)), false),
        };
        // Placed over one of ours: what that one set aside still comes back.
        let before = session
            .placed_now()
            .into_iter()
            .find(|p| p.path == full)
            .and_then(|p| p.aside);
        let sftp = session.sftp_open().await?;
        placing::place(&sftp, &full, &content, file.mode, aside.as_deref())
            .await
            .map_err(MobileError::Failed)?;
        if until {
            let placed = Placed {
                id: file.id,
                name: file.name,
                path: full,
                aside: before.or(aside),
                sha256: placing::sha256_hex(&content),
            };
            self.note_placed(&session.spec(), &placed);
            session.add_placed(placed);
        }
        Ok(())
    }

    /// Leaves the file placed at `path` (None: all) on `session`'s host for
    /// good: what it set aside becomes its backup.
    pub async fn keep_placed(
        &self,
        session: Arc<MobileSession>,
        path: Option<String>,
    ) -> Result<(), MobileError> {
        let todo = session.placed_matching(path.as_deref());
        let sftp = session.sftp_open().await?;
        let mut kept = vec![];
        for p in todo {
            placing::keep(&sftp, &p.path, p.aside.as_deref())
                .await
                .map_err(MobileError::Failed)?;
            kept.push(p.path);
        }
        self.unnote_placed(&session.spec(), &kept);
        session.drop_placed(&kept);
        Ok(())
    }

    /// Takes the file placed at `path` (None: all) away from `session`'s
    /// host, and puts back what it set aside.
    pub async fn remove_placed(
        &self,
        session: Arc<MobileSession>,
        path: Option<String>,
    ) -> Result<(), MobileError> {
        let todo = session.placed_matching(path.as_deref());
        let sftp = session.sftp_open().await?;
        let mut gone = vec![];
        for p in todo {
            placing::unplace(&sftp, &p.path, p.aside.as_deref())
                .await
                .map_err(MobileError::Failed)?;
            gone.push(p.path);
        }
        self.unnote_placed(&session.spec(), &gone);
        session.drop_placed(&gone);
        Ok(())
    }

    /// Files left on `session`'s host from before (the app stopped before it
    /// could take them away): taken away if unchanged, else left. Says what
    /// it did, if anything.
    pub async fn tidy_placed(&self, session: Arc<MobileSession>) -> Option<String> {
        let spec = session.spec();
        let held: Vec<String> = session.placed_now().into_iter().map(|p| p.path).collect();
        let left: Vec<_> = self
            .store
            .placed()
            .into_iter()
            .filter(|p| p.host == spec && !held.contains(&p.path))
            .collect();
        if left.is_empty() {
            return None;
        }
        let sftp = session.sftp_open().await.ok()?;
        let (mut gone, mut kept) = (vec![], vec![]);
        for p in left {
            match placing::tidy(&sftp, &p).await {
                Ok(true) => gone.push(p.path),
                Ok(false) => kept.push(p.path),
                Err(_) => {}
            }
        }
        self.unnote_placed(&spec, &gone);
        self.unnote_placed(&spec, &kept);
        let mut said = vec![];
        if !gone.is_empty() {
            said.push(format!(
                "Took away {} config file(s) left here from before.",
                gone.len()
            ));
        }
        for p in &kept {
            said.push(format!(
                "Left {p} as it is: it changed since it was placed."
            ));
        }
        (!said.is_empty()).then(|| said.join(" "))
    }

    /// Keeps the file at `path` on `session`'s host in the vault, as a
    /// config file for every host. Returns its id.
    pub async fn keep_host_file(
        &self,
        session: Arc<MobileSession>,
        path: String,
    ) -> Result<String, MobileError> {
        let sftp = session.sftp_open().await?;
        let full = sftp.expand(&path).await.map_err(MobileError::of)?;
        let bytes = sftp
            .read_file(&full, shellwing_vault::limits::FILE)
            .await
            .map_err(MobileError::of)?
            .ok_or_else(|| MobileError::Failed(format!("nothing at {full}")))?;
        let name = shellwing_ssh::sftp::name_of(&path).to_string();
        let f = ConfigFile {
            id: String::new(),
            name,
            category: String::new(),
            path,
            mode: 0o600,
            scope: String::new(),
            size: 0,
            binary: false,
            stays: false,
        };
        self.open_vault()?
            .save_config_file(&f, Some(&bytes))
            .map_err(|e| MobileError::Failed(shellwing_core::environment::said_error(&e)))
    }
}

#[uniffi::export]
impl MobileSession {
    /// The config files placed on the host until the session ends.
    pub fn placed(&self) -> Vec<PlacedRow> {
        self.placed_now()
            .into_iter()
            .map(|p| PlacedRow {
                name: p.name,
                path: p.path,
            })
            .collect()
    }
}

impl Shellwing {
    fn content(&self, id: &str) -> Result<zeroize::Zeroizing<Vec<u8>>, MobileError> {
        self.open_vault()?
            .config_file_content(id)
            .map_err(MobileError::of)?
            .ok_or_else(|| MobileError::Failed("It is not in the vault any more.".into()))
    }

    /// Notes a file placed until its session ends, to take away later if the
    /// session cannot.
    fn note_placed(&self, host: &str, p: &Placed) {
        let mut all = self.store.placed();
        all.retain(|x| !(x.host == host && x.path == p.path));
        all.push(p.kept(host));
        let _ = self.store.save_placed(&all);
    }

    fn unnote_placed(&self, host: &str, paths: &[String]) {
        let mut all = self.store.placed();
        let before = all.len();
        all.retain(|x| !(x.host == host && paths.contains(&x.path)));
        if all.len() != before {
            let _ = self.store.save_placed(&all);
        }
    }
}

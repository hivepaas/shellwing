//! What the phone apps call: the hosts in `hosts.toml`, and sessions to them,
//! each with a terminal whose frames the app draws. The app holds one
//! [`Shellwing`] for its life; connecting and the terminals run on its own
//! threads, never the app's main one.
//!
//! The Swift side is generated from this crate by UniFFI
//! (`scripts/build_ios.sh`); names here are what Swift sees, in its case.

uniffi::setup_scaffolding!();

mod config_files;
mod files;
mod i18n;
mod library;
mod markdown;
mod md_edit;
mod session;
mod settings;
mod sync;
mod vault;

use shellwing_i18n::tr;
use std::sync::Arc;

use shellwing_config::{Auth, AuthMethod, Host, Hosts, Store};
use shellwing_core::secrets::VaultSlot;
use shellwing_core::session::{Opens, Session};

pub use config_files::{ConfigFileRow, Found, Looked, PlaceHow, PlacedRow};
pub use files::FileEntry;
pub use i18n::LanguageRow;
pub use library::{
    EnvRow, EnvShown, FileUnder, LibraryItem, LibraryPick, NoteRow, SnippetEdit, SnippetRow,
};
pub use markdown::{MdAlert, MdAlign, MdBlock, MdCell, MdKind, MdRow, MdSpan};
pub use md_edit::{MdEdit, MdImageLine, MdLines, MdRole, MdRun, MdTable, MdWrap};
pub use session::{
    FrameData, Key, LoadedEnvRow, MobileSession, Retrying, RowData, RunData, SessionListener,
    SessionStatus,
};
pub use settings::{HostKeyWay, KnownHostRow, OnDisconnectWay, SettingsRow, ThemeRow};
pub use sync::{Connected, JoinRequest, Pairing, Place, SyncListener, SyncStatus};
pub use vault::{KeyRow, PlatformKeys, SavedSecret, VaultState};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("{0}")]
    Failed(String),
}

impl MobileError {
    fn of(e: impl std::fmt::Display) -> Self {
        MobileError::Failed(e.to_string())
    }
}

/// Runs `work` on the app's runtime and waits for it there. What Swift
/// awaits is polled by Swift's own executor, outside any Tokio runtime, and
/// SSH needs one (its channels, SFTP, their timers): every `async fn` Swift
/// calls goes through here.
pub(crate) async fn on_runtime<T: Send + 'static>(
    rt: &tokio::runtime::Handle,
    work: impl std::future::Future<Output = Result<T, MobileError>> + Send + 'static,
) -> Result<T, MobileError> {
    rt.spawn(work).await.unwrap_or_else(|e| {
        let why = match e.try_into_panic() {
            Ok(p) => p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        Err(MobileError::Failed(format!("stopped: {why}")))
    })
}

/// A host as the list shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostRow {
    pub id: String,
    pub name: String,
    /// `user@address:port`, the parts it sets.
    pub detail: String,
    /// The host's color, else its group's, as `0xrrggbb`.
    pub color: Option<u32>,
    /// What opens it: its alias, else its name.
    pub spec: String,
    pub tags: Vec<String>,
    pub favorite: bool,
}

/// A group of hosts, in the file's order; `name` None: the hosts in no group.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostSection {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub hosts: Vec<HostRow>,
    /// The favorites, first: they are in their groups too.
    pub favorites: bool,
}

/// A group as its editor has it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GroupEdit {
    /// None: not in hosts.toml yet (new, or only named by its hosts).
    pub id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    /// The theme of its hosts, unless a host has its own.
    pub theme: Option<String>,
}

/// A host as its editor has it, as on the desktop; what the phone does not
/// edit (the session log) stays as `hosts.toml` has it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostEdit {
    /// None: a new host.
    pub id: Option<String>,
    pub name: String,
    /// A short name to open it by.
    pub alias: Option<String>,
    pub address: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    /// Hosts to go through first, outermost first.
    pub jump: Vec<String>,
    /// Ports forwarded while connected: `-L 8080:localhost:80`, `-R …`, `-D 1080`.
    pub forward: Vec<String>,
    pub auth: AuthWay,
    /// With `auth` Key: `vault:<id>` for a key in the vault, else a key file.
    pub key: Option<String>,
    /// The terminal's theme on this host, by id; None: the app's.
    pub theme: Option<String>,
    /// A name of [`host_colors`] or `#rrggbb`.
    pub color: Option<String>,
    pub favorite: bool,
    /// Not synced to other devices.
    pub local_only: bool,
    pub forward_agent: bool,
    /// Seconds between keepalives (0: none); None: the app's setting.
    pub keepalive: Option<u64>,
    /// None: the app's setting.
    pub host_key: Option<HostKeyWay>,
    /// Typed into the shell once it is ready, after every connect.
    pub run: Option<String>,
}

/// How a host is signed in to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AuthWay {
    /// As ssh does: keys in the vault, then a password.
    Auto,
    Key,
    Password,
    /// The ssh-agent's keys (on a desktop: none on a phone).
    Agent,
}

/// A color a host can be marked with.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostColor {
    pub name: String,
    pub rgb: u32,
}

/// The colors a host or a group can be marked with, by name.
#[uniffi::export]
pub fn host_colors() -> Vec<HostColor> {
    shellwing_config::HOST_COLORS
        .iter()
        .map(|(n, v)| HostColor {
            name: n.to_string(),
            rgb: *v,
        })
        .collect()
}

/// Why a port forward cannot be, if it cannot: `-L 8080:localhost:80`.
#[uniffi::export]
pub fn forward_problem(spec: String) -> Option<String> {
    spec.trim()
        .parse::<shellwing_ssh::Forward>()
        .err()
        .map(|e| e.to_string())
}

#[derive(uniffi::Object)]
pub struct Shellwing {
    store: Store,
    rt: tokio::runtime::Runtime,
    /// Saved passwords and SSH keys: `vault.db` beside hosts.toml, its key
    /// in the platform's keychain.
    vault: VaultSlot,
    sync: sync::Syncing,
}

#[uniffi::export]
impl Shellwing {
    /// Keeps the configuration in `config_dir` (the app's own folder),
    /// making it if need be; the vault's key in `keys`.
    #[uniffi::constructor]
    pub fn new(config_dir: String, keys: Arc<dyn PlatformKeys>) -> Result<Arc<Self>, MobileError> {
        std::fs::create_dir_all(&config_dir).map_err(MobileError::of)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("shellwing-net")
            .build()
            .map_err(MobileError::of)?;
        let vault = VaultSlot::open_with(
            std::path::Path::new(&config_dir).join("vault.db"),
            vault::Keys(keys),
        );
        let store = Store::new(config_dir);
        // The app's own choice, else the phone's language.
        let settings = store.settings().unwrap_or_default();
        shellwing_i18n::set_language(settings.general.language.as_deref());
        // Hosts written by hand have no id yet; editing one finds it by it.
        store.assign_host_ids().map_err(MobileError::of)?;
        let sync = sync::Syncing::start(vault.clone(), store.clone(), rt.handle());
        Ok(Arc::new(Shellwing {
            store,
            rt,
            vault,
            sync,
        }))
    }

    /// The hosts by group, as the desktop's list shows them: each group in
    /// the file's order, then the hosts in none.
    pub fn sections(&self) -> Result<Vec<HostSection>, MobileError> {
        Ok(sections(&self.hosts()?))
    }

    /// The host with `id`, to edit.
    pub fn host(&self, id: String) -> Option<HostEdit> {
        let hosts = self.hosts().ok()?;
        let h = hosts.hosts.iter().find(|h| h.id.as_deref() == Some(&id))?;
        Some(HostEdit {
            id: h.id.clone(),
            name: h.name.clone(),
            alias: h.alias.clone(),
            address: h.address.clone(),
            port: h.port,
            user: h.user.clone(),
            group: h.group.clone(),
            tags: h.tags.clone(),
            jump: h.jump.clone(),
            forward: h.forward.clone(),
            auth: match h.auth.method {
                AuthMethod::Auto => AuthWay::Auto,
                AuthMethod::Key => AuthWay::Key,
                AuthMethod::Password => AuthWay::Password,
                AuthMethod::Agent => AuthWay::Agent,
            },
            key: h.auth.key.clone(),
            theme: h.theme.clone(),
            color: h.color.clone(),
            favorite: h.favorite,
            local_only: h.local_only,
            forward_agent: h.forward_agent,
            keepalive: h.keepalive,
            host_key: h.host_key.map(Into::into),
            run: h.run.clone(),
        })
    }

    /// The hosts another can jump through: all but `except` (its name).
    pub fn jump_choices(&self, except: Option<String>) -> Vec<String> {
        self.hosts()
            .map(|h| h.hosts)
            .unwrap_or_default()
            .into_iter()
            .map(|h| h.name)
            .filter(|n| except.as_deref() != Some(n.as_str()))
            .collect()
    }

    /// The groups hosts can go in.
    pub fn groups(&self) -> Vec<String> {
        let Ok(hosts) = self.hosts() else {
            return vec![];
        };
        let mut names: Vec<String> = hosts.groups.iter().map(|g| g.name.clone()).collect();
        for h in &hosts.hosts {
            if let Some(g) = &h.group
                && !names.contains(g)
            {
                names.push(g.clone());
            }
        }
        names
    }

    /// Writes `edit` into `hosts.toml`, over the host with its id or as a new
    /// one; returns its id.
    pub fn save_host(&self, edit: HostEdit) -> Result<String, MobileError> {
        let hosts = self.hosts()?;
        let host = checked(edit, &hosts).map_err(MobileError::Failed)?;
        self.store.save_host(&host).map_err(MobileError::of)
    }

    /// The group named `name`, to edit.
    pub fn group(&self, name: String) -> GroupEdit {
        let hosts = self.hosts().unwrap_or_default();
        match hosts.groups.into_iter().find(|g| g.name == name) {
            Some(g) => GroupEdit {
                id: g.id,
                name: g.name,
                color: g.color,
                theme: g.theme,
            },
            None => GroupEdit {
                id: None,
                name,
                color: None,
                theme: None,
            },
        }
    }

    /// Saves a group; renamed (from `old_name`), its hosts move with it.
    pub fn save_group(
        &self,
        edit: GroupEdit,
        old_name: Option<String>,
    ) -> Result<String, MobileError> {
        let name = edit.name.trim().to_string();
        if name.is_empty() {
            return Err(MobileError::Failed(tr!("A name is needed.")));
        }
        let hosts = self.hosts()?;
        let mine = |g: &shellwing_config::Group| match &edit.id {
            Some(id) => g.id.as_ref() == Some(id),
            None => old_name.as_deref() == Some(g.name.as_str()),
        };
        if hosts.groups.iter().any(|g| !mine(g) && g.name == name) {
            return Err(MobileError::Failed(tr!("Another group has this name.")));
        }
        let clean = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let group = shellwing_config::Group {
            id: edit.id,
            name,
            color: clean(edit.color),
            theme: clean(edit.theme),
        };
        self.store
            .save_group(&group, old_name.as_deref())
            .map_err(MobileError::of)
    }

    /// Deletes the group named `name`; its hosts stay, in no group.
    pub fn delete_group(&self, name: String) -> Result<(), MobileError> {
        // One only named by its hosts has no id: they leave it all the same.
        let id = self.group(name.clone()).id.unwrap_or_default();
        self.store.delete_group(&id, &name).map_err(MobileError::of)
    }

    pub fn delete_host(&self, id: String) -> Result<(), MobileError> {
        self.store.delete_host(&id).map_err(MobileError::of)
    }

    /// Opens a session to `spec` (a host's name or alias, or `user@host:port`).
    /// `listener` hears when there is a new frame to draw or the status moved.
    pub fn open(
        &self,
        spec: String,
        listener: Arc<dyn SessionListener>,
    ) -> Result<Arc<MobileSession>, MobileError> {
        let settings = self.store.settings().unwrap_or_default();
        let hosts = self.hosts().unwrap_or_default();
        let target = shellwing_core::target::resolve(&spec, &hosts, None);
        let session = Session::start(
            &spec,
            Opens::Host(Box::new(target)),
            &settings,
            self.rt.handle(),
            None,
            self.vault.clone(),
        )
        .map_err(MobileError::of)?;
        let name = shellwing_ssh::config::split(&spec)
            .map(|(_, n, _)| n)
            .unwrap_or_default();
        // The host's own theme, else its group's.
        let theme = hosts.find(&name).and_then(|h| {
            h.theme
                .clone()
                .or_else(|| hosts.group_of(h).and_then(|g| g.theme.clone()))
        });
        session.term.set_colors(self.colors(theme.as_deref()));
        Ok(MobileSession::new(
            session,
            listener,
            self.rt.handle(),
            self.store.clone(),
        ))
    }
}

impl Shellwing {
    fn hosts(&self) -> Result<Hosts, MobileError> {
        self.store.hosts().map_err(MobileError::of)
    }
}

/// `edit` as the host to write over the one with its id (or a new one), or
/// what is wrong with it, as the desktop's editor says.
fn checked(edit: HostEdit, hosts: &Hosts) -> Result<Host, String> {
    let clean = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let spaced = |s: &str| s.chars().any(char::is_whitespace);
    let address = edit.address.trim().to_string();
    let name = match edit.name.trim() {
        "" => address.clone(),
        n => n.to_string(),
    };
    let old = edit
        .id
        .as_ref()
        .and_then(|id| hosts.hosts.iter().find(|h| h.id.as_ref() == Some(id)));
    let me = |h: &Host| edit.id.is_some() && h.id == edit.id;
    let taken = |n: &str| {
        hosts
            .hosts
            .iter()
            .any(|h| !me(h) && (h.name == n || h.alias.as_deref() == Some(n)))
    };
    if address.is_empty() {
        return Err(tr!("Address: an address is needed."));
    }
    if spaced(&address) {
        return Err(tr!("Address: no spaces."));
    }
    if taken(&name) {
        return Err(tr!("Name: another host has this name."));
    }
    let alias = clean(edit.alias);
    if let Some(a) = &alias {
        if spaced(a) {
            return Err(tr!("Alias: no spaces."));
        }
        if taken(a) {
            return Err(tr!("Alias: another host uses this."));
        }
    }
    if edit.port == Some(0) {
        return Err(tr!("Port: 1 to 65535."));
    }
    let user = clean(edit.user);
    if user.as_deref().is_some_and(spaced) {
        return Err(tr!("User: no spaces."));
    }
    let key = clean(edit.key);
    if edit.auth == AuthWay::Key && key.is_none() {
        return Err(tr!("Key: choose a key."));
    }
    if edit.keepalive.is_some_and(|k| k > 3600) {
        return Err(tr!("Keepalive: seconds, 0 to 3600 (0: none)."));
    }
    if edit.jump.contains(&name) {
        return Err(tr!("Jump: a host cannot jump through itself."));
    }
    for f in &edit.forward {
        if let Some(e) = forward_problem(f.clone()) {
            return Err(tr!("Forward {forward}: {e}", forward = f.as_str(), e = e));
        }
    }
    let mut host = old.cloned().unwrap_or_else(|| new_host(&address));
    host.id = edit.id;
    host.name = name;
    host.alias = alias;
    host.address = address;
    host.port = edit.port;
    host.user = user;
    host.group = clean(edit.group);
    host.tags = edit
        .tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    host.jump = edit.jump;
    host.forward = edit
        .forward
        .into_iter()
        .map(|f| f.trim().to_string())
        .collect();
    host.auth = Auth {
        method: match edit.auth {
            AuthWay::Auto => AuthMethod::Auto,
            AuthWay::Key => AuthMethod::Key,
            AuthWay::Password => AuthMethod::Password,
            AuthWay::Agent => AuthMethod::Agent,
        },
        key: if edit.auth == AuthWay::Key { key } else { None },
    };
    host.theme = clean(edit.theme);
    host.color = clean(edit.color);
    host.favorite = edit.favorite;
    host.local_only = edit.local_only;
    host.forward_agent = edit.forward_agent;
    host.keepalive = edit.keepalive;
    host.host_key = edit.host_key.map(Into::into);
    host.run = clean(edit.run);
    Ok(host)
}

/// A host with nothing set but its address.
fn new_host(address: &str) -> Host {
    Host {
        id: None,
        name: address.to_string(),
        alias: None,
        address: address.to_string(),
        port: None,
        user: None,
        group: None,
        tags: vec![],
        jump: vec![],
        forward: vec![],
        auth: Auth::default(),
        theme: None,
        local_only: false,
        forward_agent: false,
        keepalive: None,
        host_key: None,
        color: None,
        favorite: false,
        run: None,
        log: None,
    }
}

fn sections(hosts: &Hosts) -> Vec<HostSection> {
    let row = |h: &Host| {
        let group_color = hosts.group_of(h).and_then(|g| g.color_rgb());
        let mut detail = h.address.clone();
        if let Some(u) = &h.user {
            detail = format!("{u}@{detail}");
        }
        if let Some(p) = h.port.filter(|p| *p != 22) {
            detail = format!("{detail}:{p}");
        }
        HostRow {
            id: h.id.clone().unwrap_or_default(),
            name: h.name.clone(),
            detail,
            color: h.color_rgb().or(group_color),
            spec: h.alias.clone().unwrap_or_else(|| h.name.clone()),
            tags: h.tags.clone(),
            favorite: h.favorite,
        }
    };
    let mut names: Vec<String> = hosts.groups.iter().map(|g| g.name.clone()).collect();
    for h in &hosts.hosts {
        if let Some(g) = &h.group
            && !names.contains(g)
        {
            names.push(g.clone());
        }
    }
    let mut out: Vec<HostSection> = names
        .into_iter()
        .map(|name| HostSection {
            color: hosts
                .groups
                .iter()
                .find(|g| g.name == name)
                .and_then(|g| g.color_rgb()),
            hosts: hosts
                .hosts
                .iter()
                .filter(|h| h.group.as_deref() == Some(name.as_str()))
                .map(row)
                .collect(),
            name: Some(name),
            favorites: false,
        })
        .collect();
    let favorites: Vec<HostRow> = hosts.hosts.iter().filter(|h| h.favorite).map(row).collect();
    if !favorites.is_empty() {
        out.insert(
            0,
            HostSection {
                name: Some("Favorites".into()),
                color: None,
                hosts: favorites,
                favorites: true,
            },
        );
    }
    let loose: Vec<HostRow> = hosts
        .hosts
        .iter()
        .filter(|h| h.group.is_none())
        .map(row)
        .collect();
    if !loose.is_empty() {
        out.push(HostSection {
            name: None,
            color: None,
            hosts: loose,
            favorites: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The platform's keychain, in memory.
    #[derive(Default)]
    struct MemKeys(std::sync::Mutex<Option<Vec<u8>>>);

    impl PlatformKeys for MemKeys {
        fn load(&self) -> Result<Option<Vec<u8>>, MobileError> {
            Ok(self.0.lock().map_err(MobileError::of)?.clone())
        }
        fn save(&self, key: Vec<u8>) -> Result<(), MobileError> {
            *self.0.lock().map_err(MobileError::of)? = Some(key);
            Ok(())
        }
        fn forget(&self) -> Result<(), MobileError> {
            *self.0.lock().map_err(MobileError::of)? = None;
            Ok(())
        }
    }

    /// An environment's values show in its text when asked for, quoted
    /// where a line would change them, and hide again unless changed; one
    /// of several lines stays kept.
    #[test]
    fn values_show_in_the_text_and_hide_again() {
        let (dir, app) = store_with("");
        let id = app
            .save_environment(
                String::new(),
                "Prod".into(),
                String::new(),
                "API=k-1\nPAD=\" a \"\nQUOTED='\"q\"'\n".into(),
            )
            .expect("env");
        let kept = |name: &str| (name.to_string(), None);
        app.open_vault()
            .expect("vault")
            .save_environment(
                &id,
                "Prod",
                "",
                &[
                    kept("API"),
                    kept("PAD"),
                    kept("QUOTED"),
                    ("PEM".into(), Some("a\nb".into())),
                ],
            )
            .expect("several lines");

        let text = "API=\nPAD=\nQUOTED=\n  export PEM=\nNEW=typed\n";
        let shown = app.env_text_shown(id.clone(), text.into()).expect("shown");
        assert_eq!(
            shown.text,
            "API=k-1\nPAD=\" a \"\nQUOTED=\"\"q\"\"\n  export PEM=\nNEW=typed\n"
        );
        assert_eq!(shown.not_shown, ["PEM"]);

        // Hidden again: only what was not changed.
        let edited = shown.text.replace("API=k-1", "API=k-2");
        let hidden = app.env_text_hidden(id.clone(), edited);
        assert_eq!(hidden, "API=k-2\nPAD=\nQUOTED=\n  export PEM=\nNEW=typed\n");

        app.save_environment(id.clone(), "Prod".into(), String::new(), hidden)
            .expect("saved");
        let mut values: Vec<(String, String)> = app
            .open_vault()
            .expect("vault")
            .environment_values(&id)
            .expect("values")
            .expect("there")
            .into_iter()
            .map(|(n, v)| (n, v.to_string()))
            .collect();
        values.sort();
        let want = [
            ("API", "k-2"),
            ("NEW", "typed"),
            ("PAD", " a "),
            ("PEM", "a\nb"),
            ("QUOTED", "\"q\""),
        ]
        .map(|(n, v)| (n.to_string(), v.to_string()));
        assert_eq!(values, want);

        // A new one keeps nothing to show.
        let new = app
            .env_text_shown(String::new(), "A=\n".into())
            .expect("new");
        assert_eq!(new.text, "A=\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    struct Quiet;

    impl SessionListener for Quiet {
        fn changed(&self) {}
    }

    /// The phone's terminal size reaches the shell on the host, as the
    /// desktop pane's does: the core asks for the PTY at it.
    #[test]
    fn the_terminals_size_is_the_shells() {
        let (dir, app) = store_with("");
        let s = app
            .open("me@127.0.0.1:1".into(), Arc::new(Quiet))
            .expect("open");
        assert_eq!(s.shell_size(), None, "none until the screen is laid out");
        s.resize(46, 20, 9, 18);
        assert_eq!(s.shell_size(), Some((46, 20)));
        drop(s);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn store_with(text: &str) -> (std::path::PathBuf, Arc<Shellwing>) {
        // One folder per test: they run at once.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("shellwing-mobile-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("hosts.toml"), text).expect("hosts");
        // What the tests read is the English, whatever the system speaks.
        std::fs::write(dir.join("config.toml"), "[general]\nlanguage = \"en\"\n").expect("config");
        let app = Shellwing::new(
            dir.to_string_lossy().into_owned(),
            Arc::new(MemKeys::default()),
        )
        .expect("app");
        (dir, app)
    }

    #[test]
    fn hosts_come_by_group_then_the_rest() {
        let (dir, app) = store_with(
            "[[group]]\nname = \"Production\"\ncolor = \"red\"\n\n\
             [[host]]\nname = \"web-1\"\naddress = \"10.0.1.11\"\nuser = \"deploy\"\ngroup = \"Production\"\n\n\
             [[host]]\nname = \"pi\"\naddress = \"pi.local\"\nport = 2222\n",
        );
        let s = app.sections().expect("sections");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name.as_deref(), Some("Production"));
        assert!(s[0].color.is_some());
        assert_eq!(s[0].hosts[0].detail, "deploy@10.0.1.11");
        assert_eq!(s[0].hosts[0].color, s[0].color);
        assert_eq!(s[1].name, None);
        assert_eq!(s[1].hosts[0].detail, "pi.local:2222");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_host_saved_keeps_what_the_phone_does_not_edit() {
        let (dir, app) = store_with(
            "[[host]]\nname = \"db\"\naddress = \"10.0.2.5\"\njump = [\"bastion\"]\nlog = true\n",
        );
        let id = app.sections().expect("sections")[0].hosts[0].id.clone();
        let mut edit = app.host(id.clone()).expect("host");
        edit.user = Some("postgres".into());
        edit.port = Some(2201);
        app.save_host(edit).expect("saved");
        let text = std::fs::read_to_string(dir.join("hosts.toml")).expect("read");
        assert!(text.contains("bastion"), "{text}");
        assert!(text.contains("postgres") && text.contains("2201"), "{text}");
        assert!(text.contains("log = true"), "{text}");

        let blank = HostEdit {
            id: None,
            name: String::new(),
            alias: None,
            address: "new.example".into(),
            port: None,
            user: None,
            group: Some("Staging".into()),
            tags: vec![],
            jump: vec![],
            forward: vec![],
            auth: AuthWay::Auto,
            key: None,
            theme: None,
            color: None,
            favorite: false,
            local_only: false,
            forward_agent: false,
            keepalive: None,
            host_key: None,
            run: None,
        };
        let refused = |e: HostEdit| app.save_host(e).expect_err("saved").to_string();
        assert!(
            refused(HostEdit {
                name: "db".into(),
                ..blank.clone()
            })
            .starts_with("Name")
        );
        assert!(
            refused(HostEdit {
                auth: AuthWay::Key,
                ..blank.clone()
            })
            .starts_with("Key")
        );
        assert!(
            refused(HostEdit {
                forward: vec!["-L x".into()],
                ..blank.clone()
            })
            .starts_with("Forward")
        );
        assert!(forward_problem("-L 8080:localhost:80".into()).is_none());
        let new = app
            .save_host(HostEdit {
                auth: AuthWay::Key,
                key: Some("vault:k1".into()),
                theme: Some("nord".into()),
                tags: vec![" web ".into(), String::new()],
                host_key: Some(HostKeyWay::Strict),
                ..blank
            })
            .expect("new");
        assert!(!new.is_empty());
        let back = app.host(new.clone()).expect("new host");
        assert_eq!(back.name, "new.example");
        assert_eq!(
            (back.key.as_deref(), back.tags.as_slice()),
            (Some("vault:k1"), &["web".to_string()][..])
        );
        assert_eq!(
            (back.theme.as_deref(), back.host_key),
            (Some("nord"), Some(HostKeyWay::Strict))
        );
        assert_eq!(app.groups(), ["Staging"]);
        app.delete_host(new.clone()).expect("deleted");
        assert!(app.host(new).is_none(), "still there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Polls `f` on this thread, as Swift's executor polls what it awaits:
    /// no Tokio runtime here.
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, Wake, Waker};
        struct Unpark(std::thread::Thread);
        impl Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut f = std::pin::pin!(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::park();
        }
    }

    #[test]
    fn what_swift_awaits_runs_on_the_apps_runtime() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "none here, as in Swift"
        );
        // A timer and a task: what SFTP's calls need, and what failed.
        let done = block_on(on_runtime(rt.handle(), async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            Ok(tokio::spawn(async { 7 }).await.expect("task"))
        }));
        assert_eq!(done.expect("ran"), 7);
        // A panic there is an error here, not a crash.
        let failed: Result<(), MobileError> = block_on(on_runtime(rt.handle(), async {
            panic!("boom");
        }));
        assert!(matches!(failed, Err(MobileError::Failed(m)) if m.contains("boom")));
    }

    /// Every async call Swift makes runs on the app's runtime: one that did
    /// not failed on a phone with "there is no reactor running".
    #[test]
    fn every_async_export_goes_through_on_runtime() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut seen = 0;
        for entry in std::fs::read_dir(&src).expect("src") {
            let path = entry.expect("entry").path();
            let text = std::fs::read_to_string(&path).expect("source");
            // The code above the file's tests.
            let code = text.split("\n#[cfg(test)]\nmod ").next().unwrap_or("");
            let lines: Vec<&str> = code.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.trim_start().starts_with("pub async fn") {
                    seen += 1;
                    let body = lines[i..(i + 14).min(lines.len())].join("\n");
                    assert!(
                        body.contains("on_runtime("),
                        "{}: {} does not run on the app's runtime",
                        path.display(),
                        l.trim()
                    );
                }
            }
        }
        assert!(seen >= 14, "the async calls: {seen}");
    }

    #[test]
    fn library_items_become_snippets() {
        let (dir, app) = store_with("");
        let items = app.library_items();
        assert!(items.len() >= 90);
        assert!(crate::library::library_tasks().contains(&"Backup".to_string()));
        let dump = items
            .iter()
            .find(|i| i.id == "backup/pg_dump")
            .expect("pg_dump");
        assert_eq!(dump.password.as_deref(), Some("PGPASSWORD"));
        let pick = |env: Option<String>| LibraryPick {
            index: dump.index,
            options: vec![],
            container: true,
            sudo: None,
            env,
        };
        assert!(
            crate::library::library_command(pick(None))
                .starts_with("docker exec -it -e PGPASSWORD {{container}} pg_dump")
        );
        // The only environment with its variable is found for it.
        let env = app
            .save_environment(
                String::new(),
                "db".into(),
                String::new(),
                "PGPASSWORD=x\n".into(),
            )
            .expect("env");
        assert_eq!(app.library_env(dump.index), Some(env.clone()));
        assert_eq!(
            app.add_from_library(vec![pick(None)], FileUnder::Both)
                .expect("added"),
            1
        );
        assert_eq!(
            app.add_from_library(vec![pick(Some(String::new()))], FileUnder::Task)
                .expect("added"),
            1
        );
        let mine = app.snippets(None);
        let both = mine
            .iter()
            .find(|s| s.category == "PostgreSQL · Backup")
            .expect("both");
        assert_eq!(both.needs, [env]);
        let task = mine.iter().find(|s| s.category == "Backup").expect("task");
        assert!(task.name.starts_with("PostgreSQL: ") && task.needs.is_empty());
        let again = app.library_items();
        assert_eq!(again[dump.index as usize].added, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn favorites_come_first_and_groups_are_edited() {
        let (dir, app) = store_with(
            "[[host]]\nname = \"a\"\naddress = \"a.example\"\ngroup = \"Lab\"\nfavorite = true\ntags = [\"pi\"]\n",
        );
        let s = app.sections().expect("sections");
        assert!(s[0].favorites && s[0].hosts[0].tags == ["pi"]);
        assert_eq!(s[1].name.as_deref(), Some("Lab"));
        let mut g = app.group("Lab".into());
        assert!(g.id.is_none(), "only named by its host");
        g.name = "Home lab".into();
        g.theme = Some("nord".into());
        app.save_group(g, Some("Lab".into())).expect("saved");
        assert_eq!(app.groups(), ["Home lab"]);
        assert_eq!(app.group("Home lab".into()).theme.as_deref(), Some("nord"));
        // A new one, with no hosts yet: listed all the same.
        let empty = GroupEdit {
            id: None,
            name: "Staging".into(),
            color: Some("sky".into()),
            theme: None,
        };
        app.save_group(empty.clone(), None).expect("new group");
        assert!(app.save_group(empty, None).is_err(), "a second Staging");
        let s = app.sections().expect("sections");
        assert!(
            s.iter()
                .any(|x| x.name.as_deref() == Some("Staging") && x.hosts.is_empty())
        );
        app.delete_group("Staging".into()).expect("deleted");
        app.delete_group("Home lab".into()).expect("deleted");
        assert!(app.groups().is_empty());
        assert_eq!(
            app.sections().expect("sections").len(),
            2,
            "favorites, then no group"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_are_read_and_written_as_the_desktop_does() {
        let (dir, app) = store_with("");
        let s = app.settings();
        assert_eq!(
            (s.scrollback, s.keepalive, s.host_key),
            (10_000, 30, HostKeyWay::Ask)
        );
        let themes = app.themes();
        assert!(themes.iter().any(|t| t.id == s.theme), "{}", s.theme);
        let other = themes
            .iter()
            .find(|t| t.id != s.theme)
            .expect("another")
            .id
            .clone();
        app.set_theme(other.clone()).expect("theme");
        app.set_scrollback(5).expect("scrollback");
        app.set_host_key(HostKeyWay::AcceptNew).expect("host key");
        app.set_keepalive(15).expect("keepalive");
        assert_eq!(
            s.on_disconnect,
            OnDisconnectWay::Reconnect,
            "the desktop's default"
        );
        app.set_on_disconnect(OnDisconnectWay::Ask)
            .expect("on disconnect");
        let s = app.settings();
        assert_eq!((s.theme, s.scrollback, s.keepalive), (other, 100, 15));
        assert_eq!(s.host_key, HostKeyWay::AcceptNew);
        assert_eq!(s.on_disconnect, OnDisconnectWay::Ask);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_vault_keeps_its_key_in_the_platform_keychain_and_keys_in_it() {
        let (dir, app) = store_with("");
        assert!(app.vault_state().ready, "{:?}", app.vault_state());
        assert!(
            app.recovery_key()
                .is_some_and(|k| k.split('-').count() == 13)
        );
        assert!(app.generate_key("  ".into()).is_err());
        let key = app.generate_key("phone".into()).expect("key");
        assert_eq!(key.kind, "ED25519");
        assert!(key.public.starts_with("ssh-ed25519 "), "{}", key.public);
        assert_eq!(app.keys(), std::slice::from_ref(&key));
        app.forget(key.id).expect("deleted");
        assert!(app.keys().is_empty());
        assert!(app.saved_secrets().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Waits up to 20 s for `done`.
    fn within(what: &str, mut done: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !done() {
            assert!(start.elapsed().as_secs() < 20, "waited for {what}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn a_phone_joins_with_the_recovery_key_and_gets_the_hosts() {
        let base =
            std::env::temp_dir().join(format!("shellwing-mobile-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (shared, a_dir, b_dir) = (base.join("drive"), base.join("a"), base.join("b"));
        for d in [&shared, &a_dir, &b_dir] {
            std::fs::create_dir_all(d).expect("dir");
        }
        std::fs::write(
            a_dir.join("hosts.toml"),
            "[[host]]\nname = \"web-1\"\naddress = \"10.0.1.11\"\n",
        )
        .expect("hosts");
        let open = |d: &std::path::Path| {
            Shellwing::new(
                d.to_string_lossy().into_owned(),
                Arc::new(MemKeys::default()),
            )
            .expect("app")
        };
        let (a, b) = (open(&a_dir), open(&b_dir));
        let place = Place::Folder {
            path: shared.to_string_lossy().into_owned(),
        };

        let Connected::Started { recovery_key } =
            a.connect_sync(place.clone(), None).expect("started")
        else {
            panic!("the empty folder was not started");
        };
        within("the first sync", || a.sync_status().last.is_some());
        assert!(a.sync_status().said.is_some());

        assert_eq!(
            b.connect_sync(place.clone(), None).expect("found"),
            Connected::Other { passphrase: false }
        );
        b.join_with_recovery_key(place, recovery_key)
            .expect("joined");
        within("web-1 on the phone", || {
            b.sections()
                .is_ok_and(|s| s.iter().flat_map(|s| &s.hosts).any(|h| h.name == "web-1"))
        });
        assert!(b.sync_status().place.is_some());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn snippets_environments_and_notes_from_the_vault() {
        let (dir, app) = store_with(
            "[[group]]\nname = \"Production\"\n\n\
             [[host]]\nname = \"web-1\"\naddress = \"10.0.1.11\"\ngroup = \"Production\"\n\n\
             [[host]]\nname = \"pi\"\naddress = \"pi.local\"\n",
        );
        let id = app
            .save_snippet(SnippetEdit {
                id: String::new(),
                name: "Restart".into(),
                category: "Kubernetes".into(),
                command: "kubectl rollout restart deploy/{{name}} -n {{ns}} # on {{host}}".into(),
                enter: true,
                scope: "group:Production".into(),
                needs: vec![],
            })
            .expect("snippet");
        let all = app.snippets(None);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].asked, ["name", "ns"]);
        assert_eq!(app.snippets(Some("web-1".into())).len(), 1);
        assert!(
            app.snippets(Some("pi".into())).is_empty(),
            "offered outside its group"
        );

        let env = app
            .save_environment(
                String::new(),
                "Staging".into(),
                String::new(),
                "export API=x\nTOKEN='t'\n".into(),
            )
            .expect("env");
        assert_eq!(app.environments()[0].names, ["API", "TOKEN"]);
        let mut edit = SnippetEdit {
            id: id.clone(),
            name: all[0].name.clone(),
            category: all[0].category.clone(),
            command: all[0].command.clone(),
            enter: true,
            scope: all[0].scope.clone(),
            needs: vec![env.clone()],
        };
        app.save_snippet(edit.clone()).expect("needs");
        assert_eq!(app.snippets(None)[0].needs, [env]);
        edit.needs.clear();
        app.save_snippet(edit).expect("needs none");
        assert!(app.snippets(None)[0].needs.is_empty());
        assert!(
            app.save_environment(
                String::new(),
                "Bad".into(),
                String::new(),
                "not a line".into()
            )
            .is_err()
        );

        let note = app
            .save_note(
                String::new(),
                "Disk".into(),
                "Ops".into(),
                true,
                vec![],
                false,
                Some("# df -h".into()),
            )
            .expect("note");
        assert_eq!(app.notes(None)[0].title, "Disk");
        assert!(app.notes(None)[0].markdown);
        assert_eq!(app.note_text(note.clone()).expect("text"), "# df -h");
        let pi = app
            .save_note(
                String::new(),
                "The Pi".into(),
                "Ops".into(),
                false,
                vec!["pi".into()],
                true,
                Some("secret".into()),
            )
            .expect("note");
        let here = app.notes(Some("pi".into()));
        assert!(here[0].here && here[0].sensitive && here[0].hosts == ["pi"]);
        assert_eq!(app.notes(Some("web-1".into()))[0].title, "Disk");
        // Saved again unshown: its text stays.
        app.save_note(
            pi.clone(),
            "Pi".into(),
            "Ops".into(),
            false,
            vec![],
            true,
            None,
        )
        .expect("saved");
        assert_eq!(app.note_text(pi).expect("text"), "secret");

        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99,
            0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        assert!(app.add_note_image(b"not an image".to_vec()).is_err());
        let line = app.add_note_image(PNG.to_vec()).expect("image");
        let id = line
            .strip_prefix("![](attachment:")
            .and_then(|l| l.strip_suffix(')'))
            .expect("a line");
        assert!(
            app.note_image(id.into())
                .is_some_and(|b| b.starts_with(b"\x89PNG"))
        );

        let row = |path: &str| ConfigFileRow {
            id: String::new(),
            name: "kube prod".into(),
            category: "Kubernetes".into(),
            path: path.into(),
            mode: 0o600,
            size: 0,
            binary: false,
            stays: true,
            scope: "host:web-1".into(),
        };
        assert!(
            app.save_config_file(row("kube/config"), Some("x".into()))
                .is_err(),
            "a relative path"
        );
        let file = app
            .save_config_file(row("~/.kube/config"), Some("apiVersion: v1\n".into()))
            .expect("file");
        assert!(app.config_files(Some("web-1".into()))[0].stays);
        assert!(
            app.config_files(Some("pi".into())).is_empty(),
            "offered on another host"
        );
        assert_eq!(
            app.config_file_text(file).expect("text").as_deref(),
            Some("apiVersion: v1\n")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

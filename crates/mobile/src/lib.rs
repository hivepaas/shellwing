//! What the phone apps call: the hosts in `hosts.toml`, and sessions to them,
//! each with a terminal whose frames the app draws. The app holds one
//! [`Shellwing`] for its life; connecting and the terminals run on its own
//! threads, never the app's main one.
//!
//! The Swift side is generated from this crate by UniFFI
//! (`scripts/build_ios.sh`); names here are what Swift sees, in its case.

uniffi::setup_scaffolding!();

mod session;

use std::sync::Arc;

use shellwing_config::{Auth, Host, Hosts, Store};
use shellwing_core::secrets::VaultSlot;
use shellwing_core::session::{Opens, Session};
use shellwing_term::{Colors, Rgb};

pub use session::{
    FrameData, Key, MobileSession, RowData, RunData, SessionListener, SessionStatus,
};

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
}

/// A group of hosts, in the file's order; `name` None: the hosts in no group.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostSection {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub hosts: Vec<HostRow>,
}

/// The fields the phone edits; the rest of a host (jumps, forwards, a theme)
/// stays as `hosts.toml` has it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HostEdit {
    /// None: a new host.
    pub id: Option<String>,
    pub name: String,
    pub address: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub group: Option<String>,
}

#[derive(uniffi::Object)]
pub struct Shellwing {
    store: Store,
    rt: tokio::runtime::Runtime,
    colors: Colors,
}

#[uniffi::export]
impl Shellwing {
    /// Keeps the configuration in `config_dir` (the app's own folder),
    /// making it if need be.
    #[uniffi::constructor]
    pub fn new(config_dir: String) -> Result<Arc<Self>, MobileError> {
        std::fs::create_dir_all(&config_dir).map_err(MobileError::of)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("shellwing-net")
            .build()
            .map_err(MobileError::of)?;
        let store = Store::new(config_dir);
        // Hosts written by hand have no id yet; editing one finds it by it.
        store.assign_host_ids().map_err(MobileError::of)?;
        Ok(Arc::new(Shellwing {
            store,
            rt,
            colors: dusk(),
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
            address: h.address.clone(),
            port: h.port,
            user: h.user.clone(),
            group: h.group.clone(),
        })
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
        let name = edit.name.trim().to_string();
        let address = edit.address.trim().to_string();
        if address.is_empty() {
            return Err(MobileError::Failed("An address is needed.".into()));
        }
        let old = edit.id.as_ref().and_then(|id| {
            self.hosts()
                .ok()?
                .hosts
                .into_iter()
                .find(|h| h.id.as_ref() == Some(id))
        });
        let mut host = old.unwrap_or_else(|| new_host(&address));
        host.id = edit.id;
        host.name = if name.is_empty() {
            address.clone()
        } else {
            name
        };
        host.address = address;
        host.port = edit.port;
        host.user = edit
            .user
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());
        host.group = edit
            .group
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty());
        self.store.save_host(&host).map_err(MobileError::of)
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
            VaultSlot::none(),
        )
        .map_err(MobileError::of)?;
        session.term.set_colors(self.colors.clone());
        Ok(MobileSession::new(session, listener, self.rt.handle()))
    }
}

impl Shellwing {
    fn hosts(&self) -> Result<Hosts, MobileError> {
        self.store.hosts().map_err(MobileError::of)
    }
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
        })
        .collect();
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
        });
    }
    out
}

/// The terminal's colors: Dusk, the desktop's default dark theme.
fn dusk() -> Colors {
    let themes = shellwing_config::Themes::builtin();
    let rgb = |c: shellwing_config::Color| Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    };
    match themes.find(shellwing_config::DEFAULT_THEME) {
        Some(t) => Colors {
            foreground: rgb(t.foreground),
            background: rgb(t.background),
            cursor: rgb(t.cursor),
            palette: t.palette.map(rgb),
        },
        None => Colors {
            foreground: Rgb {
                r: 0xe2,
                g: 0xe8,
                b: 0xf0,
            },
            background: Rgb {
                r: 0x27,
                g: 0x2e,
                b: 0x38,
            },
            cursor: Rgb {
                r: 0x4c,
                g: 0xc4,
                b: 0xf9,
            },
            palette: [Rgb::default(); 16],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(text: &str) -> (std::path::PathBuf, Arc<Shellwing>) {
        let dir = std::env::temp_dir().join(format!(
            "shellwing-mobile-{}-{}",
            std::process::id(),
            text.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("hosts.toml"), text).expect("hosts");
        let app = Shellwing::new(dir.to_string_lossy().into_owned()).expect("app");
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
        let (dir, app) =
            store_with("[[host]]\nname = \"db\"\naddress = \"10.0.2.5\"\njump = [\"bastion\"]\n");
        let id = app.sections().expect("sections")[0].hosts[0].id.clone();
        let mut edit = app.host(id.clone()).expect("host");
        edit.user = Some("postgres".into());
        edit.port = Some(2201);
        app.save_host(edit).expect("saved");
        let text = std::fs::read_to_string(dir.join("hosts.toml")).expect("read");
        assert!(text.contains("bastion"), "{text}");
        assert!(text.contains("postgres") && text.contains("2201"), "{text}");

        let new = app
            .save_host(HostEdit {
                id: None,
                name: String::new(),
                address: "new.example".into(),
                port: None,
                user: None,
                group: Some("Staging".into()),
            })
            .expect("new");
        assert!(!new.is_empty());
        assert_eq!(app.groups(), ["Staging"]);
        app.delete_host(new.clone()).expect("deleted");
        assert!(app.host(new).is_none(), "still there");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The collections of an organization the vault holds, as host lists show
//! them (docs/design/collections.md): on the desktop and the phones alike.

use shellwing_config::{Host, Hosts};
use shellwing_i18n::tr;
use shellwing_vault::{CollectionInfo, Space, Vault};

/// A collection, with its hosts once its key is held here.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionHosts {
    pub id: String,
    pub name: String,
    /// The organization it belongs to: its certificates sign in to its hosts.
    pub org_id: String,
    /// `connect`, `edit` or `manage`.
    pub permission: String,
    /// Its key is not held here yet: somebody holding it gives it.
    pub waiting: bool,
    pub hosts: Vec<Host>,
}

impl CollectionHosts {
    /// Whether the person may change its hosts.
    pub fn edits(&self) -> bool {
        matches!(self.permission.as_str(), "edit" | "manage")
    }
}

/// The collections the server served at the last sync, each with its hosts
/// if its key is held.
pub fn read(vault: &Vault) -> Vec<CollectionHosts> {
    vault
        .known_collections()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let waiting = vault.collection_versions(&c.id).is_empty();
            let hosts = if waiting {
                Vec::new()
            } else {
                hosts_of(vault, &c)
            };
            CollectionHosts {
                id: c.id,
                name: c.name,
                org_id: c.org_id,
                permission: c.permission,
                waiting,
                hosts,
            }
        })
        .collect()
}

/// A collection's hosts, read from the vault as hosts.toml's are: each marked
/// with the collection, in no group, and signing in with the organization's
/// certificates unless it says otherwise. Found by their ids, since names may
/// repeat across collections.
pub fn hosts_of(vault: &Vault, c: &CollectionInfo) -> Vec<Host> {
    let space = Space::Collection(c.id.clone());
    let (Ok(hosts), Ok(groups)) = (
        vault.entries_in(&space, "host"),
        vault.entries_in(&space, "group"),
    ) else {
        return Vec::new();
    };
    let Ok(parsed) = Hosts::from_synced(&hosts, &groups) else {
        return Vec::new();
    };
    parsed
        .hosts
        .into_iter()
        .map(|mut h| {
            h.collection = Some(c.id.clone());
            h.group = None;
            h.favorite = false;
            if h.org.is_none() {
                h.org = Some(c.org_id.clone());
            }
            h
        })
        .collect()
}

/// hosts.toml's hosts, then the collections': what names and connects.
pub fn merged(personal: &Hosts, collections: &[CollectionHosts]) -> Hosts {
    let mut hosts = personal.clone();
    for c in collections {
        hosts.hosts.extend(c.hosts.iter().cloned());
    }
    hosts
}

/// `c` if the person may change its hosts, else why not.
fn writable(c: &CollectionHosts) -> Result<(), String> {
    if c.edits() {
        Ok(())
    } else {
        Err(tr!(
            "You may connect to the hosts of {name}, not change them.",
            name = c.name.clone()
        ))
    }
}

/// Saves `host` in the collection, where it syncs to the others with
/// access; gives its id, which the host lists open it by. What only
/// hosts.toml has (a group, a favorite, this device only) is not kept.
pub fn save_host(vault: &Vault, c: &CollectionHosts, mut host: Host) -> Result<String, String> {
    writable(c)?;
    // The organization's certificates are the collection's own: not written
    // down unless another is chosen.
    if host.org.as_deref() == Some(c.org_id.as_str()) {
        host.org = None;
    }
    host.group = None;
    host.favorite = false;
    host.local_only = false;
    let id = host.id.clone().unwrap_or_else(shellwing_config::new_id);
    vault
        .update_in(
            &Space::Collection(c.id.clone()),
            &id,
            "host",
            &host.name,
            &host.sync_fields(),
        )
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Where a host's records are: its collection's space, or the personal one.
pub fn space_of(host: &Host) -> Space {
    match &host.collection {
        Some(c) => Space::Collection(c.clone()),
        None => Space::Personal,
    }
}

/// Copies the password saved for `target` in `from` to `to`, as a host
/// moves there: true when there was one to copy.
pub fn carry_password(
    vault: &Vault,
    from: &Space,
    to: &Space,
    target: &str,
) -> Result<bool, String> {
    let Some(password) = vault.password_in(from, target).map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    vault
        .set_password_in(to, target, &password)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Deletes the collection's host `id`, for everybody with access.
pub fn delete_host(vault: &Vault, c: &CollectionHosts, id: &str) -> Result<(), String> {
    writable(c)?;
    // The vault deletes by id whatever the space: only this collection's.
    if !c.hosts.iter().any(|h| h.id.as_deref() == Some(id)) {
        return Err(tr!("This host's collection is no longer synced here."));
    }
    vault.delete(id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::{Given, MemoryKeys};

    /// A vault holding version 1 of `col1`'s key, made for itself.
    fn vault(name: &str) -> (std::path::PathBuf, Vault) {
        let dir = std::env::temp_dir().join(format!(
            "shellwing-core-collections-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let v = Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("vault");
        let me = v.ensure_identity().expect("identity").public();
        let wraps = v
            .make_collection_key("col1", 1, std::slice::from_ref(&me))
            .expect("key");
        let given = Given {
            version: 1,
            wrap: wraps[0].clone(),
            wrapper: me.ed25519,
        };
        v.take_collection_keys("col1", &[given]).expect("take");
        (dir, v)
    }

    fn collection(permission: &str, hosts: Vec<Host>) -> CollectionHosts {
        CollectionHosts {
            id: "col1".into(),
            name: "Production".into(),
            org_id: "org1".into(),
            permission: permission.into(),
            waiting: false,
            hosts,
        }
    }

    fn info() -> CollectionInfo {
        CollectionInfo {
            id: "col1".into(),
            org_id: "org1".into(),
            name: "Production".into(),
            key_version: 1,
            permission: "edit".into(),
            needs_new_key: false,
        }
    }

    #[test]
    fn a_collections_host_is_saved_and_deleted_by_who_may() {
        let (dir, v) = vault("save");
        let toml = "[[group]]\nname = \"Mine\"\n\n[[host]]\nname = \"db\"\naddress = \"10.0.0.9\"\n\
                    user = \"deploy\"\ngroup = \"Mine\"\nfavorite = true\norg = \"org1\"\n";
        std::fs::write(dir.join("hosts.toml"), toml).expect("write");
        let store = shellwing_config::Store::new(dir.clone());
        let mut host = store.hosts().expect("hosts").hosts.remove(0);
        host.id = None;

        let refused = save_host(&v, &collection("connect", vec![]), host.clone());
        assert!(refused.is_err(), "Connect only");
        let id = save_host(&v, &collection("edit", vec![]), host).expect("save");
        let read = hosts_of(&v, &info());
        assert_eq!(read.len(), 1);
        let h = &read[0];
        assert_eq!(h.id.as_deref(), Some(id.as_str()));
        assert_eq!(
            (h.address.as_str(), h.user.as_deref()),
            ("10.0.0.9", Some("deploy"))
        );
        assert_eq!(h.group, None, "hosts.toml's group stays there");
        assert!(!h.favorite);
        assert_eq!(h.org.as_deref(), Some("org1"), "the collection's, as read");

        // Only this collection's host goes, and only for who may edit.
        let c = collection("edit", read.clone());
        assert!(delete_host(&v, &c, "not-its-host").is_err());
        assert!(delete_host(&v, &collection("connect", read.clone()), &id).is_err());
        delete_host(&v, &c, &id).expect("delete");
        assert!(hosts_of(&v, &info()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_moves_with_its_host_into_the_collection_only() {
        let (dir, v) = vault("password");
        let col = Space::Collection("col1".into());
        v.set_password("deploy@10.0.0.9", "s3cret").expect("save");
        assert!(
            v.password_in(&col, "deploy@10.0.0.9")
                .expect("read")
                .is_none(),
            "a personal password is not the collection's"
        );
        assert!(carry_password(&v, &Space::Personal, &col, "deploy@10.0.0.9").expect("carry"));
        let carried = v.password_in(&col, "deploy@10.0.0.9").expect("read");
        assert_eq!(carried.as_deref().map(String::as_str), Some("s3cret"));
        assert!(
            !carry_password(&v, &Space::Personal, &col, "nobody@10.0.0.1").expect("carry"),
            "nothing saved, nothing carried"
        );
        // Saved again, it replaces the collection's, as a personal one does.
        v.set_password_in(&col, "deploy@10.0.0.9", "n3w")
            .expect("save");
        let again = v.password_in(&col, "deploy@10.0.0.9").expect("read");
        assert_eq!(again.as_deref().map(String::as_str), Some("n3w"));
        assert_eq!(
            v.entries_in(&col, "password").expect("entries").len(),
            1,
            "one record per target"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hosts_password_is_found_under_its_user_host_and_port() {
        let toml =
            "[[host]]\nname = \"db\"\naddress = \"10.0.0.9\"\nuser = \"deploy\"\nport = 2202\n";
        let dir =
            std::env::temp_dir().join(format!("shellwing-core-target-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("hosts.toml"), toml).expect("write");
        let mut host = shellwing_config::Store::new(dir.clone())
            .hosts()
            .expect("hosts")
            .hosts
            .remove(0);
        let none = dir.join("no-ssh-config");
        assert_eq!(
            crate::target::password_target(&host, Some(&none)).as_deref(),
            Some("deploy@10.0.0.9:2202")
        );
        host.port = None;
        assert_eq!(
            crate::target::password_target(&host, Some(&none)).as_deref(),
            Some("deploy@10.0.0.9")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

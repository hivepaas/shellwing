//! The collections of an organization the vault holds, as host lists show
//! them (docs/design/collections.md): on the desktop and the phones alike.

use shellwing_config::{Host, Hosts};
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

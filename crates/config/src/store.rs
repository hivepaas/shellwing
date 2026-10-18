use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::de::DeserializeOwned;

use crate::hosts::Hosts;
use crate::settings::Settings;
use crate::theme::{Theme, Themes};

const SETTINGS: &str = "config.toml";
const HOSTS: &str = "hosts.toml";
const THEMES: &str = "themes";

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
        change(&mut doc).map_err(|m| {
            WriteError::Invalid(LoadError {
                file: file.into(),
                line: None,
                message: m.into(),
            })
        })?;
        fs::create_dir_all(&self.dir).map_err(io_err)?;
        // Written next to the file, then renamed over it: a crash leaves either the
        // old file or the new one, never half of one.
        let tmp = self.dir.join(format!(".{file}.{}.tmp", std::process::id()));
        fs::write(&tmp, doc.to_string()).map_err(io_err)?;
        fs::rename(&tmp, &path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            io_err(e)
        })
    }
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
    fn stamp_changes_when_a_file_does() {
        let s = temp("stamp");
        let first = s.stamp();
        assert_eq!(first, s.stamp());
        s.set_theme("nord").expect("write");
        assert_ne!(first, s.stamp());
    }
}

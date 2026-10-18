//! Settings, hosts and themes from the config directory, reloaded when the files
//! change. A file that fails to parse keeps the last good values and is
//! reported to the user.

use std::sync::Arc;

use shellwing_config::{
    CursorStyle, Hosts, LoadError, OnDisconnect, Scrollbar, Settings, Stamp, Store, Theme, Themes,
    theme_for,
};
use shellwing_term::{Colors, Rgb};

/// Everything that decides how the terminal looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub theme: Arc<Theme>,
    pub font_family: Option<String>,
    pub font_size: f32,
    pub line_height: f32,
    pub cursor: CursorStyle,
    pub scrollbar: Scrollbar,
    pub padding: (f32, f32),
    pub scrollback: usize,
    pub on_disconnect: OnDisconnect,
}

impl Look {
    pub fn colors(&self) -> Colors {
        let t = &self.theme;
        Colors {
            foreground: rgb(t.foreground),
            background: rgb(t.background),
            cursor: rgb(t.cursor),
            palette: t.palette.map(rgb),
        }
    }
}

pub fn rgb(c: shellwing_config::Color) -> Rgb {
    Rgb {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

pub struct Live {
    store: Option<Store>,
    stamp: Stamp,
    settings: Settings,
    hosts: Hosts,
    themes: Themes,
}

impl Live {
    /// Loads the config directory; `None` (no home directory) means defaults only.
    pub fn open(store: Option<Store>) -> (Live, Vec<String>) {
        let mut live = Live {
            stamp: Stamp::default(),
            store,
            settings: Settings::default(),
            hosts: Hosts::default(),
            themes: Themes::builtin(),
        };
        let problems = live.load();
        (live, problems)
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    /// The folder the vault syncs with, if any.
    pub fn sync_folder(&self) -> Option<std::path::PathBuf> {
        self.settings.sync.folder_path()
    }

    /// Saves the sync folder to `config.toml` (None: stop syncing).
    pub fn set_sync_folder(&mut self, folder: Option<&std::path::Path>) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or("No config directory to save to.")?;
        // Shown with ~ for the home folder, as people write it.
        let text = folder.map(|f| {
            match std::env::home_dir().and_then(|h| f.strip_prefix(h).ok().map(|r| r.to_path_buf()))
            {
                Some(rest) => format!("~/{}", rest.display()),
                None => f.display().to_string(),
            }
        });
        store
            .set_sync_folder(text.as_deref())
            .map_err(|e| e.to_string())?;
        self.load();
        Ok(())
    }

    /// Whether a start without a host opens the last run's tabs.
    pub fn restore_tabs(&self) -> bool {
        self.settings.general.restore_tabs
    }

    pub fn hosts(&self) -> &Hosts {
        &self.hosts
    }

    fn load(&mut self) -> Vec<String> {
        let Some(store) = &self.store else {
            return vec![];
        };
        self.stamp = store.stamp();
        let mut problems: Vec<LoadError> = vec![];
        match store.settings() {
            Ok(s) => self.settings = s,
            Err(e) => problems.push(e),
        }
        match store.hosts() {
            Ok(h) => self.hosts = h,
            Err(e) => problems.push(e),
        }
        let (themes, errors) = store.themes();
        self.themes = themes;
        problems.extend(errors);
        problems.iter().map(ToString::to_string).collect()
    }

    /// Reloads if a file changed since the last load: `Some(problems)`.
    pub fn reload_if_changed(&mut self) -> Option<Vec<String>> {
        let store = self.store.as_ref()?;
        (store.stamp() != self.stamp).then(|| self.load())
    }

    /// The look for a tab connected to the `hosts.toml` entry `host` (whose
    /// theme wins), and a problem to report (an unknown theme).
    pub fn look_for(&self, host: Option<&str>) -> (Look, Option<String>) {
        let host = host.and_then(|h| self.hosts.find(h));
        let want = theme_for(&self.settings, &self.hosts, host);
        let (theme, problem) = match self.themes.find(want) {
            Some(t) => (t.clone(), None),
            None => (
                self.themes.fallback(),
                Some(format!("Theme `{want}` not found; using the default.")),
            ),
        };
        let a = &self.settings.appearance;
        let look = Look {
            theme,
            font_family: a.font.family.clone(),
            font_size: a.font.size,
            line_height: a.line_height,
            cursor: a.cursor.style,
            scrollbar: a.scrollbar,
            padding: (a.padding.x, a.padding.y),
            scrollback: self.settings.terminal.scrollback,
            on_disconnect: self.settings.terminal.on_disconnect,
        };
        (look, problem)
    }

    /// Moves `config.toml`'s theme `step` places along the theme list and saves it,
    /// unless `host` (the tab's hosts.toml entry) sets its own. Returns the new
    /// theme, or why the change could not be made.
    pub fn step_theme(&mut self, step: isize, host: Option<&str>) -> Result<Arc<Theme>, String> {
        let host = host.and_then(|h| self.hosts.find(h));
        if let Some(h) = host
            && theme_for(&self.settings, &self.hosts, Some(h)) != self.settings.appearance.theme
        {
            return Err(format!(
                "{} sets its own theme in hosts.toml; change it there.",
                h.name
            ));
        }
        let current = self
            .themes
            .find(&self.settings.appearance.theme)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let next = self.themes.step(&current, step);
        self.save_theme(&next.id)?;
        Ok(next)
    }

    /// Sets `config.toml`'s theme to `id` and saves it.
    pub fn set_theme(&mut self, id: &str) -> Result<Arc<Theme>, String> {
        let theme = self
            .themes
            .find(id)
            .cloned()
            .ok_or_else(|| format!("Theme `{id}` not found."))?;
        self.save_theme(&theme.id)?;
        Ok(theme)
    }

    fn save_theme(&mut self, id: &str) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or("No config directory to save to.")?;
        store.set_theme(id).map_err(|e| e.to_string())?;
        self.load();
        Ok(())
    }

    pub fn themes(&self) -> &Themes {
        &self.themes
    }

    /// Whether the `hosts.toml` entry `host` sets a theme of its own (directly or
    /// through its group), which the app theme does not change.
    pub fn host_theme(&self, host: Option<&str>) -> Option<&str> {
        let h = self.hosts.find(host?)?;
        let t = theme_for(&self.settings, &self.hosts, Some(h));
        (t != self.settings.appearance.theme).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(name: &str, files: &[(&str, &str)]) -> Live {
        let dir =
            std::env::temp_dir().join(format!("shellwing-look-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for (f, text) in files {
            std::fs::write(dir.join(f), text).expect("write");
        }
        Live::open(Some(Store::new(dir))).0
    }

    #[test]
    fn host_theme_wins_and_blocks_stepping() {
        let mut l = live(
            "host",
            &[
                ("config.toml", "[appearance]\ntheme = \"nord\"\n"),
                (
                    "hosts.toml",
                    "[[host]]\nname = \"db\"\naddress = \"x\"\ntheme = \"dracula\"\n",
                ),
            ],
        );
        assert_eq!(l.look_for(None).0.theme.id, "nord");
        assert_eq!(l.look_for(Some("db")).0.theme.id, "dracula");
        assert!(l.step_theme(1, Some("db")).is_err());
        assert!(l.step_theme(1, None).is_ok(), "other tabs may still step");
    }

    #[test]
    fn stepping_saves_the_next_theme() {
        let mut l = live(
            "step",
            &[(
                "config.toml",
                "# mine\n[appearance]\ntheme = \"default-slate\"\n",
            )],
        );
        assert_eq!(l.step_theme(1, None).expect("step").id, "neon-nebula");
        assert_eq!(l.look_for(None).0.theme.id, "neon-nebula");
        assert_eq!(
            l.reload_if_changed(),
            None,
            "its own write is already loaded"
        );
    }

    #[test]
    fn a_broken_file_keeps_the_last_good_settings() {
        let mut l = live(
            "broken",
            &[("config.toml", "[appearance]\ntheme = \"nord\"\n")],
        );
        let dir = l.store.as_ref().expect("store").dir().to_path_buf();
        std::fs::write(dir.join("config.toml"), "[appearance]\ntheme = \n").expect("write");
        let problems = l.reload_if_changed().expect("noticed");
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].starts_with("config.toml line 2"),
            "{}",
            problems[0]
        );
        assert_eq!(l.look_for(None).0.theme.id, "nord");
    }

    #[test]
    fn unknown_theme_falls_back_with_a_problem() {
        let l = live(
            "unknown",
            &[("config.toml", "[appearance]\ntheme = \"nope\"\n")],
        );
        let (look, problem) = l.look_for(None);
        assert_eq!(look.theme.id, shellwing_config::DEFAULT_THEME);
        assert!(problem.is_some());
    }
}

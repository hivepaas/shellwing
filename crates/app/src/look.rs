//! Settings, hosts and themes from the config directory, reloaded when the files
//! change. A file that fails to parse keeps the last good values and is
//! reported to the user.

use std::sync::Arc;

use shellwing_config::{
    CursorStyle, Hosts, LoadError, Settings, Stamp, Store, Theme, Themes, theme_for,
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
    pub scrollback: usize,
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
    /// The `hosts.toml` entry this window connected to, whose theme wins.
    host: Option<String>,
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
            host: None,
        };
        let problems = live.load();
        (live, problems)
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    pub fn hosts(&self) -> &Hosts {
        &self.hosts
    }

    pub fn set_host(&mut self, name: Option<String>) {
        self.host = name;
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

    /// The look in effect, and a problem to report (an unknown theme).
    pub fn look(&self) -> (Look, Option<String>) {
        let host = self.host.as_deref().and_then(|h| self.hosts.find(h));
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
            scrollback: self.settings.terminal.scrollback,
        };
        (look, problem)
    }

    /// Moves `config.toml`'s theme `step` places along the theme list and saves it.
    /// Returns the new theme, or why the change could not be made.
    pub fn step_theme(&mut self, step: isize) -> Result<Arc<Theme>, String> {
        let host = self.host.as_deref().and_then(|h| self.hosts.find(h));
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
        let store = self
            .store
            .as_ref()
            .ok_or("No config directory to save to.")?;
        store.set_theme(&next.id).map_err(|e| e.to_string())?;
        self.load();
        Ok(next)
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
        assert_eq!(l.look().0.theme.id, "nord");
        l.set_host(Some("db".into()));
        assert_eq!(l.look().0.theme.id, "dracula");
        assert!(l.step_theme(1).is_err());
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
        assert_eq!(l.step_theme(1).expect("step").id, "neon-nebula");
        assert_eq!(l.look().0.theme.id, "neon-nebula");
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
        assert_eq!(l.look().0.theme.id, "nord");
    }

    #[test]
    fn unknown_theme_falls_back_with_a_problem() {
        let l = live(
            "unknown",
            &[("config.toml", "[appearance]\ntheme = \"nope\"\n")],
        );
        let (look, problem) = l.look();
        assert_eq!(look.theme.id, shellwing_config::DEFAULT_THEME);
        assert!(problem.is_some());
    }
}

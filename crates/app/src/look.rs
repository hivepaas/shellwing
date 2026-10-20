//! Settings, hosts and themes from the config directory, reloaded when the files
//! change. A file that fails to parse keeps the last good values and is
//! reported to the user.

use std::sync::Arc;

use shellwing_config::{
    ColorMode, CursorStyle, Hosts, Keymap, LoadError, OnDisconnect, Os, Scrollbar, Settings, Stamp,
    Store, Theme, Themes, theme_for,
};
use shellwing_i18n::tr;
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

/// A text size as drawn at the default UI font size; it grows and shrinks with
/// the chosen one (the window's rem follows it, see the workspace).
pub fn ui(px: f32) -> gpui::Rems {
    gpui::rems(px / 16.)
}

/// The UI font size the views' sizes are written for (body text 13 px, the
/// rem 16 px, as GPUI's).
const DRAWN_AT: f32 = 13.;

/// The window's rem for a UI font size.
pub fn rem_for(ui_size: f32) -> gpui::Pixels {
    gpui::px(16. * ui_size / DRAWN_AT)
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
    /// The window's shortcuts, as `[shortcuts]` sets them.
    keymap: Arc<Keymap>,
    /// The app is light: the terminal takes `light_theme`.
    light: bool,
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
            keymap: Arc::new(Keymap::defaults(Os::this())),
            light: false,
        };
        let problems = live.load();
        (live, problems)
    }

    pub fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    /// Where the vault syncs, if anywhere.
    pub fn sync_place(&self) -> Option<shellwing_config::SyncPlace> {
        self.settings.sync.place()
    }

    /// Saves where the vault syncs to `config.toml` (None: stop syncing).
    pub fn set_sync_place(
        &mut self,
        place: Option<&shellwing_config::SyncPlace>,
    ) -> Result<(), String> {
        use shellwing_config::{SyncPlace, SyncPlaceText};
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| tr!("No config directory to save to."))?;
        let text = place.map(|p| match p {
            // Shown with ~ for the home folder, as people write it.
            SyncPlace::Folder(f) => SyncPlaceText::Folder(
                match std::env::home_dir()
                    .and_then(|h| f.strip_prefix(h).ok().map(|r| r.to_path_buf()))
                {
                    Some(rest) => format!("~/{}", rest.display()),
                    None => f.display().to_string(),
                },
            ),
            SyncPlace::WebDav(w) => SyncPlaceText::WebDav(w.clone()),
            SyncPlace::S3(s) => SyncPlaceText::S3(s.clone()),
            SyncPlace::Git(g) => SyncPlaceText::Git(g.clone()),
        });
        store
            .set_sync_place(text.as_ref())
            .map_err(|e| e.to_string())?;
        self.load();
        Ok(())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether the app is light now: its mode, or the system's when it
    /// follows the system (`system_light`).
    pub fn light_for(&self, system_light: bool) -> bool {
        match self.settings.appearance.mode {
            ColorMode::System => system_light,
            ColorMode::Dark => false,
            ColorMode::Light => true,
        }
    }

    /// Makes the terminal's theme the light one, or the dark one.
    pub fn set_light(&mut self, light: bool) {
        self.light = light;
    }

    pub fn light(&self) -> bool {
        self.light
    }

    /// The app's theme in the mode in use (a host can set its own).
    pub fn app_theme(&self) -> &str {
        self.settings.appearance.app_theme(self.light)
    }

    /// Writes one setting to `config.toml` and loads the result.
    pub fn set_setting(
        &mut self,
        path: &[&str],
        value: Option<shellwing_config::SettingValue>,
    ) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| tr!("No config directory to save to."))?;
        store.set_setting(path, value).map_err(|e| e.to_string())?;
        let problems = self.load();
        match problems.first() {
            Some(p) => Err(p.clone()),
            None => Ok(()),
        }
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
        let (keymap, wrong) = Keymap::new(&self.settings.shortcuts, Os::this());
        self.keymap = Arc::new(keymap);
        let mut problems: Vec<String> = problems.iter().map(ToString::to_string).collect();
        problems.extend(wrong.iter().map(crate::hotkeys::problem));
        problems
    }

    pub fn keymap(&self) -> Arc<Keymap> {
        self.keymap.clone()
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
        let want = theme_for(&self.settings, &self.hosts, host, self.light);
        let (theme, problem) = match self.themes.find(want) {
            Some(t) => (t.clone(), None),
            None => (
                self.themes.fallback(),
                Some(tr!(
                    "Theme `{want}` not found; using the default.",
                    want = want
                )),
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
            && theme_for(&self.settings, &self.hosts, Some(h), self.light) != self.app_theme()
        {
            return Err(tr!(
                "{host} sets its own theme in hosts.toml; change it there.",
                host = h.name.as_str()
            ));
        }
        let current = self
            .themes
            .find(self.app_theme())
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
            .ok_or_else(|| tr!("Theme `{id}` not found.", id = id))?;
        self.save_theme(&theme.id)?;
        Ok(theme)
    }

    fn save_theme(&mut self, id: &str) -> Result<(), String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| tr!("No config directory to save to."))?;
        // The light mode's theme is a key of its own.
        if self.light {
            store.set_setting(
                &["appearance", "light_theme"],
                Some(shellwing_config::SettingValue::Str(id.into())),
            )
        } else {
            store.set_theme(id)
        }
        .map_err(|e| e.to_string())?;
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
        let t = theme_for(&self.settings, &self.hosts, Some(h), self.light);
        (t != self.app_theme()).then_some(t)
    }
}

/// A theme group's name, by its id (one of `shellwing_config::GROUPS`), in
/// the user's language; None: the user's own themes.
pub fn group_title(id: Option<&str>) -> String {
    use shellwing_i18n::tr;
    match id {
        Some("blue") => tr!("Blue and slate"),
        Some("purple") => tr!("Purple and pink"),
        Some("green") => tr!("Green and teal"),
        Some("warm") => tr!("Warm"),
        Some("neutral") => tr!("Neutral"),
        Some("gradient") => tr!("Gradient"),
        Some("gradient-light") => tr!("Gradient, light"),
        Some("light-cool") => tr!("Light, cool"),
        Some("light-warm") => tr!("Light, warm"),
        // A group a newer version has: its English name.
        Some(other) => shellwing_config::GROUPS
            .iter()
            .find(|(g, _)| *g == other)
            .map_or_else(|| other.to_string(), |(_, t)| (*t).to_string()),
        None => tr!("Your themes"),
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
    fn the_light_mode_has_a_theme_of_its_own() {
        let mut l = live(
            "light",
            &[
                (
                    "config.toml",
                    "[appearance]
mode = \"system\"\ntheme = \"nord\"\n",
                ),
                (
                    "hosts.toml",
                    "[[host]]\nname = \"db\"\naddress = \"x\"\ntheme = \"dracula\"\n",
                ),
            ],
        );
        assert!(!l.light_for(false) && l.light_for(true), "system follows");
        l.set_light(true);
        assert_eq!(l.look_for(None).0.theme.id, "dawn", "the default light one");
        assert_eq!(
            l.look_for(Some("db")).0.theme.id,
            "dracula",
            "a host keeps its own"
        );
        // Choosing while light sets the light mode's, not the dark one.
        l.set_theme("paper").expect("set");
        assert_eq!(l.settings().appearance.light_theme, "paper");
        assert_eq!(l.settings().appearance.theme, "nord");
        l.set_light(false);
        assert_eq!(l.look_for(None).0.theme.id, "nord");
        l.set_setting(
            &["appearance", "mode"],
            Some(shellwing_config::SettingValue::Str("light".into())),
        )
        .expect("mode");
        assert!(l.light_for(false), "light, whatever the system is");
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
        assert_eq!(l.step_theme(1, None).expect("step").id, "tokyo-night");
        assert_eq!(l.look_for(None).0.theme.id, "tokyo-night");
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

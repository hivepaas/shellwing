//! shellwing's configuration: the TOML files in the config directory.
//!
//! - `config.toml`: [`Settings`], edited by people and by the app.
//! - `hosts.toml`: [`Hosts`] and their groups.
//! - `themes/*.toml`: user [`Theme`]s, next to the built-in ones.
//! - `state/*.json`: what the app remembers, such as the [`WindowState`]; never
//!   edited by people and never synced.
//!
//! Missing files mean defaults. A file that fails to parse is reported as a
//! [`LoadError`] with its line, and the caller keeps what it had before. The app
//! writes `config.toml` back through `toml_edit`, so comments and order survive
//! (see [`Store::set_theme`]). Layout and rationale: `docs/design/config.md`.

mod color;
mod hosts;
mod hosts_sync;
mod settings;
pub mod settings_sync;
pub mod shortcuts;
mod state;
mod store;
mod theme;

pub use color::Color;
pub use hosts::{Auth, AuthMethod, Group, HOST_COLORS, Host, HostKey, Hosts, color_rgb};
pub use hosts_sync::Fields;
pub use settings::{
    Appearance, ColorMode, Cursor, CursorStyle, Files, Font, General, GitSync, Log, LogFormat,
    OnDisconnect, Padding, S3Sync, Scrollbar, Settings, Ssh, Sync, SyncPlace, Terminal,
    UI_FONT_SIZE, UiFont, Updates, VaultSettings, WebDavSync,
};
pub use shortcuts::{Chord, Hotkey, Keymap, Mods, Os, ShortcutSettings};
pub use state::{
    NoteWindowState, PaneState, PlacedFile, SessionState, TabState, UpdateState, WindowMode,
    WindowState,
};
pub use store::{EditFile, LoadError, SettingValue, Stamp, Store, SyncPlaceText, WriteError};
pub use theme::{DEFAULT_LIGHT_THEME, DEFAULT_THEME, GROUPS, Gradient, Theme, Themes};

/// The theme in effect, from lowest to highest precedence: `config.toml`, the
/// host's group, the host.
pub fn theme_for<'a>(
    settings: &'a Settings,
    hosts: &'a Hosts,
    host: Option<&'a Host>,
    light: bool,
) -> &'a str {
    host.and_then(|h| {
        h.theme
            .as_deref()
            .or_else(|| hosts.group_of(h).and_then(|g| g.theme.as_deref()))
    })
    .unwrap_or(settings.appearance.app_theme(light))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_beats_group_beats_settings() {
        let hosts: Hosts = toml::from_str(
            r#"
            [[group]]
            name = "Production"
            theme = "dracula"

            [[host]]
            name = "a"
            address = "10.0.0.1"
            group = "Production"

            [[host]]
            name = "b"
            address = "10.0.0.2"
            group = "Production"
            theme = "nord"

            [[host]]
            name = "c"
            address = "10.0.0.3"
            "#,
        )
        .expect("hosts");
        let settings: Settings =
            toml::from_str("[appearance]\ntheme = \"tokyo-night\"\n").expect("settings");
        let theme = |name| theme_for(&settings, &hosts, hosts.find(name), false);
        assert_eq!(theme("a"), "dracula");
        assert_eq!(theme("b"), "nord");
        assert_eq!(theme("c"), "tokyo-night");
        assert_eq!(theme_for(&settings, &hosts, None, false), "tokyo-night");
        // Light: the app's light theme, but a host's or group's own stays.
        assert_eq!(theme_for(&settings, &hosts, None, true), "dawn");
        assert_eq!(
            theme_for(&settings, &hosts, hosts.find("a"), true),
            "dracula"
        );
    }
}

//! The settings a phone has, from `config.toml` as the desktop writes it:
//! the terminal's theme, its history, and how hosts are met (their keys,
//! keepalives). Those sync with the vault, as on the desktop. The themes
//! are the desktop's built-in ones.

use shellwing_config::{HostKey, SettingValue, Theme, Themes};
use shellwing_term::{Colors, Rgb};

use crate::{MobileError, Shellwing};

/// How an unknown host key is met.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HostKeyWay {
    /// Its fingerprint shown, to accept or not.
    Ask,
    /// Kept, and gone on with.
    AcceptNew,
    /// Refused.
    Strict,
}

impl From<HostKey> for HostKeyWay {
    fn from(k: HostKey) -> Self {
        match k {
            HostKey::Ask => HostKeyWay::Ask,
            HostKey::AcceptNew => HostKeyWay::AcceptNew,
            HostKey::Strict => HostKeyWay::Strict,
        }
    }
}

impl From<HostKeyWay> for HostKey {
    fn from(k: HostKeyWay) -> Self {
        match k {
            HostKeyWay::Ask => HostKey::Ask,
            HostKeyWay::AcceptNew => HostKey::AcceptNew,
            HostKeyWay::Strict => HostKey::Strict,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ThemeRow {
    pub id: String,
    pub name: String,
    pub background: u32,
    pub foreground: u32,
    pub accent: u32,
    /// Its eight ANSI colors, for a swatch.
    pub palette: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SettingsRow {
    /// The terminal's theme, by id.
    pub theme: String,
    /// Lines of history a terminal keeps.
    pub scrollback: u64,
    pub host_key: HostKeyWay,
    /// Seconds between keepalives; 0: none.
    pub keepalive: u64,
    /// These settings sync with the vault.
    pub synced: bool,
}

#[uniffi::export]
impl Shellwing {
    pub fn settings(&self) -> SettingsRow {
        let s = self.store.settings().unwrap_or_default();
        let theme = Themes::builtin()
            .find(&s.appearance.theme)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| shellwing_config::DEFAULT_THEME.to_string());
        SettingsRow {
            theme,
            scrollback: s.terminal.scrollback as u64,
            host_key: s.ssh.host_key.into(),
            keepalive: s.ssh.keepalive,
            synced: s.sync.settings,
        }
    }

    /// The dark themes, as the desktop lists them; light ones do not suit
    /// the app's dark chrome.
    pub fn themes(&self) -> Vec<ThemeRow> {
        let dark = |t: &Theme| {
            let b = t.background;
            (b.r as u32 * 299 + b.g as u32 * 587 + b.b as u32 * 114) / 1000 < 128
        };
        Themes::builtin()
            .all()
            .iter()
            .filter(|t| dark(t))
            .map(|t| ThemeRow {
                id: t.id.clone(),
                name: t.name.clone(),
                background: hex(t.background),
                foreground: hex(t.foreground),
                accent: hex(t.accent),
                palette: t.palette[..8].iter().map(|c| hex(*c)).collect(),
            })
            .collect()
    }

    /// Sets the terminal's theme: sessions opened after take it.
    pub fn set_theme(&self, id: String) -> Result<(), MobileError> {
        self.store.set_theme(&id).map_err(MobileError::of)
    }

    pub fn set_scrollback(&self, lines: u64) -> Result<(), MobileError> {
        let lines = lines.clamp(100, 100_000) as i64;
        self.set(&["terminal", "scrollback"], SettingValue::Int(lines))
    }

    pub fn set_host_key(&self, way: HostKeyWay) -> Result<(), MobileError> {
        let name = HostKey::from(way).name().to_string();
        self.set(&["ssh", "host_key"], SettingValue::Str(name))
    }

    pub fn set_keepalive(&self, seconds: u64) -> Result<(), MobileError> {
        let s = seconds.min(3600) as i64;
        self.set(&["ssh", "keepalive"], SettingValue::Int(s))
    }

    pub fn set_settings_synced(&self, on: bool) -> Result<(), MobileError> {
        self.set(&["sync", "settings"], SettingValue::Bool(on))
    }
}

impl Shellwing {
    fn set(&self, path: &[&str], value: SettingValue) -> Result<(), MobileError> {
        self.store
            .set_setting(path, Some(value))
            .map_err(MobileError::of)
    }

    /// The terminal's colors for a session to `host_theme` (a host's own),
    /// else the app's theme.
    pub(crate) fn colors(&self, host_theme: Option<&str>) -> Colors {
        let themes = Themes::builtin();
        let app = self.store.settings().unwrap_or_default().appearance.theme;
        let t = host_theme
            .and_then(|id| themes.find(id))
            .or_else(|| themes.find(&app))
            .cloned()
            .unwrap_or_else(|| themes.fallback());
        let rgb = |c: shellwing_config::Color| Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        };
        Colors {
            foreground: rgb(t.foreground),
            background: rgb(t.background),
            cursor: rgb(t.cursor),
            palette: t.palette.map(rgb),
        }
    }
}

fn hex(c: shellwing_config::Color) -> u32 {
    (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}

//! The settings a phone has, from `config.toml` as the desktop writes it:
//! the terminal's theme, its history, and how hosts are met (their keys,
//! keepalives). Those sync with the vault, as on the desktop. The themes
//! are the desktop's built-in ones. And the hosts whose keys are known.

use shellwing_config::{HostKey, SettingValue, Theme, Themes};
use shellwing_ssh::known_hosts;
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
    /// The app's language, by tag; None: the phone's. This device's own.
    pub language: Option<String>,
}

/// A line of known_hosts: a host whose key is known.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct KnownHostRow {
    /// Its line in the file, from 1.
    pub line: u64,
    /// The names it is for; empty when they are hashed.
    pub names: Vec<String>,
    pub kind: String,
    pub fingerprint: String,
    /// `@cert-authority` or `@revoked`.
    pub marker: Option<String>,
}

#[uniffi::export]
impl Shellwing {
    /// The hosts whose keys are known, as known_hosts lists them.
    pub fn known_hosts(&self) -> Result<Vec<KnownHostRow>, MobileError> {
        let Some(path) = known_hosts::default_path() else {
            return Ok(vec![]);
        };
        Ok(known_hosts::entries(&path)
            .map_err(MobileError::of)?
            .into_iter()
            .map(|e| KnownHostRow {
                line: e.line as u64,
                names: e.names().into_iter().map(str::to_string).collect(),
                kind: e.key.kind,
                fingerprint: e.key.fingerprint,
                marker: e.marker,
            })
            .collect())
    }

    /// Forgets the known hosts on `lines`: their keys are asked about again
    /// next time. The file before is kept as `known_hosts.old`.
    pub fn forget_known_hosts(&self, lines: Vec<u64>) -> Result<u32, MobileError> {
        let Some(path) = known_hosts::default_path() else {
            return Ok(0);
        };
        let lines: Vec<usize> = lines.into_iter().map(|l| l as usize).collect();
        known_hosts::remove_lines(&path, &lines)
            .map(|n| n as u32)
            .map_err(MobileError::of)
    }

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
            language: s.general.language.filter(|l| !l.is_empty() && l != "auto"),
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

    /// Speaks `tag` (None: the phone's language) from now on.
    pub fn set_language(&self, tag: Option<String>) -> Result<(), MobileError> {
        let value = tag.clone().map(SettingValue::Str);
        self.store
            .set_setting(&["general", "language"], value)
            .map_err(MobileError::of)?;
        shellwing_i18n::set_language(tag.as_deref());
        Ok(())
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

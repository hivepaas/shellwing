use serde::Deserialize;

use crate::theme::{DEFAULT_LIGHT_THEME, DEFAULT_THEME};

/// `config.toml`. Every key is optional; unknown keys are ignored so a file
/// written by a newer version still loads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub sync: Sync,
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub ssh: Ssh,
    pub files: Files,
    pub log: Log,
    pub vault: VaultSettings,
    pub lock: LockSettings,
    pub updates: Updates,
    /// `[shortcuts]`: keys for the window's shortcuts, by name, over the
    /// defaults. This device's own: keys differ between systems.
    pub shortcuts: crate::ShortcutSettings,
}

/// Telling when a newer shellwing is out.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// Look once a day (reading the signed list of releases); off: only when
    /// asked to, in Settings › About.
    pub check: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { check: true }
    }
}

/// The vault on this device.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct VaultSettings {
    /// Minutes without a key pressed or a click before the vault locks, when
    /// its key is not in the keychain (only its passphrase opens it). None
    /// or 0: it stays open.
    pub lock_after: Option<u32>,
}

/// shellwing locked while its user is away: a light guard for a screen left
/// unlocked. This device's own.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LockSettings {
    /// Minutes without a key pressed or a click before shellwing hides its
    /// windows until the system says it is the user (Touch ID or the login
    /// password, Windows Hello, the Linux password); it asks when shellwing
    /// starts too. None or 0: it never locks by itself.
    pub after: Option<u32>,
}

/// Session logs: what a pane was sent, written to a file as it comes.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Log {
    /// Every session is logged from its start, but those of hosts with
    /// `log = false`.
    pub always: bool,
    /// The folder, `~/` allowed; None: `shellwing/logs` in the user's data
    /// folder.
    pub folder: Option<String>,
    pub format: LogFormat,
    /// Each line of a text log starts with the time.
    pub timestamps: bool,
}

impl Default for Log {
    fn default() -> Self {
        Log {
            always: false,
            folder: None,
            format: LogFormat::Text,
            timestamps: true,
        }
    }
}

impl Log {
    /// The folder with `~/` expanded; None: the default.
    pub fn folder_path(&self) -> Option<std::path::PathBuf> {
        expand_home(self.folder.as_deref()?)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// The lines, as plain text.
    #[default]
    Text,
    /// Every byte as it came, colors and all.
    Raw,
}

/// A folder as written in the settings, `~/` expanded. None when empty.
fn expand_home(f: &str) -> Option<std::path::PathBuf> {
    let f = f.trim();
    if f.is_empty() {
        return None;
    }
    Some(match (f.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => std::path::PathBuf::from(f),
    })
}

/// The Files panel.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Files {
    /// The command a remote file is edited with, its path added at the end
    /// (`code`, `gnome-text-editor`, `subl -n`). None: the system's app for it.
    pub editor: Option<String>,
}

/// Defaults for every host; a host in hosts.toml can set its own.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Ssh {
    pub host_key: crate::hosts::HostKey,
    /// Seconds between keepalives; 0: none. A host's ServerAliveInterval wins.
    pub keepalive: u64,
}

impl Default for Ssh {
    fn default() -> Self {
        Ssh {
            host_key: crate::hosts::HostKey::Ask,
            keepalive: 30,
        }
    }
}

/// Syncing the vault through a folder (docs/design/sync.md).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Sync {
    /// The folder, `~/` allowed; None: no sync.
    pub folder: Option<String>,
    /// A WebDAV server instead (Nextcloud, Synology, Koofr). Its password is
    /// in the vault, this device's own.
    pub webdav: Option<WebDavSync>,
    /// S3-compatible storage instead (AWS, R2, B2, MinIO, Wasabi). Its secret
    /// key is in the vault, this device's own.
    pub s3: Option<S3Sync>,
    /// A Git repository instead, through the system's git. A token, if one
    /// is used, is in the vault, this device's own.
    pub git: Option<GitSync>,
    /// The settings that are not this device's own sync too
    /// ([`crate::settings_sync::SYNCED`]).
    pub settings: bool,
}

impl Default for Sync {
    fn default() -> Self {
        Sync {
            folder: None,
            webdav: None,
            s3: None,
            git: None,
            settings: true,
        }
    }
}

impl Sync {
    /// The folder with `~/` expanded.
    pub fn folder_path(&self) -> Option<std::path::PathBuf> {
        expand_home(self.folder.as_deref()?)
    }

    /// Where the vault syncs: one place, the folder first should several be
    /// set by hand.
    pub fn place(&self) -> Option<SyncPlace> {
        if let Some(f) = self.folder_path() {
            return Some(SyncPlace::Folder(f));
        }
        if let Some(w) = &self.webdav {
            return Some(SyncPlace::WebDav(w.clone()));
        }
        if let Some(s3) = &self.s3 {
            return Some(SyncPlace::S3(s3.clone()));
        }
        self.git.clone().map(SyncPlace::Git)
    }
}

/// `[sync] webdav`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WebDavSync {
    /// The folder's address: `https://cloud.example.com/remote.php/dav/files/me/`.
    pub url: String,
    pub user: String,
}

/// `[sync] s3`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct S3Sync {
    /// `https://s3.<region>.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`…
    pub endpoint: String,
    /// Empty: `auto`, as R2 takes.
    pub region: String,
    pub bucket: String,
    /// A folder in the bucket; empty: its top.
    pub prefix: String,
    pub access_key: String,
}

/// `[sync] git`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct GitSync {
    /// `https://github.com/you/vault.git` or `git@github.com:you/vault.git`.
    pub url: String,
    /// Empty: the repository's default.
    pub branch: String,
    /// Who a token signs in as, where the host needs it (Gitea); empty:
    /// any, as GitHub and GitLab take.
    pub user: String,
}

/// Where the vault syncs, as `[sync]` says; secrets are not in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPlace {
    Folder(std::path::PathBuf),
    WebDav(WebDavSync),
    S3(S3Sync),
    Git(GitSync),
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct General {
    /// Open the tabs of the last run when started without a host.
    pub restore_tabs: bool,
    /// The app's language, a tag (`en`, `vi`); None or `auto`: the
    /// system's. This device's own: it does not sync.
    pub language: Option<String>,
}

impl Default for General {
    fn default() -> Self {
        General {
            restore_tabs: true,
            language: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Dark or light, or as the system is.
    pub mode: ColorMode,
    /// The terminal's theme when dark: a theme id (its file name without
    /// `.toml`) or name.
    pub theme: String,
    /// The terminal's theme when light.
    pub light_theme: String,
    pub font: Font,
    /// Row height as a multiple of the font size.
    pub line_height: f32,
    pub cursor: Cursor,
    pub scrollbar: Scrollbar,
    /// Space between the text and the pane's edges, in pixels.
    pub padding: Padding,
    /// The app's own text: tabs, the host list, the palette, panels, settings.
    pub ui_font: UiFont,
    /// The app's accent (the logo, what is chosen, buttons) is the theme's.
    pub theme_accent: bool,
}

/// The font of everything but the terminal.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct UiFont {
    /// None: the system's interface font.
    pub family: Option<String>,
    /// The size of body text; the rest scales with it.
    pub size: f32,
}

/// The UI font size by default: the terminal's, so the two match.
pub const UI_FONT_SIZE: f32 = 14.0;

impl Default for UiFont {
    fn default() -> Self {
        UiFont {
            family: None,
            size: UI_FONT_SIZE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Padding {
    pub x: f32,
    pub y: f32,
}

impl Default for Padding {
    fn default() -> Self {
        Padding { x: 4.0, y: 4.0 }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            mode: ColorMode::default(),
            theme: DEFAULT_THEME.into(),
            light_theme: DEFAULT_LIGHT_THEME.into(),
            font: Font::default(),
            line_height: 1.3,
            cursor: Cursor::default(),
            scrollbar: Scrollbar::default(),
            padding: Padding::default(),
            ui_font: UiFont::default(),
            theme_accent: true,
        }
    }
}

impl Appearance {
    /// The terminal's theme in the light mode or the dark one.
    pub fn app_theme(&self, light: bool) -> &str {
        if light {
            &self.light_theme
        } else {
            &self.theme
        }
    }
}

/// Whether the app is dark or light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// As the system is, and changing when it does.
    #[default]
    System,
    Dark,
    Light,
}

/// The overlay scrollbar at the right edge of a terminal with history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scrollbar {
    /// While scrolling, or with the pointer at the right edge.
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Font {
    /// None: the first installed of the platform's monospace fonts.
    pub family: Option<String>,
    pub size: f32,
}

impl Default for Font {
    fn default() -> Self {
        Font {
            family: None,
            size: 14.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Cursor {
    pub style: CursorStyle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// Lines of history kept per terminal.
    pub scrollback: usize,
    pub on_disconnect: OnDisconnect,
    /// The shell a local terminal runs, with its arguments; None: the
    /// user's own (`$SHELL`; PowerShell or cmd on Windows).
    pub local_shell: Option<String>,
}

impl Default for Terminal {
    fn default() -> Self {
        Terminal {
            scrollback: 10_000,
            on_disconnect: OnDisconnect::default(),
            local_shell: None,
        }
    }
}

/// What a pane does when its connection ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnDisconnect {
    /// A lost connection is retried by itself; after `exit`, Enter reconnects.
    #[default]
    Reconnect,
    /// Nothing happens by itself; Enter reconnects.
    Ask,
    /// After `exit` the pane closes; a lost connection waits for Enter.
    Close,
}

impl Settings {
    /// Brings numbers that would break the layout back into a usable range.
    pub(crate) fn sanitized(mut self) -> Self {
        let a = &mut self.appearance;
        a.font.size = a.font.size.clamp(6.0, 72.0);
        a.line_height = a.line_height.clamp(1.0, 3.0);
        a.padding.x = a.padding.x.clamp(0.0, 64.0);
        a.padding.y = a.padding.y.clamp(0.0, 64.0);
        // Past 20 the fixed-height rows of the app run out of room.
        a.ui_font.size = a.ui_font.size.clamp(10.0, 20.0);
        for family in [&mut a.font.family, &mut a.ui_font.family] {
            if family.as_deref().is_some_and(|f| f.trim().is_empty()) {
                *family = None;
            }
        }
        self.terminal.scrollback = self.terminal.scrollback.min(1_000_000);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_defaults() {
        let s: Settings = toml::from_str("").expect("parse");
        assert_eq!(s, Settings::default());
        assert_eq!(s.appearance.theme, DEFAULT_THEME);
        assert_eq!(s.appearance.font.size, 14.0);
    }

    #[test]
    fn reads_every_key_and_clamps() {
        let s: Settings = toml::from_str(
            r#"
            # A comment.
            [appearance]
            mode = "light"
            theme = "nord"
            light_theme = "paper"
            font = { family = "Fira Code", size = 200 }
            line_height = 1.5
            cursor = { style = "bar" }
            scrollbar = "always"
            padding = { x = 100 }
            ui_font = { family = "Inter", size = 40 }
            something_new = true

            [terminal]
            scrollback = 50000
            on_disconnect = "close"

            [ssh]
            host_key = "accept-new"
            keepalive = 0

            [files]
            editor = "code -n"

            [log]
            always = true
            folder = "~/logs"
            format = "raw"
            timestamps = false
            "#,
        )
        .expect("parse");
        let s = s.sanitized();
        assert_eq!(s.appearance.theme, "nord");
        assert_eq!(s.appearance.mode, ColorMode::Light);
        assert_eq!(s.appearance.app_theme(true), "paper");
        assert_eq!(s.appearance.app_theme(false), "nord");
        assert_eq!(s.appearance.font.family.as_deref(), Some("Fira Code"));
        assert_eq!(s.appearance.font.size, 72.0);
        assert_eq!(s.appearance.line_height, 1.5);
        assert_eq!(s.appearance.cursor.style, CursorStyle::Bar);
        assert_eq!(s.appearance.scrollbar, Scrollbar::Always);
        assert_eq!(s.appearance.padding, Padding { x: 64.0, y: 4.0 });
        assert_eq!(s.appearance.ui_font.family.as_deref(), Some("Inter"));
        assert_eq!(s.appearance.ui_font.size, 20.0);
        assert_eq!(s.terminal.scrollback, 50_000);
        assert_eq!(s.terminal.on_disconnect, OnDisconnect::Close);
        assert_eq!(s.ssh.host_key, crate::hosts::HostKey::AcceptNew);
        assert_eq!(s.ssh.keepalive, 0);
        assert_eq!(s.files.editor.as_deref(), Some("code -n"));
        assert!(s.log.always && !s.log.timestamps);
        assert_eq!(s.log.format, LogFormat::Raw);
        assert_eq!(
            s.log.folder_path(),
            std::env::home_dir().map(|h| h.join("logs"))
        );
    }
}

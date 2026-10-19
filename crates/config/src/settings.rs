use serde::Deserialize;

use crate::theme::DEFAULT_THEME;

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
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Sync {
    /// The folder, `~/` allowed; None: no sync.
    pub folder: Option<String>,
}

impl Sync {
    /// The folder with `~/` expanded.
    pub fn folder_path(&self) -> Option<std::path::PathBuf> {
        let f = self.folder.as_deref()?.trim();
        if f.is_empty() {
            return None;
        }
        Some(match (f.strip_prefix("~/"), std::env::home_dir()) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => std::path::PathBuf::from(f),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct General {
    /// Open the tabs of the last run when started without a host.
    pub restore_tabs: bool,
}

impl Default for General {
    fn default() -> Self {
        General { restore_tabs: true }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// A theme id (its file name without `.toml`) or name.
    pub theme: String,
    pub font: Font,
    /// Row height as a multiple of the font size.
    pub line_height: f32,
    pub cursor: Cursor,
    pub scrollbar: Scrollbar,
    /// Space between the text and the pane's edges, in pixels.
    pub padding: Padding,
    /// The app's own text: tabs, the host list, the palette, panels, settings.
    pub ui_font: UiFont,
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
            theme: DEFAULT_THEME.into(),
            font: Font::default(),
            line_height: 1.3,
            cursor: Cursor::default(),
            scrollbar: Scrollbar::default(),
            padding: Padding::default(),
            ui_font: UiFont::default(),
        }
    }
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
            theme = "nord"
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
            "#,
        )
        .expect("parse");
        let s = s.sanitized();
        assert_eq!(s.appearance.theme, "nord");
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
    }
}

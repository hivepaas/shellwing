use serde::Deserialize;

use crate::theme::DEFAULT_THEME;

/// `config.toml`. Every key is optional; unknown keys are ignored so a file
/// written by a newer version still loads.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub appearance: Appearance,
    pub terminal: Terminal,
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
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: DEFAULT_THEME.into(),
            font: Font::default(),
            line_height: 1.3,
            cursor: Cursor::default(),
            scrollbar: Scrollbar::default(),
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
}

impl Default for Terminal {
    fn default() -> Self {
        Terminal {
            scrollback: 10_000,
            on_disconnect: OnDisconnect::default(),
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
        if a.font
            .family
            .as_deref()
            .is_some_and(|f| f.trim().is_empty())
        {
            a.font.family = None;
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
            something_new = true

            [terminal]
            scrollback = 50000
            on_disconnect = "close"
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
        assert_eq!(s.terminal.scrollback, 50_000);
        assert_eq!(s.terminal.on_disconnect, OnDisconnect::Close);
    }
}

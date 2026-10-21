use serde::{Deserialize, Serialize};

/// What the last look for a newer shellwing found (`state/updates.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateState {
    /// When it looked, in seconds since 1970.
    #[serde(default)]
    pub checked_at: Option<u64>,
    /// The release it was offered, if any (`v0.2.0`).
    #[serde(default)]
    pub offered: Option<String>,
    /// A release the user said to skip.
    #[serde(default)]
    pub skipped: Option<String>,
    /// A release downloaded and checked, waiting to be installed as the app
    /// quits: its version, and its file.
    #[serde(default)]
    pub staged: Option<(String, String)>,
}

/// A note's window's size when one last closed, in logical pixels: the
/// next opens so.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteWindowState {
    pub width: f32,
    pub height: f32,
}

/// Where the window was when last seen, in logical pixels. For a maximised or
/// fullscreen window, the bounds it returns to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub mode: WindowMode,
    /// The host list's width as dragged; None: the default.
    #[serde(default)]
    pub sidebar_width: Option<f32>,
    /// The host list folded to a rail of host tiles.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// The Files panel's width as dragged; None: the default.
    #[serde(default)]
    pub files_width: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// A config file placed on a host until its tab closed, that may still be
/// there: the app quit, or the connection dropped, first. Taken away the
/// next time the host connects, if it is still as placed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedFile {
    /// The host, as the tab opened it.
    pub host: String,
    pub name: String,
    /// Where it is, in full.
    pub path: String,
    /// Where what was there before was set aside, to put back.
    pub aside: Option<String>,
    /// SHA-256 of what was placed, hex: a file since changed is left alone.
    pub sha256: String,
}

/// The tabs and panes open when the app last ran, to open again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub tabs: Vec<TabState>,
    /// The active tab.
    #[serde(default)]
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub root: PaneState,
    /// The focused pane, counted left to right and top to bottom.
    #[serde(default)]
    pub focused: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PaneState {
    /// A terminal connected to `spec` (a host name, an alias or `user@host`).
    Leaf { spec: String },
    Split {
        /// `horizontal`: side by side; `vertical`: stacked.
        horizontal: bool,
        ratio: f32,
        first: Box<PaneState>,
        second: Box<PaneState>,
    },
}

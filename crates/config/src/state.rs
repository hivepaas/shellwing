use serde::{Deserialize, Serialize};

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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
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

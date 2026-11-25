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
    /// The offered release's system package for this computer, when this copy
    /// came from one and the list names it: its file, and its SHA-256.
    #[serde(default)]
    pub package: Option<(String, String)>,
    /// A release the user said to skip.
    #[serde(default)]
    pub skipped: Option<String>,
    /// A release downloaded and checked, waiting to be installed as the app
    /// quits: its version, and its file.
    #[serde(default)]
    pub staged: Option<(String, String)>,
}

/// What a host was last seen running (`state/systems.json`, by what opens
/// it): looked for at most once a day, while its icon is the system's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSeen {
    /// The system's icon (`ubuntu`); None: the host would not say.
    #[serde(default)]
    pub system: Option<String>,
    /// When it was looked for, in seconds since 1970.
    #[serde(default)]
    pub checked_at: u64,
}

/// Who may reach which host through shellwing's MCP server, and how
/// (`state/ai.json`): this computer's alone, never synced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiState {
    #[serde(default)]
    pub clients: Vec<AiClient>,
    #[serde(default)]
    pub grants: Vec<AiGrant>,
    /// What a new grant does with a command: ask for risky ones, unless
    /// the person chose otherwise.
    #[serde(default)]
    pub default_mode: AiMode,
    /// What makes a command risky, for grants that ask for risky ones: a
    /// line each, its words in a row in the command (`rm -rf`); a word
    /// ending in `/` begins one (`/etc/`). None: [`DEFAULT_RISKY`].
    #[serde(default)]
    pub risky: Option<Vec<String>>,
}

/// What makes a command risky when the person has said nothing.
pub const DEFAULT_RISKY: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "rm -r",
    "dd",
    "mkfs",
    "fdisk",
    "parted",
    "wipefs",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "systemctl stop",
    "systemctl disable",
    "systemctl mask",
    "iptables",
    "nft",
    "ufw",
    "kill -9",
    "killall",
    "pkill",
    "chmod -R",
    "chown -R",
    "userdel",
    "passwd",
    "crontab -r",
    "drop",
    "truncate",
    "delete from",
    "docker rm",
    "docker system prune",
    "kubectl delete",
    "terraform destroy",
    "git push --force",
    "git reset --hard",
    "> /etc/",
    "> /dev/",
];

impl AiState {
    /// What makes a command risky now.
    pub fn risky(&self) -> Vec<String> {
        self.risky
            .clone()
            .unwrap_or_else(|| DEFAULT_RISKY.iter().map(|s| s.to_string()).collect())
    }
}

/// An AI client let in: by the SHA-256 of its token, never the token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiClient {
    pub id: String,
    /// Which client it is (`claude-code`, `cursor`, `stdio`…), for its
    /// config.
    pub kind: String,
    pub name: String,
    /// Hex.
    pub token_sha256: String,
    /// Seconds since 1970.
    #[serde(default)]
    pub added_at: u64,
    /// Stopped for now: its token still lets it in, but it may use no host
    /// until the person resumes it. Its grants are kept as they are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

/// A host given to a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiGrant {
    /// The client's id.
    pub client: String,
    /// What opening the host connects to, as the host list has it.
    pub host: String,
    #[serde(default)]
    pub how: AiHow,
    #[serde(default)]
    pub mode: AiMode,
    /// When it ends, in seconds since 1970; None: when it is taken back.
    #[serde(default)]
    pub until: Option<u64>,
    /// It ends when shellwing quits: not kept for the next start.
    #[serde(default)]
    pub session: bool,
    /// It may run commands.
    #[serde(default = "yes")]
    pub run: bool,
    /// It may read files and list folders.
    #[serde(default = "yes")]
    pub read: bool,
    /// It may write files.
    #[serde(default)]
    pub write: bool,
    /// It may list and read the notes about this host, but never a
    /// sensitive one (docs/design/ai-notes.md).
    #[serde(default)]
    pub notes_read: bool,
    /// It may write a note about this host, or add to the end of one it may
    /// read.
    #[serde(default)]
    pub notes_add: bool,
    /// It may list the snippets offered for this host, and run one where
    /// it may run commands (docs/design/ai-snippets.md).
    #[serde(default)]
    pub snippets: bool,
    /// Stopped for now, kept as it is to resume: the client may not use the
    /// host meanwhile. Its time (`until`) runs on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiHow {
    /// On a connection of its own, with no tab.
    #[default]
    Background,
    /// Typing into a tab to the host.
    Tab,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMode {
    /// Commands run at once; each is kept in the activity.
    Free,
    /// One that looks risky waits for the person; the rest run.
    #[default]
    Risky,
    /// Each waits for the person.
    Ask,
}

/// What the host list has folded (`state/sidebar.json`): its Actions,
/// and sections by key (a group's name, a collection's id, else the
/// section's title).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarState {
    /// Actions shown; folded by default.
    #[serde(default)]
    pub actions_open: bool,
    #[serde(default)]
    pub folded: Vec<String>,
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
    /// The drawer's width as dragged, where hosts and groups are edited;
    /// None: the default.
    #[serde(default)]
    pub drawer_width: Option<f32>,
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

#[cfg(test)]
mod ai_tests {
    use super::*;

    /// Risky commands wait unless the person says otherwise: for a new
    /// state, and for a grant or a file that says nothing.
    #[test]
    fn risky_ones_wait_by_default() {
        assert_eq!(AiState::default().default_mode, AiMode::Risky);
        let s: AiState =
            serde_json::from_str(r#"{"grants": [{"client": "c", "host": "web"}]}"#).expect("json");
        assert_eq!(s.default_mode, AiMode::Risky);
        assert_eq!(s.grants[0].mode, AiMode::Risky);
        let s: AiState = serde_json::from_str(r#"{"default_mode": "free"}"#).expect("json");
        assert_eq!(s.default_mode, AiMode::Free, "a choice made is kept");
    }

    /// A grant that says nothing of them may run commands and read files,
    /// not write them.
    #[test]
    fn a_grant_runs_and_reads_unless_it_says_otherwise() {
        let s: AiState =
            serde_json::from_str(r#"{"grants": [{"client": "c", "host": "web"}]}"#).expect("json");
        let g = &s.grants[0];
        assert!(g.run && g.read && !g.write);
        assert!(!g.notes_read && !g.notes_add, "notes only when given");
        assert!(!g.snippets, "snippets only when given");
        let s: AiState = serde_json::from_str(
            r#"{"grants": [{"client": "c", "host": "web", "run": false, "write": true}]}"#,
        )
        .expect("json");
        let g = &s.grants[0];
        assert!(!g.run && g.read && g.write);
    }

    /// Nothing is paused unless saved so, and a pause is kept.
    #[test]
    fn a_pause_is_kept() {
        let s: AiState = serde_json::from_str(
            r#"{"clients": [{"id": "c", "kind": "stdio", "name": "C", "token_sha256": "ab"}],
                "grants": [{"client": "c", "host": "web"}]}"#,
        )
        .expect("json");
        assert!(!s.clients[0].paused && !s.grants[0].paused);
        let mut paused = s.clone();
        paused.clients[0].paused = true;
        paused.grants[0].paused = true;
        let back: AiState =
            serde_json::from_str(&serde_json::to_string(&paused).expect("save")).expect("json");
        assert_eq!(back, paused);
        let plain = serde_json::to_string(&s).expect("save");
        assert!(!plain.contains("paused"), "{plain}");
    }
}

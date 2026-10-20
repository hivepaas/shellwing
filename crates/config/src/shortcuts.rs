//! The window's keyboard shortcuts: what each does, the keys it takes by
//! default on this system, and the ones `config.toml`'s `[shortcuts]` gives
//! it instead:
//!
//! ```toml
//! [shortcuts]
//! palette = ["ctrl+shift+p"]
//! find = "ctrl+shift+f"
//! broadcast = []          # no key
//! ```
//!
//! A key is written as its modifiers and then the key, joined by `+`:
//! `ctrl`, `alt` (`option`), `shift`, `cmd` (`super`, `win`), then a letter,
//! a digit, a symbol, or a named key (`left`, `pageup`, `tab`, `f5`, ...).
//! `digit` stands for 1 to 9, for going to a tab by its number.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hotkey {
    Palette,
    NewTab,
    ClosePane,
    SplitRight,
    SplitDown,
    PaneLeft,
    PaneRight,
    PaneUp,
    PaneDown,
    NextTab,
    PreviousTab,
    /// Tab 1 to 9, by the digit pressed.
    GoToTab,
    /// Every open tab, by host.
    Tabs,
    HostList,
    Files,
    Find,
    /// Type in every pane of the tab, or stop.
    Broadcast,
    Settings,
    NextTheme,
    PreviousTheme,
    Quit,
}

impl Hotkey {
    /// In the order the Keyboard page lists them.
    pub const ALL: [Hotkey; 21] = [
        Hotkey::Palette,
        Hotkey::NewTab,
        Hotkey::ClosePane,
        Hotkey::SplitRight,
        Hotkey::SplitDown,
        Hotkey::PaneLeft,
        Hotkey::PaneRight,
        Hotkey::PaneUp,
        Hotkey::PaneDown,
        Hotkey::NextTab,
        Hotkey::PreviousTab,
        Hotkey::GoToTab,
        Hotkey::Tabs,
        Hotkey::HostList,
        Hotkey::Files,
        Hotkey::Find,
        Hotkey::Broadcast,
        Hotkey::Settings,
        Hotkey::NextTheme,
        Hotkey::PreviousTheme,
        Hotkey::Quit,
    ];

    /// Its name in `[shortcuts]`.
    pub fn name(self) -> &'static str {
        match self {
            Hotkey::Palette => "palette",
            Hotkey::NewTab => "new_tab",
            Hotkey::ClosePane => "close_pane",
            Hotkey::SplitRight => "split_right",
            Hotkey::SplitDown => "split_down",
            Hotkey::PaneLeft => "pane_left",
            Hotkey::PaneRight => "pane_right",
            Hotkey::PaneUp => "pane_up",
            Hotkey::PaneDown => "pane_down",
            Hotkey::NextTab => "next_tab",
            Hotkey::PreviousTab => "previous_tab",
            Hotkey::GoToTab => "go_to_tab",
            Hotkey::Tabs => "tabs",
            Hotkey::HostList => "host_list",
            Hotkey::Files => "files",
            Hotkey::Find => "find",
            Hotkey::Broadcast => "broadcast",
            Hotkey::Settings => "settings",
            Hotkey::NextTheme => "next_theme",
            Hotkey::PreviousTheme => "previous_theme",
            Hotkey::Quit => "quit",
        }
    }

    pub fn from_name(name: &str) -> Option<Hotkey> {
        Hotkey::ALL.into_iter().find(|h| h.name() == name)
    }

    /// Takes `digit` (1 to 9) rather than one key.
    pub fn numbered(self) -> bool {
        self == Hotkey::GoToTab
    }

    /// The keys it takes when `[shortcuts]` does not name it. Ctrl+Shift on
    /// Linux and Windows, as for copy and paste in a terminal (Ctrl alone is
    /// the shell's: Ctrl+K kills the line); Cmd on macOS.
    pub fn defaults(self, os: Os) -> &'static [&'static str] {
        let mac = os == Os::Mac;
        match self {
            Hotkey::Palette if mac => &["cmd+k"],
            Hotkey::Palette => &["ctrl+shift+k", "ctrl+shift+p"],
            Hotkey::NewTab if mac => &["cmd+t"],
            Hotkey::NewTab => &["ctrl+shift+t"],
            Hotkey::ClosePane if mac => &["cmd+w"],
            Hotkey::ClosePane => &["ctrl+shift+w"],
            // Cmd+D and Cmd+Shift+D on macOS, as in iTerm.
            Hotkey::SplitRight if mac => &["cmd+d"],
            Hotkey::SplitRight => &["ctrl+shift+d"],
            Hotkey::SplitDown if mac => &["cmd+shift+d"],
            Hotkey::SplitDown => &["ctrl+shift+e"],
            // Alt+arrows, as in Windows Terminal; Cmd+Option on macOS.
            Hotkey::PaneLeft if mac => &["cmd+alt+left"],
            Hotkey::PaneLeft => &["alt+left"],
            Hotkey::PaneRight if mac => &["cmd+alt+right"],
            Hotkey::PaneRight => &["alt+right"],
            Hotkey::PaneUp if mac => &["cmd+alt+up"],
            Hotkey::PaneUp => &["alt+up"],
            Hotkey::PaneDown if mac => &["cmd+alt+down"],
            Hotkey::PaneDown => &["alt+down"],
            Hotkey::NextTab if mac => &["ctrl+tab", "ctrl+pagedown", "cmd+}"],
            Hotkey::NextTab => &["ctrl+tab", "ctrl+pagedown"],
            Hotkey::PreviousTab if mac => &["ctrl+shift+tab", "ctrl+pageup", "cmd+{"],
            Hotkey::PreviousTab => &["ctrl+shift+tab", "ctrl+pageup"],
            Hotkey::GoToTab if mac => &["cmd+digit"],
            Hotkey::GoToTab => &["alt+digit"],
            // Cmd+Shift+A on macOS, as Chrome's tab search: Cmd+A selects all.
            Hotkey::Tabs if mac => &["cmd+shift+a"],
            Hotkey::Tabs => &["ctrl+shift+a"],
            Hotkey::HostList if mac => &["cmd+b"],
            Hotkey::HostList => &["ctrl+shift+b"],
            Hotkey::Files if mac => &["cmd+o"],
            Hotkey::Files => &["ctrl+shift+o"],
            Hotkey::Find if mac => &["cmd+f"],
            Hotkey::Find => &["ctrl+shift+f"],
            Hotkey::Broadcast if mac => &["cmd+shift+i"],
            Hotkey::Broadcast => &["ctrl+shift+i"],
            // Ctrl+, sends nothing to a terminal, so it is free, as in most apps.
            Hotkey::Settings if mac => &["cmd+,"],
            Hotkey::Settings => &["ctrl+,"],
            Hotkey::NextTheme if mac => &["cmd+]"],
            Hotkey::NextTheme => &["ctrl+}"],
            Hotkey::PreviousTheme if mac => &["cmd+["],
            Hotkey::PreviousTheme => &["ctrl+{"],
            Hotkey::Quit if mac => &["cmd+q"],
            Hotkey::Quit => &[],
        }
    }

    fn default_chords(self, os: Os) -> Vec<Chord> {
        self.defaults(os)
            .iter()
            .map(|s| s.parse().expect("a default shortcut parses"))
            .collect()
    }
}

/// Which system's keys: the defaults and what the system keeps differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Modifiers held. `cmd` is Cmd on macOS, the Super (Windows) key elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

/// One key with its modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub mods: Mods,
    /// Lowercase: a letter, digit or symbol, a named key (`left`, `f5`), or
    /// [`DIGIT`].
    pub key: String,
}

/// The key of a chord that stands for 1 to 9.
pub const DIGIT: &str = "digit";

/// Symbols typed with Shift on a US keyboard, beside the key that types them.
/// The system reports Ctrl+Shift+] as Ctrl+} (with or without Shift), so a
/// chord keeps the symbol and drops the Shift.
const SHIFTED: [(char, char); 21] = [
    ('`', '~'),
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    ('\\', '|'),
    (';', ':'),
    ('\'', '"'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
];

const NAMED: &[&str] = &[
    "left",
    "right",
    "up",
    "down",
    "home",
    "end",
    "pageup",
    "pagedown",
    "tab",
    "enter",
    "escape",
    "space",
    "backspace",
    "delete",
    "insert",
];

/// Why a chord does not read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ChordError {
    #[error("no key")]
    Empty,
    #[error("`{0}` is not a modifier (ctrl, alt, shift, cmd)")]
    Modifier(String),
    #[error("`{0}` is not a key")]
    Key(String),
}

fn function_key(key: &str) -> bool {
    key.strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

fn shifted(key: &str) -> bool {
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if SHIFTED.iter().any(|s| s.1 == c))
}

impl Chord {
    /// The chord of a key pressed, as the system reports it (`key` as GPUI
    /// names it); None for a modifier alone.
    pub fn pressed(key: &str, mods: Mods) -> Option<Chord> {
        let key = key.to_lowercase();
        let key = match key.as_str() {
            "shift" | "control" | "ctrl" | "alt" | "option" | "platform" | "super" | "cmd"
            | "meta" | "win" | "function" | "fn" | "capslock" | "" => {
                return None;
            }
            " " => "space".to_string(),
            _ => key,
        };
        Some(Chord { mods, key }.normal())
    }

    /// Shift+] as }: the symbol, without the Shift.
    fn normal(mut self) -> Chord {
        let mut chars = self.key.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if self.mods.shift
                && let Some(&(_, up)) = SHIFTED.iter().find(|s| s.0 == c)
            {
                self.key = up.to_string();
            }
            if shifted(&self.key) {
                self.mods.shift = false;
            }
        }
        self
    }

    pub fn is_digit(&self) -> bool {
        self.key == DIGIT
    }

    /// How `[shortcuts]` writes it: `ctrl+shift+k`.
    pub fn text(&self, os: Os) -> String {
        let m = &self.mods;
        let mut parts = vec![];
        if m.ctrl {
            parts.push("ctrl");
        }
        if m.alt {
            parts.push("alt");
        }
        if m.shift {
            parts.push("shift");
        }
        if m.cmd {
            parts.push(if os == Os::Mac { "cmd" } else { "super" });
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// How it is shown: `Ctrl+Shift+K`, `Cmd+Option+Left`.
    pub fn label(&self, os: Os) -> String {
        let m = &self.mods;
        let mut parts: Vec<String> = vec![];
        let mac = os == Os::Mac;
        if mac && m.cmd {
            parts.push("Cmd".into());
        }
        if m.ctrl {
            parts.push("Ctrl".into());
        }
        if m.alt {
            parts.push(if mac { "Option" } else { "Alt" }.into());
        }
        if m.shift {
            parts.push("Shift".into());
        }
        if !mac && m.cmd {
            parts.push(if os == Os::Windows { "Win" } else { "Super" }.into());
        }
        let key = match self.key.as_str() {
            DIGIT => "1…9".to_string(),
            "pageup" => "PageUp".into(),
            "pagedown" => "PageDown".into(),
            "escape" => "Esc".into(),
            k if function_key(k) => k.to_uppercase(),
            k => {
                let mut c = k.chars();
                match c.next() {
                    Some(first) => first.to_uppercase().chain(c).collect(),
                    None => String::new(),
                }
            }
        };
        parts.push(key);
        parts.join("+")
    }
}

impl std::str::FromStr for Chord {
    type Err = ChordError;

    fn from_str(s: &str) -> Result<Chord, ChordError> {
        let s = s.trim().to_lowercase();
        // `ctrl++`: the key is the plus.
        let (mods_part, key) = if s == "+" {
            ("", "+")
        } else if let Some(rest) = s.strip_suffix("++") {
            (rest, "+")
        } else {
            s.rsplit_once('+').unwrap_or(("", s.as_str()))
        };
        let mut mods = Mods::default();
        for m in mods_part
            .split('+')
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            match m {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" | "opt" => mods.alt = true,
                "shift" => mods.shift = true,
                "cmd" | "command" | "super" | "win" | "meta" => mods.cmd = true,
                other => return Err(ChordError::Modifier(other.to_string())),
            }
        }
        let key = match key.trim() {
            "" => return Err(ChordError::Empty),
            "esc" => "escape",
            "return" => "enter",
            "pgup" => "pageup",
            "pgdn" => "pagedown",
            "del" => "delete",
            "ins" => "insert",
            "plus" => "+",
            "minus" => "-",
            "comma" => ",",
            k => k,
        };
        let one_char = key.chars().count() == 1 && !key.starts_with(char::is_whitespace);
        if !(one_char || NAMED.contains(&key) || function_key(key) || key == DIGIT) {
            return Err(ChordError::Key(key.to_string()));
        }
        Ok(Chord {
            mods,
            key: key.to_string(),
        }
        .normal())
    }
}

/// What taking a chord for a shortcut means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Fine,
    /// Programs in the terminal use it (Ctrl+R, Alt+B, F5): with it here,
    /// they would not get it.
    Shell,
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A key alone, or with Shift: what is typed.
    Typing,
    /// The system takes it before the app sees it.
    System,
    /// One of the shortcuts that cannot be changed (copy, paste, zoom).
    Fixed(Fixed),
}

/// The shortcuts of a pane that cannot be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixed {
    Copy,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    PageUp,
    PageDown,
    Bottom,
}

impl Fixed {
    pub const ALL: [Fixed; 8] = [
        Fixed::Copy,
        Fixed::Paste,
        Fixed::ZoomIn,
        Fixed::ZoomOut,
        Fixed::ZoomReset,
        Fixed::PageUp,
        Fixed::PageDown,
        Fixed::Bottom,
    ];

    pub fn keys(self, os: Os) -> &'static [&'static str] {
        let mac = os == Os::Mac;
        match self {
            Fixed::Copy if mac => &["cmd+c"],
            Fixed::Copy => &["ctrl+shift+c"],
            Fixed::Paste if mac => &["cmd+v", "shift+insert"],
            Fixed::Paste => &["ctrl+shift+v", "shift+insert"],
            Fixed::ZoomIn if mac => &["cmd+=", "cmd++"],
            Fixed::ZoomIn => &["ctrl+=", "ctrl++"],
            Fixed::ZoomOut if mac => &["cmd+-"],
            Fixed::ZoomOut => &["ctrl+-"],
            Fixed::ZoomReset if mac => &["cmd+0"],
            Fixed::ZoomReset => &["ctrl+0"],
            Fixed::PageUp => &["shift+pageup"],
            Fixed::PageDown => &["shift+pagedown"],
            Fixed::Bottom => &["shift+end"],
        }
    }

    pub fn chords(self, os: Os) -> Vec<Chord> {
        self.keys(os)
            .iter()
            .map(|s| s.parse().expect("a fixed shortcut parses"))
            .collect()
    }
}

/// Keys the system keeps for itself.
fn system(os: Os) -> &'static [&'static str] {
    match os {
        Os::Mac => &[
            "cmd+tab",
            "cmd+shift+tab",
            "cmd+space",
            "cmd+h",
            "cmd+alt+h",
            "cmd+m",
            "cmd+`",
            "cmd+alt+escape",
            "ctrl+cmd+q",
            "ctrl+cmd+f",
            "cmd+#",
            "cmd+$",
            "cmd+%",
            "ctrl+up",
            "ctrl+down",
            "ctrl+left",
            "ctrl+right",
        ],
        Os::Windows => &[
            "alt+tab",
            "alt+shift+tab",
            "alt+f4",
            "alt+space",
            "alt+escape",
            "ctrl+escape",
            "ctrl+alt+delete",
            "ctrl+shift+escape",
        ],
        Os::Linux => &[
            "alt+tab",
            "alt+shift+tab",
            "alt+f4",
            "alt+space",
            "ctrl+alt+delete",
            "ctrl+alt+backspace",
        ],
    }
}

/// What taking `chord` for a shortcut would mean on `os`.
pub fn verdict(chord: &Chord, os: Os) -> Verdict {
    let m = chord.mods;
    let key = chord.key.as_str();
    let fkey = function_key(key);
    if !m.ctrl && !m.alt && !m.cmd && !fkey {
        return Verdict::Refused(Refusal::Typing);
    }
    let system_key = system(os).iter().any(|s| s.parse::<Chord>().ok().as_ref() == Some(chord))
        // Windows keeps the Windows key; Linux keeps Ctrl+Alt+F1… for its consoles.
        || (os == Os::Windows && m.cmd)
        || (os == Os::Linux && m.ctrl && m.alt && fkey);
    if system_key {
        return Verdict::Refused(Refusal::System);
    }
    if let Some(f) = Fixed::ALL
        .into_iter()
        .find(|f| f.chords(os).contains(chord))
    {
        return Verdict::Refused(Refusal::Fixed(f));
    }
    let letter = key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic();
    let symbol = key.len() == 1 && !letter;
    // Ctrl+letter and the like are control characters (Ctrl+R searches the
    // history, Ctrl+[ is Escape); Alt+key is Meta (Alt+B goes back a word,
    // Option+key types a symbol on macOS); F-keys drive htop and mc.
    let control = m.ctrl
        && !m.shift
        && !m.cmd
        && (letter || matches!(key, "[" | "]" | "\\" | "/" | "_" | "@" | "^" | "space"));
    let meta = m.alt && !m.ctrl && !m.cmd && (letter || symbol || key == DIGIT);
    let plain_fkey = fkey && !m.ctrl && !m.alt && !m.cmd;
    if control || meta || plain_fkey {
        Verdict::Shell
    } else {
        Verdict::Fine
    }
}

/// `[shortcuts]`: a shortcut's name, and one key or a list (`[]`: none).
pub type ShortcutSettings = BTreeMap<String, KeyList>;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    fn items(&self) -> Vec<&str> {
        match self {
            KeyList::One(s) => vec![s.as_str()],
            KeyList::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// Something wrong in `[shortcuts]`; what it names is left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutProblem {
    /// No shortcut has that name.
    Unknown(String),
    BadKey {
        name: String,
        key: String,
        error: ChordError,
    },
    /// A key that cannot be taken.
    Refused {
        name: String,
        key: String,
        refusal: Refusal,
    },
    /// Two shortcuts given the same key: the first keeps it.
    Twice {
        key: String,
        kept: Hotkey,
        lost: Hotkey,
    },
    /// `digit` for a shortcut that takes one key, or one key for going to a
    /// tab by number.
    Misfit { name: String, key: String },
}

/// Every shortcut's keys, as `[shortcuts]` sets them over the defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    os: Os,
    bound: BTreeMap<Hotkey, Vec<Chord>>,
    /// Named in `[shortcuts]`.
    custom: BTreeSet<Hotkey>,
}

impl Keymap {
    pub fn defaults(os: Os) -> Keymap {
        Keymap::new(&ShortcutSettings::new(), os).0
    }

    /// The keys `set` gives each shortcut, the others' defaults; and what
    /// was wrong. A key given to a shortcut is taken from the one that has
    /// it by default.
    pub fn new(set: &ShortcutSettings, os: Os) -> (Keymap, Vec<ShortcutProblem>) {
        let mut problems = vec![];
        let mut wanted: BTreeMap<Hotkey, Vec<Chord>> = BTreeMap::new();
        for (name, list) in set {
            let Some(h) = Hotkey::from_name(name) else {
                problems.push(ShortcutProblem::Unknown(name.clone()));
                continue;
            };
            let mut chords = vec![];
            for key in list.items() {
                let chord = match key.parse::<Chord>() {
                    Ok(c) => c,
                    Err(error) => {
                        problems.push(ShortcutProblem::BadKey {
                            name: name.clone(),
                            key: key.to_string(),
                            error,
                        });
                        continue;
                    }
                };
                if chord.is_digit() != h.numbered() {
                    problems.push(ShortcutProblem::Misfit {
                        name: name.clone(),
                        key: key.to_string(),
                    });
                    continue;
                }
                if let Verdict::Refused(refusal) = verdict(&chord, os) {
                    problems.push(ShortcutProblem::Refused {
                        name: name.clone(),
                        key: key.to_string(),
                        refusal,
                    });
                    continue;
                }
                if !chords.contains(&chord) {
                    chords.push(chord);
                }
            }
            wanted.insert(h, chords);
        }
        let custom: BTreeSet<Hotkey> = wanted.keys().copied().collect();
        // Those set first, then the defaults of the rest, without the keys
        // already taken.
        let mut taken: BTreeMap<Chord, Hotkey> = BTreeMap::new();
        let mut bound = BTreeMap::new();
        for h in Hotkey::ALL.into_iter().filter(|h| custom.contains(h)) {
            let mut kept = vec![];
            for c in wanted.remove(&h).unwrap_or_default() {
                match taken.get(&c) {
                    Some(&first) => problems.push(ShortcutProblem::Twice {
                        key: c.text(os),
                        kept: first,
                        lost: h,
                    }),
                    None => {
                        taken.insert(c.clone(), h);
                        kept.push(c);
                    }
                }
            }
            bound.insert(h, kept);
        }
        for h in Hotkey::ALL.into_iter().filter(|h| !custom.contains(h)) {
            let kept: Vec<Chord> = h
                .default_chords(os)
                .into_iter()
                .filter(|c| !taken.contains_key(c))
                .collect();
            for c in &kept {
                taken.insert(c.clone(), h);
            }
            bound.insert(h, kept);
        }
        (Keymap { os, bound, custom }, problems)
    }

    pub fn os(&self) -> Os {
        self.os
    }

    pub fn chords(&self, h: Hotkey) -> &[Chord] {
        self.bound.get(&h).map(Vec::as_slice).unwrap_or_default()
    }

    /// The first key's label, or "" for none: for hints beside a command.
    pub fn hint(&self, h: Hotkey) -> String {
        self.chords(h)
            .first()
            .map(|c| c.label(self.os))
            .unwrap_or_default()
    }

    /// `[shortcuts]` names it.
    pub fn is_custom(&self, h: Hotkey) -> bool {
        self.custom.contains(&h)
    }

    /// Something differs from the defaults.
    pub fn any_custom(&self) -> bool {
        !self.custom.is_empty()
    }

    /// The shortcut a key pressed is, and for [`Hotkey::GoToTab`] the tab
    /// (0 for 1).
    pub fn find(&self, pressed: &Chord) -> Option<(Hotkey, Option<usize>)> {
        if let Some(h) = self.holder(pressed) {
            return Some((h, None));
        }
        let b = pressed.key.as_bytes();
        if b.len() == 1 && (b'1'..=b'9').contains(&b[0]) {
            let digit = Chord {
                mods: pressed.mods,
                key: DIGIT.into(),
            };
            if let Some(h) = self.holder(&digit) {
                return Some((h, Some((b[0] - b'1') as usize)));
            }
        }
        None
    }

    /// The shortcut that has `chord`.
    pub fn holder(&self, chord: &Chord) -> Option<Hotkey> {
        self.bound
            .iter()
            .find(|(_, cs)| cs.contains(chord))
            .map(|(h, _)| *h)
    }

    /// What `[shortcuts]` should say for `h` to have `chords`: None (leave it
    /// out) when they are its defaults.
    pub fn entry_for(&self, h: Hotkey, chords: &[Chord]) -> Option<Vec<String>> {
        (chords != h.default_chords(self.os).as_slice())
            .then(|| chords.iter().map(|c| c.text(self.os)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        s.parse().expect("a key")
    }

    fn mods(ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            cmd,
        }
    }

    #[test]
    fn keys_read_as_written_and_as_pressed() {
        let c = chord("Ctrl+Shift+K");
        assert_eq!(c.mods, mods(true, false, true, false));
        assert_eq!(c.key, "k");
        assert_eq!(c.text(Os::Linux), "ctrl+shift+k");
        assert_eq!(c.label(Os::Linux), "Ctrl+Shift+K");
        assert_eq!(chord("cmd+alt+left").label(Os::Mac), "Cmd+Option+Left");
        assert_eq!(chord("super+t").label(Os::Windows), "Win+T");
        assert_eq!(chord("ctrl++").key, "+");
        assert_eq!(chord("alt+digit").label(Os::Linux), "Alt+1…9");
        assert_eq!(chord("ctrl+pgdn").label(Os::Linux), "Ctrl+PageDown");
        // Shift+] is }, however it is written or reported.
        assert_eq!(chord("ctrl+shift+]"), chord("ctrl+}"));
        assert_eq!(
            Chord::pressed("}", mods(true, false, true, false)),
            Some(chord("ctrl+}"))
        );
        assert_eq!(
            Chord::pressed("]", mods(true, false, true, false)),
            Some(chord("ctrl+}"))
        );
        assert_eq!(
            Chord::pressed("K", mods(true, false, true, false)),
            Some(chord("ctrl+shift+k"))
        );
        assert_eq!(
            Chord::pressed("shift", mods(false, false, true, false)),
            None
        );
        assert!(matches!(
            "hyper+k".parse::<Chord>(),
            Err(ChordError::Modifier(_))
        ));
        assert!(matches!(
            "ctrl+kk".parse::<Chord>(),
            Err(ChordError::Key(_))
        ));
        assert!(matches!("ctrl+".parse::<Chord>(), Err(ChordError::Empty)));
    }

    #[test]
    fn every_default_reads_and_none_is_given_twice() {
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let map = Keymap::defaults(os);
            let mut seen = BTreeSet::new();
            for h in Hotkey::ALL {
                assert_eq!(map.chords(h).len(), h.defaults(os).len(), "{h:?} {os:?}");
                for c in map.chords(h) {
                    assert!(seen.insert(c.clone()), "{c:?} twice on {os:?}");
                    assert_eq!(c.is_digit(), h.numbered());
                    assert!(
                        !matches!(verdict(c, os), Verdict::Refused(_)),
                        "{c:?} refused on {os:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_key_pressed_finds_its_shortcut() {
        let map = Keymap::defaults(Os::Linux);
        let find = |s: &str| map.find(&chord(s));
        assert_eq!(find("ctrl+shift+k"), Some((Hotkey::Palette, None)));
        assert_eq!(find("ctrl+shift+p"), Some((Hotkey::Palette, None)));
        assert_eq!(find("alt+3"), Some((Hotkey::GoToTab, Some(2))));
        assert_eq!(find("ctrl+}"), Some((Hotkey::NextTheme, None)));
        assert_eq!(find("ctrl+shift+tab"), Some((Hotkey::PreviousTab, None)));
        assert_eq!(find("ctrl+k"), None, "the shell's kill-line");
        assert_eq!(find("alt+0"), None);
        let mac = Keymap::defaults(Os::Mac);
        assert_eq!(mac.find(&chord("cmd+k")), Some((Hotkey::Palette, None)));
        assert_eq!(mac.find(&chord("cmd+}")), Some((Hotkey::NextTab, None)));
        assert_eq!(mac.find(&chord("cmd+]")), Some((Hotkey::NextTheme, None)));
        assert_eq!(mac.find(&chord("cmd+1")), Some((Hotkey::GoToTab, Some(0))));
    }

    #[test]
    fn the_settings_give_keys_and_take_them_from_the_defaults() {
        let set: ShortcutSettings = toml::from_str(
            r#"
            palette = "ctrl+shift+t"
            find = ["ctrl+shift+g", "nonsense+x", "ctrl+shift+c", "ctrl+shift+g"]
            broadcast = []
            go_to_tab = "ctrl+k"
            files = "ctrl+shift+g"
            renamed = "ctrl+shift+y"
            "#,
        )
        .expect("toml");
        let (map, problems) = Keymap::new(&set, Os::Linux);
        assert_eq!(map.chords(Hotkey::Palette), [chord("ctrl+shift+t")]);
        // New tab loses its default to the palette, without a word.
        assert_eq!(map.chords(Hotkey::NewTab), []);
        assert_eq!(map.chords(Hotkey::Files), [chord("ctrl+shift+g")]);
        assert_eq!(map.chords(Hotkey::Broadcast), []);
        assert_eq!(map.chords(Hotkey::GoToTab), []);
        assert_eq!(map.chords(Hotkey::Find), [], "Files has it first");
        assert!(map.is_custom(Hotkey::Find) && !map.is_custom(Hotkey::ClosePane));
        assert!(problems.contains(&ShortcutProblem::Unknown("renamed".into())));
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, ShortcutProblem::BadKey { key, .. } if key == "nonsense+x"))
        );
        assert!(problems.contains(&ShortcutProblem::Refused {
            name: "find".into(),
            key: "ctrl+shift+c".into(),
            refusal: Refusal::Fixed(Fixed::Copy),
        }));
        assert!(problems.contains(&ShortcutProblem::Misfit {
            name: "go_to_tab".into(),
            key: "ctrl+k".into(),
        }));
        assert!(problems.contains(&ShortcutProblem::Twice {
            key: "ctrl+shift+g".into(),
            kept: Hotkey::Files,
            lost: Hotkey::Find,
        }));
        // Back to the defaults: left out.
        assert_eq!(
            map.entry_for(
                Hotkey::Palette,
                &[chord("ctrl+shift+k"), chord("ctrl+shift+p")]
            ),
            None
        );
        assert_eq!(
            map.entry_for(Hotkey::Palette, &[chord("ctrl+shift+p")]),
            Some(vec!["ctrl+shift+p".to_string()])
        );
    }

    #[test]
    fn keys_the_shell_or_the_system_has_are_told() {
        let v = |s: &str, os| verdict(&chord(s), os);
        assert_eq!(v("k", Os::Linux), Verdict::Refused(Refusal::Typing));
        assert_eq!(v("shift+k", Os::Linux), Verdict::Refused(Refusal::Typing));
        assert_eq!(v("alt+tab", Os::Linux), Verdict::Refused(Refusal::System));
        assert_eq!(v("super+e", Os::Windows), Verdict::Refused(Refusal::System));
        assert_eq!(
            v("ctrl+alt+f2", Os::Linux),
            Verdict::Refused(Refusal::System)
        );
        assert_eq!(v("cmd+h", Os::Mac), Verdict::Refused(Refusal::System));
        assert_eq!(
            v("ctrl+shift+v", Os::Linux),
            Verdict::Refused(Refusal::Fixed(Fixed::Paste))
        );
        assert_eq!(v("ctrl+r", Os::Linux), Verdict::Shell);
        assert_eq!(v("ctrl+[", Os::Linux), Verdict::Shell);
        assert_eq!(v("alt+b", Os::Linux), Verdict::Shell);
        assert_eq!(v("f5", Os::Linux), Verdict::Shell);
        assert_eq!(v("ctrl+r", Os::Mac), Verdict::Shell);
        assert_eq!(v("ctrl+shift+r", Os::Linux), Verdict::Fine);
        assert_eq!(v("cmd+r", Os::Mac), Verdict::Fine);
        assert_eq!(v("ctrl+f5", Os::Linux), Verdict::Fine);
        assert_eq!(v("alt+left", Os::Linux), Verdict::Fine);
    }
}

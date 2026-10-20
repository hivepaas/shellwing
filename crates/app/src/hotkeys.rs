//! The window's shortcuts as `config.toml` sets them: handed to every view
//! (as a global) for the keys it shows beside a command, and what they are
//! called.

use std::sync::Arc;

use gpui::{App, Global, Keystroke};
use shellwing_config::shortcuts::{Fixed, Refusal, ShortcutProblem};
use shellwing_config::{Chord, Hotkey, Keymap, Mods, Os};
use shellwing_i18n::tr;

/// The shortcuts in effect.
pub struct Bound(pub Arc<Keymap>);

impl Global for Bound {}

pub fn keymap(cx: &App) -> Arc<Keymap> {
    cx.try_global::<Bound>()
        .map(|b| b.0.clone())
        .unwrap_or_else(|| Arc::new(Keymap::defaults(Os::this())))
}

/// The first key of `h`, as shown; "" when it has none.
pub fn hint(cx: &App, h: Hotkey) -> String {
    keymap(cx).hint(h)
}

/// The chord of a key pressed; None for a modifier alone.
pub fn pressed(k: &Keystroke) -> Option<Chord> {
    let m = &k.modifiers;
    Chord::pressed(
        &k.key,
        Mods {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            cmd: m.platform,
        },
    )
}

/// What a shortcut does, as the Keyboard page lists it.
pub fn label(h: Hotkey) -> String {
    match h {
        Hotkey::Palette => tr!("Command palette"),
        Hotkey::NewTab => tr!("New tab"),
        Hotkey::ClosePane => tr!("Close the pane"),
        Hotkey::SplitRight => tr!("Split right"),
        Hotkey::SplitDown => tr!("Split down"),
        Hotkey::PaneLeft => tr!("Pane to the left"),
        Hotkey::PaneRight => tr!("Pane to the right"),
        Hotkey::PaneUp => tr!("Pane above"),
        Hotkey::PaneDown => tr!("Pane below"),
        Hotkey::NextTab => tr!("Next tab"),
        Hotkey::PreviousTab => tr!("Previous tab"),
        Hotkey::GoToTab => tr!("Go to tab 1–9"),
        Hotkey::Tabs => tr!("Every tab, by host"),
        Hotkey::HostList => tr!("Host list"),
        Hotkey::Files => tr!("Files"),
        Hotkey::Find => tr!("Find in the pane"),
        Hotkey::Broadcast => tr!("Type in every pane"),
        Hotkey::Settings => tr!("Settings"),
        Hotkey::NextTheme => tr!("Next theme"),
        Hotkey::PreviousTheme => tr!("Previous theme"),
        Hotkey::Quit => tr!("Quit"),
    }
}

pub fn fixed_label(f: Fixed) -> String {
    match f {
        Fixed::Copy => tr!("Copy"),
        Fixed::Paste => tr!("Paste"),
        Fixed::ZoomIn => tr!("Larger text"),
        Fixed::ZoomOut => tr!("Smaller text"),
        Fixed::ZoomReset => tr!("Text at its set size"),
        Fixed::PageUp => tr!("Scroll up a page"),
        Fixed::PageDown => tr!("Scroll down a page"),
        Fixed::Bottom => tr!("Back to the bottom"),
    }
}

/// Why `key` (as shown) cannot be taken.
pub fn refusal(r: &Refusal, key: &str) -> String {
    match r {
        Refusal::Typing if Os::this() == Os::Mac => tr!(
            "{key} alone is for typing: hold Cmd, Ctrl or Option with it.",
            key = key
        ),
        Refusal::Typing => tr!(
            "{key} alone is for typing: hold Ctrl or Alt with it.",
            key = key
        ),
        Refusal::System => tr!(
            "{key} belongs to the system: it never reaches Shellwing.",
            key = key
        ),
        Refusal::Fixed(f) => tr!(
            "{key} is {what}, which cannot be changed.",
            key = key,
            what = fixed_label(*f)
        ),
    }
}

/// A problem in `[shortcuts]`, to report.
pub fn problem(p: &ShortcutProblem) -> String {
    match p {
        ShortcutProblem::Unknown(name) => tr!(
            "config.toml, [shortcuts]: no shortcut is named `{name}`.",
            name = name
        ),
        ShortcutProblem::BadKey { name, key, error } => tr!(
            "config.toml, [shortcuts] {name}: `{key}`: {error}",
            name = name,
            key = key,
            error = error.to_string()
        ),
        ShortcutProblem::Refused {
            name,
            key,
            refusal: r,
        } => tr!(
            "config.toml, [shortcuts] {name}: {why}",
            name = name,
            why = refusal(r, key)
        ),
        ShortcutProblem::Twice { key, kept, lost } => tr!(
            "config.toml, [shortcuts]: `{key}` is given to both {kept} and {lost}; {kept} keeps it.",
            key = key,
            kept = kept.name(),
            lost = lost.name()
        ),
        ShortcutProblem::Misfit { name, key } => tr!(
            "config.toml, [shortcuts] {name}: `{key}` does not fit; only go_to_tab takes `digit` (1 to 9), and it takes nothing else.",
            name = name,
            key = key
        ),
    }
}

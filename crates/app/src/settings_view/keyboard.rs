//! The Keyboard page: every window shortcut and its keys. Clicking a key
//! takes the next one pressed in its place; + gives the shortcut one more,
//! × takes one away; Reset puts a shortcut's own keys back, Reset all every
//! shortcut's. A key another shortcut has is moved (or swapped) on a yes; one
//! the terminal's programs use is taken on a yes; one the system or typing
//! needs is refused.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use shellwing_config::shortcuts::{DIGIT, Fixed, Verdict, verdict};
use shellwing_config::{Chord, Hotkey, Keymap, Os, SettingValue};
use shellwing_i18n::tr;

use super::{SettingsView, heading, label, page, small_button};
use crate::colors::colors;
use crate::hotkeys;
use crate::look::ui;

/// What the page is doing with a key.
#[derive(Default)]
pub(super) struct Keyboard {
    /// Taking a key for this shortcut: in place of its key at this index, or
    /// beside its keys.
    recording: Option<(Hotkey, Option<usize>)>,
    /// A key pressed that wants a yes first.
    asking: Option<Asking>,
    /// Why the key just pressed cannot be taken, under its shortcut.
    refused: Option<(Hotkey, String)>,
    /// Reset all was pressed once: the second press resets.
    reset_all: bool,
}

struct Asking {
    hotkey: Hotkey,
    at: Option<usize>,
    chord: Chord,
    /// The shortcut that has the key now.
    from: Option<Hotkey>,
    /// Programs in the terminal use it.
    shell: bool,
}

impl SettingsView {
    /// Taking a key: the window's shortcuts must let it through.
    pub(crate) fn recording(&self) -> bool {
        self.keyboard.recording.is_some()
    }

    /// Leaves off taking a key, or asking about one.
    pub(super) fn stop_recording(&mut self) {
        self.keyboard = Keyboard::default();
    }

    fn keymap(&self) -> Keymap {
        Keymap::new(&self.snap.settings.shortcuts, Os::this()).0
    }

    /// A key pressed: taken for the shortcut being recorded, if one is. True
    /// when it was this page's.
    pub(super) fn record_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some((h, at)) = self.keyboard.recording else {
            return false;
        };
        let k = &ev.keystroke;
        let m = &k.modifiers;
        if k.key == "escape" && !(m.control || m.alt || m.shift || m.platform) {
            self.keyboard.recording = None;
            self.keyboard.refused = None;
            cx.notify();
            return true;
        }
        // A modifier alone: the key is still to come.
        if let Some(chord) = hotkeys::pressed(k) {
            self.pressed(h, at, chord, cx);
        }
        true
    }

    fn pressed(&mut self, h: Hotkey, at: Option<usize>, mut chord: Chord, cx: &mut Context<Self>) {
        let os = Os::this();
        if h.numbered() {
            let b = chord.key.as_bytes();
            if b.len() != 1 || !(b'1'..=b'9').contains(&b[0]) {
                self.keyboard.refused = Some((
                    h,
                    tr!(
                        "Press a digit from 1 to 9 with the keys to hold, such as {example}: the digit picks the tab.",
                        example = if os == Os::Mac { "Cmd+1" } else { "Alt+1" }
                    ),
                ));
                return cx.notify();
            }
            chord.key = DIGIT.into();
        }
        let shown = chord.label(os);
        let said = verdict(&chord, os);
        if let Verdict::Refused(r) = &said {
            self.keyboard.refused = Some((h, hotkeys::refusal(r, &shown)));
            return cx.notify();
        }
        self.keyboard.recording = None;
        self.keyboard.refused = None;
        let from = self.keymap().holder(&chord);
        if from == Some(h) {
            // One of its keys already.
            return cx.notify();
        }
        if from.is_some() || said == Verdict::Shell {
            self.keyboard.asking = Some(Asking {
                hotkey: h,
                at,
                chord,
                from,
                shell: said == Verdict::Shell,
            });
            return cx.notify();
        }
        self.give(h, at, chord, None, cx);
    }

    /// Gives `chord` to `h`, in place of its key `at` or beside its keys,
    /// taking it from `from`; which gets the key replaced, when one was.
    fn give(
        &mut self,
        h: Hotkey,
        at: Option<usize>,
        chord: Chord,
        from: Option<Hotkey>,
        cx: &mut Context<Self>,
    ) {
        let map = self.keymap();
        let mut mine = map.chords(h).to_vec();
        let old = match at {
            Some(i) if i < mine.len() => Some(std::mem::replace(&mut mine[i], chord.clone())),
            _ => {
                mine.push(chord.clone());
                None
            }
        };
        let mut writes = vec![(h, mine)];
        if let Some(b) = from {
            let mut theirs = map.chords(b).to_vec();
            if let Some(i) = theirs.iter().position(|c| *c == chord) {
                match swapped(old.as_ref(), b, &theirs) {
                    Some(o) => theirs[i] = o,
                    None => {
                        theirs.remove(i);
                    }
                }
            }
            writes.push((b, theirs));
        }
        for (h, keys) in writes {
            self.write_keys(&map, h, &keys, cx);
        }
    }

    fn write_keys(&mut self, map: &Keymap, h: Hotkey, keys: &[Chord], cx: &mut Context<Self>) {
        let value = map.entry_for(h, keys).map(SettingValue::List);
        self.change(&["shortcuts", h.name()], value, cx);
    }

    fn drop_key(&mut self, h: Hotkey, i: usize, cx: &mut Context<Self>) {
        let map = self.keymap();
        let mut keys = map.chords(h).to_vec();
        if i < keys.len() {
            keys.remove(i);
            self.write_keys(&map, h, &keys, cx);
        }
    }

    fn reset_key(&mut self, h: Hotkey, cx: &mut Context<Self>) {
        self.stop_recording();
        self.change(&["shortcuts", h.name()], None, cx);
    }

    fn reset_all_keys(&mut self, cx: &mut Context<Self>) {
        if !self.keyboard.reset_all {
            self.stop_recording();
            self.keyboard.reset_all = true;
            return cx.notify();
        }
        self.stop_recording();
        self.change(&["shortcuts"], None, cx);
    }

    fn record(
        &mut self,
        h: Hotkey,
        at: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keyboard = Keyboard {
            recording: Some((h, at)),
            ..Keyboard::default()
        };
        // Keys come to the page, whatever had them.
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn keyboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        let os = Os::this();
        let map = self.keymap();
        let kb = &self.keyboard;
        let mut list = div().flex().flex_col();
        for (n, h) in Hotkey::ALL.into_iter().enumerate() {
            let recording = kb.recording.filter(|r| r.0 == h).map(|r| r.1);
            let mut keys = div().flex().flex_wrap().items_center().gap(px(6.));
            for (i, c) in map.chords(h).iter().enumerate() {
                keys = keys.child(if recording == Some(Some(i)) {
                    waiting()
                } else {
                    chip(c.label(os))
                        .id(("key", n * 16 + i))
                        .debug_selector(|| format!("key:{h:?}:{i}"))
                        .cursor_pointer()
                        .hover(|s| s.border_color(rgb(colors().accent)))
                        .on_click(
                            cx.listener(move |v, _, window, cx| v.record(h, Some(i), window, cx)),
                        )
                        .child(
                            div()
                                .id(("key-drop", n * 16 + i))
                                .ml(px(4.))
                                .px(px(3.))
                                .rounded(px(3.))
                                .text_color(rgb(colors().muted))
                                .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
                                .child("×")
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    cx.stop_propagation();
                                    v.stop_recording();
                                    v.drop_key(h, i, cx);
                                })),
                        )
                        .into_any_element()
                });
            }
            if recording == Some(None) {
                keys = keys.child(waiting());
            } else if map.chords(h).is_empty() {
                keys = keys.child(
                    div()
                        .text_size(ui(12.))
                        .text_color(rgb(colors().muted))
                        .child(tr!("none")),
                );
            }
            let row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(190.))
                        .flex_shrink_0()
                        .text_size(ui(13.))
                        .child(hotkeys::label(h)),
                )
                .child(div().flex_1().min_w_0().child(keys))
                .child(
                    small_button_n("key-add", n, "+").on_click(
                        cx.listener(move |v, _, window, cx| v.record(h, None, window, cx)),
                    ),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex()
                        .justify_end()
                        .when(map.is_custom(h), |d| {
                            d.child(
                                small_button_n("key-reset", n, &tr!("Reset"))
                                    .on_click(cx.listener(move |v, _, _, cx| v.reset_key(h, cx))),
                            )
                        }),
                );
            let mut item = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .py(px(7.))
                .border_b_1()
                .border_color(rgb(colors().line))
                .child(row);
            if let Some((_, why)) = kb.refused.as_ref().filter(|r| r.0 == h) {
                item = item.child(
                    div()
                        .text_size(ui(12.))
                        .text_color(rgb(colors().warn))
                        .child(why.clone()),
                );
            }
            if let Some(a) = kb.asking.as_ref().filter(|a| a.hotkey == h) {
                item = item.child(self.asking(a, &map, cx));
            }
            list = list.child(item);
        }
        let mut fixed = div().flex().flex_col();
        for f in Fixed::ALL {
            let mut keys = div().flex().flex_wrap().gap(px(6.));
            for c in f.chords(os) {
                keys = keys.child(chip(c.label(os)));
            }
            fixed = fixed.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(5.))
                    .child(
                        div()
                            .w(px(190.))
                            .flex_shrink_0()
                            .text_size(ui(13.))
                            .text_color(rgb(colors().soft))
                            .child(hotkeys::fixed_label(f)),
                    )
                    .child(keys),
            );
        }
        let reset_all = if kb.reset_all {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(ui(12.))
                        .text_color(rgb(colors().warn))
                        .child(tr!("Every shortcut back to its own keys?")),
                )
                .child(
                    small_button("keys-reset-all-yes", &tr!("Reset all"))
                        .border_color(rgb(colors().warn))
                        .on_click(cx.listener(|v, _, _, cx| v.reset_all_keys(cx))),
                )
                .child(
                    small_button("keys-reset-all-no", &tr!("Cancel")).on_click(cx.listener(
                        |v, _, _, cx| {
                            v.keyboard.reset_all = false;
                            cx.notify();
                        },
                    )),
                )
        } else {
            div().when(map.any_custom(), |d| {
                d.child(
                    small_button("keys-reset-all", &tr!("Reset all"))
                        .on_click(cx.listener(|v, _, _, cx| v.reset_all_keys(cx))),
                )
            })
        };
        page(&tr!("Keyboard"))
            .child(label(&tr!(
                "Window shortcuts never reach the remote host. Click a key to change it and press the new one (Esc stops); + gives a shortcut one more key, × takes one away. They are kept in config.toml, under [shortcuts]."
            )))
            .child(div().flex().justify_end().min_h(px(28.)).child(reset_all))
            .child(div().max_w(px(640.)).child(list))
            .child(heading(&tr!("In a pane, cannot be changed")))
            .child(div().max_w(px(640.)).child(fixed))
    }

    /// Under a shortcut: the key pressed has an owner, or the terminal's
    /// programs use it; take it?
    fn asking(&self, a: &Asking, map: &Keymap, cx: &mut Context<Self>) -> gpui::Div {
        let os = Os::this();
        let key = a.chord.label(os);
        let mut said = div().flex().flex_col().gap(px(4.));
        let mut yes = tr!("Use it anyway");
        if let Some(b) = a.from {
            let old = a.at.and_then(|i| map.chords(a.hotkey).get(i));
            said = said.child(match swapped(old, b, map.chords(b)) {
                Some(o) => {
                    yes = tr!("Swap");
                    tr!(
                        "{key} is {what}'s. Swap: {what} gets {old}.",
                        key = key.clone(),
                        what = hotkeys::label(b),
                        old = o.label(os)
                    )
                }
                None => {
                    yes = tr!("Move it here");
                    tr!(
                        "{key} is {what}'s. Move it here: {what} loses it.",
                        key = key.clone(),
                        what = hotkeys::label(b)
                    )
                }
            });
        }
        if a.shell {
            said = said.child(tr!(
                "Programs in the terminal use {key} (as Ctrl+R searches the shell's history, Alt+B goes back a word): taken here, they never get it.",
                key = key
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .rounded(px(6.))
            .bg(rgb(colors().warn_bg))
            .border_1()
            .border_color(rgb(colors().warn_edge))
            .text_size(ui(12.))
            .text_color(rgb(colors().warn_text))
            .child(said)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(small_button("key-ask-yes", &yes).on_click(cx.listener(
                        |v, _, _, cx| {
                            if let Some(a) = v.keyboard.asking.take() {
                                v.give(a.hotkey, a.at, a.chord, a.from, cx);
                            }
                        },
                    )))
                    .child(
                        small_button("key-ask-no", &tr!("Cancel")).on_click(cx.listener(
                            |v, _, _, cx| {
                                v.keyboard.asking = None;
                                cx.notify();
                            },
                        )),
                    ),
            )
    }
}

/// The key `b` gets for the one it gives up: the key replaced, if one was,
/// fits `b`, and is not one of `b`'s already.
fn swapped(old: Option<&Chord>, b: Hotkey, theirs: &[Chord]) -> Option<Chord> {
    old.filter(|o| o.is_digit() == b.numbered() && !theirs.contains(o))
        .cloned()
}

fn chip(text: String) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .h(px(24.))
        .px(px(8.))
        .rounded(px(4.))
        .bg(rgb(colors().chip))
        .border_1()
        .border_color(rgb(colors().edge))
        .text_size(ui(12.))
        .child(text)
}

/// The chip of a key being taken.
fn waiting() -> gpui::AnyElement {
    chip(tr!("Press keys…"))
        .border_color(rgb(colors().accent))
        .text_color(rgb(colors().accent))
        .into_any_element()
}

/// A small button of a row: its id numbered by the row.
fn small_button_n(id: &'static str, n: usize, text: &str) -> gpui::Stateful<gpui::Div> {
    small_button((id, n), text)
}

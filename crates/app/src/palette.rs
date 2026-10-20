//! The palette (Ctrl+Shift+K, Cmd+K on macOS): type to find a host and press
//! Enter, connect to an address not in any list, or run an action. The same
//! panel picks a theme, previewing each as it is highlighted.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render,
    ScrollHandle, SharedString, Subscription, Window, div, prelude::*, px, rgb,
};
use shellwing_i18n::tr;

use crate::colors::colors;
use crate::input::{Changed, TextInput};
use crate::session::Status;
use crate::sidebar::dot;

/// What the palette asks the workspace to do.
pub enum PaletteEvent {
    Dismiss,
    Open(String),
    Act(Action),
    /// A theme is highlighted: show it, without saving.
    Preview(String),
    Pick(String),
    /// Forget a saved secret (confirmed by a second Enter).
    Forget(String),
    /// Put this on the clipboard.
    Copy(String),
    /// What was typed in answer to a question.
    Answer(String),
    /// An entry chosen for what the palette was opened for (see [`Palette::chooser`]).
    Chose(String),
    /// Run a snippet (by id): in the pane, or in every pane of the tab.
    Snippet {
        id: String,
        all: bool,
    },
    /// Load an environment (by id) into the focused pane.
    LoadEnv(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Duplicate,
    SplitRight,
    SplitDown,
    ChangeTheme,
    Files,
    Find,
    /// Every open tab, by host.
    Tabs,
    /// Log the focused pane to a file, or stop.
    Log,
    /// Save the focused pane's text, history included: plain, or an HTML
    /// page with its colors.
    SaveOutput {
        html: bool,
    },
    NewHost,
    NewGroup,
    EditHost(crate::workspace::EditTarget),
    Settings,
    Forward,
    /// Unload the ones loaded there.
    UnloadEnv,
    ToggleHosts,
    /// Type in every pane of the tab, or stop.
    Broadcast,
    CloseTab,
    EditSettings,
    EditHosts,
    ManageSecrets,
    SyncFolder,
    /// Settings › Sync, to type a WebDAV or S3 server's address.
    SyncServer,
    SyncNow,
    ShowRecoveryKey,
    /// Open the locked vault with its passphrase or recovery key.
    UnlockVault,
    /// Set or change the passphrase that keeps the vault's key.
    SetVaultPassphrase,
    RemoveVaultPassphrase,
    /// Put the vault's key in the system keychain.
    KeepVaultKey,
    /// Take it out, so every start asks for the passphrase.
    LeaveKeychain,
    /// Close the open vault until its passphrase is typed again.
    LockVault,
    /// Let Windows Hello or Touch ID open the vault, or stop.
    TogglePresence,
    /// Let a passphrase join new devices through the sync folder, or stop.
    LetPassphraseJoin,
    StopPassphraseJoin,
    StopSync,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Host(String),
    Recent(String),
    Connect(String),
    Action(Action),
    Theme(String),
    /// A vault item, by id.
    Secret(String),
    /// Text to copy.
    Copy(String),
    /// Something to read; Enter does nothing.
    Note,
    /// A host (its spec) to choose for something, such as installing a key.
    Choice(String),
    /// A snippet, by id.
    Snippet(String),
    /// An environment to load into the focused pane, by id.
    Env(String),
}

/// What part of the palette an entry is in, to jump to from the row of
/// badges under the field. Several groups can be one section (the
/// snippets of each category).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Hosts,
    Recent,
    Snippets,
    Environments,
    Actions,
    Settings,
    /// No badge.
    Other,
}

impl Part {
    fn label(self) -> String {
        match self {
            Part::Hosts => tr!("Hosts"),
            Part::Recent => tr!("Recent"),
            Part::Snippets => tr!("Snippets"),
            Part::Environments => tr!("Environments"),
            Part::Actions => tr!("Actions"),
            Part::Settings => tr!("Settings"),
            Part::Other => String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub group: SharedString,
    pub title: SharedString,
    pub detail: SharedString,
    pub status: Option<Status>,
    pub hint: SharedString,
    pub section: Part,
}

impl Entry {
    pub fn new(kind: Kind, group: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Entry {
            kind,
            group: group.into(),
            title: title.into(),
            detail: SharedString::default(),
            status: None,
            hint: SharedString::default(),
            section: Part::Other,
        }
    }

    pub fn section(mut self, s: Part) -> Self {
        self.section = s;
        self
    }

    pub fn detail(mut self, d: impl Into<SharedString>) -> Self {
        self.detail = d.into();
        self
    }

    pub fn hint(mut self, h: impl Into<SharedString>) -> Self {
        self.hint = h.into();
        self
    }

    pub fn status(mut self, s: Option<Status>) -> Self {
        self.status = s;
        self
    }
}

/// How well `query` (lowercase) matches: lower is better, None is no match.
/// The title's start beats the title, which beats the detail; letters in order
/// with gaps (`dbp` for `db-primary`) come last.
fn score(e: &Entry, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let title = e.title.to_lowercase();
    let all = format!("{title} {}", e.detail.to_lowercase());
    if title.starts_with(query) {
        Some(0)
    } else if title.contains(query) {
        Some(1)
    } else if all.contains(query) {
        Some(2)
    } else {
        let mut rest = all.chars();
        query.chars().all(|q| rest.any(|c| c == q)).then_some(3)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Connect,
    Themes,
    Secrets,
    /// Text to copy and notes.
    Show,
    /// A question: Enter answers with what is typed.
    Ask,
    /// Hosts to choose one of, for something other than connecting.
    Choose,
}

pub struct Palette {
    input: Entity<TextInput>,
    mode: Mode,
    entries: Vec<Entry>,
    shown: Vec<Entry>,
    selected: usize,
    /// Nothing is typed, so the list keeps its groups.
    unfiltered: bool,
    /// The entry one Enter away from being forgotten.
    armed: Option<usize>,
    scroll: ScrollHandle,
    _changed: Subscription,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Palette {
    /// Hosts, recent connections and actions, plus "connect to" whatever
    /// is typed; opened at `at`'s first entry, at the top, when given.
    pub fn connect(entries: Vec<Entry>, at: Option<Part>, cx: &mut Context<Self>) -> Self {
        let mut p = Self::build(
            Mode::Connect,
            entries,
            0,
            &tr!("Find a host, or type user@host"),
            cx,
        );
        if let Some(s) = at {
            p.jump(s, cx);
        }
        p
    }

    /// The sections of what is listed, in order: the badges under the field.
    fn sections(&self) -> Vec<Part> {
        let mut out: Vec<Part> = vec![];
        for e in &self.shown {
            if e.section != Part::Other && !out.contains(&e.section) {
                out.push(e.section);
            }
        }
        out
    }

    /// Selects the first entry of section `s`, with its heading at the top
    /// of the list.
    fn jump(&mut self, s: Part, cx: &mut Context<Self>) {
        let Some(ix) = self.shown.iter().position(|e| e.section == s) else {
            return;
        };
        self.selected = ix;
        self.armed = None;
        let row = self.row_index(ix);
        // The heading above it, while there are headings.
        let row = if self.headers() {
            row.saturating_sub(1)
        } else {
            row
        };
        self.scroll.scroll_to_top_of_item(row);
        cx.notify();
    }

    /// The next section (`by` 1) or the one before (-1), from the selected
    /// entry's, around.
    fn step_section(&mut self, by: isize, cx: &mut Context<Self>) {
        let sections = self.sections();
        if sections.is_empty() {
            return;
        }
        let now = self.shown.get(self.selected).map(|e| e.section);
        let at = now
            .and_then(|s| sections.iter().position(|x| *x == s))
            .map_or(0, |i| {
                (i as isize + by).rem_euclid(sections.len() as isize) as usize
            });
        self.jump(sections[at], cx);
    }

    /// Entries to read or copy, such as the recovery key.
    pub fn show(entries: Vec<Entry>, cx: &mut Context<Self>) -> Self {
        Self::build(Mode::Show, entries, 0, "", cx)
    }

    /// A question, with notes under the field; Enter answers.
    pub fn ask(placeholder: &str, notes: Vec<Entry>, cx: &mut Context<Self>) -> Self {
        Self::build(Mode::Ask, notes, 0, placeholder, cx)
    }

    /// Like [`Palette::ask`], typing a secret: the field shows dots.
    pub fn ask_secret(placeholder: &str, notes: Vec<Entry>, cx: &mut Context<Self>) -> Self {
        let mut p = Self::build(Mode::Ask, notes, 0, placeholder, cx);
        p.input = cx.new(|cx| TextInput::new(placeholder.to_string(), cx).masked());
        p._changed = cx.subscribe(&p.input, |p, _, _: &Changed, cx| p.refilter(cx));
        p
    }

    /// Saved passwords and passphrases, to forget.
    pub fn secrets(entries: Vec<Entry>, cx: &mut Context<Self>) -> Self {
        Self::build(
            Mode::Secrets,
            entries,
            0,
            &tr!("Find a saved password or passphrase"),
            cx,
        )
    }

    /// Hosts to choose one of (entries of [`Kind::Choice`]), for what the
    /// field's placeholder says.
    pub fn chooser(placeholder: &str, entries: Vec<Entry>, cx: &mut Context<Self>) -> Self {
        Self::build(Mode::Choose, entries, 0, placeholder, cx)
    }

    /// Themes, starting on `current`.
    pub fn themes(entries: Vec<Entry>, current: &str, cx: &mut Context<Self>) -> Self {
        let at = entries
            .iter()
            .position(|e| e.kind == Kind::Theme(current.to_string()))
            .unwrap_or(0);
        Self::build(Mode::Themes, entries, at, &tr!("Find a theme"), cx)
    }

    fn build(
        mode: Mode,
        entries: Vec<Entry>,
        selected: usize,
        placeholder: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new(placeholder.to_string(), cx));
        let changed = cx.subscribe(&input, |p, _, _: &Changed, cx| p.refilter(cx));
        let mut p = Palette {
            input,
            mode,
            shown: entries.clone(),
            entries,
            selected,
            unfiltered: true,
            armed: None,
            scroll: ScrollHandle::new(),
            _changed: changed,
        };
        p.refilter(cx);
        p.selected = selected.min(p.shown.len().saturating_sub(1));
        p.scroll.scroll_to_item(p.row_index(p.selected));
        p
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let typed = self.input.read(cx).text().trim().to_string();
        if self.mode == Mode::Ask {
            // The notes stay; what is typed is the answer, not a filter.
            self.shown = self.entries.clone();
            self.unfiltered = true;
            cx.notify();
            return;
        }
        let (actions_only, query) = match typed.strip_prefix('>') {
            Some(rest) if self.mode == Mode::Connect => (true, rest.trim().to_lowercase()),
            _ => (false, typed.to_lowercase()),
        };
        let mut scored: Vec<(u8, usize, Entry)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !actions_only || matches!(e.kind, Kind::Action(_)))
            .filter_map(|(i, e)| score(e, &query).map(|s| (s, i, e.clone())))
            .collect();
        if !query.is_empty() {
            scored.sort_by_key(|(s, i, _)| (*s, *i));
        }
        let best = scored.first().map(|(s, ..)| *s);
        let mut shown: Vec<Entry> = scored.into_iter().map(|(.., e)| e).collect();
        if self.mode == Mode::Connect && !actions_only && !typed.is_empty() {
            let connect = Entry::new(
                Kind::Connect(typed.clone()),
                tr!("Connect"),
                tr!("Connect to {typed}", typed = &typed),
            )
            .detail(tr!("a new address; a Host block in ~/.ssh/config applies"))
            .hint("Enter");
            // After a clearly matching host, before a weak one.
            let at = if best.is_some_and(|s| s <= 1) { 1 } else { 0 };
            shown.insert(at.min(shown.len()), connect);
        }
        self.shown = shown;
        self.unfiltered = typed.is_empty();
        self.selected = 0;
        self.highlighted(cx);
        cx.notify();
    }

    fn highlighted(&mut self, cx: &mut Context<Self>) {
        self.armed = None;
        self.scroll.scroll_to_item(self.row_index(self.selected));
        if self.mode == Mode::Themes
            && let Some(Kind::Theme(id)) = self.shown.get(self.selected).map(|e| e.kind.clone())
        {
            cx.emit(PaletteEvent::Preview(id));
        }
    }

    /// The list's child index of entry `ix`, counting the group headers above it.
    fn row_index(&self, ix: usize) -> usize {
        let headers = self.headers();
        let mut seen = 0;
        let mut last = SharedString::default();
        for (i, e) in self.shown.iter().enumerate() {
            if headers && e.group != last {
                seen += 1;
                last = e.group.clone();
            }
            if i == ix {
                return i + seen;
            }
        }
        ix
    }

    /// Group headers only while nothing is typed; a filtered list is one ranking.
    fn headers(&self) -> bool {
        matches!(self.mode, Mode::Connect | Mode::Choose | Mode::Themes) && self.unfiltered
    }

    /// `all`: Alt+Enter, a snippet for every pane of the tab.
    fn choose(&mut self, ix: usize, all: bool, cx: &mut Context<Self>) {
        let Some(e) = self.shown.get(ix) else {
            return;
        };
        cx.emit(match &e.kind {
            Kind::Host(spec) | Kind::Recent(spec) | Kind::Connect(spec) => {
                PaletteEvent::Open(spec.clone())
            }
            Kind::Action(a) => PaletteEvent::Act(a.clone()),
            Kind::Theme(id) => PaletteEvent::Pick(id.clone()),
            // Forgetting cannot be undone, so it takes a second Enter.
            Kind::Secret(_) if self.armed != Some(ix) => {
                self.armed = Some(ix);
                self.selected = ix;
                cx.notify();
                return;
            }
            Kind::Secret(id) => PaletteEvent::Forget(id.clone()),
            Kind::Copy(text) => PaletteEvent::Copy(text.clone()),
            Kind::Choice(spec) => PaletteEvent::Chose(spec.clone()),
            Kind::Snippet(id) => PaletteEvent::Snippet {
                id: id.clone(),
                all,
            },
            Kind::Env(id) => PaletteEvent::LoadEnv(id.clone()),
            Kind::Note => return,
        });
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let n = self.shown.len();
        let step = |by: isize, p: &mut Self, cx: &mut Context<Self>| {
            if n > 0 {
                p.selected = (p.selected as isize + by).rem_euclid(n as isize) as usize;
                p.highlighted(cx);
                cx.notify();
            }
        };
        match k.key.as_str() {
            "escape" => cx.emit(PaletteEvent::Dismiss),
            "enter" if self.mode == Mode::Ask => {
                let answer = self.input.read(cx).text().trim().to_string();
                if !answer.is_empty() {
                    cx.emit(PaletteEvent::Answer(answer));
                }
            }
            "enter" => self.choose(self.selected, k.modifiers.alt, cx),
            "down" => step(1, self, cx),
            "up" => step(-1, self, cx),
            "n" if k.modifiers.control => step(1, self, cx),
            "p" if k.modifiers.control => step(-1, self, cx),
            "tab" if self.mode == Mode::Connect => {
                self.step_section(if k.modifiers.shift { -1 } else { 1 }, cx)
            }
            "pagedown" => step(8, self, cx),
            "pageup" => step(-8, self, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Palette {
    /// The row of badges under the field, one a section listed; the
    /// selected entry's lit. A click jumps there.
    fn badges(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let sections = self.sections();
        if self.mode != Mode::Connect || sections.len() < 2 {
            return None;
        }
        let now = self.shown.get(self.selected).map(|e| e.section);
        let mut row = div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .px_4()
            .py(px(8.))
            .border_b_1()
            .border_color(rgb(colors().line));
        for (i, s) in sections.into_iter().enumerate() {
            let on = now == Some(s);
            row = row.child(
                div()
                    .id(("palette-section", i))
                    .px(px(9.))
                    .py(px(2.))
                    .rounded_full()
                    .border_1()
                    .text_xs()
                    .cursor_pointer()
                    .when(on, |d| {
                        d.border_color(rgb(colors().accent))
                            .bg(rgb(colors().selected))
                            .text_color(rgb(colors().text))
                    })
                    .when(!on, |d| {
                        d.border_color(rgb(colors().line))
                            .text_color(rgb(colors().muted))
                            .hover(|h| h.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
                    })
                    .on_click(cx.listener(move |p, _, window, cx| {
                        p.jump(s, cx);
                        // Back to typing.
                        window.focus(&p.input.focus_handle(cx), cx);
                    }))
                    .child(s.label()),
            );
        }
        Some(row)
    }
}

impl Focusable for Palette {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let headers = self.headers();
        let mut list = div()
            .id("palette-list")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .max_h(px(420.))
            .flex()
            .flex_col()
            .p_1();
        let mut last = SharedString::default();
        for (i, e) in self.shown.iter().enumerate() {
            if headers && e.group != last {
                last = e.group.clone();
                list = list.child(
                    div()
                        .px_3()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().muted))
                        .child(e.group.clone()),
                );
            }
            let selected = i == self.selected;
            let armed = self.armed == Some(i);
            let has_dot = matches!(e.kind, Kind::Host(_) | Kind::Choice(_));
            list = list.child(
                div()
                    .id(("palette-entry", i))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py(px(7.))
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(rgb(colors().selected)))
                    .when(!selected, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                    .on_click(cx.listener(move |p, e: &gpui::ClickEvent, _, cx| {
                        p.choose(i, e.modifiers().alt, cx)
                    }))
                    .when(has_dot, |d| d.child(dot(e.status)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_color(rgb(colors().text))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(e.title.clone()),
                            )
                            .when(!e.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(colors().muted))
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .child(e.detail.clone()),
                                )
                            }),
                    )
                    .when(armed, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(rgb(colors().bad))
                                .child(tr!("Enter again to forget")),
                        )
                    })
                    .when(!armed && selected && !e.hint.is_empty(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(rgb(colors().muted))
                                .child(e.hint.clone()),
                        )
                    }),
            );
        }
        if self.shown.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_color(rgb(colors().muted))
                    .child(tr!("Nothing matches.")),
            );
        }
        let footer = match self.mode {
            Mode::Connect => {
                tr!("↑↓ move   Tab next group   Enter open   Esc close   type > for actions only")
            }
            Mode::Themes => tr!("↑↓ preview   Enter keep   Esc go back"),
            Mode::Secrets => {
                tr!("↑↓ move   Enter twice forget   Esc close   secrets are never shown")
            }
            Mode::Show => tr!("Enter copies   Esc close"),
            Mode::Ask => tr!("Enter answers   Esc cancels"),
            Mode::Choose => tr!("↑↓ move   Enter choose   Esc cancel"),
        };
        div()
            .w(px(640.))
            .flex()
            .flex_col()
            .bg(rgb(colors().raised))
            .border_1()
            .border_color(rgb(colors().edge))
            .rounded_lg()
            .shadow_lg()
            .overflow_hidden()
            .text_sm()
            .on_key_down(cx.listener(Self::on_key))
            // Clicks inside must not reach the backdrop, which dismisses.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(48.))
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .text_base()
                    .text_color(rgb(colors().text))
                    .child(div().text_color(rgb(colors().muted)).child("›"))
                    .child(self.input.clone())
                    .child(div().text_xs().text_color(rgb(colors().muted)).child("Esc")),
            )
            .children(self.badges(cx))
            .child(list)
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .text_xs()
                    .text_color(rgb(colors().muted))
                    .child(footer),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str, detail: &str) -> Entry {
        Entry::new(Kind::Host(name.into()), tr!("Hosts"), name.to_string())
            .detail(detail.to_string())
    }

    #[test]
    fn ranks_title_start_then_title_then_detail_then_letters() {
        let db = host("db-primary", "postgres@10.0.2.8");
        assert_eq!(score(&db, "db"), Some(0));
        assert_eq!(score(&db, "primary"), Some(1));
        assert_eq!(score(&db, "postgres"), Some(2));
        assert_eq!(score(&db, "dbpr"), Some(3));
        assert_eq!(score(&db, "xyz"), None);
        assert_eq!(score(&db, ""), Some(0));
    }
}

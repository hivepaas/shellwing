//! The cloud beside Settings: how sync stands, as its color says, and the
//! panel that hangs from it: how it goes and what travels, or, without sync,
//! a line on shellwing cloud. Settings › Sync tells the rest, with the parts
//! here: the status card, shellwing cloud in full (sign in, and every device
//! has it) and beside a place set up by hand. shellwing cloud itself is not
//! there yet: its buttons wait for it.

use std::time::SystemTime;

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Render,
    Window, div, prelude::*, px, rgb, svg,
};

use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::look::ui;

/// How sync stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Standing {
    /// Nothing set up.
    Off,
    /// Synced (when, if this run synced yet).
    Synced(Option<SystemTime>),
    /// A sync is running.
    Syncing,
    /// The last sync failed: what it said.
    Failed(String),
    /// Nothing syncs while the vault is locked.
    Paused,
}

impl Standing {
    /// The cloud's color.
    pub fn color(&self) -> u32 {
        let c = colors();
        match self {
            Standing::Off => c.sync_off,
            Standing::Synced(_) | Standing::Syncing => c.sync_ok,
            Standing::Failed(_) => c.sync_bad,
            Standing::Paused => c.sync_paused,
        }
    }

    /// A problem gets a dot too: not the color alone tells it.
    pub fn dotted(&self) -> bool {
        matches!(self, Standing::Failed(_))
    }
}

/// What travels with the vault.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub hosts: usize,
    pub passwords: usize,
    pub keys: usize,
    pub snippets: usize,
    pub environments: usize,
    pub files: usize,
    pub notes: usize,
}

impl Counts {
    /// "12 hosts · 30 passwords · 4 keys": the hosts, and what else there is.
    pub fn said(&self) -> String {
        let mut parts = vec![tr_n!(self.hosts, "{n} host", "{n} hosts")];
        let rest = [
            (
                self.passwords,
                tr_n!(self.passwords, "{n} password", "{n} passwords"),
            ),
            (self.keys, tr_n!(self.keys, "{n} key", "{n} keys")),
            (
                self.snippets,
                tr_n!(self.snippets, "{n} snippet", "{n} snippets"),
            ),
            (
                self.environments,
                tr_n!(self.environments, "{n} environment", "{n} environments"),
            ),
            (
                self.files,
                tr_n!(self.files, "{n} config file", "{n} config files"),
            ),
            (self.notes, tr_n!(self.notes, "{n} note", "{n} notes")),
        ];
        parts.extend(rest.into_iter().filter(|(n, _)| *n > 0).map(|(_, s)| s));
        parts.join(" · ")
    }
}

/// How sync stands, from the workspace: for the panel and Settings › Sync.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncSnap {
    pub standing: Standing,
    /// Where it syncs, as said to the user; None: nowhere.
    pub place: Option<String>,
    /// What travels; None while the vault is not open.
    pub counts: Option<Counts>,
}

/// What the panel asks the workspace to do.
pub enum SyncPanelEvent {
    Dismiss,
    SyncNow,
    /// Settings › Sync: shellwing cloud, and the places of one's own.
    OpenSettings,
    Unlock,
}

pub struct SyncPanel {
    snap: SyncSnap,
    focus: FocusHandle,
}

impl EventEmitter<SyncPanelEvent> for SyncPanel {}

impl Focusable for SyncPanel {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

type Click = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static>;

impl SyncPanel {
    pub fn new(snap: SyncSnap, cx: &mut Context<Self>) -> Self {
        SyncPanel {
            snap,
            focus: cx.focus_handle(),
        }
    }

    pub fn set_snap(&mut self, snap: SyncSnap, cx: &mut Context<Self>) {
        if snap != self.snap {
            self.snap = snap;
            cx.notify();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "escape" {
            cx.emit(SyncPanelEvent::Dismiss);
            cx.stop_propagation();
        }
    }

    /// Nothing set up: a line on shellwing cloud, which Settings tells.
    fn not_syncing(&self, cx: &mut Context<Self>) -> gpui::Div {
        let c = colors();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(20.))
                    .child(header(c.sync_off, &tr!("NOT SYNCING")))
                    .child(
                        div()
                            .text_size(ui(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.3))
                            .child(tr!("Your hosts on every device. Just sign in.")),
                    )
                    .child(
                        div()
                            .text_size(ui(12.5))
                            .line_height(gpui::relative(1.45))
                            .text_color(rgb(c.soft))
                            .child(tr!(
                                "Hosts, passwords, keys and snippets stay on this computer for now."
                            )),
                    )
                    .child(
                        div()
                            .id("sync-see-cloud")
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .bg(rgb(c.accent))
                            .hover(|s| s.bg(rgb(c.accent_hover)))
                            .text_color(rgb(c.on_accent))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::OpenSettings)),
                            )
                            .child(tr!("See shellwing cloud")),
                    ),
            )
            .child(footer(
                Some(tr!("A folder, WebDAV, S3 or Git of your own:")),
                &tr!("Settings › Sync"),
                cx,
            ))
    }

    /// Set up: how it goes, what travels, and Settings for the rest.
    fn syncing(&self, place: &str, cx: &mut Context<Self>) -> gpui::Div {
        let s = &self.snap.standing;
        let card = status_card(
            s,
            place,
            Box::new(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::SyncNow))),
            Box::new(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::Unlock))),
        );
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(18.))
                    .child(header(s.color(), &tr!("SYNC")))
                    .child(card)
                    .when_some(self.snap.counts, |d, n| {
                        d.child(
                            div()
                                .text_size(ui(12.5))
                                .text_color(rgb(colors().soft))
                                .child(n.said()),
                        )
                    }),
            )
            .child(footer(None, &tr!("Open Settings › Sync"), cx))
    }
}

/// How sync stands, in a card: its dot, when it last synced (or why not)
/// and where, and what to do: sync now, try again, or unlock the vault.
pub fn status_card(standing: &Standing, place: &str, sync: Click, unlock: Click) -> gpui::Div {
    let c = colors();
    let s = standing;
    let (title, detail) = match s {
        Standing::Synced(Some(t)) => (
            tr!("Synced · {when}", when = ago(*t)),
            tr!("With {place}", place = place),
        ),
        Standing::Synced(None) => (tr!("Set up"), tr!("With {place}", place = place)),
        Standing::Syncing => (tr!("Syncing…"), tr!("With {place}", place = place)),
        Standing::Failed(e) => (tr!("Not synced"), format!("{place}: {e}")),
        Standing::Paused => (
            tr!("Paused: the vault is locked"),
            tr!("With {place}, once it is open", place = place),
        ),
        Standing::Off => (String::new(), String::new()),
    };
    let (edge, ground, said) = match s {
        Standing::Failed(_) => (c.warn_edge, c.warn_bg, c.warn_text),
        Standing::Paused => (c.edge, c.chip, c.soft),
        _ => (c.edge, c.good_bg, c.soft),
    };
    let act = if *s == Standing::Paused {
        small("sync-unlock", &tr!("Unlock…")).on_click(unlock)
    } else {
        small(
            "sync-now",
            &if matches!(s, Standing::Failed(_)) {
                tr!("Try again")
            } else {
                tr!("Sync now")
            },
        )
        .on_click(sync)
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(9.))
        .border_1()
        .border_color(rgb(edge))
        .bg(rgb(ground))
        .child(
            div()
                .flex_shrink_0()
                .size(px(9.))
                .rounded_full()
                .bg(rgb(s.color())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_size(ui(12.)).text_color(rgb(said)).child(detail)),
        )
        .child(act)
}

/// shellwing cloud, told in full: what it does, signing in, the plans.
pub fn invitation() -> gpui::Div {
    let c = colors();
    let point = |text: &str| {
        div()
            .flex()
            .gap(px(10.))
            .child(
                svg()
                    .path("icons/check.svg")
                    .flex_shrink_0()
                    .mt(px(2.))
                    .size(px(15.))
                    .text_color(rgb(c.sync_ok)),
            )
            // Its own width, so a long line wraps rather than runs out.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(rgb(c.soft))
                    .child(text.to_string()),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(header(c.sync_off, "SHELLWING CLOUD"))
        .child(
            div()
                .text_size(ui(19.))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(gpui::relative(1.25))
                .child(tr!("Your hosts on every device. Just sign in.")),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .text_size(ui(13.))
                .line_height(gpui::relative(1.45))
                .child(point(&tr!(
                    "Hosts, saved passwords, keys and snippets follow you to every computer."
                )))
                .child(point(&tr!(
                    "Nothing to set up: no folder, no server, no key to type on a new device."
                )))
                .child(point(&tr!(
                    "Encrypted on your devices: shellwing never sees your secrets."
                ))),
        )
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(waiting_button("cloud-create", &tr!("Create an account"), true).flex_1())
                .child(waiting_button("cloud-sign-in", &tr!("Sign in"), false)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(ui(12.))
                .text_color(rgb(c.muted))
                .child(div().flex_1().h(px(1.)).bg(rgb(c.edge)))
                .child(tr!("or continue with"))
                .child(div().flex_1().h(px(1.)).bg(rgb(c.edge))),
        )
        .child(sso_row())
        .child(coming())
        .child(plans())
}

/// shellwing cloud beside a place set up by hand.
pub fn aside() -> gpui::Div {
    let c = colors();
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(rgb(c.line))
        .bg(rgb(c.surface))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    svg()
                        .path("icons/cloud.svg")
                        .size(px(17.))
                        .text_color(rgb(c.accent)),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr!("Rather not keep it yourself?")),
                ),
        )
        .child(
            div()
                .text_size(ui(12.5))
                .line_height(gpui::relative(1.45))
                .text_color(rgb(c.soft))
                .child(tr!("shellwing cloud keeps it for you: sign in on each device, and that is all. Still end-to-end encrypted.")),
        )
        .child(sso_row())
        .child(coming())
        .child(plans())
}

/// A section's small title, with the cloud.
fn header(color: u32, title: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            svg()
                .path("icons/cloud.svg")
                .size(px(20.))
                .text_color(rgb(color)),
        )
        .child(
            div()
                .text_size(ui(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(colors().muted))
                .child(title.to_string()),
        )
}

/// A button of shellwing cloud's, waiting for it: shown as it will be,
/// dimmed, and it does nothing yet.
fn waiting_button(id: &'static str, label: &str, main: bool) -> gpui::Stateful<gpui::Div> {
    waiting(id, main).child(label.to_string())
}

/// [`waiting_button`] before its label.
fn waiting(id: &'static str, main: bool) -> gpui::Stateful<gpui::Div> {
    let c = colors();
    let b = div()
        .id(id)
        .h(px(36.))
        .px(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .rounded(px(8.))
        .opacity(0.55)
        .text_size(ui(13.));
    if main {
        b.bg(rgb(c.accent))
            .text_color(rgb(c.on_accent))
            .font_weight(FontWeight::SEMIBOLD)
    } else {
        b.border_1()
            .border_color(rgb(c.edge))
            .text_color(rgb(c.text))
    }
}

/// Signing in with another account, one per provider.
fn sso_row() -> gpui::Div {
    let c = colors();
    let provider = |id: &'static str, mark: &str, name: &str| {
        waiting(id, false)
            .flex_1()
            .bg(rgb(c.chip))
            // The provider's mark before its name.
            .child(
                div()
                    .size(px(18.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(c.text))
                    .text_color(rgb(c.surface))
                    .text_size(ui(10.))
                    .font_weight(FontWeight::BOLD)
                    .child(mark.to_string()),
            )
            .child(name.to_string())
    };
    div()
        .flex()
        .gap(px(8.))
        .child(provider("sso-google", "G", "Google"))
        .child(provider("sso-github", "GH", "GitHub"))
}

/// shellwing cloud is not open yet.
fn coming() -> gpui::Div {
    div()
        .text_size(ui(12.))
        .text_color(rgb(colors().muted))
        .child(tr!("shellwing cloud is coming: signing in opens with it."))
}

/// The plans, in a line.
fn plans() -> gpui::Div {
    let c = colors();
    // Each its share of the row, its text wrapping there: a language's
    // longer words do not push the last one out.
    let plan = |name: &str, what: &str| {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(c.text))
                    .child(name.to_string()),
            )
            .child(div().text_color(rgb(c.soft)).child(what.to_string()))
    };
    div()
        .flex()
        .justify_between()
        .gap(px(8.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(8.))
        .bg(rgb(c.page))
        .text_size(ui(12.))
        .child(plan("Free", &tr_n!(10, "{n} host", "{n} hosts")))
        .child(plan(
            "Starter",
            &tr_n!(20, "{n} host · $3.99/mo", "{n} hosts · $3.99/mo"),
        ))
        .child(plan("Pro", &tr!("unlimited · $7.99/mo")))
}

/// The panel's foot: a word, and Settings › Sync to open.
fn footer(
    said: Option<String>,
    link: &str,
    cx: &mut Context<SyncPanel>,
) -> gpui::Stateful<gpui::Div> {
    let c = colors();
    div()
        .id("sync-settings-link")
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(20.))
        .py(px(11.))
        .border_t_1()
        .border_color(rgb(c.line))
        .bg(rgb(c.panel))
        // The panel's corners: its overflow hides no child's ground, so the
        // one at the bottom rounds itself, inside the panel's border.
        .rounded_b(px(11.))
        .text_size(ui(12.))
        .text_color(rgb(c.muted))
        .cursor_pointer()
        .on_click(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::OpenSettings)))
        .when_some(said, |d, s| d.child(s))
        .child(
            div()
                .text_color(rgb(c.accent))
                .hover(|s| s.text_color(rgb(c.accent_hover)))
                .child(link.to_string()),
        )
}

fn small(id: &'static str, label: &str) -> gpui::Stateful<gpui::Div> {
    let c = colors();
    div()
        .id(id)
        .flex_shrink_0()
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(c.edge))
        .bg(rgb(c.chip))
        .hover(|s| s.bg(rgb(c.hover)))
        .text_size(ui(12.5))
        .cursor_pointer()
        .child(label.to_string())
}

/// "2 minutes ago", for a sync.
pub fn ago(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match s {
        0..=59 => tr!("just now"),
        60..=119 => tr!("a minute ago"),
        120..=3599 => tr_n!(s / 60, "{n} minute ago", "{n} minutes ago"),
        3600..=7199 => tr!("an hour ago"),
        7200..=86399 => tr_n!(s / 3600, "{n} hour ago", "{n} hours ago"),
        _ => tr_n!(s / 86400, "{n} day ago", "{n} days ago"),
    }
}

impl Render for SyncPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let body = match self.snap.place.clone() {
            None => self.not_syncing(cx),
            Some(place) => self.syncing(&place, cx),
        };
        div()
            .id("sync-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(450.))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(c.edge))
            .bg(rgb(c.raised))
            .shadow_lg()
            .overflow_hidden()
            .text_size(ui(13.))
            .text_color(rgb(c.text))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sync_is_said_in_words_of_how_long_ago() {
        let then = |s: u64| SystemTime::now() - std::time::Duration::from_secs(s);
        assert_eq!(ago(then(5)), "just now");
        assert_eq!(ago(then(90)), "a minute ago");
        assert_eq!(ago(then(600)), "10 minutes ago");
        assert_eq!(ago(then(3 * 3600)), "3 hours ago");
        assert_eq!(ago(then(3 * 86400)), "3 days ago");
    }

    #[test]
    fn what_travels_is_said_without_what_there_is_none_of() {
        let n = Counts {
            hosts: 0,
            passwords: 1,
            notes: 3,
            ..Counts::default()
        };
        assert_eq!(n.said(), "0 hosts · 1 password · 3 notes");
    }

    #[test]
    fn only_a_problem_gets_a_dot() {
        assert!(Standing::Failed("offline".into()).dotted());
        assert!(!Standing::Paused.dotted() && !Standing::Off.dotted());
    }
}

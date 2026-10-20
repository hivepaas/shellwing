//! The cloud beside Settings: how sync stands, as its color says, and the
//! panel that hangs from it. Without sync it invites to shellwing cloud
//! (sign in, and every device has it); with a folder, WebDAV, S3 or Git set
//! up by hand it says how that goes. shellwing cloud itself is not there
//! yet: its buttons wait for it.

use std::time::SystemTime;

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Render,
    Window, div, prelude::*, px, rgb, svg,
};

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

/// What the panel shows, from the workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncSnap {
    pub standing: Standing,
    /// Where it syncs, as said to the user; None: nowhere.
    pub place: Option<String>,
    /// What travels: hosts, passwords, SSH keys, snippets.
    pub counts: Option<(usize, usize, usize, usize)>,
}

/// What the panel asks the workspace to do.
pub enum SyncPanelEvent {
    Dismiss,
    SyncNow,
    /// Settings › Sync, to set up a folder or a server.
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

    /// Nothing set up: what shellwing cloud would do.
    fn invitation(&self, cx: &mut Context<Self>) -> gpui::Div {
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
                .child(div().text_color(rgb(c.soft)).child(text.to_string()))
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(20.))
                    .child(header(c.sync_off, "SHELLWING CLOUD"))
                    .child(
                        div()
                            .text_size(ui(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.25))
                            .child("Your hosts on every device. Just sign in."),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .text_size(ui(13.))
                            .line_height(gpui::relative(1.45))
                            .child(point(
                                "Hosts, saved passwords, keys and snippets follow you to every computer.",
                            ))
                            .child(point(
                                "Nothing to set up: no folder, no server, no key to type on a new device.",
                            ))
                            .child(point(
                                "Encrypted on your devices: shellwing never sees your secrets.",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(waiting_button("cloud-create", "Create an account", true).flex_1())
                            .child(waiting_button("cloud-sign-in", "Sign in", false)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(ui(12.))
                            .text_color(rgb(c.muted))
                            .child(div().flex_1().h(px(1.)).bg(rgb(c.edge)))
                            .child("or continue with")
                            .child(div().flex_1().h(px(1.)).bg(rgb(c.edge))),
                    )
                    .child(sso_row())
                    .child(coming())
                    .child(plans()),
            )
            .child(footer(cx))
    }

    /// Set up by hand: how it goes, and shellwing cloud beside it.
    fn standing(&self, place: &str, cx: &mut Context<Self>) -> gpui::Div {
        let c = colors();
        let s = &self.snap.standing;
        let (title, detail) = match s {
            Standing::Synced(Some(t)) => (format!("Synced · {}", ago(*t)), format!("With {place}")),
            Standing::Synced(None) => ("Set up".to_string(), format!("With {place}")),
            Standing::Syncing => ("Syncing…".to_string(), format!("With {place}")),
            Standing::Failed(e) => ("Not synced".to_string(), format!("{place}: {e}")),
            Standing::Paused => (
                "Paused: the vault is locked".to_string(),
                format!("With {place}, once it is open"),
            ),
            Standing::Off => (String::new(), String::new()),
        };
        let (edge, ground) = match s {
            Standing::Failed(_) => (c.warn_edge, c.warn_bg),
            Standing::Paused => (c.edge, c.chip),
            _ => (c.edge, c.good_bg),
        };
        let paused = *s == Standing::Paused;
        let act = if paused {
            small("sync-unlock", "Unlock…")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::Unlock)))
        } else {
            small(
                "sync-now",
                if matches!(s, Standing::Failed(_)) {
                    "Try again"
                } else {
                    "Sync now"
                },
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::SyncNow)))
        };
        let counts = self.snap.counts.map(|(h, p, k, n)| {
            let n_of = |n: usize, one: &str, many: &str| {
                format!("{n} {}", if n == 1 { one } else { many })
            };
            format!(
                "{}, {}, {} and {}.",
                n_of(h, "host", "hosts"),
                n_of(p, "password", "passwords"),
                n_of(k, "key", "keys"),
                n_of(n, "snippet", "snippets")
            )
        });
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(18.))
                    .child(header(s.color(), "SYNC"))
                    .child(
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
                            .child(div().flex_shrink_0().size(px(9.)).rounded_full().bg(rgb(s.color())))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                                    .child(
                                        div()
                                            .text_size(ui(12.))
                                            .text_color(rgb(c.soft))
                                            .child(detail),
                                    ),
                            )
                            .child(act),
                    )
                    .when_some(counts, |d, n| {
                        d.child(div().text_size(ui(12.5)).text_color(rgb(c.soft)).child(n))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .p(px(14.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(rgb(c.line))
                            .bg(rgb(c.page))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(svg().path("icons/cloud.svg").size(px(17.)).text_color(rgb(c.accent)))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Rather not keep it yourself?"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui(12.5))
                                    .line_height(gpui::relative(1.45))
                                    .text_color(rgb(c.soft))
                                    .child("shellwing cloud keeps it for you: sign in on each device, and that is all. Still end-to-end encrypted."),
                            )
                            .child(sso_row())
                            .child(coming()),
                    ),
            )
            .child(footer(cx))
    }
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
        .text_size(ui(13.))
        .child(label.to_string());
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
        waiting_button(id, name, false)
            .flex_1()
            .bg(rgb(c.chip))
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
        .child("shellwing cloud is coming: signing in opens with it.")
}

/// The plans, in a line.
fn plans() -> gpui::Div {
    let c = colors();
    let plan = |name: &str, what: &str| {
        div()
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
        .child(plan("Free", "10 hosts"))
        .child(plan("Starter", "20 hosts · $2.99/mo"))
        .child(plan("Pro", "unlimited · $5.99/mo"))
}

/// The way to set it up by hand.
fn footer(cx: &mut Context<SyncPanel>) -> gpui::Stateful<gpui::Div> {
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
        .text_size(ui(12.))
        .text_color(rgb(c.muted))
        .cursor_pointer()
        .on_click(cx.listener(|_, _, _, cx| cx.emit(SyncPanelEvent::OpenSettings)))
        .child("A folder, WebDAV, S3 or Git of your own:")
        .child(
            div()
                .text_color(rgb(c.accent))
                .hover(|s| s.text_color(rgb(c.accent_hover)))
                .child("Settings › Sync"),
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
fn ago(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match s {
        0..=59 => "just now".into(),
        60..=119 => "a minute ago".into(),
        120..=3599 => format!("{} minutes ago", s / 60),
        3600..=7199 => "an hour ago".into(),
        7200..=86399 => format!("{} hours ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

impl Render for SyncPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let body = match self.snap.place.clone() {
            None => self.invitation(cx),
            Some(place) => self.standing(&place, cx),
        };
        div()
            .id("sync-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(400.))
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
    fn only_a_problem_gets_a_dot() {
        assert!(Standing::Failed("offline".into()).dotted());
        assert!(!Standing::Paused.dotted() && !Standing::Off.dotted());
    }
}

//! The Known hosts page of Settings: the servers known_hosts trusts, and
//! their keys, to find and remove. A host removed is asked about again on
//! the next connection.

use std::path::PathBuf;

use gpui::{
    Context, Entity, FocusHandle, FontWeight, Render, SharedString, Window, div, prelude::*, px,
    rgb,
};
use shellwing_i18n::{tr, tr_n};
use shellwing_ssh::known_hosts::{self, Entry};

use crate::colors::colors;
use crate::host_editor::{label_text, outline_button};
use crate::input::{Changed, TextInput};
use crate::look::ui;

pub struct KnownHostsView {
    focus: FocusHandle,
    path: Option<PathBuf>,
    entries: Vec<Entry>,
    filter: Entity<TextInput>,
    notice: Option<(SharedString, bool)>,
    mono: SharedString,
    _changed: gpui::Subscription,
}

/// What a typed name is, as known_hosts writes it: `host` and port 22, or
/// `host:port` / `[host]:port`.
fn host_port(typed: &str) -> (String, u16) {
    let t = typed.trim();
    let t = t
        .strip_prefix('[')
        .map_or(t.to_string(), |r| r.replacen("]:", ":", 1));
    match t.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse() {
            Ok(port) => (h.to_string(), port),
            Err(_) => (t, 22),
        },
        _ => (t, 22),
    }
}

impl KnownHostsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let filter =
            crate::input::said(cx, || tr!("Find a host, or type one to find hashed lines"));
        let changed = cx.subscribe(&filter, |_, _, _: &Changed, cx| cx.notify());
        let mut v = KnownHostsView {
            focus: cx.focus_handle(),
            path: known_hosts::default_path(),
            entries: vec![],
            filter,
            notice: None,
            mono: SharedString::default(),
            _changed: changed,
        };
        v.refresh(cx);
        v
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        match self.path.as_deref().map(known_hosts::entries) {
            Some(Ok(e)) => self.entries = e,
            Some(Err(e)) => {
                self.notice = Some((
                    tr!("Cannot read known_hosts: {e}", e = e.to_string()).into(),
                    true,
                ))
            }
            None => self.entries.clear(),
        }
        cx.notify();
    }

    fn remove(&mut self, line: usize, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.notice = Some(match known_hosts::remove_lines(&path, &[line]) {
            Ok(_) => (
                tr!(
                    "Removed line {line}: the next connection to it asks about its key. The file as it was is known_hosts.old.",
                    line = line
                )
                .into(),
                false,
            ),
            Err(e) => (tr!("Not removed: {e}", e = e.to_string()).into(), true),
        });
        self.refresh(cx);
    }

    /// Whether an entry is shown for what is typed: a name that contains
    /// it, or a hashed name that is it.
    fn shown(e: &Entry, typed: &str) -> bool {
        if typed.is_empty() {
            return true;
        }
        let lower = typed.to_lowercase();
        if e.names().iter().any(|n| n.to_lowercase().contains(&lower)) {
            return true;
        }
        let (host, port) = host_port(typed);
        e.hashed() && e.is_for(&host, port)
    }
}

impl Render for KnownHostsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let typed = self.filter.read(cx).text().trim().to_string();
        let shown: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| Self::shown(e, &typed))
            .cloned()
            .collect();
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().line));
        if shown.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(if self.entries.is_empty() {
                        tr!("No hosts yet: each one you trust is added here.")
                    } else {
                        tr!("None matches.")
                    }),
            );
        }
        for (i, e) in shown.into_iter().enumerate() {
            let names = if e.hashed() {
                if typed.is_empty() {
                    tr!("a hashed name")
                } else {
                    tr!("{typed} (hashed)", typed = &typed)
                }
            } else {
                let mut n = e.names().join(", ");
                let hashed = e.hosts.len() - e.names().len();
                if hashed > 0 {
                    n = tr!("{names} and {hashed} hashed", names = &n, hashed = hashed);
                }
                n
            };
            let line = e.line;
            let mut title = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(names),
                )
                .child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().chip))
                        .text_size(ui(10.5))
                        .text_color(rgb(colors().soft))
                        .child(e.key.kind.clone()),
                );
            if let Some(m) = &e.marker {
                let revoked = m == "@revoked";
                title = title.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .text_size(ui(10.5))
                        .text_color(rgb(if revoked { colors().bad } else { colors().soft }))
                        .bg(rgb(if revoked {
                            colors().bad_bg
                        } else {
                            colors().chip
                        }))
                        .child(if revoked {
                            tr!("revoked")
                        } else {
                            tr!("certificate authority")
                        }),
                );
            }
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(9.))
                    .when(i > 0, |d| d.border_t_1().border_color(rgb(colors().line)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(title)
                            .child(
                                div()
                                    .text_size(ui(11.5))
                                    .text_color(rgb(colors().muted))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(div().font_family(self.mono.clone()).child(tr!(
                                        "{fingerprint} · line {line}",
                                        fingerprint = &e.key.fingerprint,
                                        line = line
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .id(("known-remove", line))
                            .debug_selector(|| format!("known-remove:{line}"))
                            .h(px(26.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .bg(rgb(colors().button))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().bad))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().button_hover)))
                            .on_click(cx.listener(move |v, _, _, cx| v.remove(line, cx)))
                            .child(tr!("Remove")),
                    ),
            );
        }
        let path = self.path.clone();
        let mut page = div()
            .id("known-hosts")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap(px(16.))
            .max_w(px(760.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(ui(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr!("Known hosts")),
                    )
                    .when_some(path.clone(), |d, p| {
                        d.child(outline_button("known-open", &tr!("Open known_hosts")).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.open_url(&format!("file://{}", p.display()))
                            }),
                        ))
                    }),
            )
            .child(div().mt(px(-6.)).text_size(ui(12.5)).text_color(rgb(colors().label)).child(tr!(
                "The servers you trust, and their keys, from {path}: the same file ssh reads. A server showing another key is refused, and its pane says what changed. Removing one makes the next connection ask about it again.",
                path = path.as_deref().map_or("known_hosts".to_string(), |p| p.display().to_string())
            )));
        if let Some((msg, bad)) = &self.notice {
            page = page.child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(if *bad { colors().bad } else { colors().good }))
                    .child(msg.clone()),
            );
        }
        page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(label_text(&tr_n!(
                    self.entries.len(),
                    "{n} key",
                    "{n} keys"
                )))
                .child(
                    div()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().field))
                        .text_size(ui(13.))
                        .child(self.filter.clone()),
                ),
        )
        .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::host_port;

    #[test]
    fn typed_names_read_as_known_hosts_writes_them() {
        assert_eq!(host_port("api.example.com"), ("api.example.com".into(), 22));
        assert_eq!(host_port("db:2222"), ("db".into(), 2222));
        assert_eq!(host_port("[db]:2222"), ("db".into(), 2222));
        assert_eq!(host_port("::1"), ("::1".into(), 22));
    }
}

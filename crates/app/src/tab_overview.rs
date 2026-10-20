//! All the open tabs, by host (the header's tab button, Ctrl+Shift+A): type
//! to find one, the arrows and Enter go to it, Ctrl+W closes it. For when
//! there are more tabs than the strip shows.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    Render, ScrollHandle, SharedString, Subscription, Window, div, prelude::*, px, rgb, svg,
};

use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::input::{Changed, TextInput};
use crate::look::ui;
use crate::session::Status;
use crate::sidebar::dot;

/// One open tab, as the workspace sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct TabRow {
    /// Its place in the strip.
    pub tab: usize,
    /// Tabs with the same key are one host's.
    pub host_key: String,
    /// The host's name, or "Local terminal".
    pub host: SharedString,
    pub local: bool,
    /// The host's color.
    pub mark: Option<u32>,
    /// `user@address`, or "this computer".
    pub address: SharedString,
    /// What the program says it is (OSC 0/2), else the address.
    pub title: SharedString,
    pub status: Status,
    /// Panes it is split into.
    pub panes: usize,
    /// The active tab.
    pub here: bool,
}

impl TabRow {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || format!("{} {} {}", self.host, self.address, self.title)
                .to_lowercase()
                .contains(query)
    }
}

/// What the overview asks the workspace to do.
pub enum OverviewEvent {
    Dismiss,
    /// Go to this tab.
    Go(usize),
    /// Close this tab.
    Close(usize),
}

pub struct TabOverview {
    input: Entity<TextInput>,
    rows: Vec<TabRow>,
    /// The rows shown, as indices into `rows`: filtered, each host's together.
    shown: Vec<usize>,
    /// The row the arrows are on, in `shown`.
    chosen: usize,
    scroll: ScrollHandle,
    _changed: Subscription,
}

impl EventEmitter<OverviewEvent> for TabOverview {}

impl TabOverview {
    pub fn new(rows: Vec<TabRow>, cx: &mut Context<Self>) -> Self {
        let input = crate::input::said(cx, || tr!("Find a tab: host, user, title"));
        let changed = cx.subscribe(&input, |o, _, _: &Changed, cx| {
            o.chosen = 0;
            o.refilter(cx);
        });
        let mut o = TabOverview {
            input,
            rows: vec![],
            shown: vec![],
            chosen: 0,
            scroll: ScrollHandle::new(),
            _changed: changed,
        };
        o.set_rows(rows, cx);
        // It opens on the tab you are in.
        o.chosen = o.shown.iter().position(|&i| o.rows[i].here).unwrap_or(0);
        o
    }

    /// The tabs as they are now (one was closed, a status changed).
    pub fn set_rows(&mut self, rows: Vec<TabRow>, cx: &mut Context<Self>) {
        if rows == self.rows {
            return;
        }
        let was = self.chosen_tab();
        self.rows = rows;
        self.refilter(cx);
        // Stays on the same tab, else on the row where it was.
        if let Some(at) = was.and_then(|t| self.shown.iter().position(|&i| self.rows[i].tab == t)) {
            self.chosen = at;
        }
        self.chosen = self.chosen.min(self.shown.len().saturating_sub(1));
    }

    fn chosen_tab(&self) -> Option<usize> {
        self.shown.get(self.chosen).map(|&i| self.rows[i].tab)
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().trim().to_lowercase();
        self.shown = grouped(&self.rows)
            .into_iter()
            .filter(|&i| self.rows[i].matches(&query))
            .collect();
        self.chosen = self.chosen.min(self.shown.len().saturating_sub(1));
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let n = self.shown.len();
        let step = |by: isize, o: &mut Self, cx: &mut Context<Self>| {
            if n > 0 {
                o.chosen = (o.chosen as isize + by).rem_euclid(n as isize) as usize;
                o.scroll.scroll_to_item(o.chosen);
                cx.notify();
            }
        };
        let close = if cfg!(target_os = "macos") {
            k.modifiers.platform
        } else {
            k.modifiers.control
        };
        match k.key.as_str() {
            "escape" => cx.emit(OverviewEvent::Dismiss),
            "enter" => {
                if let Some(t) = self.chosen_tab() {
                    cx.emit(OverviewEvent::Go(t));
                }
            }
            "w" if close && !k.modifiers.shift => {
                if let Some(t) = self.chosen_tab() {
                    cx.emit(OverviewEvent::Close(t));
                }
            }
            "down" => step(1, self, cx),
            "up" => step(-1, self, cx),
            "n" if k.modifiers.control => step(1, self, cx),
            "p" if k.modifiers.control => step(-1, self, cx),
            "pagedown" => step(8, self, cx),
            "pageup" => step(-8, self, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// The rows' indices, each host's tabs together, hosts in the order of their
/// first tab in the strip.
fn grouped(rows: &[TabRow]) -> Vec<usize> {
    let mut hosts: Vec<&str> = vec![];
    for r in rows {
        if !hosts.contains(&r.host_key.as_str()) {
            hosts.push(&r.host_key);
        }
    }
    hosts
        .iter()
        .flat_map(|h| (0..rows.len()).filter(move |&i| rows[i].host_key == *h))
        .collect()
}

impl Focusable for TabOverview {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for TabOverview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let mut list = div()
            .id("tab-overview-list")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .max_h(px(470.))
            .flex()
            .flex_col()
            .px(px(6.))
            .pt(px(4.))
            .pb(px(6.));
        let mut last: Option<&str> = None;
        for (n, &i) in self.shown.iter().enumerate() {
            let r = &self.rows[i];
            if last != Some(r.host_key.as_str()) {
                last = Some(&r.host_key);
                let count = self
                    .rows
                    .iter()
                    .filter(|o| o.host_key == r.host_key)
                    .count();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .pt(px(9.))
                        .pb(px(4.))
                        .child(if r.local {
                            svg()
                                .path("icons/terminal.svg")
                                .size(px(13.))
                                .flex_shrink_0()
                                .text_color(rgb(c.soft))
                                .into_any_element()
                        } else {
                            div()
                                .w(px(3.))
                                .h(px(13.))
                                .flex_shrink_0()
                                .rounded(px(2.))
                                .when_some(r.mark, |d, m| d.bg(rgb(m)))
                                .into_any_element()
                        })
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(c.text))
                                .child(r.host.clone()),
                        )
                        .child(
                            div()
                                .text_size(ui(11.5))
                                .text_color(rgb(c.muted))
                                .child(tr_n!(count, "{n} tab", "{n} tabs")),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(ui(11.5))
                                .text_color(rgb(c.muted))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(r.address.clone()),
                        ),
                );
            }
            let chosen = n == self.chosen;
            let tab = r.tab;
            list = list.child(
                div()
                    .id(("tab-row", tab))
                    .group("tab-row")
                    .h(px(32.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(20.))
                    .pr(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(chosen, |d| d.bg(rgb(c.menu_hover)))
                    .when(!chosen && r.here, |d| d.bg(rgb(c.selected)))
                    .when(!chosen, |d| d.hover(|s| s.bg(rgb(c.hover))))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(OverviewEvent::Go(tab))))
                    .child(dot(Some(r.status)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(rgb(c.text))
                            .child(r.title.clone()),
                    )
                    .when(r.panes > 1, |d| {
                        d.child(
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .bg(rgb(c.chip))
                                .text_size(ui(11.))
                                .text_color(rgb(c.soft))
                                .child(tr_n!(r.panes, "{n} pane", "{n} panes")),
                        )
                    })
                    .when(r.here, |d| {
                        d.child(
                            div()
                                .text_size(ui(11.))
                                .text_color(rgb(c.accent))
                                .child(tr!("this tab")),
                        )
                    })
                    .child(
                        div()
                            .min_w(px(40.))
                            .flex()
                            .justify_end()
                            .text_size(ui(11.))
                            .text_color(rgb(c.muted))
                            .child(tr!("Tab {number}", number = tab + 1)),
                    )
                    // Closes it; shown on the row the arrows are on.
                    .child(
                        div()
                            .id(("tab-row-close", tab))
                            .w(px(20.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_color(rgb(if chosen { c.soft } else { c.raised }))
                            .group_hover("tab-row", |s| s.text_color(rgb(c.soft)))
                            .hover(|s| s.bg(rgb(c.button_hover)).text_color(rgb(c.text)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                cx.emit(OverviewEvent::Close(tab));
                            }))
                            .child("×"),
                    ),
            );
        }
        if self.shown.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(18.))
                    .text_color(rgb(c.muted))
                    .child(tr!("No tab matches.")),
            );
        }
        let mac = cfg!(target_os = "macos");
        div()
            .id("tab-overview")
            .w(px(420.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(c.edge))
            .bg(rgb(c.raised))
            .shadow_lg()
            .overflow_hidden()
            .text_size(ui(13.))
            .text_color(rgb(c.text))
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(10.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .border_b_1()
                    .border_color(rgb(c.line))
                    .child(
                        div()
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(rgb(c.accent))
                            .bg(rgb(c.field))
                            .child(
                                svg()
                                    .path("icons/search.svg")
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .text_color(rgb(c.muted)),
                            )
                            .child(div().flex_1().min_w_0().child(self.input.clone()))
                            .child(
                                div()
                                    .text_size(ui(11.))
                                    .text_color(rgb(c.muted))
                                    .whitespace_nowrap()
                                    .child(crate::hotkeys::hint(
                                        cx,
                                        shellwing_config::Hotkey::Tabs,
                                    )),
                            ),
                    ),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .px(px(14.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(rgb(c.line))
                    .bg(rgb(c.panel))
                    .text_size(ui(11.5))
                    .text_color(rgb(c.muted))
                    .child(tr!("↑↓ choose"))
                    .child(tr!("Enter go to it"))
                    .child(if mac {
                        tr!("Cmd+W close it")
                    } else {
                        tr!("Ctrl+W close it")
                    })
                    .child(div().flex_1())
                    .child("Esc"),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tab: usize, host: &str, title: &str) -> TabRow {
        TabRow {
            tab,
            host_key: host.into(),
            host: host.to_string().into(),
            local: false,
            mark: None,
            address: format!("deploy@{host}").into(),
            title: title.to_string().into(),
            status: Status::Connected,
            panes: 1,
            here: false,
        }
    }

    #[test]
    fn a_hosts_tabs_go_together_in_strip_order_and_typing_finds() {
        let rows = vec![
            row(0, "api-1", "~/apps"),
            row(1, "db", "psql"),
            row(2, "api-1", "htop"),
            row(3, "web", "~"),
            row(4, "db", "~"),
        ];
        assert_eq!(grouped(&rows), [0, 2, 1, 4, 3]);
        assert!(rows[2].matches("htop") && rows[2].matches("deploy@api"));
        assert!(!rows[3].matches("psql"));
    }
}

//! The host list on the left. Its own view, drawn from a snapshot the workspace
//! hands it, so the terminal's frames do not lay it out again (it is cached).

use gpui::{
    App, Context, Div, ElementId, Entity, FontWeight, KeyDownEvent, Render, SharedString,
    WeakEntity, Window, div, prelude::*, px, rgb, rgba, svg,
};

use crate::input::{Changed, TextInput};

use crate::session::Status;
use crate::workspace::{EditTarget, Workspace};

/// The host list's width by default, and how far its edge can be dragged.
pub const WIDTH: f32 = 248.;
pub const MIN_WIDTH: f32 = 200.;
pub const MAX_WIDTH: f32 = 480.;
/// Folded to a rail of host tiles.
pub const RAIL_WIDTH: f32 = 56.;
pub const TEXT: u32 = 0xe2e8f0;
pub const MUTED: u32 = 0x8a96aa;
pub const LINE: u32 = 0x1e2736;
const PANEL: u32 = 0x0d121c;
const HOVER: u32 = 0x17233a;
/// The Edit badge on a hovered host.
const EDIT: u32 = 0xf59e0b;
const EDIT_HOVER: u32 = 0xfbbf24;

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: SharedString,
    /// Beside the title: a count, or "read only".
    pub note: SharedString,
    /// The `[[group]]` it shows, which its Edit opens.
    pub group: Option<String>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: SharedString,
    pub detail: SharedString,
    /// What opening it connects to.
    pub spec: String,
    /// The best status among its open tabs; None when none is open.
    pub status: Option<Status>,
    /// Where Edit takes it.
    pub edit: EditTarget,
    /// The host's color (its tile on the rail).
    pub color: Option<u32>,
    /// The active tab is connected to it.
    pub active: bool,
    /// What the filter matches, lowercase: name, alias, address, user, tags.
    pub search: String,
}

fn status_color(s: Status) -> u32 {
    match s {
        Status::Connecting => 0xf59e0b,
        Status::Connected => 0x22c55e,
        Status::Closed => 0x6b7588,
        Status::Lost => 0xf97316,
        Status::Failed => 0xef4444,
    }
}

/// A status dot, or an empty ring for a host with no tab.
pub fn dot(status: Option<Status>) -> Div {
    let d = div().size(px(7.)).flex_shrink_0().rounded_full();
    match status {
        Some(s) => d.bg(rgb(status_color(s))),
        None => d.border_1().border_color(rgb(0x5b6578)),
    }
}

pub struct Sidebar {
    workspace: WeakEntity<Workspace>,
    sections: Vec<Section>,
    /// The row under the pointer, which shows Edit.
    hovered: Option<u64>,
    /// The group header under the pointer, likewise.
    hovered_group: Option<usize>,
    filter: Entity<TextInput>,
    /// Shown as a rail of host tiles.
    collapsed: bool,
}

/// A host's tile on the rail: two letters from its name. `api-1` is A1,
/// `db-primary` DP, `web-01` W1, `worker` WO.
pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    let first = |p: &str| p.chars().next().map(|c| c.to_ascii_uppercase());
    let mut out = String::new();
    match parts.as_slice() {
        [] => return "?".into(),
        [only] => {
            let chars: Vec<char> = only.chars().collect();
            out.extend(first(only));
            // A name ending in a number keeps its last digit: web01 is W1.
            match chars.iter().rev().take_while(|c| c.is_ascii_digit()).last() {
                Some(_) if chars.len() > 1 => out.extend(chars.last()),
                _ => out.extend(chars.get(1).map(|c| c.to_ascii_uppercase())),
            }
        }
        [head, .., last] => {
            out.extend(first(head));
            if last.chars().all(|c| c.is_ascii_digit()) {
                out.extend(last.chars().last());
            } else {
                out.extend(first(parts[1]));
            }
        }
    }
    out
}

/// A tooltip: a title, and a line under it when there is one. Shared by the
/// host rail and the Files panel's buttons.
pub(crate) struct Tip {
    pub title: SharedString,
    pub detail: SharedString,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(11.))
            .py(px(7.))
            .rounded(px(8.))
            .bg(rgb(0x111a2a))
            .border_1()
            .border_color(rgb(0x2b3648))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .when(!self.detail.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(self.detail.clone()),
                )
            })
    }
}

impl Sidebar {
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextInput::new("Filter by name, tag or address", cx));
        cx.subscribe(&filter, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        Sidebar {
            workspace,
            sections: vec![],
            hovered: None,
            hovered_group: None,
            filter,
            collapsed: false,
        }
    }

    /// The sections with only the rows the filter matches (all words, any order).
    fn shown(&self, cx: &App) -> Vec<Section> {
        let typed = self.filter.read(cx).text().to_lowercase();
        let words: Vec<&str> = typed.split_whitespace().collect();
        if words.is_empty() {
            return self.sections.clone();
        }
        self.sections
            .iter()
            .filter_map(|s| {
                let rows: Vec<Row> = s
                    .rows
                    .iter()
                    .filter(|r| words.iter().all(|w| r.search.contains(w)))
                    .cloned()
                    .collect();
                (!rows.is_empty()).then(|| Section {
                    note: rows.len().to_string().into(),
                    rows,
                    ..s.clone()
                })
            })
            .collect()
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            // Esc empties the filter; Enter opens the first host it shows.
            "escape" => {
                self.filter.update(cx, |f, cx| f.set_text("", 0..0, cx));
            }
            "enter" => {
                let first = self.shown(cx).into_iter().flat_map(|s| s.rows).next();
                if let Some(row) = first {
                    self.filter.update(cx, |f, cx| f.set_text("", 0..0, cx));
                    let _ = self
                        .workspace
                        .update(cx, |ws, cx| ws.open(&row.spec, window, cx));
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub fn set_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.collapsed != collapsed {
            self.collapsed = collapsed;
            cx.notify();
        }
    }

    /// The rail: the host list folded to tiles, with the way back.
    fn rail(&self) -> Div {
        let icon_button = |id: &'static str, icon: &'static str, on: bool| {
            div()
                .id(id)
                .w(px(32.))
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .when(on, |d| d.bg(rgb(HOVER)))
                .hover(|s| s.bg(rgb(0x1e2a40)))
                .child(svg().path(icon).size(px(16.)).text_color(rgb(TEXT)))
        };
        let (expand, find, add) = (
            self.workspace.clone(),
            self.workspace.clone(),
            self.workspace.clone(),
        );
        let mut tiles = div()
            .id("rail")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .pt_1()
            .pb_2();
        let mut id = 0u64;
        for (n, section) in self
            .sections
            .iter()
            .filter(|s| !s.rows.is_empty())
            .enumerate()
        {
            if n > 0 {
                tiles = tiles.child(div().w(px(24.)).h(px(1.)).my(px(2.)).bg(rgb(0x263042)));
            }
            for row in &section.rows {
                id += 1;
                let (spec, ws) = (row.spec.clone(), self.workspace.clone());
                let title = row.name.clone();
                let state = match row.status {
                    Some(Status::Connected) => "connected",
                    Some(Status::Connecting) => "connecting",
                    Some(Status::Lost) => "connection lost",
                    Some(Status::Failed) => "could not connect",
                    Some(Status::Closed) => "closed",
                    None => "",
                };
                let detail: SharedString = [row.detail.to_string(), state.to_string()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into();
                let (bg, fg) = match row.color {
                    Some(c) => (rgba((c << 8) | 0x2e), rgb(c)),
                    None => (rgba(0x1a2333ff), rgb(0xcbd5e1)),
                };
                tiles = tiles.child(
                    div()
                        .id(ElementId::NamedInteger("tile".into(), id))
                        .relative()
                        .flex_shrink_0()
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.))
                        .border_2()
                        .border_color(if row.active {
                            rgba(0x38bdf8ff)
                        } else {
                            rgba(0x00000000)
                        })
                        .bg(bg)
                        .text_color(fg)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.85))
                        .tooltip(move |_, cx| {
                            let (title, detail) = (title.clone(), detail.clone());
                            cx.new(|_| Tip { title, detail }).into()
                        })
                        .on_click(move |_, window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.open(&spec, window, cx));
                        })
                        .child(initials(&row.name))
                        .child(
                            div().absolute().right(px(-3.)).bottom(px(-3.)).child(
                                div()
                                    .p(px(2.))
                                    .rounded_full()
                                    .bg(rgb(PANEL))
                                    .child(dot(row.status)),
                            ),
                        ),
                );
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .child(
                        icon_button("expand-hosts", "icons/panel-open.svg", true).on_click(
                            move |_, window, cx| {
                                let _ = expand.update(cx, |ws, cx| {
                                    ws.set_sidebar_collapsed(false, window, cx)
                                });
                            },
                        ),
                    )
                    .child(
                        icon_button("find-host", "icons/search.svg", false).on_click(
                            move |_, window, cx| {
                                let _ =
                                    find.update(cx, |ws, cx| ws.open_palette_from_rail(window, cx));
                            },
                        ),
                    ),
            )
            .child(tiles)
            .child(
                div()
                    .w_full()
                    .py_2()
                    .flex()
                    .justify_center()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .id("rail-new-host")
                            .w(px(36.))
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x263042))
                            .text_color(rgb(TEXT))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(HOVER)))
                            .on_click(move |_, window, cx| {
                                let _ = add
                                    .update(cx, |ws, cx| ws.edit_host(EditTarget::New, window, cx));
                            })
                            .child("+"),
                    ),
            )
    }

    pub fn set(&mut self, sections: Vec<Section>, cx: &mut Context<Self>) {
        if sections != self.sections {
            self.sections = sections;
            cx.notify();
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.collapsed {
            return self.rail();
        }
        let mut list = div()
            .id("hosts")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb_2();
        let mut id = 0u64;
        let shown = self.shown(cx);
        let filtering = !self.filter.read(cx).text().trim().is_empty();
        for (n, section) in shown.iter().enumerate() {
            let over = self.hovered_group == Some(n) && section.group.is_some();
            let (group, ws) = (section.group.clone(), self.workspace.clone());
            list = list.child(
                div()
                    .id(("section", n))
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(26.))
                    .px_3()
                    .mt_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .on_hover(cx.listener(move |s, over: &bool, _, cx| {
                        let now = if *over {
                            Some(n)
                        } else if s.hovered_group == Some(n) {
                            None
                        } else {
                            s.hovered_group
                        };
                        if now != s.hovered_group {
                            s.hovered_group = now;
                            cx.notify();
                        }
                    }))
                    .child(section.title.clone())
                    .when(!over, |d| {
                        d.child(
                            div()
                                .font_weight(FontWeight::NORMAL)
                                .child(section.note.clone()),
                        )
                    })
                    .when(over, |d| {
                        d.child(
                            div()
                                .id(("edit-group", n))
                                .px_2()
                                .py(px(1.))
                                .rounded_md()
                                .bg(rgb(EDIT))
                                .text_color(rgb(0x1f1300))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(EDIT_HOVER)))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let name = group.clone();
                                    let _ = ws.update(cx, |ws, cx| ws.edit_group(name, window, cx));
                                })
                                .child("Edit"),
                        )
                    }),
            );
            if section.rows.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .pb_1()
                        .text_xs()
                        .text_color(rgb(0x5b6578))
                        .child("No hosts yet"),
                );
            }
            for row in &section.rows {
                id += 1;
                let (spec, workspace) = (row.spec.clone(), self.workspace.clone());
                let this = id;
                let hovered = self.hovered == Some(this);
                let (edit, editing) = (row.edit.clone(), self.workspace.clone());
                list = list.child(
                    div()
                        .id(ElementId::NamedInteger("host".into(), id))
                        .on_hover(cx.listener(move |s, over: &bool, _, cx| {
                            let now = if *over {
                                Some(this)
                            } else if s.hovered == Some(this) {
                                None
                            } else {
                                s.hovered
                            };
                            if now != s.hovered {
                                s.hovered = now;
                                cx.notify();
                            }
                        }))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(30.))
                        .px_2()
                        .mx_1()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(HOVER)))
                        .on_click(move |_, window, cx| {
                            let _ = workspace.update(cx, |ws, cx| ws.open(&spec, window, cx));
                        })
                        .child(dot(row.status))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(rgb(TEXT))
                                .child(row.name.clone()),
                        )
                        .when(!hovered, |d| {
                            d.child(
                                div()
                                    .max_w(px(110.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(row.detail.clone()),
                            )
                        })
                        // Hovered: Edit instead of the address.
                        .when(hovered, |d| {
                            d.child(
                                div()
                                    .id(ElementId::NamedInteger("edit-host".into(), this))
                                    // An amber badge: plain to see on any row.
                                    .px_2()
                                    .py(px(1.))
                                    .rounded_md()
                                    .bg(rgb(EDIT))
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x1f1300))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(EDIT_HOVER)))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        let edit = edit.clone();
                                        let _ = editing
                                            .update(cx, |ws, cx| ws.edit_host(edit, window, cx));
                                    })
                                    .child("Edit"),
                            )
                        }),
                );
            }
        }
        if filtering && shown.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .pt_3()
                    .text_color(rgb(MUTED))
                    .child("No host matches."),
            );
        } else if self.sections.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .pt_3()
                    .text_color(rgb(MUTED))
                    .child("No hosts yet. Add one, or Host blocks to ~/.ssh/config."),
            );
        }
        let (workspace, for_group) = (self.workspace.clone(), self.workspace.clone());
        let footer = div()
            .p_2()
            .flex()
            .gap_2()
            .border_t_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .id("new-host")
                    .flex_1()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x263042))
                    .text_color(rgb(TEXT))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_click(move |_, window, cx| {
                        let _ = workspace
                            .update(cx, |ws, cx| ws.edit_host(EditTarget::New, window, cx));
                    })
                    .child("+ New host"),
            )
            .child(
                div()
                    .id("new-group")
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x263042))
                    .text_color(rgb(TEXT))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(HOVER)))
                    .on_click(move |_, window, cx| {
                        let _ = for_group.update(cx, |ws, cx| ws.edit_group(None, window, cx));
                    })
                    .child("+ Group"),
            );
        let collapse = self.workspace.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .text_sm()
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(24.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(MUTED))
                                    .child("HOSTS"),
                            )
                            .child(
                                div()
                                    .id("collapse-hosts")
                                    .w(px(26.))
                                    .h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(HOVER)))
                                    .on_click(move |_, window, cx| {
                                        let _ = collapse.update(cx, |ws, cx| {
                                            ws.set_sidebar_collapsed(true, window, cx)
                                        });
                                    })
                                    .child(
                                        svg()
                                            .path("icons/panel-close.svg")
                                            .size(px(16.))
                                            .text_color(rgb(MUTED)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(30.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(0x263042))
                            .bg(rgb(0x0b0f17))
                            .text_xs()
                            .text_color(rgb(TEXT))
                            .child(self.filter.clone()),
                    ),
            )
            .child(list)
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn tiles_get_two_letters() {
        for (name, want) in [
            ("api-1", "A1"),
            ("db-primary", "DP"),
            ("web-01", "W1"),
            ("worker", "WO"),
            ("stage-web", "SW"),
            ("nas.home", "NH"),
            ("web01", "W1"),
            ("x", "X"),
            ("--", "?"),
        ] {
            assert_eq!(initials(name), want, "{name}");
        }
    }
}

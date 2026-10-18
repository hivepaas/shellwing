//! The host list on the left. Its own view, drawn from a snapshot the workspace
//! hands it, so the terminal's frames do not lay it out again (it is cached).

use gpui::{
    App, Context, Div, ElementId, Entity, FontWeight, KeyDownEvent, Render, SharedString,
    WeakEntity, Window, div, prelude::*, px, rgb,
};

use crate::input::{Changed, TextInput};

use crate::session::Status;
use crate::workspace::{EditTarget, Workspace};

pub const WIDTH: f32 = 248.;
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
    filter: Entity<TextInput>,
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
            filter,
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
                (!rows.is_empty()).then(|| Section { rows, ..s.clone() })
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

    pub fn set(&mut self, sections: Vec<Section>, cx: &mut Context<Self>) {
        if sections != self.sections {
            self.sections = sections;
            cx.notify();
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        for section in &shown {
            list = list.child(
                div()
                    .flex()
                    .justify_between()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child(section.title.clone())
                    .child(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .child(section.note.clone()),
                    ),
            );
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
        let workspace = self.workspace.clone();
        let footer = div().p_2().border_t_1().border_color(rgb(LINE)).child(
            div()
                .id("new-host")
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
                    let _ =
                        workspace.update(cx, |ws, cx| ws.edit_host(EditTarget::New, window, cx));
                })
                .child("+ New host"),
        );
        div()
            .w(px(WIDTH))
            .flex_shrink_0()
            .h_full()
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
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child("HOSTS"),
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

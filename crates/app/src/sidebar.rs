//! The host list on the left. Its own view, drawn from a snapshot the workspace
//! hands it, so the terminal's frames do not lay it out again (it is cached).

use gpui::{
    Context, Div, ElementId, FontWeight, Render, SharedString, WeakEntity, Window, div, prelude::*,
    px, rgb,
};

use crate::session::Status;
use crate::workspace::Workspace;

pub const WIDTH: f32 = 248.;
pub const TEXT: u32 = 0xe2e8f0;
pub const MUTED: u32 = 0x8a96aa;
pub const LINE: u32 = 0x1e2736;
const PANEL: u32 = 0x0d121c;
const HOVER: u32 = 0x17233a;

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: SharedString,
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
}

impl Sidebar {
    pub fn new(workspace: WeakEntity<Workspace>) -> Self {
        Sidebar {
            workspace,
            sections: vec![],
        }
    }

    pub fn set(&mut self, sections: Vec<Section>, cx: &mut Context<Self>) {
        if sections != self.sections {
            self.sections = sections;
            cx.notify();
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .id("hosts")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb_2();
        let mut id = 0u64;
        for section in &self.sections {
            list = list.child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child(section.title.clone()),
            );
            for row in &section.rows {
                id += 1;
                let (spec, workspace) = (row.spec.clone(), self.workspace.clone());
                list = list.child(
                    div()
                        .id(ElementId::NamedInteger("host".into(), id))
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
                        .child(
                            div()
                                .max_w(px(110.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .child(row.detail.clone()),
                        ),
                );
            }
        }
        if self.sections.is_empty() {
            list =
                list.child(div().px_3().pt_3().text_color(rgb(MUTED)).child(
                    "No hosts yet. Add them to hosts.toml, or Host blocks to ~/.ssh/config.",
                ));
        }
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
            .child(list)
    }
}

//! The AI activity tab: every command an AI client ran, the one running with
//! its output as it comes, and those that wait for the person to allow
//! them (docs/configuration.md, AI access).

use gpui::{
    App, ClipboardItem, Context, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px, rgb,
};
use std::sync::Arc;

use shellwing_core::ai::{Activity, Approval, End};
use shellwing_i18n::tr;

use crate::colors::colors;
use crate::host_editor::{hint_text, outline_button, primary_button};
use crate::look::ui;
use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Waiting,
    Running,
    Failed,
}

pub struct ActivityView {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    filter: Filter,
    /// The command shown, by its job's id; None: the newest.
    shown: Option<String>,
    mono: SharedString,
}

impl Focusable for ActivityView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// How a command stands: its words, and its color.
fn standing(a: &Activity) -> (String, u32) {
    let c = colors();
    match (&a.waiting, &a.end) {
        (Some(_), _) => (tr!("waiting for you"), c.warn),
        (None, None) => (tr!("running"), c.accent),
        (None, Some(e)) if e.ok() => (tr!("exit 0"), c.ok),
        (None, Some(End::Exited(Some(code)))) => {
            (tr!("exit {code}", code = code.to_string()), c.bad)
        }
        (None, Some(End::Exited(None))) => (tr!("ended by a signal"), c.bad),
        (None, Some(End::Failed(_))) => (tr!("did not run"), c.bad),
        (None, Some(End::Stopped)) => (tr!("stopped"), c.muted),
        (None, Some(End::Refused)) => (tr!("refused by you"), c.muted),
    }
}

fn when(secs: u64) -> String {
    chrono::DateTime::from_timestamp(secs as i64, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

impl ActivityView {
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> ActivityView {
        ActivityView {
            workspace,
            focus: cx.focus_handle(),
            filter: Filter::All,
            shown: None,
            mono: SharedString::default(),
        }
    }

    fn activity(&self, cx: &App) -> Vec<Arc<Activity>> {
        self.workspace
            .upgrade()
            .map(|w| w.read(cx).ai_activity())
            .unwrap_or_default()
    }

    fn answer(&self, id: &str, approval: Approval, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.read(cx).ai_answer(id, approval);
        }
    }

    fn stop(&self, id: Option<&str>, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.read(cx).ai_stop(id);
        }
    }

    fn list(
        &self,
        all: &[Arc<Activity>],
        shown: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let keep = |a: &Arc<Activity>| match self.filter {
            Filter::All => true,
            Filter::Waiting => a.waiting.is_some(),
            Filter::Running => a.end.is_none() && a.waiting.is_none(),
            Filter::Failed => a
                .end
                .as_ref()
                .is_some_and(|e| !e.ok() && *e != End::Stopped),
        };
        let mut filters = div().flex().items_center().gap(px(6.));
        for (f, label, sel) in [
            (Filter::All, tr!("All"), "ai-filter:all"),
            (Filter::Waiting, tr!("Waiting for you"), "ai-filter:waiting"),
            (Filter::Running, tr!("Running"), "ai-filter:running"),
            (Filter::Failed, tr!("Failed"), "ai-filter:failed"),
        ] {
            let on = self.filter == f;
            filters = filters.child(
                outline_button(sel, &label)
                    .h(px(26.))
                    .px(px(10.))
                    .text_size(ui(12.))
                    .when(on, |d| d.bg(rgb(colors().selected)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.filter = f;
                        cx.notify();
                    })),
            );
        }
        let going = all.iter().any(|a| a.end.is_none());
        let mut rows = div()
            .id("ai-activity-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let shown_list: Vec<&Arc<Activity>> = all.iter().rev().filter(|a| keep(a)).collect();
        if shown_list.is_empty() {
            rows = rows.child(
                div()
                    .px(px(14.))
                    .py(px(14.))
                    .child(hint_text(&if all.is_empty() {
                        tr!("No AI client has run a command yet.")
                    } else {
                        tr!("Nothing here.")
                    })),
            );
        }
        for a in shown_list {
            let on = shown == Some(a.id.as_str());
            let (state, color) = standing(a);
            let id = a.id.clone();
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("ai-row:{}", a.id)))
                    .debug_selector({
                        let id = a.id.clone();
                        move || format!("ai-row:{id}")
                    })
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(14.))
                    .py(px(9.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .cursor_pointer()
                    .when(on, |d| d.bg(rgb(colors().selected)))
                    .when(!on && a.waiting.is_some(), |d| d.bg(rgb(colors().warn_bg)))
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.shown = Some(id.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().muted))
                            .child(
                                div()
                                    .px(px(6.))
                                    .rounded_full()
                                    .bg(rgb(crate::sidebar::AI_MARK))
                                    .text_color(rgb(colors().on_accent))
                                    .text_size(ui(11.))
                                    .child(a.client.clone()),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(colors().text))
                                    .child(a.host.clone()),
                            )
                            .child(div().flex_1())
                            .child(when(a.started)),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(self.mono.clone())
                            .text_size(ui(12.5))
                            .child(a.command.lines().next().unwrap_or("").to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(ui(12.))
                            .text_color(rgb(color))
                            .child(div().size(px(8.)).rounded_full().bg(rgb(color)))
                            .child(state),
                    ),
            );
        }
        div()
            .w(px(460.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(14.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(filters)
                    .child(div().flex_1())
                    .when(going, |d| {
                        d.child(
                            outline_button("ai-stop-all", &tr!("Stop all AI"))
                                .h(px(26.))
                                .px(px(10.))
                                .text_size(ui(12.))
                                .text_color(rgb(colors().bad))
                                .on_click(cx.listener(|v, _, _, cx| v.stop(None, cx))),
                        )
                    }),
            )
            .child(rows)
    }

    fn detail(&self, a: &Activity, cx: &mut Context<Self>) -> gpui::Div {
        let (state, color) = standing(a);
        let took = match a.ended {
            Some(e) => format!(", {} s", e.saturating_sub(a.started)),
            None => String::new(),
        };
        let reason = match &a.end {
            Some(End::Failed(why)) => Some(why.clone()),
            _ => None,
        };
        let id = a.id.clone();
        let mut head = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(22.))
            .py(px(14.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(hint_text(&tr!(
                "{client} · {host} · started {when} · {job}",
                client = a.client.clone(),
                host = a.host.clone(),
                when = when(a.started),
                job = a.id.clone()
            )))
            .child(
                div()
                    .debug_selector(|| "ai-command".into())
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .bg(rgb(colors().field))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .font_family(self.mono.clone())
                    .text_size(ui(13.))
                    .child(a.command.clone()),
            )
            .child(
                div()
                    .debug_selector(|| "ai-standing".into())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(ui(12.5))
                    .text_color(rgb(color))
                    .child(div().size(px(8.)).rounded_full().bg(rgb(color)))
                    .child(format!("{state}{took}"))
                    .when_some(reason, |d, r| {
                        d.child(div().text_color(rgb(colors().muted)).child(r))
                    }),
            );
        if let Some(why) = &a.waiting {
            let (once, session, refuse) = (id.clone(), id.clone(), id.clone());
            head = head
                .child(
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .bg(rgb(colors().warn_bg))
                        .border_1()
                        .border_color(rgb(colors().warn_edge))
                        .text_color(rgb(colors().warn_text))
                        .child(if why == shellwing_core::ai::WRITES {
                            tr!(
                                "{client} wants to write this on {host}. It waits for you: where risky ones wait, every write does.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if why.is_empty() {
                            tr!(
                                "{client} wants to run this on {host}. It waits for you: its grant asks for every command.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else {
                            tr!(
                                "{client} wants to run this on {host}. It waits for you: it has {words}, from the risky list.",
                                client = a.client.clone(),
                                host = a.host.clone(),
                                words = why.clone()
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            primary_button("ai-allow", &tr!("Run it"))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.answer(&once, Approval::Once, cx)
                                })),
                        )
                        .child(
                            outline_button("ai-allow-session", &tr!("Run it, and this command again this session"))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.answer(&session, Approval::Session, cx)
                                })),
                        )
                        .child(
                            outline_button("ai-refuse", &tr!("Refuse it"))
                                .text_color(rgb(colors().bad))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.answer(&refuse, Approval::Refuse, cx)
                                })),
                        ),
                );
        } else if a.end.is_none() {
            head = head.child(
                div().flex().child(
                    outline_button("ai-stop", &tr!("Stop"))
                        .text_color(rgb(colors().bad))
                        .on_click(cx.listener(move |v, _, _, cx| v.stop(Some(&id), cx))),
                ),
            );
        }
        let copy = a.output.clone();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(head)
            .child(
                div()
                    .id("ai-output")
                    .debug_selector(|| "ai-output".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(22.))
                    .py(px(14.))
                    .font_family(self.mono.clone())
                    .text_size(ui(12.5))
                    .child(if a.output.is_empty() {
                        div()
                            .font_family(SharedString::default())
                            .child(hint_text(&if a.end.is_some() {
                                tr!("It printed nothing.")
                            } else {
                                tr!("Nothing printed yet.")
                            }))
                    } else {
                        div().child(a.output.clone())
                    }),
            )
            .when(!a.output.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(22.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(rgb(colors().line))
                        .child(
                            outline_button("ai-copy-output", &tr!("Copy output"))
                                .h(px(28.))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                                }),
                        ),
                )
            })
    }
}

impl Render for ActivityView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let all = self.activity(cx);
        // The one asked for, else the first that waits, else the newest.
        let shown = self
            .shown
            .clone()
            .filter(|id| all.iter().any(|a| a.id == *id))
            .or_else(|| {
                all.iter()
                    .find(|a| a.waiting.is_some())
                    .or(all.last())
                    .map(|a| a.id.clone())
            });
        let detail = shown
            .as_ref()
            .and_then(|id| all.iter().find(|a| a.id == *id))
            .map(|a| self.detail(a, cx));
        div()
            .id("ai-activity")
            .debug_selector(|| "ai-activity".into())
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(rgb(colors().page))
            .text_color(rgb(colors().text))
            .text_size(ui(13.))
            .child(self.list(&all, shown.as_deref(), cx))
            .children(detail)
    }
}

//! The AI activity tab: every command an AI client ran, the one running with
//! its output as it comes, and those that wait for the person to allow
//! them (docs/configuration.md, AI access).

use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PromptLevel, Render, SharedString, StatefulInteractiveElement,
    Styled, WeakEntity, Window, div, prelude::FluentBuilder, px, rgb,
};
use std::sync::Arc;

use shellwing_core::ai::{Activity, Approval, End};
use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::host_editor::{hint_text, outline_button, primary_button};
use crate::look::ui;
use crate::term_viewer::TermViewer;
use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Waiting,
    Running,
    Failed,
}

/// The list of commands' width, as it comes; what it may be dragged to.
pub const LIST_WIDTH: f32 = 460.;
pub const MIN_LIST_WIDTH: f32 = 280.;
pub const MAX_LIST_WIDTH: f32 = 900.;

/// The list's width as dragged, kept with the window's layout.
pub struct ListWidth(pub f32);

impl gpui::Global for ListWidth {}

pub struct ActivityView {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    filter: Filter,
    /// The command shown, by its job's id; None: the newest.
    shown: Option<String>,
    mono: SharedString,
    /// The command shown and its output, drawn as a terminal draws them.
    viewer: Option<Entity<TermViewer>>,
    /// The list of commands, and a drag of its scroll bar's thumb.
    scroll: gpui::ScrollHandle,
    scroll_drag: crate::scrollbar::Drag,
    /// The same for what an addition waiting for the person would add.
    adding_scroll: gpui::ScrollHandle,
    adding_drag: crate::scrollbar::Drag,
    /// The list's edge, dragged: where the pointer was, and the width then.
    list_drag: Option<(f32, f32)>,
    /// Let go where nothing heard the release: the width is kept once the
    /// view is drawn again, with a window to keep it by.
    save_after_drag: bool,
    /// sudo's password, typed for the command waiting for it; and whether
    /// to keep it a while for that client on that host.
    sudo_input: Entity<crate::input::TextInput>,
    sudo_remember: bool,
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
        (None, Some(End::Left)) => (tr!("may still run on its host"), c.warn),
        (None, Some(End::Refused)) => (tr!("refused by you"), c.muted),
    }
}

/// The list's width now: as dragged, else as it comes.
fn list_width(cx: &App) -> f32 {
    cx.try_global::<ListWidth>().map_or(LIST_WIDTH, |w| w.0)
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
            viewer: None,
            scroll: gpui::ScrollHandle::new(),
            scroll_drag: Default::default(),
            adding_scroll: gpui::ScrollHandle::new(),
            adding_drag: Default::default(),
            list_drag: None,
            save_after_drag: false,
            sudo_input: gpui::AppContext::new(cx, |cx| {
                crate::input::TextInput::new("", cx).masked()
            }),
            sudo_remember: false,
        }
    }

    /// The person's answer to `id`, which waits for sudo's password: what
    /// they typed, or the host's saved one (`saved`). The field empties.
    fn answer_sudo(&mut self, id: &str, saved: bool, cx: &mut Context<Self>) {
        let typed = self.sudo_input.read(cx).text().to_string();
        if !saved && typed.is_empty() {
            return;
        }
        let password = (!saved).then(|| zeroize::Zeroizing::new(typed));
        if let Some(ws) = self.workspace.upgrade() {
            ws.read(cx).ai_answer_sudo(id, password, self.sudo_remember);
        }
        self.sudo_input.update(cx, |i, cx| i.set_text("", 0..0, cx));
        self.sudo_remember = false;
        cx.notify();
    }

    /// The list's edge let go: its width kept with the window's layout.
    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_drag.take().is_some() {
            self.save_width(window, cx);
        }
        cx.notify();
    }

    fn save_width(&self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .workspace
            .update(cx, |ws, cx| ws.save_widths(window, cx));
    }

    /// Opens the find bar over the output shown.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = &self.viewer {
            v.update(cx, |v, cx| v.open_find(window, cx));
        }
    }

    /// The output shown, for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn shown_text(&self, cx: &App) -> String {
        self.viewer
            .as_ref()
            .map(|v| v.read(cx).shown_text())
            .unwrap_or_default()
    }

    /// The viewer, showing `a` in the look of a tab to its host.
    fn view(&mut self, a: &Activity, cx: &mut Context<Self>) -> Option<Entity<TermViewer>> {
        let look = self.workspace.upgrade()?.read(cx).look_of_host(&a.host);
        if self.viewer.is_none() {
            self.viewer = TermViewer::spawn(look.clone(), cx);
        }
        let v = self.viewer.clone()?;
        v.update(cx, |v, cx| {
            v.set_look(look, cx);
            v.show(&a.id, Some(&a.command), &a.output, "", cx);
        });
        Some(v)
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
            .debug_selector(|| "ai-activity-list".into())
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
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
            .w(px(list_width(cx)))
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
            // The bar over the list, as the host list has it: the rows keep
            // their full width.
            .child(div().flex_1().min_h_0().relative().child(rows).child(
                crate::scrollbar::overlay(self.scroll.clone(), self.scroll_drag.clone()),
            ))
            .child(self.kept_part(all.iter().any(|a| a.end.is_some()), cx))
    }

    /// Under the list: how much the log keeps on this computer, of how
    /// much; its folder, and Clear.
    fn kept_part(&self, any: bool, cx: &mut Context<Self>) -> gpui::Div {
        let ws = self.workspace.upgrade();
        let kept = ws.as_ref().and_then(|w| w.read(cx).ai_kept());
        let path = ws.as_ref().and_then(|w| w.read(cx).ai_log_path());
        let mb = |b: u64| b as f64 / (1024. * 1024.);
        let said = match kept {
            Some((size, most)) => tr!(
                "{size} of {most} MB kept on this computer",
                size = format!("{:.1}", mb(size)),
                most = format!("{:.0}", mb(most))
            ),
            None => String::new(),
        };
        let small = |b: gpui::Stateful<gpui::Div>| b.h(px(26.)).px(px(10.)).text_size(ui(12.));
        div()
            .debug_selector(|| "ai-kept".into())
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .py(px(8.))
            .border_t_1()
            .border_color(rgb(colors().line))
            .child(div().flex_1().min_w_0().child(hint_text(&said)))
            .when_some(path.filter(|p| p.exists()), |d, path| {
                d.child(
                    small(outline_button("ai-log-folder", &tr!("Show in folder")))
                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                )
            })
            .when(any || kept.is_some_and(|(size, _)| size > 0), |d| {
                d.child(
                    small(outline_button("ai-clear", &tr!("Clear…")))
                        .on_click(cx.listener(|v, _, window, cx| v.ask_clear(window, cx))),
                )
            })
    }

    /// Clear, once the person says so: it cannot be undone.
    fn ask_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (yes, no) = (tr!("Clear activity"), tr!("Cancel"));
        let answer = window.prompt(
            PromptLevel::Warning,
            &tr!("Clear AI activity?"),
            Some(&tr!(
                "The commands AI ran and what they printed go, from this computer too. Those running or waiting for you stay."
            )),
            &[yes.as_str(), no.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let yes = matches!(answer.await, Ok(0));
            let _ = this.update_in(cx, |v, window, cx| {
                window.focus(&v.focus, cx);
                if yes && let Some(ws) = v.workspace.upgrade() {
                    ws.read(cx).ai_clear();
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn detail(&mut self, a: &Activity, cx: &mut Context<Self>) -> gpui::Div {
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
            .when_some(a.snippet.clone(), |d, name| {
                d.child(
                    div()
                        .debug_selector(|| "ai-snippet".into())
                        .text_size(ui(12.5))
                        .child(tr!("The snippet {name}", name = name)),
                )
            })
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
            // What it would add to the vault, rather than run: Add it.
            let adds = why == shellwing_core::ai::notes::NOTES
                || why == shellwing_core::ai::snippets::SUGGESTS;
            // A key not in known_hosts, on its way to the host: Trust it.
            let key = why
                .strip_prefix(shellwing_core::ai::HOST_KEY)
                .and_then(|k| k.split_once('\n'))
                .map(|(shows, key)| (shows.to_string(), key.to_string()));
            let trusts = key.is_some();
            // A pane to read: allowed for a while, not run.
            let pane = why == shellwing_core::ai::read_tab::PANE;
            // What sudo's password waits for, and whether the vault keeps one.
            let sudo = why.starts_with(shellwing_core::ai::sudo::SUDO);
            let saved = sudo && why.ends_with(shellwing_core::ai::sudo::SAVED);
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
                        .child(if let Some((shows, _)) = &key {
                            tr!(
                                "{client} wants to reach {host}, and {shows} shows a host key not in known_hosts. Trust it only if it is that host's key, as its owner gave it to you: it is kept in known_hosts, and what waits for it goes on.",
                                client = a.client.clone(),
                                host = a.host.clone(),
                                shows = shows.clone()
                            )
                        } else if pane {
                            tr!(
                                "{client} wants to read your pane on {host}: the text programs printed, and what you typed at the prompt. Allowed, it may read it for {n} minutes; it types nothing.",
                                client = a.client.clone(),
                                host = a.host.clone(),
                                n = shellwing_core::ai::panes::MINUTES.to_string()
                            )
                        } else if sudo {
                            tr!(
                                "{client} wants to run this on {host} with sudo's password. It waits for you every time: type the password, which {client} never sees.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if why == shellwing_core::ai::snippets::SUGGESTS {
                            tr!(
                                "{client} suggests this snippet for {host}. It waits for you: where risky ones wait, every snippet suggested does. Added, it is typed without Enter, for you to read before it runs.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if why == shellwing_core::ai::notes::NOTES {
                            tr!(
                                "{client} wants to add this to the notes about {host}. It waits for you: where risky ones wait, every addition to a note does.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if let Some(envs) = why.strip_prefix(shellwing_core::ai::snippets::ENVS) {
                            tr!(
                                "{client} wants to run this snippet on {host}, with the environments {envs}. It waits for you every time: their values go in, though {client} never sees them, and what it prints goes back.",
                                client = a.client.clone(),
                                host = a.host.clone(),
                                envs = envs.to_string()
                            )
                        } else if why == shellwing_core::ai::snippets::LOOK {
                            tr!(
                                "{client} wants to run this snippet on {host}. It waits for you every time: it is typed without Enter, to be read first.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if why == shellwing_core::ai::transfers::COPIES {
                            tr!(
                                "{client} wants to copy this between this computer and {host}. It waits for you: where risky ones wait, every copy does.",
                                client = a.client.clone(),
                                host = a.host.clone()
                            )
                        } else if why == shellwing_core::ai::WRITES {
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
                .when_some(key.map(|(_, k)| k), |d, k| {
                    d.child(
                        div()
                            .debug_selector(|| "ai-host-key".into())
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(colors().line))
                            .bg(rgb(colors().page))
                            .text_size(ui(12.5))
                            .font_family(self.mono.clone())
                            .child(k),
                    )
                })
                .when_some(a.adding.clone(), |d, text| {
                    // What goes into the note, as it will read there; a long
                    // one scrolls, by the bar every list has.
                    d.child(
                        div()
                            .relative()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(colors().line))
                            .bg(rgb(colors().page))
                            .child(
                                div()
                                    .id("ai-adding")
                                    .debug_selector(|| "ai-adding".into())
                                    .max_h(px(240.))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.adding_scroll)
                                    .px(px(12.))
                                    .py(px(10.))
                                    .text_size(ui(12.5))
                                    .font_family(self.mono.clone())
                                    .child(
                                        div()
                                            .debug_selector(|| "ai-adding-text".into())
                                            .child(text),
                                    ),
                            )
                            .child(crate::scrollbar::overlay(
                                self.adding_scroll.clone(),
                                self.adding_drag.clone(),
                            )),
                    )
                })
                .when(trusts, |d| {
                    let (trust, refuse) = (once.clone(), refuse.clone());
                    d.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(primary_button("ai-trust", &tr!("Trust and go on")).on_click(
                                cx.listener(move |v, _, _, cx| v.answer(&trust, Approval::Once, cx)),
                            ))
                            .child(
                                outline_button("ai-refuse", &tr!("Refuse it"))
                                    .text_color(rgb(colors().bad))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.answer(&refuse, Approval::Refuse, cx)
                                    })),
                            ),
                    )
                })
                .when(adds, |d| {
                    let (add, refuse) = (once.clone(), refuse.clone());
                    d.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(primary_button("ai-allow", &tr!("Add it")).on_click(
                                cx.listener(move |v, _, _, cx| v.answer(&add, Approval::Once, cx)),
                            ))
                            .child(
                                outline_button("ai-refuse", &tr!("Refuse it"))
                                    .text_color(rgb(colors().bad))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.answer(&refuse, Approval::Refuse, cx)
                                    })),
                            ),
                    )
                })
                .when(sudo, |d| {
                    let minutes = self
                        .workspace
                        .upgrade()
                        .map_or(0, |w| w.read(cx).ai_snapshot(cx).state.sudo_minutes());
                    let remember = self.sudo_remember;
                    let (run, use_saved, refuse) = (once.clone(), once.clone(), refuse.clone());
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .debug_selector(|| "ai-sudo-password".into())
                                    .w(px(260.))
                                    .h(px(30.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(colors().field_edge))
                                    .bg(rgb(colors().field))
                                    .child(self.sudo_input.clone()),
                            )
                            .when(minutes > 0, |d| {
                                d.child(
                                    div()
                                        .id("ai-sudo-remember")
                                        .debug_selector(|| "ai-sudo-remember".into())
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|v, _, _, cx| {
                                            v.sudo_remember = !v.sudo_remember;
                                            cx.notify();
                                        }))
                                        .child(crate::host_editor::checkbox(remember))
                                        .child(tr_n!(
                                            minutes as usize,
                                            "Remember it for {n} minute",
                                            "Remember it for {n} minutes"
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(primary_button("ai-allow", &tr!("Run it")).on_click(
                                        cx.listener(move |v, _, _, cx| v.answer_sudo(&run, false, cx)),
                                    ))
                                    .when(saved, |d| {
                                        d.child(
                                            outline_button("ai-sudo-saved", &tr!("Use the saved password"))
                                                .on_click(cx.listener(move |v, _, _, cx| {
                                                    v.answer_sudo(&use_saved, true, cx)
                                                })),
                                        )
                                    })
                                    .child(
                                        outline_button("ai-refuse", &tr!("Refuse it"))
                                            .text_color(rgb(colors().bad))
                                            .on_click(cx.listener(move |v, _, _, cx| {
                                                v.answer(&refuse, Approval::Refuse, cx)
                                            })),
                                    ),
                            ),
                    )
                })
                .when(pane, |d| {
                    let (allow, refuse) = (once.clone(), refuse.clone());
                    d.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                primary_button("ai-allow", &tr!("Allow for 15 minutes")).on_click(
                                    cx.listener(move |v, _, _, cx| v.answer(&allow, Approval::Once, cx)),
                                ),
                            )
                            .child(
                                outline_button("ai-refuse", &tr!("Refuse it"))
                                    .text_color(rgb(colors().bad))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.answer(&refuse, Approval::Refuse, cx)
                                    })),
                            ),
                    )
                })
                .when(!adds && !trusts && !sudo && !pane, |d| d.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            primary_button("ai-allow", &tr!("Run it"))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.answer(&once, Approval::Once, cx)
                                })),
                        )
                        // A snippet typed without Enter asks every time: no
                        // session to allow it for.
                        .when(
                            why != shellwing_core::ai::snippets::LOOK
                                && !why.starts_with(shellwing_core::ai::snippets::ENVS),
                            |d| {
                            d.child(
                                outline_button("ai-allow-session", &tr!("Run it, and this command again this session"))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.answer(&session, Approval::Session, cx)
                                    })),
                            )
                        })
                        .child(
                            outline_button("ai-refuse", &tr!("Refuse it"))
                                .text_color(rgb(colors().bad))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.answer(&refuse, Approval::Refuse, cx)
                                })),
                        ),
                ));
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
        let viewer = self.view(a, cx);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(head)
            .child(
                // The command at a prompt, and what it printed, as a tab to
                // the host would show them.
                div()
                    .id("ai-output")
                    .debug_selector(|| "ai-output".into())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(viewer),
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
        if std::mem::take(&mut self.save_after_drag) {
            cx.defer_in(window, |v, window, cx| v.save_width(window, cx));
        }
        let width = list_width(cx);
        let dragging = self.list_drag.is_some();
        div()
            .id("ai-activity")
            .debug_selector(|| "ai-activity".into())
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(rgb(colors().page))
            .text_color(rgb(colors().text))
            .text_size(ui(13.))
            .child(self.list(&all, shown.as_deref(), cx))
            .children(detail)
            // The list's edge: drag to resize, double-click for the default,
            // as the host list's.
            .child(
                div()
                    .id("ai-list-edge")
                    .debug_selector(|| "ai-list-edge".into())
                    .group("ai-list-edge")
                    .absolute()
                    .top_0()
                    .h_full()
                    .left(px(width - 3.))
                    .w(px(6.))
                    .flex()
                    .justify_center()
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |v, e: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if e.click_count >= 2 {
                                v.list_drag = None;
                                cx.set_global(ListWidth(LIST_WIDTH));
                                v.save_width(window, cx);
                            } else {
                                v.list_drag = Some((f32::from(e.position.x), width));
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .h_full()
                            .w(px(if dragging { 3. } else { 1. }))
                            .bg(rgb(if dragging {
                                colors().accent
                            } else {
                                colors().line
                            }))
                            .group_hover("ai-list-edge", |s| s.w(px(2.)).bg(rgb(colors().accent))),
                    ),
            )
            // Held, the edge follows the pointer anywhere in the window, as a
            // scroll bar's thumb does: out of this view too.
            .when(dragging, |d| {
                d.child(crate::scrollbar::hold(
                    cx.entity().downgrade(),
                    |v: &mut Self, at, cx| {
                        if let Some((x0, w0)) = v.list_drag {
                            let w =
                                (w0 + f32::from(at.x) - x0).clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
                            cx.set_global(ListWidth(w));
                            cx.notify();
                        }
                    },
                    |v: &mut Self, cx| {
                        if v.list_drag.take().is_some() {
                            v.save_after_drag = true;
                        }
                        cx.notify();
                    },
                ))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, window, cx| v.end_drag(window, cx)),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, window, cx| v.end_drag(window, cx)),
                )
            })
    }
}

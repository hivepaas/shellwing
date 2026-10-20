//! The Snippets page of Settings: the commands kept in the vault, to make,
//! edit and delete. They run from the palette (see `snippets`).

use gpui::{
    Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Render,
    SharedString, WeakEntity, Window, anchored, deferred, div, prelude::*, px, rgb,
};
use shellwing_i18n::tr;

use crate::colors::colors;
use crate::host_editor::{
    checkbox, hint_text, label_text, outline_button, primary_button, problem_text,
};
use crate::input::{Changed, TextInput};
use crate::look::ui;
use crate::secrets::VaultSlot;
use crate::snippets::{self, Scope, Snippet};
use crate::workspace::Workspace;

/// Which list of the scope is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Group,
    Host,
}

enum Dialog {
    Edit {
        id: String,
        name: Entity<TextInput>,
        command: Entity<TextInput>,
        scope: Scope,
        enter: bool,
        pick: Option<Pick>,
        problem: Option<String>,
        _changed: gpui::Subscription,
    },
    Delete {
        snippet: Snippet,
    },
}

pub struct SnippetsView {
    workspace: WeakEntity<Workspace>,
    vault: VaultSlot,
    focus: FocusHandle,
    list: Vec<Snippet>,
    dialog: Option<Dialog>,
    notice: Option<(SharedString, bool)>,
    /// The terminal's font, for commands.
    mono: SharedString,
}

impl SnippetsView {
    pub fn new(workspace: WeakEntity<Workspace>, vault: VaultSlot, cx: &mut Context<Self>) -> Self {
        let mut v = SnippetsView {
            workspace,
            vault,
            focus: cx.focus_handle(),
            list: vec![],
            dialog: None,
            notice: None,
            mono: SharedString::default(),
        };
        v.refresh(cx);
        v
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        cx.spawn(async move |this, cx| {
            let list = cx
                .background_executor()
                .spawn(async move { vault.get().and_then(|v| v.snippets().ok()) })
                .await;
            let _ = this.update(cx, |v, cx| {
                match list {
                    Some(l) => v.list = l,
                    None => v.notice = Some((tr!("The vault is not open.").into(), true)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn input(
        placeholder: impl Fn() -> String + 'static,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        cx.new(|cx| {
            let mut i = TextInput::said(placeholder, cx);
            i.set_text(text, text.len()..text.len(), cx);
            i
        })
    }

    fn edit(&mut self, s: Option<Snippet>, window: &mut Window, cx: &mut Context<Self>) {
        let s = s.unwrap_or(Snippet {
            id: String::new(),
            name: String::new(),
            command: String::new(),
            scope: String::new(),
            enter: true,
        });
        let name = Self::input(
            || tr!("What it does, to find it in the palette"),
            &s.name,
            cx,
        );
        let command = Self::input(|| "df -h / && free -h".to_string(), &s.command, cx);
        // The names asked for show as the command is typed.
        let changed = cx.subscribe(&command, |_, _, _: &Changed, cx| cx.notify());
        let first = if s.name.is_empty() {
            name.clone()
        } else {
            command.clone()
        };
        self.dialog = Some(Dialog::Edit {
            id: s.id,
            name,
            command,
            scope: Scope::parse(&s.scope),
            enter: s.enter,
            pick: None,
            problem: None,
            _changed: changed,
        });
        let f = first.read(cx).focus_handle(cx);
        window.focus(&f, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn vault_changed(&self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, _| ws.vault_changed());
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Edit {
            id,
            name,
            command,
            scope,
            enter,
            problem,
            ..
        }) = &mut self.dialog
        else {
            return;
        };
        let (n, c) = (
            name.read(cx).text().trim().to_string(),
            command.read(cx).text().trim().to_string(),
        );
        if n.is_empty() || c.is_empty() {
            *problem = Some(tr!("A name and a command are needed."));
            return cx.notify();
        }
        let s = Snippet {
            id: id.clone(),
            name: n,
            command: c,
            scope: scope.field(),
            enter: *enter,
        };
        match self
            .vault
            .get()
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| v.save_snippet(&s).map_err(|e| e.to_string()))
        {
            Ok(_) => {
                self.close(window, cx);
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => {
                if let Some(Dialog::Edit { problem, .. }) = &mut self.dialog {
                    *problem = Some(tr!("Not saved: {e}", e = e));
                }
                cx.notify();
            }
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Delete { snippet }) = &self.dialog else {
            return;
        };
        let (id, name) = (snippet.id.clone(), snippet.name.clone());
        let done = self
            .vault
            .get()
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| v.delete(&id).map_err(|e| e.to_string()));
        self.close(window, cx);
        match done {
            Ok(()) => {
                self.notice = Some((tr!("Deleted {name}.", name = name.as_str()).into(), false));
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.notice = Some((tr!("Not deleted: {e}", e = e).into(), true)),
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_none() {
            return;
        }
        match ev.keystroke.key.as_str() {
            "escape" => self.close(window, cx),
            "enter" => match &self.dialog {
                Some(Dialog::Edit { .. }) => self.save(window, cx),
                Some(Dialog::Delete { .. }) => self.delete(window, cx),
                None => {}
            },
            _ => return,
        }
        cx.stop_propagation();
    }

    fn small(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id.into())
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .flex_shrink_0()
            .rounded(px(6.))
            .bg(rgb(colors().button))
            .text_size(ui(12.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(colors().button_hover)))
            .child(label.to_string())
    }

    fn field(
        title: &str,
        input: &Entity<TextInput>,
        focused: bool,
        mono: Option<SharedString>,
    ) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(label_text(title))
            .child(
                div()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if focused {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().field))
                    .text_size(ui(13.))
                    .when_some(mono, |d, f| d.font_family(f))
                    .child(input.clone()),
            )
    }

    fn rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        self.list
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let scope = Scope::parse(&s.scope);
                let (edit, del) = (s.clone(), s.clone());
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(10.))
                    .when(i > 0, |d| d.border_t_1().border_color(rgb(colors().line)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div().font_weight(FontWeight::MEDIUM).child(s.name.clone()),
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
                                            .child(scope.label()),
                                    )
                                    .when(!s.enter, |d| {
                                        d.child(
                                            div()
                                                .text_size(ui(11.))
                                                .text_color(rgb(colors().muted))
                                                .child(tr!("typed, not run")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .font_family(self.mono.clone())
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().soft))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(s.command.clone()),
                            ),
                    )
                    .child(Self::small(("snippet-edit", i), &tr!("Edit…")).on_click(
                        cx.listener(move |v, _, w, cx| v.edit(Some(edit.clone()), w, cx)),
                    ))
                    .child(
                        Self::small(("snippet-delete", i), &tr!("Delete"))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                v.dialog = Some(Dialog::Delete {
                                    snippet: del.clone(),
                                });
                                window_focus(&v.focus, w, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn dialog_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let d = self.dialog.as_ref()?;
        let card = div()
            .id("snippet-dialog")
            .w(px(600.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(24.))
            .py(px(22.))
            .rounded(px(12.))
            .bg(rgb(colors().surface))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_color(rgb(colors().text))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let title = |s: &str| {
            div()
                .text_size(ui(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(s.to_string())
        };
        let buttons = |ok: &str, cx: &mut Context<Self>| {
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .pt(px(2.))
                .child(
                    outline_button("snippet-cancel", &tr!("Cancel"))
                        .on_click(cx.listener(|v, _, w, cx| v.close(w, cx))),
                )
                .child(
                    primary_button("snippet-ok", ok).on_click(cx.listener(|v, _, w, cx| {
                        match &v.dialog {
                            Some(Dialog::Edit { .. }) => v.save(w, cx),
                            Some(Dialog::Delete { .. }) => v.delete(w, cx),
                            None => {}
                        }
                    })),
                )
        };
        Some(match d {
            Dialog::Delete { snippet } => card
                .child(title(&tr!("Delete {name}?", name = snippet.name.as_str())))
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().label))
                        .child(tr!(
                            "It goes from the vault, and from your other devices once they sync."
                        )),
                )
                .child(buttons(&tr!("Delete"), cx)),
            Dialog::Edit {
                id,
                name,
                command,
                scope,
                enter,
                pick,
                problem,
                ..
            } => {
                let focused =
                    |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
                let asked = snippets::asked(command.read(cx).text());
                let mut vars = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .text_size(ui(11.5))
                    .text_color(rgb(colors().muted));
                if asked.is_empty() {
                    vars = vars.child(tr!("{{host}}, {{user}} and {{address}} are filled in; any other {{name}} is asked for when it runs."));
                } else {
                    vars = vars.child(tr!("Asked for when it runs:"));
                    for a in asked {
                        vars = vars.child(
                            div()
                                .px(px(7.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .bg(rgb(colors().warn_bg))
                                .text_color(rgb(colors().warn))
                                .font_family(self.mono.clone())
                                .child(a),
                        );
                    }
                }
                vars = vars.child(
                    div()
                        .ml(px(6.))
                        .child(tr!("Chain commands with && or ; on one line.")),
                );

                let (groups, hosts) = self
                    .workspace
                    .upgrade()
                    .map(|w| w.read(cx).snippet_scopes())
                    .unwrap_or_default();
                let mut seg = div()
                    .flex()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .overflow_hidden();
                let choices = [
                    (0usize, tr!("Every host"), matches!(scope, Scope::Every)),
                    (
                        1,
                        match scope {
                            Scope::Group(g) => format!("{g} ▾"),
                            _ => tr!("A group ▾"),
                        },
                        matches!(scope, Scope::Group(_)),
                    ),
                    (
                        2,
                        match scope {
                            Scope::Host(h) => format!("{h} ▾"),
                            _ => tr!("One host ▾"),
                        },
                        matches!(scope, Scope::Host(_)),
                    ),
                ];
                for (n, label, on) in choices {
                    seg = seg.child(
                        div()
                            .id(("scope", n))
                            .flex_1()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(ui(12.5))
                            .text_color(rgb(if on { colors().text } else { colors().soft }))
                            .cursor_pointer()
                            .when(n > 0, |d| {
                                d.border_l_1().border_color(rgb(colors().field_edge))
                            })
                            .when(on, |d| {
                                d.bg(rgb(colors().selected))
                                    .border_b_2()
                                    .border_color(rgb(colors().accent))
                            })
                            .when(!on, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(Dialog::Edit { scope, pick, .. }) = &mut v.dialog {
                                    match n {
                                        0 => {
                                            *scope = Scope::Every;
                                            *pick = None;
                                        }
                                        1 => {
                                            *pick =
                                                (*pick != Some(Pick::Group)).then_some(Pick::Group)
                                        }
                                        _ => {
                                            *pick =
                                                (*pick != Some(Pick::Host)).then_some(Pick::Host)
                                        }
                                    }
                                }
                                cx.notify();
                            }))
                            .child(label),
                    );
                }
                let mut scope_box = div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(label_text(&tr!("Offered for")))
                    .child(seg);
                if let Some(p) = pick {
                    let (names, empty) = match p {
                        Pick::Group => (groups, tr!("No groups yet: make one in the host list.")),
                        Pick::Host => (hosts, tr!("No hosts yet.")),
                    };
                    let mut list = div()
                        .id("scope-list")
                        .flex()
                        .flex_col()
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .p(px(4.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(colors().edge))
                        .bg(rgb(colors().raised));
                    if names.is_empty() {
                        list = list.child(
                            div()
                                .px_2()
                                .py(px(5.))
                                .text_size(ui(12.5))
                                .text_color(rgb(colors().muted))
                                .child(empty),
                        );
                    }
                    let p = *p;
                    for (i, name) in names.into_iter().enumerate() {
                        let shown = name.clone();
                        let chosen = match (p, scope) {
                            (Pick::Group, Scope::Group(g)) | (Pick::Host, Scope::Host(g)) => {
                                *g == name
                            }
                            _ => false,
                        };
                        list = list.child(
                            div()
                                .id(("scope-choice", i))
                                .px_2()
                                .py(px(5.))
                                .rounded(px(4.))
                                .text_size(ui(13.))
                                .cursor_pointer()
                                .when(chosen, |d| d.bg(rgb(colors().selected)))
                                .hover(|s| s.bg(rgb(colors().hover)))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    if let Some(Dialog::Edit { scope, pick, .. }) = &mut v.dialog {
                                        *scope = match p {
                                            Pick::Group => Scope::Group(name.clone()),
                                            Pick::Host => Scope::Host(name.clone()),
                                        };
                                        *pick = None;
                                    }
                                    cx.notify();
                                }))
                                .child(shown),
                        );
                    }
                    scope_box = scope_box.child(list);
                }
                scope_box = scope_box.child(hint_text(&tr!(
                    "The palette lists it in panes of those hosts only."
                )));

                let enter_on = *enter;
                let mut b = card
                    .child(title(&if id.is_empty() {
                        tr!("New snippet")
                    } else {
                        tr!("Edit snippet")
                    }))
                    .child(Self::field(&tr!("Name"), name, focused(name), None))
                    .child(
                        Self::field(
                            &tr!("Command"),
                            command,
                            focused(command),
                            Some(self.mono.clone()),
                        )
                        .child(vars),
                    )
                    .child(scope_box)
                    .child(
                        div()
                            .id("snippet-enter")
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .on_click(cx.listener(|v, _, _, cx| {
                                if let Some(Dialog::Edit { enter, .. }) = &mut v.dialog {
                                    *enter = !*enter;
                                }
                                cx.notify();
                            }))
                            .child(checkbox(enter_on))
                            .child(label_text(&tr!("Run it: press Enter after typing it"))),
                    );
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(&tr!("Save"), cx))
            }
        })
    }
}

fn window_focus(f: &FocusHandle, window: &mut Window, cx: &mut Context<SnippetsView>) {
    window.focus(f, cx);
}

impl Render for SnippetsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let rows = self.rows(cx);
        let (chord, alt) = if cfg!(target_os = "macos") {
            ("Cmd+K", "Option+Enter")
        } else {
            ("Ctrl+Shift+K", "Alt+Enter")
        };
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().line));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(tr!("None yet.")),
            );
        }
        for r in rows {
            list = list.child(r);
        }
        let mut page = div()
            .id("snippets")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .flex_col()
            .gap(px(18.))
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
                            .child(tr!("Snippets")),
                    )
                    .child(
                        primary_button("snippet-new", &tr!("New snippet…"))
                            .on_click(cx.listener(|v, _, w, cx| v.edit(None, w, cx))),
                    ),
            )
            .child(div().mt(px(-6.)).text_size(ui(12.5)).text_color(rgb(colors().label)).child(tr!(
                "Commands you run often. Run one from the palette ({chord}, then type its name): Enter runs it in the pane, {alt} in every pane of the tab. Kept in the vault, so they sync with your hosts.",
                chord = chord,
                alt = alt
            )));
        if let Some((msg, bad)) = &self.notice {
            page = page.child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(if *bad { colors().bad } else { colors().good }))
                    .child(msg.clone()),
            );
        }
        page = page.child(list).child(
            div()
                .text_size(ui(12.))
                .text_color(rgb(colors().muted))
                .child(tr!("{{host}}, {{user}} and {{address}} are filled in for the pane's host; any other {{name}} is asked for when the snippet runs.")),
        );
        if let Some(card) = self.dialog_card(window, cx) {
            let (at, shade) = crate::chrome::shade(window);
            page = page.child(
                deferred(
                    anchored().position(at).child(
                        shade
                            .id("snippet-backdrop")
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(card),
                    ),
                )
                .with_priority(2),
            );
        }
        page
    }
}

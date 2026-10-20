//! The Environments page: its list, and the dialog that edits one, a name
//! and a value a row. A value kept in the vault shows masked; showing it
//! asks the user first (Windows Hello or Touch ID, else the vault's
//! passphrase).

use std::time::Duration;

use super::confirm::Then;
use super::*;
use crate::environment;
use gpui::{App, svg};
use shellwing_vault::EnvValues;

/// How long values stay shown.
const SHOWN_FOR: Duration = Duration::from_secs(60);

/// A variable in the dialog.
pub(super) struct VarRow {
    name: Entity<TextInput>,
    value: Entity<TextInput>,
    /// Its value is the one in the vault, masked and not loaded: saving
    /// keeps it.
    kept: bool,
    /// Its kept value is shown (the eye), to mask again while unchanged.
    revealed: bool,
}

/// Where showing the values stands.
pub(super) enum Reveal {
    Hidden,
    /// Asked (see [`confirm`](super::confirm)): the values as they are in
    /// the vault, for the eyes to show at once, for a while.
    Open(EnvValues),
}

pub(super) struct EnvEdit {
    id: String,
    name: Entity<TextInput>,
    rows: Vec<VarRow>,
    pub(super) scope: Scope,
    pub(super) pick: Option<Pick>,
    reveal: Reveal,
    /// Counts each opening, so a timer closes only its own.
    shown: u64,
    problem: Option<String>,
}

impl SnippetsView {
    fn env_edit(&mut self) -> Option<&mut EnvEdit> {
        match &mut self.dialog {
            Some(Dialog::EditEnv(e)) => Some(e),
            _ => None,
        }
    }

    fn var_row(name: &str, value: &str, kept: bool, cx: &mut Context<Self>) -> VarRow {
        let name = Self::input(|| "NAME".to_string(), name, cx);
        let value = cx.new(|cx| {
            let mut i = TextInput::said(|| tr!("value"), cx).multiline(1, 6);
            i.set_text(value, value.len()..value.len(), cx);
            i
        });
        VarRow {
            name,
            value,
            kept,
            revealed: false,
        }
    }

    pub(super) fn edit_env(
        &mut self,
        e: Option<Environment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let e = e.unwrap_or(Environment {
            id: String::new(),
            name: String::new(),
            scope: String::new(),
            names: vec![],
        });
        let name = Self::input(
            || tr!("What they are for, such as AWS production"),
            &e.name,
            cx,
        );
        let mut rows: Vec<VarRow> = e
            .names
            .iter()
            .map(|n| Self::var_row(n, "", true, cx))
            .collect();
        if rows.is_empty() {
            rows.push(Self::var_row("", "", false, cx));
        }
        let first = if e.name.is_empty() {
            name.clone()
        } else {
            rows[0].name.clone()
        };
        self.dialog = Some(Dialog::EditEnv(Box::new(EnvEdit {
            id: e.id,
            name,
            rows,
            scope: Scope::parse(&e.scope),
            pick: None,
            reveal: Reveal::Hidden,
            shown: 0,
            problem: None,
        })));
        let f = first.read(cx).focus_handle(cx);
        window.focus(&f, cx);
        cx.notify();
    }

    /// Enter in the dialog: answers the question to show the values, if it
    /// is being asked, else saves.
    pub(super) fn env_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm.is_some() {
            return self.confirm_answer(cx);
        }
        self.save_env(window, cx);
    }

    pub(super) fn save_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.env_edit() else {
            return;
        };
        let n = e.name.read(cx).text().trim().to_string();
        let mut vars: environment::Parsed = vec![];
        let mut said = None;
        for row in &e.rows {
            let name = row.name.read(cx).text().trim().to_string();
            let value = row.value.read(cx).text().to_string();
            if name.is_empty() && value.is_empty() && !row.kept {
                continue;
            }
            said = if !environment::valid_name(&name) {
                Some(tr!("{name} is not a variable's name.", name = name.clone()))
            } else if vars.iter().any(|(k, _)| *k == name) {
                Some(tr!("{name} is there twice.", name = name.clone()))
            } else if !row.kept && value.is_empty() {
                Some(tr!("{name} has no value.", name = name.as_str()))
            } else {
                None
            };
            if said.is_some() {
                break;
            }
            vars.push((name, (!row.kept).then_some(value)));
        }
        if n.is_empty() {
            said = Some(tr!("A name is needed."));
        } else if said.is_none() && vars.is_empty() {
            said = Some(tr!("Add a variable: a name and its value."));
        }
        if let Some(said) = said {
            e.problem = Some(said);
            return cx.notify();
        }
        let (id, scope) = (e.id.clone(), e.scope.field());
        let saved = self
            .vault
            .get()
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| {
                v.save_environment(&id, &n, &scope, &vars)
                    .map_err(|e| environment::said_error(&e))
            });
        match saved {
            Ok(_) => {
                self.close(window, cx);
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(err) => {
                if let Some(e) = self.env_edit() {
                    e.problem = Some(tr!("Not saved: {e}", e = err));
                }
                cx.notify();
            }
        }
    }

    pub(super) fn delete_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::DeleteEnv { env }) = &self.dialog else {
            return;
        };
        let (id, name) = (env.id.clone(), env.name.clone());
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

    /// Fills the rows from `.env` text on the clipboard: a name already
    /// there takes the value, others are added.
    fn paste_dotenv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        let parsed = environment::parse_dotenv(&text);
        let Some(e) = self.env_edit() else {
            return;
        };
        let vars = match parsed {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                e.problem = Some(tr!("The clipboard holds no NAME=value lines."));
                return cx.notify();
            }
            Err(err) => {
                e.problem = Some(tr!("Not NAME=value: {e}", e = err));
                return cx.notify();
            }
        };
        e.problem = None;
        let mut rows = std::mem::take(&mut e.rows);
        rows.retain(|r| {
            r.kept
                || !r.name.read(cx).text().trim().is_empty()
                || !r.value.read(cx).text().is_empty()
        });
        for (name, value) in vars {
            let value = value.unwrap_or_default();
            match rows
                .iter_mut()
                .find(|r| r.name.read(cx).text().trim() == name)
            {
                Some(r) => {
                    r.kept = false;
                    r.revealed = false;
                    r.value
                        .update(cx, |i, cx| i.set_text(&value, value.len()..value.len(), cx));
                }
                None => rows.push(Self::var_row(&name, &value, false, cx)),
            }
        }
        if let Some(e) = self.env_edit() {
            e.rows = rows;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The eye of row `i`: shows its value, asking it is the user first
    /// unless asked a moment ago.
    fn eye(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(EnvEdit {
            reveal: Reveal::Open(_),
            ..
        }) = self.env_edit()
        {
            return self.reveal_row(i, cx);
        }
        self.ask_confirm(Then::EnvRow(i), window, cx);
    }

    /// Asked: the values from the vault, row `row`'s shown, and the eyes
    /// open at once for a while.
    pub(super) fn open_values(&mut self, row: usize, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        let Some(e) = self.env_edit() else {
            return;
        };
        let values = match vault.ready().map(|v| v.environment_values(&e.id)) {
            Some(Ok(Some(values))) => values,
            Some(Err(err)) => {
                e.problem = Some(err.to_string());
                e.reveal = Reveal::Hidden;
                return cx.notify();
            }
            _ => vec![],
        };
        e.reveal = Reveal::Open(values);
        e.shown += 1;
        let shown = e.shown;
        self.reveal_row(row, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN_FOR).await;
            let _ = this.update(cx, |v, cx| {
                if v.env_edit().is_some_and(|e| e.shown == shown) {
                    v.close_values(cx);
                }
            });
        })
        .detach();
    }

    /// Shows row `i`'s kept value.
    fn reveal_row(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(EnvEdit {
            rows,
            reveal: Reveal::Open(values),
            ..
        }) = self.env_edit()
        else {
            return;
        };
        let Some(r) = rows.get_mut(i).filter(|r| r.kept) else {
            return;
        };
        let name = r.name.read(cx).text().trim().to_string();
        let Some(value) = values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
        else {
            return;
        };
        r.kept = false;
        r.revealed = true;
        let input = r.value.clone();
        input.update(cx, |i, cx| i.set_text(&value, value.len()..value.len(), cx));
        cx.notify();
    }

    /// Whether row `r` still holds the value it was shown with.
    fn unchanged(values: &EnvValues, r: &VarRow, cx: &App) -> bool {
        let name = r.name.read(cx).text().trim().to_string();
        r.revealed
            && values
                .iter()
                .any(|(n, v)| *n == name && v.as_str() == r.value.read(cx).text())
    }

    /// Masks row `i` again, unchanged.
    fn hide_row(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(EnvEdit {
            rows,
            reveal: Reveal::Open(values),
            ..
        }) = self.env_edit()
        else {
            return;
        };
        let Some(r) = rows.get_mut(i) else {
            return;
        };
        if Self::unchanged(values, r, cx) {
            r.kept = true;
            r.revealed = false;
            r.value.update(cx, |i, cx| i.set_text("", 0..0, cx));
        }
        cx.notify();
    }

    /// The while is over: the values shown and unchanged masked again, and
    /// the next eye asks again.
    fn close_values(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.env_edit() else {
            return;
        };
        let Reveal::Open(values) = std::mem::replace(&mut e.reveal, Reveal::Hidden) else {
            return;
        };
        for r in &mut e.rows {
            if Self::unchanged(&values, r, cx) {
                r.kept = true;
                r.value.update(cx, |i, cx| i.set_text("", 0..0, cx));
            }
            r.revealed = false;
        }
        cx.notify();
    }

    pub(super) fn env_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        self.envs
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let scope = Scope::parse(&e.scope);
                let (edit, del) = (e.clone(), e.clone());
                let used = self.list.iter().filter(|s| s.needs.contains(&e.id)).count();
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
                                        div().font_weight(FontWeight::MEDIUM).child(e.name.clone()),
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
                                    .when(used > 0, |d| {
                                        d.child(
                                            div()
                                                .text_size(ui(11.))
                                                .text_color(rgb(colors().muted))
                                                .child(tr_n!(
                                                    used,
                                                    "needed by {n} snippet",
                                                    "needed by {n} snippets"
                                                )),
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
                                    .child(e.names.join("  ")),
                            ),
                    )
                    .child(Self::small(("env-edit", i), &tr!("Edit…")).on_click(
                        cx.listener(move |v, _, w, cx| v.edit_env(Some(edit.clone()), w, cx)),
                    ))
                    .child(
                        Self::small(("env-delete", i), &tr!("Delete"))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                v.dialog = Some(Dialog::DeleteEnv { env: del.clone() });
                                window_focus(&v.focus, w, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// A small icon that does something.
    fn icon_tool(id: impl Into<gpui::ElementId>, icon: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id.into())
            .flex_shrink_0()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(colors().hover)))
            .child(
                svg()
                    .path(icon)
                    .size(px(14.))
                    .text_color(rgb(colors().muted)),
            )
    }

    /// A row of the dialog: the name, the value (masked while kept, an eye
    /// to show it), and a button that takes the row out.
    fn var_line(
        &self,
        i: usize,
        row: &VarRow,
        open: Option<&EnvValues>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let focused = |e: &Entity<TextInput>| e.read(cx).focus_handle(cx).is_focused(window);
        let (name_on, value_on) = (focused(&row.name), focused(&row.value));
        let value = if row.kept {
            let input = row.value.clone();
            Self::field_shell(false, None)
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(colors().muted))
                        .child("••••••••••••"),
                )
                .child(
                    Self::icon_tool(("env-eye", i), "icons/eye.svg")
                        .on_click(cx.listener(move |v, _, w, cx| v.eye(i, w, cx))),
                )
                .child(
                    div()
                        .id(("env-change", i))
                        .flex_shrink_0()
                        .text_size(ui(12.))
                        .text_color(rgb(colors().accent))
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(cx.listener(move |v, _, w, cx| {
                            if let Some(e) = v.env_edit()
                                && let Some(r) = e.rows.get_mut(i)
                            {
                                r.kept = false;
                                r.revealed = false;
                            }
                            let f = input.read(cx).focus_handle(cx);
                            w.focus(&f, cx);
                            cx.notify();
                        }))
                        .child(tr!("Change")),
                )
        } else {
            let unchanged = open.is_some_and(|values| Self::unchanged(values, row, cx));
            Self::field_shell(value_on, Some(self.mono.clone()))
                .flex_1()
                .min_w_0()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(row.value.clone()))
                .when(unchanged, |d| {
                    d.child(
                        Self::icon_tool(("env-eye-off", i), "icons/eye-off.svg")
                            .on_click(cx.listener(move |v, _, _, cx| v.hide_row(i, cx))),
                    )
                })
        };
        div()
            .flex()
            .items_start()
            .gap(px(8.))
            .child(
                Self::field_box(&row.name, name_on, Some(self.mono.clone()))
                    .w(px(190.))
                    .flex_shrink_0(),
            )
            .child(value)
            .child(
                div()
                    .id(("env-row-out", i))
                    .h(px(32.))
                    .w(px(26.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(rgb(colors().muted))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().bad)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if let Some(e) = v.env_edit()
                            && i < e.rows.len()
                        {
                            e.rows.remove(i);
                        }
                        // Indices moved: a question about a row is moot.
                        if v.confirm
                            .as_ref()
                            .is_some_and(|c| c.then != Then::FileContent)
                        {
                            v.confirm = None;
                        }
                        cx.notify();
                    }))
                    .child("✕"),
            )
    }

    pub(super) fn env_card(
        &self,
        e: &EnvEdit,
        card: gpui::Stateful<gpui::Div>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let scope_box = self.scope_box(&e.scope, &e.pick, cx).child(hint_text(&tr!(
            "The palette offers to load it in panes of those hosts only."
        )));
        let focused = |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
        let name_field = Self::field(&tr!("Name"), &e.name, focused(&e.name), None);
        let mut vars = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label_text(&tr!("Variables")));
        let open = match &e.reveal {
            Reveal::Open(values) => Some(values),
            _ => None,
        };
        for (i, row) in e.rows.iter().enumerate() {
            vars = vars.child(self.var_line(i, row, open, window, cx));
        }
        vars = vars.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    Self::small("env-add", &tr!("Add a variable")).on_click(cx.listener(
                        |v, _, w, cx| {
                            let row = Self::var_row("", "", false, cx);
                            let f = row.name.read(cx).focus_handle(cx);
                            if let Some(e) = v.env_edit() {
                                e.rows.push(row);
                            }
                            w.focus(&f, cx);
                            cx.notify();
                        },
                    )),
                )
                .child(
                    Self::small("env-paste", &tr!("Paste a .env file"))
                        .on_click(cx.listener(|v, _, w, cx| v.paste_dotenv(w, cx))),
                ),
        );
        if let Some(line) = self.confirm_line(window, cx) {
            vars = vars.child(line);
        }
        let save = if cfg!(target_os = "macos") {
            "Cmd+Enter"
        } else {
            "Ctrl+Enter"
        };
        vars = vars.child(hint_text(&tr!(
            "A value can take several lines: Enter starts another, {save} saves.",
            save = save
        )));
        let mut b = card
            .child(dialog_title(&if e.id.is_empty() {
                tr!("New environment")
            } else {
                tr!("Edit environment")
            }))
            .child(name_field)
            .child(vars)
            .child(scope_box);
        if let Some(p) = &e.problem {
            b = b.child(problem_text(p));
        }
        b.child(Self::buttons(&tr!("Save"), cx))
    }
}

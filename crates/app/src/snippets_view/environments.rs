//! The Environments page: its list, and the dialog that edits one, a name
//! and a value a row. A value kept in the vault shows masked; showing it
//! asks the user first (Windows Hello or Touch ID, else the vault's
//! passphrase).

use std::time::Duration;

use super::*;
use crate::environment;
use shellwing_vault::{EnvValues, Error as VaultError};

/// How long values stay shown.
const SHOWN_FOR: Duration = Duration::from_secs(60);

/// A variable in the dialog.
pub(super) struct VarRow {
    name: Entity<TextInput>,
    value: Entity<TextInput>,
    /// Its value is the one in the vault, masked and not loaded: saving
    /// keeps it.
    kept: bool,
}

/// Where showing the values stands.
pub(super) enum Reveal {
    Hidden,
    /// Asking first: for the vault's passphrase, or (None: none guards the
    /// vault) for a yes.
    Asking {
        passphrase: Option<Entity<TextInput>>,
        problem: Option<String>,
    },
    /// Windows Hello or Touch ID is asking.
    Waiting,
    /// Shown, with the values as they were, to mask again those unchanged.
    Shown(EnvValues),
}

pub(super) struct EnvEdit {
    id: String,
    name: Entity<TextInput>,
    rows: Vec<VarRow>,
    pub(super) scope: Scope,
    pub(super) pick: Option<Pick>,
    reveal: Reveal,
    /// Counts each showing, so a timer hides only its own.
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
        VarRow { name, value, kept }
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
        if let Some(EnvEdit {
            reveal: Reveal::Asking { .. },
            ..
        }) = self.env_edit()
        {
            return self.confirm_reveal(cx);
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

    /// Asks it is the user before the values show: Windows Hello or Touch
    /// ID where it opens the vault, else its passphrase, else a yes.
    fn start_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let slot = self.vault.clone();
        let Some(vault) = slot.ready() else {
            return;
        };
        let presence = slot.presence().is_some() && vault.has_presence().unwrap_or(false);
        let passphrase = vault.has_vault_passphrase().unwrap_or(false);
        if presence {
            if let Some(e) = self.env_edit() {
                e.reveal = Reveal::Waiting;
            }
            cx.notify();
            let job = cx.background_spawn(async move { slot.confirm(None) });
            cx.spawn_in(window, async move |this, cx| {
                let done = job.await;
                let _ = this.update_in(cx, |v, window, cx| match done {
                    Ok(()) => v.reveal_values(cx),
                    Err(err) => {
                        // Not there, or cancelled: the passphrase instead.
                        let problem =
                            (!matches!(err, VaultError::NotPresent)).then(|| err.to_string());
                        v.ask_passphrase(passphrase, problem, window, cx);
                    }
                });
            })
            .detach();
        } else {
            self.ask_passphrase(passphrase, None, window, cx);
        }
    }

    fn ask_passphrase(
        &mut self,
        passphrase: bool,
        problem: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = passphrase
            .then(|| cx.new(|cx| TextInput::said(|| tr!("The vault's passphrase"), cx).masked()));
        let focus = input.as_ref().map(|i| i.read(cx).focus_handle(cx));
        if let Some(e) = self.env_edit() {
            e.reveal = Reveal::Asking {
                passphrase: input,
                problem,
            };
        }
        match focus {
            Some(f) => window.focus(&f, cx),
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// The answer to the question: the passphrase checked (away from the
    /// window: its KDF takes a moment), or the yes.
    fn confirm_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(EnvEdit {
            reveal: Reveal::Asking { passphrase, .. },
            ..
        }) = self.env_edit()
        else {
            return;
        };
        let Some(input) = passphrase.clone() else {
            return self.reveal_values(cx);
        };
        let typed = zeroize::Zeroizing::new(input.read(cx).text().to_string());
        input.update(cx, |i, cx| i.set_text("", 0..0, cx));
        let slot = self.vault.clone();
        let job = cx.background_spawn(async move { slot.confirm(Some(typed.as_str())) });
        cx.spawn(async move |this, cx| {
            let done = job.await;
            let _ = this.update(cx, |v, cx| match done {
                Ok(()) => v.reveal_values(cx),
                Err(err) => {
                    if let Some(EnvEdit {
                        reveal: Reveal::Asking { problem, .. },
                        ..
                    }) = v.env_edit()
                    {
                        *problem = Some(match err {
                            VaultError::WrongPassphrase => tr!("Wrong passphrase."),
                            e => e.to_string(),
                        });
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Shows the values kept in the vault, for a while.
    fn reveal_values(&mut self, cx: &mut Context<Self>) {
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
        let rows: Vec<(Entity<TextInput>, String)> = e
            .rows
            .iter_mut()
            .filter(|r| r.kept)
            .filter_map(|r| {
                let name = r.name.read(cx).text().trim().to_string();
                let value = values.iter().find(|(n, _)| *n == name)?.1.to_string();
                r.kept = false;
                Some((r.value.clone(), value))
            })
            .collect();
        e.reveal = Reveal::Shown(values);
        e.shown += 1;
        let shown = e.shown;
        for (input, value) in rows {
            input.update(cx, |i, cx| i.set_text(&value, value.len()..value.len(), cx));
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN_FOR).await;
            let _ = this.update(cx, |v, cx| {
                if v.env_edit().is_some_and(|e| e.shown == shown) {
                    v.hide_values(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Masks again the values shown that were not changed.
    fn hide_values(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.env_edit() else {
            return;
        };
        let Reveal::Shown(values) = std::mem::replace(&mut e.reveal, Reveal::Hidden) else {
            return;
        };
        for r in &mut e.rows {
            let name = r.name.read(cx).text().trim().to_string();
            let same = values
                .iter()
                .any(|(n, v)| *n == name && v.as_str() == r.value.read(cx).text());
            if !r.kept && same {
                r.kept = true;
                r.value.update(cx, |i, cx| i.set_text("", 0..0, cx));
            }
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

    /// A row of the dialog: the name, the value (masked while kept), and a
    /// button that takes the row out.
    fn var_line(
        &self,
        i: usize,
        row: &VarRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let focused = |e: &Entity<TextInput>| e.read(cx).focus_handle(cx).is_focused(window);
        let (name_on, value_on) = (focused(&row.name), focused(&row.value));
        let value = if row.kept {
            let input = row.value.clone();
            div()
                .flex_1()
                .min_w_0()
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(colors().field_edge))
                .bg(rgb(colors().chip))
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(colors().muted))
                        .child("••••••••••••"),
                )
                .child(
                    Self::small(("env-change", i), &tr!("Change")).on_click(cx.listener(
                        move |v, _, w, cx| {
                            if let Some(e) = v.env_edit()
                                && let Some(r) = e.rows.get_mut(i)
                            {
                                r.kept = false;
                            }
                            let f = input.read(cx).focus_handle(cx);
                            w.focus(&f, cx);
                            cx.notify();
                        },
                    )),
                )
        } else {
            Self::field_box(&row.value, value_on, Some(self.mono.clone()))
                .flex_1()
                .min_w_0()
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
                        cx.notify();
                    }))
                    .child("✕"),
            )
    }

    /// What showing the values stands at: a button, the question, or the
    /// wait for the system's prompt.
    fn reveal_line(
        &self,
        e: &EnvEdit,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let kept = e.rows.iter().any(|r| r.kept);
        let line = div().flex().items_center().gap(px(8.)).flex_wrap();
        Some(match &e.reveal {
            Reveal::Hidden if kept => line
                .child(
                    Self::small("env-show", &tr!("Show values"))
                        .on_click(cx.listener(|v, _, w, cx| v.start_reveal(w, cx))),
                )
                .child(hint_text(&tr!(
                    "Values kept in the vault are masked; showing them asks it is you."
                ))),
            Reveal::Hidden => return None,
            Reveal::Waiting => {
                line.child(hint_text(&tr!("Confirm it is you in the system's prompt…")))
            }
            Reveal::Shown(_) => line
                .child(
                    Self::small("env-hide", &tr!("Hide values"))
                        .on_click(cx.listener(|v, _, _, cx| v.hide_values(cx))),
                )
                .child(hint_text(&tr!(
                    "They show on screen: mind screen sharing. They are masked again in a minute."
                ))),
            Reveal::Asking {
                passphrase,
                problem,
            } => {
                let mut ask = div().flex().flex_col().gap(px(6.));
                match passphrase {
                    Some(input) => {
                        let focused = input.read(cx).focus_handle(cx).is_focused(window);
                        ask = ask
                            .child(label_text(&tr!(
                                "The vault's passphrase, to show the values"
                            )))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(Self::field_box(input, focused, None).flex_1())
                                    .child(
                                        Self::small("env-confirm", &tr!("Show"))
                                            .h(px(32.))
                                            .on_click(
                                                cx.listener(|v, _, _, cx| v.confirm_reveal(cx)),
                                            ),
                                    ),
                            );
                    }
                    None => {
                        ask = ask.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(hint_text(&tr!("No passphrase guards the vault, so anyone at this device can show them. Show the values?")).flex_1())
                                .child(Self::small("env-confirm", &tr!("Show")).on_click(
                                    cx.listener(|v, _, _, cx| v.confirm_reveal(cx)),
                                )),
                        );
                    }
                }
                if let Some(p) = problem {
                    ask = ask.child(problem_text(p));
                }
                return Some(ask);
            }
        })
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
        for (i, row) in e.rows.iter().enumerate() {
            vars = vars.child(self.var_line(i, row, window, cx));
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
        if let Some(line) = self.reveal_line(e, window, cx) {
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

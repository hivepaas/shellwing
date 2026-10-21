//! The Snippets page of Settings: the commands kept in the vault, to make,
//! edit and delete. They run from the palette (see `snippets`).

use gpui::{
    Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Render,
    SharedString, WeakEntity, Window, anchored, deferred, div, prelude::*, px, rgb,
};
use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::host_editor::{hint_text, label_text, outline_button, primary_button, problem_text};
use crate::input::TextInput;
use crate::look::ui;
use crate::secrets::VaultSlot;
use crate::snippets::{Scope, Snippet};
use crate::workspace::Workspace;
use shellwing_vault::Environment;

mod config_files;
pub use config_files::size_said;
mod confirm;
mod environments;
mod notes;

/// Which of the pages this view is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Snippets,
    Environments,
    Files,
    Notes,
}

/// Which list of the scope is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    Group,
    Host,
}

impl Dialog {
    /// The scope being chosen, and which of its lists is open.
    fn scope(&mut self) -> Option<(&mut Scope, &mut Option<Pick>)> {
        match self {
            Dialog::EditEnv(e) => Some((&mut e.scope, &mut e.pick)),
            Dialog::EditFile(f) => Some((&mut f.scope, &mut f.pick)),
            _ => None,
        }
    }
}

enum Dialog {
    Delete { snippet: Snippet },
    EditEnv(Box<environments::EnvEdit>),
    DeleteEnv { env: Environment },
    EditFile(Box<config_files::FileEdit>),
    DeleteFile { file: shellwing_vault::ConfigFile },
    DeleteNote { note: shellwing_vault::Note },
}

pub struct SnippetsView {
    workspace: WeakEntity<Workspace>,
    vault: VaultSlot,
    focus: FocusHandle,
    list: Vec<Snippet>,
    /// The environments, to choose what a snippet needs.
    envs: Vec<Environment>,
    dialog: Option<Dialog>,
    notice: Option<(SharedString, bool)>,
    /// The terminal's font, for commands.
    mono: SharedString,
    /// Which page it is.
    shown: Shown,
    /// The config files, for their page.
    files: Vec<shellwing_vault::ConfigFile>,
    /// The notes, for theirs.
    notes: Vec<shellwing_vault::Note>,
    /// Asking it is the user before a secret shows.
    confirm: Option<confirm::Confirm>,
}

impl SnippetsView {
    pub fn new(workspace: WeakEntity<Workspace>, vault: VaultSlot, cx: &mut Context<Self>) -> Self {
        Self::make(workspace, vault, Shown::Snippets, cx)
    }

    /// The Environments page: the variables snippets and shells are given.
    pub fn environments(
        workspace: WeakEntity<Workspace>,
        vault: VaultSlot,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::make(workspace, vault, Shown::Environments, cx)
    }

    /// The Config files page: files to place on hosts.
    pub fn config_files(
        workspace: WeakEntity<Workspace>,
        vault: VaultSlot,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::make(workspace, vault, Shown::Files, cx)
    }

    /// The Notes page: free text, each opening in a window of its own.
    pub fn notes(
        workspace: WeakEntity<Workspace>,
        vault: VaultSlot,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::make(workspace, vault, Shown::Notes, cx)
    }

    fn make(
        workspace: WeakEntity<Workspace>,
        vault: VaultSlot,
        shown: Shown,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = SnippetsView {
            workspace,
            vault,
            focus: cx.focus_handle(),
            list: vec![],
            envs: vec![],
            dialog: None,
            notice: None,
            mono: SharedString::default(),
            shown,
            files: vec![],
            notes: vec![],
            confirm: None,
        };
        v.refresh(cx);
        v
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        cx.spawn(async move |this, cx| {
            let list = cx
                .background_executor()
                .spawn(async move {
                    let v = vault.get()?;
                    Some((
                        v.snippets().ok()?,
                        v.environments().unwrap_or_default(),
                        v.config_files().unwrap_or_default(),
                        v.notes().unwrap_or_default(),
                    ))
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                match list {
                    Some((mut l, envs, mut files, mut notes)) => {
                        notes.sort_by(|a, b| {
                            (a.category.to_lowercase(), a.title.to_lowercase())
                                .cmp(&(b.category.to_lowercase(), b.title.to_lowercase()))
                        });
                        v.notes = notes;
                        files.sort_by(|a, b| {
                            (a.category.to_lowercase(), a.name.to_lowercase())
                                .cmp(&(b.category.to_lowercase(), b.name.to_lowercase()))
                        });
                        v.files = files;
                        // By category, the ones without first.
                        l.sort_by(|a, b| {
                            (a.category.to_lowercase(), a.name.to_lowercase())
                                .cmp(&(b.category.to_lowercase(), b.name.to_lowercase()))
                        });
                        v.list = l;
                        let mut envs = envs;
                        envs.sort_by_key(|e| e.name.to_lowercase());
                        v.envs = envs;
                    }
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

    /// The snippet library, in its window.
    fn open_library(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, cx| ws.open_library(cx));
        }
    }

    /// Opens `s` (None: a new one) in a window of its own, as a note.
    /// The form for a new environment or config file, as its page's button
    /// opens it.
    pub fn new_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown {
            Shown::Environments => self.edit_env(None, window, cx),
            Shown::Files => self.edit_file(None, window, cx),
            Shown::Snippets | Shown::Notes => {}
        }
    }

    fn edit(&mut self, s: Option<Snippet>, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, cx| ws.open_snippet(s, cx));
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        self.confirm = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn vault_changed(&self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, _| ws.vault_changed());
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
                Some(Dialog::Delete { .. }) => self.delete(window, cx),
                Some(Dialog::EditEnv(_)) => self.env_enter(window, cx),
                Some(Dialog::DeleteEnv { .. }) => self.delete_env(window, cx),
                Some(Dialog::EditFile(_)) => self.file_enter(window, cx),
                Some(Dialog::DeleteFile { .. }) => self.delete_file(window, cx),
                Some(Dialog::DeleteNote { .. }) => self.delete_note(window, cx),
                None => {}
            },
            _ => return,
        }
        cx.stop_propagation();
    }

    fn small(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
        let id = id.into();
        let selector = id.to_string();
        div()
            .id(id)
            .debug_selector(|| selector)
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
            .child(Self::field_box(input, focused, mono))
    }

    /// A field's box, without its title.
    fn field_box(
        input: &Entity<TextInput>,
        focused: bool,
        mono: Option<SharedString>,
    ) -> gpui::Div {
        Self::field_shell(focused, mono).child(input.clone())
    }

    /// The box alone, for what goes in it.
    fn field_shell(focused: bool, mono: Option<SharedString>) -> gpui::Div {
        div()
            // One line high, or as many as a field of several lines has.
            .min_h(px(32.))
            .py(px(6.))
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
    }

    /// No snippets yet: the library, to start from.
    fn library_card(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut tasks = div().flex().flex_wrap().gap(px(6.));
        for t in crate::library::TASKS {
            let n = crate::library::items()
                .iter()
                .filter(|i| i.task == t)
                .count();
            tasks = tasks.child(
                div()
                    .h(px(22.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(11.))
                    .bg(rgb(colors().chip))
                    .text_size(ui(11.5))
                    .text_color(rgb(colors().soft))
                    .child(format!("{t} {n}")),
            );
        }
        // A few, as people know them.
        let tools = [
            "Linux",
            "systemd",
            "Docker",
            "Kubernetes",
            "nginx",
            "PostgreSQL",
            "MySQL",
            "Redis",
        ];
        div()
            .debug_selector(|| "snippet-library-card".into())
            .flex()
            .gap(px(18.))
            .p(px(22.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().surface))
            .child(
                div()
                    .flex_1()
                    // As narrow as the card: the text wraps rather than
                    // pushing past its edge.
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(ui(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr!("Start from the library")),
                    )
                    .child(
                        div()
                            .debug_selector(|| "snippet-library-text".into())
                            .text_color(rgb(colors().soft))
                            .line_height(gpui::relative(1.5))
                            .child(tr!(
                                "{n} ready commands for {tools} and more: check a server, back it up, clean it, test it. Pick the ones you want; they become your own snippets, to edit as you like.",
                                n = crate::library::items().len(),
                                tools = tools.join(", ")
                            )),
                    )
                    .child(tasks)
                    .child(
                        div().flex().child(
                            primary_button("snippet-library-open", &tr!("Open the library"))
                                .on_click(cx.listener(|v, _, _, cx| v.open_library(cx))),
                        ),
                    ),
            )
    }

    fn rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        // Under headings once any has a category (sorted by it on refresh).
        let filed = self.list.iter().any(|s| !s.category.trim().is_empty());
        let mut last: Option<String> = None;
        let mut out = vec![];
        for (i, s) in self.list.iter().enumerate() {
            let cat = s.category.trim().to_lowercase();
            if filed && last.as_ref() != Some(&cat) {
                out.push(
                    div()
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(4.))
                        .when(i > 0, |d| d.border_t_1().border_color(rgb(colors().line)))
                        .text_size(ui(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(colors().muted))
                        .child(if cat.is_empty() {
                            tr!("No category")
                        } else {
                            s.category.trim().to_string()
                        })
                        .into_any_element(),
                );
            }
            let first = last.as_ref() != Some(&cat);
            last = Some(cat);
            let needs: Vec<String> = s
                .needs
                .iter()
                .filter_map(|n| self.envs.iter().find(|e| e.id == *n))
                .map(|e| e.name.clone())
                .collect();
            out.push(self.row(i, s, first && filed, needs, cx));
        }
        out
    }

    fn row(
        &self,
        i: usize,
        s: &Snippet,
        headed: bool,
        needs: Vec<String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let scope = Scope::parse(&s.scope);
        let (edit, del) = (s.clone(), s.clone());
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(10.))
            .when(i > 0 && !headed, |d| {
                d.border_t_1().border_color(rgb(colors().line))
            })
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
                            .child(div().font_weight(FontWeight::MEDIUM).child(s.name.clone()))
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
                            .when(s.origin.starts_with(crate::library::ORIGIN), |d| {
                                d.child(
                                    div()
                                        .text_size(ui(11.))
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("from the library")),
                                )
                            })
                            .when(!s.enter, |d| {
                                d.child(
                                    div()
                                        .text_size(ui(11.))
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("typed, not run")),
                                )
                            })
                            .when(!needs.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(ui(11.))
                                        .text_color(rgb(colors().warn))
                                        .child(tr!("with {envs}", envs = needs.join(", "))),
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
                            .child(crate::snippets::one_line(&s.command)),
                    ),
            )
            .child(
                Self::small(("snippet-edit", i), &tr!("Edit…"))
                    .on_click(cx.listener(move |v, _, w, cx| v.edit(Some(edit.clone()), w, cx))),
            )
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
    }

    /// Where it is offered: every host, a group's, or one host's.
    fn scope_box(&self, scope: &Scope, pick: &Option<Pick>, cx: &mut Context<Self>) -> gpui::Div {
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
                        if let Some((scope, pick)) = v.dialog.as_mut().and_then(Dialog::scope) {
                            match n {
                                0 => {
                                    *scope = Scope::Every;
                                    *pick = None;
                                }
                                1 => *pick = (*pick != Some(Pick::Group)).then_some(Pick::Group),
                                _ => *pick = (*pick != Some(Pick::Host)).then_some(Pick::Host),
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
                    (Pick::Group, Scope::Group(g)) | (Pick::Host, Scope::Host(g)) => *g == name,
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
                            if let Some((scope, pick)) = v.dialog.as_mut().and_then(Dialog::scope) {
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
        scope_box
    }

    /// Cancel, and `ok` for what the dialog is for.
    fn buttons(ok: &str, cx: &mut Context<Self>) -> gpui::Div {
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
                primary_button("snippet-ok", ok).on_click(cx.listener(
                    |v, _, w, cx| match &v.dialog {
                        Some(Dialog::Delete { .. }) => v.delete(w, cx),
                        Some(Dialog::EditEnv(_)) => v.save_env(w, cx),
                        Some(Dialog::DeleteEnv { .. }) => v.delete_env(w, cx),
                        Some(Dialog::EditFile(_)) => v.save_file(w, cx),
                        Some(Dialog::DeleteFile { .. }) => v.delete_file(w, cx),
                        Some(Dialog::DeleteNote { .. }) => v.delete_note(w, cx),
                        None => {}
                    },
                )),
            )
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
            .max_h(window.viewport_size().height - px(60.))
            .overflow_y_scroll()
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
        let title = dialog_title;
        let buttons = Self::buttons;
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
            Dialog::DeleteEnv { env } => card
                .child(title(&tr!("Delete {name}?", name = env.name.as_str())))
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().label))
                        .child(tr!(
                            "Its variables go from the vault, and from your other devices once they sync. Shells it is loaded in keep them until unloaded."
                        )),
                )
                .child(buttons(&tr!("Delete"), cx)),
            Dialog::EditEnv(e) => self.env_card(e, card, window, cx),
            Dialog::EditFile(f) => self.file_card(f, card, window, cx),
            Dialog::DeleteNote { note } => card
                .child(title(&tr!("Delete {name}?", name = note.title.as_str())))
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().label))
                        .child(tr!(
                            "It goes from the vault, and from your other devices once they sync."
                        )),
                )
                .child(buttons(&tr!("Delete"), cx)),
            Dialog::DeleteFile { file } => card
                .child(title(&tr!("Delete {name}?", name = file.name.as_str())))
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().label))
                        .child(tr!(
                            "It goes from the vault, and from your other devices once they sync. Where it was placed, it stays."
                        )),
                )
                .child(buttons(&tr!("Delete"), cx)),
        })
    }
}

fn dialog_title(s: &str) -> gpui::Div {
    div()
        .text_size(ui(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(s.to_string())
}

fn window_focus(f: &FocusHandle, window: &mut Window, cx: &mut Context<SnippetsView>) {
    window.focus(f, cx);
}

impl Render for SnippetsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let rows = match self.shown {
            Shown::Snippets => self.rows(cx),
            Shown::Environments => self.env_rows(cx),
            Shown::Files => self.file_rows(cx),
            Shown::Notes => self.note_rows(cx),
        };
        let chord = crate::hotkeys::hint(cx, shellwing_config::Hotkey::Palette);
        let alt = if cfg!(target_os = "macos") {
            "Option+Enter"
        } else {
            "Alt+Enter"
        };
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().line));
        let library_card = rows.is_empty() && self.shown == Shown::Snippets;
        if rows.is_empty() && !library_card {
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
                            .child(match self.shown {
                                Shown::Snippets => tr!("Snippets"),
                                Shown::Environments => tr!("Environments"),
                                Shown::Files => tr!("Config files"),
                                Shown::Notes => tr!("Notes"),
                            }),
                    )
                    .when(self.shown == Shown::Snippets && !library_card, |d| {
                        d.child(
                            outline_button("snippet-library", &tr!("Library…"))
                                .on_click(cx.listener(|v, _, _, cx| v.open_library(cx))),
                        )
                    })
                    .child(match self.shown {
                        Shown::Snippets => primary_button("snippet-new", &tr!("New snippet…"))
                            .on_click(cx.listener(|v, _, w, cx| v.edit(None, w, cx))),
                        Shown::Environments => primary_button("env-new", &tr!("New environment…"))
                            .on_click(cx.listener(|v, _, w, cx| v.edit_env(None, w, cx))),
                        Shown::Files => primary_button("file-new", &tr!("New config file…"))
                            .on_click(cx.listener(|v, _, w, cx| v.edit_file(None, w, cx))),
                        Shown::Notes => primary_button("note-new", &tr!("New note…"))
                            .on_click(cx.listener(|v, _, _, cx| v.open_note(None, cx))),
                    }),
            )
            .child(div().mt(px(-6.)).text_size(ui(12.5)).text_color(rgb(colors().label)).child(match self.shown {
                Shown::Environments => tr!(
                    "Variables for the shell, such as an API's keys, kept encrypted in the vault and synced with it. Load one into a pane from the palette ({chord}, under Environments) or the pane's menu, and unload it when done; or let a snippet that needs it have it for itself alone.",
                    chord = chord
                ),
                Shown::Snippets => tr!(
                    "Commands you run often. Run one from the palette ({chord}, then type its name): Enter runs it in the pane, {alt} in every pane of the tab. Kept in the vault, so they sync with your hosts.",
                    chord = chord,
                    alt = alt
                ),
                Shown::Notes => tr!(
                    "Free text, kept encrypted in the vault and synced with it: what a server is for, how to restart it, codes to keep. Each opens in a window of its own, from here or the palette ({chord}, under Notes); plain text or Markdown, saved as you type.",
                    chord = chord
                ),
                Shown::Files => tr!(
                    "Files to place on hosts, such as ~/.kube/config or ~/.aws/credentials, kept encrypted in the vault and synced with it. Place one from the palette ({chord}, under Config files) or the pane's menu: until the tab closes, or to stay.",
                    chord = chord
                ),
            }));
        if let Some((msg, bad)) = &self.notice {
            page = page.child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(if *bad { colors().bad } else { colors().good }))
                    .child(msg.clone()),
            );
        }
        page = if library_card {
            page.child(self.library_card(cx))
        } else {
            page.child(list)
        };
        if self.shown == Shown::Snippets && !library_card {
            let had: std::collections::HashSet<&str> =
                self.list.iter().map(|s| s.origin.as_str()).collect();
            let left = crate::library::items()
                .iter()
                .filter(|i| !had.contains(i.origin().as_str()))
                .count();
            if left > 0 {
                page = page.child(
                    div()
                        .id("snippet-library-more")
                        .mt(px(-8.))
                        .text_size(ui(12.))
                        .text_color(rgb(colors().accent))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(colors().accent_hover)))
                        .on_click(cx.listener(|v, _, _, cx| v.open_library(cx)))
                        .child(tr_n!(
                            left,
                            "More in the library: {n} not added yet ›",
                            "More in the library: {n} not added yet ›"
                        )),
                );
            }
        }
        page = if self.shown == Shown::Environments {
            page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(rgb(colors().warn_bg))
                    .text_size(ui(12.))
                    .text_color(rgb(colors().label))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(rgb(colors().warn)).child(tr!("While loaded, the server can read them")))
                    .child(tr!("Values are sent with the echo off and kept out of the shell's history, but a shell that has them holds them in its memory: whoever has root on that server, or runs as your user there, can read them until they are unloaded or the shell ends."))
                    .child(tr!("Load them on servers you trust, unload them when done, and prefer keys that expire or can do little, such as short-lived tokens or a role with only what the work needs.")),
            )
        } else if self.shown == Shown::Notes {
            page
        } else if self.shown == Shown::Files {
            page.child(
                div()
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(tr!("A placed file is on the server's disk, with its permissions: whoever has root there can read it until it is removed. Place secrets on servers you trust, until the tab closes.")),
            )
        } else {
            page.child(
                div()
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(tr!("{{host}}, {{user}} and {{address}} are filled in for the pane's host; any other {{name}} is asked for when the snippet runs.")),
            )
        };
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

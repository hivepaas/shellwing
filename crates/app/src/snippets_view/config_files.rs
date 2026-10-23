//! The Config files page: files kept in the vault to place on hosts
//! (`~/.kube/config`, `~/.aws/credentials`), and the dialog that edits
//! one. A content kept in the vault shows masked; showing it asks the user
//! first, as an environment's values do.

use std::path::PathBuf;

use super::confirm::Then;
use super::*;
use gpui::{PathPromptOptions, svg};
use shellwing_vault::{ConfigFile, is_text, limits};
use zeroize::Zeroizing;

/// The largest text edited here; past it, a file is replaced whole, from
/// a file on this computer (the field draws all of its text each frame).
const EDITED: usize = 64 * 1024;

/// What the dialog holds of the file's content.
pub(super) enum Content {
    /// The one in the vault, masked and not loaded: saving keeps it.
    Kept { size: usize, binary: bool },
    /// Text, being edited.
    Text(Entity<TextInput>),
    /// Bytes from a file on this computer, to keep as they are: not text,
    /// or too long to edit here.
    Bytes {
        bytes: Zeroizing<Vec<u8>>,
        from: String,
    },
}

pub(super) struct FileEdit {
    id: String,
    name: Entity<TextInput>,
    category: Entity<TextInput>,
    path: Entity<TextInput>,
    mode: Entity<TextInput>,
    pub(super) scope: Scope,
    pub(super) pick: Option<Pick>,
    content: Content,
    /// Placed to stay, rather than until the tab closes, unless the user
    /// chooses otherwise when placing it.
    stays: bool,
    problem: Option<String>,
}

/// `n` bytes, said short.
pub fn size_said(n: usize) -> String {
    const MB: usize = 1024 * 1024;
    if n < 1024 {
        tr_n!(n, "{n} byte", "{n} bytes")
    } else if n >= MB && n.is_multiple_of(MB) {
        tr!("{n} MB", n = n / MB)
    } else {
        tr!("{n} KB", n = n.div_ceil(1024))
    }
}

impl SnippetsView {
    fn file_edit(&mut self) -> Option<&mut FileEdit> {
        match &mut self.dialog {
            Some(Dialog::EditFile(f)) => Some(f),
            _ => None,
        }
    }

    fn text_area(text: &str, cx: &mut Context<Self>) -> Entity<TextInput> {
        cx.new(|cx| {
            let mut i = TextInput::said(|| tr!("The file's text"), cx).multiline(4, 14);
            i.set_text(text, 0..0, cx);
            i
        })
    }

    pub(super) fn edit_file(
        &mut self,
        f: Option<ConfigFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = f.is_none();
        let f = f.unwrap_or(ConfigFile {
            id: String::new(),
            name: String::new(),
            category: String::new(),
            path: String::new(),
            mode: 0o600,
            scope: String::new(),
            size: 0,
            binary: false,
            stays: false,
        });
        let name = Self::input(|| tr!("What it is, such as kube prod"), &f.name, cx);
        let category = Self::input(|| tr!("None"), &f.category, cx);
        let path = Self::input(|| "~/.kube/config".to_string(), &f.path, cx);
        let mode = Self::input(|| "600".to_string(), &format!("{:o}", f.mode), cx);
        let content = if new {
            Content::Text(Self::text_area("", cx))
        } else {
            Content::Kept {
                size: f.size,
                binary: f.binary,
            }
        };
        self.dialog = Some(Dialog::EditFile(Box::new(FileEdit {
            id: f.id,
            name: name.clone(),
            category,
            path,
            mode,
            scope: Scope::parse(&f.scope),
            pick: None,
            content,
            stays: f.stays,
            problem: None,
        })));
        let first = name.read(cx).focus_handle(cx);
        window.focus(&first, cx);
        cx.notify();
    }

    /// Enter in the dialog: answers the question to show the content, if
    /// it is being asked, else saves.
    pub(super) fn file_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm.is_some() {
            return self.confirm_answer(cx);
        }
        self.save_file(window, cx);
    }

    /// Confirmed: the content from the vault, as text to edit when it is
    /// text short enough.
    pub(super) fn show_file_content(&mut self, cx: &mut Context<Self>) {
        let vault = self.vault.clone();
        let Some(f) = self.file_edit() else {
            return;
        };
        let content = match vault.ready().map(|v| v.config_file_content(&f.id)) {
            Some(Ok(Some(c))) => c,
            Some(Err(e)) => {
                f.problem = Some(e.to_string());
                return cx.notify();
            }
            _ => Zeroizing::new(vec![]),
        };
        if is_text(&content) && content.len() <= EDITED {
            let text = String::from_utf8_lossy(&content).into_owned();
            let area = Self::text_area(&text, cx);
            if let Some(f) = self.file_edit() {
                f.content = Content::Text(area);
            }
        } else if let Some(f) = self.file_edit() {
            f.problem = Some(tr!(
                "It is not text, or longer than {max} KB: replace it from a file instead.",
                max = EDITED / 1024
            ));
        }
        cx.notify();
    }

    /// A file from this computer for the content: as text to edit when it
    /// is text short enough, else as it is. Names the file, and where it
    /// goes, when they are not set yet.
    fn choose_local_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { read_local(&path) }
                })
                .await;
            let _ = this.update(cx, |v, cx| v.took_local_file(path, read, cx));
        })
        .detach();
    }

    fn took_local_file(
        &mut self,
        path: PathBuf,
        read: Result<Zeroizing<Vec<u8>>, String>,
        cx: &mut Context<Self>,
    ) {
        let bytes = match read {
            Ok(b) => b,
            Err(e) => {
                if let Some(f) = self.file_edit() {
                    f.problem = Some(e);
                }
                return cx.notify();
            }
        };
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Under this computer's home: the same place under the host's.
        let target = dirs_home()
            .and_then(|h| path.strip_prefix(h).ok().map(|p| p.to_path_buf()))
            .map(|rel| format!("~/{}", rel.to_string_lossy().replace('\\', "/")));
        let content = if is_text(&bytes) && bytes.len() <= EDITED {
            Content::Text(Self::text_area(&String::from_utf8_lossy(&bytes), cx))
        } else {
            Content::Bytes {
                bytes,
                from: file_name.clone(),
            }
        };
        let Some(f) = self.file_edit() else {
            return;
        };
        f.content = content;
        f.problem = None;
        let (name, at) = (f.name.clone(), f.path.clone());
        if name.read(cx).text().trim().is_empty() {
            name.update(cx, |i, cx| i.set_text(&file_name, 0..0, cx));
        }
        if at.read(cx).text().trim().is_empty() {
            let t = target.unwrap_or_else(|| format!("~/{file_name}"));
            at.update(cx, |i, cx| i.set_text(&t, 0..0, cx));
        }
        cx.notify();
    }

    pub(super) fn save_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.file_edit() else {
            return;
        };
        let text = |i: &Entity<TextInput>| i.read(cx).text().trim().to_string();
        let (name, category, path, mode) = (
            text(&f.name),
            text(&f.category),
            text(&f.path),
            text(&f.mode),
        );
        let mode = u32::from_str_radix(&mode, 8).ok().filter(|m| *m <= 0o777);
        let said = if name.is_empty() {
            Some(tr!("A name is needed."))
        } else if !(path.starts_with('/') || path == "~" || path.starts_with("~/")) {
            Some(tr!(
                "Where it goes: a path from / or from ~/, such as ~/.kube/config."
            ))
        } else if path.ends_with('/') || path == "~" {
            Some(tr!("Where it goes: a file's path, not a folder's."))
        } else if mode.is_none() {
            Some(tr!("Permissions: three octal digits, such as 600."))
        } else {
            None
        };
        if let Some(said) = said {
            f.problem = Some(said);
            return cx.notify();
        }
        let content: Option<Zeroizing<Vec<u8>>> = match &f.content {
            Content::Kept { .. } => None,
            Content::Text(i) => Some(Zeroizing::new(i.read(cx).text().as_bytes().to_vec())),
            Content::Bytes { bytes, .. } => Some(bytes.clone()),
        };
        let meta = ConfigFile {
            id: f.id.clone(),
            name,
            category,
            path,
            mode: mode.unwrap_or(0o600),
            scope: f.scope.field(),
            size: 0,
            binary: false,
            stays: f.stays,
        };
        let saved = self
            .vault
            .get()
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| {
                v.save_config_file(&meta, content.as_deref().map(|c| &c[..]))
                    .map_err(|e| crate::environment::said_error(&e))
            });
        match saved {
            Ok(_) => {
                self.close(window, cx);
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(err) => {
                if let Some(f) = self.file_edit() {
                    f.problem = Some(tr!("Not saved: {e}", e = err));
                }
                cx.notify();
            }
        }
    }

    pub(super) fn delete_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::DeleteFile { file }) = &self.dialog else {
            return;
        };
        let (id, name) = (file.id.clone(), file.name.clone());
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

    pub(super) fn file_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let filed = self.files.iter().any(|f| !f.category.trim().is_empty());
        let mut last: Option<String> = None;
        let mut out = vec![];
        for (i, f) in self.files.iter().enumerate() {
            let cat = f.category.trim().to_lowercase();
            let headed = filed && last.as_ref() != Some(&cat);
            if headed {
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
                            f.category.trim().to_string()
                        })
                        .into_any_element(),
                );
            }
            last = Some(cat);
            let scope = Scope::parse(&f.scope);
            let (edit, del) = (f.clone(), f.clone());
            let detail = if f.binary {
                tr!("{size}, not text", size = size_said(f.size))
            } else {
                size_said(f.size)
            };
            out.push(
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
                                    .child(
                                        div().font_weight(FontWeight::MEDIUM).child(f.name.clone()),
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
                                    .child(
                                        div()
                                            .text_size(ui(11.))
                                            .text_color(rgb(colors().muted))
                                            .child(detail),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(self.mono.clone())
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().soft))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(format!("{}  {:o}", f.path, f.mode)),
                            ),
                    )
                    .child(Self::small(("file-edit", i), &tr!("Edit…")).on_click(
                        cx.listener(move |v, _, w, cx| v.edit_file(Some(edit.clone()), w, cx)),
                    ))
                    .child(
                        Self::small(("file-delete", i), &tr!("Delete"))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                v.dialog = Some(Dialog::DeleteFile { file: del.clone() });
                                window_focus(&v.focus, w, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// The content: masked with an eye while kept, the text being edited,
    /// or the file taken from this computer.
    fn content_box(&self, f: &FileEdit, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let choose = Self::small("file-choose", &tr!("Choose a file…"))
            .on_click(cx.listener(|v, _, _, cx| v.choose_local_file(cx)));
        let line = match &f.content {
            Content::Kept { size, binary } => {
                let shown = if *binary {
                    tr!("{size}, not text", size = size_said(*size))
                } else {
                    size_said(*size)
                };
                Self::field_shell(false, None)
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(colors().muted))
                            .child(format!("••••••••••••   {shown}")),
                    )
                    .when(!*binary && *size <= EDITED, |d| {
                        d.child(
                            div()
                                .id("file-eye")
                                .flex_shrink_0()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(colors().hover)))
                                .on_click(cx.listener(|v, _, w, cx| {
                                    v.ask_confirm(Then::FileContent, w, cx)
                                }))
                                .child(
                                    svg()
                                        .path("icons/eye.svg")
                                        .size(px(14.))
                                        .text_color(rgb(colors().muted)),
                                ),
                        )
                    })
            }
            Content::Text(input) => {
                let on = input.read(cx).focus_handle(cx).is_focused(window);
                Self::field_box(input, on, Some(self.mono.clone()))
            }
            Content::Bytes { bytes, from } => Self::field_shell(false, None).child(
                div().flex_1().text_color(rgb(colors().soft)).child(tr!(
                    "{file}: {size}, kept as it is",
                    file = from.as_str(),
                    size = size_said(bytes.len())
                )),
            ),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(label_text(&tr!("Content")))
            .child(line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(choose)
                    // Wraps beside the button, within the dialog.
                    .child(
                        hint_text(&tr!(
                            "Text up to {edited} is edited here; any file up to {max} can be taken from this computer.",
                            edited = size_said(EDITED),
                            max = size_said(limits::FILE)
                        ))
                        .flex_1()
                        .min_w_0(),
                    ),
            )
    }

    /// How placing it goes first: until the tab closes, or to stay.
    fn placing_box(&self, stays: bool, cx: &mut Context<Self>) -> gpui::Div {
        let mut pills = div().flex().gap(px(4.));
        let choices = [(false, tr!("Until the tab closes")), (true, tr!("To stay"))];
        for (n, (value, label)) in choices.into_iter().enumerate() {
            let on = value == stays;
            pills = pills.child(
                div()
                    .id(("file-placing", n))
                    .debug_selector(move || format!("file-placing:{n}"))
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if on {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(if on { colors().selected } else { colors().page }))
                    .text_size(ui(12.5))
                    .text_color(rgb(if on { colors().text } else { colors().soft }))
                    .cursor_pointer()
                    .when(!on, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if let Some(f) = v.file_edit() {
                            f.stays = value;
                        }
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(label_text(&tr!("When placed")))
            .child(pills)
            .child(hint_text(&tr!(
                "What placing it offers first, for Enter: taken away when the tab closes, or left on the host."
            )))
    }

    pub(super) fn file_card(
        &self,
        f: &FileEdit,
        card: gpui::Stateful<gpui::Div>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let scope_box = self.scope_box(&f.scope, &f.pick, cx).child(hint_text(&tr!(
            "The palette offers to place it in panes of those hosts only."
        )));
        let focused = |i: &Entity<TextInput>| i.read(cx).focus_handle(cx).is_focused(window);
        let (name_on, cat_on, path_on, mode_on) = (
            focused(&f.name),
            focused(&f.category),
            focused(&f.path),
            focused(&f.mode),
        );
        let place = div()
            .flex()
            .gap(px(10.))
            .child(
                Self::field(
                    &tr!("Where it goes on the host"),
                    &f.path,
                    path_on,
                    Some(self.mono.clone()),
                )
                .flex_1(),
            )
            .child(
                Self::field(
                    &tr!("Permissions"),
                    &f.mode,
                    mode_on,
                    Some(self.mono.clone()),
                )
                .w(px(110.)),
            );
        let content = self.content_box(f, window, cx);
        let when = self.placing_box(f.stays, cx);
        let mut b = card
            .child(dialog_title(&if f.id.is_empty() {
                tr!("New config file")
            } else {
                tr!("Edit config file")
            }))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(Self::field(&tr!("Name"), &f.name, name_on, None).flex_1())
                    .child(Self::field(&tr!("Category"), &f.category, cat_on, None).w(px(200.))),
            )
            .child(place)
            .child(when)
            .child(content);
        if let Some(line) = self.confirm_line(window, cx) {
            b = b.child(line);
        }
        b = b.child(scope_box);
        if let Some(p) = &f.problem {
            b = b.child(problem_text(p));
        }
        b.child(Self::buttons(&tr!("Save"), cx))
    }
}

/// This computer's home directory.
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// A file on this computer, up to the most a config file holds.
fn read_local(path: &std::path::Path) -> Result<Zeroizing<Vec<u8>>, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len() as usize;
    if size > limits::FILE {
        let o = shellwing_vault::Oversize::File(size);
        return Err(crate::environment::said_oversize(&o));
    }
    std::fs::read(path)
        .map(Zeroizing::new)
        .map_err(|e| e.to_string())
}

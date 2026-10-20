//! The Notes page: the notes, by category; each opens in its window.

use super::*;
use shellwing_vault::{Note, NoteFormat};

impl SnippetsView {
    /// Opens `note` (None: a new one) in its window.
    pub(super) fn open_note(&mut self, note: Option<Note>, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, cx| ws.open_note(note, cx));
        }
    }

    pub(super) fn delete_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::DeleteNote { note }) = &self.dialog else {
            return;
        };
        let (id, name) = (note.id.clone(), note.title.clone());
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

    pub(super) fn note_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let filed = self.notes.iter().any(|n| !n.category.trim().is_empty());
        let mut last: Option<String> = None;
        let mut out = vec![];
        for (i, n) in self.notes.iter().enumerate() {
            let cat = n.category.trim().to_lowercase();
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
                            n.category.trim().to_string()
                        })
                        .into_any_element(),
                );
            }
            last = Some(cat);
            let (open, del) = (n.clone(), n.clone());
            let mut detail = vec![if n.format == NoteFormat::Markdown {
                "Markdown".to_string()
            } else {
                tr!("Plain text")
            }];
            detail.push(super::size_said(n.size));
            if !n.hosts.is_empty() {
                detail.push(n.hosts.join(", "));
            }
            let title = if n.title.is_empty() {
                tr!("Untitled note")
            } else {
                n.title.clone()
            };
            out.push(
                div()
                    .id(("note-row", i))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .when(i > 0 && !headed, |d| {
                        d.border_t_1().border_color(rgb(colors().line))
                    })
                    .on_click(cx.listener({
                        let n = n.clone();
                        move |v, _, _, cx| v.open_note(Some(n.clone()), cx)
                    }))
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
                                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                                    .when(n.sensitive, |d| {
                                        d.child(
                                            div()
                                                .text_size(ui(11.))
                                                .text_color(rgb(colors().warn))
                                                .child(tr!("sensitive")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().soft))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(detail.join(" · ")),
                            ),
                    )
                    .child(
                        Self::small(("note-open", i), &tr!("Open")).on_click(cx.listener(
                            move |v, _, _, cx| {
                                cx.stop_propagation();
                                v.open_note(Some(open.clone()), cx)
                            },
                        )),
                    )
                    .child(
                        Self::small(("note-delete", i), &tr!("Delete"))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                cx.stop_propagation();
                                v.dialog = Some(Dialog::DeleteNote { note: del.clone() });
                                window_focus(&v.focus, w, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        out
    }
}

//! The group editor: the host editor's drawer for a `[[group]]` in hosts.toml:
//! its name, its color, and the theme its hosts use unless they set their own.
//! Renaming a group takes its hosts along; deleting one keeps its hosts,
//! without a group.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, Render,
    SharedString, Window, div, prelude::*, px, rgb,
};
use shellwing_config::{Group, HOST_COLORS, Hosts, Store};
use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::host_editor::{
    hint_text, label_text, outline_button, primary_button, problem_text, section, text_button,
};
use crate::input::TextInput;
use crate::look::ui;

pub enum GroupEvent {
    Close,
    Saved { name: String },
    Deleted { name: String },
}

pub struct GroupEditor {
    focus: FocusHandle,
    store: Option<Store>,
    hosts: Hosts,
    /// Theme ids and names.
    themes: Vec<(String, String)>,
    /// The group as opened; None for a new one.
    original: Option<Group>,
    name: Entity<TextInput>,
    color: Option<String>,
    theme: Option<String>,
    themes_open: bool,
    problem: Option<String>,
    confirm_delete: bool,
}

impl EventEmitter<GroupEvent> for GroupEditor {}

impl GroupEditor {
    pub fn new(
        store: Option<Store>,
        hosts: Hosts,
        themes: Vec<(String, String)>,
        group: Option<Group>,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = crate::input::said(cx, || tr!("Production"));
        if let Some(g) = &group {
            let n = g.name.clone();
            name.update(cx, |i, cx| i.set_text(&n, n.len()..n.len(), cx));
        }
        GroupEditor {
            focus: cx.focus_handle(),
            store,
            hosts,
            themes,
            color: group.as_ref().and_then(|g| g.color.clone()),
            theme: group.as_ref().and_then(|g| g.theme.clone()),
            original: group,
            name,
            themes_open: false,
            problem: None,
            confirm_delete: false,
        }
    }

    /// Clicking beside the drawer closes it only when nothing would be lost.
    pub fn can_close_quietly(&self, cx: &gpui::App) -> bool {
        let o = self.original.as_ref();
        self.typed(cx) == o.map(|g| g.name.clone()).unwrap_or_default()
            && self.color == o.and_then(|g| g.color.clone())
            && self.theme == o.and_then(|g| g.theme.clone())
    }

    pub fn first_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.name.focus_handle(cx)
    }

    fn typed(&self, cx: &gpui::App) -> String {
        self.name.read(cx).text().trim().to_string()
    }

    /// The hosts in the group as opened.
    fn members(&self) -> Vec<&str> {
        let Some(g) = &self.original else {
            return vec![];
        };
        self.hosts
            .hosts
            .iter()
            .filter(|h| h.group.as_deref() == Some(g.name.as_str()))
            .map(|h| h.name.as_str())
            .collect()
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = false;
        let name = self.typed(cx);
        let mine = |g: &Group| match &self.original {
            Some(o) if o.id.is_some() => o.id == g.id,
            Some(o) => o.name == g.name,
            None => false,
        };
        self.problem = if name.is_empty() {
            Some(tr!("A name is needed."))
        } else if self.hosts.groups.iter().any(|g| !mine(g) && g.name == name) {
            Some(tr!("Another group has this name."))
        } else {
            None
        };
        if self.problem.is_some() {
            return cx.notify();
        }
        let Some(store) = &self.store else {
            self.problem = Some(tr!("There is no config directory to save in."));
            return cx.notify();
        };
        let group = Group {
            id: self.original.as_ref().and_then(|o| o.id.clone()),
            name: name.clone(),
            color: self.color.clone(),
            theme: self.theme.clone(),
        };
        let old = self.original.as_ref().map(|o| o.name.as_str());
        match store.save_group(&group, old) {
            Ok(_) => cx.emit(GroupEvent::Saved { name }),
            Err(e) => {
                self.problem = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.original.clone() else {
            return;
        };
        if !self.confirm_delete {
            self.confirm_delete = true;
            return cx.notify();
        }
        let result = match (&self.store, &g.id) {
            (Some(store), Some(id)) => store.delete_group(id, &g.name).map_err(|e| e.to_string()),
            _ => Err(tr!("This group cannot be found in hosts.toml.")),
        };
        match result {
            Ok(()) => cx.emit(GroupEvent::Deleted { name: g.name }),
            Err(e) => {
                self.confirm_delete = false;
                self.problem = Some(e);
                cx.notify();
            }
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" if self.themes_open => self.themes_open = false,
            "escape" if self.confirm_delete => self.confirm_delete = false,
            "escape" => cx.emit(GroupEvent::Close),
            "enter" => self.save(cx),
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }
}

impl Focusable for GroupEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GroupEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.typed(cx);
        let title = if typed.is_empty() {
            tr!("New group")
        } else {
            typed
        };
        let dot = self
            .color
            .as_deref()
            .and_then(shellwing_config::color_rgb)
            .unwrap_or(colors().accent);
        let header = div()
            .h(px(56.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(20.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(div().size(px(10.)).rounded(px(3.)).bg(rgb(dot)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(ui(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.clone()),
            )
            .child(
                div()
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(if self.original.is_some() {
                        tr!("Group in hosts.toml")
                    } else {
                        tr!("Not saved yet")
                    }),
            )
            .child(
                div()
                    .id("group-close")
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(rgb(colors().soft))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GroupEvent::Close)))
                    .child("✕"),
            );

        let focused = self.name.focus_handle(cx).is_focused(window);
        let name_box = div()
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(if self.problem.is_some() {
                colors().bad
            } else if focused {
                colors().accent
            } else {
                colors().field_edge
            }))
            .bg(rgb(colors().field))
            .text_size(ui(13.))
            .child(self.name.clone());

        let mut swatches = div().flex().items_center().gap(px(8.)).child(
            div()
                .id("group-color-none")
                .size(px(20.))
                .rounded_full()
                .border_2()
                .border_color(rgb(if self.color.is_none() {
                    colors().text
                } else {
                    colors().edge
                }))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui(10.))
                .text_color(rgb(colors().muted))
                .on_click(cx.listener(|e, _, _, cx| {
                    e.color = None;
                    cx.notify();
                }))
                .child("–"),
        );
        for (name, value) in HOST_COLORS {
            let on = self
                .color
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(name));
            swatches = swatches.child(
                div()
                    .id(SharedString::from(format!("group-color-{name}")))
                    .size(px(20.))
                    .rounded_full()
                    .bg(rgb(value))
                    .when(on, |d| d.border_2().border_color(rgb(colors().text)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |e, _, _, cx| {
                        e.color = Some(name.to_string());
                        cx.notify();
                    })),
            );
        }

        let theme_label = self
            .theme
            .as_ref()
            .map(|t| {
                self.themes
                    .iter()
                    .find(|(id, _)| id == t)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| t.clone())
            })
            .unwrap_or_else(|| tr!("Same as the app"));
        let mut theme = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(label_text(&tr!("Theme for its hosts")))
            .child(
                div()
                    .id("group-theme")
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if self.themes_open {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().field))
                    .text_size(ui(13.))
                    .cursor_pointer()
                    .on_click(cx.listener(|e, _, _, cx| {
                        e.themes_open = !e.themes_open;
                        cx.notify();
                    }))
                    .child(theme_label)
                    .child(div().text_color(rgb(colors().muted)).child("▾")),
            );
        if self.themes_open {
            let mut list = div()
                .id("group-themes")
                .flex()
                .flex_col()
                .max_h(px(220.))
                .overflow_y_scroll()
                .p_1()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(colors().edge))
                .bg(rgb(colors().raised));
            let mut choices = vec![(None, tr!("Same as the app"))];
            choices.extend(
                self.themes
                    .iter()
                    .map(|(id, n)| (Some(id.clone()), n.clone())),
            );
            for (n, (value, name)) in choices.into_iter().enumerate() {
                let on = value == self.theme;
                list = list.child(
                    div()
                        .id(("group-theme-choice", n))
                        .px_2()
                        .py(px(5.))
                        .rounded(px(4.))
                        .text_size(ui(13.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(rgb(colors().selected)))
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |e, _, _, cx| {
                            e.theme = value.clone();
                            e.themes_open = false;
                            cx.notify();
                        }))
                        .child(name),
                );
            }
            theme = theme.child(list);
        }

        let members = self.members();
        let who = match members.len() {
            0 if self.original.is_some() => {
                tr!("No hosts yet. Choose this group in a host's editor.")
            }
            0 => tr!("Choose the group in a host's editor to put the host in it."),
            n => tr_n!(
                n,
                "{n} host: {names}",
                "{n} hosts: {names}",
                names = members.join(", ")
            ),
        };

        let body = section(&tr!("Group"))
            .border_b_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(&tr!("Name")))
                    .child(name_box)
                    .when_some(self.problem.clone(), |d, p| d.child(problem_text(&p))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().mr(px(6.)).child(label_text(&tr!("Color"))))
                    .child(swatches),
            )
            .child(theme)
            .child(hint_text(&tr!("A host's own theme wins over its group's.")))
            .child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(colors().label))
                    .child(who),
            );

        let mut buttons = div().flex().items_center().gap_2();
        if self.confirm_delete {
            let n = members.len();
            buttons = buttons
                .child(div().text_size(ui(12.5)).child(if n == 0 {
                    tr!("Delete {title}?", title = title.as_str())
                } else {
                    tr_n!(
                        n,
                        "Delete {title}? Its {n} host stay.",
                        "Delete {title}? Its {n} hosts stay.",
                        title = title.as_str()
                    )
                }))
                .child(
                    text_button("group-delete-yes", &tr!("Delete"), colors().bad)
                        .on_click(cx.listener(|e, _, _, cx| e.delete(cx))),
                )
                .child(
                    text_button("group-delete-no", &tr!("Cancel"), colors().label).on_click(
                        cx.listener(|e, _, _, cx| {
                            e.confirm_delete = false;
                            cx.notify();
                        }),
                    ),
                );
        } else if self.original.is_some() {
            buttons = buttons.child(
                text_button("group-delete", &tr!("Delete"), colors().bad)
                    .on_click(cx.listener(|e, _, _, cx| e.delete(cx))),
            );
        }
        buttons = buttons
            .child(div().flex_1())
            .child(
                outline_button("group-cancel", &tr!("Cancel"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GroupEvent::Close))),
            )
            .child(
                primary_button("group-save", &tr!("Save"))
                    .on_click(cx.listener(|e, _, _, cx| e.save(cx))),
            );

        // At the window's right edge: its bottom corner is the window's.
        let corner = crate::chrome::BottomCorners::current(cx).right;
        div()
            .id("group-editor")
            .rounded_br(corner)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors().surface))
            .border_l_1()
            .border_color(rgb(colors().edge))
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .child(header)
            .child(
                div()
                    .id("group-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .rounded_br(corner)
                    .bg(rgb(colors().panel))
                    .child(buttons),
            )
    }
}

//! Importing hosts from other SSH clients (`docs/design/import.md`): the
//! drawer that lists the apps found on this computer, reads the one chosen
//! (or a file), shows what would be imported, and writes the hosts checked
//! into `hosts.toml` in one edit.

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, Render, Window, div,
    prelude::*, px, rgb,
};
use shellwing_config::{Group, Hosts, Store};
use shellwing_i18n::{tr, tr_n};
use shellwing_import::{Installed, LeftOut, Look, Place, Plan, State, Why};

use crate::colors::colors;
use crate::host_editor::{
    checkbox, hint_text, label_text, outline_button, primary_button, problem_text, section,
};
use crate::look::ui;

pub enum ImportEvent {
    Close,
    Imported(Imported),
}

/// What an import wrote: to say, and to undo.
#[derive(Clone, Debug)]
pub struct Imported {
    pub app: &'static str,
    /// The hosts' ids.
    pub hosts: Vec<String>,
    /// The groups made: their ids and names.
    pub groups: Vec<(String, String)>,
    /// Descriptions to keep as notes: the host's name, the text.
    pub notes: Vec<(String, String)>,
    pub left_out: usize,
}

enum Step {
    /// Looking for the apps installed here.
    Looking,
    Sources,
    Reading,
    Preview(Plan),
}

pub struct ImportView {
    focus: FocusHandle,
    store: Option<Store>,
    hosts: Hosts,
    step: Step,
    /// The apps found here, to come back to from a preview.
    found: Vec<Installed>,
    problem: Option<String>,
}

impl EventEmitter<ImportEvent> for ImportView {}

impl ImportView {
    pub fn new(store: Option<Store>, hosts: Hosts, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async {
                    shellwing_import::Dirs::here()
                        .map(|d| shellwing_import::installed(&d))
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.found = found;
                if matches!(v.step, Step::Looking) {
                    v.step = Step::Sources;
                }
                cx.notify();
            });
        })
        .detach();
        ImportView {
            focus: cx.focus_handle(),
            store,
            hosts,
            step: Step::Looking,
            found: vec![],
            problem: None,
        }
    }

    pub fn first_focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// Reads the app found here at `n`.
    fn choose(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(source) = self.found.get(n).cloned() else {
            return;
        };
        self.read(cx, move || {
            shellwing_import::read(source.app, &source.place).map(|r| (source.app, r))
        });
    }

    /// Asks for a file (or a Sessions folder), and reads it.
    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(tr!("Import").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |v, cx| {
                v.read(cx, move || shellwing_import::read_file(&path));
            });
        })
        .detach();
    }

    fn read(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<
            (shellwing_import::App, shellwing_import::Read),
            shellwing_import::Error,
        > + Send
        + 'static,
    ) {
        self.step = Step::Reading;
        self.problem = None;
        cx.notify();
        let here = self.hosts.clone();
        cx.spawn(async move |this, cx| {
            let planned =
                cx.background_executor()
                    .spawn(async move {
                        work().map(|(app, read)| shellwing_import::plan(app, read, &here))
                    })
                    .await;
            let _ = this.update(cx, |v, cx| {
                match planned {
                    Ok(plan) if plan.rows.is_empty() && plan.left_out.is_empty() => {
                        v.problem = Some(tr!("No hosts in it."));
                        v.step = Step::Sources;
                    }
                    Ok(plan) => v.step = Step::Preview(plan),
                    Err(e) => {
                        v.problem = Some(said_error(&e));
                        v.step = Step::Sources;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle(&mut self, n: usize, cx: &mut Context<Self>) {
        if let Step::Preview(plan) = &mut self.step
            && let Some(row) = plan.rows.get_mut(n)
        {
            row.checked = !row.checked;
            cx.notify();
        }
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let Step::Preview(plan) = &self.step else {
            return;
        };
        let hosts = plan.hosts(&self.hosts);
        if hosts.is_empty() {
            return;
        }
        let Some(store) = &self.store else {
            self.problem = Some(tr!("There is no config directory to save in."));
            return cx.notify();
        };
        let groups: Vec<Group> = plan
            .new_groups(&self.hosts)
            .into_iter()
            .map(|name| Group {
                id: None,
                name,
                color: None,
                theme: None,
            })
            .collect();
        let notes = plan
            .rows
            .iter()
            .filter(|r| r.checked)
            .filter_map(|r| Some((r.host.name.clone(), r.description.clone()?)))
            .collect();
        match store.import(&groups, &hosts) {
            Ok((group_ids, host_ids)) => cx.emit(ImportEvent::Imported(Imported {
                app: plan.app.name(),
                hosts: host_ids,
                groups: group_ids
                    .into_iter()
                    .zip(groups.into_iter().map(|g| g.name))
                    .collect(),
                notes,
                left_out: plan.left_out.len(),
            })),
            Err(e) => {
                self.problem = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.step = Step::Sources;
        self.problem = None;
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" => cx.emit(ImportEvent::Close),
            "enter" => self.import(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn sources(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut list = div().flex().flex_col().gap(px(6.));
        match self.step {
            Step::Looking => {
                list = list.child(hint_text(&tr!("Looking for other SSH apps…")));
            }
            _ if self.found.is_empty() => {
                list = list.child(hint_text(&tr!(
                    "No other SSH app with hosts was found here."
                )));
            }
            _ => {
                for (n, source) in self.found.iter().enumerate() {
                    list = list.child(
                        div()
                            .id(("import-source", n))
                            .debug_selector(|| format!("import-source:{}", source.app.name()))
                            .px(px(12.))
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(colors().field_edge))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click(cx.listener(move |v, _, _, cx| v.choose(n, cx)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(source.app.name()),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui(11.5))
                                            .text_color(rgb(colors().muted))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(place_text(&source.place)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(ui(12.))
                                    .text_color(rgb(colors().label))
                                    .child(tr_n!(source.hosts, "{n} host", "{n} hosts")),
                            ),
                    );
                }
            }
        }
        section(&tr!("Import hosts"))
            .border_b_0()
            .child(label_text(&tr!("Found on this computer")))
            .child(list)
            .child(label_text(&tr!("Or a file another app exported")))
            .child(
                div().flex().child(
                    outline_button("import-file", &tr!("Choose a file or folder…"))
                        .on_click(cx.listener(|v, _, _, cx| v.choose_file(cx))),
                ),
            )
            .child(hint_text(&tr!(
                "PuTTY or KiTTY .reg, MobaXterm .mxtsessions, SecureCRT's XML or Sessions folder, Xshell .xts or Sessions folder, Tabby's config.yaml, an ssh config, or CSV with a header."
            )))
            .child(hint_text(&tr!(
                "Passwords are not imported: a host asks for its own the first time, and offers to keep it in the vault."
            )))
            .when_some(self.problem.clone(), |d, p| d.child(problem_text(&p)))
    }

    fn preview(&self, plan: &Plan, cx: &mut Context<Self>) -> gpui::Div {
        let checked = plan.rows.iter().filter(|r| r.checked).count();
        let mut body = section(&tr!("Import from {app}", app = plan.app.name()))
            .border_b_0()
            .child(div().text_color(rgb(colors().label)).child(tr_n!(
                checked,
                "{n} host checked, of {total}.",
                "{n} hosts checked, of {total}.",
                total = plan.rows.len()
            )));
        // By group, in the order groups first come.
        let mut groups: Vec<Option<&str>> = vec![];
        for row in &plan.rows {
            let g = row.host.group.as_deref();
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        for group in groups {
            let mut list = div().flex().flex_col();
            for (n, row) in plan
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.host.group.as_deref() == group)
            {
                let h = &row.host;
                let mut place = h.address.clone();
                if let Some(u) = &h.user {
                    place = format!("{u}@{place}");
                }
                if let Some(p) = h.port {
                    place = format!("{place}:{p}");
                }
                let state = match &row.state {
                    State::New => None,
                    State::Renamed(was) => Some(tr!(
                        "{was} here is another host: named apart.",
                        was = was.as_str()
                    )),
                    State::Here(name) => Some(tr!("Already here as {name}.", name = name.as_str())),
                    State::ForJump => Some(tr!("Added: other hosts go through it.")),
                };
                let mut text = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(h.name.clone()),
                            )
                            .child(
                                div()
                                    .text_color(rgb(colors().muted))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(place),
                            ),
                    )
                    .when_some(state, |d, s| {
                        d.child(
                            div()
                                .text_size(ui(11.5))
                                .text_color(rgb(colors().label))
                                .child(s),
                        )
                    });
                for look in &row.look {
                    text = text.child(
                        div()
                            .text_size(ui(11.5))
                            .text_color(rgb(colors().warn))
                            .child(look_text(look, plan.app.name())),
                    );
                }
                list = list.child(
                    div()
                        .id(("import-row", n))
                        .debug_selector(|| format!("import-row:{}", h.name))
                        .px(px(6.))
                        .py(px(5.))
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| v.toggle(n, cx)))
                        .child(div().pt(px(2.)).child(checkbox(row.checked)))
                        .child(text),
                );
            }
            let title = match group {
                Some(g) => g.to_string(),
                None => tr!("No group"),
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(label_text(&title))
                    .child(list),
            );
        }
        if !plan.left_out.is_empty() {
            let mut list = div().flex().flex_col().gap(px(2.));
            for l in &plan.left_out {
                list = list.child(
                    div()
                        .text_size(ui(12.))
                        .text_color(rgb(colors().muted))
                        .child(left_out_text(l)),
                );
            }
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(&tr_n!(
                        plan.left_out.len(),
                        "{n} left out",
                        "{n} left out"
                    )))
                    .child(list),
            );
        }
        body.when_some(self.problem.clone(), |d, p| d.child(problem_text(&p)))
    }
}

/// Where an app keeps its sessions, as shown.
fn place_text(place: &Place) -> String {
    match place {
        Place::File(p) | Place::Folder(p) => p.display().to_string(),
        Place::Registry(key) => format!("HKEY_CURRENT_USER\\{key}"),
    }
}

fn said_error(e: &shellwing_import::Error) -> String {
    match e {
        shellwing_import::Error::Unknown => tr!("This is not a file hosts can be imported from."),
        shellwing_import::Error::Encrypted(app) => {
            tr!(
                "{app} keeps this file encrypted, so it cannot be read here.",
                app = *app
            )
        }
        shellwing_import::Error::Io(e) => tr!("Cannot read it: {e}", e = e.to_string()),
        shellwing_import::Error::Format(e) => tr!("Cannot read it: {e}", e = e.as_str()),
    }
}

fn look_text(look: &Look, app: &str) -> String {
    match look {
        Look::PpkKey => tr!(
            "Its key is PuTTY's .ppk: convert it with puttygen (OpenSSH format) to sign in with it."
        ),
        Look::KeyInApp(name) => tr!(
            "Its key {name} is in {app}'s own store: export it there, then add it under Keys.",
            name = name.as_str(),
            app = app
        ),
        Look::JumpNotFound(name) => tr!(
            "Its jump host {name} is not among the hosts: left out.",
            name = name.as_str()
        ),
    }
}

fn left_out_text(l: &LeftOut) -> String {
    match &l.why {
        Why::NotSsh(protocol) => tr!(
            "{name}: not SSH ({protocol})",
            name = l.name.as_str(),
            protocol = protocol.as_str()
        ),
        Why::NoAddress => tr!("{name}: no address", name = l.name.as_str()),
    }
}

/// For the UI tests: what a user would see, and a way in where they would
/// click.
#[cfg(all(test, feature = "ui-tests"))]
impl ImportView {
    /// The preview's rows: name, checked, state; None before a preview.
    pub fn rows(&self) -> Option<Vec<(String, bool, State)>> {
        match &self.step {
            Step::Preview(plan) => Some(
                plan.rows
                    .iter()
                    .map(|r| (r.host.name.clone(), r.checked, r.state.clone()))
                    .collect(),
            ),
            _ => None,
        }
    }

    pub fn click_choose_file(&mut self, cx: &mut Context<Self>) {
        self.choose_file(cx);
    }

    pub fn click_import(&mut self, cx: &mut Context<Self>) {
        self.import(cx);
    }
}

impl Focusable for ImportView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImportView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .h(px(56.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(20.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex_1()
                    .text_size(ui(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("Import hosts")),
            )
            .child(
                div()
                    .id("import-close")
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(rgb(colors().soft))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ImportEvent::Close)))
                    .child("✕"),
            );
        let (body, checked) = match &self.step {
            Step::Preview(plan) => (
                self.preview(plan, cx),
                Some(plan.rows.iter().filter(|r| r.checked).count()),
            ),
            Step::Reading => (
                section(&tr!("Import hosts"))
                    .border_b_0()
                    .child(hint_text(&tr!("Reading…"))),
                None,
            ),
            _ => (self.sources(cx), None),
        };
        let mut buttons = div().flex().items_center().gap_2();
        if let Some(n) = checked {
            buttons = buttons
                .child(
                    outline_button("import-back", &tr!("Back"))
                        .on_click(cx.listener(|v, _, _, cx| v.back(cx))),
                )
                .child(div().flex_1())
                .child(
                    outline_button("import-cancel", &tr!("Cancel"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ImportEvent::Close))),
                )
                .child(
                    primary_button(
                        "import-go",
                        &tr_n!(n, "Import {n} host", "Import {n} hosts"),
                    )
                    .when(n == 0, |b| b.opacity(0.5))
                    .on_click(cx.listener(|v, _, _, cx| v.import(cx))),
                );
        } else {
            buttons = buttons.child(div().flex_1()).child(
                outline_button("import-cancel", &tr!("Cancel"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ImportEvent::Close))),
            );
        }
        // At the window's right edge: its bottom corner is the window's.
        let corner = crate::chrome::BottomCorners::current(cx).right;
        div()
            .id("import-view")
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
                    .id("import-body")
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

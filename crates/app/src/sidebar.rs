//! The host list on the left. Its own view, drawn from a snapshot the workspace
//! hands it, so the terminal's frames do not lay it out again (it is cached).

use gpui::{
    App, Context, Div, ElementId, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, Render, SharedString, WeakEntity, Window, div, prelude::*, px,
    rgb, rgba, svg,
};

use shellwing_i18n::{tr, tr_n};

use crate::input::{Changed, TextInput};

use crate::colors::colors;
use crate::host_icons::{self, Face, HostIcon};
use crate::palette::Action;
use crate::session::Status;
use crate::settings_view::Page;
use crate::workspace::{EditTarget, Workspace};

/// The host list's width by default, and how far its edge can be dragged.
pub const WIDTH: f32 = 248.;
pub const MIN_WIDTH: f32 = 200.;
pub const MAX_WIDTH: f32 = 480.;
/// Folded to a rail of host tiles.
pub const RAIL_WIDTH: f32 = 56.;
/// A host's tile, in the list as on the rail.
pub const TILE: f32 = 36.;
const TILE_RADIUS: f32 = 10.;
/// What a tile shows, a system's mark or a theme's icon, inside it.
const TILE_ICON: f32 = 24.;
/// A host's row, from the list's edge: its margin, then its padding. Its
/// tile starts a little in from its section's title (12), at 24; its name,
/// and its address under it, follow the tile after a gap. The row is the
/// tile and a little above and below it.
const ROW_MARGIN: f32 = 4.;
const ROW_PAD: f32 = 20.;
const ROW_GAP: f32 = 10.;
const ROW_HEIGHT: f32 = TILE + 12.;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub title: SharedString,
    /// Beside the title: a count, or "read only".
    pub note: SharedString,
    /// The `[[group]]` it shows, which its Edit opens.
    pub group: Option<String>,
    /// The group's color, as a chip before the title.
    pub color: Option<u32>,
    /// Above the section, opening a part of the list: "My hosts", or the
    /// organization whose collections follow. Only once some are shared.
    pub heading: Option<SharedString>,
    /// An organization's collection: others see it, so it is marked.
    pub shared: Option<Shared>,
    pub rows: Vec<Row>,
}

/// What marks a collection's section apart from a group.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    /// The collection's id.
    pub id: String,
    /// What the person may do: "Manage", "Edit" or "Read only".
    pub permission: SharedString,
    /// The title's tooltip: who sees it, and how.
    pub tip: SharedString,
    /// What the person may do to it, from its title's menu: a right click,
    /// or its button on hover. Empty, it has none.
    pub menu: Vec<MenuItem>,
}

/// An item of a collection's menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    /// For the UI tests.
    pub id: &'static str,
    pub label: SharedString,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: SharedString,
    pub detail: SharedString,
    /// What opening it connects to.
    pub spec: String,
    /// The best status among its open tabs; None when none is open.
    pub status: Option<Status>,
    /// Where Edit takes it.
    pub edit: EditTarget,
    /// The host's color (its tile on the rail).
    pub color: Option<u32>,
    /// Its tile's icon: the one it names, else its system's once seen;
    /// None: its initials.
    pub icon: Option<&'static HostIcon>,
    /// The active tab is connected to it.
    pub active: bool,
    /// What the filter matches, lowercase: name, alias, address, user, tags.
    pub search: String,
    /// Given to an AI client, and not paused: its tile says so.
    pub ai: bool,
}

fn status_color(s: Status) -> u32 {
    let c = colors();
    match s {
        Status::Connecting => c.amber,
        Status::Connected => c.ok,
        Status::Closed => c.idle,
        Status::Lost => c.lost,
        Status::Failed => c.danger,
    }
}

/// A status dot, or an empty ring for a host with no tab.
pub fn dot(status: Option<Status>) -> Div {
    let d = div().size(px(7.)).flex_shrink_0().rounded_full();
    match status {
        Some(s) => d.bg(rgb(status_color(s))),
        None => d.border_1().border_color(rgb(colors().faint)),
    }
}

/// A host's tile, the same in the list and on the rail: the local terminal's
/// icon, the host's icon or else its initials, in its color if it has one; a
/// system's mark keeps its own color where that reads. Its status dot is at
/// the corner, ringed in `ground`, what the tile sits on. `ring` marks the
/// host of the active tab (the list marks its row instead).
fn tile(row: &Row, ground: u32, ring: bool) -> Div {
    let icon = match row.icon {
        _ if matches!(row.edit, EditTarget::Local) => host_icons::find("terminal"),
        icon => icon,
    };
    host_tile(icon, &row.name, row.color, row.status, TILE, ground, ring).when(row.ai, |d| {
        d.child(
            div()
                .debug_selector({
                    let n = row.name.clone();
                    move || format!("ai-badge:{n}")
                })
                .absolute()
                .top(px(-4.))
                .right(px(-6.))
                .h(px(15.))
                .px(px(4.))
                .flex()
                .items_center()
                .rounded_full()
                .border_2()
                .border_color(rgb(ground))
                .bg(rgb(AI_MARK))
                .text_color(rgb(colors().on_accent))
                .text_size(px(9.))
                .font_weight(FontWeight::BOLD)
                .child("AI"),
        )
    })
}

/// A host's tile `size` px square, as [`tile_face`] draws it, with its status
/// dot at the corner, ringed in `ground`: in the host list and on a tab.
pub fn host_tile(
    icon: Option<&'static HostIcon>,
    name: &str,
    color: Option<u32>,
    status: Option<Status>,
    size: f32,
    ground: u32,
    ring: bool,
) -> Div {
    dotted_tile(icon, name, color, Some(status), size, ground, ring)
}

/// The tile of what is not a host (Run on hosts, AI activity), `size` px
/// square: a dot at its corner only while `busy` says something, since a
/// page has no tab to be without.
pub fn page_tile(icon: &'static HostIcon, busy: Option<Status>, size: f32, ground: u32) -> Div {
    dotted_tile(Some(icon), "", None, busy.map(Some), size, ground, false)
}

/// A tile, its corner dot `dot` if there is one: a status, or (None) the
/// empty ring of a host with no tab.
fn dotted_tile(
    icon: Option<&'static HostIcon>,
    name: &str,
    color: Option<u32>,
    dot_of: Option<Option<Status>>,
    size: f32,
    ground: u32,
    ring: bool,
) -> Div {
    // The dot is a sibling after the face: a border is drawn over an
    // element's own children.
    div()
        .relative()
        .flex_shrink_0()
        .size(px(size))
        .child(tile_face_sized(icon, name, color, ring, size))
        .when_some(dot_of, |d, status| {
            d.child(
                div().absolute().right(px(-3.)).bottom(px(-3.)).child(
                    div()
                        .p(px(2.))
                        .rounded_full()
                        .bg(rgb(ground))
                        .child(dot(status)),
                ),
            )
        })
}

/// What a tile shows, filling its box: `icon`, or else the initials of
/// `name`, on the tile's ground in `color` if there is one.
pub fn tile_face(
    icon: Option<&'static HostIcon>,
    name: &str,
    color: Option<u32>,
    ring: bool,
) -> Div {
    tile_face_sized(icon, name, color, ring, TILE)
}

/// A tile's face for a box `size` px square: its corners, its icon and its
/// initials in proportion to the list's, the initials never below 9.5 px.
pub fn tile_face_sized(
    icon: Option<&'static HostIcon>,
    name: &str,
    color: Option<u32>,
    ring: bool,
    size: f32,
) -> Div {
    let scale = size / TILE;
    let (bg, fg) = match color {
        Some(c) => (rgba((c << 8) | 0x2e), c),
        None => (rgb(colors().button), colors().soft),
    };
    let face = div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px((TILE_RADIUS * scale).round()))
        .border_2()
        .border_color(if ring {
            rgb(colors().accent)
        } else {
            rgba(0x00000000)
        })
        .bg(bg)
        .text_color(rgb(fg))
        .text_size(px((13. * scale).max(9.5)))
        .font_weight(FontWeight::SEMIBOLD);
    let icon_svg = |path: &'static str, size: f32, color: u32| {
        svg().path(path).size(px(size)).text_color(rgb(color))
    };
    let icon_size = (TILE_ICON * scale).round();
    match icon.map(|i| (i, &i.face)) {
        Some((i, Face::Mark(path))) => {
            let color = i
                .color
                .filter(|c| host_icons::reads_on(*c, colors().button))
                .unwrap_or(fg);
            face.child(icon_svg(path, icon_size, color))
        }
        Some((_, Face::Glyph(path))) => face.child(icon_svg(path, icon_size, fg)),
        // Two letters as large as initials, three a little smaller, in
        // bold: a name, not a host's initials.
        Some((i, Face::Name(text))) => {
            let color = i
                .color
                .filter(|c| host_icons::name_reads_on(*c, colors().button))
                .unwrap_or(fg);
            let size = if text.chars().count() > 2 { 12.5 } else { 14. };
            face.text_size(px((size * scale).max(7.5)))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(color))
                .child(*text)
        }
        None => face.child(initials(name)),
    }
}

pub struct Sidebar {
    workspace: WeakEntity<Workspace>,
    sections: Vec<Section>,
    /// The row under the pointer, which shows Edit.
    hovered: Option<u64>,
    /// The group header under the pointer, likewise.
    hovered_group: Option<usize>,
    filter: Entity<TextInput>,
    /// Shown as a rail of host tiles.
    collapsed: bool,
    /// A collection's menu, open: the collection's id, and where.
    menu: Option<(String, Point<Pixels>)>,
    /// The menu's, while open, for Esc; and what had the keyboard before.
    menu_focus: FocusHandle,
    focus_before: Option<FocusHandle>,
    /// Hosts picked with Ctrl+click (Cmd+click on macOS), by spec, to run a
    /// command on.
    picked: Vec<String>,
    /// What is folded: Actions, and sections by [`Section::key`].
    folds: shellwing_config::SidebarState,
    /// AI activity, among Actions once a client is let in: the commands
    /// waiting for the person.
    ai: Option<usize>,
}

/// What marks a host given to an AI client: the same violet in every
/// theme, white text on it reading in each.
pub const AI_MARK: u32 = 0x7c3aed;

/// The Run on hosts tile, at the top of the host list and of the rail.
fn run_tile(ground: u32) -> Div {
    page_tile(&host_icons::RUN, None, TILE, ground)
}

/// A host's tile on the rail: two letters from its name. `api-1` is A1,
/// `db-primary` DP, `web-01` W1, `worker` WO.
pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    let first = |p: &str| p.chars().next().map(|c| c.to_ascii_uppercase());
    let mut out = String::new();
    match parts.as_slice() {
        [] => return "?".into(),
        [only] => {
            let chars: Vec<char> = only.chars().collect();
            out.extend(first(only));
            // A name ending in a number keeps its last digit: web01 is W1.
            match chars.iter().rev().take_while(|c| c.is_ascii_digit()).last() {
                Some(_) if chars.len() > 1 => out.extend(chars.last()),
                _ => out.extend(chars.get(1).map(|c| c.to_ascii_uppercase())),
            }
        }
        [head, .., last] => {
            out.extend(first(head));
            if last.chars().all(|c| c.is_ascii_digit()) {
                out.extend(last.chars().last());
            } else {
                out.extend(first(parts[1]));
            }
        }
    }
    out
}

/// A tooltip: a title, and a line under it when there is one. Shared by the
/// host rail and the Files panel's buttons.
pub(crate) struct Tip {
    pub title: SharedString,
    pub detail: SharedString,
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(11.))
            .py(px(7.))
            .rounded(px(8.))
            .bg(rgb(colors().chip))
            .border_1()
            .border_color(rgb(colors().edge))
            .text_color(rgb(colors().text))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            // A line each, wrapped when long.
            .children(
                self.detail
                    .split('\n')
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        div()
                            .max_w(px(320.))
                            .text_xs()
                            .text_color(rgb(colors().muted))
                            .child(SharedString::from(line.to_string()))
                    }),
            )
    }
}

impl Sidebar {
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        let filter = crate::input::said(cx, || tr!("Filter by name, tag or address"));
        cx.subscribe(&filter, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        Sidebar {
            workspace,
            sections: vec![],
            hovered: None,
            hovered_group: None,
            picked: vec![],
            folds: Default::default(),
            ai: None,
            filter,
            collapsed: false,
            menu: None,
            menu_focus: cx.focus_handle(),
            focus_before: None,
        }
    }

    /// The sections with only the rows the filter matches (all words, any order).
    fn shown(&self, cx: &App) -> Vec<Section> {
        let typed = self.filter.read(cx).text().to_lowercase();
        let words: Vec<&str> = typed.split_whitespace().collect();
        if words.is_empty() {
            return self.sections.clone();
        }
        // A part's heading goes to the first of its sections still shown.
        let mut heading = None;
        self.sections
            .iter()
            .filter_map(|s| {
                if s.heading.is_some() {
                    heading = s.heading.clone();
                }
                let rows: Vec<Row> = s
                    .rows
                    .iter()
                    .filter(|r| words.iter().all(|w| r.search.contains(w)))
                    .cloned()
                    .collect();
                (!rows.is_empty()).then(|| Section {
                    note: rows.len().to_string().into(),
                    heading: heading.take(),
                    rows,
                    ..s.clone()
                })
            })
            .collect()
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            // Esc empties the filter; Enter opens the first host it shows.
            "escape" => {
                self.filter.update(cx, |f, cx| f.set_text("", 0..0, cx));
            }
            "enter" => {
                let first = self.shown(cx).into_iter().flat_map(|s| s.rows).next();
                if let Some(row) = first {
                    self.filter.update(cx, |f, cx| f.set_text("", 0..0, cx));
                    // Once this key is handled: opening refreshes this list,
                    // which cannot be updated while it handles the key.
                    let ws = self.workspace.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = ws.update(cx, |ws, cx| ws.open(&row.spec, window, cx));
                    });
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub fn set_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.collapsed != collapsed {
            self.collapsed = collapsed;
            cx.notify();
        }
    }

    /// The rail: the host list folded to tiles, with the way back.
    fn rail(&self) -> Div {
        let icon_button = |id: &'static str, icon: &'static str, on: bool| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .w(px(32.))
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .when(on, |d| d.bg(rgb(colors().hover)))
                .hover(|s| s.bg(rgb(colors().selected)))
                .child(
                    svg()
                        .path(icon)
                        .size(px(16.))
                        .text_color(rgb(colors().text)),
                )
        };
        let (expand, find, add) = (
            self.workspace.clone(),
            self.workspace.clone(),
            self.workspace.clone(),
        );
        let mut tiles = div()
            .id("rail")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .pt_1()
            .pb_2();
        let mut id = 0u64;
        for (n, section) in self
            .sections
            .iter()
            .filter(|s| !s.rows.is_empty())
            .enumerate()
        {
            if n > 0 {
                tiles = tiles.child(
                    div()
                        .w(px(24.))
                        .h(px(1.))
                        .my(px(2.))
                        .bg(rgb(colors().field_edge)),
                );
            }
            for row in &section.rows {
                id += 1;
                let (spec, ws) = (row.spec.clone(), self.workspace.clone());
                let title = row.name.clone();
                let state = match row.status {
                    Some(Status::Connected) => tr!("connected"),
                    Some(Status::Connecting) => tr!("connecting"),
                    Some(Status::Lost) => tr!("connection lost"),
                    Some(Status::Failed) => tr!("could not connect"),
                    Some(Status::Closed) => tr!("closed"),
                    None => String::new(),
                };
                let detail: SharedString = [row.detail.to_string(), state.to_string()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into();
                tiles = tiles.child(
                    div()
                        .id(ElementId::NamedInteger("tile".into(), id))
                        .debug_selector(|| format!("tile:{}", row.name))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.85))
                        .tooltip(move |_, cx| {
                            let (title, detail) = (title.clone(), detail.clone());
                            cx.new(|_| Tip { title, detail }).into()
                        })
                        .on_click({
                            let target = row.edit.clone();
                            move |ev, window, cx| {
                                let _ = ws.update(cx, |ws, cx| {
                                    ws.click_host(&target, &spec, ev.click_count(), window, cx)
                                });
                            }
                        })
                        .child(tile(row, colors().panel, row.active)),
                );
            }
        }
        // The ground is the workspace's, which rounds the window's corner.
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .border_r_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .child(
                        icon_button("expand-hosts", "icons/panel-open.svg", true).on_click(
                            move |_, window, cx| {
                                let _ = expand.update(cx, |ws, cx| {
                                    ws.set_sidebar_collapsed(false, window, cx)
                                });
                            },
                        ),
                    )
                    .child(
                        icon_button("find-host", "icons/search.svg", false).on_click(
                            move |_, window, cx| {
                                let _ =
                                    find.update(cx, |ws, cx| ws.open_palette_from_rail(window, cx));
                            },
                        ),
                    ),
            )
            .child(
                div()
                    .id("rail-run")
                    .debug_selector(|| "rail-run".into())
                    .flex_shrink_0()
                    .pb(px(8.))
                    .mb(px(4.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .tooltip(|_, cx| {
                        cx.new(|_| Tip {
                            title: tr!("Run on hosts").into(),
                            detail: tr!("One command, many hosts").into(),
                        })
                        .into()
                    })
                    .on_click({
                        let ws = self.workspace.clone();
                        move |_, window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.open_run(vec![], window, cx));
                        }
                    })
                    .child(run_tile(colors().panel)),
            )
            .child(tiles)
            .child(
                div()
                    .w_full()
                    .py_2()
                    .flex()
                    .justify_center()
                    .border_t_1()
                    .border_color(rgb(colors().line))
                    .child(
                        div()
                            .id("rail-new-host")
                            .debug_selector(|| "rail-new-host".into())
                            .w(px(36.))
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(colors().field_edge))
                            .text_color(rgb(colors().text))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click(move |_, window, cx| {
                                let _ = add
                                    .update(cx, |ws, cx| ws.edit_host(EditTarget::New, window, cx));
                            })
                            .child("+"),
                    ),
            )
    }

    /// The sections shown, each title with its hosts' names: for the UI
    /// tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn marked(&self) -> Option<String> {
        self.sections
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.active)
            .map(|r| r.name.to_string())
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn icon_of(&self, name: &str) -> Option<&'static str> {
        self.sections
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.name == name)
            .and_then(|r| r.icon)
            .map(|i| i.id)
    }

    /// For the UI tests: the sections' titles and their hosts' names.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn shown_names(&self, cx: &App) -> Vec<(String, Vec<String>)> {
        self.shown(cx)
            .into_iter()
            .map(|s| {
                let names = s.rows.iter().map(|r| r.name.to_string()).collect();
                (s.title.to_string(), names)
            })
            .collect()
    }

    pub fn set(&mut self, sections: Vec<Section>, cx: &mut Context<Self>) {
        if sections != self.sections {
            self.sections = sections;
            cx.notify();
        }
    }

    /// The items of the menu of the collection `id`, as the list shows it
    /// now: none once it is gone, or nothing may be done to it any more.
    fn menu_items(&self, id: &str) -> Vec<MenuItem> {
        self.sections
            .iter()
            .filter_map(|s| s.shared.as_ref())
            .find(|s| s.id == id)
            .map(|s| s.menu.clone())
            .unwrap_or_default()
    }

    /// Opens a collection's menu, if it has one: it has the keyboard, for
    /// Esc, until it closes.
    fn open_menu(
        &mut self,
        id: String,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu_items(&id).is_empty() {
            return;
        }
        if self.menu.is_none() {
            self.focus_before = window.focused(cx);
        }
        self.menu = Some((id, at));
        window.focus(&self.menu_focus, cx);
        cx.notify();
    }

    /// Closes the menu; the keyboard goes back where it was, unless what was
    /// chosen takes it.
    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if self.menu_focus.is_focused(window)
            && let Some(f) = self.focus_before.take()
        {
            window.focus(&f, cx);
        }
        self.focus_before = None;
        cx.notify();
    }

    fn collection_menu(
        &self,
        items: &[MenuItem],
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut menu = div()
            .id("collection-menu")
            .debug_selector(|| "collection-menu".into())
            .min_w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(colors().raised))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_sm()
            .track_focus(&self.menu_focus)
            .on_key_down(cx.listener(|s, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    s.close_menu(window, cx);
                    cx.stop_propagation();
                }
            }))
            // A press on it is its own, not the list's under it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Another click anywhere closes it (a right click on a title
            // opens that one's).
            .on_mouse_down_out(cx.listener(|s, e: &MouseDownEvent, window, cx| {
                if e.button != MouseButton::Right {
                    s.close_menu(window, cx);
                }
            }));
        for item in items {
            let (id, action, ws) = (item.id, item.action.clone(), self.workspace.clone());
            menu = menu.child(
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .px_3()
                    .py(px(5.))
                    .rounded(px(4.))
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().selected)))
                    .text_color(rgb(colors().text))
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.close_menu(window, cx);
                        let (action, ws) = (action.clone(), ws.clone());
                        // Once this click is handled: the action may refresh
                        // this list, which cannot be updated while it
                        // handles the click.
                        window.defer(cx, move |window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.run(action, window, cx));
                        });
                    }))
                    .child(item.label.clone()),
            );
        }
        gpui::deferred(
            gpui::anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element()
    }
}

/// Opens a part of the host list: a quiet label and a rule, so it reads
/// apart from the sections' titles under it.
fn part_heading(n: usize, label: SharedString) -> impl IntoElement {
    div()
        .id(("part", n))
        .debug_selector({
            let label = label.clone();
            move || format!("part:{label}")
        })
        .flex()
        .items_center()
        .gap(px(8.))
        .px_3()
        .pt_3()
        .text_size(px(11.))
        .text_color(rgb(colors().faint))
        .child(div().flex_shrink_0().child(label.clone()))
        .child(div().flex_1().h(px(1.)).bg(rgb(colors().field_edge)))
}

impl Section {
    /// What its folding is kept by: a collection's id, a group's name, else
    /// its title.
    pub fn key(&self) -> String {
        match (&self.shared, &self.group) {
            (Some(s), _) => format!("collection:{}", s.id),
            (None, Some(g)) => format!("group:{g}"),
            (None, None) => self.title.to_string(),
        }
    }
}

impl Sidebar {
    /// AI activity among Actions (Some: how many wait), or not.
    pub fn set_ai(&mut self, ai: Option<usize>, cx: &mut Context<Self>) {
        if self.ai != ai {
            self.ai = ai;
            cx.notify();
        }
    }

    /// What was folded when the app last ran.
    pub fn set_folds(&mut self, folds: shellwing_config::SidebarState, cx: &mut Context<Self>) {
        self.folds = folds;
        cx.notify();
    }

    /// Folds Actions (None) or a section, or unfolds it; then the workspace
    /// keeps what is folded. Called from a plain click handler: the
    /// workspace is not being updated then.
    fn toggle_fold(me: &WeakEntity<Sidebar>, section: Option<String>, cx: &mut gpui::App) {
        let Ok((ws, folds)) = me.update(cx, |s, cx| {
            match &section {
                None => s.folds.actions_open = !s.folds.actions_open,
                Some(key) => match s.folds.folded.iter().position(|k| k == key) {
                    Some(i) => {
                        s.folds.folded.remove(i);
                    }
                    None => s.folds.folded.push(key.clone()),
                },
            }
            cx.notify();
            (s.workspace.clone(), s.folds.clone())
        }) else {
            return;
        };
        let _ = ws.update(cx, |ws, _| ws.save_folds(&folds));
    }

    /// Picks a host to run on, or takes it away.
    fn pick(&mut self, spec: &str, cx: &mut Context<Self>) {
        match self.picked.iter().position(|s| s == spec) {
            Some(i) => {
                self.picked.remove(i);
            }
            None => self.picked.push(spec.to_string()),
        }
        cx.notify();
    }

    /// The hosts picked went to a Run tab, or were let go.
    pub fn clear_picked(&mut self, cx: &mut Context<Self>) {
        if !self.picked.is_empty() {
            self.picked.clear();
            cx.notify();
        }
    }

    #[cfg(all(test, feature = "ui-tests"))]
    pub fn picked(&self) -> Vec<String> {
        self.picked.clone()
    }

    /// What can be done across hosts, above them: Run on hosts.
    fn actions(&self, cx: &mut Context<Self>) -> Div {
        let ws = self.workspace.clone();
        let open = self.folds.actions_open;
        let me = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pt_2()
            .pb(px(if open { 6. } else { 2. }))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .id("actions-fold")
                    .debug_selector(|| "actions-fold".into())
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(26.))
                    .mx(px(ROW_MARGIN))
                    .pl(px(12. - ROW_MARGIN))
                    .pr(px(6.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors().muted))
                    .on_click(move |_, _, cx| Sidebar::toggle_fold(&me, None, cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(tr!("ACTIONS"))
                            .when(!open && self.ai.is_some_and(|n| n > 0), |d| {
                                d.child(
                                    div()
                                        .px(px(5.))
                                        .rounded_full()
                                        .bg(rgb(colors().amber))
                                        .text_color(rgb(colors().on_amber))
                                        .child(self.ai.unwrap_or(0).to_string()),
                                )
                            }),
                    )
                    .child(
                        svg()
                            .path(if open {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            })
                            .size(px(14.))
                            .text_color(rgb(colors().muted)),
                    ),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .id("action-run")
                        .debug_selector(|| "action-run".into())
                        .flex()
                        .items_center()
                        .gap(px(ROW_GAP))
                        .h(px(ROW_HEIGHT))
                        .pl(px(ROW_PAD))
                        .pr(px(10.))
                        .mx(px(ROW_MARGIN))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(move |_, window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.open_run(vec![], window, cx));
                        })
                        .child(run_tile(colors().panel))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(colors().text))
                                        .child(tr!("Run on hosts")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("One command, many hosts")),
                                ),
                        ),
                )
            })
            .when_some(self.ai.filter(|_| open), |d, waiting| {
                let ws = self.workspace.clone();
                d.child(
                    div()
                        .id("action-ai")
                        .debug_selector(|| "action-ai".into())
                        .flex()
                        .items_center()
                        .gap(px(ROW_GAP))
                        .h(px(ROW_HEIGHT))
                        .pl(px(ROW_PAD))
                        .pr(px(10.))
                        .mx(px(ROW_MARGIN))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(move |_, window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.open_activity(window, cx));
                        })
                        .child(page_tile(&host_icons::AI, None, TILE, colors().panel))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(colors().text))
                                        .child(tr!("AI activity")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(colors().muted))
                                        .child(tr!("What AI clients run")),
                                ),
                        )
                        .when(waiting > 0, |d| {
                            d.child(
                                div()
                                    .debug_selector(|| "ai-waiting".into())
                                    .px(px(6.))
                                    .rounded_full()
                                    .bg(rgb(colors().amber))
                                    .text_color(rgb(colors().on_amber))
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .child(waiting.to_string()),
                            )
                        }),
                )
            })
    }

    /// Under the list while hosts are picked: run on them, give them to AI,
    /// or let them go.
    fn picked_bar(&self, cx: &mut Context<Self>) -> Div {
        let n = self.picked.len();
        let (ws, picked) = (self.workspace.clone(), self.picked.clone());
        let (ws2, picked2) = (ws.clone(), picked.clone());
        let how = if cfg!(target_os = "macos") {
            tr!("Cmd+click to add or take away")
        } else {
            tr!("Ctrl+click to add or take away")
        };
        div()
            .debug_selector(|| "picked-bar".into())
            .flex()
            .flex_col()
            .gap(px(8.))
            .mx_2()
            .mb_2()
            .p(px(10.))
            .rounded(px(10.))
            .bg(rgb(colors().chip))
            .text_color(rgb(colors().text))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .child(tr_n!(
                                n,
                                "{n} host chosen",
                                "{n} hosts chosen",
                                n = n.to_string()
                            ))
                            .child(div().text_color(rgb(colors().muted)).child(how)),
                    )
                    .child(
                        div()
                            .id("picked-clear")
                            .debug_selector(|| "picked-clear".into())
                            .flex_shrink_0()
                            .h(px(24.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .text_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors().hover)))
                            .on_click(cx.listener(|s, _, _, cx| s.clear_picked(cx)))
                            .child(tr!("Clear")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("picked-run")
                            .debug_selector(|| "picked-run".into())
                            .flex_1()
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(rgb(colors().accent))
                            .hover(|s| s.bg(rgb(colors().accent_hover)))
                            .text_color(rgb(colors().on_accent))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                let picked = picked.clone();
                                let _ = ws.update(cx, |ws, cx| ws.open_run(picked, window, cx));
                            })
                            .child(format!("▶  {}", tr!("Run on hosts"))),
                    )
                    .child(
                        div()
                            .id("picked-give")
                            .debug_selector(|| "picked-give".into())
                            .flex_1()
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(rgb(AI_MARK))
                            .hover(|s| s.opacity(0.9))
                            .text_color(rgb(colors().on_accent))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                let picked = picked2.clone();
                                let _ = ws2.update(cx, |ws, cx| ws.open_give(picked, window, cx));
                            })
                            .child(tr!("Give to AI…")),
                    ),
            )
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.collapsed {
            if self.menu.is_some() {
                self.close_menu(window, cx);
            }
            return self.rail();
        }
        let mut list = div()
            .id("hosts")
            .debug_selector(|| "hosts".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb_2();
        let mut id = 0u64;
        let shown = self.shown(cx);
        let filtering = !self.filter.read(cx).text().trim().is_empty();
        for (n, section) in shown.iter().enumerate() {
            let over = self.hovered_group == Some(n) && section.group.is_some();
            // A collection's menu: its button on hover, in place of the count.
            let has_menu = section.shared.as_ref().is_some_and(|s| !s.menu.is_empty());
            let more = self.hovered_group == Some(n) && has_menu;
            let collection = section
                .shared
                .as_ref()
                .map(|s| s.id.clone())
                .unwrap_or_default();
            let collection_more = collection.clone();
            let (group, ws) = (section.group.clone(), self.workspace.clone());
            if let Some(heading) = &section.heading {
                list = list.child(part_heading(n, heading.clone()));
            }
            let key = section.key();
            // A filter shows what it finds, folded or not.
            let folded = !filtering && self.folds.folded.contains(&key);
            let me = cx.entity().downgrade();
            list = list.child(
                div()
                    .id(("section", n))
                    .debug_selector(|| format!("section:{}", section.title))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| Sidebar::toggle_fold(&me, Some(key.clone()), cx))
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(26.))
                    .px_3()
                    .mt_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors().muted))
                    .when(has_menu, |d| {
                        d.on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                s.open_menu(collection.clone(), e.position, window, cx);
                            }),
                        )
                    })
                    .on_hover(cx.listener(move |s, over: &bool, _, cx| {
                        let now = if *over {
                            Some(n)
                        } else if s.hovered_group == Some(n) {
                            None
                        } else {
                            s.hovered_group
                        };
                        if now != s.hovered_group {
                            s.hovered_group = now;
                            cx.notify();
                        }
                    }))
                    // A long name ends in an ellipsis: the count and Edit
                    // stay in sight.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .when_some(section.color, |d, c| {
                                d.child(
                                    div()
                                        .size(px(8.))
                                        .flex_shrink_0()
                                        .rounded(px(2.))
                                        .bg(rgb(c)),
                                )
                            })
                            .when(section.shared.is_some(), |d| {
                                d.child(
                                    svg()
                                        .path("icons/users.svg")
                                        .size(px(13.))
                                        .flex_shrink_0()
                                        .text_color(rgb(colors().accent)),
                                )
                            })
                            .child({
                                let title = div()
                                    .id(("section-title", n))
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(section.title.to_uppercase());
                                match &section.shared {
                                    Some(shared) => {
                                        let (title_tip, detail) =
                                            (section.title.clone(), shared.tip.clone());
                                        title.tooltip(move |_, cx| {
                                            let (title, detail) =
                                                (title_tip.clone(), detail.clone());
                                            cx.new(|_| Tip { title, detail }).into()
                                        })
                                    }
                                    None => title,
                                }
                            }),
                    )
                    .when(!over, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .pl_2()
                                .font_weight(FontWeight::NORMAL)
                                .when_some(section.shared.as_ref(), |d, shared| {
                                    d.child(
                                        div()
                                            .debug_selector(|| {
                                                format!("permission:{}", section.title)
                                            })
                                            .px(px(5.))
                                            .rounded(px(4.))
                                            .border_1()
                                            .border_color(rgb(colors().field_edge))
                                            .text_color(rgb(colors().soft))
                                            .child(shared.permission.clone()),
                                    )
                                })
                                .when(!more, |d| d.child(section.note.clone()))
                                .when(more, |d| {
                                    d.child(
                                        div()
                                            .id(("collection-more", n))
                                            .debug_selector(|| {
                                                format!("collection-more:{}", section.title)
                                            })
                                            .px(px(6.))
                                            .rounded_md()
                                            .cursor_pointer()
                                            .text_color(rgb(colors().text))
                                            .hover(|s| s.bg(rgb(colors().hover)))
                                            .on_click(cx.listener(
                                                move |s, e: &gpui::ClickEvent, window, cx| {
                                                    cx.stop_propagation();
                                                    s.open_menu(
                                                        collection_more.clone(),
                                                        e.position(),
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ))
                                            .child("⋯"),
                                    )
                                }),
                        )
                    })
                    .when(over && !section.rows.is_empty(), |d| {
                        let (ws, specs) = (
                            self.workspace.clone(),
                            section
                                .rows
                                .iter()
                                .map(|r| r.spec.clone())
                                .collect::<Vec<_>>(),
                        );
                        let title = section.title.clone();
                        d.child(
                            div()
                                .id(("run-group", n))
                                .debug_selector(move || format!("run-group:{title}"))
                                .flex_shrink_0()
                                .ml_2()
                                .px(px(7.))
                                .py(px(1.))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(colors().field_edge))
                                .bg(rgb(colors().field))
                                .text_color(rgb(colors().accent))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(colors().hover)))
                                .tooltip({
                                    let n = section.rows.len();
                                    let group = section.title.clone();
                                    move |_, cx| {
                                        let title = tr_n!(
                                            n,
                                            "Run on the {n} host of {group}",
                                            "Run on the {n} hosts of {group}",
                                            n = n.to_string(),
                                            group = group.to_string()
                                        );
                                        cx.new(|_| Tip {
                                            title: title.into(),
                                            detail: SharedString::default(),
                                        })
                                        .into()
                                    }
                                })
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let specs = specs.clone();
                                    let _ = ws.update(cx, |ws, cx| ws.open_run(specs, window, cx));
                                })
                                .child("▶"),
                        )
                    })
                    .when(over, |d| {
                        d.child(
                            div()
                                .id(("edit-group", n))
                                .debug_selector(|| format!("edit-group:{}", section.title))
                                .flex_shrink_0()
                                .ml_2()
                                .px_2()
                                .py(px(1.))
                                .rounded_md()
                                .bg(rgb(colors().amber))
                                .text_color(rgb(colors().on_amber))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(colors().amber_hover)))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let name = group.clone();
                                    let _ = ws.update(cx, |ws, cx| ws.edit_group(name, window, cx));
                                })
                                .child(tr!("Edit")),
                        )
                    }),
            );
            if folded {
                id += section.rows.len() as u64;
                continue;
            }
            if section.rows.is_empty() {
                list = list.child(
                    div()
                        // Where its hosts would start.
                        .pl(px(ROW_MARGIN + ROW_PAD))
                        .pr_3()
                        .pb_1()
                        .text_xs()
                        .text_color(rgb(colors().faint))
                        .child(tr!("No hosts yet")),
                );
            }
            for row in &section.rows {
                id += 1;
                let (spec, workspace) = (row.spec.clone(), self.workspace.clone());
                let this = id;
                let hovered = self.hovered == Some(this);
                let (edit, editing) = (row.edit.clone(), self.workspace.clone());
                let picked = self.picked.contains(&row.spec);
                list = list.child(
                    div()
                        .id(ElementId::NamedInteger("host".into(), id))
                        .debug_selector(|| format!("host:{}", row.name))
                        .on_hover(cx.listener(move |s, over: &bool, _, cx| {
                            let now = if *over {
                                Some(this)
                            } else if s.hovered == Some(this) {
                                None
                            } else {
                                s.hovered
                            };
                            if now != s.hovered {
                                s.hovered = now;
                                cx.notify();
                            }
                        }))
                        .flex()
                        .items_center()
                        .gap(px(ROW_GAP))
                        .h(px(ROW_HEIGHT))
                        .pl(px(ROW_PAD))
                        .pr(px(10.))
                        .mx(px(ROW_MARGIN))
                        .rounded_md()
                        .cursor_pointer()
                        // The host of the active tab; one picked to run on.
                        .when(row.active || picked, |d| d.bg(rgb(colors().selected)))
                        .when(picked, |d| d.border_1().border_color(rgb(colors().accent)))
                        .when(!row.active && !picked, |d| {
                            d.hover(|s| s.bg(rgb(colors().hover)))
                        })
                        .on_click({
                            let target = row.edit.clone();
                            let local = matches!(row.edit, EditTarget::Local);
                            // Not as the list's listener: going to a host
                            // draws the list again.
                            let me = cx.entity().downgrade();
                            move |ev: &gpui::ClickEvent, window, cx| {
                                if ev.modifiers().secondary() && !local {
                                    let _ = me.update(cx, |s, cx| s.pick(&spec, cx));
                                    return;
                                }
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.click_host(&target, &spec, ev.click_count(), window, cx)
                                });
                            }
                        })
                        // The dot's ring is the row's own ground when it is
                        // marked, so the dot does not sit in a hole.
                        .child(
                            tile(
                                row,
                                if row.active {
                                    colors().selected
                                } else {
                                    colors().panel
                                },
                                false,
                            )
                            .debug_selector(|| format!("host-tile:{}", row.name)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .whitespace_nowrap()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(rgb(colors().text))
                                        .child(row.name.clone()),
                                )
                                .when(!row.detail.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(px(11.5))
                                            .line_height(px(15.))
                                            .text_color(rgb(colors().muted))
                                            .child(row.detail.clone()),
                                    )
                                }),
                        )
                        // Hovered: Edit, at the end.
                        .when(hovered, |d| {
                            d.child(
                                div()
                                    .id(ElementId::NamedInteger("edit-host".into(), this))
                                    .debug_selector(|| format!("edit-host:{}", row.name))
                                    // An amber badge: plain to see on any row.
                                    .px_2()
                                    .py(px(1.))
                                    .rounded_md()
                                    .bg(rgb(colors().amber))
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors().on_amber))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(colors().amber_hover)))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        let edit = edit.clone();
                                        let _ = editing
                                            .update(cx, |ws, cx| ws.edit_host(edit, window, cx));
                                    })
                                    .child(tr!("Edit")),
                            )
                        }),
                );
            }
        }
        if filtering && shown.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .pt_3()
                    .text_color(rgb(colors().muted))
                    .child(tr!("No host matches.")),
            );
        } else if self.sections.is_empty() {
            let ws = self.workspace.clone();
            list = list
                .child(
                    div()
                        .px_3()
                        .pt_3()
                        .text_color(rgb(colors().muted))
                        .child(tr!(
                            "No hosts yet. Add one, or Host blocks to ~/.ssh/config."
                        )),
                )
                .child(
                    div()
                        .id("import-hosts")
                        .debug_selector(|| "import-hosts".into())
                        .mx_3()
                        .mt_2()
                        .text_color(rgb(colors().accent))
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(move |_, window, cx| {
                            let _ = ws.update(cx, |ws, cx| ws.open_import(window, cx));
                        })
                        .child(tr!("Import hosts from another app…")),
                );
        }
        let (workspace, for_group, for_about) = (
            self.workspace.clone(),
            self.workspace.clone(),
            self.workspace.clone(),
        );
        let footer = div()
            .p_2()
            .flex()
            .gap_2()
            .border_t_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .id("new-host")
                    .debug_selector(|| "new-host".into())
                    .flex_1()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .text_color(rgb(colors().text))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(move |_, window, cx| {
                        let _ = workspace
                            .update(cx, |ws, cx| ws.edit_host(EditTarget::New, window, cx));
                    })
                    .child(tr!("+ Host")),
            )
            .child(
                div()
                    .id("new-group")
                    .debug_selector(|| "new-group".into())
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .text_color(rgb(colors().text))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(move |_, window, cx| {
                        let _ = for_group.update(cx, |ws, cx| ws.edit_group(None, window, cx));
                    })
                    .child(tr!("+ Group")),
            )
            .child(
                div()
                    .id("about")
                    .debug_selector(|| "about".into())
                    .size(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .tooltip(|_, cx| {
                        cx.new(|_| Tip {
                            title: tr!("About shellwing").into(),
                            detail: SharedString::default(),
                        })
                        .into()
                    })
                    .on_click(move |_, window, cx| {
                        let _ = for_about
                            .update(cx, |ws, cx| ws.open_settings(Page::About, window, cx));
                    })
                    .child(
                        svg()
                            .path("icons/info.svg")
                            .size(px(16.))
                            .text_color(rgb(colors().soft)),
                    ),
            );
        let collapse = self.workspace.clone();
        // The collection gone from under its open menu, or nothing left to
        // do to it: the menu closes.
        let menu = match self.menu.clone() {
            Some((id, at)) => {
                let items = self.menu_items(&id);
                if items.is_empty() {
                    self.close_menu(window, cx);
                    None
                } else {
                    Some(self.collection_menu(&items, at, cx))
                }
            }
            None => None,
        };
        // The ground is the workspace's, which rounds the window's corner.
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(colors().line))
            .text_sm()
            .on_key_down(cx.listener(Self::on_key))
            .child(self.actions(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(24.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(colors().muted))
                                    .child(tr!("HOSTS")),
                            )
                            .child(
                                div()
                                    .id("collapse-hosts")
                                    .debug_selector(|| "collapse-hosts".into())
                                    .w(px(26.))
                                    .h(px(24.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(colors().hover)))
                                    .on_click(move |_, window, cx| {
                                        let _ = collapse.update(cx, |ws, cx| {
                                            ws.set_sidebar_collapsed(true, window, cx)
                                        });
                                    })
                                    .child(
                                        svg()
                                            .path("icons/panel-close.svg")
                                            .size(px(16.))
                                            .text_color(rgb(colors().muted)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .h(px(30.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(colors().field_edge))
                            .bg(rgb(colors().field))
                            .text_xs()
                            .text_color(rgb(colors().text))
                            .debug_selector(|| "host-filter".into())
                            .child(self.filter.clone()),
                    ),
            )
            .child(list)
            .when(!self.picked.is_empty(), |d| d.child(self.picked_bar(cx)))
            .child(footer)
            .when_some(menu, |d, m| d.child(m))
    }
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn tiles_get_two_letters() {
        for (name, want) in [
            ("api-1", "A1"),
            ("db-primary", "DP"),
            ("web-01", "W1"),
            ("worker", "WO"),
            ("stage-web", "SW"),
            ("nas.home", "NH"),
            ("web01", "W1"),
            ("x", "X"),
            ("--", "?"),
        ] {
            assert_eq!(initials(name), want, "{name}");
        }
    }
}

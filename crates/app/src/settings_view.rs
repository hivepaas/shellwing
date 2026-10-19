//! Settings: a page over the window, as the approved sketch draws it, with the
//! sections on the left. Every change is written to `config.toml` at once (its
//! comments kept) and applies to the open tabs; the file stays the source.
//! Only settings the app has are shown; the sketch's others come with them.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, Render,
    SharedString, WeakEntity, Window, div, linear_color_stop, linear_gradient, prelude::*, px, rgb,
    uniform_list,
};
use shellwing_config::{
    Color, CursorStyle, HostKey, OnDisconnect, Scrollbar, SettingValue, Settings, Theme,
};

use crate::input::TextInput;
use crate::keys_view::KeysView;
use crate::known_hosts_view::KnownHostsView;
use crate::look::ui;
use crate::palette::Action;
use crate::snippets_view::SnippetsView;
use crate::workspace::Workspace;

const BG: u32 = 0x0b0f17;
const BAR: u32 = 0x0f1522;
const NAV: u32 = 0x0d121c;
const LINE: u32 = 0x1e2736;
const RULE: u32 = 0x1f2937;
const FIELD_EDGE: u32 = 0x263042;
const TEXT: u32 = 0xe2e8f0;
const SOFT: u32 = 0xb6c0cf;
const LABEL: u32 = 0xa3aec0;
const MUTED: u32 = 0x8a96aa;
const ON: u32 = 0x17233a;
const ACCENT: u32 = 0x38bdf8;

pub enum SettingsEvent {
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    General,
    Appearance,
    Terminal,
    Keyboard,
    Ssh,
    Keys,
    KnownHosts,
    Snippets,
    Sync,
    Vault,
    About,
}

const PAGES: [(Page, &str); 11] = [
    (Page::General, "General"),
    (Page::Appearance, "Appearance"),
    (Page::Terminal, "Terminal"),
    (Page::Keyboard, "Keyboard"),
    (Page::Ssh, "SSH and security"),
    (Page::Keys, "Keys"),
    (Page::KnownHosts, "Known hosts"),
    (Page::Snippets, "Snippets"),
    (Page::Sync, "Sync"),
    (Page::Vault, "Vault"),
    (Page::About, "About"),
];

/// What the page shows, taken from the workspace.
#[derive(Clone)]
pub struct Snap {
    pub settings: Settings,
    pub themes: Vec<Arc<Theme>>,
    /// Installed fonts that look monospaced.
    pub fonts: Vec<String>,
    /// The font used when none is set.
    pub default_font: String,
    /// Every installed font, for the app's own text.
    pub all_fonts: Vec<String>,
    /// Text editors found on this system: (name, command).
    pub editors: Vec<(String, String)>,
    pub sync_folder: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
    pub ssh_config: Option<PathBuf>,
    pub secrets: Option<usize>,
}

pub struct SettingsView {
    focus: FocusHandle,
    workspace: WeakEntity<Workspace>,
    page: Page,
    snap: Snap,
    fonts_open: Option<Fonts>,
    /// The editor command typed for Custom.
    editor_input: Entity<TextInput>,
    /// The local shell's command, as typed.
    shell_input: Entity<TextInput>,
    /// Custom chosen, the field shown.
    custom_editor: bool,
    /// The Keys page, once shown.
    keys: Option<Entity<KeysView>>,
    /// The Snippets page, once shown.
    snippets: Option<Entity<SnippetsView>>,
    /// The Known hosts page, once shown.
    known_hosts: Option<Entity<KnownHostsView>>,
}

/// Which font list is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fonts {
    Terminal,
    Ui,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

fn color(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

fn shown_path(p: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|h| p.strip_prefix(&h).ok().map(|r| r.to_owned())) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// Installed fonts whose names say they are for code.
pub fn mono_fonts(installed: &[String]) -> Vec<String> {
    const HINTS: [&str; 12] = [
        "Mono",
        "Code",
        "Consolas",
        "Menlo",
        "Monaco",
        "Courier",
        "Hack",
        "Iosevka",
        "Fixed",
        "Terminal",
        "Cascadia",
        "Inconsolata",
    ];
    let mut fonts: Vec<String> = installed
        .iter()
        .filter(|n| HINTS.iter().any(|h| n.contains(h)))
        // CJK companions are fallbacks, not terminal fonts.
        .filter(|n| !n.contains("CJK"))
        .cloned()
        .collect();
    fonts.sort();
    fonts.dedup();
    fonts
}

impl SettingsView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        page: Page,
        snap: Snap,
        cx: &mut Context<Self>,
    ) -> Self {
        // A command that is none of the editors found is a custom one.
        let current = snap
            .settings
            .files
            .editor
            .clone()
            .filter(|e| !e.trim().is_empty());
        let custom = current
            .as_ref()
            .is_some_and(|c| !snap.editors.iter().any(|(_, cmd)| cmd == c));
        let text = if custom {
            current.unwrap_or_default()
        } else {
            String::new()
        };
        let shell = snap
            .settings
            .terminal
            .local_shell
            .clone()
            .unwrap_or_default();
        let shell_input = cx.new(|cx| {
            let mut i = TextInput::new(
                format!("{} (yours)", crate::local::shell(None).join(" ")),
                cx,
            );
            i.set_text(&shell, shell.len()..shell.len(), cx);
            i
        });
        let editor_input = cx.new(|cx| {
            let mut i = TextInput::new("e.g. code -n, or /path/to/editor --flag", cx);
            i.set_text(&text, text.len()..text.len(), cx);
            i
        });
        SettingsView {
            focus: cx.focus_handle(),
            workspace,
            page,
            snap,
            fonts_open: None,
            editor_input,
            shell_input,
            custom_editor: custom,
            keys: None,
            snippets: None,
            known_hosts: None,
        }
    }

    /// Saves the local shell typed (empty: the user's own).
    fn save_local_shell(&mut self, cx: &mut Context<Self>) {
        let cmd = self.shell_input.read(cx).text().trim().to_string();
        let value = (!cmd.is_empty()).then_some(SettingValue::Str(cmd));
        self.change(&["terminal", "local_shell"], value, cx);
    }

    /// Saves the command typed for Custom (empty: the system's editor).
    fn save_custom_editor(&mut self, cx: &mut Context<Self>) {
        let cmd = self.editor_input.read(cx).text().trim().to_string();
        let value = (!cmd.is_empty()).then_some(SettingValue::Str(cmd));
        self.change(&["files", "editor"], value, cx);
    }

    pub fn set_snap(&mut self, snap: Snap, cx: &mut Context<Self>) {
        self.snap = snap;
        cx.notify();
    }

    /// Writes one setting; the workspace applies it and hands back the result.
    fn change(
        &mut self,
        path: &'static [&'static str],
        value: Option<SettingValue>,
        cx: &mut Context<Self>,
    ) {
        let snap = self
            .workspace
            .update(cx, |ws, cx| ws.change_setting(path, value, cx));
        if let Ok(Some(snap)) = snap {
            self.snap = snap;
        }
        cx.notify();
    }

    fn pick_theme(&mut self, id: String, cx: &mut Context<Self>) {
        let snap = self
            .workspace
            .update(cx, |ws, cx| ws.set_app_theme(&id, cx));
        if let Ok(Some(snap)) = snap {
            self.snap = snap;
        }
        cx.notify();
    }

    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .workspace
            .update(cx, |ws, cx| ws.run_from_settings(action, window, cx));
    }

    fn open(&self, path: &std::path::Path, cx: &mut Context<Self>) {
        cx.open_url(&format!("file://{}", path.display()));
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        // Typing the editor command: Enter saves, the arrows are the field's.
        if self.shell_input.focus_handle(cx).is_focused(window) {
            match k.key.as_str() {
                "enter" => self.save_local_shell(cx),
                "escape" => window.focus(&self.focus, cx),
                _ => return,
            }
            cx.notify();
            return cx.stop_propagation();
        }
        if self.editor_input.focus_handle(cx).is_focused(window) {
            match k.key.as_str() {
                "enter" => self.save_custom_editor(cx),
                "escape" => window.focus(&self.focus, cx),
                _ => return,
            }
            cx.notify();
            return cx.stop_propagation();
        }
        let step = |by: isize, v: &mut Self| {
            let at = PAGES.iter().position(|(p, _)| *p == v.page).unwrap_or(0) as isize;
            v.page = PAGES[(at + by).rem_euclid(PAGES.len() as isize) as usize].0;
        };
        match k.key.as_str() {
            "escape" if self.fonts_open.is_some() => self.fonts_open = None,
            "escape" => cx.emit(SettingsEvent::Close),
            "up" => step(-1, self),
            "down" => step(1, self),
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn pills<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: Vec<(T, String)>,
        current: T,
        on_pick: impl Fn(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let on_pick = std::rc::Rc::new(on_pick);
        let mut row = div().flex().flex_wrap().gap(px(4.));
        for (n, (value, label)) in options.into_iter().enumerate() {
            let on = value == current;
            let pick = on_pick.clone();
            row = row.child(
                div()
                    .id((id, n))
                    .h(px(28.))
                    .min_w(px(40.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if on { ACCENT } else { FIELD_EDGE }))
                    .bg(rgb(if on { ON } else { BG }))
                    .text_size(ui(12.5))
                    .text_color(rgb(if on { TEXT } else { SOFT }))
                    .cursor_pointer()
                    .when(!on, |d| d.hover(|s| s.bg(rgb(0x121a28))))
                    .on_click(cx.listener(move |v, _, _, cx| pick(v, value, cx)))
                    .child(label),
            );
        }
        row
    }

    /// The fonts to choose from, the default first: (family, shown as).
    fn font_choices(&self, which: Fonts) -> Vec<(Option<String>, String)> {
        let (default, list) = match which {
            Fonts::Terminal => (
                format!("Default ({})", self.snap.default_font),
                &self.snap.fonts,
            ),
            Fonts::Ui => ("Default (the system's)".to_string(), &self.snap.all_fonts),
        };
        let mut choices = vec![(None, default)];
        choices.extend(list.iter().map(|f| (Some(f.clone()), f.clone())));
        choices
    }

    fn current_font(&self, which: Fonts) -> Option<String> {
        let a = &self.snap.settings.appearance;
        match which {
            Fonts::Terminal => a.font.family.clone(),
            Fonts::Ui => a.ui_font.family.clone(),
        }
    }

    /// A font field and, when open, the list of fonts, each in its own face.
    fn font_picker(&self, which: Fonts, cx: &mut Context<Self>) -> gpui::Div {
        const ROW: f32 = 28.;
        let current = self.current_font(which);
        let choices = self.font_choices(which);
        let shown = choices
            .iter()
            .find(|(v, _)| *v == current)
            .map(|(_, n)| n.clone())
            .or(current.clone())
            .unwrap_or_default();
        let open = self.fonts_open == Some(which);
        let mut picker = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .flex_1()
            .min_w(px(200.))
            .child(label("Font"))
            .child(
                div()
                    .id(match which {
                        Fonts::Terminal => "font-select",
                        Fonts::Ui => "ui-font-select",
                    })
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if open { ACCENT } else { FIELD_EDGE }))
                    .bg(rgb(BG))
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.fonts_open = if v.fonts_open == Some(which) {
                            None
                        } else {
                            Some(which)
                        };
                        cx.notify();
                    }))
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(shown),
                    )
                    .child(div().text_color(rgb(MUTED)).child("▾")),
            );
        if open {
            let n = choices.len();
            // Virtual: only the fonts in view are drawn (there may be hundreds).
            let list = uniform_list(
                match which {
                    Fonts::Terminal => "font-list",
                    Fonts::Ui => "ui-font-list",
                },
                n,
                cx.processor(move |v, range: std::ops::Range<usize>, _, cx| {
                    let choices = v.font_choices(which);
                    let current = v.current_font(which);
                    range
                        .map(|i| {
                            let (value, name) = choices[i].clone();
                            let on = value == current;
                            let pick = value.clone();
                            div()
                                .id(("font", i))
                                .h(px(ROW))
                                .px_2()
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .when(on, |d| d.bg(rgb(ON)))
                                .hover(|s| s.bg(rgb(0x1a2333)))
                                .when_some(value, |d, f| d.font_family(f))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    v.fonts_open = None;
                                    let path: &'static [&'static str] = match which {
                                        Fonts::Terminal => &["appearance", "font", "family"],
                                        Fonts::Ui => &["appearance", "ui_font", "family"],
                                    };
                                    v.change(path, pick.clone().map(SettingValue::Str), cx);
                                }))
                                .child(name)
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .h(px((n as f32 * ROW).min(240.)));
            picker = picker.child(
                div()
                    .p_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(0x2b3648))
                    .bg(rgb(0x111826))
                    .child(list),
            );
        }
        picker
    }

    /// The app's own font: tabs, the host list, the palette, panels, this page.
    fn app_text(&self, cx: &mut Context<Self>) -> gpui::Div {
        let sizes: Vec<(u32, String)> = [11, 12, 13, 14, 15, 16]
            .into_iter()
            .map(|s| (s, s.to_string()))
            .collect();
        let size = self.snap.settings.appearance.ui_font.size.round() as u32;
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(heading2("App text"))
                    .child(label(
                        "Tabs, the host list, the palette, panels and this page.",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .items_start()
                    .child(self.font_picker(Fonts::Ui, cx))
                    .child(labelled(
                        "Size",
                        self.pills(
                            "ui-size",
                            sizes,
                            size,
                            |v, s, cx| {
                                v.change(
                                    &["appearance", "ui_font", "size"],
                                    Some(SettingValue::Float(s as f64)),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    )),
            )
    }

    fn general(&self, cx: &mut Context<Self>) -> gpui::Div {
        let restore = self.snap.settings.general.restore_tabs;
        // The text editor for remote files: the system's, one found here, or a command.
        let configured = self
            .snap
            .settings
            .files
            .editor
            .clone()
            .filter(|e| !e.trim().is_empty());
        let mut choices: Vec<(usize, String)> = vec![(0, "The system's".to_string())];
        for (n, (name, _)) in self.snap.editors.iter().enumerate() {
            choices.push((n + 1, name.clone()));
        }
        let custom_at = choices.len();
        choices.push((custom_at, "Custom…".to_string()));
        let current = if self.custom_editor {
            custom_at
        } else {
            match &configured {
                None => 0,
                Some(c) => self
                    .snap
                    .editors
                    .iter()
                    .position(|(_, cmd)| cmd == c)
                    .map_or(custom_at, |i| i + 1),
            }
        };
        let editors = self.snap.editors.clone();
        let mut editor_box = div().flex().flex_col().gap_2().child(self.pills(
            "text-editor",
            choices,
            current,
            move |v, i, cx| {
                if i == custom_at {
                    v.custom_editor = true;
                    cx.notify();
                    return;
                }
                v.custom_editor = false;
                let value = editors
                    .get(i.wrapping_sub(1))
                    .map(|(_, cmd)| SettingValue::Str(cmd.clone()));
                v.change(&["files", "editor"], value, cx);
            },
            cx,
        ));
        if current == custom_at {
            editor_box = editor_box
                .child(
                    div()
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(FIELD_EDGE))
                        .bg(rgb(BG))
                        .text_size(ui(13.))
                        .child(self.editor_input.clone()),
                )
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(MUTED))
                        .child("The file's path is added at the end. Enter saves."),
                );
        }
        let mut page = page("General")
            .child(setting(
                "Text editor for remote files",
                "Edit in text editor, in the Files panel, opens a copy with it and writes the file back each time you save. The system's is the desktop's app for plain text.",
                editor_box,
            ))
            .child(toggle(
            "restore-tabs",
            restore,
            "Open the last tabs at start",
            "When shellwing starts without a host, it opens the tabs and splits it had.",
            cx.listener(move |v, _, _, cx| {
                v.change(
                    &["general", "restore_tabs"],
                    Some(SettingValue::Bool(!restore)),
                    cx,
                )
            }),
        ));
        if let Some(dir) = self.snap.config_dir.clone() {
            let files = [
                (
                    "config.toml",
                    "These settings, for this device. Edits in the file apply at once.",
                ),
                ("hosts.toml", "Hosts and groups. Synced. No secrets."),
                ("themes/*.toml", "Your own themes."),
                ("vault.db", "Passwords and passphrases, encrypted."),
                ("System keychain", "The vault's unlock key."),
            ];
            let mut list = div().flex().flex_col().gap(px(6.));
            for (name, what) in files {
                list = list.child(
                    div()
                        .flex()
                        .gap_3()
                        .text_size(ui(12.5))
                        .child(div().w(px(130.)).flex_shrink_0().child(name))
                        .child(div().text_color(rgb(MUTED)).child(what)),
                );
            }
            page = page
                .child(rule())
                .child(heading("Where settings live"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(ui(12.5))
                                .text_color(rgb(LABEL))
                                .child(shown_path(&dir)),
                        )
                        .child(
                            small_button("open-config-dir", "Open folder")
                                .on_click(cx.listener(move |v, _, _, cx| v.open(&dir, cx))),
                        ),
                )
                .child(list);
        }
        page
    }

    fn appearance(&self, cx: &mut Context<Self>) -> gpui::Div {
        let a = &self.snap.settings.appearance;
        let current = a.theme.clone();
        let tile = |t: &Arc<Theme>, cx: &mut Context<Self>| {
            let on = t.id == current;
            let id = t.id.clone();
            let swatch = div()
                .h(px(46.))
                .rounded(px(8.))
                .border_2()
                .border_color(rgb(if on { ACCENT } else { RULE }));
            let swatch = match &t.gradient {
                Some(g) if g.stops.len() >= 2 => swatch.bg(linear_gradient(
                    g.angle,
                    linear_color_stop(rgb(color(g.stops[0].0)), 0.),
                    linear_color_stop(rgb(color(g.stops[g.stops.len() - 1].0)), 1.),
                )),
                _ => swatch.bg(rgb(color(t.background))),
            };
            div()
                .id(SharedString::from(format!("theme-{}", t.id)))
                .flex()
                .flex_col()
                .gap(px(5.))
                .min_w_0()
                .cursor_pointer()
                .on_click(cx.listener(move |v, _, _, cx| v.pick_theme(id.clone(), cx)))
                .child(swatch)
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(if on { TEXT } else { LABEL }))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(t.name.clone()),
                )
        };
        let (gradients, solids): (Vec<_>, Vec<_>) =
            self.snap.themes.iter().partition(|t| t.gradient.is_some());
        let mut grad_grid = div().grid().grid_cols(6).gap(px(10.));
        for t in &gradients {
            grad_grid = grad_grid.child(tile(t, cx));
        }
        let mut solid_grid = div().grid().grid_cols(6).gap(px(10.));
        for t in &solids {
            solid_grid = solid_grid.child(tile(t, cx));
        }
        let themes_dir = self.snap.config_dir.as_ref().map(|d| d.join("themes"));

        // Text.
        let font_box = self.font_picker(Fonts::Terminal, cx);
        let size = a.font.size;
        let sizes: Vec<(u32, String)> = [11, 12, 13, 14, 15, 16, 18]
            .into_iter()
            .map(|s| (s, s.to_string()))
            .collect();
        let line = (a.line_height * 10.).round() as u32;
        let lines: Vec<(u32, String)> = [10, 11, 12, 13, 14, 15]
            .into_iter()
            .map(|l| (l, format!("{:.1}", l as f32 / 10.)))
            .collect();
        let cursors = vec![
            (CursorStyle::Block, "Block".to_string()),
            (CursorStyle::Bar, "Bar".to_string()),
            (CursorStyle::Underline, "Underline".to_string()),
        ];
        let bars = vec![
            (Scrollbar::Auto, "When scrolling".to_string()),
            (Scrollbar::Always, "Always".to_string()),
            (Scrollbar::Never, "Never".to_string()),
        ];
        let pads: Vec<(u32, String)> = [0, 2, 4, 8, 12]
            .into_iter()
            .map(|p| (p, p.to_string()))
            .collect();
        let text = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(heading2("Terminal text"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .items_start()
                    .child(font_box)
                    .child(labelled(
                        "Size",
                        self.pills(
                            "size",
                            sizes,
                            size.round() as u32,
                            |v, s, cx| {
                                v.change(
                                    &["appearance", "font", "size"],
                                    Some(SettingValue::Float(s as f64)),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .child(labelled(
                        "Line height",
                        self.pills(
                            "line",
                            lines,
                            line,
                            |v, l, cx| {
                                v.change(
                                    &["appearance", "line_height"],
                                    Some(SettingValue::Float(l as f64 / 10.)),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    ))
                    .child(labelled(
                        "Cursor",
                        self.pills(
                            "cursor",
                            cursors,
                            a.cursor.style,
                            |v, c, cx| {
                                let name = match c {
                                    CursorStyle::Block => "block",
                                    CursorStyle::Bar => "bar",
                                    CursorStyle::Underline => "underline",
                                };
                                v.change(
                                    &["appearance", "cursor", "style"],
                                    Some(SettingValue::Str(name.into())),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .child(labelled(
                        "Scrollbar",
                        self.pills(
                            "scrollbar",
                            bars,
                            a.scrollbar,
                            |v, b, cx| {
                                let name = match b {
                                    Scrollbar::Auto => "auto",
                                    Scrollbar::Always => "always",
                                    Scrollbar::Never => "never",
                                };
                                v.change(
                                    &["appearance", "scrollbar"],
                                    Some(SettingValue::Str(name.into())),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    ))
                    .child(labelled(
                        "Padding at the sides",
                        self.pills(
                            "pad-x",
                            pads.clone(),
                            a.padding.x.round() as u32,
                            |v, p, cx| {
                                v.change(
                                    &["appearance", "padding", "x"],
                                    Some(SettingValue::Float(p as f64)),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    ))
                    .child(labelled(
                        "Top and bottom",
                        self.pills(
                            "pad-y",
                            pads,
                            a.padding.y.round() as u32,
                            |v, p, cx| {
                                v.change(
                                    &["appearance", "padding", "y"],
                                    Some(SettingValue::Float(p as f64)),
                                    cx,
                                )
                            },
                            cx,
                        ),
                    )),
            );

        let left = div()
            .flex_1()
            .min_w(px(420.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(
                        div()
                            .text_size(ui(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Theme"),
                    )
                    .child(label(
                        "Applies to every tab. A host can have its own in its editor.",
                    )),
            )
            .when(!gradients.is_empty(), |d| {
                d.child(heading("Gradient")).child(grad_grid)
            })
            .when(!solids.is_empty(), |d| {
                d.child(heading("Solid")).child(solid_grid)
            })
            .when_some(themes_dir, |d, dir| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("themes-dir", "Open themes folder").on_click(
                            cx.listener(move |v, _, _, cx| {
                                let _ = std::fs::create_dir_all(&dir);
                                v.open(&dir, cx)
                            }),
                        ))
                        .child(label("A .toml file there is a theme of your own.")),
                )
            })
            .child(rule())
            .child(text)
            .child(rule())
            .child(self.app_text(cx));

        div()
            .flex()
            .flex_wrap()
            .gap(px(24.))
            .items_start()
            .child(left)
            .child(self.preview())
    }

    /// A terminal drawn in the chosen theme and font.
    fn preview(&self) -> gpui::Div {
        let a = &self.snap.settings.appearance;
        let theme = self
            .snap
            .themes
            .iter()
            .find(|t| t.id == a.theme)
            .or(self.snap.themes.first());
        let Some(t) = theme else {
            return div();
        };
        let p = |i: usize| rgb(color(t.palette[i]));
        let fg = rgb(color(t.foreground));
        let font = a
            .font
            .family
            .clone()
            .unwrap_or_else(|| self.snap.default_font.clone());
        let size = a.font.size;
        let line_h = size * a.line_height;
        let line = |parts: Vec<(&str, gpui::Rgba)>| {
            let mut l = div()
                .flex()
                .h(px(line_h))
                .items_center()
                .whitespace_nowrap();
            for (text, c) in parts {
                l = l.child(div().text_color(c).child(text.to_string()));
            }
            l
        };
        let prompt = |cmd: &str| {
            line(vec![
                ("deploy@web-01", p(2)),
                (":", fg),
                ("~/apps", p(4)),
                (" $ ", fg),
                (cmd, fg),
            ])
        };
        let mut screen = div()
            .flex()
            .flex_col()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(LINE))
            .overflow_hidden()
            .font_family(font)
            .text_size(px(size))
            .child(prompt("ls"))
            .child(line(vec![
                ("shop/  ", p(4)),
                ("deploy.sh  ", p(2)),
                (".env.example", fg),
            ]))
            .child(prompt("./deploy.sh"))
            .child(line(vec![("==> ", p(6)), ("Building shop/web:2.4.2", fg)]))
            .child(line(vec![("warn ", p(3)), ("cache miss for layer 3", fg)]))
            .child(line(vec![
                ("error ", p(1)),
                ("healthcheck timeout, retrying", fg),
            ]))
            .child(line(vec![("ok ", p(2)), ("deployed in 41 s", fg)]))
            .child(prompt(""));
        screen = match &t.gradient {
            Some(g) if g.stops.len() >= 2 => screen.bg(linear_gradient(
                g.angle,
                linear_color_stop(rgb(color(g.stops[0].0)), 0.),
                linear_color_stop(rgb(color(g.stops[g.stops.len() - 1].0)), 1.),
            )),
            _ => screen.bg(rgb(color(t.background))),
        };
        div()
            .w(px(400.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Preview"))
            .child(screen)
            .child(
                div()
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(rgb(0x0d121c))
                    .border_1()
                    .border_color(rgb(LINE))
                    .text_size(ui(11.5))
                    .text_color(rgb(MUTED))
                    .child("# config.toml")
                    .child("[appearance]")
                    .child(format!("theme = \"{}\"", a.theme))
                    .child(match &a.font.family {
                        Some(f) => format!("font = {{ family = \"{f}\", size = {} }}", a.font.size),
                        None => format!("font = {{ size = {} }}", a.font.size),
                    }),
            )
    }

    fn terminal(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = &self.snap.settings.terminal;
        let lines: Vec<(usize, String)> = [1_000, 10_000, 50_000, 100_000]
            .into_iter()
            .map(|n| (n, format!("{} k", n / 1000)))
            .collect();
        let drops = vec![
            (OnDisconnect::Reconnect, "Reconnect".to_string()),
            (OnDisconnect::Ask, "Ask".to_string()),
            (OnDisconnect::Close, "Close".to_string()),
        ];
        page("Terminal")
            .child(setting(
                "Scrollback",
                &format!("Lines kept per pane, now {}. More uses more memory; new tabs get it.", t.scrollback),
                self.pills("scrollback", lines, t.scrollback, |v, n, cx| {
                    v.change(&["terminal", "scrollback"], Some(SettingValue::Int(n as i64)), cx)
                }, cx),
            ))
            .child(setting(
                "When a connection drops",
                "Reconnect tries again by itself, waiting longer each time. After exit, Enter connects again.",
                self.pills("on-disconnect", drops, t.on_disconnect, |v, d, cx| {
                    let name = match d {
                        OnDisconnect::Reconnect => "reconnect",
                        OnDisconnect::Ask => "ask",
                        OnDisconnect::Close => "close",
                    };
                    v.change(&["terminal", "on_disconnect"], Some(SettingValue::Str(name.into())), cx)
                }, cx),
            ))
            .child(setting(
                "Local shell",
                "What a local terminal runs (This computer, in the host list), with its arguments. Empty: your own shell. Enter saves; new local tabs get it.",
                div().w(px(320.)).child(
                    div()
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(FIELD_EDGE))
                        .bg(rgb(BG))
                        .text_size(ui(13.))
                        .child(self.shell_input.clone()),
                ),
            ))
    }

    fn keyboard(&self) -> gpui::Div {
        let mac = cfg!(target_os = "macos");
        let (chord, pane) = if mac {
            ("Cmd", "Cmd+Alt")
        } else {
            ("Ctrl+Shift", "Alt")
        };
        let keys: Vec<(&str, String)> = vec![
            ("Command palette", format!("{chord}+K")),
            ("New tab", format!("{chord}+T")),
            ("Close the pane", format!("{chord}+W")),
            ("Split right", format!("{chord}+D")),
            (
                "Split down",
                if mac {
                    "Cmd+Shift+D".into()
                } else {
                    "Ctrl+Shift+E".into()
                },
            ),
            ("Move between panes", format!("{pane}+arrows")),
            ("Next and previous tab", "Ctrl+Tab, Ctrl+Shift+Tab".into()),
            (
                "Go to tab 1–9",
                if mac {
                    "Cmd+1…9".into()
                } else {
                    "Alt+1…9".into()
                },
            ),
            ("Host list", format!("{chord}+B")),
            ("Files", format!("{chord}+O")),
            (
                "Settings",
                if mac { "Cmd+,".into() } else { "Ctrl+,".into() },
            ),
            ("Next and previous theme", format!("{chord}+] and [")),
            (
                "Copy and paste",
                if mac {
                    "Cmd+C, Cmd+V".into()
                } else {
                    "Ctrl+Shift+C, Ctrl+Shift+V".into()
                },
            ),
            ("Back to the bottom", "Shift+End".into()),
        ];
        let mut list = div().flex().flex_col().gap(px(8.));
        for (what, key) in keys {
            list = list.child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(ui(13.))
                    .child(what)
                    .child(
                        div()
                            .px_2()
                            .rounded(px(4.))
                            .bg(rgb(0x111a2a))
                            .border_1()
                            .border_color(rgb(0x2b3648))
                            .text_size(ui(12.))
                            .child(key),
                    ),
            );
        }
        page("Keyboard")
            .child(label(
                "Window shortcuts never reach the remote host. They cannot be changed yet.",
            ))
            .child(div().max_w(px(520.)).child(list))
    }

    fn ssh(&self, cx: &mut Context<Self>) -> gpui::Div {
        let agent = if std::env::var_os("SSH_AUTH_SOCK").is_some() || cfg!(windows) {
            "Found. Its keys are tried first."
        } else {
            "Not found (SSH_AUTH_SOCK is not set)."
        };
        let file_row = |id: &'static str,
                        what: &str,
                        note: &str,
                        path: Option<PathBuf>,
                        cx: &mut Context<Self>| {
            let shown = path
                .as_deref()
                .map(shown_path)
                .unwrap_or_else(|| "none".into());
            setting(
                what,
                note,
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_size(ui(12.5))
                            .text_color(rgb(LABEL))
                            .child(shown),
                    )
                    .when_some(path.filter(|p| p.exists()), |d, p| {
                        d.child(
                            small_button(id, "Open")
                                .on_click(cx.listener(move |v, _, _, cx| v.open(&p, cx))),
                        )
                    }),
            )
        };
        let ssh = &self.snap.settings.ssh;
        let keys = vec![
            (HostKey::Ask, "Ask".to_string()),
            (HostKey::AcceptNew, "Accept new".to_string()),
            (HostKey::Strict, "Refuse".to_string()),
        ];
        let alive: Vec<(u64, String)> = [0, 15, 30, 60]
            .into_iter()
            .map(|s| {
                (
                    s,
                    if s == 0 {
                        "Off".into()
                    } else {
                        format!("{s} s")
                    },
                )
            })
            .collect();
        page("SSH and security")
            .child(setting(
                "Unknown host keys",
                "Ask shows the fingerprint and sends nothing until you accept it. Accept new records it and goes on; Refuse turns it away. A changed key is always refused. A host can have its own in its editor.",
                self.pills("host-key", keys, ssh.host_key, |v, k, cx| {
                    v.change(&["ssh", "host_key"], Some(SettingValue::Str(k.name().into())), cx)
                }, cx),
            ))
            .child(setting(
                "Keepalive",
                &format!(
                    "A check every so often, so idle connections are not dropped and dead ones are noticed after 3 misses. Now {}. A host's own (or its ServerAliveInterval) wins.",
                    if ssh.keepalive == 0 { "off".to_string() } else { format!("{} s", ssh.keepalive) }
                ),
                self.pills("keepalive", alive, ssh.keepalive, |v, s, cx| {
                    v.change(&["ssh", "keepalive"], Some(SettingValue::Int(s as i64)), cx)
                }, cx),
            ))
            .child(file_row(
                "known-hosts",
                "known_hosts file",
                "Shared with ssh. shellwing adds the keys you trust.",
                self.snap.known_hosts.clone(),
                cx,
            ))
            .child(file_row(
                "ssh-config",
                "ssh config file",
                "Read only. Its hosts show in the host list.",
                self.snap.ssh_config.clone(),
                cx,
            ))
            .child(setting("Agent", "Keys in ssh-agent (Pageant or the OpenSSH agent on Windows).", div().text_size(ui(12.5)).child(agent)))
    }

    fn sync(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut buttons = div().flex().flex_wrap().gap_2();
        let state = match &self.snap.sync_folder {
            Some(f) => {
                buttons = buttons
                    .child(
                        small_button("sync-now", "Sync now")
                            .on_click(cx.listener(|v, _, w, cx| v.run(Action::SyncNow, w, cx))),
                    )
                    .child(
                        small_button("sync-key", "Show the recovery key").on_click(
                            cx.listener(|v, _, w, cx| v.run(Action::ShowRecoveryKey, w, cx)),
                        ),
                    )
                    .child(
                        small_button("sync-stop", "Stop syncing")
                            .on_click(cx.listener(|v, _, w, cx| v.run(Action::StopSync, w, cx))),
                    );
                format!("With {}", shown_path(f))
            }
            None => {
                buttons = buttons.child(
                    small_button("sync-folder", "Sync with a folder…")
                        .on_click(cx.listener(|v, _, w, cx| v.run(Action::SyncFolder, w, cx))),
                );
                "Off".into()
            }
        };
        page("Sync")
            .child(setting(
                "Sync through a folder",
                "Hosts and saved passwords travel through a folder that another app syncs: iCloud Drive, Syncthing, Dropbox. Everything in it is encrypted with your recovery key.",
                div().flex().flex_col().gap_2().child(div().text_size(ui(12.5)).child(state)).child(buttons),
            ))
            .child(label("Hosts kept on this device only (in their editor) stay out of it."))
    }

    fn vault(&self, cx: &mut Context<Self>) -> gpui::Div {
        let count = match self.snap.secrets {
            Some(0) => "None saved yet.".to_string(),
            Some(1) => "1 saved.".to_string(),
            Some(n) => format!("{n} saved."),
            None => "The vault is not open.".to_string(),
        };
        page("Vault").child(setting(
            "Saved passwords and passphrases",
            "Offered for saving after you type one that works, and tried before asking next time. Kept in vault.db, encrypted; its key is in the system keychain.",
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().text_size(ui(12.5)).child(count))
                .child(small_button("vault-manage", "Manage…").on_click(cx.listener(|v, _, w, cx| v.run(Action::ManageSecrets, w, cx)))),
        ))
    }

    /// The Keys page: a view of its own, made when first shown.
    fn keys_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let keys = match &self.keys {
            Some(k) => k.clone(),
            None => {
                let Some((vault, rt)) = self.workspace.upgrade().map(|w| w.read(cx).keys_context())
                else {
                    return page("Keys");
                };
                let ws = self.workspace.clone();
                let k = cx.new(|cx| KeysView::new(ws, vault, rt, cx));
                self.keys = Some(k.clone());
                k
            }
        };
        div().child(keys)
    }

    /// The Snippets page: a view of its own, made when first shown.
    fn snippets_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let view = match &self.snippets {
            Some(s) => s.clone(),
            None => {
                let Some((vault, _)) = self.workspace.upgrade().map(|w| w.read(cx).keys_context())
                else {
                    return page("Snippets");
                };
                let ws = self.workspace.clone();
                let s = cx.new(|cx| SnippetsView::new(ws, vault, cx));
                self.snippets = Some(s.clone());
                s
            }
        };
        div().child(view)
    }

    fn about(&self) -> gpui::Div {
        page("About")
            .child(
                div()
                    .text_size(ui(15.))
                    .child(format!("shellwing {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(label("A native SSH client."))
    }
}

fn page(title: &str) -> gpui::Div {
    div().flex().flex_col().gap(px(16.)).max_w(px(760.)).child(
        div()
            .text_size(ui(18.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title.to_string()),
    )
}

fn heading(s: &str) -> gpui::Div {
    div()
        .text_size(ui(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(s.to_uppercase())
}

fn heading2(s: &str) -> gpui::Div {
    div()
        .text_size(ui(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(s.to_string())
}

fn label(s: &str) -> gpui::Div {
    div()
        .text_size(ui(12.))
        .text_color(rgb(LABEL))
        .child(s.to_string())
}

fn labelled(title: &str, control: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(label(title))
        .child(control)
}

fn rule() -> gpui::Div {
    div().h(px(1.)).w_full().bg(rgb(RULE))
}

/// A setting: its name and what it does on the left, the control on the right.
fn setting(title: &str, what: &str, control: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .gap(px(16.))
        .py(px(10.))
        .border_b_1()
        .border_color(rgb(RULE))
        .child(
            div()
                .w(px(300.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().text_size(ui(13.)).child(title.to_string()))
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(MUTED))
                        .child(what.to_string()),
                ),
        )
        .child(div().flex_1().min_w(px(200.)).child(control))
}

fn toggle(
    id: &'static str,
    on: bool,
    title: &str,
    what: &str,
    click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .gap_3()
        .py(px(10.))
        .border_b_1()
        .border_color(rgb(RULE))
        .cursor_pointer()
        .on_click(click)
        .child(
            div()
                .mt(px(2.))
                .size(px(16.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(if on { ACCENT } else { FIELD_EDGE }))
                .bg(rgb(if on { ACCENT } else { BG }))
                .text_size(ui(11.))
                .text_color(rgb(0x04121c))
                .when(on, |d| d.child("✓")),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().text_size(ui(13.)).child(title.to_string()))
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(MUTED))
                        .child(what.to_string()),
                ),
        )
}

fn small_button(id: &'static str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(FIELD_EDGE))
        .text_size(ui(12.))
        .text_color(rgb(SOFT))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x121a28)).text_color(rgb(TEXT)))
        .child(label.to_string())
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header =
            div()
                .h(px(40.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .pl(px(14.))
                .pr(px(6.))
                .bg(rgb(BAR))
                .border_b_1()
                .border_color(rgb(LINE))
                .child(
                    gpui::svg()
                        .path("icons/logo.svg")
                        .size(px(16.))
                        .text_color(rgb(ACCENT)),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Settings"))
                .child(div().flex_1())
                .when_some(self.snap.config_dir.clone(), |d, dir| {
                    d.child(small_button("open-config", "Open config.toml").on_click(
                        cx.listener(move |v, _, _, cx| v.open(&dir.join("config.toml"), cx)),
                    ))
                })
                .child(
                    div()
                        .id("settings-close")
                        .w(px(34.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .text_color(rgb(SOFT))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(0x1a2333)))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)))
                        .child("✕"),
                );
        let mut nav = div()
            .w(px(220.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .py(px(12.))
            .bg(rgb(NAV))
            .border_r_1()
            .border_color(rgb(LINE));
        for (n, (p, name)) in PAGES.iter().enumerate() {
            let (p, on) = (*p, *p == self.page);
            nav = nav.child(
                div()
                    .id(("settings-page", n))
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .text_color(rgb(if on { TEXT } else { SOFT }))
                    .cursor_pointer()
                    .when(on, |d| d.bg(rgb(ON)).border_l_2().border_color(rgb(ACCENT)))
                    .when(!on, |d| d.hover(|s| s.bg(rgb(0x121a28))))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.page = p;
                        v.fonts_open = None;
                        if p == Page::Keys
                            && let Some(k) = &v.keys
                        {
                            k.update(cx, |k, cx| k.refresh(cx));
                        }
                        if p == Page::Snippets
                            && let Some(s) = &v.snippets
                        {
                            s.update(cx, |s, cx| s.refresh(cx));
                        }
                        if p == Page::KnownHosts
                            && let Some(k) = &v.known_hosts
                        {
                            k.update(cx, |k, cx| k.refresh(cx));
                        }
                        cx.notify();
                    }))
                    .child(*name),
            );
        }
        let content = match self.page {
            Page::General => self.general(cx),
            Page::Appearance => self.appearance(cx),
            Page::Terminal => self.terminal(cx),
            Page::Keyboard => self.keyboard(),
            Page::Ssh => self.ssh(cx),
            Page::Keys => self.keys_page(cx),
            Page::Snippets => self.snippets_page(cx),
            Page::KnownHosts => {
                let view = self
                    .known_hosts
                    .get_or_insert_with(|| cx.new(KnownHostsView::new))
                    .clone();
                div().child(view)
            }
            Page::Sync => self.sync(cx),
            Page::Vault => self.vault(cx),
            Page::About => self.about(),
        };
        div()
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_size(ui(13.))
            .text_color(rgb(TEXT))
            .child(header)
            .child(
                div().flex_1().min_h_0().flex().child(nav).child(
                    div()
                        .id("settings-content")
                        .flex_1()
                        .min_w_0()
                        .overflow_y_scroll()
                        .px(px(24.))
                        .py(px(20.))
                        .child(content),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_fonts_are_picked_out() {
        let installed: Vec<String> = [
            "JetBrains Mono",
            "Noto Sans",
            "Noto Sans Mono CJK SC",
            "Fira Code",
            "Ubuntu",
            "DejaVu Sans Mono",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            mono_fonts(&installed),
            ["DejaVu Sans Mono", "Fira Code", "JetBrains Mono"]
        );
    }
}

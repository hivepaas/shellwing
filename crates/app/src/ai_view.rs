//! Settings › AI: the AI clients let in, each with its token and the config
//! to paste into it, and the hosts given them (docs/configuration.md, AI
//! access).

use std::collections::BTreeSet;

use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px, rgb,
};
use serde_json::json;
use shellwing_config::{AiClient, AiGrant, AiHow, AiMode, AiState};
use shellwing_core::ai;
use shellwing_i18n::tr;

use crate::colors::colors;
use crate::host_editor::{hint_text, label_text, outline_button, primary_button};
use crate::input::{Changed, TextInput};
use crate::look::ui;
use crate::run_view::Pick;
use crate::workspace::Workspace;

/// The clients a config is given for: id, name.
pub const KINDS: [(&str, &str); 10] = [
    ("claude-code", "Claude Code"),
    ("claude-desktop", "Claude Desktop"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("windsurf", "Windsurf"),
    ("zed", "Zed"),
    ("codex", "Codex CLI"),
    ("gemini", "Gemini CLI"),
    ("cline", "Cline"),
    ("stdio", "Another MCP client"),
];

fn kind_name(kind: &str) -> String {
    KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or_else(|| kind.to_string(), |(_, n)| n.to_string())
}

/// Where a client of `kind` takes its config, and the config: shellwing at
/// `exe` (the full path: an app started from a dock has no PATH), its
/// token in the environment, never on a command line.
pub fn config_for(kind: &str, exe: &str, token: &str) -> (String, String) {
    let entry = json!({"command": exe, "args": ["mcp"], "env": {"SHELLWING_TOKEN": token}});
    let wrapped = |key: &str, entry: serde_json::Value| {
        serde_json::to_string_pretty(&json!({ key: {"shellwing": entry} })).unwrap_or_default()
    };
    let quote = |s: &str| {
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
        {
            s.to_string()
        } else {
            format!("\"{}\"", s.replace('"', "\\\""))
        }
    };
    match kind {
        "claude-code" => (
            tr!("Run this once in a terminal:"),
            format!(
                "claude mcp add --scope user shellwing --env SHELLWING_TOKEN={token} -- {} mcp",
                quote(exe)
            ),
        ),
        "claude-desktop" => (
            tr!(
                "In Claude Desktop, Settings › Developer › Edit Config, add this to claude_desktop_config.json:"
            ),
            wrapped("mcpServers", entry),
        ),
        "cursor" => (
            tr!("Add this to {file}:", file = "~/.cursor/mcp.json"),
            wrapped("mcpServers", entry),
        ),
        "vscode" => {
            let mut e = entry;
            e["type"] = json!("stdio");
            (
                tr!(
                    "Add this to {file}:",
                    file = "mcp.json (MCP: Open User Configuration)"
                ),
                wrapped("servers", e),
            )
        }
        "windsurf" => (
            tr!(
                "Add this to {file}:",
                file = "~/.codeium/windsurf/mcp_config.json"
            ),
            wrapped("mcpServers", entry),
        ),
        "zed" => (
            tr!("Add this to {file}:", file = "Zed's settings.json"),
            wrapped("context_servers", entry),
        ),
        "codex" => (
            tr!("Add this to {file}:", file = "~/.codex/config.toml"),
            format!(
                "[mcp_servers.shellwing]\ncommand = {}\nargs = [\"mcp\"]\n\n[mcp_servers.shellwing.env]\nSHELLWING_TOKEN = \"{token}\"",
                toml::Value::String(exe.to_string())
            ),
        ),
        "gemini" => (
            tr!("Add this to {file}:", file = "~/.gemini/settings.json"),
            wrapped("mcpServers", entry),
        ),
        "cline" => (
            tr!("In Cline, MCP Servers › Configure, add this to cline_mcp_settings.json:"),
            wrapped("mcpServers", entry),
        ),
        _ => (
            tr!("A client that starts its MCP server itself, over stdio:"),
            format!("command  {exe} mcp\nenv      SHELLWING_TOKEN={token}"),
        ),
    }
}

/// What the page shows, from the workspace.
pub struct AiSnap {
    pub state: AiState,
    /// Clients connected now, by id.
    pub connected: BTreeSet<String>,
    pub picks: Vec<Pick>,
    /// shellwing itself, for the configs.
    pub exe: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    TakenBack,
    Session,
    Hour,
    EightHours,
}

/// The form that gives a host.
struct Giving {
    host: Option<String>,
    client: Option<String>,
    until: Until,
}

pub struct AiView {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    /// Choosing a client to add.
    adding: bool,
    /// The client just added: its id and its token, shown this once.
    token: Option<(String, String)>,
    /// A client whose config shows again (without its token).
    showing: Option<String>,
    giving: Option<Giving>,
    filter: Entity<TextInput>,
    mono: SharedString,
}

impl Focusable for AiView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AiView {
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> AiView {
        let filter = crate::input::said(cx, || tr!("Find a host"));
        cx.subscribe(&filter, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        AiView {
            workspace,
            focus: cx.focus_handle(),
            adding: false,
            token: None,
            showing: None,
            giving: None,
            filter,
            mono: SharedString::default(),
        }
    }

    fn change(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut AiState)) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.change_ai(f, cx));
        }
        cx.notify();
    }

    fn add_client(&mut self, kind: &str, cx: &mut Context<Self>) {
        let token = ai::new_token();
        let id = shellwing_config::new_id();
        let client = AiClient {
            id: id.clone(),
            kind: kind.to_string(),
            name: kind_name(kind),
            token_sha256: ai::token_hash(&token),
            added_at: ai::now(),
        };
        self.change(cx, |s| s.clients.push(client));
        self.adding = false;
        self.showing = None;
        self.token = Some((id, token));
    }

    fn give(&mut self, cx: &mut Context<Self>) {
        let Some(g) = &self.giving else {
            return;
        };
        let (Some(host), Some(client)) = (g.host.clone(), g.client.clone()) else {
            return;
        };
        let (until, session) = match g.until {
            Until::TakenBack => (None, false),
            Until::Session => (None, true),
            Until::Hour => (Some(ai::now() + 3600), false),
            Until::EightHours => (Some(ai::now() + 8 * 3600), false),
        };
        let grant = AiGrant {
            client: client.clone(),
            host: host.clone(),
            how: AiHow::Background,
            mode: AiMode::Free,
            until,
            session,
        };
        self.change(cx, move |s| {
            s.grants.retain(|g| !(g.client == client && g.host == host));
            s.grants.push(grant);
        });
        self.giving = None;
    }

    fn clients_part(&self, snap: &AiSnap, cx: &mut Context<Self>) -> gpui::Div {
        let mut part = div().flex().flex_col().gap(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(heading(&tr!("AI clients")))
                        .child(hint_text(&tr!(
                            "Agents that reach your hosts through shellwing, over MCP. They never see a key, a password or a passphrase: shellwing signs in for them."
                        ))),
                )
                .child(
                    primary_button("ai-add-client", &tr!("Add client"))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.adding = !v.adding;
                            v.token = None;
                            cx.notify();
                        })),
                ),
        );
        if self.adding {
            let mut kinds = div().flex().flex_wrap().gap(px(6.));
            for (kind, name) in KINDS {
                kinds = kinds.child(
                    outline_button(&format!("ai-kind:{kind}"), name)
                        .h(px(28.))
                        .text_size(ui(12.))
                        .on_click(cx.listener(move |v, _, _, cx| v.add_client(kind, cx))),
                );
            }
            part = part.child(card().child(label_text(&tr!("Which client?"))).child(kinds));
        }
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().surface))
            .overflow_hidden();
        if snap.state.clients.is_empty() {
            list = list.child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .child(hint_text(&tr!("No client yet: Add client lets one in."))),
            );
        }
        for c in &snap.state.clients {
            let on = snap.connected.contains(&c.id);
            let (id, id2, id3) = (c.id.clone(), c.id.clone(), c.id.clone());
            let shown = self.showing.as_ref() == Some(&c.id)
                || self.token.as_ref().is_some_and(|(t, _)| *t == c.id);
            list = list.child(
                div()
                    .debug_selector({
                        let n = c.name.clone();
                        move || format!("ai-client:{n}")
                    })
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(52.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(c.name.clone()))
                            .child(hint_text(&tr!(
                                "token …{end}",
                                end = c.token_sha256.chars().take(4).collect::<String>()
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(ui(12.))
                            .text_color(rgb(if on {
                                colors().good_text
                            } else {
                                colors().muted
                            }))
                            .child(div().size(px(7.)).rounded_full().bg(rgb(if on {
                                colors().ok
                            } else {
                                colors().idle
                            })))
                            .child(if on {
                                tr!("connected now")
                            } else {
                                tr!("not connected")
                            }),
                    )
                    .child(
                        outline_button(&format!("ai-config:{}", c.name), &tr!("Config"))
                            .h(px(28.))
                            .text_size(ui(12.))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.showing = if v.showing.as_ref() == Some(&id) {
                                    None
                                } else {
                                    Some(id.clone())
                                };
                                cx.notify();
                            })),
                    )
                    .child(
                        outline_button(&format!("ai-remove:{}", c.name), &tr!("Remove"))
                            .h(px(28.))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                let id = id2.clone();
                                if v.token.as_ref().is_some_and(|(t, _)| *t == id) {
                                    v.token = None;
                                }
                                v.change(cx, |s| {
                                    s.clients.retain(|c| c.id != id);
                                    s.grants.retain(|g| g.client != id);
                                });
                            })),
                    ),
            );
            if shown {
                let token = match &self.token {
                    Some((t, token)) if *t == id3 => Some(token.clone()),
                    _ => None,
                };
                list = list.child(self.config_part(c, token, &snap.exe));
            }
        }
        part.child(list)
    }

    /// A client's config: with its token right after it is added, else
    /// with where the token goes, since only its hash is kept.
    fn config_part(&self, c: &AiClient, token: Option<String>, exe: &str) -> gpui::Div {
        let shown_token = token
            .clone()
            .unwrap_or_else(|| tr!("(the token shown when it was added)"));
        let (where_, text) = config_for(&c.kind, exe, &shown_token);
        let copy = text.clone();
        div()
            .debug_selector(|| "ai-config".into())
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(14.))
            .py(px(12.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().page))
            .child(hint_text(&where_))
            .child(
                div()
                    .debug_selector(|| "ai-config-text".into())
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(rgb(colors().field))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .font_family(self.mono.clone())
                    .text_size(ui(12.))
                    .child(text),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        outline_button("ai-copy-config", &tr!("Copy"))
                            .h(px(28.))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            }),
                    )
                    .child(hint_text(&if token.is_some() {
                        tr!("The token shows this once: shellwing keeps only its hash. Remove takes it back.")
                    } else {
                        tr!("The token is not kept, only its hash: to see one again, remove the client and add it again.")
                    })),
            )
    }

    fn grants_part(&self, snap: &AiSnap, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let name_of = |spec: &str| {
            snap.picks
                .iter()
                .flat_map(|p| &p.hosts)
                .find(|h| h.spec == spec)
                .map_or_else(|| spec.to_string(), |h| h.name.clone())
        };
        let client_of = |id: &str| {
            snap.state
                .clients
                .iter()
                .find(|c| c.id == id)
                .map_or_else(|| id.to_string(), |c| c.name.clone())
        };
        let mut part = div().flex().flex_col().gap(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(heading(&tr!("Hosts given")))
                        .child(hint_text(&tr!(
                            "A client reaches only these. Its commands run in the background, on a connection of their own, and each is kept in a log on this computer."
                        ))),
                )
                .when(!snap.state.grants.is_empty(), |d| {
                    d.child(
                        outline_button("ai-take-all", &tr!("Take every one back"))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(|v, _, _, cx| v.change(cx, |s| s.grants.clear()))),
                    )
                })
                .when(!snap.state.clients.is_empty(), |d| {
                    d.child(
                        primary_button("ai-give", &tr!("Give a host…")).on_click(cx.listener(
                            |v, _, window, cx| {
                                v.giving = Some(Giving {
                                    host: None,
                                    client: None,
                                    until: Until::TakenBack,
                                });
                                window.focus(&v.filter.focus_handle(cx), cx);
                                cx.notify();
                            },
                        )),
                    )
                }),
        );
        if self.giving.is_some() {
            part = part.child(self.give_part(snap, window, cx));
        }
        let mut table = div()
            .flex()
            .flex_col()
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(colors().line))
            .bg(rgb(colors().surface))
            .overflow_hidden();
        if snap.state.grants.is_empty() {
            table = table.child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .child(hint_text(&tr!("No host is given to an AI client."))),
            );
        }
        for (i, g) in snap.state.grants.iter().enumerate() {
            let host = name_of(&g.host);
            let until = match (g.until, g.session) {
                (_, true) => tr!("while shellwing stays open"),
                (None, _) => tr!("until taken back"),
                (Some(t), _) if t <= ai::now() => tr!("ended"),
                (Some(t), _) => tr!(
                    "for {n} min more",
                    n = (t.saturating_sub(ai::now()) / 60 + 1).to_string()
                ),
            };
            let (client, spec) = (g.client.clone(), g.host.clone());
            table = table.child(
                div()
                    .debug_selector({
                        let h = host.clone();
                        move || format!("ai-grant:{h}")
                    })
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(44.))
                    .px(px(14.))
                    .border_b_1()
                    .border_color(rgb(colors().line))
                    .child(
                        div()
                            .w(px(170.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(host.clone()),
                    )
                    .child(div().w(px(150.)).child(client_of(&g.client)))
                    .child(div().flex_1().text_color(rgb(colors().muted)).child(until))
                    .child(
                        outline_button(&format!("ai-take-back:{i}"), &tr!("Take back"))
                            .h(px(26.))
                            .text_size(ui(12.))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                let (client, spec) = (client.clone(), spec.clone());
                                v.change(cx, |s| {
                                    s.grants.retain(|g| !(g.client == client && g.host == spec))
                                })
                            })),
                    ),
            );
        }
        part.child(table)
    }

    fn give_part(&self, snap: &AiSnap, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let Some(g) = &self.giving else {
            return div();
        };
        let query = self.filter.read(cx).text().trim().to_lowercase();
        let mut hosts = div()
            .id("ai-hosts")
            .flex()
            .flex_col()
            .max_h(px(220.))
            .overflow_y_scroll();
        for p in &snap.picks {
            for h in &p.hosts {
                if !query.is_empty()
                    && !h.name.to_lowercase().contains(&query)
                    && !h.detail.to_lowercase().contains(&query)
                {
                    continue;
                }
                let on = g.host.as_ref() == Some(&h.spec);
                let spec = h.spec.clone();
                hosts = hosts.child(
                    div()
                        .id(SharedString::from(format!(
                            "ai-pick:{}:{}",
                            p.group, h.spec
                        )))
                        .debug_selector({
                            let n = h.name.clone();
                            move || format!("ai-pick:{n}")
                        })
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(rgb(colors().selected)))
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if let Some(g) = &mut v.giving {
                                g.host = Some(spec.clone());
                            }
                            cx.notify();
                        }))
                        .child(div().font_weight(FontWeight::MEDIUM).child(h.name.clone()))
                        .child(hint_text(&format!("{} · {}", h.detail, p.group))),
                );
            }
        }
        let mut clients = div().flex().flex_wrap().gap(px(6.));
        for c in &snap.state.clients {
            let on = g.client.as_ref() == Some(&c.id);
            let id = c.id.clone();
            clients = clients.child(
                outline_button(&format!("ai-give-to:{}", c.name), &c.name)
                    .h(px(28.))
                    .text_size(ui(12.))
                    .when(on, |d| d.bg(rgb(colors().selected)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if let Some(g) = &mut v.giving {
                            g.client = Some(id.clone());
                        }
                        cx.notify();
                    })),
            );
        }
        let mut untils = div().flex().flex_wrap().gap(px(6.));
        for (u, label, sel) in [
            (
                Until::TakenBack,
                tr!("Until I take it back"),
                "ai-until:back",
            ),
            (
                Until::Session,
                tr!("While shellwing stays open"),
                "ai-until:session",
            ),
            (Until::Hour, tr!("1 hour"), "ai-until:1h"),
            (Until::EightHours, tr!("8 hours"), "ai-until:8h"),
        ] {
            let on = g.until == u;
            untils = untils.child(
                outline_button(sel, &label)
                    .h(px(28.))
                    .text_size(ui(12.))
                    .when(on, |d| d.bg(rgb(colors().selected)))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if let Some(g) = &mut v.giving {
                            g.until = u;
                        }
                        cx.notify();
                    })),
            );
        }
        let ready = g.host.is_some() && g.client.is_some();
        let focused = self.filter.focus_handle(cx).is_focused(window);
        card()
            .debug_selector(|| "ai-give-form".into())
            .child(label_text(&tr!("Host")))
            .child(
                div()
                    .w(px(280.))
                    .h(px(30.))
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
                    .child(self.filter.clone()),
            )
            .child(hosts)
            .child(label_text(&tr!("To")))
            .child(clients)
            .child(label_text(&tr!("For")))
            .child(untils)
            .child(hint_text(&tr!(
                "It never sees a key, a password or a passphrase: a host that would ask for one is refused, as is one whose key is not known yet. A host's output can carry words meant to steer an agent: give hosts to one you trust with them."
            )))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        outline_button("ai-give-cancel", &tr!("Cancel"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.giving = None;
                                cx.notify();
                            })),
                    )
                    .child(if ready {
                        primary_button("ai-give-ok", &tr!("Give access"))
                            .on_click(cx.listener(|v, _, _, cx| v.give(cx)))
                    } else {
                        outline_button("ai-give-ok", &tr!("Give access"))
                            .text_color(rgb(colors().muted))
                    }),
            )
    }
}

fn heading(s: &str) -> gpui::Div {
    div()
        .text_size(ui(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(s.to_string())
}

fn card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(rgb(colors().info_edge))
        .bg(rgb(colors().surface))
}

impl Render for AiView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let Some(snap) = self.workspace.upgrade().map(|w| w.read(cx).ai_snapshot(cx)) else {
            return div();
        };
        div()
            .debug_selector(|| "ai-page".into())
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap(px(26.))
            .max_w(px(900.))
            .child(self.clients_part(&snap, cx))
            .child(self.grants_part(&snap, window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each client's config parses as its own format, and holds the path and
    /// the token where the client reads them.
    #[test]
    fn every_config_is_well_formed() {
        let exe = r"C:\Program Files\shellwing\shellwing.exe";
        for (kind, _) in KINDS {
            let (_, text) = config_for(kind, exe, "sw_abc");
            assert!(text.contains("sw_abc"), "{kind}");
            match kind {
                "claude-code" => assert!(text.contains("\"C:\\Program Files")),
                "codex" => {
                    let v: toml::Value = toml::from_str(&text).expect("toml");
                    assert_eq!(v["mcp_servers"]["shellwing"]["command"].as_str(), Some(exe));
                    assert_eq!(
                        v["mcp_servers"]["shellwing"]["env"]["SHELLWING_TOKEN"].as_str(),
                        Some("sw_abc")
                    );
                }
                "stdio" => {}
                _ => {
                    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
                    let key = match kind {
                        "vscode" => "servers",
                        "zed" => "context_servers",
                        _ => "mcpServers",
                    };
                    assert_eq!(v[key]["shellwing"]["command"], exe, "{kind}");
                    assert_eq!(v[key]["shellwing"]["args"][0], "mcp");
                    assert_eq!(v[key]["shellwing"]["env"]["SHELLWING_TOKEN"], "sw_abc");
                }
            }
        }
    }
}

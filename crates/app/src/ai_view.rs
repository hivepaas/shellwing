//! Settings › AI: the AI clients let in, each with its token and the config
//! to paste into it, and the hosts given them (docs/configuration.md, AI
//! access).

use std::collections::BTreeSet;

use gpui::{
    App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px, rgb,
};
use serde_json::json;
use shellwing_config::{AiClient, AiGrant, AiHow, AiMode, AiState};
use shellwing_core::ai;
use shellwing_i18n::{tr, tr_n};

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

/// The form that gives hosts.
struct Giving {
    /// The hosts ticked, by spec, in the order they were.
    hosts: Vec<String>,
    client: Option<String>,
    until: Until,
    mode: AiMode,
    how: AiHow,
    /// What it may do: run commands, read files, write files, read notes,
    /// add to notes, use snippets.
    may: [bool; 6],
}

/// A mode's name, and what it does.
fn mode_text(mode: AiMode) -> (String, String) {
    match mode {
        AiMode::Free => (
            tr!("Run freely"),
            tr!("Commands run at once. Each is kept in AI activity, as it runs."),
        ),
        AiMode::Risky => (
            tr!("Ask for risky ones"),
            tr!("A command with words from the list below waits for you; the rest run."),
        ),
        AiMode::Ask => (
            tr!("Ask every time"),
            tr!("Each command waits until you allow it, or allow it for the session."),
        ),
    }
}

const MODES: [AiMode; 3] = [AiMode::Free, AiMode::Risky, AiMode::Ask];

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
    /// Clients whose hosts the table folds away, by id.
    folded: BTreeSet<String>,
    filter: Entity<TextInput>,
    /// The risky list, a line each.
    risky: Entity<TextInput>,
    /// How much AI activity this computer keeps, in MB.
    activity_mb: Entity<TextInput>,
    /// How long sudo's password may be kept for an agent, in minutes.
    sudo_minutes: Entity<TextInput>,
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
        let words = workspace
            .upgrade()
            .map(|w| w.read(cx).ai_snapshot(cx).state.risky())
            .unwrap_or_default()
            .join("\n");
        let risky = cx.new(|cx| {
            let mut i =
                TextInput::said(|| tr!("A command with these words waits"), cx).multiline(4, 10);
            i.set_text(&words, 0..0, cx);
            i
        });
        // Kept as it is typed: a line each, the empty ones left out.
        cx.subscribe(&risky, |v: &mut AiView, input, _: &Changed, cx| {
            let lines: Vec<String> = input
                .read(cx)
                .text()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            // The usual list is kept as such, so it follows the app's.
            let usual = lines
                .iter()
                .map(String::as_str)
                .eq(shellwing_config::DEFAULT_RISKY.iter().copied());
            v.change(cx, |s| s.risky = (!usual).then_some(lines));
        })
        .detach();
        let mb = workspace
            .upgrade()
            .and_then(|w| w.read(cx).ai_snapshot(cx).state.activity_mb)
            .unwrap_or(shellwing_config::AI_ACTIVITY_MB);
        let activity_mb = cx.new(|cx| {
            let mut i = TextInput::new("", cx);
            i.set_text(&mb.to_string(), 0..0, cx);
            i
        });
        // Kept as it is typed, once it is a number of MB that may be.
        cx.subscribe(&activity_mb, |v: &mut AiView, input, _: &Changed, cx| {
            let typed: Option<u32> = input.read(cx).text().trim().parse().ok();
            match typed.filter(|n| *n >= LEAST_MB) {
                Some(mb) => v.change(cx, |s| {
                    s.activity_mb = (mb != shellwing_config::AI_ACTIVITY_MB).then_some(mb)
                }),
                None => cx.notify(),
            }
        })
        .detach();
        let minutes = workspace
            .upgrade()
            .map(|w| w.read(cx).ai_snapshot(cx).state.sudo_minutes())
            .unwrap_or(shellwing_config::SUDO_MINUTES);
        let sudo_minutes = cx.new(|cx| {
            let mut i = TextInput::new("", cx);
            i.set_text(&minutes.to_string(), 0..0, cx);
            i
        });
        // Kept as it is typed, once it is a number of minutes that may be.
        cx.subscribe(&sudo_minutes, |v: &mut AiView, input, _: &Changed, cx| {
            let typed: Option<u32> = input.read(cx).text().trim().parse().ok();
            match typed.filter(|n| *n <= shellwing_config::MOST_SUDO_MINUTES) {
                Some(m) => v.change(cx, |s| {
                    s.sudo_minutes = (m != shellwing_config::SUDO_MINUTES).then_some(m)
                }),
                None => cx.notify(),
            }
        })
        .detach();
        AiView {
            workspace,
            focus: cx.focus_handle(),
            adding: false,
            token: None,
            showing: None,
            giving: None,
            folded: BTreeSet::new(),
            filter,
            risky,
            activity_mb,
            sudo_minutes,
            mono: SharedString::default(),
        }
    }

    /// The folders on this computer a client may copy files from and to: a
    /// chip each, to take away, and a way to add one.
    fn folders_part(&self, c: &AiClient, cx: &mut Context<Self>) -> gpui::Div {
        let mut row = div()
            .debug_selector({
                let n = c.name.clone();
                move || format!("ai-folders:{n}")
            })
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(14.))
            .py(px(8.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .text_size(ui(12.))
            .child(
                div()
                    .text_color(rgb(colors().muted))
                    .child(tr!("Local folders")),
            );
        if c.folders.is_empty() {
            row = row.child(hint_text(&tr!(
                "none: it copies no file to or from this computer"
            )));
        }
        for (i, f) in c.folders.iter().enumerate() {
            let (id, folder) = (c.id.clone(), f.clone());
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .bg(rgb(colors().chip))
                    .child(f.clone())
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "ai-folder-remove:{}:{i}",
                                c.name
                            )))
                            .debug_selector({
                                let n = c.name.clone();
                                move || format!("ai-folder-remove:{n}:{i}")
                            })
                            .cursor_pointer()
                            .text_color(rgb(colors().muted))
                            .child("✕")
                            .on_click(cx.listener(move |v, _, _, cx| {
                                let (id, folder) = (id.clone(), folder.clone());
                                v.change(cx, |s| {
                                    for c in s.clients.iter_mut().filter(|c| c.id == id) {
                                        c.folders.retain(|x| *x != folder);
                                    }
                                })
                            })),
                    ),
            );
        }
        let id = c.id.clone();
        row.child(
            outline_button(&format!("ai-add-folder:{}", c.name), &tr!("Add folder…"))
                .h(px(24.))
                .text_size(ui(12.))
                .on_click(cx.listener(move |v, _, _, cx| v.add_folder(id.clone(), cx))),
        )
    }

    /// Asks for a folder the client `id` may copy files from and to.
    fn add_folder(&mut self, id: String, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("Let the AI client use this folder").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(folder) = paths.into_iter().next()
            {
                let folder = folder.display().to_string();
                let _ = this.update(cx, |v, cx| {
                    v.change(cx, |s| {
                        for c in s.clients.iter_mut().filter(|c| c.id == id) {
                            if !c.folders.contains(&folder) {
                                c.folders.push(folder.clone());
                            }
                        }
                    })
                });
            }
        })
        .detach();
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
            paused: false,
            folders: vec![],
        };
        self.change(cx, |s| s.clients.push(client));
        self.adding = false;
        self.showing = None;
        self.token = Some((id, token));
    }

    /// Opens the form that gives hosts, with `hosts` (specs) ticked: the
    /// client too, when there is only one.
    pub fn start_giving(
        &mut self,
        hosts: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self
            .workspace
            .upgrade()
            .map(|w| w.read(cx).ai_snapshot(cx).state)
            .unwrap_or_default();
        let client = match state.clients.as_slice() {
            [only] => Some(only.id.clone()),
            _ => None,
        };
        self.giving = Some(Giving {
            hosts,
            client,
            until: Until::TakenBack,
            mode: state.default_mode,
            how: AiHow::Background,
            may: [true, true, false, false, false, false],
        });
        window.focus(&self.filter.focus_handle(cx), cx);
        cx.notify();
    }

    /// Gives each host ticked, a grant of its own; one the client held
    /// already is replaced.
    fn give(&mut self, cx: &mut Context<Self>) {
        let Some(g) = &self.giving else {
            return;
        };
        let (hosts, Some(client)) = (g.hosts.clone(), g.client.clone()) else {
            return;
        };
        if hosts.is_empty() {
            return;
        }
        let (until, session) = match g.until {
            Until::TakenBack => (None, false),
            Until::Session => (None, true),
            Until::Hour => (Some(ai::now() + 3600), false),
            Until::EightHours => (Some(ai::now() + 8 * 3600), false),
        };
        let grants: Vec<AiGrant> = hosts
            .iter()
            .map(|host| AiGrant {
                client: client.clone(),
                host: host.clone(),
                how: g.how,
                mode: g.mode,
                until,
                session,
                run: g.may[0],
                read: g.may[1],
                write: g.may[2],
                notes_read: g.may[3],
                notes_add: g.may[4],
                snippets: g.may[5],
                paused: false,
            })
            .collect();
        self.change(cx, move |s| {
            s.grants
                .retain(|g| !(g.client == client && hosts.contains(&g.host)));
            s.grants.extend(grants);
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
                        .min_w_0()
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
            let on = snap.connected.contains(&c.id) && !c.paused;
            let (id, id2, id3, id4) = (c.id.clone(), c.id.clone(), c.id.clone(), c.id.clone());
            let paused = c.paused;
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
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(paused, |d| d.text_color(rgb(colors().muted)))
                                    .child(c.name.clone()),
                            )
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
                            .child(if paused {
                                tr!("paused")
                            } else if on {
                                tr!("connected now")
                            } else {
                                tr!("not connected")
                            }),
                    )
                    .child(
                        outline_button(
                            &format!("ai-pause-client:{}", c.name),
                            &if paused { tr!("Resume") } else { tr!("Pause") },
                        )
                        .h(px(28.))
                        .text_size(ui(12.))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            let id = id4.clone();
                            v.change(cx, |s| {
                                for c in s.clients.iter_mut().filter(|c| c.id == id) {
                                    c.paused = !paused;
                                }
                            })
                        })),
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
            list = list.child(self.folders_part(c, cx));
            if shown {
                let token = match &self.token {
                    Some((t, token)) if *t == id3 => Some(token.clone()),
                    _ => None,
                };
                list = list.child(self.config_part(c, token, &snap.exe));
            }
        }
        part.child(list).when(!snap.state.clients.is_empty(), |d| {
            d.child(hint_text(&tr!(
                "Pause stops a client for now, and keeps its token and the hosts given it. Remove takes its token back."
            )))
        })
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
        let mut part = div().flex().flex_col().gap(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(heading(&tr!("Hosts given")))
                        .child(hint_text(&tr!(
                            "A client reaches only these. Its commands run in the background, on a connection of their own, and each is kept in AI activity."
                        ))),
                )
                .child(
                    outline_button("ai-open-activity", &tr!("AI activity")).on_click(cx.listener(
                        |v, _, window, cx| {
                            if let Some(ws) = v.workspace.upgrade() {
                                ws.update(cx, |ws, cx| {
                                    ws.close_settings(window, cx);
                                    ws.open_activity(window, cx);
                                });
                            }
                        },
                    )),
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
                        primary_button("ai-give", &tr!("Give hosts…")).on_click(cx.listener(
                            |v, _, window, cx| v.start_giving(vec![], window, cx),
                        )),
                    )
                }),
        );
        if self.giving.is_some() {
            part = part.child(self.give_part(snap, window, cx));
        }
        if snap.state.grants.is_empty() {
            return part.child(
                div()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .bg(rgb(colors().surface))
                    .px(px(14.))
                    .py(px(12.))
                    .child(hint_text(&tr!("No host is given to an AI client."))),
            );
        }
        // A card for each client, its hosts under it.
        for c in &snap.state.clients {
            let rows: Vec<(usize, &AiGrant)> = snap
                .state
                .grants
                .iter()
                .enumerate()
                .filter(|(_, g)| g.client == c.id)
                .collect();
            if rows.is_empty() {
                continue;
            }
            let open = !self.folded.contains(&c.id);
            let own = rows.iter().filter(|(_, g)| g.paused).count();
            let mut summary = tr_n!(
                rows.len(),
                "{n} host",
                "{n} hosts",
                n = rows.len().to_string()
            );
            if own > 0 && !c.paused {
                summary = format!(
                    "{summary} · {}",
                    tr_n!(own, "{n} paused", "{n} paused", n = own.to_string())
                );
            }
            let (fold_id, take_id) = (c.id.clone(), c.id.clone());
            let mut card = div()
                .debug_selector({
                    let n = c.name.clone();
                    move || format!("ai-granted:{n}")
                })
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(rgb(colors().line))
                .bg(rgb(colors().surface))
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .h(px(44.))
                        .pl(px(6.))
                        .pr(px(10.))
                        .bg(rgb(colors().page))
                        .child(
                            div()
                                .id(SharedString::from(format!("ai-fold:{}", c.id)))
                                .debug_selector({
                                    let n = c.name.clone();
                                    move || format!("ai-fold:{n}")
                                })
                                .flex_1()
                                .min_w_0()
                                .h(px(36.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(colors().hover)))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    if !v.folded.remove(&fold_id) {
                                        v.folded.insert(fold_id.clone());
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    gpui::svg()
                                        .path(if open {
                                            "icons/chevron-down.svg"
                                        } else {
                                            "icons/chevron-right.svg"
                                        })
                                        .size(px(14.))
                                        .flex_shrink_0()
                                        .text_color(rgb(colors().muted)),
                                )
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(c.name.clone()),
                                )
                                .child(hint_text(&summary))
                                .when(c.paused, |d| d.child(chip(&tr!("Client paused")))),
                        )
                        .child(
                            outline_button(
                                &format!("ai-take-all:{}", c.name),
                                &tr!("Take all back"),
                            )
                            .h(px(26.))
                            .text_size(ui(12.))
                            .text_color(rgb(colors().bad))
                            .on_click(cx.listener(
                                move |v, _, _, cx| {
                                    let id = take_id.clone();
                                    v.change(cx, |s| s.grants.retain(|g| g.client != id))
                                },
                            )),
                        ),
                );
            if open {
                for (i, g) in rows {
                    card = card.child(self.grant_row(i, g, &name_of(&g.host), c.paused, cx));
                }
            }
            part = part.child(card);
        }
        part
    }

    /// One host given, in its client's card: dimmed while it, or the
    /// client (`client_paused`), is paused.
    fn grant_row(
        &self,
        i: usize,
        g: &AiGrant,
        host: &str,
        client_paused: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut how = vec![];
        if g.run {
            how.push(match g.how {
                AiHow::Background => tr!("in the background"),
                AiHow::Tab => tr!("through a tab"),
            });
        }
        if g.read {
            how.push(tr!("reads files"));
        }
        if g.write {
            how.push(tr!("writes files"));
        }
        if g.notes_read {
            how.push(tr!("reads notes"));
        }
        if g.notes_add {
            how.push(tr!("adds to notes"));
        }
        if g.snippets {
            how.push(tr!("uses snippets"));
        }
        let until = match (g.until, g.session) {
            (_, true) => tr!("while shellwing stays open"),
            (None, _) => tr!("until taken back"),
            (Some(t), _) if t <= ai::now() => tr!("ended"),
            (Some(t), _) => tr!(
                "for {n} min more",
                n = (t.saturating_sub(ai::now()) / 60 + 1).to_string()
            ),
        };
        // What it may do and for how long, as one text that wraps: the row
        // grows with it, however many rights are ticked.
        how.push(until);
        let how = how.join(", ");
        let dim = g.paused || client_paused;
        let (client, spec, mode, paused) = (g.client.clone(), g.host.clone(), g.mode, g.paused);
        let (client2, spec2) = (client.clone(), spec.clone());
        let (client3, spec3) = (client.clone(), spec.clone());
        div()
            .debug_selector({
                let h = host.to_string();
                move || format!("ai-grant:{h}")
            })
            .flex()
            .items_center()
            .gap(px(12.))
            .min_h(px(44.))
            .px(px(14.))
            .py(px(6.))
            .border_t_1()
            .border_color(rgb(colors().line))
            .when(dim, |d| d.bg(rgb(colors().page)))
            .child(
                div()
                    .w(px(190.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(dim, |d| d.text_color(rgb(colors().muted)))
                    .child(host.to_string())
                    .when(paused, |d| {
                        d.child(
                            div()
                                .debug_selector({
                                    let h = host.to_string();
                                    move || format!("ai-grant-paused:{h}")
                                })
                                .child(chip(&tr!("Paused"))),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector({
                        let h = host.to_string();
                        move || format!("ai-grant-how:{h}")
                    })
                    .flex_1()
                    .min_w_0()
                    .text_color(rgb(colors().muted))
                    .child(how),
            )
            .child(
                outline_button(&format!("ai-mode:{i}"), &mode_text(g.mode).0)
                    .h(px(26.))
                    .flex_shrink_0()
                    .text_size(ui(12.))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        let next =
                            MODES[(MODES.iter().position(|m| *m == mode).unwrap_or(0) + 1) % 3];
                        let (client, spec) = (client.clone(), spec.clone());
                        v.change(cx, move |s| {
                            for g in s.grants.iter_mut() {
                                if g.client == client && g.host == spec {
                                    g.mode = next;
                                }
                            }
                        })
                    })),
            )
            .child(
                outline_button(
                    &format!("ai-pause:{i}"),
                    &if paused { tr!("Resume") } else { tr!("Pause") },
                )
                .h(px(26.))
                .flex_shrink_0()
                .text_size(ui(12.))
                .on_click(cx.listener(move |v, _, _, cx| {
                    let (client, spec) = (client2.clone(), spec2.clone());
                    v.change(cx, move |s| {
                        for g in s.grants.iter_mut() {
                            if g.client == client && g.host == spec {
                                g.paused = !paused;
                            }
                        }
                    })
                })),
            )
            .child(
                outline_button(&format!("ai-take-back:{i}"), &tr!("Take back"))
                    .h(px(26.))
                    .flex_shrink_0()
                    .text_size(ui(12.))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        let (client, spec) = (client3.clone(), spec3.clone());
                        v.change(cx, |s| {
                            s.grants.retain(|g| !(g.client == client && g.host == spec))
                        })
                    })),
            )
    }

    fn give_part(&self, snap: &AiSnap, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let Some(g) = &self.giving else {
            return div();
        };
        let query = self.filter.read(cx).text().trim().to_lowercase();
        // What the client chosen holds already: giving again replaces it.
        let held: Vec<&str> = g
            .client
            .as_ref()
            .map(|c| {
                snap.state
                    .grants
                    .iter()
                    .filter(|x| x.client == *c)
                    .map(|x| x.host.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let mut hosts = div()
            .id("ai-hosts")
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(300.))
            .overflow_y_scroll();
        for p in &snap.picks {
            let shown: Vec<_> = p
                .hosts
                .iter()
                .filter(|h| {
                    query.is_empty()
                        || h.name.to_lowercase().contains(&query)
                        || h.detail.to_lowercase().contains(&query)
                        || p.group.to_lowercase().contains(&query)
                })
                .collect();
            if shown.is_empty() {
                continue;
            }
            let specs: Vec<String> = shown.iter().map(|h| h.spec.clone()).collect();
            let every = specs.iter().all(|s| g.hosts.contains(s));
            hosts = hosts.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .px(px(8.))
                    .child(
                        div()
                            .text_size(ui(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(p.group.clone()),
                    )
                    .child(hint_text(&shown.len().to_string()))
                    .child(div().flex_1())
                    .child(
                        outline_button(
                            &format!("ai-pick-group:{}", p.group),
                            &if every {
                                tr!("Clear group")
                            } else {
                                tr!("Select all")
                            },
                        )
                        .h(px(24.))
                        .text_size(ui(11.5))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if let Some(g) = &mut v.giving {
                                if every {
                                    g.hosts.retain(|s| !specs.contains(s));
                                } else {
                                    for s in &specs {
                                        if !g.hosts.contains(s) {
                                            g.hosts.push(s.clone());
                                        }
                                    }
                                }
                            }
                            cx.notify();
                        })),
                    ),
            );
            for h in shown {
                let on = g.hosts.contains(&h.spec);
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
                        .h(px(34.))
                        .px(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(on, |d| d.bg(rgb(colors().selected)))
                        .hover(|s| s.bg(rgb(colors().hover)))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if let Some(g) = &mut v.giving {
                                match g.hosts.iter().position(|s| *s == spec) {
                                    Some(i) => {
                                        g.hosts.remove(i);
                                    }
                                    None => g.hosts.push(spec.clone()),
                                }
                            }
                            cx.notify();
                        }))
                        .child(crate::host_editor::checkbox(on))
                        .child(div().font_weight(FontWeight::MEDIUM).child(h.name.clone()))
                        .child(div().flex_1().min_w_0().child(hint_text(&h.detail)))
                        .when(held.contains(&h.spec.as_str()), |d| {
                            d.child(chip(&tr!("given already")))
                        }),
                );
            }
        }
        let name_of = |spec: &String| {
            snap.picks
                .iter()
                .flat_map(|p| &p.hosts)
                .find(|h| h.spec == *spec)
                .map_or_else(|| spec.clone(), |h| h.name.clone())
        };
        let n = g.hosts.len();
        let chosen = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(ui(12.))
                    .child(if n == 0 {
                        tr!("No host chosen yet.")
                    } else {
                        format!(
                            "{}: {}",
                            tr_n!(n, "{n} host chosen", "{n} hosts chosen", n = n.to_string()),
                            g.hosts.iter().map(name_of).collect::<Vec<_>>().join(", ")
                        )
                    }),
            )
            .when(n > 0, |d| {
                d.child(
                    outline_button("ai-pick-clear", &tr!("Clear"))
                        .h(px(24.))
                        .text_size(ui(11.5))
                        .on_click(cx.listener(|v, _, _, cx| {
                            if let Some(g) = &mut v.giving {
                                g.hosts.clear();
                            }
                            cx.notify();
                        })),
                )
            });
        let replaced: Vec<String> = g
            .hosts
            .iter()
            .filter(|s| held.contains(&s.as_str()))
            .map(name_of)
            .collect();
        let client_name = g
            .client
            .as_ref()
            .and_then(|c| snap.state.clients.iter().find(|x| x.id == *c))
            .map(|c| c.name.clone())
            .unwrap_or_default();
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
        let ready = n > 0 && g.client.is_some() && g.may.iter().any(|m| *m);
        let give_label = if n == 0 {
            tr!("Give access")
        } else {
            tr_n!(
                n,
                "Give access to {n} host",
                "Give access to {n} hosts",
                n = n.to_string()
            )
        };
        let focused = self.filter.focus_handle(cx).is_focused(window);
        card()
            .debug_selector(|| "ai-give-form".into())
            .child(label_text(&tr!("Hosts")))
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
            .child(chosen)
            .child(label_text(&tr!("To")))
            .child(clients)
            .child(label_text(&tr!("For")))
            .child(untils)
            .child(label_text(&tr!("How")))
            .child({
                let mut hows = div().flex().gap(px(10.));
                for (h, name, what, sel) in [
                    (
                        AiHow::Background,
                        tr!("In the background"),
                        tr!("On a connection of its own, with no tab. Each command and its output are in AI activity."),
                        "ai-give-how:background",
                    ),
                    (
                        AiHow::Tab,
                        tr!("Through a tab"),
                        tr!("Typed into a tab to the host, in its shell, where you see it; a tab opens if none is. A password the host asks for there is yours to type."),
                        "ai-give-how:tab",
                    ),
                ] {
                    let on = g.how == h;
                    hows = hows.child(
                        div()
                            .id(sel)
                            .debug_selector(move || sel.into())
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .p(px(12.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(rgb(if on { colors().accent } else { colors().line }))
                            .bg(rgb(if on { colors().selected } else { colors().surface }))
                            .cursor_pointer()
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(g) = &mut v.giving {
                                    g.how = h;
                                }
                                cx.notify();
                            }))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                            .child(hint_text(&what)),
                    );
                }
                hows
            })
            .child(label_text(&tr!("It may")))
            .child({
                let mut may = div().flex().flex_col().gap(px(6.));
                for (i, (label, what, sel)) in [
                    (tr!("Run commands"), tr!("and see what they print"), "ai-may:run"),
                    (tr!("Read files"), tr!("list folders and read files, over SFTP"), "ai-may:read"),
                    (
                        tr!("Write files"),
                        tr!("over SFTP; where risky ones wait, every write waits for you"),
                        "ai-may:write",
                    ),
                    (
                        tr!("Read notes"),
                        tr!("the notes about the host, but never a sensitive one"),
                        "ai-may:notes-read",
                    ),
                    (
                        tr!("Add to notes"),
                        tr!("a new note about the host, or more at the end of one; never changes what is there"),
                        "ai-may:notes-add",
                    ),
                    (
                        tr!("Use snippets"),
                        tr!("list the snippets offered for the host, and run one where it may run commands"),
                        "ai-may:snippets",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let on = g.may[i];
                    may = may.child(
                        div()
                            .id(sel)
                            .debug_selector(move || sel.into())
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(g) = &mut v.giving {
                                    g.may[i] = !g.may[i];
                                }
                                cx.notify();
                            }))
                            .child(crate::host_editor::checkbox(on))
                            .child(div().font_weight(FontWeight::MEDIUM).child(label))
                            .child(hint_text(&what)),
                    );
                }
                may
            })
            .child(label_text(&tr!("Commands")))
            .child({
                let mut modes = div().flex().flex_wrap().gap(px(6.));
                for m in MODES {
                    let on = g.mode == m;
                    modes = modes.child(
                        outline_button(&format!("ai-give-mode:{m:?}"), &mode_text(m).0)
                            .h(px(28.))
                            .text_size(ui(12.))
                            .when(on, |d| d.bg(rgb(colors().selected)))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(g) = &mut v.giving {
                                    g.mode = m;
                                }
                                cx.notify();
                            })),
                    );
                }
                modes
            })
            .when(!replaced.is_empty(), |d| {
                d.child(
                    div()
                        .debug_selector(|| "ai-give-replaces".into())
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(rgb(colors().chip))
                        .text_size(ui(12.))
                        .child(tr!(
                            "Given to {client} already: {hosts}. Giving them again replaces what they had.",
                            client = client_name.clone(),
                            hosts = replaced.join(", ")
                        )),
                )
            })
            .child(hint_text(&tr!(
                "It never sees a key, a password or a passphrase: a host that would ask for one is refused, and one whose key is not known yet waits for you to trust it. A host's output can carry words meant to steer an agent: give hosts to one you trust with them."
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
                        primary_button("ai-give-ok", &give_label)
                            .on_click(cx.listener(|v, _, _, cx| v.give(cx)))
                    } else {
                        outline_button("ai-give-ok", &give_label)
                            .text_color(rgb(colors().muted))
                    }),
            )
    }
}

impl AiView {
    /// What a new grant does with a command, and the risky list.
    fn modes_part(&self, snap: &AiSnap, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let mut cards = div().flex().gap(px(10.));
        for m in MODES {
            let on = snap.state.default_mode == m;
            let (name, what) = mode_text(m);
            cards = cards.child(
                div()
                    .id(SharedString::from(format!("ai-default:{m:?}")))
                    .debug_selector(move || format!("ai-default:{m:?}"))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(rgb(if on { colors().accent } else { colors().line }))
                    .bg(rgb(if on {
                        colors().selected
                    } else {
                        colors().surface
                    }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, _, _, cx| v.change(cx, |s| s.default_mode = m)))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                    .child(hint_text(&what)),
            );
        }
        let focused = self.risky.focus_handle(cx).is_focused(window);
        let usual = snap.state.risky.is_none();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(&tr!("When AI runs a command")))
                    .child(hint_text(&tr!(
                        "For hosts given from now on; each can be set apart in the table above. Every command is kept in AI activity, whichever this is."
                    ))),
            )
            .child(cards)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label_text(&tr!(
                        "Risky: a command with any of these, a line each, waits for you"
                    )))
                    .child(div().flex_1())
                    .when(!usual, |d| {
                        d.child(
                            outline_button("ai-risky-usual", &tr!("The usual list"))
                                .h(px(26.))
                                .text_size(ui(12.))
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.change(cx, |s| s.risky = None);
                                    let words = shellwing_config::DEFAULT_RISKY.join("\n");
                                    v.risky.update(cx, |i, cx| i.set_text(&words, 0..0, cx));
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector(|| "ai-risky".into())
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(if focused {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().field))
                    .font_family(self.mono.clone())
                    .text_size(ui(12.5))
                    .child(self.risky.clone()),
            )
    }
}

fn heading(s: &str) -> gpui::Div {
    div()
        .text_size(ui(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(s.to_string())
}

/// A small grey label: what is paused.
fn chip(text: &str) -> gpui::Div {
    div()
        .flex_shrink_0()
        .h(px(20.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded_full()
        .bg(rgb(colors().chip))
        .text_color(rgb(colors().text))
        .text_size(ui(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.to_string())
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
            .child(self.clients_part(&snap, cx))
            .child(self.grants_part(&snap, window, cx))
            .child(self.modes_part(&snap, window, cx))
            .child(self.sudo_part(window, cx))
            .child(self.activity_part(&snap, window, cx))
    }
}

impl AiView {
    /// How much AI activity this computer keeps, in all.
    fn activity_part(&self, snap: &AiSnap, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let mb = snap
            .state
            .activity_mb
            .unwrap_or(shellwing_config::AI_ACTIVITY_MB);
        let typed = self.activity_mb.read(cx).text().trim().to_string();
        let wrong = typed.parse::<u32>().map_or(true, |n| n < LEAST_MB);
        let focused = self.activity_mb.focus_handle(cx).is_focused(window);
        let step = |id: &'static str, label: &'static str, to: u32, cx: &mut Context<Self>| {
            outline_button(id, label)
                .h(px(30.))
                .w(px(30.))
                .px(px(0.))
                .justify_center()
                .on_click(cx.listener(move |v, _, _, cx| v.set_activity_mb(to, cx)))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(&tr!("AI activity")))
                    .child(hint_text(&tr!(
                        "Kept on this computer, as plain text, in parts of {part} MB: what each command printed, its first 64 KB. When they are more than this, the oldest part goes. AI activity shows the last 1000, and clears them all.",
                        part = (ai::log::PART / 1024 / 1024).to_string()
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(step(
                        "ai-activity-less",
                        "−",
                        mb.saturating_sub(STEP_MB).max(LEAST_MB),
                        cx,
                    ))
                    .child(
                        div()
                            .debug_selector(|| "ai-activity-mb".into())
                            .w(px(90.))
                            .h(px(30.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(if wrong {
                                colors().bad
                            } else if focused {
                                colors().accent
                            } else {
                                colors().field_edge
                            }))
                            .bg(rgb(colors().field))
                            .child(self.activity_mb.clone()),
                    )
                    .child(step(
                        "ai-activity-more",
                        "+",
                        mb.saturating_add(STEP_MB),
                        cx,
                    ))
                    .child(label_text("MB"))
                    .when(wrong, |d| {
                        d.child(
                            div()
                                .text_size(ui(12.))
                                .text_color(rgb(colors().bad_text))
                                .child(tr!(
                                    "At least {least} MB: {mb} MB is kept until then.",
                                    least = LEAST_MB.to_string(),
                                    mb = mb.to_string()
                                )),
                        )
                    }),
            )
    }

    /// Keeps `mb` (at least [`LEAST_MB`]) and shows it.
    /// How long sudo's password, given for an agent's command, may be kept
    /// for that client on that host.
    fn sudo_part(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let typed = self.sudo_minutes.read(cx).text().trim().to_string();
        let wrong = typed
            .parse::<u32>()
            .map_or(true, |n| n > shellwing_config::MOST_SUDO_MINUTES);
        let focused = self.sudo_minutes.focus_handle(cx).is_focused(window);
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(heading(&tr!("sudo's password")))
                    .child(hint_text(&tr!(
                        "An agent's command that needs sudo's password waits for you to type it, every time; it never sees it. Allowed, it may be kept for that agent on that host this long, at most {most} minutes; 0: never kept.",
                        most = shellwing_config::MOST_SUDO_MINUTES.to_string()
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .debug_selector(|| "ai-sudo-minutes".into())
                            .w(px(90.))
                            .h(px(30.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(if wrong {
                                colors().bad
                            } else if focused {
                                colors().accent
                            } else {
                                colors().field_edge
                            }))
                            .bg(rgb(colors().field))
                            .child(self.sudo_minutes.clone()),
                    )
                    .child(label_text(&tr!("minutes"))),
            )
    }

    fn set_activity_mb(&mut self, mb: u32, cx: &mut Context<Self>) {
        let mb = mb.max(LEAST_MB);
        self.activity_mb
            .update(cx, |i, cx| i.set_text(&mb.to_string(), 0..0, cx));
        self.change(cx, |s| {
            s.activity_mb = (mb != shellwing_config::AI_ACTIVITY_MB).then_some(mb)
        });
    }
}

/// The least AI activity keeps, in MB.
const LEAST_MB: u32 = ai::log::LEAST_MB;

/// What − and + change it by, in MB.
const STEP_MB: u32 = 32;

#[cfg(test)]
mod tests {
    use super::*;

    /// Each client's config parses as its own format, and holds the path and
    /// the token where the client reads them.
    #[test]
    fn every_config_is_well_formed() {
        let exe = r"C:\Program Files\shellwing\shellwing.exe";
        for (kind, _) in KINDS {
            let (_, text) = config_for(kind, exe, "shw_abc");
            assert!(text.contains("shw_abc"), "{kind}");
            match kind {
                "claude-code" => assert!(text.contains("\"C:\\Program Files")),
                "codex" => {
                    let v: toml::Value = toml::from_str(&text).expect("toml");
                    assert_eq!(v["mcp_servers"]["shellwing"]["command"].as_str(), Some(exe));
                    assert_eq!(
                        v["mcp_servers"]["shellwing"]["env"]["SHELLWING_TOKEN"].as_str(),
                        Some("shw_abc")
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
                    assert_eq!(v[key]["shellwing"]["env"]["SHELLWING_TOKEN"], "shw_abc");
                }
            }
        }
    }
}

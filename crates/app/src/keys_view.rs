//! The Keys page of Settings: keys the app keeps (in the vault, synced to
//! the other devices), key files in `~/.ssh`, and keys in the ssh-agent.
//! Keys are made, copied into the vault, written out to files, renamed and
//! deleted here; installing one on a host goes through the workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    Pixels, Point, Render, SharedString, WeakEntity, Window, anchored, deferred, div, point,
    prelude::*, px, rgb, rgba,
};
use shellwing_ssh::keys::{self, KeyFile, KeyInfo, KeyKind, ReadError};

use crate::host_editor::{hint_text, label_text, outline_button, primary_button, problem_text};
use crate::input::TextInput;
use crate::look::ui;
use crate::secrets::VaultSlot;
use crate::workspace::{KeyToInstall, Workspace};

const TEXT: u32 = 0xe2e8f0;
const SOFT: u32 = 0xb6c0cf;
const LABEL: u32 = 0xa3aec0;
const MUTED: u32 = 0x8a96aa;
const RULE: u32 = 0x1f2937;
const FIELD_EDGE: u32 = 0x263042;
const ACCENT: u32 = 0x38bdf8;
const ON: u32 = 0x17233a;
const CARD: u32 = 0x0f1522;
const BAD: u32 = 0xf87171;
const GOOD: u32 = 0x86efac;
const WARN: u32 = 0xfbbf24;

/// A key the vault keeps, as listed.
#[derive(Clone)]
struct Kept {
    id: String,
    name: String,
    info: KeyInfo,
}

/// A key file in `~/.ssh`, as listed.
#[derive(Clone)]
struct FileKey {
    file: KeyFile,
    /// Its passphrase is in the vault.
    saved: bool,
    /// The same key is kept in the vault.
    kept: bool,
}

/// What the page lists, read off the UI thread.
#[derive(Default)]
struct Listed {
    kept: Vec<Kept>,
    files: Vec<FileKey>,
    /// Why the vault could not be read.
    problem: Option<String>,
}

enum Dialog {
    Generate {
        name: Entity<TextInput>,
        comment: Entity<TextInput>,
        kind: KeyKind,
        problem: Option<String>,
        busy: bool,
    },
    Generated {
        kept: Kept,
    },
    Rename {
        id: String,
        input: Entity<TextInput>,
    },
    Delete {
        kept: Kept,
    },
    Export {
        kept: Kept,
        pass: Entity<TextInput>,
        again: Entity<TextInput>,
        problem: Option<String>,
    },
    /// A key file with a passphrase, being copied into the vault.
    Unlock {
        path: PathBuf,
        pass: Entity<TextInput>,
        problem: Option<String>,
    },
}

pub struct KeysView {
    workspace: WeakEntity<Workspace>,
    vault: VaultSlot,
    rt: tokio::runtime::Handle,
    focus: FocusHandle,
    listed: Listed,
    /// The agent's keys; None while still asking.
    agent: Option<Vec<KeyInfo>>,
    /// The hosts that sign in with each kept key, by its id.
    users: BTreeMap<String, Vec<String>>,
    /// The ⋯ menu of a kept key: where, and which.
    menu: Option<(Point<Pixels>, usize)>,
    dialog: Option<Dialog>,
    /// What just happened, under the title; true when it went wrong.
    notice: Option<(SharedString, bool)>,
    /// The terminal's font, for key file names, comments and public keys.
    mono: SharedString,
}

fn ssh_dir() -> Option<PathBuf> {
    std::env::home_dir().map(|h| h.join(".ssh"))
}

/// This computer's name, for a new key's name and comment.
fn machine() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "this computer".into())
}

fn user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

fn tilde(p: &Path) -> String {
    match std::env::home_dir().and_then(|h| p.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// A name that suits a file: letters, digits, `-` and `_`.
fn file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("id_{}", s.trim_matches('_'))
}

fn kind_label(k: KeyKind) -> &'static str {
    match k {
        KeyKind::Ed25519 => "Ed25519 (recommended)",
        KeyKind::EcdsaP256 => "ECDSA P-256",
        KeyKind::Rsa4096 => "RSA 4096",
    }
}

fn read_listed(vault: &VaultSlot) -> Listed {
    let mut listed = Listed::default();
    let v = vault.get();
    match &v {
        Some(v) => match v.ssh_keys() {
            Ok(ks) => {
                listed.kept = ks
                    .into_iter()
                    .filter_map(|k| {
                        let public = keys::ssh_key::PublicKey::from_openssh(&k.public).ok()?;
                        Some(Kept {
                            id: k.id,
                            name: k.name,
                            info: keys::info(&public),
                        })
                    })
                    .collect();
            }
            Err(e) => listed.problem = Some(e.to_string()),
        },
        None => listed.problem = vault.problem(),
    }
    let kept: Vec<String> = listed
        .kept
        .iter()
        .map(|k| k.info.fingerprint.clone())
        .collect();
    listed.files = ssh_dir()
        .map(|d| keys::key_files(&d))
        .unwrap_or_default()
        .into_iter()
        .map(|file| FileKey {
            saved: v.as_ref().is_some_and(|v| {
                v.passphrase(&file.info.fingerprint)
                    .ok()
                    .flatten()
                    .is_some()
            }),
            kept: kept.contains(&file.info.fingerprint),
            file,
        })
        .collect();
    listed
}

impl KeysView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: VaultSlot,
        rt: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut v = KeysView {
            workspace,
            vault,
            rt,
            focus: cx.focus_handle(),
            listed: Listed::default(),
            agent: None,
            users: BTreeMap::new(),
            menu: None,
            dialog: None,
            notice: None,
            mono: SharedString::default(),
        };
        v.refresh(cx);
        v
    }

    /// Reads the keys again: the vault and the files off the UI thread, the
    /// agent on the network runtime.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.users = self
            .workspace
            .upgrade()
            .map(|w| w.read(cx).key_users())
            .unwrap_or_default();
        let vault = self.vault.clone();
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_executor()
                .spawn(async move { read_listed(&vault) })
                .await;
            let _ = this.update(cx, |v, cx| {
                v.listed = listed;
                cx.notify();
            });
        })
        .detach();
        let asked = self.rt.spawn(keys::agent_keys(None));
        cx.spawn(async move |this, cx| {
            let found = asked.await.unwrap_or_default();
            let _ = this.update(cx, |v, cx| {
                v.agent = Some(found);
                cx.notify();
            });
        })
        .detach();
    }

    fn say(&mut self, msg: impl Into<SharedString>, bad: bool, cx: &mut Context<Self>) {
        self.notice = Some((msg.into(), bad));
        cx.notify();
    }

    fn input(
        placeholder: &str,
        text: &str,
        masked: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        cx.new(|cx| {
            let mut i = TextInput::new(placeholder.to_string(), cx);
            if masked {
                i = i.masked();
            }
            i.set_text(text, 0..text.len(), cx);
            i
        })
    }

    /// Opens a dialog, the keyboard in its first field.
    fn open(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        let first = match &dialog {
            Dialog::Generate { name, .. } => Some(name.clone()),
            Dialog::Rename { input, .. } => Some(input.clone()),
            Dialog::Export { pass, .. } | Dialog::Unlock { pass, .. } => Some(pass.clone()),
            Dialog::Generated { .. } | Dialog::Delete { .. } => None,
        };
        self.dialog = Some(dialog);
        self.menu = None;
        match first {
            Some(i) => {
                let f = i.read(cx).focus_handle(cx);
                window.focus(&f, cx);
            }
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn generate_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = Self::input("A name, to tell your keys apart", &machine(), false, cx);
        let comment = Self::input("user@host", &format!("{}@{}", user(), machine()), false, cx);
        self.open(
            Dialog::Generate {
                name,
                comment,
                kind: KeyKind::Ed25519,
                problem: None,
                busy: false,
            },
            window,
            cx,
        );
    }

    fn generate(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Generate {
            name,
            comment,
            kind,
            problem,
            busy,
        }) = &mut self.dialog
        else {
            return;
        };
        if *busy {
            return;
        }
        let n = name.read(cx).text().trim().to_string();
        if n.is_empty() {
            *problem = Some("A name is needed.".into());
            return cx.notify();
        }
        let (c, kind) = (comment.read(cx).text().trim().to_string(), *kind);
        *busy = true;
        *problem = None;
        cx.notify();
        let vault = self.vault.clone();
        // RSA takes a few seconds: off the UI thread.
        let made = cx.background_executor().spawn(async move {
            let key = keys::generate(kind, &c).map_err(|e| e.to_string())?;
            let info = keys::info(key.public_key());
            let private = keys::to_vault(&key).map_err(|e| e.to_string())?;
            let v = vault
                .get()
                .ok_or_else(|| vault.problem().unwrap_or("the vault is not open".into()))?;
            let id = v
                .add_ssh_key(&n, &info.public, &private)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(Kept { id, name: n, info })
        });
        cx.spawn(async move |this, cx| {
            let made = made.await;
            let _ = this.update(cx, |v, cx| {
                match made {
                    Ok(kept) => {
                        v.dialog = Some(Dialog::Generated { kept });
                        v.refresh(cx);
                        v.vault_changed(cx);
                    }
                    Err(e) => {
                        if let Some(Dialog::Generate { problem, busy, .. }) = &mut v.dialog {
                            *problem = Some(e);
                            *busy = false;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The vault changed: sync it.
    fn vault_changed(&self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, _| ws.vault_changed());
        }
    }

    fn copy_public(&mut self, info: &KeyInfo, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(info.public.clone()));
        self.say("Copied the public key.", false, cx);
    }

    fn install(&mut self, key: KeyToInstall, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |ws, cx| ws.choose_host_for_key(key, window, cx));
        }
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Rename { id, input }) = &self.dialog else {
            return;
        };
        let name = input.read(cx).text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let result = self
            .vault
            .get()
            .ok_or_else(|| "the vault is not open".to_string())
            .and_then(|v| v.rename_ssh_key(id, &name).map_err(|e| e.to_string()));
        self.close(window, cx);
        match result {
            Ok(()) => {
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(format!("Not renamed: {e}"), true, cx),
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Delete { kept }) = &self.dialog else {
            return;
        };
        let (id, name) = (kept.id.clone(), kept.name.clone());
        let result = self
            .vault
            .get()
            .ok_or_else(|| "the vault is not open".to_string())
            .and_then(|v| v.delete(&id).map_err(|e| e.to_string()));
        self.close(window, cx);
        match result {
            Ok(()) => {
                self.say(format!("Deleted {name}."), false, cx);
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(format!("Not deleted: {e}"), true, cx),
        }
    }

    /// Asks where to write the key, then writes it and its `.pub`.
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Dialog::Export {
            kept,
            pass,
            again,
            problem,
        }) = &mut self.dialog
        else {
            return;
        };
        let (p, a) = (
            pass.read(cx).text().to_string(),
            again.read(cx).text().to_string(),
        );
        if p != a {
            *problem = Some("The two passphrases differ.".into());
            return cx.notify();
        }
        let kept = kept.clone();
        let private = self
            .vault
            .get()
            .and_then(|v| v.ssh_keys().ok())
            .and_then(|ks| ks.into_iter().find(|k| k.id == kept.id))
            .map(|k| k.private);
        let text = private
            .ok_or_else(|| "the key is not in the vault".to_string())
            .and_then(|pem| {
                keys::parse(&pem, None).map_err(|_| "the key does not read".to_string())
            })
            .and_then(|key| keys::export(&key, Some(&p)).map_err(|e| e.to_string()));
        let text = match text {
            Ok(t) => t,
            Err(e) => {
                if let Some(Dialog::Export { problem, .. }) = &mut self.dialog {
                    *problem = Some(e);
                }
                return cx.notify();
            }
        };
        self.close(window, cx);
        let dir = ssh_dir().unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        let picked = cx.prompt_for_new_path(&dir, Some(&file_name(&kept.name)));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let written = keys::write_files(&path, &text, &kept.info.public);
            let _ = this.update(cx, |v, cx| match written {
                Ok(()) => {
                    v.say(
                        format!("Written to {} and its .pub.", tilde(&path)),
                        false,
                        cx,
                    );
                    v.refresh(cx);
                }
                Err(e) => v.say(format!("Not written: {e}"), true, cx),
            });
        })
        .detach();
    }

    /// Copies a key file into the vault, asking for its passphrase when it
    /// has one the vault does not know.
    fn keep_file(
        &mut self,
        path: PathBuf,
        typed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return self.say(format!("Cannot read {}: {e}", tilde(&path)), true, cx),
        };
        let vault = self.vault.get();
        // The passphrase typed, else the one the vault has for it.
        let saved = || {
            let public = keys::ssh_key::PrivateKey::from_openssh(text.trim()).ok()?;
            let fp = keys::info(public.public_key()).fingerprint;
            vault
                .as_ref()?
                .passphrase(&fp)
                .ok()
                .flatten()
                .map(|p| p.to_string())
        };
        let pass = typed.or_else(saved);
        let key = match keys::parse(&text, pass.as_deref()) {
            Ok(k) => k,
            Err(ReadError::Passphrase) => {
                let wrong = matches!(self.dialog, Some(Dialog::Unlock { .. }));
                let input = Self::input("Its passphrase", "", true, cx);
                return self.open(
                    Dialog::Unlock {
                        path,
                        pass: input,
                        problem: wrong.then(|| "That passphrase does not open it.".into()),
                    },
                    window,
                    cx,
                );
            }
            Err(ReadError::NotAKey(_)) => {
                return self.say(
                    format!("{} is not an OpenSSH private key.", tilde(&path)),
                    true,
                    cx,
                );
            }
        };
        let mut info = keys::info(key.public_key());
        // A locked key's comment is in its .pub.
        if info.comment.is_empty()
            && let Some(f) = self.listed.files.iter().find(|f| f.file.path == path)
        {
            info = f.file.info.clone();
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "key".into());
        let added = keys::to_vault(&key)
            .map_err(|e| e.to_string())
            .and_then(|pem| {
                vault
                    .ok_or_else(|| "the vault is not open".to_string())?
                    .add_ssh_key(&name, &info.public, &pem)
                    .map_err(|e| e.to_string())
            });
        self.dialog = None;
        window.focus(&self.focus, cx);
        match added {
            Ok(_) => {
                self.say(
                    format!("{name} is in the vault now, and syncs to your other devices."),
                    false,
                    cx,
                );
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(format!("Not copied: {e}"), true, cx),
        }
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |v, window, cx| v.keep_file(path, None, window, cx));
        })
        .detach();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        if self.menu.is_some() && key == "escape" {
            self.menu = None;
            cx.notify();
            return cx.stop_propagation();
        }
        if self.dialog.is_none() {
            return;
        }
        match key {
            "escape" => self.close(window, cx),
            "enter" => match &self.dialog {
                Some(Dialog::Generate { .. }) => self.generate(cx),
                Some(Dialog::Rename { .. }) => self.rename(window, cx),
                Some(Dialog::Delete { .. }) => self.delete(window, cx),
                Some(Dialog::Export { .. }) => self.export(window, cx),
                Some(Dialog::Unlock { path, pass, .. }) => {
                    let (path, typed) = (path.clone(), pass.read(cx).text().to_string());
                    self.keep_file(path, Some(typed), window, cx);
                }
                Some(Dialog::Generated { .. }) => self.close(window, cx),
                None => {}
            },
            _ => return,
        }
        cx.stop_propagation();
    }

    fn chip(text: impl Into<SharedString>, fg: u32, bg: u32) -> gpui::Div {
        div()
            .px(px(6.))
            .py(px(1.))
            .rounded(px(4.))
            .text_size(ui(10.5))
            .text_color(rgb(fg))
            .bg(rgb(bg))
            .child(text.into())
    }

    fn kind_chip(kind: &str) -> gpui::Div {
        Self::chip(kind.to_string(), SOFT, 0x111a2a)
            .border_1()
            .border_color(rgb(FIELD_EDGE))
    }

    fn small(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id.into())
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .flex_shrink_0()
            .rounded(px(6.))
            .bg(rgb(0x1a2333))
            .text_size(ui(12.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x243047)))
            .child(label.to_string())
    }

    fn section(title: &str, note: &str, rows: Vec<gpui::AnyElement>, empty: &str) -> gpui::Div {
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(RULE));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(ui(12.))
                    .text_color(rgb(MUTED))
                    .child(empty.to_string()),
            );
        }
        for (i, r) in rows.into_iter().enumerate() {
            list = list.child(
                div()
                    .when(i > 0, |d| d.border_t_1().border_color(rgb(RULE)))
                    .child(r),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(ui(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child(title.to_uppercase()),
                    )
                    .child(
                        div()
                            .text_size(ui(11.5))
                            .text_color(rgb(MUTED))
                            .child(note.to_string()),
                    ),
            )
            .child(list)
    }

    /// One key's row: its name, kind and marks, the fingerprint and more.
    fn row(
        name: gpui::Div,
        marks: Vec<gpui::Div>,
        detail: String,
        actions: gpui::Div,
    ) -> gpui::AnyElement {
        let mut title = div().flex().items_center().gap(px(8.)).child(name);
        for m in marks {
            title = title.child(m);
        }
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(title)
                    .child(
                        div()
                            .text_size(ui(11.5))
                            .text_color(rgb(MUTED))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(detail),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn kept_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        self.listed
            .kept
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let users = self.users.get(&k.id).map(Vec::as_slice).unwrap_or_default();
                let used = match users {
                    [] => "not used by a host yet".to_string(),
                    [one] => format!("used by {one}"),
                    [a, b] => format!("used by {a}, {b}"),
                    more => format!("used by {} hosts", more.len()),
                };
                let mut detail = vec![k.info.fingerprint.clone()];
                if !k.info.comment.is_empty() {
                    detail.push(k.info.comment.clone());
                }
                detail.push(used);
                let (info, install) = (
                    k.info.clone(),
                    KeyToInstall {
                        name: k.name.clone(),
                        public: k.info.public.clone(),
                        vault_id: Some(k.id.clone()),
                    },
                );
                let open = self.menu.is_some_and(|(_, m)| m == i);
                Self::row(
                    div().font_weight(FontWeight::MEDIUM).child(k.name.clone()),
                    vec![Self::kind_chip(&k.info.kind)],
                    detail.join(" · "),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            Self::small(("kept-copy", i), "Copy public key")
                                .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                        )
                        .child(Self::small(("kept-install", i), "Install on…").on_click(
                            cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                        ))
                        .child(
                            Self::small(("kept-more", i), "⋯")
                                .when(open, |d| d.bg(rgb(0x243047)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |v, e: &gpui::MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        v.menu = if v.menu.is_some_and(|(_, m)| m == i) {
                                            None
                                        } else {
                                            Some((e.position, i))
                                        };
                                        cx.notify();
                                    }),
                                ),
                        ),
                )
            })
            .collect()
    }

    fn file_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        self.listed
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let pass = if !f.file.encrypted {
                    Self::chip("no passphrase", WARN, 0x2a2110)
                } else if f.saved {
                    Self::chip("passphrase saved in the vault", GOOD, 0x12291d)
                } else {
                    Self::chip("passphrase", SOFT, 0x111a2a)
                };
                let mut marks = vec![Self::kind_chip(&f.file.info.kind), pass];
                if f.kept {
                    marks.push(Self::chip("also in the vault", SOFT, 0x111a2a));
                }
                let mut detail = vec![f.file.info.fingerprint.clone()];
                if !f.file.info.comment.is_empty() {
                    detail.push(f.file.info.comment.clone());
                }
                let (info, install, path) = (
                    f.file.info.clone(),
                    KeyToInstall {
                        name: tilde(&f.file.path),
                        public: f.file.info.public.clone(),
                        vault_id: None,
                    },
                    f.file.path.clone(),
                );
                let mut actions = div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        Self::small(("file-copy", i), "Copy public key")
                            .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                    )
                    .child(Self::small(("file-install", i), "Install on…").on_click(
                        cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                    ));
                if !f.kept {
                    actions = actions.child(
                        Self::small(("file-keep", i), "Copy to vault")
                            .bg(rgb(0x0b0f17))
                            .border_1()
                            .border_color(rgb(FIELD_EDGE))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                v.keep_file(path.clone(), None, w, cx)
                            })),
                    );
                }
                Self::row(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .font_family(self.mono.clone())
                        .text_size(ui(12.5))
                        .child(
                            f.file
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        ),
                    marks,
                    detail.join(" · "),
                    actions,
                )
            })
            .collect()
    }

    fn agent_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let Some(agent) = &self.agent else {
            return vec![];
        };
        agent
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let same = self
                    .listed
                    .files
                    .iter()
                    .find(|f| f.file.info.fingerprint == k.fingerprint)
                    .map(|f| format!("the same key as {}", tilde(&f.file.path)))
                    .or_else(|| {
                        self.listed
                            .kept
                            .iter()
                            .find(|x| x.info.fingerprint == k.fingerprint)
                            .map(|x| format!("the same key as {} in the vault", x.name))
                    })
                    .unwrap_or_else(|| "only in the agent".into());
                let (info, install) = (
                    k.clone(),
                    KeyToInstall {
                        name: if k.comment.is_empty() {
                            "the agent's key".into()
                        } else {
                            k.comment.clone()
                        },
                        public: k.public.clone(),
                        vault_id: None,
                    },
                );
                Self::row(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(if k.comment.is_empty() {
                            "(no comment)".to_string()
                        } else {
                            k.comment.clone()
                        }),
                    vec![Self::kind_chip(&k.kind)],
                    format!("{} · {same}", k.fingerprint),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            Self::small(("agent-copy", i), "Copy public key")
                                .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                        )
                        .child(Self::small(("agent-install", i), "Install on…").on_click(
                            cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                        )),
                )
            })
            .collect()
    }

    fn menu(
        &self,
        at: Point<Pixels>,
        i: usize,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let k = self.listed.kept.get(i)?.clone();
        let item = |id: &'static str, label: &str, danger: bool| {
            div()
                .id(id)
                .px(px(10.))
                .h(px(30.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(ON)))
                .text_color(rgb(if danger { BAD } else { TEXT }))
                .child(label.to_string())
        };
        let (k1, k2, k3) = (k.clone(), k.clone(), k);
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("key-menu")
                            .w(px(190.))
                            .p(px(4.))
                            .flex()
                            .flex_col()
                            .rounded(px(8.))
                            .bg(rgb(0x111a2a))
                            .border_1()
                            .border_color(rgb(0x2b3648))
                            .shadow_lg()
                            .text_size(ui(13.))
                            .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                                v.menu = None;
                                cx.notify();
                            }))
                            .child(item("menu-export", "Export to file…", false).on_click(
                                cx.listener(move |v, _, w, cx| {
                                    let pass = Self::input("A passphrase, or none", "", true, cx);
                                    let again = Self::input("The passphrase again", "", true, cx);
                                    v.open(
                                        Dialog::Export {
                                            kept: k1.clone(),
                                            pass,
                                            again,
                                            problem: None,
                                        },
                                        w,
                                        cx,
                                    );
                                }),
                            ))
                            .child(item("menu-rename", "Rename…", false).on_click(cx.listener(
                                move |v, _, w, cx| {
                                    let input = Self::input("Name", &k2.name, false, cx);
                                    v.open(
                                        Dialog::Rename {
                                            id: k2.id.clone(),
                                            input,
                                        },
                                        w,
                                        cx,
                                    );
                                },
                            )))
                            .child(div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(rgb(FIELD_EDGE)))
                            .child(item("menu-delete", "Delete…", true).on_click(cx.listener(
                                move |v, _, w, cx| {
                                    v.open(Dialog::Delete { kept: k3.clone() }, w, cx)
                                },
                            ))),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
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
            .child(
                div()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if focused { ACCENT } else { FIELD_EDGE }))
                    .bg(rgb(0x0b0f17))
                    .text_size(ui(13.))
                    .when_some(mono, |d, f| d.font_family(f))
                    .child(input.clone()),
            )
    }

    fn dialog_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let d = self.dialog.as_ref()?;
        let focused = |i: &Entity<TextInput>, window: &Window, cx: &gpui::App| {
            i.read(cx).focus_handle(cx).is_focused(window)
        };
        let buttons = |cancel: Option<&'static str>,
                       ok: (&'static str, &str),
                       cx: &mut Context<Self>| {
            let mut row = div().flex().justify_end().gap(px(8.)).pt(px(2.));
            if let Some(c) = cancel {
                row = row.child(
                    outline_button("dialog-cancel", c)
                        .on_click(cx.listener(|v, _, w, cx| v.close(w, cx))),
                );
            }
            row.child(primary_button(ok.0, ok.1).on_click(cx.listener(
                |v, _, w, cx| match &v.dialog {
                    Some(Dialog::Generate { .. }) => v.generate(cx),
                    Some(Dialog::Rename { .. }) => v.rename(w, cx),
                    Some(Dialog::Delete { .. }) => v.delete(w, cx),
                    Some(Dialog::Export { .. }) => v.export(w, cx),
                    Some(Dialog::Unlock { path, pass, .. }) => {
                        let (path, typed) = (path.clone(), pass.read(cx).text().to_string());
                        v.keep_file(path, Some(typed), w, cx);
                    }
                    Some(Dialog::Generated { .. }) | None => v.close(w, cx),
                },
            )))
        };
        let title = |s: String| {
            div()
                .text_size(ui(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(s)
        };
        let card = div()
            .id("key-dialog")
            .w(px(540.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(24.))
            .py(px(22.))
            .rounded(px(12.))
            .bg(rgb(CARD))
            .border_1()
            .border_color(rgb(0x2b3648))
            .shadow_lg()
            .text_color(rgb(TEXT))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let body = match d {
            Dialog::Generate {
                name,
                comment,
                kind,
                problem,
                busy,
            } => {
                let mut seg = div()
                    .flex()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(FIELD_EDGE))
                    .overflow_hidden();
                for (n, k) in [KeyKind::Ed25519, KeyKind::EcdsaP256, KeyKind::Rsa4096]
                    .into_iter()
                    .enumerate()
                {
                    let on = *kind == k;
                    seg = seg.child(
                        div()
                            .id(("gen-kind", n))
                            .flex_1()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(ui(12.5))
                            .text_color(rgb(if on { TEXT } else { SOFT }))
                            .cursor_pointer()
                            .when(n > 0, |d| d.border_l_1().border_color(rgb(FIELD_EDGE)))
                            .when(on, |d| d.bg(rgb(ON)).border_b_2().border_color(rgb(ACCENT)))
                            .when(!on, |d| d.hover(|s| s.bg(rgb(0x121a28))))
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(Dialog::Generate { kind, .. }) = &mut v.dialog {
                                    *kind = k;
                                }
                                cx.notify();
                            }))
                            .child(kind_label(k)),
                    );
                }
                let mut b = card
                    .child(title("Generate a key".into()))
                    .child(
                        Self::field("Name", name, focused(name, window, cx), None)
                            .child(hint_text("Only for you, to tell your keys apart.")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(label_text("Type"))
                            .child(seg)
                            .child(hint_text(
                                "Ed25519 is short, fast and taken by OpenSSH 6.5 and later. RSA for very old servers.",
                            )),
                    )
                    .child(
                        Self::field("Comment", comment, focused(comment, window, cx), Some(self.mono.clone())).child(
                            hint_text(
                                "Written at the end of the public key: what a server's authorized_keys shows.",
                            ),
                        ),
                    )
                    .child(
                        // A block, not a flex row: text in a row does not wrap.
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(rgb(0x111a2a))
                            .border_1()
                            .border_color(rgb(RULE))
                            .text_size(ui(12.5))
                            .text_color(rgb(SOFT))
                            .child(
                                "Kept in the vault, encrypted, and synced to your other devices, so it needs no passphrase. To use it with ssh or git too, export it to a file, with a passphrase if you like.",
                            ),
                    );
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(
                    Some("Cancel"),
                    ("gen-ok", if *busy { "Generating…" } else { "Generate" }),
                    cx,
                ))
            }
            Dialog::Generated { kept } => {
                let (info, install) = (
                    kept.info.clone(),
                    KeyToInstall {
                        name: kept.name.clone(),
                        public: kept.info.public.clone(),
                        vault_id: Some(kept.id.clone()),
                    },
                );
                let k = kept.clone();
                card.child(title(format!("{} is ready", kept.name)))
                    .child(div().text_size(ui(12.5)).text_color(rgb(LABEL)).child(
                        "A server lets this key in once its public key is in the server's ~/.ssh/authorized_keys. Install it on a host from here, or copy it into a web console (GitHub, GitLab, a cloud provider).",
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .flex()
                                    .child(div().flex_1().child(label_text("Public key")))
                                    .child(
                                        div()
                                            .text_size(ui(11.5))
                                            .text_color(rgb(MUTED))
                                            .child(kept.info.fingerprint.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(8.))
                                    .px(px(12.))
                                    .py(px(10.))
                                    .rounded(px(8.))
                                    .bg(rgb(0x0b0f17))
                                    .border_1()
                                    .border_color(rgb(FIELD_EDGE))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_family(self.mono.clone())
                                            .text_size(ui(12.))
                                            .text_color(rgb(0xcbd5e1))
                                            .child(kept.info.public.clone()),
                                    )
                                    .child(Self::small("gen-copy", "Copy").on_click(cx.listener(
                                        move |v, _, _, cx| v.copy_public(&info, cx),
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(outline_button("gen-export", "Export to file…").on_click(
                                cx.listener(move |v, _, w, cx| {
                                    let pass = Self::input("A passphrase, or none", "", true, cx);
                                    let again = Self::input("The passphrase again", "", true, cx);
                                    v.open(
                                        Dialog::Export {
                                            kept: k.clone(),
                                            pass,
                                            again,
                                            problem: None,
                                        },
                                        w,
                                        cx,
                                    );
                                }),
                            ))
                            .child(div().flex_1())
                            .child(
                                outline_button("gen-done", "Done")
                                    .on_click(cx.listener(|v, _, w, cx| v.close(w, cx))),
                            )
                            .child(primary_button("gen-install", "Install on a host…").on_click(
                                cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                            )),
                    )
            }
            Dialog::Rename { input, .. } => card
                .child(title("Rename the key".into()))
                .child(Self::field("Name", input, true, None))
                .child(buttons(Some("Cancel"), ("rename-ok", "Rename"), cx)),
            Dialog::Delete { kept } => {
                let users = self.users.get(&kept.id).cloned().unwrap_or_default();
                let what = if users.is_empty() {
                    "Servers that have its public key will not let it in from here any more, from any of your devices.".to_string()
                } else {
                    format!(
                        "{} sign in with it: they will try the other ways (the agent, ~/.ssh, a password) instead. Servers that have its public key keep it until you remove it there.",
                        users.join(", ")
                    )
                };
                card.child(title(format!("Delete {}?", kept.name)))
                    .child(div().text_size(ui(12.5)).text_color(rgb(LABEL)).child(what))
                    .child(buttons(Some("Cancel"), ("delete-ok", "Delete"), cx))
            }
            Dialog::Export {
                kept,
                pass,
                again,
                problem,
            } => {
                let mut b = card
                    .child(title(format!("Export {}", kept.name)))
                    .child(div().text_size(ui(12.5)).text_color(rgb(LABEL)).child(
                        "Written as an OpenSSH key file, for ssh, git and other tools, with its .pub beside it. A passphrase locks the file; the key in the vault stays as it is.",
                    ))
                    .child(Self::field("Passphrase", pass, focused(pass, window, cx), None))
                    .child(Self::field("Again", again, focused(again, window, cx), None));
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(Some("Cancel"), ("export-ok", "Choose where…"), cx))
            }
            Dialog::Unlock {
                path,
                pass,
                problem,
            } => {
                let mut b = card
                    .child(title(format!("Unlock {}", tilde(path))))
                    .child(div().text_size(ui(12.5)).text_color(rgb(LABEL)).child(
                        "This key file has a passphrase. Type it to copy the key into the vault, where it needs none; the file stays as it is.",
                    ))
                    .child(Self::field("Passphrase", pass, true, None));
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(Some("Cancel"), ("unlock-ok", "Copy to vault"), cx))
            }
        };
        Some(body)
    }
}

impl Render for KeysView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mono.is_empty() {
            self.mono = crate::view::TermView::default_font(window).into();
        }
        let kept = self.kept_rows(cx);
        let files = self.file_rows(cx);
        let agent = self.agent_rows(cx);
        let agent_empty = if self.agent.is_none() {
            "Asking the agent…"
        } else {
            "No agent, or it holds no keys."
        };
        let mut page = div()
            .id("keys")
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
                            .child("Keys"),
                    )
                    .child(
                        outline_button("keys-import", "Import key file…")
                            .on_click(cx.listener(|v, _, _, cx| v.import(cx))),
                    )
                    .child(
                        primary_button("keys-generate", "Generate key…")
                            .on_click(cx.listener(|v, _, w, cx| v.generate_dialog(w, cx))),
                    ),
            )
            .child(
                div()
                    .mt(px(-6.))
                    .text_size(ui(12.5))
                    .text_color(rgb(LABEL))
                    .child("Keys for signing in to servers. Keys you generate or import here are kept in the vault, encrypted, and sync to your other devices."),
            );
        if let Some((msg, bad)) = &self.notice {
            page = page.child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(if *bad { BAD } else { GOOD }))
                    .child(msg.clone()),
            );
        }
        if let Some(p) = &self.listed.problem {
            page = page.child(problem_text(&format!("The vault cannot be read: {p}")));
        }
        page = page
            .child(Self::section(
                "In the vault",
                "synced · used only by shellwing",
                kept,
                "None yet: generate one, or copy one of your key files here.",
            ))
            .child(Self::section(
                "In ~/.ssh",
                "key files on this computer, shared with ssh and git",
                files,
                "No key files.",
            ))
            .child(Self::section(
                "In ssh-agent",
                "offered first when connecting",
                agent,
                agent_empty,
            ))
            .child(
                div()
                    .text_size(ui(12.))
                    .text_color(rgb(MUTED))
                    .child("Hosts set to Automatic try the agent, then ~/.ssh, then the vault, then ask for a password."),
            );
        if let Some((at, i)) = self.menu
            && let Some(m) = self.menu(at, i, cx)
        {
            page = page.child(m);
        }
        if let Some(card) = self.dialog_card(window, cx) {
            let size = window.viewport_size();
            page = page.child(
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div()
                            .id("key-dialog-backdrop")
                            .w(size.width)
                            .h(size.height)
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgba(0x03060ca8))
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

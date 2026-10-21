//! The Keys page of Settings: keys the app keeps (in the vault, synced to
//! the other devices), key files in `~/.ssh`, and keys in the ssh-agent.
//! Keys are made, copied into the vault, written out to files, renamed and
//! deleted here; installing one on a host goes through the workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui::{
    ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    Pixels, Point, Render, SharedString, WeakEntity, Window, anchored, deferred, div, prelude::*,
    px, rgb,
};
use shellwing_i18n::{tr, tr_n};
use shellwing_ssh::keys::{self, KeyFile, KeyInfo, KeyKind, ReadError};

use crate::colors::colors;
use crate::host_editor::{hint_text, label_text, outline_button, primary_button, problem_text};
use crate::input::TextInput;
use crate::look::ui;
use crate::secrets::VaultSlot;
use crate::workspace::{KeyToInstall, Workspace};

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
    /// A key file with a passphrase: its passphrase, for `then`.
    Unlock {
        path: PathBuf,
        pass: Entity<TextInput>,
        problem: Option<String>,
        then: Use,
    },
}

/// What a key file is opened for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// A copy in the vault.
    Keep,
    /// The key handed to the ssh-agent.
    Agent,
    /// Its passphrase kept in the vault.
    Save,
}

/// Whose ⋯ menu is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuFor {
    Kept(usize),
    File(usize),
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
    /// A ⋯ menu: where, and whose.
    menu: Option<(Point<Pixels>, MenuFor)>,
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
pub(crate) fn machine() -> String {
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

pub(crate) fn user() -> String {
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

fn kind_label(k: KeyKind) -> String {
    match k {
        KeyKind::Ed25519 => tr!("Ed25519 (recommended)"),
        KeyKind::EcdsaP256 => "ECDSA P-256".into(),
        KeyKind::Rsa4096 => "RSA 4096".into(),
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
        placeholder: impl Fn() -> String + 'static,
        text: &str,
        masked: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        cx.new(|cx| {
            let mut i = TextInput::said(placeholder, cx);
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
        let name = Self::input(
            || tr!("A name, to tell your keys apart"),
            &machine(),
            false,
            cx,
        );
        let comment = Self::input(
            || "user@host".to_string(),
            &format!("{}@{}", user(), machine()),
            false,
            cx,
        );
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
            *problem = Some(tr!("A name is needed."));
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
            let v = vault.get().ok_or_else(|| {
                vault
                    .problem()
                    .unwrap_or_else(|| tr!("the vault is not open"))
            })?;
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
        self.say(tr!("Copied the public key."), false, cx);
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
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| v.rename_ssh_key(id, &name).map_err(|e| e.to_string()));
        self.close(window, cx);
        match result {
            Ok(()) => {
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(tr!("Not renamed: {e}", e = e), true, cx),
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
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| v.delete(&id).map_err(|e| e.to_string()));
        self.close(window, cx);
        match result {
            Ok(()) => {
                self.say(tr!("Deleted {name}.", name = &name), false, cx);
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(tr!("Not deleted: {e}", e = e), true, cx),
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
            *problem = Some(tr!("The two passphrases differ."));
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
            .ok_or_else(|| tr!("the key is not in the vault"))
            .and_then(|pem| keys::parse(&pem, None).map_err(|_| tr!("the key does not read")))
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
                        tr!("Written to {path} and its .pub.", path = tilde(&path)),
                        false,
                        cx,
                    );
                    v.refresh(cx);
                }
                Err(e) => v.say(tr!("Not written: {e}", e = e.to_string()), true, cx),
            });
        })
        .detach();
    }

    /// Opens a key file for `then`, asking for its passphrase when it has
    /// one the vault does not know (or, to save one, always).
    fn with_file(
        &mut self,
        path: PathBuf,
        typed: Option<String>,
        then: Use,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                return self.say(
                    tr!(
                        "Cannot read {path}: {e}",
                        path = tilde(&path),
                        e = e.to_string()
                    ),
                    true,
                    cx,
                );
            }
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
        // Saving one means typing it.
        let pass = if then == Use::Save {
            typed.clone()
        } else {
            typed.clone().or_else(saved)
        };
        let key = match keys::parse(&text, pass.as_deref()) {
            Ok(k) if then != Use::Save || typed.is_some() => k,
            Ok(_) | Err(ReadError::Passphrase) => {
                let wrong = typed.is_some();
                let input = Self::input(|| tr!("Its passphrase"), "", true, cx);
                return self.open(
                    Dialog::Unlock {
                        path,
                        pass: input,
                        problem: wrong.then(|| tr!("That passphrase does not open it.")),
                        then,
                    },
                    window,
                    cx,
                );
            }
            Err(ReadError::NotAKey(_)) => {
                return self.say(
                    tr!("{path} is not an OpenSSH private key.", path = tilde(&path)),
                    true,
                    cx,
                );
            }
        };
        let file_name = tilde(&path);
        match then {
            Use::Keep => {}
            Use::Agent => {
                self.dialog = None;
                window.focus(&self.focus, cx);
                return self.hand_to_agent(key, file_name, cx);
            }
            Use::Save => {
                self.dialog = None;
                window.focus(&self.focus, cx);
                let fp = keys::info(key.public_key()).fingerprint;
                let saved = vault
                    .ok_or_else(|| tr!("the vault is not open"))
                    .and_then(|v| {
                        v.set_passphrase(&fp, &file_name, pass.as_deref().unwrap_or_default())
                            .map_err(|e| e.to_string())
                    });
                match saved {
                    Ok(()) => {
                        self.say(
                            tr!(
                                "Saved the passphrase of {file_name}: it is not asked for again.",
                                file_name = &file_name
                            ),
                            false,
                            cx,
                        );
                        self.vault_changed(cx);
                        self.refresh(cx);
                    }
                    Err(e) => self.say(tr!("Not saved: {e}", e = e), true, cx),
                }
                return;
            }
        }
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
                    .ok_or_else(|| tr!("the vault is not open"))?
                    .add_ssh_key(&name, &info.public, &pem)
                    .map_err(|e| e.to_string())
            });
        self.dialog = None;
        window.focus(&self.focus, cx);
        match added {
            Ok(_) => {
                self.say(
                    tr!(
                        "{name} is in the vault now, and syncs to your other devices.",
                        name = &name
                    ),
                    false,
                    cx,
                );
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(tr!("Not copied: {e}", e = e), true, cx),
        }
    }

    /// Hands `key` to the ssh-agent, until it stops.
    fn hand_to_agent(
        &mut self,
        key: keys::ssh_key::PrivateKey,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let task = self
            .rt
            .spawn(async move { keys::add_to_agent(&key, None, None).await });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |v, cx| {
                match done {
                    Ok(Ok(())) => v.say(
                        tr!(
                            "{name} is in the ssh-agent now, until it stops.",
                            name = &name
                        ),
                        false,
                        cx,
                    ),
                    Ok(Err(e)) => v.say(tr!("Not added: {e}", e = e.to_string()), true, cx),
                    Err(e) => v.say(tr!("Not added: {e}", e = e.to_string()), true, cx),
                }
                v.refresh(cx);
            });
        })
        .detach();
    }

    /// Takes the key with this public line out of the ssh-agent.
    fn out_of_agent(&mut self, public: &str, name: String, cx: &mut Context<Self>) {
        let Ok(key) = keys::ssh_key::PublicKey::from_openssh(public) else {
            return;
        };
        let task = self
            .rt
            .spawn(async move { keys::remove_from_agent(&key, None).await });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |v, cx| {
                match done {
                    Ok(Ok(())) => v.say(
                        tr!("{name} is out of the ssh-agent.", name = &name),
                        false,
                        cx,
                    ),
                    Ok(Err(e)) => v.say(tr!("Not removed: {e}", e = e.to_string()), true, cx),
                    Err(e) => v.say(tr!("Not removed: {e}", e = e.to_string()), true, cx),
                }
                v.refresh(cx);
            });
        })
        .detach();
    }

    /// A kept key, handed to the ssh-agent.
    fn kept_to_agent(&mut self, kept: &Kept, cx: &mut Context<Self>) {
        let key = self
            .vault
            .get()
            .and_then(|v| v.ssh_keys().ok())
            .and_then(|ks| ks.into_iter().find(|k| k.id == kept.id))
            .and_then(|k| keys::parse(&k.private, None).ok());
        match key {
            Some(key) => self.hand_to_agent(key, kept.name.clone(), cx),
            None => self.say(tr!("The key is not in the vault."), true, cx),
        }
    }

    /// Forgets the passphrase the vault has for a key file.
    fn forget_passphrase(&mut self, file: &KeyFile, cx: &mut Context<Self>) {
        let done = self
            .vault
            .get()
            .ok_or_else(|| tr!("the vault is not open"))
            .and_then(|v| {
                let items = v.items().map_err(|e| e.to_string())?;
                for item in items {
                    if let shellwing_vault::Item::Passphrase {
                        id, fingerprint, ..
                    } = item
                        && fingerprint == file.info.fingerprint
                    {
                        v.delete(&id).map_err(|e| e.to_string())?;
                    }
                }
                Ok(())
            });
        match done {
            Ok(()) => {
                self.say(
                    tr!(
                        "Forgot the passphrase of {path}: it is asked for again.",
                        path = tilde(&file.path)
                    ),
                    false,
                    cx,
                );
                self.vault_changed(cx);
                self.refresh(cx);
            }
            Err(e) => self.say(tr!("Not forgotten: {e}", e = e), true, cx),
        }
    }

    /// Whether the agent holds the key with this fingerprint.
    fn in_agent(&self, fingerprint: &str) -> bool {
        self.agent
            .as_ref()
            .is_some_and(|a| a.iter().any(|k| k.fingerprint == fingerprint))
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
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
            let _ = this.update_in(cx, |v, window, cx| {
                v.with_file(path, None, Use::Keep, window, cx)
            });
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
                Some(Dialog::Unlock {
                    path, pass, then, ..
                }) => {
                    let (path, typed, then) =
                        (path.clone(), pass.read(cx).text().to_string(), *then);
                    self.with_file(path, Some(typed), then, window, cx);
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
        Self::chip(kind.to_string(), colors().soft, colors().chip)
            .border_1()
            .border_color(rgb(colors().field_edge))
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

    fn section(title: &str, note: &str, rows: Vec<gpui::AnyElement>, empty: &str) -> gpui::Div {
        let mut list = div()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().line));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(empty.to_string()),
            );
        }
        for (i, r) in rows.into_iter().enumerate() {
            list = list.child(
                div()
                    .when(i > 0, |d| d.border_t_1().border_color(rgb(colors().line)))
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
                            .text_color(rgb(colors().muted))
                            .child(title.to_uppercase()),
                    )
                    .child(
                        div()
                            .text_size(ui(11.5))
                            .text_color(rgb(colors().muted))
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
                            .text_color(rgb(colors().muted))
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
                    [] => tr!("not used by a host yet"),
                    [one] => tr!("used by {one}", one = one),
                    [a, b] => tr!("used by {a}, {b}", a = a, b = b),
                    more => tr_n!(more.len(), "used by {n} host", "used by {n} hosts"),
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
                let open = self.menu.is_some_and(|(_, m)| m == MenuFor::Kept(i));
                Self::row(
                    div().font_weight(FontWeight::MEDIUM).child(k.name.clone()),
                    vec![Self::kind_chip(&k.info.kind)],
                    detail.join(" · "),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            Self::small(("kept-copy", i), &tr!("Copy public key"))
                                .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                        )
                        .child(
                            Self::small(("kept-install", i), &tr!("Install on…")).on_click(
                                cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                            ),
                        )
                        .child(
                            Self::small(("kept-more", i), "⋯")
                                .when(open, |d| d.bg(rgb(colors().button_hover)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |v, e: &gpui::MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        v.menu =
                                            if v.menu.is_some_and(|(_, m)| m == MenuFor::Kept(i)) {
                                                None
                                            } else {
                                                Some((e.position, MenuFor::Kept(i)))
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
                    Self::chip(tr!("no passphrase"), colors().warn, colors().warn_bg)
                } else if f.saved {
                    Self::chip(
                        tr!("passphrase saved in the vault"),
                        colors().good,
                        colors().good_bg,
                    )
                } else {
                    Self::chip(tr!("passphrase"), colors().soft, colors().chip)
                };
                let mut marks = vec![Self::kind_chip(&f.file.info.kind), pass];
                if f.kept {
                    marks.push(Self::chip(
                        tr!("also in the vault"),
                        colors().soft,
                        colors().chip,
                    ));
                }
                let mut detail = vec![f.file.info.fingerprint.clone()];
                if !f.file.info.comment.is_empty() {
                    detail.push(f.file.info.comment.clone());
                }
                let (info, install) = (
                    f.file.info.clone(),
                    KeyToInstall {
                        name: tilde(&f.file.path),
                        public: f.file.info.public.clone(),
                        vault_id: None,
                    },
                );
                let mut actions = div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        Self::small(("file-copy", i), &tr!("Copy public key"))
                            .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                    )
                    .child(
                        Self::small(("file-install", i), &tr!("Install on…")).on_click(
                            cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                        ),
                    );
                let open = self.menu.is_some_and(|(_, m)| m == MenuFor::File(i));
                actions = actions.child(
                    Self::small(("file-more", i), "⋯")
                        .when(open, |d| d.bg(rgb(colors().button_hover)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |v, e: &gpui::MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                v.menu = if v.menu.is_some_and(|(_, m)| m == MenuFor::File(i)) {
                                    None
                                } else {
                                    Some((e.position, MenuFor::File(i)))
                                };
                                cx.notify();
                            }),
                        ),
                );
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
                    .map(|f| tr!("the same key as {path}", path = tilde(&f.file.path)))
                    .or_else(|| {
                        self.listed
                            .kept
                            .iter()
                            .find(|x| x.info.fingerprint == k.fingerprint)
                            .map(|x| tr!("the same key as {name} in the vault", name = &x.name))
                    })
                    .unwrap_or_else(|| tr!("only in the agent"));
                let (info, install) = (
                    k.clone(),
                    KeyToInstall {
                        name: if k.comment.is_empty() {
                            tr!("the agent's key")
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
                            tr!("(no comment)")
                        } else {
                            k.comment.clone()
                        }),
                    vec![Self::kind_chip(&k.kind)],
                    format!("{} · {same}", k.fingerprint),
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(
                            Self::small(("agent-copy", i), &tr!("Copy public key"))
                                .on_click(cx.listener(move |v, _, _, cx| v.copy_public(&info, cx))),
                        )
                        .child(
                            Self::small(("agent-install", i), &tr!("Install on…")).on_click(
                                cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                            ),
                        ),
                )
            })
            .collect()
    }

    fn menu(
        &self,
        at: Point<Pixels>,
        whose: MenuFor,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let item = |id: &'static str, label: &str, danger: bool| {
            div()
                .id(id)
                .debug_selector(|| id.into())
                .px(px(10.))
                .h(px(30.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().selected)))
                .text_color(rgb(if danger { colors().bad } else { colors().text }))
                .child(label.to_string())
        };
        let line = || {
            div()
                .h(px(1.))
                .mx(px(6.))
                .my(px(4.))
                .bg(rgb(colors().field_edge))
        };
        let mut menu = div()
            .id("key-menu")
            .min_w(px(190.))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .bg(rgb(colors().chip))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_size(ui(13.))
            .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                v.menu = None;
                cx.notify();
            }));
        match whose {
            MenuFor::Kept(i) => {
                let k = self.listed.kept.get(i)?.clone();
                let (k1, k2, k3, k4) = (k.clone(), k.clone(), k.clone(), k.clone());
                menu = menu
                    .child(
                        item("menu-export", &tr!("Export to file…"), false).on_click(cx.listener(
                            move |v, _, w, cx| {
                                let pass =
                                    Self::input(|| tr!("A passphrase, or none"), "", true, cx);
                                let again =
                                    Self::input(|| tr!("The passphrase again"), "", true, cx);
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
                            },
                        )),
                    )
                    .child(
                        item("menu-rename", &tr!("Rename…"), false).on_click(cx.listener(
                            move |v, _, w, cx| {
                                let input = Self::input(|| tr!("Name"), &k2.name, false, cx);
                                v.open(
                                    Dialog::Rename {
                                        id: k2.id.clone(),
                                        input,
                                    },
                                    w,
                                    cx,
                                );
                            },
                        )),
                    );
                menu = if self.in_agent(&k.info.fingerprint) {
                    menu.child(
                        item("menu-agent-out", &tr!("Remove from ssh-agent"), false).on_click(
                            cx.listener(move |v, _, _, cx| {
                                v.menu = None;
                                v.out_of_agent(&k3.info.public, k3.name.clone(), cx);
                            }),
                        ),
                    )
                } else {
                    menu.child(
                        item("menu-agent", &tr!("Add to ssh-agent"), false).on_click(cx.listener(
                            move |v, _, _, cx| {
                                v.menu = None;
                                v.kept_to_agent(&k3, cx);
                            },
                        )),
                    )
                };
                menu =
                    menu.child(line())
                        .child(
                            item("menu-delete", &tr!("Delete…"), true).on_click(cx.listener(
                                move |v, _, w, cx| {
                                    v.open(Dialog::Delete { kept: k4.clone() }, w, cx)
                                },
                            )),
                        );
            }
            MenuFor::File(i) => {
                let f = self.listed.files.get(i)?.clone();
                let (path1, path2, path3) = (
                    f.file.path.clone(),
                    f.file.path.clone(),
                    f.file.path.clone(),
                );
                if !f.kept {
                    menu = menu.child(item("menu-keep", &tr!("Copy to vault"), false).on_click(
                        cx.listener(move |v, _, w, cx| {
                            v.menu = None;
                            v.with_file(path1.clone(), None, Use::Keep, w, cx);
                        }),
                    ));
                }
                if f.file.encrypted && !f.saved {
                    menu = menu.child(
                        item("menu-save-pass", &tr!("Save its passphrase…"), false).on_click(
                            cx.listener(move |v, _, w, cx| {
                                let pass = Self::input(|| tr!("Its passphrase"), "", true, cx);
                                v.open(
                                    Dialog::Unlock {
                                        path: path2.clone(),
                                        pass,
                                        problem: None,
                                        then: Use::Save,
                                    },
                                    w,
                                    cx,
                                );
                            }),
                        ),
                    );
                }
                if f.saved {
                    let file = f.file.clone();
                    menu = menu.child(
                        item(
                            "menu-forget-pass",
                            &tr!("Forget its saved passphrase"),
                            false,
                        )
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.menu = None;
                            v.forget_passphrase(&file, cx);
                        })),
                    );
                }
                menu = if self.in_agent(&f.file.info.fingerprint) {
                    let (public, name) = (f.file.info.public.clone(), tilde(&f.file.path));
                    menu.child(
                        item("menu-agent-out", &tr!("Remove from ssh-agent"), false).on_click(
                            cx.listener(move |v, _, _, cx| {
                                v.menu = None;
                                v.out_of_agent(&public, name.clone(), cx);
                            }),
                        ),
                    )
                } else {
                    menu.child(
                        item("menu-agent", &tr!("Add to ssh-agent"), false).on_click(cx.listener(
                            move |v, _, w, cx| {
                                v.menu = None;
                                v.with_file(path3.clone(), None, Use::Agent, w, cx);
                            },
                        )),
                    )
                };
            }
        }
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
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
                    .debug_selector(|| format!("field:{title}"))
                    .h(px(32.))
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
        let buttons = |cancel: Option<&str>, ok: (&'static str, &str), cx: &mut Context<Self>| {
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
                    Some(Dialog::Unlock {
                        path, pass, then, ..
                    }) => {
                        let (path, typed, then) =
                            (path.clone(), pass.read(cx).text().to_string(), *then);
                        v.with_file(path, Some(typed), then, w, cx);
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
            .bg(rgb(colors().surface))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .text_color(rgb(colors().text))
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
                    .border_color(rgb(colors().field_edge))
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
                                if let Some(Dialog::Generate { kind, .. }) = &mut v.dialog {
                                    *kind = k;
                                }
                                cx.notify();
                            }))
                            .child(kind_label(k)),
                    );
                }
                let mut b = card
                    .child(title(tr!("Generate a key")))
                    .child(
                        Self::field(&tr!("Name"), name, focused(name, window, cx), None)
                            .child(hint_text(&tr!("Only for you, to tell your keys apart."))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(label_text(&tr!("Type")))
                            .child(seg)
                            .child(hint_text(&tr!(
                                "Ed25519 is short, fast and taken by OpenSSH 6.5 and later. RSA for very old servers."
                            ))),
                    )
                    .child(
                        Self::field(&tr!("Comment"), comment, focused(comment, window, cx), Some(self.mono.clone())).child(
                            hint_text(&tr!(
                                "Written at the end of the public key: what a server's authorized_keys shows."
                            )),
                        ),
                    )
                    .child(
                        // A block, not a flex row: text in a row does not wrap.
                        div()
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(rgb(colors().chip))
                            .border_1()
                            .border_color(rgb(colors().line))
                            .text_size(ui(12.5))
                            .text_color(rgb(colors().soft))
                            .child(tr!(
                                "Kept in the vault, encrypted, and synced to your other devices, so it needs no passphrase. To use it with ssh or git too, export it to a file, with a passphrase if you like."
                            )),
                    );
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(
                    Some(tr!("Cancel").as_str()),
                    (
                        "gen-ok",
                        if *busy {
                            tr!("Generating…")
                        } else {
                            tr!("Generate")
                        }
                        .as_str(),
                    ),
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
                card.child(title(tr!("{name} is ready", name = &kept.name)))
                    .child(div().text_size(ui(12.5)).text_color(rgb(colors().label)).child(tr!(
                        "A server lets this key in once its public key is in the server's ~/.ssh/authorized_keys. Install it on a host from here, or copy it into a web console (GitHub, GitLab, a cloud provider)."
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .flex()
                                    .child(div().flex_1().child(label_text(&tr!("Public key"))))
                                    .child(
                                        div()
                                            .text_size(ui(11.5))
                                            .text_color(rgb(colors().muted))
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
                                    .bg(rgb(colors().field))
                                    .border_1()
                                    .border_color(rgb(colors().field_edge))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_family(self.mono.clone())
                                            .text_size(ui(12.))
                                            .text_color(rgb(colors().soft))
                                            .child(kept.info.public.clone()),
                                    )
                                    .child(Self::small("gen-copy", &tr!("Copy")).on_click(cx.listener(
                                        move |v, _, _, cx| v.copy_public(&info, cx),
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(outline_button("gen-export", &tr!("Export to file…")).on_click(
                                cx.listener(move |v, _, w, cx| {
                                    let pass = Self::input(|| tr!("A passphrase, or none"), "", true, cx);
                                    let again = Self::input(|| tr!("The passphrase again"), "", true, cx);
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
                                outline_button("gen-done", &tr!("Done"))
                                    .on_click(cx.listener(|v, _, w, cx| v.close(w, cx))),
                            )
                            .child(primary_button("gen-install", &tr!("Install on a host…")).on_click(
                                cx.listener(move |v, _, w, cx| v.install(install.clone(), w, cx)),
                            )),
                    )
            }
            Dialog::Rename { input, .. } => card
                .child(title(tr!("Rename the key")))
                .child(Self::field(&tr!("Name"), input, true, None))
                .child(buttons(
                    Some(tr!("Cancel").as_str()),
                    ("rename-ok", tr!("Rename").as_str()),
                    cx,
                )),
            Dialog::Delete { kept } => {
                let users = self.users.get(&kept.id).cloned().unwrap_or_default();
                let what = if users.is_empty() {
                    tr!(
                        "Servers that have its public key will not let it in from here any more, from any of your devices."
                    )
                } else {
                    tr!(
                        "{hosts} sign in with it: they will try the other ways (the agent, ~/.ssh, a password) instead. Servers that have its public key keep it until you remove it there.",
                        hosts = users.join(", ")
                    )
                };
                card.child(title(tr!("Delete {name}?", name = &kept.name)))
                    .child(
                        div()
                            .text_size(ui(12.5))
                            .text_color(rgb(colors().label))
                            .child(what),
                    )
                    .child(buttons(
                        Some(tr!("Cancel").as_str()),
                        ("delete-ok", tr!("Delete").as_str()),
                        cx,
                    ))
            }
            Dialog::Export {
                kept,
                pass,
                again,
                problem,
            } => {
                let mut b = card
                    .child(title(tr!("Export {name}", name = &kept.name)))
                    .child(div().text_size(ui(12.5)).text_color(rgb(colors().label)).child(tr!(
                        "Written as an OpenSSH key file, for ssh, git and other tools, with its .pub beside it. A passphrase locks the file; the key in the vault stays as it is."
                    )))
                    .child(Self::field(&tr!("Passphrase"), pass, focused(pass, window, cx), None))
                    .child(Self::field(&tr!("Again"), again, focused(again, window, cx), None));
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(
                    Some(tr!("Cancel").as_str()),
                    ("export-ok", tr!("Choose where…").as_str()),
                    cx,
                ))
            }
            Dialog::Unlock {
                path,
                pass,
                problem,
                then,
            } => {
                let (head, what, ok) = match then {
                    Use::Keep => (
                        tr!("Unlock {path}", path = tilde(path)),
                        tr!(
                            "This key file has a passphrase. Type it to copy the key into the vault, where it needs none; the file stays as it is."
                        ),
                        tr!("Copy to vault"),
                    ),
                    Use::Agent => (
                        tr!("Unlock {path}", path = tilde(path)),
                        tr!(
                            "This key file has a passphrase. Type it to hand the key to the ssh-agent; the file stays as it is."
                        ),
                        tr!("Add to ssh-agent"),
                    ),
                    Use::Save => (
                        tr!("Save the passphrase of {path}", path = tilde(path)),
                        tr!(
                            "Kept in the vault, encrypted, and synced: shellwing then signs in with this key without asking, here and on your other devices."
                        ),
                        tr!("Save"),
                    ),
                };
                let mut b = card
                    .child(title(head))
                    .child(
                        div()
                            .text_size(ui(12.5))
                            .text_color(rgb(colors().label))
                            .child(what),
                    )
                    .child(Self::field(&tr!("Passphrase"), pass, true, None));
                if let Some(p) = problem {
                    b = b.child(problem_text(p));
                }
                b.child(buttons(
                    Some(tr!("Cancel").as_str()),
                    ("unlock-ok", ok.as_str()),
                    cx,
                ))
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
            tr!("Asking the agent…")
        } else {
            tr!("No agent, or it holds no keys.")
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
                            .child(tr!("Keys")),
                    )
                    .child(
                        outline_button("keys-import", &tr!("Import key file…"))
                            .on_click(cx.listener(|v, _, _, cx| v.import(cx))),
                    )
                    .child(
                        primary_button("keys-generate", &tr!("Generate key…"))
                            .on_click(cx.listener(|v, _, w, cx| v.generate_dialog(w, cx))),
                    ),
            )
            .child(
                div()
                    .mt(px(-6.))
                    .text_size(ui(12.5))
                    .text_color(rgb(colors().label))
                    .child(tr!("Keys for signing in to servers. Keys you generate or import here are kept in the vault, encrypted, and sync to your other devices.")),
            );
        if let Some((msg, bad)) = &self.notice {
            page = page.child(
                div()
                    .text_size(ui(12.5))
                    .text_color(rgb(if *bad { colors().bad } else { colors().good }))
                    .child(msg.clone()),
            );
        }
        if let Some(p) = &self.listed.problem {
            page = page.child(problem_text(&tr!("The vault cannot be read: {p}", p = p)));
        }
        page = page
            .child(Self::section(
                &tr!("In the vault"),
                &tr!("synced · used only by shellwing"),
                kept,
                &tr!("None yet: generate one, or copy one of your key files here."),
            ))
            .child(Self::section(
                &tr!("In ~/.ssh"),
                &tr!("key files on this computer, shared with ssh and git"),
                files,
                &tr!("No key files."),
            ))
            .child(Self::section(
                &tr!("In ssh-agent"),
                &tr!("offered first when connecting"),
                agent,
                &agent_empty,
            ))
            .child(
                div()
                    .text_size(ui(12.))
                    .text_color(rgb(colors().muted))
                    .child(tr!("Hosts set to Automatic try the agent, then ~/.ssh, then the vault, then ask for a password.")),
            );
        if let Some((at, whose)) = self.menu
            && let Some(m) = self.menu(at, whose, cx)
        {
            page = page.child(m);
        }
        if let Some(card) = self.dialog_card(window, cx) {
            let (at, shade) = crate::chrome::shade(window);
            page = page.child(
                deferred(
                    anchored().position(at).child(
                        shade
                            .id("key-dialog-backdrop")
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

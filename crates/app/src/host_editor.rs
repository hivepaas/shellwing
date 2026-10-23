//! The host editor: a drawer on the right for adding or changing a host in
//! hosts.toml, as the approved sketch draws it. Saving writes only what changed
//! and keeps the file's comments, so editing by hand and here mix freely.
//! Hosts from `~/.ssh/config` are shown read only: shellwing never writes that
//! file, but can copy a host from it into hosts.toml.
//!
//! Tab and Shift+Tab move between fields; Enter saves; Esc closes (or first
//! closes an open list or field).

use gpui::{
    Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    Render, SharedString, Window, div, prelude::*, px, rgb,
};
use shellwing_config::{Auth, AuthMethod, HOST_COLORS, Host, HostKey, Hosts, Store};
use shellwing_i18n::tr;
use shellwing_ssh::config::{self as ssh_config, Resolved};

use crate::colors::colors;
use crate::input::TextInput;
use crate::look::ui;

pub const WIDTH: f32 = 520.;

pub enum EditorEvent {
    Close,
    /// Written to hosts.toml; connect to it when asked, or install a key
    /// made here on it.
    Saved {
        name: String,
        connect: bool,
        install: Option<crate::workspace::KeyToInstall>,
    },
    Deleted {
        name: String,
    },
    OpenSshConfig,
}

/// What the editor works with, taken from the workspace when it opens.
pub struct EditorContext {
    pub store: Option<Store>,
    pub hosts: Hosts,
    pub ssh_aliases: Vec<String>,
    /// Theme ids and names.
    pub themes: Vec<(String, String)>,
    /// The app's settings that a host's empty field falls back to.
    pub app_ssh: shellwing_config::Ssh,
    /// Whether the app logs every session (`[log] always`).
    pub app_logs: bool,
    /// Keys to choose from: the vault's, then the files in `~/.ssh`.
    pub keys: Vec<KeyChoice>,
    /// Where a key made here is kept.
    pub vault: crate::secrets::VaultSlot,
}

/// The Key list's choice that makes a key for the host.
const GENERATE: &str = "\0generate";

/// A key the host can sign in with.
#[derive(Clone)]
pub struct KeyChoice {
    /// As hosts.toml writes it: `vault:<id>`, or a path.
    pub value: String,
    pub name: String,
    /// `ED25519`, `RSA 3072`…
    pub kind: String,
    /// Kept in the vault rather than a file.
    pub vault: bool,
}

enum Mode {
    Edit,
    /// A host from `~/.ssh/config`, as ssh would reach it.
    SshConfig {
        alias: String,
        resolved: Option<Box<Resolved>>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Group,
    Theme,
    Key,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Name,
    Alias,
    Address,
    Port,
    User,
    Key,
    Jump,
    Forward,
    Keepalive,
}

struct Inputs {
    name: Entity<TextInput>,
    alias: Entity<TextInput>,
    address: Entity<TextInput>,
    port: Entity<TextInput>,
    user: Entity<TextInput>,
    key: Entity<TextInput>,
    tags: Entity<TextInput>,
    jump: Entity<TextInput>,
    forward: Entity<TextInput>,
    keepalive: Entity<TextInput>,
    run: Entity<TextInput>,
    new_group: Entity<TextInput>,
}

pub struct HostEditor {
    focus: FocusHandle,
    ctx: EditorContext,
    /// A key made here for the host, which the host does not have yet.
    new_key: Option<crate::workspace::KeyToInstall>,
    mode: Mode,
    /// The host as opened; None for a new one.
    original: Option<Host>,
    inputs: Inputs,
    method: AuthMethod,
    jumps: Vec<String>,
    forwards: Vec<String>,
    group: Option<String>,
    theme: Option<String>,
    local_only: bool,
    forward_agent: bool,
    host_key: Option<HostKey>,
    log: Option<bool>,
    color: Option<String>,
    favorite: bool,
    /// A group being named here, made when the host is saved.
    naming_group: bool,
    menu: Option<Menu>,
    adding_jump: bool,
    adding_forward: bool,
    problems: Vec<(Field, String)>,
    save_problem: Option<String>,
    confirm_delete: bool,
    /// The form as it opened, to tell whether anything was changed.
    opened_as: Option<Host>,
}

impl EventEmitter<EditorEvent> for HostEditor {}

fn opt(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// `~/…` for paths in the home folder, as people write them in hosts.toml.
fn tilde(path: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|h| path.strip_prefix(&h).ok().map(|p| p.to_owned())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn method_label(m: AuthMethod) -> String {
    match m {
        AuthMethod::Auto => tr!("Automatic"),
        AuthMethod::Key => tr!("Key file"),
        AuthMethod::Password => tr!("Password"),
        AuthMethod::Agent => tr!("Agent only"),
    }
}

impl HostEditor {
    fn blank(ctx: EditorContext, mode: Mode, cx: &mut Context<Self>) -> Self {
        // Placeholders said again when drawn: they follow the language.
        fn input(
            cx: &mut Context<HostEditor>,
            hint: impl Fn() -> String + 'static,
        ) -> gpui::Entity<TextInput> {
            crate::input::said(cx, hint)
        }
        let keepalive = ctx.app_ssh.keepalive.to_string();
        let inputs = Inputs {
            name: input(cx, || "web-1".to_string()),
            alias: input(cx, || tr!("optional")),
            address: input(cx, || tr!("10.0.0.5 or host.example.com")),
            port: input(cx, || "22".to_string()),
            user: input(cx, || tr!("your login name")),
            key: input(cx, || "~/.ssh/id_ed25519".to_string()),
            tags: input(cx, || "api, docker".to_string()),
            jump: input(cx, || tr!("a host here, an ssh alias, or user@host")),
            forward: input(cx, || "-L 8080:localhost:80".to_string()),
            keepalive: input(cx, move || keepalive.clone()),
            run: input(cx, || "cd ~/apps && exec $SHELL -l".to_string()),
            new_group: input(cx, || tr!("Staging")),
        };
        HostEditor {
            focus: cx.focus_handle(),
            ctx,
            new_key: None,
            mode,
            original: None,
            inputs,
            method: AuthMethod::Auto,
            jumps: vec![],
            forwards: vec![],
            group: None,
            theme: None,
            local_only: false,
            forward_agent: false,
            host_key: None,
            log: None,
            color: None,
            favorite: false,
            naming_group: false,
            menu: None,
            adding_jump: false,
            adding_forward: false,
            problems: vec![],
            save_problem: None,
            confirm_delete: false,
            opened_as: None,
        }
    }

    pub fn new_host(ctx: EditorContext, cx: &mut Context<Self>) -> Self {
        let mut e = Self::blank(ctx, Mode::Edit, cx);
        e.opened_as = Some(e.current(cx));
        e
    }

    pub fn edit(ctx: EditorContext, host: Host, cx: &mut Context<Self>) -> Self {
        let mut e = Self::blank(ctx, Mode::Edit, cx);
        e.fill(&host, cx);
        e.original = Some(host);
        e.opened_as = Some(e.current(cx));
        e
    }

    /// A host from `~/.ssh/config`, read only until copied.
    pub fn ssh_config(ctx: EditorContext, alias: String, cx: &mut Context<Self>) -> Self {
        let resolved = ssh_config::resolve(&alias, ssh_config::default_config_path().as_deref())
            .ok()
            .map(Box::new);
        Self::blank(ctx, Mode::SshConfig { alias, resolved }, cx)
    }

    /// Where the keyboard goes first.
    pub fn first_focus(&self, cx: &gpui::App) -> FocusHandle {
        match self.mode {
            Mode::Edit => self.inputs.name.focus_handle(cx),
            Mode::SshConfig { .. } => self.focus.clone(),
        }
    }

    fn set(&self, input: &Entity<TextInput>, text: &str, cx: &mut Context<Self>) {
        input.update(cx, |i, cx| i.set_text(text, text.len()..text.len(), cx));
    }

    fn fill(&mut self, h: &Host, cx: &mut Context<Self>) {
        let i = &self.inputs;
        self.set(&i.name, &h.name, cx);
        self.set(&i.alias, h.alias.as_deref().unwrap_or(""), cx);
        self.set(&i.address, &h.address, cx);
        self.set(
            &i.port,
            &h.port.map(|p| p.to_string()).unwrap_or_default(),
            cx,
        );
        self.set(&i.user, h.user.as_deref().unwrap_or(""), cx);
        self.set(&i.key, h.auth.key.as_deref().unwrap_or(""), cx);
        self.set(&i.tags, &h.tags.join(", "), cx);
        self.set(
            &i.keepalive,
            &h.keepalive.map(|k| k.to_string()).unwrap_or_default(),
            cx,
        );
        self.forward_agent = h.forward_agent;
        self.host_key = h.host_key;
        self.log = h.log;
        self.set(&i.run, h.run.as_deref().unwrap_or(""), cx);
        self.color = h.color.clone();
        self.favorite = h.favorite;
        self.method = h.auth.method;
        self.jumps = h.jump.clone();
        self.forwards = h.forward.clone();
        self.group = h.group.clone();
        self.theme = h.theme.clone();
        self.local_only = h.local_only;
    }

    fn text(&self, input: &Entity<TextInput>, cx: &gpui::App) -> String {
        input.read(cx).text().trim().to_string()
    }

    /// The form as a host, unchecked.
    fn current(&self, cx: &gpui::App) -> Host {
        let i = &self.inputs;
        let mut tags: Vec<String> = vec![];
        for t in self.text(&i.tags, cx).split(',') {
            let t = t.trim().to_string();
            if !t.is_empty() && !tags.contains(&t) {
                tags.push(t);
            }
        }
        Host {
            id: self.original.as_ref().and_then(|o| o.id.clone()),
            name: self.text(&i.name, cx),
            alias: opt(self.text(&i.alias, cx)),
            address: self.text(&i.address, cx),
            port: self.text(&i.port, cx).parse().ok(),
            user: opt(self.text(&i.user, cx)),
            group: if self.naming_group {
                opt(self.text(&i.new_group, cx))
            } else {
                self.group.clone()
            },
            tags,
            jump: self.jumps.clone(),
            forward: self.forwards.clone(),
            auth: Auth {
                method: self.method,
                key: (self.method == AuthMethod::Key)
                    .then(|| self.text(&i.key, cx))
                    .filter(|k| !k.is_empty()),
            },
            theme: self.theme.clone(),
            local_only: self.local_only,
            forward_agent: self.forward_agent,
            keepalive: self.text(&i.keepalive, cx).parse().ok(),
            host_key: self.host_key,
            color: self.color.clone(),
            favorite: self.favorite,
            run: opt(self.text(&i.run, cx)),
            log: self.log,
        }
    }

    fn dirty(&self, cx: &gpui::App) -> bool {
        matches!(self.mode, Mode::Edit) && self.opened_as.as_ref() != Some(&self.current(cx))
    }

    /// Clicking beside the drawer closes it only when nothing would be lost.
    pub fn can_close_quietly(&self, cx: &gpui::App) -> bool {
        !self.dirty(cx)
    }

    /// Another host already has `name` as its name or alias.
    fn taken(&self, name: &str) -> bool {
        let me = |h: &Host| match &self.original {
            Some(o) if o.id.is_some() => o.id == h.id,
            Some(o) => o.name == h.name,
            None => false,
        };
        self.ctx
            .hosts
            .hosts
            .iter()
            .any(|h| !me(h) && (h.name == name || h.alias.as_deref() == Some(name)))
    }

    /// The host to save, or None with the problems shown by their fields.
    fn checked(&mut self, cx: &gpui::App) -> Option<Host> {
        let host = self.current(cx);
        let i = &self.inputs;
        let mut problems = vec![];
        let spaced = |s: &str| s.chars().any(char::is_whitespace);
        if host.name.is_empty() {
            problems.push((Field::Name, tr!("A name is needed.")));
        } else if self.taken(&host.name) {
            problems.push((Field::Name, tr!("Another host has this name.")));
        }
        if let Some(a) = &host.alias {
            if spaced(a) {
                problems.push((Field::Alias, tr!("No spaces.")));
            } else if self.taken(a) {
                problems.push((Field::Alias, tr!("Another host uses this.")));
            }
        }
        if host.address.is_empty() {
            problems.push((Field::Address, tr!("An address is needed.")));
        } else if spaced(&host.address) {
            problems.push((Field::Address, tr!("No spaces.")));
        }
        let port = self.text(&i.port, cx);
        if !port.is_empty() && port.parse::<u16>().map_or(true, |p| p == 0) {
            problems.push((Field::Port, tr!("1 to 65535.")));
        }
        if host.user.as_deref().is_some_and(spaced) {
            problems.push((Field::User, tr!("No spaces.")));
        }
        if self.method == AuthMethod::Key && host.auth.key.is_none() {
            problems.push((Field::Key, tr!("Choose a key.")));
        }
        let keepalive = self.text(&i.keepalive, cx);
        if !keepalive.is_empty() && keepalive.parse::<u64>().map_or(true, |k| k > 3600) {
            problems.push((Field::Keepalive, tr!("Seconds, 0 to 3600 (0: none).")));
        }
        if host.jump.contains(&host.name) {
            problems.push((Field::Jump, tr!("A host cannot jump through itself.")));
        }
        self.problems = problems;
        self.problems.is_empty().then_some(host)
    }

    fn problem(&self, f: Field) -> Option<&str> {
        self.problems
            .iter()
            .find(|(x, _)| *x == f)
            .map(|(_, p)| p.as_str())
    }

    fn save(&mut self, connect: bool, cx: &mut Context<Self>) {
        self.save_with(connect, None, cx);
    }

    /// Makes an Ed25519 key for the host, keeps it in the vault, and signs
    /// in with it.
    fn generate_key(&mut self, cx: &mut Context<Self>) {
        use shellwing_ssh::keys;
        let host = self.text(&self.inputs.name, cx);
        let name = if host.is_empty() {
            tr!("new key")
        } else {
            host
        };
        let comment = format!(
            "{}@{}",
            crate::keys_view::user(),
            crate::keys_view::machine()
        );
        let made = keys::generate(keys::KeyKind::Ed25519, &comment)
            .map_err(|e| e.to_string())
            .and_then(|key| {
                let info = keys::info(key.public_key());
                let pem = keys::to_vault(&key).map_err(|e| e.to_string())?;
                let vault = self
                    .ctx
                    .vault
                    .get()
                    .ok_or_else(|| tr!("the vault is not open"))?;
                let id = vault
                    .add_ssh_key(&name, &info.public, &pem)
                    .map_err(|e| e.to_string())?;
                Ok((id, info))
            });
        self.problems.retain(|(f, _)| *f != Field::Key);
        match made {
            Ok((id, info)) => {
                let value = format!("{}{id}", crate::target::VAULT_KEY);
                self.ctx.keys.insert(
                    0,
                    KeyChoice {
                        value: value.clone(),
                        name: name.clone(),
                        kind: info.kind.clone(),
                        vault: true,
                    },
                );
                let key = self.inputs.key.clone();
                self.set(&key, &value, cx);
                self.new_key = Some(crate::workspace::KeyToInstall {
                    name,
                    public: info.public,
                    vault_id: Some(id),
                });
            }
            Err(e) => self
                .problems
                .push((Field::Key, tr!("No key made: {e}", e = e))),
        }
        cx.notify();
    }

    fn save_with(
        &mut self,
        connect: bool,
        install: Option<crate::workspace::KeyToInstall>,
        cx: &mut Context<Self>,
    ) {
        self.confirm_delete = false;
        let Some(host) = self.checked(cx) else {
            self.save_problem = None;
            return cx.notify();
        };
        let Some(store) = &self.ctx.store else {
            self.save_problem = Some(tr!("There is no config directory to save in."));
            return cx.notify();
        };
        match store.save_host(&host) {
            Ok(_) => cx.emit(EditorEvent::Saved {
                name: host.name,
                connect,
                install,
            }),
            Err(e) => {
                self.save_problem = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(original) = &self.original else {
            return;
        };
        if !self.confirm_delete {
            self.confirm_delete = true;
            return cx.notify();
        }
        let result = match (&self.ctx.store, &original.id) {
            (Some(store), Some(id)) => store.delete_host(id).map_err(|e| e.to_string()),
            _ => Err(tr!("This host cannot be found in hosts.toml.")),
        };
        match result {
            Ok(()) => cx.emit(EditorEvent::Deleted {
                name: original.name.clone(),
            }),
            Err(e) => {
                self.confirm_delete = false;
                self.save_problem = Some(e);
                cx.notify();
            }
        }
    }

    /// `base`, or `base 2`, `base 3`… when another host has it.
    fn free_name(&self, base: &str) -> String {
        let taken = |n: &str| {
            self.ctx
                .hosts
                .hosts
                .iter()
                .any(|h| h.name == n || h.alias.as_deref() == Some(n))
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|n| !taken(n))
            .unwrap_or_else(|| base.to_string())
    }

    /// The same host, as a new one not saved yet.
    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.text(&self.inputs.name, cx);
        let copy = self.free_name(&tr!("{name} copy", name = &name));
        self.original = None;
        self.confirm_delete = false;
        self.set(&self.inputs.name.clone(), &copy, cx);
        // An alias must stay unique: the copy goes without.
        self.set(&self.inputs.alias.clone(), "", cx);
        self.opened_as = None;
        window.focus(&self.inputs.name.focus_handle(cx), cx);
        cx.notify();
    }

    /// Turns the `~/.ssh/config` host into a new hosts.toml host to save.
    fn copy_from_ssh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Mode::SshConfig { alias, resolved } = &self.mode else {
            return;
        };
        let alias = alias.clone();
        let mut host = Host {
            id: None,
            name: alias.clone(),
            alias: None,
            address: alias.clone(),
            port: None,
            user: None,
            group: None,
            tags: vec![],
            jump: vec![],
            forward: vec![],
            auth: Auth::default(),
            theme: None,
            local_only: false,
            forward_agent: false,
            keepalive: None,
            host_key: None,
            color: None,
            favorite: false,
            run: None,
            log: None,
        };
        if let Some(r) = resolved {
            host.forward_agent = r.forward_agent;
            host.keepalive = r.keepalive.map(|k| k.as_secs());
            host.address = r.target.host.clone();
            host.port = (r.target.port != 22).then_some(r.target.port);
            // Your own login name is what ssh uses anyway; written out, it would
            // be wrong on a device where you are someone else.
            let me = std::env::var("USER").or_else(|_| std::env::var("USERNAME"));
            host.user =
                (me.as_deref() != Ok(r.target.user.as_str())).then(|| r.target.user.clone());
            host.jump = r.jumps.iter().map(|j| j.alias.clone()).collect();
            // The copy goes to the address itself, which ~/.ssh/config's
            // block for the alias does not match: its forwards come along.
            host.forward = r.forwards.clone();
            if let Some(key) = r.identity_files.first() {
                host.auth = Auth {
                    method: AuthMethod::Key,
                    key: Some(tilde(key)),
                };
            }
        }
        host.name = self.free_name(&alias);
        self.mode = Mode::Edit;
        self.fill(&host, cx);
        self.opened_as = None;
        window.focus(&self.inputs.name.focus_handle(cx), cx);
        cx.notify();
    }

    fn browse_key(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr!("Choose").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(p) = paths.first()
            {
                let path = tilde(p);
                let _ = this.update(cx, |e, cx| {
                    let key = e.inputs.key.clone();
                    e.set(&key, &path, cx);
                    e.problems.retain(|(f, _)| *f != Field::Key);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn add_jump(&mut self, cx: &mut Context<Self>) {
        let name = self.text(&self.inputs.jump, cx);
        self.add_jump_named(name, cx);
    }

    fn add_jump_named(&mut self, name: String, cx: &mut Context<Self>) {
        if name.is_empty() || self.jumps.contains(&name) {
            return;
        }
        self.jumps.push(name);
        let jump = self.inputs.jump.clone();
        self.set(&jump, "", cx);
        self.problems.retain(|(f, _)| *f != Field::Jump);
        cx.notify();
    }

    fn add_forward(&mut self, cx: &mut Context<Self>) {
        let spec = self.text(&self.inputs.forward, cx);
        if spec.is_empty() {
            return;
        }
        self.problems.retain(|(f, _)| *f != Field::Forward);
        match spec.parse::<shellwing_ssh::Forward>() {
            Ok(_) if self.forwards.contains(&spec) => {}
            Ok(_) => {
                self.forwards.push(spec);
                let forward = self.inputs.forward.clone();
                self.set(&forward, "", cx);
            }
            Err(e) => self.problems.push((Field::Forward, e.to_string())),
        }
        cx.notify();
    }

    /// The fields Tab moves through, in order.
    fn tab_order(&self, cx: &gpui::App) -> Vec<FocusHandle> {
        let i = &self.inputs;
        let mut order = vec![&i.name, &i.alias, &i.address, &i.port, &i.user];
        if self.adding_jump {
            order.push(&i.jump);
        }
        if self.adding_forward {
            order.push(&i.forward);
        }
        order.push(&i.tags);
        order.push(&i.run);
        order.push(&i.keepalive);
        order.into_iter().map(|e| e.focus_handle(cx)).collect()
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let focused = |e: &Entity<TextInput>, cx: &gpui::App| e.focus_handle(cx).is_focused(window);
        match k.key.as_str() {
            "escape" => {
                if self.menu.is_some() {
                    self.menu = None;
                } else if self.confirm_delete {
                    self.confirm_delete = false;
                } else if focused(&self.inputs.jump, cx) && self.adding_jump {
                    self.adding_jump = false;
                    window.focus(&self.focus, cx);
                } else if focused(&self.inputs.forward, cx) && self.adding_forward {
                    self.adding_forward = false;
                    window.focus(&self.focus, cx);
                } else {
                    cx.emit(EditorEvent::Close);
                }
                cx.notify();
            }
            "tab" if matches!(self.mode, Mode::Edit) => {
                let order = self.tab_order(cx);
                let at = order.iter().position(|f| f.is_focused(window));
                let n = order.len() as isize;
                let next = match at {
                    Some(i) if k.modifiers.shift => (i as isize - 1).rem_euclid(n),
                    Some(i) => (i as isize + 1).rem_euclid(n),
                    None => 0,
                };
                window.focus(&order[next as usize], cx);
            }
            "enter" if matches!(self.mode, Mode::Edit) => {
                if focused(&self.inputs.jump, cx) {
                    self.add_jump(cx);
                } else if focused(&self.inputs.forward, cx) {
                    self.add_forward(cx);
                } else {
                    let connect = k.modifiers.control || k.modifiers.platform;
                    self.save(connect, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// A row of choices, the current one marked; a click sets it with `set`.
    fn segments<T: Copy + PartialEq + 'static>(
        id: &'static str,
        choices: &[(T, &str)],
        current: T,
        set: fn(&mut Self, T),
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut row = div()
            .flex()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(colors().field_edge))
            .overflow_hidden();
        for (n, &(value, name)) in choices.iter().enumerate() {
            let on = current == value;
            row = row.child(
                div()
                    .id((id, n))
                    .flex_1()
                    .h(px(30.))
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
                    .on_click(cx.listener(move |e, _, _, cx| {
                        set(e, value);
                        cx.notify();
                    }))
                    .child(name.to_string()),
            );
        }
        row
    }

    fn input_box(
        &self,
        input: &Entity<TextInput>,
        bad: bool,
        window: &Window,
        cx: &gpui::App,
    ) -> gpui::Div {
        let focused = input.focus_handle(cx).is_focused(window);
        div()
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(if bad {
                colors().bad
            } else if focused {
                colors().accent
            } else {
                colors().field_edge
            }))
            .bg(rgb(colors().field))
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .child(input.clone())
    }

    fn field(
        &self,
        label: &str,
        input: &Entity<TextInput>,
        which: Field,
        window: &Window,
        cx: &gpui::App,
    ) -> gpui::Div {
        let problem = self.problem(which);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .flex_1()
            .min_w_0()
            .child(label_text(label))
            .child(self.input_box(input, problem.is_some(), window, cx))
            .when_some(problem, |d, p| d.child(problem_text(p)))
    }

    fn select(
        &self,
        id: &str,
        label: &str,
        value: String,
        menu: Menu,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let open = self.menu == Some(menu);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .flex_1()
            .min_w_0()
            .child(label_text(label))
            .child(
                div()
                    .id(SharedString::from(id.to_string()))
                    .debug_selector(|| id.to_string())
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if open {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().field))
                    .text_size(ui(13.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |e, _, _, cx| {
                        e.menu = if e.menu == Some(menu) {
                            None
                        } else {
                            Some(menu)
                        };
                        cx.notify();
                    }))
                    .child(
                        div()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(value),
                    )
                    .child(div().text_color(rgb(colors().muted)).child("▾")),
            )
    }

    /// The choices of an open list: (value, label).
    fn menu_list(
        &self,
        menu: Menu,
        choices: Vec<(Option<String>, String)>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let current = match menu {
            Menu::Group => self.group.clone(),
            Menu::Theme => self.theme.clone(),
            Menu::Key => Some(self.text(&self.inputs.key, cx)),
        };
        let mut list = div()
            .id(match menu {
                Menu::Group => "group-menu",
                Menu::Theme => "theme-menu",
                Menu::Key => "key-menu",
            })
            .flex()
            .flex_col()
            .max_h(px(220.))
            .overflow_y_scroll()
            .p_1()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(colors().edge))
            .bg(rgb(colors().raised));
        for (i, (value, label)) in choices.into_iter().enumerate() {
            let chosen = value == current;
            list = list.child(
                div()
                    .id(("choice", i))
                    .debug_selector(|| format!("choice:{label}"))
                    .px_2()
                    .py(px(5.))
                    .rounded(px(4.))
                    .text_size(ui(13.))
                    .cursor_pointer()
                    .when(chosen, |d| d.bg(rgb(colors().selected)))
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(move |e, _, _, cx| {
                        match menu {
                            Menu::Group => {
                                e.group = value.clone();
                                e.naming_group = false;
                            }
                            Menu::Theme => e.theme = value.clone(),
                            // No value: another file, to look for.
                            Menu::Key => match &value {
                                Some(v) if v == GENERATE => e.generate_key(cx),
                                Some(v) => {
                                    let key = e.inputs.key.clone();
                                    e.set(&key, v, cx);
                                    e.problems.retain(|(f, _)| *f != Field::Key);
                                }
                                None => e.browse_key(cx),
                            },
                        }
                        e.menu = None;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        list
    }

    fn jump_detail(&self, name: &str) -> String {
        if let Some(h) = self.ctx.hosts.find(name) {
            match &h.user {
                Some(u) => format!("{u}@{}", h.address),
                None => h.address.clone(),
            }
        } else if self.ctx.ssh_aliases.iter().any(|a| a == name) {
            "~/.ssh/config".into()
        } else {
            tr!("host name")
        }
    }

    fn render_ssh(
        &self,
        alias: &str,
        resolved: Option<&Resolved>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mut facts = div().flex().flex_col().gap_1();
        match resolved {
            Some(r) => {
                facts = facts.child(fact(&tr!("Reaches"), r.target.to_string()));
                if let Some(k) = r.identity_files.first() {
                    facts = facts.child(fact(&tr!("Key"), tilde(k)));
                }
                if !r.jumps.is_empty() {
                    let via: Vec<&str> = r.jumps.iter().map(|j| j.alias.as_str()).collect();
                    facts = facts.child(fact(&tr!("Through"), via.join(" → ")));
                }
                if !r.forwards.is_empty() {
                    facts = facts.child(fact(&tr!("Forwards"), r.forwards.join(", ")));
                }
            }
            None => facts = facts.child(hint_text(&tr!("It could not be read."))),
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header(alias, &tr!("From ~/.ssh/config"), None, cx))
            .child(
                section(&tr!("Host from ~/.ssh/config"))
                    .child(hint_text(&tr!(
                        "shellwing reads this file but never writes to it. Edit it in your editor, or make a copy here to add a group, a theme, forwards or a saved password."
                    )))
                    .child(facts)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                outline_button("ssh-open", &tr!("Open config file")).on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(EditorEvent::OpenSshConfig)),
                                ),
                            )
                            .child(primary_button("ssh-copy", &tr!("Copy to my hosts")).on_click(
                                cx.listener(|e, _, window, cx| e.copy_from_ssh(window, cx)),
                            )),
                    ),
            )
    }
}

pub(crate) fn label_text(s: &str) -> gpui::Div {
    div()
        .text_size(ui(12.))
        .text_color(rgb(colors().label))
        .child(s.to_string())
}

pub(crate) fn hint_text(s: &str) -> gpui::Div {
    div()
        .text_size(ui(11.5))
        .text_color(rgb(colors().muted))
        .child(s.to_string())
}

pub(crate) fn problem_text(s: &str) -> gpui::Div {
    div()
        .text_size(ui(11.5))
        .text_color(rgb(colors().bad))
        .child(s.to_string())
}

fn fact(label: &str, value: String) -> gpui::Div {
    div()
        .flex()
        .gap_2()
        .text_size(ui(12.5))
        .child(
            div()
                .w(px(70.))
                .text_color(rgb(colors().muted))
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(value),
        )
}

pub(crate) fn section(title: &str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .px(px(20.))
        .py(px(14.))
        .border_b_1()
        .border_color(rgb(colors().line))
        .child(
            div()
                .text_size(ui(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(colors().muted))
                .child(title.to_uppercase()),
        )
}

fn row() -> gpui::Div {
    div().flex().gap(px(10.))
}

fn header(title: &str, note: &str, color: Option<u32>, cx: &mut Context<HostEditor>) -> gpui::Div {
    div()
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
                .size(px(10.))
                .rounded(px(3.))
                .bg(rgb(color.unwrap_or(colors().accent))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(ui(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(ui(12.))
                .text_color(rgb(colors().muted))
                .child(note.to_string()),
        )
        .child(
            div()
                .id("editor-close")
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_color(rgb(colors().soft))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors().hover)))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Close)))
                .child("✕"),
        )
}

pub(crate) fn text_button(id: &str, label: &str, color: u32) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .debug_selector(|| id.to_string())
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(colors().hover)))
        .child(label.to_string())
}

pub(crate) fn outline_button(id: &str, label: &str) -> gpui::Stateful<gpui::Div> {
    text_button(id, label, colors().text)
        .px(px(14.))
        .border_1()
        .border_color(rgb(colors().field_edge))
}

pub(crate) fn primary_button(id: &str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .debug_selector(|| id.to_string())
        .h(px(32.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(rgb(colors().accent))
        .hover(|s| s.bg(rgb(colors().accent_hover)))
        .text_color(rgb(colors().on_accent))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .child(label.to_string())
}

fn chip(
    id: ElementId,
    name: &str,
    detail: String,
    remove: impl Fn(&mut HostEditor, &mut Context<HostEditor>) + 'static,
    cx: &mut Context<HostEditor>,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .pl(px(10.))
        .pr(px(4.))
        .rounded(px(13.))
        .border_1()
        .border_color(rgb(colors().edge))
        .bg(rgb(colors().chip))
        .text_size(ui(12.))
        .child(name.to_string())
        .when(!detail.is_empty(), |d| {
            d.child(div().text_color(rgb(colors().muted)).child(detail))
        })
        .child(
            div()
                .id(id)
                .px(px(4.))
                .rounded(px(4.))
                .text_color(rgb(colors().muted))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(colors().text)))
                .on_click(cx.listener(move |e, _, _, cx| {
                    remove(e, cx);
                    cx.notify();
                }))
                .child("✕"),
        )
}

fn dashed(id: &str, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .debug_selector(|| id.to_string())
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(13.))
        .border_1()
        .border_dashed()
        .border_color(rgb(colors().edge))
        .text_size(ui(12.))
        .text_color(rgb(colors().label))
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(colors().text)))
        .child(label.to_string())
}

impl Focusable for HostEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HostEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // At the window's right edge: its bottom corner is the window's.
        let corner = crate::chrome::BottomCorners::current(cx).right;
        let shell = div()
            .id("host-editor")
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
            .text_color(rgb(colors().text));
        if let Mode::SshConfig { alias, resolved } = &self.mode {
            let (alias, resolved) = (alias.clone(), resolved.clone());
            return shell.child(self.render_ssh(&alias, resolved.as_deref(), cx));
        }

        let i = &self.inputs;
        let title = match self.text(&i.name, cx) {
            n if n.is_empty() => tr!("New host"),
            n => n,
        };
        let note = if self.original.is_some() {
            tr!("Saved in hosts.toml")
        } else {
            tr!("Not saved yet")
        };
        // The host's own color, else its group's.
        let color = self.current(cx).color_rgb().or_else(|| {
            self.group
                .as_ref()
                .and_then(|g| self.ctx.hosts.groups.iter().find(|x| &x.name == g))
                .and_then(|g| g.color_rgb())
        });

        let connection = section(&tr!("Connection"))
            .child(
                row()
                    .child(self.field(&tr!("Name"), &i.name, Field::Name, window, cx))
                    .child(self.field(
                        &tr!("Alias for the command line"),
                        &i.alias,
                        Field::Alias,
                        window,
                        cx,
                    )),
            )
            .child(
                row()
                    .child(
                        self.field(&tr!("Address"), &i.address, Field::Address, window, cx)
                            .flex_grow(3.),
                    )
                    .child(
                        self.field(&tr!("Port"), &i.port, Field::Port, window, cx)
                            .flex_grow(1.),
                    )
                    .child(
                        self.field(&tr!("User"), &i.user, Field::User, window, cx)
                            .flex_grow(2.),
                    ),
            );

        let mut seg = div()
            .flex()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(colors().field_edge))
            .overflow_hidden();
        for (n, m) in [
            AuthMethod::Auto,
            AuthMethod::Key,
            AuthMethod::Password,
            AuthMethod::Agent,
        ]
        .into_iter()
        .enumerate()
        {
            let on = self.method == m;
            seg = seg.child(
                div()
                    .id(("method", n))
                    .debug_selector(|| format!("method:{n}"))
                    .flex_1()
                    .h(px(30.))
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
                    .on_click(cx.listener(move |e, _, _, cx| {
                        e.method = m;
                        e.problems.retain(|(f, _)| *f != Field::Key);
                        cx.notify();
                    }))
                    .child(method_label(m)),
            );
        }
        let mut sign_in = section(&tr!("Sign in")).child(seg);
        let key_text = self.text(&i.key, cx);
        let chosen_key = self.ctx.keys.iter().find(|k| k.value == key_text);
        if self.method == AuthMethod::Key {
            let shown = match chosen_key {
                Some(k) if k.vault => tr!(
                    "{name} · in the vault · {kind}",
                    name = &k.name,
                    kind = &k.kind
                ),
                Some(k) => format!("{} · {}", k.value, k.kind),
                None if key_text.is_empty() => tr!("Choose a key"),
                None => key_text.clone(),
            };
            sign_in = sign_in.child(row().child(self.select(
                "key-select",
                &tr!("Key"),
                shown,
                Menu::Key,
                cx,
            )));
            if self.menu == Some(Menu::Key) {
                let mut choices: Vec<(Option<String>, String)> = self
                    .ctx
                    .keys
                    .iter()
                    .map(|k| {
                        let label = if k.vault {
                            tr!(
                                "{name}   in the vault · {kind}",
                                name = &k.name,
                                kind = &k.kind
                            )
                        } else {
                            format!("{}   {}", k.value, k.kind)
                        };
                        (Some(k.value.clone()), label)
                    })
                    .collect();
                choices.push((None, tr!("Another key file…")));
                let host = self.text(&i.name, cx);
                choices.push((
                    Some(GENERATE.to_string()),
                    if host.is_empty() {
                        tr!("+ Generate a key for this host")
                    } else {
                        tr!("+ Generate a key for {host}", host = &host)
                    },
                ));
                sign_in = sign_in.child(self.menu_list(Menu::Key, choices, cx));
            }
            // A key made here: the host has to be given it.
            if let Some(k) = self.new_key.clone()
                && k.vault_id
                    .as_deref()
                    .is_some_and(|id| key_text == format!("{}{id}", crate::target::VAULT_KEY))
            {
                let host = self.text(&i.name, cx);
                let host = if host.is_empty() {
                    tr!("the host")
                } else {
                    host
                };
                sign_in = sign_in.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(div().flex_1().min_w_0().child(hint_text(&tr!(
                            "{name} is new: {host} does not have it yet. Saving and installing signs in once another way (a password, say), then adds it.",
                            name = &k.name,
                            host = &host
                        ))))
                        .child(
                            primary_button("save-install", &tr!("Save and install on {host}", host = &host))
                                .flex_shrink_0()
                                .on_click(cx.listener(move |e, _, _, cx| {
                                    let key = e.new_key.clone();
                                    e.save_with(false, key, cx);
                                })),
                        ),
                );
            }
            if let Some((_, p)) = self.problems.iter().find(|(f, _)| *f == Field::Key) {
                sign_in = sign_in.child(problem_text(p));
            }
        }
        sign_in = sign_in.child(hint_text(&match self.method {
            AuthMethod::Auto => {
                tr!("Tries the agent, then your keys in ~/.ssh and the vault, then asks for a password, like ssh.")
            }
            AuthMethod::Key if chosen_key.is_some_and(|k| k.vault) => {
                tr!("Kept in the vault: no passphrase to ask for.")
            }
            AuthMethod::Key => {
                tr!("Passphrase: asked for when needed, and can be saved in the vault.")
            }
            AuthMethod::Password => {
                tr!("Asked for when connecting, and can be saved in the vault.")
            }
            AuthMethod::Agent => tr!("Uses the keys in your ssh-agent."),
        }));

        // Route: the jump hosts, in order, then this host.
        let mut chain = div().flex().flex_wrap().items_center().gap(px(6.));
        for (n, j) in self.jumps.iter().enumerate() {
            let name = j.clone();
            chain = chain
                .child(chip(
                    ("remove-jump", n).into(),
                    j,
                    self.jump_detail(j),
                    move |e, _| e.jumps.retain(|x| *x != name),
                    cx,
                ))
                .child(div().text_color(rgb(colors().faint)).child("→"));
        }
        chain = chain.child(
            div()
                .text_size(ui(12.))
                .text_color(rgb(colors().soft))
                .child(title.clone()),
        );
        if !self.adding_jump {
            chain = chain.child(dashed("add-jump", &tr!("+ Add jump")).ml_1().on_click(
                cx.listener(|e, _, window, cx| {
                    e.adding_jump = true;
                    window.focus(&e.inputs.jump.focus_handle(cx), cx);
                    cx.notify();
                }),
            ));
        }
        let mut route = section(&tr!("Route"))
            .child(label_text(&tr!("Jump through, in order")))
            .child(chain);
        if self.adding_jump {
            let typed = self.text(&i.jump, cx).to_lowercase();
            let me = self.text(&i.name, cx);
            let mut seen = vec![];
            let suggestions: Vec<String> = self
                .ctx
                .hosts
                .hosts
                .iter()
                .map(|h| h.name.clone())
                .chain(self.ctx.ssh_aliases.iter().cloned())
                .filter(|n| *n != me && !self.jumps.contains(n))
                .filter(|n| typed.is_empty() || n.to_lowercase().contains(&typed))
                .filter(|n| {
                    let new = !seen.contains(n);
                    seen.push(n.clone());
                    new
                })
                .take(8)
                .collect();
            let mut picks = div().flex().flex_wrap().gap(px(6.));
            for (n, s) in suggestions.into_iter().enumerate() {
                let name = s.clone();
                picks =
                    picks.child(dashed(&format!("pick-{n}"), &s).on_click(
                        cx.listener(move |e, _, _, cx| e.add_jump_named(name.clone(), cx)),
                    ));
            }
            route = route
                .child(self.input_box(&i.jump, self.problem(Field::Jump).is_some(), window, cx))
                .child(picks)
                .child(hint_text(&tr!(
                    "Enter adds what you typed · Esc stops adding"
                )));
        }
        if let Some(p) = self.problem(Field::Jump) {
            route = route.child(problem_text(p));
        }

        let mut forwards = div().flex().flex_wrap().items_center().gap(px(6.));
        for (n, f) in self.forwards.iter().enumerate() {
            let spec = f.clone();
            forwards = forwards.child(chip(
                ("remove-forward", n).into(),
                f,
                String::new(),
                move |e, _| e.forwards.retain(|x| *x != spec),
                cx,
            ));
        }
        if !self.adding_forward {
            forwards = forwards.child(dashed("add-forward", &tr!("+ Add forward")).on_click(
                cx.listener(|e, _, window, cx| {
                    e.adding_forward = true;
                    window.focus(&e.inputs.forward.focus_handle(cx), cx);
                    cx.notify();
                }),
            ));
        }
        let mut forwarding = section(&tr!("Port forwarding")).child(forwards);
        if self.adding_forward {
            forwarding = forwarding
                .child(self.input_box(
                    &i.forward,
                    self.problem(Field::Forward).is_some(),
                    window,
                    cx,
                ))
                .child(hint_text(
                    "-L [bind:]port:host:hostport · -R [bind:]port:host:hostport · -D [bind:]port",
                ));
        }
        if let Some(p) = self.problem(Field::Forward) {
            forwarding = forwarding.child(problem_text(p));
        }
        forwarding = forwarding.child(hint_text(&tr!("Set up every time this host connects.")));

        let sync = section(&tr!("Sync"))
            .child(
                div()
                    .id("local-only")
                    .debug_selector(|| "local-only".into())
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(|e, _, _, cx| {
                        e.local_only = !e.local_only;
                        cx.notify();
                    }))
                    .child(checkbox(self.local_only))
                    .child(label_text(&tr!("Keep on this device only"))),
            )
            .child(hint_text(&if self.local_only {
                tr!("Not synced: other devices do not see this host.")
            } else {
                tr!("Synced with your other devices when sync is on.")
            }));

        let group_label = if self.naming_group {
            tr!("New group")
        } else {
            self.group.clone().unwrap_or_else(|| tr!("None"))
        };
        let theme_label = self
            .theme
            .as_ref()
            .map(|t| {
                self.ctx
                    .themes
                    .iter()
                    .find(|(id, _)| id == t)
                    .map(|(_, n)| tr!("{n} (this host only)", n = n))
                    .unwrap_or_else(|| t.clone())
            })
            .unwrap_or_else(|| tr!("Same as the app"));
        let mut organize = section(&tr!("Organize")).child(
            row()
                .child(self.select("group-select", &tr!("Group"), group_label, Menu::Group, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flex_1()
                        .min_w_0()
                        .child(label_text(&tr!("Tags")))
                        .child(self.input_box(&i.tags, false, window, cx)),
                ),
        );
        if self.menu == Some(Menu::Group) {
            let mut choices = vec![(None, tr!("None"))];
            choices.extend(
                self.ctx
                    .hosts
                    .groups
                    .iter()
                    .map(|g| (Some(g.name.clone()), g.name.clone())),
            );
            if let Some(g) = &self.group
                && !self.ctx.hosts.groups.iter().any(|x| &x.name == g)
            {
                choices.push((Some(g.clone()), g.clone()));
            }
            organize = organize
                .child(self.menu_list(Menu::Group, choices, cx))
                .child(
                    dashed("new-group", &tr!("+ New group…")).on_click(cx.listener(
                        |e, _, window, cx| {
                            e.menu = None;
                            e.naming_group = true;
                            window.focus(&e.inputs.new_group.focus_handle(cx), cx);
                            cx.notify();
                        },
                    )),
                );
        }
        if self.naming_group {
            organize = organize
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label_text(&tr!("New group's name")))
                        .child(self.input_box(&i.new_group, false, window, cx)),
                )
                .child(hint_text(&tr!("Made when you save this host.")));
        }
        // The colors, as round swatches; the chosen one ringed.
        let mut swatches = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().mr(px(6.)).child(label_text(&tr!("Color"))));
        let none_on = self.color.is_none();
        swatches = swatches.child(
            div()
                .id("color-none")
                .debug_selector(|| "color-none".into())
                .size(px(20.))
                .rounded_full()
                .border_2()
                .border_color(rgb(if none_on {
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
                    .id(SharedString::from(format!("color-{name}")))
                    .debug_selector(|| format!("color-{name}"))
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
        swatches =
            swatches.child(hint_text(&tr!("Shown on the tab and the pane border")).ml(px(8.)));
        organize = organize.child(swatches).child(
            div()
                .id("favorite")
                .debug_selector(|| "favorite".into())
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(|e, _, _, cx| {
                    e.favorite = !e.favorite;
                    cx.notify();
                }))
                .child(checkbox(self.favorite))
                .child(label_text(&tr!("Favorite: listed first in the host list"))),
        );

        let mut terminal = section(&tr!("Terminal")).child(row().child(self.select(
            "theme-select",
            &tr!("Theme"),
            theme_label,
            Menu::Theme,
            cx,
        )));
        if self.menu == Some(Menu::Theme) {
            let mut choices = vec![(None, tr!("Same as the app"))];
            choices.extend(
                self.ctx
                    .themes
                    .iter()
                    .map(|(id, name)| (Some(id.clone()), name.clone())),
            );
            terminal = terminal.child(self.menu_list(Menu::Theme, choices, cx));
        }
        terminal = terminal
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(&tr!("Run after connecting")))
                    .child(self.input_box(&i.run, false, window, cx)),
            )
            .child(hint_text(&tr!(
                "Typed into the shell once it is ready, after every connect and reconnect."
            )));

        let keys = Self::segments(
            "host-key",
            &[
                (None, tr!("App setting").as_str()),
                (Some(HostKey::Ask), tr!("Ask").as_str()),
                (Some(HostKey::AcceptNew), tr!("Accept new").as_str()),
                (Some(HostKey::Strict), tr!("Strict").as_str()),
            ],
            self.host_key,
            |e, v| e.host_key = v,
            cx,
        );
        let logs = Self::segments(
            "host-log",
            &[
                (None, tr!("App setting").as_str()),
                (Some(true), tr!("Always").as_str()),
                (Some(false), tr!("Never").as_str()),
            ],
            self.log,
            |e, v| e.log = v,
            cx,
        );
        let app = &self.ctx.app_ssh;
        let app_key = match app.host_key {
            HostKey::Ask => tr!("Ask"),
            HostKey::AcceptNew => tr!("Accept new"),
            HostKey::Strict => tr!("Strict"),
        };
        let app_alive = match app.keepalive {
            0 => tr!("none"),
            s => tr!("{s} s", s = s),
        };
        let mut advanced = section(&tr!("Advanced"))
            .border_b_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(&tr!("Unknown host key")))
                    .child(keys)
                    .child(hint_text(&tr!(
                        "The app's setting is {app_key} now. A changed key is always refused.",
                        app_key = &app_key
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label_text(&tr!("Session log")))
                    .child(logs)
                    .child(hint_text(&tr!(
                        "Always: each session is logged to a file from its start. Never: not even when the app logs every session. The app logs {logs} now.",
                        logs = if self.ctx.app_logs {
                            tr!("every session")
                        } else {
                            tr!("none")
                        }
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label_text(&tr!("Keepalive")))
                            .child(self.input_box(
                                &i.keepalive,
                                self.problem(Field::Keepalive).is_some(),
                                window,
                                cx,
                            )),
                    )
                    .child(
                        hint_text(&tr!(
                            "Seconds between checks; 0: none. Empty: the app's setting ({app_alive}).",
                            app_alive = &app_alive
                        ))
                        .pb(px(7.)),
                    ),
            );
        if let Some(p) = self.problem(Field::Keepalive) {
            advanced = advanced.child(problem_text(p));
        }
        let advanced = advanced
            .child(
                div()
                    .id("forward-agent")
                    .debug_selector(|| "forward-agent".into())
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(|e, _, _, cx| {
                        e.forward_agent = !e.forward_agent;
                        cx.notify();
                    }))
                    .child(checkbox(self.forward_agent))
                    .child(label_text(&tr!("Forward my agent to this host"))),
            )
            .child(hint_text(&if self.forward_agent {
                tr!("While you are connected, whoever is root on this host can sign in elsewhere with your keys too. Only for hosts you trust.")
            } else {
                tr!("From this host you could then git push or ssh further with the keys on this computer; they are never copied there.")
            }));

        let mut footer = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .px(px(20.))
            .py(px(12.))
            .border_t_1()
            .border_color(rgb(colors().line))
            .rounded_br(corner)
            .bg(rgb(colors().panel));
        if let Some(p) = &self.save_problem {
            footer = footer.child(problem_text(p));
        }
        if !self.problems.is_empty() {
            footer = footer.child(problem_text(&tr!("Some fields need a look.")));
        }
        let buttons = if self.confirm_delete {
            // The question and its answers alone: a long name wraps, and
            // saving waits for the answer.
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(ui(12.5))
                        .child(tr!("Delete {title} from hosts.toml?", title = &title)),
                )
                .child(
                    text_button("delete-yes", &tr!("Delete"), colors().bad)
                        .on_click(cx.listener(|e, _, _, cx| e.delete(cx))),
                )
                .child(
                    text_button("delete-no", &tr!("Cancel"), colors().label).on_click(cx.listener(
                        |e, _, _, cx| {
                            e.confirm_delete = false;
                            cx.notify();
                        },
                    )),
                )
        } else {
            let mut others = div().flex().items_center().gap_2();
            if self.original.is_some() {
                others = others
                    .child(
                        text_button("delete", &tr!("Delete"), colors().bad)
                            .on_click(cx.listener(|e, _, _, cx| e.delete(cx))),
                    )
                    .child(
                        text_button("duplicate", &tr!("Duplicate"), colors().label)
                            .on_click(cx.listener(|e, _, window, cx| e.duplicate(window, cx))),
                    );
            }
            let saving = div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    outline_button("save", &tr!("Save"))
                        .on_click(cx.listener(|e, _, _, cx| e.save(false, cx))),
                )
                .child(
                    primary_button("save-connect", &tr!("Save and connect"))
                        .on_click(cx.listener(|e, _, _, cx| e.save(true, cx))),
                );
            // Too long for one line (a long translation): the saving buttons
            // go to the next, still on the right.
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(others)
                .child(saving)
        };
        footer = footer.child(buttons);

        shell
            .child(header(&title, &note, color, cx))
            .child(
                div()
                    .id("editor-body")
                    .debug_selector(|| "editor-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(connection)
                    .child(sign_in)
                    .child(route)
                    .child(forwarding)
                    .child(sync)
                    .child(organize)
                    .child(terminal)
                    .child(advanced),
            )
            .child(footer)
    }
}

pub(crate) fn checkbox(on: bool) -> gpui::Div {
    div()
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(if on {
            colors().accent
        } else {
            colors().field_edge
        }))
        .bg(rgb(if on { colors().accent } else { colors().field }))
        .text_size(ui(11.))
        .text_color(rgb(colors().on_accent))
        .when(on, |d| d.child("✓"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_paths_are_written_with_a_tilde() {
        if let Some(home) = std::env::home_dir() {
            assert_eq!(tilde(&home.join(".ssh/id_ed25519")), "~/.ssh/id_ed25519");
        }
        assert_eq!(tilde(std::path::Path::new("/etc/keys/k")), "/etc/keys/k");
    }
}

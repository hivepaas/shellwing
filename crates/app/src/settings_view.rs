//! Settings: a page over the window, as the approved sketch draws it, with the
//! sections on the left. Every change is written to `config.toml` at once (its
//! comments kept) and applies to the open tabs; the file stays the source.
//! Only settings the app has are shown; the sketch's others come with them.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, Render,
    SharedString, WeakEntity, Window, div, prelude::*, px, rgb, uniform_list,
};
use shellwing_config::{
    Color, ColorMode, CursorStyle, GitSync, HostKey, LogFormat, OnDisconnect, S3Sync, Scrollbar,
    SettingValue, Settings, SyncPlace, Theme, WebDavSync,
};

use shellwing_i18n::{tr, tr_n};

use crate::colors::colors;
use crate::input::TextInput;
use crate::keys_view::KeysView;
use crate::known_hosts_view::KnownHostsView;
use crate::look::ui;
use crate::notice::{Notice, colored};
use crate::palette::Action;
use crate::snippets_view::SnippetsView;
use crate::workspace::Workspace;

mod about;
mod keyboard;

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
    Environments,
    ConfigFiles,
    Notes,
    Sync,
    Vault,
    About,
}

const PAGES: [Page; 14] = [
    Page::General,
    Page::Appearance,
    Page::Terminal,
    Page::Keyboard,
    Page::Ssh,
    Page::Keys,
    Page::KnownHosts,
    Page::Snippets,
    Page::Environments,
    Page::ConfigFiles,
    Page::Notes,
    Page::Sync,
    Page::Vault,
    Page::About,
];

impl Page {
    /// The page's name, as the list on the left shows it.
    fn title(self) -> String {
        match self {
            Page::General => tr!("General"),
            Page::Appearance => tr!("Appearance"),
            Page::Terminal => tr!("Terminal"),
            Page::Keyboard => tr!("Keyboard"),
            Page::Ssh => tr!("SSH and security"),
            Page::Keys => tr!("Keys"),
            Page::KnownHosts => tr!("Known hosts"),
            Page::Snippets => tr!("Snippets"),
            Page::Environments => tr!("Environments"),
            Page::ConfigFiles => tr!("Config files"),
            Page::Notes => tr!("Notes"),
            Page::Sync => tr!("Sync"),
            Page::Vault => tr!("Vault"),
            Page::About => tr!("About"),
        }
    }
}

/// What the page shows, taken from the workspace.
#[derive(Clone)]
pub struct Snap {
    pub settings: Settings,
    /// The app is light now: the theme chosen is the light mode's.
    pub light: bool,
    pub themes: Vec<Arc<Theme>>,
    /// Installed fonts that look monospaced.
    pub fonts: Vec<String>,
    /// The font used when none is set.
    pub default_font: String,
    /// Every installed font, for the app's own text.
    pub all_fonts: Vec<String>,
    /// Text editors found on this system: (name, command).
    pub editors: Vec<(String, String)>,
    /// Where the vault syncs now.
    pub sync_place: Option<SyncPlace>,
    pub config_dir: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
    pub ssh_config: Option<PathBuf>,
    pub secrets: Option<usize>,
    /// The vault waits for its passphrase or recovery key.
    pub vault_locked: bool,
    /// A passphrase keeps the vault's key; None while it is not open.
    pub vault_passphrase: Option<bool>,
    /// The system keychain keeps the vault's key.
    pub vault_keychain: bool,
    /// It is open, and its passphrase alone opens it again.
    pub vault_can_lock: bool,
    /// Windows Hello or Touch ID, where this system has it, and whether it
    /// opens the vault.
    pub presence: Option<(&'static str, bool)>,
    /// A passphrase joins new devices to the vault in the sync folder; None
    /// while not syncing.
    pub passphrase_joins: Option<bool>,
    /// How sync stands, as the cloud's panel says.
    pub sync: crate::sync_panel::SyncSnap,
    /// Whether a newer shellwing is out.
    pub update: crate::workspace::UpdateSnap,
}

pub struct SettingsView {
    focus: FocusHandle,
    /// What the app has to say while Settings covers the tabs (where it
    /// would say it otherwise), and which saying it is, to put it away.
    notice: Option<(Notice, u64)>,
    notices: u64,
    /// Gradient themes drawn for their tiles and the preview, by theme and
    /// size: GPUI's own gradients take two stops, these take them all.
    gradients:
        std::cell::RefCell<std::collections::HashMap<(String, u32, u32), Arc<gpui::RenderImage>>>,
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
    /// The Environments page, once shown.
    environments: Option<Entity<SnippetsView>>,
    /// The Config files page, once shown.
    config_files: Option<Entity<SnippetsView>>,
    /// The Notes page, once shown.
    notes: Option<Entity<SnippetsView>>,
    /// The Known hosts page, once shown.
    known_hosts: Option<Entity<KnownHostsView>>,
    /// Where to sync, as typed before connecting.
    sync_form: SyncForm,
    /// The Keyboard page taking a key, or asking about one.
    keyboard: keyboard::Keyboard,
}

/// The Sync page's choice of a place, before syncing starts.
struct SyncForm {
    kind: SyncKind,
    /// Address, user, password.
    dav: [Entity<TextInput>; 3],
    /// Endpoint, region, bucket, folder in it, access key, secret key.
    s3: [Entity<TextInput>; 6],
    /// Repository, branch, token, the token's user.
    git: [Entity<TextInput>; 4],
    /// What is missing, after Connect.
    problem: Option<String>,
    /// Why git cannot be used here, after Connect, with how to install it.
    no_git: Option<shellwing_remote::NoGit>,
    /// Installing git now.
    installing: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncKind {
    Folder,
    WebDav,
    S3,
    Git,
}

impl SyncForm {
    fn new(sync: &shellwing_config::Sync, cx: &mut Context<SettingsView>) -> SyncForm {
        // Placeholders said again when drawn: they follow the language.
        fn field(
            hint: impl Fn() -> String + 'static,
            text: &str,
            masked: bool,
            cx: &mut Context<SettingsView>,
        ) -> Entity<TextInput> {
            cx.new(|cx| {
                let mut i = TextInput::said(hint, cx);
                if masked {
                    i = i.masked();
                }
                i.set_text(text, text.len()..text.len(), cx);
                i
            })
        }
        let dav = sync.webdav.clone().unwrap_or_default();
        let s3 = sync.s3.clone().unwrap_or_default();
        let git = sync.git.clone().unwrap_or_default();
        SyncForm {
            kind: if sync.git.is_some() {
                SyncKind::Git
            } else if sync.s3.is_some() {
                SyncKind::S3
            } else if sync.webdav.is_some() {
                SyncKind::WebDav
            } else {
                SyncKind::Folder
            },
            dav: [
                field(
                    || "https://cloud.example.com/remote.php/dav/files/you/".to_string(),
                    &dav.url,
                    false,
                    cx,
                ),
                field(|| "you".to_string(), &dav.user, false, cx),
                field(
                    || tr!("Its password (an app password, where it has them)"),
                    "",
                    true,
                    cx,
                ),
            ],
            s3: [
                field(
                    || "https://s3.eu-west-1.amazonaws.com".to_string(),
                    &s3.endpoint,
                    false,
                    cx,
                ),
                field(|| "auto".to_string(), &s3.region, false, cx),
                field(|| "my-bucket".to_string(), &s3.bucket, false, cx),
                field(|| tr!("none: the top of the bucket"), &s3.prefix, false, cx),
                field(|| "AKIA…".to_string(), &s3.access_key, false, cx),
                field(|| tr!("Its secret"), "", true, cx),
            ],
            git: [
                field(
                    || tr!("https://github.com/you/vault.git, or git@github.com:you/vault.git"),
                    &git.url,
                    false,
                    cx,
                ),
                field(|| tr!("its default"), &git.branch, false, cx),
                field(
                    || tr!("none: git signs in as it does for you"),
                    "",
                    true,
                    cx,
                ),
                field(|| tr!("any (Gitea: yours)"), &git.user, false, cx),
            ],
            problem: None,
            no_git: None,
            installing: false,
        }
    }

    /// The fields of the place chosen, in order.
    fn fields(&self) -> &[Entity<TextInput>] {
        match self.kind {
            SyncKind::Folder => &[],
            SyncKind::WebDav => &self.dav,
            SyncKind::S3 => &self.s3,
            SyncKind::Git => &self.git,
        }
    }

    /// The place typed and its secret (none for Git without a token), or
    /// what is missing.
    fn place(&self, cx: &gpui::App) -> Result<(SyncPlace, Option<String>), String> {
        let text = |i: &Entity<TextInput>| i.read(cx).text().trim().to_string();
        match self.kind {
            SyncKind::Folder => Err(tr!("choose a folder")),
            SyncKind::WebDav => {
                let [url, user, password] = &self.dav;
                let (url, user) = (text(url), text(user));
                let password = password.read(cx).text().to_string();
                if url.is_empty() || user.is_empty() || password.is_empty() {
                    return Err(tr!("Fill in the address, the user and the password."));
                }
                if !url.starts_with("https://") && !url.starts_with("http://") {
                    return Err(tr!("The address starts with https://."));
                }
                Ok((SyncPlace::WebDav(WebDavSync { url, user }), Some(password)))
            }
            SyncKind::S3 => {
                let [endpoint, region, bucket, prefix, access_key, secret] = &self.s3;
                let place = S3Sync {
                    endpoint: text(endpoint),
                    region: text(region),
                    bucket: text(bucket),
                    prefix: text(prefix),
                    access_key: text(access_key),
                };
                let secret = secret.read(cx).text().to_string();
                if place.endpoint.is_empty()
                    || place.bucket.is_empty()
                    || place.access_key.is_empty()
                    || secret.is_empty()
                {
                    return Err(tr!(
                        "Fill in the endpoint, the bucket, the access key and its secret."
                    ));
                }
                if !place.endpoint.starts_with("https://") && !place.endpoint.starts_with("http://")
                {
                    return Err(tr!("The endpoint starts with https://."));
                }
                Ok((SyncPlace::S3(place), Some(secret)))
            }
            SyncKind::Git => {
                let [url, branch, token, user] = &self.git;
                let place = GitSync {
                    url: text(url),
                    branch: text(branch),
                    user: text(user),
                };
                if place.url.is_empty() {
                    return Err(tr!("Fill in the repository's address."));
                }
                let token = token.read(cx).text().trim().to_string();
                Ok((SyncPlace::Git(place), (!token.is_empty()).then_some(token)))
            }
        }
    }
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
            let yours = crate::local::shell(None).join(" ");
            let mut i = TextInput::said(move || tr!("{shell} (yours)", shell = &yours), cx);
            i.set_text(&shell, shell.len()..shell.len(), cx);
            i
        });
        let editor_input = cx.new(|cx| {
            let mut i = TextInput::said(|| tr!("e.g. code -n, or /path/to/editor --flag"), cx);
            i.set_text(&text, text.len()..text.len(), cx);
            i
        });
        let sync_form = SyncForm::new(&snap.settings.sync, cx);
        SettingsView {
            notice: None,
            notices: 0,
            sync_form,
            gradients: Default::default(),
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
            environments: None,
            config_files: None,
            notes: None,
            known_hosts: None,
            keyboard: Default::default(),
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

    /// A note changed in its window: the Notes page lists it again.
    /// A snippet saved or deleted in its window: the list shows it.
    pub fn snippets_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = &self.snippets {
            n.update(cx, |n, cx| n.refresh(cx));
        }
    }

    pub fn notes_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = &self.notes {
            n.update(cx, |n, cx| n.refresh(cx));
        }
    }

    pub fn set_snap(&mut self, snap: Snap, cx: &mut Context<Self>) {
        self.snap = snap;
        cx.notify();
    }

    /// Writes one setting; the workspace applies it and hands back the result.
    fn change(&mut self, path: &[&str], value: Option<SettingValue>, cx: &mut Context<Self>) {
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

    /// Asks for the folder logs go to.
    fn choose_log_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("Keep logs here").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |v, cx| {
                    let folder = SettingValue::Str(folder.display().to_string());
                    v.change(&["log", "folder"], Some(folder), cx)
                });
            }
        })
        .detach();
    }

    /// Starts syncing with the server typed on the Sync page.
    fn connect_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.sync_form.place(cx) {
            Ok((place, secret)) => {
                self.sync_form.problem = None;
                // Git: the system's, or how to get it, before anything else.
                self.sync_form.no_git = None;
                if matches!(place, SyncPlace::Git(_))
                    && let Err(e) = shellwing_remote::Git::find()
                {
                    self.sync_form.no_git = Some(e);
                    return cx.notify();
                }
                // Kept in the vault from here on, not in the field.
                for f in [
                    &self.sync_form.dav[2],
                    &self.sync_form.s3[5],
                    &self.sync_form.git[2],
                ] {
                    f.update(cx, |i, cx| i.set_text("", 0..0, cx));
                }
                window.focus(&self.focus, cx);
                let _ = self
                    .workspace
                    .update(cx, |ws, cx| ws.connect_sync(place, secret, window, cx));
            }
            Err(e) => self.sync_form.problem = Some(e),
        }
        cx.notify();
    }

    /// Installs git where the system asks for the rights itself, then looks
    /// for it again.
    fn install_git(&mut self, how: shellwing_remote::Install, cx: &mut Context<Self>) {
        self.sync_form.installing = true;
        cx.notify();
        let job = cx.background_spawn(async move {
            shellwing_remote::git::install(&how)?;
            Ok::<_, std::io::Error>(shellwing_remote::Git::find())
        });
        cx.spawn(async move |this, cx| {
            let done = job.await;
            let _ = this.update(cx, |v, cx| {
                let form = &mut v.sync_form;
                form.installing = false;
                match done {
                    Ok(Ok(git)) => {
                        form.no_git = None;
                        form.problem = None;
                        v.say(
                            Notice::good(tr!(
                                "git {version} is installed: Connect again.",
                                version = git.version()
                            )),
                            cx,
                        );
                    }
                    // Apple's dialog goes on by itself after this returns.
                    Ok(Err(_)) if cfg!(target_os = "macos") => {
                        form.problem = Some(tr!("When Apple's installer is done, Connect again."));
                    }
                    Ok(Err(e)) => form.no_git = Some(e),
                    Err(e) => {
                        form.problem = Some(tr!("git was not installed: {e}", e = e.to_string()))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A notice from the page, through the workspace.
    fn say(&mut self, msg: impl Into<Notice>, cx: &mut Context<Self>) {
        self.show_notice(msg, Duration::from_secs(8), cx);
    }

    /// Shows `msg` over the page for `shown`.
    pub fn show_notice(&mut self, msg: impl Into<Notice>, shown: Duration, cx: &mut Context<Self>) {
        self.notices += 1;
        let n = self.notices;
        self.notice = Some((msg.into(), n));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(shown).await;
            let _ = this.update(cx, |v, cx| {
                if v.notice.as_ref().is_some_and(|(_, at)| *at == n) {
                    v.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Connecting to the sync place failed: said beside Connect, until the
    /// next try.
    pub fn sync_failed(&mut self, why: String, cx: &mut Context<Self>) {
        self.sync_form.problem = Some(why);
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.record_key(ev, cx) {
            return cx.stop_propagation();
        }
        let k = &ev.keystroke;
        // Typing a server's address and sign-in: Enter connects, Tab goes on.
        let fields = self.sync_form.fields().to_vec();
        if let Some(at) = fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(window))
        {
            match k.key.as_str() {
                "enter" => self.connect_sync(window, cx),
                "escape" => window.focus(&self.focus, cx),
                "tab" => {
                    let next = if k.modifiers.shift {
                        at.checked_sub(1).unwrap_or(fields.len() - 1)
                    } else {
                        (at + 1) % fields.len()
                    };
                    window.focus(&fields[next].focus_handle(cx), cx);
                }
                _ => return,
            }
            cx.notify();
            return cx.stop_propagation();
        }
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
            let at = PAGES.iter().position(|p| *p == v.page).unwrap_or(0) as isize;
            v.stop_recording();
            v.page = PAGES[(at + by).rem_euclid(PAGES.len() as isize) as usize];
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
                    .debug_selector(|| format!("{id}:{n}"))
                    .h(px(28.))
                    .min_w(px(40.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if on {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(if on { colors().selected } else { colors().page }))
                    .text_size(ui(12.5))
                    .text_color(rgb(if on { colors().text } else { colors().soft }))
                    .cursor_pointer()
                    .when(!on, |d| d.hover(|s| s.bg(rgb(colors().hover))))
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
                tr!("Default ({font})", font = &self.snap.default_font),
                &self.snap.fonts,
            ),
            Fonts::Ui => (tr!("Default (the system's)"), &self.snap.all_fonts),
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
            .child(label(&tr!("Font")))
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
                    .border_color(rgb(if open {
                        colors().accent
                    } else {
                        colors().field_edge
                    }))
                    .bg(rgb(colors().page))
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
                    .child(div().text_color(rgb(colors().muted)).child("▾")),
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
                                .when(on, |d| d.bg(rgb(colors().selected)))
                                .hover(|s| s.bg(rgb(colors().hover)))
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
                    .border_color(rgb(colors().edge))
                    .bg(rgb(colors().raised))
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
                    .child(heading2(&tr!("App text")))
                    .child(label(&tr!(
                        "Tabs, the host list, the palette, panels and this page."
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .items_start()
                    .child(self.font_picker(Fonts::Ui, cx))
                    .child(labelled(
                        &tr!("Size"),
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
        let mut choices: Vec<(usize, String)> = vec![(0, tr!("The system's"))];
        for (n, (name, _)) in self.snap.editors.iter().enumerate() {
            choices.push((n + 1, name.clone()));
        }
        let custom_at = choices.len();
        choices.push((custom_at, tr!("Custom…")));
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
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().page))
                        .text_size(ui(13.))
                        .child(self.editor_input.clone()),
                )
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(colors().muted))
                        .child(tr!("The file's path is added at the end. Enter saves.")),
                );
        }
        // The language: the system's (0), or one of those there are (1…),
        // each named in itself.
        let chosen = self.snap.settings.general.language.clone();
        let current = shellwing_i18n::LANGUAGES
            .iter()
            .position(|(t, _)| chosen.as_deref().map(str::trim) == Some(*t))
            .map_or(0, |i| i + 1);
        let system = shellwing_i18n::system_language();
        let system_name = shellwing_i18n::LANGUAGES
            .iter()
            .find(|(t, _)| *t == system)
            .map_or(system, |(_, n)| *n);
        let mut languages: Vec<(usize, String)> =
            vec![(0, tr!("As the system ({language})", language = system_name))];
        languages.extend(
            shellwing_i18n::LANGUAGES
                .iter()
                .enumerate()
                .map(|(i, (_, n))| (i + 1, (*n).to_string())),
        );
        let language = self.pills(
            "language",
            languages,
            current,
            |v, i, cx| {
                let tag = i
                    .checked_sub(1)
                    .and_then(|i| shellwing_i18n::LANGUAGES.get(i))
                    .map(|(t, _)| SettingValue::Str((*t).to_string()));
                v.change(&["general", "language"], tag, cx)
            },
            cx,
        );
        let mut page = page(&tr!("General"))
            .child(setting(
                &tr!("Language"),
                &tr!("shellwing's own words: menus, Settings, questions and notices. What the terminal shows is the server's. This device's own: it does not sync."),
                language,
            ))
            .child(setting(
                &tr!("Text editor for remote files"),
                &tr!("Edit in text editor, in the Files panel, opens a copy with it and writes the file back each time you save. The system's is the desktop's app for plain text."),
                editor_box,
            ))
            .child(toggle(
            "restore-tabs",
            restore,
            &tr!("Open the last tabs at start"),
            &tr!("When shellwing starts without a host, it opens the tabs and splits it had."),
            cx.listener(move |v, _, _, cx| {
                v.change(
                    &["general", "restore_tabs"],
                    Some(SettingValue::Bool(!restore)),
                    cx,
                )
            }),
        ))
        .child(setting(
            &tr!("Hosts from another app"),
            &tr!("PuTTY, KiTTY, MobaXterm, SecureCRT, Xshell, Tabby, ~/.ssh/config or a CSV file: their hosts, groups, jump hosts and forwards, shown first to choose from. Passwords are not imported."),
            div().flex().child(
                small_button("import-hosts", &tr!("Import…"))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::ImportHosts, w, cx))),
            ),
        ));
        if let Some(dir) = self.snap.config_dir.clone() {
            let files: [(String, String); 5] = [
                (
                    "config.toml".into(),
                    tr!("These settings, for this device. Edits in the file apply at once."),
                ),
                (
                    "hosts.toml".into(),
                    tr!("Hosts and groups. Synced. No secrets."),
                ),
                ("themes/*.toml".into(), tr!("Your own themes.")),
                (
                    "vault.db".into(),
                    tr!("Passwords and passphrases, encrypted."),
                ),
                (tr!("System keychain"), tr!("The vault's unlock key.")),
            ];
            let mut list = div().flex().flex_col().gap(px(6.));
            for (name, what) in files {
                list = list.child(
                    div()
                        .flex()
                        .gap_3()
                        .text_size(ui(12.5))
                        .child(div().w(px(130.)).flex_shrink_0().child(name))
                        .child(div().text_color(rgb(colors().muted)).child(what)),
                );
            }
            page = page
                .child(rule())
                .child(heading(&tr!("Where settings live")))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(ui(12.5))
                                .text_color(rgb(colors().label))
                                .child(shown_path(&dir)),
                        )
                        .child(
                            small_button("open-config-dir", &tr!("Open folder"))
                                .on_click(cx.listener(move |v, _, _, cx| v.open(&dir, cx))),
                        ),
                )
                .child(list);
        }
        page
    }

    fn appearance(&self, cx: &mut Context<Self>) -> gpui::Div {
        let a = &self.snap.settings.appearance;
        let light = self.snap.light;
        let current = a.app_theme(light).to_string();
        let modes = vec![
            (ColorMode::System, tr!("System")),
            (ColorMode::Dark, tr!("Dark")),
            (ColorMode::Light, tr!("Light")),
        ];
        let tile = |t: &Arc<Theme>, cx: &mut Context<Self>| {
            let on = t.id == current;
            let id = t.id.clone();
            let swatch = div()
                .h(px(46.))
                .rounded(px(8.))
                .border_2()
                .border_color(rgb(if on { colors().accent } else { colors().line }));
            let swatch = match self.gradient(t, 96, 40) {
                Some(image) => swatch.relative().child(Self::painted(image, px(6.))),
                None => swatch.bg(rgb(color(t.background))),
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
                        .text_color(rgb(if on { colors().text } else { colors().label }))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(t.name.clone()),
                )
        };
        // By group; while light, the light ones first. The user's own last.
        let mut order: Vec<(Option<&str>, String)> = shellwing_config::GROUPS
            .iter()
            .map(|(id, _)| (Some(*id), crate::look::group_title(Some(id))))
            .collect();
        if light {
            order.sort_by_key(|(id, _)| !id.is_some_and(|g| g.contains("light")));
        }
        order.push((None, crate::look::group_title(None)));
        let mut groups = div().flex().flex_col().gap(px(14.));
        for (id, title) in order {
            let members: Vec<&Arc<Theme>> = self
                .snap
                .themes
                .iter()
                .filter(|t| match id {
                    Some(g) => t.group.as_deref() == Some(g),
                    // Not in a group the app knows.
                    None => !shellwing_config::GROUPS
                        .iter()
                        .any(|(g, _)| t.group.as_deref() == Some(*g)),
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            let mut grid = div().grid().grid_cols(6).gap(px(10.));
            for t in members {
                grid = grid.child(tile(t, cx));
            }
            groups = groups.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading(&title))
                    .child(grid),
            );
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
            (CursorStyle::Block, tr!("Block")),
            (CursorStyle::Bar, tr!("Bar")),
            (CursorStyle::Underline, tr!("Underline")),
        ];
        let bars = vec![
            (Scrollbar::Auto, tr!("When scrolling")),
            (Scrollbar::Always, tr!("Always")),
            (Scrollbar::Never, tr!("Never")),
        ];
        let pads: Vec<(u32, String)> = [0, 2, 4, 8, 12]
            .into_iter()
            .map(|p| (p, p.to_string()))
            .collect();
        let text = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(heading2(&tr!("Terminal text")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .items_start()
                    .child(font_box)
                    .child(labelled(
                        &tr!("Size"),
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
                        &tr!("Line height"),
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
                        &tr!("Cursor"),
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
                        &tr!("Scrollbar"),
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
                        &tr!("Padding at the sides"),
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
                        &tr!("Top and bottom"),
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
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_size(ui(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr!("Mode")),
                    )
                    .child(self.pills(
                        "color-mode",
                        modes,
                        a.mode,
                        |v, m, cx| {
                            let name = match m {
                                ColorMode::System => "system",
                                ColorMode::Dark => "dark",
                                ColorMode::Light => "light",
                            };
                            v.change(&["appearance", "mode"], Some(SettingValue::Str(name.into())), cx)
                        },
                        cx,
                    ))
                    .child(label(&match a.mode {
                        ColorMode::System if light => tr!("The system is light now."),
                        ColorMode::System => tr!("The system is dark now."),
                        _ => String::new(),
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(
                        div()
                            .text_size(ui(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if light { tr!("Light theme") } else { tr!("Dark theme") }),
                    )
                    .child(label(&if light {
                        tr!("The terminal's, while the app is light; the dark mode keeps its own. A host can have its own in its editor.")
                    } else {
                        tr!("The terminal's, while the app is dark; the light mode keeps its own. A host can have its own in its editor.")
                    })),
            )
            .child(groups)
            .when_some(themes_dir, |d, dir| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("themes-dir", &tr!("Open themes folder")).on_click(
                            cx.listener(move |v, _, _, cx| {
                                let _ = std::fs::create_dir_all(&dir);
                                v.open(&dir, cx)
                            }),
                        ))
                        .child(label(&tr!("A .toml file there is a theme of your own."))),
                )
            })
            .child(toggle(
                "theme-accent",
                a.theme_accent,
                &tr!("The app's accent from the theme"),
                &tr!("The logo, what is chosen and the main buttons take the theme's accent color, in place of the app's blue. One too close to the app's background is not taken."),
                {
                    let on = a.theme_accent;
                    cx.listener(move |v, _, _, cx| {
                        v.change(&["appearance", "theme_accent"], Some(SettingValue::Bool(!on)), cx)
                    })
                },
            ))
            .child(rule())
            .child(text)
            .child(rule())
            .child(self.app_text(cx));

        // The preview is beside the page, not in it: it stays in view while
        // the themes scroll (see the render).
        left
    }

    /// `t`'s gradient, all its stops, drawn at `w` by `h` (it is stretched to
    /// fit), as the terminal draws it; None for a theme without one.
    fn gradient(&self, t: &Theme, w: u32, h: u32) -> Option<Arc<gpui::RenderImage>> {
        let g = t.gradient.as_ref()?;
        let key = (t.id.clone(), w, h);
        if let Some(image) = self.gradients.borrow().get(&key) {
            return Some(image.clone());
        }
        let buffer = image::RgbaImage::from_raw(w, h, g.render_bgra(w, h))?;
        let image = Arc::new(gpui::RenderImage::new([image::Frame::new(buffer)]));
        self.gradients.borrow_mut().insert(key, image.clone());
        Some(image)
    }

    /// An image filling its parent, behind what comes after it.
    fn painted(image: Arc<gpui::RenderImage>, radius: gpui::Pixels) -> gpui::Img {
        gpui::img(gpui::ImageSource::Render(image))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(radius)
            .object_fit(gpui::ObjectFit::Fill)
    }

    /// A terminal drawn in the chosen theme and font.
    fn preview(&self) -> gpui::Div {
        let a = &self.snap.settings.appearance;
        let theme = self
            .snap
            .themes
            .iter()
            .find(|t| t.id == a.app_theme(self.snap.light))
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
        let screen = div()
            .flex()
            .flex_col()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().line))
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
        // The gradient goes under the text, which is painted after it.
        let screen = match self.gradient(t, 200, 130) {
            Some(image) => div()
                .relative()
                .child(Self::painted(image, px(8.)))
                .child(screen),
            None => screen.bg(rgb(color(t.background))),
        };
        div()
            .w(px(400.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading(&tr!("Preview")))
            .child(screen)
            .child(
                div()
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(rgb(colors().panel))
                    .border_1()
                    .border_color(rgb(colors().line))
                    .text_size(ui(11.5))
                    .text_color(rgb(colors().muted))
                    .child("# config.toml")
                    .child("[appearance]")
                    .child(if self.snap.light {
                        format!("light_theme = \"{}\"", a.light_theme)
                    } else {
                        format!("theme = \"{}\"", a.theme)
                    })
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
            .map(|n| (n, tr!("{k} k", k = n / 1000)))
            .collect();
        let drops = vec![
            (OnDisconnect::Reconnect, tr!("Reconnect")),
            (OnDisconnect::Ask, tr!("Ask")),
            (OnDisconnect::Close, tr!("Close")),
        ];
        page(&tr!("Terminal"))
            .child(setting(
                &tr!("Scrollback"),
                &tr!("Lines kept per pane, now {lines}. More uses more memory; new tabs get it.", lines = t.scrollback),
                self.pills("scrollback", lines, t.scrollback, |v, n, cx| {
                    v.change(&["terminal", "scrollback"], Some(SettingValue::Int(n as i64)), cx)
                }, cx),
            ))
            .child(setting(
                &tr!("When a connection drops"),
                &tr!("Reconnect tries again by itself, waiting longer each time. After exit, Enter connects again."),
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
                &tr!("Local shell"),
                &tr!("What a local terminal runs (This computer, in the host list), with its arguments. Empty: your own shell. Enter saves; new local tabs get it."),
                div().w(px(320.)).child(
                    div()
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(colors().field_edge))
                        .bg(rgb(colors().page))
                        .text_size(ui(13.))
                        .child(self.shell_input.clone()),
                ),
            ))
            .child(self.logs(cx))
    }

    fn logs(&self, cx: &mut Context<Self>) -> gpui::Div {
        let l = &self.snap.settings.log;
        let always = l.always;
        let timestamps = l.timestamps;
        let folder = shellwing_core::logs::folder(l);
        let formats = vec![(LogFormat::Text, tr!("Text")), (LogFormat::Raw, tr!("Raw"))];
        let mut buttons = div().flex().flex_wrap().gap(px(6.)).child(
            small_button("log-folder", &tr!("Change…"))
                .on_click(cx.listener(|v, _, _, cx| v.choose_log_folder(cx))),
        );
        if let Some(dir) = folder.clone() {
            buttons = buttons.child(small_button("log-open", &tr!("Open")).on_click(cx.listener(
                move |v, _, _, cx| {
                    // Made first, as no log may have been written yet; if it
                    // cannot be, the file manager says what is wrong.
                    let _ = shellwing_core::logs::make_folder(&dir);
                    v.open(&dir, cx);
                },
            )));
        }
        if l.folder.is_some() {
            buttons = buttons.child(
                small_button("log-default", &tr!("Use the default"))
                    .on_click(cx.listener(|v, _, _, cx| v.change(&["log", "folder"], None, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(8.))
            .child(heading(&tr!("Session logs")))
            .child(div().flex().flex_col().child(toggle(
                "log-always",
                always,
                &tr!("Log every session"),
                &tr!("From its start, to a file of its own. A host can say otherwise in its editor, and Start Logging in a pane's menu logs it from then on."),
                cx.listener(move |v, _, _, cx| {
                    v.change(&["log", "always"], Some(SettingValue::Bool(!always)), cx)
                }),
            ))
            .child(setting(
                &tr!("Format"),
                &tr!("Text: the lines as they were left, colors gone; what full-screen programs (vim, htop) draw is left out. Raw: every byte as it came, which cat or less -R shows in color."),
                self.pills("log-format", formats, l.format, |v, f, cx| {
                    let name = match f {
                        LogFormat::Text => "text",
                        LogFormat::Raw => "raw",
                    };
                    v.change(&["log", "format"], Some(SettingValue::Str(name.into())), cx)
                }, cx),
            ))
            .when(l.format == LogFormat::Text, |d| {
                d.child(toggle(
                    "log-timestamps",
                    timestamps,
                    &tr!("The time on each line"),
                    &tr!("Each line of a text log starts with the date and time it was written."),
                    cx.listener(move |v, _, _, cx| {
                        v.change(&["log", "timestamps"], Some(SettingValue::Bool(!timestamps)), cx)
                    }),
                ))
            })
            .child(setting(
                &tr!("Folder"),
                &match &folder {
                    Some(dir) => tr!(
                        "Logs go to {folder}. Only you can read them: they hold whatever the sessions showed.",
                        folder = dir.display().to_string()
                    ),
                    None => tr!("No folder for logs: choose one."),
                },
                buttons,
            )))
    }

    fn ssh(&self, cx: &mut Context<Self>) -> gpui::Div {
        let agent = if std::env::var_os("SSH_AUTH_SOCK").is_some() || cfg!(windows) {
            tr!("Found. Its keys are tried first.")
        } else {
            tr!("Not found (SSH_AUTH_SOCK is not set).")
        };
        let file_row = |id: &'static str,
                        what: &str,
                        note: &str,
                        path: Option<PathBuf>,
                        cx: &mut Context<Self>| {
            let shown = path
                .as_deref()
                .map(shown_path)
                .unwrap_or_else(|| tr!("none"));
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
                            .text_color(rgb(colors().label))
                            .child(shown),
                    )
                    .when_some(path.filter(|p| p.exists()), |d, p| {
                        d.child(
                            small_button(id, &tr!("Open"))
                                .on_click(cx.listener(move |v, _, _, cx| v.open(&p, cx))),
                        )
                    }),
            )
        };
        let ssh = &self.snap.settings.ssh;
        let keys = vec![
            (HostKey::Ask, tr!("Ask")),
            (HostKey::AcceptNew, tr!("Accept new")),
            (HostKey::Strict, tr!("Refuse")),
        ];
        let alive: Vec<(u64, String)> = [0, 15, 30, 60]
            .into_iter()
            .map(|s| {
                (
                    s,
                    if s == 0 {
                        tr!("Off")
                    } else {
                        tr!("{s} s", s = s)
                    },
                )
            })
            .collect();
        page(&tr!("SSH and security"))
            .child(setting(
                &tr!("Unknown host keys"),
                &tr!("Ask shows the fingerprint and sends nothing until you accept it. Accept new records it and goes on; Refuse turns it away. A changed key is always refused. A host can have its own in its editor."),
                self.pills("host-key", keys, ssh.host_key, |v, k, cx| {
                    v.change(&["ssh", "host_key"], Some(SettingValue::Str(k.name().into())), cx)
                }, cx),
            ))
            .child(setting(
                &tr!("Keepalive"),
                &tr!(
                    "A check every so often, so idle connections are not dropped and dead ones are noticed after 3 misses. Now {now}. A host's own (or its ServerAliveInterval) wins.",
                    now = if ssh.keepalive == 0 { tr!("off") } else { tr!("{s} s", s = ssh.keepalive) }
                ),
                self.pills("keepalive", alive, ssh.keepalive, |v, s, cx| {
                    v.change(&["ssh", "keepalive"], Some(SettingValue::Int(s as i64)), cx)
                }, cx),
            ))
            .child(file_row(
                "known-hosts",
                &tr!("known_hosts file"),
                &tr!("Shared with ssh. shellwing adds the keys you trust."),
                self.snap.known_hosts.clone(),
                cx,
            ))
            .child(file_row(
                "ssh-config",
                &tr!("ssh config file"),
                &tr!("Read only. Its hosts show in the host list."),
                self.snap.ssh_config.clone(),
                cx,
            ))
            .child(setting(&tr!("Agent"), &tr!("Keys in ssh-agent (Pageant or the OpenSSH agent on Windows)."), div().text_size(ui(12.5)).child(agent)))
    }

    /// Settings › Sync. Not syncing: shellwing cloud, then a place of one's
    /// own to set up. Syncing: how it goes, the place, the options, and
    /// shellwing cloud beside it.
    fn sync(&self, cx: &mut Context<Self>) -> gpui::Div {
        let settings_on = self.snap.settings.sync.settings;
        let options = div()
            .flex()
            .flex_col()
            .child(toggle(
                "sync-settings",
                settings_on,
                &tr!("Sync settings too"),
                &tr!("The look (dark or light, themes, fonts), the terminal's and SSH's defaults, and logging. Each device keeps its own sync folder, log folder, local shell, editor and keyboard shortcuts."),
                cx.listener(move |v, _, _, cx| {
                    v.change(&["sync", "settings"], Some(SettingValue::Bool(!settings_on)), cx)
                }),
            ))
            .when_some(self.snap.passphrase_joins, |d, joins| {
                d.child(toggle(
                    "sync-passphrase",
                    joins,
                    &tr!("Let a passphrase join new devices"),
                    &tr!("A new device that syncs the same place types it instead of the recovery key or being approved here. It is kept there, sealed by the passphrase: anyone who can read the place can try to guess it, so it has to be hard to guess."),
                    cx.listener(move |v, _, w, cx| {
                        let a = if joins {
                            Action::StopPassphraseJoin
                        } else {
                            Action::LetPassphraseJoin
                        };
                        v.run(a, w, cx)
                    }),
                ))
            });
        let local = label(&tr!(
            "Hosts kept on this device only (in their editor) stay out of it."
        ));
        let Some(p) = &self.snap.sync_place else {
            return page(&tr!("Sync"))
                .child(
                    div()
                        .p(px(22.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(rgb(colors().line))
                        .bg(rgb(colors().surface))
                        .child(crate::sync_panel::invitation()),
                )
                .child(heading(&tr!("Or a place of your own")).pt(px(6.)))
                .child(label(&tr!("Hosts, saved passwords, keys and snippets travel through a folder another app syncs (iCloud Drive, Syncthing, Dropbox), a WebDAV server, S3-compatible storage or Git. Everything there is encrypted with your recovery key; the server never sees a secret.")))
                .child(self.sync_choice(cx))
                .child(heading(&tr!("When it syncs")).pt(px(6.)))
                .child(options)
                .child(local);
        };
        let shown = match p {
            SyncPlace::Folder(f) => shown_path(f),
            other => crate::syncing::describe(other),
        };
        let snap = &self.snap.sync;
        let card = crate::sync_panel::status_card(
            &snap.standing,
            snap.place.as_deref().unwrap_or(&shown),
            Box::new(cx.listener(|v, _, w, cx| v.run(Action::SyncNow, w, cx))),
            Box::new(cx.listener(|v, _, w, cx| v.run(Action::UnlockVault, w, cx))),
        );
        let place = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.))
            .py(px(10.))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                div()
                    .flex_1()
                    .min_w(px(240.))
                    .text_size(ui(13.))
                    .child(tr!("With {shown}", shown = shown)),
            )
            .child(
                small_button("sync-key", &tr!("Show the recovery key"))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::ShowRecoveryKey, w, cx))),
            )
            .child(
                small_button("sync-stop", &tr!("Stop syncing"))
                    .border_color(rgb(colors().bad_edge))
                    .text_color(rgb(colors().bad_text))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::StopSync, w, cx))),
            );
        page(&tr!("Sync"))
            .child(card)
            .when_some(snap.counts, |d, n| {
                d.child(
                    div()
                        .mt(px(-6.))
                        .px(px(4.))
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().soft))
                        .child(n.said()),
                )
            })
            .child(heading(&tr!("Your place")).pt(px(6.)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(place)
                    .child(div().pt(px(8.)).child(local)),
            )
            .child(heading(&tr!("Options")).pt(px(6.)))
            .child(options)
            .child(div().pt(px(8.)).child(crate::sync_panel::aside()))
    }

    /// Where to sync, while not syncing: a folder, or a server's address
    /// and sign-in.
    fn sync_choice(&self, cx: &mut Context<Self>) -> gpui::Div {
        let form = &self.sync_form;
        let kinds = vec![
            (SyncKind::Folder, tr!("A folder")),
            (SyncKind::WebDav, "WebDAV".to_string()),
            (SyncKind::S3, tr!("S3-compatible")),
            (SyncKind::Git, "Git".to_string()),
        ];
        let pick = self.pills(
            "sync-kind",
            kinds,
            form.kind,
            |v, k, cx| {
                v.sync_form.kind = k;
                v.sync_form.problem = None;
                cx.notify();
            },
            cx,
        );
        let field = |title: &str, input: &Entity<TextInput>| {
            let selector = format!("sync-field:{title}");
            labelled(
                title,
                div()
                    .debug_selector(move || selector)
                    .w(px(380.))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(colors().field_edge))
                    .bg(rgb(colors().page))
                    .text_size(ui(13.))
                    .child(input.clone()),
            )
        };
        let connect = small_button("sync-connect", &tr!("Connect"))
            .on_click(cx.listener(|v, _, w, cx| v.connect_sync(w, cx)));
        let body = match form.kind {
            SyncKind::Folder => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(label(&tr!("iCloud Drive, Syncthing, a Dropbox or OneDrive folder, a network share.")))
                .child(
                    div().flex().child(
                        small_button("sync-folder", &tr!("Choose a folder…")).on_click(
                            cx.listener(|v, _, w, cx| v.run(Action::SyncFolder, w, cx)),
                        ),
                    ),
                ),
            SyncKind::WebDav => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(label(&tr!("Nextcloud, ownCloud, Synology, Koofr, Fastmail: the address of a folder there.")))
                .child(field(&tr!("Address"), &form.dav[0]))
                .child(field(&tr!("User"), &form.dav[1]))
                .child(field(&tr!("Password"), &form.dav[2])),
            SyncKind::S3 => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(label(&tr!("AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: a bucket, and a key that can read and write it.")))
                .child(field(&tr!("Endpoint"), &form.s3[0]))
                .child(field(&tr!("Region (if it asks for one)"), &form.s3[1]))
                .child(field(&tr!("Bucket"), &form.s3[2]))
                .child(field(&tr!("Folder in the bucket"), &form.s3[3]))
                .child(field(&tr!("Access key"), &form.s3[4]))
                .child(field(&tr!("Secret key"), &form.s3[5])),
            SyncKind::Git => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(label(&tr!("A private repository on GitHub, GitLab, Gitea or your own server, through this system's git: it signs in as it does for you (its credential manager, your SSH keys and agent). Each change is a commit.")))
                // Images are large and git forgets nothing (note-images.md).
                .child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(colors().warn_text))
                        .child(tr!(
                            "Git keeps every version: images in notes stay in the repository's history, encrypted, even after they are removed, so it only grows."
                        )),
                )
                .child(field(&tr!("Repository"), &form.git[0]))
                .child(field(&tr!("Branch"), &form.git[1]))
                .child(field(&tr!("Token (if git cannot sign in by itself)"), &form.git[2]))
                .child(field(&tr!("The token's user"), &form.git[3]))
                .when_some(form.no_git.clone(), |d, no| d.child(self.no_git(no, cx))),
        };
        let remote = form.kind != SyncKind::Folder;
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(pick)
            .child(body)
            .when(remote, |d| {
                d.child(label(&tr!(
                    "The password, secret key or token stays on this device, in the vault; other devices sign in with their own."
                )))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(connect)
                        .when(form.installing, |d| {
                            d.child(div().text_size(ui(12.5)).child(tr!("Installing git…")))
                        })
                        .when_some(form.problem.clone(), |d, p| {
                            d.child(div().text_size(ui(12.5)).text_color(rgb(colors().bad)).child(p))
                        }),
                )
            })
    }

    /// Why git cannot be used, and how to get it.
    fn no_git(&self, no: shellwing_remote::NoGit, cx: &mut Context<Self>) -> gpui::Div {
        use shellwing_remote::{Install, NoGit};
        let how = match &no {
            NoGit::Missing(how) => Some(how.clone()),
            _ => None,
        };
        let mut what = div()
            .flex()
            .flex_col()
            .gap_2()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(colors().warn_edge))
            .bg(rgb(colors().warn_bg))
            .text_size(ui(12.5))
            .child(
                div()
                    .text_color(rgb(colors().warn_text))
                    .child(format!("{no}.")),
            );
        what =
            match how {
                Some(Install::CommandLineTools) => what
                    .child(label(&tr!(
                        "macOS installs it with Apple's Command Line Tools, in a dialog of its own."
                    )))
                    .child(div().flex().child(
                        small_button("git-install", &tr!("Install git…")).on_click(
                            cx.listener(|v, _, _, cx| v.install_git(Install::CommandLineTools, cx)),
                        ),
                    )),
                Some(Install::Winget) => what
                    .child(label(&tr!(
                        "Windows installs it with winget, and asks to allow it."
                    )))
                    .child(div().flex().child(
                        small_button("git-install", &tr!("Install git…")).on_click(
                            cx.listener(|v, _, _, cx| v.install_git(Install::Winget, cx)),
                        ),
                    )),
                Some(Install::Download(url)) => what.child(
                    div().flex().child(
                        small_button("git-download", &tr!("Download git"))
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
                    ),
                ),
                Some(Install::Command(cmd)) => {
                    let copied = cmd.clone();
                    what.child(label(&tr!("Install it in a terminal, then Connect again:")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .px(px(8.))
                                        .py(px(4.))
                                        .rounded(px(5.))
                                        .bg(rgb(colors().field))
                                        .font_family("monospace")
                                        .child(cmd),
                                )
                                .child(small_button("git-copy", &tr!("Copy")).on_click(
                                    cx.listener(move |v, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                            copied.clone(),
                                        ));
                                        v.say(Notice::good(tr!("Copied.")), cx);
                                    }),
                                )),
                        )
                }
                // Too old, or failing: what it said is above.
                None => what.child(label(&tr!(
                    "Install a newer git (git-scm.com), then Connect again."
                ))),
            };
        what
    }

    fn vault(&self, cx: &mut Context<Self>) -> gpui::Div {
        let count = match self.snap.secrets {
            Some(0) => tr!("None saved yet."),
            Some(n) => tr_n!(n, "{n} saved.", "{n} saved."),
            None => tr!("The vault is not open."),
        };
        // Where the vault's key is kept, and what to change about it.
        let (state, mut buttons) = (
            match (
                self.snap.vault_locked,
                self.snap.vault_passphrase,
                self.snap.vault_keychain,
            ) {
                (true, _, _) => tr!("Locked: its passphrase or recovery key opens it."),
                (_, None, _) => tr!("Not open."),
                (_, Some(true), true) => {
                    tr!("In the system keychain, and kept by a passphrase too.")
                }
                (_, Some(true), false) => {
                    tr!("Kept by its passphrase: typed when shellwing starts.")
                }
                (_, Some(false), true) => tr!("In the system keychain."),
                (_, Some(false), false) => {
                    tr!("Open, but nothing keeps its key: set a passphrase.")
                }
            },
            div().flex().flex_wrap().gap_2(),
        );
        if self.snap.vault_locked {
            buttons = buttons.child(
                small_button("vault-unlock", &tr!("Unlock…"))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::UnlockVault, w, cx))),
            );
        } else if let Some(has) = self.snap.vault_passphrase {
            buttons = buttons.child(
                small_button(
                    "vault-pass",
                    &if has {
                        tr!("Change the passphrase…")
                    } else {
                        tr!("Set a passphrase…")
                    },
                )
                .on_click(cx.listener(|v, _, w, cx| v.run(Action::SetVaultPassphrase, w, cx))),
            );
            if self.snap.vault_can_lock {
                buttons = buttons.child(
                    small_button("vault-lock", &tr!("Lock now"))
                        .on_click(cx.listener(|v, _, w, cx| v.run(Action::LockVault, w, cx))),
                );
            }
            if has && self.snap.vault_keychain {
                buttons = buttons
                    .child(
                        small_button("vault-leave", &tr!("Ask for it at every start")).on_click(
                            cx.listener(|v, _, w, cx| v.run(Action::LeaveKeychain, w, cx)),
                        ),
                    )
                    .child(
                        small_button("vault-pass-off", &tr!("Remove the passphrase")).on_click(
                            cx.listener(|v, _, w, cx| v.run(Action::RemoveVaultPassphrase, w, cx)),
                        ),
                    );
            }
            if !self.snap.vault_keychain {
                buttons = buttons.child(
                    small_button("vault-keep", &tr!("Keep it in the system keychain"))
                        .on_click(cx.listener(|v, _, w, cx| v.run(Action::KeepVaultKey, w, cx))),
                );
            }
            buttons = buttons.child(
                small_button("vault-rk", &tr!("Show the recovery key"))
                    .on_click(cx.listener(|v, _, w, cx| v.run(Action::ShowRecoveryKey, w, cx))),
            );
        }
        // shellwing's own lock, where this system can ask that it is the user.
        let lock = shellwing_presence::confirm::how().map(|_| {
            let after = self.snap.settings.lock.after.unwrap_or(0);
            let options = [0, 1, 5, 15, 30, 60]
                .into_iter()
                .map(|m| {
                    let label = match m {
                        0 => tr!("Off"),
                        60 => tr!("1 hour"),
                        m => tr!("{m} min", m = m),
                    };
                    (m, label)
                })
                .collect();
            let what = if cfg!(target_os = "macos") {
                tr!("When shellwing starts, and after this long without a key pressed or a click, its windows hide until Touch ID or your Mac's password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.")
            } else if cfg!(windows) {
                tr!("When shellwing starts, and after this long without a key pressed or a click, its windows hide until Windows Hello or your Windows password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.")
            } else {
                tr!("When shellwing starts, and after this long without a key pressed or a click, its windows hide until your login password says it is you. A light guard for a screen left unlocked: the sessions go on underneath.")
            };
            setting(
                &tr!("Lock shellwing when away"),
                &what,
                self.pills(
                    "app-lock-after",
                    options,
                    after,
                    |v, m, cx| {
                        let value = (m > 0).then_some(SettingValue::Int(i64::from(m)));
                        v.change(&["lock", "after"], value, cx)
                    },
                    cx,
                ),
            )
        });
        page(&tr!("Vault"))
            .children(lock)
            .child(setting(
                &tr!("Saved passwords and passphrases"),
                &tr!("Offered for saving after you type one that works, and tried before asking next time. Kept in vault.db, encrypted."),
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_size(ui(12.5)).child(count))
                    .child(small_button("vault-manage", &tr!("Manage…")).on_click(cx.listener(|v, _, w, cx| v.run(Action::ManageSecrets, w, cx)))),
            ))
            .child(setting(
                &tr!("The vault's key"),
                &tr!("What opens vault.db. A passphrase keeps it where there is no system keychain (or as well as one); the recovery key always opens it, if the passphrase is forgotten or the keychain loses it."),
                div().flex().flex_col().gap_2().child(div().text_size(ui(12.5)).child(state)).child(buttons),
            ))
            .when_some(
                self.snap
                    .presence
                    .filter(|_| self.snap.vault_passphrase == Some(true)),
                |d, (name, on)| {
                    let what = if cfg!(target_os = "macos") {
                        tr!("When the vault is locked, {name} opens it, as its passphrase does (which still works). The fingers enrolled now: adding one stops it, until it is turned on again. Needs a signed build of shellwing.", name = name)
                    } else {
                        tr!("When the vault is locked, {name} (a fingerprint, the face or this device's PIN) opens it, as its passphrase does (which still works). The key is kept by this device's security chip.", name = name)
                    };
                    d.child(toggle(
                        "vault-presence",
                        on,
                        &tr!("Unlock with {name}", name = name),
                        &what,
                        cx.listener(|v, _, w, cx| v.run(Action::TogglePresence, w, cx)),
                    ))
                },
            )
            .when(self.snap.vault_passphrase == Some(true), |d| {
                let after = self.snap.settings.vault.lock_after.unwrap_or(0);
                let options = [0, 5, 15, 30, 60]
                    .into_iter()
                    .map(|m| {
                        let label = match m {
                            0 => tr!("Never"),
                            60 => tr!("1 hour"),
                            m => tr!("{m} min", m = m),
                        };
                        (m, label)
                    })
                    .collect();
                d.child(setting(
                    &tr!("Lock when idle"),
                    &tr!("After this long without a key pressed or a click, the vault locks until its passphrase is typed. Only while the system keychain does not hold its key: each start asks for the passphrase then."),
                    self.pills(
                        "vault-lock-after",
                        options,
                        after,
                        |v, m, cx| {
                            let value = (m > 0).then_some(SettingValue::Int(i64::from(m)));
                            v.change(&["vault", "lock_after"], value, cx)
                        },
                        cx,
                    ),
                ))
            })
    }

    /// The Keys page: a view of its own, made when first shown.
    fn keys_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let keys = match &self.keys {
            Some(k) => k.clone(),
            None => {
                let Some((vault, rt)) = self.workspace.upgrade().map(|w| w.read(cx).keys_context())
                else {
                    return page(&tr!("Keys"));
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
                    return page(&tr!("Snippets"));
                };
                let ws = self.workspace.clone();
                let s = cx.new(|cx| SnippetsView::new(ws, vault, cx));
                self.snippets = Some(s.clone());
                s
            }
        };
        div().child(view)
    }

    /// The Environments page, likewise.
    fn environments_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        match self.environments_view(cx) {
            Some(view) => div().child(view),
            None => page(&tr!("Environments")),
        }
    }

    fn environments_view(&mut self, cx: &mut Context<Self>) -> Option<Entity<SnippetsView>> {
        if self.environments.is_none() {
            let (vault, _) = self.workspace.upgrade()?.read(cx).keys_context();
            let ws = self.workspace.clone();
            self.environments = Some(cx.new(|cx| SnippetsView::environments(ws, vault, cx)));
        }
        self.environments.clone()
    }

    /// The Config files page, likewise.
    fn config_files_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        match self.config_files_view(cx) {
            Some(view) => div().child(view),
            None => page(&tr!("Config files")),
        }
    }

    fn config_files_view(&mut self, cx: &mut Context<Self>) -> Option<Entity<SnippetsView>> {
        if self.config_files.is_none() {
            let (vault, _) = self.workspace.upgrade()?.read(cx).keys_context();
            let ws = self.workspace.clone();
            self.config_files = Some(cx.new(|cx| SnippetsView::config_files(ws, vault, cx)));
        }
        self.config_files.clone()
    }

    /// The form for a new environment or config file, on its page.
    pub fn new_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = match self.page {
            Page::Environments => self.environments_view(cx),
            Page::ConfigFiles => self.config_files_view(cx),
            _ => None,
        };
        if let Some(view) = view {
            view.update(cx, |v, cx| v.new_item(window, cx));
        }
    }

    /// The Notes page, likewise.
    fn notes_page(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let view = match &self.notes {
            Some(s) => s.clone(),
            None => {
                let Some((vault, _)) = self.workspace.upgrade().map(|w| w.read(cx).keys_context())
                else {
                    return page(&tr!("Notes"));
                };
                let ws = self.workspace.clone();
                let s = cx.new(|cx| SnippetsView::notes(ws, vault, cx));
                self.notes = Some(s.clone());
                s
            }
        };
        div().child(view)
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
        .text_color(rgb(colors().muted))
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
        .text_color(rgb(colors().label))
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
    div().h(px(1.)).w_full().bg(rgb(colors().line))
}

/// A setting: its name and what it does on the left, the control on the right.
fn setting(title: &str, what: &str, control: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .gap(px(16.))
        .py(px(10.))
        .border_b_1()
        .border_color(rgb(colors().line))
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
                        .text_color(rgb(colors().muted))
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
        .debug_selector(|| id.into())
        .flex()
        .gap_3()
        .py(px(10.))
        .border_b_1()
        .border_color(rgb(colors().line))
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
                .border_color(rgb(if on {
                    colors().accent
                } else {
                    colors().field_edge
                }))
                .bg(rgb(if on { colors().accent } else { colors().page }))
                .text_size(ui(11.))
                .text_color(rgb(colors().on_accent))
                .when(on, |d| d.child("✓")),
        )
        .child(
            // The rest of the row, wrapping its text within the page.
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().text_size(ui(13.)).child(title.to_string()))
                .child(
                    div()
                        .text_size(ui(11.5))
                        .text_color(rgb(colors().muted))
                        .child(what.to_string()),
                ),
        )
}

fn small_button(id: impl Into<gpui::ElementId>, label: &str) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(|| selector)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(colors().field_edge))
        .text_size(ui(12.))
        .text_color(rgb(colors().soft))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(colors().hover)).text_color(rgb(colors().text)))
        .child(label.to_string())
}

impl SettingsView {
    /// The page showing: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn page(&self) -> Page {
        self.page
    }

    /// What the Sync page says beside Connect: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn sync_problem(&self) -> Option<String> {
        self.sync_form.problem.clone()
    }

    /// The notice over the page: for the UI tests.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn notice_shown(&self) -> Option<Notice> {
        self.notice.as_ref().map(|(m, _)| m.clone())
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(14.))
            .pr(px(6.))
            .bg(rgb(colors().surface))
            .border_b_1()
            .border_color(rgb(colors().line))
            .child(
                gpui::svg()
                    .path("icons/logo.svg")
                    .size(px(16.))
                    .text_color(rgb(colors().accent)),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("Settings")),
            )
            .child(div().flex_1())
            .when_some(self.snap.config_dir.clone(), |d, dir| {
                d.child(
                    small_button("open-config", &tr!("Open config.toml")).on_click(
                        cx.listener(move |v, _, _, cx| v.open(&dir.join("config.toml"), cx)),
                    ),
                )
            })
            .child(
                div()
                    .id("settings-close")
                    .debug_selector(|| "settings-close".into())
                    .w(px(34.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(rgb(colors().soft))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors().hover)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)))
                    .child("✕"),
            );
        // Settings fills the window's body, its bottom corners too.
        let corners = crate::chrome::BottomCorners::current(cx);
        let mut nav = div()
            .w(px(220.))
            .rounded_bl(corners.left)
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .py(px(12.))
            .bg(rgb(colors().panel))
            .border_r_1()
            .border_color(rgb(colors().line));
        for (n, p) in PAGES.iter().enumerate() {
            let (p, on) = (*p, *p == self.page);
            nav = nav.child(
                div()
                    .id(("settings-page", n))
                    .debug_selector(|| format!("settings-page:{p:?}"))
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .text_color(rgb(if on { colors().text } else { colors().soft }))
                    .cursor_pointer()
                    .when(on, |d| {
                        d.bg(rgb(colors().selected))
                            .border_l_2()
                            .border_color(rgb(colors().accent))
                    })
                    .when(!on, |d| d.hover(|s| s.bg(rgb(colors().hover))))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.stop_recording();
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
                        if p == Page::Environments
                            && let Some(s) = &v.environments
                        {
                            s.update(cx, |s, cx| s.refresh(cx));
                        }
                        if p == Page::ConfigFiles
                            && let Some(s) = &v.config_files
                        {
                            s.update(cx, |s, cx| s.refresh(cx));
                        }
                        if p == Page::Notes
                            && let Some(s) = &v.notes
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
                    .child(p.title())
                    // How sync stands, in the cloud's color.
                    .when(p == Page::Sync, |d| {
                        d.child(
                            div().ml(px(8.)).size(px(7.)).rounded_full().bg(rgb(self
                                .snap
                                .sync
                                .standing
                                .color())),
                        )
                    }),
            );
        }
        let content = match self.page {
            Page::General => self.general(cx),
            Page::Appearance => self.appearance(cx),
            Page::Terminal => self.terminal(cx),
            Page::Keyboard => self.keyboard(cx),
            Page::Ssh => self.ssh(cx),
            Page::Keys => self.keys_page(cx),
            Page::Snippets => self.snippets_page(cx),
            Page::Environments => self.environments_page(cx),
            Page::ConfigFiles => self.config_files_page(cx),
            Page::Notes => self.notes_page(cx),
            Page::KnownHosts => {
                let view = self
                    .known_hosts
                    .get_or_insert_with(|| cx.new(KnownHostsView::new))
                    .clone();
                div().child(view)
            }
            Page::Sync => self.sync(cx),
            Page::Vault => self.vault(cx),
            Page::About => self.about(cx),
        };
        corners
            .round(div())
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors().page))
            .text_size(ui(13.))
            .text_color(rgb(colors().text))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(nav)
                    .child(
                        div()
                            .id("settings-content")
                            .debug_selector(|| "settings-content".into())
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .px(px(24.))
                            .py(px(20.))
                            .child(content),
                    )
                    // Appearance: the terminal as chosen, beside what scrolls.
                    .when(self.page == Page::Appearance, |d| {
                        d.child(
                            div()
                                .id("settings-preview")
                                .flex_shrink_0()
                                .overflow_y_scroll()
                                .pt(px(20.))
                                .pr(px(24.))
                                .pb(px(20.))
                                .child(self.preview()),
                        )
                    }),
            )
            .when_some(self.notice.as_ref().map(|(m, _)| m.clone()), |d, m| {
                let notice = div()
                    .absolute()
                    .right(px(20.))
                    .bottom(px(20.))
                    .max_w(px(460.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .shadow_lg();
                d.child(colored(notice, m.tone).child(m.text))
            })
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

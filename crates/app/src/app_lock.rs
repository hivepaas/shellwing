//! shellwing locked while its user is away (`[lock] after` in config.toml):
//! its windows hidden until the system says it is them, by Touch ID or the
//! Mac's password, Windows Hello, or the Linux login password. A light guard
//! for a screen left unlocked: the sessions go on underneath, and nothing is
//! encrypted by it (the vault has its own lock).

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, Render, SharedString, Window, div, prelude::*, px, rgb,
};
use shellwing_i18n::tr;
use shellwing_presence::confirm::How;
use zeroize::Zeroizing;

use crate::colors::colors;
use crate::host_editor::{outline_button, primary_button};
use crate::look::ui;

/// Whether shellwing is locked: every window shows that, not itself.
#[derive(Default)]
pub struct Locked(pub bool);

impl gpui::Global for Locked {}

pub fn is_locked(cx: &App) -> bool {
    cx.try_global::<Locked>().is_some_and(|l| l.0)
}

/// While locked, keys reach no window but the main one, where the lock
/// takes them: under its cover, a note's title would still take what is
/// typed, and its editor's keys (paste, undo) before any listener of ours.
/// Once, at start.
pub fn guard_other_windows(cx: &mut App) {
    cx.intercept_keystrokes(|_, window, cx| {
        let main = window
            .root::<crate::workspace::Workspace>()
            .flatten()
            .is_some();
        if is_locked(cx) && !main {
            cx.stop_propagation();
        }
    })
    .detach();
}

/// What says it is the user: the system ([`System`]), or the UI tests'.
pub trait Confirmer: Send + Sync {
    /// How it asks; None where this system cannot.
    fn how(&self) -> Option<How>;
    /// The system asks, in its own dialog: macOS says `reason` after
    /// "shellwing is trying to", Windows shows `description`.
    fn ask(&self, reason: &str, description: &str) -> Result<bool, String>;
    /// Whether `password` is the user's login password; `said` gets what the
    /// system says meanwhile.
    fn check_password(&self, password: &str, said: &dyn Fn(String)) -> Result<bool, String>;
}

/// The system's own: see `shellwing_presence::confirm`.
pub struct System;

impl Confirmer for System {
    fn how(&self) -> Option<How> {
        shellwing_presence::confirm::how()
    }

    fn ask(&self, reason: &str, description: &str) -> Result<bool, String> {
        shellwing_presence::confirm::ask(reason, description)
    }

    fn check_password(&self, password: &str, said: &dyn Fn(String)) -> Result<bool, String> {
        shellwing_presence::confirm::check_password(password, said)
    }
}

/// The confirmer the UI tests put in, in place of the system's.
#[derive(Clone)]
pub struct ConfirmWith(pub Arc<dyn Confirmer>);

impl gpui::Global for ConfirmWith {}

pub fn confirmer(cx: &App) -> Arc<dyn Confirmer> {
    cx.try_global::<ConfirmWith>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| Arc::new(System))
}

/// It is the user, or they chose to quit.
pub enum LockEvent {
    Unlocked,
    Quit,
}

/// What covers the main window while shellwing is locked.
pub struct LockView {
    focus: FocusHandle,
    confirmer: Arc<dyn Confirmer>,
    how: How,
    /// Linux: the login password, as typed. Taken key by key, here: the
    /// lock is drawn over everything, where no text field takes input.
    password: Option<Zeroizing<String>>,
    /// A prompt or a check is under way.
    asking: bool,
    /// What the system said last, or why it is still locked.
    said: Option<(SharedString, bool)>,
}

impl EventEmitter<LockEvent> for LockView {}

impl Focusable for LockView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LockView {
    /// Locked, asking at once when `ask_now` (at start: the user is there;
    /// after being away, not until they come back and ask).
    pub fn new(
        confirmer: Arc<dyn Confirmer>,
        how: How,
        ask_now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password = (how == How::Password).then(|| Zeroizing::new(String::new()));
        let mut v = LockView {
            focus: cx.focus_handle(),
            confirmer,
            how,
            password,
            asking: false,
            said: None,
        };
        v.take_keys(window, cx);
        if ask_now && how == How::System {
            v.unlock(window, cx);
        }
        v
    }

    /// The keyboard comes here, and to nothing under it.
    pub fn take_keys(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    /// Asks the system, or checks the password typed.
    fn unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.asking {
            return;
        }
        let confirmer = self.confirmer.clone();
        // What the system says on the way (a fingerprint asked first): the
        // job's end closes with it.
        let (tx, rx) = async_channel::unbounded::<String>();
        let job = match self.how {
            How::System => {
                drop(tx);
                let (reason, description) = (
                    tr!("unlock its windows"),
                    tr!("Confirm it is you to unlock shellwing."),
                );
                self.said = None;
                cx.background_spawn(async move { confirmer.ask(&reason, &description) })
            }
            How::Password => {
                let Some(typed) = self
                    .password
                    .as_mut()
                    .filter(|p| !p.is_empty())
                    .map(|p| std::mem::replace(p, Zeroizing::new(String::new())))
                else {
                    return;
                };
                self.said = Some((tr!("Checking…").into(), false));
                cx.background_spawn(async move {
                    let said = move |m: String| {
                        let _ = tx.try_send(m);
                    };
                    confirmer.check_password(&typed, &said)
                })
            }
        };
        self.asking = true;
        // One task, in order: what was said shows before the answer, never
        // over it.
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(m) = rx.recv().await {
                let shown = this.update(cx, |v, cx| {
                    v.said = Some((m.into(), false));
                    cx.notify();
                });
                if shown.is_err() {
                    return;
                }
            }
            let answer = job.await;
            let _ = this.update_in(cx, |v, window, cx| v.answered(answer, window, cx));
        })
        .detach();
        cx.notify();
    }

    fn answered(
        &mut self,
        answer: Result<bool, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.asking = false;
        match answer {
            Ok(true) => {
                self.said = None;
                cx.emit(LockEvent::Unlocked);
            }
            Ok(false) => {
                let why = match self.how {
                    How::System => tr!("Not confirmed: shellwing stays locked."),
                    How::Password => tr!("That is not the password."),
                };
                self.said = Some((why.into(), true));
                self.take_keys(window, cx);
            }
            Err(e) => {
                self.said = Some((tr!("Cannot confirm it is you: {e}", e = e).into(), true));
                self.take_keys(window, cx);
            }
        }
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control
        };
        match k.key.as_str() {
            "enter" => self.unlock(window, cx),
            _ if self.asking => {}
            "backspace" => {
                if let Some(p) = self.password.as_mut() {
                    p.pop();
                }
            }
            "escape" => {
                if let Some(p) = self.password.as_mut() {
                    p.clear();
                }
            }
            // From a password manager.
            "v" if cmd => {
                if let (Some(p), Some(text)) = (
                    self.password.as_mut(),
                    cx.read_from_clipboard().and_then(|c| c.text()),
                ) {
                    let text = Zeroizing::new(text);
                    p.push_str(text.trim_end_matches(['\r', '\n']));
                }
            }
            _ => match (self.password.as_mut(), &k.key_char) {
                (Some(p), Some(c))
                    if !(m.control || m.platform) && !c.chars().any(char::is_control) =>
                {
                    p.push_str(c)
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// For the UI tests: what it says now.
    #[cfg(all(test, feature = "ui-tests"))]
    pub fn said(&self) -> Option<String> {
        self.said.as_ref().map(|(s, _)| s.to_string())
    }
}

impl Render for LockView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let how_to = match self.how {
            How::System if cfg!(windows) => {
                tr!("Windows Hello or your Windows password unlocks it.")
            }
            How::System => tr!("Touch ID or your Mac's password unlocks it."),
            How::Password => tr!("Your login password unlocks it."),
        };
        let unlock = primary_button("lock-unlock", &tr!("Unlock"))
            .on_click(cx.listener(|v, _, window, cx| v.unlock(window, cx)));
        let quit = outline_button("lock-quit", &tr!("Quit shellwing"))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(LockEvent::Quit)));
        let card = div()
            .id("lock-card")
            .debug_selector(|| "lock-card".into())
            .w(px(380.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .px(px(28.))
            .py(px(26.))
            .rounded(px(12.))
            .bg(rgb(colors().surface))
            .border_1()
            .border_color(rgb(colors().edge))
            .shadow_lg()
            .child(
                gpui::svg()
                    .path("icons/logo.svg")
                    .size(px(40.))
                    .text_color(rgb(colors().accent)),
            )
            .child(
                div()
                    .text_size(ui(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("shellwing is locked")),
            )
            .child(
                div()
                    .text_size(ui(13.))
                    .text_color(rgb(colors().label))
                    .child(how_to),
            )
            .when_some(
                self.password.as_ref().map(|p| p.chars().count()),
                |d, typed| {
                    // A dot a character, as other password fields show it.
                    let (shown, color) = if typed == 0 {
                        (tr!("Your login password"), colors().faint)
                    } else {
                        ("•".repeat(typed), colors().text)
                    };
                    d.child(
                        div()
                            .debug_selector(|| "lock-password".into())
                            .w_full()
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(colors().accent))
                            .bg(rgb(colors().page))
                            .text_color(rgb(color))
                            .child(shown),
                    )
                },
            )
            .when_some(self.said.clone(), |d, (said, bad)| {
                d.child(
                    div()
                        .text_size(ui(12.5))
                        .text_color(rgb(if bad { colors().bad } else { colors().label }))
                        .child(said),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(quit)
                    .child(unlock),
            );
        div()
            .id("lock")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(colors().page))
            .text_color(rgb(colors().text))
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(card)
    }
}

/// What covers another window (a note, a snippet, the library) while
/// shellwing is locked; None while it is not.
pub fn cover(cx: &App) -> Option<AnyElement> {
    is_locked(cx).then(|| {
        gpui::deferred(
            div()
                .id("locked-elsewhere")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(colors().page))
                .text_color(rgb(colors().label))
                .text_size(ui(13.))
                .child(tr!("shellwing is locked: unlock it in its main window.")),
        )
        .with_priority(100)
        .into_any_element()
    })
}

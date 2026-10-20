//! Asking it is the user before a secret shows (an environment's value, a
//! config file's content): Windows Hello or Touch ID where it opens the
//! vault, else the vault's passphrase, else a yes.

use super::*;
use shellwing_vault::Error as VaultError;

/// What shows once the user is confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Then {
    /// The value of the environment's row.
    EnvRow(usize),
    /// The config file's content.
    FileContent,
}

/// The question being asked.
pub(super) struct Confirm {
    /// The vault's passphrase as typed; None: no passphrase guards the
    /// vault, a yes is asked for.
    passphrase: Option<Entity<TextInput>>,
    /// Windows Hello or Touch ID is asking.
    waiting: bool,
    problem: Option<String>,
    pub(super) then: Then,
}

impl SnippetsView {
    /// Asks it is the user, then shows `then`.
    pub(super) fn ask_confirm(&mut self, then: Then, window: &mut Window, cx: &mut Context<Self>) {
        let slot = self.vault.clone();
        let Some(vault) = slot.ready() else {
            return;
        };
        let presence = slot.presence().is_some() && vault.has_presence().unwrap_or(false);
        let passphrase = vault.has_vault_passphrase().unwrap_or(false);
        if !presence {
            return self.ask_passphrase(passphrase, None, then, window, cx);
        }
        self.confirm = Some(Confirm {
            passphrase: None,
            waiting: true,
            problem: None,
            then,
        });
        cx.notify();
        let job = cx.background_spawn(async move { slot.confirm(None) });
        cx.spawn_in(window, async move |this, cx| {
            let done = job.await;
            let _ = this.update_in(cx, |v, window, cx| match done {
                Ok(()) => v.confirmed(then, cx),
                Err(err) => {
                    // Not there, or cancelled: the passphrase instead.
                    let problem = (!matches!(err, VaultError::NotPresent)).then(|| err.to_string());
                    v.ask_passphrase(passphrase, problem, then, window, cx);
                }
            });
        })
        .detach();
    }

    fn ask_passphrase(
        &mut self,
        passphrase: bool,
        problem: Option<String>,
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = passphrase
            .then(|| cx.new(|cx| TextInput::said(|| tr!("The vault's passphrase"), cx).masked()));
        let focus = input.as_ref().map(|i| i.read(cx).focus_handle(cx));
        self.confirm = Some(Confirm {
            passphrase: input,
            waiting: false,
            problem,
            then,
        });
        match focus {
            Some(f) => window.focus(&f, cx),
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// The answer to the question: the passphrase checked (away from the
    /// window: its KDF takes a moment), or the yes.
    pub(super) fn confirm_answer(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.confirm.as_ref().filter(|c| !c.waiting) else {
            return;
        };
        let then = c.then;
        let Some(input) = c.passphrase.clone() else {
            return self.confirmed(then, cx);
        };
        let typed = zeroize::Zeroizing::new(input.read(cx).text().to_string());
        input.update(cx, |i, cx| i.set_text("", 0..0, cx));
        let slot = self.vault.clone();
        let job = cx.background_spawn(async move { slot.confirm(Some(typed.as_str())) });
        cx.spawn(async move |this, cx| {
            let done = job.await;
            let _ = this.update(cx, |v, cx| match done {
                Ok(()) => v.confirmed(then, cx),
                Err(err) => {
                    if let Some(c) = v.confirm.as_mut() {
                        c.problem = Some(match err {
                            VaultError::WrongPassphrase => tr!("Wrong passphrase."),
                            e => e.to_string(),
                        });
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn confirmed(&mut self, then: Then, cx: &mut Context<Self>) {
        self.confirm = None;
        match then {
            Then::EnvRow(i) => self.open_values(i, cx),
            Then::FileContent => self.show_file_content(cx),
        }
    }

    /// The question, or the wait for the system's prompt; nothing when
    /// none is asked.
    pub(super) fn confirm_line(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let c = self.confirm.as_ref()?;
        if c.waiting {
            return Some(div().child(hint_text(&tr!("Confirm it is you in the system's prompt…"))));
        }
        let mut ask = div().flex().flex_col().gap(px(6.));
        match &c.passphrase {
            Some(input) => {
                let focused = input.read(cx).focus_handle(cx).is_focused(window);
                ask = ask
                    .child(label_text(&tr!(
                        "The vault's passphrase, to show the values"
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(Self::field_box(input, focused, None).flex_1())
                            .child(
                                Self::small("confirm-show", &tr!("Show"))
                                    .h(px(32.))
                                    .on_click(cx.listener(|v, _, _, cx| v.confirm_answer(cx))),
                            ),
                    );
            }
            None => {
                ask = ask.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(hint_text(&tr!("No passphrase guards the vault, so anyone at this device can show them. Show the values?")).flex_1().min_w_0())
                        .child(
                            Self::small("confirm-show", &tr!("Show"))
                                .on_click(cx.listener(|v, _, _, cx| v.confirm_answer(cx))),
                        ),
                );
            }
        }
        if let Some(p) = &c.problem {
            ask = ask.child(problem_text(p));
        }
        Some(ask)
    }
}

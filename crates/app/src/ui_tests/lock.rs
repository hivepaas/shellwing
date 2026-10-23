//! shellwing locked while its user is away (`[lock] after`): the system's
//! answer played by a confirmer of the test's own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use shellwing_presence::confirm::How;

use super::{Ui, scenario};
use crate::app_lock::{ConfirmWith, Confirmer};
use crate::session::Status;

/// Says yes or no as the scenario says, and counts the times it was asked.
pub(super) struct Answers {
    how: How,
    yes: AtomicBool,
    asked: AtomicUsize,
}

impl Confirmer for Answers {
    fn how(&self) -> Option<How> {
        Some(self.how)
    }

    fn ask(&self, _reason: &str, _description: &str) -> Result<bool, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(self.yes.load(Ordering::SeqCst))
    }

    fn check_password(&self, password: &str, said: &dyn Fn(String)) -> Result<bool, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        said("Place your finger on the reader".into());
        Ok(password == "correct horse")
    }
}

/// The system's answer, `yes` until told otherwise, put in before the
/// window opens; `[lock] after` set to `after` minutes.
pub(super) fn answering(ui: &mut Ui, how: How, yes: bool, after: u32) -> Arc<Answers> {
    let answers = Arc::new(Answers {
        how,
        yes: AtomicBool::new(yes),
        asked: AtomicUsize::new(0),
    });
    let confirmer: Arc<dyn Confirmer> = answers.clone();
    ui.cx.update(|cx| cx.set_global(ConfirmWith(confirmer)));
    ui.config_file("config.toml", &format!("[lock]\nafter = {after}\n"));
    answers
}

pub(super) fn locked(ui: &mut Ui) -> bool {
    ui.ws(|ws, _, _| ws.app_locked())
}

fn said(ui: &mut Ui) -> Option<String> {
    ui.ws(|ws, _, cx| ws.lock_said(cx))
}

scenario!(
    /// Locked from the start: the system asks at once. Refused, it stays
    /// locked, says so, and keeps keys and shortcuts from the terminal and
    /// the palette; once it is the user, the terminal has them again.
    locked_at_start_until_the_system_says_it_is_the_user,
    |ui: &mut Ui| {
        let answers = answering(ui, How::System, false, 5);
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        assert!(locked(ui), "not locked at start");
        ui.wait_until("the refusal said", |ui| {
            said(ui).is_some_and(|s| s.starts_with("Not confirmed"))
        });
        assert_eq!(answers.asked.load(Ordering::SeqCst), 1, "asked at once, once");

        // Typed while locked: not the shell's. Enter is the lock's own: it asks
        // again, and is refused again.
        ui.type_text("echo zz-while-locked");
        ui.keys("enter");
        ui.wait_until("asked again", |_| answers.asked.load(Ordering::SeqCst) == 2);
        ui.shortcut("k");
        ui.settle(Duration::from_millis(300));
        assert!(!ui.ws(|ws, _, _| ws.palette_open()), "the palette opened");
        assert!(locked(ui));
        assert!(!ui.screen().contains("zz-while-locked"), "keys reached the shell");

        answers.yes.store(true, Ordering::SeqCst);
        ui.click("lock-unlock");
        ui.wait_until("unlocked", |ui| !locked(ui));
        assert_eq!(answers.asked.load(Ordering::SeqCst), 3);
        ui.line("echo zz-after-unlock");
        ui.wait_for("zz-after-unlock");
    }
);

scenario!(
    /// After this long without a key or a click it locks again, without
    /// asking until the user, back, asks.
    locks_again_after_being_away,
    |ui: &mut Ui| {
        let answers = answering(ui, How::System, true, 1);
        ui.connect("local:");
        ui.wait_until("unlocked at start", |ui| !locked(ui));
        assert_eq!(answers.asked.load(Ordering::SeqCst), 1);

        ui.ws(|ws, _, _| ws.away_for(Duration::from_secs(59)));
        ui.reload_config();
        assert!(!locked(ui), "locked before a minute");

        ui.ws(|ws, _, _| ws.away_for(Duration::from_secs(61)));
        ui.reload_config();
        assert!(locked(ui), "not locked after a minute away");
        ui.settle(Duration::from_millis(200));
        assert_eq!(answers.asked.load(Ordering::SeqCst), 1, "asked while away");

        ui.click("lock-unlock");
        ui.wait_until("unlocked", |ui| !locked(ui));
    }
);

scenario!(
    /// Where shellwing asks for the password itself (Linux): a wrong one
    /// says so and stays locked; the right one unlocks.
    the_login_password_unlocks_where_shellwing_asks_for_it,
    |ui: &mut Ui| {
        let answers = answering(ui, How::Password, true, 5);
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        assert!(locked(ui));
        assert_eq!(answers.asked.load(Ordering::SeqCst), 0, "checked with nothing typed");

        ui.line("wrong horse");
        ui.wait_until("the wrong password said", |ui| {
            said(ui).is_some_and(|s| s.starts_with("That is not the password"))
        });
        assert!(locked(ui));

        ui.line("correct horse");
        ui.wait_until("unlocked", |ui| !locked(ui));
        assert_eq!(answers.asked.load(Ordering::SeqCst), 2);
    }
);

scenario!(
    /// Settings › Vault sets it: minutes away in config.toml's [lock].
    settings_set_the_minutes_away,
    |ui: &mut Ui| {
        ui.open();
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Vault");
        ui.click("app-lock-after:2");
        ui.wait_until("config.toml to say it", |ui| {
            std::fs::read_to_string(ui.config_dir().join("config.toml"))
                .is_ok_and(|c| c.contains("[lock]") && c.contains("after = 5"))
        });
    }
);

scenario!(
    /// What was open when it locked takes no answer from under it: a
    /// question is put away as Escape puts it (the tab stays), and a right
    /// click on the lock opens no terminal menu.
    nothing_under_the_lock_takes_an_answer,
    |ui: &mut Ui| {
        answering(ui, How::System, true, 5);
        // The app's own questions, drawn in its window, as main has them.
        ui.cx.update(|cx| cx.set_prompt_builder(crate::prompt::render));
        ui.connect("local:");
        ui.wait_until("unlocked at start", |ui| !locked(ui));
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.shortcut("w");
        ui.wait_until("the question", |ui| ui.find("prompt-card").is_some());

        ui.ws(|ws, _, _| ws.away_for(Duration::from_secs(6 * 60)));
        ui.reload_config();
        assert!(locked(ui), "not locked after being away");
        ui.wait_until("the question put away", |ui| ui.find("prompt-card").is_none());
        assert_eq!(ui.tabs(), 1, "the tab closed under the lock");
        assert_eq!(ui.status(), Status::Connected);

        ui.right_click("lock-card");
        assert!(!ui.view(|v, _| v.menu_open()), "the terminal's menu opened under the lock");
    }
);

scenario!(
    /// Quit on the lock asks first while connected, as any quit does;
    /// Cancel leaves it locked.
    quitting_from_the_lock_asks_first,
    |ui: &mut Ui| {
        answering(ui, How::System, false, 5);
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.wait_until("the refusal said", |ui| said(ui).is_some());
        ui.click("lock-quit");
        ui.wait_until("the question", |ui| ui.cx.has_pending_prompt());
        let (message, _) = ui.cx.pending_prompt().expect("a prompt");
        assert_eq!(message, "Quit shellwing?");
        ui.answer_prompt("Cancel");
        assert!(locked(ui));
        assert_eq!(ui.status(), Status::Connected);
    }
);

scenario!(
    /// A note's window takes no key while shellwing is locked: typed into
    /// its title under the cover, nothing reaches it.
    a_note_takes_no_key_while_locked,
    |ui: &mut Ui| {
        answering(ui, How::System, true, 5);
        ui.open();
        ui.wait_until("unlocked at start", |ui| !locked(ui));
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.ws(|ws, _, cx| ws.open_note(None, cx));
        ui.wait_until("the note's window", |ui| ui.note_window().is_some());
        ui.wait_until("its text", |ui| ui.note(|n, _, _| n.is_loaded()));
        let window = ui.note_window().expect("a note window").into();
        ui.note(|n, window, cx| n.focus_title(window, cx));
        ui.type_in(window, "Disk");
        assert_eq!(ui.note(|n, _, cx| n.title_text(cx)), "Disk");

        ui.palette("Lock shellwing");
        assert!(locked(ui));
        ui.note(|n, window, cx| n.focus_title(window, cx));
        ui.type_in(window, "usage");
        ui.keys_in(window, "backspace");
        assert_eq!(
            ui.note(|n, _, cx| n.title_text(cx)),
            "Disk",
            "keys reached the note under the lock"
        );
    }
);

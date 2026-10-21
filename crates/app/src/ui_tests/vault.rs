//! Saved passwords: offered after a password that worked, used next time
//! without asking. (The vault keeps its key in memory in tests, as with a
//! system keychain.)

use super::{Policy, USER, Ui, WELCOME, scenario};

/// Waits for the offer to save the password and answers it.
fn answer_offer(ui: &mut Ui, save: bool) {
    ui.wait_until("the offer to save the password", |ui| {
        ui.view(|v, _| v.save_offered())
    });
    ui.view(|v, cx| v.answer_save_offer(save, cx));
}

scenario!(
    /// Save: the next connection to the host signs in with no question.
    saved_password_is_used_next_time,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, true);
        ui.wait_for_notice_in("Saved in the vault", crate::notice::Tone::Good);
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked again");
        assert_eq!(
            server.seen(|s| s.auth.clone()),
            ["password:ok", "password:ok"]
        );
    }
);

scenario!(
    /// Not now: nothing is kept, the next connection asks again.
    password_not_saved,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, false);
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
    }
);

scenario!(
    /// A saved password that stopped working (changed on the server): it is
    /// tried, then the user is asked, and can sign in.
    saved_password_changed_on_the_server,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, true);
        ui.wait_for_notice("Saved in the vault");
        // The same address, a server with another password.
        server.kill();
        let changed = crate::ui_tests::Server::start_with_key(
            &ui.rt,
            Policy {
                password: Some("new-password".into()),
                ..Default::default()
            },
            russh::keys::PrivateKey::random(
                &mut rand::rng(),
                russh::keys::ssh_key::Algorithm::Ed25519,
            )
            .expect("key"),
            server.port,
        );
        // Its key is new too: trust it, as after a reinstall.
        std::fs::write(ui.dir.join("known_hosts"), format!("{}\n", changed.known_hosts_line()))
            .expect("known_hosts");
        ui.palette(&changed.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for("password:");
        ui.line("new-password");
        ui.wait_for(WELCOME);
        assert_eq!(changed.seen(|s| s.auth.clone()), ["password:wrong", "password:ok"]);
    }
);

scenario!(
    /// A vault with a passphrase, out of the system keychain, locked: a
    /// connection then asks for the password (the saved one is in the locked
    /// vault); once the passphrase unlocks it, the next connection signs in
    /// with the saved password again.
    locked_vault_keeps_its_passwords_until_unlocked,
    |ui| {
        const PASSPHRASE: &str = "correct horse battery";
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, true);
        ui.wait_for_notice("Saved in the vault");
        // Settings › Vault: a passphrase (typed twice), then out of the
        // keychain, so that it can lock.
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Vault");
        ui.click("vault-pass");
        ui.line(PASSPHRASE);
        ui.line(PASSPHRASE);
        ui.wait_for_notice("The passphrase opens the vault too now.");
        ui.click("vault-leave");
        ui.wait_for_notice("out of the system keychain");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette("Lock the vault");
        ui.wait_for_notice("The vault is locked.");
        // Locked: the password is asked for.
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for(&format!("{USER}@127.0.0.1's password:"));
        ui.keys("ctrl-c");
        // Unlocked: the saved password signs in again.
        ui.palette("Unlock the vault");
        ui.wait_until("the passphrase asked for", |ui| {
            ui.ws(|ws, _, _| ws.palette_open())
        });
        ui.line(PASSPHRASE);
        ui.wait_for_notice("The vault is open.");
        ui.palette(&server.spec());
        ui.wait_until("a third tab", |ui| ui.tabs() == 3);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked again");
        let auth = server.seen(|s| s.auth.clone());
        assert_eq!(auth.last().map(String::as_str), Some("password:ok"), "{auth:?}");
    }
);

scenario!(
    /// The recovery key, shown in Settings › Vault and copied, opens the
    /// locked vault in place of its passphrase; the saved password signs in
    /// again.
    recovery_key_unlocks_the_vault,
    |ui| {
        const PASSPHRASE: &str = "correct horse battery";
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, true);
        ui.wait_for_notice("Saved in the vault");
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Vault");
        // Show the recovery key; Enter copies it.
        ui.click("vault-rk");
        ui.wait_until("the key shown", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.keys("enter");
        ui.wait_until("the key copied", |ui| ui.clipboard().is_some());
        let key = ui.clipboard().unwrap_or_default();
        assert_eq!(key.split('-').count(), 13, "{key}");
        // A passphrase, out of the keychain, locked.
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Vault");
        ui.click("vault-pass");
        ui.line(PASSPHRASE);
        ui.line(PASSPHRASE);
        ui.wait_for_notice("The passphrase opens the vault too now.");
        ui.click("vault-leave");
        ui.wait_for_notice("out of the system keychain");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette("Lock the vault");
        ui.wait_for_notice("The vault is locked.");
        // The recovery key, typed where the passphrase is asked.
        ui.palette("Unlock the vault");
        ui.wait_until("the question", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(&key);
        ui.wait_for_notice("The vault is open.");
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked again");
    }
);

scenario!(
    /// Saved passwords and passphrases… (palette): a saved password forgotten
    /// (Enter, then Enter again to be sure); the next connection asks again.
    forget_a_saved_password,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        answer_offer(ui, true);
        ui.wait_for_notice("Saved in the vault");
        ui.palette("Saved passwords and passphrases");
        ui.wait_until("the saved secrets", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.keys("enter");
        ui.keys("enter");
        ui.wait_for_notice("Forgotten.");
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
    }
);

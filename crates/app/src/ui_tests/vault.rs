//! Saved passwords: offered after a password that worked, used next time
//! without asking. (The vault keeps its key in memory in tests, as with a
//! system keychain.)

use super::{Policy, Ui, WELCOME, scenario};

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
        ui.wait_for_notice("Saved in the vault");
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

//! Keys the app makes and keeps (Settings › Keys), installed on a host as a
//! user does: Install on a host…, then signing in with the key.

use super::{Policy, Server, USER, Ui, WELCOME, scenario};

/// hosts.toml with the host `web` for `server`.
fn web(ui: &Ui, server: &Server) {
    ui.config_file(
        "hosts.toml",
        &format!(
            "[[host]]\nname = \"web\"\naddress = \"127.0.0.1\"\nport = {}\nuser = \"{USER}\"\n",
            server.port
        ),
    );
}

/// Settings › Keys › Generate key…: a key named `name`, made and kept in
/// the vault; the dialog that says it is ready stays open.
fn make_key(ui: &mut Ui, name: &str) {
    ui.palette("Settings");
    ui.wait_until("Settings", |ui| ui.settings_page().is_some());
    ui.click("settings-page:Keys");
    ui.click("keys-generate");
    ui.keys("secondary-a");
    ui.type_text(name);
    ui.keys("enter");
    ui.wait_until("the key to be made", |ui| ui.find("gen-install").is_some());
}

scenario!(
    /// A key made in Settings › Keys and installed with Install on a host…:
    /// the host's tab opens, the password signs in, the key goes into the
    /// server's authorized keys, and once the offer to use it for the host
    /// is taken, the host signs in with it, with no password.
    make_a_key_and_install_it_on_a_host,
    |ui| {
        let server = ui.server(Policy {
            install: true,
            ..Policy::password()
        });
        ui.trust(&server);
        web(ui, &server);
        ui.open();
        make_key(ui, "work-key");
        ui.click("gen-install");
        ui.wait_until("the host chooser", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line("web");
        ui.login();
        ui.wait_until("the offer to use the key", |ui| ui.find("key-use").is_some());
        let installed = server.seen(|s| s.installed.clone());
        assert_eq!(installed.len(), 1, "{installed:?}");
        assert!(installed[0].starts_with("ssh-ed25519 "), "{installed:?}");
        ui.click("key-use");
        ui.wait_until("hosts.toml to name the key", |ui| {
            ui.hosts_toml().contains("key = \"vault:")
        });
        ui.palette("web");
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked for the password");
        let auth = server.seen(|s| s.auth.clone());
        assert_eq!(auth.last().map(String::as_str), Some("publickey:ok"), "{auth:?}");
    }
);

scenario!(
    /// A server that refuses to run the install script: the user is told the
    /// key was not installed, and nothing is offered.
    install_refused_by_the_server,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        web(ui, &server);
        ui.open();
        make_key(ui, "work-key");
        ui.click("gen-install");
        ui.wait_until("the host chooser", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line("web");
        ui.login();
        ui.wait_until("the notice", |ui| {
            ui.notice()
                .is_some_and(|n| n.contains("work-key was not installed on web"))
        });
        assert!(ui.find("key-use").is_none(), "using the key was offered");
        assert!(!ui.hosts_toml().contains("vault:"));
    }
);

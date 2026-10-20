//! Saved hosts (hosts.toml) and settings (config.toml), as a user writes
//! them, and what they change.

use super::{PASSWORD, Policy, USER, WELCOME, scenario};
use crate::session::Status;

/// A hosts.toml entry `name` for the server, with more lines.
fn host(name: &str, port: u16, more: &str) -> String {
    format!(
        "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {port}\nuser = \"{USER}\"\n{more}\n"
    )
}

scenario!(
    /// A saved host, opened by typing its name in the palette: the tab is
    /// named after it.
    saved_host_from_the_palette,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("prod-api", server.port, ""));
        ui.open();
        ui.palette("prod-api");
        ui.login();
        assert_eq!(ui.view(|v, _| v.session().title.clone()), "prod-api");
    }
);

scenario!(
    /// A host's `run`: typed into the shell once it is up.
    saved_host_runs_a_command_after_connecting,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file(
            "hosts.toml",
            &host("worker", server.port, "run = \"tail app.log\""),
        );
        ui.connect("worker");
        ui.login();
        ui.wait_for("you said: tail app.log");
    }
);

scenario!(
    /// A host's forwards start with it.
    saved_host_forwards,
    |ui| {
        let service = crate::ui_tests::server::echo_service(&ui.rt);
        let server = ui.server(Policy {
            forward: true,
            ..Policy::password()
        });
        ui.trust(&server);
        let local = crate::ui_tests::free_port();
        ui.config_file(
            "hosts.toml",
            &host(
                "db",
                server.port,
                &format!("forward = [\"-L {local}:127.0.0.1:{service}\"]"),
            ),
        );
        ui.connect("db");
        ui.login();
        ui.wait_for(&format!("forwarding localhost:{local}"));
        assert_eq!(crate::ui_tests::talk(local, "via hosts.toml"), "echo: via hosts.toml");
    }
);

scenario!(
    /// `host_key = "accept-new"` (for one host): a first visit does not ask,
    /// and the key is saved.
    accept_new_host_keys_for_one_host,
    |ui| {
        let server = ui.server(Policy::password());
        ui.config_file(
            "hosts.toml",
            &host("lab", server.port, "host_key = \"accept-new\""),
        );
        ui.connect("lab");
        ui.wait_for("password:");
        assert!(!ui.screen().contains("continue connecting"));
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        let key = server.host_key.to_openssh().expect("key");
        assert!(ui.known_hosts().contains(&key));
    }
);

scenario!(
    /// `[ssh] host_key = "accept-new"` in config.toml, for every host.
    accept_new_host_keys_everywhere,
    |ui| {
        let server = ui.server(Policy::password());
        ui.config_file("config.toml", "[ssh]\nhost_key = \"accept-new\"\n");
        ui.connect(&server.spec());
        ui.wait_for("password:");
        assert!(!ui.screen().contains("continue connecting"));
    }
);

scenario!(
    /// `[terminal] on_disconnect = "close"`: the tab goes when the shell
    /// exits.
    tab_closes_on_exit_when_set_so,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("config.toml", "[terminal]\non_disconnect = \"close\"\n");
        ui.connect(&server.spec());
        ui.login();
        ui.line("exit");
        ui.wait_until("the tab to close", |ui| ui.tabs() == 0);
    }
);

scenario!(
    /// A host added to hosts.toml while the app runs can be opened at once.
    hosts_file_edited_while_running,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.open();
        ui.config_file("hosts.toml", &host("added-later", server.port, ""));
        ui.reload_config();
        ui.palette("added-later");
        ui.login();
        assert_eq!(ui.status(), Status::Connected);
    }
);

scenario!(
    /// A hosts.toml with a mistake: the app still opens, says what is wrong,
    /// and connecting by address works.
    broken_hosts_file,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", "[[host]]\nname = \"half\nport = oops\n");
        ui.open();
        ui.palette(&server.spec());
        ui.login();
    }
);

scenario!(
    /// A custom key for the palette in config.toml's `[shortcuts]`.
    custom_shortcut,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("config.toml", "[shortcuts]\npalette = [\"ctrl+alt+j\"]\n");
        ui.open();
        ui.keys("ctrl-alt-j");
        ui.wait_until("the palette", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(&server.spec());
        ui.login();
    }
);

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

/// A MobaXterm export: a host already in hosts.toml, one in a folder in a
/// folder going through a jump host, and a remote desktop.
const MOBAXTERM: &str = "[Bookmarks]\r\nSubRep=\r\nImgNum=42\r\n\
web=#109#0%10.0.1.11%22%deploy%#MobaFont%10#0# #-1\r\n\r\n\
[Bookmarks_1]\r\nSubRep=Prod\\EU\r\nImgNum=41\r\n\
api=#109#0%10.0.1.12%2222%ops%%%%%bastion.example.com%22%admin#MobaFont%10#0#Order API#-1\r\n\
desktop=#91#4%10.0.2.5%3389#MobaFont%10#0# #-1\r\n";

scenario!(
    /// Hosts imported from a MobaXterm export chosen in the file dialog: the
    /// preview leaves the host already here unchecked and adds the jump host;
    /// Import writes the rest with their group, keeping hosts.toml's
    /// comments, and the description as a note; Undo import takes it all out
    /// (`docs/design/import.md`).
    hosts_are_imported_from_another_app_and_undone,
    |ui| {
        use shellwing_import::State;
        ui.config_file(
            "hosts.toml",
            "# Mine.\n[[host]]\nname = \"web\"\naddress = \"10.0.1.11\"\nuser = \"deploy\"\n",
        );
        let export = ui.dir.join("sessions.mxtsessions");
        std::fs::write(&export, MOBAXTERM).expect("export");
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Import hosts from another app");
        ui.wait_until("the import drawer", |ui| ui.import(|_, _| ()).is_some());
        ui.import(|v, cx| v.click_choose_file(cx));
        ui.wait_until("the file dialog", |ui| ui.cx.did_prompt_for_paths());
        let chosen = export.clone();
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
        ui.wait_until("the preview", |ui| ui.import(|v, _| v.rows()).flatten().is_some());
        let rows = ui.import(|v, _| v.rows()).flatten().expect("rows");
        assert_eq!(
            rows,
            [
                ("web".to_string(), false, State::Here("web".into())),
                ("api".to_string(), true, State::New),
                ("bastion.example.com".to_string(), true, State::ForJump),
            ]
        );
        ui.import(|v, cx| v.click_import(cx));
        ui.wait_for_notice("Imported 2 hosts from MobaXterm. 1 left out.");
        assert!(ui.import(|_, _| ()).is_none(), "the drawer closed");
        let hosts = ui.hosts_toml();
        assert!(hosts.starts_with("# Mine."), "{hosts}");
        for line in [
            "name = \"api\"",
            "group = \"Prod / EU\"",
            "jump = [\"bastion.example.com\"]",
            "tags = [\"imported:mobaxterm\"]",
        ] {
            assert!(hosts.contains(line), "{line} in:\n{hosts}");
        }
        ui.wait_until("the hosts listed", |ui| ui.host_names().contains(&"api".to_string()));
        let vault = ui.vault().expect("vault");
        let notes = vault.notes().expect("notes");
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].title.as_str(), notes[0].hosts.as_slice()), ("api", &["api".to_string()][..]));

        ui.palette("Undo import from MobaXterm");
        ui.wait_for_notice("The import from MobaXterm is undone.");
        let hosts = ui.hosts_toml();
        assert!(!hosts.contains("api") && !hosts.contains("Prod / EU"), "{hosts}");
        assert!(hosts.contains("name = \"web\""), "{hosts}");
        assert!(vault.notes().expect("notes").is_empty());
    }
);

scenario!(
    /// Settings › General opens the import drawer over Settings; closing it
    /// leaves Settings where it was.
    import_is_offered_in_settings,
    |ui| {
        ui.open();
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:General");
        ui.click("import-hosts");
        ui.wait_until("the import drawer", |ui| ui.import(|_, _| ()).is_some());
        ui.draw();
        assert!(ui.find("import-file").is_some(), "the drawer is drawn over Settings");
        ui.keys("escape");
        ui.wait_until("the drawer to close", |ui| ui.import(|_, _| ()).is_none());
        assert_eq!(ui.settings_page(), Some(crate::settings_view::Page::General));
    }
);

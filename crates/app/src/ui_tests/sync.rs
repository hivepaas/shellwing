//! Syncing, as a user sets it up: Settings › Sync, and a folder shared with
//! another device, played in the test by a second vault joined to the same
//! folder with the recovery key (a laptop and a desktop sharing a cloud
//! drive).

use std::collections::BTreeMap;

use shellwing_vault::{Folder, MemoryKeys, Snippet, Vault};

use super::{Policy, USER, Ui, scenario};

scenario!(
    /// A folder chosen to sync with; another device joins it and adds a host
    /// and a snippet; after Sync now, the host is in the list and the
    /// snippet in the vault here.
    changes_from_another_device_arrive,
    |ui| {
        let server = ui.server(Policy::password());
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        let shared = ui.dir.join("cloud-drive");
        std::fs::create_dir_all(&shared).expect("folder");
        ui.palette("Sync passwords with a folder");
        ui.wait_until("the folder dialog", |ui| ui.cx.did_prompt_for_paths());
        let chosen = shared.clone();
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
        ui.wait_until("the folder set up", |_| {
            shared.join("shellwing/format.json").exists()
        });
        ui.keys("escape");
        let key = ui
            .vault()
            .and_then(|v| v.recovery_key().ok().flatten())
            .expect("recovery key")
            .to_string();
        // The other device.
        let keys = MemoryKeys::default();
        let other_dir = ui.dir.join("other-device");
        std::fs::create_dir_all(&other_dir).expect("other dir");
        let other = Vault::open(&other_dir.join("vault.db"), &keys).expect("other vault");
        let folder = Folder::new(&shared);
        other.join(&folder, &key).expect("join");
        let fields: BTreeMap<String, String> = [
            ("name", "db".to_string()),
            ("address", "127.0.0.1".to_string()),
            ("port", server.port.to_string()),
            ("user", USER.to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        other
            .update("0190a1b2c3d4e5f60718293a4b5c6d7e", "host", "db", &fields)
            .expect("host");
        other
            .save_snippet(&Snippet {
                id: String::new(),
                name: "Uptime".into(),
                command: "uptime".into(),
                scope: String::new(),
                enter: true,
                category: String::new(),
                needs: vec![],
                origin: String::new(),
            })
            .expect("snippet");
        other.sync(&folder).expect("the other device syncs");
        // Here.
        ui.palette("Sync now");
        ui.wait_until("the host from the other device", |ui| {
            ui.reload_config();
            ui.host_names().contains(&"db".to_string())
        });
        assert!(ui.hosts_toml().contains("name = \"db\""), "{}", ui.hosts_toml());
        ui.wait_until("the snippet from the other device", |ui| {
            ui.vault()
                .and_then(|v| v.snippets().ok())
                .is_some_and(|s| s.iter().any(|s| s.name == "Uptime"))
        });
    }
);

scenario!(
    /// A Git repository that is not there: Connect says why on the Sync page,
    /// beside Connect and over the page, and keeps nothing in config.toml.
    /// (Said under Settings, on the tab, nobody saw it.)
    a_git_repository_that_fails_says_why_in_settings,
    |ui: &mut Ui| {
        let missing = ui.config_dir().join("no-such-repo.git");
        ui.open();
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Sync");
        ui.click("sync-kind:3");
        ui.click("sync-field:Repository");
        ui.type_text(&format!("file://{}", missing.display()));
        ui.click("sync-connect");
        ui.wait_until("the reason beside Connect", |ui| {
            ui.sync_problem().is_some_and(|p| p.starts_with("Cannot sync"))
        });
        ui.wait_for_notice("Cannot sync");
        let config = std::fs::read_to_string(ui.config_dir().join("config.toml"))
            .unwrap_or_default();
        assert!(!config.contains("[sync]"), "{config}");
    }
);

/// Another device that has synced with `shared` already, holding the host
/// `db` for `server` and its saved password; its vault, the folder, and its
/// recovery key.
fn other_device_first(
    ui: &Ui,
    shared: &std::path::Path,
    server: &crate::ui_tests::Server,
) -> (Vault, Folder, String) {
    std::fs::create_dir_all(shared).expect("folder");
    let other_dir = ui.dir.join("other-device");
    std::fs::create_dir_all(&other_dir).expect("other dir");
    let keys = MemoryKeys::default();
    let other = Vault::open(&other_dir.join("vault.db"), &keys).expect("other vault");
    let folder = Folder::new(shared);
    let key = other.start_sync(&folder).expect("start").to_string();
    let fields: BTreeMap<String, String> = [
        ("name", "db".to_string()),
        ("address", "127.0.0.1".to_string()),
        ("port", server.port.to_string()),
        ("user", USER.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    other
        .update("0190a1b2c3d4e5f60718293a4b5c6d7f", "host", "db", &fields)
        .expect("host");
    other
        .set_password(
            &format!("{USER}@127.0.0.1:{}", server.port),
            crate::ui_tests::PASSWORD,
        )
        .expect("password");
    other.sync(&folder).expect("the other device syncs");
    (other, folder, key)
}

/// Sync passwords with a folder…, choosing `shared`, which holds another
/// device's vault: the app asks how to join it.
fn choose_folder(ui: &mut Ui, shared: &std::path::Path) {
    ui.open();
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    ui.palette("Sync passwords with a folder");
    ui.wait_until("the folder dialog", |ui| ui.cx.did_prompt_for_paths());
    let chosen = shared.to_path_buf();
    ui.cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    ui.wait_until("how to join", |ui| ui.ws(|ws, _, _| ws.palette_open()));
}

/// Once joined: the other device's host is listed, and its saved password
/// signs in to it with no question.
fn joined_with_the_other_devices_records(ui: &mut Ui, server: &crate::ui_tests::Server) {
    // Joined: the question goes, and the folder is kept in config.toml.
    // ("Joined: …" is said, then at once what the first sync brought.)
    ui.wait_until("joined", |ui| {
        !ui.ws(|ws, _, _| ws.palette_open())
            && std::fs::read_to_string(ui.config_dir().join("config.toml"))
                .is_ok_and(|c| c.contains("[sync]"))
    });
    ui.wait_until("the other device's host", |ui| {
        ui.reload_config();
        ui.host_names().contains(&"db".to_string())
    });
    ui.wait_until("the other device's password", |ui| {
        ui.vault()
            .and_then(|v| v.password(&format!("{USER}@127.0.0.1:{}", server.port)).ok().flatten())
            .is_some()
    });
    ui.click("host:db");
    ui.wait_for(crate::ui_tests::WELCOME);
    assert!(!ui.screen().contains("password:"), "asked for the password");
    assert_eq!(server.seen(|s| s.auth.clone()), ["password:ok"]);
}

scenario!(
    /// A folder that holds another device's vault, joined by typing its
    /// recovery key: the other device's hosts and passwords are here.
    join_a_folder_with_the_recovery_key,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        let shared = ui.dir.join("cloud-drive");
        let (_other, _folder, key) = other_device_first(ui, &shared, &server);
        choose_folder(ui, &shared);
        ui.line("Type the recovery key");
        ui.wait_until("the recovery key asked for", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(&key);
        joined_with_the_other_devices_records(ui, &server);
    }
);

scenario!(
    /// The same folder joined with no key typed: approved on the other
    /// device, which sees this one ask.
    join_a_folder_approved_on_the_other_device,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        let shared = ui.dir.join("cloud-drive");
        let (other, folder, _key) = other_device_first(ui, &shared, &server);
        choose_folder(ui, &shared);
        ui.line("Approve it on a device that syncs");
        let mut answerer = shellwing_vault::Answerer::new("Laptop");
        let mut asked = None;
        ui.wait_until("this device's request, on the other", |_| {
            if let Ok(requests) = answerer.poll(&other, &folder) {
                asked = requests.into_iter().next().or(asked.take());
            }
            asked.is_some()
        });
        let request = asked.expect("a request");
        answerer.approve(&other, &folder, &request.id).expect("approve");
        joined_with_the_other_devices_records(ui, &server);
    }
);

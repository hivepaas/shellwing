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
        ui.wait_for_notice_in("Cannot sync", crate::notice::Tone::Bad);
        let config = std::fs::read_to_string(ui.config_dir().join("config.toml"))
            .unwrap_or_default();
        assert!(!config.contains("[sync]"), "{config}");
    }
);

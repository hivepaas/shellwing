//! Settings › Sync, as a user sets it up: what Connect says while Settings
//! covers the tabs.

use super::{Ui, scenario};

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

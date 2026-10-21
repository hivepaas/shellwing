//! What covers the window (Settings, the host and group editors, the
//! palette) keeps the mouse from the terminal under it.

use super::scenario;
use crate::session::Status;

scenario!(
    /// A right click on Settings opens no terminal menu under it; on the
    /// terminal, once Settings is closed, it does.
    a_right_click_on_settings_is_not_the_terminals,
    |ui| {
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.right_click("settings-content");
        assert!(!ui.view(|v, _| v.menu_open()), "the terminal's menu opened under Settings");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.right_click("terminal");
        assert!(ui.view(|v, _| v.menu_open()), "no menu on the terminal");
    }
);

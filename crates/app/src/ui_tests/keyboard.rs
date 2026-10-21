//! Settings › Keyboard, as a user changes a shortcut: click its key, press
//! the new one.

use super::{Ui, scenario};

/// Settings › Keyboard, shown.
fn keyboard_page(ui: &mut Ui) {
    ui.palette("Settings");
    ui.wait_until("Settings", |ui| ui.settings_page().is_some());
    ui.click("settings-page:Keyboard");
}

fn config(ui: &Ui) -> String {
    std::fs::read_to_string(ui.config_dir().join("config.toml")).unwrap_or_default()
}

/// A shortcut as config.toml writes it: `ctrl+shift+<key>`, `cmd+<key>`
/// on macOS.
fn written(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("\"cmd+{key}\"")
    } else {
        format!("\"ctrl+shift+{key}\"")
    }
}

scenario!(
    /// The palette's key changed in Settings › Keyboard: the new key opens
    /// it, the old one no longer does, and config.toml keeps it.
    palette_key_changed_in_settings,
    |ui| {
        ui.open();
        keyboard_page(ui);
        ui.click("key:Palette:0");
        ui.shortcut("y");
        ui.wait_until("config.toml", |ui| config(ui).contains(&written("y")));
        assert!(!config(ui).contains(&written("k")), "{}", config(ui));
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.shortcut("k");
        ui.settle(std::time::Duration::from_millis(100));
        assert!(!ui.ws(|ws, _, _| ws.palette_open()), "the old key opened it");
        ui.shortcut("y");
        ui.wait_until("the palette", |ui| ui.ws(|ws, _, _| ws.palette_open()));
    }
);

scenario!(
    /// A key another shortcut has: the page asks, and Swap gives each the
    /// other's key.
    key_of_another_shortcut_swapped,
    |ui| {
        ui.open();
        keyboard_page(ui);
        ui.click("key:Palette:0");
        ui.shortcut("t");
        ui.wait_until("the question", |ui| ui.find("key-ask-yes").is_some());
        ui.click("key-ask-yes");
        ui.wait_until("config.toml", |ui| {
            let c = config(ui);
            c.contains("palette") && c.contains(&written("t")) && c.contains("new_tab")
        });
        let c = config(ui);
        let new_tab = c
            .lines()
            .find(|l| l.trim_start().starts_with("new_tab"))
            .unwrap_or_default();
        assert!(new_tab.contains(&written("k")), "New tab did not get the palette's key:\n{c}");
    }
);

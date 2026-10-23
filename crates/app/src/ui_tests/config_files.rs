//! Config files kept in the vault and placed on servers (the pane's menu):
//! kept from a server, placed to stay, and placed over a file until the tab
//! closes, then taken away with the old file put back. The server side is
//! OpenSSH's sftp-server, so these run where it is installed.

use std::path::PathBuf;

use shellwing_vault::ConfigFile;

use super::server::has_sftp;
use super::{Policy, Server, Ui, scenario};

/// A server whose SFTP home holds `notes.txt`, signed in to; the vault open.
fn with_sftp(ui: &mut Ui) -> Option<(PathBuf, Server)> {
    if !has_sftp() {
        eprintln!("skipped: OpenSSH's sftp-server is not installed");
        return None;
    }
    let home = ui.dir.join("remote");
    std::fs::create_dir_all(&home).expect("remote dir");
    std::fs::write(home.join("notes.txt"), "remember the milk\n").expect("file");
    let server = ui.server(Policy {
        sftp: Some(home.clone()),
        ..Policy::password()
    });
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    Some((home, server))
}

/// A config file kept in the vault for every host: `content`, to go to
/// `path` with `mode`.
fn kept(ui: &mut Ui, name: &str, path: &str, mode: u32, content: &[u8]) {
    ui.vault()
        .expect("vault")
        .save_config_file(
            &ConfigFile {
                id: String::new(),
                name: name.into(),
                category: String::new(),
                path: path.into(),
                mode,
                scope: String::new(),
                size: content.len(),
                binary: false,
                stays: false,
            },
            Some(content),
        )
        .expect("kept");
}

/// Place Config File… (the pane's menu), the first file, at the place it
/// names (Enter), then the way chosen by `how` (typed, then Enter).
fn place(ui: &mut Ui, how: &str) {
    ui.right_click("terminal");
    ui.click("menu-place-file");
    ui.wait_until("the files", |ui| ui.ws(|ws, _, _| ws.palette_open()));
    ui.keys("enter");
    ui.wait_until("where", |ui| ui.ws(|ws, _, _| ws.palette_open()));
    ui.keys("enter");
    ui.wait_until("how", |ui| ui.ws(|ws, _, _| ws.palette_open()));
    ui.line(how);
}

scenario!(
    /// Keep a File in the Vault… (the pane's menu): a file on the server,
    /// named from the home folder, is kept in the vault whole, and listed in
    /// Settings › Config files.
    keep_a_server_file_in_the_vault,
    |ui| {
        let Some((_home, _server)) = with_sftp(ui) else {
            return;
        };
        ui.right_click("terminal");
        ui.click("menu-save-file");
        ui.wait_until("the question", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        // The answer starts with "~/".
        ui.line("notes.txt");
        ui.wait_for_notice("Kept ~/notes.txt in the vault");
        let vault = ui.vault().expect("vault");
        let file = vault
            .config_files()
            .expect("files")
            .into_iter()
            .find(|f| f.name == "notes.txt")
            .expect("kept");
        assert_eq!((file.path.as_str(), file.mode), ("~/notes.txt", 0o600));
        let content = vault.config_file_content(&file.id).expect("read").expect("content");
        assert_eq!(content.as_slice(), b"remember the milk\n");
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:ConfigFiles");
        ui.wait_until("it listed", |ui| ui.find("file-edit-0").is_some());
    }
);

scenario!(
    /// Place Config File…, to stay: the file is written where it goes, the
    /// folders made, with its permissions.
    place_a_config_file_to_stay,
    |ui| {
        let Some((home, _server)) = with_sftp(ui) else {
            return;
        };
        kept(ui, "settings.ini", "~/.config/tool/settings.ini", 0o640, b"[tool]\nlevel = 3\n");
        place(ui, "Place it to stay");
        ui.wait_for_notice("Placed settings.ini at");
        let placed = home.join(".config/tool/settings.ini");
        assert_eq!(std::fs::read(&placed).expect("placed"), b"[tool]\nlevel = 3\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&placed).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }
    }
);

scenario!(
    /// Placed over the file there until the tab closes: the server's file is
    /// put aside; Remove (the pane's menu) takes the placed one away and puts
    /// the old one back.
    placed_over_a_file_until_removed,
    |ui| {
        let Some((home, _server)) = with_sftp(ui) else {
            return;
        };
        kept(ui, "notes.txt", "~/notes.txt", 0o600, b"from the vault\n");
        place(ui, "Replace the file there until the tab closes");
        ui.wait_for_notice("until the tab closes");
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt")).expect("placed"),
            "from the vault\n"
        );
        ui.right_click("terminal");
        ui.click("menu-remove-files");
        ui.wait_for_notice("Removed 1 file.");
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt")).expect("put back"),
            "remember the milk\n",
            "the server's own file is not back"
        );
        let left: Vec<String> = std::fs::read_dir(&home)
            .expect("home")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("shellwing"))
            .collect();
        assert!(left.is_empty(), "left behind: {left:?}");
    }
);

/// What `state/placed.json` notes, to take away later.
fn noted(ui: &Ui) -> String {
    std::fs::read_to_string(ui.config_dir().join("state/placed.json")).unwrap_or_default()
}

/// Closes the tab, connected (Close, asked), and waits for it to go.
fn close_tab(ui: &mut Ui) {
    ui.shortcut("w");
    ui.answer_prompt("Close");
    ui.wait_until("the tab to close", |ui| ui.tabs() == 0);
}

scenario!(
    /// Placed over a file until the tab closes, then kept (the pane's menu):
    /// the server's old file becomes its backup, and closing the tab leaves
    /// the placed one.
    placed_over_a_file_then_kept,
    |ui| {
        let Some((home, _server)) = with_sftp(ui) else {
            return;
        };
        kept(ui, "notes.txt", "~/notes.txt", 0o600, b"from the vault\n");
        place(ui, "Replace the file there until the tab closes");
        ui.wait_for_notice("until the tab closes");
        ui.right_click("terminal");
        ui.click("menu-keep-files");
        ui.wait_for_notice("stays when the tab closes. What was there before is");
        assert!(ui.view(|v, _| v.placed_files().is_empty()));
        assert!(!noted(ui).contains("notes.txt"), "still noted to take away");
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt.bak")).expect("backup"),
            "remember the milk\n"
        );
        assert!(!home.join("notes.txt.shellwing-saved").exists());
        close_tab(ui);
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt")).expect("kept"),
            "from the vault\n",
            "taken away though kept"
        );
    }
);

scenario!(
    /// Placed until the tab closes, then kept with its badge's Keep: closing
    /// the tab leaves it.
    kept_from_its_badge,
    |ui| {
        let Some((home, _server)) = with_sftp(ui) else {
            return;
        };
        kept(ui, "npmrc", "~/.npmrc", 0o600, b"registry=https://r.example\n");
        place(ui, "Place it until the tab closes");
        ui.wait_for_notice("until the tab closes");
        assert!(noted(ui).contains(".npmrc"));
        ui.click("file-keep-0");
        ui.wait_for_notice("stays when the tab closes.");
        assert!(!noted(ui).contains(".npmrc"), "still noted to take away");
        close_tab(ui);
        assert_eq!(
            std::fs::read_to_string(home.join(".npmrc")).expect("kept"),
            "registry=https://r.example\n"
        );
    }
);

scenario!(
    /// A file set to stay (Settings › Config files › When placed): Enter,
    /// placing it, places it to stay.
    a_file_set_to_stay_is_placed_to_stay,
    |ui| {
        let Some((home, _server)) = with_sftp(ui) else {
            return;
        };
        // Not named like Settings, which the palette would find first.
        kept(ui, "tool.ini", "~/.config/tool/tool.ini", 0o600, b"[tool]\n");
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:ConfigFiles");
        ui.wait_until("it listed", |ui| ui.find("file-edit-0").is_some());
        ui.click("file-edit-0");
        ui.click("file-placing:1");
        ui.click("snippet-ok");
        ui.wait_until("it set to stay", |ui| {
            ui.vault()
                .and_then(|v| v.config_files().ok())
                .is_some_and(|f| f.first().is_some_and(|f| f.stays))
        });
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        // Enter only: the file's own way.
        place(ui, "");
        ui.wait_for_notice("Placed tool.ini at");
        assert!(
            ui.view(|v, _| v.placed_files().is_empty()),
            "placed until the tab closes"
        );
        assert!(home.join(".config/tool/tool.ini").exists());
    }
);

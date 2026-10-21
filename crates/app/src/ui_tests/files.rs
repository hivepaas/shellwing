//! The Files panel (SFTP): browsing, renaming, deleting, making folders and
//! downloading, checked on the server's disk. The server side is OpenSSH's
//! sftp-server, so these scenarios run where it is installed (Linux and
//! macOS); elsewhere they say so and pass.

use std::path::{Path, PathBuf};

use gpui::Entity;

use super::server::has_sftp;
use super::{Policy, Ui, scenario};
use crate::files::FilesPanel;

/// A server whose SFTP home holds `notes.txt` and `projects/readme.md`,
/// signed in to, with the Files panel open and ready. `None` without
/// sftp-server.
fn with_files(ui: &mut Ui) -> Option<(PathBuf, super::Server)> {
    if !has_sftp() {
        eprintln!("skipped: OpenSSH's sftp-server is not installed");
        return None;
    }
    let home = ui.dir.join("remote");
    std::fs::create_dir_all(home.join("projects")).expect("remote dir");
    std::fs::write(home.join("notes.txt"), "remember the milk\n").expect("file");
    std::fs::write(home.join("projects/readme.md"), "# readme\n").expect("file");
    let server = ui.server(Policy {
        sftp: Some(home.clone()),
        ..Policy::password()
    });
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    ui.shortcut("o");
    wait_ready(ui);
    Some((home, server))
}

fn panel(ui: &mut Ui) -> Entity<FilesPanel> {
    ui.view(|v, _| v.files_panel()).expect("the Files panel")
}

/// The panel's folder and entries.
fn shown(ui: &mut Ui) -> (Option<String>, Vec<String>, String) {
    let p = panel(ui);
    ui.cx.read(|cx| p.read(cx).shown())
}

fn wait_ready(ui: &mut Ui) {
    ui.wait_until("the files to be listed", |ui| {
        ui.view(|v, _| v.files_panel()).is_some() && shown(ui).2 == "ready"
    });
}

fn wait_entries(ui: &mut Ui, what: &str, f: impl Fn(&[String]) -> bool) {
    ui.wait_until(what, |ui| {
        let (_, entries, state) = shown(ui);
        state == "ready" && f(&entries)
    });
}

fn select(ui: &mut Ui, name: &str) {
    let p = panel(ui);
    let found = ui
        .cx
        .update(|cx| p.update(cx, |p, cx| p.select_named(name, cx)));
    assert!(found, "{name} is not listed: {:?}", shown(ui).1);
}

fn exists(home: &Path, rel: &str) -> bool {
    home.join(rel).exists()
}

scenario!(
    /// The panel lists the home folder; Enter opens a folder, Backspace goes
    /// back up.
    browse_folders,
    |ui| {
        let Some(_) = with_files(ui) else { return };
        let (_, entries, _) = shown(ui);
        assert!(entries.contains(&"notes.txt".to_string()), "{entries:?}");
        assert!(entries.contains(&"projects/".to_string()), "{entries:?}");
        select(ui, "projects");
        ui.keys("enter");
        wait_entries(ui, "the folder's entries", |e| e.contains(&"readme.md".to_string()));
        assert!(shown(ui).0.unwrap_or_default().ends_with("projects"));
        ui.keys("backspace");
        wait_entries(ui, "the home folder again", |e| e.contains(&"notes.txt".to_string()));
    }
);

scenario!(
    /// F2 renames a file on the server.
    rename_a_file,
    |ui| {
        let Some((home, _)) = with_files(ui) else { return };
        select(ui, "notes.txt");
        ui.keys("f2");
        ui.settle(std::time::Duration::from_millis(100));
        // The name is selected up to its extension: typing replaces it.
        ui.type_text("todo");
        ui.keys("enter");
        wait_entries(ui, "the new name", |e| e.contains(&"todo.txt".to_string()));
        assert!(exists(&home, "todo.txt") && !exists(&home, "notes.txt"));
    }
);

scenario!(
    /// Delete asks first; Enter confirms, and the file goes from the server.
    delete_a_file,
    |ui| {
        let Some((home, _)) = with_files(ui) else { return };
        select(ui, "notes.txt");
        ui.keys("delete");
        ui.settle(std::time::Duration::from_millis(200));
        assert!(exists(&home, "notes.txt"), "deleted without asking");
        ui.keys("enter");
        wait_entries(ui, "the file to go", |e| !e.contains(&"notes.txt".to_string()));
        assert!(!exists(&home, "notes.txt"));
    }
);

scenario!(
    /// Escape at the delete question keeps the file.
    delete_cancelled,
    |ui| {
        let Some((home, _)) = with_files(ui) else { return };
        select(ui, "notes.txt");
        ui.keys("delete");
        ui.settle(std::time::Duration::from_millis(200));
        ui.keys("escape");
        ui.settle(std::time::Duration::from_millis(300));
        assert!(exists(&home, "notes.txt"));
    }
);

scenario!(
    /// A new folder, named in the panel, appears on the server.
    make_a_folder,
    |ui| {
        let Some((home, _)) = with_files(ui) else { return };
        let p = panel(ui);
        let window = ui.window();
        window
            .update(&mut ui.cx, |_, window, cx| {
                p.update(cx, |p, cx| p.new_folder(window, cx))
            })
            .expect("window");
        ui.line("backups");
        wait_entries(ui, "the new folder", |e| e.contains(&"backups/".to_string()));
        assert!(home.join("backups").is_dir());
    }
);

scenario!(
    /// Enter on a file downloads it, whole.
    download_a_file,
    |ui| {
        let Some(_) = with_files(ui) else { return };
        let downloads = ui.dir.join("downloads");
        std::fs::create_dir_all(&downloads).expect("downloads");
        let p = panel(ui);
        let d = downloads.clone();
        ui.cx.update(|cx| p.update(cx, |p, _| p.set_downloads(d)));
        select(ui, "notes.txt");
        ui.keys("enter");
        ui.wait_until("the download", |_| downloads.join("notes.txt").exists());
        ui.wait_until("the whole file", |_| {
            std::fs::read_to_string(downloads.join("notes.txt")).ok().as_deref()
                == Some("remember the milk\n")
        });
    }
);

scenario!(
    /// A file changed on the server shows after a refresh (reopening the
    /// folder).
    changes_on_the_server_show_after_refresh,
    |ui| {
        let Some((home, _)) = with_files(ui) else { return };
        std::fs::write(home.join("new-on-server.log"), "x").expect("file");
        select(ui, "projects");
        ui.keys("enter");
        wait_entries(ui, "projects", |e| e.contains(&"readme.md".to_string()));
        ui.keys("backspace");
        wait_entries(ui, "the new file", |e| e.contains(&"new-on-server.log".to_string()));
    }
);

scenario!(
    /// A server without SFTP: the panel says so, the shell is unharmed.
    server_without_sftp,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.shortcut("o");
        ui.wait_until("the panel to fail", |ui| {
            ui.view(|v, _| v.files_panel()).is_some() && shown(ui).2.starts_with("failed")
        });
        ui.keys("escape");
        ui.line("shell still here");
        ui.wait_for("you said: shell still here");
    }
);

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

scenario!(
    /// Upload…: the file chosen in the dialog goes to the folder shown, as it
    /// is, and is listed.
    upload_a_file,
    |ui| {
        let Some((home, _server)) = with_files(ui) else {
            return;
        };
        let local = ui.dir.join("local");
        std::fs::create_dir_all(&local).expect("local dir");
        let report = local.join("report.csv");
        std::fs::write(&report, "day,visits\nmon,42\n").expect("file");
        ui.click("files-upload");
        ui.wait_until("the file dialog", |ui| ui.cx.did_prompt_for_paths());
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![report]));
        wait_entries(ui, "report.csv listed", |e| e.iter().any(|n| n == "report.csv"));
        ui.wait_until("the upload to finish", |_| {
            std::fs::read_to_string(home.join("report.csv")).is_ok_and(|t| t == "day,visits\nmon,42\n")
        });
    }
);

/// Edit in text editor (right click › Edit), with `true` as the editor: the
/// copy is made and watched, as with a real editor. Returns the copy's path.
fn edit(ui: &mut Ui, name: &str) -> PathBuf {
    ui.right_click(&format!("file:{name}"));
    ui.click("menu-edit");
    let mut copy = PathBuf::new();
    ui.wait_until("the copy to be watched", |ui| {
        let p = panel(ui);
        let edits = ui.cx.read(|cx| p.read(cx).edits_shown());
        match edits.iter().find(|(n, ..)| n == name) {
            Some((_, path, step)) if step == "watching" => {
                copy = path.clone();
                true
            }
            _ => false,
        }
    });
    copy
}

/// Where the edit of `name` stands.
fn edit_step(ui: &mut Ui, name: &str) -> String {
    let p = panel(ui);
    let edits = ui.cx.read(|cx| p.read(cx).edits_shown());
    edits
        .into_iter()
        .find(|(n, ..)| n == name)
        .map(|(_, _, step)| step)
        .unwrap_or_default()
}

/// The editor saves `text` into the copy: a later time than it was written
/// with, as an editor's save has.
fn save_in_editor(copy: &Path, text: &str) {
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(copy, text).expect("save the copy");
}

/// `[files] editor = "true"`: an editor that opens nothing and returns.
const EDITOR: &str = "[files]\neditor = \"true\"\n";

scenario!(
    /// Editing a remote file: its copy opens in the editor, and each save is
    /// written back to the server.
    edit_a_file_and_save_it,
    |ui| {
        ui.config_file("config.toml", EDITOR);
        let Some((home, _server)) = with_files(ui) else {
            return;
        };
        let copy = edit(ui, "notes.txt");
        assert_eq!(
            std::fs::read_to_string(&copy).expect("copy"),
            "remember the milk\n"
        );
        save_in_editor(&copy, "remember the milk\nand the bread\n");
        ui.wait_until("the save to reach the server", |ui| {
            // The copy is looked at every second, on the app's clock.
            ui.advance(std::time::Duration::from_millis(300));
            std::fs::read_to_string(home.join("notes.txt"))
                .is_ok_and(|t| t == "remember the milk\nand the bread\n")
        });
        ui.wait_until("watching again", |ui| edit_step(ui, "notes.txt") == "watching");
    }
);

scenario!(
    /// A file changed on the server while it is being edited: the save does
    /// not overwrite it; the edit says it changed there, for the user to
    /// choose.
    edit_conflicts_with_a_change_on_the_server,
    |ui| {
        ui.config_file("config.toml", EDITOR);
        let Some((home, _server)) = with_files(ui) else {
            return;
        };
        let copy = edit(ui, "notes.txt");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(home.join("notes.txt"), "someone else wrote this\n").expect("write");
        save_in_editor(&copy, "my change\n");
        ui.wait_until("the conflict", |ui| {
            ui.advance(std::time::Duration::from_millis(300));
            edit_step(ui, "notes.txt") == "conflict"
        });
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt")).expect("server file"),
            "someone else wrote this\n",
            "the server's change was overwritten"
        );
    }
);

scenario!(
    /// The connection drops while a remote file is being edited (Wi-Fi gone,
    /// the server restarting): a save then cannot be written back, and the
    /// copy keeps it; once connected again, it is written back.
    edit_saved_while_disconnected_reaches_the_server_on_reconnect,
    |ui| {
        if !has_sftp() {
            eprintln!("skipped: OpenSSH's sftp-server is not installed");
            return;
        }
        ui.config_file("config.toml", EDITOR);
        let home = ui.dir.join("remote");
        std::fs::create_dir_all(&home).expect("remote dir");
        std::fs::write(home.join("notes.txt"), "remember the milk\n").expect("file");
        let key = russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .expect("key");
        let policy = Policy {
            sftp: Some(home.clone()),
            ..Policy::password()
        };
        let server = crate::ui_tests::Server::start_with_key(&ui.rt, policy.clone(), key.clone(), 0);
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.shortcut("o");
        wait_ready(ui);
        let copy = edit(ui, "notes.txt");
        // The server goes away; the editor saves meanwhile.
        server.kill();
        ui.wait_until("the session to be lost", |ui| {
            ui.status() == crate::session::Status::Lost
        });
        save_in_editor(&copy, "remember the milk\nwritten offline\n");
        ui.advance(std::time::Duration::from_millis(1200));
        assert_eq!(
            std::fs::read_to_string(home.join("notes.txt")).expect("server file"),
            "remember the milk\n",
            "written with no connection"
        );
        assert_eq!(
            std::fs::read_to_string(&copy).expect("copy"),
            "remember the milk\nwritten offline\n",
            "the copy lost the save"
        );
        // Back: the same server, the same key.
        let again = crate::ui_tests::Server::start_with_key(&ui.rt, policy, key, server.port);
        // The tab tries again on its own, on the app's clock (an attempt
        // while the server was away makes it wait longer).
        ui.wait_until("a new attempt", |ui| {
            ui.advance(std::time::Duration::from_millis(500));
            again.seen(|s| s.connections) >= 1
        });
        ui.wait_for_count("password:", 2);
        // The Files panel has the keys: a click in the terminal, then the
        // password.
        ui.click_cell(1, 1, gpui::Modifiers::none());
        ui.line(crate::ui_tests::PASSWORD);
        ui.wait_until("connected again", |ui| {
            ui.status() == crate::session::Status::Connected
        });
        ui.wait_until("the save written back", |ui| {
            ui.advance(std::time::Duration::from_millis(300));
            std::fs::read_to_string(home.join("notes.txt"))
                .is_ok_and(|t| t == "remember the milk\nwritten offline\n")
        });
    }
);

scenario!(
    /// Download As… (right click): the file goes where the save dialog says.
    download_as,
    |ui| {
        let Some((_home, _server)) = with_files(ui) else {
            return;
        };
        ui.right_click("file:notes.txt");
        ui.click("menu-download-as");
        ui.wait_until("the save dialog", |ui| ui.cx.did_prompt_for_new_path());
        let target = ui.dir.join("chosen-name.txt");
        let chosen = target.clone();
        ui.cx.simulate_new_path_selection(move |_| Some(chosen));
        ui.wait_until("the download", |_| {
            std::fs::read_to_string(&target).is_ok_and(|t| t == "remember the milk\n")
        });
    }
);

scenario!(
    /// The `..` row above a folder's files: a double click goes up.
    parent_row_goes_up,
    |ui| {
        let Some((_home, _server)) = with_files(ui) else {
            return;
        };
        let p = panel(ui);
        ui.cx.update(|cx| p.update(cx, |p, cx| p.select_named("projects", cx)));
        ui.keys("enter");
        wait_entries(ui, "projects' files", |e| e == ["readme.md"]);
        ui.double_click("files-parent");
        wait_entries(ui, "the home folder again", |e| e.iter().any(|n| n == "notes.txt"));
        assert!(shown(ui).0.is_some_and(|d| d.ends_with("remote")));
    }
);

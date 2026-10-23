//! Syncing, as a user sets it up: Settings › Sync, and a folder shared with
//! another device, played in the test by a second vault joined to the same
//! folder with the recovery key (a laptop and a desktop sharing a cloud
//! drive).

use std::collections::BTreeMap;

use shellwing_presence::confirm::How;
use shellwing_vault::{Folder, MemoryKeys, Snippet, Vault};

use super::{Policy, USER, Ui, lock, scenario};

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
        // Connect needs the vault open (else it says so, as a notice, and
        // goes no further); a busy machine opens it later than the click.
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Sync");
        ui.click("sync-kind:3");
        ui.click("sync-field:Repository");
        // A file URL as git takes it everywhere: file:///C:/… on Windows
        // (file://C:\… names a host "C:", which git looks for on the network).
        let path = missing.display().to_string().replace('\\', "/");
        ui.type_text(&format!("file:///{}", path.trim_start_matches('/')));
        ui.click("sync-connect");
        // git itself, started under a full run's load, can take long on
        // Windows: up to the 90 s the app gives it.
        ui.wait_until_within(
            "the reason beside Connect",
            std::time::Duration::from_secs(120),
            |ui| ui.sync_problem().is_some_and(|p| p.starts_with("Cannot sync")),
        );
        ui.wait_for_notice_in("Cannot sync", crate::notice::Tone::Bad);
        let config = std::fs::read_to_string(ui.config_dir().join("config.toml"))
            .unwrap_or_default();
        assert!(!config.contains("[sync]"), "{config}");
    }
);

/// A bare repository anyone can read, served over plain HTTP as git's dumb
/// protocol reads it (it takes no push): its address.
fn public_repository(ui: &Ui) -> String {
    use std::io::{Read, Write};
    let root = ui.dir.join("public-git");
    std::fs::create_dir_all(&root).expect("dir");
    for args in [
        &[
            "-c",
            "init.defaultBranch=main",
            "init",
            "-q",
            "--bare",
            "vault.git",
        ][..],
        &["-C", "vault.git", "update-server-info"],
    ] {
        let made = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .expect("git");
        assert!(made.success(), "git {args:?}");
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let path = path
                .split('?')
                .next()
                .unwrap_or("/")
                .trim_start_matches('/');
            let answer = match std::fs::read(root.join(path)) {
                Ok(body) => [
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes(),
                    body,
                ]
                .concat(),
                Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = s.write_all(&answer);
        }
    });
    format!("http://127.0.0.1:{port}/vault.git")
}

scenario!(
    /// A Git repository anyone can read: Connect asks first. Another one, the
    /// first answer, connects nothing and says why beside Connect; Use it
    /// anyway goes on to sync there (refused here: the test's server takes no
    /// push).
    a_public_git_repository_is_asked_about_first,
    |ui: &mut Ui| {
        let url = public_repository(ui);
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        sync_page(ui);
        ui.click("sync-kind:3");
        ui.click("sync-field:Repository");
        ui.type_text(&url);
        ui.click("sync-connect");
        ui.answer_prompt("Use another repository");
        ui.wait_until("the reason beside Connect", |ui| {
            ui.sync_problem()
                .is_some_and(|p| p.contains("anyone can read this repository"))
        });
        let config = std::fs::read_to_string(ui.config_dir().join("config.toml"))
            .unwrap_or_default();
        assert!(!config.contains("[sync]"), "{config}");

        ui.click("sync-connect");
        ui.answer_prompt("Use it anyway");
        ui.wait_until_within(
            "the sync to be tried there",
            std::time::Duration::from_secs(120),
            |ui| ui.sync_problem().is_some_and(|p| p.starts_with("Cannot sync")),
        );
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
    ui.cx
        .simulate_path_prompt_response(move |_| Some(vec![chosen]));
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
            .and_then(|v| {
                v.password(&format!("{USER}@127.0.0.1:{}", server.port))
                    .ok()
                    .flatten()
            })
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

/// Syncing here with a new, empty folder, chosen from the palette: the
/// folder, and the vault's recovery key (as the user writes it down).
fn sync_with_a_new_folder(ui: &mut Ui) -> (std::path::PathBuf, String) {
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    let shared = ui.dir.join("cloud-drive");
    std::fs::create_dir_all(&shared).expect("folder");
    ui.palette("Sync passwords with a folder");
    ui.wait_until("the folder dialog", |ui| ui.cx.did_prompt_for_paths());
    let chosen = shared.clone();
    ui.cx
        .simulate_path_prompt_response(move |_| Some(vec![chosen]));
    ui.wait_until("the folder set up", |_| {
        shared.join("shellwing/format.json").exists()
    });
    // The recovery key is shown: written down, put away.
    ui.keys("escape");
    let key = ui
        .vault()
        .and_then(|v| v.recovery_key().ok().flatten())
        .expect("recovery key")
        .to_string();
    (shared, key)
}

/// Another device, joined to `shared` with the recovery key, synced once.
fn other_device_joins(ui: &Ui, shared: &std::path::Path, key: &str) -> (Vault, Folder) {
    let other_dir = ui.dir.join("other-device");
    std::fs::create_dir_all(&other_dir).expect("other dir");
    let other =
        Vault::open(&other_dir.join("vault.db"), &MemoryKeys::default()).expect("other vault");
    let folder = Folder::new(shared);
    other.join(&folder, key).expect("join");
    other.sync(&folder).expect("the other device syncs");
    (other, folder)
}

/// The hosts the other device has: (name, fields), after syncing again.
fn hosts_there(other: &Vault, folder: &Folder) -> Vec<(String, BTreeMap<String, String>)> {
    let _ = other.sync(folder);
    other
        .entries("host")
        .unwrap_or_default()
        .into_iter()
        .map(|(_, f)| (f.get("name").cloned().unwrap_or_default(), f))
        .collect()
}

/// Sync now, here.
fn sync_now(ui: &mut Ui) {
    ui.palette("Sync now");
}

/// A hosts.toml entry `name` for 127.0.0.1:`port`, with more lines.
fn host_line(name: &str, port: u16, more: &str) -> String {
    format!(
        "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {port}\nuser = \"{USER}\"\n{more}\n"
    )
}

scenario!(
    /// What is made here reaches the other device: a host, a saved password,
    /// a snippet. A host kept on this device only does not.
    changes_made_here_reach_the_other_device,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &[
                host_line("web", 2201, ""),
                host_line("laptop-only", 2202, "local_only = true"),
            ]
            .join("\n"),
        );
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let vault = ui.vault().expect("vault");
        vault
            .set_password(&format!("{USER}@127.0.0.1:2201"), "made here")
            .expect("password");
        vault
            .save_snippet(&Snippet {
                id: String::new(),
                name: "Disk usage".into(),
                command: "df -h".into(),
                scope: String::new(),
                enter: true,
                category: String::new(),
                needs: vec![],
                origin: String::new(),
            })
            .expect("snippet");
        let (other, folder) = other_device_joins(ui, &shared, &key);
        sync_now(ui);
        ui.wait_until("the host, password and snippet there", |_| {
            let hosts = hosts_there(&other, &folder);
            hosts.iter().any(|(n, _)| n == "web")
                && other
                    .password(&format!("{USER}@127.0.0.1:2201"))
                    .ok()
                    .flatten()
                    .is_some_and(|p| p.as_str() == "made here")
                && other
                    .snippets()
                    .is_ok_and(|s| s.iter().any(|s| s.name == "Disk usage"))
        });
        let hosts = hosts_there(&other, &folder);
        assert!(
            !hosts.iter().any(|(n, _)| n == "laptop-only"),
            "a host kept on this device went out: {hosts:?}"
        );
    }
);

scenario!(
    /// A host deleted on the other device goes from here too; one deleted
    /// here (in the host editor) goes from there.
    deletions_travel_both_ways,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &[host_line("web", 2201, ""), host_line("db", 2202, "")].join("\n"),
        );
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        sync_now(ui);
        ui.wait_until("both hosts there", |_| hosts_there(&other, &folder).len() == 2);
        // Deleted there.
        let db = other
            .entries("host")
            .expect("hosts")
            .into_iter()
            .find(|(_, f)| f.get("name").is_some_and(|n| n == "db"))
            .map(|(id, _)| id)
            .expect("db there");
        other.delete(&db).expect("delete");
        other.sync(&folder).expect("sync");
        sync_now(ui);
        ui.wait_until("db gone here", |ui| {
            ui.reload_config();
            !ui.hosts_toml().contains("name = \"db\"")
                && !ui.host_names().contains(&"db".to_string())
        });
        // Deleted here.
        ui.hover("host:web");
        ui.click("edit-host:web");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.click("delete");
        ui.click("delete-yes");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        sync_now(ui);
        ui.wait_until("web gone there", |_| hosts_there(&other, &folder).is_empty());
    }
);

scenario!(
    /// The same host changed on two devices, each a different field: both
    /// changes are kept, here and there.
    edits_on_two_devices_are_both_kept,
    |ui| {
        ui.config_file("hosts.toml", &host_line("web", 2201, ""));
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        sync_now(ui);
        ui.wait_until("web there", |_| hosts_there(&other, &folder).len() == 1);
        // There: the port.
        let (id, mut fields) = other
            .entries("host")
            .expect("hosts")
            .into_iter()
            .next()
            .expect("web there");
        fields.insert("port".into(), "2299".into());
        other.update(&id, "host", "web", &fields).expect("edit there");
        other.sync(&folder).expect("sync there");
        // Here, meanwhile: the user, in the host editor.
        ui.hover("host:web");
        ui.click("edit-host:web");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.keys("tab tab tab tab"); // name, alias, address, port, to the user
        ui.keys("secondary-a");
        ui.type_text("deploy");
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        sync_now(ui);
        ui.wait_until("both edits here", |ui| {
            ui.reload_config();
            let h = ui.hosts_toml();
            h.contains("port = 2299") && h.contains("user = \"deploy\"")
        });
        ui.wait_until("both edits there", |_| {
            hosts_there(&other, &folder).iter().any(|(_, f)| {
                f.get("port").is_some_and(|p| p == "2299")
                    && f.get("user").is_some_and(|u| u == "deploy")
            })
        });
    }
);

/// A new device asks to join the vault synced here (pairing): the app is
/// woken to look, and the question it asks comes up. The asking device's
/// vault, the folder and its request.
fn a_device_asks_to_join(
    ui: &mut Ui,
    shared: &std::path::Path,
) -> (Vault, Folder, shellwing_vault::Ask) {
    let new_dir = ui.dir.join("new-device");
    std::fs::create_dir_all(&new_dir).expect("new device dir");
    let new = Vault::open(&new_dir.join("vault.db"), &MemoryKeys::default()).expect("vault");
    let folder = Folder::new(shared);
    let mut ask = shellwing_vault::Ask::new(&folder, "Desktop").expect("ask");
    sync_now(ui);
    ui.wait_until("the question here", |ui| {
        let _ = ask.step(&new, &folder);
        ui.cx.has_pending_prompt()
    });
    let (message, _) = ui.cx.pending_prompt().expect("a prompt");
    assert_eq!(message, "Let Desktop into this vault?");
    (new, folder, ask)
}

scenario!(
    /// Another device asks to join (pairing), and is approved here: it gets
    /// the vault's keys, and with them the password saved here.
    another_device_approved_here,
    |ui| {
        ui.open();
        let (shared, _key) = sync_with_a_new_folder(ui);
        let target = format!("{USER}@127.0.0.1:2201");
        ui.vault()
            .expect("vault")
            .set_password(&target, "made here")
            .expect("password");
        sync_now(ui);
        let (new, folder, mut ask) = a_device_asks_to_join(ui, &shared);
        ui.answer_prompt("Approve");
        ui.wait_until("the new device in", |_| {
            matches!(ask.step(&new, &folder), Ok(shellwing_vault::AskState::Joined))
        });
        ui.wait_until("the password there", |_| {
            let _ = new.sync(&folder);
            new.password(&target)
                .ok()
                .flatten()
                .is_some_and(|p| p.as_str() == "made here")
        });
    }
);

scenario!(
    /// Another device asks to join, and is refused here: it is told so, and
    /// gets nothing.
    another_device_refused_here,
    |ui| {
        ui.open();
        let (shared, _key) = sync_with_a_new_folder(ui);
        let (new, folder, mut ask) = a_device_asks_to_join(ui, &shared);
        ui.answer_prompt("Refuse");
        ui.wait_until("the new device refused", |_| {
            matches!(ask.step(&new, &folder), Ok(shellwing_vault::AskState::Refused))
        });
        assert!(new.sync(&folder).is_err(), "a refused device could sync");
    }
);

scenario!(
    /// A device asking to join while shellwing is locked is not asked about
    /// over the lock, where anyone could approve it: the question waits for
    /// the user.
    a_device_asking_while_locked_waits_for_the_user,
    |ui| {
        let _answers = lock::answering(ui, How::System, true, 5);
        ui.open();
        ui.wait_until("unlocked at start", |ui| !lock::locked(ui));
        let (shared, _key) = sync_with_a_new_folder(ui);
        ui.palette("Lock shellwing");
        assert!(lock::locked(ui));

        let new_dir = ui.dir.join("new-device");
        std::fs::create_dir_all(&new_dir).expect("new device dir");
        let new = Vault::open(&new_dir.join("vault.db"), &MemoryKeys::default()).expect("vault");
        let folder = Folder::new(&shared);
        let mut ask = shellwing_vault::Ask::new(&folder, "Desktop").expect("ask");
        ui.ws(|ws, _, _| ws.sync_now());
        ui.wait_until("the request held", |ui| {
            let _ = ask.step(&new, &folder);
            ui.ws(|ws, _, _| ws.held_requests()) == 1
        });
        assert!(!ui.cx.has_pending_prompt(), "asked over the lock");

        ui.click("lock-unlock");
        ui.wait_until("the question, unlocked", |ui| ui.cx.has_pending_prompt());
        let (message, _) = ui.cx.pending_prompt().expect("a prompt");
        assert_eq!(message, "Let Desktop into this vault?");
        ui.answer_prompt("Refuse");
        ui.wait_until("the new device refused", |_| {
            matches!(ask.step(&new, &folder), Ok(shellwing_vault::AskState::Refused))
        });
    }
);

scenario!(
    /// Stop syncing: the folder is no longer in config.toml, and what the
    /// other device adds after does not come here.
    stopped_syncing_takes_nothing_more,
    |ui| {
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        ui.palette("Stop syncing");
        ui.wait_for_notice("Stopped syncing");
        let config =
            std::fs::read_to_string(ui.config_dir().join("config.toml")).unwrap_or_default();
        assert!(!config.contains("cloud-drive"), "{config}");
        let fields: BTreeMap<String, String> = [
            ("name", "db"),
            ("address", "127.0.0.1"),
            ("port", "2202"),
            ("user", USER),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        other
            .update("0190a1b2c3d4e5f60718293a4b5c6d80", "host", "db", &fields)
            .expect("host");
        other.sync(&folder).expect("sync there");
        ui.settle(std::time::Duration::from_secs(3));
        ui.reload_config();
        assert!(!ui.host_names().contains(&"db".to_string()), "{:?}", ui.host_names());
    }
);

/// Settings › Sync, shown.
fn sync_page(ui: &mut Ui) {
    ui.palette("Settings");
    ui.wait_until("Settings", |ui| ui.settings_page().is_some());
    ui.click("settings-page:Sync");
}

scenario!(
    /// Let a passphrase join new devices (Settings › Sync): a new device
    /// joins by typing it, with no recovery key and nobody approving, and
    /// gets the password saved here.
    a_passphrase_joins_a_new_device,
    |ui| {
        const PASSPHRASE: &str = "a long sync passphrase";
        ui.open();
        let (shared, _key) = sync_with_a_new_folder(ui);
        let target = format!("{USER}@127.0.0.1:2201");
        ui.vault()
            .expect("vault")
            .set_password(&target, "made here")
            .expect("password");
        sync_now(ui);
        sync_page(ui);
        ui.click("sync-passphrase");
        ui.wait_until("the passphrase asked for", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(PASSPHRASE);
        ui.line(PASSPHRASE);
        ui.wait_for_notice("New devices can join with the sync passphrase now.");
        let new_dir = ui.dir.join("new-device");
        std::fs::create_dir_all(&new_dir).expect("dir");
        let new = Vault::open(&new_dir.join("vault.db"), &MemoryKeys::default()).expect("vault");
        let folder = Folder::new(&shared);
        assert!(new.join_with_passphrase(&folder, "not it at all").is_err());
        new.join_with_passphrase(&folder, PASSPHRASE).expect("join");
        ui.wait_until("the password there", |_| {
            let _ = new.sync(&folder);
            new.password(&target)
                .ok()
                .flatten()
                .is_some_and(|p| p.as_str() == "made here")
        });
    }
);

scenario!(
    /// Settings sync too, as they do unless turned off (Settings › Sync ›
    /// Sync settings too): the theme chosen here reaches the other device,
    /// and one chosen there comes here.
    settings_travel_both_ways,
    |ui| {
        ui.config_file("config.toml", "[appearance]\ntheme = \"nord\"\n");
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        sync_now(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        let settings = |other: &Vault| {
            let _ = other.sync(&folder);
            other
                .entries("settings")
                .unwrap_or_default()
                .into_iter()
                .next()
        };
        ui.wait_until("the theme there", |_| {
            settings(&other).is_some_and(|(_, f)| {
                f.get("appearance.theme").is_some_and(|t| t.contains("nord"))
            })
        });
        let (id, mut fields) = settings(&other).expect("settings there");
        fields.insert("appearance.theme".into(), "\"dracula\"".into());
        other.update(&id, "settings", "Settings", &fields).expect("edit there");
        other.sync(&folder).expect("sync there");
        sync_now(ui);
        ui.wait_until("the theme here", |ui| {
            std::fs::read_to_string(ui.config_dir().join("config.toml"))
                .is_ok_and(|c| c.contains("theme = \"dracula\""))
        });
    }
);

scenario!(
    /// A hosts.toml with a mistake does not stop passwords syncing: the
    /// other device's saved password still comes, and the file is left as it
    /// is.
    a_broken_hosts_file_does_not_stop_passwords,
    |ui| {
        const BROKEN: &str = "[[host]]\nname = \"half\nport = oops\n";
        ui.config_file("hosts.toml", BROKEN);
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        let target = format!("{USER}@127.0.0.1:2201");
        other.set_password(&target, "made there").expect("password");
        other.sync(&folder).expect("sync there");
        sync_now(ui);
        ui.wait_until("the password here", |ui| {
            ui.vault()
                .and_then(|v| v.password(&target).ok().flatten())
                .is_some_and(|p| p.as_str() == "made there")
        });
        assert_eq!(ui.hosts_toml(), BROKEN, "hosts.toml was changed");
    }
);

scenario!(
    /// A locked vault does not sync: the other device's host waits; once the
    /// passphrase unlocks the vault, it comes.
    a_locked_vault_syncs_once_unlocked,
    |ui| {
        const PASSPHRASE: &str = "correct horse battery";
        ui.open();
        let (shared, key) = sync_with_a_new_folder(ui);
        let (other, folder) = other_device_joins(ui, &shared, &key);
        // A passphrase, out of the keychain, locked (as in vault.rs).
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Vault");
        ui.click("vault-pass");
        ui.line(PASSPHRASE);
        ui.line(PASSPHRASE);
        ui.wait_for_notice("The passphrase opens the vault too now.");
        ui.click("vault-leave");
        ui.wait_for_notice("out of the system keychain");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette("Lock the vault");
        ui.wait_for_notice("The vault is locked.");
        // There: a host.
        let fields: BTreeMap<String, String> = [
            ("name", "db"),
            ("address", "127.0.0.1"),
            ("port", "2202"),
            ("user", USER),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        other
            .update("0190a1b2c3d4e5f60718293a4b5c6d81", "host", "db", &fields)
            .expect("host");
        other.sync(&folder).expect("sync there");
        // Locked here: nothing comes.
        ui.settle(std::time::Duration::from_secs(3));
        ui.reload_config();
        assert!(!ui.hosts_toml().contains("name = \"db\""), "synced while locked");
        // The app asks for the passphrase on its own, a locked vault being
        // seen; typed, the vault is open and the host comes.
        ui.wait_until("the passphrase asked for", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(PASSPHRASE);
        // Open, it syncs at once ("Synced 1 change…" is said over "The vault
        // is open.").
        ui.wait_until("the vault open", |ui| ui.vault().is_some());
        ui.wait_until("the host here", |ui| {
            ui.reload_config();
            ui.host_names().contains(&"db".to_string())
        });
    }
);

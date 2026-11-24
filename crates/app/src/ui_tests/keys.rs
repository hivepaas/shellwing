//! Keys the app makes and keeps (Settings › Keys), installed on a host as a
//! user does: Install on a host…, then signing in with the key.

use super::{Policy, Server, USER, Ui, WELCOME, scenario};

/// hosts.toml with the host `web` for `server`.
fn web(ui: &Ui, server: &Server) {
    ui.config_file(
        "hosts.toml",
        &format!(
            "[[host]]\nname = \"web\"\naddress = \"127.0.0.1\"\nport = {}\nuser = \"{USER}\"\n",
            server.port
        ),
    );
}

/// Settings › Keys › Generate key…: a key named `name`, made and kept in
/// the vault; the dialog that says it is ready stays open.
fn make_key(ui: &mut Ui, name: &str) {
    ui.palette("Settings");
    ui.wait_until("Settings", |ui| ui.settings_page().is_some());
    ui.click("settings-page:Keys");
    ui.click("keys-generate");
    ui.keys("secondary-a");
    ui.type_text(name);
    ui.keys("enter");
    ui.wait_until("the key to be made", |ui| ui.find("gen-install").is_some());
}

scenario!(
    /// A key made in Settings › Keys and installed with Install on a host…:
    /// the host's tab opens, the password signs in, the key goes into the
    /// server's authorized keys, and once the offer to use it for the host
    /// is taken, the host signs in with it, with no password.
    make_a_key_and_install_it_on_a_host,
    |ui| {
        let server = ui.server(Policy {
            install: true,
            ..Policy::password()
        });
        ui.trust(&server);
        web(ui, &server);
        ui.open();
        make_key(ui, "work-key");
        ui.click("gen-install");
        ui.wait_until("the host chooser", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line("web");
        ui.login();
        ui.wait_until("the offer to use the key", |ui| ui.find("key-use").is_some());
        let installed = server.seen(|s| s.installed.clone());
        assert_eq!(installed.len(), 1, "{installed:?}");
        assert!(installed[0].starts_with("ssh-ed25519 "), "{installed:?}");
        ui.click("key-use");
        ui.wait_until("hosts.toml to name the key", |ui| {
            ui.hosts_toml().contains("key = \"vault:")
        });
        ui.palette_new_tab("web");
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked for the password");
        let auth = server.seen(|s| s.auth.clone());
        assert_eq!(auth.last().map(String::as_str), Some("publickey:ok"), "{auth:?}");
    }
);

scenario!(
    /// A server that refuses to run the install script: the user is told the
    /// key was not installed, and nothing is offered.
    install_refused_by_the_server,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        web(ui, &server);
        ui.open();
        make_key(ui, "work-key");
        ui.click("gen-install");
        ui.wait_until("the host chooser", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line("web");
        ui.login();
        ui.wait_until("the notice", |ui| {
            ui.notice()
                .is_some_and(|n| n.contains("work-key was not installed on web"))
        });
        assert!(ui.find("key-use").is_none(), "using the key was offered");
        assert!(!ui.hosts_toml().contains("vault:"));
    }
);

scenario!(
    /// Settings › Keys › Import key file…: the key file is kept in the vault,
    /// and signs in with no file on disk any more.
    imported_key_file_signs_in,
    |ui| {
        let (path, key) = crate::ui_tests::connect::key_file(ui, "id_work", None);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Keys");
        ui.click("keys-import");
        ui.wait_until("the file dialog", |ui| ui.cx.did_prompt_for_paths());
        let chosen = path.clone();
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
        ui.wait_until("the key in the vault", |ui| {
            ui.vault()
                .and_then(|v| v.ssh_keys().ok())
                .is_some_and(|keys| keys.iter().any(|k| k.name == "id_work"))
        });
        std::fs::remove_file(&path).expect("remove the key file");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette(&server.spec());
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked for a password");
        assert_eq!(
            server.seen(|s| s.auth.last().cloned()).as_deref(),
            Some("publickey:ok")
        );
    }
);

/// A key made as make_key does, its dialog closed: the list shows it.
fn made(ui: &mut Ui, name: &str) {
    make_key(ui, name);
    ui.click("gen-done");
    ui.wait_until("the key listed", |ui| ui.find("kept-more-0").is_some());
}

fn vault_keys(ui: &mut Ui) -> Vec<String> {
    ui.vault()
        .and_then(|v| v.ssh_keys().ok())
        .map(|k| k.into_iter().map(|k| k.name).collect())
        .unwrap_or_default()
}

scenario!(
    /// A vault key renamed, then deleted, from its ⋯ menu.
    rename_and_delete_a_vault_key,
    |ui| {
        ui.open();
        made(ui, "work-key");
        ui.click("kept-more-0");
        ui.click("menu-rename");
        ui.keys("secondary-a");
        ui.type_text("deploy-key");
        ui.keys("enter");
        ui.wait_until("renamed", |ui| vault_keys(ui) == ["deploy-key"]);
        ui.click("kept-more-0");
        ui.click("menu-delete");
        ui.answer_prompt("Delete");
        ui.wait_until("deleted", |ui| vault_keys(ui).is_empty());
    }
);

scenario!(
    /// Export to file…, with a passphrase: an OpenSSH key file that opens
    /// only with it, and its .pub beside it.
    export_a_key_with_a_passphrase,
    |ui| {
        const PASS: &str = "export passphrase";
        ui.open();
        made(ui, "work-key");
        ui.click("kept-more-0");
        ui.click("menu-export");
        ui.type_text(PASS);
        ui.click("field:Again");
        ui.type_text(PASS);
        ui.click("export-ok");
        ui.wait_until("the save dialog", |ui| ui.cx.did_prompt_for_new_path());
        let target = ui.dir.join("exported-key");
        let chosen = target.clone();
        ui.cx.simulate_new_path_selection(move |_| Some(chosen));
        ui.wait_until("the files", |_| {
            target.exists() && target.with_extension("pub").exists()
        });
        let text = std::fs::read_to_string(&target).expect("key file");
        assert!(shellwing_ssh::keys::parse(&text, Some(PASS)).is_ok(), "does not open");
        assert!(shellwing_ssh::keys::parse(&text, None).is_err(), "not locked");
        let public = std::fs::read_to_string(target.with_extension("pub")).expect("pub");
        assert!(public.starts_with("ssh-ed25519 "), "{public}");
    }
);

/// A key file only an administrator could read, in the test's folder: here
/// the stand-in for the system's prompt reads it. Its private key.
fn root_only_key(
    ui: &Ui,
    name: &str,
    passphrase: Option<&str>,
) -> (std::path::PathBuf, russh::keys::PrivateKey) {
    use shellwing_ssh::keys;
    let _ = crate::keys_view::TEST_AS_ADMIN.set(|p| {
        std::fs::read_to_string(p)
            .map(zeroize::Zeroizing::new)
            .map_err(|e| e.to_string())
    });
    let key = keys::generate(keys::KeyKind::Ed25519, "root@server").expect("key");
    let path = ui.config_dir().join(name);
    std::fs::write(
        &path,
        keys::export(&key, passphrase).expect("export").as_bytes(),
    )
    .expect("write");
    (path, key)
}

scenario!(
    /// The host editor's Key › Key file as administrator…: the path typed,
    /// the file read once (here by the tests' stand-in for the system's
    /// password prompt), the key goes into the vault and becomes the
    /// host's; saved, the host signs in with it, with no password.
    a_key_only_root_can_read_signs_in,
    |ui| {
        let (path, key) = root_only_key(ui, "root_key", None);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Policy::default()
        });
        ui.trust(&server);
        web(ui, &server);
        ui.open();
        ui.hover("host:web");
        ui.click("edit-host:web");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.click("method:1");
        ui.click("key-select");
        ui.click("choice:Key file as administrator…");
        ui.type_text(&path.display().to_string());
        ui.keys("enter");
        ui.wait_until("the key in the vault", |ui| ui.find("admin-key-read").is_none());
        ui.click("save");
        ui.wait_until("hosts.toml to name the key", |ui| {
            ui.hosts_toml().contains("key = \"vault:")
        });
        let kept = ui.vault().expect("vault").ssh_keys().expect("keys");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "root_key");
        ui.click("host:web");
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password:"), "asked for the password");
    }
);

scenario!(
    /// Settings › Keys › As administrator…: a key file with a passphrase is
    /// read, then its passphrase asked; a wrong one says so, the right one
    /// puts the key in the vault. A path that is not one is refused first.
    import_a_root_only_key_with_a_passphrase,
    |ui| {
        let (path, _) = root_only_key(ui, "locked_key", Some("hunter2"));
        ui.open();
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Keys");
        ui.click("keys-import-admin");
        ui.type_text("root/key");
        ui.keys("enter");
        ui.wait_until("a full path asked for", |ui| ui.find("as-admin-problem").is_some());
        assert!(ui.find("as-admin-pass").is_none());
        ui.keys("secondary-a");
        ui.type_text(&path.display().to_string());
        ui.keys("enter");
        ui.wait_until("its passphrase asked", |ui| ui.find("as-admin-pass").is_some());
        assert!(ui.find("as-admin-problem").is_none(), "not wrong yet");
        ui.type_text("nope");
        ui.keys("enter");
        ui.wait_until("a wrong passphrase said", |ui| ui.find("as-admin-problem").is_some());
        ui.keys("secondary-a");
        ui.type_text("hunter2");
        ui.keys("enter");
        ui.wait_until("the key in the vault", |ui| {
            ui.vault()
                .and_then(|v| v.ssh_keys().ok())
                .is_some_and(|k| k.iter().any(|k| k.name == "locked_key"))
        });
        assert!(ui.find("as-admin-ok").is_none(), "the dialog closed");
    }
);

//! Connecting and signing in: what a user meets before the shell.

use std::path::PathBuf;

use russh::keys::PrivateKey;
use russh::keys::ssh_key::{Algorithm, LineEnding};

use super::{OTP, PASSWORD, Policy, USER, Ui, WELCOME, scenario};
use crate::session::Status;

/// A new key pair in the scratch directory, encrypted with `passphrase` if
/// given; the private key's path.
pub(super) fn key_file(ui: &Ui, name: &str, passphrase: Option<&str>) -> (PathBuf, PrivateKey) {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    let stored = match passphrase {
        Some(p) => key.encrypt(&mut rand::rng(), p).expect("encrypt"),
        None => key.clone(),
    };
    let path = ui.dir.join(name);
    std::fs::write(
        &path,
        stored.to_openssh(LineEnding::LF).expect("pem").as_bytes(),
    )
    .expect("key file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("key file mode");
    }
    (path, key)
}

/// An ssh config entry `alias` for 127.0.0.1:`port`, with more lines.
pub(super) fn alias(alias: &str, port: u16, more: &str) -> String {
    format!("Host {alias}\n  HostName 127.0.0.1\n  Port {port}\n  User {USER}\n{more}")
}

scenario!(
    /// A known host: the password is asked for in the terminal, typed (and
    /// not shown), and the shell then answers what is typed.
    password_login_and_a_command,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for(&format!("{USER}@127.0.0.1's password:"));
        assert_eq!(ui.status(), Status::Connecting);
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        assert_eq!(ui.status(), Status::Connected);
        ui.line("hello shellwing");
        let screen = ui.wait_for("you said: hello shellwing");
        assert!(!screen.contains(PASSWORD), "the password was shown:\n{screen}");
        assert_eq!(server.seen(|s| s.auth.clone()), ["password:ok"]);
    }
);

scenario!(
    /// A mistyped password is asked for again, as OpenSSH does, and the right
    /// one then gets in.
    wrong_password_is_asked_again,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for("password:");
        ui.line("not-the-password");
        ui.wait_for("Permission denied, please try again.");
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        assert_eq!(
            server.seen(|s| s.auth.clone()),
            ["password:wrong", "password:ok"]
        );
    }
);

scenario!(
    /// A first visit: the key's fingerprint is shown, `yes` trusts it, and it
    /// is saved to known_hosts, so the next visit does not ask.
    new_host_key_is_asked_about_and_saved,
    |ui| {
        let server = ui.server(Policy::password());
        ui.connect(&server.spec());
        ui.wait_for("can't be established");
        ui.wait_for("SHA256:");
        ui.line("yes");
        ui.login();
        let key = server.host_key.to_openssh().expect("key");
        assert!(
            ui.known_hosts().contains(&key),
            "not saved:\n{}",
            ui.known_hosts()
        );
    }
);

scenario!(
    /// Answering `no` to a new host key: nothing is sent to the server (no
    /// password asked for), the tab says it failed, known_hosts stays empty.
    new_host_key_refused,
    |ui| {
        let server = ui.server(Policy::password());
        ui.connect(&server.spec());
        ui.wait_for("continue connecting");
        ui.line("no");
        ui.wait_until("the tab to fail", |ui| ui.status() == Status::Failed);
        assert!(!ui.screen().contains("password:"));
        assert!(server.seen(|s| s.auth.is_empty()));
        assert!(ui.known_hosts().trim().is_empty());
    }
);

scenario!(
    /// Anything but yes, no or the fingerprint is asked again.
    host_key_question_insists_on_an_answer,
    |ui| {
        let server = ui.server(Policy::password());
        ui.connect(&server.spec());
        ui.wait_for("continue connecting");
        ui.line("maybe");
        ui.wait_for("Please type 'yes', 'no' or the fingerprint");
        ui.line("yes");
        ui.login();
    }
);

scenario!(
    /// The host's key changed since the last visit (a reinstall, or someone in
    /// the middle): the connection stops before signing in, and the password
    /// is never sent.
    changed_host_key_stops_before_sign_in,
    |ui| {
        let server = ui.server(Policy::password());
        let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
        let known = format!(
            "[127.0.0.1]:{} {}\n",
            server.port,
            other.public_key().to_openssh().expect("key")
        );
        std::fs::write(ui.dir.join("known_hosts"), known).expect("known_hosts");
        ui.connect(&server.spec());
        ui.wait_until("the tab to fail", |ui| ui.status() == Status::Failed);
        let screen = ui.screen();
        assert!(!screen.contains("password:"), "asked for a password:\n{screen}");
        assert!(server.seen(|s| s.auth.is_empty()), "something was sent");
        assert!(
            ui.view(|v, _| v.session().changed_key().is_some()),
            "the change is not offered to review"
        );
    }
);

scenario!(
    /// A key from the ssh config's IdentityFile: in with no question.
    key_login_from_ssh_config,
    |ui| {
        let (path, key) = key_file(ui, "id_test", None);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", path.display()),
        ));
        ui.connect("box");
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password"));
        assert_eq!(server.seen(|s| s.auth.clone()), ["publickey:ok"]);
    }
);

scenario!(
    /// An encrypted key: its passphrase is asked for; a wrong one again.
    encrypted_key_asks_for_its_passphrase,
    |ui| {
        let (path, key) = key_file(ui, "id_locked", Some("correct horse"));
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", path.display()),
        ));
        ui.connect("box");
        ui.wait_for("Enter passphrase for key");
        ui.line("battery staple");
        ui.wait_for("Bad passphrase, try again.");
        ui.line("correct horse");
        ui.wait_for(WELCOME);
    }
);

scenario!(
    /// The key the server does not know is tried, then the password asked for.
    unknown_key_falls_back_to_password,
    |ui| {
        let (path, _) = key_file(ui, "id_other", None);
        let mut policy = Policy::password();
        let accepted = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
        policy.key = Some(accepted.public_key().clone());
        let server = ui.server(policy);
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", path.display()),
        ));
        ui.connect("box");
        ui.login();
    }
);

scenario!(
    /// Password, then a one-time code (keyboard-interactive), as with a
    /// second factor.
    password_then_one_time_code,
    |ui| {
        let server = ui.server(Policy {
            otp: Some(OTP.into()),
            ..Policy::password()
        });
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for("password:");
        ui.line(PASSWORD);
        ui.wait_for("Verification code:");
        ui.line(OTP);
        ui.wait_for(WELCOME);
        assert_eq!(server.seen(|s| s.auth.clone()), ["password:ok", "otp:ok"]);
    }
);

scenario!(
    /// A server that asks for the password through PAM (keyboard-interactive),
    /// as macOS's sshd does.
    password_through_keyboard_interactive,
    |ui| {
        let server = ui.server(Policy {
            pam: true,
            ..Policy::password()
        });
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for("Password:");
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains(PASSWORD));
    }
);

scenario!(
    /// Ctrl+C at the password prompt gives up: nothing more is asked or sent.
    ctrl_c_cancels_the_password_prompt,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for("password:");
        ui.type_text("half");
        ui.keys("ctrl-c");
        ui.wait_until("the tab to fail", |ui| ui.status() == Status::Failed);
        assert!(server.seen(|s| s.auth.is_empty()), "a password was sent");
    }
);

scenario!(
    /// An ssh config alias: its HostName, Port and User are used.
    ssh_config_alias,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.ssh_config(&alias("staging", server.port, ""));
        ui.connect("staging");
        ui.wait_for(&format!("{USER}@127.0.0.1's password:"));
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
    }
);

scenario!(
    /// No host on the command line: the palette's shortcut opens it, an
    /// address typed there and Enter open a tab that connects.
    connect_from_the_palette,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.open();
        assert_eq!(ui.tabs(), 0);
        ui.palette(&server.spec());
        ui.login();
        assert_eq!(ui.tabs(), 1);
    }
);

scenario!(
    /// ProxyJump: the jump host and the target are each checked and each
    /// asks for its password.
    proxy_jump_through_a_bastion,
    |ui| {
        let bastion = ui.server(Policy {
            forward: true,
            ..Policy::password()
        });
        let target = ui.server(Policy::password());
        ui.trust(&bastion);
        ui.trust(&target);
        ui.ssh_config(&format!(
            "{}{}",
            alias("bastion", bastion.port, ""),
            alias("inner", target.port, "  ProxyJump bastion\n")
        ));
        ui.connect("inner");
        ui.wait_for("password:");
        ui.line(PASSWORD);
        ui.wait_for_count("password:", 2);
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        assert_eq!(bastion.seen(|s| s.forwards.len()), 1);
        assert_eq!(target.seen(|s| s.auth.clone()), ["password:ok"]);
    }
);

scenario!(
    /// A key, then a one-time code: the usual two-factor setup
    /// (`AuthenticationMethods publickey,keyboard-interactive`).
    key_then_one_time_code,
    |ui| {
        let (path, key) = key_file(ui, "id_2fa", None);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            otp: Some(OTP.into()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", path.display()),
        ));
        ui.connect("box");
        ui.wait_for("Verification code:");
        ui.line(OTP);
        ui.wait_for(WELCOME);
        assert_eq!(server.seen(|s| s.auth.clone()), ["publickey:ok", "otp:ok"]);
    }
);

scenario!(
    /// A host connected to before is under Recent in the palette: a part of
    /// its address finds it, and Enter opens it.
    recent_connection_from_the_palette,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        let state = ui.config_dir().join("state");
        std::fs::create_dir_all(&state).expect("state dir");
        std::fs::write(
            state.join("recent.json"),
            format!("[\"{}\"]", server.spec()),
        )
        .expect("recent.json");
        ui.open();
        // Only a part of it: on its own, ":PORT" is no address to connect to.
        ui.palette(&format!(":{}", server.port));
        ui.login();
        assert_eq!(ui.tabs(), 1);
        assert_eq!(server.seen(|s| s.connections), 1);
    }
);

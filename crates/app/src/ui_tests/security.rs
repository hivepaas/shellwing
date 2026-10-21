//! Trust, as a user sets it up: OpenSSH certificates for users and hosts,
//! the ssh-agent and forwarding it, and taking a host out of known_hosts.

use russh::keys::PrivateKey;
use russh::keys::ssh_key::Algorithm;
use russh::keys::ssh_key::certificate::CertType;

use super::connect::{alias, key_file};
use super::server::{Server, certify, now, valid_now};
use super::{PASSWORD, Policy, USER, Ui, WELCOME, scenario};
use crate::session::Status;
use crate::settings_view::Page;

fn new_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key")
}

/// A key in the scratch directory with a certificate beside it
/// (`<key>-cert.pub`, where OpenSSH looks), signed by `ca` for [`USER`].
fn certified_key(ui: &Ui, ca: &PrivateKey, valid: std::ops::Range<u64>) -> std::path::PathBuf {
    let (path, key) = key_file(ui, "id_work", None);
    let cert = certify(ca, key.public_key(), CertType::User, USER, valid);
    let mut cert_path = path.clone().into_os_string();
    cert_path.push("-cert.pub");
    std::fs::write(cert_path, cert.to_openssh().expect("certificate")).expect("write");
    path
}

/// known_hosts trusting `ca` for every port of 127.0.0.1.
fn trust_authority(ui: &Ui, ca: &PrivateKey) {
    std::fs::write(
        ui.dir.join("known_hosts"),
        format!(
            "@cert-authority [127.0.0.1]:* {}\n",
            ca.public_key().to_openssh().expect("key")
        ),
    )
    .expect("known_hosts");
}

scenario!(
    /// A user certificate beside the key the ssh config names: in with no
    /// question, on the certificate.
    user_certificate_signs_in,
    |ui| {
        let ca = new_key();
        let key = certified_key(ui, &ca, valid_now());
        let server = ui.server(Policy {
            user_ca: Some(ca.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", key.display()),
        ));
        ui.connect("box");
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password"));
        let auth = server.seen(|s| s.auth.clone());
        assert_eq!(auth.last().map(String::as_str), Some("certificate:ok"), "{auth:?}");
    }
);

scenario!(
    /// An expired user certificate does not get in; the password is asked
    /// for, and gets in.
    expired_user_certificate_falls_back_to_the_password,
    |ui| {
        let ca = new_key();
        let key = certified_key(ui, &ca, now() - 7200..now() - 3600);
        let server = ui.server(Policy {
            user_ca: Some(ca.public_key().clone()),
            ..Policy::password()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", key.display()),
        ));
        ui.connect("box");
        ui.login();
        let auth = server.seen(|s| s.auth.clone());
        assert!(!auth.iter().any(|a| a == "certificate:ok"), "{auth:?}");
        assert_eq!(auth.last().map(String::as_str), Some("password:ok"), "{auth:?}");
    }
);

scenario!(
    /// A user certificate from an authority the server does not trust is
    /// refused; the password still gets in.
    user_certificate_from_another_authority_is_refused,
    |ui| {
        let (trusted, other) = (new_key(), new_key());
        let key = certified_key(ui, &other, valid_now());
        let server = ui.server(Policy {
            user_ca: Some(trusted.public_key().clone()),
            ..Policy::password()
        });
        ui.trust(&server);
        ui.ssh_config(&alias(
            "box",
            server.port,
            &format!("  IdentityFile {}\n", key.display()),
        ));
        ui.connect("box");
        ui.login();
        let auth = server.seen(|s| s.auth.clone());
        assert!(auth.contains(&"certificate:wrong".to_string()), "{auth:?}");
        assert_eq!(auth.last().map(String::as_str), Some("password:ok"), "{auth:?}");
    }
);

scenario!(
    /// A host whose key is certified by an authority known_hosts trusts
    /// (`@cert-authority`): no question about its key, and nothing learned.
    host_certificate_from_a_trusted_authority,
    |ui| {
        let ca = new_key();
        trust_authority(ui, &ca);
        let before = ui.known_hosts();
        let server = Server::start_certified(&ui.rt, Policy::password(), &ca, valid_now());
        ui.connect(&server.spec());
        ui.login();
        assert!(!ui.screen().contains("can't be established"));
        assert_eq!(ui.known_hosts(), before, "the host's key was learned");
    }
);

scenario!(
    /// A host certificate from an authority known_hosts does not trust stops
    /// the connection before signing in: no password is asked for or sent.
    host_certificate_from_an_unknown_authority_stops,
    |ui| {
        let (trusted, other) = (new_key(), new_key());
        trust_authority(ui, &trusted);
        let server = Server::start_certified(&ui.rt, Policy::password(), &other, valid_now());
        ui.connect(&server.spec());
        ui.wait_until("the tab to fail", |ui| ui.status() == Status::Failed);
        // The status changes before the terminal draws the reason.
        let screen = ui.wait_for("host certificate is not accepted");
        assert!(!screen.contains("password:"), "asked for a password:\n{screen}");
        assert!(server.seen(|s| s.auth.is_empty()), "something was sent");
    }
);

scenario!(
    /// An expired host certificate stops the connection too.
    expired_host_certificate_stops,
    |ui| {
        let ca = new_key();
        trust_authority(ui, &ca);
        let server =
            Server::start_certified(&ui.rt, Policy::password(), &ca, now() - 7200..now() - 3600);
        ui.connect(&server.spec());
        ui.wait_until("the tab to fail", |ui| ui.status() == Status::Failed);
        ui.wait_for("host certificate is not accepted");
        assert!(server.seen(|s| s.auth.is_empty()), "something was sent");
    }
);

#[cfg(unix)]
scenario!(
    /// A key the user's ssh-agent holds signs in, with no key file and no
    /// question.
    agent_key_signs_in,
    |ui| {
        let key = new_key();
        ui.agent(&[&key]);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for(WELCOME);
        assert!(!ui.screen().contains("password"));
        assert_eq!(server.seen(|s| s.auth.clone()), ["publickey:ok"]);
    }
);

/// hosts.toml with one host `name` for `server`, with more lines.
#[cfg(unix)]
fn saved(ui: &Ui, server: &Server, name: &str, more: &str) {
    ui.config_file(
        "hosts.toml",
        &format!(
            "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {}\nuser = \"{USER}\"\n{more}\n",
            server.port
        ),
    );
}

#[cfg(unix)]
scenario!(
    /// A host set to forward the agent (`forward_agent = true`): on the
    /// server, the agent answers with the user's keys.
    agent_forwarded_when_the_host_says_so,
    |ui| {
        let (a, b) = (new_key(), new_key());
        ui.agent(&[&a, &b]);
        let server = ui.server(Policy::password());
        ui.trust(&server);
        saved(ui, &server, "jump-box", "forward_agent = true");
        ui.connect("jump-box");
        ui.login();
        ui.line("agent");
        ui.wait_for("agent: 2 keys");
        assert!(server.seen(|s| s.agent));
    }
);

#[cfg(unix)]
scenario!(
    /// Without `forward_agent`, a server cannot reach the user's agent: it
    /// is not asked for, and a server that tries anyway gets nothing.
    agent_not_forwarded_unless_asked,
    |ui| {
        let key = new_key();
        ui.agent(&[&key]);
        let server = ui.server(Policy::password());
        ui.trust(&server);
        saved(ui, &server, "web", "");
        ui.connect("web");
        // The agent's key is tried first; the server knows none.
        ui.login();
        ui.line("agent");
        ui.wait_for("agent: none forwarded");
        assert!(!server.seen(|s| s.agent), "forwarding was asked for");
        assert!(matches!(server.seen(|s| s.agent_keys.clone()), Some(Err(_))));
    }
);

scenario!(
    /// Settings › Known hosts: Remove takes the host out of known_hosts
    /// (kept as known_hosts.old), and the next connection asks about its key
    /// again.
    removed_known_host_is_asked_about_again,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        let key = server.host_key.to_openssh().expect("key");
        ui.open();
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:KnownHosts");
        assert_eq!(ui.settings_page(), Some(Page::KnownHosts));
        ui.click("known-remove:1");
        ui.wait_until("the line removed", |ui| !ui.known_hosts().contains(&key));
        let old = std::fs::read_to_string(ui.dir.join("known_hosts.old")).unwrap_or_default();
        assert!(old.contains(&key), "known_hosts.old:\n{old}");
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette(&server.spec());
        ui.wait_for("can't be established");
        ui.line("yes");
        ui.wait_for("password:");
        ui.line(PASSWORD);
        ui.wait_for(WELCOME);
        assert!(ui.known_hosts().contains(&key), "learned again");
    }
);

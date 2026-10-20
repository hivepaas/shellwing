//! A session's life after signing in: how it ends, and what a user sees when
//! the server or the network goes away.

use super::server::{closed_port, silent_port};
use super::{PASSWORD, Policy, USER, WELCOME, scenario};
use crate::session::Status;

scenario!(
    /// `exit` ends the shell in order: the session is closed, not lost.
    exit_closes_the_session,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.line("exit");
        ui.wait_until("the session to close", |ui| {
            ui.tabs() == 0 || ui.status() == Status::Closed
        });
    }
);

scenario!(
    /// Ctrl+D on an empty line ends the shell, as in bash.
    ctrl_d_closes_the_session,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.keys("ctrl-d");
        ui.wait_until("the session to close", |ui| {
            ui.tabs() == 0 || ui.status() == Status::Closed
        });
    }
);

scenario!(
    /// The server drops the connection in the middle of a session (a crash,
    /// a reboot): the tab says the connection was lost, and keeps what was on
    /// the screen.
    server_drops_the_connection,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.line("before the crash");
        ui.wait_for("you said: before the crash");
        ui.line("die");
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
        assert!(ui.screen().contains("you said: before the crash"));
    }
);

scenario!(
    /// The server process goes away with no goodbye: lost too.
    server_killed,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        server.kill();
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
    }
);

scenario!(
    /// Nothing listens on the port: the attempt ends with a message, and the
    /// tab does not stay "connecting".
    connection_refused,
    |ui| {
        let port = closed_port();
        ui.connect(&format!("{USER}@127.0.0.1:{port}"));
        // The status can change before the terminal has drawn the reason.
        ui.wait_until("the attempt to end, with a reason", |ui| {
            let screen = ui.screen().to_lowercase();
            matches!(ui.status(), Status::Lost | Status::Failed)
                && (screen.contains("refused") || screen.contains("os error"))
        });
    }
);

scenario!(
    /// A host name that does not resolve.
    unknown_host_name,
    |ui| {
        ui.connect(&format!("{USER}@no-such-host.invalid"));
        ui.wait_until("the attempt to end", |ui| {
            matches!(ui.status(), Status::Lost | Status::Failed)
        });
        assert!(!ui.screen().contains("password:"));
    }
);

scenario!(
    /// A server that accepts the connection and then says nothing: the tab
    /// shows it is still connecting, and stays usable (a new tab opens).
    silent_server_keeps_connecting,
    |ui| {
        let port = silent_port(&ui.rt);
        ui.connect(&format!("{USER}@127.0.0.1:{port}"));
        ui.settle(std::time::Duration::from_secs(2));
        assert_eq!(ui.status(), Status::Connecting);
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
    }
);

scenario!(
    /// Typed while the shell starts: nothing is lost, it reaches the shell.
    typing_right_after_sign_in_reaches_the_shell,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.wait_for("password:");
        ui.line(PASSWORD);
        ui.line("early words");
        ui.wait_for(WELCOME);
        ui.wait_for("you said: early words");
    }
);

scenario!(
    /// A server slow to answer (a loaded machine, a long way off) still gets
    /// signed in to.
    slow_server,
    |ui| {
        let server = ui.server(Policy {
            slow_start: Some(std::time::Duration::from_secs(2)),
            ..Policy::password()
        });
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
    }
);

scenario!(
    /// The connection drops: the tab tries again on its own after a moment,
    /// and the user signs in again.
    lost_connection_reconnects_by_itself,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.line("die");
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
        // The first retry comes after a second.
        ui.advance(std::time::Duration::from_millis(1500));
        ui.wait_until("a new attempt", |_| server.seen(|s| s.connections) == 2);
        ui.wait_for_count("password:", 2);
        ui.line(PASSWORD);
        ui.wait_until("connected again", |ui| ui.status() == Status::Connected);
    }
);

scenario!(
    /// After `exit`, Enter connects again.
    enter_reconnects_a_closed_session,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.line("exit");
        ui.wait_until("the session to close", |ui| ui.status() == Status::Closed);
        ui.keys("enter");
        ui.wait_until("a new attempt", |_| server.seen(|s| s.connections) == 2);
        ui.wait_for_count("password:", 2);
        ui.line(PASSWORD);
        ui.wait_until("connected again", |ui| ui.status() == Status::Connected);
    }
);

scenario!(
    /// A failed sign-in (wrong passwords) can be tried again with Enter.
    enter_retries_after_failed_sign_in,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        for _ in 0..3 {
            ui.wait_until("a password prompt", |ui| ui.screen().contains("password:"));
            ui.line("wrong");
            ui.settle(std::time::Duration::from_millis(200));
        }
        ui.wait_until("the attempt to fail", |ui| ui.status() == Status::Failed);
        assert!(ui.screen().contains("permission denied"));
        ui.keys("enter");
        ui.wait_until("a new attempt", |_| server.seen(|s| s.connections) == 2);
        // Three prompts in the first attempt, a fourth in the new one.
        ui.wait_for_count("password:", 4);
        ui.line(PASSWORD);
        ui.wait_until("connected", |ui| ui.status() == Status::Connected);
    }
);

scenario!(
    /// The network goes silent (Wi-Fi gone, a laptop asleep): keepalives
    /// notice, the tab says the connection was lost, and once the network
    /// is back a new attempt gets through.
    network_goes_silent,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("config.toml", "[ssh]\nkeepalive = 1\n");
        ui.connect(&server.spec());
        ui.login();
        server.stall(true);
        // Three keepalives unanswered, a second apart.
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
        server.stall(false);
        ui.advance(std::time::Duration::from_millis(1500));
        ui.wait_until("a new attempt", |_| server.seen(|s| s.connections) >= 2);
        ui.wait_for_count("password:", 2);
        ui.line(PASSWORD);
        ui.wait_until("connected again", |ui| ui.status() == Status::Connected);
    }
);

scenario!(
    /// The server restarts (same address, same key): the tab reconnects.
    server_restarts,
    |ui| {
        let key = russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .expect("key");
        let server = crate::ui_tests::Server::start_with_key(&ui.rt, Policy::password(), key.clone(), 0);
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        server.kill();
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
        let again = crate::ui_tests::Server::start_with_key(&ui.rt, Policy::password(), key, server.port);
        ui.advance(std::time::Duration::from_millis(1500));
        ui.wait_until("a new attempt", |_| again.seen(|s| s.connections) >= 1);
        ui.wait_for_count("password:", 2);
        ui.line(PASSWORD);
        ui.wait_until("connected again", |ui| ui.status() == Status::Connected);
    }
);

scenario!(
    /// The server stays down: the tab keeps trying, backing off, and Escape
    /// stops the countdown.
    server_stays_down,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        server.kill();
        ui.wait_until("the session to be lost", |ui| ui.status() == Status::Lost);
        ui.advance(std::time::Duration::from_millis(1500));
        ui.wait_until("lost again", |ui| ui.status() == Status::Lost);
        ui.keys("escape");
        ui.advance(std::time::Duration::from_secs(5));
        assert_eq!(ui.status(), Status::Lost);
    }
);

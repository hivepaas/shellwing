//! Run on hosts: its tab, the ways to it, signing in once, sudo, the results.

use std::path::PathBuf;

use shellwing_core::run::Outcome;

use super::server::{PASSWORD, Server};
use super::{Policy, USER, Ui, scenario};

fn host(name: &str, port: u16, group: &str) -> String {
    format!(
        "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {port}\nuser = \"{USER}\"\ngroup = \"{group}\"\n"
    )
}

/// A server that runs what it is sent with `sh -c`, in a home of its own,
/// with a stand-in for sudo first in `PATH`: it asks its askpass, as
/// `sudo -A` does, and runs the command for the password `pa ss`.
fn shell_server(ui: &mut Ui) -> (Server, PathBuf) {
    let home = ui.config_dir().join("remote-home");
    let bin = ui.config_dir().join("remote-bin");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&bin).expect("bin");
    let sudo = bin.join("sudo");
    std::fs::write(
        &sudo,
        "#!/bin/sh\n\
         [ \"$1\" = -A ] || { echo 'sudo: a terminal is required to read the password' >&2; exit 1; }\n\
         shift; [ \"$1\" = -p ] && { prompt=$2; shift 2; }\n\
         pw=$(\"$SUDO_ASKPASS\" \"$prompt\")\n\
         [ \"$pw\" = 'pa ss' ] && exec \"$@\"\n\
         echo 'sudo: 3 incorrect password attempts' >&2; exit 1\n",
    )
    .expect("sudo");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sudo, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let server = ui.server(Policy {
        shell: Some((home.clone(), bin)),
        ..Policy::password()
    });
    ui.trust(&server);
    (server, home)
}

/// Answers every password the hosts ask, from the confirm step.
fn answer_passwords(ui: &mut Ui, hosts: usize) {
    for _ in 0..hosts {
        ui.wait_until("a host to ask", |ui| ui.run(|r, _| r.asked() > 0));
        ui.click("run-ask-input:0");
        ui.line(PASSWORD);
    }
}

/// Waits until the outputs' terminals show `want`, each with its last
/// line ending left out: a terminal just made draws what it is fed a
/// frame later.
fn shown(ui: &mut Ui, want: &[&str]) {
    ui.wait_until(&format!("the outputs to show {want:?}"), |ui| {
        let shown = ui.run(|r, cx| r.shown_outputs(cx));
        shown.iter().map(|s| s.trim_end()).eq(want.iter().copied())
    });
}

fn outcome<'a>(ended: &'a [(String, Outcome)], host: &str) -> &'a shellwing_core::run::Ran {
    match ended.iter().find(|(h, _)| h == host) {
        Some((_, Outcome::Ran(r))) => r,
        other => panic!("{host}: {other:?}"),
    }
}

scenario!(
    /// A group's ▶ opens a Run tab with its hosts chosen. Ctrl+Enter asks
    /// to run; each host's password is asked in the tab, once; then it runs
    /// on every one, the script given on its input, not on a command line.
    run_on_a_groups_hosts,
    |ui| {
        if cfg!(windows) {
            return;
        }
        let (server, home) = shell_server(ui);
        ui.config_file(
            "hosts.toml",
            &format!(
                "[[group]]\nname = \"Production\"\n\n[[group]]\nname = \"Staging\"\n\n{}{}{}",
                host("api-1", server.port, "Production"),
                host("api-2", server.port, "Production"),
                host("other", server.port, "Staging"),
            ),
        );
        ui.open();
        ui.hover("section:Production");
        ui.click("run-group:Production");
        assert_eq!(ui.run(|r, _| r.chosen_names()), ["api-1", "api-2"]);
        assert!(ui.find("run-empty").is_some(), "nothing ran yet");

        ui.type_text("echo \"it's $HOME\"; read x; echo \"[$x]\"");
        ui.keys("ctrl-enter");
        assert!(ui.find("run-confirm").is_some());
        answer_passwords(ui, 2);
        ui.wait_until("signed in", |ui| ui.run(|r, cx| r.ready_to_go(cx)));
        ui.click("run-confirm-go");
        ui.wait_until("the run's end", |ui| ui.run(|r, _| r.done()));

        let ended = ui.run(|r, _| r.outcomes());
        assert_eq!(ended.len(), 2);
        for h in ["api-1", "api-2"] {
            let r = outcome(&ended, h);
            assert_eq!(r.status, Some(0), "{r:?}");
            assert_eq!(r.stdout, format!("it's {}\n[]\n", home.display()));
        }
        assert!(ui.find("run-group-out:0").is_some(), "one group: they said the same");
        assert!(ui.find("run-group-out:1").is_none());
        // Drawn as a terminal: the output as it is, its last line ending left out.
        // A terminal for the one group.
        shown(ui, &[&format!("it's {}\n[]", home.display())]);
        // On the input, read whole: never on the command line.
        let seen = server.seen.lock().expect("seen");
        assert_eq!(seen.commands.len(), 2);
        for (command, input) in &seen.commands {
            assert!(!command.contains("echo"), "{command}");
            assert!(String::from_utf8_lossy(input).contains("read x"));
        }
    }
);

scenario!(
    /// A script that uses sudo asks for its password at the confirm step;
    /// sudo on the host gets it, and it is in no command line. A wrong one
    /// is said as sudo's refusal; the field is empty for the next run.
    sudo_gets_its_password,
    |ui| {
        if cfg!(windows) {
            return;
        }
        let (server, _) = shell_server(ui);
        ui.config_file("hosts.toml", &host("db", server.port, "Production"));
        ui.open();
        assert!(ui.find("action-run").is_none(), "Actions start folded");
        ui.click("actions-fold");
        ui.click("action-run");
        assert!(ui.run(|r, _| r.chosen_names().is_empty()));
        ui.click("run-pick:db");
        ui.click("run-script");
        ui.type_text("sudo echo as root");
        ui.click("run-go");
        let said = ui.run(|r, cx| r.said(cx));
        assert_eq!(said, (None, "sudo echo as root".to_string()));
        answer_passwords(ui, 1);
        ui.wait_until("signed in", |ui| {
            ui.find("run-sudo").is_some() && ui.run(|r, cx| !r.ready_to_go(cx))
        });
        ui.click("run-sudo");
        ui.type_text("pa ss");
        ui.wait_until("ready", |ui| ui.run(|r, cx| r.ready_to_go(cx)));
        ui.keys("enter");
        ui.wait_until("the run's end", |ui| ui.run(|r, _| r.done()));
        let ended = ui.run(|r, _| r.outcomes());
        assert_eq!(outcome(&ended, "db").stdout, "as root\n");
        for (command, _) in &server.seen.lock().expect("seen").commands {
            assert!(!command.contains("pa ss"), "{command}");
        }

        // Again, connected already: only sudo's password is asked.
        ui.click("run-again");
        ui.wait_until("asks for sudo", |ui| ui.find("run-sudo").is_some());
        assert_eq!(ui.run(|r, _| r.asked()), 0);
        ui.click("run-sudo");
        ui.type_text("wrong");
        ui.click("run-confirm-go");
        ui.wait_until("the run's end", |ui| ui.run(|r, _| r.done()));
        let ended = ui.run(|r, _| r.outcomes());
        let r = outcome(&ended, "db");
        assert!(r.sudo_refused() && r.status == Some(1), "{r:?}");
        assert!(ui.find("run-again-failed").is_some());
    }
);

scenario!(
    /// Ctrl+click picks hosts, which a bar under the list runs on; a plain
    /// click still goes to the host. The palette opens a Run tab too, and
    /// none is kept for the next start.
    ctrl_click_picks_hosts_to_run_on,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &format!(
                "{}{}{}",
                host("a", 1, "G"),
                host("b", 1, "G"),
                host("c", 1, "G")
            ),
        );
        ui.open();
        assert!(ui.find("picked-bar").is_none());
        let ctrl = if cfg!(target_os = "macos") {
            gpui::Modifiers::command()
        } else {
            gpui::Modifiers::control()
        };
        ui.click_with("host:a", ctrl);
        ui.click_with("host:c", ctrl);
        assert_eq!(ui.tabs(), 0, "a pick opens nothing");
        assert!(ui.find("picked-bar").is_some());
        ui.click_with("host:a", ctrl);
        ui.click_with("host:b", ctrl);
        ui.click("picked-run");
        assert_eq!(ui.run(|r, _| r.chosen_names()), ["b", "c"]);
        assert!(ui.find("picked-bar").is_none(), "the picks went to the tab");
        assert!(ui.ws(|ws, _, cx| ws.picked_hosts(cx).is_empty()));

        ui.palette("Run on hosts");
        assert_eq!(ui.tabs(), 2);
        assert!(ui.run(|r, _| r.chosen_names().is_empty()));
        let saved = ui.ws(|ws, _, cx| ws.saved_tab_count(cx));
        assert_eq!(saved, 0, "Run tabs are not opened again at the next start");
    }
);

scenario!(
    /// One host first: it runs alone; the others wait until *Run on the
    /// other N*, and then run too.
    one_host_first_then_the_others,
    |ui| {
        if cfg!(windows) {
            return;
        }
        let (server, _) = shell_server(ui);
        ui.config_file(
            "hosts.toml",
            &format!(
                "[[group]]\nname = \"Production\"\n\n{}{}",
                host("api-1", server.port, "Production"),
                host("api-2", server.port, "Production"),
            ),
        );
        ui.open();
        ui.hover("section:Production");
        ui.click("run-group:Production");
        ui.type_text("echo ok");
        ui.click("run-one-first");
        ui.keys("ctrl-enter");
        answer_passwords(ui, 2);
        ui.wait_until("signed in", |ui| ui.run(|r, cx| r.ready_to_go(cx)));
        ui.click("run-confirm-go");
        ui.wait_until("the first host's end", |ui| ui.run(|r, _| r.done()));
        let ended = ui.run(|r, _| r.outcomes());
        assert_eq!(ended.len(), 1, "{ended:?}");
        assert_eq!(server.seen.lock().expect("seen").commands.len(), 1);
        ui.click("run-go-on");
        ui.wait_until("the others' end", |ui| ui.run(|r, _| r.done()));
        let ended = ui.run(|r, _| r.outcomes());
        assert_eq!(ended.len(), 2);
        for h in ["api-1", "api-2"] {
            assert_eq!(outcome(&ended, h).stdout, "ok\n");
        }
        assert!(ui.find("run-go-on").is_none());
    }
);

scenario!(
    /// Actions and each section fold with a click on their title, and stay
    /// so at the next start; a filter shows the hosts it finds, folded or
    /// not.
    the_host_list_folds,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &format!(
                "[[group]]\nname = \"Production\"\n\n{}{}",
                host("api-1", 1, "Production"),
                host("api-2", 1, "Production"),
            ),
        );
        ui.open();
        assert!(ui.find("action-run").is_none());
        ui.click("actions-fold");
        assert!(ui.find("action-run").is_some());
        assert!(ui.find("host:api-1").is_some());
        ui.click("section:Production");
        assert!(ui.find("host:api-1").is_none(), "folded");
        assert!(ui.find("section:Production").is_some(), "its title stays");
        ui.click("host-filter");
        ui.type_text("api-2");
        assert!(ui.find("host:api-2").is_some(), "found, though folded");
        let saved = std::fs::read_to_string(ui.config_dir().join("state/sidebar.json"))
            .expect("sidebar.json");
        let saved: shellwing_config::SidebarState = serde_json::from_str(&saved).expect("json");
        assert!(saved.actions_open);
        assert_eq!(saved.folded, ["group:Production"]);
    }
);

scenario!(
    /// The ACTIONS chevron and the HOSTS heading's button stand in one
    /// column, folded or not.
    the_headings_icons_line_up,
    |ui| {
        ui.config_file("hosts.toml", &host("a", 1, "G"));
        ui.open();
        for _ in 0..2 {
            let chevron = ui.find("actions-chevron").expect("the chevron");
            let button = ui.find("collapse-hosts").expect("the button");
            let (a, b) = (chevron.center().x, button.center().x);
            assert!(
                (a - b).abs() < gpui::px(0.5),
                "the chevron at {a:?}, the button's icon at {b:?}"
            );
            ui.click("actions-fold");
        }
    }
);

scenario!(
    /// What a host prints shows while it runs: among the groups when it
    /// runs alone, in its row in the table; all of it once it ends.
    run_output_shows_as_it_is_printed,
    |ui| {
        if cfg!(windows) {
            return;
        }
        let (server, home) = shell_server(ui);
        ui.config_file(
            "hosts.toml",
            &format!("[[group]]\nname = \"G\"\n\n{}", host("api-1", server.port, "G")),
        );
        ui.open();
        ui.hover("section:G");
        ui.click("run-group:G");
        let go = home.join("go");
        ui.type_text(&format!(
            "echo first; while [ ! -e '{}' ]; do sleep 0.05; done; echo second",
            go.display()
        ));
        ui.keys("ctrl-enter");
        answer_passwords(ui, 1);
        ui.wait_until("signed in", |ui| ui.run(|r, cx| r.ready_to_go(cx)));
        ui.click("run-confirm-go");
        ui.wait_until("the first line, while it runs", |ui| {
            ui.run(|r, cx| r.shown_outputs(cx).iter().any(|s| s.trim_end() == "first"))
        });
        assert!(!ui.run(|r, _| r.done()), "it still runs");
        assert!(ui.find("run-going").is_some(), "with those still going");
        assert!(ui.find("run-output").is_some(), "drawn as a terminal");

        ui.click("run-by-host");
        ui.click("run-row:api-1");
        // The row's own.
        shown(ui, &["first"]);

        std::fs::write(&go, "").expect("go");
        ui.wait_until("the run's end", |ui| ui.run(|r, _| r.done()));
        shown(ui, &["first\nsecond"]);
    }
);

scenario!(
    /// A host's output is drawn as a terminal draws it: colors as colors,
    /// its errors after it, in red, as text; one host's row opens to its own.
    run_output_is_drawn_as_a_terminal,
    |ui| {
        if cfg!(windows) {
            return;
        }
        let (server, _home) = shell_server(ui);
        ui.config_file(
            "hosts.toml",
            &format!("[[group]]\nname = \"G\"\n\n{}", host("api-1", server.port, "G")),
        );
        ui.open();
        ui.hover("section:G");
        ui.click("run-group:G");
        ui.type_text("printf '\\033[32mgreen\\033[0m\\n'; echo oops >&2; exit 3");
        ui.keys("ctrl-enter");
        answer_passwords(ui, 1);
        ui.wait_until("signed in", |ui| ui.run(|r, cx| r.ready_to_go(cx)));
        ui.click("run-confirm-go");
        ui.wait_until("the run's end", |ui| ui.run(|r, _| r.done()));
        assert!(ui.find("run-output").is_some(), "drawn as a terminal");
        // No escapes.
        shown(ui, &["green\noops"]);

        ui.click("run-by-host");
        ui.click("run-row:api-1");
        // The row's own, the group's let go.
        shown(ui, &["green\noops"]);
    }
);

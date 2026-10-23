//! The sidebar, clicked as a user does: the host list and its filter, the
//! host and group editors it opens, and its way to Settings › About.

use super::{Policy, USER, scenario};
use crate::settings_view::Page;
use crate::ui_tests::server::closed_port;

/// The row the list always has, for this computer's shell.
const LOCAL: &str = "Local terminal";

/// A hosts.toml entry `name` for 127.0.0.1:`port`.
fn host(name: &str, port: u16) -> String {
    format!(
        "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {port}\nuser = \"{USER}\"\n"
    )
}

scenario!(
    /// "+ Host": the editor opens on its name; the name, the address, the port
    /// and the user typed, Enter saves the host to hosts.toml and lists it,
    /// and a click on it connects.
    add_a_host_and_connect_to_it,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.open();
        assert_eq!(ui.host_names(), [LOCAL]);
        ui.click("new-host");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.type_text("web");
        ui.keys("tab tab"); // past the alias, to the address
        ui.type_text("127.0.0.1");
        ui.keys("tab");
        ui.type_text(&server.port.to_string());
        ui.keys("tab");
        ui.type_text(USER);
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        let hosts = ui.hosts_toml();
        for line in [
            "name = \"web\"".to_string(),
            "address = \"127.0.0.1\"".to_string(),
            format!("port = {}", server.port),
            format!("user = \"{USER}\""),
        ] {
            assert!(hosts.contains(&line), "{line} in hosts.toml:\n{hosts}");
        }
        assert_eq!(ui.host_names(), [LOCAL, "web"]);
        ui.click("host:web");
        ui.login();
        assert_eq!(ui.tabs(), 1);
        assert_eq!(ui.view(|v, _| v.session().title.clone()), "web");
    }
);

scenario!(
    /// Ctrl+Enter (Cmd+Enter on macOS) saves the new host and connects to it
    /// at once.
    save_and_connect_at_once,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.open();
        ui.click("new-host");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.type_text("api");
        ui.keys("tab tab");
        ui.type_text("127.0.0.1");
        ui.keys("tab");
        ui.type_text(&server.port.to_string());
        ui.keys("tab");
        ui.type_text(USER);
        ui.keys("secondary-enter");
        ui.login();
        assert!(ui.hosts_toml().contains("name = \"api\""));
        assert_eq!(ui.view(|v, _| v.session().title.clone()), "api");
    }
);

scenario!(
    /// A host with no address is not saved: the editor stays open, hosts.toml
    /// is not written; Escape then closes the editor and nothing is listed.
    host_without_an_address_is_not_saved,
    |ui| {
        ui.open();
        ui.click("new-host");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.type_text("half-done");
        ui.keys("enter");
        ui.settle(std::time::Duration::from_millis(200));
        assert!(ui.host_editor_open(), "the editor stays open");
        assert_eq!(ui.hosts_toml(), "");
        ui.keys("escape");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        assert_eq!(ui.hosts_toml(), "");
        assert_eq!(ui.host_names(), [LOCAL]);
    }
);

scenario!(
    /// Edit, shown over a host under the mouse: the port is put right, the
    /// file keeps its comment and the other host, and the host connects.
    edit_a_host_from_the_list,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file(
            "hosts.toml",
            &format!(
                "# Written by hand.\n{}\n{}",
                host("web", closed_port()),
                host("db", server.port)
            ),
        );
        ui.open();
        ui.hover("host:web");
        ui.click("edit-host:web");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.keys("tab tab tab"); // name, alias, address, to the port
        ui.keys("secondary-a");
        ui.type_text(&server.port.to_string());
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        let hosts = ui.hosts_toml();
        assert!(hosts.starts_with("# Written by hand."), "{hosts}");
        assert!(!hosts.contains(&format!("port = {}", closed_port())), "{hosts}");
        assert_eq!(
            hosts.matches(&format!("port = {}", server.port)).count(),
            2,
            "both hosts on the server's port now:\n{hosts}"
        );
        ui.click("host:web");
        ui.login();
    }
);

scenario!(
    /// "+ Group": the group editor opens; a name and Enter add the group to
    /// hosts.toml, and the list shows it, empty.
    add_a_group,
    |ui| {
        ui.open();
        ui.click("new-group");
        ui.wait_until("the group editor", |ui| ui.group_editor_open());
        ui.type_text("Databases");
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.group_editor_open());
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("[[group]]"), "{hosts}");
        assert!(hosts.contains("name = \"Databases\""), "{hosts}");
        let list = ui.host_list();
        assert!(
            list.iter().any(|(title, hosts)| title == "Databases" && hosts.is_empty()),
            "{list:?}"
        );
    }
);

scenario!(
    /// The filter: a click on it and a few letters show only the hosts they
    /// match; Enter opens the first of them; Escape shows them all again.
    filter_the_host_list,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file(
            "hosts.toml",
            &[
                host("web-1", closed_port()),
                host("db-primary", server.port),
                host("db-replica", closed_port()),
            ]
            .join("\n"),
        );
        ui.open();
        assert_eq!(
            ui.host_names(),
            [LOCAL, "web-1", "db-primary", "db-replica"]
        );
        ui.click("host-filter");
        ui.type_text("db");
        assert_eq!(ui.host_names(), ["db-primary", "db-replica"]);
        ui.type_text(" rep");
        assert_eq!(ui.host_names(), ["db-replica"]);
        ui.keys("escape");
        assert_eq!(ui.host_names().len(), 4, "Escape empties the filter");
        ui.click("host-filter");
        ui.type_text("primary");
        ui.keys("enter");
        ui.login();
        assert_eq!(ui.view(|v, _| v.session().title.clone()), "db-primary");
        assert_eq!(ui.host_names().len(), 4, "opening empties the filter");
    }
);

scenario!(
    /// The info button after "+ Group" opens Settings on About.
    about_from_the_sidebar,
    |ui| {
        ui.open();
        assert_eq!(ui.settings_page(), None);
        ui.click("about");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        assert_eq!(ui.settings_page(), Some(Page::About));
    }
);

scenario!(
    /// About's buttons open their pages, and Copy version info puts the
    /// version on the clipboard, which a notice says.
    about_page_buttons,
    |ui| {
        ui.open();
        ui.click("about");
        ui.wait_until("About", |ui| ui.settings_page() == Some(Page::About));
        for (button, url) in [
            ("about-site", "https://shellwing.app"),
            ("about-github", "https://github.com/hivepaas/shellwing"),
            (
                "about-issue",
                "https://github.com/hivepaas/shellwing/issues/new",
            ),
            (
                "about-releases",
                "https://github.com/hivepaas/shellwing/releases",
            ),
            ("about-hivepaas", "https://hivepaas.com"),
            ("about-hivepaas-github", "https://github.com/hivepaas/hivepaas"),
        ] {
            ui.click(button);
            assert_eq!(ui.opened_url().as_deref(), Some(url), "{button}");
        }
        ui.click("about-copy-version");
        let copied = ui.clipboard().unwrap_or_default();
        assert!(
            copied.starts_with(&format!("shellwing {} (", env!("CARGO_PKG_VERSION"))),
            "{copied}"
        );
        ui.wait_for_notice("shellwing");
    }
);

scenario!(
    /// The host list folded to a rail of tiles: a click on a host's tile
    /// connects to it; the rail unfolds back to the list.
    rail_tile_connects,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("web", server.port));
        ui.open();
        ui.click("collapse-hosts");
        ui.wait_until("the rail", |ui| ui.find("hosts").is_none());
        ui.click("tile:web");
        ui.login();
        assert_eq!(ui.tabs(), 1);
        assert_eq!(ui.view(|v, _| v.session().title.clone()), "web");
        ui.click("expand-hosts");
        ui.wait_until("the list again", |ui| ui.find("hosts").is_some());
    }
);

/// Opens the editor on a saved host, as hovering it and clicking Edit does.
fn edit_host(ui: &mut crate::ui_tests::Ui, name: &str) {
    ui.hover(&format!("host:{name}"));
    ui.click(&format!("edit-host:{name}"));
    ui.wait_until("the host editor", |ui| ui.host_editor_open());
}

/// Saves the editor's host with its Save button.
fn save_host(ui: &mut crate::ui_tests::Ui) {
    ui.click("save");
    ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
}

scenario!(
    /// The editor's Group: a host put in a group is listed under it.
    host_put_in_a_group_in_the_editor,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &format!("[[group]]\nname = \"Databases\"\n\n{}", host("db", closed_port())),
        );
        ui.open();
        edit_host(ui, "db");
        ui.click("group-select");
        ui.click("choice:Databases");
        save_host(ui);
        assert!(ui.hosts_toml().contains("group = \"Databases\""), "{}", ui.hosts_toml());
        let list = ui.host_list();
        assert!(
            list.iter().any(|(t, h)| t == "Databases" && h == &["db"]),
            "{list:?}"
        );
    }
);

scenario!(
    /// A favorite (the editor's Favorite) is listed first, under Favorites.
    favorite_listed_first,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &[host("web", closed_port()), host("db", closed_port())].join("\n"),
        );
        ui.open();
        edit_host(ui, "db");
        ui.click("favorite");
        save_host(ui);
        assert!(ui.hosts_toml().contains("favorite = true"), "{}", ui.hosts_toml());
        let list = ui.host_list();
        assert_eq!(list.get(1), Some(&("Favorites".to_string(), vec!["db".to_string()])), "{list:?}");
    }
);

scenario!(
    /// A jump host added in the editor: the host is reached through it, each
    /// asking for its password.
    jump_host_set_in_the_editor,
    |ui| {
        let bastion = ui.server(Policy {
            forward: true,
            ..Policy::password()
        });
        let target = ui.server(Policy::password());
        ui.trust(&bastion);
        ui.trust(&target);
        ui.config_file(
            "hosts.toml",
            &[host("bastion", bastion.port), host("inner", target.port)].join("\n"),
        );
        ui.open();
        edit_host(ui, "inner");
        ui.click("add-jump");
        ui.type_text("bastion");
        ui.keys("enter");
        ui.keys("escape");
        save_host(ui);
        assert!(ui.hosts_toml().contains("jump = [\"bastion\"]"), "{}", ui.hosts_toml());
        ui.click("host:inner");
        ui.wait_for("password:");
        ui.line(crate::ui_tests::PASSWORD);
        ui.wait_for_count("password:", 2);
        ui.line(crate::ui_tests::PASSWORD);
        ui.wait_for(crate::ui_tests::WELCOME);
        assert_eq!(bastion.seen(|s| s.forwards.len()), 1);
        assert_eq!(target.seen(|s| s.auth.clone()), ["password:ok"]);
    }
);

scenario!(
    /// A host from ~/.ssh/config, copied to hosts.toml from the editor: it
    /// moves from the ~/.ssh/config section to the user's hosts.
    host_copied_from_the_ssh_config,
    |ui| {
        ui.ssh_config(&format!(
            "Host staging\n  HostName 127.0.0.1\n  Port {}\n  User {USER}\n",
            closed_port()
        ));
        ui.open();
        let list = ui.host_list();
        assert!(
            list.iter().any(|(t, h)| t == "~/.ssh/config" && h == &["staging"]),
            "{list:?}"
        );
        edit_host(ui, "staging");
        ui.click("ssh-copy");
        save_host(ui);
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("name = \"staging\""), "{hosts}");
        assert!(hosts.contains("address = \"127.0.0.1\""), "{hosts}");
        let list = ui.host_list();
        assert!(
            list.iter().any(|(t, h)| t == "Hosts" && h.contains(&"staging".to_string())),
            "{list:?}"
        );
        assert!(!list.iter().any(|(t, _)| t == "~/.ssh/config"), "{list:?}");
    }
);

/// hosts.toml: the group Databases with db1 and db2, and web on its own.
fn grouped(ui: &crate::ui_tests::Ui) {
    let in_group = |name: &str| format!("{}group = \"Databases\"\n", host(name, closed_port()));
    ui.config_file(
        "hosts.toml",
        &format!(
            "[[group]]\nname = \"Databases\"\n\n{}\n{}\n{}",
            in_group("db1"),
            in_group("db2"),
            host("web", closed_port())
        ),
    );
}

/// Opens the group editor, as hovering the group's header and clicking
/// Edit does.
fn edit_group(ui: &mut crate::ui_tests::Ui, name: &str) {
    ui.hover(&format!("section:{name}"));
    ui.click(&format!("edit-group:{name}"));
    ui.wait_until("the group editor", |ui| ui.group_editor_open());
}

scenario!(
    /// A group renamed in its editor takes its hosts along.
    rename_a_group_takes_its_hosts_along,
    |ui| {
        grouped(ui);
        ui.open();
        edit_group(ui, "Databases");
        ui.keys("secondary-a");
        ui.type_text("Data stores");
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.group_editor_open());
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("name = \"Data stores\""), "{hosts}");
        assert_eq!(hosts.matches("group = \"Data stores\"").count(), 2, "{hosts}");
        assert!(!hosts.contains("Databases"), "{hosts}");
        let list = ui.host_list();
        assert!(
            list.iter().any(|(t, h)| t == "Data stores" && h == &["db1", "db2"]),
            "{list:?}"
        );
    }
);

scenario!(
    /// A group deleted (after its editor asks) keeps its hosts, with no group.
    delete_a_group_keeps_its_hosts,
    |ui| {
        grouped(ui);
        ui.open();
        edit_group(ui, "Databases");
        ui.click("group-delete");
        ui.click("group-delete-yes");
        ui.wait_until("the editor to close", |ui| !ui.group_editor_open());
        let hosts = ui.hosts_toml();
        assert!(!hosts.contains("[[group]]") && !hosts.contains("group ="), "{hosts}");
        for name in ["db1", "db2", "web"] {
            assert!(hosts.contains(&format!("name = \"{name}\"")), "{name} is gone:\n{hosts}");
        }
        let list = ui.host_list();
        assert!(
            list.iter().any(|(t, h)| t == "Hosts" && h.len() == 3),
            "{list:?}"
        );
    }
);

scenario!(
    /// The editor's buttons stay within it: asking to delete a host with a
    /// long name, in a language whose labels are long.
    the_editors_buttons_stay_within_it,
    |ui| {
        ui.config_file("config.toml", "[general]\nlanguage = \"fr\"\n");
        let name = "a-rather-long-name-for-the-database-host.example.internal";
        ui.config_file("hosts.toml", &host(name, closed_port()));
        ui.open();
        edit_host(ui, name);
        let edge = ui.bounds("editor-body").right();
        assert!(ui.bounds("save-connect").right() <= edge, "Save and connect past the edge");
        ui.click("delete");
        assert!(ui.bounds("delete-no").right() <= edge, "Cancel past the edge");
        assert!(ui.find("save-connect").is_none(), "saving beside the question");
        ui.click("delete-no");
        assert!(ui.bounds("save-connect").right() <= edge);
    }
);

scenario!(
    /// Delete, in the host editor, asks, then takes the host out of
    /// hosts.toml; the others stay.
    delete_a_host,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &[host("web", closed_port()), host("db", closed_port())].join("\n"),
        );
        ui.open();
        edit_host(ui, "web");
        ui.click("delete");
        ui.click("delete-yes");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        let hosts = ui.hosts_toml();
        assert!(!hosts.contains("name = \"web\""), "{hosts}");
        assert!(hosts.contains("name = \"db\""), "{hosts}");
        assert_eq!(ui.host_names(), [LOCAL, "db"]);
    }
);

scenario!(
    /// Duplicate, in the host editor: a copy named "web copy", saved beside
    /// the original.
    duplicate_a_host,
    |ui| {
        ui.config_file("hosts.toml", &host("web", closed_port()));
        ui.open();
        edit_host(ui, "web");
        ui.click("duplicate");
        save_host(ui);
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("name = \"web copy\""), "{hosts}");
        assert!(hosts.contains("name = \"web\""), "{hosts}");
        let names = ui.host_names();
        assert!(names.contains(&"web copy".to_string()), "{names:?}");
    }
);

scenario!(
    /// The editor's options: kept on this device only, the agent forwarded,
    /// a keepalive; hosts.toml gets each.
    host_options_written,
    |ui| {
        ui.config_file("hosts.toml", &host("web", closed_port()));
        ui.open();
        edit_host(ui, "web");
        ui.click("local-only");
        ui.click("forward-agent");
        ui.click("save");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        edit_host(ui, "web");
        // From the name, Shift+Tab wraps to the last field: the keepalive.
        ui.keys("shift-tab");
        ui.type_text("30");
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        let hosts = ui.hosts_toml();
        for line in ["local_only = true", "forward_agent = true", "keepalive = 30"] {
            assert!(hosts.contains(line), "{line} in:\n{hosts}");
        }
    }
);

scenario!(
    /// The filter matches tags too, every word typed, in any case.
    filter_matches_tags,
    |ui| {
        let tagged = |name: &str, tags: &str| {
            format!("{}tags = [{tags}]\n", host(name, closed_port()))
        };
        ui.config_file(
            "hosts.toml",
            &[
                tagged("api", "\"payments\", \"eu\""),
                tagged("worker", "\"payments\", \"us\""),
                tagged("web", "\"frontend\", \"eu\""),
            ]
            .join("\n"),
        );
        ui.open();
        ui.click("host-filter");
        ui.type_text("payments");
        assert_eq!(ui.host_names(), ["api", "worker"]);
        ui.type_text(" EU");
        assert_eq!(ui.host_names(), ["api"]);
    }
);

scenario!(
    /// Edit host db… from its connected pane (palette): a theme chosen there
    /// is saved and colors the pane.
    edit_host_from_a_connected_pane,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("db", server.port));
        ui.connect("db");
        ui.login();
        let before = ui.view(|v, _| v.latest_frame().background);
        ui.palette("Edit host db");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.click("theme-select");
        ui.click("choice:Nord");
        save_host(ui);
        assert!(ui.hosts_toml().contains("theme = \"nord\""), "{}", ui.hosts_toml());
        ui.wait_until("the pane's new colors", |ui| {
            ui.view(|v, _| v.latest_frame().background) != before
        });
    }
);

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
    /// The drawer where a host is edited: its left edge drags it wider, up
    /// to 1000px, and a double click puts it back. The width is kept for
    /// the next drawer, a group's too, and with the window's layout.
    drag_the_editors_edge,
    |ui| {
        ui.open();
        ui.click("new-host");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        assert_eq!(ui.drawer_width().round(), 520.);
        ui.drag_by("drawer-edge", -200.);
        assert_eq!(ui.drawer_width().round(), 720.);
        ui.keys("escape");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        ui.click("new-group");
        ui.wait_until("the group editor", |ui| ui.group_editor_open());
        assert_eq!(ui.drawer_width().round(), 720.);
        let saved = std::fs::read_to_string(ui.config_dir().join("state/window.json"))
            .expect("the window's layout is saved");
        assert!(saved.contains("\"drawer_width\": 720.0"), "{saved}");
        ui.drag_by("drawer-edge", -600.);
        assert_eq!(ui.drawer_width().round(), 1000.);
        ui.double_click("drawer-edge");
        assert_eq!(ui.drawer_width().round(), 520.);
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

scenario!(
    /// A host's row is its tile beside its name and address, the tile with
    /// a little room above and below; folded to the rail, the tile keeps its
    /// size.
    rows_and_rail_tiles_match,
    |ui| {
        ui.config_file("hosts.toml", &host("web", closed_port()));
        ui.open();
        let row = ui.bounds("host:web");
        let tile = ui.bounds("host-tile:web");
        assert_eq!(tile.size.width, gpui::px(crate::sidebar::TILE));
        assert_eq!(tile.size.height, gpui::px(crate::sidebar::TILE));
        assert_eq!(row.size.height, gpui::px(crate::sidebar::TILE + 12.));
        ui.click("collapse-hosts");
        ui.wait_until("the rail", |ui| ui.find("hosts").is_none());
        assert_eq!(ui.bounds("tile:web").size, tile.size);
    }
);

scenario!(
    /// Connected, a host is asked which system it runs, and its tile shows
    /// that system's mark, kept for the next start; connected again the same
    /// day, it is not asked again. A host whose icon is chosen in hosts.toml
    /// shows that one and is never asked.
    a_hosts_tile_shows_its_system,
    |ui| {
        let server = ui.server(Policy {
            os_release: Some("PRETTY_NAME=\"Debian GNU/Linux 12\"\nID=debian".into()),
            ..Policy::password()
        });
        ui.trust(&server);
        ui.config_file(
            "hosts.toml",
            &format!(
                "{}{}icon = \"database\"\n",
                host("web", server.port),
                host("db", server.port)
            ),
        );
        ui.open();
        assert_eq!(ui.host_icon("web"), None, "not seen yet: initials");
        assert_eq!(ui.host_icon("db"), Some("database"));
        ui.click("host:web");
        ui.login();
        ui.wait_until("the system seen", |ui| ui.host_icon("web") == Some("debian"));
        let systems = std::fs::read_to_string(ui.config_dir().join("state/systems.json"))
            .expect("systems.json");
        assert!(systems.contains("\"web\"") && systems.contains("debian"), "{systems}");
        ui.click("host:db");
        ui.login();
        ui.click("host:web");
        ui.login();
        assert_eq!(ui.tabs(), 3);
        assert_eq!(server.seen.lock().expect("seen").systems_asked, 1);
    }
);

scenario!(
    /// A host that will not run the question keeps its initials, and is not
    /// asked again the same day.
    a_host_that_will_not_say_keeps_its_initials,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("web", server.port));
        ui.open();
        ui.click("host:web");
        ui.login();
        ui.wait_until("the answer kept", |ui| {
            std::fs::read_to_string(ui.config_dir().join("state/systems.json"))
                .is_ok_and(|s| s.contains("\"web\""))
        });
        assert_eq!(ui.host_icon("web"), None);
        assert_eq!(server.seen.lock().expect("seen").systems_asked, 1);
    }
);

scenario!(
    /// The editor's Icon: *Change* opens the app's icons, narrowed by what is
    /// typed; the one picked closes them, shows on the host's tile once
    /// saved, and is written to hosts.toml. *Automatic* takes it away again.
    pick_a_hosts_icon,
    |ui| {
        ui.config_file("hosts.toml", &host("web", closed_port()));
        ui.open();
        edit_host(ui, "web");
        assert!(ui.find("icon-menu").is_none(), "closed until asked");
        ui.click("icon-change");
        ui.type_text("data");
        assert!(ui.find("icon:database").is_some());
        assert!(ui.find("icon:ubuntu").is_none(), "narrowed by the search");
        ui.click("icon:database");
        assert!(ui.find("icon-menu").is_none());
        save_host(ui);
        let toml = ui.hosts_toml();
        assert!(toml.contains("icon = \"database\""), "{toml}");
        assert_eq!(ui.host_icon("web"), Some("database"));

        edit_host(ui, "web");
        ui.click("icon-change");
        ui.click("icon-auto");
        save_host(ui);
        let toml = ui.hosts_toml();
        assert!(!toml.contains("icon ="), "{toml}");
        assert_eq!(ui.host_icon("web"), None);
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

scenario!(
    /// A host ~/.ssh/config reaches through a ProxyCommand, copied to
    /// hosts.toml: the command comes along, beside the real address, and the
    /// copy goes through it.
    host_copied_with_its_proxy_command,
    |ui| {
        let Some(relay) = crate::ui_tests::server::relay_command() else {
            eprintln!("skipped: no nc to relay with");
            return;
        };
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.ssh_config(&format!(
            "Host tunnel\n  HostName 127.0.0.1\n  Port {}\n  User {USER}\n  ProxyCommand {relay}\n",
            server.port
        ));
        ui.open();
        edit_host(ui, "tunnel");
        ui.click("ssh-copy");
        save_host(ui);
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("address = \"127.0.0.1\""), "{hosts}");
        assert!(hosts.contains("proxy_command = "), "{hosts}");
        ui.click("host:tunnel");
        ui.wait_for("through `");
        ui.login();
    }
);

scenario!(
    /// X11 windows, chosen in the host editor: written to hosts.toml, and
    /// shown again when the host is opened; "As ~/.ssh/config" takes it out.
    x11_chosen_in_the_host_editor,
    |ui| {
        ui.config_file("hosts.toml", &host("web", closed_port()));
        ui.open();
        edit_host(ui, "web");
        ui.click("host-x11:3");
        save_host(ui);
        assert!(ui.hosts_toml().contains("x11 = \"trusted\""), "{}", ui.hosts_toml());
        edit_host(ui, "web");
        ui.click("host-x11:0");
        save_host(ui);
        assert!(!ui.hosts_toml().contains("x11"), "{}", ui.hosts_toml());
    }
);

scenario!(
    /// A command typed into the host editor (Route, under the jump hosts):
    /// saved to hosts.toml, and the host goes through it.
    proxy_command_typed_in_the_host_editor,
    |ui| {
        let Some(relay) = crate::ui_tests::server::relay_command() else {
            eprintln!("skipped: no nc to relay with");
            return;
        };
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("web", server.port));
        ui.open();
        edit_host(ui, "web");
        ui.keys("tab tab tab tab tab"); // alias, address, port, user, to the command
        ui.type_text(&relay);
        save_host(ui);
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("proxy_command = "), "{hosts}");
        ui.click("host:web");
        ui.wait_for("through `");
        ui.login();
        assert_eq!(server.seen(|s| s.connections), 1);
    }
);

scenario!(
    /// Jump hosts and a command are one or the other: the editor says so
    /// and stays open, and nothing is saved.
    jump_hosts_and_a_command_are_refused_together,
    |ui| {
        ui.config_file(
            "hosts.toml",
            &format!("{}jump = [\"bastion\"]\n", host("web", closed_port())),
        );
        ui.open();
        edit_host(ui, "web");
        ui.keys("tab tab tab tab tab");
        ui.type_text("nc %h %p");
        ui.click("save");
        ui.settle(std::time::Duration::from_millis(100));
        assert!(ui.host_editor_open(), "the editor stays open");
        assert!(!ui.hosts_toml().contains("proxy_command"));
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
    /// The group editor's buttons stay within it: asking to delete a group
    /// with a long name and hosts, in a language whose labels are long.
    the_group_editors_buttons_stay_within_it,
    |ui| {
        ui.config_file("config.toml", "[general]\nlanguage = \"fr\"\n");
        let group = "Production databases in the Frankfurt region";
        let in_group = |name: &str| format!("{}group = \"{group}\"\n", host(name, closed_port()));
        ui.config_file(
            "hosts.toml",
            &format!(
                "[[group]]\nname = \"{group}\"\n\n{}\n{}",
                in_group("db1"),
                in_group("db2")
            ),
        );
        ui.open();
        edit_group(ui, group);
        let edge = ui.bounds("group-body").right();
        assert!(ui.bounds("group-save").right() <= edge, "Save past the edge");
        ui.click("group-delete");
        assert!(ui.bounds("group-delete-no").right() <= edge, "Cancel past the edge");
        assert!(ui.find("group-save").is_none(), "saving beside the question");
        ui.click("group-delete-no");
        assert!(ui.bounds("group-save").right() <= edge);
    }
);

scenario!(
    /// A group takes its hosts' certificates from an organization chosen in
    /// its editor, once the server has listed some.
    a_group_takes_certificates_from_an_organization,
    |ui| {
        grouped(ui);
        ui.open();
        edit_group(ui, "Databases");
        assert!(ui.find("group-org").is_none(), "no server, no choice");
        ui.ws(|ws, _, cx| ws.know_orgs(vec![("o1".into(), "Platform team".into())], cx));
        ui.click("group-org");
        ui.click("group-org-choice:Platform team");
        ui.keys("enter");
        ui.wait_until("the editor to close", |ui| !ui.group_editor_open());
        let hosts = ui.hosts_toml();
        assert!(hosts.contains("org = \"o1\""), "{hosts}");
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

scenario!(
    /// The host list reads in two parts once an organization shares a
    /// collection: the person's own hosts under "My hosts", then the
    /// collections under "Shared · <organization>", each marked with what
    /// the person may do. With none shared, there are no parts.
    shared_collections_read_apart_from_my_hosts,
    |ui| {
        ui.config_file("hosts.toml", &host("web", 22));
        ui.open();
        assert!(ui.find("part:My hosts").is_none(), "no parts yet");
        let web = ui.ws(|ws, _, _| ws.saved_host("web")).expect("web");
        let shared_host = |id: &str, name: &str, collection: &str| {
            let mut h = web.clone();
            h.id = Some(id.into());
            h.name = name.into();
            h.collection = Some(collection.into());
            h
        };
        let collection = |id: &str, name: &str, org: &str, permission: &str, hosts| {
            crate::look::CollectionHosts {
                id: id.into(),
                name: name.into(),
                org_id: org.into(),
                permission: permission.into(),
                waiting: false,
                hosts,
            }
        };
        let production = collection(
            "c1",
            "Production",
            "org1",
            "manage",
            vec![shared_host("h1", "db", "c1")],
        );
        let staging = collection("c2", "Staging", "org1", "connect", vec![]);
        ui.ws(|ws, _, cx| {
            ws.show_collections(
                vec![("org1".into(), "Acme".into())],
                vec![production, staging],
                cx,
            )
        });
        let list = ui.host_list();
        let titles: Vec<&str> = list.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            titles,
            ["This computer", "Hosts", "Production", "Staging"],
            "{list:?}"
        );
        let my = ui.bounds("part:My hosts");
        let shared = ui.bounds("part:Shared · Acme");
        let hosts = ui.bounds("section:Hosts");
        let production = ui.bounds("section:Production");
        assert!(my.origin.y < hosts.origin.y, "My hosts heads the own hosts");
        assert!(hosts.origin.y < shared.origin.y && shared.origin.y < production.origin.y);
        assert!(ui.find("permission:Production").is_some());
        assert!(ui.find("permission:Staging").is_some());
        assert!(ui.find("permission:Hosts").is_none(), "only collections are marked");
    }
);

/// The test vault holds version 1 of the key of collection `id`, as the app
/// of somebody with access does once given it.
fn hold_collection_key(
    ui: &mut crate::ui_tests::Ui,
    id: &str,
) -> std::sync::Arc<shellwing_vault::Vault> {
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    let vault = ui.vault().expect("vault");
    let me = vault.ensure_identity().expect("identity").public();
    let wraps = vault
        .make_collection_key(id, 1, std::slice::from_ref(&me))
        .expect("key");
    let given = shellwing_vault::Given {
        version: 1,
        wrap: wraps[0].clone(),
        wrapper: me.ed25519,
    };
    vault.take_collection_keys(id, &[given]).expect("take");
    vault
}

fn production(
    permission: &str,
    hosts: Vec<shellwing_config::Host>,
) -> crate::look::CollectionHosts {
    crate::look::CollectionHosts {
        id: "c1".into(),
        name: "Production".into(),
        org_id: "org1".into(),
        permission: permission.into(),
        waiting: false,
        hosts,
    }
}

scenario!(
    /// A host of hosts.toml moved into a collection from its editor's
    /// "Saved in": the editor says it moves; saved, it is in the collection
    /// and no longer in hosts.toml. Its saved password stays the person's
    /// own: none goes into the collection, which shares no passwords.
    move_a_host_into_a_collection,
    |ui| {
        let port = closed_port();
        ui.config_file("hosts.toml", &host("web", port));
        ui.open();
        let vault = hold_collection_key(ui, "c1");
        let target = format!("{USER}@127.0.0.1:{port}");
        vault.set_password(&target, "pw").expect("password");
        ui.ws(|ws, _, cx| {
            ws.show_collections(
                vec![("org1".into(), "Acme".into())],
                vec![production("edit", vec![])],
                cx,
            )
        });
        edit_host(ui, "web");
        assert!(ui.find("move-note").is_none(), "where it is: no move");
        ui.click("place-select");
        ui.click("choice:Production");
        assert!(ui.find("move-note").is_some(), "saving moves it");
        assert!(ui.find("group-select").is_none(), "no group in a collection");
        assert!(ui.find("favorite").is_none());
        assert!(ui.find("local-only").is_none());
        save_host(ui);

        assert!(!ui.hosts_toml().contains("name = \"web\""), "{}", ui.hosts_toml());
        let col = shellwing_vault::Space::Collection("c1".into());
        let hosts = vault.entries_in(&col, "host").expect("entries");
        assert_eq!(hosts.len(), 1, "in the collection");
        assert!(
            vault.entries_in(&col, "password").expect("entries").is_empty(),
            "no password shared"
        );
        assert!(vault.password(&target).expect("read").is_some(), "still mine");
    }
);

scenario!(
    /// A collection's title has a menu, by a right click or its button on
    /// hover, with what the person may do to it: a new host in it, and what
    /// the palette offers for it. Its New host saves in the collection.
    /// Connect only, there is nothing to do, and no menu.
    a_collections_title_has_a_menu,
    |ui| {
        ui.open();
        let vault = hold_collection_key(ui, "c1");
        ui.ws(|ws, _, cx| {
            ws.show_collections(
                vec![("org1".into(), "Acme".into())],
                vec![production("manage", vec![])],
                cx,
            )
        });
        let items = [
            "collection-new-host",
            "collection-access",
            "collection-give-key",
        ];
        ui.right_click("section:Production");
        for id in items {
            assert!(ui.find(id).is_some(), "{id}");
        }
        assert!(ui.find("collection-checklist").is_none(), "no key replaced");
        ui.keys("escape");
        assert!(ui.find("collection-menu").is_none(), "Esc closes it");

        // Manage access goes where the palette's does: with no server here,
        // it says collections need one.
        ui.hover("section:Production");
        ui.click("collection-more:Production");
        ui.click("collection-access");
        assert!(ui.find("collection-menu").is_none());
        ui.wait_for_notice("Collections need syncing with a shellwing server.");

        ui.right_click("section:Production");
        ui.click("collection-new-host");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        assert!(ui.find("group-select").is_none(), "in the collection: no group");
        ui.type_text("api");
        ui.keys("tab tab");
        ui.type_text("127.0.0.1");
        ui.keys("tab");
        ui.type_text(&closed_port().to_string());
        ui.keys("tab");
        ui.type_text(USER);
        save_host(ui);
        assert!(!ui.hosts_toml().contains("name = \"api\""), "{}", ui.hosts_toml());
        let col = shellwing_vault::Space::Collection("c1".into());
        assert_eq!(vault.entries_in(&col, "host").expect("entries").len(), 1);

        ui.ws(|ws, _, cx| ws.show_collections(vec![], vec![production("edit", vec![])], cx));
        ui.right_click("section:Production");
        assert!(ui.find("collection-new-host").is_some());
        assert!(ui.find("collection-access").is_none(), "Edit: no access to manage");
        ui.keys("escape");

        ui.ws(|ws, _, cx| ws.show_collections(vec![], vec![production("connect", vec![])], cx));
        ui.right_click("section:Production");
        assert!(ui.find("collection-menu").is_none(), "Connect: no menu");
        ui.hover("section:Production");
        assert!(ui.find("collection-more:Production").is_none());
    }
);

scenario!(
    /// A collection's host in the editor: no group, favorite or this-device
    /// toggle, and a key from the person's own vault is flagged, since the
    /// others with access do not have it. Connect only: it cannot be moved.
    a_collections_host_in_the_editor,
    |ui| {
        ui.config_file("hosts.toml", &host("web", 22));
        ui.open();
        let mut db = ui.ws(|ws, _, _| ws.saved_host("web")).expect("web");
        db.id = Some("h1".into());
        db.name = "db".into();
        db.collection = Some("c1".into());
        db.auth.method = shellwing_config::AuthMethod::Key;
        db.auth.key = Some(format!("{}k1", crate::target::VAULT_KEY));
        ui.ws(|ws, _, cx| ws.show_collections(vec![], vec![production("edit", vec![db.clone()])], cx));
        edit_host(ui, "db");
        assert!(ui.find("group-select").is_none());
        assert!(ui.find("favorite").is_none());
        assert!(ui.find("local-only").is_none());
        assert!(ui.find("vault-key-shared").is_some(), "the others have no such key");
        assert!(ui.find("place-select").is_some(), "Edit: it may be moved");
        ui.keys("escape");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());

        ui.ws(|ws, _, cx| ws.show_collections(vec![], vec![production("connect", vec![db])], cx));
        edit_host(ui, "db");
        assert!(ui.find("place-select").is_none(), "Connect: it stays where it is");
    }
);

scenario!(
    /// A collection's host that asks for a command typed once connected:
    /// others wrote it, so it connects without it and says so. Allowed, it
    /// connects again with it, and is not asked again.
    a_collections_host_connects_without_what_it_asks_until_allowed,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("web", server.port));
        ui.open();
        let mut db = ui.ws(|ws, _, _| ws.saved_host("web")).expect("web");
        db.id = Some("h1".into());
        db.name = "db".into();
        db.collection = Some("c1".into());
        db.run = Some("echo ASKED-RUN".into());
        ui.ws(|ws, _, cx| ws.show_collections(vec![], vec![production("edit", vec![db])], cx));
        ui.click("host:db");
        ui.login();
        ui.settle(std::time::Duration::from_millis(300));
        assert!(ui.view(|v, _| v.holds_back()), "the bar says what was held back");
        assert!(!ui.screen().contains("ASKED-RUN"), "not run:\n{}", ui.screen());

        ui.click("held-allow");
        // The same pane connects again: its second prompt.
        ui.wait_for_count("password:", 2);
        ui.line(crate::ui_tests::server::PASSWORD);
        ui.wait_for("ASKED-RUN");
        assert!(!ui.view(|v, _| v.holds_back()));
        let allowed = shellwing_config::Store::new(ui.config_dir()).allowed();
        assert!(allowed.contains_key("h1"), "{allowed:?}");

        // Opened again: allowed already.
        ui.click("host:db");
        ui.login();
        assert!(!ui.view(|v, _| v.holds_back()), "not asked again");
    }
);

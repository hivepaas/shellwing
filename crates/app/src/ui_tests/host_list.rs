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
            ("about-github", "https://github.com/hivepaas/shellwing"),
            (
                "about-issue",
                "https://github.com/hivepaas/shellwing/issues/new",
            ),
            (
                "about-releases",
                "https://github.com/hivepaas/shellwing/releases",
            ),
            (
                "about-license",
                "https://github.com/hivepaas/shellwing/blob/main/LICENSE",
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

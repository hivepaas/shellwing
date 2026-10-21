//! Snippets and environments, kept in the vault and used from the palette:
//! a command run (or only typed) in the pane, its holes asked for, run in
//! every pane; variables loaded into the shell without showing their values,
//! unloaded, and given to one snippet alone.

use std::time::Duration;

use shellwing_vault::Snippet;

use super::{Policy, Server, USER, Ui, scenario};

/// A secret value with what a terminal must not take as typed text: spaces,
/// a backslash, a newline and Vietnamese (bytes past ASCII).
const SECRET: &str = "p@ss w0rd\\x\nTiếng Việt";

/// Once the vault is open, keeps a snippet in it.
fn snippet(ui: &mut Ui, name: &str, command: &str, enter: bool, needs: Vec<String>) {
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    let vault = ui.vault().expect("vault");
    vault
        .save_snippet(&Snippet {
            id: String::new(),
            name: name.into(),
            command: command.into(),
            scope: String::new(),
            enter,
            category: String::new(),
            needs,
            origin: String::new(),
        })
        .expect("snippet saved");
}

/// Once the vault is open, keeps the environment `name` with `DB_PASSWORD`
/// = [`SECRET`] and `DB_USER` = `admin`; its id.
fn environment(ui: &mut Ui, name: &str) -> String {
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    let vault = ui.vault().expect("vault");
    vault
        .save_environment(
            "",
            name,
            "",
            &[
                ("DB_PASSWORD".into(), Some(SECRET.into())),
                ("DB_USER".into(), Some("admin".into())),
            ],
        )
        .expect("environment saved")
}

/// The host `web` in hosts.toml, connected and signed in to.
fn signed_in(ui: &mut Ui) -> Server {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.config_file(
        "hosts.toml",
        &format!(
            "[[host]]\nname = \"web\"\naddress = \"127.0.0.1\"\nport = {}\nuser = \"{USER}\"\n",
            server.port
        ),
    );
    ui.connect("web");
    ui.login();
    server
}

/// How many times the shell ran `line`.
fn ran(server: &Server, line: &str) -> usize {
    server.seen(|s| s.lines.iter().filter(|l| l.as_str() == line).count())
}

scenario!(
    /// A snippet chosen in the palette is typed into the pane and run.
    snippet_runs_from_the_palette,
    |ui| {
        let server = signed_in(ui);
        snippet(ui, "Disk usage", "df -h /var", true, vec![]);
        ui.palette("Disk usage");
        ui.wait_for("you said: df -h /var");
        assert_eq!(ran(&server, "df -h /var"), 1);
    }
);

scenario!(
    /// A snippet set not to run is only typed: the user reads it, then
    /// presses Enter.
    snippet_only_typed_waits_for_enter,
    |ui| {
        let server = signed_in(ui);
        snippet(ui, "Drop the cache", "rm -rf /tmp/cache", false, vec![]);
        ui.palette("Drop the cache");
        ui.wait_for("$ rm -rf /tmp/cache");
        ui.settle(Duration::from_millis(200));
        assert_eq!(ran(&server, "rm -rf /tmp/cache"), 0, "it ran by itself");
        ui.keys("enter");
        ui.wait_for("you said: rm -rf /tmp/cache");
    }
);

scenario!(
    /// A snippet's holes: `{{host}}` and `{{user}}` are filled in from the
    /// pane; another is asked for in the palette, then the command runs.
    snippet_asks_for_its_holes,
    |ui| {
        signed_in(ui);
        snippet(
            ui,
            "Restart a service",
            "systemctl restart {{service}} # on {{host}} as {{user}}",
            true,
            vec![],
        );
        ui.palette("Restart a service");
        ui.wait_until("the question", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line("nginx");
        ui.wait_for(&format!(
            "you said: systemctl restart nginx # on web as {USER}"
        ));
    }
);

scenario!(
    /// Alt+Enter on a snippet runs it in every pane of the tab.
    snippet_runs_in_every_pane,
    |ui| {
        let server = signed_in(ui);
        snippet(ui, "Uptime", "uptime -p", true, vec![]);
        ui.shortcut("d");
        ui.wait_until("a second pane", |ui| ui.panes() == 2);
        ui.login();
        ui.shortcut("k");
        ui.type_text("Uptime");
        ui.keys("alt-enter");
        ui.wait_until("both panes to run it", |_| ran(&server, "uptime -p") == 2);
    }
);

scenario!(
    /// An environment loaded from the palette: its variables are in the
    /// shell, and their values never show on the screen or as a command line
    /// the shell keeps; unloading takes them out again.
    environment_loads_without_showing_its_values,
    |ui| {
        let server = signed_in(ui);
        environment(ui, "Staging database");
        ui.palette("Staging database");
        ui.wait_for_notice("Staging database is loaded into this shell.");
        let screen = ui.screen();
        assert!(!screen.contains("p@ss"), "the value showed:\n{screen}");
        let lines = server.seen(|s| s.lines.clone());
        assert!(
            !lines.iter().any(|l| l.contains("p@ss")),
            "the value was a command line: {lines:?}"
        );
        ui.line("printenv DB_USER");
        ui.wait_for("admin");
        ui.line("printenv DB_PASSWORD");
        ui.wait_for("p@ss w0rd\\x");
        ui.wait_for("Tiếng Việt");
        // Unloaded from the palette: gone from the shell.
        ui.palette("Unload environment");
        ui.wait_until("the unload", |_| {
            ran(&server, " unset DB_PASSWORD DB_USER") == 1
                || ran(&server, " unset DB_USER DB_PASSWORD") == 1
        });
        ui.line("printenv DB_USER");
        ui.settle(Duration::from_millis(200));
        let after = ui.screen();
        let tail = after.rsplit("printenv DB_USER").next().unwrap_or_default();
        assert!(!tail.contains("admin"), "still set:\n{after}");
    }
);

scenario!(
    /// A snippet that needs an environment runs in a subshell that has its
    /// variables; the pane's own shell does not get them.
    snippet_with_an_environment,
    |ui| {
        let server = signed_in(ui);
        let env = environment(ui, "Staging database");
        snippet(ui, "Who am I on the database", "printenv DB_USER", true, vec![env]);
        ui.palette("Who am I on the database");
        ui.wait_for("admin");
        let screen = ui.screen();
        assert!(!screen.contains("p@ss"), "the value showed:\n{screen}");
        ui.line("printenv DB_USER");
        ui.settle(Duration::from_millis(200));
        let after = ui.screen();
        let tail = after.rsplit("printenv DB_USER").next().unwrap_or_default();
        assert!(!tail.contains("admin"), "the pane's shell has it:\n{after}");
        assert_eq!(ran(&server, "printenv DB_USER"), 2);
    }
);

scenario!(
    /// The snippet library: a command ticked and added becomes the user's
    /// snippet, filed under its tool, and runs from the palette.
    snippet_added_from_the_library,
    |ui| {
        let _server = signed_in(ui);
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Snippet library");
        ui.wait_until("the library", |ui| ui.library_window().is_some());
        let lib = ui.library_window().expect("library").into();
        ui.click_in(lib, "library-check:system/memory");
        ui.click_in(lib, "library-add");
        ui.wait_until("the snippet in the vault", |ui| {
            ui.vault()
                .and_then(|v| v.snippets().ok())
                .is_some_and(|s| s.iter().any(|s| s.name == "Memory"))
        });
        let added = ui
            .vault()
            .and_then(|v| v.snippets().ok())
            .and_then(|s| s.into_iter().find(|s| s.name == "Memory"))
            .expect("added");
        assert_eq!(added.origin, "library:system/memory");
        assert_eq!(added.category, "System");
        ui.palette("Memory");
        ui.wait_for("you said: free -h");
    }
);

scenario!(
    /// An environment made in Settings › Environments from a pasted .env
    /// file, then loaded into the shell from the palette.
    environment_made_in_settings_loads,
    |ui| {
        let _server = signed_in(ui);
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Environments");
        ui.click("env-new");
        ui.type_text("Staging");
        ui.copy_to_clipboard("DB_USER=admin\nDB_PASSWORD=s3cret\n");
        ui.click("env-paste");
        ui.keys("enter");
        ui.wait_until("the environment in the vault", |ui| {
            ui.vault()
                .and_then(|v| v.environments().ok())
                .is_some_and(|e| {
                    e.iter().any(|e| {
                        let mut names = e.names.clone();
                        names.sort();
                        e.name == "Staging" && names == ["DB_PASSWORD", "DB_USER"]
                    })
                })
        });
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette("Staging");
        ui.wait_for_notice("Staging is loaded into this shell.");
        ui.line("printenv DB_USER");
        ui.wait_for("admin");
    }
);

scenario!(
    /// The palette's badges are all there with nothing in a section: a
    /// click on an empty one scrolls to its heading, with nothing under it.
    palette_badges_with_empty_sections,
    |ui| {
        ui.open();
        ui.shortcut("k");
        ui.wait_until("the palette", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        for part in [
            "Hosts",
            "Recent",
            "Snippets",
            "Environments",
            "ConfigFiles",
            "Notes",
            "Actions",
            "Settings",
        ] {
            assert!(ui.find(&format!("palette-section:{part}")).is_some(), "{part}");
        }
        // Nothing can be made under Recent: it stays empty.
        let before = ui.bounds("palette-empty:Recent").top();
        ui.click("palette-section:Recent");
        let list = ui.bounds("palette-list");
        ui.wait_until("the empty heading scrolled into view", |ui| {
            let b = ui.bounds("palette-empty:Recent");
            b.top() < before && b.top() >= list.top() && b.bottom() <= list.bottom()
        });
    }
);

scenario!(
    /// With no environment or config file yet, their sections in the
    /// palette offer a new one, which opens its form in Settings.
    palette_offers_the_first_environment_and_config_file,
    |ui| {
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("New environment");
        ui.wait_until("Settings › Environments", |ui| {
            ui.settings_page() == Some(crate::settings_view::Page::Environments)
        });
        assert!(ui.find("env-paste").is_some(), "no new environment's form");
        ui.keys("escape");
        ui.wait_until("the form to close", |ui| ui.find("env-paste").is_none());
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.palette("New config file");
        ui.wait_until("Settings › Config files", |ui| {
            ui.settings_page() == Some(crate::settings_view::Page::ConfigFiles)
        });
        assert!(ui.find("file-choose").is_some(), "no new config file's form");
    }
);

scenario!(
    /// The snippet window (Settings › Snippets › New snippet…): a name and a
    /// command typed and saved (Ctrl/Cmd+S) are in the vault; Run in the pane
    /// runs it there.
    snippet_written_in_its_window,
    |ui| {
        let _server = signed_in(ui);
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Snippets");
        ui.click("snippet-new");
        ui.wait_until("the snippet window", |ui| ui.snippet_window().is_some());
        let w = ui.snippet_window().expect("window").into();
        // The name has the keys first.
        ui.type_in(w, "Disk usage");
        ui.click_in(w, "snippet-command");
        ui.type_in(w, "df -h /var");
        ui.keys_in(w, "secondary-s");
        ui.wait_until("the snippet in the vault", |ui| {
            ui.vault()
                .and_then(|v| v.snippets().ok())
                .is_some_and(|s| s.iter().any(|s| s.name == "Disk usage" && s.command == "df -h /var"))
        });
        ui.click("settings-close");
        ui.wait_until("Settings to close", |ui| ui.settings_page().is_none());
        ui.click_in(w, "snippet-run");
        ui.wait_for("you said: df -h /var");
    }
);

scenario!(
    /// The library's card in Settings › Snippets, in a narrow window: its
    /// text wraps inside the card instead of running past its edge.
    the_library_card_wraps_in_a_narrow_window,
    |ui| {
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.resize(760., 640.);
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:Snippets");
        let card = ui.bounds("snippet-library-card");
        let text = ui.bounds("snippet-library-text");
        assert!(card.right() <= gpui::px(760.), "the card fits the window: {card:?}");
        assert!(text.right() <= card.right(), "the text stays in the card: {text:?} in {card:?}");
        assert!(text.size.height > gpui::px(40.), "it wraps onto more lines: {text:?}");
    }
);

scenario!(
    /// The library's File under list opens right above its button (it is at
    /// the window's foot), not somewhere off it.
    the_file_under_list_opens_at_its_button,
    |ui| {
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        ui.palette("Snippet library");
        ui.wait_until("the library", |ui| ui.library_window().is_some());
        let lib = ui.library_window().expect("library").into();
        ui.click_in(lib, "library-check:system/memory");
        ui.click_in(lib, "library-file");
        let button = ui.find_in(lib, "library-file").expect("the button");
        let list = ui.find_in(lib, "library-file-list").expect("the list");
        let gap = button.top() - list.bottom();
        assert!(gap >= gpui::px(0.) && gap <= gpui::px(6.), "above it: {list:?} over {button:?}");
        assert!((list.left() - button.left()).abs() <= gpui::px(1.), "from its left edge");
    }
);

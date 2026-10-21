//! Settings a user changes, and what follows: the session log, saving the
//! output, a config.toml with a mistake, and the language.

use std::path::{Path, PathBuf};

use shellwing_i18n::tr;

use super::{PASSWORD, Policy, Server, Ui, scenario};

/// Connected to a server and signed in.
fn signed_in(ui: &mut Ui) -> Server {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    server
}

/// The text of the `.log` files in `dir`, together.
fn logs(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A TOML literal string for `path` (backslashes and all, on Windows).
fn literal(path: &Path) -> String {
    format!("'{}'", path.display())
}

scenario!(
    /// `[log] always = true`: the session is written to a file in the folder
    /// set, as text; the password typed at the prompt is not in it.
    session_logged_to_a_file,
    |ui| {
        let folder = ui.dir.join("session-logs");
        ui.config_file(
            "config.toml",
            &format!(
                "[log]\nalways = true\nfolder = {}\ntimestamps = false\n",
                literal(&folder)
            ),
        );
        let _server = signed_in(ui);
        ui.line("deploy the release");
        ui.wait_for("you said: deploy the release");
        ui.wait_until("the log to have it", |_| {
            logs(&folder).contains("you said: deploy the release")
        });
        let log = logs(&folder);
        assert!(!log.contains(PASSWORD), "the password is in the log:\n{log}");
        assert!(!log.contains('\x1b'), "escape sequences in a text log:\n{log}");
    }
);

scenario!(
    /// Save the output as text, from the palette: the file chosen in the save
    /// dialog has what the session showed.
    save_the_output_as_text,
    |ui| {
        let _server = signed_in(ui);
        ui.line("uptime report");
        ui.wait_for("you said: uptime report");
        ui.palette("Save the output");
        ui.wait_until("the save dialog", |ui| ui.cx.did_prompt_for_new_path());
        let path: PathBuf = ui.dir.join("output.txt");
        let chosen = path.clone();
        ui.cx.simulate_new_path_selection(move |_| Some(chosen));
        ui.wait_for_notice("Saved as text");
        let text = std::fs::read_to_string(&path).expect("saved file");
        assert!(text.contains("you said: uptime report"), "{text}");
    }
);

scenario!(
    /// A config.toml with a mistake at start: the app opens anyway, says
    /// where the mistake is, and connects.
    broken_config_at_start,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("config.toml", "[general\nlanguage = \n");
        ui.open();
        ui.wait_for_notice("config.toml line");
        ui.palette(&server.spec());
        ui.login();
    }
);

scenario!(
    /// config.toml edited into a mistake while the app runs: it says so, and
    /// keeps the settings it had (here, a custom key for the palette).
    config_broken_while_running_keeps_its_settings,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.config_file("config.toml", "[shortcuts]\npalette = [\"ctrl+alt+j\"]\n");
        ui.open();
        // A later write: the reload tells files apart by time and size.
        std::thread::sleep(std::time::Duration::from_millis(20));
        ui.config_file("config.toml", "[shortcuts]\npalette = [\"ctrl+alt+j\"\n");
        ui.reload_config();
        ui.wait_for_notice("config.toml line");
        ui.keys("ctrl-alt-j");
        ui.wait_until("the palette", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(&server.spec());
        ui.login();
    }
);

scenario!(
    /// Settings › General › Language: choosing Tiếng Việt writes it to
    /// config.toml and the app speaks it at once; English brings it back.
    language_chosen_in_settings,
    |ui| {
        ui.config_file("config.toml", "[general]\nlanguage = \"en\"\n");
        ui.open();
        assert!(ui.host_names().iter().any(|h| h == "Local terminal"));
        // The pills: the system's first, then the languages in their order.
        let pill = |tag: &str| {
            let n = shellwing_i18n::LANGUAGES
                .iter()
                .position(|(t, _)| *t == tag)
                .expect("language");
            format!("language:{}", n + 1)
        };
        ui.palette("Settings");
        ui.wait_until("Settings", |ui| ui.settings_page().is_some());
        ui.click("settings-page:General");
        ui.click(&pill("vi"));
        ui.wait_until("config.toml", |ui| {
            std::fs::read_to_string(ui.config_dir().join("config.toml"))
                .is_ok_and(|t| t.contains("language = \"vi\""))
        });
        assert_eq!(shellwing_i18n::language(), "vi");
        let local = tr!("Local terminal");
        assert_ne!(local, "Local terminal", "no Vietnamese for it");
        ui.wait_until("the host list in Vietnamese", |ui| {
            ui.host_names().contains(&local)
        });
        ui.click(&pill("en"));
        ui.wait_until("the host list in English", |ui| {
            ui.host_names().iter().any(|h| h == "Local terminal")
        });
    }
);

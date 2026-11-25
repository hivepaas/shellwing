//! AI access: a client let in from Settings › AI, a host given to it, and
//! the client reaching it over MCP through the app's socket.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use serde_json::{Value, json};
use shellwing_core::ai::socket::Endpoint;

use super::{Policy, USER, Ui, scenario};
use crate::settings_view::Page;

fn host(name: &str, port: u16) -> String {
    format!(
        "[[host]]\nname = \"{name}\"\naddress = \"127.0.0.1\"\nport = {port}\nuser = \"{USER}\"\n"
    )
}

/// An MCP client on the app's socket, a line each way.
struct Client {
    to: UnixStream,
    from: BufReader<UnixStream>,
    next: u64,
}

impl Client {
    /// Says who it is with `token`; the app's answer to that.
    fn open(ui: &Ui, token: &str) -> (Client, Value) {
        let at = Endpoint::for_config(&ui.config_dir());
        let to = UnixStream::connect(&at.0).expect("the app's socket");
        to.set_read_timeout(Some(Duration::from_secs(20)))
            .expect("timeout");
        let from = BufReader::new(to.try_clone().expect("clone"));
        let mut c = Client { to, from, next: 1 };
        c.send(&json!({ "token": token }));
        let hello = c.read();
        (c, hello)
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.to, "{v}").expect("write");
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.from.read_line(&mut line).expect("read");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }

    /// Calls `method` and waits for its answer, past any notification.
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let v = self.read();
            if v["id"] == id {
                return v;
            }
        }
    }

    /// [`Client::tool`], the app's window going on meanwhile: what a tab
    /// does needs it, where the rest is the network's.
    fn tool_in(&mut self, ui: &mut Ui, name: &str, args: Value) -> (String, bool) {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                          "params": {"name": name, "arguments": args}}));
        self.to
            .set_read_timeout(Some(Duration::from_millis(15)))
            .expect("timeout");
        let mut line = String::new();
        let start = std::time::Instant::now();
        let v = loop {
            ui.settle(Duration::from_millis(15));
            match self.from.read_line(&mut line) {
                Ok(_) if line.ends_with('\n') => {
                    let v: Value = serde_json::from_str(&line).expect("json");
                    line.clear();
                    if v["id"] == id {
                        break v;
                    }
                }
                _ => {}
            }
            assert!(
                start.elapsed() < Duration::from_secs(40),
                "no answer to {name}"
            );
        };
        self.to
            .set_read_timeout(Some(Duration::from_secs(20)))
            .expect("timeout");
        let text = v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{v}"))
            .to_string();
        (text, v["result"]["isError"] == true)
    }

    /// A tool's text, and whether it is an error.
    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let v = self.call("tools/call", json!({"name": name, "arguments": args}));
        let text = v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{v}"))
            .to_string();
        (text, v["result"]["isError"] == true)
    }
}

scenario!(
    /// Settings › AI lets a client in, with a token shown once (only its
    /// hash kept), and gives it one host, whose tile says so. Over the
    /// socket, the client sees that host alone, runs a command there,
    /// follows its output as a job, and is refused the host not given,
    /// and the one taken back.
    an_ai_client_runs_on_the_host_given_it,
    |ui| {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
        ui.agent(&[&key]);
        let home = ui.config_dir().join("remote-home");
        std::fs::create_dir_all(&home).expect("home");
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            shell: Some((home.clone(), home.clone())),
            ..Default::default()
        });
        ui.trust(&server);
        ui.config_file(
            "hosts.toml",
            &format!("{}{}", host("web", server.port), host("db", server.port)),
        );
        ui.open();
        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-add-client");
        ui.click("ai-kind:claude-code");
        ui.click("ai-copy-config");
        let config = ui.clipboard().expect("the config");
        let token = config
            .split("SHELLWING_TOKEN=")
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .expect("a token")
            .to_string();
        assert!(token.starts_with("shw_"), "{config}");
        let saved = std::fs::read_to_string(ui.config_dir().join("state/ai.json")).expect("ai.json");
        assert!(!saved.contains(&token), "the token itself is not kept");
        assert!(saved.contains(&shellwing_core::ai::token_hash(&token)));

        ui.click("ai-give");
        ui.click("ai-pick:web");
        ui.click("ai-give-to:Claude Code");
        ui.click("ai-give-ok");
        assert!(ui.find("ai-grant:web").is_some());
        assert!(ui.find("ai-grant:db").is_none());
        ui.click("settings-close");
        assert!(ui.find("ai-badge:web").is_some(), "its tile says so");
        assert!(ui.find("ai-badge:db").is_none());

        // A client with no known token hears why.
        let (_, hello) = Client::open(ui, "shw_nope");
        assert!(hello["error"].as_str().expect("error").contains("not known"), "{hello}");

        let (mut c, hello) = Client::open(ui, &token);
        assert_eq!(hello["ok"], true, "{hello}");
        let init = c.call("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test"}}));
        assert_eq!(init["result"]["serverInfo"]["name"], "shellwing");
        let (hosts, _) = c.tool("list_hosts", json!({}));
        assert!(hosts.starts_with("web\t"), "{hosts}");
        assert!(!hosts.contains("db"), "{hosts}");

        // Short: its end and output at once.
        let (said, error) = c.tool("run", json!({"host": "web", "command": "echo \"$HOME\"; echo oops >&2; exit 3"}));
        assert!(!error, "{said}");
        assert!(said.starts_with("exit 3 after"), "{said}");
        assert!(said.contains(&home.display().to_string()) && said.contains("oops"), "{said}");

        // Long: a job, followed until its end.
        let (said, _) = c.tool("run", json!({"host": "web", "command": "echo first; sleep 2; echo second", "wait_seconds": 1}));
        assert!(said.contains("still running") && said.contains("first"), "{said}");
        let job = said.split_whitespace().next().expect("job id").to_string();
        assert!(job.starts_with("j-"), "{said}");
        let mut rest = String::new();
        for _ in 0..10 {
            let (said, _) = c.tool("job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.contains("exit 0") && rest.contains("second") && !rest.contains("first"), "{rest}");

        let (said, error) = c.tool("run", json!({"host": "db", "command": "true"}));
        assert!(error && said.contains("not one of the hosts"), "{said}");

        // Taken back: refused from then on.
        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-take-back:0");
        let (said, error) = c.tool("run", json!({"host": "web", "command": "true"}));
        assert!(error && said.contains("not one of the hosts"), "{said}");
        let log = std::fs::read_to_string(ui.config_dir().join("state/ai-activity.jsonl")).expect("log");
        assert_eq!(log.lines().count(), 2, "each command kept: {log}");
        assert!(log.contains("echo first"));
    }
);

/// A client let in and given `web` (and `db`, not given), its token; the
/// grant's commands as `mode` says (`Free`, `Risky`, `Ask`).
fn given_web(ui: &mut Ui, mode: &str) -> String {
    given_web_so(ui, mode, "background")
}

/// [`given_web`], `how` the host is reached: `background` or `tab`.
fn given_web_so(ui: &mut Ui, mode: &str, how: &str) -> String {
    given_web_may(ui, mode, how, &[])
}

/// [`given_web_so`], the boxes of *It may* named in `flip` (`run`, `read`,
/// `write`) clicked; its home served over SFTP too.
fn given_web_may(ui: &mut Ui, mode: &str, how: &str, flip: &[&str]) -> String {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    ui.agent(&[&key]);
    let home = ui.config_dir().join("remote-home");
    std::fs::create_dir_all(&home).expect("home");
    let server = ui.server(Policy {
        key: Some(key.public_key().clone()),
        shell: Some((home.clone(), home.clone())),
        sftp: Some(home),
        ..Default::default()
    });
    ui.trust(&server);
    ui.config_file(
        "hosts.toml",
        &format!("{}{}", host("web", server.port), host("db", server.port)),
    );
    ui.open();
    ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
    ui.click("ai-add-client");
    ui.click("ai-kind:codex");
    ui.click("ai-copy-config");
    let config = ui.clipboard().expect("the config");
    let token = config
        .split("SHELLWING_TOKEN = \"")
        .nth(1)
        .and_then(|t| t.split('"').next())
        .expect("a token")
        .to_string();
    ui.click("ai-give");
    ui.click("ai-pick:web");
    ui.click("ai-give-to:Codex CLI");
    ui.click(&format!("ai-give-mode:{mode}"));
    ui.click(&format!("ai-give-how:{how}"));
    for f in flip {
        ui.click(&format!("ai-may:{f}"));
    }
    ui.click("ai-give-ok");
    ui.click("settings-close");
    token
}

/// Follows a job until it ends: all it said.
fn follow(c: &mut Client, job: &str) -> String {
    let mut all = String::new();
    for _ in 0..10 {
        let (said, _) = c.tool("job_output", json!({"job": job, "wait_seconds": 5}));
        all.push_str(&said);
        if !said.contains("still running") && !said.contains("waits for the user") {
            break;
        }
    }
    all
}

scenario!(
    /// A grant that asks every time: a command waits, the AI is told so,
    /// AI activity shows it and the host list counts it; Run it runs it,
    /// Refuse refuses it, and Run it this session lets the same command
    /// run from then on without asking.
    a_command_waits_until_allowed,
    |ui| {
        let token = given_web(ui, "Ask");
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool("run", json!({"host": "web", "command": "echo allowed", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.wait_until("the host list to count it", |ui| ui.find("ai-waiting").is_some() || ui.find("actions-fold").is_some());
        ui.click("actions-fold");
        assert!(ui.find("ai-waiting").is_some(), "one waits");
        ui.click("action-ai");
        assert!(ui.find("ai-activity").is_some());
        ui.click("ai-allow");
        let said = follow(&mut c, &job);
        assert!(said.contains("exit 0") && said.contains("allowed"), "{said}");

        let (said, _) = c.tool("run", json!({"host": "web", "command": "rm -rf /tmp/x", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.wait_until("it to wait", |ui| ui.find("ai-refuse").is_some());
        ui.click("ai-refuse");
        let said = follow(&mut c, &job);
        assert!(said.contains("refused by the user"), "{said}");

        let (said, _) = c.tool("run", json!({"host": "web", "command": "hostname", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.wait_until("it to wait", |ui| ui.find("ai-allow-session").is_some());
        ui.click("ai-allow-session");
        assert!(follow(&mut c, &job).contains("exit 0"));
        let (said, _) = c.tool("run", json!({"host": "web", "command": "hostname", "wait_seconds": 5}));
        assert!(said.starts_with("exit 0"), "allowed for the session: {said}");
        assert!(ui.find("ai-waiting").is_none(), "nothing waits");
    }
);

scenario!(
    /// The title bar says when a command waits, wherever the person is: no
    /// fold to open, no tab to be on. It opens AI activity, and goes when
    /// the command is answered.
    the_title_bar_says_a_command_waits,
    |ui| {
        let token = given_web(ui, "Ask");
        let (mut c, _) = Client::open(ui, &token);
        assert!(ui.find("ai-waiting-bar").is_none(), "nothing waits yet");

        let (said, _) = c.tool(
            "run",
            json!({"host": "web", "command": "echo waiting", "wait_seconds": 1}),
        );
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();

        // The host list's count needs Actions unfolded; this does not.
        ui.wait_until("the title bar to say so", |ui| {
            ui.find("ai-waiting-bar").is_some()
        });
        ui.click("ai-waiting-bar");
        assert!(ui.find("ai-activity").is_some(), "it opens AI activity");

        ui.click("ai-allow");
        assert!(follow(&mut c, &job).contains("exit 0"));
        ui.wait_until("the title bar to stop saying so", |ui| {
            ui.find("ai-waiting-bar").is_none()
        });
    }
);

scenario!(
    /// A grant that asks for risky ones: a plain command runs at once, one
    /// with words from the risky list waits; the list is the person's to
    /// change, and the table changes a grant's mode.
    only_risky_commands_wait,
    |ui| {
        let token = given_web(ui, "Risky");
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool("run", json!({"host": "web", "command": "df -h /", "wait_seconds": 5}));
        assert!(said.starts_with("exit 0"), "{said}");
        let (said, _) = c.tool("run", json!({"host": "web", "command": "sudo systemctl stop nginx", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        let (said, _) = c.tool("job_stop", json!({"job": job}));
        assert!(said.contains("stopped"), "{said}");

        // A word of the person's own.
        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-risky");
        ui.keys("ctrl-end");
        ui.type_text("\nhostname");
        let (said, _) = c.tool("run", json!({"host": "web", "command": "hostname", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        c.tool("job_stop", json!({"job": job}));
        // Run freely, from the table: it runs.
        ui.click("ai-mode:0");
        ui.click("ai-mode:0");
        let (said, _) = c.tool("run", json!({"host": "web", "command": "hostname", "wait_seconds": 5}));
        assert!(said.starts_with("exit 0"), "{said}");
    }
);

scenario!(
    /// A host given through a tab: with none open, one opens; the command is
    /// typed there, where its output shows, and the AI gets that output and
    /// the exit status, its marks unseen. A strip says whose tab it is;
    /// paused, a command waits until the person lets it go on.
    an_ai_command_is_typed_into_a_tab,
    |ui| {
        let token = given_web_so(ui, "Free", "tab");
        let (mut c, _) = Client::open(ui, &token);
        assert_eq!(ui.tabs(), 0);
        let (said, error) = c.tool_in(ui, "run", json!({"host": "web", "command": "echo \"from the tab: $((6 * 7))\"", "wait_seconds": 15}));
        assert!(!error, "{said}");
        assert!(said.starts_with("exit 0"), "{said}");
        assert!(said.contains("from the tab: 42"), "{said}");
        assert!(!said.contains("swai"), "the marks are not output: {said}");
        assert_eq!(ui.tabs(), 1, "a tab opened for it");
        ui.wait_for("from the tab: 42");
        assert!(ui.find("ai-strip").is_some());

        let (said, _) = c.tool_in(ui, "run", json!({"host": "web", "command": "false", "wait_seconds": 10}));
        assert!(said.starts_with("exit 1"), "{said}");

        ui.click("ai-pause");
        let (said, _) = c.tool_in(ui, "run", json!({"host": "web", "command": "echo later", "wait_seconds": 1}));
        assert!(said.contains("still running"), "paused: {said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click("ai-resume");
        let mut rest = String::new();
        for _ in 0..5 {
            let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.contains("exit 0") && rest.contains("later"), "{rest}");
        assert_eq!(ui.tabs(), 1, "the same tab, again");
    }
);

scenario!(
    /// Files over SFTP, where the grant lets: a folder listed, text read as
    /// it is and a binary file in base64, a write that waits (risky ones
    /// do) and is written once allowed.
    an_ai_reads_and_writes_files,
    |ui| {
        if !crate::ui_tests::server::has_sftp() {
            eprintln!("skipped: no sftp-server on this system");
            return;
        }
        let token = given_web_may(ui, "Risky", "background", &["write"]);
        let home = ui.config_dir().join("remote-home");
        std::fs::write(home.join("bin.dat"), [0u8, 1, 2, 255]).expect("bin");
        let (mut c, _) = Client::open(ui, &token);

        let (said, error) = c.tool_in(ui, "write_file", json!({"host": "web", "path": "notes.txt", "content": "it's here\n", "wait_seconds": 1}));
        assert!(!error && said.contains("waits for the user"), "a write waits: {said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click("actions-fold");
        ui.click("action-ai");
        ui.click("ai-allow");
        let mut rest = String::new();
        for _ in 0..5 {
            let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.contains("Wrote 10 B at"), "{rest}");
        assert_eq!(std::fs::read_to_string(home.join("notes.txt")).expect("written"), "it's here\n");

        let (said, error) = c.tool_in(ui, "read_file", json!({"host": "web", "path": "notes.txt"}));
        assert!(!error && said.contains("(text)") && said.ends_with("it's here\n"), "{said}");
        let (said, _) = c.tool_in(ui, "read_file", json!({"host": "web", "path": "~/bin.dat"}));
        assert!(said.contains("binary, in base64") && said.ends_with("AAEC/w=="), "{said}");
        let (said, _) = c.tool_in(ui, "list_dir", json!({"host": "web"}));
        assert!(said.contains("-  notes.txt  10 B") && said.contains("-  bin.dat  4 B"), "{said}");
        let (said, _) = c.tool_in(ui, "list_dir", json!({"host": "web", "pattern": "*.dat"}));
        assert!(said.starts_with(&format!("{}: 1 entry like *.dat", home.display())) && !said.contains("notes.txt"), "{said}");
        let (said, _) = c.tool_in(ui, "list_dir", json!({"host": "web", "limit": 1}));
        assert!(said.contains("showing 1 to 1; the rest from offset 1") && said.lines().count() == 2, "{said}");
        let (said, error) = c.tool_in(ui, "read_file", json!({"host": "web", "path": "nope.txt"}));
        assert!(error && said.contains("no such file"), "{said}");
    }
);

scenario!(
    /// A grant to read files alone: no commands, no writes.
    a_grant_to_read_files_alone,
    |ui| {
        if !crate::ui_tests::server::has_sftp() {
            return;
        }
        let token = given_web_may(ui, "Free", "background", &["run"]);
        let (mut c, _) = Client::open(ui, &token);
        let (hosts, _) = c.tool("list_hosts", json!({}));
        assert!(hosts.contains("reading files") && !hosts.contains("run"), "{hosts}");
        let (said, error) = c.tool("run", json!({"host": "web", "command": "true"}));
        assert!(error && said.contains("not to run commands"), "{said}");
        let (said, error) = c.tool("write_file", json!({"host": "web", "path": "x", "content": "x"}));
        assert!(error && said.contains("not given") && said.contains("write"), "{said}");
        let (said, error) = c.tool_in(ui, "list_dir", json!({"host": "web"}));
        assert!(!error && said.contains("entries"), "{said}");
    }
);

/// A client let in (Codex CLI) and three hosts to give it, `web`, `db` and
/// `cache`, none given yet; Settings › AI stays open. Its token.
fn three_hosts(ui: &mut Ui) -> String {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    ui.agent(&[&key]);
    let home = ui.config_dir().join("remote-home");
    std::fs::create_dir_all(&home).expect("home");
    let server = ui.server(Policy {
        key: Some(key.public_key().clone()),
        shell: Some((home.clone(), home)),
        ..Default::default()
    });
    ui.trust(&server);
    ui.config_file(
        "hosts.toml",
        &format!(
            "{}{}{}",
            host("web", server.port),
            host("db", server.port),
            host("cache", server.port)
        ),
    );
    ui.open();
    ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
    ui.click("ai-add-client");
    ui.click("ai-kind:codex");
    ui.click("ai-copy-config");
    let config = ui.clipboard().expect("the config");
    config
        .split("SHELLWING_TOKEN = \"")
        .nth(1)
        .and_then(|t| t.split('"').next())
        .expect("a token")
        .to_string()
}

/// Where `host`'s grant is in the saved order, which its buttons are named
/// by.
fn grant_of(ui: &mut Ui, host: &str) -> usize {
    ui.ws(|ws, _, cx| {
        let snap = ws.ai_snapshot(cx);
        let spec = snap
            .picks
            .iter()
            .flat_map(|p| &p.hosts)
            .find(|h| h.name == host)
            .map(|h| h.spec.clone())
            .expect("the host");
        snap.state
            .grants
            .iter()
            .position(|g| g.host == spec)
            .expect("its grant")
    })
}

scenario!(
    /// Given with Read notes, an agent lists and reads the notes about its
    /// host from the vault, and sees an image one shows; a sensitive one
    /// stays out, as one about no host does.
    an_ai_reads_the_notes_about_its_host,
    |ui| {
        let token = given_web_may(ui, "Ask", "background", &["notes-read"]);
        let vault = ui.vault().expect("the vault");
        let png = b"\x89PNG\r\n\x1a\nnot really".to_vec();
        let image = vault.add_attachment(&png).expect("image");
        let note = |title: &str, hosts: &[&str], sensitive: bool| shellwing_vault::Note {
            id: String::new(),
            title: title.into(),
            category: "Ops".into(),
            format: shellwing_vault::NoteFormat::Markdown,
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
            sensitive,
            shown_to_ai: false,
            changed: String::new(),
            size: 0,
        };
        let restart = vault
            .save_note(
                &note("Restarting web", &["web", "db"], false),
                Some(&format!("sudo systemctl restart nginx\n\n![](attachment:{image})\n")),
            )
            .expect("note");
        let secret = vault
            .save_note(&note("Web's root", &["web"], true), Some("hunter2"))
            .expect("note");
        vault
            .save_note(&note("My own", &[], false), Some("not for agents"))
            .expect("note");
        let (mut c, _) = Client::open(ui, &token);

        let (hosts, _) = c.tool_in(ui, "list_hosts", json!({}));
        assert!(hosts.contains("list_notes"), "said in list_hosts: {hosts}");
        let (listed, error) = c.tool_in(ui, "list_notes", json!({}));
        assert!(!error && listed.starts_with("1 note you may read (about web, or about no host and shown to AI)"), "{listed}");
        assert!(listed.contains(&format!("{restart}\tRestarting web\tOps\tmarkdown\tweb\t")), "db left out: {listed}");
        assert!(!listed.contains("Web's root") && !listed.contains("My own"), "{listed}");

        // Reads wait for no one, even where every command asks.
        let (read, error) = c.tool_in(ui, "read_note", json!({"id": restart}));
        assert!(!error && read.contains("sudo systemctl restart nginx"), "{read}");
        let (said, error) = c.tool_in(ui, "read_note", json!({"id": secret}));
        assert!(error && said.starts_with("No note"), "sensitive: {said}");

        let v = c.call("tools/call", json!({"name": "read_note_image", "arguments": {"id": restart, "image": image}}));
        assert_eq!(v["result"]["content"][1]["type"], "image", "{v}");
        assert_eq!(v["result"]["content"][1]["mimeType"], "image/png");
        use base64::Engine;
        let data = v["result"]["content"][1]["data"].as_str().expect("data");
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(data).expect("base64"), png);
    }
);

scenario!(
    /// Given with Add to notes, an agent's addition waits for the person
    /// (risky ones wait, and every addition does), who sees what goes in:
    /// allowed, it is at the note's end after the line saying who wrote it;
    /// a new note refused is not made.
    an_ai_adds_to_a_note_once_allowed,
    |ui| {
        let token = given_web_may(ui, "Risky", "background", &["notes-read", "notes-add"]);
        let vault = ui.vault().expect("the vault");
        let note = shellwing_vault::Note {
            id: String::new(),
            title: "Restarting web".into(),
            category: String::new(),
            format: shellwing_vault::NoteFormat::Markdown,
            hosts: vec!["web".into()],
            sensitive: false,
            shown_to_ai: false,
            changed: String::new(),
            size: 0,
        };
        let id = vault.save_note(&note, Some("sudo systemctl restart nginx")).expect("note");
        let (mut c, _) = Client::open(ui, &token);
        let (said, error) = c.tool_in(ui, "append_note", json!({"id": id, "text": "The cert renews on the 1st.", "wait_seconds": 1}));
        assert!(!error && said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        assert_eq!(vault.note_text(&id).expect("text").as_deref().map(String::as_str), Some("sudo systemctl restart nginx"), "not before it is allowed");

        ui.click("actions-fold");
        ui.click("action-ai");
        assert!(ui.find("ai-adding").is_some(), "what goes in is shown");
        ui.click("ai-allow");
        let mut rest = String::new();
        for _ in 0..5 {
            let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.contains("Added") && rest.contains("Restarting web"), "{rest}");
        let text = vault.note_text(&id).expect("text").expect("the note").to_string();
        assert!(text.starts_with("sudo systemctl restart nginx\n\n> Added by Codex CLI, "), "{text}");
        assert!(text.ends_with("\n\nThe cert renews on the 1st.\n"), "{text}");

        let before = vault.notes().expect("notes").len();
        let (said, _) = c.tool_in(ui, "add_note", json!({"hosts": ["web"], "title": "Disk", "text": "Cleaned /var/log.", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click(&format!("ai-row:{job}"));
        ui.click("ai-refuse");
        let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
        assert!(said.contains("refused by the user"), "{said}");
        assert_eq!(vault.notes().expect("notes").len(), before, "no note made");
    }
);

scenario!(
    /// A note about no host reaches no agent, until the person ticks Show
    /// to AI in its window: then one holding Read notes on a host lists it.
    a_note_about_no_host_once_shown_to_ai,
    |ui| {
        let token = given_web_may(ui, "Risky", "background", &["notes-read"]);
        let vault = ui.vault().expect("the vault");
        let note = shellwing_vault::Note {
            id: String::new(),
            title: "On-call runbook".into(),
            category: String::new(),
            format: shellwing_vault::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            shown_to_ai: false,
            changed: String::new(),
            size: 0,
        };
        let id = vault.save_note(&note, Some("Page the on-call first.")).expect("note");
        let (mut c, _) = Client::open(ui, &token);
        let (listed, _) = c.tool_in(ui, "list_notes", json!({}));
        assert!(listed.starts_with("No note"), "kept from AI: {listed}");

        let saved = vault.notes().expect("notes").into_iter().find(|n| n.id == id);
        ui.ws(|ws, _, cx| ws.open_note(saved, cx));
        ui.wait_until("the note's window", |ui| ui.note_window().is_some());
        ui.wait_until("its text", |ui| ui.note(|n, _, _| n.is_loaded()));
        let window = ui.note_window().expect("a note window").into();
        ui.click_in(window, "note-ai");
        ui.wait_until("kept shown to AI", |_| {
            vault.notes().expect("notes").iter().any(|n| n.id == id && n.shown_to_ai)
        });
        let (listed, _) = c.tool_in(ui, "list_notes", json!({}));
        assert!(listed.contains(&format!("{id}\tOn-call runbook\t-\tmarkdown\tno host\t")), "{listed}");
        let (read, error) = c.tool_in(ui, "read_note", json!({"id": id}));
        assert!(!error && read.ends_with("Page the on-call first."), "{read}");
    }
);

scenario!(
    /// Given with Use snippets, an agent lists the snippets offered for its
    /// host and runs one, its values filled in, the host's own too. One
    /// typed without Enter waits for the person though the grant runs
    /// freely, and no session allows it.
    an_ai_runs_a_snippet,
    |ui| {
        let token = given_web_may(ui, "Free", "background", &["snippets"]);
        let vault = ui.vault().expect("the vault");
        let snippet = |name: &str, command: &str, scope: &str, enter: bool| shellwing_vault::Snippet {
            id: String::new(),
            name: name.into(),
            command: command.into(),
            scope: scope.into(),
            enter,
            category: String::new(),
            needs: vec![],
            origin: String::new(),
        };
        let greet = vault
            .save_snippet(&snippet("Greet", "echo \"hello {{who}} from {{host}}\"", "host:web", true))
            .expect("snippet");
        let look = vault
            .save_snippet(&snippet("Look first", "echo looked", "", false))
            .expect("snippet");
        vault
            .save_snippet(&snippet("For db", "echo db", "host:db", true))
            .expect("snippet");
        let (mut c, _) = Client::open(ui, &token);

        let (listed, error) = c.tool_in(ui, "list_snippets", json!({"host": "web"}));
        assert!(!error && listed.starts_with("2 snippets offered for web"), "{listed}");
        assert!(listed.contains(&format!("{greet}\tGreet\t-\tvalues: who\truns as a command")), "{listed}");
        assert!(!listed.contains("For db"), "{listed}");

        let (said, error) = c.tool_in(ui, "run_snippet", json!({"host": "web", "id": greet, "values": {"who": "world"}, "wait_seconds": 15}));
        assert!(!error && said.starts_with("exit 0"), "{said}");
        assert!(said.contains("hello world from web"), "{said}");
        let (said, error) = c.tool_in(ui, "run_snippet", json!({"host": "web", "id": greet, "values": {"who": "$(id)"}}));
        assert!(error && said.contains("who has '$'"), "{said}");

        let (said, _) = c.tool_in(ui, "run_snippet", json!({"host": "web", "id": look, "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click("actions-fold");
        ui.click("action-ai");
        ui.click(&format!("ai-row:{job}"));
        assert!(ui.find("ai-snippet").is_some(), "the snippet is named");
        assert!(ui.find("ai-allow-session").is_none(), "no session for it");
        ui.click("ai-allow");
        let mut rest = String::new();
        for _ in 0..5 {
            let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.starts_with("exit 0") && rest.contains("looked"), "{rest}");
    }
);

/// Gives `web` with Use snippets (`how` it is reached), and keeps a
/// snippet for it that needs an environment holding `DEPLOY_TOKEN`: the
/// client's token, and the snippet's id.
fn snippet_with_env(ui: &mut Ui, how: &str) -> (String, String) {
    let token = given_web_may(ui, "Free", how, &["snippets"]);
    let vault = ui.vault().expect("the vault");
    let env = vault
        .save_environment(
            "",
            "Deploy keys",
            "",
            &[("DEPLOY_TOKEN".into(), Some("t0p-s3cret\nline 2".into()))],
        )
        .expect("environment");
    let id = vault
        .save_snippet(&shellwing_vault::Snippet {
            id: String::new(),
            name: "Deploy check".into(),
            command: "echo \"{{stage}} token: ${#DEPLOY_TOKEN} bytes, $(printf %s \"$DEPLOY_TOKEN\" | wc -l) newline\"".into(),
            scope: "host:web".into(),
            enter: true,
            category: String::new(),
            needs: vec![env],
            origin: String::new(),
        })
        .expect("snippet");
    (token, id)
}

/// Runs snippet `id` on web, allows it in AI activity, and follows it to
/// its end: what it printed and how it ended.
fn allowed_snippet(ui: &mut Ui, c: &mut Client, id: &str) -> String {
    let (said, _) = c.tool_in(
        ui,
        "run_snippet",
        json!({"host": "web", "id": id, "values": {"stage": "prod"}, "wait_seconds": 1}),
    );
    assert!(
        said.contains("waits for the user"),
        "it waits, though web runs freely: {said}"
    );
    let job = said.split_whitespace().next().expect("job").to_string();
    ui.click("actions-fold");
    ui.click("action-ai");
    ui.click(&format!("ai-row:{job}"));
    assert!(ui.find("ai-allow-session").is_none(), "no session for it");
    ui.click("ai-allow");
    let mut rest = String::new();
    for _ in 0..8 {
        let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
        rest.push_str(&said);
        if said.starts_with("exit") {
            break;
        }
    }
    let kept = ui.ws(|ws, _, _| {
        ws.ai_activity()
            .iter()
            .find(|a| a.id == job)
            .map(|a| a.command.clone())
    });
    assert_eq!(
        kept.as_deref(),
        Some(
            "echo \"prod token: ${#DEPLOY_TOKEN} bytes, $(printf %s \"$DEPLOY_TOKEN\" | wc -l) newline\""
        ),
        "kept as filled in"
    );
    rest
}

scenario!(
    /// A snippet that needs an environment, run by an agent on its own
    /// connection: it waits for the person though the grant runs freely;
    /// allowed, it has the variable, its value (two lines) sent on the
    /// input, and the agent is told only what the command printed.
    an_ai_runs_a_snippet_with_an_environment,
    |ui| {
        let (token, id) = snippet_with_env(ui, "background");
        let (mut c, _) = Client::open(ui, &token);
        let rest = allowed_snippet(ui, &mut c, &id);
        assert!(rest.starts_with("exit 0") && rest.contains("prod token: 17 bytes, 1 newline"), "{rest}");
        assert!(!rest.contains("s3cret"), "{rest}");
    }
);

scenario!(
    /// The same, through a tab: typed into it as a pane runs such a
    /// snippet, the values sent once the shell is ready, with the echo off.
    an_ai_runs_a_snippet_with_an_environment_in_a_tab,
    |ui| {
        let (token, id) = snippet_with_env(ui, "tab");
        let (mut c, _) = Client::open(ui, &token);
        let rest = allowed_snippet(ui, &mut c, &id);
        assert!(rest.starts_with("exit 0") && rest.contains("prod token: 17 bytes, 1 newline"), "{rest}");
        assert!(!rest.contains("s3cret"), "{rest}");
        assert!(!ui.screen().contains("s3cret"), "not on the tab's screen:\n{}", ui.screen());
    }
);

scenario!(
    /// An agent suggests a snippet: it waits for the person (risky ones
    /// wait, and every suggestion does), who sees it first; allowed, it is
    /// the host's alone, typed without Enter, and Settings says who
    /// suggested it.
    an_ai_suggests_a_snippet,
    |ui| {
        let token = given_web_may(ui, "Risky", "background", &["snippets"]);
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool_in(ui, "add_snippet", json!({"host": "web", "name": "Recent errors", "command": "journalctl -p err -n {{lines}}", "category": "Logs", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        let vault = ui.vault().expect("the vault");
        assert!(vault.snippets().expect("snippets").is_empty(), "not before it is allowed");
        ui.click("actions-fold");
        ui.click("action-ai");
        ui.click(&format!("ai-row:{job}"));
        assert!(ui.find("ai-adding").is_some(), "what it suggests is shown");
        ui.click("ai-allow");
        let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
        assert!(said.contains("Added the snippet \"Recent errors\" for web"), "{said}");
        let kept = vault.snippets().expect("snippets");
        assert_eq!(kept.len(), 1);
        let s = &kept[0];
        assert_eq!(
            (s.scope.as_str(), s.enter, s.origin.as_str(), s.category.as_str()),
            ("host:web", false, "ai:Codex CLI", "Logs")
        );
        let (listed, _) = c.tool_in(ui, "list_snippets", json!({"host": "web"}));
        assert!(listed.contains("Recent errors\tLogs\tvalues: lines\ttyped without Enter"), "{listed}");
        ui.ws(|ws, window, cx| ws.open_settings(Page::Snippets, window, cx));
        assert!(ui.find("snippet-from-ai").is_some(), "Settings says who suggested it");
    }
);

scenario!(
    /// AI activity longer than its pane scrolls, by the bar every list has:
    /// its thumb dragged to the bottom shows the oldest command, the newest
    /// gone off the top.
    a_long_activity_scrolls_by_its_bar,
    |ui| {
        let log: String = (1..=40)
            .map(|i| {
                serde_json::to_string(&shellwing_core::ai::Activity {
                    id: format!("j-{i}"),
                    client: "Codex CLI".into(),
                    host: "web".into(),
                    command: format!("echo {i}"),
                    started: 1_764_000_000 + i,
                    ended: Some(1_764_000_000 + i),
                    end: Some(shellwing_core::ai::End::Exited(Some(0))),
                    output: String::new(),
                    waiting: None,
                    adding: None,
                    snippet: None,
                    client_id: None,
                    spec: None,
                    ran: None,
                })
                .expect("json")
                    + "\n"
            })
            .collect();
        let state = ui.config_dir().join("state");
        std::fs::create_dir_all(&state).expect("state");
        std::fs::write(state.join("ai-activity.jsonl"), log).expect("log");
        ui.open();
        ui.resize(1000., 500.);
        ui.ws(|ws, window, cx| ws.open_activity(window, cx));
        ui.wait_until("the newest", |ui| ui.find("ai-row:j-40").is_some());
        let list = ui.bounds("ai-activity-list");
        let newest = ui.bounds("ai-row:j-40");
        assert!(newest.top() >= list.top(), "the newest shows first");
        let oldest = ui.bounds("ai-row:j-1");
        assert!(oldest.top() > list.bottom(), "the oldest is below what shows");

        // The thumb, at the top of the bar along the list's right edge,
        // dragged to its bottom.
        let x = list.right() - gpui::px(4.);
        ui.flick(
            gpui::point(x, list.top() + gpui::px(6.)),
            gpui::point(x, list.bottom() + gpui::px(40.)),
            Duration::ZERO,
        );
        ui.draw();
        let oldest = ui.bounds("ai-row:j-1");
        assert!(
            oldest.bottom() <= list.bottom() + gpui::px(1.),
            "scrolled to the end: {oldest:?} in {list:?}"
        );
        assert!(ui.bounds("ai-row:j-40").bottom() < list.top(), "the newest went off the top");
    }
);

scenario!(
    /// A long addition waiting for the person scrolls in its box, by the bar
    /// every list has: its thumb dragged down shows the rest.
    a_long_addition_scrolls_by_its_bar,
    |ui| {
        let token = given_web_may(ui, "Risky", "background", &["notes-read", "notes-add"]);
        let vault = ui.vault().expect("the vault");
        let id = vault
            .save_note(
                &shellwing_vault::Note {
                    id: String::new(),
                    title: "Long".into(),
                    category: String::new(),
                    format: shellwing_vault::NoteFormat::Markdown,
                    hosts: vec!["web".into()],
                    sensitive: false,
                    shown_to_ai: false,
                    changed: String::new(),
                    size: 0,
                },
                Some("start"),
            )
            .expect("note");
        let text: String = (1..=80).map(|i| format!("line {i}\n")).collect();
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool_in(ui, "append_note", json!({"id": id, "text": text, "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click("actions-fold");
        ui.click("action-ai");
        ui.click(&format!("ai-row:{job}"));
        let boxed = ui.bounds("ai-adding");
        let before = ui.bounds("ai-adding-text");
        assert!(before.size.height > boxed.size.height, "longer than its box");
        let x = boxed.right() - gpui::px(4.);
        ui.flick(
            gpui::point(x, boxed.top() + gpui::px(6.)),
            gpui::point(x, boxed.bottom() + gpui::px(40.)),
            Duration::ZERO,
        );
        ui.draw();
        let after = ui.bounds("ai-adding-text");
        assert!(
            (after.bottom() - boxed.bottom()).abs() <= gpui::px(12.),
            "scrolled to its end: {after:?} in {boxed:?}"
        );
    }
);

scenario!(
    /// AI activity's list is as wide as its edge is dragged, within limits,
    /// and stays so with the window's layout; a double-click on the edge
    /// gives it its width again.
    the_activity_list_is_as_wide_as_dragged,
    |ui| {
        ui.open();
        ui.resize(1400., 700.);
        ui.ws(|ws, window, cx| ws.open_activity(window, cx));
        let width = |ui: &mut Ui| ui.bounds("ai-activity-list").size.width;
        // The column's own width, its 1 px edge aside.
        let start = width(ui);
        assert!(
            (start - gpui::px(crate::activity_view::LIST_WIDTH)).abs() <= gpui::px(1.),
            "as it comes: {start:?}"
        );
        let drag = |ui: &mut Ui, by: f32| {
            let edge = ui.bounds("ai-list-edge").center();
            ui.flick(edge, gpui::point(edge.x + gpui::px(by), edge.y), Duration::ZERO);
            ui.draw();
        };
        drag(ui, 140.);
        assert_eq!(width(ui), start + gpui::px(140.), "wider as dragged");
        let kept: shellwing_config::WindowState = serde_json::from_str(
            &std::fs::read_to_string(ui.config_dir().join("state/window.json")).expect("window.json"),
        )
        .expect("a window state");
        assert_eq!(
            kept.activity_list_width,
            Some(crate::activity_view::LIST_WIDTH + 140.),
            "kept with the window's layout"
        );
        drag(ui, -2000.);
        assert_eq!(
            width(ui),
            start - gpui::px(crate::activity_view::LIST_WIDTH - crate::activity_view::MIN_LIST_WIDTH),
            "no narrower than its least"
        );
        ui.double_click("ai-list-edge");
        ui.draw();
        assert_eq!(width(ui), start, "as it came, on a double-click");
    }
);

/// Allows job `job`, waiting in AI activity, and follows it to its end.
fn allowed(ui: &mut Ui, c: &mut Client, job: &str) -> String {
    ui.click(&format!("ai-row:{job}"));
    ui.click("ai-allow");
    let mut rest = String::new();
    for _ in 0..8 {
        let (said, error) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
        rest.push_str(&said);
        if error || said.starts_with("exit") || said.contains("did not run") {
            break;
        }
    }
    rest
}

scenario!(
    /// Files and folders copied between this computer and a host, their
    /// bytes never through the agent: only within the folders given its
    /// client, each copy waiting where risky ones wait, nothing written over
    /// that should not be.
    an_ai_copies_files_both_ways,
    |ui| {
        if !crate::ui_tests::server::has_sftp() {
            eprintln!("skipped: no sftp-server on this system");
            return;
        }
        let token = given_web_may(ui, "Risky", "background", &["write"]);
        let work = ui.dir.join("work");
        std::fs::create_dir_all(work.join("site")).expect("work");
        std::fs::write(work.join("build.txt"), "hello build\n").expect("build");
        std::fs::write(work.join("site/index.html"), "<h1>hi</h1>\n").expect("site");
        let home = ui.config_dir().join("remote-home");
        let (mut c, _) = Client::open(ui, &token);
        let local = |p: &str| work.join(p).display().to_string();

        let (said, error) = c.tool_in(ui, "upload", json!({"host": "web", "local_path": local("build.txt"), "remote_path": "build.txt"}));
        assert!(error && said.contains("none yet"), "no folder given yet: {said}");
        let folder = work.display().to_string();
        ui.ws(|ws, _, cx| ws.change_ai(|s| s.clients[0].folders = vec![folder], cx));
        let (said, error) = c.tool_in(ui, "upload", json!({"host": "web", "local_path": "/etc/hostname", "remote_path": "x"}));
        assert!(error && said.contains("in none of the folders"), "{said}");

        ui.click("actions-fold");
        ui.click("action-ai");
        let (said, _) = c.tool_in(ui, "upload", json!({"host": "web", "local_path": local("build.txt"), "remote_path": "build.txt", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        let rest = allowed(ui, &mut c, &job);
        assert!(rest.contains("Uploaded 12 B to web:"), "{rest}");
        assert_eq!(std::fs::read_to_string(home.join("build.txt")).expect("there"), "hello build\n");

        let (said, _) = c.tool_in(ui, "upload", json!({"host": "web", "local_path": local("site"), "remote_path": "site", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        let rest = allowed(ui, &mut c, &job);
        assert!(rest.contains("Uploaded"), "{rest}");
        assert_eq!(std::fs::read_to_string(home.join("site/index.html")).expect("a folder"), "<h1>hi</h1>\n");
        let (said, _) = c.tool_in(ui, "upload", json!({"host": "web", "local_path": local("site"), "remote_path": "site", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        let rest = allowed(ui, &mut c, &job);
        assert!(rest.contains("is already there"), "a folder goes where nothing is: {rest}");

        std::fs::write(home.join("server.log"), "line 1\nline 2\n").expect("log");
        let (said, _) = c.tool_in(ui, "download", json!({"host": "web", "remote_path": "server.log", "local_path": local("server.log"), "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        let rest = allowed(ui, &mut c, &job);
        assert!(rest.contains("Downloaded 14 B from web"), "{rest}");
        assert_eq!(std::fs::read_to_string(work.join("server.log")).expect("here"), "line 1\nline 2\n");
        assert!(!rest.contains("line 1"), "its bytes never go to the agent: {rest}");
        let log = std::fs::read_to_string(ui.config_dir().join("state/ai-activity.jsonl")).unwrap_or_default();
        assert!(!log.contains("hello build") && !log.contains("line 2"), "nor to the log");
    }
);

scenario!(
    /// sudo's password, on a host given in the background: a command that
    /// needs it says how to ask; asked, it waits for the person though the
    /// grant runs freely, who types it (the agent never sees it) and may keep
    /// it a while, so the next sudo run goes at once.
    an_ai_runs_sudo_with_the_password_typed,
    |ui| {
        let token = given_web_may(ui, "Free", "background", &[]);
        // A stand-in for sudo, first in PATH: it asks its askpass, as sudo -A
        // does, and runs the command for the password `pa ss`.
        let sudo = ui.config_dir().join("remote-home").join("sudo");
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
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sudo, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool_in(ui, "run", json!({"host": "web", "command": "sudo echo as root", "wait_seconds": 15}));
        assert!(said.contains("run the command again with sudo: true"), "says how: {said}");

        let (said, _) = c.tool_in(ui, "run", json!({"host": "web", "command": "sudo echo as root", "sudo": true, "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "waits though it runs freely: {said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.click("actions-fold");
        ui.click("action-ai");
        ui.click(&format!("ai-row:{job}"));
        assert!(ui.find("ai-allow-session").is_none(), "no session for it");
        assert!(ui.find("ai-sudo-saved").is_none(), "no saved password to offer");
        ui.click("ai-sudo-password");
        ui.type_text("pa ss");
        ui.click("ai-sudo-remember");
        ui.click("ai-allow");
        let mut rest = String::new();
        for _ in 0..5 {
            let (said, _) = c.tool_in(ui, "job_output", json!({"job": job, "wait_seconds": 5}));
            rest.push_str(&said);
            if said.starts_with("exit") {
                break;
            }
        }
        assert!(rest.starts_with("exit 0") && rest.contains("as root"), "{rest}");
        assert!(!rest.contains("pa ss"), "{rest}");

        // Kept a while: the next goes at once.
        let (said, _) = c.tool_in(ui, "run", json!({"host": "web", "command": "sudo echo again", "sudo": true, "wait_seconds": 15}));
        assert!(said.starts_with("exit 0") && said.contains("again"), "kept: {said}");
        let log = std::fs::read_to_string(ui.config_dir().join("state/ai-activity.jsonl")).unwrap_or_default();
        assert!(!log.contains("pa ss"), "the password is in no log");

        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        assert!(ui.find("ai-sudo-minutes").is_some(), "how long it is kept, in Settings");
    }
);

scenario!(
    /// A pane is let read by an AI client from its menu, the client chosen in
    /// a dialog: the pane gets AI's violet edge and a mark saying who and how
    /// long, its tab a violet dot. The menu, or the mark's ✕, ends it.
    a_pane_is_let_read_by_an_ai_client,
    |ui| {
        let _token = given_web_may(ui, "Free", "background", &[]);
        ui.ws(|ws, window, cx| ws.open(crate::local::SPEC, window, cx));
        ui.wait_until("the shell to start", |ui| ui.tabs() == 1 && ui.status() == crate::session::Status::Connected);
        assert!(ui.find("ai-reader").is_none(), "not read unless let");

        ui.right_click("terminal");
        ui.click("menu-ai-read");
        ui.answer_prompt("Codex CLI");
        ui.wait_until("the mark", |ui| ui.find("ai-reader").is_some());
        assert!(ui.find("tab-ai-read:0").is_some(), "its tab says so");
        let pane = ui.ws(|ws, _, _| crate::workspace::Workspace::pane_id(&ws.active_view()));
        let readers = ui.ws(|ws, _, _| ws.ai_pane_readers());
        assert_eq!(readers.len(), 1);
        assert_eq!(readers[0].1, pane, "this pane");

        // The menu ends it.
        ui.right_click("terminal");
        ui.click("menu-ai-read");
        ui.wait_until("no mark", |ui| ui.find("ai-reader").is_none());
        assert!(ui.find("tab-ai-read:0").is_none());
        assert!(ui.ws(|ws, _, _| ws.ai_pane_readers()).is_empty());

        // Cancel lets none read it.
        ui.right_click("terminal");
        ui.click("menu-ai-read");
        ui.answer_prompt("Cancel");
        assert!(ui.find("ai-reader").is_none());

        // So does the mark's ✕.
        ui.right_click("terminal");
        ui.click("menu-ai-read");
        ui.answer_prompt("Codex CLI");
        ui.wait_until("the mark", |ui| ui.find("ai-reader").is_some());
        ui.click("ai-reader-stop");
        ui.wait_until("no mark", |ui| ui.find("ai-reader").is_none());
        assert!(ui.ws(|ws, _, _| ws.ai_pane_readers()).is_empty());
    }
);

scenario!(
    /// Notes and snippets are given apart from files, and only when ticked:
    /// a host given as it comes has none of those rights, one given with
    /// them ticked has them, and they are kept.
    notes_are_given_only_when_ticked,
    |ui| {
        let _token = three_hosts(ui);
        ui.click("ai-give");
        ui.click("ai-pick:db");
        ui.click("ai-give-ok");
        ui.click("ai-give");
        ui.click("ai-pick:web");
        ui.click("ai-may:notes-read");
        ui.click("ai-may:notes-add");
        ui.click("ai-may:snippets");
        ui.click("ai-give-ok");
        let rights = |ui: &mut Ui, host: &str| {
            let i = grant_of(ui, host);
            ui.ws(|ws, _, cx| {
                let g = ws.ai_snapshot(cx).state.grants[i].clone();
                (g.read, g.notes_read, g.notes_add, g.snippets)
            })
        };
        assert_eq!(rights(ui, "db"), (true, false, false, false), "not unless ticked");
        assert_eq!(rights(ui, "web"), (true, true, true, true));
        let saved = std::fs::read_to_string(ui.config_dir().join("state/ai.json")).expect("ai.json");
        assert!(saved.contains("\"notes_read\": true") && saved.contains("\"notes_add\": true"), "kept: {saved}");
        assert!(saved.contains("\"snippets\": true"), "kept: {saved}");
    }
);

scenario!(
    /// A host given every right says them all on its row, wrapped: the row
    /// grows to hold them, and its text stays inside it.
    a_host_given_everything_says_it_inside_its_row,
    |ui| {
        let _token = three_hosts(ui);
        ui.click("ai-give");
        ui.click("ai-pick:web");
        for f in ["write", "notes-read", "notes-add", "snippets"] {
            ui.click(&format!("ai-may:{f}"));
        }
        ui.click("ai-give-ok");
        let row = ui.find("ai-grant:web").expect("row");
        let how = ui.find("ai-grant-how:web").expect("what it may do");
        assert!(f32::from(how.size.height) > 30., "wrapped: {how:?}");
        assert!(f32::from(row.size.height) > 45., "grown: {row:?}");
        assert!(how.top() >= row.top() && how.bottom() <= row.bottom(), "{how:?} in {row:?}");
    }
);

scenario!(
    /// A whole group is given at once, a grant for each host; one given
    /// again is said to be replaced, and is. Paused, a host stops what it
    /// ran, is still listed, says it is paused and runs nothing; its tile
    /// loses its mark. Resumed, it runs again.
    many_hosts_given_at_once_and_one_paused,
    |ui| {
        let token = three_hosts(ui);
        ui.click("ai-give");
        ui.click("ai-pick-group:Hosts");
        assert!(ui.find("ai-give-replaces").is_none());
        // The one client is chosen already.
        ui.click("ai-give-ok");
        for h in ["web", "db", "cache"] {
            assert!(ui.find(&format!("ai-grant:{h}")).is_some(), "{h} given");
        }
        ui.click("ai-give");
        ui.click("ai-pick:web");
        assert!(ui.find("ai-give-replaces").is_some(), "web is given already");
        ui.click("ai-give-ok");
        let grants = ui.ws(|ws, _, cx| ws.ai_snapshot(cx).state.grants.len());
        assert_eq!(grants, 3, "replaced, not added");

        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool(
            "run",
            json!({"host": "db", "command": "sleep 30", "wait_seconds": 1}),
        );
        assert!(said.contains("still running"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        let db = grant_of(ui, "db");
        ui.click(&format!("ai-pause:{db}"));
        assert!(ui.find("ai-grant-paused:db").is_some());
        let (said, _) = c.tool("job_output", json!({"job": job, "wait_seconds": 5}));
        assert!(said.contains("stopped"), "pausing stops what runs there: {said}");
        let (hosts, _) = c.tool("list_hosts", json!({}));
        assert!(hosts.contains("paused by the user"), "{hosts}");
        assert_eq!(hosts.lines().count(), 3, "{hosts}");
        let (said, error) = c.tool("run", json!({"host": "db", "command": "true"}));
        assert!(error && said.contains("db is paused for Codex CLI"), "{said}");
        let (said, error) = c.tool("run", json!({"host": "web", "command": "true"}));
        assert!(!error && said.starts_with("exit 0"), "{said}");
        let saved = std::fs::read_to_string(ui.config_dir().join("state/ai.json")).expect("ai.json");
        assert!(saved.contains("\"paused\""), "the pause is kept: {saved}");

        ui.click("settings-close");
        assert!(ui.find("ai-badge:db").is_none(), "its tile's mark goes");
        assert!(ui.find("ai-badge:web").is_some());

        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click(&format!("ai-pause:{db}"));
        assert!(ui.find("ai-grant-paused:db").is_none());
        let (said, error) = c.tool("run", json!({"host": "db", "command": "true"}));
        assert!(!error && said.starts_with("exit 0"), "resumed: {said}");
        ui.click("settings-close");
        assert!(ui.find("ai-badge:db").is_some(), "its mark is back");
    }
);

scenario!(
    /// Hosts picked in the host list go to the form, ticked. A client paused
    /// is told so whatever it asks, and its hosts' tiles lose their mark;
    /// resumed, it
    /// reaches them again, its grants as they were.
    picked_hosts_given_and_the_client_paused,
    |ui| {
        let token = three_hosts(ui);
        ui.click("settings-close");
        let ctrl = if cfg!(target_os = "macos") {
            gpui::Modifiers::command()
        } else {
            gpui::Modifiers::control()
        };
        ui.click_with("host:web", ctrl);
        ui.click_with("host:cache", ctrl);
        ui.click("picked-give");
        assert!(ui.find("ai-give-form").is_some(), "the form opens");
        assert!(ui.find("picked-bar").is_none(), "the picks went to it");
        ui.click("ai-give-ok");
        assert!(ui.find("ai-grant:web").is_some() && ui.find("ai-grant:cache").is_some());
        assert!(ui.find("ai-grant:db").is_none());

        let (mut c, _) = Client::open(ui, &token);
        ui.click("ai-pause-client:Codex CLI");
        assert!(ui.find("ai-granted:Codex CLI").is_some());
        let (hosts, _) = c.tool("list_hosts", json!({}));
        assert!(hosts.contains("Codex CLI is paused"), "{hosts}");
        let (said, error) = c.tool("run", json!({"host": "web", "command": "true"}));
        assert!(error && said.contains("Codex CLI is paused"), "{said}");
        ui.click("settings-close");
        assert!(ui.find("ai-badge:web").is_none() && ui.find("ai-badge:cache").is_none());

        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-pause-client:Codex CLI");
        let (said, error) = c.tool("run", json!({"host": "web", "command": "true"}));
        assert!(!error && said.starts_with("exit 0"), "resumed: {said}");
        let (hosts, _) = c.tool("list_hosts", json!({}));
        assert_eq!(hosts.lines().count(), 2, "{hosts}");
        ui.click("settings-close");
        assert!(ui.find("ai-badge:web").is_some() && ui.find("ai-badge:cache").is_some());
    }
);

scenario!(
    /// AI activity draws a command at a prompt and its output as a tab's
    /// terminal does: colors as colors, not escapes; a running command's
    /// output added as it comes, the prompt once; and the find shortcut
    /// opens a find bar over it.
    activity_shows_output_as_a_terminal,
    |ui| {
        let token = given_web(ui, "Free");
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool(
            "run",
            json!({"host": "web", "command": "printf '\\033[31mred\\033[0m\\nplain\\n'"}),
        );
        assert!(said.starts_with("exit 0"), "{said}");
        ui.ws(|ws, window, cx| ws.open_activity(window, cx));
        ui.wait_until("the output to show", |ui| {
            ui.ws(|ws, _, cx| ws.activity_text(cx)).contains("plain")
        });
        let text = ui.ws(|ws, _, cx| ws.activity_text(cx));
        assert!(text.starts_with("$ printf"), "the command at a prompt: {text}");
        assert!(text.contains("\nred\nplain"), "{text}");
        let output = text.split_once('\n').map_or("", |(_, o)| o);
        assert!(!output.contains("[31m"), "a color is drawn, not shown: {text}");

        let (said, _) = c.tool(
            "run",
            // What it prints is not in the command: $((…)) makes it.
            json!({"host": "web", "command": "echo one-$((1+1)); sleep 2; echo three-$((1+2))", "wait_seconds": 1}),
        );
        let job = said.split_whitespace().next().expect("job").to_string();
        ui.wait_until("the running one to show", |ui| {
            ui.ws(|ws, _, cx| ws.activity_text(cx)).contains("one-2")
        });
        let text = ui.ws(|ws, _, cx| ws.activity_text(cx));
        assert!(!text.contains("three-3"), "{text}");
        assert!(follow(&mut c, &job).contains("exit 0"));
        ui.wait_until("the rest to come", |ui| {
            ui.ws(|ws, _, cx| ws.activity_text(cx)).contains("three-3")
        });
        let text = ui.ws(|ws, _, cx| ws.activity_text(cx));
        assert_eq!(text.matches("$ echo one").count(), 1, "the prompt once: {text}");
        assert!(text.contains("one-2\nthree-3"), "{text}");

        ui.keys(if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-shift-f" });
        assert!(ui.find("viewer-find").is_some(), "the find bar opens");
    }
);

scenario!(
    /// AI activity says how much its log keeps on this computer, of how
    /// much. Settings › AI sets how much, in MB, 32 at least: typed, or
    /// with − and +. Clear, once confirmed, empties it, the log too.
    activity_kept_chosen_and_cleared,
    |ui| {
        let token = given_web(ui, "Free");
        let (mut c, _) = Client::open(ui, &token);
        for word in ["one", "two"] {
            let (said, _) = c.tool("run", json!({"host": "web", "command": format!("echo {word}")}));
            assert!(said.starts_with("exit 0"), "{said}");
        }
        let mb = 1024 * 1024;
        let kept = |ui: &mut Ui| ui.ws(|ws, _, _| ws.ai_kept()).expect("a log");
        ui.wait_until("both kept", |ui| ui.ws(|ws, _, _| ws.ai_activity()).iter().filter(|a| a.end.is_some()).count() == 2);
        let (size, most) = kept(ui);
        assert!(size > 0, "on this computer");
        assert_eq!(most, 32 * mb, "32 MB unless chosen");

        let chosen = |ui: &mut Ui| {
            let saved = std::fs::read_to_string(ui.config_dir().join("state/ai.json")).expect("ai.json");
            let v: serde_json::Value = serde_json::from_str(&saved).expect("json");
            v.get("activity_mb").and_then(|m| m.as_u64())
        };
        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-activity-more");
        assert_eq!(kept(ui).1, 64 * mb);
        assert_eq!(chosen(ui), Some(64), "saved");
        ui.click("ai-activity-less");
        ui.click("ai-activity-less");
        assert_eq!(kept(ui).1, 32 * mb, "never under 32");
        assert_eq!(chosen(ui), None, "the usual, not written");

        ui.click("ai-activity-mb");
        ui.keys("ctrl-a");
        ui.type_text("500");
        assert_eq!(kept(ui).1, 500 * mb, "as typed, no most");
        assert_eq!(chosen(ui), Some(500));
        ui.keys("ctrl-a");
        ui.type_text("10");
        assert_eq!(kept(ui).1, 500 * mb, "under 32: not taken");
        assert_eq!(chosen(ui), Some(500));
        ui.click("ai-activity-more");
        assert_eq!(chosen(ui), Some(532), "+ from what is kept");
        ui.click("settings-close");

        ui.ws(|ws, window, cx| ws.open_activity(window, cx));
        assert!(ui.find("ai-kept").is_some(), "how much it keeps");
        assert!(ui.find("ai-log-folder").is_some(), "its folder");
        ui.click("ai-clear");
        ui.answer_prompt("Cancel");
        assert!(kept(ui).0 > 0, "kept");
        ui.click("ai-clear");
        ui.answer_prompt("Clear activity");
        ui.wait_until("cleared", |ui| ui.ws(|ws, _, _| ws.ai_activity()).is_empty());
        assert_eq!(kept(ui).0, 0, "on this computer too");
        assert!(!ui.config_dir().join("state/ai-activity.jsonl").exists());
        assert!(ui.find("ai-clear").is_none(), "nothing to clear");
    }
);

scenario!(
    /// An agent reads back what it ran: the command, how it ended and its
    /// last lines, then one in full; reading it back is in AI activity too.
    /// Taken back, the host takes its commands with it.
    an_agent_reads_back_what_it_ran,
    |ui| {
        let token = given_web(ui, "Free");
        let (mut c, _) = Client::open(ui, &token);
        let (said, _) = c.tool("run", json!({"host": "web", "command": "for i in one two three four; do echo $i; done"}));
        assert!(said.starts_with("exit 0"), "{said}");
        let (said, error) = c.tool("activity", json!({}));
        assert!(!error && said.starts_with("1 command you ran on web"), "{said}");
        assert!(said.contains("for i in one") && said.contains("exit 0 after"), "{said}");
        assert!(said.contains("    four") && !said.contains("    one"), "the last lines: {said}");
        let job = said
            .lines()
            .find(|l| l.contains("for i in one"))
            .and_then(|l| l.split('\t').next())
            .expect("its id")
            .to_string();
        let (full, _) = c.tool("activity", json!({"job": job}));
        assert!(full.contains("one\ntwo\nthree\nfour"), "in full: {full}");
        let seen = format!("activity {job}");
        ui.wait_until("AI activity to show it read back", |ui| {
            ui.ws(|ws, _, _| ws.ai_activity().last().is_some_and(|a| a.command == seen))
        });

        ui.ws(|ws, window, cx| ws.open_settings(Page::Ai, window, cx));
        ui.click("ai-take-back:0");
        let (said, _) = c.tool("activity", json!({}));
        assert!(said.starts_with("No host is given") && !said.contains("for i"), "{said}");
    }
);

scenario!(
    /// A host whose key is not in known_hosts: the agent's command waits
    /// for the person, AI activity showing the key. Refused, nothing runs
    /// and nothing is kept; asked again and trusted, the key is kept and
    /// the command runs. The agent never trusts it itself.
    a_new_host_key_waits_for_the_person,
    |ui| {
        let token = given_web(ui, "Free");
        let known = ui.known_hosts();
        let key = known.split_whitespace().nth(2).expect("the server's key").to_string();
        std::fs::write(ui.dir.join("known_hosts"), "").expect("forget it");
        let (mut c, _) = Client::open(ui, &token);
        let waits = |ui: &mut Ui| {
            ui.wait_until("the key to wait for the person", |ui| {
                ui.ws(|ws, _, _| {
                    ws.ai_activity().last().is_some_and(|a| {
                        a.waiting.as_deref().is_some_and(|w| w.starts_with(shellwing_core::ai::HOST_KEY) && w.contains("SHA256:"))
                    })
                })
            });
        };

        let (said, _) = c.tool("run", json!({"host": "web", "command": "echo refused", "wait_seconds": 1}));
        assert!(said.contains("waits for the user"), "{said}");
        let job = said.split_whitespace().next().expect("job").to_string();
        waits(ui);
        ui.ws(|ws, window, cx| ws.open_activity(window, cx));
        assert!(ui.find("ai-host-key").is_some(), "the key shown");
        assert!(ui.find("ai-allow-session").is_none(), "no session to trust it for");
        ui.click("ai-refuse");
        let said = follow(&mut c, &job);
        assert!(said.contains("not accepted") && !said.contains("refused\n"), "{said}");
        assert!(ui.known_hosts().is_empty(), "nothing kept: {}", ui.known_hosts());

        let (said, _) = c.tool("run", json!({"host": "web", "command": "echo trusted", "wait_seconds": 1}));
        let job = said.split_whitespace().next().expect("job").to_string();
        waits(ui);
        ui.click("ai-trust");
        let said = follow(&mut c, &job);
        assert!(said.contains("exit 0") && said.contains("trusted"), "{said}");
        assert!(ui.known_hosts().contains(&key), "kept: {}", ui.known_hosts());
    }
);

scenario!(
    /// A second shellwing started on the same config asks this one to come
    /// forward: with a host, a tab opens to it here; with none, nothing
    /// opens. The second one never draws a window (main.rs).
    a_second_start_shows_this_one,
    |ui| {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
        ui.agent(&[&key]);
        let server = ui.server(Policy {
            key: Some(key.public_key().clone()),
            ..Default::default()
        });
        ui.trust(&server);
        ui.config_file("hosts.toml", &host("web", server.port));
        ui.open();
        let at = Endpoint::for_config(&ui.config_dir());
        let ask = |ui: &mut Ui, args: Value| {
            let mut s = UnixStream::connect(&at.0).expect("the open one");
            s.write_all(format!("{}\n", json!({ "show": args })).as_bytes()).expect("ask");
            let mut reply = String::new();
            BufReader::new(s).read_line(&mut reply).expect("reply");
            ui.settle(Duration::from_millis(50));
            reply
        };
        let before = ui.tabs();
        assert_eq!(ask(ui, json!([])).trim(), r#"{"ok":true}"#);
        assert_eq!(ui.tabs(), before, "nothing asked, nothing opened");
        ask(ui, json!(["web"]));
        ui.wait_until("a tab to web", |ui| ui.tabs() == before + 1);
        assert_eq!(ui.view(|v, _| v.session().spec.clone()), "web");
    }
);

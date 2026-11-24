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

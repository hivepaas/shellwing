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
        assert!(token.starts_with("sw_"), "{config}");
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
        let (_, hello) = Client::open(ui, "sw_nope");
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

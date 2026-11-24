//! The Model Context Protocol, as shellwing speaks it to one client: JSON-RPC
//! 2.0, a message a line (the stdio transport), and the tools a client
//! reaches its hosts with.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use shellwing_config::{AiClient, AiHow};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use super::{End, Hub, Said};

/// The protocol's versions this server speaks, newest first.
pub const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long `run` and `job_output` wait at the most, and by default.
pub const MOST_WAIT: u64 = 60;
pub const WAIT: u64 = 10;

const INSTRUCTIONS: &str = "shellwing gives you the hosts its user chose, and signs in for you: \
you never see a key or a password. list_hosts says which hosts you have. run starts a command \
on one, as `ssh host 'command'` would, with no terminal, and waits up to wait_seconds for its \
end; a longer one goes on as a job, whose new output job_output gives you as it comes. \
job_stop stops it. A command that asks for input (a sudo password, y/n) gets none: use \
non-interactive forms (sudo -n, apt-get -y). The user sees every command you run, and may \
have one wait until they allow it: run then says so, and job_output tells you once it ran.";

/// Serves `client` on `reader` and `writer` until it goes: a request at a
/// time is read, and each answered on its own, so a long `run` does not
/// hold up a `ping`.
pub async fn serve<R, W>(hub: Arc<Hub>, client: AiClient, reader: R, mut writer: W)
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let writing = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut line = msg.to_string();
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
        }
    });
    hub.client_connected(&client.id, true);
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(error(Value::Null, -32700, &format!("not JSON: {e}")));
                continue;
            }
        };
        // A batch (2025-03-26): each of its messages.
        let msgs = match msg {
            Value::Array(all) => all,
            one => vec![one],
        };
        for msg in msgs {
            let (hub, client, tx) = (hub.clone(), client.clone(), tx.clone());
            tokio::spawn(async move {
                if let Some(answer) = answer(&hub, &client, &msg, &tx).await {
                    let _ = tx.send(answer);
                }
            });
        }
    }
    hub.client_connected(&client.id, false);
    drop(tx);
    let _ = writing.await;
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// The answer to one message; None for a notification, which gets none.
pub async fn answer(
    hub: &Arc<Hub>,
    client: &AiClient,
    msg: &Value,
    tx: &tokio::sync::mpsc::UnboundedSender<Value>,
) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str)?;
    let id = msg.get("id").cloned()?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => ok(id, initialized(&params)),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let progress = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .cloned();
            match call(hub, client, name, &args, progress, tx).await {
                Ok(text) => ok(id, json!({"content": [{"type": "text", "text": text}]})),
                Err(Problem::Tool(text)) => ok(
                    id,
                    json!({"content": [{"type": "text", "text": text}], "isError": true}),
                ),
                Err(Problem::Unknown) => error(id, -32602, &format!("no tool called {name}")),
            }
        }
        _ => error(id, -32601, &format!("no method {method}")),
    })
}

enum Problem {
    /// The tool said no: the model reads why.
    Tool(String),
    Unknown,
}

/// The answer to `initialize`, with `params` as the client sent them: the
/// version it asked for if this server speaks it.
pub fn initialized(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| VERSIONS.contains(v))
        .unwrap_or(VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "shellwing", "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS,
    })
}

/// The tools, as `tools/list` gives them.
pub fn tools() -> Value {
    let wait = json!({
        "type": "integer",
        "minimum": 0,
        "maximum": MOST_WAIT,
        "description": format!("Seconds to wait, {WAIT} if not given."),
    });
    json!([
        {
            "name": "list_hosts",
            "title": "List the hosts",
            "description": "The hosts the user gave you, by name, with their address and how you may use them.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": true},
        },
        {
            "name": "run",
            "title": "Run a command",
            "description": "Runs a shell command on a host, as `ssh host 'command'` would, with no terminal and no input. Waits up to wait_seconds for its end: then its output (stdout and stderr, as they came) and its exit status. Still running after that, it goes on as a job: you get its output so far and its id, for job_output and job_stop.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": {"type": "string", "description": "The host's name, as list_hosts gives it."},
                    "command": {"type": "string", "description": "The command, run by the host's login shell."},
                    "wait_seconds": wait,
                },
                "required": ["host", "command"],
            },
            "annotations": {"destructiveHint": true, "openWorldHint": true},
        },
        {
            "name": "job_output",
            "title": "Read a job's output",
            "description": "What a job printed since you last asked. Waits up to wait_seconds for something new (or its end) when there is nothing yet, so call it again and again to follow a long command.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "job": {"type": "string", "description": "The id run gave."},
                    "wait_seconds": wait,
                },
                "required": ["job"],
            },
            "annotations": {"readOnlyHint": true},
        },
        {
            "name": "job_stop",
            "title": "Stop a job",
            "description": "Stops a job that still runs.",
            "inputSchema": {
                "type": "object",
                "properties": {"job": {"type": "string", "description": "The id run gave."}},
                "required": ["job"],
            },
        },
    ])
}

fn text_arg(args: &Value, name: &str) -> Result<String, Problem> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Problem::Tool(format!("{name} is missing")))
}

fn wait_arg(args: &Value) -> Duration {
    Duration::from_secs(
        args.get("wait_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(WAIT)
            .min(MOST_WAIT),
    )
}

async fn call(
    hub: &Arc<Hub>,
    client: &AiClient,
    name: &str,
    args: &Value,
    progress: Option<Value>,
    tx: &tokio::sync::mpsc::UnboundedSender<Value>,
) -> Result<String, Problem> {
    match name {
        "list_hosts" => {
            let given = hub.given(&client.id);
            if given.is_empty() {
                return Ok("No host is given to you yet: the user gives hosts in shellwing, under Settings › AI.".into());
            }
            Ok(given
                .iter()
                .map(|g| {
                    let how = match g.grant.how {
                        AiHow::Background => "commands with run",
                        AiHow::Tab => "through its tab (not yet in this shellwing)",
                    };
                    format!("{}\t{}\t{how}", g.name, g.detail)
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "run" => {
            let host = text_arg(args, "host")?;
            let command = text_arg(args, "command")?;
            let job = hub.run(client, &host, &command).map_err(Problem::Tool)?;
            let said = follow(&job, wait_arg(args), true, progress, tx).await;
            Ok(report(&job.id, &said))
        }
        "job_output" => {
            let id = text_arg(args, "job")?;
            let job = hub
                .job(&client.id, &id)
                .ok_or_else(|| Problem::Tool(format!("no job {id}")))?;
            let said = follow(&job, wait_arg(args), false, progress, tx).await;
            Ok(report(&job.id, &said))
        }
        "job_stop" => {
            let id = text_arg(args, "job")?;
            let job = hub
                .job(&client.id, &id)
                .ok_or_else(|| Problem::Tool(format!("no job {id}")))?;
            if job.ended() {
                return Ok(format!("{id} had ended already."));
            }
            job.stop();
            hub.ended(&job);
            Ok(format!("{id} is stopped."))
        }
        _ => Err(Problem::Unknown),
    }
}

/// Waits as [`super::Job::wait`] does, telling a client that asked for
/// progress how much came, each second meanwhile.
async fn follow(
    job: &super::Job,
    most: Duration,
    for_end: bool,
    progress: Option<Value>,
    tx: &tokio::sync::mpsc::UnboundedSender<Value>,
) -> Said {
    let Some(token) = progress else {
        return job.wait(most, for_end).await;
    };
    let deadline = tokio::time::Instant::now() + most;
    let mut said = Said {
        new: String::new(),
        end: None,
        seconds: 0,
        cut: false,
        waiting: false,
    };
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let step = job.wait(left.min(Duration::from_secs(1)), true).await;
        said.new.push_str(&step.new);
        said.end = step.end;
        said.seconds = step.seconds;
        said.cut |= step.cut;
        said.waiting = step.waiting;
        let last = said.new.trim_end().lines().last().unwrap_or("").to_string();
        let _ = tx.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"progressToken": token, "progress": said.new.len(), "message": last},
        }));
        if said.end.is_some() || left.is_zero() {
            return said;
        }
        // Not waiting for the end: what came is enough.
        if !for_end && !said.new.is_empty() {
            return said;
        }
    }
}

/// A job's news, as the model reads it.
fn report(id: &str, said: &Said) -> String {
    let mut out = match &said.end {
        Some(end @ End::Exited(_)) => format!("{} after {} s.", end.said(), said.seconds),
        Some(end) => format!("{}.", end.said()),
        None if said.waiting => format!(
            "{id} waits for the user, who chooses whether it runs: job_output says once they have; job_stop takes it back."
        ),
        None => format!(
            "{id} is still running after {} s: job_output gives what comes next; job_stop stops it.",
            said.seconds
        ),
    };
    if said.cut {
        out.push_str(&format!(
            " It printed more than {} MB: the rest was not kept.",
            super::KEEP / 1024 / 1024
        ));
    }
    if said.new.is_empty() {
        if !said.waiting && said.end != Some(End::Refused) {
            out.push_str(if said.end.is_some() {
                " Nothing more was printed."
            } else {
                " Nothing new was printed."
            });
        }
    } else {
        out.push_str("\n\n");
        out.push_str(&said.new);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub() -> Arc<Hub> {
        struct None_;
        impl shellwing_ssh::Secrets for None_ {
            fn password(&self, _: &str) -> Option<String> {
                None
            }
            fn passphrase(&self, _: &str) -> Option<String> {
                None
            }
            fn learned(&self, _: shellwing_ssh::Learned) {}
        }
        let (tx, _rx) = async_channel::unbounded();
        Hub::new(
            Handle::current(),
            Arc::new(None_),
            &shellwing_config::Ssh::default(),
            None,
            tx,
        )
    }
    use tokio::runtime::Handle;

    fn client() -> AiClient {
        AiClient {
            id: "c1".into(),
            kind: "stdio".into(),
            name: "Some agent".into(),
            token_sha256: super::super::token_hash("shw_t"),
            added_at: 0,
        }
    }

    async fn ask(hub: &Arc<Hub>, msg: Value) -> Option<Value> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        answer(hub, &client(), &msg, &tx).await
    }

    #[tokio::test]
    async fn it_says_who_it_is_in_the_version_asked() {
        let hub = hub();
        let a = ask(
            &hub,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "x"}}}),
        )
        .await
        .expect("answer");
        assert_eq!(a["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(a["result"]["serverInfo"]["name"], "shellwing");
        assert!(a["result"]["capabilities"]["tools"].is_object());
        let a = ask(
            &hub,
            json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
        )
        .await
        .expect("answer");
        assert_eq!(a["result"]["protocolVersion"], VERSIONS[0]);
        // A notification gets no answer.
        assert!(
            ask(
                &hub,
                json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn its_tools_and_what_they_refuse() {
        let hub = hub();
        let a = ask(
            &hub,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .await
        .expect("answer");
        let names: Vec<&str> = a["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert_eq!(names, ["list_hosts", "run", "job_output", "job_stop"]);
        let a = ask(
            &hub,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "list_hosts", "arguments": {}}}),
        )
        .await
        .expect("answer");
        assert!(
            a["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("No host")
        );
        // A host not given is refused, as a tool's error the model reads.
        let a = ask(
            &hub,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "run", "arguments": {"host": "db", "command": "true"}}}),
        )
        .await
        .expect("answer");
        assert_eq!(a["result"]["isError"], true);
        assert!(
            a["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("not one of the hosts")
        );
        let a = ask(&hub, json!({"jsonrpc": "2.0", "id": 4, "method": "nope"}))
            .await
            .expect("answer");
        assert_eq!(a["error"]["code"], -32601);
    }

    #[test]
    fn the_model_reads_how_far_it_got() {
        let running = Said {
            new: "step 1\n".into(),
            end: None,
            seconds: 10,
            cut: false,
            waiting: false,
        };
        let r = report("j-4", &running);
        assert!(r.starts_with("j-4 is still running after 10 s"), "{r}");
        assert!(r.ends_with("step 1\n"));
        let done = Said {
            new: String::new(),
            end: Some(End::Exited(Some(2))),
            seconds: 3,
            cut: false,
            waiting: false,
        };
        assert_eq!(
            report("j-4", &done),
            "exit 2 after 3 s. Nothing more was printed."
        );
    }
}

//! Where the app listens for AI clients, and `shellwing mcp`, which a client
//! starts: it hands the client's MCP messages to the app and back. A Unix
//! socket only its owner may open, or a named pipe on Windows.
//!
//! The first line each way says who is there: the client's token
//! (`{"token": "shw_…"}`), and the app's answer (`{"ok": true}`, or
//! `{"error": "…"}`). MCP's messages follow, a line each.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use super::Hub;

/// Where the app listens, for the config directory `dir`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(pub PathBuf);

impl Endpoint {
    pub fn for_config(dir: &Path) -> Endpoint {
        #[cfg(windows)]
        {
            use sha2::{Digest, Sha256};
            let h = Sha256::digest(dir.to_string_lossy().as_bytes());
            let tag: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
            Endpoint(PathBuf::from(format!(r"\\.\pipe\shellwing-mcp-{tag}")))
        }
        #[cfg(not(windows))]
        {
            let path = dir.join("state").join("mcp.sock");
            // A socket's path is short (104 bytes on macOS): else one in the
            // temp folder, named for the directory.
            if path.as_os_str().len() < 100 {
                return Endpoint(path);
            }
            use sha2::{Digest, Sha256};
            let h = Sha256::digest(dir.to_string_lossy().as_bytes());
            let tag: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
            Endpoint(std::env::temp_dir().join(format!("shellwing-mcp-{tag}.sock")))
        }
    }
}

/// What a client is told when it cannot be served: why, and what to do.
pub fn refused_because(why: &str) -> String {
    format!("shellwing: {why}")
}

/// Serves AI clients at `at` until the app quits. Another shellwing already
/// listening there keeps it.
pub async fn listen(hub: Arc<Hub>, at: Endpoint) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = &at.0;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if path.exists() {
            // One that answers is another shellwing's; one that does not
            // was left behind.
            if tokio::net::UnixStream::connect(path).await.is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "another shellwing listens for AI clients",
                ));
            }
            let _ = std::fs::remove_file(path);
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        loop {
            let (stream, _) = listener.accept().await?;
            let hub = hub.clone();
            tokio::spawn(async move {
                let (r, w) = stream.into_split();
                serve_one(hub, r, w).await;
            });
        }
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = at.0.to_string_lossy().into_owned();
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)?;
        loop {
            server.connect().await?;
            let connected = server;
            server = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&name)?;
            let hub = hub.clone();
            tokio::spawn(async move {
                let (r, w) = tokio::io::split(connected);
                serve_one(hub, r, w).await;
            });
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (hub, at);
        Err(std::io::Error::other("no AI clients on this system"))
    }
}

/// One client: its token first, then MCP. Or a second shellwing started
/// on the same config ([`show`]): this one comes forward instead.
async fn serve_one<R, W>(hub: Arc<Hub>, r: R, mut w: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut r = BufReader::new(r);
    let mut hello = String::new();
    if r.read_line(&mut hello).await.is_err() {
        return;
    }
    let hello = serde_json::from_str::<Value>(&hello).ok();
    if let Some(args) = hello.as_ref().and_then(|v| v.get("show")) {
        let args = args
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        hub.show(args);
        let _ = w.write_all(b"{\"ok\":true}\n").await;
        return;
    }
    let token = hello
        .as_ref()
        .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_string));
    let client = token.as_deref().and_then(|t| hub.client(t));
    let Some(client) = client else {
        let why = if token.is_none() {
            "this client has no token: set SHELLWING_TOKEN to the one Settings › AI gave it"
        } else {
            "this client's token is not known: add the client again in Settings › AI"
        };
        let _ = w
            .write_all(format!("{}\n", json!({ "error": why })).as_bytes())
            .await;
        return;
    };
    if w.write_all(b"{\"ok\":true}\n").await.is_err() {
        return;
    }
    super::mcp::serve(hub, client, r, w).await;
}

/// `shellwing mcp`: the client's stdio to the app at `at`, `token` its
/// SHELLWING_TOKEN.
///
/// It always says who it is (`initialize`) and what tools it has, so a
/// client counts it as up even while the app is closed or its token is not
/// known; a tool called then is the app's again once it is reached, each
/// call trying anew, and else says why to the model.
pub async fn bridge(at: Endpoint, token: Option<String>) -> std::io::Result<()> {
    bridge_on(at, token, tokio::io::stdin(), tokio::io::stdout()).await
}

type Reader = tokio::io::Lines<BufReader<Box<dyn AsyncRead + Unpin + Send>>>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

/// The app listening at `at`, reached: what it says, and where to say it.
async fn open(at: &Endpoint) -> std::io::Result<(Box<dyn AsyncRead + Unpin + Send>, Writer)> {
    #[cfg(unix)]
    {
        let s = tokio::net::UnixStream::connect(&at.0).await?;
        let (r, w) = s.into_split();
        Ok((Box::new(r), Box::new(w)))
    }
    #[cfg(windows)]
    {
        let s = tokio::net::windows::named_pipe::ClientOptions::new().open(&at.0)?;
        let (r, w) = tokio::io::split(s);
        Ok((Box::new(r), Box::new(w)))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = at;
        Err(std::io::Error::other("no AI clients on this system"))
    }
}

/// A second shellwing started on the config of one open at `at`: that one
/// is asked to come forward and open `args` (a host and its forwards, as
/// typed), and whether it did. One open from before this was asked is not
/// counted, and nor is none.
pub async fn show(at: &Endpoint, args: &[String]) -> bool {
    let asked = async {
        let (r, mut w) = open(at).await.ok()?;
        let hello = format!("{}\n", json!({ "show": args }));
        w.write_all(hello.as_bytes()).await.ok()?;
        let mut reply = String::new();
        BufReader::new(r).read_line(&mut reply).await.ok()?;
        let reply: Value = serde_json::from_str(&reply).ok()?;
        Some(reply["ok"] == true)
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), asked)
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// The app, reached with `token`: its answers, a line each, and where the
/// client's messages go; else why not.
async fn reach(at: &Endpoint, token: Option<&str>) -> Result<(Reader, Writer), String> {
    let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) else {
        return Err(
            "SHELLWING_TOKEN is not set: Settings › AI › Add client gives the config to paste"
                .into(),
        );
    };
    let not_open = |_| {
        "the app is not open: start shellwing, keep it open while the agent works, and call the tool again"
            .to_string()
    };
    let (r, w) = open(at).await.map_err(not_open)?;
    let mut r = BufReader::new(r);
    let mut w = w;
    let hello = format!("{}\n", json!({ "token": token }));
    w.write_all(hello.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    r.read_line(&mut reply).await.map_err(|e| e.to_string())?;
    let reply: Value =
        serde_json::from_str(&reply).unwrap_or(json!({"error": "the app did not answer"}));
    if let Some(why) = reply.get("error").and_then(Value::as_str) {
        return Err(why.to_string());
    }
    Ok((r.lines(), w))
}

/// A message that wants an answer: a method and an id.
fn is_request(msg: &Value) -> bool {
    msg.get("method").is_some() && msg.get("id").is_some_and(|id| !id.is_null())
}

/// An answer to a request, as a line: an id and no method.
fn is_answer(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .is_ok_and(|v| v.get("method").is_none() && v.get("id").is_some_and(|id| !id.is_null()))
}

/// Writes `v` as a line, and sends it.
async fn say<O: AsyncWrite + Unpin>(out: &mut O, v: Value) -> std::io::Result<()> {
    out.write_all(format!("{v}\n").as_bytes()).await?;
    out.flush().await
}

/// [`bridge`], between `stdin` and `stdout`.
pub async fn bridge_on<I, O>(
    at: Endpoint,
    token: Option<String>,
    stdin: I,
    mut stdout: O,
) -> std::io::Result<()>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let mut client = BufReader::new(stdin).lines();
    let mut app = reach(&at, token.as_deref()).await;
    // What the client said to begin with, to say again to an app reached
    // later.
    let mut begun: Option<Value> = None;
    // Requests sent to the app and not answered yet. Once the client is done
    // asking, these are what the bridge waits for: Windows' named pipes
    // cannot tell the app the client is done (no half-close), so the app
    // closing its end cannot be waited for.
    let mut waiting: usize = 0;
    loop {
        let Ok((answers, to_app)) = &mut app else {
            // Not reached: what it can say alone, and a new try per tool.
            let Some(line) = client.next_line().await? else {
                return Ok(());
            };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            match (method, id) {
                ("initialize", Some(id)) => {
                    begun = Some(params.clone());
                    say(&mut stdout, json!({"jsonrpc": "2.0", "id": id, "result": super::mcp::initialized(&params)})).await?;
                }
                ("ping", Some(id)) => {
                    say(
                        &mut stdout,
                        json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    )
                    .await?;
                }
                ("tools/list", Some(id)) => {
                    say(&mut stdout, json!({"jsonrpc": "2.0", "id": id, "result": {"tools": super::mcp::tools()}})).await?;
                }
                (_, None) => {}
                (_, Some(id)) => {
                    app = reach(&at, token.as_deref()).await;
                    match &mut app {
                        Ok((answers, to_app)) => {
                            // Begun again with the app, its own answer let go.
                            if let Some(params) = &begun {
                                let hello = json!({"jsonrpc": "2.0", "id": "shellwing-bridge", "method": "initialize", "params": params});
                                to_app.write_all(format!("{hello}\n").as_bytes()).await?;
                                let _ = answers.next_line().await?;
                                to_app
                                    .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
                                    .await?;
                            }
                            to_app.write_all(format!("{line}\n").as_bytes()).await?;
                            waiting = 1;
                        }
                        Err(why) => {
                            let why = refused_because(why);
                            let answer = if method == "tools/call" {
                                json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": why}], "isError": true}})
                            } else {
                                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": why}})
                            };
                            say(&mut stdout, answer).await?;
                        }
                    }
                }
            }
            continue;
        };
        tokio::select! {
            line = client.next_line() => match line? {
                Some(line) => {
                    let msg = serde_json::from_str::<Value>(&line).ok();
                    if let Some(msg) = &msg
                        && msg.get("method").and_then(Value::as_str) == Some("initialize")
                    {
                        begun = msg.get("params").cloned();
                    }
                    if to_app.write_all(format!("{line}\n").as_bytes()).await.is_err() {
                        app = Err("the app went".into());
                        waiting = 0;
                    } else if msg.as_ref().is_some_and(is_request) {
                        waiting += 1;
                    }
                }
                // The client is done asking: the app hears so, and the
                // bridge ends once what it was asked is answered.
                None => {
                    let _ = to_app.shutdown().await;
                    while waiting > 0 {
                        let Ok(Some(answer)) = answers.next_line().await else {
                            break;
                        };
                        if is_answer(&answer) {
                            waiting -= 1;
                        }
                        stdout.write_all(format!("{answer}\n").as_bytes()).await?;
                    }
                    stdout.flush().await?;
                    return Ok(());
                }
            },
            answer = answers.next_line() => match answer {
                Ok(Some(answer)) => {
                    if is_answer(&answer) {
                        waiting = waiting.saturating_sub(1);
                    }
                    stdout.write_all(format!("{answer}\n").as_bytes()).await?;
                    stdout.flush().await?;
                }
                // The app quit: the next tool tries again.
                _ => {
                    app = Err("the app quit".into());
                    waiting = 0;
                }
            },
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_long_directory_gets_a_short_socket() {
        let short = Endpoint::for_config(Path::new("/home/me/.config/shellwing"));
        assert_eq!(
            short.0,
            Path::new("/home/me/.config/shellwing/state/mcp.sock")
        );
        let long = "/x".repeat(60);
        let e = Endpoint::for_config(Path::new(&long));
        assert!(e.0.as_os_str().len() < 100, "{e:?}");
        assert_eq!(
            e,
            Endpoint::for_config(Path::new(&long)),
            "the same each time"
        );
    }
}

#[cfg(all(test, unix))]
mod through {
    use super::*;
    use shellwing_config::{AiClient, AiState};

    struct NoSecrets;
    impl shellwing_ssh::Secrets for NoSecrets {
        fn password(&self, _: &str) -> Option<String> {
            None
        }
        fn passphrase(&self, _: &str) -> Option<String> {
            None
        }
        fn learned(&self, _: shellwing_ssh::Learned) {}
    }

    /// A client on the bridge, its messages a line each.
    struct Talk {
        to: tokio::io::DuplexStream,
        from: tokio::io::Lines<BufReader<tokio::io::DuplexStream>>,
    }

    impl Talk {
        fn start(at: &Endpoint, token: &str) -> Talk {
            let (to, stdin) = tokio::io::duplex(1 << 16);
            let (stdout, from) = tokio::io::duplex(1 << 16);
            tokio::spawn(bridge_on(at.clone(), Some(token.into()), stdin, stdout));
            Talk {
                to,
                from: BufReader::new(from).lines(),
            }
        }

        async fn ask(&mut self, msg: Value) -> Value {
            self.to
                .write_all(format!("{msg}\n").as_bytes())
                .await
                .expect("write");
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(5), self.from.next_line())
                    .await
                    .expect("in time")
                    .expect("read")
                    .expect("a line");
            serde_json::from_str(&line).expect("json")
        }

        async fn tool(&mut self, id: u64, name: &str) -> (String, bool) {
            let v = self
                .ask(json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": {}}}))
                .await;
            (
                v["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                v["result"]["isError"] == true,
            )
        }
    }

    /// With the app closed, the bridge still begins and lists its tools, so
    /// the client counts it as up; a tool says why it cannot be used, and
    /// works once the app is open, in the same session. A token the app does
    /// not know: the same, and the tool says so.
    #[tokio::test]
    async fn the_bridge_is_up_before_the_app_is() {
        let dir = std::env::temp_dir().join(format!("sw-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = Endpoint::for_config(&dir);

        let mut c = Talk::start(&at, "shw_t");
        let init = c
            .ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}))
            .await;
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "{init}");
        c.to.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .expect("write");
        let list = c
            .ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .await;
        assert_eq!(list["result"]["tools"].as_array().map(Vec::len), Some(16));
        let (said, error) = c.tool(3, "list_hosts").await;
        assert!(error && said.contains("not open"), "{said}");

        let (tx, _rx) = async_channel::unbounded();
        let hub = Hub::new(
            tokio::runtime::Handle::current(),
            Arc::new(NoSecrets),
            &shellwing_config::Ssh::default(),
            None,
            tx,
        );
        hub.set_state(AiState {
            clients: vec![AiClient {
                id: "c".into(),
                kind: "stdio".into(),
                name: "x".into(),
                token_sha256: super::super::token_hash("shw_t"),
                added_at: 0,
                paused: false,
            }],
            ..Default::default()
        });
        tokio::spawn(listen(hub, at.clone()));
        for _ in 0..100 {
            if at.0.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        // The app open now: the same session reaches it.
        let (said, error) = c.tool(4, "list_hosts").await;
        assert!(!error && said.contains("No host is given"), "{said}");
        let pong = c
            .ask(json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}))
            .await;
        assert_eq!(pong["result"], json!({}));

        let mut other = Talk::start(&at, "shw_other");
        let init = other
            .ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}))
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "shellwing");
        let (said, error) = other.tool(2, "list_hosts").await;
        assert!(error && said.contains("not known"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// A second shellwing: told no when none is open, and else the open
    /// one hears its arguments, as typed.
    #[tokio::test]
    async fn a_second_start_reaches_the_open_one() {
        let dir = std::env::temp_dir().join(format!("sw-show-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = Endpoint::for_config(&dir);
        assert!(!show(&at, &[]).await, "none open");

        let (tx, rx) = async_channel::unbounded();
        let hub = Hub::new(
            tokio::runtime::Handle::current(),
            Arc::new(NoSecrets),
            &shellwing_config::Ssh::default(),
            None,
            tx,
        );
        tokio::spawn(listen(hub, at.clone()));
        for _ in 0..100 {
            if at.0.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let args: Vec<String> = ["web", "-L", "8080:localhost:80"]
            .map(String::from)
            .to_vec();
        assert!(show(&at, &args).await, "the open one said yes");
        let heard = rx.recv().await.expect("an event");
        assert!(
            matches!(&heard, super::super::HubEvent::Show { args: got } if *got == args),
            "{:?}",
            matches!(heard, super::super::HubEvent::Show { .. })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// The client done asking, the bridge ends once each of its requests
    /// is answered, though the app keeps its end open: Windows' named pipes
    /// cannot tell an app the client is done, and a bridge that waited for
    /// the app to close was left running there.
    #[tokio::test]
    async fn the_bridge_ends_once_answered_though_the_app_stays() {
        use tokio::io::AsyncBufReadExt;
        let dir = std::env::temp_dir().join(format!("sw-ends-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let at = Endpoint(dir.join("app.sock"));
        let listener = tokio::net::UnixListener::bind(&at.0).expect("bind");
        // An app that answers each request, a little later, and never
        // closes: it does not hear the client is done.
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.expect("accept");
            let (r, mut w) = s.into_split();
            let mut lines = BufReader::new(r).lines();
            let _ = lines.next_line().await;
            w.write_all(b"{\"ok\":true}\n").await.expect("hello");
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).expect("json");
                if let Some(id) = v.get("id") {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    let answer = json!({"jsonrpc": "2.0", "id": id, "result": {}});
                    w.write_all(format!("{answer}\n").as_bytes())
                        .await
                        .expect("answer");
                }
            }
            std::future::pending::<()>().await;
            drop(w);
        });
        let asked = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let mut out = vec![];
        let ended = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            bridge_on(at, Some("shw_t".into()), asked.as_bytes(), &mut out),
        )
        .await;
        assert!(ended.is_ok(), "still running");
        let said = String::from_utf8(out).expect("text");
        assert_eq!(said.lines().count(), 2, "both answered: {said}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

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

/// One client: its token first, then MCP.
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
    let token = serde_json::from_str::<Value>(&hello)
        .ok()
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
/// SHELLWING_TOKEN. With no app to reach, every request is answered with
/// why, so the client can show it.
pub async fn bridge(at: Endpoint, token: Option<String>) -> std::io::Result<()> {
    bridge_on(at, token, tokio::io::stdin(), tokio::io::stdout()).await
}

/// [`bridge`], between `stdin` and `stdout`.
pub async fn bridge_on<I, O>(
    at: Endpoint,
    token: Option<String>,
    stdin: I,
    stdout: O,
) -> std::io::Result<()>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let Some(token) = token.filter(|t| !t.trim().is_empty()) else {
        return refuse(
            stdin,
            stdout,
            &refused_because(
                "SHELLWING_TOKEN is not set: Settings › AI › Add client gives the config to paste",
            ),
        )
        .await;
    };
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(&at.0).await;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(&at.0);
    #[cfg(not(any(unix, windows)))]
    let stream: std::io::Result<tokio::io::DuplexStream> =
        Err(std::io::Error::other("no AI clients on this system"));
    let Ok(stream) = stream else {
        return refuse(
            stdin,
            stdout,
            &refused_because(
                "the app is not open: start shellwing, and keep it open while the agent works",
            ),
        )
        .await;
    };
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r);
    w.write_all(format!("{}\n", json!({ "token": token.trim() })).as_bytes())
        .await?;
    let mut reply = String::new();
    r.read_line(&mut reply).await?;
    let reply: Value = serde_json::from_str(&reply).unwrap_or(json!({"error": "no answer"}));
    if let Some(why) = reply.get("error").and_then(Value::as_str) {
        return refuse(stdin, stdout, &refused_because(why)).await;
    }
    let (mut stdin, mut stdout) = (stdin, stdout);
    let up = async {
        let _ = tokio::io::copy(&mut stdin, &mut w).await;
        // The client is done asking: the app hears so, and answers what it
        // was asked.
        let _ = w.shutdown().await;
    };
    let down = tokio::io::copy(&mut r, &mut stdout);
    tokio::pin!(up, down);
    // The app gone ends it; the client done waits for the last answers.
    tokio::select! {
        _ = &mut down => return Ok(()),
        _ = &mut up => {}
    }
    let _ = down.await;
    Ok(())
}

/// Answers each request on `input` with `why`, until it ends.
async fn refuse<R, W>(input: R, mut output: W, why: &str) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(input).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let answer = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": why}});
        output.write_all(format!("{answer}\n").as_bytes()).await?;
        output.flush().await?;
    }
    Ok(())
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

    #[tokio::test]
    async fn without_the_app_each_request_hears_why() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n".to_vec();
        let mut out = vec![];
        refuse(&input[..], &mut out, "shellwing: it is not open")
            .await
            .expect("refuse");
        let out = String::from_utf8(out).expect("utf-8");
        assert_eq!(out.lines().count(), 1, "a notification gets nothing");
        let v: Value = serde_json::from_str(out.trim()).expect("json");
        assert_eq!(v["id"], 1);
        assert_eq!(v["error"]["message"], "shellwing: it is not open");
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

    /// `shellwing mcp` to a listening app: the client's request goes
    /// through, and its answer comes back; a wrong token, and no app, are
    /// answered with why.
    #[tokio::test]
    async fn a_client_reaches_the_app_through_the_bridge() {
        let dir = std::env::temp_dir().join(format!("sw-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = Endpoint::for_config(&dir);
        let ask = b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n".to_vec();

        let mut out = vec![];
        bridge_on(at.clone(), Some("shw_t".into()), &ask[..], &mut out)
            .await
            .expect("bridge");
        assert!(String::from_utf8_lossy(&out).contains("is not open"));

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
        let mut out = vec![];
        bridge_on(at.clone(), Some("shw_t".into()), &ask[..], &mut out)
            .await
            .expect("bridge");
        let v: Value = serde_json::from_str(String::from_utf8_lossy(&out).trim()).expect("json");
        assert_eq!(
            (v["id"].clone(), v["result"].clone()),
            (json!(7), json!({}))
        );

        let mut out = vec![];
        bridge_on(at.clone(), Some("shw_other".into()), &ask[..], &mut out)
            .await
            .expect("bridge");
        assert!(String::from_utf8_lossy(&out).contains("not known"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

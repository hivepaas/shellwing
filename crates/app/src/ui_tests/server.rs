//! An SSH server for the UI tests, in the test process: the authentication
//! methods a scenario asks for, a PTY, and a small shell whose commands play
//! the parts of real programs and real failures (a flood of output, a full
//! screen program, a title, a dying server). It records what it saw, so a
//! scenario can check the server's side too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::certificate::{Builder, CertType};
use russh::keys::ssh_key::{Algorithm, Certificate};
use russh::keys::{PrivateKey, PublicKey};
use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;

pub const USER: &str = "tester";
pub const PASSWORD: &str = "opensesame";
pub const OTP: &str = "424242";
/// What the shell prints first, and its prompt.
pub const WELCOME: &str = "welcome to the test shell";
pub const PROMPT: &str = "$ ";
/// What `unicode` prints: Vietnamese, CJK (wide) and an emoji.
pub const UNICODE: &str = "Tiếng Việt có dấu — 日本語 — 🚀";

/// How the server lets users in, and what else it allows.
#[derive(Clone, Default)]
pub struct Policy {
    /// Accept this password (the "password" method, or keyboard-interactive
    /// with `pam`).
    pub password: Option<String>,
    /// Ask for the password through keyboard-interactive, as PAM does.
    pub pam: bool,
    /// After the first factor, ask for this one-time code
    /// (keyboard-interactive).
    pub otp: Option<String>,
    /// Accept this key.
    pub key: Option<PublicKey>,
    /// Accept user certificates this authority signed for [`USER`]
    /// (`TrustedUserCAKeys`). Expired ones are refused before they get here.
    pub user_ca: Option<PublicKey>,
    /// Carry -L and -D (direct-tcpip) and -R (tcpip-forward).
    pub forward: bool,
    /// Serve SFTP from this directory with OpenSSH's sftp-server, where it is
    /// installed.
    pub sftp: Option<PathBuf>,
    /// Wait this long before answering the first message of a connection.
    pub slow_start: Option<Duration>,
}

impl Policy {
    pub fn password() -> Self {
        Policy {
            password: Some(PASSWORD.into()),
            ..Default::default()
        }
    }
}

/// What the server saw.
#[derive(Default, Debug)]
pub struct Seen {
    /// Connections accepted.
    pub connections: usize,
    /// Authentication attempts, as "method:outcome" ("password:ok", ...).
    pub auth: Vec<String>,
    /// Every byte typed into shells, as it arrived.
    pub input: Vec<u8>,
    /// Lines the shell read.
    pub lines: Vec<String>,
    /// The PTY size asked for, then every change: (cols, rows).
    pub sizes: Vec<(u32, u32)>,
    /// Environment variables sent.
    pub env: Vec<(String, String)>,
    /// Agent forwarding asked for.
    pub agent: bool,
    /// What the `agent` command got from the forwarded agent: the number of
    /// keys it holds, or why it got nothing.
    pub agent_keys: Option<Result<u32, String>>,
    /// Ports opened for -L/-D (host:port) and bound for -R.
    pub forwards: Vec<String>,
}

/// A running server. Clients reach it through a TCP relay in front of it, so
/// a scenario can cut the network ([`Server::kill`]) or let it go silent
/// ([`Server::stall`]) as real ones do.
pub struct Server {
    pub port: u16,
    pub host_key: PublicKey,
    pub seen: Arc<Mutex<Seen>>,
    /// The relay's tasks: they own the sockets, so aborting them cuts every
    /// connection.
    tasks: Arc<Mutex<Vec<AbortHandle>>>,
    stalled: Arc<std::sync::atomic::AtomicBool>,
}

impl Server {
    /// Starts a server on 127.0.0.1 in `rt`, with a new Ed25519 host key.
    pub fn start(rt: &tokio::runtime::Runtime, policy: Policy) -> Server {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
        Self::start_with_key(rt, policy, key, 0)
    }

    /// Starts a server whose new host key `ca` certified for 127.0.0.1, as a
    /// fleet whose clients trust the authority (`@cert-authority`) and not
    /// each host's key.
    pub fn start_certified(
        rt: &tokio::runtime::Runtime,
        policy: Policy,
        ca: &PrivateKey,
        valid: std::ops::Range<u64>,
    ) -> Server {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
        let cert = certify(ca, key.public_key(), CertType::Host, "127.0.0.1", valid);
        Self::start_inner(rt, policy, key, vec![cert], 0)
    }

    /// Starts a server with `key` on `port` (0: any free one).
    pub fn start_with_key(
        rt: &tokio::runtime::Runtime,
        policy: Policy,
        key: PrivateKey,
        port: u16,
    ) -> Server {
        Self::start_inner(rt, policy, key, vec![], port)
    }

    fn start_inner(
        rt: &tokio::runtime::Runtime,
        policy: Policy,
        key: PrivateKey,
        certificates: Vec<Certificate>,
        port: u16,
    ) -> Server {
        let host_key = key.public_key().clone();
        let mut methods = vec![];
        if policy.key.is_some() || policy.user_ca.is_some() {
            methods.push(MethodKind::PublicKey);
        }
        if policy.password.is_some() && !policy.pam {
            methods.push(MethodKind::Password);
        }
        if policy.pam || policy.otp.is_some() {
            methods.push(MethodKind::KeyboardInteractive);
        }
        let config = Arc::new(server::Config {
            methods: MethodSet::from(&methods[..]),
            keys: vec![key],
            certificates,
            auth_rejection_time: Duration::from_millis(1),
            auth_rejection_time_initial: Some(Duration::ZERO),
            inactivity_timeout: None,
            ..Default::default()
        });
        // The SSH server itself, on a port of its own.
        let inner = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
        let inner_port = inner.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let seen2 = seen.clone();
        rt.spawn(async move {
            while let Ok((socket, _)) = inner.accept().await {
                seen2.lock().expect("seen").connections += 1;
                let shell = Shell {
                    policy: policy.clone(),
                    methods: methods.clone(),
                    seen: seen2.clone(),
                    first_factor_done: false,
                    opened: HashMap::new(),
                    shells: HashMap::new(),
                };
                let (config, slow) = (config.clone(), policy.slow_start);
                tokio::spawn(async move {
                    if let Some(wait) = slow {
                        tokio::time::sleep(wait).await;
                    }
                    if let Ok(running) = server::run_stream(config, socket, shell).await {
                        let _ = running.await;
                    }
                });
            }
        });
        // The relay clients connect to.
        let listener = rt
            .block_on(TcpListener::bind(("127.0.0.1", port)))
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let tasks: Arc<Mutex<Vec<AbortHandle>>> = Arc::default();
        let stalled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tasks2, stalled2) = (tasks.clone(), stalled.clone());
        let accept = rt.spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let Ok(upstream) = TcpStream::connect(("127.0.0.1", inner_port)).await else {
                    continue;
                };
                let (cr, cw) = client.into_split();
                let (ur, uw) = upstream.into_split();
                let mut list = tasks2.lock().expect("tasks");
                for task in [
                    tokio::spawn(relay(cr, uw, stalled2.clone())),
                    tokio::spawn(relay(ur, cw, stalled2.clone())),
                ] {
                    list.push(task.abort_handle());
                }
            }
        });
        tasks.lock().expect("tasks").push(accept.abort_handle());
        Server {
            port,
            host_key,
            seen,
            tasks,
            stalled,
        }
    }

    /// The server's line for a known_hosts file.
    pub fn known_hosts_line(&self) -> String {
        format!(
            "[127.0.0.1]:{} {}",
            self.port,
            self.host_key.to_openssh().expect("public key")
        )
    }

    /// `user@127.0.0.1:port`, as typed on a command line or in the palette.
    pub fn spec(&self) -> String {
        format!("{USER}@127.0.0.1:{}", self.port)
    }

    /// Stops listening and drops every connection at once, as a server that
    /// dies or a network that goes away does.
    pub fn kill(&self) {
        for task in self.tasks.lock().expect("tasks").drain(..) {
            task.abort();
        }
    }

    /// Nothing gets through any more, either way, and nothing is closed: a
    /// cable pulled, Wi-Fi gone, a laptop lid shut. `false` lets it flow again.
    pub fn stall(&self, stalled: bool) {
        self.stalled
            .store(stalled, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn seen<R>(&self, f: impl FnOnce(&Seen) -> R) -> R {
        f(&self.seen.lock().expect("seen"))
    }
}

/// Copies one direction of a relayed connection; holds the bytes while the
/// network is stalled.
async fn relay(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    stalled: Arc<std::sync::atomic::AtomicBool>,
) {
    use tokio::io::AsyncReadExt;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        while stalled.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            break;
        }
    }
    let _ = to.shutdown().await;
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_secs()
}

/// From a minute ago for an hour: a certificate valid now.
pub fn valid_now() -> std::ops::Range<u64> {
    now() - 60..now() + 3600
}

/// A certificate for `key` of `kind`, signed by `ca` for `principal`.
pub fn certify(
    ca: &PrivateKey,
    key: &PublicKey,
    kind: CertType,
    principal: &str,
    valid: std::ops::Range<u64>,
) -> Certificate {
    let mut b = Builder::new_with_random_nonce(
        &mut rand::rng(),
        key.key_data().clone(),
        valid.start,
        valid.end,
    )
    .expect("certificate");
    b.cert_type(kind).expect("type");
    b.key_id("ui-test").expect("id");
    b.valid_principal(principal).expect("principal");
    b.sign(ca).expect("sign")
}

/// Asks the client's forwarded agent how many keys it holds, over a channel
/// the server opens back to the client (as `ssh-add -l` on a server does).
async fn agent_keys(handle: &server::Handle) -> Result<u32, String> {
    let channel = handle
        .channel_open_agent()
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = channel.into_stream();
    // SSH_AGENTC_REQUEST_IDENTITIES
    stream
        .write_all(&[0, 0, 0, 1, 11])
        .await
        .map_err(|e| e.to_string())?;
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|e| e.to_string())?;
    let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    // SSH_AGENT_IDENTITIES_ANSWER, then the number of keys.
    match body.as_slice() {
        [12, a, b, c, d, ..] => Ok(u32::from_be_bytes([*a, *b, *c, *d])),
        _ => Err(format!("unexpected answer {body:?}")),
    }
}

/// A port nothing listens on: connecting is refused. Port 1 (tcpmux) is
/// privileged and unused everywhere; a port freed by the test could be taken
/// by another scenario running at the same time.
pub fn closed_port() -> u16 {
    1
}

/// A port that accepts connections but never says anything: an SSH server
/// that hangs, or a firewall that swallows packets after the handshake.
pub fn silent_port(rt: &tokio::runtime::Runtime) -> u16 {
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        let mut held = vec![];
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    port
}

/// A TCP service on 127.0.0.1 that answers each line with `echo: <line>`:
/// what a forwarded port leads to.
pub fn echo_service(rt: &tokio::runtime::Runtime) -> u16 {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if write
                        .write_all(format!("echo: {line}\n").as_bytes())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn sftp_server() -> Option<PathBuf> {
    [
        "/usr/lib/openssh/sftp-server",
        "/usr/libexec/sftp-server",
        "/usr/libexec/openssh/sftp-server",
        "/usr/lib/ssh/sftp-server",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

/// Whether SFTP scenarios can run here (OpenSSH's sftp-server is installed).
pub fn has_sftp() -> bool {
    sftp_server().is_some()
}

/// A shell's state: the line being typed, and whether a full screen program
/// has the screen.
#[derive(Default)]
struct ShellState {
    line: Vec<u8>,
    alternate: bool,
}

struct Shell {
    policy: Policy,
    methods: Vec<MethodKind>,
    seen: Arc<Mutex<Seen>>,
    first_factor_done: bool,
    /// Session channels not yet a shell or a subsystem.
    opened: HashMap<ChannelId, Channel<Msg>>,
    shells: HashMap<ChannelId, ShellState>,
}

impl Shell {
    fn record(&self, what: String) {
        self.seen.lock().expect("seen").auth.push(what);
    }

    fn reject(&self) -> Auth {
        Auth::Reject {
            proceed_with_methods: Some(MethodSet::from(&self.methods[..])),
            partial_success: false,
        }
    }

    /// The first factor passed: in, or on to the one-time code.
    fn first_factor(&mut self) -> Auth {
        if self.policy.otp.is_some() {
            self.first_factor_done = true;
            Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: true,
            }
        } else {
            Auth::Accept
        }
    }

    /// Runs one line the user typed.
    fn run(
        &mut self,
        channel: ChannelId,
        line: &str,
        session: &mut Session,
    ) -> Result<(), russh::Error> {
        self.seen.lock().expect("seen").lines.push(line.to_string());
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
        let out: String = match cmd {
            "" => String::new(),
            "exit" => {
                let code = arg.parse().unwrap_or(0);
                session.data(channel, "\r\n")?;
                session.exit_status_request(channel, code)?;
                session.eof(channel)?;
                return session.close(channel);
            }
            // The connection drops with nothing said: a crash, a reboot.
            "die" => return Err(russh::Error::Disconnect),
            "lines" => {
                let n: usize = arg.parse().unwrap_or(10);
                let mut chunk = String::new();
                for i in 1..=n {
                    chunk.push_str(&format!("line {i}\r\n"));
                    if chunk.len() > 16 * 1024 {
                        session.data(channel, std::mem::take(&mut chunk))?;
                    }
                }
                chunk
            }
            "title" => format!("\x1b]0;{arg}\x07"),
            "unicode" => format!("{UNICODE}\r\n"),
            "size" => {
                let size = self.seen.lock().expect("seen").sizes.last().copied();
                match size {
                    Some((c, r)) => format!("size {c}x{r}\r\n"),
                    None => "size none\r\n".into(),
                }
            }
            "env" => {
                let env = &self.seen.lock().expect("seen").env;
                let value = env
                    .iter()
                    .rev()
                    .find(|(k, _)| k == arg)
                    .map(|(_, v)| v.clone());
                format!("{arg}={}\r\n", value.unwrap_or_default())
            }
            // `ssh-add -l` through agent forwarding: answered once the agent
            // has.
            "agent" => {
                let (handle, seen) = (session.handle(), self.seen.clone());
                tokio::spawn(async move {
                    let got = agent_keys(&handle).await;
                    let out = match &got {
                        Ok(n) => format!("agent: {n} keys"),
                        Err(_) => "agent: none forwarded".to_string(),
                    };
                    seen.lock().expect("seen").agent_keys = Some(got);
                    let _ = handle.data(channel, format!("\r\n{out}\r\n{PROMPT}")).await;
                });
                return Ok(());
            }
            "bracketed" => "\x1b[?2004h".into(),
            "bell" => "\x07".into(),
            "red" => "\x1b[31mred text\x1b[0m\r\n".into(),
            "clear" => "\x1b[2J\x1b[H".into(),
            // A full screen program (vim, htop, less): the alternate screen
            // until `q` is typed.
            "fullscreen" => {
                if let Some(s) = self.shells.get_mut(&channel) {
                    s.alternate = true;
                }
                return session.data(channel, "\x1b[?1049h\x1b[2J\x1b[HFULL SCREEN PROGRAM");
            }
            _ => format!("you said: {line}\r\n"),
        };
        session.data(channel, format!("\r\n{out}{PROMPT}"))
    }
}

impl server::Handler for Shell {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = user == USER && self.policy.password.as_deref() == Some(password);
        self.record(format!("password:{}", if ok { "ok" } else { "wrong" }));
        Ok(if ok {
            self.first_factor()
        } else {
            self.reject()
        })
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        // A certificate's key is offered as itself: whether the certificate
        // holds is for `auth_openssh_certificate`.
        Ok(
            if self.policy.user_ca.is_some()
                || self.policy.key.as_ref().map(PublicKey::key_data) == Some(key.key_data())
            {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        let ok = user == USER
            && self.policy.key.as_ref().map(PublicKey::key_data) == Some(key.key_data());
        self.record(format!("publickey:{}", if ok { "ok" } else { "wrong" }));
        Ok(if ok {
            self.first_factor()
        } else {
            Auth::reject()
        })
    }

    async fn auth_openssh_certificate(
        &mut self,
        user: &str,
        certificate: &Certificate,
    ) -> Result<Auth, Self::Error> {
        let ok = user == USER
            && certificate.cert_type() == CertType::User
            && certificate.valid_principals().iter().any(|p| p == USER)
            && self.policy.user_ca.as_ref().map(PublicKey::key_data)
                == Some(certificate.signature_key());
        self.record(format!("certificate:{}", if ok { "ok" } else { "wrong" }));
        Ok(if ok {
            self.first_factor()
        } else {
            self.reject()
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        // The password through PAM first, then the code.
        let (name, question, answer, second) = if self.first_factor_done {
            match self.policy.otp.clone() {
                Some(otp) => ("Two-factor", "Verification code: ", otp, true),
                None => return Ok(Auth::reject()),
            }
        } else {
            match (self.policy.pam, self.policy.password.clone()) {
                (true, Some(pw)) => ("", "Password:", pw, false),
                _ => return Ok(Auth::reject()),
            }
        };
        Ok(match response {
            None => Auth::Partial {
                name: name.into(),
                instructions: "".into(),
                prompts: vec![(question.into(), false)].into(),
            },
            Some(mut r) => {
                let ok = user == USER && r.next().as_deref() == Some(answer.as_bytes());
                let what = if second { "otp" } else { "pam" };
                self.record(format!("{what}:{}", if ok { "ok" } else { "wrong" }));
                match (ok, second) {
                    (true, true) => Auth::Accept,
                    (true, false) => self.first_factor(),
                    (false, _) => self.reject(),
                }
            }
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.opened.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.seen.lock().expect("seen").sizes.push((cols, rows));
        session.channel_success(channel)
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.seen.lock().expect("seen").sizes.push((cols, rows));
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.seen
            .lock()
            .expect("seen")
            .env
            .push((name.to_string(), value.to_string()));
        session.channel_success(channel)
    }

    async fn agent_request(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.seen.lock().expect("seen").agent = true;
        Ok(true)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.opened.remove(&channel);
        self.shells.insert(channel, ShellState::default());
        session.channel_success(channel)?;
        session.data(channel, format!("{WELCOME}\r\n{PROMPT}"))
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.shells.contains_key(&channel) {
            return Ok(());
        }
        self.seen
            .lock()
            .expect("seen")
            .input
            .extend_from_slice(data);
        for &b in data {
            let alternate = self.shells.get(&channel).is_some_and(|s| s.alternate);
            if alternate {
                // The full screen program quits on `q`.
                if b == b'q' {
                    if let Some(s) = self.shells.get_mut(&channel) {
                        s.alternate = false;
                    }
                    session.data(channel, format!("\x1b[?1049l{PROMPT}"))?;
                }
                continue;
            }
            match b {
                b'\r' => {
                    let line = self
                        .shells
                        .get_mut(&channel)
                        .map(|s| std::mem::take(&mut s.line))
                        .unwrap_or_default();
                    let line = String::from_utf8_lossy(&line).into_owned();
                    self.run(channel, &line, session)?;
                }
                // Backspace.
                0x7f | 0x08 => {
                    if self
                        .shells
                        .get_mut(&channel)
                        .and_then(|s| s.line.pop())
                        .is_some()
                    {
                        session.data(channel, "\x08 \x08")?;
                    }
                }
                // Ctrl+C: the line is dropped.
                0x03 => {
                    if let Some(s) = self.shells.get_mut(&channel) {
                        s.line.clear();
                    }
                    session.data(channel, format!("^C\r\n{PROMPT}"))?;
                }
                // Ctrl+D on an empty line: the shell ends.
                0x04 => {
                    if self.shells.get(&channel).is_some_and(|s| s.line.is_empty()) {
                        self.run(channel, "exit", session)?;
                        return Ok(());
                    }
                }
                // Escape sequences (arrows, bracketed paste marks) are not part of a line.
                0x1b => {}
                _ => {
                    if let Some(s) = self.shells.get_mut(&channel) {
                        s.line.push(b);
                    }
                    session.data(channel, vec![b])?;
                }
            }
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let opened = self.opened.remove(&channel);
        let (Some(dir), Some(binary), Some(opened), "sftp") =
            (self.policy.sftp.clone(), sftp_server(), opened, name)
        else {
            return session.channel_failure(channel);
        };
        let Ok(mut child) = tokio::process::Command::new(binary)
            .current_dir(dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        else {
            return session.channel_failure(channel);
        };
        session.channel_success(channel)?;
        let (mut stdin, mut stdout) = (child.stdin.take(), child.stdout.take());
        tokio::spawn(async move {
            let (mut from, mut to) = tokio::io::split(opened.into_stream());
            let up = async {
                if let Some(stdin) = stdin.as_mut() {
                    let _ = tokio::io::copy(&mut from, stdin).await;
                }
            };
            let down = async {
                if let Some(stdout) = stdout.as_mut() {
                    let _ = tokio::io::copy(stdout, &mut to).await;
                }
                let _ = to.shutdown().await;
            };
            tokio::join!(up, down);
            let _ = child.wait().await;
        });
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.policy.forward {
            return Ok(()); // dropping the handle refuses the channel
        }
        self.seen
            .lock()
            .expect("seen")
            .forwards
            .push(format!("{host}:{port}"));
        let Ok(mut tcp) = TcpStream::connect((host, port as u16)).await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
        });
        Ok(())
    }

    /// Listens on 127.0.0.1 and sends each connection back to the client.
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if !self.policy.forward {
            return Ok(false);
        }
        let Ok(listener) = TcpListener::bind(("127.0.0.1", *port as u16)).await else {
            return Ok(false);
        };
        *port = listener.local_addr().map_or(0, |a| a.port() as u32);
        self.seen
            .lock()
            .expect("seen")
            .forwards
            .push(format!("remote:{port}"));
        let (handle, address, bound) = (session.handle(), address.to_string(), *port);
        tokio::spawn(async move {
            while let Ok((mut tcp, peer)) = listener.accept().await {
                let (handle, address) = (handle.clone(), address.clone());
                tokio::spawn(async move {
                    let opened = handle
                        .channel_open_forwarded_tcpip(
                            address,
                            bound,
                            peer.ip().to_string(),
                            peer.port() as u32,
                        )
                        .await;
                    if let Ok(channel) = opened {
                        let mut stream = channel.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                    }
                });
            }
        });
        Ok(true)
    }
}

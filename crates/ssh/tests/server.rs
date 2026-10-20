//! End-to-end tests against an in-process russh server: host key questions,
//! password retries, one-time codes, encrypted keys, cancelling, ProxyJump,
//! port forwarding and SFTP (served by OpenSSH's sftp-server, where installed).
//! Keys are generated at run time; nothing secret lives in the repository.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::certificate::{Builder, CertType};
use russh::keys::ssh_key::{Algorithm, Certificate, HashAlg, LineEnding};
use russh::keys::{PrivateKey, PublicKey};
use russh::server::{self, Auth as ServerAuth, ChannelOpenHandle, Msg, Session as ServerSession};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use shellwing_ssh::{
    Active, Auth, Error, Forward, HostKeyPolicy, Input, Options, Prompt, Prompter, Resolved, Target,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Default)]
struct Policy {
    password: Option<&'static str>,
    otp: Option<&'static str>,
    /// Ask for `password` through keyboard-interactive, as PAM does (macOS sshd).
    pam: bool,
    key: Option<PublicKey>,
    /// Allow direct-tcpip (act as a jump host, carry -L and -D) and tcpip-forward (-R).
    forward: bool,
    /// Serve the sftp subsystem from this directory.
    sftp: Option<PathBuf>,
    /// What came back from the client's agent when this server asked it
    /// something: on a forwarding request, or (`rogue`) unasked at shell start.
    agent_reply: Option<AgentReply>,
    rogue: bool,
    /// Run commands (exec requests) with `sh`, with this as `$HOME`.
    home: Option<PathBuf>,
    /// Accept user certificates this authority signed.
    user_ca: Option<PublicKey>,
    /// The password is only the first factor: once it is right, the server
    /// wants `otp` through keyboard-interactive (`AuthenticationMethods
    /// password,keyboard-interactive`).
    then_otp: bool,
}

/// A certificate for `key`, signed by `ca`, valid from a minute ago for an hour.
fn certify(ca: &PrivateKey, key: &PublicKey, kind: CertType, principal: &str) -> Certificate {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_secs();
    let mut b = Builder::new_with_random_nonce(
        &mut rand::rng(),
        key.key_data().clone(),
        now - 60,
        now + 3600,
    )
    .expect("builder");
    b.cert_type(kind).expect("type");
    b.key_id("test").expect("id");
    b.valid_principal(principal).expect("principal");
    b.sign(ca).expect("sign")
}

/// The agent's answer as the server got it, or why it got none.
type AgentReply = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

/// Opens an agent channel to the client, asks the agent for its identities,
/// and records the answer (or the refusal).
fn ask_agent(handle: server::Handle, record: AgentReply) {
    tokio::spawn(async move {
        let answer = async {
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
            Ok::<_, String>(body)
        }
        .await;
        *record.lock().expect("record") = Some(answer);
    });
}

struct TestServer {
    policy: Policy,
    methods: Vec<MethodKind>,
    /// Session channels, whose input is echoed back (direct-tcpip channels are not).
    sessions: Vec<ChannelId>,
    /// Session channels not yet a shell or a subsystem. A shell's channel is
    /// dropped (its input arrives in `data`); a subsystem's becomes a stream.
    opened: HashMap<ChannelId, Channel<Msg>>,
    /// The first factor passed (with `then_otp`).
    first_factor: bool,
}

impl TestServer {
    /// Rejects but keeps every method available, as OpenSSH does (russh's server
    /// would otherwise drop "password" after one wrong try).
    fn reject(&self) -> ServerAuth {
        ServerAuth::Reject {
            proceed_with_methods: Some(MethodSet::from(&self.methods[..])),
            partial_success: false,
        }
    }
}

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        _user: &str,
        password: &str,
    ) -> Result<ServerAuth, Self::Error> {
        Ok(
            if self.policy.password == Some(password) && self.policy.then_otp {
                self.first_factor = true;
                ServerAuth::Reject {
                    proceed_with_methods: Some(MethodSet::from(
                        &[MethodKind::KeyboardInteractive][..],
                    )),
                    partial_success: true,
                }
            } else if self.policy.password == Some(password) {
                ServerAuth::Accept
            } else {
                self.reject()
            },
        )
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        key: &PublicKey,
    ) -> Result<ServerAuth, Self::Error> {
        // The key itself: a key on the wire has no comment. With an authority
        // to trust, any key may go on: its certificate decides, or the key's
        // own check refuses it.
        Ok(
            if self.policy.user_ca.is_some()
                || self.policy.key.as_ref().map(PublicKey::key_data) == Some(key.key_data())
            {
                ServerAuth::Accept
            } else {
                ServerAuth::reject()
            },
        )
    }

    async fn auth_openssh_certificate(
        &mut self,
        user: &str,
        certificate: &Certificate,
    ) -> Result<ServerAuth, Self::Error> {
        let trusted = self
            .policy
            .user_ca
            .as_ref()
            .map(|ca| ca.fingerprint(HashAlg::Sha256));
        let ok = trusted.is_some_and(|fp| {
            certificate.cert_type() == CertType::User
                && certificate.validate([&fp]).is_ok()
                && certificate.valid_principals().iter().any(|p| p == user)
        });
        Ok(if ok {
            ServerAuth::Accept
        } else {
            ServerAuth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: &PublicKey,
    ) -> Result<ServerAuth, Self::Error> {
        // The key itself: a key on the wire has no comment.
        Ok(
            if self.policy.key.as_ref().map(PublicKey::key_data) == Some(key.key_data()) {
                ServerAuth::Accept
            } else {
                ServerAuth::reject()
            },
        )
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<ServerAuth, Self::Error> {
        // The code comes second, after the password.
        if self.policy.then_otp && !self.first_factor {
            return Ok(self.reject());
        }
        let (name, question, answer) =
            match (self.policy.pam, self.policy.password, self.policy.otp) {
                (true, Some(pw), _) => ("", "Password:", pw),
                (_, _, Some(otp)) => ("Two-factor", "Verification code: ", otp),
                _ => return Ok(ServerAuth::reject()),
            };
        Ok(match response {
            None => ServerAuth::Partial {
                name: name.into(),
                instructions: "".into(),
                prompts: vec![(question.into(), false)].into(),
            },
            Some(mut r) => {
                if r.next().as_deref() == Some(answer.as_bytes()) {
                    ServerAuth::Accept
                } else {
                    self.reject()
                }
            }
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        self.sessions.push(channel.id());
        self.opened.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        self.opened.remove(&channel);
        if self.policy.rogue
            && let Some(record) = self.policy.agent_reply.clone()
        {
            ask_agent(session.handle(), record);
        }
        session.channel_success(channel)?;
        session.data(channel, "welcome\r\n")
    }

    async fn agent_request(
        &mut self,
        _channel: ChannelId,
        session: &mut ServerSession,
    ) -> Result<bool, Self::Error> {
        if let Some(record) = self.policy.agent_reply.clone() {
            ask_agent(session.handle(), record);
        }
        Ok(true)
    }

    /// A command: run by `sh` with `policy.home` as `$HOME`, its input,
    /// output, errors and exit status over the channel.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        let opened = self.opened.remove(&channel);
        let (Some(home), Some(opened)) = (self.policy.home.clone(), opened) else {
            return session.channel_failure(channel);
        };
        let Ok(mut child) = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(String::from_utf8_lossy(data).into_owned())
            .env("HOME", &home)
            .current_dir(&home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        else {
            return session.channel_failure(channel);
        };
        self.sessions.retain(|c| *c != channel);
        session.channel_success(channel)?;
        let handle = session.handle();
        tokio::spawn(async move {
            let (stdin, stdout, stderr) =
                (child.stdin.take(), child.stdout.take(), child.stderr.take());
            let mut errors = opened.make_writer_ext(Some(1));
            let (mut from, mut to) = tokio::io::split(opened.into_stream());
            let up = async move {
                if let Some(mut stdin) = stdin {
                    let _ = tokio::io::copy(&mut from, &mut stdin).await;
                }
            };
            let down = async {
                if let Some(mut out) = stdout {
                    let _ = tokio::io::copy(&mut out, &mut to).await;
                }
            };
            let err = async {
                if let Some(mut e) = stderr {
                    let _ = tokio::io::copy(&mut e, &mut errors).await;
                }
            };
            tokio::join!(up, down, err);
            let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(1);
            let _ = handle.exit_status_request(channel, code as u32).await;
            let _ = to.shutdown().await;
            let _ = handle.close(channel).await;
        });
        Ok(())
    }

    /// `sftp`: OpenSSH's sftp-server, talking over the channel.
    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut ServerSession,
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
        // Its input is the server's from now on, not an echo.
        self.sessions.retain(|c| *c != channel);
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

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        if !self.sessions.contains(&channel) {
            return Ok(());
        }
        match data {
            // The shell exits in order.
            b"exit\r" => {
                session.exit_status_request(channel, 0)?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            // The server dies: the connection drops with nothing said.
            b"die\r" => return Err(russh::Error::Disconnect),
            _ => session.data(channel, data.to_vec())?,
        }
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
        _session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        if !self.policy.forward {
            return Ok(()); // dropping the handle refuses the channel
        }
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
        session: &mut ServerSession,
    ) -> Result<bool, Self::Error> {
        if !self.policy.forward {
            return Ok(false);
        }
        let Ok(listener) = TcpListener::bind(("127.0.0.1", *port as u16)).await else {
            return Ok(false);
        };
        *port = listener.local_addr().map_or(0, |a| a.port() as u32);
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

/// OpenSSH's sftp-server, where this system has one.
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

/// Starts a server on 127.0.0.1 and returns its port and host public key.
async fn serve(policy: Policy, methods: &[MethodKind]) -> (u16, PublicKey) {
    serve_as(policy, methods, None).await
}

/// Like [`serve`], with a host certificate `host_ca` signed when there is one.
async fn serve_as(
    policy: Policy,
    methods: &[MethodKind],
    host_ca: Option<&PrivateKey>,
) -> (u16, PublicKey) {
    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
    let public = host_key.public_key().clone();
    let certificates = host_ca
        .map(|ca| vec![certify(ca, &public, CertType::Host, "127.0.0.1")])
        .unwrap_or_default();
    let config = Arc::new(server::Config {
        methods: MethodSet::from(methods),
        keys: vec![host_key],
        certificates,
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let methods = methods.to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let (config, policy, methods) = (config.clone(), policy.clone(), methods.clone());
            tokio::spawn(async move {
                let handler = TestServer {
                    policy,
                    methods,
                    sessions: vec![],
                    opened: HashMap::new(),
                    first_factor: false,
                };
                if let Ok(running) = server::run_stream(config, socket, handler).await {
                    let _ = running.await;
                }
            });
        }
    });
    (port, public)
}

/// Answers prompts from a script and records what was asked.
fn scripted(answers: Vec<Option<&'static str>>) -> (Prompter, Arc<Mutex<Vec<Prompt>>>) {
    let (prompter, mut rx) = Prompter::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        let mut answers = answers.into_iter();
        while let Some(req) = rx.recv().await {
            log.lock().expect("log").push(req.prompt.clone());
            let answer = answers.next().unwrap_or(None).map(str::to_string);
            let _ = req.reply.send(answer);
        }
    });
    (prompter, asked)
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("shellwing-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        Fixture { dir }
    }

    fn known_hosts(&self) -> PathBuf {
        self.dir.join("known_hosts")
    }

    fn options(&self, target: Resolved, prompter: Prompter) -> Options {
        Options {
            target,
            auth: Auth::default(), // no agent, no stored password: everything is asked
            policy: HostKeyPolicy::Ask,
            known_hosts: Some(self.known_hosts()),
            prompter: Some(prompter),
            secrets: None,
            forwarded: None,
            keepalive: None,
            cols: 80,
            rows: 24,
            term: "xterm-256color".into(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn local(port: u16, user: &str) -> Resolved {
    Resolved::direct(Target {
        user: user.into(),
        host: "127.0.0.1".into(),
        port,
    })
}

/// Connects and waits for the shell's greeting.
async fn connect(opts: Options) -> Result<(), Error> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    let session = shellwing_ssh::connect(
        opts,
        Box::new(move |b| sink.lock().expect("out").extend_from_slice(b)),
        Box::new(|_| {}),
    )
    .await?;
    for _ in 0..200 {
        if out.lock().expect("out").windows(7).any(|w| w == b"welcome") {
            drop(session);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no greeting from the shell");
}

/// Connects, types `line` and returns how the session ended.
async fn end_with(opts: Options, line: &[u8]) -> shellwing_ssh::Ended {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let session = shellwing_ssh::connect(
        opts,
        Box::new(|_| {}),
        Box::new(move |ended| {
            if let Some(tx) = tx.lock().expect("tx").take() {
                let _ = tx.send(ended);
            }
        }),
    )
    .await
    .expect("connect");
    session.write(line.to_vec());
    let ended = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("ended in time")
        .expect("ended");
    drop(session);
    ended
}

fn kinds(asked: &Arc<Mutex<Vec<Prompt>>>) -> Vec<&'static str> {
    asked
        .lock()
        .expect("log")
        .iter()
        .map(|p| match p {
            Prompt::HostKey { again: false, .. } => "hostkey",
            Prompt::HostKey { again: true, .. } => "hostkey-again",
            Prompt::Password { retry: false, .. } => "password",
            Prompt::Password { retry: true, .. } => "password-retry",
            Prompt::Passphrase { retry: false, .. } => "passphrase",
            Prompt::Passphrase { retry: true, .. } => "passphrase-retry",
            Prompt::Question { .. } => "question",
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn new_host_is_asked_about_once_then_known() {
    let fx = Fixture::new("hostkey-yes");
    let (port, host_key) = serve(
        Policy {
            password: Some("secret"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;

    // An unclear answer is asked again, then "yes" records the key.
    let (p, asked) = scripted(vec![Some("maybe"), Some("yes"), Some("secret")]);
    connect(fx.options(local(port, "alice"), p))
        .await
        .expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "hostkey-again", "password"]);
    let fp = host_key
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    match &asked.lock().expect("log")[0] {
        Prompt::HostKey {
            fingerprint, host, ..
        } => {
            assert_eq!(fingerprint, &fp);
            assert_eq!(host, &format!("[127.0.0.1]:{port}"));
        }
        other => panic!("unexpected {other:?}"),
    }

    let (p, asked) = scripted(vec![Some("secret")]);
    connect(fx.options(local(port, "alice"), p))
        .await
        .expect("second connect");
    assert_eq!(
        kinds(&asked),
        ["password"],
        "a known host is not asked about again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refusing_the_host_key_sends_nothing() {
    let fx = Fixture::new("hostkey-no");
    let (port, _) = serve(
        Policy {
            password: Some("secret"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, asked) = scripted(vec![Some("no")]);
    let err = connect(fx.options(local(port, "alice"), p))
        .await
        .expect_err("refused");
    assert!(matches!(err, Error::HostKeyRejected { .. }), "got {err}");
    assert_eq!(
        kinds(&asked),
        ["hostkey"],
        "no password was asked for, so none was sent"
    );
    assert!(
        !fx.known_hosts().exists()
            || std::fs::read_to_string(fx.known_hosts())
                .expect("read")
                .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_password_is_asked_again() {
    let fx = Fixture::new("password");
    let (port, _) = serve(
        Policy {
            password: Some("secret"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, asked) = scripted(vec![Some("yes"), Some("wrong"), Some("secret")]);
    connect(fx.options(local(port, "alice"), p))
        .await
        .expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "password", "password-retry"]);
    let text = asked.lock().expect("log")[2].text();
    assert!(
        text.starts_with("Permission denied, please try again.\r\nalice@127.0.0.1's password: "),
        "{text:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn password_then_one_time_code() {
    // `AuthenticationMethods password,keyboard-interactive`: the password is
    // accepted in part, and the code is asked for after it.
    let fx = Fixture::new("password-then-otp");
    let (port, _) = serve(
        Policy {
            password: Some("hunter2"),
            otp: Some("123456"),
            then_otp: true,
            ..Default::default()
        },
        &[MethodKind::Password, MethodKind::KeyboardInteractive],
    )
    .await;
    let (p, asked) = scripted(vec![Some("yes"), Some("hunter2"), Some("123456")]);
    connect(fx.options(local(port, "alice"), p))
        .await
        .expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "password", "question"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_time_code_through_keyboard_interactive() {
    let fx = Fixture::new("otp");
    let (port, _) = serve(
        Policy {
            otp: Some("123456"),
            ..Default::default()
        },
        &[MethodKind::KeyboardInteractive],
    )
    .await;
    let (p, asked) = scripted(vec![Some("yes"), Some("123456")]);
    connect(fx.options(local(port, "alice"), p))
        .await
        .expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "question"]);
    match &asked.lock().expect("log")[1] {
        Prompt::Question {
            text, echo, name, ..
        } => {
            assert_eq!(
                (text.as_str(), *echo, name.as_str()),
                ("Verification code: ", false, "Two-factor")
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn encrypted_key_asks_for_its_passphrase() {
    let fx = Fixture::new("passphrase");
    let client = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    let encrypted = client
        .encrypt(&mut rand::rng(), "correct horse")
        .expect("encrypt");
    let key_file = fx.dir.join("id_ed25519");
    std::fs::write(
        &key_file,
        encrypted
            .to_openssh(LineEnding::LF)
            .expect("pem")
            .as_bytes(),
    )
    .expect("write");

    let (port, _) = serve(
        Policy {
            key: Some(client.public_key().clone()),
            ..Default::default()
        },
        &[MethodKind::PublicKey],
    )
    .await;
    let mut target = local(port, "alice");
    target.identity_files = vec![key_file];
    let (p, asked) = scripted(vec![Some("yes"), Some("wrong"), Some("correct horse")]);
    connect(fx.options(target, p)).await.expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "passphrase", "passphrase-retry"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_prompt_stops_the_connection() {
    let fx = Fixture::new("cancel");
    let (port, _) = serve(
        Policy {
            password: Some("secret"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, _) = scripted(vec![Some("yes"), None]);
    let err = connect(fx.options(local(port, "alice"), p))
        .await
        .expect_err("cancelled");
    assert!(matches!(err, Error::Cancelled), "got {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_jump_checks_and_authenticates_each_hop() {
    let fx = Fixture::new("jump");
    let (target_port, _) = serve(
        Policy {
            password: Some("inner"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (jump_port, _) = serve(
        Policy {
            password: Some("outer"),
            forward: true,
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;

    let mut target = local(target_port, "alice");
    target.jumps = vec![local(jump_port, "jumper")];
    // Jump host first (its key, its password), then the target through it.
    let (p, asked) = scripted(vec![Some("yes"), Some("outer"), Some("yes"), Some("inner")]);
    connect(fx.options(target, p))
        .await
        .expect("connect through the jump host");
    assert_eq!(
        kinds(&asked),
        ["hostkey", "password", "hostkey", "password"]
    );
    let targets: Vec<String> = asked
        .lock()
        .expect("log")
        .iter()
        .filter_map(|p| match p {
            Prompt::Password { target, .. } => Some(target.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(targets, ["jumper@127.0.0.1", "alice@127.0.0.1"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn methods_the_server_does_not_offer_are_not_asked_for() {
    let fx = Fixture::new("methods");
    // Key-only server and no key: no password or code should be asked for.
    let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    let (port, _) = serve(
        Policy {
            key: Some(other.public_key().clone()),
            ..Default::default()
        },
        &[MethodKind::PublicKey],
    )
    .await;
    let (p, asked) = scripted(vec![Some("yes")]);
    let mut target = local(port, "alice");
    target.identity_files = vec![fx.dir.join("missing_key")];
    let err = connect(fx.options(target, p))
        .await
        .expect_err("no usable method");
    assert!(matches!(err, Error::AuthFailed), "got {err}");
    assert_eq!(kinds(&asked), ["hostkey"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exit_closes_but_a_dying_server_disconnects() {
    let fx = Fixture::new("ending");
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let opts = || {
        let (prompter, _) = scripted(vec![Some("yes"), Some("pw"), Some("pw")]);
        fx.options(local(port, "me"), prompter)
    };
    assert!(matches!(
        end_with(opts(), b"exit\r").await,
        shellwing_ssh::Ended::Closed {
            exit_status: Some(0)
        }
    ));
    assert!(matches!(
        end_with(opts(), b"die\r").await,
        shellwing_ssh::Ended::Error(Error::Disconnected)
    ));
}

/// Saved secrets for a test, and what the connection reported as learned.
#[derive(Default)]
struct Saved {
    password: Option<String>,
    passphrase: Option<(String, String)>,
    learned: Mutex<Vec<shellwing_ssh::Learned>>,
    /// Keys the app keeps: id, and the key in OpenSSH's format.
    stored: Vec<(String, String)>,
}

impl shellwing_ssh::Secrets for Saved {
    fn password(&self, _target: &str) -> Option<String> {
        self.password.clone()
    }

    fn passphrase(&self, fingerprint: &str) -> Option<String> {
        self.passphrase
            .as_ref()
            .filter(|(fp, _)| fp == fingerprint)
            .map(|(_, p)| p.clone())
    }

    fn learned(&self, secret: shellwing_ssh::Learned) {
        self.learned.lock().expect("learned").push(secret);
    }

    fn keys(&self) -> Vec<shellwing_ssh::keys::StoredKey> {
        self.stored
            .iter()
            .map(|(id, text)| shellwing_ssh::keys::StoredKey {
                id: id.clone(),
                name: id.clone(),
                openssh: shellwing_ssh::keys::Zeroizing::new(text.clone()),
            })
            .collect()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_password_is_used_and_a_typed_one_reported() {
    let fx = Fixture::new("saved-password");
    let (port, _) = serve(
        Policy {
            password: Some("secret"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    // Saved and right: only the host key is asked.
    let saved = Arc::new(Saved {
        password: Some("secret".into()),
        ..Default::default()
    });
    let (p, asked) = scripted(vec![Some("yes")]);
    let mut opts = fx.options(local(port, "alice"), p);
    opts.secrets = Some(saved.clone());
    connect(opts).await.expect("connect");
    assert_eq!(kinds(&asked), ["hostkey"]);
    assert!(saved.learned.lock().expect("learned").is_empty());

    // Saved but stale: asked, and the typed one is reported under user@host:port.
    let stale = Arc::new(Saved {
        password: Some("old".into()),
        ..Default::default()
    });
    let (p, asked) = scripted(vec![Some("secret")]);
    let mut opts = fx.options(local(port, "alice"), p);
    opts.secrets = Some(stale.clone());
    connect(opts).await.expect("connect");
    assert_eq!(kinds(&asked), ["password"]);
    assert_eq!(
        *stale.learned.lock().expect("learned"),
        [shellwing_ssh::Learned::Password {
            target: format!("alice@127.0.0.1:{port}"),
            password: "secret".into(),
        }]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pam_password_is_reported_then_used_without_asking() {
    let fx = Fixture::new("pam-password");
    let (port, _) = serve(
        Policy {
            password: Some("secret"),
            pam: true,
            ..Default::default()
        },
        &[MethodKind::KeyboardInteractive],
    )
    .await;
    let learned = |s: &Saved| s.learned.lock().expect("learned").clone();
    let typed = [shellwing_ssh::Learned::Password {
        target: format!("alice@127.0.0.1:{port}"),
        password: "secret".into(),
    }];

    // Nothing saved: PAM's question is asked, and the answer reported like a password.
    let none = Arc::new(Saved::default());
    let (p, asked) = scripted(vec![Some("yes"), Some("secret")]);
    let mut opts = fx.options(local(port, "alice"), p);
    opts.secrets = Some(none.clone());
    connect(opts).await.expect("connect");
    assert_eq!(kinds(&asked), ["hostkey", "question"]);
    assert_eq!(learned(&none), typed);

    // Saved and right: it answers PAM, nothing is asked or reported.
    let saved = Arc::new(Saved {
        password: Some("secret".into()),
        ..Default::default()
    });
    let (p, asked) = scripted(vec![]);
    let mut opts = fx.options(local(port, "alice"), p);
    opts.secrets = Some(saved.clone());
    connect(opts).await.expect("connect");
    assert!(kinds(&asked).is_empty());
    assert!(learned(&saved).is_empty());

    // Saved but stale: asked again, and the typed one reported.
    let stale = Arc::new(Saved {
        password: Some("old".into()),
        ..Default::default()
    });
    let (p, asked) = scripted(vec![Some("secret")]);
    let mut opts = fx.options(local(port, "alice"), p);
    opts.secrets = Some(stale.clone());
    connect(opts).await.expect("connect");
    assert_eq!(kinds(&asked), ["question"]);
    assert_eq!(learned(&stale), typed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_saved_passphrase_unlocks_the_key_without_asking() {
    let fx = Fixture::new("saved-passphrase");
    let client = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    let fingerprint = client
        .public_key()
        .fingerprint(Default::default())
        .to_string();
    let encrypted = client
        .encrypt(&mut rand::rng(), "correct horse")
        .expect("encrypt");
    let key_file = fx.dir.join("id_ed25519");
    std::fs::write(
        &key_file,
        encrypted
            .to_openssh(LineEnding::LF)
            .expect("pem")
            .as_bytes(),
    )
    .expect("write");
    let (port, _) = serve(
        Policy {
            key: Some(client.public_key().clone()),
            ..Default::default()
        },
        &[MethodKind::PublicKey],
    )
    .await;
    let mut target = local(port, "alice");
    target.identity_files = vec![key_file];
    let saved = Arc::new(Saved {
        passphrase: Some((fingerprint, "correct horse".into())),
        ..Default::default()
    });
    let (p, asked) = scripted(vec![Some("yes")]);
    let mut opts = fx.options(target, p);
    opts.secrets = Some(saved);
    connect(opts).await.expect("connect");
    assert_eq!(kinds(&asked), ["hostkey"]);
}

/// A TCP server that sends back what it gets.
async fn echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = sock.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

/// Sends `msg` and expects it back.
async fn echoes(sock: &mut TcpStream, msg: &[u8]) {
    sock.write_all(msg).await.expect("write");
    let mut back = vec![0; msg.len()];
    tokio::time::timeout(Duration::from_secs(5), sock.read_exact(&mut back))
        .await
        .expect("echo in time")
        .expect("read");
    assert_eq!(back, msg);
}

struct Forwarding {
    session: shellwing_ssh::Session,
    reports: Vec<Result<Active, Error>>,
    ended: tokio::sync::oneshot::Receiver<()>,
    _fx: Fixture,
}

/// Connects to a server that allows forwarding, with `forwards` set up.
async fn forwarding(name: &str, allow: bool, forwards: &[String]) -> Forwarding {
    let fx = Fixture::new(name);
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            forward: allow,
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, _) = scripted(vec![Some("yes"), Some("pw")]);
    let mut target = local(port, "me");
    target.forwards = forwards.to_vec();
    let mut opts = fx.options(target, p);
    let reports = Arc::new(Mutex::new(vec![]));
    let log = reports.clone();
    opts.forwarded = Some(Box::new(move |r| log.lock().expect("log").push(r)));
    let (tx, ended) = tokio::sync::oneshot::channel();
    let session = shellwing_ssh::connect(
        opts,
        Box::new(|_| {}),
        Box::new(move |_| {
            let _ = tx.send(());
        }),
    )
    .await
    .expect("connect");
    let reports = std::mem::take(&mut *reports.lock().expect("log"));
    Forwarding {
        session,
        reports,
        ended,
        _fx: fx,
    }
}

fn port_of(r: &Result<Active, Error>) -> u16 {
    match r {
        Ok(a) => a.port,
        Err(e) => panic!("forward failed: {e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn local_forwards_carry_connections_and_close_with_the_session() {
    let to = echo().await;
    let f = forwarding("fwd-local", true, &[format!("-L 0:127.0.0.1:{to}")]).await;
    assert_eq!(f.reports.len(), 1);
    let port = port_of(&f.reports[0]);
    assert_ne!(port, 0, "the port it got, not the 0 asked for");
    // Two connections at once, each its own channel.
    let mut a = TcpStream::connect(("127.0.0.1", port)).await.expect("a");
    let mut b = TcpStream::connect(("127.0.0.1", port)).await.expect("b");
    echoes(&mut a, b"first").await;
    echoes(&mut b, b"second").await;
    echoes(&mut a, b"again").await;

    drop(f.session);
    f.ended.await.expect("ended");
    let mut closed = false;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(closed, "the port stays open after the session ended");
}

#[tokio::test(flavor = "multi_thread")]
async fn socks_connections_go_where_they_ask() {
    let to = echo().await;
    let f = forwarding("fwd-socks", true, &["-D 0".to_string()]).await;
    let port = port_of(&f.reports[0]);

    // SOCKS 5, with the destination as a name.
    let mut s = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("socks5");
    s.write_all(&[5, 1, 0]).await.expect("hello");
    let mut choice = [0u8; 2];
    s.read_exact(&mut choice).await.expect("method");
    assert_eq!(choice, [5, 0], "no authentication");
    let name = b"127.0.0.1";
    let mut req = vec![5, 1, 0, 3, name.len() as u8];
    req.extend_from_slice(name);
    req.extend_from_slice(&to.to_be_bytes());
    s.write_all(&req).await.expect("connect");
    let mut reply = [0u8; 10];
    s.read_exact(&mut reply).await.expect("reply");
    assert_eq!(reply[1], 0, "succeeded");
    echoes(&mut s, b"through socks5").await;

    // SOCKS 4, by address.
    let mut s = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("socks4");
    let mut req = vec![4, 1];
    req.extend_from_slice(&to.to_be_bytes());
    req.extend_from_slice(&[127, 0, 0, 1]);
    req.extend_from_slice(b"me\0");
    s.write_all(&req).await.expect("connect");
    let mut reply = [0u8; 8];
    s.read_exact(&mut reply).await.expect("reply");
    assert_eq!(reply[1], 0x5a, "granted");
    echoes(&mut s, b"through socks4").await;

    // Somewhere nothing listens: refused, as SOCKS says it. The port stays
    // bound (not listening), so no test running beside this one takes it.
    let closed = tokio::net::TcpSocket::new_v4().expect("socket");
    closed
        .bind("127.0.0.1:0".parse().expect("addr"))
        .expect("bind");
    let nowhere = closed.local_addr().expect("addr").port();
    let mut s = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("socks5");
    s.write_all(&[5, 1, 0]).await.expect("hello");
    s.read_exact(&mut choice).await.expect("method");
    let mut req = vec![5, 1, 0, 1, 127, 0, 0, 1];
    req.extend_from_slice(&nowhere.to_be_bytes());
    s.write_all(&req).await.expect("connect");
    s.read_exact(&mut reply[..2]).await.expect("reply");
    assert_ne!(reply[1], 0, "not reported as connected");
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_forwards_bring_the_servers_connections_here() {
    let to = echo().await;
    let f = forwarding("fwd-remote", true, &[format!("-R 0:127.0.0.1:{to}")]).await;
    let port = port_of(&f.reports[0]);
    assert_ne!(port, 0, "the port the server took");
    let mut s = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("server port");
    echoes(&mut s, b"from the server side").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn forwards_that_fail_are_reported_and_the_session_goes_on() {
    let taken = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let taken_port = taken.local_addr().expect("addr").port();
    let to = echo().await;
    let f = forwarding(
        "fwd-fail",
        false,
        &[
            format!("-L {taken_port}:127.0.0.1:{to}"),
            "-L nonsense".to_string(),
            format!("-R 0:127.0.0.1:{to}"),
        ],
    )
    .await;
    let errors: Vec<String> = f
        .reports
        .iter()
        .map(|r| r.as_ref().expect_err("should fail").to_string())
        .collect();
    assert!(errors[0].starts_with("cannot forward -L"), "{}", errors[0]);
    assert!(errors[1].starts_with("bad forward"), "{}", errors[1]);
    assert!(errors[2].contains("server refused"), "{}", errors[2]);

    // Asking for one later works the same way, here refused by the server too.
    let (reply, answer) = tokio::sync::oneshot::channel();
    let forward: Forward = format!("-L 0:127.0.0.1:{to}").parse().expect("spec");
    let _ = f.session.sender().send(Input::Forward { forward, reply });
    let active = answer.await.expect("answer").expect("a local port opens");
    // The server allows no direct-tcpip, so the connection is closed at once.
    let mut s = TcpStream::connect(("127.0.0.1", active.port))
        .await
        .expect("connect");
    let mut buf = [0u8; 1];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
        .await
        .expect("closed in time")
        .unwrap_or(0);
    assert_eq!(n, 0);
    drop(taken);
}

/// Connects with a password and opens SFTP.
async fn sftp_session(
    fx: &Fixture,
    serve_from: Option<PathBuf>,
) -> (shellwing_ssh::Session, Result<shellwing_ssh::Sftp, Error>) {
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            sftp: serve_from,
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, _) = scripted(vec![Some("yes"), Some("pw")]);
    let session = shellwing_ssh::connect(
        fx.options(local(port, "me"), p),
        Box::new(|_| {}),
        Box::new(|_| {}),
    )
    .await
    .expect("connect");
    let (reply, answer) = tokio::sync::oneshot::channel();
    let _ = session.sender().send(Input::Sftp { reply });
    let sftp = tokio::time::timeout(Duration::from_secs(10), answer)
        .await
        .expect("sftp in time")
        .expect("answer");
    (session, sftp)
}

#[tokio::test(flavor = "multi_thread")]
async fn files_are_listed_downloaded_and_uploaded_over_sftp() {
    use shellwing_ssh::sftp::{Progress, join};
    if sftp_server().is_none() {
        eprintln!("skipped: no sftp-server on this system");
        return;
    }
    let fx = Fixture::new("sftp");
    let remote = fx.dir.join("remote");
    let local = fx.dir.join("local");
    std::fs::create_dir_all(remote.join("sub/deeper")).expect("dirs");
    std::fs::create_dir_all(&local).expect("local");
    std::fs::write(remote.join("a.txt"), "hello").expect("a");
    // Bigger than the pipeline, so reads really overlap.
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(remote.join("sub/big.bin"), &big).expect("big");
    std::fs::write(remote.join("sub/deeper/c.txt"), "deep").expect("c");
    #[cfg(unix)]
    std::os::unix::fs::symlink("sub", remote.join("to-sub")).expect("link");

    let (_session, sftp) = sftp_session(&fx, Some(remote.clone())).await;
    let sftp = sftp.expect("sftp opens");
    let home = sftp.home().await.expect("home");
    assert_eq!(
        std::fs::canonicalize(&home).ok(),
        std::fs::canonicalize(&remote).ok()
    );

    let listed = sftp.list(&home).await.expect("list");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    #[cfg(unix)]
    {
        assert_eq!(names, ["sub", "to-sub", "a.txt"], "directories first");
        assert!(listed[1].kind == shellwing_ssh::sftp::Kind::Link && listed[1].is_dir());
    }
    #[cfg(not(unix))]
    assert_eq!(names, ["sub", "a.txt"]);
    assert_eq!(listed.last().map(|e| e.size), Some(5));
    assert!(
        sftp.stat(&join(&home, "nope"))
            .await
            .expect("stat")
            .is_none()
    );

    // A whole directory, with progress known up front.
    let progress = Progress::default();
    sftp.download(&join(&home, "sub"), &local.join("sub"), &progress)
        .await
        .expect("download dir");
    assert_eq!(std::fs::read(local.join("sub/big.bin")).expect("big"), big);
    assert_eq!(
        std::fs::read_to_string(local.join("sub/deeper/c.txt")).expect("c"),
        "deep"
    );
    assert_eq!(progress.get(), (3_000_004, 3_000_004));
    let parts = std::fs::read_dir(local.join("sub"))
        .expect("ls")
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".part"))
        })
        .count();
    assert_eq!(parts, 0, "no .part file left behind");

    // Uploading a directory, and a file over an existing one.
    std::fs::create_dir_all(local.join("up/nested")).expect("up");
    std::fs::write(local.join("up/x.txt"), "x").expect("x");
    std::fs::write(local.join("up/nested/y.txt"), "y").expect("y");
    std::fs::write(local.join("a.txt"), "replaced").expect("a");
    let progress = Progress::default();
    sftp.upload(&local.join("up"), &join(&home, "up"), &progress)
        .await
        .expect("upload dir");
    sftp.upload(&local.join("a.txt"), &join(&home, "a.txt"), &progress)
        .await
        .expect("upload file");
    assert_eq!(
        std::fs::read_to_string(remote.join("up/nested/y.txt")).expect("y"),
        "y"
    );
    assert_eq!(
        std::fs::read_to_string(remote.join("a.txt")).expect("a"),
        "replaced"
    );

    let gone = sftp
        .download(
            &join(&home, "nope"),
            &local.join("nope"),
            &Progress::default(),
        )
        .await
        .expect_err("nothing there");
    assert!(gone.to_string().contains("is gone"), "{gone}");
}

#[tokio::test(flavor = "multi_thread")]
async fn folders_are_made_renamed_and_removed_over_sftp() {
    use shellwing_ssh::sftp::join;
    if sftp_server().is_none() {
        eprintln!("skipped: no sftp-server on this system");
        return;
    }
    let fx = Fixture::new("sftp-manage");
    let remote = fx.dir.join("remote");
    std::fs::create_dir_all(remote.join("old/inner/deep")).expect("dirs");
    std::fs::write(remote.join("old/inner/deep/f.txt"), "f").expect("f");
    std::fs::write(remote.join("old/g.txt"), "g").expect("g");
    std::fs::write(remote.join("keep.txt"), "keep").expect("keep");
    #[cfg(unix)]
    std::os::unix::fs::symlink(remote.join("keep.txt"), remote.join("old/to-keep")).expect("link");

    let (_session, sftp) = sftp_session(&fx, Some(remote.clone())).await;
    let sftp = sftp.expect("sftp opens");
    let home = sftp.home().await.expect("home");

    sftp.mkdir(&join(&home, "new")).await.expect("mkdir");
    assert!(remote.join("new").is_dir());
    let again = sftp.mkdir(&join(&home, "new")).await.expect_err("there");
    assert!(again.to_string().contains("already there"), "{again}");

    sftp.rename(&join(&home, "old"), &join(&home, "renamed"))
        .await
        .expect("rename");
    assert!(remote.join("renamed/inner/deep/f.txt").is_file());
    let onto = sftp
        .rename(&join(&home, "renamed"), &join(&home, "keep.txt"))
        .await
        .expect_err("taken");
    assert!(onto.to_string().contains("already there"), "{onto}");

    // A whole tree goes; a link inside goes as a link, not what it points at.
    sftp.remove(&join(&home, "renamed"))
        .await
        .expect("remove dir");
    assert!(!remote.join("renamed").exists());
    assert_eq!(
        std::fs::read_to_string(remote.join("keep.txt")).expect("keep"),
        "keep"
    );
    sftp.remove(&join(&home, "keep.txt"))
        .await
        .expect("remove file");
    assert!(!remote.join("keep.txt").exists());
    sftp.remove(&join(&home, "nothing"))
        .await
        .expect("already gone is fine");
}

#[tokio::test(flavor = "multi_thread")]
async fn config_files_are_placed_whole_and_taken_back() {
    if sftp_server().is_none() {
        eprintln!("skipped: no sftp-server on this system");
        return;
    }
    let fx = Fixture::new("sftp-place");
    let remote = fx.dir.join("remote");
    std::fs::create_dir_all(&remote).expect("remote");
    std::fs::write(remote.join(".npmrc"), "theirs").expect("theirs");
    let (_session, sftp) = sftp_session(&fx, Some(remote.clone())).await;
    let sftp = sftp.expect("sftp opens");

    let kube = sftp.expand("~/.kube/config").await.expect("expand");
    assert!(kube.ends_with("/.kube/config"), "{kube}");
    assert!(sftp.read_file(&kube, 1024).await.expect("read").is_none());
    // Its directory made, 0700; the file 0600.
    sftp.place(&kube, b"apiVersion: v1\n", 0o600, None)
        .await
        .expect("placed");
    assert_eq!(
        std::fs::read(remote.join(".kube/config")).expect("kube"),
        b"apiVersion: v1\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |p: &std::path::Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&remote.join(".kube/config")), 0o600);
        assert_eq!(mode(&remote.join(".kube")), 0o700);
    }
    assert_eq!(
        sftp.read_file(&kube, 1024).await.expect("read").as_deref(),
        Some(&b"apiVersion: v1\n"[..])
    );
    assert!(sftp.read_file(&kube, 4).await.is_err(), "past the most");

    // Over a file there: what was there set aside, and put back.
    let npmrc = sftp.expand("~/.npmrc").await.expect("expand");
    let aside = format!("{npmrc}.shellwing-saved");
    sftp.place(&npmrc, b"ours", 0o600, Some(&aside))
        .await
        .expect("placed over");
    assert_eq!(std::fs::read(remote.join(".npmrc")).expect("ours"), b"ours");
    assert_eq!(
        std::fs::read(remote.join(".npmrc.shellwing-saved")).expect("aside"),
        b"theirs"
    );
    sftp.unplace(&npmrc, Some(&aside))
        .await
        .expect("taken back");
    assert_eq!(
        std::fs::read(remote.join(".npmrc")).expect("back"),
        b"theirs"
    );
    assert!(!remote.join(".npmrc.shellwing-saved").exists());
    // Over a file, not set aside: it is replaced, nothing left beside it.
    sftp.place(&npmrc, b"again", 0o600, None)
        .await
        .expect("replaced");
    sftp.unplace(&kube, None).await.expect("removed");
    assert!(!remote.join(".kube/config").exists());
    let left: Vec<String> = std::fs::read_dir(&remote)
        .expect("ls")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("shellwing"))
        .collect();
    assert!(left.is_empty(), "nothing left beside: {left:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_without_sftp_says_so() {
    let fx = Fixture::new("no-sftp");
    let (_session, sftp) = sftp_session(&fx, None).await;
    let e = sftp.err().expect("refused");
    assert!(e.to_string().contains("does not offer SFTP"), "{e}");
}

/// A stand-in ssh-agent on a Unix socket: answers every request with "no
/// identities" and counts the connections it served.
#[cfg(unix)]
fn fake_agent(path: &std::path::Path) -> Arc<std::sync::atomic::AtomicUsize> {
    let served = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = served.clone();
    let listener = tokio::net::UnixListener::bind(path).expect("agent socket");
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut len = [0u8; 4];
                while sock.read_exact(&mut len).await.is_ok() {
                    let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
                    if sock.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    // SSH_AGENT_IDENTITIES_ANSWER, no keys.
                    let _ = sock.write_all(&[0, 0, 0, 5, 12, 0, 0, 0, 0]).await;
                }
            });
        }
    });
    served
}

/// Connects with agent forwarding on or off and returns what the server's
/// question to the agent got.
#[cfg(unix)]
async fn agent_answer(name: &str, forward: bool, rogue: bool) -> (Result<Vec<u8>, String>, usize) {
    let fx = Fixture::new(name);
    let socket = fx.dir.join("agent.sock");
    let served = fake_agent(&socket);
    let record = Arc::new(Mutex::new(None));
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            agent_reply: Some(record.clone()),
            rogue,
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, _) = scripted(vec![Some("yes"), Some("pw")]);
    let mut target = local(port, "me");
    target.forward_agent = forward;
    let mut opts = fx.options(target, p);
    opts.auth.agent_socket = Some(socket);
    let session = shellwing_ssh::connect(opts, Box::new(|_| {}), Box::new(|_| {}))
        .await
        .expect("connect");
    for _ in 0..300 {
        if let Some(answer) = record.lock().expect("record").take() {
            drop(session);
            return (answer, served.load(std::sync::atomic::Ordering::SeqCst));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the server never heard back about the agent");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_forwarded_agent_answers_the_server() {
    let (answer, served) = agent_answer("agent-fwd", true, false).await;
    assert_eq!(
        answer,
        Ok(vec![12, 0, 0, 0, 0]),
        "the local agent's own reply"
    );
    assert_eq!(served, 1);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_server_cannot_reach_the_agent_unless_forwarding_is_on() {
    let (answer, served) = agent_answer("agent-rogue", false, true).await;
    assert!(
        answer.is_err(),
        "the agent channel must be refused: {answer:?}"
    );
    assert_eq!(served, 0, "the local agent was never contacted");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_can_take_new_keys_without_asking() {
    let fx = Fixture::new("host-key-policy");
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    // The app asks; this host is set to accept new keys, so only the password is asked.
    let (p, asked) = scripted(vec![Some("pw")]);
    let mut target = local(port, "me");
    target.host_key = Some(HostKeyPolicy::AcceptNew);
    connect(fx.options(target, p)).await.expect("connect");
    assert_eq!(kinds(&asked), ["password"]);
    assert!(
        std::fs::read_to_string(fx.known_hosts())
            .expect("known_hosts")
            .contains(&format!("[127.0.0.1]:{port}")),
        "the key was recorded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_the_app_keeps_signs_in_without_a_file() {
    use shellwing_ssh::keys::{KeyKind, generate, to_vault};
    let fx = Fixture::new("stored-key");
    let kept = generate(KeyKind::Ed25519, "kept").expect("key");
    let other = generate(KeyKind::Ed25519, "other").expect("key");
    let (port, _) = serve(
        Policy {
            key: Some(kept.public_key().clone()),
            ..Default::default()
        },
        &[MethodKind::PublicKey],
    )
    .await;
    let saved = Arc::new(Saved {
        stored: vec![
            ("other".into(), to_vault(&other).expect("pem").to_string()),
            ("kept".into(), to_vault(&kept).expect("pem").to_string()),
        ],
        ..Default::default()
    });
    let with = |key: &str, answers| {
        let mut target = local(port, "alice");
        target.vault_key = Some(key.into());
        let (p, asked) = scripted(answers);
        let mut opts = fx.options(target, p);
        opts.secrets = Some(saved.clone());
        (opts, asked)
    };
    let (opts, asked) = with("kept", vec![Some("yes")]);
    connect(opts).await.expect("the kept key signs in");
    assert_eq!(kinds(&asked), ["hostkey"], "nothing else was asked");
    // A host given another key offers only that one.
    let (opts, _) = with("other", vec![]);
    assert!(connect(opts).await.is_err());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_public_key_is_installed_beside_the_shell() {
    use shellwing_ssh::keys::{INSTALL, Installed, installed};
    let fx = Fixture::new("install");
    let home = fx.dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let (port, _) = serve(
        Policy {
            password: Some("pw"),
            home: Some(home.clone()),
            ..Default::default()
        },
        &[MethodKind::Password],
    )
    .await;
    let (p, _) = scripted(vec![Some("yes"), Some("pw")]);
    let session = shellwing_ssh::connect(
        fx.options(local(port, "me"), p),
        Box::new(|_| {}),
        Box::new(|_| {}),
    )
    .await
    .expect("connect");
    let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample me@here";
    let install = || async {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let _ = session.sender().send(Input::Exec {
            command: INSTALL.into(),
            stdin: format!("{line}\n").into_bytes(),
            reply,
        });
        let out = tokio::time::timeout(Duration::from_secs(10), answer)
            .await
            .expect("in time")
            .expect("answer")
            .expect("exec");
        installed(out.status, &out.stdout, &out.stderr)
    };
    assert_eq!(install().await, Ok(Installed::Added));
    assert_eq!(install().await, Ok(Installed::Present));
    let keys = std::fs::read_to_string(home.join(".ssh/authorized_keys")).expect("keys");
    assert_eq!(keys, format!("{line}\n"));
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: PathBuf| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(home.join(".ssh")), 0o700);
        assert_eq!(mode(home.join(".ssh/authorized_keys")), 0o600);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_certificate_from_a_trusted_authority_is_accepted_and_not_learned() {
    let fx = Fixture::new("hostcert");
    let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("ca");
    let policy = Policy {
        password: Some("pw"),
        ..Default::default()
    };
    let (port, _) = serve_as(policy, &[MethodKind::Password], Some(&ca)).await;
    let lines = format!(
        "@cert-authority [127.0.0.1]:* {}\n",
        ca.public_key().to_openssh().expect("ca line")
    );
    std::fs::write(fx.known_hosts(), &lines).expect("known_hosts");
    let (prompter, asked) = scripted(vec![Some("pw")]);
    let mut opts = fx.options(local(port, "u"), prompter);
    // Nothing known about the key itself, and nothing may be learned.
    opts.policy = HostKeyPolicy::Strict;
    connect(opts).await.expect("signed by a trusted authority");
    assert!(
        asked
            .lock()
            .expect("asked")
            .iter()
            .all(|p| !matches!(p, Prompt::HostKey { .. })),
        "never asked about the key"
    );
    assert_eq!(
        std::fs::read_to_string(fx.known_hosts()).expect("read"),
        lines
    );

    // Another authority than the one that signed it: refused.
    let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("other");
    std::fs::write(
        fx.known_hosts(),
        format!(
            "@cert-authority [127.0.0.1]:* {}\n",
            other.public_key().to_openssh().expect("line")
        ),
    )
    .expect("known_hosts");
    let (prompter, _) = scripted(vec![Some("pw")]);
    let mut opts = fx.options(local(port, "u"), prompter);
    opts.policy = HostKeyPolicy::Strict;
    let err = connect(opts)
        .await
        .expect_err("no trusted authority signed it");
    assert!(matches!(err, Error::HostCertificate { .. }), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_certificate_beside_the_key_signs_in() {
    let fx = Fixture::new("usercert");
    let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("ca");
    let policy = Policy {
        user_ca: Some(ca.public_key().clone()),
        ..Default::default()
    };
    let (port, _) = serve(policy, &[MethodKind::PublicKey]).await;
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key");
    let file = fx.dir.join("id_test");
    std::fs::write(
        &file,
        key.to_openssh(LineEnding::LF).expect("pem").as_bytes(),
    )
    .expect("key file");

    // The key alone is not enough: the server takes certificates only.
    let mut target = local(port, "deploy");
    target.identity_files = vec![file.clone()];
    let (prompter, _) = scripted(vec![]);
    let mut opts = fx.options(target.clone(), prompter);
    opts.policy = HostKeyPolicy::AcceptNew;
    assert!(connect(opts).await.is_err(), "no certificate yet");

    let cert = certify(&ca, key.public_key(), CertType::User, "deploy");
    let mut cert_file = file.as_os_str().to_owned();
    cert_file.push("-cert.pub");
    std::fs::write(&cert_file, cert.to_openssh().expect("cert")).expect("cert file");
    let (prompter, _) = scripted(vec![]);
    let mut opts = fx.options(target, prompter);
    opts.policy = HostKeyPolicy::AcceptNew;
    connect(opts).await.expect("signed in with the certificate");
}

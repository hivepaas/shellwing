//! End-to-end tests against an in-process russh server: host key questions,
//! password retries, one-time codes, encrypted keys, cancelling, and ProxyJump.
//! Keys are generated at run time; nothing secret lives in the repository.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::{Algorithm, LineEnding};
use russh::keys::{PrivateKey, PublicKey};
use russh::server::{self, Auth as ServerAuth, ChannelOpenHandle, Msg, Session as ServerSession};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use shellwing_ssh::{Auth, Error, HostKeyPolicy, Options, Prompt, Prompter, Resolved, Target};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Default)]
struct Policy {
    password: Option<&'static str>,
    otp: Option<&'static str>,
    key: Option<PublicKey>,
    /// Allow direct-tcpip, i.e. act as a jump host.
    forward: bool,
}

struct TestServer {
    policy: Policy,
    methods: Vec<MethodKind>,
    /// Session channels, whose input is echoed back (direct-tcpip channels are not).
    sessions: Vec<ChannelId>,
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
        Ok(if self.policy.password == Some(password) {
            ServerAuth::Accept
        } else {
            self.reject()
        })
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        key: &PublicKey,
    ) -> Result<ServerAuth, Self::Error> {
        Ok(if self.policy.key.as_ref() == Some(key) {
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
        Ok(if self.policy.key.as_ref() == Some(key) {
            ServerAuth::Accept
        } else {
            ServerAuth::reject()
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<ServerAuth, Self::Error> {
        let Some(otp) = self.policy.otp else {
            return Ok(ServerAuth::reject());
        };
        Ok(match response {
            None => ServerAuth::Partial {
                name: "Two-factor".into(),
                instructions: "".into(),
                prompts: vec![("Verification code: ".into(), false)].into(),
            },
            Some(mut r) => {
                if r.next().as_deref() == Some(otp.as_bytes()) {
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
        session.channel_success(channel)?;
        session.data(channel, "welcome\r\n")
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
}

/// Starts a server on 127.0.0.1 and returns its port and host public key.
async fn serve(policy: Policy, methods: &[MethodKind]) -> (u16, PublicKey) {
    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key");
    let public = host_key.public_key().clone();
    let config = Arc::new(server::Config {
        methods: MethodSet::from(methods),
        keys: vec![host_key],
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

//! End-to-end tests against real OpenSSH servers, which
//! `scripts/e2e_openssh.sh` starts in Docker and describes in the
//! environment: a bastion this machine reaches, and servers of several
//! OpenSSH versions behind it that only the bastion reaches. Hosts are
//! trusted through a host CA (`@cert-authority`), as an organization sets it
//! up, and checked strictly: nothing is asked unless a test says so.
//! Without the script's environment, each test says it was skipped.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use shellwing_ssh::forward::{Dest, Listen};
use shellwing_ssh::sftp::{Progress, join};
use shellwing_ssh::{
    Active, Auth, Ended, Error, Exec, Forward, HostKeyPolicy, Input, Options, Prompt, Prompter,
    Resolved, Session, Sftp, Target,
};
use tokio::io::AsyncReadExt;

/// The user every server has.
const USER: &str = "tester";

/// The servers the script started, and the keys it made.
struct Lab {
    dir: PathBuf,
    bastion: Target,
    /// The servers behind the bastion, by the host names it knows them by.
    targets: Vec<String>,
}

impl Lab {
    fn get() -> Option<Lab> {
        let dir = PathBuf::from(std::env::var_os("SHELLWING_E2E_DIR")?);
        let bastion = std::env::var("SHELLWING_E2E_BASTION").ok()?;
        let (host, port) = bastion.rsplit_once(':')?;
        let targets = std::env::var("SHELLWING_E2E_TARGETS").ok()?;
        Some(Lab {
            dir,
            bastion: Target {
                user: USER.into(),
                host: host.into(),
                port: port.parse().ok()?,
            },
            targets: targets.split(',').map(str::to_string).collect(),
        })
    }

    fn key(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn password(&self) -> String {
        std::fs::read_to_string(self.dir.join("server/password")).expect("password")
    }

    /// A scratch file of this run's.
    fn scratch(&self, name: &str) -> PathBuf {
        self.dir.join("home").join(name)
    }

    /// The bastion, signed in to with the ed25519 key.
    fn bastion(&self) -> Resolved {
        let mut r = Resolved::direct(self.bastion.clone());
        r.alias = "bastion".into();
        r.identity_files = vec![self.key("id_ed25519")];
        r
    }

    /// `host` through the bastion (ProxyJump), signed in to with `key`;
    /// None: no key file, so a password.
    fn behind(&self, host: &str, key: Option<&str>) -> Resolved {
        let mut r = Resolved::direct(Target {
            user: USER.into(),
            host: host.into(),
            port: 22,
        });
        r.jumps = vec![self.bastion()];
        r.identity_files = key.map(|k| vec![self.key(k)]).unwrap_or_default();
        r
    }

    fn options(&self, target: Resolved) -> Options {
        Options {
            target,
            auth: Auth::default(),
            policy: HostKeyPolicy::Strict,
            known_hosts: Some(self.dir.join("known_hosts")),
            prompter: None,
            secrets: None,
            forwarded: None,
            keepalive: None,
            size: None,
            cols: 80,
            rows: 24,
            term: "xterm-256color".into(),
        }
    }

    /// OpenSSH's own client to the bastion, as a ProxyCommand: `then`
    /// follows its options (`-W %h:%p`, or a command to run there). It reads
    /// nothing of this machine's and checks the bastion against the host CA.
    fn openssh(&self, then: &str) -> String {
        format!(
            "ssh -F none -i \"{}\" -p {} -o BatchMode=yes -o IdentitiesOnly=yes -o IdentityAgent=none -o StrictHostKeyChecking=yes -o UserKnownHostsFile=\"{}\" -o LogLevel=ERROR {USER}@{} {then}",
            self.key("id_ed25519").display(),
            self.bastion.port,
            self.dir.join("known_hosts").display(),
            self.bastion.host
        )
    }
}

/// The lab, or a skipped test.
macro_rules! lab {
    () => {
        match Lab::get() {
            Some(lab) => lab,
            None => {
                eprintln!("skipped: started by scripts/e2e_openssh.sh");
                return;
            }
        }
    };
}

/// Whether `program` is on PATH.
fn has(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A session, what its shell printed, and how it ended.
struct Shell {
    session: Session,
    out: Arc<Mutex<Vec<u8>>>,
    ended: tokio::sync::oneshot::Receiver<Ended>,
}

async fn open(opts: Options) -> Result<Shell, Error> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let sink = out.clone();
    let (tx, ended) = tokio::sync::oneshot::channel();
    let session = shellwing_ssh::connect(
        opts,
        Box::new(move |b| sink.lock().expect("out").extend_from_slice(b)),
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
    )
    .await?;
    Ok(Shell {
        session,
        out,
        ended,
    })
}

/// Connects, or fails the test naming `what`.
async fn open_or_fail(opts: Options, what: &str) -> Shell {
    match open(opts).await {
        Ok(s) => s,
        Err(e) => panic!("{what}: {e}"),
    }
}

impl Shell {
    fn send(&self, input: Input) {
        assert!(
            self.session.sender().send(input).is_ok(),
            "the session is gone"
        );
    }

    /// Runs `command` beside the shell.
    async fn run(&self, command: &str) -> Exec {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.send(Input::Exec {
            command: command.into(),
            stdin: vec![],
            reply,
        });
        rx.await.expect("reply").expect("exec")
    }

    /// `id -un` there: who it signed in as.
    async fn who(&self) -> String {
        let out = self.run("id -un").await;
        assert_eq!(
            out.status,
            Some(0),
            "{:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    async fn sftp(&self) -> Sftp {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.send(Input::Sftp { reply });
        rx.await.expect("reply").expect("sftp")
    }

    async fn forward(&self, forward: Forward) -> Active {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.send(Input::Forward { forward, reply });
        rx.await.expect("reply").expect("forward")
    }

    /// Waits for `text` in what the shell printed.
    async fn wait_for(&self, text: &str) {
        for _ in 0..500 {
            if String::from_utf8_lossy(&self.out.lock().expect("out")).contains(text) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let out = String::from_utf8_lossy(&self.out.lock().expect("out")).into_owned();
        panic!("{text:?} never came:\n{out}");
    }
}

/// Answers prompts with `answers` in turn and records what was asked.
fn scripted(answers: Vec<String>) -> (Prompter, Arc<Mutex<Vec<Prompt>>>) {
    let (prompter, mut rx) = Prompter::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        let mut answers = answers.into_iter();
        while let Some(req) = rx.recv().await {
            log.lock().expect("log").push(req.prompt.clone());
            let _ = req.reply.send(answers.next());
        }
    });
    (prompter, asked)
}

#[tokio::test(flavor = "multi_thread")]
async fn every_server_takes_each_kind_of_key() {
    let lab = lab!();
    for host in &lab.targets {
        for key in ["id_ed25519", "id_rsa", "id_ecdsa"] {
            let shell = open_or_fail(
                lab.options(lab.behind(host, Some(key))),
                &format!("{host} with {key}"),
            )
            .await;
            assert_eq!(shell.who().await, USER, "{host} with {key}");
        }
        // Which OpenSSH it was, for the log.
        let shell = open_or_fail(lab.options(lab.behind(host, Some("id_ed25519"))), host).await;
        let version = shell.run("ssh -V").await;
        eprintln!(
            "{host}: {}",
            String::from_utf8_lossy(&version.stderr).trim()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_is_asked_for_and_taken() {
    let lab = lab!();
    for host in &lab.targets {
        let (prompter, asked) = scripted(vec![lab.password()]);
        let mut opts = lab.options(lab.behind(host, None));
        opts.prompter = Some(prompter);
        let shell = open_or_fail(opts, host).await;
        assert_eq!(shell.who().await, USER, "{host}");
        let asked = asked.lock().expect("asked").clone();
        assert!(
            matches!(asked[..], [Prompt::Password { .. }]),
            "{host}: {asked:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_certificate_from_the_user_ca_signs_in() {
    let lab = lab!();
    // id_cert is in no authorized_keys: only its certificate, found beside
    // it as id_cert-cert.pub, lets it in.
    for host in &lab.targets {
        let shell = open_or_fail(lab.options(lab.behind(host, Some("id_cert"))), host).await;
        assert_eq!(shell.who().await, USER, "{host}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hosts_are_known_through_the_host_ca_alone() {
    let lab = lab!();
    open_or_fail(lab.options(lab.bastion()), "the bastion").await;
    let empty = lab.scratch("known_hosts_empty");
    std::fs::write(&empty, "").expect("known_hosts");
    let mut opts = lab.options(lab.bastion());
    opts.known_hosts = Some(empty);
    let err = open(opts)
        .await
        .err()
        .expect("connected to a host it does not know");
    assert!(matches!(err, Error::HostKeyUnknown { .. }), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_proxy_command_of_openssh_reaches_every_server() {
    let lab = lab!();
    if !has("ssh") {
        eprintln!("skipped: no OpenSSH client");
        return;
    }
    // `ssh -W` as people write it for a bastion, and the older way: nc run
    // on the bastion.
    for then in ["-W %h:%p", "nc %h %p"] {
        for host in &lab.targets {
            let mut r = lab.behind(host, Some("id_ed25519"));
            r.jumps.clear();
            r.proxy_command = Some(lab.openssh(then));
            let shell = open_or_fail(lab.options(r), &format!("{host} through {then}")).await;
            assert_eq!(shell.who().await, USER, "{host} through {then}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn files_go_back_and_forth_over_sftp() {
    let lab = lab!();
    let body: Vec<u8> = (0..300_000u32).map(|i| (i * 31 + i / 7) as u8).collect();
    for host in &lab.targets {
        let shell = open_or_fail(lab.options(lab.behind(host, Some("id_ed25519"))), host).await;
        let sftp = shell.sftp().await;
        let home = sftp.home().await.expect("home");
        let remote = join(&home, "e2e.bin");
        let up = lab.scratch(&format!("up-{host}.bin"));
        let down = lab.scratch(&format!("down-{host}.bin"));
        std::fs::write(&up, &body).expect("local file");
        sftp.upload(&up, &remote, &Progress::default())
            .await
            .expect("upload");
        let size = sftp.stat(&remote).await.expect("stat").map(|e| e.size);
        assert_eq!(size, Some(body.len() as u64), "{host}");
        sftp.download(&remote, &down, &Progress::default())
            .await
            .expect("download");
        assert!(
            std::fs::read(&down).expect("downloaded") == body,
            "{host}: not the same bytes"
        );
        sftp.remove(&remote).await.expect("remove");
        assert_eq!(sftp.stat(&remote).await.expect("stat"), None, "{host}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_local_forward_reaches_what_the_server_reaches() {
    let lab = lab!();
    for host in &lab.targets {
        let shell = open_or_fail(lab.options(lab.behind(host, Some("id_ed25519"))), host).await;
        // The server's own sshd, as it sees itself.
        let active = shell
            .forward(Forward::Local {
                listen: Listen {
                    host: None,
                    port: 0,
                },
                to: Dest {
                    host: "localhost".into(),
                    port: 22,
                },
            })
            .await;
        let mut sock = tokio::net::TcpStream::connect(("127.0.0.1", active.port))
            .await
            .expect("the forwarded port");
        let mut banner = [0u8; 16];
        sock.read_exact(&mut banner).await.expect("banner");
        assert_eq!(&banner, b"SSH-2.0-OpenSSH_", "{host}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shells_exit_status_comes_back() {
    let lab = lab!();
    for host in &lab.targets {
        let shell = open_or_fail(lab.options(lab.behind(host, Some("id_ed25519"))), host).await;
        shell.session.write(b"exit 7\n".to_vec());
        let ended = tokio::time::timeout(Duration::from_secs(10), shell.ended)
            .await
            .expect("the session to end")
            .expect("ended");
        assert!(
            matches!(
                ended,
                Ended::Closed {
                    exit_status: Some(7)
                }
            ),
            "{host}: {ended:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_forwarded_when_asked_for() {
    let lab = lab!();
    if !has("ssh-agent") {
        eprintln!("skipped: no ssh-agent");
        return;
    }
    let sock = lab.scratch("agent.sock");
    let mut agent = std::process::Command::new("ssh-agent")
        .arg("-D")
        .arg("-a")
        .arg(&sock)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("ssh-agent");
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let added = std::process::Command::new("ssh-add")
        .arg(lab.key("id_ecdsa"))
        .env("SSH_AUTH_SOCK", &sock)
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ssh-add");
    assert!(added.success());
    for host in &lab.targets {
        let mut r = lab.behind(host, Some("id_ed25519"));
        r.forward_agent = true;
        let mut opts = lab.options(r);
        opts.auth.agent_socket = Some(sock.clone());
        let shell = open_or_fail(opts, host).await;
        shell.session.write(b"ssh-add -l\n".to_vec());
        shell.wait_for("shellwing-e2e-id_ecdsa").await;
    }
    let _ = agent.kill();
    let _ = agent.wait();
}

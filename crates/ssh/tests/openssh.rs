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

/// An ssh-agent of the test's own, holding `keys` in that order; stopped
/// when dropped.
struct TestAgent {
    sock: PathBuf,
    child: std::process::Child,
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_agent(lab: &Lab, name: &str, keys: &[PathBuf]) -> TestAgent {
    let sock = lab.scratch(&format!("{name}.sock"));
    let _ = std::fs::remove_file(&sock);
    let child = std::process::Command::new("ssh-agent")
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
        std::thread::sleep(Duration::from_millis(20));
    }
    for key in keys {
        let added = std::process::Command::new("ssh-add")
            .arg(key)
            .env("SSH_AUTH_SOCK", &sock)
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ssh-add");
        assert!(added.success(), "ssh-add {}", key.display());
    }
    TestAgent { sock, child }
}

/// `n` keys no server knows, made once per run.
fn decoys(lab: &Lab, n: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let path = lab.scratch(&format!("decoy{i}"));
            if !path.exists() {
                let made = std::process::Command::new("ssh-keygen")
                    .args(["-q", "-t", "ed25519", "-N", "", "-C", "decoy", "-f"])
                    .arg(&path)
                    .status()
                    .expect("ssh-keygen");
                assert!(made.success());
            }
            path
        })
        .collect()
}

/// Signs in to the bastion as `lab`, which `config` (lines under its Host)
/// describes, with OpenSSH's own client and with shellwing: whether
/// OpenSSH got in, and how shellwing did.
async fn both_sign_in(lab: &Lab, name: &str, config: &str) -> (bool, Result<(), Error>) {
    let path = lab.scratch(&format!("{name}.ssh_config"));
    std::fs::write(
        &path,
        format!(
            "Host lab\n  HostName {}\n  Port {}\n  User {USER}\n{config}",
            lab.bastion.host, lab.bastion.port
        ),
    )
    .expect("ssh config");
    let openssh = std::process::Command::new("ssh")
        .arg("-F")
        .arg(&path)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
        ])
        .arg(format!(
            "UserKnownHostsFile={}",
            lab.dir.join("known_hosts").display()
        ))
        .args(["lab", "true"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let resolved = shellwing_ssh::config::resolve("lab", Some(&path)).expect("resolve");
    let mut opts = lab.options(resolved);
    // As the app signs in: the agent first.
    opts.auth.agent = true;
    (openssh, open(opts).await.map(|_| ()))
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_keys_are_offered_as_openssh_offers_them() {
    let lab = lab!();
    if !has("ssh-agent") || !has("ssh") {
        eprintln!("skipped: no OpenSSH client or ssh-agent");
        return;
    }
    // Seven keys the server does not know: more than its MaxAuthTries (6).
    let decoys = decoys(&lab, 7);
    let mut held = decoys.clone();
    held.push(lab.key("id_ed25519"));
    // The right key last, after the seven; and an agent with only the seven.
    let crowded = start_agent(&lab, "crowded", &held);
    let others = start_agent(&lab, "others", &decoys);
    let agent = |a: &TestAgent| format!("  IdentityAgent \"{}\"\n", a.sock.display());
    // The key's .pub alone, as 1Password and Secretive have it.
    let named = format!("  IdentityFile {}\n", lab.key("id_ed25519.pub").display());
    let on_disk = format!("  IdentityFile {}\n", lab.key("id_rsa").display());
    let only = "  IdentitiesOnly yes\n";
    let cases = [
        (
            "the agent's key IdentityFile names goes first",
            format!("{}{named}", agent(&crowded)),
            true,
        ),
        (
            "the agent's other keys use up the tries before a key file",
            format!("{}{on_disk}", agent(&others)),
            false,
        ),
        (
            "IdentitiesOnly keeps the agent's other keys out",
            format!("{}{on_disk}{only}", agent(&others)),
            true,
        ),
        (
            "IdentitiesOnly with the agent's key named",
            format!("{}{named}{only}", agent(&crowded)),
            true,
        ),
        (
            "IdentityAgent none: no agent to sign with",
            format!("  IdentityAgent none\n{named}"),
            false,
        ),
    ];
    for (i, (what, config, gets_in)) in cases.iter().enumerate() {
        let (openssh, ours) = both_sign_in(&lab, &format!("agent{i}"), config).await;
        assert_eq!(openssh, *gets_in, "OpenSSH, {what}");
        assert_eq!(ours.is_ok(), *gets_in, "shellwing, {what}: {ours:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_forwarded_when_asked_for() {
    let lab = lab!();
    if !has("ssh-agent") {
        eprintln!("skipped: no ssh-agent");
        return;
    }
    let agent = start_agent(&lab, "forwarded", &[lab.key("id_ecdsa")]);
    for host in &lab.targets {
        let mut r = lab.behind(host, Some("id_ed25519"));
        r.forward_agent = true;
        let mut opts = lab.options(r);
        opts.auth.agent_socket = Some(agent.sock.clone());
        let shell = open_or_fail(opts, host).await;
        shell.session.write(b"ssh-add -l\n".to_vec());
        shell.wait_for("shellwing-e2e-id_ecdsa").await;
    }
    // The agent IdentityAgent names is the one forwarded, as in ssh.
    let path = lab.scratch("forward.ssh_config");
    std::fs::write(
        &path,
        format!(
            "Host lab\n  HostName {}\n  Port {}\n  User {USER}\n  IdentityFile {}\n  IdentityAgent \"{}\"\n  ForwardAgent yes\n",
            lab.bastion.host,
            lab.bastion.port,
            lab.key("id_ed25519").display(),
            agent.sock.display()
        ),
    )
    .expect("ssh config");
    let resolved = shellwing_ssh::config::resolve("lab", Some(&path)).expect("resolve");
    let shell = open_or_fail(lab.options(resolved), "the bastion").await;
    shell.session.write(b"ssh-add -l\n".to_vec());
    shell.wait_for("shellwing-e2e-id_ecdsa").await;
}

/// What a session printed, and its exit status.
type Ran = (String, Option<u32>);

/// Runs `alias` from an ssh config of the lab's, with `more` under its own
/// Host, with OpenSSH's own client and with shellwing. Returns what each
/// printed (ssh's standard output) and its exit status. The servers are
/// behind the bastion through the config's ProxyJump.
async fn both_run(lab: &Lab, name: &str, alias: &str, more: &str) -> (Ran, Ran) {
    let path = lab.scratch(&format!("{name}.ssh_config"));
    std::fs::write(
        &path,
        format!(
            "Host {alias}\n{more}Host *\n  User {USER}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  UserKnownHostsFile {}\n  StrictHostKeyChecking yes\n  BatchMode yes\nHost bastion\n  HostName {}\n  Port {}\nHost {}\n  ProxyJump bastion\n",
            lab.key("id_ed25519").display(),
            lab.dir.join("known_hosts").display(),
            lab.bastion.host,
            lab.bastion.port,
            lab.targets.join(" ")
        ),
    )
    .expect("ssh config");
    let out = std::process::Command::new("ssh")
        .arg("-F")
        .arg(&path)
        .arg(alias)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .expect("ssh");
    let theirs = (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().map(|c| c as u32),
    );
    let resolved = shellwing_ssh::config::resolve(alias, Some(&path)).expect("resolve");
    let shell = open_or_fail(lab.options(resolved), alias).await;
    let ended = tokio::time::timeout(Duration::from_secs(20), shell.ended)
        .await
        .expect("the session to end")
        .expect("ended");
    let exit = match ended {
        Ended::Closed { exit_status } => exit_status,
        Ended::Error(e) => panic!("{alias}: {e}"),
    };
    let printed = String::from_utf8_lossy(&shell.out.lock().expect("out")).into_owned();
    (theirs, (printed, exit))
}

#[tokio::test(flavor = "multi_thread")]
async fn session_options_are_taken_as_openssh_takes_them() {
    let lab = lab!();
    if !has("ssh") {
        eprintln!("skipped: no OpenSSH client");
        return;
    }
    for host in &lab.targets {
        // A variable from here (SendEnv), one from the config (SetEnv), and
        // a command instead of the shell, with its tokens.
        let ((theirs, their_exit), (ours, our_exit)) = both_run(
            &lab,
            &format!("env-{host}"),
            host,
            "  SendEnv SHELLWING_E2E_SENT\n  SetEnv SHELLWING_SET=\"set in config\"\n  RemoteCommand echo \"[$SHELLWING_E2E_SENT|$SHELLWING_SET|%r@%n]\"; tty; exit 3\n",
        )
        .await;
        let line = format!("[sent by this machine|set in config|{USER}@{host}]");
        assert!(theirs.contains(&line), "OpenSSH, {host}: {theirs:?}");
        assert!(ours.contains(&line), "shellwing, {host}: {ours:?}");
        assert_eq!((their_exit, our_exit), (Some(3), Some(3)), "{host}");
        // shellwing is a terminal, so the command gets one. ssh here gets
        // none, its input not being one (RequestTTY auto).
        assert!(ours.contains("/dev/pts/"), "{host}: {ours:?}");
        assert!(theirs.contains("not a tty"), "{host}: {theirs:?}");

        // RequestTTY no: no terminal; the lines still start at the left,
        // and what goes to standard error is shown too.
        let ((theirs, _), (ours, our_exit)) = both_run(
            &lab,
            &format!("notty-{host}"),
            host,
            "  RequestTTY no\n  RemoteCommand tty; echo second; echo oops >&2\n",
        )
        .await;
        assert!(
            theirs.contains("not a tty\nsecond\n"),
            "OpenSSH, {host}: {theirs:?}"
        );
        assert!(
            ours.contains("not a tty\r\nsecond\r\n"),
            "shellwing, {host}: {ours:?}"
        );
        assert!(ours.contains("oops"), "shellwing, {host}: {ours:?}");
        assert_eq!(our_exit, Some(0), "{host}");
    }
}

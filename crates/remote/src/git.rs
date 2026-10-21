//! Syncing through a Git repository, with the system's own `git`: it signs
//! in as the user does (Git Credential Manager, the keychain, `gh`, SSH keys
//! and the agent), and needs nothing built into shellwing but a few
//! commands.
//!
//! The clone on this device is a cache, made again if lost; the vault holds
//! what matters. A round: fetch, and reset to what the repository has (no
//! Git merge ever: the engine merges, field by field); the engine reads and
//! writes the files; commit and push. A push refused because another device
//! pushed first starts the round again over theirs (see
//! `shellwing_vault::transact`).

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use shellwing_spawn::Unblocked;
use shellwing_vault::{Backend, Folder};

/// The oldest git taken: 2.31 reads settings from the environment
/// (`GIT_CONFIG_COUNT`), which keeps a token out of the command line.
pub const MIN_VERSION: (u32, u32, u32) = (2, 31, 0);

/// A command that takes longer is stopped: a fetch or a push of a few
/// small files does not.
const TIMEOUT: Duration = Duration::from_secs(90);

/// A refresh this soon after the last one, with nothing written since,
/// fetches nothing new: pairing and the sync look several times a minute.
const FRESH_FOR: Duration = Duration::from_secs(2);

/// The `git` of this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Git {
    exe: PathBuf,
    version: String,
}

/// Why there is no git to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoGit {
    /// Not installed; how to install it here.
    Missing(Install),
    /// Older than [`MIN_VERSION`]: the version it is.
    TooOld(String),
    /// There, but it does not run: what it said.
    Broken(String),
}

/// How git is installed on this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// macOS: Apple's Command Line Tools, through the system's own dialog.
    CommandLineTools,
    /// Windows: winget, which asks for the rights itself.
    Winget,
    /// Windows without winget: the download page.
    Download(&'static str),
    /// Linux and the rest: a command to run in a terminal (it needs sudo).
    Command(String),
}

impl std::fmt::Display for NoGit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (a, b, c) = MIN_VERSION;
        match self {
            NoGit::Missing(_) => write!(f, "git is not installed"),
            NoGit::TooOld(v) => write!(f, "git {v} is too old: {a}.{b}.{c} or newer is needed"),
            NoGit::Broken(e) => write!(f, "git does not work here: {e}"),
        }
    }
}

impl Git {
    /// The system's git, looked for where installers put it too: an app
    /// started from the desktop does not get a shell's `PATH`.
    pub fn find() -> Result<Git, NoGit> {
        let mut broken = None;
        for exe in candidates() {
            match version_of(&exe) {
                Ok(v) if parse_version(&v) >= Some(MIN_VERSION) => {
                    return Ok(Git { exe, version: v });
                }
                Ok(v) => broken = broken.or(Some(NoGit::TooOld(v))),
                Err(e) => broken = broken.or(Some(NoGit::Broken(e))),
            }
        }
        Err(broken.unwrap_or_else(|| NoGit::Missing(install_here())))
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// A git command, run where nothing can be typed: no terminal prompt
    /// (a credential manager's own window still shows), messages in
    /// English to tell errors apart, and on Windows no console window.
    fn command(&self, dir: Option<&Path>) -> Command {
        let mut c = Command::new(&self.exe);
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
        if let Some(d) = dir {
            c.current_dir(d);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        c
    }
}

/// Where git may be, in order: `PATH`, then where installers put it.
fn candidates() -> Vec<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let mut found: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(name)).collect())
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        found.extend(["/opt/homebrew/bin/git", "/usr/local/bin/git"].map(PathBuf::from));
        // /usr/bin/git without the Command Line Tools is a stub that opens
        // Apple's install dialog: only asked when the tools are there.
        if !command_line_tools() {
            found.retain(|p| p != Path::new("/usr/bin/git"));
        } else {
            found.push("/usr/bin/git".into());
        }
    }
    if cfg!(windows) {
        for base in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(b) = std::env::var_os(base) {
                let b = PathBuf::from(b);
                found.push(b.join("Git").join("cmd").join(name));
                found.push(b.join("Programs").join("Git").join("cmd").join(name));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    found.retain(|p| p.is_file() && seen.insert(p.clone()));
    found
}

/// Whether Apple's Command Line Tools (or Xcode) are installed.
fn command_line_tools() -> bool {
    Command::new("xcode-select")
        .arg("-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status_unblocked()
        .is_ok_and(|s| s.success())
}

/// `git --version`'s number.
fn version_of(exe: &Path) -> Result<String, String> {
    let git = Git {
        exe: exe.to_path_buf(),
        version: String::new(),
    };
    let out = run(git.command(None).arg("--version"), Duration::from_secs(20))
        .map_err(|e| e.to_string())?;
    if !out.ok {
        return Err(first_line(&out.stderr));
    }
    // "git version 2.43.0" or "git version 2.39.3 (Apple Git-146)".
    out.stdout
        .split_whitespace()
        .nth(2)
        .map(str::to_string)
        .ok_or_else(|| first_line(&out.stdout))
}

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut n = v.split(['.', '-']).map(|p| p.parse::<u32>().ok());
    Some((n.next()??, n.next()??, n.next().flatten().unwrap_or(0)))
}

/// How git is installed here.
fn install_here() -> Install {
    if cfg!(target_os = "macos") {
        return Install::CommandLineTools;
    }
    if cfg!(windows) {
        let winget = Command::new("winget")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status_unblocked()
            .is_ok_and(|s| s.success());
        return if winget {
            Install::Winget
        } else {
            Install::Download("https://git-scm.com/download/win")
        };
    }
    let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    Install::Command(linux_install(&os).to_string())
}

/// The command that installs git on the distribution `/etc/os-release`
/// describes.
fn linux_install(os_release: &str) -> &'static str {
    let ids: Vec<String> = os_release
        .lines()
        .filter_map(|l| {
            l.strip_prefix("ID=")
                .or_else(|| l.strip_prefix("ID_LIKE="))
                .map(|v| v.trim_matches('"').to_lowercase())
        })
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();
    let is = |name: &str| ids.iter().any(|i| i == name);
    if is("debian") || is("ubuntu") {
        "sudo apt install git"
    } else if is("fedora") || is("rhel") || is("centos") {
        "sudo dnf install git"
    } else if is("arch") {
        "sudo pacman -S git"
    } else if is("opensuse") || is("suse") {
        "sudo zypper install git"
    } else if is("alpine") {
        "sudo apk add git"
    } else if is("nixos") {
        "nix-env -iA nixpkgs.git"
    } else {
        "install git with your system's package manager"
    }
}

/// Starts installing git where the system can ask for the rights itself:
/// Apple's dialog on macOS, winget (and its UAC prompt) on Windows. Done
/// when that is; the caller looks for git again after.
pub fn install(how: &Install) -> io::Result<()> {
    let mut c = match how {
        Install::CommandLineTools => {
            let mut c = Command::new("xcode-select");
            c.arg("--install");
            c
        }
        Install::Winget => {
            let mut c = Command::new("winget");
            c.args([
                "install",
                "--id",
                "Git.Git",
                "-e",
                "--source",
                "winget",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]);
            c
        }
        Install::Download(_) | Install::Command(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "git is installed by hand here",
            ));
        }
    };
    let out = run(
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        Duration::from_secs(30 * 60),
    )?;
    if out.ok {
        Ok(())
    } else {
        Err(io::Error::other(first_line(
            if out.stderr.trim().is_empty() {
                &out.stdout
            } else {
                &out.stderr
            },
        )))
    }
}

/// What a command printed, and whether it succeeded.
struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// Runs `c`, stopped after `limit`.
fn run(c: &mut Command, limit: Duration) -> io::Result<Output> {
    // A group of its own, so that stopping it stops what it started too
    // (ssh, a credential helper), which would otherwise live on.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(c, 0);
    let mut child = c.spawn_unblocked()?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    // Read as it comes, so a full pipe never stops the command.
    let reading_out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let reading_err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    eprintln!("DIAG git spawned {:?} pid={}", c.get_args().collect::<Vec<_>>(), child.id());
    let status = loop {
        if let Some(status) = child.try_wait()? {
            eprintln!("DIAG git exited {status:?} after {:?}", start.elapsed());
            break status;
        }
        if start.elapsed() > limit {
            #[cfg(unix)]
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", child.id())])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "git took too long and was stopped",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = reading_out.join().unwrap_or_default();
    eprintln!("DIAG git stdout read after {:?}", start.elapsed());
    let stderr = reading_err.join().unwrap_or_default();
    eprintln!("DIAG git stderr read after {:?}: {stderr:?}", start.elapsed());
    Ok(Output {
        ok: status.success(),
        stdout,
        stderr,
    })
}

fn first_line(s: &str) -> String {
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no message");
    crate::short(line)
}

/// What a failed git command said, in a line.
fn failed(stderr: &str) -> io::Error {
    let says = |needles: &[&str]| needles.iter().any(|n| stderr.contains(n));
    // The line that says it, without git's "fatal: " and "error: ".
    let detail = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("hint:"))
        .map(|l| {
            l.trim_start_matches("fatal: ")
                .trim_start_matches("error: ")
                .to_string()
        })
        .unwrap_or_default();
    let (kind, text) = if says(&["Host key verification failed"]) {
        (
            io::ErrorKind::PermissionDenied,
            "SSH does not know the server's key yet: connect to it once with ssh (in a terminal, or in shellwing) and accept it".to_string(),
        )
    } else if says(&["Permission denied (publickey"]) {
        (
            io::ErrorKind::PermissionDenied,
            "the server refused this device's SSH key: add it to your account there, or load it in your SSH agent".to_string(),
        )
    } else if says(&[
        "Authentication failed",
        "could not read Username",
        "could not read Password",
        "terminal prompts disabled",
        "returned error: 401",
        "returned error: 403",
        "Invalid username or password",
    ]) {
        (
            io::ErrorKind::PermissionDenied,
            "the repository refused the sign-in: sign in with git once (or set a token), and check it may push there".to_string(),
        )
    } else if says(&[
        "Repository not found",
        "does not appear to be a git repository",
        "returned error: 404",
        "not found",
    ]) {
        (
            io::ErrorKind::NotFound,
            "no such repository, or this account cannot see it".to_string(),
        )
    } else if says(&[
        "Could not resolve host",
        "Connection refused",
        "Connection timed out",
        "Network is unreachable",
        "Failed to connect",
        "Could not connect",
    ]) {
        (
            io::ErrorKind::Other,
            format!("the server cannot be reached: {}", crate::short(&detail)),
        )
    } else {
        (
            io::ErrorKind::Other,
            format!("git: {}", crate::short(&detail)),
        )
    };
    io::Error::new(kind, text)
}

/// A Git repository the vault syncs through, by its clone on this device.
pub struct GitRepo {
    git: Git,
    /// The clone.
    dir: PathBuf,
    /// The files in it, as a folder backend has them.
    folder: Folder,
    branch: String,
    /// Settings for each command: a token, how SSH runs.
    env: Vec<(String, String)>,
    device: String,
    /// See [`FRESH_FOR`].
    fresh_for: Duration,
    state: Mutex<RepoState>,
}

#[derive(Default)]
struct RepoState {
    /// Written since the last refresh, not published.
    pending: bool,
    /// When the clone last was what the repository has.
    fresh: Option<Instant>,
}

impl GitRepo {
    /// The repository at `url`, by its clone in `dir` (made, or made again,
    /// if not one of it). `branch` empty: the repository's own default.
    /// `token` (for HTTPS) goes as a header, from the environment; without
    /// one, git signs in as it does for the user.
    pub fn open(
        git: Git,
        dir: &Path,
        url: &str,
        branch: &str,
        token: Option<(&str, &str)>,
        device: &str,
    ) -> io::Result<GitRepo> {
        let mut env = vec![];
        if let Some((user, token)) = token {
            use base64::Engine as _;
            let basic = base64::engine::general_purpose::STANDARD.encode(format!(
                "{}:{token}",
                if user.is_empty() {
                    "x-access-token"
                } else {
                    user
                }
            ));
            env.extend([
                ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
                ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
                (
                    "GIT_CONFIG_VALUE_0".into(),
                    format!("Authorization: Basic {basic}"),
                ),
            ]);
        }
        let mut repo = GitRepo {
            git,
            dir: dir.to_path_buf(),
            folder: Folder::new(dir),
            branch: branch.trim().to_string(),
            env,
            device: device.to_string(),
            fresh_for: FRESH_FOR,
            state: Mutex::default(),
        };
        repo.prepare(url)?;
        Ok(repo)
    }

    /// Makes the clone (an empty repository with the remote: the first
    /// refresh fetches), and finds the branch.
    fn prepare(&mut self, url: &str) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        if !self.dir.join(".git").is_dir() {
            self.git(&["-c", "init.defaultBranch=main", "init", "-q"])?;
        }
        if self.git(&["remote", "set-url", "origin", url]).is_err() {
            self.git(&["remote", "add", "origin", url])?;
        }
        // SSH must not ask for a password or a host key where nobody can
        // answer, unless the user set how SSH runs for git.
        let own_ssh = std::env::var_os("GIT_SSH_COMMAND").is_some()
            || std::env::var_os("GIT_SSH").is_some()
            || self.git(&["config", "--get", "core.sshCommand"]).is_ok();
        if !own_ssh {
            self.env
                .push(("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes".into()));
        }
        if self.branch.is_empty() {
            // "ref: refs/heads/main	HEAD"; nothing for an empty repository.
            let head = self.git(&["ls-remote", "--symref", "origin", "HEAD"])?;
            self.branch = head
                .lines()
                .find_map(|l| l.strip_prefix("ref: refs/heads/"))
                .and_then(|l| l.split_whitespace().next())
                .unwrap_or("main")
                .to_string();
        }
        Ok(())
    }

    /// Runs git in the clone; what it printed, or what went wrong.
    fn git(&self, args: &[&str]) -> io::Result<String> {
        let mut c = self.git.command(Some(&self.dir));
        c.args(args);
        for (k, v) in &self.env {
            c.env(k, v);
        }
        let out = run(&mut c, TIMEOUT)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(failed(&out.stderr))
        }
    }

    /// Like [`GitRepo::git`], but a failure is its stderr, to look at.
    fn git_said(&self, args: &[&str]) -> io::Result<Result<String, String>> {
        let mut c = self.git.command(Some(&self.dir));
        c.args(args);
        for (k, v) in &self.env {
            c.env(k, v);
        }
        let out = run(&mut c, TIMEOUT)?;
        Ok(if out.ok {
            Ok(out.stdout)
        } else {
            Err(out.stderr)
        })
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    fn written(&self) -> io::Result<()> {
        self.state.lock().map_err(crate::poisoned)?.pending = true;
        Ok(())
    }
}

impl Backend for GitRepo {
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        self.folder.list(dir)
    }

    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        self.folder.read(name)
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        self.folder.write(name, data)?;
        self.written()
    }

    fn remove(&self, name: &str) -> io::Result<()> {
        self.folder.remove(name)?;
        self.written()
    }

    fn refresh(&self) -> io::Result<()> {
        let mut state = self.state.lock().map_err(crate::poisoned)?;
        if !state.pending && state.fresh.is_some_and(|t| t.elapsed() < self.fresh_for) {
            return Ok(());
        }
        let theirs = format!("refs/remotes/origin/{}", self.branch);
        let fetched = self.git_said(&[
            "fetch",
            "-q",
            "--depth",
            "1",
            "origin",
            &format!("+refs/heads/{}:{theirs}", self.branch),
        ])?;
        match fetched {
            Ok(_) => {
                self.git(&["reset", "-q", "--hard", &theirs])?;
            }
            // An empty repository, or one without the branch yet: nothing
            // of the vault there.
            Err(e) if e.contains("couldn't find remote ref") => {
                self.git(&[
                    "rm",
                    "-r",
                    "-q",
                    "--cached",
                    "--ignore-unmatch",
                    "--",
                    "shellwing",
                ])?;
                let _ = std::fs::remove_dir_all(self.dir.join("shellwing"));
            }
            Err(e) => return Err(failed(&e)),
        }
        self.git(&["clean", "-fdxq", "--", "shellwing"])?;
        state.pending = false;
        state.fresh = Some(Instant::now());
        Ok(())
    }

    fn publish(&self) -> io::Result<bool> {
        let mut state = self.state.lock().map_err(crate::poisoned)?;
        if !state.pending {
            return Ok(true);
        }
        self.git(&["add", "-A", "--", "shellwing"])?;
        // Written back as it was: nothing to commit.
        if self.git(&["diff", "--cached", "--quiet"]).is_ok() {
            state.pending = false;
            return Ok(true);
        }
        let device: String = self
            .device
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let email = format!("user.email=shellwing@{device}");
        self.git(&[
            "-c",
            "user.name=shellwing",
            "-c",
            &email,
            // The user's own settings must not get in the way: no signing
            // prompt, no hooks of theirs.
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            &format!("Sync from {}", self.device),
        ])?;
        let pushed = self.git_said(&[
            "push",
            "-q",
            "--no-verify",
            "origin",
            &format!("HEAD:refs/heads/{}", self.branch),
        ])?;
        match pushed {
            Ok(_) => {
                state.pending = false;
                state.fresh = Some(Instant::now());
                Ok(true)
            }
            // Another device pushed first: its commits come at the refresh.
            Err(e)
                if e.contains("[rejected]")
                    || e.contains("non-fast-forward")
                    || e.contains("fetch first")
                    || e.contains("[remote rejected]") && e.contains("cannot lock ref") =>
            {
                Ok(false)
            }
            Err(e) => Err(failed(&e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_distributions_are_read() {
        assert_eq!(parse_version("2.43.0"), Some((2, 43, 0)));
        assert_eq!(parse_version("2.39.3"), Some((2, 39, 3)));
        assert_eq!(parse_version("2.45.1.windows.1"), Some((2, 45, 1)));
        assert_eq!(parse_version("2.31"), Some((2, 31, 0)));
        assert!(parse_version("2.30.9") < Some(MIN_VERSION));
        assert_eq!(
            linux_install("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"),
            "sudo apt install git"
        );
        assert_eq!(
            linux_install("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"),
            "sudo dnf install git"
        );
        assert_eq!(
            linux_install("ID=endeavouros\nID_LIKE=arch\n"),
            "sudo pacman -S git"
        );
    }

    #[test]
    fn git_errors_are_said_in_a_line() {
        let e = failed(
            "remote: Repository not found.\nfatal: repository 'https://github.com/a/b.git/' not found\n",
        );
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        let e = failed(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
        );
        assert!(e.to_string().contains("refused the sign-in"), "{e}");
        let e = failed(
            "Host key verification failed.\nfatal: Could not read from remote repository.\n",
        );
        assert!(e.to_string().contains("server's key"), "{e}");
        let e = failed("fatal: unable to access 'https://x/': Could not resolve host: x\n");
        assert!(
            e.to_string().starts_with("the server cannot be reached"),
            "{e}"
        );
        let e = failed("hint: something\nfatal: something odd\n");
        assert_eq!(e.to_string(), "git: something odd");
    }

    use shellwing_vault::{MemoryKeys, Vault};

    /// A bare repository in `dir`, as a server's would be, and clones of it
    /// for two devices.
    fn two_clones(dir: &Path) -> (GitRepo, GitRepo) {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("dir");
        let git = Git::find().expect("git, to run these tests");
        let bare = dir.join("server.git");
        let made = run(
            git.command(Some(dir))
                .args(["-c", "init.defaultBranch=main", "init", "-q", "--bare"])
                .arg(&bare),
            TIMEOUT,
        )
        .expect("init");
        assert!(made.ok, "{}", made.stderr);
        let url = bare.display().to_string();
        let mut a =
            GitRepo::open(git.clone(), &dir.join("a"), &url, "", None, "laptop").expect("a");
        let mut b = GitRepo::open(git, &dir.join("b"), &url, "", None, "desktop").expect("b");
        // Each round fetches: the devices take turns faster than people do.
        a.fresh_for = Duration::ZERO;
        b.fresh_for = Duration::ZERO;
        assert_eq!(a.branch(), "main", "an empty repository: main");
        (a, b)
    }

    #[test]
    fn two_devices_sync_through_a_repository() {
        let dir = std::env::temp_dir().join(format!("shellwing-git-{}", std::process::id()));
        let (a_side, b_side) = two_clones(&dir);
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        let rk = shellwing_vault::transact(&a_side, |s| a.start_sync(s))
            .expect("start")
            .to_string();
        a.sync(&a_side).expect("sync a");
        let state = shellwing_vault::transact(&b_side, |s| b.folder_state(s)).expect("state");
        assert_eq!(state, shellwing_vault::FolderState::Other);
        shellwing_vault::transact(&b_side, |s| b.join(s, &rk)).expect("join");
        b.set_password("root@db", "from-b").expect("save");
        b.sync(&b_side).expect("sync b");
        a.sync(&a_side).expect("sync a again");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("from-a"));
        assert_eq!(pw(&a, "root@db").as_deref(), Some("from-b"));
        // Nothing new: no commit.
        let log = |r: &GitRepo| r.git(&["rev-list", "--count", "HEAD"]).expect("log");
        let before = log(&a_side);
        a.sync(&a_side).expect("idle");
        assert_eq!(log(&a_side), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clone where another device pushes between this one's round and its
    /// push.
    struct Racing<'a> {
        inner: &'a GitRepo,
        meanwhile: Mutex<Option<Box<dyn FnOnce() + Send + 'a>>>,
    }

    impl Backend for Racing<'_> {
        fn list(&self, dir: &str) -> io::Result<Vec<String>> {
            self.inner.list(dir)
        }
        fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
            self.inner.read(name)
        }
        fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
            self.inner.write(name, data)
        }
        fn remove(&self, name: &str) -> io::Result<()> {
            self.inner.remove(name)
        }
        fn refresh(&self) -> io::Result<()> {
            self.inner.refresh()
        }
        fn publish(&self) -> io::Result<bool> {
            if let Some(push_theirs) = self.meanwhile.lock().expect("lock").take() {
                push_theirs();
            }
            self.inner.publish()
        }
    }

    #[test]
    fn a_push_refused_is_done_again_over_theirs() {
        let dir = std::env::temp_dir().join(format!("shellwing-git-race-{}", std::process::id()));
        let (a_side, b_side) = two_clones(&dir);
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_password("deploy@web", "first").expect("save");
        let rk = shellwing_vault::transact(&a_side, |s| a.start_sync(s))
            .expect("start")
            .to_string();
        a.sync(&a_side).expect("sync a");
        shellwing_vault::transact(&b_side, |s| b.join(s, &rk)).expect("join");
        b.sync(&b_side).expect("sync b");

        a.set_password("deploy@web", "from-a").expect("edit a");
        a.set_password("other@web", "only-a").expect("new on a");
        std::thread::sleep(Duration::from_millis(5));
        b.set_password("deploy@web", "from-b").expect("edit b");
        let racing = Racing {
            inner: &a_side,
            meanwhile: Mutex::new(Some(Box::new(|| {
                b.sync(&b_side).expect("b pushes first");
            }))),
        };
        a.sync(&racing).expect("sync a");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(
            pw(&a, "deploy@web").as_deref(),
            Some("from-b"),
            "B's newer edit"
        );
        b.sync(&b_side).expect("sync b");
        assert_eq!(
            pw(&b, "other@web").as_deref(),
            Some("only-a"),
            "A's still went"
        );
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("from-b"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two devices that synced note "Disk usage" through a repository, then
    /// changed its text without seeing each other's change, B later.
    fn note_changed_on_both(dir: &Path, a_side: &GitRepo, b_side: &GitRepo) -> (Vault, Vault) {
        use crate::tests::write_note;
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_device_name("desk");
        b.set_device_name("laptop");
        let note = write_note(&a, "", "df -h");
        let rk = shellwing_vault::transact(a_side, |s| a.start_sync(s))
            .expect("start")
            .to_string();
        a.sync(a_side).expect("sync a");
        shellwing_vault::transact(b_side, |s| b.join(s, &rk)).expect("join");
        b.sync(b_side).expect("sync b");
        write_note(&a, &note, "df -h /var");
        std::thread::sleep(Duration::from_millis(5));
        write_note(&b, &note, "du -sh /var/log");
        (a, b)
    }

    #[test]
    fn a_note_changed_on_two_devices_keeps_both_texts() {
        use crate::tests::{both_kept, notes_of};
        let dir = std::env::temp_dir().join(format!("shellwing-git-note-{}", std::process::id()));
        let (a_side, b_side) = two_clones(&dir);
        let (a, b) = note_changed_on_both(&dir, &a_side, &b_side);
        assert!(a.sync(&a_side).expect("sync a").copies.is_empty());
        let report = b.sync(&b_side).expect("sync b");
        assert_eq!(report.copies, ["Disk usage (from desk)"]);
        assert!(
            a.sync(&a_side).expect("sync a").copies.is_empty(),
            "made once"
        );
        assert_eq!(notes_of(&a), both_kept());
        assert_eq!(notes_of(&b), both_kept());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_made_in_a_round_whose_push_was_refused_is_kept_and_said() {
        use crate::tests::{both_kept, notes_of};
        let dir =
            std::env::temp_dir().join(format!("shellwing-git-note-race-{}", std::process::id()));
        let (a_side, b_side) = two_clones(&dir);
        let (a, b) = note_changed_on_both(&dir, &a_side, &b_side);
        // B's text is out first: A finds the loss, then B pushes again
        // before A does.
        b.sync(&b_side).expect("sync b");
        let racing = Racing {
            inner: &a_side,
            meanwhile: Mutex::new(Some(Box::new(|| {
                b.set_password("root@db", "from-b").expect("save");
                b.sync(&b_side).expect("b pushes first");
            }))),
        };
        let report = a.sync(&racing).expect("sync a");
        assert_eq!(report.copies, ["Disk usage (from desk)"]);
        b.sync(&b_side).expect("sync b");
        assert_eq!(notes_of(&a), both_kept());
        assert_eq!(notes_of(&b), both_kept());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

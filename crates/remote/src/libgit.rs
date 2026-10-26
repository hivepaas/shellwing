//! Syncing through a Git repository with libgit2, where there is no `git`
//! to run: on a phone. It does what [`crate::GitRepo`] does with the
//! system's git, the same files in the same repository, so a phone and a
//! computer sync through one: fetch, and reset to what the repository has
//! (no Git merge: the engine merges); the engine reads and writes the files;
//! commit and push, and a push refused because another device pushed first
//! starts the round again over theirs.
//!
//! Only HTTPS, signed in with a token: a phone has no SSH agent, nor the
//! sign-ins a computer's git has.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use git2::build::CheckoutBuilder;
use git2::{
    Cred, Direction, ErrorClass, ErrorCode, FetchOptions, IndexAddOption, ObjectType, PushOptions,
    RemoteCallbacks, Repository, ResetType, Signature, Time,
};
use shellwing_vault::{Backend, Folder};

/// See `git.rs`: a refresh this soon after the last, with nothing written,
/// fetches nothing new.
const FRESH_FOR: Duration = Duration::from_secs(2);

/// The folder of the vault in the repository, as `git.rs` has it.
const VAULT_DIR: &str = "shellwing";

/// A Git repository the vault syncs through, by its clone on this device.
pub struct LibGitRepo {
    dir: PathBuf,
    folder: Folder,
    url: String,
    branch: String,
    /// `user` (or `x-access-token`) and the token, for HTTPS.
    token: Option<(String, String)>,
    /// This device in commits: an id, never its name.
    device: String,
    pub(crate) fresh_for: Duration,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Written since the last refresh, not published.
    pending: bool,
    /// When the clone last was what the repository has.
    fresh: Option<Instant>,
}

impl LibGitRepo {
    /// The repository at `url`, by its clone in `dir` (made, or made again,
    /// if not one of it). `branch` empty: the repository's own default.
    pub fn open(
        dir: &Path,
        url: &str,
        branch: &str,
        token: Option<(&str, &str)>,
        device: &str,
    ) -> io::Result<LibGitRepo> {
        let url = url.trim();
        if !(url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("file://"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "on this device a repository is reached over HTTPS, with a token: its https:// address",
            ));
        }
        let mut repo = LibGitRepo {
            dir: dir.to_path_buf(),
            folder: Folder::new(dir),
            url: url.to_string(),
            branch: branch.trim().to_string(),
            token: token.map(|(u, t)| {
                let user = if u.is_empty() { "x-access-token" } else { u };
                (user.to_string(), t.to_string())
            }),
            device: device.to_string(),
            fresh_for: FRESH_FOR,
            state: Mutex::default(),
        };
        repo.prepare()?;
        Ok(repo)
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Makes the clone (an empty repository with the remote: the first
    /// refresh fetches), and finds the branch.
    fn prepare(&mut self) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let repo = match Repository::open(&self.dir) {
            Ok(r) => r,
            Err(_) => {
                let mut opts = git2::RepositoryInitOptions::new();
                opts.initial_head("main");
                Repository::init_opts(&self.dir, &opts).map_err(said)?
            }
        };
        if repo.find_remote("origin").is_ok() {
            repo.remote_set_url("origin", &self.url).map_err(said)?;
        } else {
            repo.remote("origin", &self.url).map_err(said)?;
        }
        if self.branch.is_empty() {
            let mut remote = repo.find_remote("origin").map_err(said)?;
            let found = {
                let conn = remote
                    .connect_auth(Direction::Fetch, Some(self.callbacks()), None)
                    .map_err(said)?;
                // An empty repository has no HEAD to follow.
                conn.default_branch()
                    .ok()
                    .and_then(|b| b.as_str().map(str::to_string))
                    .and_then(|b| b.strip_prefix("refs/heads/").map(str::to_string))
            };
            self.branch = found.unwrap_or_else(|| "main".to_string());
        }
        Ok(())
    }

    fn repo(&self) -> io::Result<Repository> {
        Repository::open(&self.dir).map_err(said)
    }

    /// Signing in with the token, if there is one.
    fn callbacks(&self) -> RemoteCallbacks<'_> {
        let mut cb = RemoteCallbacks::new();
        let mut tried = false;
        cb.credentials(move |_url, _user, allowed| {
            // Asked again after a refusal: it would only be refused again.
            if tried {
                return Err(git2::Error::from_str("the repository refused the sign-in"));
            }
            tried = true;
            match &self.token {
                Some((user, token))
                    if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) =>
                {
                    Cred::userpass_plaintext(user, token)
                }
                _ => Err(git2::Error::from_str(
                    "the repository asks to sign in: set a token that may push there",
                )),
            }
        });
        cb
    }

    fn written(&self) -> io::Result<()> {
        self.state.lock().map_err(crate::poisoned)?.pending = true;
        Ok(())
    }

    /// The vault's folder made what the repository has: fetched, and the
    /// clone reset to it (what was not there removed).
    fn reset_to_theirs(&self, repo: &Repository) -> io::Result<()> {
        let theirs = format!("refs/remotes/origin/{}", self.branch);
        let mut remote = repo.find_remote("origin").map_err(said)?;
        let mut fetch = FetchOptions::new();
        // Pruned: a branch gone there goes from here too. Not shallow, as
        // git.rs fetches: libgit2 pushes from a shallow clone unreliably,
        // and after the first fetch only what is new comes.
        fetch
            .remote_callbacks(self.callbacks())
            .prune(git2::FetchPrune::On);
        let refspec = format!("+refs/heads/{}:{theirs}", self.branch);
        remote
            .fetch(&[refspec.as_str()], Some(&mut fetch), None)
            .map_err(said)?;
        // Not there: an empty repository, or one without the branch yet.
        let found = repo.find_reference(&theirs).is_ok();
        let mut checkout = CheckoutBuilder::new();
        checkout.force().remove_untracked(true).remove_ignored(true);
        if found {
            let commit = repo
                .find_reference(&theirs)
                .and_then(|r| r.peel(ObjectType::Commit))
                .map_err(said)?;
            repo.reset(&commit, ResetType::Hard, Some(&mut checkout))
                .map_err(said)?;
        } else {
            // Nothing of the vault there: none of it here either.
            let mut index = repo.index().map_err(said)?;
            index.remove_dir(Path::new(VAULT_DIR), 0).map_err(said)?;
            index.write().map_err(said)?;
            let _ = std::fs::remove_dir_all(self.dir.join(VAULT_DIR));
        }
        Ok(())
    }

    /// Commits what changed in the vault's folder; false when nothing did.
    fn commit(&self, repo: &Repository) -> io::Result<bool> {
        let mut index = repo.index().map_err(said)?;
        index
            .add_all([VAULT_DIR], IndexAddOption::DEFAULT, None)
            .map_err(said)?;
        // Files removed here go from the index too.
        index.update_all([VAULT_DIR], None).map_err(said)?;
        index.write().map_err(said)?;
        let tree_id = index.write_tree().map_err(said)?;
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        // Written back as it was: nothing to commit.
        if parent.as_ref().is_some_and(|p| p.tree_id() == tree_id) {
            return Ok(false);
        }
        let tree = repo.find_tree(tree_id).map_err(said)?;
        let device: String = self
            .device
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        // Commits in UTC: a local time would say where the device is.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sig = Signature::new(
            "shellwing",
            &format!("shellwing@{device}"),
            &Time::new(now, 0),
        )
        .map_err(said)?;
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("Sync from {}", self.device),
            &tree,
            &parents,
        )
        .map_err(said)?;
        Ok(true)
    }

    /// Pushes the clone's HEAD to the branch: false when another device
    /// pushed first.
    fn push(&self, repo: &Repository) -> io::Result<bool> {
        let head = repo.head().map_err(said)?;
        let local = head
            .name()
            .ok_or_else(|| io::Error::other("the clone's HEAD has no name"))?
            .to_string();
        let mut remote = repo.find_remote("origin").map_err(said)?;
        let refused = std::cell::Cell::new(false);
        let mut cb = self.callbacks();
        cb.push_update_reference(|_name, status| {
            if status.is_some() {
                refused.set(true);
            }
            Ok(())
        });
        let mut push = PushOptions::new();
        push.remote_callbacks(cb);
        let refspec = format!("{local}:refs/heads/{}", self.branch);
        match remote.push(&[refspec.as_str()], Some(&mut push)) {
            Ok(()) if refused.get() => Ok(false),
            Ok(()) => Ok(true),
            Err(e) if e.code() == ErrorCode::NotFastForward => Ok(false),
            Err(e) if e.class() == ErrorClass::Reference && e.message().contains("fast") => {
                Ok(false)
            }
            Err(e) => Err(said(e)),
        }
    }
}

impl Backend for LibGitRepo {
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
        self.reset_to_theirs(&self.repo()?)?;
        state.pending = false;
        state.fresh = Some(Instant::now());
        Ok(())
    }

    fn publish(&self) -> io::Result<bool> {
        let mut state = self.state.lock().map_err(crate::poisoned)?;
        if !state.pending {
            return Ok(true);
        }
        let repo = self.repo()?;
        if !self.commit(&repo)? {
            state.pending = false;
            return Ok(true);
        }
        let pushed = self.push(&repo)?;
        if pushed {
            state.pending = false;
            state.fresh = Some(Instant::now());
        }
        Ok(pushed)
    }
}

/// libgit2's error, said as `git.rs` says git's.
fn said(e: git2::Error) -> io::Error {
    let m = e.message().to_string();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    let (kind, text) = if e.code() == ErrorCode::Auth
        || has(&[
            "401",
            "403",
            "authentication",
            "sign-in",
            "sign in",
            "Unauthorized",
        ]) {
        (
            io::ErrorKind::PermissionDenied,
            "the repository refused the sign-in: check the token, and that it may push there"
                .to_string(),
        )
    } else if e.code() == ErrorCode::NotFound && e.class() == ErrorClass::Http
        || has(&["404", "not found"])
    {
        (
            io::ErrorKind::NotFound,
            "no such repository, or this account cannot see it".to_string(),
        )
    } else if matches!(
        e.class(),
        ErrorClass::Net | ErrorClass::Os | ErrorClass::Ssl
    ) {
        (
            io::ErrorKind::Other,
            format!("the server cannot be reached: {}", crate::short(&m)),
        )
    } else {
        (io::ErrorKind::Other, format!("git: {}", crate::short(&m)))
    };
    io::Error::new(kind, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::{MemoryKeys, Vault};

    /// A bare repository standing for the server, and its `file://` address.
    fn server(dir: &Path) -> String {
        let bare = dir.join("server.git");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.bare(true).initial_head("main");
        Repository::init_opts(&bare, &opts).expect("bare");
        format!("file://{}", bare.display())
    }

    /// A clone that always fetches: two of them take turns in a test.
    fn clone_of(dir: &Path, url: &str, branch: &str, device: &str) -> LibGitRepo {
        let mut r = LibGitRepo::open(dir, url, branch, None, device).expect("clone");
        r.fresh_for = Duration::ZERO;
        r
    }

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("shellwing-libgit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    #[test]
    fn only_https_is_offered() {
        let dir = temp("ssh");
        let e = LibGitRepo::open(&dir.join("c"), "git@github.com:me/vault.git", "", None, "d")
            .err()
            .expect("refused");
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_go_round_two_clones() {
        let dir = temp("round");
        let url = server(&dir);
        let a = clone_of(&dir.join("a"), &url, "", "dev-a");
        assert_eq!(a.branch(), "main", "an empty repository: main");
        a.refresh().expect("refresh empty");
        a.write("shellwing/records/one", b"1").expect("write");
        assert!(a.publish().expect("publish"));

        let b = clone_of(&dir.join("b"), &url, "main", "dev-b");
        b.refresh().expect("refresh");
        assert_eq!(
            b.read("shellwing/records/one").expect("read").as_deref(),
            Some(&b"1"[..])
        );
        b.remove("shellwing/records/one").expect("remove");
        b.write("shellwing/records/two", b"2").expect("write");
        assert!(b.publish().expect("publish"));

        // A clone written before the other's push: refused, then done over.
        a.write("shellwing/records/three", b"3").expect("write");
        assert!(!a.publish().expect("refused"), "pushed over another device");
        a.refresh().expect("refresh");
        assert!(
            a.read("shellwing/records/one").expect("read").is_none(),
            "gone there"
        );
        assert!(
            a.read("shellwing/records/three").expect("read").is_none(),
            "reset to theirs"
        );
        a.write("shellwing/records/three", b"3").expect("write");
        assert!(a.publish().expect("publish"));
        b.refresh().expect("refresh");
        let mut names = b.list("shellwing/records").expect("list");
        names.sort();
        assert_eq!(names, ["three", "two"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_vaults_sync_through_it() {
        let dir = temp("vaults");
        let url = server(&dir);
        let a_side = clone_of(&dir.join("a"), &url, "", "phone");
        let b_side = clone_of(&dir.join("b"), &url, "", "laptop");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        let rk = shellwing_vault::transact(&a_side, |s| a.start_sync(s))
            .expect("start")
            .to_string();
        a.sync(&a_side).expect("sync a");
        shellwing_vault::transact(&b_side, |s| b.join(s, &rk)).expect("join");
        b.set_password("root@db", "from-b").expect("save");
        b.sync(&b_side).expect("sync b");
        a.sync(&a_side).expect("sync a again");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("from-a"));
        assert_eq!(pw(&a, "root@db").as_deref(), Some("from-b"));
        // Each commit by its device's id, in UTC.
        let repo = Repository::open(dir.join("a")).expect("clone");
        let head = repo.head().expect("head").peel_to_commit().expect("commit");
        let who = head.author();
        assert_eq!(who.name(), Some("shellwing"));
        assert!(
            who.email()
                .is_some_and(|e| e == "shellwing@phone" || e == "shellwing@laptop")
        );
        assert_eq!(who.when().offset_minutes(), 0);
        // Nothing new: no commit.
        a.sync(&a_side).expect("idle");
        let again = Repository::open(dir.join("a")).expect("clone");
        assert_eq!(again.head().expect("head").target(), Some(head.id()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

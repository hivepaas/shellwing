//! Places to sync the vault with over the network, WebDAV and S3-compatible
//! storage (through OpenDAL) and Git repositories (through the system's
//! `git`, see [`git`]), beside the folder on disk. Files go under
//! `shellwing/` in the place given, as the folder backend puts them.
//!
//! The engine reads every record at each sync. Over the network that would
//! be a request per record every minute, so a remote keeps what it last
//! read with the ETag the listing gave it, and reads again only what
//! changed.
//!
//! The same ETags make writes conditional: a record is written only if no
//! other device wrote it since this one looked (`If-Match`, or
//! `If-None-Match: *` for a new one). S3 does it through OpenDAL; WebDAV,
//! which OpenDAL writes without conditions, through a `PUT` of our own.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opendal::{ErrorKind, Operator, blocking, layers, services};
use shellwing_vault::{Backend, Folder};

pub mod git;
pub use git::{Git, GitRepo, Install, NoGit};

/// Where the vault syncs. Secrets (a password, a secret key) are in it:
/// not `Debug`, so they are never printed by accident.
#[derive(Clone, PartialEq, Eq)]
pub enum Place {
    Folder(PathBuf),
    /// `url` is the folder's address, such as Nextcloud's
    /// `https://cloud.example.com/remote.php/dav/files/me/`.
    WebDav {
        url: String,
        user: String,
        password: String,
    },
    /// AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi: the endpoint (for
    /// AWS, `https://s3.<region>.amazonaws.com`), a bucket, and a key with
    /// access to it. `prefix` is a folder in the bucket, or empty.
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        prefix: String,
        access_key: String,
        secret_key: String,
    },
    /// A Git repository: `https://…` or SSH (`git@host:you/vault.git`).
    /// `branch` empty: its default. `token` empty: git signs in as it does
    /// for the user; else as `user` (empty: any) with the token. `device`
    /// names the commits.
    Git {
        url: String,
        branch: String,
        user: String,
        token: String,
        /// How this device signs its commits: an id, not its name, since
        /// others may read the repository.
        device: String,
    },
}

impl Place {
    /// How the place is named to the user: the folder, or the server and
    /// what is in it.
    pub fn describe(&self) -> String {
        match self {
            Place::Folder(f) => f.display().to_string(),
            Place::WebDav { url, user, .. } => format!("{user} at {url} (WebDAV)"),
            Place::S3 {
                endpoint,
                bucket,
                prefix,
                ..
            } => {
                let host = host_of(endpoint);
                match prefix.trim_matches('/') {
                    "" => format!("{bucket} on {host} (S3)"),
                    p => format!("{bucket}/{p} on {host} (S3)"),
                }
            }
            Place::Git { url, branch, .. } => {
                let repo = url
                    .trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_start_matches("ssh://")
                    .trim_end_matches('/')
                    .trim_end_matches(".git");
                match branch.trim() {
                    "" => format!("{repo} (Git)"),
                    b => format!("{repo}, {b} (Git)"),
                }
            }
        }
    }

    /// The backend that reads and writes there. Making a remote one does not
    /// reach the server yet.
    pub fn open(&self) -> io::Result<Arc<dyn Backend>> {
        match self {
            Place::Folder(f) => Ok(Arc::new(Folder::new(f))),
            Place::WebDav {
                url,
                user,
                password,
            } => {
                let (origin, path) = split_url(url)?;
                let root = format!("{}/shellwing/", path.trim_end_matches('/'));
                let builder = services::Webdav::default()
                    .endpoint(origin)
                    .root(&root)
                    .username(user)
                    .password(password);
                let dav = DavPut {
                    base: format!("{origin}{root}"),
                    user: user.clone(),
                    password: password.clone(),
                };
                Remote::open(Operator::new(builder).map_err(to_io)?, Some(dav))
            }
            Place::S3 {
                endpoint,
                region,
                bucket,
                prefix,
                access_key,
                secret_key,
            } => {
                let root = match prefix.trim_matches('/') {
                    "" => "/shellwing/".to_string(),
                    p => format!("/{p}/shellwing/"),
                };
                let builder = services::S3::default()
                    .endpoint(endpoint.trim_end_matches('/'))
                    .region(if region.is_empty() { "auto" } else { region })
                    .bucket(bucket)
                    .root(&root)
                    .access_key_id(access_key)
                    .secret_access_key(secret_key)
                    // Only what is typed here: not ~/.aws, not the
                    // machine's cloud role.
                    .disable_config_load()
                    .disable_ec2_metadata();
                Remote::open(Operator::new(builder).map_err(to_io)?, None)
            }
            Place::Git {
                url,
                branch,
                user,
                token,
                device,
            } => {
                let git = Git::find()
                    .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                let dir = clone_dir(url, branch)?;
                let token = (!token.is_empty()).then_some((user.as_str(), token.as_str()));
                Ok(Arc::new(GitRepo::open(
                    git, &dir, url, branch, token, device,
                )?))
            }
        }
    }
}

/// Where the clone of a repository goes: a cache, made again if lost.
fn clone_dir(url: &str, branch: &str) -> io::Result<PathBuf> {
    let cache = dirs::cache_dir()
        .ok_or_else(|| io::Error::other("this system has no cache folder for the clone"))?;
    // FNV-1a: a stable name for the repository and branch.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{}\n{}", url.trim(), branch.trim()).bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    Ok(cache
        .join("shellwing")
        .join("git")
        .join(format!("{hash:016x}")))
}

/// `https://host:port` and the path after it (`/` if none).
fn split_url(url: &str) -> io::Result<(&str, &str)> {
    let url = url.trim();
    let after = url.find("://").map(|i| i + 3).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the address must start with https:// (or http://)",
        )
    })?;
    Ok(match url[after..].find('/') {
        Some(i) => (&url[..after + i], &url[after + i..]),
        None => (url, "/"),
    })
}

fn host_of(url: &str) -> &str {
    split_url(url)
        .map(|(origin, _)| origin)
        .unwrap_or(url)
        .split("://")
        .last()
        .unwrap_or(url)
}

/// A place on a server. Calls block: the sync runs on a thread of its own.
pub struct Remote {
    /// Drives the requests; OpenDAL's blocking calls wait on it.
    runtime: Option<tokio::runtime::Runtime>,
    op: blocking::Operator,
    /// The files this backend knows to be there (from the last listing, or
    /// written since), with their ETags where the server gave one.
    listed: Mutex<HashMap<String, Option<String>>>,
    /// What was read of a file, and under which ETag.
    read: Mutex<HashMap<String, (String, Vec<u8>)>>,
    /// WebDAV's conditional writes, which OpenDAL does not make, and the
    /// client they go through.
    dav: Option<(DavPut, reqwest::Client)>,
    /// The server takes conditional writes; off once it said it does not,
    /// or keeps refusing them though its file was looked at again.
    conditional: AtomicBool,
    /// Writes refused in a row.
    refused: AtomicUsize,
}

/// Writing a file over WebDAV ourselves, to send `If-Match`.
struct DavPut {
    /// The address files are under, ending in `/shellwing/`.
    base: String,
    user: String,
    password: String,
}

/// What a write is conditional on.
enum Condition {
    /// The file still has this ETag.
    Tag(String),
    /// There is no such file yet.
    Absent,
}

/// How a conditional write went.
enum Put {
    /// Written; the file's new ETag, if the server said.
    Done(Option<String>),
    /// Another device wrote it meanwhile.
    Changed,
    /// The server cannot do it: write without a condition.
    Unconditional,
}

/// Conditional writes refused in a row, each after the file was looked at
/// again, before they are given up as not working with this server.
const REFUSED_AT_MOST: usize = 6;

impl Remote {
    fn open(op: Operator, dav: Option<DavPut>) -> io::Result<Arc<dyn Backend>> {
        // OpenDAL's HTTP client: reqwest, on rustls with ring. Once is
        // enough; again is harmless (the first one installed stays).
        let _ = rustls::crypto::ring::default_provider().install_default();
        opendal::init_default_registry();
        opendal_http_transport_reqwest::install_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("shellwing-remote")
            .enable_all()
            .build()?;
        let op = op
            .layer(layers::TimeoutLayer::new().with_timeout(TIMEOUT))
            // A dropped request is tried twice more, soon: the sync itself comes
            // back every minute.
            .layer(
                layers::RetryLayer::new()
                    .with_max_times(2)
                    .with_min_delay(Duration::from_millis(500)),
            );
        let (op, client) = {
            let _in = runtime.enter();
            let client = reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .map_err(|e| io::Error::other(short(&e.to_string())))?;
            (blocking::Operator::new(op).map_err(to_io)?, client)
        };
        let mut remote = Remote::with(op, Some(runtime));
        remote.dav = dav.map(|d| (d, client));
        Ok(Arc::new(remote))
    }

    fn with(op: blocking::Operator, runtime: Option<tokio::runtime::Runtime>) -> Remote {
        Remote {
            runtime,
            op,
            listed: Mutex::default(),
            read: Mutex::default(),
            dav: None,
            conditional: AtomicBool::new(true),
            refused: AtomicUsize::new(0),
        }
    }

    /// Notes what was written: its new tag, and the bytes under it.
    fn wrote(&self, name: &str, data: &[u8], tag: Option<String>) -> io::Result<()> {
        let mut read = self.read.lock().map_err(poisoned)?;
        match &tag {
            Some(tag) if kept_read(name) => {
                read.insert(name.to_string(), (tag.clone(), data.to_vec()));
            }
            _ => {
                read.remove(name);
            }
        }
        self.listed
            .lock()
            .map_err(poisoned)?
            .insert(name.to_string(), tag);
        Ok(())
    }

    /// Looks at `name` again, after another device wrote it: its tag and
    /// bytes now, so the next read and write go by them.
    fn look_again(&self, name: &str) -> io::Result<()> {
        let tag = match self.op.stat(name) {
            Ok(m) => m.etag().map(bare),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                self.listed.lock().map_err(poisoned)?.remove(name);
                self.read.lock().map_err(poisoned)?.remove(name);
                return Ok(());
            }
            Err(e) => return Err(to_io(e)),
        };
        if !kept_read(name) {
            // Written once and never again (an image): only that it is there.
            self.listed
                .lock()
                .map_err(poisoned)?
                .insert(name.to_string(), tag);
            return Ok(());
        }
        let bytes = self.op.read(name).map_err(to_io)?.to_vec();
        // Should it change between the two, the next write is refused again.
        self.wrote(name, &bytes, tag)
    }

    /// S3's conditional write, through OpenDAL.
    fn s3_put(&self, name: &str, data: &[u8], condition: &Condition) -> io::Result<Put> {
        let can = self.op.info().capability();
        let mut opts = opendal::options::WriteOptions::default();
        match condition {
            Condition::Tag(tag) if can.write_with_if_match => opts.if_match = Some(quoted(tag)),
            Condition::Absent if can.write_with_if_not_exists => opts.if_not_exists = true,
            _ => return Ok(Put::Unconditional),
        }
        match self.op.write_options(name, data.to_vec(), opts) {
            Ok(meta) => Ok(Put::Done(meta.etag().map(bare))),
            Err(e) if e.kind() == ErrorKind::ConditionNotMatch => Ok(Put::Changed),
            Err(e) if e.kind() == ErrorKind::Unsupported || status_of(&e) == Some(501) => {
                Ok(Put::Unconditional)
            }
            Err(e) => Err(to_io(e)),
        }
    }

    /// WebDAV's conditional write: a `PUT` with `If-Match` or
    /// `If-None-Match: *`, which RFC 4918 servers (Nextcloud, Apache's,
    /// Synology's) honour.
    fn dav_put(
        &self,
        (dav, client): &(DavPut, reqwest::Client),
        name: &str,
        data: &[u8],
        condition: &Condition,
    ) -> io::Result<Put> {
        let Some(runtime) = &self.runtime else {
            return Ok(Put::Unconditional);
        };
        let request = client
            .put(format!("{}{name}", dav.base))
            .basic_auth(&dav.user, Some(&dav.password))
            .body(data.to_vec());
        let request = match condition {
            Condition::Tag(tag) => request.header("If-Match", quoted(tag)),
            Condition::Absent => request.header("If-None-Match", "*"),
        };
        let answer = runtime
            .block_on(async move { request.send().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        let tag = answer
            .headers()
            .get("etag")
            .and_then(|t| t.to_str().ok())
            .map(bare);
        Ok(match answer.status().as_u16() {
            200..=299 => Put::Done(tag),
            412 => Put::Changed,
            // A folder not made yet (OpenDAL makes it), or a server that does
            // not take conditions.
            409 | 405 | 400 | 501 => Put::Unconditional,
            code => return Err(said(ErrorKind::Unexpected, Some(code), String::new())),
        })
    }
}

/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// An ETag as `If-Match` takes it: quoted.
fn quoted(tag: &str) -> String {
    format!("\"{tag}\"")
}

/// An ETag as kept here: without its quotes, which some answers have and
/// others not (WebDAV's listing and its `PUT`), so the same tag compares
/// the same. A weak one keeps its `W/`.
fn bare(tag: &str) -> String {
    match tag.strip_prefix("W/") {
        Some(weak) => format!("W/{}", weak.trim_matches('"')),
        None => tag.trim_matches('"').to_string(),
    }
}

impl Backend for Remote {
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        let entries = match self.op.list(&format!("{dir}/")) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => vec![],
            Err(e) => return Err(to_io(e)),
        };
        let mut names = vec![];
        let mut listed = self.listed.lock().map_err(poisoned)?;
        listed.retain(|k, _| !k.starts_with(&format!("{dir}/")));
        for e in entries {
            if e.metadata().is_dir() {
                continue;
            }
            let name = e.name().trim_end_matches('/').to_string();
            if name.is_empty() || name.starts_with('.') {
                continue;
            }
            listed.insert(format!("{dir}/{name}"), e.metadata().etag().map(bare));
            names.push(name);
        }
        Ok(names)
    }

    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let tag = self
            .listed
            .lock()
            .map_err(poisoned)?
            .get(name)
            .cloned()
            .flatten();
        if let Some(tag) = &tag
            && kept_read(name)
            && let Some((had, bytes)) = self.read.lock().map_err(poisoned)?.get(name)
            && had == tag
        {
            return Ok(Some(bytes.clone()));
        }
        let bytes = match self.op.read(name) {
            Ok(b) => b.to_vec(),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(to_io(e)),
        };
        // Kept under the listing's tag: should the file have changed since,
        // the next listing's differs and it is read again.
        if let Some(tag) = tag
            && kept_read(name)
        {
            self.read
                .lock()
                .map_err(poisoned)?
                .insert(name.to_string(), (tag, bytes.clone()));
        }
        Ok(Some(bytes))
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let meta = self.op.write(name, data.to_vec()).map_err(to_io)?;
        self.wrote(name, data, meta.etag().map(bare))
    }

    fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
        let seen = self.listed.lock().map_err(poisoned)?.get(name).cloned();
        let condition = match seen {
            // A weak tag says the content is alike, not the same: no good
            // for telling whether it changed.
            Some(Some(tag)) if !tag.starts_with("W/") => Some(Condition::Tag(tag)),
            Some(_) => None,
            None => Some(Condition::Absent),
        };
        let condition = condition.filter(|_| self.conditional.load(Ordering::Relaxed));
        let new = matches!(condition, Some(Condition::Absent));
        let put = match condition {
            None => Put::Unconditional,
            Some(c) => match &self.dav {
                Some(dav) => self.dav_put(dav, name, data, &c)?,
                None => self.s3_put(name, data, &c)?,
            },
        };
        match put {
            Put::Done(tag) => {
                self.refused.store(0, Ordering::Relaxed);
                self.wrote(name, data, tag)?;
                Ok(true)
            }
            Put::Changed => {
                self.look_again(name)?;
                let there = self.listed.lock().map_err(poisoned)?.contains_key(name);
                if new && !there {
                    // Refused, yet still not there: no other device wrote it.
                    // Some servers refuse a new file whose folder is not made
                    // yet; OpenDAL makes it.
                    self.write(name, data)?;
                    return Ok(true);
                }
                if self.refused.fetch_add(1, Ordering::Relaxed) + 1 >= REFUSED_AT_MOST {
                    // Refused whatever the tag: this server's tags do not
                    // work for this. Written as before conditions.
                    self.conditional.store(false, Ordering::Relaxed);
                }
                Ok(false)
            }
            Put::Unconditional => {
                self.write(name, data)?;
                Ok(true)
            }
        }
    }

    fn remove(&self, name: &str) -> io::Result<()> {
        match self.op.delete(name) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(to_io(e)),
        }
        self.read.lock().map_err(poisoned)?.remove(name);
        self.listed.lock().map_err(poisoned)?.remove(name);
        Ok(())
    }
}

/// OpenDAL's error, said shortly: its text holds the server's whole answer,
/// a page of HTML at times.
fn to_io(e: opendal::Error) -> io::Error {
    let status = status_of(&e);
    let fallback = match (e.kind(), status) {
        (ErrorKind::ConfigInvalid, _) => short(e.message()),
        (_, Some(_)) => String::new(),
        // No answer: the network, the name, the certificate.
        _ => {
            let mut cause: &dyn std::error::Error = &e;
            while let Some(next) = cause.source() {
                cause = next;
            }
            short(&cause.to_string())
        }
    };
    said(e.kind(), status, fallback)
}

/// The HTTP status of the server's answer that `e` came from, if one came.
fn status_of(e: &opendal::Error) -> Option<u16> {
    e.to_string()
        .split("status: ")
        .nth(1)
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse().ok())
}

/// What went wrong, in a line, from the kind of error and the status.
fn said(kind: ErrorKind, status: Option<u16>, fallback: String) -> io::Error {
    let (kind, text) = match (kind, status) {
        (_, Some(401)) | (ErrorKind::PermissionDenied, _) | (_, Some(403)) => (
            io::ErrorKind::PermissionDenied,
            "the server refused the sign-in: check the user and password (or the key), and that they may write there".to_string(),
        ),
        (ErrorKind::NotFound, _) | (_, Some(404)) => (
            io::ErrorKind::NotFound,
            "the server has no such folder or bucket".to_string(),
        ),
        (ErrorKind::ConfigInvalid, _) => (io::ErrorKind::InvalidInput, fallback),
        (ErrorKind::RateLimited, _) | (_, Some(429)) => (
            io::ErrorKind::Other,
            "the server asks to slow down; trying again later".to_string(),
        ),
        (_, Some(405)) => (
            io::ErrorKind::Other,
            "that address does not take WebDAV (405): check it is the folder's WebDAV address".to_string(),
        ),
        (_, Some(code)) => (io::ErrorKind::Other, format!("the server answered {code}")),
        _ => (io::ErrorKind::Other, fallback),
    };
    io::Error::new(kind, text)
}

/// The first line of `s`, at most 200 characters.
fn short(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default();
    match line.char_indices().nth(200) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

/// Whether a file read is kept for the next round, as records are (each
/// round reads every one). Images are read once, when a note first needs
/// them, and are large: keeping them would hold every image in memory.
fn kept_read(name: &str) -> bool {
    !name.starts_with("attachments/")
}

fn poisoned<T>(_: T) -> io::Error {
    io::Error::other("a sync thread panicked")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_vault::{FolderState, MemoryKeys, Vault};

    /// A remote on OpenDAL's memory service: the same code as WebDAV's and
    /// S3's above the transport, which has no ETags, so nothing is cached.
    fn memory() -> (tokio::runtime::Runtime, Remote) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let op = Operator::new(services::Memory::default()).expect("memory");
        let op = {
            let _in = runtime.enter();
            blocking::Operator::new(op).expect("blocking")
        };
        (runtime, Remote::with(op, None))
    }

    #[test]
    fn two_devices_sync_through_a_remote() {
        let (_rt, remote) = memory();
        let dir = std::env::temp_dir().join(format!("shellwing-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        assert!(remote.list("records").expect("empty").is_empty());
        assert_eq!(remote.read("format.json").expect("none"), None);
        a.set_password("deploy@web", "from-a").expect("save");
        let rk = a.start_sync(&remote).expect("start").to_string();
        a.sync(&remote).expect("sync a");
        assert_eq!(b.folder_state(&remote).expect("state"), FolderState::Other);
        b.join(&remote, &rk).expect("join");
        b.sync(&remote).expect("sync b");
        assert_eq!(
            b.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("from-a")
        );
        remote.remove("records/nothing.sw").expect("absent is fine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_is_kept_while_the_listing_gives_the_same_etag() {
        let (_rt, remote) = memory();
        remote.write("records/a.sw", b"one").expect("write");
        // As a WebDAV or S3 listing would have it.
        let tagged = |tag: &str| {
            remote
                .listed
                .lock()
                .expect("lock")
                .insert("records/a.sw".into(), Some(tag.into()));
        };
        tagged("\"1\"");
        assert_eq!(
            remote.read("records/a.sw").expect("read").as_deref(),
            Some(&b"one"[..])
        );
        // Changed behind its back, same tag: the kept bytes.
        remote
            .op
            .write("records/a.sw", b"two".to_vec())
            .expect("write");
        assert_eq!(
            remote.read("records/a.sw").expect("read").as_deref(),
            Some(&b"one"[..])
        );
        // A new tag: read again.
        tagged("\"2\"");
        assert_eq!(
            remote.read("records/a.sw").expect("read").as_deref(),
            Some(&b"two"[..])
        );
    }

    #[test]
    fn places_are_named_without_their_secrets() {
        let s3 = Place::S3 {
            endpoint: "https://acct.r2.cloudflarestorage.com".into(),
            region: String::new(),
            bucket: "vault".into(),
            prefix: "/me/".into(),
            access_key: "AKIA".into(),
            secret_key: "s3cret".into(),
        };
        assert_eq!(
            s3.describe(),
            "vault/me on acct.r2.cloudflarestorage.com (S3)"
        );
        let dav = Place::WebDav {
            url: "https://cloud.example.com/remote.php/dav/files/me/".into(),
            user: "me".into(),
            password: "pw".into(),
        };
        assert!(!dav.describe().contains("pw"));
        assert_eq!(
            split_url("https://cloud.example.com/remote.php/dav/").expect("url"),
            ("https://cloud.example.com", "/remote.php/dav/")
        );
        assert_eq!(
            split_url("http://nas:5005").expect("url"),
            ("http://nas:5005", "/")
        );
        assert!(split_url("cloud.example.com").is_err());
    }

    /// A WebDAV server in memory on a port of its own, counting the records
    /// fetched from it.
    fn dav_server(rt: &tokio::runtime::Runtime) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        dav_server_counting(rt, "/records/")
    }

    /// A WebDAV server in memory, counting the GETs of paths with `part`.
    fn dav_server_counting(
        rt: &tokio::runtime::Runtime,
        part: &'static str,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let fetched = Arc::new(AtomicUsize::new(0));
        let counted = fetched.clone();
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let dav = dav_server::DavHandler::builder()
            .filesystem(dav_server::memfs::MemFs::new())
            .locksystem(dav_server::memls::MemLs::new())
            .build_handler();
        rt.spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (dav, counted) = (dav.clone(), counted.clone());
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req: hyper::Request<_>| {
                        if req.method() == hyper::Method::GET && req.uri().path().contains(part) {
                            counted.fetch_add(1, Ordering::Relaxed);
                        }
                        let dav = dav.clone();
                        async move { Ok::<_, std::convert::Infallible>(dav.handle(req).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (format!("http://127.0.0.1:{port}/dav/me/"), fetched)
    }

    #[test]
    fn webdav_syncs_and_fetches_only_what_changed() {
        use std::sync::atomic::Ordering;
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, fetched) = dav_server(&rt);
        let place = Place::WebDav {
            url,
            user: "me".into(),
            password: "pw".into(),
        };
        let (a_side, b_side) = (place.open().expect("a"), place.open().expect("b"));
        let dir = std::env::temp_dir().join(format!("shellwing-dav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        a.set_password("root@db", "also-a").expect("save");
        let rk = a.start_sync(a_side.as_ref()).expect("start").to_string();
        a.sync(a_side.as_ref()).expect("sync a");
        b.join(b_side.as_ref(), &rk).expect("join");
        b.sync(b_side.as_ref()).expect("sync b");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(pw(&b, "root@db").as_deref(), Some("also-a"));

        // Nothing new: B fetches no record again.
        let before = fetched.load(Ordering::Relaxed);
        b.sync(b_side.as_ref()).expect("idle");
        assert_eq!(
            fetched.load(Ordering::Relaxed),
            before,
            "read from what it kept"
        );
        // One edit on A: B fetches that one record.
        a.set_password("deploy@web", "changed").expect("edit");
        a.sync(a_side.as_ref()).expect("push");
        let before = fetched.load(Ordering::Relaxed);
        b.sync(b_side.as_ref()).expect("pull");
        assert_eq!(fetched.load(Ordering::Relaxed), before + 1);
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("changed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webdav_carries_a_notes_images() {
        use std::sync::atomic::Ordering;
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, fetched) = dav_server_counting(&rt, "/attachments/");
        let place = Place::WebDav {
            url,
            user: "me".into(),
            password: "pw".into(),
        };
        let (a_side, b_side) = (place.open().expect("a"), place.open().expect("b"));
        let dir = std::env::temp_dir().join(format!("shellwing-dav-img-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        let mut image = b"\x89PNG\r\n\x1a\n".to_vec();
        image.extend(std::iter::repeat_n(7u8, 4096));
        let id = a.add_attachment(&image).expect("image");
        let note = shellwing_vault::Note {
            id: String::new(),
            title: "Dashboard".into(),
            category: String::new(),
            format: shellwing_vault::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        a.save_note(&note, Some(&shellwing_vault::attachment_line(&id)))
            .expect("note");
        let rk = a.start_sync(a_side.as_ref()).expect("start").to_string();
        assert_eq!(a.sync(a_side.as_ref()).expect("sync a").images_pushed, 1);
        b.join(b_side.as_ref(), &rk).expect("join");
        assert_eq!(b.sync(b_side.as_ref()).expect("sync b").images_pulled, 1);
        assert_eq!(
            b.attachment(&id).expect("read").map(|i| i.to_vec()),
            Some(image)
        );
        assert_eq!(fetched.load(Ordering::Relaxed), 1);
        // Neither device sends or fetches it again.
        assert_eq!(a.sync(a_side.as_ref()).expect("idle a").images_pushed, 0);
        b.sync(b_side.as_ref()).expect("idle b");
        assert_eq!(fetched.load(Ordering::Relaxed), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn s3_syncs_two_devices() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let dir = std::env::temp_dir().join(format!("shellwing-s3-{}", std::process::id()));
        let port = s3_server(&rt, &dir);
        let place = |secret: &str| Place::S3 {
            endpoint: format!("http://127.0.0.1:{port}"),
            region: "us-east-1".into(),
            bucket: "vault".into(),
            prefix: "me".into(),
            access_key: "AKIATEST".into(),
            secret_key: secret.into(),
        };
        s3_two_devices(&dir, place);
    }

    /// An S3 server over `dir/server`, with a bucket `vault`, signing in as
    /// `AKIATEST` / `s3cret`; its port.
    fn s3_server(rt: &tokio::runtime::Runtime, dir: &std::path::Path) -> u16 {
        let _ = std::fs::remove_dir_all(dir);
        // A bucket is a folder of the server's.
        std::fs::create_dir_all(dir.join("server/vault")).expect("bucket");
        let fs = s3s_fs::FileSystem::new(dir.join("server")).expect("fs");
        let service = {
            let mut b = s3s::service::S3ServiceBuilder::new(fs);
            b.set_auth(s3s::auth::SimpleAuth::from_single("AKIATEST", "s3cret"));
            b.build()
        };
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        rt.spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let service = service.clone();
                tokio::spawn(async move {
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        port
    }

    fn s3_two_devices(dir: &std::path::Path, place: impl Fn(&str) -> Place) {
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        let wrong = place("wrong").open().expect("open");
        assert!(
            a.folder_state(wrong.as_ref()).is_err(),
            "a wrong secret is refused"
        );
        let (a_side, b_side) = (
            place("s3cret").open().expect("a"),
            place("s3cret").open().expect("b"),
        );
        a.set_password("deploy@web", "from-a").expect("save");
        let rk = a.start_sync(a_side.as_ref()).expect("start").to_string();
        a.sync(a_side.as_ref()).expect("sync a");
        assert!(
            dir.join("server/vault/me/shellwing/format.json").exists(),
            "under the prefix"
        );
        b.join(b_side.as_ref(), &rk).expect("join");
        b.set_password("root@db", "from-b").expect("save");
        b.sync(b_side.as_ref()).expect("sync b");
        a.sync(a_side.as_ref()).expect("sync a again");
        let pw = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
        assert_eq!(pw(&b, "deploy@web").as_deref(), Some("from-a"));
        assert_eq!(pw(&a, "root@db").as_deref(), Some("from-b"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_server_there_is_said_in_a_line() {
        // Nothing listens on port 9 of this machine.
        let place = Place::WebDav {
            url: "http://127.0.0.1:9/dav/".into(),
            user: "me".into(),
            password: "pw".into(),
        };
        let e = place
            .open()
            .expect("open")
            .list("records")
            .expect_err("no server");
        let text = e.to_string();
        assert!(
            !text.is_empty() && !text.contains('\n') && text.chars().count() <= 201,
            "{text}"
        );
        assert!(short(&"x".repeat(300)).ends_with('…'));
    }

    /// A remote where another device writes between this one's listing and
    /// its write: the server must refuse this one's write.
    struct Racing<'a> {
        inner: Arc<dyn Backend>,
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
        fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
            if name.starts_with("records/")
                && let Some(write_theirs) = self.meanwhile.lock().expect("lock").take()
            {
                write_theirs();
            }
            self.inner.replace(name, data)
        }
    }

    /// Two devices on `place`; the second writes a record while the first
    /// syncs it.
    fn race(dir: &std::path::Path, place: &Place) {
        let _ = std::fs::create_dir_all(dir);
        let a = Vault::open(&dir.join("race-a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("race-b.db"), &MemoryKeys::default()).expect("b");
        let (a_side, b_side) = (place.open().expect("a"), place.open().expect("b"));
        a.set_password("deploy@web", "first").expect("save");
        let rk = a.start_sync(a_side.as_ref()).expect("start").to_string();
        a.sync(a_side.as_ref()).expect("sync a");
        b.join(b_side.as_ref(), &rk).expect("join");
        b.sync(b_side.as_ref()).expect("sync b");

        a.set_password("deploy@web", "from-a").expect("edit a");
        // Later by the clock: B's edit is the newer.
        std::thread::sleep(Duration::from_millis(5));
        b.set_password("deploy@web", "from-b").expect("edit b");
        let racing = Racing {
            inner: a_side.clone(),
            meanwhile: Mutex::new(Some(Box::new(|| {
                b.sync(b_side.as_ref()).expect("b meanwhile");
            }))),
        };
        let report = a.sync(&racing).expect("sync a");
        assert!(
            report.pulled >= 1,
            "the server refused A's write: {report:?}"
        );
        let pw = |v: &Vault| {
            v.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
        };
        assert_eq!(
            pw(&a).as_deref(),
            Some("from-b"),
            "B's newer edit, taken in"
        );
        // What the server has now is the merge: B reads nothing new.
        let again = b.sync(b_side.as_ref()).expect("sync b");
        assert_eq!(again.pulled, 0, "{again:?}");
        assert_eq!(pw(&b).as_deref(), Some("from-b"));
    }

    /// Saves note `id` (new when empty), "Disk usage", holding `text`: its id.
    pub(crate) fn write_note(v: &Vault, id: &str, text: &str) -> String {
        let note = shellwing_vault::Note {
            id: id.into(),
            title: "Disk usage".into(),
            category: String::new(),
            format: shellwing_vault::NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        v.save_note(&note, Some(text)).expect("note")
    }

    /// Each note's title and text, in order.
    pub(crate) fn notes_of(v: &Vault) -> Vec<(String, String)> {
        let mut notes: Vec<(String, String)> = v
            .notes()
            .expect("notes")
            .into_iter()
            .map(|n| {
                let text = v.note_text(&n.id).expect("text").expect("there");
                (n.title, text.to_string())
            })
            .collect();
        notes.sort();
        notes
    }

    /// Note "Disk usage" changed on "desk" (A) and then on "laptop" (B),
    /// neither seeing the other's change, kept as the note and a copy.
    pub(crate) fn both_kept() -> Vec<(String, String)> {
        vec![
            ("Disk usage".into(), "du -sh /var/log".into()),
            ("Disk usage (from desk)".into(), "df -h /var".into()),
        ]
    }

    /// Two devices on `place` change a note's text without seeing each
    /// other's change, B later. With `meanwhile`, B writes while A syncs, so
    /// the server refuses A's write; otherwise A syncs first.
    fn note_changed_on_both(dir: &std::path::Path, place: &Place, meanwhile: bool) {
        let _ = std::fs::create_dir_all(dir);
        let a = Vault::open(&dir.join("note-a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("note-b.db"), &MemoryKeys::default()).expect("b");
        a.set_device_name("desk");
        b.set_device_name("laptop");
        let (a_side, b_side) = (place.open().expect("a"), place.open().expect("b"));
        let note = write_note(&a, "", "df -h");
        let rk = a.start_sync(a_side.as_ref()).expect("start").to_string();
        a.sync(a_side.as_ref()).expect("sync a");
        b.join(b_side.as_ref(), &rk).expect("join");
        b.sync(b_side.as_ref()).expect("sync b");

        write_note(&a, &note, "df -h /var");
        std::thread::sleep(Duration::from_millis(5));
        write_note(&b, &note, "du -sh /var/log");
        let copies = if meanwhile {
            let racing = Racing {
                inner: a_side.clone(),
                meanwhile: Mutex::new(Some(Box::new(|| {
                    b.sync(b_side.as_ref()).expect("b meanwhile");
                }))),
            };
            let report = a.sync(&racing).expect("sync a");
            assert!(report.pulled >= 1, "the server refused A's write");
            report.copies
        } else {
            assert!(a.sync(a_side.as_ref()).expect("sync a").copies.is_empty());
            b.sync(b_side.as_ref()).expect("sync b").copies
        };
        assert_eq!(copies, ["Disk usage (from desk)"]);
        for _ in 0..2 {
            let a_report = a.sync(a_side.as_ref()).expect("sync a");
            let b_report = b.sync(b_side.as_ref()).expect("sync b");
            assert!(
                a_report.copies.is_empty() && b_report.copies.is_empty(),
                "made once"
            );
        }
        assert_eq!(notes_of(&a), both_kept());
        assert_eq!(notes_of(&b), both_kept());
    }

    #[test]
    fn webdav_keeps_both_texts_of_a_note_changed_on_two_devices() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, _) = dav_server(&rt);
        let dir = std::env::temp_dir().join(format!("shellwing-dav-note-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let place = Place::WebDav {
            url,
            user: "me".into(),
            password: "pw".into(),
        };
        note_changed_on_both(&dir, &place, false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webdav_keeps_a_text_lost_to_a_write_made_meanwhile() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, _) = dav_server(&rt);
        let dir =
            std::env::temp_dir().join(format!("shellwing-dav-note-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let place = Place::WebDav {
            url,
            user: "me".into(),
            password: "pw".into(),
        };
        note_changed_on_both(&dir, &place, true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webdav_refuses_a_write_over_another_devices() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, _) = dav_server(&rt);
        let dir = std::env::temp_dir().join(format!("shellwing-dav-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        race(
            &dir,
            &Place::WebDav {
                url,
                user: "me".into(),
                password: "pw".into(),
            },
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two backends on one place: what one wrote since the other last
    /// looked makes the other's write refused, once.
    fn conditional_writes(place: &Place) {
        let (a, b) = (place.open().expect("a"), place.open().expect("b"));
        assert!(a.list("records").expect("list").is_empty());
        assert!(
            a.replace("records/x.sw", b"one").expect("new"),
            "absent: written"
        );
        b.write("records/x.sw", b"two").expect("b writes");
        assert!(
            !a.replace("records/x.sw", b"three").expect("refused"),
            "B wrote since"
        );
        assert_eq!(
            a.read("records/x.sw").expect("read").as_deref(),
            Some(&b"two"[..])
        );
        assert!(
            a.replace("records/x.sw", b"three").expect("again"),
            "now it knows"
        );
        // A new file the other made meanwhile: refused too.
        assert!(
            b.list("records")
                .expect("list")
                .contains(&"x.sw".to_string())
        );
        a.write("records/y.sw", b"a-made").expect("a makes y");
        assert!(
            !b.replace("records/y.sw", b"b-made").expect("refused"),
            "A made it"
        );
        assert_eq!(
            b.read("records/y.sw").expect("read").as_deref(),
            Some(&b"a-made"[..])
        );
    }

    #[test]
    fn webdav_writes_only_over_what_it_saw() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (url, _) = dav_server(&rt);
        let place = Place::WebDav {
            url,
            user: "me".into(),
            password: "pw".into(),
        };
        // The folder made first: this server refuses a new file in a folder
        // not made yet, as some do (the engine's own writes cover that).
        place
            .open()
            .expect("open")
            .write("records/.keep", b"")
            .expect("folder");
        conditional_writes(&place);
    }

    #[test]
    fn s3_writes_only_over_what_it_saw() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let dir = std::env::temp_dir().join(format!("shellwing-s3-cas-{}", std::process::id()));
        let port = s3_server(&rt, &dir);
        conditional_writes(&Place::S3 {
            endpoint: format!("http://127.0.0.1:{port}"),
            region: "us-east-1".into(),
            bucket: "vault".into(),
            prefix: String::new(),
            access_key: "AKIATEST".into(),
            secret_key: "s3cret".into(),
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Places to sync the vault with over the network, WebDAV and S3-compatible
//! storage (through OpenDAL), beside the folder on disk. Files go under
//! `shellwing/` in the place given, as the folder backend puts them.
//!
//! The engine reads every record at each sync. Over the network that would
//! be a request per record every minute, so a remote keeps what it last
//! read with the ETag the listing gave it, and reads again only what
//! changed.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opendal::{ErrorKind, Operator, blocking, layers, services};
use shellwing_vault::{Backend, Folder};

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
                let builder = services::Webdav::default()
                    .endpoint(origin)
                    .root(&format!("{}/shellwing/", path.trim_end_matches('/')))
                    .username(user)
                    .password(password);
                Remote::open(Operator::new(builder).map_err(to_io)?)
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
                Remote::open(Operator::new(builder).map_err(to_io)?)
            }
        }
    }
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
    _runtime: Option<tokio::runtime::Runtime>,
    op: blocking::Operator,
    /// Each file's ETag in the last listing, by name.
    listed: Mutex<HashMap<String, String>>,
    /// What was read of a file, and under which ETag.
    read: Mutex<HashMap<String, (String, Vec<u8>)>>,
}

impl Remote {
    fn open(op: Operator) -> io::Result<Arc<dyn Backend>> {
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
            .layer(layers::TimeoutLayer::new().with_timeout(Duration::from_secs(30)))
            // A dropped request is tried twice more, soon: the sync itself comes
            // back every minute.
            .layer(
                layers::RetryLayer::new()
                    .with_max_times(2)
                    .with_min_delay(Duration::from_millis(500)),
            );
        let op = {
            let _in = runtime.enter();
            blocking::Operator::new(op).map_err(to_io)?
        };
        Ok(Arc::new(Remote::with(op, Some(runtime))))
    }

    fn with(op: blocking::Operator, runtime: Option<tokio::runtime::Runtime>) -> Remote {
        Remote {
            _runtime: runtime,
            op,
            listed: Mutex::default(),
            read: Mutex::default(),
        }
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
            if let Some(tag) = e.metadata().etag() {
                listed.insert(format!("{dir}/{name}"), tag.to_string());
            }
            names.push(name);
        }
        Ok(names)
    }

    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let tag = self.listed.lock().map_err(poisoned)?.get(name).cloned();
        if let Some(tag) = &tag
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
        if let Some(tag) = tag {
            self.read
                .lock()
                .map_err(poisoned)?
                .insert(name.to_string(), (tag, bytes.clone()));
        }
        Ok(Some(bytes))
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let meta = self.op.write(name, data.to_vec()).map_err(to_io)?;
        let mut read = self.read.lock().map_err(poisoned)?;
        match meta.etag() {
            Some(tag) => {
                read.insert(name.to_string(), (tag.to_string(), data.to_vec()));
            }
            None => {
                read.remove(name);
            }
        }
        Ok(())
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
    let full = e.to_string();
    // "status: 401" in the response it keeps, if one came.
    let status = full
        .split("status: ")
        .nth(1)
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse::<u16>().ok());
    let (kind, text) = match (e.kind(), status) {
        (_, Some(401)) | (ErrorKind::PermissionDenied, _) => (
            io::ErrorKind::PermissionDenied,
            "the server refused the sign-in: check the user and password (or the key), and that they may write there".to_string(),
        ),
        (ErrorKind::NotFound, _) | (_, Some(404)) => (
            io::ErrorKind::NotFound,
            "the server has no such folder or bucket".to_string(),
        ),
        (ErrorKind::ConfigInvalid, _) => (io::ErrorKind::InvalidInput, short(e.message())),
        (ErrorKind::RateLimited, _) => (
            io::ErrorKind::Other,
            "the server asks to slow down; trying again later".to_string(),
        ),
        (_, Some(405)) => (
            io::ErrorKind::Other,
            "that address does not take WebDAV (405): check it is the folder's WebDAV address".to_string(),
        ),
        (_, Some(code)) => (io::ErrorKind::Other, format!("the server answered {code}")),
        // No answer: the network, the name, the certificate.
        _ => {
            let mut cause: &dyn std::error::Error = &e;
            while let Some(next) = cause.source() {
                cause = next;
            }
            (io::ErrorKind::Other, short(&cause.to_string()))
        }
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
                .insert("records/a.sw".into(), tag.into());
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
                        if req.method() == hyper::Method::GET
                            && req.uri().path().contains("/records/")
                        {
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
    fn s3_syncs_two_devices() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let dir = std::env::temp_dir().join(format!("shellwing-s3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
        let place = |secret: &str| Place::S3 {
            endpoint: format!("http://127.0.0.1:{port}"),
            region: "us-east-1".into(),
            bucket: "vault".into(),
            prefix: "me".into(),
            access_key: "AKIATEST".into(),
            secret_key: secret.into(),
        };
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
        let _ = std::fs::remove_dir_all(&dir);
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
}

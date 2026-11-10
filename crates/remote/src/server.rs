//! A shellwing server: the cloud, or one somebody hosts. To the engine it is
//! one more place files go. To the server, a device signed in as a person:
//! it never sees a key, and what it keeps is sealed before it gets here.
//!
//! **Signing in** follows RFC 8252. [`SignIn`] listens on a loopback port and
//! gives an address to open in the browser, where the server does its own
//! sign-in and the person approves this device. The browser comes back to the
//! port with a code, which is exchanged with a PKCE verifier for a refresh
//! token: the place's secret, kept in the vault. The app never sees a
//! password or an identity provider's token.
//!
//! **Syncing.** The server numbers every change to a space, so a listing asks
//! only for what changed since the last one, and costs one small request. It
//! decides conditional writes itself, on every store it may be using.
//!
//! **Certificates.** The person's organizations there sign a public key for
//! the accounts their admins granted ([`Server::certificate`]); the key's
//! private half stays here.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use shellwing_vault::Backend;

use crate::{TIMEOUT, bare, kept_read, poisoned, quoted, short};

/// How long a person has to sign in, in the browser.
const SIGN_IN_TIME: Duration = Duration::from_secs(5 * 60);
/// How often the wait for the browser looks at its port and at being called off.
const SIGN_IN_POLL: Duration = Duration::from_millis(100);
/// How long an access token is used before asking for another. Short of what
/// servers give, by a clock of this device's own: a token that stops earlier
/// is answered 401, and renewed then.
const ACCESS_LIFE: Duration = Duration::from_secs(5 * 60);
/// The most changes asked for at a time.
const CHANGES_PAGE: u32 = 1000;

/// What the app shows in the browser when it is done there.
const SIGNED_IN_PAGE: &str = "<!doctype html><meta charset=utf-8><title>shellwing</title>\
<body style='font:16px system-ui;margin:4em auto;max-width:28em;text-align:center'>\
<h2>You are signed in</h2><p>You can close this tab and go back to shellwing.</p>";
const FAILED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>shellwing</title>\
<body style='font:16px system-ui;margin:4em auto;max-width:28em;text-align:center'>\
<h2>That did not work</h2><p>Go back to shellwing and try again.</p>";

/// A server's address as given, checked: `https://`, or `http://` on this
/// machine, since tokens go to it. No trailing slash.
pub fn check_address(address: &str) -> Result<String, &'static str> {
    let address = address.trim().trim_end_matches('/');
    if let Some(rest) = address.strip_prefix("http://") {
        let authority = rest.split('/').next().unwrap_or_default();
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
            _ => authority,
        };
        if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Err("the address must start with https://");
        }
    } else if !address.starts_with("https://") {
        return Err("the address must start with https://");
    }
    if address.len() <= "https://".len() {
        return Err("the address has no server in it");
    }
    Ok(address.to_string())
}

/// `text` as a URL's query takes it.
fn query_encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A query's value, undone.
fn query_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn random_text(bytes: usize) -> io::Result<String> {
    let mut data = vec![0u8; bytes];
    getrandom::fill(&mut data).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(data))
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    // reqwest, on rustls with ring. Once is enough; again is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("shellwing-server")
        .enable_all()
        .build()
}

fn client(runtime: &tokio::runtime::Runtime) -> io::Result<Client> {
    let _in = runtime.enter();
    Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| io::Error::other(short(&e.to_string())))
}

/// A sign-in under way: a port listening, and the address to open.
pub struct SignIn {
    server: String,
    listener: TcpListener,
    redirect: String,
    verifier: String,
    state: String,
    browser_url: String,
}

impl SignIn {
    /// Starts listening. Nothing has reached the server yet.
    pub fn begin(address: &str, device: &str, platform: &str) -> io::Result<SignIn> {
        let server =
            check_address(address).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let redirect = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr()?.port()
        );
        // 32 bytes: 43 characters, the least RFC 7636 takes.
        let verifier = random_text(32)?;
        let state = random_text(16)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let browser_url = format!(
            "{server}/connect-device/?redirect_uri={}&code_challenge={challenge}&state={state}&device_name={}&platform={}",
            query_encoded(&redirect),
            query_encoded(device),
            query_encoded(platform),
        );
        Ok(SignIn {
            server,
            listener,
            redirect,
            verifier,
            state,
            browser_url,
        })
    }

    /// What to open in the browser.
    pub fn browser_url(&self) -> &str {
        &self.browser_url
    }

    /// Waits for the browser, then exchanges its code. Gives the refresh
    /// token, the place's secret. Blocks: not for the UI thread.
    pub fn wait(self, cancel: &AtomicBool) -> io::Result<String> {
        let code = self.wait_for_code(cancel)?;
        let runtime = runtime()?;
        let client = client(&runtime)?;
        let request = client
            .post(format!("{}/api/devices/token", self.server))
            .json(&serde_json::json!({
                "code": code,
                "codeVerifier": self.verifier,
                "redirectUri": self.redirect,
            }));
        let answer = runtime
            .block_on(async move { request.send().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        if !answer.status().is_success() {
            return Err(refused(&runtime, answer));
        }
        let tokens: Answer<Tokens> = runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        tokens
            .data
            .refresh_token
            .ok_or_else(|| io::Error::other("the server gave no token"))
    }

    fn wait_for_code(&self, cancel: &AtomicBool) -> io::Result<String> {
        let end = Instant::now() + SIGN_IN_TIME;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "signing in was called off",
                ));
            }
            if Instant::now() > end {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "signing in took too long: try again",
                ));
            }
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(SIGN_IN_POLL);
                    continue;
                }
                Err(e) => return Err(e),
            };
            // Anything on this machine can knock: only the state lets one in.
            if let Some(code) = self.answer_browser(stream) {
                return Ok(code);
            }
        }
    }

    /// Answers one request on the port, and gives its code if it is the one.
    fn answer_browser(&self, stream: std::net::TcpStream) -> Option<String> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        // One line, at most 8 KB: what the port takes from whoever knocks.
        (&mut reader).take(8 * 1024).read_line(&mut line).ok()?;
        let target = line.split_whitespace().nth(1)?.to_string();
        let mut stream = reader.into_inner();
        let code = target.strip_prefix("/callback?").and_then(|query| {
            let value = |name: &str| {
                query.split('&').find_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    (k == name).then(|| query_decoded(v))
                })
            };
            (value("state")?.as_str() == self.state).then(|| value("code"))?
        });
        let (status, page) = match &code {
            Some(_) => ("200 OK", SIGNED_IN_PAGE),
            None => ("400 Bad Request", FAILED_PAGE),
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        code
    }
}

/// Tells the server this device is signing out: its token stops working.
/// Blocks: not for the UI thread.
pub fn sign_out(address: &str, refresh_token: &str) -> io::Result<()> {
    let server =
        check_address(address).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let runtime = runtime()?;
    let request = client(&runtime)?
        .post(format!("{server}/api/devices/sign-out"))
        .json(&serde_json::json!({ "refreshToken": refresh_token }));
    let answer = runtime
        .block_on(async move { request.send().await })
        .map_err(|e| io::Error::other(short(&e.to_string())))?;
    if answer.status().is_success() {
        Ok(())
    } else {
        Err(refused(&runtime, answer))
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpaceAnswer {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    org_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    key_version: u32,
    #[serde(default)]
    permission: String,
    #[serde(default)]
    needs_new_key: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityAnswer {
    x25519_public: String,
    ed25519_public: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberAnswer {
    user_id: String,
    username: String,
    #[serde(default)]
    full_name: String,
    role: String,
    permission: String,
    served: bool,
    has_key: bool,
    identity: Option<IdentityAnswer>,
}

#[derive(Deserialize)]
struct CollectionAnswer {
    members: Vec<MemberAnswer>,
    #[serde(default)]
    grants: Vec<Grant>,
}

/// A collection's grant: to a person (`user_id`) or a group (`group_id`),
/// with `connect`, `edit` or `manage`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Grant {
    pub user_id: String,
    pub username: String,
    pub full_name: String,
    pub group_id: String,
    pub group_name: String,
    pub permission: String,
}

impl Grant {
    /// Who it is to, as people read it.
    pub fn name(&self) -> String {
        if !self.group_id.is_empty() {
            return self.group_name.clone();
        }
        match self.full_name.as_str() {
            "" => self.username.clone(),
            full => format!("{full} ({})", self.username),
        }
    }

    /// The same person or group as `other`.
    pub fn same_grantee(&self, other: &Grant) -> bool {
        self.user_id == other.user_id && self.group_id == other.group_id
    }
}

/// Somebody of the organization a collection can be granted to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantablePerson {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub full_name: String,
    /// `owner`, `admin` or `member`: owners have Manage already.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub suspended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantableGroup {
    pub group_id: String,
    pub name: String,
}

/// Whom a collection can be granted to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Grantable {
    pub people: Vec<GrantablePerson>,
    pub groups: Vec<GrantableGroup>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WrapAnswer {
    version: u32,
    recipient_x25519: String,
    ephemeral_public: String,
    sealed_key: String,
    commitment: String,
    signature: String,
    wrapper_ed25519: String,
}

#[derive(Deserialize)]
struct KeysAnswer {
    wraps: Vec<WrapAnswer>,
}

/// Fixed-size bytes from base64.
fn decoded_array<const N: usize>(text: &str) -> io::Result<[u8; N]> {
    B64.decode(text)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| io::Error::other("the server sent a key of the wrong size"))
}

fn identity_of(i: &IdentityAnswer) -> io::Result<shellwing_vault::PublicIdentity> {
    Ok(shellwing_vault::PublicIdentity {
        x25519: decoded_array(&i.x25519_public)?,
        ed25519: decoded_array(&i.ed25519_public)?,
        signature: decoded_array(&i.signature)?,
    })
}

fn wraps_body(version: u32, wraps: &[(String, shellwing_vault::Wrap)]) -> serde_json::Value {
    serde_json::json!({
        "version": version,
        "wraps": wraps.iter().map(|(user, w)| serde_json::json!({
            "userId": user,
            "recipientX25519": B64.encode(w.recipient_x25519),
            "ephemeralPublic": B64.encode(w.ephemeral_public),
            "sealedKey": B64.encode(&w.sealed),
            "commitment": B64.encode(w.commitment),
            "signature": B64.encode(w.signature),
        })).collect::<Vec<_>>(),
    })
}

#[derive(Deserialize)]
struct Changes {
    revision: i64,
    more: bool,
    objects: Vec<Changed>,
}

#[derive(Deserialize)]
struct Changed {
    name: String,
    etag: String,
    deleted: bool,
}

#[derive(Deserialize)]
struct Problem {
    #[serde(default)]
    code: String,
    #[serde(default)]
    detail: String,
}

/// Why the server said no, as the sync log says it.
fn refused(runtime: &tokio::runtime::Runtime, answer: Response) -> io::Error {
    let status = answer.status();
    let problem: Option<Problem> = runtime.block_on(async move { answer.json().await }).ok();
    let (code, detail) = problem.map(|p| (p.code, p.detail)).unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this device has to sign in to the server again: connect again in Settings › Sync",
        ),
        _ if code == "ERR_SYNC_SPACE_FULL" => {
            io::Error::other("the server has no more room for this vault")
        }
        _ if code == "ERR_SYNC_OBJECT_TOO_LARGE" => {
            io::Error::other("a file is too large for this server")
        }
        _ if !detail.is_empty() => io::Error::other(format!("the server said: {}", short(&detail))),
        _ => io::Error::other(format!("the server answered {}", status.as_u16())),
    }
}

/// The backend of a shellwing server.
pub struct Server {
    base: String,
    refresh_token: String,
    runtime: tokio::runtime::Runtime,
    client: Client,
    state: Mutex<State>,
    /// The stores of the collections synced from here, kept so that each
    /// asks only for what changed since.
    collections: Mutex<HashMap<String, Arc<Server>>>,
}

#[derive(Default)]
struct State {
    access: Option<(String, Instant)>,
    space: Option<String>,
    /// The revision the listing has reached.
    since: i64,
    /// Each file there and its ETag, as last seen.
    known: HashMap<String, String>,
    /// What was read, with the ETag it had.
    kept: HashMap<String, (String, Vec<u8>)>,
    /// The identity published from here, not to send it at every sync.
    published: Option<shellwing_vault::PublicIdentity>,
}

impl Server {
    /// Making it does not reach the server yet.
    pub fn open(address: &str, refresh_token: &str) -> io::Result<Server> {
        let base =
            check_address(address).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let runtime = runtime()?;
        let client = client(&runtime)?;
        Ok(Server {
            base,
            refresh_token: refresh_token.to_string(),
            runtime,
            client,
            state: Mutex::new(State::default()),
            collections: Mutex::new(HashMap::new()),
        })
    }

    /// The store of one collection: the same server and sign-in, another space.
    fn collection_server(&self, space: &str) -> io::Result<Arc<Server>> {
        let mut stores = self.collections.lock().map_err(poisoned)?;
        if let Some(s) = stores.get(space) {
            return Ok(s.clone());
        }
        let runtime = runtime()?;
        let client = client(&runtime)?;
        let access = self.state()?.access.clone();
        let store = Arc::new(Server {
            base: self.base.clone(),
            refresh_token: self.refresh_token.clone(),
            runtime,
            client,
            state: Mutex::new(State {
                access,
                space: Some(space.to_string()),
                ..State::default()
            }),
            collections: Mutex::new(HashMap::new()),
        });
        stores.insert(space.to_string(), store.clone());
        Ok(store)
    }

    /// The JSON of a successful answer, or why it is not one.
    fn answered<T: serde::de::DeserializeOwned>(&self, answer: Response) -> io::Result<T> {
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        let parsed: Answer<T> = self
            .runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        Ok(parsed.data)
    }

    fn state(&self) -> io::Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(poisoned)
    }

    fn send(&self, request: RequestBuilder) -> io::Result<Response> {
        self.runtime
            .block_on(async move { request.send().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))
    }

    /// An access token, renewed when it has been used for long enough.
    fn access(&self) -> io::Result<String> {
        if let Some((token, until)) = &self.state()?.access
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let answer = self.send(
            self.client
                .post(format!("{}/api/devices/refresh", self.base))
                .json(&serde_json::json!({ "refreshToken": self.refresh_token })),
        )?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        let tokens: Answer<Tokens> = self
            .runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        let token = tokens.data.access_token;
        self.state()?.access = Some((token.clone(), Instant::now() + ACCESS_LIFE));
        Ok(token)
    }

    /// A request with the token. A 401 may be a token the server stopped
    /// taking before its time (it restarted without its cache): asks for
    /// another, once.
    fn authorized(
        &self,
        method: Method,
        path_and_query: &str,
        build: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> io::Result<Response> {
        let url = format!("{}/api{path_and_query}", self.base);
        let attempt = |token: String| {
            self.send(build(
                self.client.request(method.clone(), &url).bearer_auth(token),
            ))
        };
        let answer = attempt(self.access()?)?;
        if answer.status() != StatusCode::UNAUTHORIZED {
            return Ok(answer);
        }
        self.state()?.access = None;
        attempt(self.access()?)
    }

    /// The space this vault syncs in: the person's own.
    fn space(&self) -> io::Result<String> {
        if let Some(space) = &self.state()?.space {
            return Ok(space.clone());
        }
        let answer = self.authorized(Method::GET, "/sync/spaces", |r| r)?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        let spaces: Answer<Vec<SpaceAnswer>> = self
            .runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        let space = spaces
            .data
            .into_iter()
            .find(|s| s.kind == "personal")
            .map(|s| s.id)
            .ok_or_else(|| io::Error::other("the server has no space for this person"))?;
        self.state()?.space = Some(space.clone());
        Ok(space)
    }

    fn object_path(&self, name: &str) -> io::Result<String> {
        Ok(format!(
            "/sync/spaces/{}/object?name={}",
            self.space()?,
            query_encoded(name)
        ))
    }

    /// Brings `known` up to date with what changed since the last time.
    fn pull(&self) -> io::Result<()> {
        let space = self.space()?;
        loop {
            let since = self.state()?.since;
            let answer = self.authorized(
                Method::GET,
                &format!("/sync/spaces/{space}/changes?since={since}&limit={CHANGES_PAGE}"),
                |r| r,
            )?;
            if !answer.status().is_success() {
                return Err(refused(&self.runtime, answer));
            }
            let changes: Answer<Changes> = self
                .runtime
                .block_on(async move { answer.json().await })
                .map_err(|e| io::Error::other(short(&e.to_string())))?;
            let changes = changes.data;
            let mut state = self.state()?;
            for object in changes.objects {
                if object.deleted {
                    state.known.remove(&object.name);
                    state.kept.remove(&object.name);
                } else {
                    state.known.insert(object.name, object.etag);
                }
            }
            state.since = changes.revision;
            if !changes.more {
                return Ok(());
            }
        }
    }

    /// Notes what the file holds after this device wrote it.
    fn wrote(&self, name: &str, data: &[u8], answer: &Response) -> io::Result<()> {
        let mut state = self.state()?;
        match answer.headers().get("etag").and_then(|t| t.to_str().ok()) {
            Some(tag) => {
                let tag = bare(tag);
                if kept_read(name) {
                    state
                        .kept
                        .insert(name.to_string(), (tag.clone(), data.to_vec()));
                }
                state.known.insert(name.to_string(), tag);
            }
            None => {
                state.known.remove(name);
                state.kept.remove(name);
            }
        }
        Ok(())
    }
}

/// An organization the person is in.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Org {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
struct Certificate {
    certificate: String,
}

/// Organizations and their SSH certificates: what the person was let in to
/// is the server's to say, not the app's.
impl Server {
    /// The organizations the person is in.
    pub fn orgs(&self) -> io::Result<Vec<Org>> {
        let answer = self.authorized(Method::GET, "/orgs", |r| r)?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        let orgs: Answer<Vec<Org>> = self
            .runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        Ok(orgs.data)
    }

    /// `org`'s certificate for `public_key` (an OpenSSH line), as a line of a
    /// `-cert.pub` file. The accounts it logs in as are the ones an admin
    /// granted the person there.
    pub fn certificate(&self, org: &str, public_key: &str) -> io::Result<String> {
        let path = format!("/orgs/{}/ssh/certificates", query_encoded(org));
        let body = serde_json::json!({ "publicKey": public_key });
        let answer = self.authorized(Method::POST, &path, |r| r.json(&body))?;
        let status = answer.status();
        if !status.is_success() {
            let problem: Option<Problem> = self
                .runtime
                .block_on(async move { answer.json().await })
                .ok();
            let code = problem.map(|p| p.code).unwrap_or_default();
            return Err(io::Error::other(match (status, code.as_str()) {
                (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _) => {
                    "this device has to sign in to the server again: connect again in Settings › Sync".into()
                }
                (StatusCode::NOT_FOUND, _) => {
                    "you are not in this organization on the server, or it is gone".into()
                }
                (_, "ERR_SSH_CA_NOT_SET_UP") => {
                    "the organization has no certificate authority yet: an admin makes one".into()
                }
                (_, "ERR_SSH_NO_PRINCIPALS") => {
                    "you may log in as no account yet: an admin grants you some".into()
                }
                _ => format!("the server answered {}", status.as_u16()),
            }));
        }
        let certificate: Answer<Certificate> = self
            .runtime
            .block_on(async move { answer.json().await })
            .map_err(|e| io::Error::other(short(&e.to_string())))?;
        Ok(certificate.data.certificate)
    }
}

/// Who has access to a collection, for those who manage it.
impl Server {
    /// The collection's grants.
    pub fn collection_grants(&self, c: &shellwing_vault::CollectionInfo) -> io::Result<Vec<Grant>> {
        let answer = self.authorized(Method::GET, &collection_path(c, ""), |r| r)?;
        let detail: CollectionAnswer = self.answered(answer)?;
        Ok(detail.grants)
    }

    /// Whom it can be granted to.
    pub fn grantable(&self, c: &shellwing_vault::CollectionInfo) -> io::Result<Grantable> {
        let answer = self.authorized(Method::GET, &collection_path(c, "/grantable"), |r| r)?;
        self.answered(answer)
    }

    /// Makes these its grants, and no others; gives them as the server keeps
    /// them.
    pub fn set_grants(
        &self,
        c: &shellwing_vault::CollectionInfo,
        grants: &[Grant],
    ) -> io::Result<Vec<Grant>> {
        let body = serde_json::json!({
            "grants": grants
                .iter()
                .map(|g| serde_json::json!({
                    "userId": g.user_id,
                    "groupId": g.group_id,
                    "permission": g.permission,
                }))
                .collect::<Vec<_>>(),
        });
        let answer = self.authorized(Method::PUT, &collection_path(c, "/grants"), |r| {
            r.json(&body)
        })?;
        self.answered(answer)
    }
}

/// A collection's path on the server.
fn collection_path(c: &shellwing_vault::CollectionInfo, rest: &str) -> String {
    format!(
        "/orgs/{}/collections/{}{rest}",
        query_encoded(&c.org_id),
        query_encoded(&c.id)
    )
}

impl shellwing_vault::Collections for Server {
    fn list_collections(&self) -> io::Result<Vec<shellwing_vault::CollectionInfo>> {
        let answer = self.authorized(Method::GET, "/sync/spaces", |r| r)?;
        let spaces: Vec<SpaceAnswer> = self.answered(answer)?;
        Ok(spaces
            .into_iter()
            .filter(|s| s.kind == "collection")
            .map(|s| shellwing_vault::CollectionInfo {
                id: s.id,
                org_id: s.org_id,
                name: s.name,
                key_version: s.key_version,
                permission: s.permission,
                needs_new_key: s.needs_new_key,
            })
            .collect())
    }

    fn collection_members(
        &self,
        c: &shellwing_vault::CollectionInfo,
    ) -> io::Result<Vec<shellwing_vault::CollectionMember>> {
        let answer = self.authorized(Method::GET, &collection_path(c, ""), |r| r)?;
        let detail: CollectionAnswer = self.answered(answer)?;
        detail
            .members
            .into_iter()
            .map(|m| {
                Ok(shellwing_vault::CollectionMember {
                    identity: m.identity.as_ref().map(identity_of).transpose()?,
                    user_id: m.user_id,
                    username: m.username,
                    full_name: m.full_name,
                    role: m.role,
                    permission: m.permission,
                    served: m.served,
                    has_key: m.has_key,
                })
            })
            .collect()
    }

    fn collection_keys(
        &self,
        c: &shellwing_vault::CollectionInfo,
    ) -> io::Result<Vec<shellwing_vault::Given>> {
        let answer = self.authorized(Method::GET, &collection_path(c, "/keys"), |r| r)?;
        let keys: KeysAnswer = self.answered(answer)?;
        keys.wraps
            .into_iter()
            .map(|w| {
                Ok(shellwing_vault::Given {
                    version: w.version,
                    wrapper: decoded_array(&w.wrapper_ed25519)?,
                    wrap: shellwing_vault::Wrap {
                        recipient_x25519: decoded_array(&w.recipient_x25519)?,
                        ephemeral_public: decoded_array(&w.ephemeral_public)?,
                        sealed: B64
                            .decode(&w.sealed_key)
                            .map_err(|_| io::Error::other("the server sent a damaged key"))?,
                        commitment: decoded_array(&w.commitment)?,
                        signature: decoded_array(&w.signature)?,
                    },
                })
            })
            .collect()
    }

    fn create_collection_key(
        &self,
        c: &shellwing_vault::CollectionInfo,
        version: u32,
        wraps: &[(String, shellwing_vault::Wrap)],
    ) -> io::Result<bool> {
        let body = wraps_body(version, wraps);
        let answer = self.authorized(Method::POST, &collection_path(c, "/keys"), |r| {
            r.json(&body)
        })?;
        if answer.status() == StatusCode::CONFLICT {
            return Ok(false);
        }
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        Ok(true)
    }

    fn add_collection_wraps(
        &self,
        c: &shellwing_vault::CollectionInfo,
        version: u32,
        wraps: &[(String, shellwing_vault::Wrap)],
    ) -> io::Result<()> {
        let body = wraps_body(version, wraps);
        let answer = self.authorized(Method::POST, &collection_path(c, "/wraps"), |r| {
            r.json(&body)
        })?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        Ok(())
    }

    fn collection_store(
        &self,
        c: &shellwing_vault::CollectionInfo,
    ) -> io::Result<Box<dyn Backend>> {
        Ok(Box::new(Shared(self.collection_server(&c.id)?)))
    }
}

/// A collection's store, kept by the server it came from.
struct Shared(Arc<Server>);

impl Backend for Shared {
    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        self.0.list(dir)
    }
    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        self.0.read(name)
    }
    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        self.0.write(name, data)
    }
    fn remove(&self, name: &str) -> io::Result<()> {
        self.0.remove(name)
    }
    fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
        self.0.replace(name, data)
    }
}

impl Backend for Server {
    fn takes_identity(&self) -> bool {
        true
    }

    fn collections(&self) -> Option<&dyn shellwing_vault::Collections> {
        Some(self)
    }

    fn publish_identity(&self, identity: &shellwing_vault::PublicIdentity) -> io::Result<()> {
        if self.state()?.published.as_ref() == Some(identity) {
            return Ok(());
        }
        let body = serde_json::json!({
            "x25519Public": B64.encode(identity.x25519),
            "ed25519Public": B64.encode(identity.ed25519),
            "signature": B64.encode(identity.signature),
        });
        let answer = self.authorized(Method::PUT, "/users/current/identity", |r| r.json(&body))?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        self.state()?.published = Some(identity.clone());
        Ok(())
    }

    fn list(&self, dir: &str) -> io::Result<Vec<String>> {
        self.pull()?;
        let prefix = format!("{dir}/");
        Ok(self
            .state()?
            .known
            .keys()
            .filter_map(|name| name.strip_prefix(&prefix))
            .filter(|name| !name.contains('/'))
            .map(str::to_string)
            .collect())
    }

    fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let held = {
            let state = self.state()?;
            let held = state.kept.get(name).cloned();
            // In a folder, as of the last listing, which each round makes.
            // `format.json` is looked at each time: nothing lists it.
            if name.contains('/')
                && let (Some(tag), Some((had, bytes))) = (state.known.get(name), &held)
                && tag == had
            {
                return Ok(Some(bytes.clone()));
            }
            held
        };
        let path = self.object_path(name)?;
        let answer = self.authorized(Method::GET, &path, |r| match &held {
            Some((tag, _)) => r.header("If-None-Match", quoted(tag)),
            None => r,
        })?;
        match answer.status() {
            StatusCode::NOT_MODIFIED => Ok(held.map(|(_, bytes)| bytes)),
            StatusCode::NOT_FOUND => {
                let mut state = self.state()?;
                state.known.remove(name);
                state.kept.remove(name);
                Ok(None)
            }
            status if status.is_success() => {
                let tag = answer
                    .headers()
                    .get("etag")
                    .and_then(|t| t.to_str().ok())
                    .map(bare);
                let bytes = self
                    .runtime
                    .block_on(async move { answer.bytes().await })
                    .map_err(|e| io::Error::other(short(&e.to_string())))?
                    .to_vec();
                if let Some(tag) = tag {
                    let mut state = self.state()?;
                    if kept_read(name) {
                        state
                            .kept
                            .insert(name.to_string(), (tag.clone(), bytes.clone()));
                    }
                    state.known.insert(name.to_string(), tag);
                }
                Ok(Some(bytes))
            }
            _ => Err(refused(&self.runtime, answer)),
        }
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let path = self.object_path(name)?;
        let answer = self.authorized(Method::PUT, &path, |r| r.body(data.to_vec()))?;
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        self.wrote(name, data, &answer)
    }

    fn remove(&self, name: &str) -> io::Result<()> {
        let path = self.object_path(name)?;
        let answer = self.authorized(Method::DELETE, &path, |r| r)?;
        if !answer.status().is_success() && answer.status() != StatusCode::NOT_FOUND {
            return Err(refused(&self.runtime, answer));
        }
        let mut state = self.state()?;
        state.known.remove(name);
        state.kept.remove(name);
        Ok(())
    }

    fn replace(&self, name: &str, data: &[u8]) -> io::Result<bool> {
        let seen = self.state()?.known.get(name).cloned();
        let path = self.object_path(name)?;
        let answer = self.authorized(Method::PUT, &path, |r| {
            let r = r.body(data.to_vec());
            match &seen {
                Some(tag) => r.header("If-Match", quoted(tag)),
                None => r.header("If-None-Match", "*"),
            }
        })?;
        if answer.status() == StatusCode::PRECONDITION_FAILED {
            // Reading it now has to give theirs, not what was kept.
            self.state()?.kept.remove(name);
            return Ok(false);
        }
        if !answer.status().is_success() {
            return Err(refused(&self.runtime, answer));
        }
        self.wrote(name, data, &answer)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    use shellwing_vault::{FolderState, MemoryKeys, Vault};

    use super::*;

    /// A server as far as the app knows one: the same answers, in memory.
    /// The real one is tried by `against_a_real_server` below.
    #[derive(Default)]
    struct Fake {
        files: BTreeMap<String, (String, Vec<u8>)>,
        /// What changed, oldest first: revision, name, removed.
        log: Vec<(i64, String, bool)>,
        token_number: usize,
        valid_token: String,
        /// Refresh tokens are refused.
        signed_out: bool,
        requests: Vec<String>,
        /// The identity published, as sent.
        identity: Option<serde_json::Value>,
    }

    struct Http {
        method: String,
        target: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    fn read_request(stream: &mut TcpStream) -> Option<Http> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next()?.to_string(), parts.next()?.to_string());
        let mut headers = HashMap::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).ok()?;
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            let (k, v) = header.split_once(':')?;
            headers.insert(k.to_lowercase(), v.trim().to_string());
        }
        let size: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0; size];
        reader.read_exact(&mut body).ok()?;
        Some(Http {
            method,
            target,
            headers,
            body,
        })
    }

    fn reply(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &[u8]) {
        let mut head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    }

    fn json_error(code: &str) -> Vec<u8> {
        serde_json::json!({ "code": code, "detail": code })
            .to_string()
            .into_bytes()
    }

    fn name_of(target: &str) -> String {
        let query = target
            .split_once("name=")
            .map(|(_, n)| n)
            .unwrap_or_default();
        query_decoded(query.split('&').next().unwrap_or_default())
    }

    fn serve(fake: &Mutex<Fake>, stream: &mut TcpStream) {
        let Some(request) = read_request(stream) else {
            return;
        };
        let mut f = fake.lock().expect("fake");
        f.requests
            .push(format!("{} {}", request.method, request.target));
        let path = request
            .target
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        if request.method == "POST" && path == "/api/devices/token" {
            let body = serde_json::json!({ "data": { "accessToken": "unused", "refreshToken": "refresh-1" } });
            return reply(stream, 200, &[], body.to_string().as_bytes());
        }
        if request.method == "POST" && path == "/api/devices/refresh" {
            if f.signed_out {
                return reply(stream, 401, &[], &json_error("ERR_DEVICE_TOKEN_INVALID"));
            }
            f.token_number += 1;
            f.valid_token = format!("access-{}", f.token_number);
            let body = serde_json::json!({ "data": { "accessToken": f.valid_token } });
            return reply(stream, 200, &[], body.to_string().as_bytes());
        }
        let bearer = format!("Bearer {}", f.valid_token);
        if request.headers.get("authorization") != Some(&bearer) {
            return reply(stream, 401, &[], &json_error("ERR_SESSION_JWT_INVALID"));
        }
        if request.method == "PUT" && path == "/api/users/current/identity" {
            f.identity = serde_json::from_slice(&request.body).ok();
            return reply(stream, 200, &[], b"{}");
        }
        if path == "/api/orgs" {
            let body = serde_json::json!({ "data": [{ "id": "o1", "slug": "team", "name": "Team", "role": "member" }] });
            return reply(stream, 200, &[], body.to_string().as_bytes());
        }
        if request.method == "POST" && path.ends_with("/ssh/certificates") {
            let key: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let key = key["publicKey"].as_str().unwrap_or_default();
            return match path.as_str() {
                "/api/orgs/o1/ssh/certificates" => {
                    let body = serde_json::json!({ "data": { "certificate": format!("certified {key}"), "principals": ["deploy"] } });
                    reply(stream, 201, &[], body.to_string().as_bytes())
                }
                "/api/orgs/o2/ssh/certificates" => {
                    reply(stream, 412, &[], &json_error("ERR_SSH_NO_PRINCIPALS"))
                }
                _ => reply(stream, 404, &[], &json_error("ERR_NOT_FOUND")),
            };
        }
        if path == "/api/sync/spaces" {
            let body = serde_json::json!({ "data": [{ "id": "s1", "type": "personal" }] });
            return reply(stream, 200, &[], body.to_string().as_bytes());
        }
        if path == "/api/sync/spaces/s1/changes" {
            let since: i64 = request
                .target
                .split("since=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let revision = f.log.last().map(|e| e.0).unwrap_or(0);
            let mut latest: BTreeMap<&str, (i64, bool)> = BTreeMap::new();
            for (rev, name, removed) in &f.log {
                latest.insert(name, (*rev, *removed));
            }
            let objects: Vec<_> = latest
                .iter()
                .filter(|(_, (rev, _))| *rev > since)
                .map(|(name, (_, removed))| {
                    let etag = f.files.get(*name).map(|e| e.0.clone()).unwrap_or_default();
                    serde_json::json!({ "name": name, "etag": etag, "size": 0, "deleted": removed })
                })
                .collect();
            let body = serde_json::json!({ "data": { "revision": revision, "more": false, "objects": objects } });
            return reply(stream, 200, &[], body.to_string().as_bytes());
        }
        let name = name_of(&request.target);
        let current = f.files.get(&name).cloned();
        let if_match = request.headers.get("if-match").map(|t| bare(t));
        let if_absent = request
            .headers
            .get("if-none-match")
            .is_some_and(|t| t == "*");
        match request.method.as_str() {
            "GET" => match current {
                None => reply(stream, 404, &[], &json_error("ERR_NOT_FOUND")),
                Some((tag, _))
                    if request.headers.get("if-none-match").map(|t| bare(t))
                        == Some(tag.clone()) =>
                {
                    reply(stream, 304, &[("ETag", quoted(&tag))], b"")
                }
                Some((tag, bytes)) => reply(stream, 200, &[("ETag", quoted(&tag))], &bytes),
            },
            "PUT" => {
                let wrong = match (&if_match, &current) {
                    (Some(want), Some((tag, _))) => want != tag,
                    (Some(_), None) => true,
                    _ => if_absent && current.is_some(),
                };
                if wrong {
                    return reply(stream, 412, &[], &json_error("ERR_SYNC_OBJECT_CHANGED"));
                }
                let tag = hex_of(&request.body);
                let revision = f.log.last().map(|e| e.0).unwrap_or(0) + 1;
                f.log.push((revision, name.clone(), false));
                f.files.insert(name, (tag.clone(), request.body));
                reply(stream, 200, &[("ETag", quoted(&tag))], b"{}")
            }
            "DELETE" => {
                if current.is_some() {
                    let revision = f.log.last().map(|e| e.0).unwrap_or(0) + 1;
                    f.log.push((revision, name.clone(), true));
                    f.files.remove(&name);
                }
                reply(stream, 200, &[], b"{}")
            }
            _ => reply(stream, 405, &[], b""),
        }
    }

    fn hex_of(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn fake() -> (String, Arc<Mutex<Fake>>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let address = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("addr").port()
        );
        let state = Arc::new(Mutex::new(Fake::default()));
        let served = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let served = served.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    serve(&served, &mut stream);
                });
            }
        });
        (address, state)
    }

    #[test]
    fn an_organization_certifies_a_key_and_says_why_it_will_not() {
        let (address, fake) = fake();
        fake.lock().expect("fake").valid_token = "nothing yet".into();
        let server = Server::open(&address, "refresh-1").expect("open");
        assert_eq!(
            server.orgs().expect("orgs"),
            [Org {
                id: "o1".into(),
                name: "Team".into()
            }]
        );
        assert_eq!(
            server
                .certificate("o1", "ssh-ed25519 AAAA")
                .expect("certificate"),
            "certified ssh-ed25519 AAAA"
        );
        let no_grant = server.certificate("o2", "ssh-ed25519 AAAA");
        assert!(
            no_grant.is_err_and(|e| e.to_string().contains("no account yet")),
            "nothing granted"
        );
        let elsewhere = server.certificate("o3", "ssh-ed25519 AAAA");
        assert!(
            elsewhere.is_err_and(|e| e.to_string().contains("not in this organization")),
            "not a member"
        );
    }

    #[test]
    fn addresses() {
        assert_eq!(
            check_address(" https://s.example.com/ ").as_deref(),
            Ok("https://s.example.com")
        );
        assert_eq!(
            check_address("http://localhost:10200").as_deref(),
            Ok("http://localhost:10200")
        );
        assert_eq!(
            check_address("http://127.0.0.1:1/x").as_deref(),
            Ok("http://127.0.0.1:1/x")
        );
        assert_eq!(
            check_address("http://[::1]:1").as_deref(),
            Ok("http://[::1]:1")
        );
        // Tokens would cross the network in the clear.
        for bad in [
            "http://s.example.com",
            "http://localhost.example.com",
            "http://127.0.0.1.example.com:1",
            "ftp://s.example.com",
            "s.example.com",
            "",
            "https://",
        ] {
            assert!(check_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn query_text_comes_back_the_same() {
        for text in [
            "plain",
            "a b&c=d/é?",
            "http://127.0.0.1:1/callback",
            "100%",
            "%zz",
            "%4",
        ] {
            assert_eq!(query_decoded(&query_encoded(text)), text);
        }
        assert_eq!(
            query_decoded("%zz"),
            "%zz",
            "a broken escape stays as it is"
        );
        assert_eq!(query_decoded("%4"), "%4");
    }

    #[test]
    fn two_devices_share_a_vault_through_a_server() {
        let dir = std::env::temp_dir().join(format!("shellwing-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let (address, _) = fake();
        let here = Server::open(&address, "refresh-1").expect("open");
        let there = Server::open(&address, "refresh-1").expect("open");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");

        assert!(here.list("records").expect("empty").is_empty());
        assert_eq!(here.read("format.json").expect("none"), None);
        a.set_password("deploy@web", "from-a").expect("save");
        let key = a.start_sync(&here).expect("start").to_string();
        a.sync(&here).expect("sync a");
        assert_eq!(b.folder_state(&there).expect("state"), FolderState::Other);
        b.join(&there, &key).expect("join");
        b.sync(&there).expect("sync b");
        assert_eq!(
            b.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("from-a")
        );

        b.set_password("deploy@web", "from-b").expect("change");
        b.sync(&there).expect("sync b");
        a.sync(&here).expect("sync a");
        assert_eq!(
            a.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("from-b")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_over_what_another_device_changed_is_refused() {
        let (address, _) = fake();
        let one = Server::open(&address, "r").expect("open");
        let two = Server::open(&address, "r").expect("open");
        assert!(one.replace("records/a.sw", b"one").expect("new"));
        assert!(
            !two.replace("records/a.sw", b"two").expect("not new"),
            "it is there"
        );
        assert_eq!(
            two.read("records/a.sw").expect("read").as_deref(),
            Some(&b"one"[..])
        );
        assert!(two.replace("records/a.sw", b"two").expect("seen now"));
        assert!(!one.replace("records/a.sw", b"three").expect("stale"));
        assert_eq!(
            one.read("records/a.sw").expect("read").as_deref(),
            Some(&b"two"[..])
        );

        two.remove("records/a.sw").expect("remove");
        two.remove("records/a.sw").expect("already gone");
        assert!(one.list("records").expect("list").is_empty());
        assert_eq!(one.read("records/a.sw").expect("gone"), None);
    }

    /// A round with nothing new costs the listing, and `format.json`.
    #[test]
    fn what_did_not_change_is_not_fetched_again() {
        let (address, state) = fake();
        let writer = Server::open(&address, "r").expect("open");
        writer.write("format.json", b"{}").expect("write");
        writer.write("records/a.sw", b"one").expect("write");
        writer.write("attachments/i.sw", b"image").expect("write");

        let reader = Server::open(&address, "r").expect("open");
        assert_eq!(reader.list("records").expect("list"), ["a.sw"]);
        assert_eq!(
            reader.read("records/a.sw").expect("read").as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(
            reader.read("format.json").expect("read").as_deref(),
            Some(&b"{}"[..])
        );
        state.lock().expect("fake").requests.clear();

        assert_eq!(reader.list("records").expect("list"), ["a.sw"]);
        assert_eq!(
            reader.read("records/a.sw").expect("read").as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(
            reader.read("format.json").expect("read").as_deref(),
            Some(&b"{}"[..])
        );
        let requests = state.lock().expect("fake").requests.clone();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert!(requests[0].contains("/changes?since=3"), "{requests:?}");
        assert!(requests[1].contains("format.json"), "{requests:?}");

        // A change shows in the next listing, and only it is read.
        writer.write("records/a.sw", b"two").expect("write");
        reader.list("records").expect("list");
        assert_eq!(
            reader.read("records/a.sw").expect("read").as_deref(),
            Some(&b"two"[..])
        );
        // Images are not kept in memory.
        assert_eq!(
            reader.read("attachments/i.sw").expect("read").as_deref(),
            Some(&b"image"[..])
        );
        assert!(
            !reader
                .state()
                .expect("state")
                .kept
                .contains_key("attachments/i.sw")
        );
    }

    #[test]
    fn a_token_the_server_stopped_taking_is_renewed_and_a_revoked_device_is_told() {
        let (address, state) = fake();
        let server = Server::open(&address, "r").expect("open");
        server.write("format.json", b"{}").expect("write");

        // As after the server lost its cache.
        state.lock().expect("fake").valid_token = "another".into();
        server
            .write("format.json", b"[]")
            .expect("renewed by itself");

        let mut f = state.lock().expect("fake");
        f.valid_token = "another".into();
        f.signed_out = true;
        drop(f);
        let e = server.write("format.json", b"{}").expect_err("revoked");
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("sign in"), "{e}");
    }

    /// As a browser comes back: `path` on the app's port.
    fn knock(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("write");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    }

    fn value_in(url: &str, name: &str) -> String {
        let query = url.split_once('?').expect("query").1;
        query
            .split('&')
            .find_map(|p| {
                p.split_once('=')
                    .filter(|(k, _)| *k == name)
                    .map(|(_, v)| query_decoded(v))
            })
            .expect(name)
    }

    #[test]
    fn signing_in_comes_back_with_a_token() {
        let (address, state) = fake();
        let sign_in = SignIn::begin(&address, "my laptop", "linux").expect("begin");
        let url = sign_in.browser_url().to_string();
        assert!(
            url.starts_with(&format!("{address}/connect-device/?")),
            "{url}"
        );
        assert_eq!(value_in(&url, "device_name"), "my laptop");
        assert_eq!(value_in(&url, "platform"), "linux");
        assert_eq!(value_in(&url, "code_challenge").len(), 43);
        assert!(
            !url.contains(&sign_in.verifier),
            "the verifier never goes to the browser"
        );
        let redirect = value_in(&url, "redirect_uri");
        assert!(
            redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"),
            "{redirect}"
        );
        let port: u16 = redirect["http://127.0.0.1:".len()..]
            .trim_end_matches("/callback")
            .parse()
            .expect("port");
        let good = value_in(&url, "state");
        let cancel = AtomicBool::new(false);
        let waiting = std::thread::spawn(move || sign_in.wait(&cancel));

        // Others on this machine knock: none of them gets in.
        assert!(knock(port, "/").starts_with("HTTP/1.1 400"));
        assert!(knock(port, "/favicon.ico").starts_with("HTTP/1.1 400"));
        assert!(knock(port, "/callback?code=stolen&state=wrong").starts_with("HTTP/1.1 400"));
        assert!(knock(port, "/callback?state=wrong").starts_with("HTTP/1.1 400"));

        let answer = knock(port, &format!("/callback?code=the-code&state={good}"));
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(answer.contains("You are signed in"));
        assert_eq!(
            waiting.join().expect("thread").expect("signed in"),
            "refresh-1"
        );
        assert!(
            state
                .lock()
                .expect("fake")
                .requests
                .iter()
                .any(|r| r == "POST /api/devices/token")
        );
    }

    #[test]
    fn signing_in_can_be_called_off() {
        let (address, _) = fake();
        let sign_in = SignIn::begin(&address, "d", "linux").expect("begin");
        let cancel = Arc::new(AtomicBool::new(false));
        let called_off = cancel.clone();
        let waiting = std::thread::spawn(move || sign_in.wait(&called_off));
        cancel.store(true, Ordering::Relaxed);
        let e = waiting.join().expect("thread").expect_err("off");
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn an_identity_is_published_once_until_it_changes() {
        let (address, fake) = fake();
        let server = Server::open(&address, "refresh-1").expect("open");
        assert!(server.takes_identity());
        let identity = shellwing_vault::PublicIdentity {
            x25519: [1; 32],
            ed25519: [2; 32],
            signature: [3; 64],
        };
        let puts = |fake: &Arc<Mutex<Fake>>| {
            let f = fake.lock().expect("lock");
            f.requests
                .iter()
                .filter(|r| r.starts_with("PUT /api/users/current/identity"))
                .count()
        };
        server.publish_identity(&identity).expect("publish");
        server.publish_identity(&identity).expect("again");
        assert_eq!(puts(&fake), 1, "the same one goes once");
        let sent = fake.lock().expect("lock").identity.clone().expect("sent");
        assert_eq!(sent["x25519Public"], B64.encode([1u8; 32]));
        assert_eq!(sent["signature"], B64.encode([3u8; 64]));

        let other = shellwing_vault::PublicIdentity {
            x25519: [4; 32],
            ..identity
        };
        server.publish_identity(&other).expect("another");
        assert_eq!(puts(&fake), 2);
    }

    #[test]
    fn signing_in_refuses_an_address_in_the_clear() {
        assert!(SignIn::begin("http://s.example.com", "d", "linux").is_err());
        assert!(Server::open("http://s.example.com", "r").is_err());
        assert!(sign_out("http://s.example.com", "r").is_err());
    }

    /// The whole of it against a running server: signing in as a browser
    /// would, then two devices sharing a vault, then signing out. Needs the
    /// address, and a user and password there:
    /// `SHELLWING_TEST_SERVER`, `_USER`, `_PASSWORD`.
    #[test]
    fn against_a_real_server() {
        let Ok(address) = std::env::var("SHELLWING_TEST_SERVER") else {
            return;
        };
        let user = std::env::var("SHELLWING_TEST_SERVER_USER").expect("user");
        let password = std::env::var("SHELLWING_TEST_SERVER_PASSWORD").expect("password");
        let dir = std::env::temp_dir().join(format!("shellwing-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");

        let runtime = runtime().expect("runtime");
        let client = client(&runtime).expect("client");
        let post =
            |path: &str, bearer: Option<&str>, body: serde_json::Value| -> serde_json::Value {
                let mut request = client.post(format!("{address}/api{path}")).json(&body);
                if let Some(token) = bearer {
                    request = request.bearer_auth(token);
                }
                let answer = runtime
                    .block_on(async move { request.send().await })
                    .expect("post");
                assert!(answer.status().is_success(), "{path}: {}", answer.status());
                runtime
                    .block_on(async move { answer.json().await })
                    .expect("json")
            };
        let web = post(
            "/auth/login-with-password",
            None,
            serde_json::json!({ "username": user, "password": password }),
        );
        let web = web["data"]["session"]["accessToken"]
            .as_str()
            .expect("session")
            .to_string();

        let sign_in_device = |name: &str| -> String {
            let sign_in = SignIn::begin(&address, name, "linux").expect("begin");
            let url = sign_in.browser_url().to_string();
            let (redirect, state) = (value_in(&url, "redirect_uri"), value_in(&url, "state"));
            let approved = post(
                "/devices/authorize",
                Some(&web),
                serde_json::json!({
                    "redirectUri": redirect,
                    "codeChallenge": value_in(&url, "code_challenge"),
                    "state": state,
                    "deviceName": name,
                    "platform": "linux",
                }),
            );
            let back = approved["data"]["redirectUrl"]
                .as_str()
                .expect("redirect")
                .to_string();
            let cancel = AtomicBool::new(false);
            std::thread::scope(|s| {
                let waiting = s.spawn(|| sign_in.wait(&cancel));
                let port: u16 = redirect["http://127.0.0.1:".len()..]
                    .trim_end_matches("/callback")
                    .parse()
                    .expect("port");
                let path = back.split_once("/callback").expect("path").1;
                assert!(knock(port, &format!("/callback{path}")).starts_with("HTTP/1.1 200"));
                waiting.join().expect("thread").expect("signed in")
            })
        };
        let first = sign_in_device("first");
        let second = sign_in_device("second");

        // A server keeps a person's vault from earlier runs: this one starts over.
        let here = Server::open(&address, &first).expect("open");
        for dir in ["records", "attachments", "pairing"] {
            for name in here.list(dir).expect("list") {
                here.remove(&format!("{dir}/{name}")).expect("remove");
            }
        }
        here.remove("format.json").expect("remove");

        let there = Server::open(&address, &second).expect("open");
        let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
        let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
        a.set_password("deploy@web", "from-a").expect("save");
        let key = a.start_sync(&here).expect("start").to_string();
        a.sync(&here).expect("sync a");
        b.join(&there, &key).expect("join");
        b.sync(&there).expect("sync b");
        assert_eq!(
            b.password("deploy@web")
                .expect("read")
                .map(|p| p.to_string())
                .as_deref(),
            Some("from-a")
        );
        // Both change it, one after the other, without seeing each other first.
        a.set_password("deploy@a", "only-a").expect("save");
        b.set_password("deploy@b", "only-b").expect("save");
        a.sync(&here).expect("sync a");
        b.sync(&there).expect("sync b");
        a.sync(&here).expect("sync a");
        for vault in [&a, &b] {
            assert_eq!(
                vault
                    .password("deploy@a")
                    .expect("read")
                    .map(|p| p.to_string())
                    .as_deref(),
                Some("only-a")
            );
            assert_eq!(
                vault
                    .password("deploy@b")
                    .expect("read")
                    .map(|p| p.to_string())
                    .as_deref(),
                Some("only-b")
            );
        }

        sign_out(&address, &second).expect("sign out");
        let fresh = Server::open(&address, &second).expect("open");
        let e = fresh.list("records").expect_err("signed out");
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        sign_out(&address, &first).expect("sign out");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

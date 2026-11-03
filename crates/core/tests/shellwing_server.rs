//! The app against a real shellwing server: two devices sign in as the browser
//! lets them, share a vault through the server, and one is given an SSH
//! certificate by the organization. Skipped unless `SHELLWING_TEST_SERVER`
//! (its address), `SHELLWING_TEST_SERVER_USER` and
//! `SHELLWING_TEST_SERVER_PASSWORD` name an admin of its organization.
//!
//! It empties that person's synced data and makes a certificate authority if
//! there is none: run it against a server made for testing, as
//! shellwing-server's `scripts/e2e.sh` starts one.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use reqwest::Method;
use serde_json::{Value, json};
use shellwing_remote::Place;
use shellwing_remote::server::{self, Server, SignIn};
use shellwing_vault::{Backend, FolderState, MemoryKeys, Vault};

struct Settings {
    address: String,
    user: String,
    password: String,
}

fn settings() -> Option<Settings> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    Some(Settings {
        address: var("SHELLWING_TEST_SERVER")?
            .trim_end_matches('/')
            .to_string(),
        user: var("SHELLWING_TEST_SERVER_USER")?,
        password: var("SHELLWING_TEST_SERVER_PASSWORD")?,
    })
}

/// The person in the browser: signed in to the web app, with its token.
struct Browser {
    address: String,
    token: String,
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl Browser {
    fn sign_in(s: &Settings) -> Browser {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let client = reqwest::Client::new();
        let mut browser = Browser {
            address: s.address.clone(),
            token: String::new(),
            runtime,
            client,
        };
        let answer = browser.call(
            Method::POST,
            "/auth/login-with-password",
            Some(json!({ "username": s.user, "password": s.password })),
        );
        browser.token = answer["data"]["session"]["accessToken"]
            .as_str()
            .unwrap_or_else(|| panic!("signing in did not give a session: {answer}"))
            .to_string();
        browser
    }

    fn call(&self, method: Method, path: &str, body: Option<Value>) -> Value {
        let mut request = self
            .client
            .request(method.clone(), format!("{}/api{path}", self.address));
        if !self.token.is_empty() {
            request = request.bearer_auth(&self.token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.runtime.block_on(async {
            let answer = request.send().await.expect("the server answers");
            let status = answer.status();
            let text = answer.text().await.expect("an answer");
            assert!(status.is_success(), "{method} {path}: {status}: {text}");
            serde_json::from_str(&text).unwrap_or(Value::Null)
        })
    }

    /// Approves a device, as the connect-device page does, and gives its
    /// refresh token.
    fn connect(&self, device: &str) -> String {
        let sign_in = SignIn::begin(&self.address, device, "linux").expect("begin");
        let url = sign_in.browser_url().to_string();
        let query = |name: &str| {
            let (_, query) = url.split_once('?').expect("a query");
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
                .map(decoded)
                .unwrap_or_else(|| panic!("{name} is not in {url}"))
        };
        let answer = self.call(
            Method::POST,
            "/devices/authorize",
            Some(json!({
                "redirectUri": query("redirect_uri"),
                "codeChallenge": query("code_challenge"),
                "state": query("state"),
                "deviceName": query("device_name"),
                "platform": query("platform"),
            })),
        );
        let back = answer["data"]["redirectUrl"]
            .as_str()
            .expect("where the browser goes")
            .to_string();
        // The browser goes back to the app, which answers it while it waits.
        let browser = std::thread::spawn(move || get(&back));
        let token = sign_in.wait(&AtomicBool::new(false)).expect("tokens");
        assert!(browser.join().expect("browser").starts_with("HTTP/1.1 200"));
        token
    }
}

fn decoded(text: &str) -> String {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).expect("hex"));
                i += 3;
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
    String::from_utf8(out).expect("utf-8")
}

/// A plain GET of an `http://127.0.0.1:<port>/…` address: the app's port.
fn get(url: &str) -> String {
    let rest = url.strip_prefix("http://").expect("http");
    let (host, path) = rest.split_once('/').expect("a path");
    let mut stream = TcpStream::connect(host).expect("the app's port");
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .expect("ask");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("answer");
    answer
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shellwing-server-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// Removes what earlier runs left in the person's space.
fn empty(b: &dyn Backend) {
    for dir in ["records", "pairing", "attachments"] {
        for name in b.list(dir).expect("list") {
            b.remove(&format!("{dir}/{name}")).expect("remove");
        }
    }
    b.remove("format.json").expect("remove");
}

#[test]
fn two_devices_share_a_vault_and_one_is_given_a_certificate() {
    let Some(s) = settings() else {
        eprintln!("skipped: SHELLWING_TEST_SERVER, _USER and _PASSWORD are not set");
        return;
    };
    let browser = Browser::sign_in(&s);
    let (token_a, token_b) = (browser.connect("e2e a"), browser.connect("e2e b"));
    let place = |token: &str| Place::Server {
        url: s.address.clone(),
        refresh_token: token.to_string(),
    };
    let (a_place, b_place) = (
        place(&token_a).open().expect("a's place"),
        place(&token_b).open().expect("b's place"),
    );
    empty(a_place.as_ref());

    // A starts syncing; B joins with the recovery key.
    let dir = temp("vault");
    let a = Vault::open(&dir.join("a.db"), &MemoryKeys::default()).expect("a");
    let b = Vault::open(&dir.join("b.db"), &MemoryKeys::default()).expect("b");
    a.set_password("deploy@web", "from-a").expect("save");
    let recovery = a.start_sync(a_place.as_ref()).expect("start").to_string();
    a.sync(a_place.as_ref()).expect("sync a");
    assert_eq!(
        b.folder_state(b_place.as_ref()).expect("state"),
        FolderState::Other
    );
    b.join(b_place.as_ref(), &recovery).expect("join");
    b.sync(b_place.as_ref()).expect("sync b");
    let password = |v: &Vault, t: &str| v.password(t).expect("read").map(|p| p.to_string());
    assert_eq!(password(&b, "deploy@web").as_deref(), Some("from-a"));

    // A change goes back the other way.
    b.set_password("root@db", "from-b").expect("save");
    b.sync(b_place.as_ref()).expect("sync b");
    a.sync(a_place.as_ref()).expect("sync a");
    assert_eq!(password(&a, "root@db").as_deref(), Some("from-b"));

    // The organization certifies A's key for the accounts it is granted.
    let orgs = Server::open(&s.address, &token_a)
        .expect("a's server")
        .orgs()
        .expect("orgs");
    let org = &orgs.first().expect("an organization").id;
    let cas = browser.call(Method::GET, &format!("/orgs/{org}/ssh/cas"), None);
    let active = cas["data"]
        .as_array()
        .and_then(|cas| cas.iter().find(|ca| ca["status"] == "active"))
        .cloned();
    let ca = match active {
        Some(ca) => ca,
        None => browser.call(Method::POST, &format!("/orgs/{org}/ssh/cas"), None)["data"].clone(),
    };
    let me = browser.call(Method::GET, "/sessions/me", None);
    let principals = format!(
        "/orgs/{org}/ssh/members/{}/principals",
        me["data"]["user"]["id"].as_str().expect("my id")
    );
    browser.call(
        Method::PUT,
        &principals,
        Some(json!({ "principals": ["e2e"] })),
    );
    let certified =
        shellwing_core::certs::certified(&s.address, &token_a, org).expect("a certificate");
    assert_eq!(certified.certificate.valid_principals(), ["e2e"]);
    let ca_key = russh::keys::PublicKey::from_openssh(ca["publicKey"].as_str().expect("its key"))
        .expect("the CA's key");
    assert_eq!(certified.certificate.signature_key(), ca_key.key_data());

    // Without accounts, no new certificate.
    browser.call(Method::PUT, &principals, Some(json!({ "principals": [] })));
    shellwing_core::certs::forget();
    let refused = shellwing_core::certs::certified(&s.address, &token_a, org)
        .expect_err("no accounts, no certificate");
    assert!(refused.contains("account"), "{refused}");

    // A device that signs out can no longer sync.
    server::sign_out(&s.address, &token_b).expect("sign out");
    let after = place(&token_b).open().expect("b's place");
    assert!(b.sync(after.as_ref()).is_err());
}

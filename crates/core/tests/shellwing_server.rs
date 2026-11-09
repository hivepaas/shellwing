//! The app against a real shellwing server: two devices sign in as the browser
//! lets them, share a vault through the server, and one is given an SSH
//! certificate by the organization; three people share a collection, and one
//! loses access to it. Skipped unless `SHELLWING_TEST_SERVER`
//! (its address), `SHELLWING_TEST_SERVER_USER` and
//! `SHELLWING_TEST_SERVER_PASSWORD` name an admin of its organization.
//!
//! It empties that person's synced data and makes a certificate authority if
//! there is none: run it against a server made for testing, as
//! shellwing-server's `scripts/e2e.sh` starts one.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use reqwest::Method;
use serde_json::{Value, json};
use shellwing_remote::Place;
use shellwing_remote::server::{self, Server, SignIn};
use shellwing_vault::{
    Backend, CollectionMember, CollectionState, Collections, FolderState, MemoryKeys, Space, Vault,
};

/// The tests share the admin, whose identity and space they change: one at a
/// time.
static SERVER: Mutex<()> = Mutex::new(());

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
    let _one = SERVER.lock().unwrap_or_else(|e| e.into_inner());
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

    // A's identity goes to B through sync, and to the server, which checks it.
    let identity = a.ensure_identity().expect("identity").public();
    a.sync(a_place.as_ref()).expect("sync a");
    a_place.publish_identity(&identity).expect("publish");
    b.sync(b_place.as_ref()).expect("sync b");
    assert_eq!(
        b.identity().expect("read").expect("synced").public(),
        identity
    );
    let published = browser.call(Method::GET, "/users/current/identity", None);
    assert_eq!(published["data"]["fingerprint"], identity.fingerprint());

    // A collection made on the web has no key: A's app makes it, and B, the
    // same person, opens it with the identity it synced.
    let org_id = Server::open(&s.address, &token_a)
        .expect("a's server")
        .orgs()
        .expect("orgs")
        .first()
        .expect("an organization")
        .id
        .clone();
    let made = browser.call(
        Method::POST,
        &format!("/orgs/{org_id}/collections"),
        Some(json!({ "name": format!("e2e {}", std::process::id()) })),
    );
    let col = made["data"]["id"].as_str().expect("its id").to_string();
    let a_host = a_place.collections().expect("a server serves collections");
    let synced = |v: &Vault, host: &dyn shellwing_vault::Collections| {
        let out = v.sync_collections(host).expect("sync collections");
        out.into_iter()
            .find(|o| o.collection.id == col)
            .expect("listed")
            .state
    };
    assert!(matches!(synced(&a, a_host), CollectionState::Synced { .. }));
    let space = Space::Collection(col.clone());
    let values = [("address".to_string(), "10.1.2.3".to_string())].into();
    a.update_in(
        &space,
        "c0110000000000000000000000000001",
        "host",
        "db",
        &values,
    )
    .expect("save in the collection");
    assert!(matches!(
        synced(&a, a_host),
        CollectionState::Synced { pushed: 1, .. }
    ));
    let b_host = b_place.collections().expect("b's server");
    assert!(matches!(
        synced(&b, b_host),
        CollectionState::Synced { pulled: 1, .. }
    ));
    assert_eq!(
        b.entries_in(&space, "host").expect("entries")[0].1["address"],
        "10.1.2.3"
    );
    let detail = browser.call(
        Method::GET,
        &format!("/orgs/{org_id}/collections/{col}"),
        None,
    );
    assert_eq!(detail["data"]["collection"]["keyVersion"], 1);
    assert_eq!(detail["data"]["collection"]["hasKey"], true);
    browser.call(
        Method::DELETE,
        &format!("/orgs/{org_id}/collections/{col}"),
        None,
    );

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

/// Somebody the admin invited, signed up, with a device of theirs and a vault
/// whose identity the server has.
struct Person {
    id: String,
    server: Server,
    vault: Vault,
}

impl Person {
    fn invite(admin: &Browser, s: &Settings, name: &str, dir: &std::path::Path) -> Person {
        let username = format!("e2e-{name}-{}", std::process::id());
        let password = format!("E2e-{}-Pass1!", std::process::id());
        let invited = admin.call(
            Method::POST,
            "/users/invite",
            Some(json!({
                "email": format!("{username}@example.test"),
                "role": "member",
                "securityOption": "password-only",
                "sendInviteEmail": false,
                // A day: the people of a run do not outlast it by much.
                "accessExpireAt": (chrono::Utc::now() + chrono::Duration::days(1))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "capabilities": null,
                "moduleAccesses": null,
            })),
        );
        let link = invited["data"]["inviteLink"]
            .as_str()
            .expect("an invite link");
        let (_, token) = link.split_once("token=").expect("its token");
        let open = Browser {
            address: s.address.clone(),
            token: String::new(),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
            client: reqwest::Client::new(),
        };
        open.call(
            Method::POST,
            "/users/signup-complete",
            Some(json!({
                "inviteToken": decoded(token),
                "username": username,
                "fullName": name,
                "password": password,
                "passcode": "",
                "mfaTotpSecret": "",
                "photo": null,
            })),
        );
        let browser = Browser::sign_in(&Settings {
            address: s.address.clone(),
            user: username,
            password,
        });
        let me = browser.call(Method::GET, "/sessions/me", None);
        let id = me["data"]["user"]["id"]
            .as_str()
            .expect("their id")
            .to_string();
        let server = Server::open(&s.address, &browser.connect(&format!("e2e {name}")))
            .expect("their server");
        let vault = Vault::open(&dir.join(format!("{name}.db")), &MemoryKeys::default())
            .expect("their vault");
        let identity = vault.ensure_identity().expect("identity").public();
        server.publish_identity(&identity).expect("publish");
        Person { id, server, vault }
    }

    /// How syncing the collection `col` went for them; None: not served.
    fn sync(&self, col: &str) -> Option<shellwing_vault::CollectionOutcome> {
        self.vault
            .sync_collections(&self.server)
            .expect("sync collections")
            .into_iter()
            .find(|o| o.collection.id == col)
    }

    fn fingerprint(&self) -> String {
        self.vault
            .identity()
            .expect("read")
            .expect("identity")
            .public()
            .fingerprint()
    }
}

#[test]
fn three_people_share_a_collection_and_one_loses_access() {
    let Some(s) = settings() else {
        eprintln!("skipped: SHELLWING_TEST_SERVER, _USER and _PASSWORD are not set");
        return;
    };
    let _one = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let admin = Browser::sign_in(&s);
    let dir = temp("people");
    let alice = Person::invite(&admin, &s, "alice", &dir);
    let bob = Person::invite(&admin, &s, "bob", &dir);
    let carol = Person::invite(&admin, &s, "carol", &dir);
    let org_id = alice.server.orgs().expect("orgs")[0].id.clone();

    // The admin makes the collection on the web and grants it: the web has no
    // key, so Alice's app, with Manage, makes the first.
    let made = admin.call(
        Method::POST,
        &format!("/orgs/{org_id}/collections"),
        Some(json!({ "name": format!("e2e shared {}", std::process::id()) })),
    );
    let col = made["data"]["id"].as_str().expect("its id").to_string();
    let grants = |people: &[(&Person, &str)]| {
        let grants: Vec<Value> = people
            .iter()
            .map(|(p, permission)| json!({ "userId": p.id, "permission": permission }))
            .collect();
        admin.call(
            Method::PUT,
            &format!("/orgs/{org_id}/collections/{col}/grants"),
            Some(json!({ "grants": grants })),
        );
    };
    grants(&[(&alice, "manage"), (&bob, "edit"), (&carol, "edit")]);
    assert_eq!(
        bob.sync(&col).expect("served").state,
        CollectionState::Waiting
    );
    assert!(matches!(
        alice.sync(&col).expect("served").state,
        CollectionState::Synced { .. }
    ));

    // Alice gives it to who waits, once each fingerprint is what their own
    // app shows.
    let info = alice.sync(&col).expect("served").collection;
    let waiting: Vec<CollectionMember> = alice
        .server
        .collection_members(&info)
        .expect("members")
        .into_iter()
        .filter(|m| !m.has_key && m.identity.is_some())
        .collect();
    let mut ids: Vec<&str> = waiting.iter().map(|m| m.user_id.as_str()).collect();
    ids.sort();
    let mut expected = [bob.id.as_str(), carol.id.as_str()];
    expected.sort();
    assert_eq!(ids, expected);
    for m in &waiting {
        let them = if m.user_id == bob.id { &bob } else { &carol };
        assert_eq!(
            m.identity.as_ref().expect("identity").fingerprint(),
            them.fingerprint()
        );
    }
    alice
        .vault
        .give_collection_key(&alice.server, &info, &waiting)
        .expect("give");

    // A host and its password, written by Alice, read by the others; Bob's
    // change comes back.
    let space = Space::Collection(col.clone());
    let host = "c0220000000000000000000000000001";
    let address = |v: &str| [("address".to_string(), v.to_string())].into();
    alice
        .vault
        .update_in(&space, host, "host", "db", &address("10.2.0.1"))
        .expect("save");
    let secret = [
        ("target".to_string(), "deploy@10.2.0.1:22".to_string()),
        ("password".to_string(), "shared-secret".to_string()),
    ]
    .into();
    alice
        .vault
        .update_in(
            &space,
            "c0220000000000000000000000000002",
            "password",
            "db password",
            &secret,
        )
        .expect("save");
    assert!(matches!(
        alice.sync(&col).expect("served").state,
        CollectionState::Synced { pushed: 2, .. }
    ));
    for p in [&bob, &carol] {
        assert!(matches!(
            p.sync(&col).expect("served").state,
            CollectionState::Synced { pulled: 2, .. }
        ));
        assert_eq!(
            p.vault
                .password_in(&space, "deploy@10.2.0.1:22")
                .expect("read")
                .as_deref()
                .map(|p| p.as_str()),
            Some("shared-secret")
        );
    }
    bob.vault
        .update_in(&space, host, "host", "db", &address("10.2.0.2"))
        .expect("edit");
    bob.sync(&col).expect("served");
    alice.sync(&col).expect("served");
    assert_eq!(
        alice.vault.entries_in(&space, "host").expect("entries")[0].1["address"],
        "10.2.0.2"
    );

    // Carol loses access: Alice's app replaces the key, for who still has
    // it, and lists the password to change.
    grants(&[(&alice, "manage"), (&bob, "edit")]);
    assert!(carol.sync(&col).is_none(), "no longer served");
    let replaced = alice.sync(&col).expect("served");
    assert_eq!(replaced.replaced, Some(2));
    assert!(matches!(replaced.state, CollectionState::Synced { .. }));
    let lists = alice.vault.key_checklists().expect("lists");
    assert_eq!(
        lists
            .iter()
            .find(|l| l.collection == col)
            .expect("its list")
            .secrets,
        ["db password"]
    );
    let detail = admin.call(
        Method::GET,
        &format!("/orgs/{org_id}/collections/{col}"),
        None,
    );
    assert_eq!(detail["data"]["collection"]["keyVersion"], 2);
    assert_eq!(detail["data"]["collection"]["needsNewKey"], false);

    // Every record in the store is sealed with the new key, which Bob takes.
    let store = alice.server.collection_store(&info).expect("store");
    let names = store.list("records").expect("records");
    assert_eq!(names.len(), 2);
    for name in names {
        let bytes = store
            .read(&format!("records/{name}"))
            .expect("read")
            .expect("there");
        let envelope: Value = serde_json::from_slice(&bytes).expect("an envelope");
        assert_eq!(envelope["key"], 2, "{name}");
    }
    assert!(bob.sync(&col).expect("served").replaced.is_none());
    assert_eq!(bob.vault.collection_versions(&col), [1, 2]);
    assert_eq!(
        bob.vault.entries_in(&space, "host").expect("entries")[0].1["address"],
        "10.2.0.2"
    );

    admin.call(
        Method::DELETE,
        &format!("/orgs/{org_id}/collections/{col}"),
        None,
    );
    for p in [&alice, &bob, &carol] {
        admin.call(Method::DELETE, &format!("/users/{}", p.id), None);
    }
}

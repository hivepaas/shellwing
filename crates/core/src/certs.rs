//! SSH certificates from the organizations on the shellwing server this
//! device syncs with. Each organization certifies a key made for it here and
//! held in memory while the app runs: only its public half leaves, and it is
//! never written anywhere. Another key and certificate are asked for when
//! most of the time of the last is gone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use shellwing_remote::server::Server;
use shellwing_ssh::Certified;
use shellwing_ssh::keys::{KeyKind, generate};

/// A certificate is renewed once less than this part of its time is left
/// (a tenth: 48 minutes of 8 hours).
const RENEW_WHEN_LEFT: u64 = 10;

#[derive(Default)]
struct Held {
    /// The server, as it was opened: its address and the device's token.
    server: Option<(String, String, Arc<Server>)>,
    /// By server address and organization.
    certified: HashMap<(String, String), Certified>,
}

fn held() -> &'static Mutex<Held> {
    static HELD: OnceLock<Mutex<Held>> = OnceLock::new();
    HELD.get_or_init(Mutex::default)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Still good for a while at `now`.
fn fresh(c: &Certified, now: u64) -> bool {
    let (after, before) = c.valid();
    let left = before.saturating_sub(after) * RENEW_WHEN_LEFT / 100;
    after <= now && now.saturating_add(left) < before
}

/// `org`'s certificate, from the server at `address` this device signed in to
/// with `token`. It blocks while it asks; one asks at a time.
pub fn certified(address: &str, token: &str, org: &str) -> Result<Certified, String> {
    let mut held = held()
        .lock()
        .map_err(|_| "certificates: a lock was poisoned".to_string())?;
    let server = match &held.server {
        Some((a, t, server)) if a == address && t == token => server.clone(),
        _ => {
            let server = Arc::new(Server::open(address, token).map_err(|e| e.to_string())?);
            held.server = Some((address.into(), token.into(), server.clone()));
            server
        }
    };
    certified_from(&mut held.certified, address, org, now(), |public| {
        server.certificate(org, public).map_err(|e| e.to_string())
    })
}

/// The one in `certified` while it is fresh, else a new key's, with the
/// certificate `ask` gets for its public half.
fn certified_from(
    certified: &mut HashMap<(String, String), Certified>,
    address: &str,
    org: &str,
    now: u64,
    ask: impl FnOnce(&str) -> Result<String, String>,
) -> Result<Certified, String> {
    let at = (address.to_string(), org.to_string());
    if let Some(c) = certified.get(&at).filter(|c| fresh(c, now)) {
        return Ok(c.clone());
    }
    let key = generate(KeyKind::Ed25519, "shellwing").map_err(|e| e.to_string())?;
    let public = key.public_key().to_openssh().map_err(|e| e.to_string())?;
    let c = Certified::new(key, &ask(&public)?)?;
    certified.insert(at, c.clone());
    Ok(c)
}

/// Lets go of every key and certificate: this device no longer syncs with
/// that server, or signs in to it as someone else.
pub fn forget() {
    if let Ok(mut held) = held().lock() {
        *held = Held::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::PrivateKey;
    use russh::keys::ssh_key::PublicKey;
    use russh::keys::ssh_key::certificate::{Builder, CertType};

    /// What an organization's CA signs: `public` for `deploy`, from `after`
    /// for `hours`.
    fn sign(ca: &PrivateKey, public: &str, after: u64, hours: u64) -> String {
        let public = PublicKey::from_openssh(public).expect("public");
        let mut b = Builder::new(
            [7u8; 16].to_vec(),
            public.key_data().clone(),
            after,
            after + hours * 3600,
        )
        .expect("builder");
        b.cert_type(CertType::User).expect("type");
        b.key_id("someone").expect("id");
        b.valid_principal("deploy").expect("principal");
        b.sign(ca).expect("sign").to_openssh().expect("line")
    }

    fn key() -> PrivateKey {
        generate(KeyKind::Ed25519, "test").expect("key")
    }

    #[test]
    fn a_certificate_is_kept_until_most_of_its_time_is_gone() {
        let ca = key();
        let mut held = HashMap::new();
        let start = 1_800_000_000;
        let asked = std::cell::Cell::new(0);
        let mut get = |now: u64| {
            certified_from(&mut held, "https://s", "o1", now, |public| {
                asked.set(asked.get() + 1);
                Ok(sign(&ca, public, start, 8))
            })
            .expect("certified")
        };
        let first = get(start + 60);
        let later = get(start + 7 * 3600);
        assert_eq!(asked.get(), 1, "seven hours in: the same");
        assert_eq!(first.key.public_key(), later.key.public_key());
        let renewed = get(start + 7 * 3600 + 20 * 60);
        assert_eq!(asked.get(), 2, "under 48 minutes left: another");
        assert_ne!(
            first.key.public_key(),
            renewed.key.public_key(),
            "a new key with it"
        );
    }

    #[test]
    fn a_certificate_for_another_key_or_none_is_not_kept() {
        let ca = key();
        let other = key().public_key().to_openssh().expect("line");
        let mut held = HashMap::new();
        let wrong = certified_from(&mut held, "https://s", "o1", 1, |_| {
            Ok(sign(&ca, &other, 0, 8))
        });
        assert!(wrong.is_err_and(|e| e.contains("another key")));
        let refused = certified_from(&mut held, "https://s", "o1", 1, |_| {
            Err("you may log in as no account yet".into())
        });
        assert!(refused.is_err_and(|e| e.contains("no account")));
        assert!(held.is_empty());
    }
}

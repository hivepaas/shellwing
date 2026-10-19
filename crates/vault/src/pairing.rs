//! Pairing: a device that syncs lets a new one into its vault through the sync
//! folder, without the recovery key being typed (docs/design/vault.md, Keys).
//!
//! The new device (the asker) and one that syncs (the answerer) agree a key by
//! X25519, and both show a six-digit code made from the two public keys. Once
//! the user sees the same code on both screens and approves, the answerer
//! seals the vault's key and its recovery key with the agreed key.
//!
//! Whoever can write the folder could put keys of their own in between; the
//! two codes would then differ. So that they cannot pick keys until the codes
//! happen to match, the asker commits to its key (a hash of it) before it sees
//! the answerer's, and shows the key only after: neither key can be chosen
//! knowing the other, and a forged pair matches by chance one time in a
//! million.
//!
//! Each file in `pairing/` is written by one side only:
//!
//! - `<id>.ask.json`, the asker: its device name and the hash of its key.
//! - `<id>.answer.json`, the answerer: its device name and its key.
//! - `<id>.reveal.json`, the asker: its key, and the answer it took.
//! - `<id>.key.json`, the answerer: the vault's keys sealed for the asker, or
//!   that it refused.
//!
//! A request is dropped after [`LIFETIME`].

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use curve25519_dalek::montgomery::MontgomeryPoint;
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::recovery::RecoveryKey;
use crate::sync::{Backend, FolderState};
use crate::{Error, KEY_LEN, Result, Vault, cipher, random};

const DIR: &str = "pairing";
/// How long a request stays open.
pub const LIFETIME_MS: u64 = 10 * 60 * 1000;
const COMMIT: &[u8] = b"shellwing pairing commit v1";
const CODE: &[u8] = b"shellwing pairing code v1";
const SESSION: &[u8] = b"shellwing pairing key v1";
const SEALED: &[u8] = b"shellwing pairing keys v1";

type Key = [u8; KEY_LEN];

#[derive(Serialize, Deserialize)]
struct AskFile {
    v: u32,
    device: String,
    commit: String,
    at: u64,
}

#[derive(Serialize, Deserialize)]
struct AnswerFile {
    v: u32,
    device: String,
    public: String,
}

#[derive(Serialize, Deserialize)]
struct RevealFile {
    v: u32,
    public: String,
    /// The answerer's key it took: another device may have answered too.
    answer: String,
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    v: u32,
    #[serde(default)]
    refused: bool,
    /// The vault's key, then its recovery key, sealed with the agreed key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
}

fn now_ms() -> u64 {
    crate::now_ms() as u64
}

fn file(id: &str, part: &str) -> String {
    format!("{DIR}/{id}.{part}.json")
}

fn read<T: for<'de> Deserialize<'de>>(b: &dyn Backend, name: &str) -> Result<Option<T>> {
    match b.read(name)? {
        Some(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        None => Ok(None),
    }
}

fn write<T: Serialize>(b: &dyn Backend, name: &str, value: &T) -> Result<()> {
    Ok(b.write(name, &serde_json::to_vec(value)?)?)
}

/// Removes every file of request `id`.
fn forget(b: &dyn Backend, id: &str) -> Result<()> {
    for part in ["ask", "answer", "reveal", "key"] {
        b.remove(&file(id, part))?;
    }
    Ok(())
}

fn key_from(text: &str) -> Option<Key> {
    B64.decode(text).ok()?.try_into().ok()
}

/// A key pair for one request: the secret, and the public key.
fn key_pair() -> Result<(Zeroizing<Key>, Key)> {
    let secret = Zeroizing::new(random::<KEY_LEN>()?);
    let public = MontgomeryPoint::mul_base_clamped(*secret).to_bytes();
    Ok((secret, public))
}

/// What the two agree, from one's secret and the other's public key.
fn agree(secret: &Key, theirs: &Key) -> Result<Zeroizing<Key>> {
    let shared = Zeroizing::new(MontgomeryPoint(*theirs).mul_clamped(*secret).to_bytes());
    // A low-order key gives everyone the same result: refused.
    if *shared == [0; KEY_LEN] {
        return Err(Error::Pairing(
            "the other device's key is not usable".into(),
        ));
    }
    Ok(shared)
}

fn commitment(public: &Key) -> String {
    B64.encode(
        Sha256::new()
            .chain_update(COMMIT)
            .chain_update(public)
            .finalize(),
    )
}

/// The code both screens show: six digits from the two keys.
fn code(answer: &Key, ask: &Key) -> String {
    let h = Sha256::new()
        .chain_update(CODE)
        .chain_update(answer)
        .chain_update(ask)
        .finalize();
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// The key the vault's keys travel under.
fn session_key(shared: &Key, answer: &Key, ask: &Key, id: &str) -> Result<Zeroizing<Key>> {
    let mut salt = Vec::with_capacity(2 * KEY_LEN);
    salt.extend_from_slice(answer);
    salt.extend_from_slice(ask);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(&[SESSION, id.as_bytes()].concat(), &mut key[..])
        .map_err(|_| Error::Pairing("cannot derive a key".into()))?;
    Ok(key)
}

fn sealed_aad(id: &str) -> Vec<u8> {
    [SEALED, id.as_bytes()].concat()
}

/// Where a request to join stands, on the device asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskState {
    /// No device that syncs has answered yet.
    Waiting,
    /// `device` answered: both show `code`, and the user approves there.
    Compare {
        device: String,
        code: String,
    },
    /// In: this device now has the vault's keys.
    Joined,
    Refused,
    /// Nobody approved in time.
    Expired,
}

/// A request to join the vault in a sync folder, from the device asking.
pub struct Ask {
    id: String,
    secret: Zeroizing<Key>,
    public: Key,
    at: u64,
    /// The answer taken: the answerer's key and device name.
    answer: Option<(Key, String)>,
}

impl Ask {
    /// Asks to join the vault in `b`, as `device` (a name to show there).
    pub fn new(b: &dyn Backend, device: &str) -> Result<Ask> {
        let (secret, public) = key_pair()?;
        let id: String = random::<12>()?.iter().map(|b| format!("{b:02x}")).collect();
        let at = now_ms();
        write(
            b,
            &file(&id, "ask"),
            &AskFile {
                v: 1,
                device: device.into(),
                commit: commitment(&public),
                at,
            },
        )?;
        Ok(Ask {
            id,
            secret,
            public,
            at,
            answer: None,
        })
    }

    /// Looks for what the other side wrote and does the next step; joins
    /// `vault` to the folder's vault once approved.
    pub fn step(&mut self, vault: &Vault, b: &dyn Backend) -> Result<AskState> {
        if now_ms().saturating_sub(self.at) > LIFETIME_MS {
            forget(b, &self.id)?;
            return Ok(AskState::Expired);
        }
        let Some((answer, device)) = self.answer.clone() else {
            let Some(a) = read::<AnswerFile>(b, &file(&self.id, "answer"))? else {
                return Ok(AskState::Waiting);
            };
            let Some(key) = key_from(&a.public) else {
                return Ok(AskState::Waiting);
            };
            // Now that its key is known, ours is shown.
            write(
                b,
                &file(&self.id, "reveal"),
                &RevealFile {
                    v: 1,
                    public: B64.encode(self.public),
                    answer: a.public.clone(),
                },
            )?;
            self.answer = Some((key, a.device.clone()));
            return Ok(AskState::Compare {
                device: a.device,
                code: code(&key, &self.public),
            });
        };
        let compare = AskState::Compare {
            device,
            code: code(&answer, &self.public),
        };
        let Some(k) = read::<KeyFile>(b, &file(&self.id, "key"))? else {
            return Ok(compare);
        };
        if k.refused {
            forget(b, &self.id)?;
            return Ok(AskState::Refused);
        }
        let Some(sealed) = k.sealed.and_then(|s| B64.decode(s).ok()) else {
            return Ok(compare);
        };
        let shared = agree(&self.secret, &answer)?;
        let session = session_key(&shared, &answer, &self.public, &self.id)?;
        let (plain, _) = cipher::open(&session, &sealed, &sealed_aad(&self.id))
            .map_err(|_| Error::Pairing("the keys sent cannot be opened".into()))?;
        if plain.len() != 2 * KEY_LEN {
            return Err(Error::Pairing("the keys sent are malformed".into()));
        }
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        key.copy_from_slice(&plain[..KEY_LEN]);
        let mut rk = Zeroizing::new([0u8; KEY_LEN]);
        rk.copy_from_slice(&plain[KEY_LEN..]);
        if !vault.is_key_of(b, &key)? {
            return Err(Error::Pairing(
                "the key sent is not the folder's vault's".into(),
            ));
        }
        vault.adopt(key, &RecoveryKey(rk))?;
        forget(b, &self.id)?;
        Ok(AskState::Joined)
    }

    /// Withdraws the request.
    pub fn cancel(&self, b: &dyn Backend) -> Result<()> {
        forget(b, &self.id)
    }
}

/// A request a device that syncs can approve: the user checks that `device`
/// shows `code`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub device: String,
    pub code: String,
}

struct Answering {
    secret: Zeroizing<Key>,
    public: Key,
    device: String,
    commit: String,
    /// The asker's key, once shown and checked against its commitment.
    asker: Option<Key>,
}

/// Answers requests to join, on a device that syncs; it keeps its keys for
/// them between looks at the folder.
pub struct Answerer {
    device: String,
    open: HashMap<String, Answering>,
}

impl Answerer {
    /// `device` is this device's name, shown to the one asking.
    pub fn new(device: &str) -> Answerer {
        Answerer {
            device: device.into(),
            open: HashMap::new(),
        }
    }

    /// Whether a request is in progress, so the folder is looked at often.
    pub fn busy(&self) -> bool {
        !self.open.is_empty()
    }

    /// Answers new requests in `b` and returns those ready for the user:
    /// the asker has shown its key, so the code can be compared.
    pub fn poll(&mut self, vault: &Vault, b: &dyn Backend) -> Result<Vec<Request>> {
        if vault.folder_state(b)? != FolderState::Ours {
            self.open.clear();
            return Ok(vec![]);
        }
        let now = now_ms();
        let mut seen = vec![];
        for name in b.list(DIR)? {
            let Some(id) = name.strip_suffix(".ask.json") else {
                continue;
            };
            let Some(ask) = read::<AskFile>(b, &format!("{DIR}/{name}"))? else {
                continue;
            };
            if now.saturating_sub(ask.at) > LIFETIME_MS {
                forget(b, id)?;
                self.open.remove(id);
                continue;
            }
            seen.push(id.to_string());
            if !self.open.contains_key(id) {
                // Answered already, by another device: theirs to finish.
                if b.read(&file(id, "answer"))?.is_some() {
                    continue;
                }
                let (secret, public) = key_pair()?;
                write(
                    b,
                    &file(id, "answer"),
                    &AnswerFile {
                        v: 1,
                        device: self.device.clone(),
                        public: B64.encode(public),
                    },
                )?;
                self.open.insert(
                    id.to_string(),
                    Answering {
                        secret,
                        public,
                        device: ask.device,
                        commit: ask.commit,
                        asker: None,
                    },
                );
            }
        }
        self.open.retain(|id, _| seen.contains(id));
        let mut ready = vec![];
        let mut wrong = vec![];
        for (id, a) in &mut self.open {
            if a.asker.is_none()
                && let Some(r) = read::<RevealFile>(b, &file(id, "reveal"))?
            {
                if r.answer != B64.encode(a.public) {
                    // It took another device's answer.
                    wrong.push((id.clone(), false));
                    continue;
                }
                match key_from(&r.public).filter(|k| commitment(k) == a.commit) {
                    Some(k) => a.asker = Some(k),
                    // Not the key it committed to: someone is in between.
                    None => {
                        wrong.push((id.clone(), true));
                        continue;
                    }
                }
            }
            if let Some(asker) = &a.asker {
                ready.push(Request {
                    id: id.clone(),
                    device: a.device.clone(),
                    code: code(&a.public, asker),
                });
            }
        }
        for (id, refuse) in wrong {
            if refuse {
                self.refuse(b, &id)?;
            } else {
                self.open.remove(&id);
            }
        }
        ready.sort_by(|x, y| x.id.cmp(&y.id));
        Ok(ready)
    }

    /// Lets the asker of `id` in: seals the vault's key and recovery key for it.
    pub fn approve(&mut self, vault: &Vault, b: &dyn Backend, id: &str) -> Result<()> {
        let a = self
            .open
            .remove(id)
            .ok_or_else(|| Error::Pairing("that request is gone".into()))?;
        let asker = a
            .asker
            .ok_or_else(|| Error::Pairing("the other device has not shown its key".into()))?;
        let rk = vault
            .recovery_key()?
            .ok_or_else(|| Error::Pairing("this device has no recovery key to send".into()))?;
        let shared = agree(&a.secret, &asker)?;
        let session = session_key(&shared, &a.public, &asker, id)?;
        let mut plain = Zeroizing::new(Vec::with_capacity(2 * KEY_LEN));
        plain.extend_from_slice(&vault.sk()[..]);
        plain.extend_from_slice(&rk.0[..]);
        let sealed = cipher::seal(vault.cipher, &session, &plain, &sealed_aad(id))?;
        write(
            b,
            &file(id, "key"),
            &KeyFile {
                v: 1,
                refused: false,
                sealed: Some(B64.encode(sealed)),
            },
        )
    }

    /// Turns the asker of `id` away.
    pub fn refuse(&mut self, b: &dyn Backend, id: &str) -> Result<()> {
        self.open.remove(id);
        write(
            b,
            &file(id, "key"),
            &KeyFile {
                v: 1,
                refused: true,
                sealed: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::Folder;
    use crate::{MemoryKeys, Vault};

    fn device(root: &std::path::Path, name: &str) -> Vault {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("dir");
        Vault::open(&dir.join("vault.db"), &MemoryKeys::default()).expect("vault")
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shellwing-pair-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_new_device_joins_once_the_codes_match_and_it_is_approved() {
        let root = temp("join");
        let folder = Folder::new(&root.join("sync"));
        let (a, b) = (device(&root, "a"), device(&root, "b"));
        a.set_password("deploy@api:22", "hunter2").expect("save");
        let rk = a.start_sync(&folder).expect("start");
        a.sync(&folder).expect("sync");

        let mut ask = Ask::new(&folder, "laptop").expect("ask");
        assert_eq!(ask.step(&b, &folder).expect("step"), AskState::Waiting);
        let mut answerer = Answerer::new("desktop");
        assert!(
            answerer.poll(&a, &folder).expect("poll").is_empty(),
            "no key shown yet"
        );
        let AskState::Compare { device, code } = ask.step(&b, &folder).expect("step") else {
            panic!("no answer");
        };
        assert_eq!(device, "desktop");
        let ready = answerer.poll(&a, &folder).expect("poll");
        assert_eq!(ready.len(), 1);
        assert_eq!(
            (ready[0].device.as_str(), ready[0].code.as_str()),
            ("laptop", code.as_str())
        );
        answerer
            .approve(&a, &folder, &ready[0].id)
            .expect("approve");
        assert_eq!(ask.step(&b, &folder).expect("step"), AskState::Joined);

        // It reads the vault, and has its recovery key to show.
        b.sync(&folder).expect("sync");
        assert_eq!(
            b.password("deploy@api:22")
                .expect("read")
                .as_deref()
                .map(|s| s.as_str()),
            Some("hunter2")
        );
        assert_eq!(
            b.recovery_key().expect("rk").map(|k| k.to_string()),
            Some(rk.to_string())
        );
        assert!(
            folder.list(DIR).expect("list").is_empty(),
            "its files are gone"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_key_swapped_in_between_is_caught_and_refused_turns_away() {
        let root = temp("swap");
        let folder = Folder::new(&root.join("sync"));
        let (a, b) = (device(&root, "a"), device(&root, "b"));
        a.start_sync(&folder).expect("start");

        let mut ask = Ask::new(&folder, "laptop").expect("ask");
        let mut answerer = Answerer::new("desktop");
        answerer.poll(&a, &folder).expect("poll");
        ask.step(&b, &folder).expect("step");
        // Someone shows another key than the one committed to.
        let (_, other) = key_pair().expect("pair");
        let name = file(&ask.id, "reveal");
        let mut r: RevealFile = read(&folder, &name).expect("read").expect("reveal");
        r.public = B64.encode(other);
        write(&folder, &name, &r).expect("write");
        assert!(answerer.poll(&a, &folder).expect("poll").is_empty());
        assert_eq!(ask.step(&b, &folder).expect("step"), AskState::Refused);

        // Refused by hand, too.
        let mut ask = Ask::new(&folder, "laptop").expect("ask");
        answerer.poll(&a, &folder).expect("poll");
        ask.step(&b, &folder).expect("step");
        let ready = answerer.poll(&a, &folder).expect("poll");
        answerer.refuse(&folder, &ready[0].id).expect("refuse");
        assert_eq!(ask.step(&b, &folder).expect("step"), AskState::Refused);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_codes_come_from_both_keys() {
        let (_, x) = key_pair().expect("pair");
        let (_, y) = key_pair().expect("pair");
        let c = code(&x, &y);
        assert_eq!(c.len(), 7);
        assert_ne!(c, code(&y, &x), "order matters");
        let (s1, p1) = key_pair().expect("pair");
        let (s2, p2) = key_pair().expect("pair");
        assert_eq!(
            *agree(&s1, &p2).expect("agree"),
            *agree(&s2, &p1).expect("agree")
        );
        assert!(
            agree(&s1, &[0; KEY_LEN]).is_err(),
            "a low-order key is refused"
        );
    }
}

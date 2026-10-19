//! SSH keys: making one, reading the ones there are (files in `~/.ssh`, keys
//! in the agent, keys the app keeps), writing one out, and adding a public
//! key to a server's `authorized_keys`.
//!
//! Keys the app keeps are OpenSSH private keys without a passphrase: the vault
//! they live in is encrypted already. One written out to a file can have one.

use std::path::{Path, PathBuf};

pub use russh::keys::ssh_key;
use russh::keys::ssh_key::{Algorithm, EcdsaCurve, HashAlg, LineEnding, PrivateKey, PublicKey};
pub use zeroize::Zeroizing;

use crate::Error;

/// The kinds of key the app makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Short and fast; OpenSSH 6.5 (2014) and later take it.
    Ed25519,
    EcdsaP256,
    /// For very old servers.
    Rsa4096,
}

/// What is shown of a key: no secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInfo {
    /// `ED25519`, `ECDSA P-256`, `RSA 3072`…
    pub kind: String,
    /// `SHA256:…`, as `ssh-keygen -l` writes it.
    pub fingerprint: String,
    pub comment: String,
    /// One line, as `authorized_keys` takes it: `ssh-ed25519 AAAA… comment`.
    pub public: String,
}

/// A key file found on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyFile {
    pub path: PathBuf,
    pub info: KeyInfo,
    /// It has a passphrase.
    pub encrypted: bool,
}

/// A key kept in the vault, as authentication gets it (see
/// [`crate::Secrets::keys`]).
pub struct StoredKey {
    pub id: String,
    pub name: String,
    /// The private key, in OpenSSH's format.
    pub openssh: Zeroizing<String>,
}

fn bad(e: impl std::fmt::Display) -> Error {
    Error::Key(e.to_string())
}

/// Makes a new key.
pub fn generate(kind: KeyKind, comment: &str) -> Result<PrivateKey, Error> {
    let mut rng = rand::rng();
    let mut key = match kind {
        KeyKind::Ed25519 => PrivateKey::random(&mut rng, Algorithm::Ed25519),
        KeyKind::EcdsaP256 => PrivateKey::random(
            &mut rng,
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256,
            },
        ),
        KeyKind::Rsa4096 => PrivateKey::random(&mut rng, Algorithm::Rsa { hash: None }),
    }
    .map_err(bad)?;
    key.set_comment(comment);
    Ok(key)
}

/// How a key is named to people: `ED25519`, `ECDSA P-256`, `RSA 4096`.
fn kind_of(key: &PublicKey) -> String {
    match key.algorithm() {
        Algorithm::Ed25519 => "ED25519".into(),
        Algorithm::Ecdsa { curve } => match curve {
            EcdsaCurve::NistP256 => "ECDSA P-256".into(),
            EcdsaCurve::NistP384 => "ECDSA P-384".into(),
            EcdsaCurve::NistP521 => "ECDSA P-521".into(),
        },
        Algorithm::Rsa { .. } => {
            let bits = key
                .key_data()
                .rsa()
                .and_then(|k| k.n().as_positive_bytes())
                .map(|n| n.len() * 8 - n.first().map_or(0, |b| b.leading_zeros() as usize));
            match bits {
                Some(b) => format!("RSA {b}"),
                None => "RSA".into(),
            }
        }
        Algorithm::SkEd25519 => "ED25519-SK".into(),
        Algorithm::SkEcdsaSha2NistP256 => "ECDSA-SK".into(),
        other => other.as_str().to_uppercase(),
    }
}

/// What is shown of a public key.
pub fn info(key: &PublicKey) -> KeyInfo {
    KeyInfo {
        kind: kind_of(key),
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        comment: key.comment().to_string(),
        public: key.to_openssh().unwrap_or_default(),
    }
}

/// Why a key could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// It has a passphrase, and none (or a wrong one) was given.
    Passphrase,
    /// Not an OpenSSH private key.
    NotAKey(String),
}

/// Reads a private key in OpenSSH's format, unlocking it with `passphrase`
/// when it has one.
pub fn parse(text: &str, passphrase: Option<&str>) -> Result<PrivateKey, ReadError> {
    let key =
        PrivateKey::from_openssh(text.trim()).map_err(|e| ReadError::NotAKey(e.to_string()))?;
    if !key.is_encrypted() {
        return Ok(key);
    }
    match passphrase {
        Some(p) if !p.is_empty() => key.decrypt(p).map_err(|_| ReadError::Passphrase),
        _ => Err(ReadError::Passphrase),
    }
}

/// A key as the vault keeps it: OpenSSH's format, no passphrase.
pub fn to_vault(key: &PrivateKey) -> Result<Zeroizing<String>, Error> {
    key.to_openssh(LineEnding::LF).map_err(bad)
}

/// A key as a file for ssh: OpenSSH's format, locked with `passphrase` when
/// one is given.
pub fn export(key: &PrivateKey, passphrase: Option<&str>) -> Result<Zeroizing<String>, Error> {
    match passphrase.filter(|p| !p.is_empty()) {
        Some(p) => key
            .encrypt(&mut rand::rng(), p)
            .map_err(bad)?
            .to_openssh(LineEnding::LF)
            .map_err(bad),
        None => to_vault(key),
    }
}

/// Writes `text` to a new file only its owner can read, as ssh insists for
/// a private key, and the public key beside it (`<path>.pub`).
pub fn write_files(path: &Path, text: &str, public: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    open.open(path)?.write_all(text.as_bytes())?;
    let mut pub_path = path.as_os_str().to_owned();
    pub_path.push(".pub");
    std::fs::write(PathBuf::from(pub_path), format!("{public}\n"))
}

/// The private key files in `dir` (`~/.ssh`): every file that reads as an
/// OpenSSH private key, by name. Their public half is read without the
/// passphrase.
pub fn key_files(dir: &Path) -> Vec<KeyFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut found: Vec<KeyFile> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            // Small files only: a key is a few KB; config and known_hosts are skipped by name.
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            !name.ends_with(".pub")
                && !matches!(
                    name,
                    "config" | "known_hosts" | "known_hosts.old" | "authorized_keys"
                )
                && std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() < 64 * 1024)
        })
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            if !text
                .trim_start()
                .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
            {
                return None;
            }
            let key = PrivateKey::from_openssh(text.trim()).ok()?;
            // A key with a passphrase keeps its comment locked away: the
            // `.pub` beside it has it, when it is the same key.
            let mut public = key.public_key().clone();
            if public.comment().is_empty()
                && let Some(p) = pub_beside(&path)
                && p.key_data() == public.key_data()
            {
                public = p;
            }
            Some(KeyFile {
                info: info(&public),
                encrypted: key.is_encrypted(),
                path,
            })
        })
        .collect();
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// The public key in `<path>.pub`, if it reads as one.
fn pub_beside(path: &Path) -> Option<PublicKey> {
    let mut p = path.as_os_str().to_owned();
    p.push(".pub");
    PublicKey::from_openssh(std::fs::read_to_string(PathBuf::from(p)).ok()?.trim()).ok()
}

/// The keys the platform's ssh-agent holds (see [`crate::auth`]).
pub async fn agent_keys(socket: Option<&Path>) -> Vec<KeyInfo> {
    let mut keys = vec![];
    for mut agent in crate::auth::agents(socket).await {
        for id in agent.request_identities().await.unwrap_or_default() {
            keys.push(info(&id.public_key()));
        }
    }
    keys
}

/// Hands `key` to the platform's ssh-agent (the first there is), for
/// `lifetime` when given, else until the agent stops. ssh, git and a
/// forwarded agent can then sign with it.
pub async fn add_to_agent(
    key: &PrivateKey,
    lifetime: Option<std::time::Duration>,
    socket: Option<&Path>,
) -> Result<(), Error> {
    let constraints: Vec<_> = lifetime
        .map(|d| russh::keys::agent::Constraint::KeyLifetime {
            seconds: d.as_secs().min(u32::MAX as u64) as u32,
        })
        .into_iter()
        .collect();
    let mut agent = crate::auth::agents(socket)
        .await
        .into_iter()
        .next()
        .ok_or_else(|| Error::Key("no ssh-agent is running".into()))?;
    agent.add_identity(key, &constraints).await.map_err(bad)
}

/// Takes a key out of the ssh-agent again.
pub async fn remove_from_agent(public: &PublicKey, socket: Option<&Path>) -> Result<(), Error> {
    let mut agent = crate::auth::agents(socket)
        .await
        .into_iter()
        .next()
        .ok_or_else(|| Error::Key("no ssh-agent is running".into()))?;
    agent.remove_identity(public).await.map_err(bad)
}

/// Run on the server, with the public key line as its input: adds it to
/// `~/.ssh/authorized_keys` unless it is there, making the folder and the
/// file as sshd wants them (private to the user). Prints `added` or
/// `present`. Under `sh`, whatever the login shell is.
pub const INSTALL: &str = r#"exec sh -c 'umask 077
IFS= read -r key || [ -n "$key" ] || exit 3
mkdir -p "$HOME/.ssh" && cd "$HOME/.ssh" && touch authorized_keys || exit 4
if grep -qxF -e "$key" authorized_keys; then echo present; exit 0; fi
if [ -s authorized_keys ] && [ -n "$(tail -c 1 authorized_keys)" ]; then echo >> authorized_keys; fi
printf "%s\n" "$key" >> authorized_keys || exit 5
command -v restorecon >/dev/null 2>&1 && restorecon -F "$HOME/.ssh" "$HOME/.ssh/authorized_keys" >/dev/null 2>&1
echo added'"#;

/// How [`INSTALL`] went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    Added,
    /// The key was there already.
    Present,
}

/// Reads what [`INSTALL`] printed and how it ended.
pub fn installed(status: Option<u32>, stdout: &[u8], stderr: &[u8]) -> Result<Installed, String> {
    let out = String::from_utf8_lossy(stdout);
    match (status, out.trim()) {
        (Some(0), "added") => Ok(Installed::Added),
        (Some(0), "present") => Ok(Installed::Present),
        _ => {
            let err = String::from_utf8_lossy(stderr).trim().to_string();
            Err(if err.is_empty() {
                format!(
                    "the server did not add the key (exit status {})",
                    status.map_or("unknown".to_string(), |s| s.to_string())
                )
            } else {
                err
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_keys_read_back_with_and_without_a_passphrase() {
        for kind in [KeyKind::Ed25519, KeyKind::EcdsaP256] {
            let key = generate(kind, "me@here").expect("generate");
            let i = info(key.public_key());
            assert!(i.fingerprint.starts_with("SHA256:"));
            assert!(i.public.ends_with(" me@here"));
            let plain = to_vault(&key).expect("vault");
            assert_eq!(
                parse(&plain, None).expect("plain").public_key(),
                key.public_key()
            );
            let locked = export(&key, Some("secret")).expect("export");
            assert_eq!(parse(&locked, None).err(), Some(ReadError::Passphrase));
            assert_eq!(
                parse(&locked, Some("wrong")).err(),
                Some(ReadError::Passphrase)
            );
            let back = parse(&locked, Some("secret")).expect("unlocked");
            assert_eq!(back.public_key(), key.public_key());
        }
        assert_eq!(
            info(generate(KeyKind::Ed25519, "").expect("k").public_key()).kind,
            "ED25519"
        );
        assert_eq!(
            info(generate(KeyKind::EcdsaP256, "").expect("k").public_key()).kind,
            "ECDSA P-256"
        );
        assert!(matches!(
            parse("not a key", None),
            Err(ReadError::NotAKey(_))
        ));
    }

    #[test]
    fn key_files_are_found_and_written_private() {
        let dir = std::env::temp_dir().join(format!("shellwing-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let key = generate(KeyKind::Ed25519, "file@test").expect("generate");
        let text = export(&key, Some("pw")).expect("export");
        let public = info(key.public_key()).public;
        write_files(&dir.join("id_test"), &text, &public).expect("write");
        // Never over a key that is there.
        assert!(write_files(&dir.join("id_test"), &text, &public).is_err());
        std::fs::write(dir.join("config"), "Host *\n").expect("config");
        std::fs::write(dir.join("notes"), "hello").expect("notes");
        let found = key_files(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, dir.join("id_test"));
        assert!(found[0].encrypted);
        assert_eq!(found[0].info.public, public);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("id_test"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real ssh-agent, on a socket of its own: a kept key goes in and out.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn keys_go_into_the_agent_and_out() {
        let Ok(found) = std::process::Command::new("sh")
            .args(["-c", "command -v ssh-agent"])
            .output()
        else {
            return;
        };
        if !found.status.success() {
            eprintln!("skipped: no ssh-agent");
            return;
        }
        let dir = std::env::temp_dir().join(format!("shellwing-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let sock = dir.join("agent.sock");
        let mut agent = std::process::Command::new("ssh-agent")
            .arg("-D")
            .arg("-a")
            .arg(&sock)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("ssh-agent");
        for _ in 0..100 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let key = generate(KeyKind::Ed25519, "agent@test").expect("key");
        let fp = info(key.public_key()).fingerprint;
        add_to_agent(&key, Some(std::time::Duration::from_secs(600)), Some(&sock))
            .await
            .expect("add");
        let listed = agent_keys(Some(&sock)).await;
        assert!(listed.iter().any(|k| k.fingerprint == fp), "{listed:?}");
        remove_from_agent(key.public_key(), Some(&sock))
            .await
            .expect("remove");
        assert!(
            agent_keys(Some(&sock))
                .await
                .iter()
                .all(|k| k.fingerprint != fp)
        );
        let _ = agent.kill();
        let _ = agent.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The install script under a real `sh`, in a home of its own.
    #[cfg(unix)]
    #[test]
    fn install_adds_a_key_once_and_keeps_what_is_there() {
        use std::io::Write;
        let home = std::env::temp_dir().join(format!("shellwing-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".ssh")).expect("home");
        // An existing key without a newline at the end.
        std::fs::write(home.join(".ssh/authorized_keys"), "ssh-ed25519 AAAAold old").expect("old");
        let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDx new 'quoted' $comment";
        let run = || {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(INSTALL)
                .env("HOME", &home)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("sh");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(format!("{line}\n").as_bytes())
                .expect("write");
            let out = child.wait_with_output().expect("wait");
            installed(
                out.status.code().map(|c| c as u32),
                &out.stdout,
                &out.stderr,
            )
        };
        assert_eq!(run(), Ok(Installed::Added));
        assert_eq!(run(), Ok(Installed::Present));
        let keys = std::fs::read_to_string(home.join(".ssh/authorized_keys")).expect("read");
        assert_eq!(keys, format!("ssh-ed25519 AAAAold old\n{line}\n"));
        let _ = std::fs::remove_dir_all(&home);
    }
}

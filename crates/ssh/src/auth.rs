//! Client authentication, in OpenSSH's order: agent keys, key files (asking for
//! passphrases), keyboard-interactive (one-time codes, or PAM asking the password),
//! then password. Methods the server no longer offers are skipped, and each
//! interactive method gets three tries.

use std::path::PathBuf;
use std::sync::Arc;

use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::ssh_key::Certificate;
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh::{MethodKind, MethodSet};

use crate::hostkey::Checker;
use crate::prompt::{Prompt, Prompter};
use crate::secrets::{Learned, Secrets};
use crate::{Auth, Error, Target};

const TRIES: usize = 3;
/// Rounds of methods: a server may want more than one ("password, then a
/// code"), saying each time one is accepted that more is needed.
const ROUNDS: usize = 3;

type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// The ssh-agents this platform offers: `socket`, else `$SSH_AUTH_SOCK` on
/// Unix; on Windows the OpenSSH agent's named pipe, then Pageant.
pub(crate) async fn agents(socket: Option<&std::path::Path>) -> Vec<Agent> {
    let mut found = Vec::new();
    #[cfg(unix)]
    {
        let agent = match socket {
            Some(path) => AgentClient::connect_uds(path).await,
            None => AgentClient::connect_env().await,
        };
        if let Ok(a) = agent {
            found.push(a.dynamic());
        }
    }
    #[cfg(not(unix))]
    let _ = socket;
    #[cfg(windows)]
    {
        if let Ok(a) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
            found.push(a.dynamic());
        }
        if let Ok(a) = AgentClient::connect_pageant().await {
            found.push(a.dynamic());
        }
    }
    found
}

/// Tracks which methods the server still accepts (`None` until it has told
/// us), and whether one was accepted with more needed (a partial success).
#[derive(Default)]
struct Methods {
    allowed: Option<Vec<MethodKind>>,
    partial: bool,
}

impl Methods {
    fn allows(&self, m: MethodKind) -> bool {
        self.allowed.as_ref().is_none_or(|v| v.contains(&m))
    }

    fn set(&mut self, remaining: &MethodSet, partial: bool) {
        self.allowed = Some(remaining.iter().copied().collect());
        self.partial |= partial;
    }

    fn update(&mut self, r: &AuthResult) {
        if let AuthResult::Failure {
            remaining_methods,
            partial_success,
        } = r
        {
            self.set(remaining_methods, *partial_success);
        }
    }
}

/// Whether a keyboard-interactive question asks for the account password, as PAM
/// does ("Password:"): hidden, naming a password, and not one being changed.
fn asks_password(text: &str, echo: bool) -> bool {
    let text = text.to_lowercase();
    !echo
        && text.contains("password")
        && !["new", "retype", "again", "old", "current"]
            .iter()
            .any(|w| text.contains(w))
}

/// The SHA256 fingerprint of a key file's public half, readable without the
/// passphrase.
fn fingerprint(path: &PathBuf) -> Option<String> {
    let key = russh::keys::ssh_key::PrivateKey::read_openssh_file(path).ok()?;
    Some(key.public_key().fingerprint(Default::default()).to_string())
}

/// Loads a key file, trying a saved passphrase, then asking. `None`: skip the key.
async fn load_key(
    path: &PathBuf,
    prompter: Option<&Prompter>,
    secrets: Option<&dyn Secrets>,
) -> Result<Option<PrivateKey>, Error> {
    match russh::keys::load_secret_key(path, None) {
        Ok(k) => return Ok(Some(k)),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(_) => return Ok(None), // unreadable or not a key: try the next one, like ssh
    }
    let fp = fingerprint(path);
    if let (Some(s), Some(fp)) = (secrets, &fp)
        && let Some(saved) = s.passphrase(fp)
        && let Ok(k) = russh::keys::load_secret_key(path, Some(&saved))
    {
        return Ok(Some(k));
    }
    let Some(prompter) = prompter else {
        return Ok(None);
    };
    let key = path.display().to_string();
    for attempt in 0..TRIES {
        let Some(pass) = prompter
            .ask(Prompt::Passphrase {
                key: key.clone(),
                retry: attempt > 0,
            })
            .await
        else {
            return Err(Error::Cancelled);
        };
        if pass.is_empty() {
            return Ok(None); // Enter alone skips the key, as in ssh
        }
        if let Ok(k) = russh::keys::load_secret_key(path, Some(&pass)) {
            if let (Some(s), Some(fp)) = (secrets, fp) {
                s.learned(Learned::Passphrase {
                    fingerprint: fp,
                    key_path: key,
                    passphrase: pass,
                });
            }
            return Ok(Some(k));
        }
    }
    Ok(None)
}

/// Which of the keys the app keeps to offer.
pub(crate) enum StoredKeys<'a> {
    All,
    Only(&'a str),
    None,
}

impl<'a> StoredKeys<'a> {
    /// A host's: its own key, else every one unless it names its key files.
    pub(crate) fn of(hop: &'a crate::Resolved) -> Self {
        match (&hop.vault_key, hop.identity_files.is_empty()) {
            (Some(id), _) => StoredKeys::Only(id),
            (None, true) => StoredKeys::All,
            (None, false) => StoredKeys::None,
        }
    }

    fn wants(&self, id: &str) -> bool {
        match self {
            StoredKeys::All => true,
            StoredKeys::Only(only) => *only == id,
            StoredKeys::None => false,
        }
    }
}

/// A certificate for `key`: one of `certs` (`CertificateFile`), else
/// `<key file>-cert.pub` beside it, as OpenSSH looks for one.
pub(crate) fn certificate_for(
    key: &PrivateKey,
    file: Option<&std::path::Path>,
    certs: &[PathBuf],
) -> Option<Certificate> {
    let beside = file.map(|f| {
        let mut name = f.as_os_str().to_owned();
        name.push("-cert.pub");
        PathBuf::from(name)
    });
    certs.iter().chain(beside.iter()).find_map(|path| {
        let text = std::fs::read_to_string(path).ok()?;
        let cert = Certificate::from_openssh(text.trim()).ok()?;
        (cert.public_key() == key.public_key().key_data()).then_some(cert)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn authenticate(
    handle: &mut Handle<Checker>,
    target: &Target,
    keys: &[PathBuf],
    certs: &[PathBuf],
    stored: StoredKeys<'_>,
    auth: &Auth,
    prompter: Option<&Prompter>,
    secrets: Option<&dyn Secrets>,
) -> Result<(), Error> {
    let user = target.user.as_str();
    let mut methods = Methods::default();

    // Each round tries the methods in order; a partial success (one accepted,
    // more needed) starts another with the methods the server names.
    for _ in 0..ROUNDS {
        methods.partial = false;

        if auth.agent && methods.allows(MethodKind::PublicKey) {
            for mut agent in agents(auth.agent_socket.as_deref()).await {
                for id in agent.request_identities().await.unwrap_or_default() {
                    // A certificate the agent holds signs in as itself.
                    if let AgentIdentity::Certificate { certificate, .. } = id {
                        let hash = if certificate.algorithm().is_rsa() {
                            handle.best_supported_rsa_hash().await?.flatten()
                        } else {
                            None
                        };
                        if let Ok(r) = handle
                            .authenticate_certificate_with(user, certificate, hash, &mut agent)
                            .await
                        {
                            if r.success() {
                                return Ok(());
                            }
                            methods.update(&r);
                        }
                        continue;
                    }
                    let key = id.public_key().into_owned();
                    let hash = if key.algorithm().is_rsa() {
                        handle.best_supported_rsa_hash().await?.flatten()
                    } else {
                        None
                    };
                    if let Ok(r) = handle
                        .authenticate_publickey_with(user, key, hash, &mut agent)
                        .await
                    {
                        if r.success() {
                            return Ok(());
                        }
                        methods.update(&r);
                    }
                }
            }
        }

        if methods.allows(MethodKind::PublicKey) {
            for path in keys {
                let Some(key) = load_key(path, prompter, secrets).await? else {
                    continue;
                };
                // With a certificate for it, that first.
                if let Some(cert) = certificate_for(&key, Some(path), certs) {
                    let r = handle
                        .authenticate_openssh_cert(user, Arc::new(key.clone()), cert)
                        .await?;
                    if r.success() {
                        return Ok(());
                    }
                    methods.update(&r);
                }
                let hash = if key.algorithm().is_rsa() {
                    handle.best_supported_rsa_hash().await?.flatten()
                } else {
                    None
                };
                let r = handle
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                    .await?;
                if r.success() {
                    return Ok(());
                }
                methods.update(&r);
            }
        }

        // Keys the app keeps: no passphrase, the vault is encrypted already.
        if methods.allows(MethodKind::PublicKey)
            && let Some(s) = secrets
            && !matches!(stored, StoredKeys::None)
        {
            for k in s.keys().into_iter().filter(|k| stored.wants(&k.id)) {
                let Ok(key) = crate::keys::parse(&k.openssh, None) else {
                    continue;
                };
                if let Some(cert) = certificate_for(&key, None, certs) {
                    let r = handle
                        .authenticate_openssh_cert(user, Arc::new(key.clone()), cert)
                        .await?;
                    if r.success() {
                        return Ok(());
                    }
                    methods.update(&r);
                }
                let hash = if key.algorithm().is_rsa() {
                    handle.best_supported_rsa_hash().await?.flatten()
                } else {
                    None
                };
                let r = handle
                    .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                    .await?;
                if r.success() {
                    return Ok(());
                }
                methods.update(&r);
            }
        }

        // Saved under user@host:port; asked as OpenSSH asks, without the port.
        let (key, shown) = (target.to_string(), format!("{user}@{}", target.host));
        let saved = secrets.and_then(|s| s.password(&key));

        if let Some(prompter) = prompter
            && methods.allows(MethodKind::KeyboardInteractive)
        {
            // PAM asks for the password here, so the known ones answer it before the user.
            let mut known = auth.password.iter().chain(saved.iter());
            for _ in 0..TRIES {
                // The password the user typed in this attempt, to offer saving.
                let mut typed = None;
                let mut reply = handle
                    .authenticate_keyboard_interactive_start(user, None)
                    .await?;
                loop {
                    match reply {
                        KeyboardInteractiveAuthResponse::Success => {
                            if let (Some(s), Some(password)) = (secrets, typed.take()) {
                                s.learned(Learned::Password {
                                    target: key.clone(),
                                    password,
                                });
                            }
                            return Ok(());
                        }
                        KeyboardInteractiveAuthResponse::Failure {
                            remaining_methods,
                            partial_success,
                        } => {
                            methods.set(&remaining_methods, partial_success);
                            break;
                        }
                        KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. }
                            if prompts.len() == 1
                                && asks_password(&prompts[0].prompt, prompts[0].echo) =>
                        {
                            let pw = match known.next() {
                                Some(pw) => {
                                    typed = None;
                                    pw.clone()
                                }
                                None => {
                                    let q = Prompt::Question {
                                        name: String::new(),
                                        instructions: String::new(),
                                        text: prompts[0].prompt.clone(),
                                        echo: false,
                                    };
                                    let pw = prompter.ask(q).await.ok_or(Error::Cancelled)?;
                                    typed = Some(pw.clone());
                                    pw
                                }
                            };
                            reply = handle
                                .authenticate_keyboard_interactive_respond(vec![pw])
                                .await?;
                        }
                        KeyboardInteractiveAuthResponse::InfoRequest {
                            name,
                            instructions,
                            prompts,
                        } => {
                            let mut answers = Vec::with_capacity(prompts.len());
                            for (i, p) in prompts.iter().enumerate() {
                                // Show the round's name and instructions once, with its first question.
                                let (name, instructions) = if i == 0 {
                                    (name.clone(), instructions.clone())
                                } else {
                                    Default::default()
                                };
                                let q = Prompt::Question {
                                    name,
                                    instructions,
                                    text: p.prompt.clone(),
                                    echo: p.echo,
                                };
                                answers.push(prompter.ask(q).await.ok_or(Error::Cancelled)?);
                            }
                            reply = handle
                                .authenticate_keyboard_interactive_respond(answers)
                                .await?;
                        }
                    }
                }
                if !methods.allows(MethodKind::KeyboardInteractive) {
                    break;
                }
            }
        }

        if methods.allows(MethodKind::Password) {
            for pw in auth.password.iter().chain(saved.iter()) {
                if !methods.allows(MethodKind::Password) {
                    break;
                }
                let r = handle.authenticate_password(user, pw).await?;
                if r.success() {
                    return Ok(());
                }
                methods.update(&r);
            }
            if let Some(prompter) = prompter {
                for attempt in 0..TRIES {
                    if !methods.allows(MethodKind::Password) {
                        break;
                    }
                    let prompt = Prompt::Password {
                        target: shown.clone(),
                        retry: attempt > 0,
                    };
                    let pw = prompter.ask(prompt).await.ok_or(Error::Cancelled)?;
                    let r = handle.authenticate_password(user, &pw).await?;
                    // Right, whether or not more is needed after it: worth saving.
                    let right = r.success()
                        || matches!(
                            r,
                            AuthResult::Failure {
                                partial_success: true,
                                ..
                            }
                        );
                    if right && let Some(s) = secrets {
                        s.learned(Learned::Password {
                            target: key.clone(),
                            password: pw,
                        });
                    }
                    if r.success() {
                        return Ok(());
                    }
                    methods.update(&r);
                }
            }
        }
        if !methods.partial {
            break;
        }
    }
    Err(Error::AuthFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pam_password_prompts_count_as_the_password() {
        assert!(asks_password("Password:", false));
        assert!(asks_password("(deploy@10.0.0.5) Password: ", false));
        assert!(asks_password("Password for deploy@host: ", false));
        assert!(asks_password("deploy's password: ", false));
    }

    #[test]
    fn other_questions_are_not_the_password() {
        assert!(!asks_password("Verification code: ", false));
        assert!(!asks_password(
            "One-time password (OATH) for `deploy':",
            true
        ));
        assert!(!asks_password("Password:", true));
        assert!(!asks_password("New password: ", false));
        assert!(!asks_password("Retype new password: ", false));
        assert!(!asks_password("(current) UNIX password: ", false));
    }
}

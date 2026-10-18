//! Client authentication, in OpenSSH's order: agent keys, key files (asking for
//! passphrases), keyboard-interactive (one-time codes), then password. Methods the
//! server no longer offers are skipped, and each interactive method gets three tries.

use std::path::PathBuf;
use std::sync::Arc;

use russh::MethodKind;
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};

use crate::hostkey::Checker;
use crate::prompt::{Prompt, Prompter};
use crate::{Auth, Error};

const TRIES: usize = 3;

type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// The ssh-agents this platform offers: `$SSH_AUTH_SOCK` on Unix; on Windows the
/// OpenSSH agent's named pipe, then Pageant.
async fn agents() -> Vec<Agent> {
    let mut found = Vec::new();
    #[cfg(unix)]
    if let Ok(a) = AgentClient::connect_env().await {
        found.push(a.dynamic());
    }
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

/// Tracks which methods the server still accepts; `None` until it has told us.
struct Methods(Option<Vec<MethodKind>>);

impl Methods {
    fn allows(&self, m: MethodKind) -> bool {
        self.0.as_ref().is_none_or(|v| v.contains(&m))
    }

    fn update(&mut self, r: &AuthResult) {
        if let AuthResult::Failure {
            remaining_methods, ..
        } = r
        {
            self.0 = Some(remaining_methods.iter().copied().collect());
        }
    }
}

/// Loads a key file, asking for its passphrase if it has one. `None`: skip the key.
async fn load_key(
    path: &PathBuf,
    prompter: Option<&Prompter>,
) -> Result<Option<PrivateKey>, Error> {
    match russh::keys::load_secret_key(path, None) {
        Ok(k) => return Ok(Some(k)),
        Err(russh::keys::Error::KeyIsEncrypted) => {}
        Err(_) => return Ok(None), // unreadable or not a key: try the next one, like ssh
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
            return Ok(Some(k));
        }
    }
    Ok(None)
}

pub(crate) async fn authenticate(
    handle: &mut Handle<Checker>,
    user: &str,
    host: &str,
    keys: &[PathBuf],
    auth: &Auth,
    prompter: Option<&Prompter>,
) -> Result<(), Error> {
    let mut methods = Methods(None);

    if auth.agent && methods.allows(MethodKind::PublicKey) {
        for mut agent in agents().await {
            for id in agent.request_identities().await.unwrap_or_default() {
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
            let Some(key) = load_key(path, prompter).await? else {
                continue;
            };
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

    if let Some(prompter) = prompter
        && methods.allows(MethodKind::KeyboardInteractive)
    {
        for _ in 0..TRIES {
            let mut reply = handle
                .authenticate_keyboard_interactive_start(user, None)
                .await?;
            loop {
                match reply {
                    KeyboardInteractiveAuthResponse::Success => return Ok(()),
                    KeyboardInteractiveAuthResponse::Failure {
                        remaining_methods, ..
                    } => {
                        methods.0 = Some(remaining_methods.iter().copied().collect());
                        break;
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
        if let Some(pw) = &auth.password {
            let r = handle.authenticate_password(user, pw).await?;
            if r.success() {
                return Ok(());
            }
            methods.update(&r);
        }
        if let Some(prompter) = prompter {
            let target = format!("{user}@{host}");
            for attempt in 0..TRIES {
                if !methods.allows(MethodKind::Password) {
                    break;
                }
                let prompt = Prompt::Password {
                    target: target.clone(),
                    retry: attempt > 0,
                };
                let pw = prompter.ask(prompt).await.ok_or(Error::Cancelled)?;
                let r = handle.authenticate_password(user, &pw).await?;
                if r.success() {
                    return Ok(());
                }
                methods.update(&r);
            }
        }
    }
    Err(Error::AuthFailed)
}

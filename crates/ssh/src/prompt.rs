//! Questions the connection needs a person to answer.
//!
//! The SSH code never reads input itself. It sends a [`PromptRequest`] and waits
//! for the answer, so the app can ask inside the terminal (as `ssh` does), in a
//! dialog, or from a test script. Wording follows OpenSSH so it reads familiar.

use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// An unknown host key; answer `yes` to trust and record it.
    HostKey {
        host: String,
        algorithm: String,
        fingerprint: String,
        /// The previous answer was neither yes nor no.
        again: bool,
    },
    Password {
        /// `user@host`.
        target: String,
        /// The previous password was refused.
        retry: bool,
    },
    Passphrase {
        key: String,
        retry: bool,
    },
    /// One question of a keyboard-interactive round (one-time codes, PAM).
    Question {
        name: String,
        instructions: String,
        text: String,
        echo: bool,
    },
}

impl Prompt {
    /// Whether typed characters should be shown.
    pub fn echo(&self) -> bool {
        match self {
            Prompt::HostKey { .. } => true,
            Prompt::Question { echo, .. } => *echo,
            Prompt::Password { .. } | Prompt::Passphrase { .. } => false,
        }
    }

    /// The text to show, with terminal line endings.
    pub fn text(&self) -> String {
        match self {
            Prompt::HostKey { again: true, .. } => {
                "Please type 'yes', 'no' or the fingerprint: ".into()
            }
            Prompt::HostKey {
                host,
                algorithm,
                fingerprint,
                ..
            } => format!(
                "The authenticity of host '{host}' can't be established.\r\n\
                 {algorithm} key fingerprint is {fingerprint}.\r\n\
                 Are you sure you want to continue connecting (yes/no/[fingerprint])? "
            ),
            Prompt::Password { target, retry } => {
                let again = if *retry {
                    "Permission denied, please try again.\r\n"
                } else {
                    ""
                };
                format!("{again}{target}'s password: ")
            }
            Prompt::Passphrase { key, retry } => {
                let again = if *retry {
                    "Bad passphrase, try again.\r\n"
                } else {
                    ""
                };
                format!("{again}Enter passphrase for key '{key}': ")
            }
            Prompt::Question {
                name,
                instructions,
                text,
                ..
            } => {
                let mut s = String::new();
                for line in [name, instructions] {
                    if !line.is_empty() {
                        s.push_str(&line.replace('\n', "\r\n"));
                        s.push_str("\r\n");
                    }
                }
                s.push_str(text);
                s
            }
        }
    }
}

pub struct PromptRequest {
    pub prompt: Prompt,
    /// `None` cancels (Ctrl+C, window closed).
    pub reply: oneshot::Sender<Option<String>>,
}

/// Where the connection sends its questions.
#[derive(Clone, Debug)]
pub struct Prompter(mpsc::UnboundedSender<PromptRequest>);

impl Prompter {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<PromptRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Prompter(tx), rx)
    }

    /// Asks and waits for the answer; `None` if cancelled or nobody is listening.
    pub async fn ask(&self, prompt: Prompt) -> Option<String> {
        let (reply, answer) = oneshot::channel();
        self.0.send(PromptRequest { prompt, reply }).ok()?;
        answer.await.ok().flatten()
    }
}

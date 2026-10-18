//! Where keyboard input goes: nowhere while connecting, into a line answer while
//! the connection asks something (password, passphrase, code, host key), and to
//! the SSH session once it runs.
//!
//! Answers are edited like a simple line: printable text, Backspace, Ctrl+U to
//! clear, Enter to submit, Ctrl+C to cancel. Escape sequences (arrows, the
//! bracketed-paste markers) are ignored, so pasting a password works.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use shellwing_ssh::{Input, PromptRequest};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

enum Mode {
    Waiting,
    Prompt {
        echo: bool,
        answer: String,
        in_escape: bool,
        reply: oneshot::Sender<Option<String>>,
    },
    Session(UnboundedSender<Input>),
}

struct State {
    mode: Mode,
    /// The last terminal size, sent when the session starts.
    size: Option<(u32, u32)>,
}

/// Routes input; cheap to clone.
#[derive(Clone)]
pub struct Writer {
    state: Arc<Mutex<State>>,
    /// Bytes to show in the terminal (prompt text and echo). Forwarded on another
    /// thread: `write` runs on the terminal thread, which must not feed itself.
    show: mpsc::Sender<Vec<u8>>,
}

impl Writer {
    /// Returns the writer and the receiver of bytes to show in the terminal.
    pub fn new() -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (show, shown) = mpsc::channel();
        let state = Arc::new(Mutex::new(State {
            mode: Mode::Waiting,
            size: None,
        }));
        (Writer { state, show }, shown)
    }

    pub fn write(&self, bytes: Vec<u8>) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        match &mut st.mode {
            Mode::Session(tx) => {
                let _ = tx.send(Input::Data(bytes));
            }
            Mode::Prompt { .. } => self.prompt_input(&mut st, &bytes),
            Mode::Waiting => {}
        }
    }

    pub fn resize(&self, cols: u32, rows: u32) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        st.size = Some((cols, rows));
        if let Mode::Session(tx) = &st.mode {
            let _ = tx.send(Input::Resize { cols, rows });
        }
    }

    /// Shows a question and collects the answer from the keyboard.
    pub fn ask(&self, req: PromptRequest) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        let _ = self.show.send(req.prompt.text().into_bytes());
        st.mode = Mode::Prompt {
            echo: req.prompt.echo(),
            answer: String::new(),
            in_escape: false,
            reply: req.reply,
        };
    }

    /// Input goes to the session from now on.
    pub fn start_session(&self, tx: UnboundedSender<Input>) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        if let Some((cols, rows)) = st.size {
            let _ = tx.send(Input::Resize { cols, rows });
        }
        st.mode = Mode::Session(tx);
    }

    /// Sends `input` to the running session; false if none runs.
    pub fn send(&self, input: Input) -> bool {
        self.state.lock().is_ok_and(|st| match &st.mode {
            Mode::Session(tx) => tx.send(input).is_ok(),
            _ => false,
        })
    }

    /// The session ended or failed: drop input until the next connection.
    pub fn stop(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.mode = Mode::Waiting;
        }
    }

    /// A question whose answer must not be shown: a password, a passphrase, a
    /// hidden one-time code.
    pub fn is_secret_prompt(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|st| matches!(st.mode, Mode::Prompt { echo: false, .. }))
    }

    pub fn is_session(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|st| matches!(st.mode, Mode::Session(_)))
    }

    fn prompt_input(&self, st: &mut State, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes).into_owned();
        for c in text.chars() {
            let Mode::Prompt {
                echo,
                answer,
                in_escape,
                ..
            } = &mut st.mode
            else {
                return;
            };
            if *in_escape {
                // CSI and SS3 sequences end with a letter or '~'.
                if c.is_ascii_alphabetic() || c == '~' {
                    *in_escape = false;
                }
                continue;
            }
            match c {
                '\x1b' => *in_escape = true,
                '\r' | '\n' => {
                    let _ = self.show.send(b"\r\n".to_vec());
                    let answer = std::mem::take(answer);
                    self.finish(st, Some(answer));
                    return;
                }
                '\x03' => {
                    let _ = self.show.send(b"^C\r\n".to_vec());
                    self.finish(st, None);
                    return;
                }
                '\x7f' | '\x08' => {
                    if answer.pop().is_some() && *echo {
                        let _ = self.show.send(b"\x08 \x08".to_vec());
                    }
                }
                '\x15' => {
                    if *echo {
                        let n = answer.chars().count();
                        let _ = self.show.send("\x08 \x08".repeat(n).into_bytes());
                    }
                    answer.clear();
                }
                c if c.is_control() => {}
                c => {
                    answer.push(c);
                    if *echo {
                        let _ = self.show.send(c.to_string().into_bytes());
                    }
                }
            }
        }
    }

    fn finish(&self, st: &mut State, answer: Option<String>) {
        if let Mode::Prompt { reply, .. } = std::mem::replace(&mut st.mode, Mode::Waiting) {
            let _ = reply.send(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellwing_ssh::Prompt;

    fn ask(w: &Writer, echo: bool) -> oneshot::Receiver<Option<String>> {
        let (reply, answer) = oneshot::channel();
        let prompt = if echo {
            Prompt::Question {
                name: String::new(),
                instructions: String::new(),
                text: "Code: ".into(),
                echo: true,
            }
        } else {
            Prompt::Password {
                target: "me@h".into(),
                retry: false,
            }
        };
        w.ask(PromptRequest { prompt, reply });
        answer
    }

    fn shown(rx: &mpsc::Receiver<Vec<u8>>) -> String {
        String::from_utf8(rx.try_iter().flatten().collect()).expect("utf8")
    }

    #[test]
    fn password_is_edited_and_never_echoed() {
        let (w, rx) = Writer::new();
        let mut answer = ask(&w, false);
        assert_eq!(shown(&rx), "me@h's password: ");
        assert!(w.is_secret_prompt());
        w.write(b"hunter3".to_vec());
        w.write(vec![0x7f]); // backspace
        w.write(b"2\r".to_vec());
        assert_eq!(answer.try_recv().ok(), Some(Some("hunter2".into())));
        assert!(!w.is_secret_prompt());
        assert_eq!(shown(&rx), "\r\n", "nothing of the password is shown");
    }

    #[test]
    fn echoed_answers_show_and_ctrl_c_cancels() {
        let (w, rx) = Writer::new();
        let mut answer = ask(&w, true);
        let _ = shown(&rx);
        assert!(
            !w.is_secret_prompt(),
            "echoed answers may use the input method"
        );
        w.write(b"12".to_vec());
        w.write(vec![0x15]); // Ctrl+U clears
        w.write("tiếng".as_bytes().to_vec());
        assert_eq!(shown(&rx), "12\x08 \x08\x08 \x08tiếng");
        w.write(vec![0x03]);
        assert_eq!(answer.try_recv().ok(), Some(None));
    }

    #[test]
    fn pasted_answers_lose_the_bracketed_paste_markers() {
        let (w, _rx) = Writer::new();
        let mut answer = ask(&w, false);
        w.write(b"\x1b[200~s3cret\x1b[201~\r".to_vec());
        assert_eq!(answer.try_recv().ok(), Some(Some("s3cret".into())));
    }

    #[test]
    fn the_session_gets_the_size_seen_before_it_started() {
        let (w, _rx) = Writer::new();
        w.resize(132, 40);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        w.start_session(tx);
        assert!(matches!(
            rx.try_recv(),
            Ok(Input::Resize {
                cols: 132,
                rows: 40
            })
        ));
        w.write(b"ls\r".to_vec());
        assert!(matches!(rx.try_recv(), Ok(Input::Data(d)) if d == b"ls\r"));
    }
}

//! An AI's command typed into a tab: the line typed, and what came back
//! between its marks. The marks are OSC sequences a terminal ignores, so
//! the tab shows the command and its output and nothing more; the line as
//! typed spells them with printf's escapes, so its echo is not taken for
//! them.

/// The line typed for `command` as job `id`: kept out of the history (its
/// leading space), the command run by `eval`, so a `#` or a `&` in it
/// leaves the closing mark alone. Its end: `\r`.
pub fn wrap(id: &str, command: &str) -> String {
    format!(" {}\r", marked(id, command))
}

/// `command` between job `id`'s marks, as [`wrap`] types it: what a
/// snippet that needs environments runs in its subshell.
pub fn marked(id: &str, command: &str) -> String {
    let quoted = format!("'{}'", command.replace('\'', "'\\''"));
    format!(
        "printf '\\033]1337;swai-begin-{id}\\007'; eval {quoted}; printf '\\033]1337;swai-end-{id};%s\\007' \"$?\""
    )
}

fn begin(id: &str) -> Vec<u8> {
    format!("\x1b]1337;swai-begin-{id}\x07").into_bytes()
}

fn end(id: &str) -> Vec<u8> {
    format!("\x1b]1337;swai-end-{id};").into_bytes()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// What a tab printed, read for one job's marks as it comes.
pub struct Catch {
    begin: Vec<u8>,
    end: Vec<u8>,
    inside: bool,
    /// What may be the start of a mark, kept for the next bytes.
    held: Vec<u8>,
}

/// What came of the bytes fed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Caught {
    /// The command's output, as the terminal got it.
    pub output: Vec<u8>,
    /// Its exit status, once its closing mark came; Some(None) when the
    /// mark said nothing a number.
    pub ended: Option<Option<u32>>,
}

impl Catch {
    pub fn new(id: &str) -> Catch {
        Catch {
            begin: begin(id),
            end: end(id),
            inside: false,
            held: vec![],
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Caught {
        let mut out = Caught::default();
        self.held.extend_from_slice(bytes);
        if !self.inside {
            match find(&self.held, &self.begin) {
                Some(at) => {
                    self.held.drain(..at + self.begin.len());
                    self.inside = true;
                }
                None => {
                    let keep = self.begin.len().saturating_sub(1).min(self.held.len());
                    self.held.drain(..self.held.len() - keep);
                    return out;
                }
            }
        }
        match find(&self.held, &self.end) {
            Some(at) => {
                let after = at + self.end.len();
                match self.held[after..].iter().position(|b| *b == 0x07) {
                    Some(bel) => {
                        out.output.extend_from_slice(&self.held[..at]);
                        let code = std::str::from_utf8(&self.held[after..after + bel])
                            .ok()
                            .and_then(|s| s.trim().parse().ok());
                        out.ended = Some(code);
                        self.held.clear();
                    }
                    // Its status still on the way.
                    None => {
                        out.output.extend_from_slice(&self.held[..at]);
                        self.held.drain(..at);
                    }
                }
            }
            None => {
                // All but what may be the start of the closing mark.
                let keep = self.end.len().saturating_sub(1).min(self.held.len());
                let upto = self.held.len() - keep;
                out.output.extend_from_slice(&self.held[..upto]);
                self.held.drain(..upto);
            }
        }
        out
    }
}

/// A terminal's output as plain text: its escape sequences out, its line
/// ends `\n`, and a line written over after a `\r` (a progress bar) as it
/// was last.
pub fn plain(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: to its final byte.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC, DCS and the like: to BEL or ST.
                Some(']' | 'P' | '^' | '_') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\x07' | '\x08' | '\x00' => {}
            c => out.push(c),
        }
    }
    out.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.rsplit('\r').next().unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// What a shell prints for the line typed: its echo, then what it ran.
    fn shell(line: &str) -> Vec<u8> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(line.trim_end_matches('\r'))
            .output()
            .expect("sh");
        let mut tty = line.as_bytes().to_vec();
        tty.extend_from_slice(b"\r\n");
        tty.extend_from_slice(&out.stdout);
        tty.extend_from_slice(b"$ ");
        tty
    }

    fn catch_all(id: &str, bytes: &[u8], step: usize) -> (Vec<u8>, Option<Option<u32>>) {
        let mut c = Catch::new(id);
        let (mut output, mut ended) = (vec![], None);
        for chunk in bytes.chunks(step) {
            let got = c.feed(chunk);
            output.extend(got.output);
            ended = ended.or(got.ended);
        }
        (output, ended)
    }

    #[test]
    fn the_marks_hold_the_output_and_the_status() {
        let line = wrap("j-7", "echo 'it'\"'\"'s here' # a comment; exit 9\nfalse");
        let tty = shell(&line);
        // Fed whole, or a byte at a time: the same.
        for step in [tty.len(), 1, 3] {
            let (out, ended) = catch_all("j-7", &tty, step);
            assert_eq!(String::from_utf8_lossy(&out), "it's here\n", "step {step}");
            assert_eq!(ended, Some(Some(1)), "false's status, step {step}");
        }
        // Another job's marks are not this one's.
        assert_eq!(catch_all("j-8", &tty, 5), (vec![], None));
    }

    #[test]
    fn a_command_with_a_trailing_ampersand_still_closes() {
        let tty = shell(&wrap("j-1", "true &"));
        assert_eq!(catch_all("j-1", &tty, 2).1, Some(Some(0)));
    }

    #[test]
    fn plain_text_of_a_terminal() {
        assert_eq!(
            plain(b"\x1b[1;32mok\x1b[0m\r\nline 2\r\n\x1b]0;title\x07done"),
            "ok\nline 2\ndone"
        );
        assert_eq!(plain(b"10%\r50%\r100%\r\nnext"), "100%\nnext");
    }
}

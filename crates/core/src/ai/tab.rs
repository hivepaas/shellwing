//! An AI's command typed into a tab: the line typed, and what came back
//! between its marks. The marks are OSC sequences a terminal ignores, so
//! the tab shows the command and its output and nothing more; the line as
//! typed spells them with printf's escapes, so its echo is not taken for
//! them.

/// A stopped command still running in its tab this long after Ctrl+C gets
/// one more: one pressed the moment it started can wait in the shell until
/// what it runs ends.
pub const TAB_STOP_AGAIN: std::time::Duration = std::time::Duration::from_millis(1500);

/// The line typed for `command` as job `id`: kept out of the history (its
/// leading space), the command run by `eval`, so a `#` or a `&` in it
/// leaves the closing mark alone. Its end: `\r`.
pub fn wrap(id: &str, command: &str) -> String {
    format!(" {}\r", marked(id, command))
}

/// `command` between job `id`'s marks, as [`wrap`] types it: what a
/// snippet that needs environments runs in its subshell.
///
/// It runs in a subshell of its own, which takes Ctrl+C as `exit 130`: the
/// shell typed into would otherwise drop the rest of the line, the closing
/// mark with it, and the tab would wait for that mark for ever. So too a
/// `cd` or an `exit` in it is its own, not the tab's shell's.
pub fn marked(id: &str, command: &str) -> String {
    let quoted = format!("'{}'", command.replace('\'', "'\\''"));
    format!(
        "printf '\\033]1337;swai-begin-{id}\\007'; (trap 'exit 130' INT; eval {quoted}); printf '\\033]1337;swai-end-{id};%s\\007' \"$?\""
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

/// How much of the end of `hay` may be the start of `needle`, the rest of
/// it still to come: what is held back for the next bytes.
fn partial(hay: &[u8], needle: &[u8]) -> usize {
    (1..needle.len().min(hay.len() + 1))
        .rev()
        .find(|n| hay.ends_with(&needle[..*n]))
        .unwrap_or(0)
}

/// What a tab printed, read for one job's marks as it comes.
pub struct Catch {
    begin: Vec<u8>,
    end: Vec<u8>,
    inside: bool,
    /// Its closing mark came: what follows (the next prompt) is not its.
    done: bool,
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
            done: false,
            held: vec![],
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Caught {
        let mut out = Caught::default();
        if self.done {
            return out;
        }
        self.held.extend_from_slice(bytes);
        if !self.inside {
            match find(&self.held, &self.begin) {
                Some(at) => {
                    self.held.drain(..at + self.begin.len());
                    self.inside = true;
                }
                None => {
                    let keep = partial(&self.held, &self.begin);
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
                        self.done = true;
                    }
                    // Its status still on the way.
                    None => {
                        out.output.extend_from_slice(&self.held[..at]);
                        self.held.drain(..at);
                    }
                }
            }
            None => {
                // All but what may be the start of the closing mark: a line
                // shorter than the mark is handed on at once, not held until
                // the command ends (or for ever, were its end never seen).
                let keep = partial(&self.held, &self.end);
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

    /// What a command prints is handed on as it comes, a short line too:
    /// only what may be the start of the closing mark waits for more.
    #[test]
    fn a_short_line_is_handed_on_at_once() {
        let mut c = Catch::new("j-3");
        assert_eq!(c.feed(b"$ typed\r\n").output, b"");
        assert_eq!(c.feed(&begin("j-3")).output, b"");
        assert_eq!(c.feed(b"started\r\n").output, b"started\r\n");
        // A part of the closing mark waits for the rest, and is no output.
        let end = end("j-3");
        assert_eq!(c.feed(&end[..5]).output, b"");
        let got = c.feed(&[&end[5..], b"0\x07".as_slice()].concat());
        assert_eq!((got.output, got.ended), (vec![], Some(Some(0))));
        assert_eq!(partial(b"ab\x1b]13", b"\x1b]1337;"), 4);
        assert_eq!(partial(b"abc", b"\x1b]1337;"), 0);
    }

    #[test]
    fn a_command_with_a_trailing_ampersand_still_closes() {
        let tty = shell(&wrap("j-1", "true &"));
        assert_eq!(catch_all("j-1", &tty, 2).1, Some(Some(0)));
    }

    /// A shell in a terminal of its own, `argv`, where it is on this
    /// computer: what it is typed, and what it prints.
    fn terminal(
        argv: &[&str],
    ) -> Option<(
        tokio::sync::mpsc::UnboundedSender<shellwing_ssh::Input>,
        std::sync::mpsc::Receiver<Vec<u8>>,
    )> {
        let there = std::process::Command::new(argv[0])
            .args(["-c", "true"])
            .status()
            .is_ok_and(|s| s.success());
        if !there {
            return None;
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let done = crate::local::start(
            argv.iter().map(|a| a.to_string()).collect(),
            (100, 30),
            rx,
            Box::new(move |b| {
                let _ = out_tx.send(b.to_vec());
            }),
        )
        .expect("shell");
        drop(done);
        Some((tx, out_rx))
    }

    /// Job `id`'s output and end, from what `out` prints, for up to 10
    /// seconds; `each` sees the output so far as it comes.
    fn follow(
        id: &str,
        out: &std::sync::mpsc::Receiver<Vec<u8>>,
        mut each: impl FnMut(&str),
    ) -> (String, Option<Option<u32>>, String) {
        use std::time::{Duration, Instant};
        let (mut c, mut output, mut raw) = (Catch::new(id), vec![], vec![]);
        let by = Instant::now() + Duration::from_secs(10);
        while Instant::now() < by {
            let Ok(bytes) = out.recv_timeout(Duration::from_millis(50)) else {
                each(&plain(&output));
                continue;
            };
            raw.extend_from_slice(&bytes);
            let got = c.feed(&bytes);
            output.extend(got.output);
            if got.ended.is_some() {
                return (plain(&output), got.ended, String::new());
            }
            each(&plain(&output));
        }
        (
            plain(&output),
            None,
            String::from_utf8_lossy(&raw).into_owned(),
        )
    }

    const SHELLS: [&[&str]; 2] = [
        &["bash", "--norc", "--noprofile", "-i"],
        &["zsh", "-f", "-i"],
    ];

    /// Ctrl+C at the shell a tab runs, as the person or a stop sends it:
    /// the command ends, its subshell says 130, and the closing mark still
    /// comes, so the tab is free again; the shell itself goes on. In bash
    /// and zsh, each in a terminal of its own, where they are.
    #[tokio::test(flavor = "multi_thread")]
    async fn ctrl_c_ends_it_and_its_marks_still_close() {
        use shellwing_ssh::Input;
        use std::time::{Duration, Instant};
        for argv in SHELLS {
            let Some((tx, out)) = terminal(argv) else {
                continue;
            };
            tokio::time::sleep(Duration::from_millis(500)).await;
            let line = wrap("j-1", "echo started; sleep 30; echo late");
            tx.send(Input::Data(line.into_bytes())).expect("type");
            // Ctrl+C a moment after it started, as a person presses it.
            let (mut seen, mut sent) = (None::<Instant>, false);
            let (said, ended, raw) = follow("j-1", &out, |so_far| {
                if so_far.contains("started") && !sent {
                    let at = *seen.get_or_insert_with(Instant::now);
                    if at.elapsed() > Duration::from_millis(300) {
                        let _ = tx.send(Input::Data(vec![0x03]));
                        sent = true;
                    }
                }
            });
            assert_eq!(ended, Some(Some(130)), "{}: {raw:?}", argv[0]);
            assert!(
                said.contains("started") && !said.contains("late"),
                "{}: {said:?}",
                argv[0]
            );
            // The shell typed into goes on.
            tx.send(Input::Data(wrap("j-2", "echo again").into_bytes()))
                .expect("type");
            let (said, ended, raw) = follow("j-2", &out, |_| {});
            assert_eq!(ended, Some(Some(0)), "{}: {raw:?}", argv[0]);
            assert_eq!(said.trim(), "again", "{}", argv[0]);
        }
    }

    /// Ctrl+C the moment it started, before what it runs did: bash holds
    /// it for the subshell until that command ends. A second one, as the
    /// app sends while the tab still runs it, ends it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_ctrl_c_ends_one_too_early_for_the_first() {
        use shellwing_ssh::Input;
        use std::time::{Duration, Instant};
        for argv in SHELLS {
            let Some((tx, out)) = terminal(argv) else {
                continue;
            };
            tokio::time::sleep(Duration::from_millis(500)).await;
            let line = wrap("j-1", "echo started; sleep 30; echo late");
            tx.send(Input::Data(line.into_bytes())).expect("type");
            let (mut first, mut second) = (None::<Instant>, false);
            let (_, ended, raw) = follow("j-1", &out, |so_far| {
                if so_far.contains("started") && first.is_none() {
                    let _ = tx.send(Input::Data(vec![0x03]));
                    first = Some(Instant::now());
                }
                if !second && first.is_some_and(|f| f.elapsed() > TAB_STOP_AGAIN) {
                    let _ = tx.send(Input::Data(vec![0x03]));
                    second = true;
                }
            });
            assert_eq!(ended, Some(Some(130)), "{}: {raw:?}", argv[0]);
        }
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

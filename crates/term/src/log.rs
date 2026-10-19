//! Logs of what a terminal was sent, written as it comes: the plain text of
//! each line, or every byte as it arrived.
//!
//! Plain text is read from the bytes, not from the screen. Within a line the
//! cursor is followed as the terminal would follow it: carriage returns,
//! backspaces, the moves and erases a shell's line editor sends, and lines
//! longer than the terminal is wide, which wrap. So a progress bar logs its
//! last state, and a command edited at the prompt logs as it was entered.
//! Colors, titles and the other escape sequences go. What full-screen
//! programs (vim, htop, less) draw on the alternate screen is left out: it is
//! not lines of output.

use std::io::{self, Write};

use unicode_width::UnicodeWidthChar;

/// How a log keeps what was shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// The lines, as plain text.
    #[default]
    Text,
    /// Every byte as it arrived, escape sequences and all: `cat` or
    /// `less -R` shows it in color.
    Raw,
}

/// Written before each line of text that is not blank, such as the time.
pub type StampFn = Box<dyn FnMut() -> String + Send>;

/// A log being written. The terminal thread feeds it (see
/// [`crate::TermHandle::log`]).
pub struct Log {
    out: io::BufWriter<Box<dyn Write + Send>>,
    lines: Option<Lines>,
}

impl Log {
    pub fn new(out: Box<dyn Write + Send>, format: LogFormat, stamp: Option<StampFn>) -> Log {
        Log {
            out: io::BufWriter::new(out),
            lines: (format == LogFormat::Text).then(|| Lines::new(stamp)),
        }
    }

    /// Adds bytes the terminal was sent.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.lines {
            None => self.out.write_all(bytes),
            Some(l) => l.feed(bytes, &mut self.out),
        }
    }

    /// The terminal's width, which decides where long lines wrap.
    pub fn resize(&mut self, cols: u16) {
        if let Some(l) = &mut self.lines {
            l.resize(cols);
        }
    }

    /// Writes out what is buffered; a line not ended yet waits.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// Ends the log: the line not ended yet (a prompt) is written too.
    pub fn finish(mut self) -> io::Result<()> {
        if let Some(l) = &mut self.lines {
            l.break_line(&mut self.out)?;
        }
        self.out.flush()
    }
}

/// The right half of a wide character.
const HALF: char = '\0';
/// A line this long is written as it is, and a new one started: output
/// without newlines (a binary file shown) does not grow without end.
const MAX_LINE: usize = 1 << 16;
/// Escape sequence parameters kept; longer ones are not cursor moves.
const MAX_PARAMS: usize = 32;

#[derive(Clone)]
struct Cell {
    /// `' '` for a blank, [`HALF`] for the right half of a wide character.
    c: char,
    /// Marks written after it that combine with it (an accent).
    marks: Option<Box<str>>,
}

const BLANK: Cell = Cell {
    c: ' ',
    marks: None,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    /// After ESC and an intermediate byte (`ESC ( B`): one more byte.
    EscArg,
    Csi,
    /// A string (OSC, DCS, APC, PM, SOS), until BEL or ST.
    Str,
    /// ESC in a string: ST if `\` follows.
    StrEsc,
}

/// The line being written, as the cells of its rows (more than one when it
/// wrapped), and the cursor in it.
struct Lines {
    width: usize,
    cells: Vec<Cell>,
    row: usize,
    col: usize,
    /// The last column was written: the next character goes on the next row.
    wrap: bool,
    /// Rows the line covers.
    rows: usize,
    /// Counts the lines written, so a saved cursor is put back only in its own.
    line_no: u64,
    saved: (u64, usize, usize),
    /// The last character written, for REP.
    last: char,
    state: State,
    params: Vec<u8>,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_want: usize,
    /// On the alternate screen: nothing is kept.
    alt: bool,
    stamp: Option<StampFn>,
}

impl Lines {
    fn new(stamp: Option<StampFn>) -> Lines {
        Lines {
            width: 80,
            cells: Vec::new(),
            row: 0,
            col: 0,
            wrap: false,
            rows: 1,
            line_no: 0,
            saved: (0, 0, 0),
            last: ' ',
            state: State::Ground,
            params: Vec::new(),
            utf8: [0; 4],
            utf8_len: 0,
            utf8_want: 0,
            alt: false,
            stamp,
        }
    }

    fn resize(&mut self, cols: u16) {
        let at = self.index();
        self.width = usize::from(cols).max(2);
        self.row = at / self.width;
        self.col = at % self.width;
        self.rows = self
            .rows
            .max(self.cells.len().div_ceil(self.width))
            .max(self.row + 1);
        self.wrap = false;
    }

    fn index(&self) -> usize {
        self.row * self.width + self.col
    }

    fn feed(&mut self, bytes: &[u8], out: &mut impl Write) -> io::Result<()> {
        for &b in bytes {
            match self.state {
                State::Ground => self.ground(b, out)?,
                State::Esc => self.escape(b, out)?,
                State::EscArg => self.state = State::Ground,
                State::Csi => match b {
                    0x20..=0x3f => {
                        if self.params.len() < MAX_PARAMS {
                            self.params.push(b);
                        }
                    }
                    0x40..=0x7e => {
                        self.state = State::Ground;
                        self.csi(b, out)?;
                    }
                    0x1b => self.state = State::Esc,
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => {}
                },
                State::Str => match b {
                    0x07 | 0x18 | 0x1a => self.state = State::Ground,
                    0x1b => self.state = State::StrEsc,
                    _ => {}
                },
                State::StrEsc => {
                    if b == b'\\' {
                        self.state = State::Ground;
                    } else {
                        self.escape(b, out)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn ground(&mut self, b: u8, out: &mut impl Write) -> io::Result<()> {
        if b < 0x80 {
            self.utf8_len = 0;
        }
        match b {
            0x1b => self.state = State::Esc,
            b'\n' | 0x0b | 0x0c => self.line_feed(true, out)?,
            b'\r' => (self.col, self.wrap) = (0, false),
            0x08 => (self.col, self.wrap) = (self.col.saturating_sub(1), false),
            b'\t' => self.col = ((self.col / 8 + 1) * 8).min(self.width - 1),
            0x00..=0x1f | 0x7f => {}
            0x80..=0xff => {
                if let Some(c) = self.utf8_byte(b) {
                    self.put(c, out)?;
                }
            }
            _ => self.put(char::from(b), out)?,
        }
        Ok(())
    }

    /// Gathers the bytes of a UTF-8 character; the character once whole.
    fn utf8_byte(&mut self, b: u8) -> Option<char> {
        if b & 0xc0 == 0x80 {
            if self.utf8_len == 0 || self.utf8_len >= self.utf8_want {
                return None;
            }
            self.utf8[self.utf8_len] = b;
            self.utf8_len += 1;
            if self.utf8_len < self.utf8_want {
                return None;
            }
            self.utf8_len = 0;
            return Some(
                std::str::from_utf8(&self.utf8[..self.utf8_want])
                    .ok()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(char::REPLACEMENT_CHARACTER),
            );
        }
        self.utf8_want = match b {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => {
                self.utf8_len = 0;
                return Some(char::REPLACEMENT_CHARACTER);
            }
        };
        self.utf8[0] = b;
        self.utf8_len = 1;
        None
    }

    fn escape(&mut self, b: u8, out: &mut impl Write) -> io::Result<()> {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.params.clear();
                self.state = State::Csi;
            }
            b']' | b'P' | b'X' | b'^' | b'_' => self.state = State::Str,
            0x20..=0x2f => self.state = State::EscArg,
            0x1b => self.state = State::Esc,
            b'7' => self.save(),
            b'8' => self.restore(),
            // Index (down a row), next line, reverse index (up a row).
            b'D' => self.line_feed(false, out)?,
            b'E' => {
                self.line_feed(false, out)?;
                self.col = 0;
            }
            b'M' => self.up(1, out)?,
            // Reset.
            b'c' => {
                self.alt = false;
                self.break_line(out)?;
                self.col = 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn csi(&mut self, fin: u8, out: &mut impl Write) -> io::Result<()> {
        let (private, intermediate) = (
            self.params
                .first()
                .is_some_and(|b| (b'<'..=b'?').contains(b)),
            self.params.iter().any(|b| (0x20..=0x2f).contains(b)),
        );
        if intermediate {
            return Ok(());
        }
        let args: Vec<usize> = self.params[usize::from(private)..]
            .split(|&b| b == b';')
            .map(|p| {
                p.iter()
                    .take_while(|b| b.is_ascii_digit())
                    .fold(0usize, |n, b| {
                        n.saturating_mul(10).saturating_add(usize::from(b - b'0'))
                    })
            })
            .collect();
        if private {
            if matches!(fin, b'h' | b'l')
                && self.params.first() == Some(&b'?')
                && let Some(on) = args
                    .iter()
                    .any(|a| matches!(a, 47 | 1047 | 1049))
                    .then_some(fin == b'h')
            {
                self.alt_screen(on, out)?;
            }
            return Ok(());
        }
        // A count: 0 and missing mean 1.
        let n = |i: usize| args.get(i).copied().filter(|&v| v > 0).unwrap_or(1);
        let mode = args.first().copied().unwrap_or(0);
        match fin {
            b'A' => self.up(n(0), out)?,
            b'B' | b'e' => self.down(n(0), out)?,
            b'C' | b'a' => {
                self.col = (self.col + n(0)).min(self.width - 1);
                self.wrap = false;
            }
            b'D' => {
                self.col = self.col.saturating_sub(n(0));
                self.wrap = false;
            }
            b'E' => {
                self.down(n(0), out)?;
                self.col = 0;
            }
            b'F' => {
                self.up(n(0), out)?;
                self.col = 0;
            }
            b'G' | b'`' => {
                self.col = (n(0) - 1).min(self.width - 1);
                self.wrap = false;
            }
            // Moves to a row of the screen: where this line is on it is not
            // known, so the line ends here.
            b'H' | b'f' => {
                self.break_line(out)?;
                self.col = (n(1) - 1).min(self.width - 1);
            }
            b'd' => self.break_line(out)?,
            b'K' => {
                let start = self.row * self.width;
                let range = match mode {
                    0 => self.index()..start + self.width,
                    1 => start..self.index() + 1,
                    _ => start..start + self.width,
                };
                self.blank(range);
            }
            b'J' => match mode {
                0 => {
                    let at = self.index();
                    self.cells.truncate(at);
                }
                1 => self.blank(0..self.index() + 1),
                _ => self.cells.clear(),
            },
            b'P' => {
                let (col, k) = (self.col, n(0));
                self.edit_row(|row| {
                    if col < row.len() {
                        row.drain(col..(col + k).min(row.len()));
                    }
                });
            }
            b'@' => {
                let (col, k) = (self.col, n(0));
                self.edit_row(|row| {
                    if col < row.len() {
                        row.splice(col..col, std::iter::repeat_n(BLANK, k));
                    }
                });
            }
            b'X' => {
                let at = self.index();
                let end = (at + n(0)).min((self.row + 1) * self.width);
                self.blank(at..end);
            }
            b'b' => {
                let c = self.last;
                for _ in 0..n(0).min(MAX_LINE) {
                    self.put(c, out)?;
                }
            }
            b's' if self.params.is_empty() => self.save(),
            b'u' if self.params.is_empty() => self.restore(),
            _ => {}
        }
        Ok(())
    }

    /// Changes the cursor's row: its cells, the rows after it staying where
    /// they are.
    fn edit_row(&mut self, f: impl FnOnce(&mut Vec<Cell>)) {
        let start = self.row * self.width;
        if start >= self.cells.len() {
            return;
        }
        let end = (start + self.width).min(self.cells.len());
        let full = end - start == self.width;
        let mut row: Vec<Cell> = self.cells[start..end].to_vec();
        f(&mut row);
        if full && self.cells.len() > end {
            // More rows follow: the row keeps its width.
            row.resize(self.width, BLANK);
        }
        row.truncate(self.width);
        self.cells.splice(start..end, row);
    }

    fn blank(&mut self, range: std::ops::Range<usize>) {
        let end = range.end.min(self.cells.len());
        for c in self
            .cells
            .get_mut(range.start.min(end)..end)
            .into_iter()
            .flatten()
        {
            *c = BLANK;
        }
    }

    fn put(&mut self, c: char, out: &mut impl Write) -> io::Result<()> {
        if self.alt {
            return Ok(());
        }
        let w = c.width().unwrap_or(0).min(2);
        if w == 0 {
            // A combining mark: it goes with the character before it.
            let at = match (self.wrap, self.index()) {
                (true, i) => i,
                (false, 0) => return Ok(()),
                (false, i) => i - 1,
            };
            let at = match self.cells.get(at) {
                Some(cell) if cell.c == HALF && at > 0 => at - 1,
                _ => at,
            };
            if let Some(cell) = self.cells.get_mut(at) {
                let mut marks = cell.marks.take().map(String::from).unwrap_or_default();
                marks.push(c);
                cell.marks = Some(marks.into());
            }
            return Ok(());
        }
        if self.wrap {
            self.next_row();
        }
        if w == 2 && self.col + 1 >= self.width {
            // A wide character does not fit in the last column: it wraps.
            self.next_row();
        }
        let at = self.index();
        if self.cells.len() < at + w {
            self.cells.resize(at + w, BLANK);
        }
        for i in at..at + w {
            self.unwide(i);
        }
        self.cells[at] = Cell { c, marks: None };
        if w == 2 {
            self.cells[at + 1] = Cell {
                c: HALF,
                marks: None,
            };
        }
        self.col += w;
        if self.col >= self.width {
            self.col = self.width - 1;
            self.wrap = true;
        }
        self.last = c;
        if self.cells.len() >= MAX_LINE {
            self.break_line(out)?;
            self.col = 0;
        }
        Ok(())
    }

    /// Writing over half of a wide character blanks its other half.
    fn unwide(&mut self, i: usize) {
        if i > 0 && self.cells.get(i).is_some_and(|c| c.c == HALF) {
            self.cells[i - 1] = BLANK;
        }
        if self.cells.get(i + 1).is_some_and(|c| c.c == HALF) {
            self.cells[i + 1] = BLANK;
        }
    }

    fn next_row(&mut self) {
        self.row += 1;
        self.col = 0;
        self.wrap = false;
        self.rows = self.rows.max(self.row + 1);
    }

    /// A line feed: down a row of this line if it wrapped there, else the
    /// line is done and the next one starts (at the left when `home`).
    fn line_feed(&mut self, home: bool, out: &mut impl Write) -> io::Result<()> {
        self.wrap = false;
        if self.alt {
            return Ok(());
        }
        if self.row + 1 < self.rows {
            self.row += 1;
            return Ok(());
        }
        self.write_line(out)?;
        self.start_line();
        if home {
            self.col = 0;
        }
        Ok(())
    }

    fn up(&mut self, n: usize, out: &mut impl Write) -> io::Result<()> {
        self.wrap = false;
        if self.row >= n {
            self.row -= n;
            Ok(())
        } else {
            // Into the lines above, written already.
            self.break_line(out)
        }
    }

    fn down(&mut self, n: usize, out: &mut impl Write) -> io::Result<()> {
        self.wrap = false;
        if self.row + n < self.rows {
            self.row += n;
            Ok(())
        } else {
            self.break_line(out)
        }
    }

    fn save(&mut self) {
        self.saved = (self.line_no, self.row, self.col);
    }

    fn restore(&mut self) {
        let (line, row, col) = self.saved;
        if line == self.line_no {
            self.row = row.min(self.rows - 1);
        }
        self.col = col.min(self.width - 1);
        self.wrap = false;
    }

    fn alt_screen(&mut self, on: bool, out: &mut impl Write) -> io::Result<()> {
        if on && !self.alt {
            self.break_line(out)?;
        }
        if !on && self.alt {
            // The screen comes back as it was; the line being written goes on.
            self.cells.clear();
            self.start_line();
        }
        self.alt = on;
        Ok(())
    }

    /// Ends the line where it is, written if it has any text, and starts
    /// another at the cursor's column.
    fn break_line(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.cells.iter().any(|c| c.c != ' ' && c.c != HALF) {
            self.write_line(out)?;
        }
        self.start_line();
        Ok(())
    }

    fn start_line(&mut self) {
        self.cells.clear();
        (self.row, self.rows, self.wrap) = (0, 1, false);
        self.line_no += 1;
    }

    fn write_line(&mut self, out: &mut impl Write) -> io::Result<()> {
        let mut text = String::with_capacity(self.cells.len());
        for cell in &self.cells {
            if cell.c != HALF {
                text.push(cell.c);
                if let Some(m) = &cell.marks {
                    text.push_str(m);
                }
            }
        }
        let text = text.trim_end();
        if !text.is_empty()
            && let Some(stamp) = &mut self.stamp
        {
            out.write_all(stamp().as_bytes())?;
        }
        out.write_all(text.as_bytes())?;
        out.write_all(b"\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Written to memory, for reading back.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("sink").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn text_of(width: u16, chunks: &[&[u8]], stamp: Option<StampFn>) -> String {
        let sink = Sink::default();
        let mut log = Log::new(Box::new(sink.clone()), LogFormat::Text, stamp);
        log.resize(width);
        for c in chunks {
            log.write(c).expect("write");
        }
        log.finish().expect("finish");
        String::from_utf8(sink.0.lock().expect("sink").clone()).expect("utf-8")
    }

    fn text(bytes: &str) -> String {
        text_of(80, &[bytes.as_bytes()], None)
    }

    #[test]
    fn colors_and_titles_go() {
        assert_eq!(
            text("\x1b]0;deploy@api: ~\x07\x1b[1;32mok\x1b[0m done\r\n"),
            "ok done\n"
        );
        assert_eq!(
            text("see \x1b]8;;https://example.com\x1b\\the docs\x1b]8;;\x1b\\\r\n"),
            "see the docs\n"
        );
        assert_eq!(text("\x1b(B\x1b[?2004hplain\r\n"), "plain\n");
    }

    #[test]
    fn a_line_logs_as_it_was_left() {
        // A progress bar redraws its line.
        assert_eq!(text(" 10%\r 50%\r100%\r\n"), "100%\n");
        // A typo fixed at the prompt: backspace, then erase to the end.
        assert_eq!(text("$ lsx\x08\x1b[K -l\r\nfile\r\n"), "$ ls -l\nfile\n");
        // Deleting characters (DCH), and making room for some (ICH).
        assert_eq!(text("$ echo hi\x1b[4D\x1b[2P\r\n"), "$ echhi\n");
        assert_eq!(text("$ ls\x1b[2D\x1b[3@abc\r\n"), "$ abcls\n");
        // Blank lines stay; trailing spaces go.
        assert_eq!(text("a   \r\n\r\nb\r\n"), "a\n\nb\n");
    }

    #[test]
    fn long_lines_wrap_and_are_edited_across_rows() {
        // 15 characters at 10 columns: two rows. Up into the first row and
        // fix a character there, then down and on.
        let got = text_of(10, &[b"$ 0123456789abc\x1b[AX\x1b[B\r\n"], None);
        assert_eq!(got, "$ 012X456789abc\n");
        // A line feed in a wrapped line moves down it, not out of it.
        let got = text_of(10, &[b"$ 0123456789abc\x1b[A\r\n\r\nmore\r\n"], None);
        assert_eq!(got, "$ 0123456789abc\nmore\n");
    }

    #[test]
    fn full_screen_programs_are_left_out() {
        assert_eq!(
            text("$ vim notes\r\n\x1b[?1049h\x1b[H\x1b[2J~\r\n~\r\n\x1b[?1049l$ ls\r\nnotes\r\n"),
            "$ vim notes\n$ ls\nnotes\n"
        );
    }

    #[test]
    fn characters_are_whole_across_chunks_and_wide_ones_take_two_columns() {
        let s = "Tiếng Việt\r\n".as_bytes();
        let (a, b) = s.split_at(4);
        assert_eq!(text_of(80, &[a, b], None), "Tiếng Việt\n");
        // Back two columns lands on the second wide character's left half.
        assert_eq!(text("日本\x08\x08X\r\n"), "日X\n");
        // A combining accent stays with its letter.
        assert_eq!(text("cafe\u{301}\r\n"), "cafe\u{301}\n");
    }

    #[test]
    fn stamps_mark_lines_with_text_and_the_last_line_is_written_at_the_end() {
        let stamp: StampFn = Box::new(|| "[t] ".to_string());
        assert_eq!(
            text_of(80, &[b"one\r\n\r\n$ "], Some(stamp)),
            "[t] one\n\n[t] $\n"
        );
    }

    #[test]
    fn raw_logs_keep_every_byte() {
        let sink = Sink::default();
        let mut log = Log::new(Box::new(sink.clone()), LogFormat::Raw, None);
        log.write(b"\x1b[31mred\x1b[0m\r\n").expect("write");
        log.finish().expect("finish");
        assert_eq!(*sink.0.lock().expect("sink"), b"\x1b[31mred\x1b[0m\r\n");
    }
}

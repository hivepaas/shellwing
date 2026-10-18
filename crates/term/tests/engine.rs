//! Drives the terminal thread through `TermHandle` the way the UI does.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use shellwing_term::{Event, Frame, Mods, MouseAction, MouseButton, MouseInput, TermHandle};

const CELL: (u32, u32) = (10, 20);

struct Harness {
    term: TermHandle,
    written: Receiver<Vec<u8>>,
    events: Receiver<Event>,
    woke: Receiver<()>,
}

fn harness() -> Harness {
    let (wtx, written) = channel();
    let (etx, events) = channel();
    let (ktx, woke) = channel();
    let term = TermHandle::spawn(
        40,
        5,
        100,
        Box::new(move |b| {
            let _ = wtx.send(b);
        }),
        Box::new(move || {
            let _ = ktx.send(());
        }),
        Box::new(move |e| {
            let _ = etx.send(e);
        }),
    )
    .expect("spawn");
    term.resize(40, 5, CELL.0, CELL.1);
    Harness {
        term,
        written,
        events,
        woke,
    }
}

impl Harness {
    /// Waits until a frame satisfying `pred` is built.
    fn frame_where(&self, pred: impl Fn(&Frame) -> bool) -> Frame {
        for _ in 0..200 {
            if let Some(f) = self.term.take_frame()
                && pred(&f)
            {
                return (*f).clone();
            }
            let _ = self.woke.recv_timeout(Duration::from_millis(20));
        }
        panic!("no matching frame");
    }

    fn row_text(f: &Frame, y: usize) -> String {
        f.rows[y].text.trim_end().to_string()
    }

    fn written_all(&self) -> Vec<u8> {
        std::thread::sleep(Duration::from_millis(50));
        self.written.try_iter().flatten().collect()
    }

    fn mouse(&self, action: MouseAction, x: f32, y: f32, mods: Mods) {
        self.term.mouse(MouseInput {
            action,
            button: Some(MouseButton::Left),
            x,
            y,
            mods,
        });
    }
}

#[test]
fn drag_selects_and_copy_returns_the_text() {
    let h = harness();
    h.term.feed(b"hello world\r\nsecond".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "hello world");

    let none = Mods::default();
    // Drag across "hello" on the first row (cells 0..=4). Like Ghostty, the last
    // cell counts only once the pointer is past its middle, hence x = 48 in cell 4.
    h.mouse(MouseAction::Press, 1.0, 5.0, none);
    h.mouse(MouseAction::Motion, 48.0, 5.0, none);
    h.mouse(MouseAction::Release, 48.0, 5.0, none);
    let f = h.frame_where(|f| f.rows[0].runs.iter().any(|r| r.selected));
    assert!(
        f.rows[0].runs[0].selected,
        "selection is reported in the frame"
    );

    h.term.copy();
    let copied = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("copy event");
    assert_eq!(copied, Event::Copied("hello".into()));
}

#[test]
fn unsafe_paste_is_refused_until_bracketed() {
    let h = harness();
    h.term.paste("rm -rf x\nls".into(), false);
    let ev = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("refusal");
    assert_eq!(ev, Event::PasteRefused("rm -rf x\nls".into()));
    assert!(h.written_all().is_empty(), "nothing reaches the host");

    // Once the program enables bracketed paste, the same text is framed and sent.
    h.term.feed(b"\x1b[?2004h".to_vec());
    h.term.paste("rm -rf x\nls".into(), false);
    let out = String::from_utf8(h.written_all()).expect("utf8");
    assert!(
        out.starts_with("\x1b[200~") && out.ends_with("\x1b[201~"),
        "got {out:?}"
    );
}

#[test]
fn wheel_scrolls_history_and_typing_returns_to_bottom() {
    let h = harness();
    let lines: String = (0..50).map(|i| format!("line {i}\r\n")).collect();
    h.term.feed(lines.into_bytes());
    h.frame_where(|f| f.rows.iter().any(|r| r.text.starts_with("line 49")));

    h.term.wheel(-10.0, 5.0, 5.0, Mods::default());
    let f = h.frame_where(|f| f.scrolled_back);
    assert!(
        f.cursor.is_none(),
        "the cursor is hidden while scrolled back"
    );

    h.term.text("x".into());
    h.frame_where(|f| !f.scrolled_back);
    assert_eq!(h.written_all(), b"x");
}

#[test]
fn mouse_goes_to_the_program_when_it_asks() {
    let h = harness();
    // SGR mouse reporting with button tracking, as htop or vim enable it.
    h.term.feed(b"\x1b[?1000h\x1b[?1006h".to_vec());
    h.frame_where(|_| true);
    h.mouse(MouseAction::Press, 25.0, 25.0, Mods::default());
    let out = String::from_utf8(h.written_all()).expect("utf8");
    // Cell (2, 1) in SGR is 1-based: column 3, row 2.
    assert_eq!(out, "\x1b[<0;3;2M");

    // Shift still selects instead of reporting.
    let shift = Mods {
        shift: true,
        ..Default::default()
    };
    h.mouse(MouseAction::Release, 25.0, 25.0, Mods::default());
    let _ = h.written_all();
    h.mouse(MouseAction::Press, 25.0, 25.0, shift);
    assert!(h.written_all().is_empty());
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "dev builds compile libghostty in Zig's Debug mode; run with cargo test --release"
)]
fn frames_keep_coming_while_output_floods() {
    // Regression: a full input queue used to drop the UI's "frame taken" notice,
    // after which no frame was ever built again. Big chunks keep the parser busy so
    // the queue is full while frames are being taken.
    let h = harness();
    let term = h.term.clone();
    let line = format!("{}\r\n", "x".repeat(78));
    let chunk = line.repeat(800); // ~64 KB
    let feeder = std::thread::spawn(move || {
        for _ in 0..100 {
            term.feed(chunk.clone().into_bytes());
        }
        term.feed(b"the end\r\n".to_vec());
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut frames = 0;
    loop {
        if let Some(f) = h.term.take_frame() {
            frames += 1;
            if f.rows.iter().any(|r| r.text.starts_with("the end")) {
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "frames stopped after {frames}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    feeder.join().expect("feeder");
    assert!(
        frames > 1,
        "the screen updated during the flood, not only after it ({frames} frames)"
    );
}

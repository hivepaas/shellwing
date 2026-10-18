//! Drives the terminal thread through `TermHandle` the way the UI does.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use shellwing_term::{
    Colors, Event, Frame, LinkSpan, Mods, MouseAction, MouseButton, MouseInput, Rgb, TermHandle,
};

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
            link: false,
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

#[test]
fn hovering_finds_a_printed_url() {
    let h = harness();
    h.term
        .feed(b"visit https://example.com/docs today".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0).starts_with("visit"));
    // Cell 10 is inside the URL (columns 6..30).
    h.term.hover(Some((105.0, 5.0)));
    let f = h.frame_where(|f| f.hover_link.is_some());
    assert_eq!(
        f.hover_link,
        Some(LinkSpan {
            row: 0,
            start: 6,
            end: 30
        })
    );
    h.term.hover(None);
    h.frame_where(|f| f.hover_link.is_none());
}

#[test]
fn link_click_opens_osc8_targets_but_not_unsafe_schemes() {
    let h = harness();
    // OSC 8: the visible text differs from the target.
    h.term.feed(b"\x1b]8;;https://ghostty.org/\x1b\\click me\x1b]8;;\x1b\\ \x1b]8;;file:///etc/passwd\x1b\\local\x1b]8;;\x1b\\".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0).starts_with("click me"));
    let click = |x: f32| {
        h.term.mouse(MouseInput {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            x,
            y: 5.0,
            mods: Mods::default(),
            link: true,
        });
        h.term.mouse(MouseInput {
            action: MouseAction::Release,
            button: Some(MouseButton::Left),
            x,
            y: 5.0,
            mods: Mods::default(),
            link: true,
        });
    };
    click(25.0);
    assert_eq!(
        h.events.recv_timeout(Duration::from_secs(2)).ok(),
        Some(Event::OpenLink("https://ghostty.org/".into()))
    );
    click(105.0); // "local" -> file://, never opened
    assert!(h.events.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn wide_glyphs_take_two_cells() {
    let h = harness();
    h.term.feed("a日本b".as_bytes().to_vec());
    let f = h.frame_where(|f| f.rows[0].text.starts_with("a日本b"));
    let runs: Vec<(u16, u16, bool)> = f.rows[0]
        .runs
        .iter()
        .take(4)
        .map(|r| (r.col, r.cells, r.wide))
        .collect();
    assert_eq!(runs[0], (0, 1, false));
    assert_eq!(runs[1], (1, 2, true));
    assert_eq!(runs[2], (3, 2, true));
    assert_eq!(
        runs[3].0, 5,
        "the run after the wide glyphs starts at column 5"
    );
}

fn theme(base: u8) -> Colors {
    let c = |v: u8| Rgb { r: v, g: v, b: v };
    let mut palette = [c(0); 16];
    palette[1] = Rgb {
        r: base,
        g: 0,
        b: 0,
    };
    Colors {
        foreground: c(base + 1),
        background: c(base + 2),
        cursor: c(base + 3),
        palette,
    }
}

#[test]
fn theme_colors_reach_text_already_on_screen() {
    let h = harness();
    h.term.feed(b"\x1b[31mred\x1b[0m plain".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "red plain");

    h.term.set_colors(theme(100));
    let f = h.frame_where(|f| {
        f.background
            == Rgb {
                r: 102,
                g: 102,
                b: 102,
            }
    });
    let runs = &f.rows[0].runs;
    assert_eq!(
        runs[0].fg,
        Rgb { r: 100, g: 0, b: 0 },
        "ANSI red from the palette"
    );
    assert_eq!(
        runs[1].fg,
        Rgb {
            r: 101,
            g: 101,
            b: 101
        },
        "default foreground"
    );
    assert_eq!(runs[1].bg, None, "default background stays unpainted");
    assert_eq!(
        f.cursor_color,
        Some(Rgb {
            r: 103,
            g: 103,
            b: 103
        })
    );
}

#[test]
fn a_programs_color_override_survives_a_theme_change() {
    let h = harness();
    h.term.set_colors(theme(100));
    h.term.feed(b"\x1b]11;#102030\x1b\\x".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "x");
    h.term.set_colors(theme(50));
    let f = h.frame_where(|f| {
        f.foreground
            == Rgb {
                r: 51,
                g: 51,
                b: 51,
            }
    });
    assert_eq!(
        f.background,
        Rgb {
            r: 0x10,
            g: 0x20,
            b: 0x30
        }
    );
}

#[test]
fn close_ends_the_thread_and_drops_its_callbacks() {
    let alive = std::sync::Arc::new(());
    let held = alive.clone();
    let term = TermHandle::spawn(
        10,
        3,
        10,
        Box::new(move |_| {
            let _ = &held;
        }),
        Box::new(|| {}),
        Box::new(|_| {}),
    )
    .expect("spawn");
    term.close();
    for _ in 0..100 {
        if std::sync::Arc::strong_count(&alive) == 1 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the write callback was not dropped");
}

fn cursor(f: &Frame) -> (u16, u16) {
    f.cursor.expect("cursor shown")
}

#[test]
fn resetting_modes_keeps_the_text_and_the_cursor() {
    let h = harness();
    h.term.feed(b"one\r\ntwo\r\n$ ".to_vec());
    let before = h.frame_where(|f| Harness::row_text(f, 2) == "$");
    assert_eq!(cursor(&before), (2, 2));
    h.term.reset_modes();
    h.term.feed(b"x".to_vec());
    let f = h.frame_where(|f| Harness::row_text(f, 2) == "$ x");
    assert_eq!(Harness::row_text(&f, 0), "one", "nothing is overwritten");
    assert_eq!(cursor(&f), (3, 2));
}

#[test]
fn resetting_modes_leaves_a_forgotten_alternate_screen() {
    let h = harness();
    h.term.feed(b"$ vim\r\n".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "$ vim");
    // vim takes the alternate screen, then its connection dies.
    h.term.feed(b"\x1b[?1049h\x1b[2J\x1b[Hvim screen".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "vim screen");
    h.term.reset_modes();
    h.term.feed(b"back".to_vec());
    let f = h.frame_where(|f| Harness::row_text(f, 1) == "back");
    assert_eq!(
        Harness::row_text(&f, 0),
        "$ vim",
        "the shell's screen is back"
    );
}

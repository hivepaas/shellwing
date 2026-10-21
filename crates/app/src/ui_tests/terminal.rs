//! The terminal as a user works in it: typing, editing keys, a flood of
//! output, other scripts, full screen programs, titles.

use super::server::UNICODE;
use super::{PROMPT, Policy, Ui, scenario};

/// Signed in to a fresh server, at the shell's prompt.
fn signed_in(ui: &mut Ui) -> super::Server {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    server
}

scenario!(
    /// Backspace edits the line before it is sent.
    backspace_edits_the_line,
    |ui| {
        let server = signed_in(ui);
        ui.type_text("hellp");
        ui.keys("backspace");
        ui.line("o");
        ui.wait_for("you said: hello");
        assert_eq!(server.seen(|s| s.lines.clone()), ["hello"]);
    }
);

scenario!(
    /// Ctrl+C reaches the program as ^C (it is not taken as copy with nothing
    /// selected), and the line typed so far is dropped.
    ctrl_c_reaches_the_shell,
    |ui| {
        let server = signed_in(ui);
        ui.type_text("rm -rf something");
        ui.keys("ctrl-c");
        ui.wait_for("^C");
        ui.line("after");
        ui.wait_for("you said: after");
        assert_eq!(server.seen(|s| s.lines.clone()), ["after"]);
        assert!(server.seen(|s| s.input.contains(&0x03)));
    }
);

scenario!(
    /// The arrow keys go to the program as escape sequences (shell history,
    /// editors).
    arrow_keys_reach_the_program,
    |ui| {
        let server = signed_in(ui);
        ui.keys("up down left right");
        ui.settle(std::time::Duration::from_millis(300));
        let input = server.seen(|s| s.input.clone());
        for &arrow in b"ABCD" {
            assert!(
                input.windows(3).any(|w| w[0] == 0x1b && w[2] == arrow),
                "no arrow {} in {:?}",
                arrow as char,
                String::from_utf8_lossy(&input)
            );
        }
    }
);

scenario!(
    /// A burst of output (a build log, `cat` of a big file) is all shown,
    /// and the terminal answers right after it.
    a_flood_of_output,
    |ui| {
        signed_in(ui);
        ui.line("lines 20000");
        ui.wait_for("line 20000");
        ui.line("still here");
        ui.wait_for("you said: still here");
    }
);

scenario!(
    /// Vietnamese with its marks, wide CJK glyphs and an emoji arrive whole.
    unicode_text,
    |ui| {
        signed_in(ui);
        ui.line("unicode");
        ui.wait_for(UNICODE);
    }
);

scenario!(
    /// Text typed with diacritics (as from a Vietnamese input method) reaches
    /// the shell as UTF-8.
    typing_unicode,
    |ui| {
        let server = signed_in(ui);
        ui.line("xin chào thế giới");
        ui.wait_for("you said: xin chào thế giới");
        assert_eq!(server.seen(|s| s.lines.clone()), ["xin chào thế giới"]);
    }
);

scenario!(
    /// A full screen program takes the alternate screen; quitting it brings
    /// back the shell's screen as it was.
    full_screen_program_and_back,
    |ui| {
        signed_in(ui);
        ui.line("before vim");
        ui.wait_for("you said: before vim");
        ui.line("fullscreen");
        ui.wait_for("FULL SCREEN PROGRAM");
        assert!(ui.view(|v, _| v.latest_frame().alternate));
        assert!(!ui.screen().contains("you said: before vim"));
        ui.type_text("q");
        ui.wait_for("you said: before vim");
        assert!(!ui.view(|v, _| v.latest_frame().alternate));
    }
);

scenario!(
    /// A program's title (OSC 0) becomes the tab's.
    program_sets_the_tab_title,
    |ui| {
        signed_in(ui);
        ui.line("title deploying api-server");
        ui.wait_until("the title", |ui| {
            ui.view(|v, _| v.shell_title().map(|t| t.to_string()))
                .as_deref()
                == Some("deploying api-server")
        });
    }
);

scenario!(
    /// Colors and clearing the screen.
    colors_and_clear,
    |ui| {
        signed_in(ui);
        ui.line("red");
        ui.wait_for("red text");
        let red = ui.view(|v, _| {
            let frame = v.latest_frame();
            frame.rows.iter().any(|row| {
                row.text.contains("red text") && row.runs.iter().any(|r| r.fg.r > 150 && r.fg.g < 120)
            })
        });
        assert!(red, "the text is not red");
        ui.line("clear");
        ui.wait_until("a clear screen", |ui| {
            let screen = ui.screen();
            !screen.contains("red text") && screen.trim() == PROMPT.trim()
        });
    }
);

scenario!(
    /// The terminal's size goes to the server with the PTY.
    pty_size_reaches_the_server,
    |ui| {
        let server = signed_in(ui);
        let sizes = server.seen(|s| s.sizes.clone());
        let (cols, rows) = *sizes.first().expect("a PTY was asked for");
        assert!(cols >= 20 && rows >= 5, "a tiny PTY: {cols}x{rows}");
    }
);

scenario!(
    /// Find counts the matches on screen and in the history; Escape closes it.
    find_in_the_terminal,
    |ui| {
        signed_in(ui);
        ui.line("lines 50");
        ui.wait_for("line 50");
        ui.keys(if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-shift-f" });
        ui.type_text("line 4");
        // "line 4" and "line 40" to "line 49".
        ui.wait_until("11 matches", |ui| {
            ui.view(|v, _| v.latest_frame().find.map(|f| (f.total, f.searching)))
                == Some((11, false))
        });
        ui.keys("escape");
        ui.wait_until("find to close", |ui| ui.view(|v, _| v.latest_frame().find.is_none()));
    }
);

scenario!(
    /// Pasting one line sends it as typed.
    paste_a_line,
    |ui| {
        let server = signed_in(ui);
        ui.copy_to_clipboard("pasted words");
        ui.keys(if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-shift-v" });
        ui.keys("enter");
        ui.wait_for("you said: pasted words");
        assert_eq!(server.seen(|s| s.lines.clone()), ["pasted words"]);
    }
);

scenario!(
    /// A program that asked for bracketed paste gets the paste marked, so it
    /// does not run it.
    bracketed_paste,
    |ui| {
        let server = signed_in(ui);
        ui.line("bracketed");
        ui.settle(std::time::Duration::from_millis(300));
        ui.copy_to_clipboard("echo one\necho two");
        ui.keys(if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-shift-v" });
        ui.wait_until("the marked paste", |_| {
            let input = server.seen(|s| String::from_utf8_lossy(&s.input).into_owned());
            input.contains("\x1b[200~") && input.contains("\x1b[201~")
        });
    }
);

scenario!(
    /// Several lines pasted into a shell that would run them: held back
    /// with a warning; pasting again goes through.
    multi_line_paste_is_held_back,
    |ui| {
        let server = signed_in(ui);
        ui.copy_to_clipboard("echo one\necho two\n");
        let paste = if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-shift-v" };
        ui.keys(paste);
        ui.wait_for_notice_in("run commands", crate::notice::Tone::Warn);
        ui.settle(std::time::Duration::from_millis(200));
        assert!(server.seen(|s| s.lines.is_empty()), "it ran");
        ui.keys(paste);
        ui.wait_for("you said: echo two");
    }
);

scenario!(
    /// The window grows: the server is told the new size.
    resize_reaches_the_server,
    |ui| {
        let server = signed_in(ui);
        let before = server.seen(|s| s.sizes.last().copied()).expect("a size");
        ui.resize(1600., 1000.);
        ui.wait_until("a new size on the server", |_| {
            server.seen(|s| s.sizes.last().copied()).is_some_and(|after| after != before)
        });
        ui.line("size");
        let (c, r) = server.seen(|s| *s.sizes.last().expect("size"));
        ui.wait_for(&format!("size {c}x{r}"));
    }
);

scenario!(
    /// Lines ended by a bare carriage return (as a web page can hide in what
    /// is copied from it) run commands as surely as newlines: held back too.
    paste_with_carriage_returns_is_held_back,
    |ui| {
        let server = signed_in(ui);
        ui.copy_to_clipboard("echo one\recho two\r");
        let paste = if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-shift-v" };
        ui.keys(paste);
        ui.settle(std::time::Duration::from_millis(500));
        assert!(
            server.seen(|s| s.lines.is_empty()),
            "pasted commands ran with no warning: {:?}",
            server.seen(|s| s.lines.clone())
        );
    }
);

scenario!(
    /// Ctrl/Cmd and = makes this pane's font bigger, and 0 brings it back.
    zoom_keys_change_the_font_size,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.draw();
        let cell = ui.view(|v, _| v.cell_size());
        ui.keys("secondary-=");
        ui.wait_for_notice("Font size 15");
        ui.draw();
        assert!(ui.view(|v, _| v.cell_size()).0 > cell.0);
        ui.keys("secondary-0");
        ui.wait_for_notice("Font size 14");
        ui.draw();
        assert_eq!(ui.view(|v, _| v.cell_size()), cell);
    }
);

scenario!(
    /// Find with match case (Alt+C) and regular expressions (Alt+R): the
    /// count follows each option; a broken pattern says so.
    find_with_match_case_and_regex,
    |ui| {
        let server = ui.server(Policy::password());
        ui.trust(&server);
        ui.connect(&server.spec());
        ui.login();
        ui.line("Alpha alpha ALPHA");
        ui.wait_for("you said: Alpha alpha ALPHA");
        ui.shortcut("f");
        ui.type_text("alpha");
        let found = |ui: &mut crate::ui_tests::Ui| {
            ui.view(|v, _| v.latest_frame().find)
                .filter(|f| !f.searching)
        };
        ui.wait_until("6 found", |ui| found(ui).is_some_and(|f| f.total == 6));
        ui.keys("alt-c");
        ui.wait_until("2 found, matching case", |ui| found(ui).is_some_and(|f| f.total == 2));
        ui.keys("alt-c");
        ui.keys("alt-r");
        for _ in 0.."alpha".len() {
            ui.keys("backspace");
        }
        // "al.ha" is no text on the screen; as a pattern it is each alpha.
        ui.type_text("al.ha");
        ui.wait_until("words by the pattern", |ui| found(ui).is_some_and(|f| f.total == 6));
        ui.type_text("(");
        ui.wait_until("a broken pattern", |ui| {
            ui.view(|v, _| v.latest_frame().find).is_some_and(|f| f.invalid)
        });
    }
);

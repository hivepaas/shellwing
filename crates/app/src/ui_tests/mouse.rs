//! The mouse in the terminal: selecting text and copying it, and links.

use gpui::Modifiers;

use super::{Policy, Server, Ui, scenario};

/// Connected to a server and signed in.
fn signed_in(ui: &mut Ui) -> Server {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    server
}

/// Signed in, the shell having said a long line of numbered words (`w00
/// w01 … w39`): the row it is on and the column it starts at.
fn long_line(ui: &mut Ui) -> (Server, usize, usize) {
    let server = signed_in(ui);
    let words: Vec<String> = (0..40).map(|i| format!("w{i:02}")).collect();
    ui.line(&words.join(" "));
    ui.wait_for("you said: w00");
    let (row, col) = ui.find_text("you said: w00");
    (server, row, col + "you said: ".len())
}

/// The column of word `n` on the long line starting at `start`.
fn word(start: usize, n: usize) -> usize {
    start + n * 4
}

scenario!(
    /// Dragging over words near the start of a line selects them, and the
    /// copy keys put them on the clipboard.
    select_and_copy_near_the_start,
    |ui| {
        let (_server, row, start) = long_line(ui);
        ui.drag(
            (row, word(start, 1)),
            (row, word(start, 3) + 2),
            Modifiers::none(),
        );
        ui.shortcut("c");
        ui.wait_until("the copy", |ui| ui.clipboard().is_some());
        assert_eq!(ui.clipboard().as_deref(), Some("w01 w02 w03"));
    }
);

scenario!(
    /// Far along a line, a drag selects the words drawn under the mouse,
    /// not words further right.
    select_and_copy_far_along_a_line,
    |ui| {
        let (_server, row, start) = long_line(ui);
        ui.drag(
            (row, word(start, 30)),
            (row, word(start, 32) + 2),
            Modifiers::none(),
        );
        ui.shortcut("c");
        ui.wait_until("the copy", |ui| ui.clipboard().is_some());
        assert_eq!(ui.clipboard().as_deref(), Some("w30 w31 w32"));
    }
);

/// Signed in, with the shell's lines `links` (`link <url> <text>`) said:
/// OSC 8 hyperlinks, each on its own row, as `ls --hyperlink` or a build
/// tool prints them.
fn links(ui: &mut Ui, links: &[(&str, &str)]) -> Server {
    let server = signed_in(ui);
    for (url, text) in links {
        ui.line(&format!("link {url} {text}"));
    }
    ui.wait_for_count(links.last().map_or("", |l| l.1), 2);
    server
}

/// The row that starts with `text` (a link's own row, not the line typed).
fn row_of(ui: &mut Ui, text: &str) -> usize {
    let frame = ui.view(|v, _| v.latest_frame());
    frame
        .rows
        .iter()
        .position(|r| r.text.starts_with(text))
        .unwrap_or_else(|| panic!("no row starts with {text:?}:\n{}", ui.screen()))
}

scenario!(
    /// Ctrl+click (Cmd on macOS) on a hyperlink opens it in the browser.
    ctrl_click_opens_a_link,
    |ui| {
        let _server = links(ui, &[("https://example.com/docs", "Read-the-docs")]);
        let row = row_of(ui, "Read-the-docs");
        ui.click_cell(row, 3, Modifiers::secondary_key());
        assert_eq!(ui.opened_url().as_deref(), Some("https://example.com/docs"));
    }
);

scenario!(
    /// A plain click on a hyperlink opens nothing (it starts a selection).
    plain_click_on_a_link_opens_nothing,
    |ui| {
        let _server = links(ui, &[("https://example.com/docs", "Read-the-docs")]);
        let row = row_of(ui, "Read-the-docs");
        ui.click_cell(row, 3, Modifiers::none());
        assert_eq!(ui.opened_url(), None);
    }
);

scenario!(
    /// Links to a local file or to script are not opened, even with
    /// Ctrl+click: a server could print them to run something here.
    file_and_script_links_are_not_opened,
    |ui| {
        let _server = links(
            ui,
            &[
                ("file:///etc/passwd", "Local-file"),
                ("javascript:alert(1)", "Some-script"),
            ],
        );
        for text in ["Local-file", "Some-script"] {
            let row = row_of(ui, text);
            ui.click_cell(row, 3, Modifiers::secondary_key());
        }
        assert_eq!(ui.opened_url(), None);
    }
);

scenario!(
    /// An address in plain text far along a line opens with Ctrl+click on
    /// its last letter, where it is drawn.
    address_far_along_a_line_opens,
    |ui| {
        const URL: &str = "https://example.com/far";
        let _server = signed_in(ui);
        ui.line(&format!("{} {URL}", "x".repeat(120)));
        ui.wait_for("you said: xxx");
        let (row, col) = ui.find_text("you said: xxx");
        let last = col + "you said: ".len() + 121 + URL.len() - 1;
        ui.click_cell(row, last, Modifiers::secondary_key());
        assert_eq!(ui.opened_url().as_deref(), Some(URL));
    }
);

scenario!(
    /// A full screen program that takes the mouse gets a click as a report of
    /// the cell under it, far along the screen too (column and row from 1).
    clicks_reach_a_full_screen_program,
    |ui| {
        let server = signed_in(ui);
        ui.line("mouse");
        ui.wait_for("MOUSE PROGRAM");
        ui.wait_until("the program to take the mouse", |ui| {
            ui.view(|v, _| v.latest_frame().mouse_tracking)
        });
        ui.click_cell(5, 120, Modifiers::none());
        let (press, release) = ("\x1b[<0;121;6M", "\x1b[<0;121;6m");
        ui.wait_until("the click's reports", |_| {
            let input = server.seen(|s| String::from_utf8_lossy(&s.input).into_owned());
            input.contains(press) && input.contains(release)
        });
    }
);

scenario!(
    /// After a flood of output, the wheel scrolls back through it; typing
    /// brings the newest lines back.
    scroll_back_after_a_flood,
    |ui| {
        let _server = signed_in(ui);
        ui.line("lines 400");
        ui.wait_for("line 400");
        let at = ui.cell_point(10, 10);
        let mut vcx = gpui::VisualTestContext::from_window(ui.window().into(), &ui.cx);
        for _ in 0..10 {
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: at,
                delta: gpui::ScrollDelta::Lines(gpui::point(0., 5.)),
                ..Default::default()
            });
        }
        ui.wait_until("scrolled back", |ui| {
            ui.view(|v, _| v.latest_frame().scrolled_back)
        });
        let screen = ui.screen();
        assert!(!screen.contains("line 400"), "still at the bottom:\n{screen}");
        // Some 50 lines further up, a screenful of what came before.
        let first = ui.view(|v, _| v.latest_frame().scroll.offset);
        assert!(first < 400 - 50, "scrolled {first}:\n{screen}");
        assert!(screen.contains("line 2"), "not in the output:\n{screen}");
        ui.type_text("x");
        ui.wait_until("back at the bottom", |ui| {
            !ui.view(|v, _| v.latest_frame().scrolled_back)
        });
        assert!(ui.screen().contains("line 400"));
    }
);

scenario!(
    /// The pane's right-click menu: Copy takes the selection, Paste types the
    /// clipboard into the shell.
    copy_and_paste_from_the_pane_menu,
    |ui| {
        let (_server, row, start) = long_line(ui);
        ui.drag((row, word(start, 4)), (row, word(start, 5) + 2), Modifiers::none());
        ui.wait_until("a selection", |ui| ui.view(|v, _| v.latest_frame().selected));
        ui.right_click("terminal");
        ui.click("menu-copy");
        ui.wait_until("the copy", |ui| ui.clipboard().as_deref() == Some("w04 w05"));
        ui.copy_to_clipboard("pasted words");
        ui.right_click("terminal");
        ui.click("menu-paste");
        ui.keys("enter");
        ui.wait_for("you said: pasted words");
    }
);

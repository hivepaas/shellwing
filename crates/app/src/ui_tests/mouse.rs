//! The mouse in the terminal: selecting text and copying it, and links.

use gpui::Modifiers;

use super::{Policy, Ui, scenario};

/// A server, signed in to, whose shell has said a long line of numbered
/// words (`w00 w01 … w39`), the row it is on and the column it starts at.
fn long_line(ui: &mut Ui) -> (usize, usize) {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    let words: Vec<String> = (0..40).map(|i| format!("w{i:02}")).collect();
    ui.line(&words.join(" "));
    ui.wait_for("you said: w00");
    let (row, col) = ui.find_text("you said: w00");
    std::mem::forget(server);
    (row, col + "you said: ".len())
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
        let (row, start) = long_line(ui);
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
        let (row, start) = long_line(ui);
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

//! Tabs and panes: several sessions in one window, as a user keeps them.

use super::{Policy, Ui, scenario};
use crate::session::Status;

fn signed_in(ui: &mut Ui) -> super::Server {
    let server = ui.server(Policy::password());
    ui.trust(&server);
    ui.connect(&server.spec());
    ui.login();
    server
}

scenario!(
    /// Two tabs to two servers: each keeps its own session, and switching
    /// tabs switches what is typed into.
    two_tabs_two_servers,
    |ui| {
        let a = signed_in(ui);
        let b = ui.server(Policy::password());
        ui.trust(&b);
        ui.palette(&b.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
        ui.line("to b");
        ui.wait_for("you said: to b");
        ui.keys("ctrl-tab");
        ui.wait_until("the first tab", |ui| ui.ws(|ws, _, _| ws.active_index()) == 0);
        ui.line("to a");
        ui.wait_for("you said: to a");
        assert_eq!(a.seen(|s| s.lines.clone()), ["to a"]);
        assert_eq!(b.seen(|s| s.lines.clone()), ["to b"]);
    }
);

scenario!(
    /// Alt+number (Cmd on macOS) goes straight to a tab.
    go_to_a_tab_by_number,
    |ui| {
        let server = signed_in(ui);
        ui.palette(&server.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
        let first = if cfg!(target_os = "macos") { "cmd-1" } else { "alt-1" };
        ui.keys(first);
        ui.wait_until("the first tab", |ui| ui.ws(|ws, _, _| ws.active_index()) == 0);
        let second = if cfg!(target_os = "macos") { "cmd-2" } else { "alt-2" };
        ui.keys(second);
        ui.wait_until("the second tab", |ui| ui.ws(|ws, _, _| ws.active_index()) == 1);
    }
);

scenario!(
    /// Splitting a pane opens a second session to the same host beside it.
    split_opens_a_second_session,
    |ui| {
        let server = signed_in(ui);
        ui.shortcut("d");
        ui.wait_until("two panes", |ui| ui.panes() == 2);
        ui.login();
        ui.line("in the new pane");
        ui.wait_for("you said: in the new pane");
        assert_eq!(ui.tabs(), 1);
        assert_eq!(server.seen(|s| s.connections), 2);
    }
);

scenario!(
    /// Closing a pane whose session is open asks first; Cancel keeps it,
    /// Close ends it.
    closing_an_open_session_asks_first,
    |ui| {
        signed_in(ui);
        let close = if cfg!(target_os = "macos") { "cmd-w" } else { "ctrl-shift-w" };
        ui.keys(close);
        ui.answer_prompt("Cancel");
        assert_eq!(ui.tabs(), 1);
        assert_eq!(ui.status(), Status::Connected);
        ui.keys(close);
        ui.answer_prompt("Close");
        ui.wait_until("the tab to close", |ui| ui.tabs() == 0);
    }
);

scenario!(
    /// A session that already ended closes without a question.
    closing_an_ended_session_does_not_ask,
    |ui| {
        signed_in(ui);
        ui.line("exit");
        ui.wait_until("the session to close", |ui| ui.status() == Status::Closed);
        let close = if cfg!(target_os = "macos") { "cmd-w" } else { "ctrl-shift-w" };
        ui.keys(close);
        ui.wait_until("the tab to close", |ui| ui.tabs() == 0);
        assert!(!ui.cx.has_pending_prompt());
    }
);

scenario!(
    /// Typing to every pane (broadcast): one line reaches both sessions.
    broadcast_typing_reaches_every_pane,
    |ui| {
        let server = signed_in(ui);
        ui.shortcut("d");
        ui.wait_until("two panes", |ui| ui.panes() == 2);
        ui.login();
        ui.keys(if cfg!(target_os = "macos") { "cmd-shift-i" } else { "ctrl-shift-i" });
        ui.line("uptime everywhere");
        ui.wait_until("both sessions to get it", |_| {
            server.seen(|s| s.lines.iter().filter(|l| *l == "uptime everywhere").count()) == 2
        });
    }
);

scenario!(
    /// Broadcast with one pane only says how to get a second one.
    broadcast_needs_two_panes,
    |ui| {
        let server = signed_in(ui);
        ui.keys(if cfg!(target_os = "macos") { "cmd-shift-i" } else { "ctrl-shift-i" });
        ui.wait_for_notice("Split");
        ui.line("just here");
        ui.wait_for("you said: just here");
        assert_eq!(server.seen(|s| s.lines.clone()), ["just here"]);
    }
);

scenario!(
    /// One tab's server dying leaves the other tab working.
    one_tab_dies_the_other_lives,
    |ui| {
        let a = signed_in(ui);
        let b = ui.server(Policy::password());
        ui.trust(&b);
        ui.palette(&b.spec());
        ui.wait_until("a second tab", |ui| ui.tabs() == 2);
        ui.login();
        a.kill();
        ui.settle(std::time::Duration::from_millis(500));
        ui.line("still fine");
        ui.wait_for("you said: still fine");
        ui.keys("ctrl-tab");
        ui.wait_until("the first tab lost", |ui| {
            ui.ws(|ws, _, _| ws.active_index()) == 0 && ui.status() != Status::Connected
        });
    }
);

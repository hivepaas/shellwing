//! The local terminal: a shell on this computer, beside the SSH tabs.

use super::scenario;
use crate::session::Status;

/// A command that prints `shellwing-42` in the platform's shell.
fn arithmetic() -> &'static str {
    if cfg!(windows) {
        "echo \"shellwing-$(40+2)\""
    } else {
        "echo shellwing-$((40+2))"
    }
}

scenario!(
    /// The local terminal runs this computer's shell.
    local_shell_runs_commands,
    |ui| {
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.settle(std::time::Duration::from_millis(500));
        ui.line(arithmetic());
        ui.wait_for("shellwing-42");
    }
);

scenario!(
    /// From the palette: "Local terminal".
    local_shell_from_the_palette,
    |ui| {
        ui.open();
        ui.palette("local");
        ui.wait_until("a tab", |ui| ui.tabs() == 1);
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.settle(std::time::Duration::from_millis(500));
        ui.line(arithmetic());
        ui.wait_for("shellwing-42");
    }
);

scenario!(
    /// `exit` ends the local shell; Enter starts it again.
    local_shell_exits_and_restarts,
    |ui| {
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.settle(std::time::Duration::from_millis(500));
        ui.line("exit");
        ui.wait_until("the shell to end", |ui| ui.status() == Status::Closed);
        ui.keys("enter");
        ui.wait_until("the shell to start again", |ui| ui.status() == Status::Connected);
        ui.settle(std::time::Duration::from_millis(500));
        ui.line(arithmetic());
        ui.wait_for("shellwing-42");
    }
);

scenario!(
    /// The Files panel is for servers: on a local pane it says so.
    files_panel_is_for_servers,
    |ui| {
        ui.connect("local:");
        ui.wait_until("the shell to start", |ui| ui.status() == Status::Connected);
        ui.shortcut("o");
        ui.wait_for_notice("SFTP");
        assert!(!ui.view(|v, _| v.files_shown()));
    }
);

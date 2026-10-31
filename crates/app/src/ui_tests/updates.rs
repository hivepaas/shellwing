//! Looking for updates, as Settings › About shows it: a newer release in the
//! signed list is offered, a skipped one is not offered again, a list whose
//! signature does not hold is refused, and no list yet is "up to date".
//!
//! The list is crates/update's test data: signed with test keys (thrown
//! away), offering v99.0.0 on the stable channel, with a `.deb` and an `.rpm`
//! for x86_64 (crates/update/testdata/make.sh).

use shellwing_update::{Install, PackageTool, Version};

use super::server::http_file;
use super::{Ui, scenario};
use crate::settings_view::Page;

const SIGNED: &str = include_str!("../../../update/testdata/release.signed.json");
const KEYS: [(&str, &str); 2] = [
    (
        "test-ed",
        include_str!("../../../update/testdata/test-ed.pub.pem"),
    ),
    (
        "test-ml",
        include_str!("../../../update/testdata/test-ml.pub.pem"),
    ),
];

/// The release the test list offers.
fn offered() -> Version {
    Version::parse("v99.0.0").expect("version")
}

/// The window open, the list served (`None`: not published yet) and
/// trusted, and Check for updates chosen in the palette; Settings › About
/// shows, once the look is done. False when this shellwing is not older than
/// the list's release (see the module's note).
fn check(ui: &mut Ui, list: Option<&str>) -> bool {
    if offered() <= shellwing_update::current() {
        eprintln!("skipped: the test list's release is not newer than this build");
        return false;
    }
    let port = http_file(&ui.rt, list.map(|l| l.as_bytes().to_vec()));
    shellwing_update::set_test_source(
        &format!("http://127.0.0.1:{port}/release.signed.json"),
        &KEYS,
    );
    ui.open();
    ui.palette("Check for updates");
    ui.wait_until("About", |ui| ui.settings_page() == Some(Page::About));
    wait_checked(ui);
    true
}

fn wait_checked(ui: &mut Ui) {
    ui.wait_until("the look to end", |ui| {
        !ui.ws(|ws, _, _| ws.update_checking())
    });
}

fn offer(ui: &mut Ui) -> Option<Version> {
    ui.ws(|ws, _, _| ws.update_offered())
}

fn problem(ui: &mut Ui) -> Option<String> {
    ui.ws(|ws, _, _| ws.update_problem())
}

scenario!(
    /// A newer release in the signed list is offered in About, with a way
    /// to get it.
    newer_release_offered,
    |ui| {
        if !check(ui, Some(SIGNED)) {
            return;
        }
        assert_eq!(problem(ui), None);
        assert_eq!(offer(ui), Some(offered()));
        assert!(ui.find("update-get").is_some(), "no way to get it");
    }
);

scenario!(
    /// Skip this version: the release is not offered again, even after
    /// looking again.
    skipped_release_not_offered_again,
    |ui| {
        if !check(ui, Some(SIGNED)) {
            return;
        }
        ui.click("update-skip");
        ui.wait_until("the offer to go", |ui| offer(ui).is_none());
        assert!(ui.find("update-get").is_none());
        ui.click("update-check");
        wait_checked(ui);
        assert_eq!(offer(ui), None, "offered again");
        assert_eq!(problem(ui), None);
    }
);

scenario!(
    /// A list whose signature does not hold (changed on the way, or not the
    /// release's) is refused: nothing is offered, and About says why.
    list_with_a_bad_signature_refused,
    |ui| {
        // One character of the first signature changed.
        let at = SIGNED.find("\"sig\":\"").expect("a signature") + "\"sig\":\"".len();
        let mut tampered = SIGNED.to_string();
        let was = tampered.as_bytes()[at];
        let now = if was == b'A' { "B" } else { "A" };
        tampered.replace_range(at..at + 1, now);
        if !check(ui, Some(&tampered)) {
            return;
        }
        assert_eq!(offer(ui), None);
        let why = problem(ui).unwrap_or_default();
        assert!(why.contains("signature"), "{why}");
        assert!(ui.find("update-get").is_none());
    }
);

scenario!(
    /// No list published yet (before the first release): up to date, with
    /// nothing wrong.
    no_list_yet_is_up_to_date,
    |ui| {
        if !check(ui, None) {
            return;
        }
        assert_eq!(offer(ui), None);
        assert_eq!(problem(ui), None);
    }
);

scenario!(
    /// The row over the panes tells of the release, and Update goes to the
    /// section of About that gets it.
    the_row_tells_and_goes_to_about,
    |ui| {
        if !check(ui, Some(SIGNED)) {
            return;
        }
        ui.ws(|ws, window, cx| ws.close_settings_for_test(window, cx));
        assert!(ui.find("update-row").is_some(), "no row");
        ui.click("update-row-open");
        ui.wait_until("About", |ui| ui.settings_page() == Some(Page::About));
        assert!(ui.find("update-get").is_some());
    }
);

scenario!(
    /// The row's ✕ closes it for this run and offers the release still; its
    /// Skip offers it no more.
    the_row_closes_and_skips,
    |ui| {
        if !check(ui, Some(SIGNED)) {
            return;
        }
        ui.ws(|ws, window, cx| ws.close_settings_for_test(window, cx));
        ui.click("update-row-close");
        ui.wait_until("the row to go", |ui| ui.find("update-row").is_none());
        assert_eq!(offer(ui), Some(offered()), "closing is not skipping");

        ui.ws(|ws, _, cx| ws.reopen_update_row_for_test(cx));
        ui.click("update-row-skip");
        ui.wait_until("the row to go", |ui| ui.find("update-row").is_none());
        assert_eq!(offer(ui), None);
    }
);

scenario!(
    /// A copy from the release's `.deb`: About shows the commands with the
    /// list's hash, and opening them types them into a new local terminal,
    /// with Enter left to the user.
    a_package_gets_the_commands_typed_not_run,
    |ui| {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return;
        }
        shellwing_update::set_test_install(Install::Package(PackageTool::Apt));
        if !check(ui, Some(SIGNED)) {
            return;
        }
        assert!(ui.find("update-steps").is_some(), "no commands");
        assert!(ui.find("update-get").is_none());
        let steps = ui.ws(|ws, _, _| ws.update_steps()).expect("steps");
        assert!(steps.iter().any(|s| s.contains(
            "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae  shellwing_99.0.0_amd64.deb"
        )));
        assert!(steps.iter().any(|s| s == "sudo apt install ./shellwing_99.0.0_amd64.deb"));

        ui.click("update-type");
        ui.wait_until("a tab", |ui| ui.tabs() == 1);
        ui.wait_for("mktemp");
        ui.settle(std::time::Duration::from_millis(500));
        // Not run: the shell has made no temporary folder, and curl said nothing.
        let screen = ui.screen();
        assert!(!screen.contains("sha256sum: "), "{screen}");
        assert!(!screen.contains("curl: "), "{screen}");
    }
);

scenario!(
    /// A package whose release the list names no file for: the page, and what
    /// to do with it, not commands.
    a_package_the_list_does_not_name_gets_the_page,
    |ui| {
        // The list has no package for this tool's kind on another architecture.
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return;
        }
        shellwing_update::set_test_install(Install::Package(PackageTool::Apt));
        if !check(ui, Some(SIGNED)) {
            return;
        }
        assert!(ui.find("update-steps").is_none());
        assert!(ui.find("update-get").is_some());
    }
);

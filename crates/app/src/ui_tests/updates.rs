//! Looking for updates, as Settings › About shows it: a newer release in the
//! signed list is offered, a skipped one is not offered again, a list whose
//! signature does not hold is refused, and no list yet is "up to date".
//!
//! The list is crates/update's test data: signed with test keys (thrown
//! away), offering v0.2.0 on the stable channel. Once shellwing is 0.2.0 or
//! newer, make it again with a newer release (crates/update/testdata/make.sh).

use shellwing_update::Version;

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
    Version::parse("v0.2.0").expect("version")
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

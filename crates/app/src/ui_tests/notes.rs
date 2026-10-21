//! Images in notes: pasted, chosen from a file, kept in the vault and shown
//! again when the note opens (`docs/design/note-images.md`).

use std::path::PathBuf;

use gpui::{ClipboardEntry, ClipboardItem, ExternalPaths, Image, ImageFormat};

use super::{Ui, scenario};

/// A screenshot as PNG: flat colors and sharp bars.
fn screenshot_png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(640, 400, |x, y| {
        if y % 40 < 12 && x % 300 < 220 {
            image::Rgb([30, 30, 30])
        } else {
            image::Rgb([245, 246, 248])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

/// A new note's window, open with its text loaded, the caret at its end.
fn new_note(ui: &mut Ui) {
    ui.open();
    ui.wait_until("the vault to open", |ui| ui.vault().is_some());
    ui.ws(|ws, _, cx| ws.open_note(None, cx));
    ui.wait_until("the note's window", |ui| ui.note_window().is_some());
    ui.wait_until("its text", |ui| ui.note(|n, _, _| n.is_loaded()));
    ui.note(|n, window, cx| n.focus_text_end(window, cx));
}

fn paste_keys() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

/// Pastes what the clipboard holds into the note.
fn paste(ui: &mut Ui) {
    let window = ui.note_window().expect("a note window");
    ui.cx.simulate_keystrokes(window.into(), paste_keys());
}

/// Draws the note's window until its images are drawn (they load off the UI
/// thread once the editor asks for them).
fn wait_for_images(ui: &mut Ui, n: usize) {
    ui.wait_until(&format!("{n} image(s) drawn"), |ui| {
        let window = ui.note_window().expect("a note window");
        ui.draw_window(window.into());
        ui.note(|v, _, _| v.images_ready()) >= n
    });
}

fn text(ui: &mut Ui) -> String {
    ui.note(|n, _, cx| n.editor_text(cx))
}

scenario!(
    /// A screenshot on the clipboard, pasted into a note: kept in the vault,
    /// a line for it at the caret, and drawn.
    paste_a_screenshot_into_a_note,
    |ui| {
        new_note(ui);
        let before = text(ui);
        ui.cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            screenshot_png(),
        )));
        paste(ui);
        ui.wait_until("the image's line", |ui| text(ui).contains("![](attachment:"));
        let after = text(ui);
        assert!(after.starts_with(&before), "the text before stays: {after:?}");
        let id = shellwing_vault::attachment_refs(&after)
            .pop()
            .expect("a reference");
        let kept = ui.vault().expect("vault").attachment(&id).expect("read");
        assert!(
            kept.is_some_and(|b| b.starts_with(b"\x89PNG")),
            "kept as PNG"
        );
        wait_for_images(ui, 1);
        assert_eq!(ui.note(|n, _, _| n.problem()), None);
    }
);

scenario!(
    /// The note saved and opened again: its image is read back from the
    /// vault and drawn.
    a_note_keeps_its_images,
    |ui| {
        new_note(ui);
        ui.cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            screenshot_png(),
        )));
        paste(ui);
        ui.wait_until("the image's line", |ui| text(ui).contains("![](attachment:"));
        ui.note(|n, _, cx| n.save_now(cx));
        let id = ui.note(|n, _, _| n.note_id());
        assert!(!id.is_empty(), "saved");
        let window = ui.note_window().expect("window");
        window
            .update(&mut ui.cx, |_, window, _| window.remove_window())
            .expect("close");
        ui.cx.run_until_parked();
        let vault = ui.vault().expect("vault");
        let note = vault
            .notes()
            .expect("notes")
            .into_iter()
            .find(|n| n.id == id)
            .expect("the note");
        ui.ws(|ws, _, cx| ws.open_note(Some(note), cx));
        ui.wait_until("the note's window", |ui| ui.note_window().is_some());
        ui.wait_until("its text", |ui| ui.note(|n, _, _| n.is_loaded()));
        assert!(text(ui).contains("![](attachment:"));
        wait_for_images(ui, 1);
    }
);

scenario!(
    /// "Insert Image…": a file chosen in the dialog goes in.
    insert_an_image_from_a_file,
    |ui| {
        new_note(ui);
        let file = ui.dir.join("diagram.png");
        std::fs::write(&file, screenshot_png()).expect("file");
        ui.note(|n, _, cx| n.choose_insert_image(cx));
        ui.wait_until("the dialog", |ui| ui.cx.did_prompt_for_paths());
        let chosen = file.clone();
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
        ui.wait_until("the image's line", |ui| text(ui).contains("![](attachment:"));
        wait_for_images(ui, 1);
    }
);

scenario!(
    /// A file copied that is not an image (a script): refused, said why, and
    /// nothing goes into the note.
    a_file_that_is_not_an_image_is_refused,
    |ui| {
        new_note(ui);
        let before = text(ui);
        let script: PathBuf = ui.dir.join("deploy.sh");
        std::fs::write(&script, "#!/bin/sh\necho deploy\n").expect("file");
        ui.cx.write_to_clipboard(ClipboardItem {
            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                vec![script].into(),
            ))],
        });
        paste(ui);
        ui.wait_until("a problem said", |ui| ui.note(|n, _, _| n.problem()).is_some());
        let said = ui.note(|n, _, _| n.problem()).unwrap_or_default();
        assert!(said.contains("deploy.sh"), "{said}");
        assert_eq!(text(ui), before);
    }
);

scenario!(
    /// A plain text note shows no images: pasting one says so.
    images_only_in_markdown_notes,
    |ui| {
        new_note(ui);
        ui.note(|n, window, cx| n.make_plain(window, cx));
        ui.note(|n, window, cx| n.focus_text_end(window, cx));
        let before = text(ui);
        ui.cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            screenshot_png(),
        )));
        paste(ui);
        ui.wait_until("a problem said", |ui| ui.note(|n, _, _| n.problem()).is_some());
        assert!(
            ui.note(|n, _, _| n.problem())
                .unwrap_or_default()
                .contains("Markdown")
        );
        assert_eq!(text(ui), before);
    }
);

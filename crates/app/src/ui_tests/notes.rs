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

/// Pastes `image` and waits for its line; the attachment's src.
fn pasted_image(ui: &mut Ui, image: Vec<u8>) -> String {
    ui.cx
        .write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            image,
        )));
    paste(ui);
    ui.wait_until("the image's line", |ui| {
        text(ui).contains("![](attachment:")
    });
    let id = shellwing_vault::attachment_refs(&text(ui))
        .pop()
        .expect("a reference");
    format!("attachment:{id}")
}

scenario!(
    /// Copy Image, in an image's menu: the image itself on the clipboard.
    copy_an_image,
    |ui| {
        new_note(ui);
        let src = pasted_image(ui, screenshot_png());
        ui.cx.write_to_clipboard(ClipboardItem::new_string("something else".into()));
        ui.note(|n, _, cx| n.image_item("copy-image", &src, cx));
        let item = ui.cx.read_from_clipboard().expect("clipboard");
        let copied = item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(i) => Some(i.bytes.clone()),
            _ => None,
        });
        assert!(
            copied.is_some_and(|b| b.starts_with(b"\x89PNG")),
            "the image is on the clipboard"
        );
    }
);

scenario!(
    /// Save Image As…, in an image's menu: written where the dialog says.
    save_an_image_as_a_file,
    |ui| {
        new_note(ui);
        let src = pasted_image(ui, screenshot_png());
        let target = ui.dir.join("saved.png");
        ui.note(|n, _, cx| n.image_item("save-image", &src, cx));
        let to = target.clone();
        ui.cx.simulate_new_path_selection(move |_| Some(to));
        ui.wait_until("the file", |_| {
            std::fs::read(&target).is_ok_and(|b| b.starts_with(b"\x89PNG"))
        });
    }
);

scenario!(
    /// Files dropped in the middle of a line go in after that line, not
    /// splitting it.
    a_dropped_image_goes_after_the_line_under_the_pointer,
    |ui| {
        new_note(ui);
        let first_line = text(ui).lines().next().unwrap_or_default().to_string();
        let file = ui.dir.join("dropped.png");
        std::fs::write(&file, screenshot_png()).expect("file");
        // Dropped over the middle of the first line.
        ui.note(|n, _, cx| n.drop_at(vec![file], first_line.len() / 2, cx));
        ui.wait_until("the image's line", |ui| text(ui).contains("![](attachment:"));
        let after = text(ui);
        let mut lines = after.lines();
        assert_eq!(lines.next(), Some(first_line.as_str()), "the line is whole");
        assert!(lines.next().is_some_and(|l| l.starts_with("![](attachment:")));
    }
);

scenario!(
    /// Markdown pasted from elsewhere with an image as a data: URI: the
    /// image becomes an attachment, and the URI leaves the note's text.
    a_pasted_data_uri_becomes_an_attachment,
    |ui| {
        use base64::Engine as _;
        new_note(ui);
        let uri = base64::engine::general_purpose::STANDARD.encode(screenshot_png());
        ui.cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "From the wiki:\n![chart](data:image/png;base64,{uri})\n"
        )));
        paste(ui);
        ui.wait_until("the data: URI to become an attachment", |ui| {
            let t = text(ui);
            t.contains("![chart](attachment:") && !t.contains("data:image")
        });
        wait_for_images(ui, 1);
    }
);

scenario!(
    /// Export…: asked first (the files are not encrypted), then the note as
    /// note.md and its images in images/, the references rewritten.
    export_a_note_with_its_images,
    |ui| {
        new_note(ui);
        let src = pasted_image(ui, screenshot_png());
        let id = src.trim_start_matches("attachment:").to_string();
        ui.note(|n, window, cx| n.click_export(window, cx));
        ui.answer_prompt("Export");
        let dir = ui.dir.join("exports");
        std::fs::create_dir_all(&dir).expect("dir");
        ui.wait_until("the folder dialog", |ui| ui.cx.did_prompt_for_paths());
        let to = dir.clone();
        ui.cx.simulate_path_prompt_response(move |_| Some(vec![to]));
        ui.wait_until("the export", |_| {
            std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some())
        });
        let folder = std::fs::read_dir(&dir)
            .expect("dir")
            .next()
            .expect("a folder")
            .expect("entry")
            .path();
        ui.wait_until("note.md", |_| folder.join("note.md").exists());
        let md = std::fs::read_to_string(folder.join("note.md")).expect("note.md");
        assert!(md.contains(&format!("![](images/{id}.png)")), "{md}");
        assert!(!md.contains("attachment:"));
        assert!(folder.join(format!("images/{id}.png")).exists());
    }
);

/// A web server on this computer that serves `body` at any path, counting
/// the requests; its base URL.
fn image_server(body: Vec<u8>) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    (format!("http://127.0.0.1:{port}"), hits)
}

fn hits(n: &std::sync::Arc<std::sync::atomic::AtomicUsize>) -> usize {
    n.load(std::sync::atomic::Ordering::SeqCst)
}

scenario!(
    /// A note with an image on the web: shown as a label, never fetched by
    /// itself (opening a note tells no server anything).
    a_web_image_is_not_fetched_by_itself,
    |ui| {
        let (base, served) = image_server(screenshot_png());
        new_note(ui);
        ui.note(|n, _, cx| {
            n.set_text_for_test(&format!("# Dashboard\n![load]({base}/load.png)\n"), cx)
        });
        for _ in 0..5 {
            let window = ui.note_window().expect("window");
            ui.draw_window(window.into());
        }
        ui.settle(std::time::Duration::from_millis(300));
        assert_eq!(hits(&served), 0, "the server heard from the app");
        assert_eq!(ui.note(|n, _, _| n.images_ready()), 0);
    }
);

scenario!(
    /// Download Images, for an image on this computer's network: refused,
    /// said why, nothing asked of the server.
    a_web_image_on_a_private_network_is_not_downloaded,
    |ui| {
        let (base, served) = image_server(screenshot_png());
        new_note(ui);
        let written = format!("![router]({base}/admin.png)\n");
        ui.note(|n, _, cx| n.set_text_for_test(&written, cx));
        ui.note(|n, window, cx| n.click_download_images(window, cx));
        ui.answer_prompt("Download");
        ui.wait_until("a problem said", |ui| ui.note(|n, _, _| n.problem()).is_some());
        let said = ui.note(|n, _, _| n.problem()).unwrap_or_default();
        assert!(said.contains("private network"), "{said}");
        assert_eq!(hits(&served), 0);
        assert_eq!(text(ui), written);
    }
);

scenario!(
    /// Download Images: once asked, the image is downloaded once, kept as an
    /// attachment, its link replaced, and drawn; no more requests after.
    a_web_image_downloaded_into_the_note,
    |ui| {
        // The test's server is on this computer, which is otherwise refused.
        crate::note_images::ALLOW_LOCAL.store(true, std::sync::atomic::Ordering::Relaxed);
        let (base, served) = image_server(screenshot_png());
        new_note(ui);
        ui.note(|n, _, cx| {
            n.set_text_for_test(&format!("# Dashboard\n![load]({base}/load.png){{width=400}}\n"), cx)
        });
        ui.note(|n, window, cx| n.click_download_images(window, cx));
        ui.answer_prompt("Download");
        ui.wait_until("the link replaced", |ui| {
            text(ui).contains("![load](attachment:")
        });
        let t = text(ui);
        assert!(!t.contains(&base), "{t}");
        assert!(t.contains("{width=400}"), "the width stays: {t}");
        wait_for_images(ui, 1);
        assert_eq!(hits(&served), 1);
    }
);

/// Each note's title and text in `vault`, in order.
fn notes_of(vault: &shellwing_vault::Vault) -> Vec<(String, String)> {
    let mut notes: Vec<(String, String)> = vault
        .notes()
        .expect("notes")
        .into_iter()
        .map(|n| {
            let text = vault.note_text(&n.id).expect("text").expect("there");
            (n.title, text.to_string())
        })
        .collect();
    notes.sort();
    notes
}

scenario!(
    /// A note changed in its window while another device changed it too,
    /// neither seeing the other's change: both texts stay, the other
    /// device's as a note beside it, and the app says so
    /// (`docs/design/note-conflicts.md`).
    a_note_changed_on_two_devices_keeps_both_texts,
    |ui| {
        use shellwing_vault::{Folder, FolderState, MemoryKeys, Note, NoteFormat, Vault};
        // As main does.
        shellwing_vault::title_copies(crate::syncing::copy_title);
        let shared = ui.dir.join("shared");
        std::fs::create_dir_all(&shared).expect("shared folder");
        ui.config_file("config.toml", &format!("[sync]\nfolder = {shared:?}\n"));
        ui.open();
        ui.wait_until("the vault to open", |ui| ui.vault().is_some());
        let vault = ui.vault().expect("vault");
        let folder = Folder::new(&shared);
        ui.wait_until("the folder to sync", |_| {
            vault.folder_state(&folder).ok() == Some(FolderState::Ours)
        });
        let note = Note {
            id: String::new(),
            title: "Disk usage".into(),
            category: String::new(),
            format: NoteFormat::Markdown,
            hosts: vec![],
            sensitive: false,
            changed: String::new(),
            size: 0,
        };
        let id = vault.save_note(&note, Some("df -h")).expect("note");
        ui.ws(|ws, _, _| ws.sync_now());
        let record = shared.join(format!("shellwing/records/{id}.sw"));
        ui.wait_until("the note in the folder", |_| record.exists());

        // Opened here: it syncs again when its window closes.
        let note = Note { id: id.clone(), ..note };
        ui.ws(|ws, _, cx| ws.open_note(Some(note.clone()), cx));
        ui.wait_until("its text", |ui| ui.note(|n, _, _| n.is_loaded()));

        // Meanwhile another device changes it.
        let other = Vault::open(&ui.dir.join("other.db"), &MemoryKeys::default()).expect("other");
        let rk = vault.recovery_key().expect("read").expect("a key").to_string();
        other.join(&folder, &rk).expect("join");
        other.set_device_name("laptop");
        other.sync(&folder).expect("sync other");
        other.save_note(&note, Some("du -sh /var/log")).expect("edit other");
        other.sync(&folder).expect("sync other");

        // Then it is changed here, and the window closes.
        ui.note(|n, window, cx| n.click_edit(window, cx));
        ui.note(|n, window, cx| n.focus_text_end(window, cx));
        let window = ui.note_window().expect("window");
        ui.cx.simulate_input(window.into(), " /var");
        ui.wait_until("the edit", |ui| text(ui) == "df -h /var");
        window
            .update(&mut ui.cx, |_, window, _| window.remove_window())
            .expect("close");
        ui.wait_for_notice(
            "A note was changed on two devices: the other version is kept as Disk usage (from laptop).",
        );
        let both = vec![
            ("Disk usage".to_string(), "df -h /var".to_string()),
            ("Disk usage (from laptop)".to_string(), "du -sh /var/log".to_string()),
        ];
        assert_eq!(notes_of(&vault), both);
        let report = other.sync(&folder).expect("sync other");
        assert!(report.copies.is_empty(), "made once");
        assert_eq!(notes_of(&other), both);
    }
);

scenario!(
    /// A note's For a host list opens right below its chip, not off it.
    the_host_list_opens_below_its_chip,
    |ui| {
        new_note(ui);
        let window = ui.note_window().expect("a note window").into();
        ui.click_in(window, "note-add-host");
        let chip = ui.find_in(window, "note-add-host").expect("the chip");
        let list = ui.find_in(window, "note-host-list").expect("the list");
        let gap = list.top() - chip.bottom();
        assert!(gap >= gpui::px(0.) && gap <= gpui::px(6.), "below it: {list:?} under {chip:?}");
        assert!((list.left() - chip.left()).abs() <= gpui::px(1.), "from its left edge");
    }
);

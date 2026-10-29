//! The UI font where the settings name none. macOS and Windows draw in the
//! system's (GPUI's `.SystemUIFont`). Linux has no such font GPUI knows: it
//! asks for IBM Plex Sans, then takes the first of Helvetica, Segoe UI,
//! Ubuntu… that is installed, and the old Ubuntu font has no Vietnamese ệ or
//! ị: drawn as a letter and a dot below, the dot landed beside it. There the
//! desktop's own font (GNOME's, KDE's) is taken when it has the letters the
//! app's languages need, else the first well-known one installed that does.

use std::time::Duration;

use gpui::{App, SharedString, TextSystem, px};

/// Letters a UI font must have whole: Vietnamese, whose dot below a font
/// without them cannot place.
const PROBE: &str = "ạặậẹệịọộợụựỵẩễ";

/// Fonts most Linux desktops have, tried after the desktop's own.
const FAMILIES: [&str; 6] = [
    "Noto Sans",
    "Adwaita Sans",
    "Cantarell",
    "Ubuntu Sans",
    "DejaVu Sans",
    "Liberation Sans",
];

/// The family found at start, if the system's would not do (Linux).
pub struct SystemUiFont(pub Option<SharedString>);

impl gpui::Global for SystemUiFont {}

/// The UI font: the one the settings name, else the one found at start.
pub fn family(set: Option<&str>, cx: &App) -> Option<SharedString> {
    set.map(|f| SharedString::from(f.to_string()))
        .or_else(|| cx.try_global::<SystemUiFont>().and_then(|f| f.0.clone()))
}

/// Finds the family for the UI where the settings name none: None on macOS
/// and Windows, whose system font GPUI draws.
pub fn find(cx: &App) -> Option<SharedString> {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return None;
    }
    let text = cx.text_system();
    let installed = text.all_font_names();
    let desktop = desktop_font().and_then(|said| family_in(&said, &installed));
    desktop
        .as_deref()
        .into_iter()
        .chain(FAMILIES)
        .find(|f| installed.iter().any(|i| i == f) && draws(text, f))
        .map(|f| SharedString::from(f.to_string()))
}

/// Whether `family` has every letter of [`PROBE`].
fn draws(text: &TextSystem, family: &str) -> bool {
    let id = text.resolve_font(&gpui::font(family.to_string()));
    PROBE.chars().all(|c| text.advance(id, px(16.), c).is_ok())
}

/// The desktop's UI font as it says it (`Inter 11`): KDE's on KDE, else
/// GNOME's (which GNOME-based desktops share).
fn desktop_font() -> Option<String> {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("KDE"));
    if kde {
        kde_font().or_else(gnome_font)
    } else {
        gnome_font().or_else(kde_font)
    }
}

/// GNOME's, from gsettings: given half a second, as it starts the app.
fn gnome_font() -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "font-name"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let said = out.ok().filter(|o| o.status.success()).map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .trim_matches('\'')
                .to_string()
        });
        let _ = tx.send(said);
    });
    rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
}

/// KDE's, from kdeglobals.
fn kde_font() -> Option<String> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))?;
    kde_general_font(&std::fs::read_to_string(dir.join("kdeglobals")).ok()?)
}

/// `font=` of kdeglobals' `[General]`: its family, before the first comma.
fn kde_general_font(text: &str) -> Option<String> {
    let mut general = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general && let Some(value) = line.strip_prefix("font=") {
            return value.split(',').next().map(str::to_string);
        }
    }
    None
}

/// The installed family a font description names (`Inter Medium 11`): its
/// words before the size, as many as make one.
fn family_in(said: &str, installed: &[String]) -> Option<String> {
    let mut words: Vec<&str> = said.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|w| w.trim_end_matches("px").parse::<f32>().is_ok())
    {
        words.pop();
    }
    (1..=words.len())
        .rev()
        .map(|n| words[..n].join(" "))
        .find(|f| installed.iter().any(|i| i == f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_names_its_installed_family() {
        let installed: Vec<String> = ["Inter", "Noto Sans", "Ubuntu Sans"]
            .map(String::from)
            .into();
        assert_eq!(family_in("Inter 11", &installed).as_deref(), Some("Inter"));
        assert_eq!(
            family_in("Noto Sans Medium 10.5", &installed).as_deref(),
            Some("Noto Sans")
        );
        assert_eq!(
            family_in("Ubuntu Sans", &installed).as_deref(),
            Some("Ubuntu Sans")
        );
        // A generic name, or a font not installed, names none.
        assert_eq!(family_in("Sans 11", &installed), None);
        assert_eq!(family_in("Cantarell 11", &installed), None);
        assert_eq!(family_in("", &installed), None);
    }

    #[test]
    fn kde_says_its_font_in_general() {
        let text = "[Colors:View]\nfont=Wrong,9\n\n[General]\nColorScheme=Breeze\nfont=Noto Sans,10,-1,5,400,0,0,0,0,0\n\n[WM]\nfont=Other,10\n";
        assert_eq!(kde_general_font(text).as_deref(), Some("Noto Sans"));
        assert_eq!(kde_general_font("[General]\nColorScheme=Breeze\n"), None);
    }
}

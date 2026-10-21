//! The app's own colors, in one place: what each one is for, and its value in
//! the palette in use. The terminal's colors come from its theme instead.
//!
//! Views ask [`colors`] for a role (`colors().text`, `colors().edge`), never
//! for a value, so a palette is changed here alone. There are two, [`DUSK`]
//! and [`LIGHT`], filling in the same roles; the workspace picks one with
//! [`set_light`] and redraws.
//!
//! Values are `0xRRGGBB` for `gpui::rgb`; those ending in an alpha byte
//! (`0xRRGGBBAA`, see [`Palette::scrim`]) are for `gpui::rgba`.

/// Every color the app draws with, by what it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    // Surfaces, from the window's frame inwards.
    /// The title bar and its tabs.
    pub chrome: u32,
    /// Side panels: the host list, the Files panel, the Settings menu, an
    /// editor's footer.
    pub panel: u32,
    /// A page's own ground, such as Settings' and the welcome screen's.
    pub page: u32,
    /// Drawers, dialogs and cards on a page.
    pub surface: u32,
    /// What floats over the rest: menus, the palette, tooltips, toasts.
    pub raised: u32,
    /// Small filled tags and wells: a key's type, a count.
    pub chip: u32,
    /// Text fields, sunk below what is around them.
    pub field: u32,
    /// The active tab, which joins the pane under it.
    pub tab_on: u32,

    // Lines.
    /// Dividers and rules.
    pub line: u32,
    /// Around a text field.
    pub field_edge: u32,
    /// Around menus, cards, buttons.
    pub edge: u32,
    /// The window's own outline.
    pub window_edge: u32,

    // States.
    /// A row or button under the pointer.
    pub hover: u32,
    /// A menu item under the pointer.
    pub menu_hover: u32,
    /// What is chosen: the current row, the choice that is on.
    pub selected: u32,
    /// A button's fill, such as the window's buttons.
    pub button: u32,
    pub button_hover: u32,

    // Text and icons.
    pub text: u32,
    /// Second to the text: idle tabs, icons, values.
    pub soft: u32,
    /// Labels above fields.
    pub label: u32,
    /// Hints and notes.
    pub muted: u32,
    /// Placeholders and what is off.
    pub faint: u32,

    // The accent.
    pub accent: u32,
    pub accent_hover: u32,
    /// Text on the accent.
    pub on_accent: u32,

    // Status.
    /// Connected: the status dot.
    pub ok: u32,
    /// Good news, as text.
    pub good: u32,
    /// A warning, as text.
    pub warn: u32,
    /// The amber fill: connecting, typing in every pane, the Edit badge.
    pub amber: u32,
    pub amber_hover: u32,
    /// Text on amber.
    pub on_amber: u32,
    /// The connection was lost.
    pub lost: u32,
    /// The session ended.
    pub idle: u32,
    /// Bad news, as text.
    pub bad: u32,
    /// The red fill: failed, logging.
    pub danger: u32,

    // Notices, by tone (notice.rs): each a ground, an edge and text. The
    // good ground is also a good one's tag.
    pub good_bg: u32,
    pub warn_bg: u32,
    pub warn_edge: u32,
    pub warn_text: u32,
    pub warn_hover: u32,
    pub bad_bg: u32,
    pub bad_edge: u32,
    pub bad_text: u32,
    pub bad_hover: u32,
    pub good_edge: u32,
    pub good_text: u32,
    pub info_bg: u32,
    pub info_edge: u32,
    pub info_text: u32,

    // Translucent, `0xRRGGBBAA`.
    /// Behind a dialog or a drawer.
    pub scrim: u32,
    /// A bar over the terminal (connection ended, a host key changed).
    pub banner: u32,
    /// A bar over the terminal that says something went wrong.
    pub bad_banner: u32,
    /// Over a pane that does not get what is typed in every pane.
    pub dim_pane: u32,
    /// The lightest wash: the row of the active link.
    pub tint: u32,
    /// The window's and menus' shadow.
    pub shadow: u32,

    // The sync button's cloud.
    /// Nothing set up: an invitation, not a warning.
    pub sync_off: u32,
    /// Synced, or syncing.
    pub sync_ok: u32,
    /// The last sync failed.
    pub sync_bad: u32,
    /// Waiting: the vault is locked.
    pub sync_paused: u32,

    // The Files panel.
    pub folder: u32,
    /// A transfer's progress.
    pub progress: u32,
    /// Where dropped files go.
    pub drop: u32,

    // Shell code, as a snippet's editor colors it.
    /// A command's name.
    pub code_command: u32,
    /// `if`, `then`, `done`.
    pub code_keyword: u32,
    /// `-f`, `--all`.
    pub code_flag: u32,
    /// Quoted text.
    pub code_text: u32,
    /// `$VAR`, `$(…)`.
    pub code_expansion: u32,
    /// `|`, `&&`, `>`.
    pub code_operator: u32,
    /// `{{name}}`, filled in when it runs: on a ground of its own, `0xRRGGBBAA`.
    pub code_hole: u32,
    pub code_hole_ground: u32,

    // Finding text in the terminal.
    pub find_match: u32,
    /// The match Enter moved to.
    pub find_current: u32,
    /// Text on a match.
    pub find_text: u32,
}

/// The dark palette: blue-gray, a step lighter than near-black.
pub const DUSK: Palette = Palette {
    chrome: 0x21272f,
    panel: 0x252b34,
    page: 0x272e38,
    surface: 0x2b323d,
    raised: 0x2e3541,
    chip: 0x343c48,
    field: 0x1c2129,
    tab_on: 0x272e38,

    line: 0x363e4a,
    field_edge: 0x424b58,
    edge: 0x46505e,
    window_edge: 0x14181d,

    hover: 0x313843,
    menu_hover: 0x3b4553,
    selected: 0x344155,
    button: 0x2f3641,
    button_hover: 0x3d4552,

    text: 0xe7ebf0,
    soft: 0xc9d0d9,
    label: 0xb3bcc8,
    muted: 0xa0aab7,
    faint: 0x7d8796,

    accent: 0x4cc4f9,
    accent_hover: 0x7dd3fc,
    on_accent: 0x04121c,

    ok: 0x22c55e,
    good: 0x86efac,
    warn: 0xfbbf24,
    amber: 0xf59e0b,
    amber_hover: 0xfbbf24,
    on_amber: 0x1f1300,
    lost: 0xf97316,
    idle: 0x7b8595,
    bad: 0xf87171,
    danger: 0xef4444,

    good_bg: 0x1d3a2b,
    warn_bg: 0x3a2e14,
    warn_edge: 0x8a6414,
    warn_text: 0xfde68a,
    warn_hover: 0x4a3a18,
    bad_bg: 0x3a1a1f,
    bad_edge: 0x8a2a2a,
    bad_text: 0xfecaca,
    bad_hover: 0xa33232,
    good_edge: 0x2f7a4f,
    good_text: 0xbbf7d0,
    info_bg: 0x1b3550,
    info_edge: 0x2f6694,
    info_text: 0xd3ebfb,

    scrim: 0x0b0e13a0,
    banner: 0x1c2129ee,
    bad_banner: 0x2a1418f0,
    dim_pane: 0x1c212966,
    tint: 0xffffff0d,
    shadow: 0x00000073,

    sync_off: 0xf6c08a,
    sync_ok: 0x4ade80,
    sync_bad: 0xfb923c,
    sync_paused: 0x8b95a5,
    folder: 0x7aa2f7,
    progress: 0x3b82f6,
    drop: 0x2a3a55,

    code_command: 0x7dd3fc,
    code_keyword: 0xf0abfc,
    code_flag: 0xc4b5fd,
    code_text: 0xfde68a,
    code_expansion: 0xf9a8d4,
    code_operator: 0xfb923c,
    code_hole: 0x7dd3fc,
    code_hole_ground: 0x4cc4f933,

    find_match: 0xffe082,
    find_current: 0xf2a57e,
    find_text: 0x000000,
};

/// The light palette: cool grays, white where things float.
pub const LIGHT: Palette = Palette {
    chrome: 0xe8ecf1,
    panel: 0xf1f4f7,
    page: 0xf7f9fb,
    surface: 0xfbfcfd,
    raised: 0xffffff,
    chip: 0xe6ebf1,
    field: 0xffffff,
    tab_on: 0xfbfcfe,

    line: 0xd8dee5,
    field_edge: 0xc3ccd6,
    edge: 0xc3ccd6,
    window_edge: 0xa9b2bd,

    hover: 0xe3e9f0,
    menu_hover: 0xe6edf5,
    selected: 0xd9e8f8,
    button: 0xdce2e9,
    button_hover: 0xcfd7e0,

    text: 0x1b2230,
    soft: 0x3a4454,
    label: 0x4a5465,
    muted: 0x5a6576,
    faint: 0x8a94a3,

    accent: 0x0284c7,
    accent_hover: 0x0369a1,
    on_accent: 0xffffff,

    ok: 0x16a34a,
    good: 0x15803d,
    warn: 0xb45309,
    amber: 0xd97706,
    amber_hover: 0xb45309,
    on_amber: 0xffffff,
    lost: 0xea580c,
    idle: 0x94a3b8,
    bad: 0xc81e1e,
    danger: 0xdc2626,

    good_bg: 0xdcfce7,
    warn_bg: 0xfef3c7,
    warn_edge: 0xf2b84b,
    warn_text: 0x92400e,
    warn_hover: 0xfde68a,
    bad_bg: 0xfdecec,
    bad_edge: 0xeba3a3,
    bad_text: 0x991b1b,
    bad_hover: 0xfbd5d5,
    good_edge: 0x7fcf9c,
    good_text: 0x14532d,
    info_bg: 0xe2f1fc,
    info_edge: 0x8fc3ea,
    info_text: 0x0b3a5c,

    scrim: 0x0f172a59,
    banner: 0xffffffee,
    bad_banner: 0xfdf0f0f0,
    dim_pane: 0xffffff80,
    tint: 0x0000000d,
    shadow: 0x0f172a33,

    sync_off: 0xc77a2c,
    sync_ok: 0x16a34a,
    sync_bad: 0xea580c,
    sync_paused: 0x6b7280,
    folder: 0x2563eb,
    progress: 0x2563eb,
    drop: 0xdbeafe,

    code_command: 0x0369a1,
    code_keyword: 0xa21caf,
    code_flag: 0x6d28d9,
    code_text: 0x92400e,
    code_expansion: 0xbe185d,
    code_operator: 0xc2410c,
    code_hole: 0x0369a1,
    code_hole_ground: 0x0284c726,

    find_match: 0xffd54f,
    find_current: 0xf2a57e,
    find_text: 0x000000,
};

static LIGHT_IN_USE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The theme's accent in place of the palette's, `0xRRGGBB`; [`NO_ACCENT`]:
/// the palette's own.
static ACCENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(NO_ACCENT);
const NO_ACCENT: u32 = u32::MAX;

/// The palette in use, with the theme's accent if one was set.
pub fn colors() -> Palette {
    let light = LIGHT_IN_USE.load(std::sync::atomic::Ordering::Relaxed);
    let mut p = if light { LIGHT } else { DUSK };
    let accent = ACCENT.load(std::sync::atomic::Ordering::Relaxed);
    if accent != NO_ACCENT {
        p.accent = accent;
        // Lighter on dark, darker on light, as the palettes' own hover is.
        p.accent_hover = mix(accent, if light { 0x000000 } else { 0xffffff }, 0.25);
        // Whichever of dark and white text reads better on it.
        p.on_accent = if contrast(accent, 0x04121c) > contrast(accent, 0xffffff) {
            0x04121c
        } else {
            0xffffff
        };
    }
    p
}

/// Uses the light palette, or the dark one. Views pick it up when drawn
/// next; cached ones must be told to redraw.
pub fn set_light(light: bool) {
    LIGHT_IN_USE.store(light, std::sync::atomic::Ordering::Relaxed);
}

/// Takes the accent from a theme (`0xRRGGBB`), or (None) the palette's.
/// One that would not stand out from the page (contrast under 3:1) is
/// left out: the palette's is used instead.
pub fn set_accent(accent: Option<u32>) {
    let light = LIGHT_IN_USE.load(std::sync::atomic::Ordering::Relaxed);
    let page = if light { LIGHT.page } else { DUSK.page };
    let accent = accent.filter(|a| contrast(*a, page) >= 3.);
    ACCENT.store(
        accent.unwrap_or(NO_ACCENT),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn channels(c: u32) -> [f32; 3] {
    [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| v as f32 / 255.)
}

/// WCAG's relative luminance.
fn luminance(c: u32) -> f32 {
    let [r, g, b] = channels(c).map(|v| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG's contrast ratio, 1 to 21.
fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `a` moved `t` of the way to `b`.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let (a, b) = (channels(a), channels(b));
    let c: Vec<u32> = (0..3)
        .map(|i| ((a[i] + (b[i] - a[i]) * t) * 255.).round() as u32)
        .collect();
    (c[0] << 16) | (c[1] << 8) | c[2]
}

/// `color` (`0xRRGGBB`) with an alpha, for `gpui::rgba`.
pub fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color << 8) | u32::from(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_accent_is_taken_unless_it_would_not_show() {
        set_light(false);
        set_accent(Some(0xbd93f9));
        assert_eq!(colors().accent, 0xbd93f9);
        assert_eq!(colors().on_accent, 0x04121c, "dark text on a light accent");
        // Too close to Dusk's page to stand out.
        set_accent(Some(0x2f3a48));
        assert_eq!(colors().accent, DUSK.accent);
        // A dark blue would not show on Dusk; it does on the light palette.
        set_light(true);
        set_accent(Some(0x1d4ed8));
        assert_eq!(colors().accent, 0x1d4ed8);
        assert_eq!(colors().on_accent, 0xffffff, "white on a dark one");
        set_light(false);
        set_accent(None);
        assert_eq!(colors(), DUSK);
        assert!((contrast(0xffffff, 0x000000) - 21.).abs() < 0.01);
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
    }

    #[test]
    fn every_notice_reads_well_and_stands_out_from_the_page() {
        for p in [DUSK, LIGHT] {
            for (tone, ground, edge, text) in [
                ("info", p.info_bg, p.info_edge, p.info_text),
                ("good", p.good_bg, p.good_edge, p.good_text),
                ("warn", p.warn_bg, p.warn_edge, p.warn_text),
                ("bad", p.bad_bg, p.bad_edge, p.bad_text),
            ] {
                assert!(contrast(text, ground) >= 4.5, "{tone}: its text reads");
                for under in [p.page, p.surface, p.chrome] {
                    assert!(contrast(edge, under) >= 1.5, "{tone}: its box shows");
                }
            }
        }
    }
}

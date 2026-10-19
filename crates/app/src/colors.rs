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

    // Notices: a warning's box, an error's box; a good one's tag.
    pub good_bg: u32,
    pub warn_bg: u32,
    pub warn_edge: u32,
    pub warn_text: u32,
    pub warn_hover: u32,
    pub bad_bg: u32,
    pub bad_edge: u32,
    pub bad_text: u32,
    pub bad_hover: u32,

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

    // The Files panel.
    pub folder: u32,
    /// A transfer's progress.
    pub progress: u32,
    /// Where dropped files go.
    pub drop: u32,

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

    scrim: 0x0b0e13a0,
    banner: 0x1c2129ee,
    bad_banner: 0x2a1418f0,
    dim_pane: 0x1c212966,
    tint: 0xffffff0d,
    shadow: 0x00000073,

    folder: 0x7aa2f7,
    progress: 0x3b82f6,
    drop: 0x2a3a55,

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
    bad_edge: 0xf5b5b5,
    bad_text: 0x991b1b,
    bad_hover: 0xfbd5d5,

    scrim: 0x0f172a59,
    banner: 0xffffffee,
    bad_banner: 0xfdf0f0f0,
    dim_pane: 0xffffff80,
    tint: 0x0000000d,
    shadow: 0x0f172a33,

    folder: 0x2563eb,
    progress: 0x2563eb,
    drop: 0xdbeafe,

    find_match: 0xffd54f,
    find_current: 0xf2a57e,
    find_text: 0x000000,
};

static LIGHT_IN_USE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The palette in use.
pub fn colors() -> &'static Palette {
    if LIGHT_IN_USE.load(std::sync::atomic::Ordering::Relaxed) {
        &LIGHT
    } else {
        &DUSK
    }
}

/// Uses the light palette, or the dark one. Views pick it up when drawn
/// next; cached ones must be told to redraw.
pub fn set_light(light: bool) {
    LIGHT_IN_USE.store(light, std::sync::atomic::Ordering::Relaxed);
}

/// `color` (`0xRRGGBB`) with an alpha, for `gpui::rgba`.
pub fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color << 8) | u32::from(alpha)
}

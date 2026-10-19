use std::sync::Arc;

use serde::Deserialize;

use crate::color::Color;
use crate::store::LoadError;

/// The theme used when none is set or the one set does not exist.
pub const DEFAULT_THEME: &str = "dusk";
/// The theme used in the light mode when none is set.
pub const DEFAULT_LIGHT_THEME: &str = "dawn";

/// The groups themes are shown in, in order: (id, title). A theme names its
/// own with `group`; one that names none is the user's.
pub const GROUPS: &[(&str, &str)] = &[
    ("blue", "Blue and slate"),
    ("purple", "Purple and pink"),
    ("green", "Green and teal"),
    ("warm", "Warm"),
    ("neutral", "Neutral"),
    ("gradient", "Gradient"),
    ("gradient-light", "Gradient, light"),
    ("light-cool", "Light, cool"),
    ("light-warm", "Light, warm"),
];

/// Built in, by group, in the order the theme shortcut steps through them.
/// Those from iTerm2-Color-Schemes (MIT) say so in their files.
const BUILTIN: &[(&str, &str)] = &[
    ("dusk", include_str!("../themes/dusk.toml")),
    (
        "default-slate",
        include_str!("../themes/default-slate.toml"),
    ),
    ("tokyo-night", include_str!("../themes/tokyo-night.toml")),
    ("nord", include_str!("../themes/nord.toml")),
    ("cobalt2", include_str!("../themes/cobalt2.toml")),
    ("iceberg-dark", include_str!("../themes/iceberg-dark.toml")),
    (
        "kanagawa-wave",
        include_str!("../themes/kanagawa-wave.toml"),
    ),
    (
        "material-ocean",
        include_str!("../themes/material-ocean.toml"),
    ),
    ("night-owl", include_str!("../themes/night-owl.toml")),
    ("dracula", include_str!("../themes/dracula.toml")),
    ("catppuccin", include_str!("../themes/catppuccin.toml")),
    (
        "rose-pine-moon",
        include_str!("../themes/rose-pine-moon.toml"),
    ),
    ("rose-pine", include_str!("../themes/rose-pine.toml")),
    ("synthwave", include_str!("../themes/synthwave.toml")),
    ("ubuntu", include_str!("../themes/ubuntu.toml")),
    (
        "everforest-dark",
        include_str!("../themes/everforest-dark.toml"),
    ),
    ("homebrew", include_str!("../themes/homebrew.toml")),
    ("oceanic-next", include_str!("../themes/oceanic-next.toml")),
    ("poimandres", include_str!("../themes/poimandres.toml")),
    (
        "solarized-dark",
        include_str!("../themes/solarized-dark.toml"),
    ),
    ("ayu-mirage", include_str!("../themes/ayu-mirage.toml")),
    ("flexoki-dark", include_str!("../themes/flexoki-dark.toml")),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.toml")),
    ("melange-dark", include_str!("../themes/melange-dark.toml")),
    ("monokai-pro", include_str!("../themes/monokai-pro.toml")),
    ("zenburn", include_str!("../themes/zenburn.toml")),
    ("github-dark", include_str!("../themes/github-dark.toml")),
    ("one-dark", include_str!("../themes/one-dark.toml")),
    ("carbonfox", include_str!("../themes/carbonfox.toml")),
    ("nightfox", include_str!("../themes/nightfox.toml")),
    ("oxocarbon", include_str!("../themes/oxocarbon.toml")),
    ("vesper", include_str!("../themes/vesper.toml")),
    ("neon-nebula", include_str!("../themes/neon-nebula.toml")),
    ("cyberpunk", include_str!("../themes/cyberpunk.toml")),
    (
        "midnight-aurora",
        include_str!("../themes/midnight-aurora.toml"),
    ),
    ("sunset-glow", include_str!("../themes/sunset-glow.toml")),
    ("abyssal-deep", include_str!("../themes/abyssal-deep.toml")),
    (
        "cosmic-velvet",
        include_str!("../themes/cosmic-velvet.toml"),
    ),
    ("honey-amber", include_str!("../themes/honey-amber.toml")),
    (
        "emerald-forest",
        include_str!("../themes/emerald-forest.toml"),
    ),
    (
        "graphite-mist",
        include_str!("../themes/graphite-mist.toml"),
    ),
    (
        "crimson-ember",
        include_str!("../themes/crimson-ember.toml"),
    ),
    ("arctic-dawn", include_str!("../themes/arctic-dawn.toml")),
    ("matrix-rain", include_str!("../themes/matrix-rain.toml")),
    (
        "aurora-borealis",
        include_str!("../themes/aurora-borealis.toml"),
    ),
    ("desert-dune", include_str!("../themes/desert-dune.toml")),
    ("indigo-dream", include_str!("../themes/indigo-dream.toml")),
    ("ocean-glass", include_str!("../themes/ocean-glass.toml")),
    ("sakura-night", include_str!("../themes/sakura-night.toml")),
    ("mint-glass", include_str!("../themes/mint-glass.toml")),
    ("morning-mist", include_str!("../themes/morning-mist.toml")),
    ("peach-sorbet", include_str!("../themes/peach-sorbet.toml")),
    ("dawn", include_str!("../themes/dawn.toml")),
    ("ayu-light", include_str!("../themes/ayu-light.toml")),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
    ("github-light", include_str!("../themes/github-light.toml")),
    (
        "iceberg-light",
        include_str!("../themes/iceberg-light.toml"),
    ),
    ("light-owl", include_str!("../themes/light-owl.toml")),
    ("one-light", include_str!("../themes/one-light.toml")),
    (
        "tokyo-night-day",
        include_str!("../themes/tokyo-night-day.toml"),
    ),
    ("paper", include_str!("../themes/paper.toml")),
    ("alabaster", include_str!("../themes/alabaster.toml")),
    (
        "everforest-light",
        include_str!("../themes/everforest-light.toml"),
    ),
    (
        "flexoki-light",
        include_str!("../themes/flexoki-light.toml"),
    ),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.toml"),
    ),
    (
        "kanagawa-lotus",
        include_str!("../themes/kanagawa-lotus.toml"),
    ),
    (
        "rose-pine-dawn",
        include_str!("../themes/rose-pine-dawn.toml"),
    ),
    (
        "selenized-light",
        include_str!("../themes/selenized-light.toml"),
    ),
    (
        "solarized-light",
        include_str!("../themes/solarized-light.toml"),
    ),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The file name without `.toml`.
    pub id: String,
    pub name: String,
    /// One of [`GROUPS`]' ids; None for a theme of the user's.
    pub group: Option<String>,
    /// For UI accents (the active tab, focus rings).
    pub accent: Color,
    /// The terminal's default background: what programs see and what inverse
    /// text uses. Painted as is when there is no gradient.
    pub background: Color,
    /// Painted behind the cells instead of `background`.
    pub gradient: Option<Gradient>,
    pub foreground: Color,
    pub cursor: Color,
    /// The character under a block cursor.
    pub cursor_text: Color,
    pub selection_background: Color,
    /// None keeps each character's own color.
    pub selection_foreground: Option<Color>,
    /// ANSI colors 0–15: black, red, green, yellow, blue, magenta, cyan, white,
    /// then the bright variants.
    pub palette: [Color; 16],
}

/// A CSS-style linear gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// Degrees, as in CSS: 0 points up, 90 right, 135 to the bottom right.
    pub angle: f32,
    /// Colors and their positions in 0..=1, in increasing order.
    pub stops: Vec<(Color, f32)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: Option<String>,
    group: Option<String>,
    accent: Option<Color>,
    background: Color,
    gradient: Option<GradientFile>,
    foreground: Color,
    cursor: Option<Color>,
    cursor_text: Option<Color>,
    selection_background: Option<Color>,
    selection_foreground: Option<Color>,
    palette: Vec<Color>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GradientFile {
    #[serde(default = "default_angle")]
    angle: f32,
    /// `"#rrggbb"` or `"#rrggbb 40%"`.
    stops: Vec<String>,
}

fn default_angle() -> f32 {
    180.0
}

impl Theme {
    /// Parses a theme file; `id` is its file name without `.toml`.
    pub fn parse(id: &str, text: &str) -> Result<Theme, LoadError> {
        let file = format!("themes/{id}.toml");
        let f: ThemeFile = toml::from_str(text).map_err(|e| LoadError::toml(&file, text, &e))?;
        let invalid = |message: String| LoadError {
            file: file.clone(),
            line: None,
            message,
        };
        let palette: [Color; 16] = f.palette.try_into().map_err(|p: Vec<Color>| {
            invalid(format!("palette needs 16 colors, found {}", p.len()))
        })?;
        let gradient = f
            .gradient
            .map(|g| Gradient::parse(g.angle, &g.stops))
            .transpose()
            .map_err(invalid)?;
        Ok(Theme {
            id: id.to_string(),
            name: f.name.unwrap_or_else(|| id.to_string()),
            group: f.group,
            accent: f.accent.unwrap_or(palette[4]),
            background: f.background,
            gradient,
            foreground: f.foreground,
            cursor: f.cursor.unwrap_or(f.foreground),
            cursor_text: f.cursor_text.unwrap_or(f.background),
            selection_background: f.selection_background.unwrap_or(palette[8]),
            selection_foreground: f.selection_foreground,
            palette,
        })
    }
}

impl Gradient {
    fn parse(angle: f32, stops: &[String]) -> Result<Gradient, String> {
        if stops.len() < 2 {
            return Err("a gradient needs at least two stops".into());
        }
        let mut colors = Vec::with_capacity(stops.len());
        let mut positions = Vec::with_capacity(stops.len());
        for s in stops {
            let mut parts = s.split_whitespace();
            let color: Color = parts.next().unwrap_or_default().parse()?;
            let pos = match parts.next() {
                Some(p) => Some(
                    p.strip_suffix('%')
                        .and_then(|n| n.parse::<f32>().ok())
                        .ok_or_else(|| format!("`{p}` in `{s}` is not a percentage"))?
                        / 100.0,
                ),
                None => None,
            };
            colors.push(color);
            positions.push(pos);
        }
        // Stops without a position are spread evenly, as CSS does for the simple case.
        let n = (stops.len() - 1) as f32;
        let mut last = 0.0f32;
        let stops = colors
            .into_iter()
            .zip(positions)
            .enumerate()
            .map(|(i, (c, p))| {
                let at = p.unwrap_or(i as f32 / n).clamp(0.0, 1.0).max(last);
                last = at;
                (c, at)
            })
            .collect();
        Ok(Gradient {
            angle: angle.rem_euclid(360.0),
            stops,
        })
    }

    /// The color at `t` along the gradient line, `0.0..=1.0`, in 0–255 sRGB.
    pub fn at(&self, t: f32) -> [f32; 3] {
        let rgb = |c: Color| [c.r as f32, c.g as f32, c.b as f32];
        let (first, last) = (self.stops[0], self.stops[self.stops.len() - 1]);
        if t <= first.1 {
            return rgb(first.0);
        }
        for w in self.stops.windows(2) {
            let ((a, pa), (b, pb)) = (w[0], w[1]);
            if t <= pb {
                let f = if pb > pa { (t - pa) / (pb - pa) } else { 1.0 };
                let (a, b) = (rgb(a), rgb(b));
                return [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f);
            }
        }
        rgb(last.0)
    }

    /// BGRA pixels of a `w`×`h` box filled as CSS `linear-gradient` would, with
    /// light dithering against banding.
    pub fn render_bgra(&self, w: u32, h: u32) -> Vec<u8> {
        const BAYER: [f32; 16] = [
            0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
        ];
        let (wf, hf) = (w as f32, h as f32);
        let a = self.angle.to_radians();
        let (dx, dy) = (a.sin(), -a.cos());
        // CSS: the gradient line runs through the center, long enough that its
        // ends' perpendiculars touch the corners.
        let len = (wf * dx).abs() + (hf * dy).abs();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x as f32 + 0.5 - wf / 2.0, y as f32 + 0.5 - hf / 2.0);
                let t = (px * dx + py * dy) / len + 0.5;
                let c = self.at(t);
                let d = BAYER[((y % 4) * 4 + x % 4) as usize] / 16.0 - 0.5;
                let q = |v: f32| (v + d).round().clamp(0.0, 255.0) as u8;
                out.extend_from_slice(&[q(c[2]), q(c[1]), q(c[0]), 255]);
            }
        }
        out
    }
}

/// Built-in themes plus the user's, which replace a built-in with the same id.
#[derive(Clone, Debug)]
pub struct Themes {
    list: Vec<Arc<Theme>>,
}

impl Default for Themes {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Themes {
    pub fn builtin() -> Themes {
        Themes {
            list: BUILTIN
                .iter()
                .filter_map(|(id, text)| Theme::parse(id, text).ok())
                .map(Arc::new)
                .collect(),
        }
    }

    pub fn all(&self) -> &[Arc<Theme>] {
        &self.list
    }

    /// By id, else by name; both ignore case.
    pub fn find(&self, key: &str) -> Option<&Arc<Theme>> {
        self.list
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(key))
            .or_else(|| self.list.iter().find(|t| t.name.eq_ignore_ascii_case(key)))
    }

    /// The default theme (or the first, if a broken user file replaced it).
    pub fn fallback(&self) -> Arc<Theme> {
        self.find(DEFAULT_THEME)
            .or(self.list.first())
            .cloned()
            .unwrap_or_else(|| Arc::new(fallback_theme()))
    }

    /// The theme `step` places after `id` in the list, wrapping around.
    pub fn step(&self, id: &str, step: isize) -> Arc<Theme> {
        let n = self.list.len() as isize;
        if n == 0 {
            return self.fallback();
        }
        let i = self.list.iter().position(|t| t.id == id).unwrap_or(0) as isize;
        self.list[(i + step).rem_euclid(n) as usize].clone()
    }

    pub(crate) fn add(&mut self, theme: Theme) {
        let theme = Arc::new(theme);
        match self.list.iter_mut().find(|t| t.id == theme.id) {
            Some(slot) => *slot = theme,
            None => self.list.push(theme),
        }
    }
}

/// Only if every built-in failed to parse, which the tests rule out.
fn fallback_theme() -> Theme {
    let gray = Color::new(0x80, 0x80, 0x80);
    Theme {
        id: DEFAULT_THEME.into(),
        name: "Default".into(),
        group: None,
        accent: gray,
        background: Color::new(0, 0, 0),
        gradient: None,
        foreground: Color::new(0xdd, 0xdd, 0xdd),
        cursor: Color::new(0xdd, 0xdd, 0xdd),
        cursor_text: Color::new(0, 0, 0),
        selection_background: gray,
        selection_foreground: None,
        palette: [gray; 16],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses() {
        for (id, text) in BUILTIN {
            let t = Theme::parse(id, text).unwrap_or_else(|e| panic!("{id}: {e}"));
            if let Some(g) = &t.gradient {
                assert!(g.stops.len() >= 2, "{id}");
                assert!(g.stops.windows(2).all(|w| w[0].1 <= w[1].1), "{id}");
            }
        }
        let all = Themes::builtin();
        assert_eq!(all.all().len(), BUILTIN.len());
        assert_eq!(
            all.all().iter().filter(|t| t.gradient.is_some()).count(),
            20
        );
        assert_eq!(all.fallback().id, DEFAULT_THEME);
        // Each in a known group, and the list in the groups' order.
        let rank = |t: &Theme| {
            GROUPS
                .iter()
                .position(|(g, _)| Some(*g) == t.group.as_deref())
                .unwrap_or_else(|| panic!("{}: no known group", t.id))
        };
        assert!(all.all().windows(2).all(|w| rank(&w[0]) <= rank(&w[1])));
        // The light ones are light: the light mode lists them first.
        for t in all.all() {
            let light = t.group.as_deref().is_some_and(|g| g.contains("light"));
            let (r, g, b) = (t.background.r, t.background.g, t.background.b);
            let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
            assert_eq!(light, luma > 128, "{}", t.id);
        }
    }

    #[test]
    fn finds_by_id_or_name_and_steps_around() {
        let all = Themes::builtin();
        assert_eq!(
            all.find("NEON-NEBULA").map(|t| t.name.as_str()),
            Some("Neon Nebula")
        );
        assert_eq!(
            all.find("tokyo night").map(|t| t.id.as_str()),
            Some("tokyo-night")
        );
        assert_eq!(all.step("dusk", 1).id, "default-slate");
        assert_eq!(all.step("dusk", -1).id, "solarized-light");
        assert_eq!(all.step("solarized-light", 1).id, "dusk");
    }

    #[test]
    fn user_theme_replaces_a_builtin_with_the_same_id() {
        let mut all = Themes::builtin();
        let mut t = Theme::parse("nord", BUILTIN[16].1).expect("parse");
        t.name = "My Nord".into();
        all.add(t);
        assert_eq!(all.all().len(), BUILTIN.len());
        assert_eq!(all.find("nord").map(|t| t.name.as_str()), Some("My Nord"));
    }

    #[test]
    fn reports_bad_files() {
        let err = Theme::parse(
            "x",
            "background = \"#000\"\nforeground = \"#fff\"\npalette = []\n",
        )
        .expect_err("short palette");
        assert!(err.message.contains("16 colors"), "{err}");
        let err = Theme::parse("x", "background = \"#000\"\n\nforeground = \"white\"\n")
            .expect_err("bad color");
        assert_eq!(err.line, Some(3), "{err}");
    }

    #[test]
    fn stops_without_positions_spread_evenly() {
        let g = Gradient::parse(
            90.0,
            &["#000000".into(), "#808080".into(), "#ffffff".into()],
        )
        .expect("parse");
        let at: Vec<f32> = g.stops.iter().map(|s| s.1).collect();
        assert_eq!(at, [0.0, 0.5, 1.0]);
        assert_eq!(g.at(0.25), [64.0, 64.0, 64.0]);
    }

    #[test]
    fn renders_like_css() {
        // 135deg: the first stop in the top-left corner, the last in the bottom-right.
        let g =
            Gradient::parse(135.0, &["#ff0000 0%".into(), "#0000ff 100%".into()]).expect("parse");
        let (w, h) = (40u32, 20u32);
        let px = g.render_bgra(w, h);
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            (px[i + 2], px[i])
        };
        let (r, b) = at(0, 0);
        assert!(r > 240 && b < 15, "top-left {r} {b}");
        let (r, b) = at(w - 1, h - 1);
        assert!(r < 15 && b > 240, "bottom-right {r} {b}");
        // In a 2:1 box the top-right corner is two thirds along the line, as in CSS.
        let (r, b) = at(w - 1, 0);
        assert!(
            (75..=95).contains(&r) && (160..=180).contains(&b),
            "top-right {r} {b}"
        );
    }
}

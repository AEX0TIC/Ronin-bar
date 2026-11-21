//! The nine themes. A theme colours the bar and its panel, and sets Windows' dark/light mode and
//! accent colour.

/// An sRGB colour.
pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    /// Bar and panel background.
    pub bg: Rgb,
    /// Buttons, sliders, cards.
    pub surface: Rgb,
    /// Main text.
    pub fg: Rgb,
    /// Secondary text.
    pub muted: Rgb,
    /// Highlights; also Windows' accent colour.
    pub accent: Rgb,
    /// Windows dark mode on (true) or light mode (false).
    pub dark: bool,
}

pub const THEMES: [Theme; 9] = [
    Theme { id: "samurai", name: "Ronin", bg: [0x0a, 0x0a, 0x0c], surface: [0x17, 0x16, 0x1a], fg: [0xed, 0xe8, 0xdf], muted: [0x8f, 0x89, 0x7e], accent: [0xd1, 0x26, 0x3b], dark: true },
    Theme { id: "sakura", name: "Sakura Night", bg: [0x0f, 0x0c, 0x10], surface: [0x1d, 0x17, 0x1e], fg: [0xf5, 0xea, 0xf0], muted: [0xa3, 0x92, 0x9c], accent: [0xf2, 0x9b, 0xbd], dark: true },
    Theme { id: "neon-tokyo", name: "Neon Tokyo", bg: [0x07, 0x08, 0x0f], surface: [0x12, 0x14, 0x2a], fg: [0xe6, 0xe9, 0xff], muted: [0x8a, 0x91, 0xc2], accent: [0x27, 0xe1, 0xf2], dark: true },
    Theme { id: "midnight", name: "Midnight Gold", bg: [0x0a, 0x0d, 0x14], surface: [0x16, 0x1b, 0x27], fg: [0xe9, 0xe6, 0xde], muted: [0x8d, 0x91, 0x9e], accent: [0xcf, 0xa7, 0x5e], dark: true },
    Theme { id: "matcha", name: "Matcha Paper", bg: [0xf2, 0xef, 0xe6], surface: [0xe3, 0xdf, 0xd0], fg: [0x1c, 0x21, 0x17], muted: [0x5a, 0x61, 0x50], accent: [0x4b, 0x7a, 0x35], dark: false },
    // Added later; new themes always go at the end so saved choices keep their meaning.
    Theme { id: "ember", name: "Ember", bg: [0x0f, 0x0a, 0x08], surface: [0x1d, 0x15, 0x12], fg: [0xf3, 0xe9, 0xe1], muted: [0x9c, 0x8b, 0x80], accent: [0xff, 0x8a, 0x3d], dark: true },
    Theme { id: "oni", name: "Oni Violet", bg: [0x0c, 0x0a, 0x14], surface: [0x19, 0x15, 0x27], fg: [0xec, 0xe8, 0xf7], muted: [0x95, 0x8f, 0xb0], accent: [0xa7, 0x8b, 0xfa], dark: true },
    Theme { id: "jade", name: "Jade Temple", bg: [0x07, 0x11, 0x0e], surface: [0x12, 0x20, 0x1b], fg: [0xe3, 0xf1, 0xea], muted: [0x84, 0xa3, 0x96], accent: [0x3e, 0xcf, 0x8e], dark: true },
    Theme { id: "washi", name: "Washi", bg: [0xf6, 0xf1, 0xe7], surface: [0xe8, 0xe1, 0xd2], fg: [0x1f, 0x1a, 0x17], muted: [0x65, 0x5c, 0x52], accent: [0xb8, 0x32, 0x3a], dark: false },
];

/// The theme with this id, or the first one.
pub fn by_id(id: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

/// The next theme after `id`, wrapping around.
pub fn next(id: &str) -> &'static Theme {
    let i = THEMES.iter().position(|t| t.id == id).unwrap_or(THEMES.len() - 1);
    &THEMES[(i + 1) % THEMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance.
    fn lum(c: Rgb) -> f64 {
        let ch = |v: u8| { let s = v as f64 / 255.0; if s <= 0.039_28 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) } };
        0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
    }
    fn contrast(a: Rgb, b: Rgb) -> f64 { let (x, y) = (lum(a), lum(b)); (x.max(y) + 0.05) / (x.min(y) + 0.05) }

    #[test]
    fn nine_distinct_readable_themes() {
        assert_eq!(THEMES.len(), 9);
        for (i, t) in THEMES.iter().enumerate() {
            assert!(THEMES.iter().skip(i + 1).all(|o| o.id != t.id), "duplicate id {}", t.id);
            assert!(contrast(t.fg, t.bg) >= 7.0, "{}: text on bar must be easy to read", t.id);
            assert!(contrast(t.fg, t.surface) >= 4.5, "{}: text on buttons", t.id);
            assert!(contrast(t.muted, t.bg) >= 3.0, "{}: secondary text", t.id);
            assert_eq!(t.dark, lum(t.bg) < 0.2, "{}: dark flag matches the background", t.id);
        }
    }

    #[test]
    fn lookup_and_cycle() {
        assert_eq!(by_id("matcha").name, "Matcha Paper");
        assert_eq!(by_id("nope").id, "samurai");
        assert_eq!(next("samurai").id, "sakura");
        assert_eq!(next("matcha").id, "ember");
        assert_eq!(next("washi").id, "samurai");
        // Saved theme ids must keep pointing at the same theme (and its built-in wallpaper).
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(ids[..5], ["samurai", "sakura", "neon-tokyo", "midnight", "matcha"]);
        assert_eq!(next("unknown").id, "samurai");
    }
}

//! The five themes. A theme colours the bar and its panel, and sets Windows' dark/light mode and
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

pub const THEMES: [Theme; 5] = [
    Theme { id: "sakura", name: "Sakura", bg: [0x1d, 0x15, 0x1f], surface: [0x2c, 0x21, 0x2f], fg: [0xf6, 0xe3, 0xec], muted: [0xa8, 0x8f, 0xa0], accent: [0xff, 0x8f, 0xb8], dark: true },
    Theme { id: "samurai", name: "Samurai", bg: [0x12, 0x10, 0x10], surface: [0x22, 0x1c, 0x1c], fg: [0xf0, 0xe6, 0xdc], muted: [0x9c, 0x8c, 0x84], accent: [0xd6, 0x2b, 0x2b], dark: true },
    Theme { id: "neon-tokyo", name: "Neon Tokyo", bg: [0x0c, 0x0f, 0x1c], surface: [0x16, 0x1b, 0x30], fg: [0xe6, 0xf1, 0xff], muted: [0x86, 0x93, 0xb5], accent: [0x2e, 0xe6, 0xd6], dark: true },
    Theme { id: "matcha", name: "Matcha", bg: [0xf3, 0xf1, 0xe6], surface: [0xe2, 0xe4, 0xd2], fg: [0x26, 0x2b, 0x1f], muted: [0x6b, 0x72, 0x5c], accent: [0x5c, 0x8a, 0x3a], dark: false },
    Theme { id: "midnight", name: "Midnight", bg: [0x0f, 0x11, 0x1a], surface: [0x1b, 0x1f, 0x2e], fg: [0xdd, 0xe3, 0xf3], muted: [0x7d, 0x86, 0xa3], accent: [0x7c, 0x8c, 0xff], dark: true },
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
    fn five_distinct_readable_themes() {
        assert_eq!(THEMES.len(), 5);
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
        assert_eq!(by_id("matcha").name, "Matcha");
        assert_eq!(by_id("nope").id, "sakura");
        assert_eq!(next("sakura").id, "samurai");
        assert_eq!(next("midnight").id, "sakura");
        assert_eq!(next("unknown").id, "sakura");
    }
}

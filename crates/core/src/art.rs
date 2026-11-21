//! Ronin Bar's own wallpapers, drawn in code: one per theme, so the desktop looks right on first
//! start and offline. A sunset over layered mountains, with a lone ronin on the nearest ridge.
use crate::theme::{Rgb, Theme, THEMES};

/// Ids of the built-in wallpapers, one per theme, in theme order.
pub const ORIGINAL_IDS: [&str; 9] = ["ronin1", "ronin2", "ronin3", "ronin4", "ronin5", "ronin6", "ronin7", "ronin8", "ronin9"];

/// Where the sun stands (fraction of the width) in each theme's wallpaper.
const SUN_X: [f32; 9] = [0.70, 0.30, 0.64, 0.36, 0.68, 0.32, 0.66, 0.34, 0.72];
pub const ORIGINALS_NAME: &str = "Ronin originals";

/// The built-in wallpaper drawn in this theme's colours.
pub fn original_for(theme_id: &str) -> &'static str {
    ORIGINAL_IDS[THEMES.iter().position(|t| t.id == theme_id).unwrap_or(0)]
}

/// The theme a built-in wallpaper is drawn in.
pub fn original_theme(id: &str) -> Option<&'static Theme> {
    ORIGINAL_IDS.iter().position(|o| *o == id).map(|i| &THEMES[i])
}

fn mix(a: Rgb, b: Rgb, t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] as f32 + (b[i] as f32 - a[i] as f32) * t)
}

fn to8(c: [f32; 3]) -> Rgb { c.map(|v| v.round().clamp(0.0, 255.0) as u8) }

/// A deterministic hash for grain and ridge shapes.
fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Height (0 = top, 1 = bottom) of mountain ridge `layer` at horizontal position `x` (0..1).
fn ridge(x: f32, layer: usize, seed: u32) -> f32 {
    let base = [0.60, 0.70, 0.80][layer];
    let amp = [0.13, 0.10, 0.07][layer];
    let s = (seed as f32) * 0.37 + layer as f32 * 1.9;
    let wave = (x * 3.1 + s).sin() * 0.55 + (x * 7.3 + s * 2.1).sin() * 0.28 + (x * 17.0 + s * 0.7).sin() * 0.12
        + (x * 41.0 + s * 3.3).sin() * 0.05;
    base - amp * wave.abs().powf(0.9) - amp * 0.3
}

/// Draws the built-in wallpaper for `t` at `w`×`h`, as RGB bytes (row by row).
pub fn wallpaper(t: &Theme, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w.clamp(16, 8192), h.clamp(16, 8192));
    let seed = THEMES.iter().position(|o| o.id == t.id).unwrap_or(0) as u32 + 1;
    let (wf, hf) = (w as f32, h as f32);
    let black: Rgb = [0, 0, 0];
    let deep = if t.dark { to8(mix(t.bg, black, 0.55)) } else { t.fg };
    let sky_top = to8(mix(t.bg, t.accent, if t.dark { 0.06 } else { 0.04 }));
    let sky_low = to8(mix(t.bg, t.accent, if t.dark { 0.42 } else { 0.30 }));
    let sun_c = to8(mix(t.accent, [255, 255, 255], if t.dark { 0.18 } else { 0.05 }));
    let (sun_x, sun_y, sun_r) = (wf * SUN_X[(seed as usize - 1) % SUN_X.len()], hf * 0.50, hf * 0.16);
    // Mountains fade from misty (far) to ink (near).
    let layers = [to8(mix(sky_low, deep, 0.35)), to8(mix(sky_low, deep, 0.62)), to8(mix(sky_low, deep, 0.88))];
    let ridges: Vec<[f32; 3]> = (0..w).map(|x| {
        let xf = x as f32 / wf;
        [ridge(xf, 0, seed) * hf, ridge(xf, 1, seed) * hf, ridge(xf, 2, seed) * hf]
    }).collect();
    // The ronin stands on the nearest ridge, opposite the sun.
    let rx = if sun_x > wf * 0.5 { wf * 0.24 } else { wf * 0.76 };
    let ry = ridges[(rx as u32).min(w - 1) as usize][2];
    let u = hf / 1080.0 * 1.5;

    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        let yf = y as f32;
        let sky = mix(sky_top, sky_low, (yf / (hf * 0.75)).powf(1.4));
        for x in 0..w {
            let xf = x as f32;
            let d = ((xf - sun_x).powi(2) + (yf - sun_y).powi(2)).sqrt();
            let mut c = sky;
            // Glow, then the sun disc with a soft edge.
            let glow = (-(d / (sun_r * 2.6)).powi(2)).exp() * if t.dark { 0.35 } else { 0.22 };
            c = [0, 1, 2].map(|i| c[i] + (t.accent[i] as f32 - c[i]) * glow);
            let edge = ((sun_r - d) / 1.5).clamp(0.0, 1.0);
            c = [0, 1, 2].map(|i| c[i] + (sun_c[i] as f32 - c[i]) * edge);
            for (l, col) in layers.iter().enumerate() {
                let top = ridges[x as usize][l];
                let a = (yf - top + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    // Mist at the foot of each far ridge.
                    let mist = if l < 2 { ((yf - top) / (hf * 0.12)).clamp(0.0, 1.0) * 0.18 } else { 0.0 };
                    let m = mix(*col, sky_low, mist);
                    c = [0, 1, 2].map(|i| c[i] + (m[i] - c[i]) * a);
                }
            }
            // The ronin: straw hat, cloak, and a katana across the back.
            let (dx, dy) = ((xf - rx) / u, (yf - ry) / u);
            let hat = dy > -96.0 && dy < -84.0 && dx.abs() < 4.0 + (dy + 96.0) * 1.85;
            let head = (dx.powi(2) + (dy + 78.0).powi(2)).sqrt() < 8.0;
            let body = (-72.0..=0.0).contains(&dy) && dx.abs() < 9.0 + (dy + 72.0) * 0.17;
            let blade = {
                let (px, py) = (dx + 2.0, dy + 40.0);
                let along = (px - py) * std::f32::consts::FRAC_1_SQRT_2;
                let across = ((px + py) * std::f32::consts::FRAC_1_SQRT_2).abs();
                along > -30.0 && along < 44.0 && across < 1.6
            };
            if hat || head || body || blade {
                let ink = if t.dark { to8(mix(deep, black, 0.6)) } else { to8(mix(t.fg, black, 0.3)) };
                c = ink.map(|v| v as f32);
            }
            // Fine grain hides banding in the gradients.
            let g = (hash(x, y, seed) & 0xff) as f32 / 255.0 - 0.5;
            for v in &mut c { *v += g * 3.0; }
            out.extend_from_slice(&to8(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> Rgb {
        let i = ((y * w + x) * 3) as usize;
        [img[i], img[i + 1], img[i + 2]]
    }
    fn bright(c: Rgb) -> u32 { c.iter().map(|v| *v as u32).sum() }

    #[test]
    fn one_original_per_theme() {
        assert_eq!(ORIGINAL_IDS.len(), THEMES.len());
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(original_for(t.id), ORIGINAL_IDS[i]);
            assert_eq!(original_theme(ORIGINAL_IDS[i]).map(|o| o.id), Some(t.id));
            assert!(crate::wallhaven::is_id(ORIGINAL_IDS[i]));
        }
        assert_eq!(original_theme("abc123"), None);
    }

    #[test]
    fn draws_sky_sun_and_mountains() {
        let (w, h) = (320, 180);
        for t in THEMES.iter() {
            let img = wallpaper(t, w, h);
            assert_eq!(img.len(), (w * h * 3) as usize);
            assert_eq!(img, wallpaper(t, w, h), "the same every time");
            // The sun stands out from the sky above it; on dark themes the bottom is ink.
            let i = THEMES.iter().position(|o| o.id == t.id).unwrap();
            let (sx, sy) = ((w as f32 * SUN_X[i]) as u32, (h as f32 * 0.39) as u32); // upper part of the sun, above any ridge
            assert!(bright(px(&img, w, sx, sy)).abs_diff(bright(px(&img, w, sx, 2))) > 90, "{}", t.id);
            if t.dark { assert!(bright(px(&img, w, 5, h - 2)) < bright(px(&img, w, sx, sy)), "{}", t.id); }
        }
    }
}

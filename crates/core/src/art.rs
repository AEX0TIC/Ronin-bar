//! Ronin Bar's own wallpapers, drawn in code: one per theme, so the desktop looks right on first
//! start and offline. A sunset over layered mountains: stars, cloud bands across the sun, a torii
//! gate against it, birds, and a lone ronin on the nearest ridge.
use crate::theme::{Rgb, Theme, THEMES};

/// Ids of the built-in wallpapers, one per theme, in theme order.
pub const ORIGINAL_IDS: [&str; 9] = ["ronin1", "ronin2", "ronin3", "ronin4", "ronin5", "ronin6", "ronin7", "ronin8", "ronin9"];

/// Bumped whenever the drawing changes, so wallpapers drawn by an older version are redrawn.
pub const ART_VERSION: u32 = 2;

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

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy, wx, wy) = (b.0 - a.0, b.1 - a.1, p.0 - a.0, p.1 - a.1);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy).max(1e-6)).clamp(0.0, 1.0);
    ((wx - vx * t).powi(2) + (wy - vy * t).powi(2)).sqrt()
}

/// A torii gate standing on y = 0 (units: 1 = 1/1080 of the screen height), x centred.
fn torii(dx: f32, dy: f32) -> bool {
    let post = (dx.abs() - 40.0).abs() < 4.5 && (-112.0..=0.0).contains(&dy);
    let kasagi = dx.abs() < 66.0 && dy < -110.0 && dy > -124.0 - (dx / 66.0).powi(2) * 7.0;
    let nuki = dx.abs() < 52.0 && (-96.0..=-88.0).contains(&dy);
    let tsuka = dx.abs() < 3.5 && (-110.0..=-96.0).contains(&dy);
    post || kasagi || nuki || tsuka
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
    let v = hf / 1080.0;
    // A torii on the far ridge, just beside the sun's centre, so it stands against the disc.
    let tx = sun_x + sun_r * 0.35;
    let ty = ridges[(tx as u32).min(w - 1) as usize][0] + 2.0 * v;
    let torii_c = to8(mix(layers[0], deep, 0.35));
    // Cloud bands across the upper half of the sun (the lower half is behind the ridges): (y offset, left, right) in sun radii.
    let bands = [(-0.55f32, -1.6f32, 0.7f32), (-0.30, -0.6, 1.9), (-0.08, -2.0, 1.0)];
    let band_c = mix(sky_low, t.bg, 0.25);
    // A few birds above the sun.
    let birds: Vec<(f32, f32, f32)> = (0..5).map(|i| {
        let h1 = hash(i, 7, seed);
        (sun_x + (((h1 & 0xff) as f32 / 255.0) - 0.6) * sun_r * 3.2, sun_y - sun_r * (1.25 + ((h1 >> 8) & 0xff) as f32 / 255.0 * 0.9), 0.6 + ((h1 >> 16) & 0xff) as f32 / 255.0 * 0.6)
    }).collect();
    let petals = matches!(t.id, "sakura" | "washi");
    let petal_c = mix(t.accent, [255, 255, 255], 0.35);
    let ink = if t.dark { to8(mix(deep, black, 0.6)) } else { to8(mix(t.fg, black, 0.3)) };
    let star_cell = (hf / 540.0).max(1.0) as u32;

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
            // Stars in the upper sky of dark themes, fading towards the horizon and the glow.
            if t.dark && yf < hf * 0.55 {
                let hs = hash(x / star_cell, y / star_cell, seed + 77);
                if hs.is_multiple_of(997) {
                    let a = (1.0 - yf / (hf * 0.55)) * (1.0 - glow * 2.5).max(0.0) * (0.35 + ((hs >> 12) & 0xff) as f32 / 255.0 * 0.65);
                    c = [0, 1, 2].map(|i| c[i] + (255.0 - c[i]) * a);
                }
            }
            c = [0, 1, 2].map(|i| c[i] + (t.accent[i] as f32 - c[i]) * glow);
            let edge = ((sun_r - d) / 1.5).clamp(0.0, 1.0);
            c = [0, 1, 2].map(|i| c[i] + (sun_c[i] as f32 - c[i]) * edge);
            // Thin cloud bands drifting across the sun.
            for (off, l, r) in bands {
                let by = sun_y + off * sun_r;
                let half = sun_r * 0.045 + v;
                if (yf - by).abs() < half && xf > sun_x + l * sun_r && xf < sun_x + r * sun_r {
                    let ends = ((xf - (sun_x + l * sun_r)).min(sun_x + r * sun_r - xf) / (sun_r * 0.25)).clamp(0.0, 1.0);
                    let a = ends * (1.0 - ((yf - by).abs() / half).powi(2)).clamp(0.0, 1.0);
                    c = [0, 1, 2].map(|i| c[i] + (band_c[i] - c[i]) * a);
                }
            }
            // Birds: small open "v"s.
            for &(bx, by, sz) in &birds {
                let (px, py) = ((xf - bx) / v, (yf - by) / v);
                if px.abs() < 20.0 * sz && py.abs() < 10.0 * sz {
                    let dd = seg_dist((px, py), (-14.0 * sz, -5.0 * sz), (0.0, 2.0 * sz)).min(seg_dist((px, py), (0.0, 2.0 * sz), (14.0 * sz, -5.0 * sz)));
                    let a = (1.6 - dd).clamp(0.0, 1.0);
                    c = [0, 1, 2].map(|i| c[i] + (ink[i] as f32 - c[i]) * a * 0.85);
                }
            }
            for (l, col) in layers.iter().enumerate() {
                let top = ridges[x as usize][l];
                let a = (yf - top + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    // Mist at the foot of each far ridge.
                    let mist = if l < 2 { ((yf - top) / (hf * 0.12)).clamp(0.0, 1.0) * 0.18 } else { 0.0 };
                    let m = mix(*col, sky_low, mist);
                    c = [0, 1, 2].map(|i| c[i] + (m[i] - c[i]) * a);
                }
                // The torii stands on the far ridge, in front of the sun but behind nearer ridges.
                if l == 0 && torii((xf - tx) / v, (yf - ty) / v) { c = torii_c.map(|q| q as f32); }
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
            if hat || head || body || blade { c = ink.map(|q| q as f32); }
            // Cherry petals drifting in front of everything (Sakura and Washi).
            if petals {
                let cell = 46.0 * v;
                let (cx, cy) = ((xf / cell) as u32, (yf / cell) as u32);
                let hp = hash(cx, cy, seed + 31);
                if hp.is_multiple_of(9) {
                    let (ox, oy) = ((cx as f32 + 0.5) * cell, (cy as f32 + 0.5) * cell);
                    let ang = ((hp >> 8) & 0xff) as f32 / 255.0 * std::f32::consts::PI;
                    let (ddx, ddy) = (xf - ox, yf - oy);
                    let (ax, ay) = (ddx * ang.cos() + ddy * ang.sin(), -ddx * ang.sin() + ddy * ang.cos());
                    let r = (ax / (4.2 * v)).powi(2) + (ay / (2.4 * v)).powi(2);
                    if r < 1.0 { c = [0, 1, 2].map(|i| c[i] + (petal_c[i] - c[i]) * (1.0 - r).sqrt() * 0.9); }
                }
            }
            // A soft vignette.
            let (nx, ny) = (xf / wf - 0.5, yf / hf - 0.5);
            let vig = 1.0 - 0.22 * (nx * nx + ny * ny) * 2.0;
            for q in &mut c { *q *= vig; }
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

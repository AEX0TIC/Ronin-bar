//! Ronin's night light: a warmer screen in the evening, made by the colour ramp the graphics card
//! uses to turn each red, green and blue level into light. Nothing here talks to Windows; the app
//! hands the ramp to the screen.

/// How strong the night light is when first turned on (percent).
pub const DEFAULT_STRENGTH: u8 = 50;

/// How much green and blue are kept at full strength: about 3400 K, the warmest Windows accepts
/// without a registry change.
const MIN_GREEN: f32 = 0.80;
const MIN_BLUE: f32 = 0.60;

/// The red, green and blue scale for a strength of 0–100 % (0 = no change).
pub fn scale(strength: u8) -> (f32, f32, f32) {
    let s = f32::from(strength.min(100)) / 100.0;
    (1.0, 1.0 - (1.0 - MIN_GREEN) * s, 1.0 - (1.0 - MIN_BLUE) * s)
}

/// The 3 × 256 ramp (red, then green, then blue) for a strength; strength 0 is the plain screen.
pub fn ramp(strength: u8) -> [[u16; 256]; 3] {
    let (r, g, b) = scale(strength);
    let mut out = [[0u16; 256]; 3];
    for (k, f) in [r, g, b].into_iter().enumerate() {
        for (i, v) in out[k].iter_mut().enumerate() {
            *v = ((i as f32 * 257.0) * f).round().clamp(0.0, 65535.0) as u16;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_the_plain_screen() {
        for ch in ramp(0) {
            for (i, v) in ch.iter().enumerate() { assert_eq!(*v as usize, i * 257); }
        }
    }

    #[test]
    fn warmer_means_less_blue_than_green_than_red() {
        let r = ramp(100);
        assert_eq!(r[0][255], 65535);
        assert!(r[1][255] < r[0][255] && r[2][255] < r[1][255]);
        assert!(r[2][255] >= (65535.0 * MIN_BLUE) as u16 - 1);
        // Each channel still rises from dark to bright.
        for ch in &r { assert!(ch.windows(2).all(|w| w[0] <= w[1])); }
        // Over 100 % is treated as 100 %.
        assert_eq!(ramp(250), r);
    }
}

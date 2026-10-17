//! Colour tables for the LUT correction modes (Perceptual, Saturation, Relative and Absolute
//! colorimetric).
//!
//! # Licence
//!
//! `magir2x.lut.z` is the MAGIR2X table of the Magicard Linux CUPS driver (`rastertoultra`,
//! GPL-2.0-or-later, (c) Magicard Ltd), re-packed by `tools/gen-lut.py`. The interpolation in
//! [`correct`] follows the driver's `ColourCorrectHelix` routine (one corner lookup plus a linear
//! step towards the opposite grid corner, per ink). Because of that, **this module and every
//! binary that contains it are GPL-2.0-or-later work**. See `LICENSE-GPL-NOTES.md`.
//!
//! The driver applies exactly this table to the Pro360-board family (Magicard 300, 300NEO, 360,
//! Pronto, ...) when the PPD option `ColourCorrection` is Perceptual..Absolute, so the preview
//! shows what the driver will do.

use std::sync::OnceLock;

/// Grid points per axis (grid step 4 over 0..=256).
const N: usize = 65;
const STEP: i32 = 4;
const BLOB: &[u8] = include_bytes!("magir2x.lut.z");

/// The four tables stored in the driver, in the driver's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Perceptual = 0,
    Saturation = 1,
    RelativeColorimetric = 2,
    AbsoluteColorimetric = 3,
}

/// `[r][g][b]` -> 12 bytes: cyan x4 intents, magenta x4, yellow x4.
fn table() -> &'static [u8] {
    static T: OnceLock<Vec<u8>> = OnceLock::new();
    T.get_or_init(|| {
        let n3 = N * N * N;
        let planes = miniz_oxide::inflate::decompress_to_vec_zlib(BLOB).expect("corrupt colour table blob");
        assert_eq!(planes.len(), n3 * 12, "colour table has the wrong size");
        let mut out = vec![0u8; n3 * 12];
        for p in 0..12 {
            let plane = &planes[p * n3..(p + 1) * n3];
            for rg in 0..N * N {
                let mut acc = 0u8; // undo the delta along the blue axis
                for b in 0..N {
                    acc = acc.wrapping_add(plane[rg * N + b]);
                    out[(rg * N + b) * 12 + p] = acc;
                }
            }
        }
        out
    })
}

#[inline]
fn entry(t: &[u8], r: usize, g: usize, b: usize, ink: usize, intent: usize) -> i32 {
    t[((r * N + g) * N + b) * 12 + ink * 4 + intent] as i32
}

/// Rounds half away from zero (the driver's `adj` rounding).
#[inline]
fn round_away(x: f64) -> i32 {
    if x < 0.0 {
        (x - 0.5).ceil() as i32
    } else {
        (x + 0.5).floor() as i32
    }
}

/// Corrects one RGB pixel the way the driver does and returns the RGB that corresponds to the
/// ink amounts it would print (`255 - C`, `255 - M`, `255 - Y`). Pure black `0,0,0` is left alone:
/// the driver prints it on the resin panel instead of correcting it.
pub fn correct(rgb: [u8; 3], intent: Intent) -> [u8; 3] {
    if rgb == [0, 0, 0] {
        return rgb;
    }
    let t = table();
    let it = intent as usize;
    let lo = [rgb[0] as i32 / STEP, rgb[1] as i32 / STEP, rgb[2] as i32 / STEP];
    let off = [rgb[0] as i32 - lo[0] * STEP, rgb[1] as i32 - lo[1] * STEP, rgb[2] as i32 - lo[2] * STEP];
    let (l0, l1, l2) = (lo[0] as usize, lo[1] as usize, lo[2] as usize);
    let (u0, u1, u2) = (l0 + 1, l1 + 1, l2 + 1);
    let mut out = [0u8; 3];
    for ink in 0..3 {
        let base = entry(t, l0, l1, l2, ink, it);
        let hi = entry(t, u0, u1, u2, ink, it);
        // Same ink as channel: cyan follows red, magenta green, yellow blue.
        let adj = round_away(off[ink] as f64 * (hi - base) as f64 / STEP as f64);
        let amount = ((base + adj) & 0xff) as u8;
        out[ink] = 255 - amount;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_decodes_to_expected_size_and_known_entries() {
        let t = table();
        assert_eq!(t.len(), N * N * N * 12);
        // Black corner and white corner of the Perceptual/Absolute tables (values from magir2x.c).
        assert_eq!(entry(t, 0, 0, 0, 0, 0), 221);
        assert_eq!(entry(t, 0, 0, 0, 2, 3), 255);
        assert_eq!(entry(t, 64, 64, 64, 0, 0), 10);
    }

    #[test]
    fn white_becomes_slightly_off_white_and_black_stays() {
        assert_eq!(correct([0, 0, 0], Intent::AbsoluteColorimetric), [0, 0, 0]);
        let w = correct([255, 255, 255], Intent::Perceptual);
        assert!(w.iter().all(|&c| (234..=246).contains(&c)), "{w:?}");
        assert!(w[2] < w[1], "blue lower than green (warm/greenish grey): {w:?}");
    }
}

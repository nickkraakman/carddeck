//! Colour correction for the **on-screen preview** (Hub-style "Colour Correct" + adjustment sliders).
//!
//! Pipeline, in the order Hub applies it to the colour layer:
//! 1. per-channel multipliers (`red`, `green`, `blue`, 0.5..1.5)
//! 2. brightness (-0.5..0.5, added as `brightness * 255`)
//! 3. contrast (-15..15, factor `((contrast + 100) / 100)^2` around mid grey)
//! 4. saturation (-5..5, chroma scaled by `2^saturation`, luma kept)
//! 5. the colour correction mode
//!    * `None`: nothing
//!    * `Gamma1` / `Gamma2`: a per-channel tone curve. Hub gets these from a Windows GDI call inside its
//!      closed `colour-correction.dll`; the curves here are **fitted** to screenshots of Hub (see
//!      `docs/COLOUR-CORRECTION.md`), not exact.
//!    * `Perceptual` / `Saturation` / `RelativeColorimetric` / `AbsoluteColorimetric`: the Magicard driver's
//!      own MAGIR2X colour table ([`lut`], GPL)
//!
//! None of this touches the bitmap that is printed (the Magicard driver does the LUT modes itself;
//! see [`Correction::ppd_choice`]), unless [`crate::print::PrintOptions::bake_non_driver_adjustments`]
//! is switched on for the parts the driver cannot do.

pub mod lut;
pub mod store;

use image::{RgbImage, RgbaImage};
use serde_json::Value;

/// Hub's "Colour Correct" choices. The JSON names are what Hub writes to `adjustments.correction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Correction {
    None,
    Gamma1,
    Gamma2,
    Perceptual,
    Saturation,
    RelativeColorimetric,
    AbsoluteColorimetric,
}

impl Correction {
    /// In the order of Hub's drop-down.
    pub const ALL: [Correction; 7] = [
        Correction::None,
        Correction::Gamma1,
        Correction::Gamma2,
        Correction::AbsoluteColorimetric,
        Correction::RelativeColorimetric,
        Correction::Perceptual,
        Correction::Saturation,
    ];

    /// Name used in the `.card` file (`adjustments.correction`).
    pub fn json_name(self) -> &'static str {
        match self {
            Correction::None => "None",
            Correction::Gamma1 => "Gamma1",
            Correction::Gamma2 => "Gamma2",
            Correction::Perceptual => "Perceptual",
            Correction::Saturation => "Saturation",
            Correction::RelativeColorimetric => "RelativeColorimetric",
            Correction::AbsoluteColorimetric => "AbsoluteColorimetric",
        }
    }

    /// Text shown in the UI drop-down (same wording as Hub).
    pub fn label(self) -> &'static str {
        match self {
            Correction::None => "None",
            Correction::Gamma1 => "Gamma 1",
            Correction::Gamma2 => "Gamma 2",
            Correction::Perceptual => "Perceptual",
            Correction::Saturation => "Saturation",
            Correction::RelativeColorimetric => "Relative Colorimetric",
            Correction::AbsoluteColorimetric => "Absolute Colorimetric",
        }
    }

    /// Parses Hub's names (case-insensitive), including the legacy values Hub's own file converter maps.
    pub fn from_json_name(s: &str) -> Option<Correction> {
        // Tolerate display labels ("Gamma 1", "Relative Colorimetric") as well as Hub's enum names.
        let key: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        Some(match key.as_str() {
            "none" => Correction::None,
            "gamma1" => Correction::Gamma1,
            "gamma2" => Correction::Gamma2,
            "perceptual" => Correction::Perceptual,
            "saturation" => Correction::Saturation,
            "relativecolorimetric" | "relcolorimetric" => Correction::RelativeColorimetric,
            "absolutecolorimetric" | "abscolorimetric" => Correction::AbsoluteColorimetric,
            // Old Hub files (<= 1.0.8): the converter turns these into Gamma1.
            "icc_internal" | "icc_external" | "iccinternal" | "iccexternal" => Correction::Gamma1,
            _ => return None,
        })
    }

    /// The Magicard driver (CUPS) choice for the PPD option `ColourCorrection` that matches this
    /// mode, e.g. `5ColourCorrection`. Names/values from `pro300.ppd`, `pro300neo.ppd`, `pro360*.ppd`
    /// (all Pro360-board PPDs agree):
    ///
    /// | choice | text in PPD |
    /// |---|---|
    /// | `0ColourCorrection` | None |
    /// | `1ColourCorrection` | Driver managed (the driver's legacy internal table, `ICC_Internal`; Hub has no entry for it) |
    /// | `2ColourCorrection` | Perceptual |
    /// | `3ColourCorrection` | Saturation |
    /// | `4ColourCorrection` | Relative colorimetric |
    /// | `5ColourCorrection` | Absolute colorimetric (the PPD default) |
    ///
    /// `Gamma1`/`Gamma2` have **no exact driver equivalent**. They map to `1ColourCorrection`
    /// (Driver managed): that is the driver's own internal correction, and Hub's file converter turns
    /// the old `ICC_Internal` value into `Gamma1`, so it is the closest analogue. It is *not* the same
    /// curve as the Gamma preview ([`Correction::has_driver_equivalent`] is false). When the Gamma
    /// is baked into the print bitmap instead, [`Correction::ppd_choice_when_baked`] sends None.
    pub fn ppd_choice(self) -> &'static str {
        match self {
            Correction::None => "0ColourCorrection",
            Correction::Gamma1 | Correction::Gamma2 => "1ColourCorrection",
            Correction::Perceptual => "2ColourCorrection",
            Correction::Saturation => "3ColourCorrection",
            Correction::RelativeColorimetric => "4ColourCorrection",
            Correction::AbsoluteColorimetric => "5ColourCorrection",
        }
    }

    /// The choice to send when Gamma 1/2 have already been applied to the print bitmap (opt-in bake):
    /// the driver must then do nothing, otherwise the correction would be applied twice.
    pub fn ppd_choice_when_baked(self) -> &'static str {
        match self {
            Correction::Gamma1 | Correction::Gamma2 => "0ColourCorrection",
            other => other.ppd_choice(),
        }
    }

    /// True if the driver can do this correction itself (None and the four LUT modes).
    pub fn has_driver_equivalent(self) -> bool {
        !matches!(self, Correction::Gamma1 | Correction::Gamma2)
    }

    fn intent(self) -> Option<lut::Intent> {
        match self {
            Correction::Perceptual => Some(lut::Intent::Perceptual),
            Correction::Saturation => Some(lut::Intent::Saturation),
            Correction::RelativeColorimetric => Some(lut::Intent::RelativeColorimetric),
            Correction::AbsoluteColorimetric => Some(lut::Intent::AbsoluteColorimetric),
            _ => None,
        }
    }
}

/// Slider limits, same as Hub's Print page.
pub const BRIGHTNESS_RANGE: (f64, f64) = (-0.5, 0.5);
pub const CONTRAST_RANGE: (f64, f64) = (-15.0, 15.0);
pub const SATURATION_RANGE: (f64, f64) = (-5.0, 5.0);
pub const RGB_RANGE: (f64, f64) = (0.5, 1.5);

/// The `adjustments` block of a `.card` file. JSON keys are Hub's:
/// `correction, brightness, contrast, saturation, red, green, blue`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustments {
    pub correction: Correction,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

impl Default for Adjustments {
    /// What Hub uses for a file without an `adjustments` block: Gamma 1, sliders at neutral.
    fn default() -> Self {
        Adjustments { correction: Correction::Gamma1, brightness: 0.0, contrast: 0.0, saturation: 0.0, red: 1.0, green: 1.0, blue: 1.0 }
    }
}

impl Adjustments {
    /// Sliders neutral and correction `None`: applying this changes nothing.
    pub fn identity() -> Self {
        Adjustments { correction: Correction::None, ..Default::default() }
    }

    /// True if the six sliders are at their neutral positions.
    pub fn sliders_neutral(&self) -> bool {
        self.brightness == 0.0 && self.contrast == 0.0 && self.saturation == 0.0 && self.red == 1.0 && self.green == 1.0 && self.blue == 1.0
    }

    /// Reads the `adjustments` object leniently (case-insensitive keys, numbers or numeric strings,
    /// anything missing or broken falls back to Hub's default). Values are clamped to the slider ranges.
    pub fn from_json(v: Option<&Value>) -> Adjustments {
        let mut a = Adjustments::default();
        let Some(o) = v.and_then(|v| v.as_object()) else { return a };
        let get = |name: &str| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v);
        let num = |name: &str| -> Option<f64> {
            match get(name)? {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            }
            .filter(|x| x.is_finite())
        };
        if let Some(c) = get("correction").and_then(|v| v.as_str()).and_then(Correction::from_json_name) {
            a.correction = c;
        }
        if let Some(x) = num("brightness") {
            a.brightness = x;
        }
        if let Some(x) = num("contrast") {
            a.contrast = x;
        }
        if let Some(x) = num("saturation") {
            a.saturation = x;
        }
        if let Some(x) = num("red") {
            a.red = x;
        }
        if let Some(x) = num("green") {
            a.green = x;
        }
        if let Some(x) = num("blue") {
            a.blue = x;
        }
        a.clamped()
    }

    /// Clamps every slider to Hub's range and rounds to 6 decimals (what gets written to the file).
    pub fn clamped(mut self) -> Adjustments {
        let f = |x: f64, r: (f64, f64)| (x.clamp(r.0, r.1) * 1e6).round() / 1e6;
        self.brightness = f(self.brightness, BRIGHTNESS_RANGE);
        self.contrast = f(self.contrast, CONTRAST_RANGE);
        self.saturation = f(self.saturation, SATURATION_RANGE);
        self.red = f(self.red, RGB_RANGE);
        self.green = f(self.green, RGB_RANGE);
        self.blue = f(self.blue, RGB_RANGE);
        self
    }
}

// ---------------------------------------------------------------------------------------------
// pixel pipeline
// ---------------------------------------------------------------------------------------------

/// Tone curve `255 * clamp(c * (x/255)^g + b)` for the two Gamma modes.
///
/// FITTED, not exact: parameters come from least-squares fits to Hub preview screenshots of the
/// 'Soft Summer' card (30 swatches x 3 channels, residual <= 2/255; see docs/COLOUR-CORRECTION.md).
fn gamma_params(c: Correction) -> Option<(f64, f64, f64)> {
    match c {
        Correction::Gamma1 => Some((1.5231, 1.0049, -0.0053)),
        Correction::Gamma2 => Some((1.4958, 1.0578, -0.0569)),
        _ => None,
    }
}

fn gamma_table(c: Correction) -> Option<[u8; 256]> {
    let (g, k, b) = gamma_params(c)?;
    let mut t = [0u8; 256];
    for (i, o) in t.iter_mut().enumerate() {
        let x = i as f64 / 255.0;
        *o = (255.0 * (k * x.powf(g) + b).clamp(0.0, 1.0)).round() as u8;
    }
    Some(t)
}

#[inline]
fn q(v: f64) -> f64 {
    // Hub's filters work on 8-bit canvas data: every stage clamps and rounds.
    v.clamp(0.0, 255.0).round()
}

/// Which stages [`apply_to_buffer`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stages {
    /// Sliders + the selected correction (what the preview shows).
    Preview,
    /// Sliders + Gamma 1/2 only; the LUT modes are left to the driver. Used by the opt-in print bake.
    NonDriver,
}

/// Applies the pipeline in place to an interleaved 8-bit buffer (`stride` 3 for RGB, 4 for RGBA;
/// alpha is not touched). Pixels with alpha 0 are skipped.
pub fn apply_to_buffer(buf: &mut [u8], stride: usize, adj: &Adjustments, stages: Stages) {
    debug_assert!(stride == 3 || stride == 4);
    let adj = adj.clamped();
    let sliders = !adj.sliders_neutral();
    let gamma = gamma_table(adj.correction);
    let intent = match stages {
        Stages::Preview => adj.correction.intent(),
        Stages::NonDriver => None,
    };
    if !sliders && gamma.is_none() && intent.is_none() {
        return;
    }
    let mul = [adj.red, adj.green, adj.blue];
    let contrast = ((adj.contrast + 100.0) / 100.0).powi(2);
    let sat = 2f64.powf(adj.saturation);
    let bright = adj.brightness * 255.0;
    for px in buf.chunks_exact_mut(stride) {
        if stride == 4 && px[3] == 0 {
            continue;
        }
        if sliders {
            let mut v = [px[0] as f64, px[1] as f64, px[2] as f64];
            for i in 0..3 {
                if mul[i] != 1.0 {
                    v[i] = q(v[i] * mul[i]);
                }
            }
            if bright != 0.0 {
                for x in &mut v {
                    *x = q(*x + bright);
                }
            }
            if adj.contrast != 0.0 {
                for x in &mut v {
                    *x = q(((*x / 255.0 - 0.5) * contrast + 0.5) * 255.0);
                }
            }
            if adj.saturation != 0.0 {
                let l = 0.299 * v[0] + 0.587 * v[1] + 0.114 * v[2];
                for x in &mut v {
                    *x = q(l + sat * (*x - l));
                }
            }
            px[0] = v[0] as u8;
            px[1] = v[1] as u8;
            px[2] = v[2] as u8;
        }
        if let Some(t) = &gamma {
            px[0] = t[px[0] as usize];
            px[1] = t[px[1] as usize];
            px[2] = t[px[2] as usize];
        } else if let Some(i) = intent {
            let o = lut::correct([px[0], px[1], px[2]], i);
            px[0] = o[0];
            px[1] = o[1];
            px[2] = o[2];
        }
    }
}

/// Preview correction of an RGB image (sliders + selected mode).
pub fn apply_rgb(img: &mut RgbImage, adj: &Adjustments) {
    apply_to_buffer(img.as_mut(), 3, adj, Stages::Preview);
}

/// Preview correction of an RGBA image (transparent card corners stay transparent).
pub fn apply_rgba(img: &mut RgbaImage, adj: &Adjustments) {
    apply_to_buffer(img.as_mut(), 4, adj, Stages::Preview);
}

/// Returns the preview version of one pixel (sliders + mode); handy for tests and tools.
pub fn correct_pixel(rgb: [u8; 3], adj: &Adjustments) -> [u8; 3] {
    let mut b = rgb;
    apply_to_buffer(&mut b, 3, adj, Stages::Preview);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_legacy_values_map() {
        for c in Correction::ALL {
            assert_eq!(Correction::from_json_name(c.json_name()), Some(c));
        }
        assert_eq!(Correction::from_json_name("ICC_External"), Some(Correction::Gamma1));
        // display labels (as the first print-UI commit stored them) are still understood
        assert_eq!(Correction::from_json_name("Gamma 2"), Some(Correction::Gamma2));
        assert_eq!(Correction::from_json_name("Relative Colorimetric"), Some(Correction::RelativeColorimetric));
        assert_eq!(Correction::from_json_name("RelColorimetric"), Some(Correction::RelativeColorimetric));
        assert_eq!(Correction::from_json_name("AbsColorimetric"), Some(Correction::AbsoluteColorimetric));
        assert_eq!(Correction::from_json_name("nonsense"), None);
    }

    #[test]
    fn missing_block_means_hub_default() {
        let a = Adjustments::from_json(None);
        assert_eq!(a, Adjustments::default());
        assert_eq!(a.correction, Correction::Gamma1);
    }

    #[test]
    fn parse_is_lenient_and_clamped() {
        let v: Value = serde_json::from_str(r#"{"Correction":"Perceptual","brightness":"0.25","contrast":99,"red":1.061062,"blue":null}"#).unwrap();
        let a = Adjustments::from_json(Some(&v));
        assert_eq!(a.correction, Correction::Perceptual);
        assert_eq!(a.brightness, 0.25);
        assert_eq!(a.contrast, 15.0);
        assert_eq!(a.red, 1.061062);
        assert_eq!(a.blue, 1.0);
    }

    #[test]
    fn identity_changes_nothing() {
        let mut px = [12u8, 200, 77, 255, 255, 255];
        let orig = px;
        apply_to_buffer(&mut px, 3, &Adjustments::identity(), Stages::Preview);
        assert_eq!(px, orig);
    }

    #[test]
    fn sliders_behave() {
        let id = Adjustments::identity();
        // red multiplier
        let a = Adjustments { red: 1.5, ..id };
        assert_eq!(correct_pixel([100, 100, 100], &a), [150, 100, 100]);
        // brightness +0.2 adds 51 and clamps
        let a = Adjustments { brightness: 0.2, ..id };
        assert_eq!(correct_pixel([100, 220, 0], &a), [151, 255, 51]);
        // contrast keeps mid grey roughly, pushes others apart
        let a = Adjustments { contrast: 10.0, ..id };
        let m = correct_pixel([128, 128, 128], &a);
        assert!((m[0] as i32 - 128).abs() <= 1);
        let d = correct_pixel([64, 64, 64], &a);
        assert!(d[0] < 64);
        // saturation -5 -> almost grey, +ve keeps luma
        let a = Adjustments { saturation: -5.0, ..id };
        let g = correct_pixel([200, 50, 50], &a);
        assert!((g[0] as i32 - g[1] as i32).abs() <= 6, "{g:?}");
        let a = Adjustments { saturation: 1.0, ..id };
        let s = correct_pixel([200, 100, 100], &a);
        assert!(s[0] > 200 && s[1] < 100);
        // grey stays grey under any saturation
        assert_eq!(correct_pixel([90, 90, 90], &a), [90, 90, 90]);
    }

    #[test]
    fn gamma_modes_darken_but_keep_white_and_black() {
        for c in [Correction::Gamma1, Correction::Gamma2] {
            let a = Adjustments { correction: c, ..Adjustments::identity() };
            assert_eq!(correct_pixel([255, 255, 255], &a), [255, 255, 255]);
            assert_eq!(correct_pixel([0, 0, 0], &a), [0, 0, 0]);
            let m = correct_pixel([128, 128, 128], &a);
            assert!(m[0] < 110 && m[0] > 60, "{m:?}");
        }
        // Gamma 2 is darker in the shadows than Gamma 1
        let g1 = correct_pixel([74, 74, 74], &Adjustments { correction: Correction::Gamma1, ..Adjustments::identity() });
        let g2 = correct_pixel([74, 74, 74], &Adjustments { correction: Correction::Gamma2, ..Adjustments::identity() });
        assert!(g2[0] < g1[0]);
    }

    #[test]
    fn lut_modes_tint_white_and_leave_pure_black() {
        for c in [Correction::Perceptual, Correction::Saturation, Correction::RelativeColorimetric, Correction::AbsoluteColorimetric] {
            let a = Adjustments { correction: c, ..Adjustments::identity() };
            let w = correct_pixel([255, 255, 255], &a);
            assert!(w[0] < 250 && w[0] > 232 && w[2] < w[1], "{c:?} {w:?}");
            assert_eq!(correct_pixel([0, 0, 0], &a), [0, 0, 0]);
        }
    }

    #[test]
    fn rgba_skips_transparent_pixels_and_keeps_alpha() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([255, 255, 255, 0]));
        img.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        apply_rgba(&mut img, &Adjustments { correction: Correction::Perceptual, ..Adjustments::identity() });
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 0]);
        let p = img.get_pixel(1, 0).0;
        assert_eq!(p[3], 255);
        assert!(p[0] < 250);
    }

    #[test]
    fn ppd_mapping_table() {
        use Correction::*;
        let table = [
            (None, "0ColourCorrection", true),
            (Gamma1, "1ColourCorrection", false),
            (Gamma2, "1ColourCorrection", false),
            (Perceptual, "2ColourCorrection", true),
            (Saturation, "3ColourCorrection", true),
            (RelativeColorimetric, "4ColourCorrection", true),
            (AbsoluteColorimetric, "5ColourCorrection", true),
        ];
        for (c, choice, exact) in table {
            assert_eq!(c.ppd_choice(), choice, "{c:?}");
            assert_eq!(c.has_driver_equivalent(), exact, "{c:?}");
        }
    }
}

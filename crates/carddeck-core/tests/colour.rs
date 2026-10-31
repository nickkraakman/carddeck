//! Colour-correction preview tests. Headless, no private samples needed.
//!
//! * Numeric check against what Hub's preview showed for a set of swatches (fixture CSV, derived from
//!   screenshots; see docs/COLOUR-CORRECTION.md). LUT modes are an exact port of the driver table, so
//!   the only error is screenshot quantisation; Gamma 1/2 are fitted curves.
//! * Print path: the print bitmap is never pre-corrected, the matching `ColourCorrection` option is sent.
//! * `adjustments` block: read from / written to a real `.card` layout without touching anything else.

use carddeck_core::colour::{self, store, Adjustments, Correction};
use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions};
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("fixtures/hub-colour-swatches.csv");

fn hex(h: &str) -> [u8; 3] {
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    [v(1), v(3), v(5)]
}

struct Row {
    setting: Correction,
    src: [u8; 3],
    hub: [u8; 3],
}

fn rows() -> Vec<Row> {
    FIXTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("setting"))
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            Row {
                setting: Correction::from_json_name(f[0]).unwrap(),
                src: hex(f[1]),
                hub: [f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap()],
            }
        })
        .collect()
}

/// (mean abs error per channel, max abs error) of our preview vs Hub for one setting.
fn error_for(c: Correction) -> (f64, i32, usize) {
    let adj = Adjustments { correction: c, ..Adjustments::identity() };
    let (mut sum, mut max, mut n) = (0.0, 0, 0usize);
    for r in rows().iter().filter(|r| r.setting == c) {
        let ours = colour::correct_pixel(r.src, &adj);
        for i in 0..3 {
            let e = (ours[i] as i32 - r.hub[i] as i32).abs();
            sum += e as f64;
            max = max.max(e);
        }
        n += 1;
    }
    (sum / (n as f64 * 3.0), max, n)
}

#[test]
fn matches_hub_preview_swatches() {
    // Tolerances: noise floor of the screenshot method is about 0.2 mean / 2 max (see the None row).
    for (c, mean_tol, max_tol) in [
        (Correction::None, 0.5, 2),
        (Correction::Perceptual, 0.6, 3),
        (Correction::Saturation, 0.6, 3),
        (Correction::RelativeColorimetric, 0.6, 3),
        (Correction::AbsoluteColorimetric, 0.6, 3),
        // fitted curves: slightly looser
        (Correction::Gamma1, 0.8, 4),
        (Correction::Gamma2, 0.8, 4),
    ] {
        let (mean, max, n) = error_for(c);
        assert!(n >= 25, "{c:?}: only {n} swatches in the fixture");
        assert!(mean <= mean_tol && max <= max_tol, "{c:?}: mean {mean:.2} (tol {mean_tol}), max {max} (tol {max_tol})");
    }
}

#[test]
fn sanity_checks_from_the_screenshots() {
    // Card background: the LUT modes turn white into a slightly greenish off-white (~#F0F0EB), the Gamma modes keep white.
    for c in [Correction::Perceptual, Correction::Saturation, Correction::RelativeColorimetric, Correction::AbsoluteColorimetric] {
        let w = colour::correct_pixel([255, 255, 255], &Adjustments { correction: c, ..Adjustments::identity() });
        assert!((236..=246).contains(&w[0]) && (238..=246).contains(&w[1]) && (230..=242).contains(&w[2]), "{c:?}: {w:?}");
        assert!(w[1] >= w[0] && w[1] > w[2], "{c:?}: green-ish, not pink: {w:?}");
    }
    for c in [Correction::Gamma1, Correction::Gamma2] {
        assert_eq!(colour::correct_pixel([255, 255, 255], &Adjustments { correction: c, ..Adjustments::identity() }), [255, 255, 255]);
    }
    // Gamma 1/2 darken and deepen colours.
    let teal = [0x01u8, 0x8d, 0x8c];
    for c in [Correction::Gamma1, Correction::Gamma2] {
        let o = colour::correct_pixel(teal, &Adjustments { correction: c, ..Adjustments::identity() });
        assert!(o[1] < teal[1] - 20 && o[2] < teal[2] - 20, "{c:?}: {o:?}");
    }
    // Absolute/Relative make greens far more saturated than the original (chroma = max - min).
    let green = [0x64u8, 0x80, 0x72]; // grey-green swatch, chroma 28
    let chroma = |p: [u8; 3]| *p.iter().max().unwrap() as i32 - *p.iter().min().unwrap() as i32;
    for c in [Correction::AbsoluteColorimetric, Correction::RelativeColorimetric] {
        let o = colour::correct_pixel(green, &Adjustments { correction: c, ..Adjustments::identity() });
        assert!(chroma(o) * 2 >= chroma(green) * 3, "{c:?}: {o:?}"); // at least 1.5x
    }
}

// ---------------------------------------------------------------------------------------------
// synthetic card with an adjustments block
// ---------------------------------------------------------------------------------------------

fn card_text(adjustments: Option<Value>) -> String {
    let side = |fill: &str| {
        json!({
            "background": {"width": 1013, "height": 642, "cornerRadius": 20, "fill": "#ffffff", "fillType": "colour", "fillPriority": "color"},
            "elements": [
                {"className": "Rect", "name": "Rect", "x": 100, "y": 100, "width": 200, "height": 200, "fill": fill, "fillPriority": "color", "strokeWidth": 0},
                {"className": "Rect", "name": "Rect", "x": 400, "y": 100, "width": 100, "height": 100, "fill": "#000000", "fillPriority": "color", "strokeWidth": 0}
            ]
        })
    };
    let mut doc = json!({
        "about": {"hubVersion": "2.0.8", "type": {"name": "CR80", "widthPixels": 1013, "heightPixels": 642, "CardSize": 0},
                  "preview": "data:image/png;base64,AAAA", "doubleSided": true, "portrait": false, "darkCard": false},
        "front": side("#018d8c"),
        "back": side("#c78899"),
    });
    if let Some(a) = adjustments {
        doc["adjustments"] = a;
    }
    serde_json::to_string_pretty(&doc).unwrap().replace('\n', "\r\n")
}

#[test]
fn card_reads_its_adjustments_block() {
    let c = Card::parse(card_text(Some(json!({"correction": "AbsoluteColorimetric", "brightness": 0.1, "red": 1.2}))).as_bytes()).unwrap();
    assert!(c.has_adjustments);
    assert_eq!(c.adjustments.correction, Correction::AbsoluteColorimetric);
    assert_eq!(c.adjustments.brightness, 0.1);
    assert_eq!(c.adjustments.red, 1.2);
    assert_eq!(c.adjustments.green, 1.0);
    let c = Card::parse(card_text(None).as_bytes()).unwrap();
    assert!(!c.has_adjustments);
    assert_eq!(c.adjustments, Adjustments::default(), "Hub's default for files without the block");
}

#[test]
fn preview_is_corrected_but_print_bitmap_is_not() {
    let fonts = FontSet::system();
    let text = card_text(Some(json!({"correction": "Perceptual", "brightness": 0, "contrast": 0, "saturation": 0, "red": 1, "green": 1, "blue": 1})));
    let card = Card::parse(text.as_bytes()).unwrap();
    let r = render_side(&card, false, &fonts, &RenderOptions::default());
    // Preview: transformed
    let prev = r.to_corrected_rgba(&card.adjustments);
    let raw = r.to_rgb_over_white();
    assert_ne!(prev.get_pixel(150, 150).0[..3], raw.get_pixel(150, 150).0[..]);
    assert_eq!(prev.get_pixel(150, 150).0[..3], colour::correct_pixel([0x01, 0x8d, 0x8c], &card.adjustments));
    assert_eq!(prev.get_pixel(450, 150).0[..3], [0, 0, 0], "pure black is left alone (resin K)");
    assert_eq!(prev.get_pixel(0, 0).0[3], 0, "transparent card corner stays transparent");
    // Print: the page bitmaps are identical with and without a correction selected
    let mut o = PrintOptions { sides: Sides::Both, ..Default::default() };
    let plain = build_job(&card, &fonts, &o).unwrap();
    o.use_colour(&card.adjustments, false);
    let with = build_job(&card, &fonts, &o).unwrap();
    assert_eq!(plain.pages.len(), with.pages.len());
    for (a, b) in plain.pages.iter().zip(&with.pages) {
        assert_eq!(a.rgb.as_raw(), b.rgb.as_raw(), "print bitmap must not be pre-corrected");
    }
    // ... and those pages contain the *original* colours (page is the card turned and cropped, so look for them)
    assert!(with.pages[0].rgb.pixels().any(|p| p.0 == [0x01, 0x8d, 0x8c]));
    assert!(with.pages[0].rgb.pixels().any(|p| p.0 == [0, 0, 0]));
}

#[test]
fn bake_is_opt_in_and_only_does_what_the_driver_cannot() {
    let fonts = FontSet::system();
    let card = Card::parse(card_text(None).as_bytes()).unwrap();
    let find = |job: &Job, c: [u8; 3]| job.pages[0].rgb.pixels().any(|p| p.0 == c);
    let cc = |o: &PrintOptions| ppd_options(o).into_iter().find(|(k, _)| k == "ColourCorrection").unwrap().1;
    // Gamma is never baked, even with bake on: the driver must still colour-match.
    // Baking it and sending None printed light, washed-out cards.
    let adj = Adjustments { correction: Correction::Gamma2, ..Adjustments::identity() };
    let mut o = PrintOptions { sides: Sides::Front, ..Default::default() };
    o.use_colour(&adj, true);
    let job = build_job(&card, &fonts, &o).unwrap();
    assert!(find(&job, [0x01, 0x8d, 0x8c]), "gamma must not be baked");
    assert!(find(&job, [255, 255, 255]));
    assert!(find(&job, [0, 0, 0]));
    assert_eq!(cc(&o), "1ColourCorrection");
    // Sliders are baked, and the driver option stays Driver managed.
    let adj = Adjustments { correction: Correction::Gamma2, brightness: -0.1, ..Adjustments::identity() };
    o.use_colour(&adj, true);
    let job = build_job(&card, &fonts, &o).unwrap();
    let sliders_only = Adjustments { correction: Correction::None, brightness: -0.1, ..Adjustments::identity() };
    assert!(find(&job, colour::correct_pixel([255, 255, 255], &sliders_only)));
    assert!(!find(&job, colour::correct_pixel([255, 255, 255], &adj)), "gamma must not be applied on top of the sliders");
    assert_eq!(cc(&o), "1ColourCorrection");
    // LUT modes are never baked, even with bake on.
    let adj = Adjustments { correction: Correction::AbsoluteColorimetric, ..Adjustments::identity() };
    o.use_colour(&adj, true);
    let job = build_job(&card, &fonts, &o).unwrap();
    assert!(find(&job, [0x01, 0x8d, 0x8c]));
    assert!(find(&job, [255, 255, 255]));
    assert_eq!(cc(&o), "5ColourCorrection");
}

#[test]
fn lp_options_carry_the_matching_colour_correction() {
    let args = |c: Option<Correction>| {
        let mut o = PrintOptions { printer: Some("Magicard_300".into()), ..Default::default() };
        if let Some(c) = c {
            o.use_colour(&Adjustments { correction: c, ..Adjustments::identity() }, false);
        }
        lp_command_line(std::path::Path::new("/tmp/job.pdf"), &o)
    };
    // No colour settings -> no option (existing snapshot test covers the full string).
    assert!(!args(None).contains("ColourCorrection"));
    for (c, want) in [
        (Correction::None, "ColourCorrection=0ColourCorrection"),
        (Correction::Gamma1, "ColourCorrection=1ColourCorrection"),
        (Correction::Gamma2, "ColourCorrection=1ColourCorrection"),
        (Correction::Perceptual, "ColourCorrection=2ColourCorrection"),
        (Correction::Saturation, "ColourCorrection=3ColourCorrection"),
        (Correction::RelativeColorimetric, "ColourCorrection=4ColourCorrection"),
        (Correction::AbsoluteColorimetric, "ColourCorrection=5ColourCorrection"),
    ] {
        let cmd = args(Some(c));
        assert!(cmd.contains(&format!("-o {want} ")), "{c:?}: {cmd}");
    }
    // Exact option order of a full command (colour option sits between the overcoat and print-scaling options).
    assert_eq!(
        args(Some(Correction::AbsoluteColorimetric)),
        "lp -d Magicard_300 -n 1 -t CardDeck -o PageSize=1XCR80 -o UTDuplex=1Both -o CFColourFormat=1ColourYMCK -o CFBlackResin=0BlackResinK \
         -o CBColourFormat=1ColourYMCK -o CBBlackResin=0BlackResinK -o CFOverCoat=1PrintOvercoat -o CBOverCoat=1PrintOvercoat \
         -o ColourCorrection=5ColourCorrection -o print-scaling=none /tmp/job.pdf"
    );
    // An explicit extra option still wins.
    let mut o = PrintOptions::default();
    o.use_colour(&Adjustments { correction: Correction::Perceptual, ..Adjustments::identity() }, false);
    o.extra_options.push(("ColourCorrection".into(), "0ColourCorrection".into()));
    let opts = ppd_options(&o);
    assert_eq!(opts.iter().filter(|(k, _)| k == "ColourCorrection").count(), 1);
    assert!(opts.contains(&("ColourCorrection".to_string(), "0ColourCorrection".to_string())));
}

#[test]
fn warnings_explain_gamma_and_sliders() {
    let fonts = FontSet::system();
    let card = Card::parse(card_text(None).as_bytes()).unwrap();
    let mut o = PrintOptions { sides: Sides::Front, ..Default::default() };
    o.use_colour(&Adjustments { correction: Correction::Gamma1, red: 1.2, ..Adjustments::identity() }, false);
    let w = build_job(&card, &fonts, &o).unwrap().warnings.join("\n");
    assert!(w.contains("Gamma 1 has no driver curve"), "{w}");
    assert!(w.contains("sliders are preview only"), "{w}");
    o.use_colour(&Adjustments { correction: Correction::Perceptual, ..Adjustments::identity() }, false);
    assert!(build_job(&card, &fonts, &o).unwrap().warnings.iter().all(|w| !w.contains("Gamma")));
}

#[test]
fn ppd_validation_knows_the_colour_correction_choices() {
    let ppd = parse_lpoptions(
        "UTDuplex/Duplex: 0Front *1Both 2Back\n\
         ColourCorrection/Colour correction: 0ColourCorrection 1ColourCorrection 2ColourCorrection 3ColourCorrection 4ColourCorrection *5ColourCorrection\n",
    );
    let mut o = PrintOptions::default();
    o.use_colour(&Adjustments { correction: Correction::RelativeColorimetric, ..Adjustments::identity() }, false);
    let problems = validate_against_ppd(&o, &ppd);
    assert!(problems.iter().all(|p| !p.contains("ColourCorrection")), "{problems:?}");
    // A PPD that lacks the option is reported.
    let ppd = parse_lpoptions("UTDuplex/Duplex: 0Front *1Both 2Back\n");
    assert!(validate_against_ppd(&o, &ppd).iter().any(|p| p.contains("ColourCorrection")));
}

#[test]
fn saving_adjustments_into_a_card_file_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.card");
    let original = card_text(Some(json!({"correction": "Saturation", "brightness": 0, "contrast": 0, "saturation": 0, "red": 1, "green": 1.061062, "blue": 1})));
    std::fs::write(&path, &original).unwrap();
    let adj = Adjustments { correction: Correction::Gamma2, brightness: -0.1, contrast: 2.0, saturation: 0.5, red: 1.1, green: 0.9, blue: 1.0 };
    store::save_adjustments(&path, &adj).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    // All bytes before the block are identical; CRLF is kept; the file still loads.
    let at = original.find("\"adjustments\"").unwrap();
    assert_eq!(&saved[..at], &original[..at]);
    assert!(!saved.replace("\r\n", "").contains('\n'));
    let card = Card::from_file(&path).unwrap();
    assert_eq!(card.adjustments, adj);
    // reset to Hub's defaults round-trips too
    store::save_adjustments(&path, &Adjustments::default()).unwrap();
    assert_eq!(Card::from_file(&path).unwrap().adjustments, Adjustments::default());
}

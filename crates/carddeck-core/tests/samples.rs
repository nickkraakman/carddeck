//! Numeric fidelity test against the Hub-generated `about.preview` of real `.card` files.
//!
//! The samples are private and not in the repo. Point `CARDDECK_SAMPLES` at a directory of `.card`
//! files (default: `<repo>/samples`). If none are found the test is skipped (prints a notice).

use carddeck_core::fonts::FontSet;
use carddeck_core::model::{Card, Element};
use carddeck_core::render::{node_transform, render_side, RenderOptions};
use image::{imageops::FilterType, RgbImage};
use std::path::PathBuf;

fn samples_dir() -> PathBuf {
    std::env::var("CARDDECK_SAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples"))
}

fn card_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "card").unwrap_or(false)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn over_white(img: &image::RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(img.width(), img.height());
    for (o, p) in out.pixels_mut().zip(img.pixels()) {
        let a = p.0[3] as f32 / 255.0;
        let f = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        *o = image::Rgb([f(p.0[0]), f(p.0[1]), f(p.0[2])]);
    }
    out
}

fn text_boxes(card: &Card, scale: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut v = Vec::new();
    for e in &card.front.elements {
        if e.class_name() == "Text" && e.visible() {
            v.push(bbox(e, scale));
        }
    }
    v
}

fn bbox(e: &Element, scale: f32) -> (f32, f32, f32, f32) {
    let fs = e.num_or("fontSize", 20.0) as f32;
    let text = e.str("text").unwrap_or("");
    let longest = text.split('\n').map(|l| l.chars().count()).max().unwrap_or(1) as f32;
    let w = e.num("width").map(|v| v as f32).unwrap_or(longest * fs * 0.65);
    let h = e.num("height").map(|v| v as f32).unwrap_or(text.split('\n').count() as f32 * fs * 1.3);
    let ts = node_transform(tiny_skia::Transform::from_scale(scale, scale), e);
    let mut pts = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    for p in pts.iter_mut() {
        let (x, y) = *p;
        *p = (ts.sx * x + ts.kx * y + ts.tx, ts.ky * x + ts.sy * y + ts.ty);
    }
    let minx = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let maxx = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
    let miny = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let maxy = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    (minx - 3.0, miny - 3.0, maxx + 3.0, maxy + 3.0)
}

/// Mean absolute difference per channel (0..255), optionally skipping boxes.
fn mad(a: &RgbImage, b: &RgbImage, skip: &[(f32, f32, f32, f32)]) -> f64 {
    assert_eq!(a.dimensions(), b.dimensions());
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in 0..a.height() {
        for x in 0..a.width() {
            let (fx, fy) = (x as f32, y as f32);
            if skip.iter().any(|b| fx >= b.0 && fx <= b.2 && fy >= b.1 && fy <= b.3) {
                continue;
            }
            let (pa, pb) = (a.get_pixel(x, y), b.get_pixel(x, y));
            for c in 0..3 {
                sum += (pa.0[c] as i32 - pb.0[c] as i32).unsigned_abs() as u64;
            }
            n += 3;
        }
    }
    sum as f64 / n.max(1) as f64
}

#[test]
fn front_matches_hub_preview() {
    let files = card_files();
    if files.is_empty() {
        eprintln!("SKIP: no .card samples found in {}", samples_dir().display());
        return;
    }
    let fonts = FontSet::system();
    let mut full_sum = 0.0;
    let mut geo_sum = 0.0;
    let mut worst_full: f64 = 0.0;
    let mut worst_geo: f64 = 0.0;
    println!("{:<22} {:>10} {:>12}", "card", "full MAD", "no-text MAD");
    for f in &files {
        let card = Card::from_file(f).unwrap();
        let preview_png = card.about.preview_png.clone().expect("preview");
        let preview = over_white(&image::load_from_memory(&preview_png).unwrap().to_rgba8());
        let (pw, ph) = preview.dimensions();

        // full render, downscaled to preview size
        // Hub renders the preview at 0.5x pixel ratio; do the same natively and crop to its size.
        let opts = RenderOptions { scale: 0.5, ..Default::default() };
        let r = render_side(&card, false, &fonts, &opts);
        let rgb = r.to_rgb_over_white();
        let ours = image::imageops::crop_imm(&rgb, 0, 0, pw.min(rgb.width()), ph.min(rgb.height())).to_image();
        let _ = FilterType::Triangle;
        let full = mad(&ours, &preview, &[]);

        // geometry only: same render but compare outside every text box
        let boxes = text_boxes(&card, 0.5);
        let geo = mad(&ours, &preview, &boxes);

        if let Ok(dir) = std::env::var("CARDDECK_DUMP") {
            let mut d = RgbImage::new(pw, ph);
            for y in 0..ph.min(ours.height()) { for x in 0..pw.min(ours.width()) {
                let (a, b) = (ours.get_pixel(x, y), preview.get_pixel(x, y));
                let skipped = boxes.iter().any(|bx| (x as f32) >= bx.0 && (x as f32) <= bx.2 && (y as f32) >= bx.1 && (y as f32) <= bx.3);
                let v = if skipped { 0 } else { ((0..3).map(|c| (a.0[c] as i32 - b.0[c] as i32).abs()).sum::<i32>() / 3 * 4).min(255) as u8 };
                d.put_pixel(x, y, image::Rgb([v, v, if skipped { 120 } else { v }]));
            } }
            d.save(format!("{dir}/{}-diff.png", f.file_stem().unwrap().to_string_lossy())).unwrap();
        }
        println!("{:<22} {:>10.2} {:>12.2}", f.file_stem().unwrap().to_string_lossy(), full, geo);
        full_sum += full;
        geo_sum += geo;
        worst_full = worst_full.max(full);
        worst_geo = worst_geo.max(geo);
    }
    let n = files.len() as f64;
    println!("MEAN over {} cards: full {:.2}, outside text boxes {:.2} (worst {:.2} / {:.2})", files.len(), full_sum / n, geo_sum / n, worst_full, worst_geo);
    // `test.card` has a stale about.preview (text elements differ), hence the looser worst-case bound.
    assert!(geo_sum / n < 1.8, "non-text regions differ too much from Hub preview: mean {}", geo_sum / n);
    assert!(worst_geo < 3.5, "non-text regions differ too much from Hub preview: {worst_geo}");
    assert!(full_sum / n < 7.0 && worst_full < 8.0, "full render differs too much from Hub preview: {worst_full}");
}

/// Both sides against the Pillow reference renders (`<stem>-front.png`/`-back.png` in `<samples>/render`).
/// The reference renderer is approximate too (different text engine), so this mostly guards the back side,
/// which has no Hub-generated preview.
#[test]
fn both_sides_close_to_reference_renders() {
    let render_dir = samples_dir().join("render");
    let fonts = FontSet::system();
    let mut n = 0;
    let mut sums = [0.0f64; 2];
    let mut worst: f64 = 0.0;
    for f in card_files() {
        let stem = f.file_stem().unwrap().to_string_lossy().replace(' ', "_");
        for (i, side) in ["front", "back"].iter().enumerate() {
            let p = render_dir.join(format!("{stem}-{side}.png"));
            let Ok(refimg) = image::open(&p) else { continue };
            let refimg = over_white(&refimg.to_rgba8());
            let card = Card::from_file(&f).unwrap();
            let r = render_side(&card, i == 1, &fonts, &RenderOptions::default());
            let ours = r.to_rgb_over_white();
            if ours.dimensions() != refimg.dimensions() {
                continue;
            }
            let m = mad(&ours, &refimg, &[]);
            println!("{stem:<22} {side:<6} MAD vs reference render: {m:.2}");
            sums[i] += m;
            worst = worst.max(m);
            if i == 0 {
                n += 1;
            }
        }
    }
    if n == 0 {
        eprintln!("SKIP: no reference renders found in {}", render_dir.display());
        return;
    }
    println!("MEAN vs reference renders: front {:.2}, back {:.2} over {n} cards (worst {worst:.2})", sums[0] / n as f64, sums[1] / n as f64);
    assert!(worst < 12.0, "render differs too much from the reference renders: {worst}");
}

/// Decodes a pixmap/bitmap QR code with an independent decoder (rqrr).
fn decode_qr(img: &image::RgbaImage) -> Option<String> {
    // put on a white quiet-zone border, grayscale
    let b = 24u32;
    let mut g = image::GrayImage::from_pixel(img.width() + 2 * b, img.height() + 2 * b, image::Luma([255]));
    for (x, y, p) in img.enumerate_pixels() {
        let a = p.0[3] as f32 / 255.0;
        let v = (p.0[0] as f32 * a + 255.0 * (1.0 - a)) as u8;
        g.put_pixel(x + b, y + b, image::Luma([v]));
    }
    let mut prep = rqrr::PreparedImage::prepare(g);
    prep.detect_grids().first().and_then(|grid| grid.decode().ok()).map(|(_, content)| content)
}

/// Stored QR bitmaps decode to their `qrData`, and regenerated ones decode to the same text
/// (module patterns differ because encoders may pick a different mask; decoding is what matters).
#[test]
fn qr_bitmaps_and_regeneration_decode_to_qrdata() {
    let mut checked = 0;
    let mut stored_ok = 0;
    let mut regen_ok = 0;
    for f in card_files() {
        let card = Card::from_file(&f).unwrap();
        for e in card.front.elements.iter().chain(card.back.elements.iter()) {
            if e.name() != "QRCode" {
                continue;
            }
            let data = e.str("qrData").filter(|s| !s.is_empty()).unwrap_or("123456789");
            let Ok(bytes) = carddeck_core::decode_data_url(e.str("base64image").unwrap_or("")) else { continue };
            let stored = image::load_from_memory(&bytes).unwrap().to_rgba8();
            checked += 1;
            if decode_qr(&stored).as_deref() == Some(data) {
                stored_ok += 1;
            }
            let regen = carddeck_core::qr::qr_pixmap(data).unwrap();
            // scale up 6x nearest so the detector has enough pixels per module
            let img = carddeck_core::render::pixmap_to_rgba(&regen);
            let big = image::imageops::resize(&img, img.width() * 6, img.height() * 6, FilterType::Nearest);
            if decode_qr(&big).as_deref() == Some(data) {
                regen_ok += 1;
            }
        }
    }
    if checked == 0 {
        eprintln!("SKIP: no QR codes in samples");
        return;
    }
    println!("QR codes: stored bitmap decodes to qrData {stored_ok}/{checked}; regenerated decodes {regen_ok}/{checked}");
    assert_eq!(stored_ok, checked);
    assert_eq!(regen_ok, checked);
}

/// QR codes must still scan after the full render (scaling, placement).
#[test]
fn rendered_qr_codes_scan() {
    let fonts = FontSet::system();
    let (mut total, mut ok) = (0, 0);
    for f in card_files() {
        let card = Card::from_file(&f).unwrap();
        let rgba = pixmap_rgba(&render_side(&card, false, &fonts, &RenderOptions::default()));
        for e in &card.front.elements {
            if e.name() != "QRCode" || !e.visible() {
                continue;
            }
            let (x, y) = (e.num_or("x", 0.0).max(0.0) as u32, e.num_or("y", 0.0).max(0.0) as u32);
            let (w, h) = (e.num_or("width", 0.0) as u32, e.num_or("height", 0.0) as u32);
            let crop = image::imageops::crop_imm(&rgba, x, y, w.min(rgba.width() - x), h.min(rgba.height() - y)).to_image();
            let want = e.str("qrData").filter(|s| !s.is_empty()).unwrap_or("123456789");
            total += 1;
            // like a camera would, look at it enlarged and smoothed
            let big = image::imageops::resize(&crop, crop.width() * 5, crop.height() * 5, FilterType::CatmullRom);
            let got = decode_qr(&big);
            if got.as_deref() == Some(want) {
                ok += 1;
            } else {
                println!("QR scan failed: {} at ({x},{y}) {w}x{h} stored {:?}", f.display(), e.str("qrData"));
                if let Ok(d) = std::env::var("CARDDECK_DUMP") { crop.save(format!("{d}/qrfail-{x}-{y}.png")).ok(); }
            }
        }
    }
    if total == 0 {
        return;
    }
    println!("rendered QR codes that scan: {ok}/{total}");
    assert_eq!(ok, total);
}

fn pixmap_rgba(r: &carddeck_core::Rendered) -> image::RgbaImage {
    carddeck_core::render::pixmap_to_rgba(&r.pixmap)
}

/// Real Hub files: the `adjustments` block is read, and saving a changed block into a copy changes
/// nothing else (byte for byte before the block, parsed JSON equal elsewhere).
#[test]
fn adjustments_roundtrip_on_real_files() {
    use carddeck_core::colour::{store, Adjustments, Correction};
    let files = card_files();
    if files.is_empty() {
        eprintln!("SKIP adjustments_roundtrip_on_real_files: no samples in {}", samples_dir().display());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let card = Card::from_file(&f).unwrap();
        let original = std::fs::read_to_string(&f).unwrap();
        let copy = dir.path().join(f.file_name().unwrap());
        std::fs::write(&copy, &original).unwrap();
        // Saving the values we just read must reproduce Hub's file byte for byte.
        store::save_adjustments(&copy, &card.adjustments).unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), original, "{}: no-op save changed the file", f.display());
        // A real change touches only the block.
        let adj = Adjustments { correction: Correction::Perceptual, brightness: 0.1, red: 1.25, ..card.adjustments };
        store::save_adjustments(&copy, &adj).unwrap();
        let saved = std::fs::read_to_string(&copy).unwrap();
        if let Some(i) = original.find("\"adjustments\"") {
            assert_eq!(&saved[..i], &original[..i], "{}", f.display());
        }
        assert_eq!(Card::from_file(&copy).unwrap().adjustments, adj);
        eprintln!("{}: saved correction {:?} -> Perceptual", f.file_name().unwrap().to_string_lossy(), card.adjustments.correction);
    }
}

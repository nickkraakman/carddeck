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
    assert!(worst_geo < 3.0, "non-text regions differ too much from Hub preview: {worst_geo}");
    assert!(worst_full < 12.0, "full render differs too much from Hub preview: {worst_full}");
}

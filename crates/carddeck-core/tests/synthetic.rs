//! Tests on synthetic cards (no private samples needed).

use std::collections::BTreeMap;

use base64::Engine;
use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions};
use serde_json::{json, Value};

fn png_data_url(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> String {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba(f(x, y));
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out))
}

fn side(elements: Vec<Value>) -> Value {
    json!({
        "background": {"width": 1013, "height": 642, "cornerRadius": 20, "fill": "#ffffff", "fillType": "colour", "fillPriority": "color"},
        "elements": elements
    })
}

fn card_json(front: Vec<Value>, back: Vec<Value>) -> Vec<u8> {
    let preview = png_data_url(4, 4, |_, _| [255, 255, 255, 255]);
    serde_json::to_vec(&json!({
        "about": {"hubVersion": "2.0.8", "type": {"name": "CR80", "widthPixels": 1013, "heightPixels": 642, "CardSize": 0},
                  "preview": preview, "doubleSided": true, "portrait": false, "darkCard": false},
        "front": side(front),
        "back": side(back),
        "adjustments": {}
    }))
    .unwrap()
}

fn rect(x: f64, y: f64, w: f64, h: f64, fill: &str) -> Value {
    json!({"className": "Rect", "name": "Rect", "x": x, "y": y, "width": w, "height": h, "fill": fill, "fillPriority": "color", "strokeWidth": 0})
}

fn fonts() -> FontSet {
    FontSet::system()
}

fn px(img: &image::RgbImage, x: u32, y: u32) -> [u8; 3] {
    img.get_pixel(x, y).0
}

#[test]
fn parses_and_renders_basic_shapes() {
    let mut rot = rect(500.0, 300.0, 100.0, 50.0, "#00ff00");
    rot["rotation"] = json!(90);
    let mut half = rect(700.0, 100.0, 50.0, 50.0, "#000000");
    half["opacity"] = json!(0.5);
    let bytes = card_json(
        vec![
            rect(10.0, 10.0, 100.0, 100.0, "#000000"),
            rect(200.0, 10.0, 100.0, 100.0, "#ff0000"),
            rot,
            half,
            json!({"className":"Rect","x":-50,"y":500,"width":200,"height":300,"fill":"#0000ff","visible":false}),
        ],
        vec![],
    );
    let card = Card::parse(&bytes).unwrap();
    assert_eq!(card.frame_size(), (1013, 642));
    assert!(card.about.double_sided);
    let r = render_side(&card, false, &fonts(), &RenderOptions::default());
    assert_eq!((r.width(), r.height()), (1013, 642));
    let rgb = r.to_rgb_over_white();
    assert_eq!(px(&rgb, 50, 50), [0, 0, 0], "pure black rect stays exactly #000000 (resin rule)");
    assert_eq!(px(&rgb, 250, 50), [255, 0, 0]);
    // rotated 90 degrees clockwise about (500,300): occupies x 450..500, y 300..400
    assert_eq!(px(&rgb, 475, 350), [0, 255, 0]);
    assert_eq!(px(&rgb, 550, 325), [255, 255, 255]);
    // 50 % black over white
    let g = px(&rgb, 720, 120);
    assert!((g[0] as i32 - 128).abs() <= 2, "{g:?}");
    // invisible element skipped
    assert_eq!(px(&rgb, 20, 560), [255, 255, 255]);
    // card clip: corner pixel transparent
    assert_eq!(r.pixmap.pixel(0, 0).unwrap().alpha(), 0);
    assert_eq!(r.pixmap.pixel(500, 0).unwrap().alpha(), 255);
}

#[test]
fn konva_transform_order_scale_offset() {
    // x,y translate; offset shifts the local origin; scale applies around the offset
    let mut e = rect(0.0, 0.0, 20.0, 20.0, "#ff0000");
    e["x"] = json!(100);
    e["y"] = json!(100);
    e["offsetX"] = json!(10);
    e["offsetY"] = json!(10);
    e["scaleX"] = json!(2);
    e["scaleY"] = json!(2);
    let card = Card::parse(&card_json(vec![e], vec![])).unwrap();
    let rgb = render_side(&card, false, &fonts(), &RenderOptions::default()).to_rgb_over_white();
    // local (0..20) -> minus offset (-10..10) -> scale 2 (-20..20) -> plus (100,100): 80..120
    assert_eq!(px(&rgb, 82, 82), [255, 0, 0]);
    assert_eq!(px(&rgb, 118, 118), [255, 0, 0]);
    assert_eq!(px(&rgb, 78, 100), [255, 255, 255]);
    assert_eq!(px(&rgb, 122, 100), [255, 255, 255]);
}

#[test]
fn gradient_and_stroke() {
    let mut g = rect(100.0, 100.0, 200.0, 50.0, "#ffffff");
    g["fillPriority"] = json!("linear-gradient");
    g["fillLinearGradientColorStops"] = json!([0, "#000000", 1, "#ffffff"]);
    g["fillLinearGradientStartPointX"] = json!(0);
    g["fillLinearGradientStartPointY"] = json!(0);
    g["fillLinearGradientEndPointX"] = json!(200);
    g["fillLinearGradientEndPointY"] = json!(0);
    let mut s = rect(400.0, 100.0, 100.0, 100.0, "#ffffff");
    s["stroke"] = json!("#000000");
    s["strokeWidth"] = json!(10);
    let card = Card::parse(&card_json(vec![g, s], vec![])).unwrap();
    let rgb = render_side(&card, false, &fonts(), &RenderOptions::default()).to_rgb_over_white();
    assert!(px(&rgb, 102, 125)[0] < 10);
    assert!(px(&rgb, 298, 125)[0] > 245);
    let mid = px(&rgb, 200, 125)[0];
    assert!((mid as i32 - 128).abs() < 8, "{mid}");
    // stroke is centred on the edge: 5 px outside, 5 px inside
    assert_eq!(px(&rgb, 397, 150), [0, 0, 0]);
    assert_eq!(px(&rgb, 403, 150), [0, 0, 0]);
    assert_eq!(px(&rgb, 410, 150), [255, 255, 255]);
}

#[test]
fn image_black_is_nudged_for_photos_but_not_for_qr() {
    let url = png_data_url(10, 10, |_, _| [0, 0, 0, 255]);
    let img = json!({"className":"Image","name":"Image","x":100,"y":100,"width":100,"height":100,"base64image":url,"monochrome":false});
    let qr = json!({"className":"Image","name":"QRCode","x":300,"y":100,"width":100,"height":100,"base64image":url});
    let card = Card::parse(&card_json(vec![img, qr], vec![])).unwrap();
    let on = render_side(&card, false, &fonts(), &RenderOptions::default()).to_rgb_over_white();
    assert_eq!(px(&on, 150, 150), [1, 1, 1], "photo black must be nudged to 1,1,1");
    assert_eq!(px(&on, 350, 150), [0, 0, 0], "QR stays pure black");
    let off = render_side(&card, false, &fonts(), &RenderOptions { prevent_resin_for_images: false, ..Default::default() }).to_rgb_over_white();
    assert_eq!(px(&off, 150, 150), [0, 0, 0]);
}

#[test]
fn image_downscale_is_smooth_not_aliased() {
    // 1px checkerboard shrunk 8x must average to mid gray
    let url = png_data_url(80, 80, |x, y| if (x + y) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] });
    let img = json!({"className":"Image","name":"Image","x":100,"y":100,"width":10,"height":10,"base64image":url});
    let card = Card::parse(&card_json(vec![img], vec![])).unwrap();
    let rgb = render_side(&card, false, &fonts(), &RenderOptions::default()).to_rgb_over_white();
    let v = px(&rgb, 105, 105)[0] as i32;
    assert!((v - 128).abs() < 20, "{v}");
}

#[test]
fn text_draws_and_aligns() {
    let t = |align: &str| json!({"className":"Text","name":"Text","text":"Hello","x":100,"y":100,"width":400,"height":80,
        "fontSize":60,"fontFamily":"Arial","fill":"#000000","align":align,"verticalAlign":"middle","wrap":"word"});
    let ink = |align: &str| -> (u32, u32) {
        let card = Card::parse(&card_json(vec![t(align)], vec![])).unwrap();
        let r = render_side(&card, false, &fonts(), &RenderOptions::default());
        let rgb = r.to_rgb_over_white();
        let mut minx = u32::MAX;
        let mut maxx = 0;
        for y in 100..180 {
            for x in 100..500 {
                if px(&rgb, x, y)[0] < 128 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                }
            }
        }
        (minx, maxx)
    };
    let (l0, l1) = ink("left");
    let (c0, c1) = ink("center");
    let (r0, r1) = ink("right");
    assert!(l0 < 120 && l1 > l0 + 100, "left ink {l0}..{l1}");
    let cmid = (c0 + c1) / 2;
    assert!((cmid as i32 - 300).abs() < 12, "center ink mid {cmid}");
    assert!(r1 > 480 && r1 <= 500 && r0 > 250, "right ink {r0}..{r1}");
}

#[test]
fn unsupported_elements_warn_not_fail() {
    let weird = json!({"className":"Wobble","name":"Wobble","x":1,"y":1});
    let card = Card::parse(&card_json(vec![weird], vec![])).unwrap();
    let r = render_side(&card, false, &fonts(), &RenderOptions::default());
    assert!(r.warnings.iter().any(|w| w.contains("Wobble")), "{:?}", r.warnings);
}

#[test]
fn qr_regeneration_when_bitmap_missing() {
    let qr = json!({"className":"Image","name":"QRCode","x":100,"y":100,"width":118,"height":118,"qrData":"https://example.com/x"});
    let card = Card::parse(&card_json(vec![qr], vec![])).unwrap();
    let r = render_side(&card, false, &fonts(), &RenderOptions::default());
    assert!(r.warnings.iter().any(|w| w.contains("regenerated")));
    let rgb = r.to_rgb_over_white();
    let dark = (100..218).flat_map(|y| (100..218).map(move |x| (x, y))).filter(|&(x, y)| px(&rgb, x, y)[0] < 40).count();
    assert!(dark > 2000, "QR should contain many dark pixels, got {dark}");
}

#[test]
fn bad_files_give_errors() {
    assert!(Card::parse(b"not json").is_err());
    assert!(Card::parse(b"{}").is_err());
    assert!(Card::parse(b"[1,2]").is_err());
}

#[test]
fn bom_is_tolerated() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend(card_json(vec![], vec![]));
    assert!(Card::parse(&b).is_ok());
}

// ---------------------------------------------------------------------------------------------
// print job
// ---------------------------------------------------------------------------------------------

fn marker_card() -> Card {
    // red 60x60 square at the top-left corner, blue at bottom-right of the landscape card
    Card::parse(&card_json(
        vec![rect(0.0, 0.0, 60.0, 60.0, "#ff0000"), rect(953.0, 582.0, 60.0, 60.0, "#0000ff")],
        vec![rect(0.0, 0.0, 60.0, 60.0, "#00ff00")],
    ))
    .unwrap()
}

#[test]
fn job_pages_are_driver_raster_size_and_ordered_front_back() {
    let job = build_job(&marker_card(), &fonts(), &PrintOptions::default()).unwrap();
    assert_eq!(job.pages.len(), 2);
    assert_eq!(job.pages[0].label, "front");
    assert_eq!(job.pages[1].label, "back");
    for p in &job.pages {
        assert_eq!(p.rgb.dimensions(), (PAGE_W, PAGE_H));
    }
}

#[test]
fn rotation_ccw_puts_card_top_left_at_page_bottom_left() {
    let o = PrintOptions { sides: Sides::Front, ..Default::default() };
    let job = build_job(&marker_card(), &fonts(), &o).unwrap();
    let p = &job.pages[0].rgb;
    // CCW 90: top edge -> left edge, left edge -> bottom edge, so top-left -> bottom-left.
    assert_eq!(px(p, 10, PAGE_H - 10), [255, 0, 0]);
    // bottom-right of the card -> top-right of the page
    assert_eq!(px(p, PAGE_W - 10, 10), [0, 0, 255]);
}

#[test]
fn rotation_cw_and_extra_180() {
    let o = PrintOptions { sides: Sides::Front, turn: PageTurn::Cw90, ..Default::default() };
    let p = &build_job(&marker_card(), &fonts(), &o).unwrap().pages[0].rgb;
    assert_eq!(px(p, PAGE_W - 10, 10), [255, 0, 0], "CW: top-left -> top-right");
    let o = PrintOptions { sides: Sides::Front, turn: PageTurn::Cw90, rotate_front_180: true, ..Default::default() };
    let p = &build_job(&marker_card(), &fonts(), &o).unwrap().pages[0].rgb;
    assert_eq!(px(p, 10, PAGE_H - 10), [255, 0, 0], "CW + 180 == CCW");
}

#[test]
fn crop_keeps_pure_black_pure() {
    let card = Card::parse(&card_json(vec![rect(0.0, 0.0, 1013.0, 642.0, "#000000")], vec![])).unwrap();
    let o = PrintOptions { sides: Sides::Front, ..Default::default() };
    let p = &build_job(&card, &fonts(), &o).unwrap().pages[0].rgb;
    // away from the rounded corners everything is exactly 0,0,0 (card corners are white after the clip)
    assert_eq!(px(p, 300, 500), [0, 0, 0]);
    assert!(p.pixels().filter(|q| q.0 == [0, 0, 0]).count() > 600_000);
    assert!(p.pixels().all(|q| q.0 == [0, 0, 0] || q.0[0] > 0 && q.0 == [q.0[0]; 3]), "only pure black or grey AA/white");
}

#[test]
fn copies_expand_in_pairs() {
    let o = PrintOptions { copies: 3, ..Default::default() };
    let job = build_job(&marker_card(), &fonts(), &o).unwrap();
    let labels: Vec<_> = job.pages.iter().map(|p| p.label).collect();
    assert_eq!(labels, ["front", "back", "front", "back", "front", "back"]);
}

#[test]
fn back_only_and_front_only() {
    let o = PrintOptions { sides: Sides::Back, ..Default::default() };
    let job = build_job(&marker_card(), &fonts(), &o).unwrap();
    assert_eq!(job.pages.len(), 1);
    assert_eq!(job.pages[0].label, "back");
}

#[test]
fn pdf_is_valid_shape() {
    let job = build_job(&marker_card(), &fonts(), &PrintOptions::default()).unwrap();
    let pdf = write_pdf(&job);
    assert!(pdf.starts_with(b"%PDF-"));
    let s = String::from_utf8_lossy(&pdf);
    assert_eq!(s.matches("/Type /Page\n").count() + s.matches("/Type /Page ").count() + s.matches("/Type /Page>").count(), 2, "two pages");
    assert!(s.contains("/MediaBox [0 0 153 243]"));
    assert!(s.contains("/FlateDecode"));
    assert!(s.contains("/DeviceRGB"));
    assert!(s.contains("/Width 638") && s.contains("/Height 1013"));
}

#[test]
fn lp_command_snapshot() {
    let o = PrintOptions { printer: Some("Magicard_300".into()), ..Default::default() };
    let cmd = lp_command_line(std::path::Path::new("/tmp/job.pdf"), &o);
    assert_eq!(
        cmd,
        "lp -d Magicard_300 -n 1 -t CardDeck -o PageSize=1XCR80 -o UTDuplex=1Both -o CFColourFormat=1ColourYMCK -o CFBlackResin=0BlackResinK \
         -o CBColourFormat=1ColourYMCK -o CBBlackResin=0BlackResinK -o CFOverCoat=1PrintOvercoat -o CBOverCoat=1PrintOvercoat -o print-scaling=none /tmp/job.pdf"
    );
    let o = PrintOptions {
        printer: Some("My Printer".into()),
        sides: Sides::Front,
        overcoat_front: false,
        black: BlackMode::CompositeYmc,
        extra_options: vec![("CFRotation".into(), "1Rotation".into())],
        ..Default::default()
    };
    let cmd = lp_command_line(std::path::Path::new("/tmp/a b.pdf"), &o);
    assert!(cmd.contains("-d 'My Printer'"));
    assert!(cmd.contains("UTDuplex=0Front"));
    assert!(cmd.contains("CFColourFormat=0ColourYMC") && cmd.contains("CFBlackResin=1BlackResinYMC"));
    assert!(cmd.contains("CFOverCoat=0PrintNoOvercoat"));
    assert!(cmd.contains("CFRotation=1Rotation"));
    assert!(cmd.ends_with("'/tmp/a b.pdf'"));
}

#[test]
fn colour_correction_maps_hub_labels_to_ppd() {
    assert_eq!(resolve_colour_correction("Saturation", None).as_deref(), Some("3ColourCorrection"));
    assert_eq!(resolve_colour_correction("Gamma 2", None).as_deref(), Some("1ColourCorrection"));
    assert_eq!(resolve_colour_correction("Relative Colorimetric", None).as_deref(), Some("4ColourCorrection"));
    assert_eq!(resolve_colour_correction("None", None).as_deref(), Some("0ColourCorrection"));

    let o = PrintOptions {
        printer: Some("Magicard_300".into()),
        colour_correction: Some("Saturation".into()),
        ..Default::default()
    };
    let cmd = lp_command_line(std::path::Path::new("/tmp/job.pdf"), &o);
    assert!(cmd.contains("ColourCorrection=3ColourCorrection"), "{cmd}");

    // Live PPD choice list (tokens only, as lpoptions -l prints them).
    let mut ppd = BTreeMap::new();
    ppd.insert(
        "ColourCorrection".into(),
        PpdOption {
            choices: vec![
                "0ColourCorrection".into(),
                "1ColourCorrection".into(),
                "2ColourCorrection".into(),
                "3ColourCorrection".into(),
                "4ColourCorrection".into(),
                "5ColourCorrection".into(),
            ],
            default: Some("5ColourCorrection".into()),
        },
    );
    assert_eq!(
        resolve_colour_correction("Absolute Colorimetric", Some(&ppd)).as_deref(),
        Some("5ColourCorrection")
    );
}

#[test]
fn lpstat_and_lpoptions_parsing() {
    let out = "printer Magicard_300 is idle.  enabled since Sat 17 Oct 2026 20:00:00 CEST\nprinter Other_Printer disabled since Fri 16 Oct 2026 - Paused\nprinter Busy now printing Busy-12.  enabled since X\n";
    let p = parse_lpstat_p(out);
    assert_eq!(p.len(), 3);
    assert_eq!(p[0].name, "Magicard_300");
    assert!(p[0].enabled);
    assert!(!p[1].enabled);
    assert!(p[2].status.starts_with("now printing"));
    assert_eq!(parse_lpstat_d("system default destination: Magicard_300\n").as_deref(), Some("Magicard_300"));
    assert_eq!(parse_lpstat_d("no system default destination\n"), None);

    let lo = "PageSize/Media Size: *1XCR80 2XCR79\nUTDuplex/Duplex Mode: 0Front *1Both 2Back\nCFOverCoat/Front Overcoat: 0PrintNoOvercoat *1PrintOvercoat\n";
    let ppd = parse_lpoptions(lo);
    assert_eq!(ppd["UTDuplex"].default.as_deref(), Some("1Both"));
    assert_eq!(ppd["UTDuplex"].choices, ["0Front", "1Both", "2Back"]);
    let problems = validate_against_ppd(&PrintOptions::default(), &ppd);
    assert!(problems.iter().any(|p| p.contains("CFColourFormat")), "{problems:?}");
    assert!(!problems.iter().any(|p| p.contains("UTDuplex")));
    let not_magicard = parse_lpoptions("PageSize/Media Size: *A4 Letter\n");
    assert_eq!(validate_against_ppd(&PrintOptions::default(), &not_magicard).len(), 1);
}

#[test]
fn save_job_writes_files() {
    let dir = tempfile::tempdir().unwrap();
    let o = PrintOptions::default();
    let job = build_job(&marker_card(), &fonts(), &o).unwrap();
    let files = save_job(dir.path(), &job, &o).unwrap();
    assert_eq!(files.len(), 4);
    assert!(dir.path().join("job.pdf").exists());
    assert!(dir.path().join("page-1-front.png").exists());
    assert!(dir.path().join("page-2-back.png").exists());
    let cmd = std::fs::read_to_string(dir.path().join("command.txt")).unwrap();
    assert!(cmd.starts_with("lp ") && cmd.contains("job.pdf"));
}

#[test]
fn portrait_design_is_turned_like_hub() {
    // 642x1013 design: front rotated -90 (CCW) to landscape, then the driver turn
    let mut v: Value = serde_json::from_slice(&card_json(vec![], vec![])).unwrap();
    v["about"]["portrait"] = json!(true);
    for s in ["front", "back"] {
        v[s]["background"]["width"] = json!(642);
        v[s]["background"]["height"] = json!(1013);
    }
    v["front"]["elements"] = json!([rect(0.0, 0.0, 60.0, 60.0, "#ff0000")]);
    let card = Card::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(card.frame_size(), (642, 1013));
    let o = PrintOptions { sides: Sides::Front, ..Default::default() };
    let p = &build_job(&card, &fonts(), &o).unwrap().pages[0].rgb;
    assert_eq!(p.dimensions(), (PAGE_W, PAGE_H));
    // portrait top-left --CCW--> landscape bottom-left --CCW--> page bottom-right
    assert_eq!(px(p, PAGE_W - 10, PAGE_H - 10), [255, 0, 0]);
}

#[test]
fn calibration_card_renders_and_prints() {
    let card = carddeck_core::calibration::calibration_card();
    let job = build_job(&card, &fonts(), &PrintOptions::default()).unwrap();
    assert_eq!(job.pages.len(), 2);
    let r = render_side(&card, false, &fonts(), &RenderOptions::default());
    let rgb = r.to_rgb_over_white();
    assert_eq!(px(&rgb, 60, 60), [255, 0, 0]);
    assert_eq!(px(&rgb, 60, 580), [0, 0, 0], "bottom-left marker is pure black");
}

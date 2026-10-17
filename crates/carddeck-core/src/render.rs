//! Renders one card side to a bitmap, mimicking how the Hub designer (Konva.js) draws it.
//!
//! Node transform order (Konva): `translate(x,y) · rotate · skew · scale · translate(-offsetX,-offsetY)`.
//! Everything is clipped to the card's rounded rectangle (radius 20 px).

use std::collections::BTreeSet;
use std::f32::consts::PI;

use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder,
    Pattern, Pixmap, Point, RadialGradient, Rect, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::fonts::{FaceChain, FontSet};
use crate::model::{Card, Element};

#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Output pixels per design pixel (1.0 = 300 dpi).
    pub scale: f32,
    /// Turn pure-black pixels of photographic images into (1,1,1) so they never print with resin.
    pub prevent_resin_for_images: bool,
    /// Skip all text elements (used by the numeric fidelity test to separate font error from geometry error).
    pub skip_text: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { scale: 1.0, prevent_resin_for_images: true, skip_text: false }
    }
}

pub struct Rendered {
    /// Premultiplied RGBA; pixels outside the card's rounded corners are transparent.
    pub pixmap: Pixmap,
    pub warnings: Vec<String>,
}

impl Rendered {
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }
    /// RGB composited over white (what ends up on the card stock).
    pub fn to_rgb_over_white(&self) -> image::RgbImage {
        pixmap_to_rgb_over_white(&self.pixmap)
    }
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let img = pixmap_to_rgba(&self.pixmap);
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("png encode");
        out
    }
}

pub fn pixmap_to_rgba(p: &Pixmap) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(p.width(), p.height());
    for (dst, src) in img.pixels_mut().zip(p.pixels()) {
        let c = src.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img
}

pub fn pixmap_to_rgb_over_white(p: &Pixmap) -> image::RgbImage {
    let mut img = image::RgbImage::new(p.width(), p.height());
    for (dst, src) in img.pixels_mut().zip(p.pixels()) {
        let a = src.alpha() as u32;
        // premultiplied: out = c + (255 - a) * white
        let f = |c: u8| (c as u32 + (255 - a)).min(255) as u8;
        *dst = image::Rgb([f(src.red()), f(src.green()), f(src.blue())]);
    }
    img
}

// ---------------------------------------------------------------------------------------------
// colours
// ---------------------------------------------------------------------------------------------

pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        let v1 = |i: usize| u8::from_str_radix(&h.get(i..i + 1)?.repeat(2), 16).ok();
        return match h.len() {
            3 => Some(Color::from_rgba8(v1(0)?, v1(1)?, v1(2)?, 255)),
            4 => Some(Color::from_rgba8(v1(0)?, v1(1)?, v1(2)?, v1(3)?)),
            6 => Some(Color::from_rgba8(v(0)?, v(2)?, v(4)?, 255)),
            8 => Some(Color::from_rgba8(v(0)?, v(2)?, v(4)?, v(6)?)),
            _ => None,
        };
    }
    let lower = s.to_lowercase();
    if let Some(body) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")) {
        let body = body.trim_end_matches(')');
        let parts: Vec<f32> = body.split(',').filter_map(|p| p.trim().trim_end_matches('%').parse().ok()).collect();
        if parts.len() >= 3 {
            let a = parts.get(3).copied().unwrap_or(1.0);
            return Some(Color::from_rgba8(
                parts[0].clamp(0.0, 255.0) as u8,
                parts[1].clamp(0.0, 255.0) as u8,
                parts[2].clamp(0.0, 255.0) as u8,
                (a.clamp(0.0, 1.0) * 255.0).round() as u8,
            ));
        }
        return None;
    }
    let named = match lower.as_str() {
        "white" => (255, 255, 255, 255),
        "black" => (0, 0, 0, 255),
        "red" => (255, 0, 0, 255),
        "green" => (0, 128, 0, 255),
        "blue" => (0, 0, 255, 255),
        "yellow" => (255, 255, 0, 255),
        "gray" | "grey" => (128, 128, 128, 255),
        "orange" => (255, 165, 0, 255),
        "purple" => (128, 0, 128, 255),
        "transparent" | "none" => (0, 0, 0, 0),
        _ => return None,
    };
    Some(Color::from_rgba8(named.0, named.1, named.2, named.3))
}

fn with_opacity(mut c: Color, opacity: f32) -> Color {
    c.set_alpha((c.alpha() * opacity).clamp(0.0, 1.0));
    c
}

// ---------------------------------------------------------------------------------------------
// transforms and paths
// ---------------------------------------------------------------------------------------------

/// Konva node transform appended to `base`.
pub fn node_transform(base: Transform, e: &Element) -> Transform {
    let x = e.num_or("x", 0.0) as f32;
    let y = e.num_or("y", 0.0) as f32;
    let rot = e.num_or("rotation", 0.0) as f32;
    let sx = e.num_or("scaleX", 1.0) as f32;
    let sy = e.num_or("scaleY", 1.0) as f32;
    let kx = e.num_or("skewX", 0.0) as f32;
    let ky = e.num_or("skewY", 0.0) as f32;
    let ox = e.num_or("offsetX", 0.0) as f32;
    let oy = e.num_or("offsetY", 0.0) as f32;
    let mut t = base.pre_translate(x, y);
    if rot != 0.0 {
        t = t.pre_concat(Transform::from_rotate(rot));
    }
    if kx != 0.0 || ky != 0.0 {
        t = t.pre_concat(Transform::from_row(1.0, ky, kx, 1.0, 0.0, 0.0));
    }
    if sx != 1.0 || sy != 1.0 {
        t = t.pre_scale(sx, sy);
    }
    if ox != 0.0 || oy != 0.0 {
        t = t.pre_translate(-ox, -oy);
    }
    t
}

const KAPPA: f32 = 0.552_284_75;

pub fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, radii: [f32; 4]) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let max_r = (w / 2.0).min(h / 2.0);
    let [tl, tr, br, bl] = radii.map(|r| r.clamp(0.0, max_r));
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    if tr > 0.0 {
        pb.cubic_to(x + w - tr + tr * KAPPA, y, x + w, y + tr - tr * KAPPA, x + w, y + tr);
    }
    pb.line_to(x + w, y + h - br);
    if br > 0.0 {
        pb.cubic_to(x + w, y + h - br + br * KAPPA, x + w - br + br * KAPPA, y + h, x + w - br, y + h);
    }
    pb.line_to(x + bl, y + h);
    if bl > 0.0 {
        pb.cubic_to(x + bl - bl * KAPPA, y + h, x, y + h - bl + bl * KAPPA, x, y + h - bl);
    }
    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.cubic_to(x, y + tl - tl * KAPPA, x + tl - tl * KAPPA, y, x + tl, y);
    }
    pb.close();
    pb.finish()
}

fn corner_radii(e: &Element) -> [f32; 4] {
    match e.attrs.get("cornerRadius") {
        Some(serde_json::Value::Array(a)) => {
            let mut r = [0.0f32; 4];
            for (i, v) in a.iter().take(4).enumerate() {
                r[i] = v.as_f64().unwrap_or(0.0) as f32;
            }
            r
        }
        _ => [e.num_or("cornerRadius", 0.0) as f32; 4],
    }
}

// ---------------------------------------------------------------------------------------------
// renderer
// ---------------------------------------------------------------------------------------------

struct Ctx<'a> {
    pm: Pixmap,
    clip: Mask,
    base: Transform,
    fonts: &'a FontSet,
    opts: &'a RenderOptions,
    warnings: BTreeSet<String>,
}

pub fn render_side(card: &Card, back: bool, fonts: &FontSet, opts: &RenderOptions) -> Rendered {
    let (fw, fh) = card.frame_size();
    let k = opts.scale.max(0.05);
    let pw = ((fw as f32 * k).round() as u32).max(1);
    let ph = ((fh as f32 * k).round() as u32).max(1);
    let side = card.side(back);
    let base = Transform::from_scale(k, k);
    let mut clip = Mask::new(pw, ph).expect("mask");
    let radius = side.background.num_or("cornerRadius", 20.0).max(0.0) as f32;
    if let Some(p) = rounded_rect_path(0.0, 0.0, fw as f32, fh as f32, [radius; 4]) {
        clip.fill_path(&p, FillRule::Winding, true, base);
    }
    let mut ctx = Ctx {
        pm: Pixmap::new(pw, ph).expect("pixmap"),
        clip,
        base,
        fonts,
        opts,
        warnings: BTreeSet::new(),
    };

    // background
    let mut bg = side.background.clone();
    if !bg.attrs.contains_key("fill") && card.about.dark_card {
        bg.attrs.insert("fill".into(), "#000000".into());
    }
    if bg.visible() {
        let base_fill = if card.about.dark_card { "#000000" } else { "#ffffff" };
        if bg.str("fill").is_none() && !bg.has("base64image") {
            bg.attrs.insert("fill".into(), base_fill.into());
        }
        bg.attrs.insert("x".into(), 0.into());
        bg.attrs.insert("y".into(), 0.into());
        draw_rect(&mut ctx, &bg);
    }

    for e in &side.elements {
        if !e.visible() {
            continue;
        }
        ctx.draw_element(e);
    }
    Rendered { pixmap: ctx.pm, warnings: ctx.warnings.into_iter().collect() }
}

impl<'a> Ctx<'a> {
    fn warn(&mut self, s: String) {
        self.warnings.insert(s);
    }

    fn draw_element(&mut self, e: &Element) {
        let class = e.class_name();
        let name = e.name();
        match (class, name) {
            ("Text", _) | (_, "Text") => {
                if !self.opts.skip_text {
                    draw_text(self, e)
                }
            }
            ("Rect", _) => draw_rect(self, e),
            ("Image", "QRCode") => draw_qr(self, e),
            ("Image", _) => draw_image(self, e),
            ("Circle", _) | ("Ellipse", _) => draw_ellipse(self, e),
            ("Line", _) | ("Star", _) | ("RegularPolygon", _) => draw_poly(self, e),
            ("Group", _) => {
                self.warn("Groups are not supported; contents skipped".into());
            }
            (c, n) => {
                let label = if !n.is_empty() { n } else { c };
                self.warn(format!("Unsupported element type \"{label}\" was skipped"));
            }
        }
    }
}

// ---- fills ----------------------------------------------------------------------------------

enum Fill {
    None,
    Solid(Color),
    Linear { start: Point, end: Point, stops: Vec<GradientStop> },
    Radial { start: Point, end: Point, radius: f32, stops: Vec<GradientStop> },
    Pattern { pm: Pixmap, ts: Transform, spread: SpreadMode, opacity: f32 },
}

fn make_stops(stops: &[(f32, String)], opacity: f32) -> Option<Vec<GradientStop>> {
    let mut out = Vec::new();
    for (off, c) in stops {
        let col = with_opacity(parse_color(c)?, opacity);
        out.push(GradientStop::new(off.clamp(0.0, 1.0), col));
    }
    Some(out)
}

fn build_fill(ctx: &mut Ctx, e: &Element, w: f32, h: f32, opacity: f32) -> Fill {
    // Konva decides by `fillPriority`; the Hub-private `fillType` is only a hint when that is missing.
    let priority = e
        .str("fillPriority")
        .map(|s| s.to_string())
        .unwrap_or_else(|| match e.str("fillType") {
            Some("gradient") => "linear-gradient".into(),
            Some("radial") => "radial-gradient".into(),
            Some("image") => "pattern".into(),
            _ => "color".into(),
        });
    if e.str("fillType") == Some("none") || e.boolean("fillEnabled") == Some(false) {
        return Fill::None;
    }
    match priority.as_str() {
        "linear-gradient" => {
            if let Some(stops) = e.stops("fillLinearGradientColorStops").and_then(|s| make_stops(&s, opacity)) {
                return Fill::Linear {
                    start: Point::from_xy(
                        e.num_or("fillLinearGradientStartPointX", 0.0) as f32,
                        e.num_or("fillLinearGradientStartPointY", 0.0) as f32,
                    ),
                    end: Point::from_xy(
                        e.num_or("fillLinearGradientEndPointX", w as f64) as f32,
                        e.num_or("fillLinearGradientEndPointY", 0.0) as f32,
                    ),
                    stops,
                };
            }
        }
        "radial-gradient" => {
            if let Some(stops) = e.stops("fillRadialGradientColorStops").and_then(|s| make_stops(&s, opacity)) {
                if e.num_or("fillRadialGradientStartRadius", 0.0) > 0.0 {
                    ctx.warn("Radial gradient start radius is approximated as 0".into());
                }
                let sx = e.num_or("fillRadialGradientStartPointX", 0.0) as f32;
                let sy = e.num_or("fillRadialGradientStartPointY", 0.0) as f32;
                return Fill::Radial {
                    start: Point::from_xy(sx, sy),
                    end: Point::from_xy(
                        e.num_or("fillRadialGradientEndPointX", sx as f64) as f32,
                        e.num_or("fillRadialGradientEndPointY", sy as f64) as f32,
                    ),
                    radius: (e.num_or("fillRadialGradientEndRadius", (w.max(h) / 2.0) as f64) as f32).max(0.01),
                    stops,
                };
            }
        }
        "pattern" => {
            if let Some(pm) = e.str("base64image").and_then(|u| decode_to_pixmap(u, false)) {
                let mut ts = Transform::from_translate(
                    e.num_or("fillPatternX", 0.0) as f32,
                    e.num_or("fillPatternY", 0.0) as f32,
                );
                let rot = e.num_or("fillPatternRotation", 0.0) as f32;
                if rot != 0.0 {
                    ts = ts.pre_concat(Transform::from_rotate(rot));
                }
                ts = ts.pre_scale(e.num_or("fillPatternScaleX", 1.0) as f32, e.num_or("fillPatternScaleY", 1.0) as f32);
                let spread = match e.str("fillPatternRepeat") {
                    Some("no-repeat") => SpreadMode::Pad,
                    _ => SpreadMode::Repeat,
                };
                return Fill::Pattern { pm, ts, spread, opacity };
            }
        }
        _ => {}
    }
    match e.str("fill").and_then(parse_color) {
        Some(c) if c.alpha() > 0.0 => Fill::Solid(with_opacity(c, opacity)),
        _ => Fill::None,
    }
}

fn fill_path(ctx: &mut Ctx, path: &Path, fill: &Fill, ts: Transform) {
    let clip = ctx.clip.clone();
    fill_path_masked(&mut ctx.pm, &clip, path, fill, ts);
}

fn fill_path_masked(pm: &mut Pixmap, clip: &Mask, path: &Path, fill: &Fill, ts: Transform) {
    let mut paint = Paint::default();
    paint.anti_alias = true;
    match fill {
        Fill::None => return,
        Fill::Solid(c) => paint.set_color(*c),
        Fill::Linear { start, end, stops } => {
            match LinearGradient::new(*start, *end, stops.clone(), SpreadMode::Pad, Transform::identity()) {
                Some(s) => paint.shader = s,
                None => return,
            }
        }
        Fill::Radial { start, end, radius, stops } => {
            match RadialGradient::new(*start, *end, *radius, stops.clone(), SpreadMode::Pad, Transform::identity()) {
                Some(s) => paint.shader = s,
                None => return,
            }
        }
        Fill::Pattern { pm: pat, ts: pts, spread, opacity } => {
            paint.shader = Pattern::new(pat.as_ref(), *spread, FilterQuality::Bilinear, *opacity, *pts);
        }
    }
    pm.fill_path(path, &paint, FillRule::Winding, ts, Some(clip));
}

struct StrokeSpec {
    color: Color,
    width: f32,
    cap: LineCap,
    join: LineJoin,
    dash: Option<Vec<f32>>,
}

fn stroke_spec(e: &Element, opacity: f32) -> Option<StrokeSpec> {
    if e.boolean("strokeEnabled") == Some(false) {
        return None;
    }
    let width = e.num_or("strokeWidth", 0.0) as f32;
    if width <= 0.0 {
        return None;
    }
    let color = with_opacity(parse_color(e.str("stroke")?)?, opacity);
    if color.alpha() <= 0.0 {
        return None;
    }
    let cap = match e.str("lineCap") {
        Some("round") => LineCap::Round,
        Some("square") => LineCap::Square,
        _ => LineCap::Butt,
    };
    let join = match e.str("lineJoin") {
        Some("round") => LineJoin::Round,
        Some("bevel") => LineJoin::Bevel,
        _ => LineJoin::Miter,
    };
    let dash = e.num_array("dash").filter(|d| d.len() >= 2).map(|d| d.iter().map(|v| *v as f32).collect());
    Some(StrokeSpec { color, width, cap, join, dash })
}

fn stroke_path(ctx: &mut Ctx, path: &Path, s: &StrokeSpec, ts: Transform) {
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color(s.color);
    let mut st = Stroke { width: s.width, line_cap: s.cap, line_join: s.join, ..Default::default() };
    if let Some(d) = &s.dash {
        let mut arr = d.clone();
        if arr.len() % 2 == 1 {
            arr.extend(d.clone());
        }
        st.dash = StrokeDash::new(arr, 0.0);
    }
    let clip = ctx.clip.clone();
    ctx.pm.stroke_path(path, &paint, &st, ts, Some(&clip));
}

fn opacity_of(e: &Element) -> f32 {
    e.num_or("opacity", 1.0).clamp(0.0, 1.0) as f32
}

// ---- shapes ---------------------------------------------------------------------------------

fn draw_rect(ctx: &mut Ctx, e: &Element) {
    let w = e.num_or("width", 0.0) as f32;
    let h = e.num_or("height", 0.0) as f32;
    let ts = node_transform(ctx.base, e);
    let op = opacity_of(e);
    let Some(path) = rounded_rect_path(0.0, 0.0, w, h, corner_radii(e)) else { return };
    let fill = build_fill(ctx, e, w, h, op);
    let stroke = stroke_spec(e, op);
    let after = e.boolean("fillAfterStrokeEnabled") == Some(true);
    if after {
        if let Some(s) = &stroke {
            stroke_path(ctx, &path, s, ts);
        }
        fill_path(ctx, &path, &fill, ts);
    } else {
        fill_path(ctx, &path, &fill, ts);
        if let Some(s) = &stroke {
            stroke_path(ctx, &path, s, ts);
        }
    }
}

fn draw_ellipse(ctx: &mut Ctx, e: &Element) {
    let (rx, ry) = if e.class_name() == "Ellipse" {
        (e.num_or("radiusX", e.num_or("radius", 0.0)) as f32, e.num_or("radiusY", e.num_or("radius", 0.0)) as f32)
    } else {
        let r = e.num_or("radius", 0.0) as f32;
        (r, r)
    };
    let Some(rect) = Rect::from_xywh(-rx, -ry, rx * 2.0, ry * 2.0) else { return };
    let Some(path) = PathBuilder::from_oval(rect) else { return };
    let ts = node_transform(ctx.base, e);
    let op = opacity_of(e);
    let fill = build_fill(ctx, e, rx * 2.0, ry * 2.0, op);
    fill_path(ctx, &path, &fill, ts);
    if let Some(s) = stroke_spec(e, op) {
        stroke_path(ctx, &path, &s, ts);
    }
}

fn draw_poly(ctx: &mut Ctx, e: &Element) {
    let mut pts: Vec<(f32, f32)> = Vec::new();
    match e.class_name() {
        "Star" => {
            let n = e.num_or("numPoints", 5.0).max(2.0) as usize;
            let ir = e.num_or("innerRadius", 0.0) as f32;
            let or = e.num_or("outerRadius", 0.0) as f32;
            for i in 0..n * 2 {
                let r = if i % 2 == 0 { or } else { ir };
                let a = i as f32 * PI / n as f32;
                pts.push((r * a.sin(), -r * a.cos()));
            }
        }
        "RegularPolygon" => {
            let n = e.num_or("sides", 3.0).max(3.0) as usize;
            let r = e.num_or("radius", 0.0) as f32;
            for i in 0..n {
                let a = i as f32 * 2.0 * PI / n as f32;
                pts.push((r * a.sin(), -r * a.cos()));
            }
        }
        _ => {
            if let Some(p) = e.num_array("points") {
                for c in p.chunks(2) {
                    if c.len() == 2 {
                        pts.push((c[0] as f32, c[1] as f32));
                    }
                }
            }
            if e.num_or("tension", 0.0) > 0.0 {
                ctx.warn("Line tension (curves) is drawn as straight segments".into());
            }
        }
    }
    if pts.len() < 2 {
        return;
    }
    let closed = e.class_name() != "Line" || e.boolean("closed") == Some(true);
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    if closed {
        pb.close();
    }
    let Some(path) = pb.finish() else { return };
    let ts = node_transform(ctx.base, e);
    let op = opacity_of(e);
    if closed {
        let fill = build_fill(ctx, e, 0.0, 0.0, op);
        fill_path(ctx, &path, &fill, ts);
    }
    if let Some(s) = stroke_spec(e, op) {
        stroke_path(ctx, &path, &s, ts);
    }
}

// ---- images ---------------------------------------------------------------------------------

/// Decodes a `data:` URL (PNG/JPEG) into a premultiplied pixmap. `nudge_black` maps (0,0,0) to (1,1,1).
pub fn decode_to_pixmap(url: &str, nudge_black: bool) -> Option<Pixmap> {
    let bytes = crate::decode_data_url(url).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    rgba_to_pixmap(img, nudge_black)
}

fn rgba_to_pixmap(mut img: image::RgbaImage, nudge_black: bool) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut pm = Pixmap::new(w, h)?;
    if nudge_black {
        for p in img.pixels_mut() {
            if p.0[3] > 0 && p.0[0] == 0 && p.0[1] == 0 && p.0[2] == 0 {
                p.0[0] = 1;
                p.0[1] = 1;
                p.0[2] = 1;
            }
        }
    }
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pm)
}

/// Resizes a premultiplied pixmap with a high-quality filter (used when shrinking icons and photos).
fn resize_pixmap(src: &Pixmap, w: u32, h: u32) -> Option<Pixmap> {
    // image's resize works on straight alpha; premultiplied data is resized as plain RGBA instead,
    // which is the correct way to filter alpha images.
    let img = image::RgbaImage::from_raw(src.width(), src.height(), src.data().to_vec())?;
    let out = image::imageops::resize(&img, w.max(1), h.max(1), image::imageops::FilterType::Lanczos3);
    let mut pm = Pixmap::new(out.width(), out.height())?;
    for (dst, s) in pm.pixels_mut().iter_mut().zip(out.pixels()) {
        let [r, g, b, a] = s.0;
        // Lanczos overshoot can break the premultiplied invariant; clamp channels to alpha.
        let r = r.min(a);
        let g = g.min(a);
        let b = b.min(a);
        *dst = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, a)?;
    }
    Some(pm)
}

fn crop_pixmap(src: &Pixmap, x: u32, y: u32, w: u32, h: u32) -> Option<Pixmap> {
    let rect = tiny_skia::IntRect::from_xywh(x as i32, y as i32, w, h)?;
    src.clone_rect(rect)
}

#[derive(Clone, Copy, PartialEq)]
enum ImgMode {
    /// Photos and icons: area filter when shrinking, bicubic/bilinear otherwise.
    Smooth,
    /// Barcodes: never filtered.
    Nearest,
}

fn draw_image_pixmap(ctx: &mut Ctx, e: &Element, src: Pixmap, mode: ImgMode) {
    let smooth = mode == ImgMode::Smooth;
    let (iw, ih) = (src.width() as f32, src.height() as f32);
    // crop
    let (mut cx, mut cy, mut cw, mut ch) = (
        e.num_or("cropX", 0.0) as f32,
        e.num_or("cropY", 0.0) as f32,
        e.num_or("cropWidth", 0.0) as f32,
        e.num_or("cropHeight", 0.0) as f32,
    );
    if let Some(serde_json::Value::Object(c)) = e.attrs.get("crop") {
        let g = |k: &str| c.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        cx = g("x");
        cy = g("y");
        cw = g("width");
        ch = g("height");
    }
    let mut pm = src;
    if cw > 0.0 && ch > 0.0 {
        let x0 = cx.clamp(0.0, iw - 1.0) as u32;
        let y0 = cy.clamp(0.0, ih - 1.0) as u32;
        let w0 = (cw as u32).clamp(1, pm.width() - x0);
        let h0 = (ch as u32).clamp(1, pm.height() - y0);
        if let Some(c) = crop_pixmap(&pm, x0, y0, w0, h0) {
            pm = c;
        }
    }
    let w = e.num_or("width", pm.width() as f64) as f32;
    let h = e.num_or("height", pm.height() as f64) as f32;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let ts = node_transform(ctx.base, e);
    // effective device scale per axis
    let sx = ((ts.sx * ts.sx + ts.ky * ts.ky).sqrt()) * (w / pm.width() as f32);
    let sy = ((ts.kx * ts.kx + ts.sy * ts.sy).sqrt()) * (h / pm.height() as f32);
    if smooth && (sx < 0.8 || sy < 0.8) {
        let tw = ((pm.width() as f32 * sx.min(1.0)).round() as u32).max(1);
        let th = ((pm.height() as f32 * sy.min(1.0)).round() as u32).max(1);
        if let Some(r) = resize_pixmap(&pm, tw, th) {
            pm = r;
        }
    }
    let local = Transform::from_scale(w / pm.width() as f32, h / pm.height() as f32);
    let quality = if !smooth {
        FilterQuality::Nearest
    } else if sx > 1.05 || sy > 1.05 {
        FilterQuality::Bicubic
    } else {
        FilterQuality::Bilinear
    };
    let op = opacity_of(e);
    let Some(rect_path) = rounded_rect_path(0.0, 0.0, w, h, corner_radii(e)) else { return };
    let shader = Pattern::new(pm.as_ref(), SpreadMode::Pad, quality, op, local);
    let mut paint = Paint::default();
    paint.shader = shader;
    paint.anti_alias = true;
    ctx.pm.fill_path(&rect_path, &paint, FillRule::Winding, ts, Some(&ctx.clip));
    if let Some(s) = stroke_spec(e, op) {
        stroke_path(ctx, &rect_path, &s, ts);
    }
}

fn draw_image(ctx: &mut Ctx, e: &Element) {
    let is_barcode = e.name() == "Barcode";
    let Some(url) = e.str("base64image").filter(|s| !s.is_empty()) else {
        ctx.warn("Image element without embedded pixels was skipped".into());
        return;
    };
    let nudge = ctx.opts.prevent_resin_for_images && !is_barcode && e.boolean("monochrome") != Some(true);
    match decode_to_pixmap(url, nudge) {
        Some(pm) => draw_image_pixmap(ctx, e, pm, if is_barcode { ImgMode::Nearest } else { ImgMode::Smooth }),
        None => ctx.warn("An image could not be decoded and was skipped".into()),
    }
}

fn draw_qr(ctx: &mut Ctx, e: &Element) {
    // Prefer the stored bitmap (that is what Hub prints); regenerate from qrData if it is unusable.
    if let Some(url) = e.str("base64image").filter(|s| !s.is_empty()) {
        if let Some(pm) = decode_to_pixmap(url, false) {
            // Nearest neighbour, like Hub's print path (smoothing is disabled for QR codes there).
            draw_image_pixmap(ctx, e, pm, ImgMode::Nearest);
            return;
        }
    }
    let data = e.str("qrData").filter(|s| !s.is_empty()).unwrap_or("123456789");
    match crate::qr::qr_pixmap(data) {
        Some(pm) => {
            ctx.warn("QR code bitmap missing; regenerated from its data".into());
            draw_image_pixmap(ctx, e, pm, ImgMode::Nearest)
        }
        None => ctx.warn("QR code could not be rendered".into()),
    }
}

// ---- text -----------------------------------------------------------------------------------

struct TextStyle {
    size: f32,
    letter_spacing: f32,
    /// Horizontal squeeze applied to advances and outlines (font substitution / fit-to-box).
    xs: f32,
}

/// Konva's line breaking for `wrap` = word | char | none (see `Text._setTextData`).
fn layout_lines(chain: &FaceChain, text: &str, st: &TextStyle, width: Option<f32>, wrap: &str, max_h: Option<f32>, lh_px: f32) -> (Vec<String>, bool) {
    let measure = |s: &str| chain.measure(s, st.size, st.letter_spacing) * st.xs;
    let mut truncated = false;
    let should_wrap = wrap != "none" && width.is_some();
    let max_w = width.unwrap_or(f32::MAX);
    let mut out: Vec<String> = Vec::new();
    let mut cur_h = 0.0f32;
    let height_exceeded = |cur_h: f32| max_h.map(|m| cur_h + lh_px > m + 0.001).unwrap_or(false);
    let n_paras = text.split('\n').count();
    'paras: for (para_idx, para) in text.split('\n').enumerate() {
        let mut line: Vec<char> = para.chars().collect();
        if !should_wrap {
            out.push(para.to_string());
            cur_h += lh_px;
            if height_exceeded(cur_h) {
                { truncated = para_idx + 1 < n_paras; break 'paras; }
            }
            continue;
        }
        if line.is_empty() || measure(para) <= max_w {
            out.push(para.to_string());
            cur_h += lh_px;
            if height_exceeded(cur_h) {
                { truncated = para_idx + 1 < n_paras; break 'paras; }
            }
            continue;
        }
        while !line.is_empty() {
            // largest prefix that fits
            let (mut low, mut high) = (0usize, line.len());
            let mut matched: Option<usize> = None;
            while low < high {
                let mid = (low + high) / 2;
                let sub: String = line[..=mid].iter().collect();
                if measure(&sub) <= max_w {
                    low = mid + 1;
                    matched = Some(mid + 1);
                } else {
                    high = mid;
                }
            }
            let Some(mut mlen) = matched else {
                // not even one char fits: emit it anyway to avoid an endless loop (Konva stops here)
                break;
            };
            if wrap == "word" {
                let next = line.get(mlen).copied();
                let wrap_idx = if matches!(next, Some(' ') | Some('-')) {
                    mlen
                } else {
                    let prefix = &line[..mlen];
                    prefix
                        .iter()
                        .rposition(|c| *c == ' ' || *c == '-')
                        .map(|i| i + 1)
                        .unwrap_or(0)
                };
                if wrap_idx > 0 {
                    mlen = wrap_idx;
                }
            }
            let piece: String = line[..mlen].iter().collect::<String>().trim_end().to_string();
            out.push(piece);
            cur_h += lh_px;
            let rest: String = line[mlen..].iter().collect::<String>().trim_start().to_string();
            line = rest.chars().collect();
            if height_exceeded(cur_h) {
                { truncated = !line.is_empty() || para_idx + 1 < n_paras; break 'paras; }
            }
            if !line.is_empty() {
                let s: String = line.iter().collect();
                if measure(&s) <= max_w {
                    out.push(s);
                    cur_h += lh_px;
                    if height_exceeded(cur_h) {
                        { truncated = para_idx + 1 < n_paras; break 'paras; }
                    }
                    break;
                }
            }
        }
    }
    (out, truncated)
}

fn draw_text(ctx: &mut Ctx, e: &Element) {
    let text = e.str("text").unwrap_or("");
    if text.is_empty() {
        return;
    }
    let size = e.num_or("fontSize", 12.0) as f32;
    if size <= 0.0 {
        return;
    }
    let family = e.str("fontFamily").unwrap_or("Arial");
    let style = e.str("fontStyle").unwrap_or("").to_lowercase();
    let bold = style.contains("bold");
    let italic = style.contains("italic");
    let Some(chain) = ctx.fonts.chain(family, bold, italic) else {
        ctx.warn("No fonts available to draw text".into());
        return;
    };
    let used = chain.primary().family.clone();
    if !used.eq_ignore_ascii_case(family.trim()) {
        ctx.warn(format!("Font \"{family}\" is not installed; using \"{used}\""));
    }
    let padding = e.num_or("padding", 0.0) as f32;
    let line_height = e.num_or("lineHeight", 1.0) as f32;
    let lh_px = line_height * size;
    let width_attr = e.num("width").filter(|w| *w > 0.0).map(|w| w as f32);
    let height_attr = e.num("height").filter(|h| *h > 0.0).map(|h| h as f32);
    let wrap = e.str("wrap").unwrap_or("word");
    let inner_w = width_attr.map(|w| w - padding * 2.0);
    let inner_h = height_attr.map(|h| h - padding * 2.0);
    let substituted = !used.eq_ignore_ascii_case(family.trim());
    let base_xs = if substituted { crate::fonts::substitute_x_scale(family) } else { 1.0 };
    let mut st = TextStyle { size, letter_spacing: e.num_or("letterSpacing", 0.0) as f32, xs: base_xs };
    let (mut lines, mut truncated) = layout_lines(&chain, text, &st, inner_w, wrap, inner_h, lh_px);
    if substituted {
        // The design was laid out with a narrower/wider font than the one we have. Hub fitted the text to
        // its box, so squeeze the substitute horizontally (down to 75%) when it would not fit.
        let autosize = e.boolean("autoSize") == Some(true);
        let widest = |lines: &Vec<String>, st: &TextStyle| lines.iter().map(|l| chain.measure(l, size, st.letter_spacing) * st.xs).fold(0.0f32, f32::max);
        if let Some(iw) = inner_w {
            if autosize && widest(&lines, &st) > iw {
                st.xs = (st.xs * iw / widest(&lines, &st)).max(base_xs * 0.75);
                (lines, truncated) = layout_lines(&chain, text, &st, inner_w, wrap, inner_h, lh_px);
            }
        }
        if truncated {
            let mut f = 0.95;
            while truncated && f >= 0.74 {
                st.xs = base_xs * f;
                (lines, truncated) = layout_lines(&chain, text, &st, inner_w, wrap, inner_h, lh_px);
                f -= 0.05;
            }
        }
    }
    if truncated {
        // Hub never truncates its own layout, so this is a substitution artefact: show everything.
        st.xs = base_xs.min(st.xs);
        (lines, _) = layout_lines(&chain, text, &st, inner_w, wrap, None, lh_px);
    }
    if lines.is_empty() {
        return;
    }
    let mut shaped: Vec<_> = lines.iter().map(|l| chain.shape(l, size, st.letter_spacing)).collect();
    for sl in shaped.iter_mut() {
        sl.width *= st.xs;
    }
    let text_w = shaped.iter().map(|s| s.width).fold(0.0f32, f32::max);
    let box_w = width_attr.unwrap_or(text_w + padding * 2.0);
    let box_h = height_attr.unwrap_or(lines.len() as f32 * lh_px + padding * 2.0);
    let valign = e.str("verticalAlign").unwrap_or("top");
    let align = e.str("align").unwrap_or("left");
    let block_h = lines.len() as f32 * lh_px;
    let align_y = match valign {
        "middle" => (box_h - block_h - padding * 2.0) / 2.0,
        "bottom" => box_h - block_h - padding * 2.0,
        _ => 0.0,
    };
    let (asc, desc) = chain.primary().metrics_em();
    // textBaseline = "middle": baseline sits (ascent - descent)/2 below the line-box centre.
    let mid_offset = (asc - desc) / 2.0 * size;
    let ts = node_transform(ctx.base, e);
    let op = opacity_of(e);
    let fill_col = e.str("fill").and_then(parse_color).unwrap_or(Color::BLACK);
    let fill_col = with_opacity(fill_col, op);
    let stroke = stroke_spec(e, op);
    let decoration = e.str("textDecoration").unwrap_or("");
    let upem0 = chain.primary().units_per_em();
    let _ = upem0;

    let mut pb = PathBuilder::new();
    let mut deco = PathBuilder::new();
    for (n, sl) in shaped.iter().enumerate() {
        let lx = padding
            + match align {
                "right" => box_w - sl.width - padding * 2.0,
                "center" => (box_w - sl.width - padding * 2.0) / 2.0,
                _ => 0.0,
            };
        let base_y = padding + align_y + n as f32 * lh_px + lh_px / 2.0 + mid_offset;
        for g in &sl.glyphs {
            let face = &chain.faces[g.face];
            if let Some(path) = face.outline(g.gid) {
                let s = size / face.units_per_em();
                let t = Transform::from_row(s * st.xs, 0.0, 0.0, -s, lx + g.x * st.xs, base_y + g.y);
                if let Some(p) = path.transform(t) {
                    pb.push_path(&p);
                }
            }
        }
        if decoration.contains("underline") {
            if let Some(r) = Rect::from_xywh(lx, base_y + size * 0.1, sl.width, (size / 16.0).max(1.0)) {
                deco.push_rect(r);
            }
        }
        if decoration.contains("line-through") {
            if let Some(r) = Rect::from_xywh(lx, base_y - size * 0.3, sl.width, (size / 16.0).max(1.0)) {
                deco.push_rect(r);
            }
        }
    }
    if let Some(path) = pb.finish() {
        fill_path(ctx, &path, &Fill::Solid(fill_col), ts);
        if let Some(s) = &stroke {
            stroke_path(ctx, &path, s, ts);
        }
    }
    if let Some(path) = deco.finish() {
        fill_path(ctx, &path, &Fill::Solid(fill_col), ts);
    }
}

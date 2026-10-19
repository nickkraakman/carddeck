//! Print job building for the Magicard CUPS driver.
//!
//! Pipeline: render each side (1013x642) -> turn into the driver's portrait raster page
//! (638x1013, `PageSize=1XCR80`) -> write a lossless PDF (one image per page, odd page = front,
//! even page = back) -> `lp -d <printer> -o <PPD options> job.pdf`.
//!
//! The driver option names come from the GPL Magicard driver's PPDs (`pro300.ppd`). Orientation of
//! the raster page is derived from reading the driver source but is **unverified on hardware**, so
//! it is a visible setting (see README: test-print procedure).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use image::{ImageFormat, RgbImage};
use image::imageops;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

use crate::colour::{self, Adjustments, Correction, Stages};
use crate::fonts::FontSet;
use crate::render::{render_side, RenderOptions};
use crate::{Card, Error};

/// Driver raster page, pixels at 300 dpi (PPD `1XCR80` = 153 x 243 pt).
pub const PAGE_W: u32 = 638;
pub const PAGE_H: u32 = 1013;
pub const PAGE_W_PT: f32 = 153.0;
pub const PAGE_H_PT: f32 = 243.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sides {
    Front,
    Both,
    Back,
}

impl Sides {
    pub fn ppd(self) -> &'static str {
        match self {
            Sides::Front => "0Front",
            Sides::Both => "1Both",
            Sides::Back => "2Back",
        }
    }
}

/// Direction in which the landscape card bitmap is turned to fit the portrait raster page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageTurn {
    /// Top edge of the card becomes the left edge of the page. Matches our reading of the driver. UNVERIFIED.
    Ccw90,
    Cw90,
}

/// How the 642 px card width is fitted into the 638 px wide page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageFit {
    /// Cut 2 px off each long edge (no resampling, pure black stays pure black).
    Crop,
    /// Resample 642 -> 638 (whole card visible, ~0.6 percent squeeze, blurs 1px black edges).
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlackMode {
    /// YMCK: pure #000000 pixels print on the K resin panel (`CFColourFormat=1ColourYMCK`, `CFBlackResin=0BlackResinK`).
    ResinK,
    /// YMC only, black is composite (`0ColourYMC` + `1BlackResinYMC`). Never uses the resin panel.
    CompositeYmc,
    /// Single K panel only, monochrome (`2MonoK`).
    MonoK,
}

#[derive(Clone, Debug)]
pub struct PrintOptions {
    pub printer: Option<String>,
    pub sides: Sides,
    pub copies: u32,
    pub turn: PageTurn,
    pub fit: PageFit,
    /// Extra 180 degree turn of the bitmap (visible in the saved job) for the front / back page.
    pub rotate_front_180: bool,
    pub rotate_back_180: bool,
    pub overcoat_front: bool,
    pub overcoat_back: bool,
    pub black: BlackMode,
    /// Photo black nudged to (1,1,1) so photos never use resin.
    pub prevent_resin_for_images: bool,
    /// Extra raw `-o name=value` pairs (advanced: e.g. `CFRotation=1Rotation`).
    pub extra_options: Vec<(String, String)>,
    /// Colour correction chosen for the card. It is sent to the driver as the matching PPD option
    /// `ColourCorrection` (see [`Correction::ppd_choice`]); the print bitmap itself is NOT corrected,
    /// so the driver is the only one doing it (no double correction). `None` = send no option.
    pub colour_correction: Option<Correction>,
    /// The card's current colour settings (sliders + mode), used for warnings and the optional bake.
    pub adjustments: Option<Adjustments>,
    /// Opt-in: bake the parts the driver cannot do (Brightness/Contrast/Saturation/RGB sliders and
    /// Gamma 1/2) into the print bitmap. LUT modes are never baked. Default off.
    pub bake_adjustments: bool,
}

impl PrintOptions {
    /// Takes the colour settings of the card: sends the matching driver option, and (if `bake`) also
    /// bakes the non-driver parts into the bitmap.
    pub fn use_colour(&mut self, adj: &Adjustments, bake: bool) {
        self.colour_correction = Some(adj.correction);
        self.adjustments = Some(*adj);
        self.bake_adjustments = bake;
    }
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            printer: None,
            sides: Sides::Both,
            copies: 1,
            turn: PageTurn::Ccw90,
            fit: PageFit::Crop,
            rotate_front_180: false,
            rotate_back_180: false,
            overcoat_front: true,
            overcoat_back: true,
            black: BlackMode::ResinK,
            prevent_resin_for_images: true,
            extra_options: Vec::new(),
            colour_correction: None,
            adjustments: None,
            bake_adjustments: false,
        }
    }
}

pub struct JobPage {
    pub label: &'static str,
    /// Exactly PAGE_W x PAGE_H.
    pub rgb: RgbImage,
}

pub struct Job {
    pub pages: Vec<JobPage>,
    pub warnings: Vec<String>,
}

/// Turns a rendered card side (design frame orientation) into the driver's portrait raster page.
pub fn to_page(card: &Card, side_rgb: &RgbImage, back: bool, o: &PrintOptions) -> RgbImage {
    // Hub always hands the printer a landscape bitmap; portrait designs are turned first
    // (front -90 degrees, back +90 degrees, like Hub's own `portraitToLandscape`).
    let landscape = if card.about.portrait || side_rgb.height() > side_rgb.width() {
        if back {
            imageops::rotate90(side_rgb)
        } else {
            imageops::rotate270(side_rgb)
        }
    } else {
        side_rgb.clone()
    };
    let mut page = match o.turn {
        PageTurn::Ccw90 => imageops::rotate270(&landscape),
        PageTurn::Cw90 => imageops::rotate90(&landscape),
    };
    if (back && o.rotate_back_180) || (!back && o.rotate_front_180) {
        page = imageops::rotate180(&page);
    }
    fit_to_page(page, o.fit)
}

fn fit_to_page(page: RgbImage, fit: PageFit) -> RgbImage {
    let (w, h) = page.dimensions();
    if (w, h) == (PAGE_W, PAGE_H) {
        return page;
    }
    match fit {
        PageFit::Crop if w >= PAGE_W && h >= PAGE_H => {
            let x = (w - PAGE_W) / 2;
            let y = (h - PAGE_H) / 2;
            imageops::crop_imm(&page, x, y, PAGE_W, PAGE_H).to_image()
        }
        _ => imageops::resize(&page, PAGE_W, PAGE_H, imageops::FilterType::Triangle),
    }
}

pub fn build_job(card: &Card, fonts: &FontSet, o: &PrintOptions) -> Result<Job, Error> {
    let mut warnings = Vec::new();
    let ropts = RenderOptions { scale: 1.0, prevent_resin_for_images: o.prevent_resin_for_images, skip_text: false };
    let want_front = matches!(o.sides, Sides::Front | Sides::Both);
    let want_back = matches!(o.sides, Sides::Back | Sides::Both);
    let mut singles: Vec<JobPage> = Vec::new();
    warnings.extend(colour_warnings(o));
    let bake = |rgb: &mut RgbImage| {
        if let (true, Some(adj)) = (o.bake_adjustments, &o.adjustments) {
            colour::apply_to_buffer(rgb.as_mut(), 3, adj, Stages::NonDriver);
        }
    };
    if want_front {
        let r = render_side(card, false, fonts, &ropts);
        warnings.extend(r.warnings.iter().map(|w| format!("front: {w}")));
        let mut rgb = r.to_rgb_over_white();
        bake(&mut rgb);
        singles.push(JobPage { label: "front", rgb: to_page(card, &rgb, false, o) });
    }
    if want_back {
        if !card.about.double_sided && o.sides == Sides::Both {
            warnings.push("This card is not marked double-sided, but both sides were requested.".into());
        }
        let r = render_side(card, true, fonts, &ropts);
        warnings.extend(r.warnings.iter().map(|w| format!("back: {w}")));
        let mut rgb = r.to_rgb_over_white();
        bake(&mut rgb);
        singles.push(JobPage { label: "back", rgb: to_page(card, &rgb, true, o) });
    }
    if singles.is_empty() {
        return Err(Error::Print("nothing to print".into()));
    }
    // Copies are expanded into the page list (front, back, front, back...) so that duplex pairing
    // stays correct regardless of how the spooler collates.
    let mut pages = Vec::new();
    for _ in 0..o.copies.max(1) {
        for p in &singles {
            pages.push(JobPage { label: p.label, rgb: p.rgb.clone() });
        }
    }
    warnings.sort();
    warnings.dedup();
    Ok(Job { pages, warnings })
}

/// Explains what the printer will and will not do about colour for these options.
fn colour_warnings(o: &PrintOptions) -> Vec<String> {
    let mut w = Vec::new();
    let Some(c) = o.colour_correction else { return w };
    let baked = o.bake_adjustments;
    if matches!(c, Correction::Gamma1 | Correction::Gamma2) {
        if baked {
            w.push(format!("{} has no driver option: it is baked into the print bitmap and the driver gets ColourCorrection=None (no double correction).", c.label()));
        } else {
            w.push(format!(
                "{} has no exact Magicard driver option: the printer gets the closest one, ColourCorrection=Driver managed, so the print will not match the on-screen preview exactly. Bake the preview curve into the bitmap to print that curve instead (the app always does this; the CLI needs --bake-adjustments).",
                c.label()
            ));
        }
    }
    if o.adjustments.map(|a| !a.sliders_neutral()).unwrap_or(false) {
        if baked {
            w.push("Brightness/Contrast/Saturation/RGB sliders are baked into the print bitmap; pure black text may no longer be exactly #000000 (resin).".into());
        } else {
            w.push("Brightness/Contrast/Saturation/RGB sliders are preview only: the driver has no such options and the print bitmap is not changed.".into());
        }
    }
    w
}

/// Lossless PDF: one page per bitmap, page size 153 x 243 pt, image filling the page, DeviceRGB, no interpolation.
pub fn write_pdf(job: &Job) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    pdf.catalog(catalog).pages(tree);
    let n = job.pages.len() as i32;
    let page_refs: Vec<Ref> = (0..n).map(|i| Ref::new(10 + i * 3)).collect();
    pdf.pages(tree).kids(page_refs.iter().copied()).count(n);
    for (i, p) in job.pages.iter().enumerate() {
        let page_id = Ref::new(10 + i as i32 * 3);
        let content_id = Ref::new(11 + i as i32 * 3);
        let image_id = Ref::new(12 + i as i32 * 3);
        let mut page = pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, PAGE_W_PT, PAGE_H_PT));
        page.parent(tree);
        page.contents(content_id);
        page.resources().x_objects().pair(Name(b"Im0"), image_id);
        page.finish();

        let mut c = Content::new();
        c.save_state();
        c.transform([PAGE_W_PT, 0.0, 0.0, PAGE_H_PT, 0.0, 0.0]);
        c.x_object(Name(b"Im0"));
        c.restore_state();
        pdf.stream(content_id, &c.finish());

        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(p.rgb.as_raw(), 6);
        let mut img = pdf.image_xobject(image_id, &compressed);
        img.filter(Filter::FlateDecode);
        img.width(p.rgb.width() as i32);
        img.height(p.rgb.height() as i32);
        img.color_space().device_rgb();
        img.bits_per_component(8);
        img.interpolate(false);
        img.finish();
    }
    pdf.finish()
}

/// PPD options for `lp`, in a stable order. Names come from the Magicard PPDs.
pub fn ppd_options(o: &PrintOptions) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![
        ("PageSize".into(), "1XCR80".into()),
        ("UTDuplex".into(), o.sides.ppd().into()),
    ];
    let (fmt, resin) = match o.black {
        BlackMode::ResinK => ("1ColourYMCK", "0BlackResinK"),
        BlackMode::CompositeYmc => ("0ColourYMC", "1BlackResinYMC"),
        BlackMode::MonoK => ("2MonoK", "0BlackResinK"),
    };
    for side in ["CF", "CB"] {
        v.push((format!("{side}ColourFormat"), fmt.into()));
        v.push((format!("{side}BlackResin"), resin.into()));
    }
    v.push(("CFOverCoat".into(), if o.overcoat_front { "1PrintOvercoat" } else { "0PrintNoOvercoat" }.into()));
    v.push(("CBOverCoat".into(), if o.overcoat_back { "1PrintOvercoat" } else { "0PrintNoOvercoat" }.into()));
    if let Some(c) = o.colour_correction {
        let choice = if o.bake_adjustments && o.adjustments.is_some() { c.ppd_choice_when_baked() } else { c.ppd_choice() };
        v.push(("ColourCorrection".into(), choice.into()));
    }
    v.push(("print-scaling".into(), "none".into()));
    for (k, val) in &o.extra_options {
        if let Some(slot) = v.iter_mut().find(|(n, _)| n == k) {
            slot.1 = val.clone();
        } else {
            v.push((k.clone(), val.clone()));
        }
    }
    v
}

/// The `lp` argument vector (without the program name).
pub fn lp_args(pdf: &Path, o: &PrintOptions) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(p) = &o.printer {
        a.push("-d".into());
        a.push(p.clone());
    }
    a.push("-n".into());
    a.push("1".into()); // copies are already expanded into the PDF
    a.push("-t".into());
    a.push("CardDeck".into());
    for (k, v) in ppd_options(o) {
        a.push("-o".into());
        a.push(format!("{k}={v}"));
    }
    a.push(pdf.display().to_string());
    a
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_=./:@".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn lp_command_line(pdf: &Path, o: &PrintOptions) -> String {
    let mut s = String::from("lp");
    for a in lp_args(pdf, o) {
        s.push(' ');
        s.push_str(&shell_quote(&a));
    }
    s
}

/// Dry run: writes `job.pdf`, `page-N-<side>.png` and `command.txt` into `dir`.
pub fn save_job(dir: &Path, job: &Job, o: &PrintOptions) -> Result<Vec<PathBuf>, Error> {
    std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
    let mut files = Vec::new();
    let pdf_path = dir.join("job.pdf");
    std::fs::write(&pdf_path, write_pdf(job)).map_err(|e| Error::Io(e.to_string()))?;
    files.push(pdf_path.clone());
    for (i, p) in job.pages.iter().enumerate() {
        let path = dir.join(format!("page-{}-{}.png", i + 1, p.label));
        p.rgb.save(&path).map_err(|e| Error::Io(e.to_string()))?;
        files.push(path);
    }
    let cmd = dir.join("command.txt");
    let abs = pdf_path.canonicalize().unwrap_or(pdf_path);
    std::fs::write(&cmd, format!("{}\n", lp_command_line(&abs, o))).map_err(|e| Error::Io(e.to_string()))?;
    files.push(cmd);
    Ok(files)
}

// ---------------------------------------------------------------------------------------------
// CUPS queries (pure parsing; executing is up to the caller)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Printer {
    pub name: String,
    pub status: String,
    pub enabled: bool,
}

/// Parses `lpstat -p` output.
pub fn parse_lpstat_p(out: &str) -> Vec<Printer> {
    let mut v = Vec::new();
    for line in out.lines() {
        let Some(rest) = line.strip_prefix("printer ") else { continue };
        let mut it = rest.splitn(2, ' ');
        let name = it.next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let status = it.next().unwrap_or("").trim().to_string();
        let enabled = !status.contains("disabled");
        v.push(Printer { name, status, enabled });
    }
    v
}

/// Parses `lpstat -d` output ("system default destination: NAME").
pub fn parse_lpstat_d(out: &str) -> Option<String> {
    out.lines().find_map(|l| l.split_once("default destination:").map(|(_, n)| n.trim().to_string()))
}

#[derive(Clone, Debug, Default)]
pub struct PpdOption {
    pub choices: Vec<String>,
    pub default: Option<String>,
}

/// Parses `lpoptions -p <printer> -l` output: `UTDuplex/Duplex Mode: 0Front *1Both 2Back`.
pub fn parse_lpoptions(out: &str) -> BTreeMap<String, PpdOption> {
    let mut m = BTreeMap::new();
    for line in out.lines() {
        let Some((head, tail)) = line.split_once(':') else { continue };
        let name = head.split('/').next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let mut opt = PpdOption::default();
        for tok in tail.split_whitespace() {
            if let Some(d) = tok.strip_prefix('*') {
                opt.default = Some(d.to_string());
                opt.choices.push(d.to_string());
            } else {
                opt.choices.push(tok.to_string());
            }
        }
        m.insert(name, opt);
    }
    m
}

/// Compares our options with what the printer's PPD offers; returns human-readable problems.
pub fn validate_against_ppd(o: &PrintOptions, ppd: &BTreeMap<String, PpdOption>) -> Vec<String> {
    let mut problems = Vec::new();
    if !ppd.contains_key("UTDuplex") {
        problems.push("This queue does not look like a Magicard driver queue (no UTDuplex option). Options will be sent anyway.".into());
        return problems;
    }
    for (k, v) in ppd_options(o) {
        if k == "print-scaling" {
            continue;
        }
        match ppd.get(&k) {
            None => problems.push(format!("The printer has no option {k}; it will be ignored")),
            Some(opt) if !opt.choices.contains(&v) => {
                problems.push(format!("Option {k} has no value {v} (choices: {})", opt.choices.join(", ")))
            }
            _ => {}
        }
    }
    problems
}

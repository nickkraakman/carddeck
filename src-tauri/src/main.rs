//! CardDeck desktop shell (Tauri v2). All logic lives in `carddeck-core`; this file only wires
//! commands, file opening (dialog, drag and drop, macOS "Open With" / double click) and CUPS calls.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use carddeck_core::colour::{self, Adjustments, Correction};
use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions, Rendered};
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use tauri::Emitter;
use tauri::{Manager, State};

struct Loaded {
    card: Card,
    name: String,
    path: Option<String>,
    /// Uncorrected renders; the preview colour correction is applied to these on demand.
    front: Rendered,
    back: Rendered,
    /// Current (possibly unsaved) colour settings, and what the file has.
    adj: Mutex<Adjustments>,
    saved: Mutex<Adjustments>,
}

impl Loaded {
    fn new(app: &tauri::AppHandle, state: &AppState, card: Card, name: String, path: Option<String>) -> Loaded {
        let f = fonts(app, state);
        let opts = RenderOptions::default();
        let front = render_side(&card, false, &f, &opts);
        let back = render_side(&card, true, &f, &opts);
        let adj = card.adjustments;
        Loaded { card, name, path, front, back, adj: Mutex::new(adj), saved: Mutex::new(adj) }
    }
}

#[derive(Default)]
struct AppState {
    current: Mutex<Option<Arc<Loaded>>>,
    pending: Mutex<Vec<String>>,
    fonts: OnceLock<Arc<FontSet>>,
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

fn fonts(app: &tauri::AppHandle, state: &AppState) -> Arc<FontSet> {
    state
        .fonts
        .get_or_init(|| {
            let mut f = FontSet::system();
            if let Ok(dir) = app.path().app_config_dir() {
                f.load_dir(&dir.join("fonts"));
                let overrides = dir.join("fonts.json");
                if overrides.exists() {
                    let _ = f.load_overrides(&overrides);
                }
            }
            Arc::new(f)
        })
        .clone()
}

#[derive(Serialize)]
struct CardView {
    name: String,
    path: Option<String>,
    type_name: String,
    width: u32,
    height: u32,
    hub_version: String,
    double_sided: bool,
    portrait: bool,
    dark_card: bool,
    front: String,
    back: String,
    front_elements: usize,
    back_elements: usize,
    warnings: Vec<String>,
    adjustments: AdjustmentsDto,
    has_adjustments: bool,
    /// False for the built-in calibration card (no file to save into).
    can_save: bool,
    corrections: Vec<CorrectionOption>,
}

/// Colour settings as the UI sees them (same JSON names as the `.card` file).
#[derive(Serialize, Deserialize, Clone)]
struct AdjustmentsDto {
    correction: String,
    brightness: f64,
    contrast: f64,
    saturation: f64,
    red: f64,
    green: f64,
    blue: f64,
}

impl AdjustmentsDto {
    fn from_core(a: &Adjustments) -> Self {
        AdjustmentsDto {
            correction: a.correction.json_name().to_string(),
            brightness: a.brightness,
            contrast: a.contrast,
            saturation: a.saturation,
            red: a.red,
            green: a.green,
            blue: a.blue,
        }
    }
    fn to_core(&self) -> Adjustments {
        Adjustments {
            correction: Correction::from_json_name(&self.correction).unwrap_or(Correction::Gamma1),
            brightness: self.brightness,
            contrast: self.contrast,
            saturation: self.saturation,
            red: self.red,
            green: self.green,
            blue: self.blue,
        }
        .clamped()
    }
}

#[derive(Serialize)]
struct CorrectionOption {
    id: &'static str,
    label: &'static str,
    /// PPD choice the printer gets for this mode.
    driver_choice: &'static str,
    /// False for Gamma 1/2: no exact driver option. The print sends Driver managed and does not bake the curve.
    driver_exact: bool,
}

fn correction_options() -> Vec<CorrectionOption> {
    Correction::ALL
        .iter()
        .map(|c| CorrectionOption { id: c.json_name(), label: c.label(), driver_choice: c.ppd_choice(), driver_exact: c.has_driver_equivalent() })
        .collect()
}

/// PNGs of both sides with the given colour settings applied.
#[derive(Serialize)]
struct Previews {
    front: String,
    back: String,
}

fn previews(loaded: &Loaded, adj: &Adjustments) -> Previews {
    Previews { front: data_url(&loaded.front.to_corrected_png(adj)), back: data_url(&loaded.back.to_corrected_png(adj)) }
}

fn view_of(_app: &tauri::AppHandle, _state: &AppState, loaded: &Loaded) -> CardView {
    let front = &loaded.front;
    let back = &loaded.back;
    let adj = *loaded.adj.lock().unwrap();
    let mut warnings: Vec<String> = Vec::new();
    for f in &loaded.card.about.features {
        warnings.push(format!("Card has \"{f}\" enabled. CardDeck does not handle it (no cut-out/overcoat holes)."));
    }
    let v = &loaded.card.about.hub_version;
    if v.is_empty() {
        warnings.push("File has no Hub version.".into());
    } else if v.split('.').next().and_then(|m| m.parse::<u32>().ok()).map(|m| m < 2).unwrap_or(false) {
        warnings.push(format!("Hub {v} is an old file format; open and re-save it in Hub 2 for best results."));
    } else if v.split('.').next().and_then(|m| m.parse::<u32>().ok()).map(|m| m > 2).unwrap_or(false) {
        warnings.push(format!("Hub {v} is newer than the versions CardDeck was tested with (2.0.x)."));
    }
    warnings.extend(front.warnings.iter().map(|w| format!("Front: {w}")));
    warnings.extend(back.warnings.iter().map(|w| format!("Back: {w}")));
    let (width, height) = loaded.card.frame_size();
    CardView {
        name: loaded.name.clone(),
        path: loaded.path.clone(),
        type_name: loaded.card.about.type_name.clone(),
        width,
        height,
        hub_version: loaded.card.about.hub_version.clone(),
        double_sided: loaded.card.about.double_sided,
        portrait: loaded.card.about.portrait,
        dark_card: loaded.card.about.dark_card,
        front: data_url(&front.to_corrected_png(&adj)),
        back: data_url(&back.to_corrected_png(&adj)),
        adjustments: AdjustmentsDto::from_core(&adj),
        has_adjustments: loaded.card.has_adjustments,
        can_save: loaded.path.is_some(),
        corrections: correction_options(),
        front_elements: loaded.card.front.elements.len(),
        back_elements: loaded.card.back.elements.len(),
        warnings,
    }
}

#[tauri::command]
async fn open_card(app: tauri::AppHandle, state: State<'_, AppState>, path: String) -> Result<CardView, String> {
    let p = PathBuf::from(&path);
    let card = tauri::async_runtime::spawn_blocking(move || Card::from_file(&p))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let name = PathBuf::from(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
    let app2 = app.clone();
    let (loaded, view) = tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        let loaded = Arc::new(Loaded::new(&app2, &st, card, name, Some(path)));
        let view = view_of(&app2, &st, &loaded);
        (loaded, view)
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.current.lock().unwrap() = Some(loaded);
    Ok(view)
}

#[tauri::command]
async fn open_calibration(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<CardView, String> {
    let app2 = app.clone();
    let (loaded, view) = tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        let loaded = Arc::new(Loaded::new(&app2, &st, carddeck_core::calibration::calibration_card(), "Calibration card".into(), None));
        let view = view_of(&app2, &st, &loaded);
        (loaded, view)
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.current.lock().unwrap() = Some(loaded);
    Ok(view)
}

// ---- colour correction --------------------------------------------------------------------------

#[derive(Serialize)]
struct AdjustResult {
    previews: Previews,
    /// True if the settings are now stored in the .card file.
    saved: bool,
    /// Settings differ from the file (and were not saved).
    dirty: bool,
}

fn adjust_result(loaded: &Loaded, saved: bool) -> AdjustResult {
    let adj = *loaded.adj.lock().unwrap();
    let on_disk = *loaded.saved.lock().unwrap();
    AdjustResult { previews: previews(loaded, &adj), saved, dirty: adj != on_disk }
}

fn write_to_file(loaded: &Loaded) -> Result<(), String> {
    let path = loaded.path.as_ref().ok_or("This card has no file to save into")?;
    let adj = *loaded.adj.lock().unwrap();
    colour::store::save_adjustments(std::path::Path::new(path), &adj).map_err(|e| e.to_string())?;
    *loaded.saved.lock().unwrap() = adj;
    Ok(())
}

/// Changes the colour settings; re-renders the preview from the cached uncorrected render. With
/// `save`, the `adjustments` block of the .card file is updated too (nothing else in the file changes).
#[tauri::command]
async fn set_adjustments(state: State<'_, AppState>, adj: AdjustmentsDto, save: bool) -> Result<AdjustResult, String> {
    let loaded = current(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        *loaded.adj.lock().unwrap() = adj.to_core();
        let mut saved = false;
        if save && loaded.path.is_some() {
            write_to_file(&loaded)?;
            saved = true;
        }
        Ok(adjust_result(&loaded, saved))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn take_pending_open(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.pending.lock().unwrap())
}

// ---- printing ---------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(default)]
struct JobOpts {
    printer: Option<String>,
    sides: String,
    copies: u32,
    /// `NAME=VALUE` lines
    extra: Vec<String>,
}

impl Default for JobOpts {
    fn default() -> Self {
        JobOpts {
            printer: None,
            sides: "both".into(),
            copies: 1,
            extra: Vec::new(),
        }
    }
}

impl JobOpts {
    fn to_options(&self) -> PrintOptions {
        let mut o = PrintOptions {
            printer: self.printer.clone().filter(|p| !p.is_empty()),
            sides: match self.sides.as_str() {
                "front" => Sides::Front,
                "back" => Sides::Back,
                _ => Sides::Both,
            },
            copies: self.copies.clamp(1, 99),
            // Overcoat is always on for both sides. Orientation / black / photo-nudge stay on the
            // sensible defaults in PrintOptions.
            ..PrintOptions::default()
        };
        for line in &self.extra {
            if let Some((k, v)) = line.trim().split_once('=') {
                if !k.trim().is_empty() && !v.trim().is_empty() {
                    o.extra_options.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
        }
        o
    }
}

fn current(state: &AppState) -> Result<Arc<Loaded>, String> {
    state.current.lock().unwrap().clone().ok_or_else(|| "Open a .card file first".to_string())
}

fn build(app: &tauri::AppHandle, state: &AppState, opts: &JobOpts) -> Result<(Job, PrintOptions), String> {
    let loaded = current(state)?;
    let f = fonts(app, state);
    let mut o = opts.to_options();
    // Sliders have no driver options, so they are baked into the bitmap. Gamma is not: baking it and
    // sending ColourCorrection=None printed light, washed-out cards. The driver gets Driver managed.
    let adj = *loaded.adj.lock().unwrap();
    o.use_colour(&adj, true);
    let job = build_job(&loaded.card, &f, &o).map_err(|e| e.to_string())?;
    Ok((job, o))
}

#[tauri::command]
async fn print_job(app: tauri::AppHandle, opts: JobOpts) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (job, o) = build(&app, &state, &opts)?;
        if o.printer.is_none() {
            return Err("Choose a printer first".to_string());
        }
        let dir = std::env::temp_dir().join(format!("carddeck-{}", std::process::id()));
        let files = carddeck_core::print::save_job(&dir, &job, &o).map_err(|e| e.to_string())?;
        let out = Command::new("lp")
            .args(lp_args(&files[0], &o))
            .output()
            .map_err(|e| format!("Could not run lp: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!("lp failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct PrinterList {
    printers: Vec<Printer>,
    default: Option<String>,
    error: Option<String>,
}

#[tauri::command]
async fn list_printers() -> PrinterList {
    tauri::async_runtime::spawn_blocking(|| {
        let p = Command::new("lpstat").arg("-p").output();
        let d = Command::new("lpstat").arg("-d").output();
        let default = d.ok().and_then(|o| parse_lpstat_d(&String::from_utf8_lossy(&o.stdout)));
        match p {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).to_string();
                let err = if !o.status.success() && text.trim().is_empty() {
                    let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    if e.contains("No destinations") || e.is_empty() { None } else { Some(e) }
                } else {
                    None
                };
                PrinterList { printers: parse_lpstat_p(&text), default, error: err }
            }
            Err(e) => PrinterList { printers: vec![], default, error: Some(format!("Could not run lpstat: {e}")) },
        }
    })
    .await
    .unwrap_or(PrinterList { printers: vec![], default: None, error: Some("internal error".into()) })
}

fn main() {
    let state = AppState::default();
    for arg in std::env::args().skip(1) {
        if arg.to_lowercase().ends_with(".card") {
            state.pending.lock().unwrap().push(arg);
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![open_card, open_calibration, take_pending_open, print_job, list_printers, set_adjustments])
        .build(tauri::generate_context!())
        .expect("error while building CardDeck");
    app.run(|handle, event| {
        // macOS: double click on a .card in Finder / "Open With" / drop on the Dock icon.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &event {
            for u in urls {
                if let Ok(p) = u.to_file_path() {
                    let p = p.display().to_string();
                    handle.state::<AppState>().pending.lock().unwrap().push(p.clone());
                    let _ = handle.emit("open-file", p);
                }
            }
        }
        let _ = (&handle, &event);
    });
}

//! CardDeck desktop shell (Tauri v2). All logic lives in `carddeck-core`; this file only wires
//! commands, file opening (dialog, drag and drop, macOS "Open With" / double click) and CUPS calls.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions};
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use tauri::Emitter;
use tauri::{Manager, State};

struct Loaded {
    card: Card,
    name: String,
    path: Option<String>,
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
}

fn view_of(app: &tauri::AppHandle, state: &AppState, loaded: &Loaded) -> CardView {
    let f = fonts(app, state);
    let opts = RenderOptions::default();
    let front = render_side(&loaded.card, false, &f, &opts);
    let back = render_side(&loaded.card, true, &f, &opts);
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
        front: data_url(&front.to_png()),
        back: data_url(&back.to_png()),
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
    let loaded = Arc::new(Loaded { card, name, path: Some(path) });
    *state.current.lock().unwrap() = Some(loaded.clone());
    let app2 = app.clone();
    let view = tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        view_of(&app2, &st, &loaded)
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(view)
}

#[tauri::command]
async fn open_calibration(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<CardView, String> {
    let loaded = Arc::new(Loaded { card: carddeck_core::calibration::calibration_card(), name: "Calibration card".into(), path: None });
    *state.current.lock().unwrap() = Some(loaded.clone());
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = app2.state::<AppState>();
        view_of(&app2, &st, &loaded)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn take_pending_open(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.pending.lock().unwrap())
}

// ---- printing ---------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct JobOpts {
    printer: Option<String>,
    sides: String,
    copies: u32,
    turn: String,
    fit: String,
    rotate_front_180: bool,
    rotate_back_180: bool,
    overcoat_front: bool,
    overcoat_back: bool,
    black: String,
    photo_nudge: bool,
    /// `NAME=VALUE` lines
    extra: Vec<String>,
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
            turn: if self.turn == "cw" { PageTurn::Cw90 } else { PageTurn::Ccw90 },
            fit: if self.fit == "stretch" { PageFit::Stretch } else { PageFit::Crop },
            rotate_front_180: self.rotate_front_180,
            rotate_back_180: self.rotate_back_180,
            overcoat_front: self.overcoat_front,
            overcoat_back: self.overcoat_back,
            black: match self.black.as_str() {
                "composite" => BlackMode::CompositeYmc,
                "mono" => BlackMode::MonoK,
                _ => BlackMode::ResinK,
            },
            prevent_resin_for_images: self.photo_nudge,
            extra_options: Vec::new(),
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

#[derive(Serialize)]
struct PagePreview {
    label: String,
    png: String,
}

#[derive(Serialize)]
struct JobPreview {
    pages: Vec<PagePreview>,
    command: String,
    warnings: Vec<String>,
    problems: Vec<String>,
}

fn current(state: &AppState) -> Result<Arc<Loaded>, String> {
    state.current.lock().unwrap().clone().ok_or_else(|| "Open a .card file first".to_string())
}

fn ppd_problems(o: &PrintOptions) -> Vec<String> {
    let Some(p) = &o.printer else { return vec![] };
    match Command::new("lpoptions").args(["-p", p, "-l"]).output() {
        Ok(out) if out.status.success() => {
            let ppd = parse_lpoptions(&String::from_utf8_lossy(&out.stdout));
            validate_against_ppd(o, &ppd)
        }
        Ok(out) => vec![format!("lpoptions failed: {}", String::from_utf8_lossy(&out.stderr).trim())],
        Err(e) => vec![format!("Could not run lpoptions: {e}")],
    }
}

fn build(app: &tauri::AppHandle, state: &AppState, opts: &JobOpts) -> Result<(Job, PrintOptions), String> {
    let loaded = current(state)?;
    let f = fonts(app, state);
    let o = opts.to_options();
    let job = build_job(&loaded.card, &f, &o).map_err(|e| e.to_string())?;
    Ok((job, o))
}

#[tauri::command]
async fn preview_job(app: tauri::AppHandle, opts: JobOpts) -> Result<JobPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (job, o) = build(&app, &state, &opts)?;
        let pages = job
            .pages
            .iter()
            .take(2)
            .map(|p| {
                let mut png = Vec::new();
                image_png(&p.rgb, &mut png);
                PagePreview { label: p.label.to_string(), png: data_url(&png) }
            })
            .collect();
        let command = lp_command_line(std::path::Path::new("<job.pdf>"), &o);
        Ok(JobPreview { pages, command, warnings: job.warnings.clone(), problems: ppd_problems(&o) })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn image_png(rgb: &carddeck_core::print::RgbImage, out: &mut Vec<u8>) {
    rgb.write_to(&mut std::io::Cursor::new(out), carddeck_core::print::ImageFormat::Png).ok();
}

#[tauri::command]
async fn save_job(app: tauri::AppHandle, opts: JobOpts, dir: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (job, o) = build(&app, &state, &opts)?;
        let stem = current(&state)?.name.trim_end_matches(".card").replace(['/', '\\', ':'], "_");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let target = PathBuf::from(dir).join(format!("{stem}-job-{stamp}"));
        let files = carddeck_core::print::save_job(&target, &job, &o).map_err(|e| e.to_string())?;
        Ok(files.into_iter().map(|p| p.display().to_string()).collect())
    })
    .await
    .map_err(|e| e.to_string())?
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
        let out = Command::new("lp").args(lp_args(&files[0], &o)).output().map_err(|e| format!("Could not run lp: {e}"))?;
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
        .invoke_handler(tauri::generate_handler![open_card, open_calibration, take_pending_open, preview_job, save_job, print_job, list_printers])
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

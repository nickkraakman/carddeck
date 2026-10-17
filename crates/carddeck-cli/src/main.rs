//! `carddeck` command line: inspect, render and dry-run print jobs without any GUI.

use std::path::PathBuf;

use carddeck_core::colour::{self, Adjustments, Correction};
use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions};

const USAGE: &str = "carddeck - Magicard Hub .card viewer/printer (core CLI)

USAGE:
  carddeck info <file.card>
  carddeck render <file.card> <outdir> [--scale 1.0] [--adjust] [--correction NAME]
  carddeck adjust <file.card> [--correction NAME] [--brightness V] [--contrast V] [--saturation V]
                              [--red V] [--green V] [--blue V] [--reset]
  carddeck job <file.card> <outdir> [options]      dry run: writes job.pdf, page PNGs, command.txt
  carddeck print <file.card> -d <printer> [options] build the job and send it with lp
  carddeck calibrate <outdir> [options]             dry-run job for the built-in calibration card
  carddeck printers                                 list CUPS queues (lpstat -p)

JOB/PRINT OPTIONS:
  --sides front|both|back     (default: both if the card is double sided, else front)
  --copies N                  (default 1)
  --turn ccw|cw               turn landscape card into the portrait page (default ccw, UNVERIFIED)
  --fit crop|stretch          642->638 px fit (default crop)
  --rotate-front-180 / --rotate-back-180
  --no-overcoat-front / --no-overcoat-back
  --black resin|composite|mono   (default resin: pure #000000 -> K panel)
  --no-photo-nudge            keep pure-black photo pixels (they will print on resin)
  --opt NAME=VALUE            extra lp -o option, repeatable (e.g. CFRotation=1Rotation)
  --correction NAME           (alias --colour-correct) colour correction to send to the driver (default: the one saved in the card's
                              `adjustments`); NAME = None Gamma1 Gamma2 Perceptual Saturation
                              RelativeColorimetric AbsoluteColorimetric. Sent as PPD option ColourCorrection.
  --no-correction-option      send no ColourCorrection option (driver default)
  --bake-adjustments          also bake sliders + Gamma 1/2 into the print bitmap (default: preview only)
  -d <printer>                CUPS queue name
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn parse_opts(args: &[String], card: &Card) -> Result<PrintOptions, String> {
    let mut o = PrintOptions {
        sides: if card.about.double_sided { Sides::Both } else { Sides::Front },
        ..Default::default()
    };
    let mut i = 0;
    let (mut corr, mut no_corr_opt, mut bake) = (None::<Correction>, false, false);
    let val = |i: usize| args.get(i + 1).cloned().ok_or_else(|| format!("{} needs a value", args[i]));
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                o.printer = Some(val(i)?);
                i += 1;
            }
            "--sides" => {
                o.sides = match val(i)?.as_str() {
                    "front" => Sides::Front,
                    "both" => Sides::Both,
                    "back" => Sides::Back,
                    v => return Err(format!("bad --sides {v}")),
                };
                i += 1;
            }
            "--copies" => {
                o.copies = val(i)?.parse().map_err(|_| "bad --copies")?;
                i += 1;
            }
            "--turn" => {
                o.turn = match val(i)?.as_str() {
                    "ccw" => PageTurn::Ccw90,
                    "cw" => PageTurn::Cw90,
                    v => return Err(format!("bad --turn {v}")),
                };
                i += 1;
            }
            "--fit" => {
                o.fit = match val(i)?.as_str() {
                    "crop" => PageFit::Crop,
                    "stretch" => PageFit::Stretch,
                    v => return Err(format!("bad --fit {v}")),
                };
                i += 1;
            }
            "--black" => {
                o.black = match val(i)?.as_str() {
                    "resin" => BlackMode::ResinK,
                    "composite" => BlackMode::CompositeYmc,
                    "mono" => BlackMode::MonoK,
                    v => return Err(format!("bad --black {v}")),
                };
                i += 1;
            }
            "--rotate-front-180" => o.rotate_front_180 = true,
            "--rotate-back-180" => o.rotate_back_180 = true,
            "--no-overcoat-front" => o.overcoat_front = false,
            "--no-overcoat-back" => o.overcoat_back = false,
            "--no-photo-nudge" => o.prevent_resin_for_images = false,
            "--correction" | "--colour-correct" => {
                let v = val(i)?;
                corr = Some(Correction::from_json_name(&v).ok_or_else(|| format!("bad --correction {v}"))?);
                i += 1;
            }
            "--no-correction-option" => no_corr_opt = true,
            "--bake-adjustments" => bake = true,
            "--opt" => {
                let v = val(i)?;
                let (k, x) = v.split_once('=').ok_or("--opt needs NAME=VALUE")?;
                o.extra_options.push((k.into(), x.into()));
                i += 1;
            }
            other => return Err(format!("unknown option {other}")),
        }
        i += 1;
    }
    let mut adj = card.adjustments;
    if let Some(c) = corr {
        adj.correction = c;
    }
    o.use_colour(&adj, bake);
    if no_corr_opt {
        o.colour_correction = None;
    }
    Ok(o)
}

fn flag_value(args: &[String], name: &str) -> Result<Option<f64>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).and_then(|v| v.parse::<f64>().ok()).map(Some).ok_or_else(|| format!("{name} needs a number")),
    }
}

fn describe(a: &Adjustments) -> String {
    format!(
        "correction {}  brightness {}  contrast {}  saturation {}  red {}  green {}  blue {}",
        a.correction.json_name(), a.brightness, a.contrast, a.saturation, a.red, a.green, a.blue
    )
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        Some("info") if args.len() >= 2 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let (w, h) = card.frame_size();
            println!("type: {}   frame: {w}x{h}px   hub: {}", card.about.type_name, card.about.hub_version);
            println!("double sided: {}   portrait: {}   dark card: {}", card.about.double_sided, card.about.portrait, card.about.dark_card);
            println!("front elements: {}   back elements: {}", card.front.elements.len(), card.back.elements.len());
            if card.has_adjustments {
                println!("colour correct: {}", card.adjustments.correction.label());
            }
            for f in &card.about.features {
                println!("unsupported feature flag: {f}");
            }
            Ok(())
        }
        Some("render") if args.len() >= 3 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let scale = args.iter().position(|a| a == "--scale").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1.0);
            // On-screen preview correction (never used for printing).
            let mut adj = card.adjustments;
            let mut apply = args.iter().any(|a| a == "--adjust");
            if let Some(i) = args.iter().position(|a| a == "--correction") {
                let v = args.get(i + 1).ok_or("--correction needs a value")?;
                adj.correction = Correction::from_json_name(v).ok_or_else(|| format!("bad --correction {v}"))?;
                apply = true;
            }
            let fonts = FontSet::system();
            let out = PathBuf::from(&args[2]);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let stem = PathBuf::from(&args[1]).file_stem().unwrap().to_string_lossy().to_string();
            for (back, name) in [(false, "front"), (true, "back")] {
                let r = render_side(&card, back, &fonts, &RenderOptions { scale, ..Default::default() });
                let png = if apply { r.to_corrected_png(&adj) } else { r.to_png() };
                std::fs::write(out.join(format!("{stem}-{name}.png")), png).map_err(|e| e.to_string())?;
                for w in &r.warnings {
                    eprintln!("warning ({name}): {w}");
                }
            }
            Ok(())
        }
        Some("adjust") if args.len() >= 2 => {
            let path = PathBuf::from(&args[1]);
            let card = Card::from_file(&path).map_err(|e| e.to_string())?;
            let mut a = card.adjustments;
            let rest = &args[2..];
            let mut changed = false;
            if rest.iter().any(|x| x == "--reset") {
                a = Adjustments::default();
                changed = true;
            }
            if let Some(i) = rest.iter().position(|x| x == "--correction") {
                let v = rest.get(i + 1).ok_or("--correction needs a value")?;
                a.correction = Correction::from_json_name(v).ok_or_else(|| format!("bad --correction {v}"))?;
                changed = true;
            }
            for (flag, field) in [("--brightness", 0), ("--contrast", 1), ("--saturation", 2), ("--red", 3), ("--green", 4), ("--blue", 5)] {
                if let Some(x) = flag_value(rest, flag)? {
                    match field {
                        0 => a.brightness = x,
                        1 => a.contrast = x,
                        2 => a.saturation = x,
                        3 => a.red = x,
                        4 => a.green = x,
                        _ => a.blue = x,
                    }
                    changed = true;
                }
            }
            let a = a.clamped();
            if changed {
                colour::store::save_adjustments(&path, &a).map_err(|e| e.to_string())?;
                println!("saved: {}", describe(&a));
            } else {
                println!("{}{}", describe(&a), if card.has_adjustments { "" } else { "   (file has no adjustments block: Hub default)" });
                println!("driver option for printing: ColourCorrection={}{}", a.correction.ppd_choice(),
                    if a.correction.has_driver_equivalent() { "" } else { "   (no driver equivalent: preview only)" });
            }
            Ok(())
        }
        Some("job") if args.len() >= 3 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let o = parse_opts(&args[3..], &card)?;
            let job = build_job(&card, &FontSet::system(), &o).map_err(|e| e.to_string())?;
            for w in &job.warnings {
                eprintln!("warning: {w}");
            }
            let files = save_job(&PathBuf::from(&args[2]), &job, &o).map_err(|e| e.to_string())?;
            for f in files {
                println!("{}", f.display());
            }
            Ok(())
        }
        Some("print") if args.len() >= 2 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let o = parse_opts(&args[2..], &card)?;
            if o.printer.is_none() {
                return Err("print needs -d <printer> (see `carddeck printers`)".into());
            }
            let job = build_job(&card, &FontSet::system(), &o).map_err(|e| e.to_string())?;
            let dir = std::env::temp_dir().join(format!("carddeck-{}", std::process::id()));
            let files = save_job(&dir, &job, &o).map_err(|e| e.to_string())?;
            let pdf = files[0].clone();
            let status = std::process::Command::new("lp").args(lp_args(&pdf, &o)).status().map_err(|e| format!("could not run lp: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("lp failed: {status}"))
            }
        }
        Some("calibrate") if args.len() >= 2 => {
            // Same options as `job`, but for the built-in calibration card.
            let card = carddeck_core::calibration::calibration_card();
            let o = parse_opts(&args[2..], &card)?;
            let job = build_job(&card, &FontSet::system(), &o).map_err(|e| e.to_string())?;
            for f in save_job(&PathBuf::from(&args[1]), &job, &o).map_err(|e| e.to_string())? {
                println!("{}", f.display());
            }
            Ok(())
        }
        Some("printers") => {
            let out = std::process::Command::new("lpstat").arg("-p").output().map_err(|e| format!("could not run lpstat: {e}"))?;
            for p in parse_lpstat_p(&String::from_utf8_lossy(&out.stdout)) {
                println!("{}\t{}", p.name, p.status);
            }
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            Ok(())
        }
    }
}

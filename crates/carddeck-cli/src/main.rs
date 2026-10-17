//! `carddeck` command line: inspect, render and dry-run print jobs without any GUI.

use std::path::PathBuf;

use carddeck_core::fonts::FontSet;
use carddeck_core::print::*;
use carddeck_core::{render_side, Card, RenderOptions};

const USAGE: &str = "carddeck - Magicard Hub .card viewer/printer (core CLI)

USAGE:
  carddeck info <file.card>
  carddeck render <file.card> <outdir> [--scale 1.0]
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
  --colour-correct LABEL      Hub Colour Correct (default: adjustments.correction from the file)
  --opt NAME=VALUE            extra lp -o option, repeatable (e.g. CFRotation=1Rotation)
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
        colour_correction: card.adjustments.correction.clone(),
        ..Default::default()
    };
    let mut i = 0;
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
            "--colour-correct" => {
                o.colour_correction = Some(val(i)?);
                i += 1;
            }
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
    Ok(o)
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        Some("info") if args.len() >= 2 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let (w, h) = card.frame_size();
            println!("type: {}   frame: {w}x{h}px   hub: {}", card.about.type_name, card.about.hub_version);
            println!("double sided: {}   portrait: {}   dark card: {}", card.about.double_sided, card.about.portrait, card.about.dark_card);
            println!("front elements: {}   back elements: {}", card.front.elements.len(), card.back.elements.len());
            if let Some(c) = &card.adjustments.correction {
                println!("colour correct: {c}");
            }
            for f in &card.about.features {
                println!("unsupported feature flag: {f}");
            }
            Ok(())
        }
        Some("render") if args.len() >= 3 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).map_err(|e| e.to_string())?;
            let scale = args.iter().position(|a| a == "--scale").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1.0);
            let fonts = FontSet::system();
            let out = PathBuf::from(&args[2]);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let stem = PathBuf::from(&args[1]).file_stem().unwrap().to_string_lossy().to_string();
            for (back, name) in [(false, "front"), (true, "back")] {
                let r = render_side(&card, back, &fonts, &RenderOptions { scale, ..Default::default() });
                std::fs::write(out.join(format!("{stem}-{name}.png")), r.to_png()).map_err(|e| e.to_string())?;
                for w in &r.warnings {
                    eprintln!("warning ({name}): {w}");
                }
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

use std::path::PathBuf;
use carddeck_core::{fonts::FontSet, render_side, Card, RenderOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("render") if args.len() >= 3 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).expect("open");
            let fonts = FontSet::system();
            let out = PathBuf::from(&args[2]);
            std::fs::create_dir_all(&out).unwrap();
            let stem = PathBuf::from(&args[1]).file_stem().unwrap().to_string_lossy().to_string();
            for (back, name) in [(false, "front"), (true, "back")] {
                let r = render_side(&card, back, &fonts, &RenderOptions::default());
                std::fs::write(out.join(format!("{stem}-{name}.png")), r.to_png()).unwrap();
                for w in &r.warnings { eprintln!("warning ({name}): {w}"); }
            }
        }
        Some("renderhalf") if args.len() >= 3 => {
            let card = Card::from_file(&PathBuf::from(&args[1])).expect("open");
            let fonts = FontSet::system();
            let r = render_side(&card, false, &fonts, &RenderOptions { scale: 0.5, ..Default::default() });
            std::fs::write(&args[2], r.to_png()).unwrap();
        }
        _ => eprintln!("usage: carddeck render <file.card> <outdir>"),
    }
}

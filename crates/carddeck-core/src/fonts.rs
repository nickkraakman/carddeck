//! Font resolution, text shaping and glyph outlines.
//!
//! `.card` files only store a font *family name* (Myriad Pro, Minion Pro, Georgia, Arial...).
//! Several of those are not installed on macOS, so every name goes through a
//! fallback table (see [`default_fallbacks`]). Users can override or extend the table with
//! a `fonts.json` file (`{"myriad pro": ["Avenir Next", "Helvetica Neue"]}`), see
//! [`FontSet::load_overrides`].

use std::collections::HashMap;
use std::path::Path as FsPath;
use std::sync::Arc;

use fontdb::{Database, Family, Query, Stretch, Style, Weight, ID};
use tiny_skia::{Path, PathBuilder};

/// A resolved face: owned font bytes and the collection index.
#[derive(Clone)]
pub struct Face {
    pub id: ID,
    data: Arc<Vec<u8>>,
    index: u32,
    pub family: String,
}

pub struct FontSet {
    db: Database,
    /// lowercase family -> actual family name in the database
    families: HashMap<String, String>,
    overrides: HashMap<String, Vec<String>>,
    data_cache: std::sync::Mutex<HashMap<ID, Arc<Vec<u8>>>>,
}

/// One positioned glyph, in pixels relative to the line origin (baseline, left).
#[derive(Clone, Debug)]
pub struct Glyph {
    pub face: usize,
    pub gid: u16,
    pub x: f32,
    pub y: f32,
}

pub struct ShapedLine {
    pub width: f32,
    pub glyphs: Vec<Glyph>,
}

/// Faces chosen for one text element: primary first, then per-glyph fallbacks.
pub struct FaceChain {
    pub faces: Vec<Face>,
}

pub fn default_fallbacks(family: &str) -> Vec<&'static str> {
    // Ordered best-first. Linux names are included so the renderer is testable headless.
    let sans: Vec<&'static str> = vec![
        "Helvetica Neue", "Helvetica", "Arial", "Liberation Sans", "Nimbus Sans", "DejaVu Sans", "Noto Sans",
    ];
    let serif: Vec<&'static str> = vec![
        "Times New Roman", "Times", "Georgia", "Liberation Serif", "Nimbus Roman", "DejaVu Serif", "Noto Serif",
    ];
    let mono: Vec<&'static str> = vec![
        "Courier New", "Courier", "Menlo", "Liberation Mono", "Nimbus Mono PS", "DejaVu Sans Mono",
    ];
    match family.trim().to_lowercase().as_str() {
        "myriad pro" | "myriad" | "myriad pro cond" | "myriad pro light" => {
            let mut v = vec!["Myriad Pro"];
            v.extend(sans);
            v
        }
        "minion pro" | "minion" => {
            let mut v = vec!["Minion Pro"];
            v.extend(serif);
            v
        }
        "georgia" => {
            let mut v = vec!["Georgia"];
            v.extend(["Times New Roman", "Times", "Liberation Serif", "Nimbus Roman", "DejaVu Serif"]);
            v
        }
        "times new roman" | "times" | "cambria" | "garamond" | "palatino" | "palatino linotype" | "book antiqua" => {
            let mut v = vec!["Times New Roman", "Times"];
            v.extend(&serif[2..]);
            v
        }
        "courier new" | "courier" | "consolas" | "lucida console" | "monaco" | "menlo" => {
            let mut v = vec!["Courier New", "Courier"];
            v.extend(&mono[2..]);
            v
        }
        "arial" | "arial black" | "arial narrow" | "helvetica" | "helvetica neue" => {
            let mut v = vec!["Arial", "Helvetica Neue", "Helvetica"];
            v.extend(&sans[3..]);
            v
        }
        "verdana" | "tahoma" | "trebuchet ms" | "segoe ui" | "calibri" | "open sans" | "roboto" | "lato" => {
            let mut v = vec!["Verdana"];
            v.extend(&sans);
            v
        }
        other => {
            let mut v = Vec::new();
            if other.contains("mono") || other.contains("courier") || other.contains("typewriter") {
                v.extend(mono);
            } else if other.contains("serif") && !other.contains("sans")
                || other.contains("roman")
                || other.contains("garamond")
                || other.contains("georgia")
            {
                v.extend(serif);
            } else {
                v.extend(sans);
            }
            v
        }
    }
}

const GLYPH_FALLBACK_FAMILIES: &[&str] = &[
    "Arial Unicode MS", "Apple Symbols", "Helvetica Neue", "Arial", "Noto Sans", "DejaVu Sans", "Liberation Sans",
    "Hiragino Sans", "PingFang SC", "Noto Sans CJK JP",
];

impl FontSet {
    /// Loads system fonts (macOS: /System/Library/Fonts, /Library/Fonts, ~/Library/Fonts).
    pub fn system() -> FontSet {
        let mut db = Database::new();
        db.load_system_fonts();
        FontSet::from_db(db)
    }

    pub fn from_db(db: Database) -> FontSet {
        let mut families = HashMap::new();
        for face in db.faces() {
            for (name, _) in &face.families {
                families.entry(name.to_lowercase()).or_insert_with(|| name.clone());
            }
        }
        FontSet { db, families, overrides: HashMap::new(), data_cache: Default::default() }
    }

    /// Adds a directory of extra fonts (for example `~/Library/Application Support/CardDeck/fonts`).
    pub fn load_dir(&mut self, dir: &FsPath) {
        if dir.is_dir() {
            self.db.load_fonts_dir(dir);
            self.families.clear();
            for face in self.db.faces() {
                for (name, _) in &face.families {
                    self.families.entry(name.to_lowercase()).or_insert_with(|| name.clone());
                }
            }
        }
    }

    /// `fonts.json`: `{ "<card font family, any case>": ["Preferred", "Fallback", ...] }`.
    pub fn load_overrides(&mut self, path: &FsPath) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let map: HashMap<String, Vec<String>> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        for (k, v) in map {
            self.overrides.insert(k.to_lowercase(), v);
        }
        Ok(())
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.families.contains_key(&name.to_lowercase())
    }

    pub fn face_count(&self) -> usize {
        self.db.len()
    }

    fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<Face> {
        let actual = self.families.get(&family.to_lowercase())?;
        let q = Query {
            families: &[Family::Name(actual)],
            weight: if bold { Weight::BOLD } else { Weight::NORMAL },
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        };
        let id = self.db.query(&q)?;
        self.face_from_id(id, actual.clone())
    }

    fn face_from_id(&self, id: ID, family: String) -> Option<Face> {
        let mut cache = self.data_cache.lock().ok()?;
        let (index, data) = if let Some(d) = cache.get(&id) {
            let index = self.db.face(id)?.index;
            (index, d.clone())
        } else {
            let data = self.db.with_face_data(id, |d, _| Arc::new(d.to_vec()))?;
            let index = self.db.face(id)?.index;
            cache.insert(id, data.clone());
            (index, data)
        };
        Some(Face { id, data, index, family })
    }

    /// Candidate family names for a card font name, best first.
    pub fn candidates(&self, family: &str) -> Vec<String> {
        let key = family.trim().to_lowercase();
        let mut out: Vec<String> = Vec::new();
        if let Some(o) = self.overrides.get(&key) {
            out.extend(o.iter().cloned());
        }
        out.push(family.trim().to_string());
        out.extend(default_fallbacks(family).into_iter().map(String::from));
        out
    }

    /// Returns the face chain and the family actually used for the primary face.
    pub fn chain(&self, family: &str, bold: bool, italic: bool) -> Option<FaceChain> {
        let mut faces = Vec::new();
        for cand in self.candidates(family) {
            if let Some(f) = self.pick(&cand, bold, italic) {
                faces.push(f);
                break;
            }
        }
        if faces.is_empty() {
            // Last resort: any face at all.
            let id = self.db.faces().next()?.id;
            let fam = self.db.face(id)?.families.first().map(|f| f.0.clone()).unwrap_or_default();
            faces.push(self.face_from_id(id, fam)?);
        }
        for fam in GLYPH_FALLBACK_FAMILIES {
            if let Some(f) = self.pick(fam, bold, italic) {
                if !faces.iter().any(|x| x.id == f.id) {
                    faces.push(f);
                }
            }
            if faces.len() >= 4 {
                break;
            }
        }
        Some(FaceChain { faces })
    }
}

impl Face {
    fn with_ttf<R>(&self, f: impl FnOnce(&ttf_parser::Face) -> R) -> Option<R> {
        let face = ttf_parser::Face::parse(&self.data, self.index).ok()?;
        Some(f(&face))
    }
    pub fn has_glyph(&self, c: char) -> bool {
        self.with_ttf(|f| f.glyph_index(c).map(|g| g.0 != 0).unwrap_or(false)).unwrap_or(false)
    }
    pub fn units_per_em(&self) -> f32 {
        self.with_ttf(|f| f.units_per_em() as f32).unwrap_or(1000.0)
    }
    /// (ascent, descent) in em units; descent is positive.
    pub fn metrics_em(&self) -> (f32, f32) {
        self.with_ttf(|f| {
            let u = f.units_per_em() as f32;
            (f.ascender() as f32 / u, -(f.descender() as f32) / u)
        })
        .unwrap_or((0.9, 0.2))
    }
    pub fn outline(&self, gid: u16) -> Option<Path> {
        self.with_ttf(|f| {
            let mut b = Outline(PathBuilder::new());
            f.outline_glyph(ttf_parser::GlyphId(gid), &mut b)?;
            b.0.finish()
        })
        .flatten()
    }
}

struct Outline(PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y)
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y)
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y)
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y)
    }
    fn close(&mut self) {
        self.0.close()
    }
}

impl FaceChain {
    pub fn primary(&self) -> &Face {
        &self.faces[0]
    }

    /// Shapes one line of text at `size` px with `letter_spacing` px between characters.
    /// Glyph outlines are in font units: scale by `size / units_per_em` (see [`ShapedLine`] users).
    pub fn shape(&self, text: &str, size: f32, letter_spacing: f32) -> ShapedLine {
        let mut glyphs = Vec::new();
        let mut pen = 0.0f32;
        // split into runs by covering face
        let mut runs: Vec<(usize, String)> = Vec::new();
        for c in text.chars() {
            let fi = if c.is_control() || c == '\u{200b}' {
                0
            } else {
                self.faces.iter().position(|f| f.has_glyph(c)).unwrap_or(0)
            };
            match runs.last_mut() {
                Some((f, s)) if *f == fi => s.push(c),
                _ => runs.push((fi, c.to_string())),
            }
        }
        for (fi, run) in runs {
            let face = &self.faces[fi];
            let Some(rb) = rustybuzz::Face::from_slice(&face.data, face.index) else { continue };
            let scale = size / rb.units_per_em() as f32;
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(&run);
            let out = rustybuzz::shape(&rb, &[], buf);
            let infos = out.glyph_infos();
            let pos = out.glyph_positions();
            for (i, p) in infos.iter().zip(pos.iter()) {
                glyphs.push(Glyph {
                    face: fi,
                    gid: i.glyph_id as u16,
                    x: pen + p.x_offset as f32 * scale,
                    y: -(p.y_offset as f32) * scale,
                });
                pen += p.x_advance as f32 * scale + letter_spacing;
            }
        }
        ShapedLine { width: pen, glyphs }
    }

    pub fn measure(&self, text: &str, size: f32, letter_spacing: f32) -> f32 {
        self.shape(text, size, letter_spacing).width
    }
}

/// Horizontal scale applied when a font is replaced by a substitute that is known to be wider.
/// Myriad Pro is noticeably narrower than Helvetica/Arial (about 12 percent for bold).
pub fn substitute_x_scale(family: &str) -> f32 {
    match family.trim().to_lowercase().as_str() {
        "myriad pro" | "myriad" => 0.9,
        _ => 1.0,
    }
}

//! `.card` document model.
//!
//! A `.card` file is UTF-8 JSON: a serialisation of a Konva scene plus a few
//! application fields. We parse leniently: numbers may be strings, almost every
//! attribute is optional, unknown keys are ignored.

use serde_json::{Map, Value};

use crate::Error;

#[derive(Debug, Clone)]
pub struct Card {
    pub about: About,
    /// Hub print adjustments (`adjustments` in the JSON). Colour correct is remembered per file.
    pub adjustments: Adjustments,
    pub front: Side,
    pub back: Side,
}

/// Hub `adjustments` block. Colour correct maps to the Magicard PPD `ColourCorrection` option;
/// brightness / contrast / RGB multipliers are stored but not applied by CardDeck yet.
#[derive(Debug, Clone, Default)]
pub struct Adjustments {
    /// Hub label, e.g. `"Saturation"`, `"Gamma 2"`, `"Relative Colorimetric"`, `"None"`.
    pub correction: Option<String>,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

#[derive(Debug, Clone, Default)]
pub struct About {
    pub hub_version: String,
    pub type_name: String,
    /// Landscape pixel size at 300 dpi as stored in the file (1013x642 for CR80).
    pub width_px: u32,
    pub height_px: u32,
    pub double_sided: bool,
    pub portrait: bool,
    pub dark_card: bool,
    /// Feature flags that we do not render/handle (chip, stripe, ...).
    pub features: Vec<String>,
    /// `about.preview` decoded PNG bytes (front, 0.5x), if present.
    pub preview_png: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Side {
    pub background: Element,
    pub elements: Vec<Element>,
}

/// One Konva node attribute dictionary.
#[derive(Debug, Clone, Default)]
pub struct Element {
    pub attrs: Map<String, Value>,
}

impl Element {
    pub fn num(&self, key: &str) -> Option<f64> {
        match self.attrs.get(key)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().trim_end_matches("px").parse::<f64>().ok(),
            _ => None,
        }
        .filter(|v| v.is_finite())
    }
    pub fn num_or(&self, key: &str, d: f64) -> f64 {
        self.num(key).unwrap_or(d)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.attrs.get(key)?.as_str()
    }
    pub fn boolean(&self, key: &str) -> Option<bool> {
        self.attrs.get(key)?.as_bool()
    }
    pub fn has(&self, key: &str) -> bool {
        self.attrs.get(key).map(|v| !v.is_null()).unwrap_or(false)
    }
    pub fn class_name(&self) -> &str {
        self.str("className").unwrap_or("")
    }
    /// Hub's logical type (`Text`, `Rect`, `Image`, `QRCode`, `Barcode`, ...).
    pub fn name(&self) -> &str {
        self.str("name").unwrap_or("")
    }
    pub fn visible(&self) -> bool {
        self.boolean("visible").unwrap_or(true)
    }
    pub fn num_array(&self, key: &str) -> Option<Vec<f64>> {
        let arr = self.attrs.get(key)?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for v in arr {
            out.push(match v {
                Value::Number(n) => n.as_f64()?,
                Value::String(s) => s.parse().ok()?,
                _ => return None,
            });
        }
        Some(out)
    }
    /// Gradient colour stops `[offset, colour, offset, colour, ...]`.
    pub fn stops(&self, key: &str) -> Option<Vec<(f32, String)>> {
        let arr = self.attrs.get(key)?.as_array()?;
        let mut out = Vec::new();
        for pair in arr.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let off = match &pair[0] {
                Value::Number(n) => n.as_f64()? as f32,
                Value::String(s) => s.parse().ok()?,
                _ => return None,
            };
            out.push((off, pair[1].as_str()?.to_string()));
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

impl Card {
    pub fn parse(bytes: &[u8]) -> Result<Card, Error> {
        // Tolerate a UTF-8 BOM.
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let root: Value = serde_json::from_slice(bytes).map_err(|e| Error::Parse(e.to_string()))?;
        let obj = root
            .as_object()
            .ok_or_else(|| Error::Parse("top level is not a JSON object".into()))?;
        let about = parse_about(obj.get("about"))?;
        let default_w = if about.portrait { about.height_px } else { about.width_px };
        let default_h = if about.portrait { about.width_px } else { about.height_px };
        let adjustments = parse_adjustments(obj.get("adjustments"));
        let front = parse_side(obj.get("front"), default_w, default_h)
            .map_err(|e| Error::Parse(format!("front: {e}")))?;
        let back = parse_side(obj.get("back"), default_w, default_h)
            .map_err(|e| Error::Parse(format!("back: {e}")))?;
        Ok(Card { about, adjustments, front, back })
    }

    /// Updates `adjustments.correction` in the JSON file (creates the block if missing).
    /// Other adjustment fields are left untouched.
    pub fn write_colour_correction(path: &std::path::Path, correction: &str) -> Result<(), Error> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        let mut root: Value = serde_json::from_slice(&bytes).map_err(|e| Error::Parse(e.to_string()))?;
        let obj = root
            .as_object_mut()
            .ok_or_else(|| Error::Parse("top level is not a JSON object".into()))?;
        let adj = obj.entry("adjustments".to_string()).or_insert_with(|| Value::Object(Map::new()));
        let adj = adj
            .as_object_mut()
            .ok_or_else(|| Error::Parse("`adjustments` is not an object".into()))?;
        adj.insert("correction".into(), Value::String(correction.to_string()));
        let out = serde_json::to_vec_pretty(&root).map_err(|e| Error::Io(e.to_string()))?;
        std::fs::write(path, out).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        Ok(())
    }

    pub fn from_file(path: &std::path::Path) -> Result<Card, Error> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        Card::parse(&bytes)
    }

    pub fn side(&self, back: bool) -> &Side {
        if back {
            &self.back
        } else {
            &self.front
        }
    }

    /// Size of the design frame in pixels (swapped for portrait cards).
    pub fn frame_size(&self) -> (u32, u32) {
        let w = self.front.background.num("width").unwrap_or(0.0);
        let h = self.front.background.num("height").unwrap_or(0.0);
        if w >= 1.0 && h >= 1.0 {
            (w.round() as u32, h.round() as u32)
        } else if self.about.portrait {
            (self.about.height_px, self.about.width_px)
        } else {
            (self.about.width_px, self.about.height_px)
        }
    }
}

fn parse_adjustments(v: Option<&Value>) -> Adjustments {
    let Some(o) = v.and_then(|v| v.as_object()) else {
        return Adjustments::default();
    };
    let num = |k: &str, d: f64| -> f64 {
        match o.get(k) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(d),
            Some(Value::String(s)) => s.parse().unwrap_or(d),
            _ => d,
        }
    };
    Adjustments {
        correction: o
            .get("correction")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        brightness: num("brightness", 0.0),
        contrast: num("contrast", 0.0),
        saturation: num("saturation", 0.0),
        red: num("red", 1.0),
        green: num("green", 1.0),
        blue: num("blue", 1.0),
    }
}

fn parse_about(v: Option<&Value>) -> Result<About, Error> {
    let o = v
        .and_then(|v| v.as_object())
        .ok_or_else(|| Error::Parse("missing `about` object".into()))?;
    let b = |k: &str| o.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let ty = o.get("type").and_then(|v| v.as_object());
    let tn = |k: &str| -> Option<f64> {
        match ty?.get(k)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    };
    let card_size = tn("CardSize").map(|v| v as i64);
    let (dw, dh) = match card_size {
        Some(1) => (991, 615),
        Some(2) => (1288, 642),
        Some(3) => (1654, 642),
        _ => (1013, 642),
    };
    let mut features = Vec::new();
    for (k, label) in [
        ("ISOstripe", "ISO magnetic stripe"),
        ("JISstripe", "JIS magnetic stripe"),
        ("smartChip6pin", "6-pin chip cut-out"),
        ("smartChip8pin", "8-pin chip cut-out"),
        ("holopatch", "HoloPatch area"),
        ("cutout", "slot cut-out"),
    ] {
        if b(k) {
            features.push(label.to_string());
        }
    }
    let preview_png = o
        .get("preview")
        .and_then(|v| v.as_str())
        .and_then(|s| crate::decode_data_url(s).ok());
    Ok(About {
        hub_version: o.get("hubVersion").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        type_name: ty.and_then(|t| t.get("name")).and_then(|v| v.as_str()).unwrap_or("CR80").to_string(),
        width_px: tn("widthPixels").map(|v| v as u32).filter(|v| *v > 0).unwrap_or(dw),
        height_px: tn("heightPixels").map(|v| v as u32).filter(|v| *v > 0).unwrap_or(dh),
        double_sided: b("doubleSided"),
        portrait: b("portrait"),
        dark_card: b("darkCard"),
        features,
        preview_png,
    })
}

fn parse_side(v: Option<&Value>, dw: u32, dh: u32) -> Result<Side, String> {
    let o = v.and_then(|v| v.as_object()).ok_or("missing side object")?;
    let mut background = Element {
        attrs: o
            .get("background")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default(),
    };
    if background.num("width").is_none() {
        background.attrs.insert("width".into(), dw.into());
    }
    if background.num("height").is_none() {
        background.attrs.insert("height".into(), dh.into());
    }
    let mut elements = Vec::new();
    if let Some(arr) = o.get("elements").and_then(|v| v.as_array()) {
        for e in arr {
            if let Some(m) = e.as_object() {
                elements.push(Element { attrs: m.clone() });
            }
        }
    }
    Ok(Side { background, elements })
}

//! Writing the `adjustments` block back into a `.card` file.
//!
//! The file is edited as text: only the bytes of the top-level `"adjustments"` value are replaced
//! (or the block is appended if the file has none). Everything else, including multi-megabyte base64
//! images, key order, indentation, line endings (Hub writes CRLF) and a possible BOM, stays byte for byte.
//! The block is written in Hub's own layout and key order:
//! `correction, brightness, contrast, saturation, red, green, blue`.
//! Keys inside an existing block that we do not know are kept (after ours).

use std::path::Path;

use serde_json::{Map, Value};

use super::Adjustments;
use crate::Error;

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// `i` points at the opening quote; returns the index after the closing quote.
fn skip_string(b: &[u8], mut i: usize) -> Result<usize, Error> {
    debug_assert_eq!(b[i], b'"');
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err(Error::Parse("unterminated string".into()))
}

/// Returns the index after the JSON value starting at `i`.
fn skip_value(b: &[u8], mut i: usize) -> Result<usize, Error> {
    match b.get(i) {
        None => Err(Error::Parse("unexpected end of file".into())),
        Some(b'"') => skip_string(b, i),
        Some(b'{') | Some(b'[') => {
            let mut depth = 0usize;
            while i < b.len() {
                match b[i] {
                    b'"' => {
                        i = skip_string(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            Err(Error::Parse("unterminated object or array".into()))
        }
        Some(_) => {
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            Ok(i)
        }
    }
}

struct TopLevel {
    /// byte offset of the first character after the opening `{`
    open_end: usize,
    /// (start, end) of the `"adjustments"` value, if present
    adjustments: Option<(usize, usize)>,
    /// end of the last member's value, if any member exists
    last_value_end: Option<usize>,
    /// whitespace between `{` and the first key (e.g. "\r\n  "), if any member exists
    first_gap: Option<(usize, usize)>,
}

fn scan_top_level(text: &str) -> Result<TopLevel, Error> {
    let b = text.as_bytes();
    let mut i = skip_ws(b, if text.starts_with('\u{feff}') { 3 } else { 0 });
    if b.get(i) != Some(&b'{') {
        return Err(Error::Parse("top level is not a JSON object".into()));
    }
    i += 1;
    let open_end = i;
    let mut t = TopLevel { open_end, adjustments: None, last_value_end: None, first_gap: None };
    loop {
        let ws_start = i;
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b'}') => return Ok(t),
            Some(b'"') => {}
            _ => return Err(Error::Parse("malformed top-level object".into())),
        }
        if t.first_gap.is_none() {
            t.first_gap = Some((ws_start, i));
        }
        let key_end = skip_string(b, i)?;
        let key = &text[i + 1..key_end - 1];
        i = skip_ws(b, key_end);
        if b.get(i) != Some(&b':') {
            return Err(Error::Parse("expected ':' after key".into()));
        }
        i = skip_ws(b, i + 1);
        let vstart = i;
        let vend = skip_value(b, i)?;
        if key == "adjustments" {
            t.adjustments = Some((vstart, vend));
        }
        t.last_value_end = Some(vend);
        i = skip_ws(b, vend);
        match b.get(i) {
            Some(b',') => i += 1,
            Some(b'}') => return Ok(t),
            _ => return Err(Error::Parse("expected ',' or '}'".into())),
        }
    }
}

fn num(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e12 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// Renders the block in Hub's layout. `indent` is one indentation level (usually two spaces), `nl` the newline
/// style; `base` is the indentation of the line holding the key. `indent == None` renders compact JSON.
fn render_block(adj: &Adjustments, extra: &Map<String, Value>, indent: Option<&str>, nl: &str, base: &str) -> String {
    let a = adj.clamped();
    let mut items: Vec<(String, String)> = vec![
        ("correction".into(), format!("\"{}\"", a.correction.json_name())),
        ("brightness".into(), num(a.brightness)),
        ("contrast".into(), num(a.contrast)),
        ("saturation".into(), num(a.saturation)),
        ("red".into(), num(a.red)),
        ("green".into(), num(a.green)),
        ("blue".into(), num(a.blue)),
    ];
    for (k, v) in extra {
        if !items.iter().any(|(n, _)| n.eq_ignore_ascii_case(k)) {
            items.push((k.clone(), serde_json::to_string(v).unwrap_or_else(|_| "null".into())));
        }
    }
    match indent {
        Some(ind) => {
            let mut s = String::from("{");
            for (n, (k, v)) in items.iter().enumerate() {
                s.push_str(nl);
                s.push_str(base);
                s.push_str(ind);
                s.push_str(&format!("{}: {}", serde_json::to_string(k).unwrap(), v));
                if n + 1 < items.len() {
                    s.push(',');
                }
            }
            s.push_str(nl);
            s.push_str(base);
            s.push('}');
            s
        }
        None => {
            let body: Vec<String> = items.iter().map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), v)).collect();
            format!("{{{}}}", body.join(","))
        }
    }
}

/// Returns `text` with its `adjustments` block set to `adj` (see the module docs for what is preserved).
pub fn patch_adjustments(text: &str, adj: &Adjustments) -> Result<String, Error> {
    let t = scan_top_level(text)?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    // Indentation level: whitespace after the last newline in the gap before the first key.
    let (indent, pretty) = match t.first_gap {
        Some((s, e)) => {
            let gap = &text[s..e];
            match gap.rfind('\n') {
                Some(p) => (gap[p + 1..].to_string(), true),
                None => (String::new(), false),
            }
        }
        None => ("  ".to_string(), true),
    };
    let one_level = if indent.is_empty() { "  ".to_string() } else { indent.clone() };
    match t.adjustments {
        Some((s, e)) => {
            let extra = serde_json::from_str::<Value>(&text[s..e]).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
            let block = render_block(adj, &extra, pretty.then_some(one_level.as_str()), nl, &indent);
            Ok(format!("{}{}{}", &text[..s], block, &text[e..]))
        }
        None => {
            let block = render_block(adj, &Map::new(), pretty.then_some(one_level.as_str()), nl, &indent);
            let member = if pretty { format!("\"adjustments\": {block}") } else { format!("\"adjustments\":{block}") };
            match t.last_value_end {
                Some(end) => {
                    let sep = if pretty { format!(",{nl}{indent}") } else { ",".to_string() };
                    Ok(format!("{}{}{}{}", &text[..end], sep, member, &text[end..]))
                }
                None => {
                    let lead = if pretty { format!("{nl}{one_level}") } else { String::new() };
                    let trail = if pretty { nl.to_string() } else { String::new() };
                    Ok(format!("{}{}{}{}{}", &text[..t.open_end], lead, member, trail, &text[t.open_end..]))
                }
            }
        }
    }
}

/// Saves `adj` into the `.card` file at `path`: reads it, patches the block, checks that nothing else
/// changed (parsed JSON comparison), writes a temp file next to it and renames it over the original.
pub fn save_adjustments(path: &Path, adj: &Adjustments) -> Result<(), Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    let text = String::from_utf8(bytes).map_err(|_| Error::Parse("file is not valid UTF-8".into()))?;
    let patched = patch_adjustments(&text, adj)?;
    verify_only_adjustments_changed(&text, &patched, adj)?;
    let tmp = path.with_extension("card.carddeck-tmp");
    std::fs::write(&tmp, patched.as_bytes()).map_err(|e| Error::Io(format!("{}: {e}", tmp.display())))?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Io(format!("could not replace {}: {e}", path.display()))
    })
}

/// Safety net: the patched text must parse, `adjustments` must read back as `adj`, and every other
/// top-level member must be equal to the original.
pub fn verify_only_adjustments_changed(original: &str, patched: &str, adj: &Adjustments) -> Result<(), Error> {
    let strip = |s: &str| -> Result<Value, Error> {
        serde_json::from_str::<Value>(s.strip_prefix('\u{feff}').unwrap_or(s)).map_err(|e| Error::Parse(e.to_string()))
    };
    let mut a = strip(original)?;
    let mut b = strip(patched)?;
    let (Some(ao), Some(bo)) = (a.as_object_mut(), b.as_object_mut()) else {
        return Err(Error::Parse("top level is not an object".into()));
    };
    let new_adj = bo.remove("adjustments");
    ao.remove("adjustments");
    if ao != bo {
        return Err(Error::Parse("internal check failed: other parts of the file would change; nothing written".into()));
    }
    if Adjustments::from_json(new_adj.as_ref()) != adj.clamped() {
        return Err(Error::Parse("internal check failed: adjustments did not read back; nothing written".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Correction;
    use super::*;

    fn sample(nl: &str) -> String {
        [
            "{",
            "  \"about\": {",
            "    \"preview\": \"data:image/png;base64,AAAA\\\"}{\",",
            "    \"hubVersion\": \"2.0.8\"",
            "  },",
            "  \"front\": { \"elements\": [ 1, 2.50, {\"a\": \"}\"} ] },",
            "  \"adjustments\": {",
            "    \"correction\": \"Saturation\",",
            "    \"brightness\": 0,",
            "    \"contrast\": 0,",
            "    \"saturation\": 0,",
            "    \"red\": 1,",
            "    \"green\": 1.061062,",
            "    \"blue\": 1",
            "  }",
            "}",
        ]
        .join(nl)
    }

    #[test]
    fn replaces_only_the_block_and_keeps_crlf() {
        let text = sample("\r\n");
        let adj = Adjustments { correction: Correction::Perceptual, brightness: 0.25, green: 1.1, ..Adjustments::default() };
        let out = patch_adjustments(&text, &adj).unwrap();
        let i = text.find("\"adjustments\"").unwrap();
        assert_eq!(&out[..i], &text[..i], "everything before the block is untouched");
        assert!(out.ends_with("\r\n  }\r\n}"));
        assert!(out.contains("\"correction\": \"Perceptual\",\r\n    \"brightness\": 0.25,"));
        assert!(!out.contains("\r\r") && !out.replace("\r\n", "").contains('\n'));
        verify_only_adjustments_changed(&text, &out, &adj).unwrap();
        // Writing the same values again gives the same bytes (idempotent).
        assert_eq!(patch_adjustments(&out, &adj).unwrap(), out);
    }

    #[test]
    fn unchanged_values_reproduce_hubs_text_exactly() {
        let text = sample("\r\n");
        let v: Value = serde_json::from_str(&text).unwrap();
        let adj = Adjustments::from_json(v.get("adjustments"));
        assert_eq!(patch_adjustments(&text, &adj).unwrap(), text);
    }

    #[test]
    fn appends_block_when_missing() {
        let text = "{\n  \"about\": {},\n  \"front\": {}\n}".to_string();
        let adj = Adjustments { correction: Correction::None, ..Adjustments::default() };
        let out = patch_adjustments(&text, &adj).unwrap();
        assert!(out.starts_with("{\n  \"about\": {},\n  \"front\": {},\n  \"adjustments\": {\n    \"correction\": \"None\","));
        assert!(out.ends_with("\n  }\n}"));
        verify_only_adjustments_changed(&text, &out, &adj).unwrap();
    }

    #[test]
    fn keeps_unknown_keys_inside_the_block_and_handles_bom_and_compact() {
        let text = "\u{feff}{\"a\":1,\"adjustments\":{\"correction\":\"Gamma2\",\"extra\":[1,2]}}".to_string();
        let adj = Adjustments { correction: Correction::AbsoluteColorimetric, red: 1.25, ..Adjustments::default() };
        let out = patch_adjustments(&text, &adj).unwrap();
        assert!(out.starts_with('\u{feff}'));
        assert!(out.contains("\"correction\":\"AbsoluteColorimetric\""));
        assert!(out.contains("\"red\":1.25"));
        assert!(out.contains("\"extra\":[1,2]"));
        verify_only_adjustments_changed(&text, &out, &adj).unwrap();
        let empty = patch_adjustments("{}", &adj).unwrap();
        serde_json::from_str::<Value>(&empty).unwrap();
    }

    #[test]
    fn rejects_non_objects() {
        assert!(patch_adjustments("[1,2]", &Adjustments::default()).is_err());
        assert!(patch_adjustments("{\"a\": ", &Adjustments::default()).is_err());
    }

    #[test]
    fn save_adjustments_writes_atomically_and_preserves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.card");
        std::fs::write(&p, sample("\r\n")).unwrap();
        let adj = Adjustments { correction: Correction::RelativeColorimetric, contrast: -3.5, ..Adjustments::default() };
        save_adjustments(&p, &adj).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(Adjustments::from_json(v.get("adjustments")), adj);
        assert!(!dir.path().join("x.card.carddeck-tmp").exists());
        let i = sample("\r\n").find("\"adjustments\"").unwrap();
        assert_eq!(&text[..i], &sample("\r\n")[..i]);
    }
}

//! CardDeck core: `.card` parsing, rendering and print-job building.
pub mod calibration;
pub mod fonts;
pub mod model;
pub mod print;
pub mod qr;
pub mod render;

pub use model::Card;
pub use render::{render_side, RenderOptions, Rendered};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read card file: {0}")]
    Io(String),
    #[error("not a valid .card file: {0}")]
    Parse(String),
    #[error("{0}")]
    Print(String),
}

/// Decodes `data:image/png;base64,....` (or plain base64) into bytes.
pub fn decode_data_url(s: &str) -> Result<Vec<u8>, Error> {
    use base64::Engine;
    let b64 = match s.find(',') {
        Some(i) if s.starts_with("data:") => &s[i + 1..],
        _ => s,
    };
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| Error::Parse(format!("bad base64: {e}")))
}

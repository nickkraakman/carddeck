//! QR code regeneration (used only if a card's stored QR bitmap is missing).

use qrcode::{Color as QColor, EcLevel, QrCode};
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Module matrix (true = dark), no quiet zone.
pub fn qr_modules(data: &str) -> Option<(usize, Vec<bool>)> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).ok()?;
    let w = code.width();
    let v = code.to_colors().into_iter().map(|c| c == QColor::Dark).collect();
    Some((w, v))
}

/// One pixel per module, black on transparent-white, no quiet zone (bwip-js `padding: 0`).
pub fn qr_pixmap(data: &str) -> Option<Pixmap> {
    let (w, m) = qr_modules(data)?;
    let mut pm = Pixmap::new(w as u32, w as u32)?;
    for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
        *px = if m[i] {
            PremultipliedColorU8::from_rgba(0, 0, 0, 255)?
        } else {
            PremultipliedColorU8::from_rgba(255, 255, 255, 255)?
        };
    }
    Some(pm)
}

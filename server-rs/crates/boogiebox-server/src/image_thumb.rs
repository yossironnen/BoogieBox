//! Defines Rust server support logic for Image Thumb.

use image::{imageops::FilterType, ImageReader};
use std::path::Path;

/// Documents the Generate Thumbnail public API surface.
pub fn generate_thumbnail(
    source_path: &Path,
    dest_path: &Path,
    max_size: u32,
) -> Result<(), String> {
    let img = ImageReader::open(source_path)
        .map_err(|e| format!("open: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("format: {e}"))?
        .decode()
        .map_err(|e| format!("decode: {e}"))?;

    let (w, h) = (img.width(), img.height());
    let (new_w, new_h) = if w <= max_size && h <= max_size {
        (w, h)
    } else {
        let scale = max_size as f64 / w.max(h) as f64;
        (
            (w as f64 * scale).round() as u32,
            (h as f64 * scale).round() as u32,
        )
    };

    let resized = img.resize(new_w, new_h, FilterType::Lanczos3);

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }

    resized.save(dest_path).map_err(|e| format!("save: {e}"))?;
    Ok(())
}

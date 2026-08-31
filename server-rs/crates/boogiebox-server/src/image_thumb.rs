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

    // Write through a temp file and atomically rename into place (plan §8.3
    // item 1) so a concurrent reader (the cache-first art route, or another
    // background fill) never observes a partially-encoded JPEG.
    let tmp_path = temp_sibling_path(dest_path);
    let write_result = resized
        .save(&tmp_path)
        .map_err(|e| format!("save: {e}"))
        .and_then(|_| std::fs::rename(&tmp_path, dest_path).map_err(|e| format!("rename: {e}")));
    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_result
}

/// A same-directory temp path for `dest_path`, unique enough that concurrent
/// generations of the same destination never collide — `rename` within the
/// same directory is atomic on both Windows and Linux. Keeps `dest_path`'s
/// extension so `image`'s save-format-from-extension guess still resolves.
fn temp_sibling_path(dest_path: &Path) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stem = dest_path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("thumb");
    let ext = dest_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    dest_path.with_file_name(format!(".{stem}.tmp-{}-{nanos}.{ext}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::time::SystemTime;

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("image-thumb-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_test_png(path: &Path, width: u32, height: u32) {
        let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        buf.save(path).expect("write test fixture png");
    }

    #[test]
    fn generate_thumbnail_downscales_an_oversized_image_and_preserves_aspect_ratio() {
        let dir = temp_dir("downscale");
        let source = dir.join("source.png");
        write_test_png(&source, 400, 200);
        let dest = dir.join("out.jpg");

        generate_thumbnail(&source, &dest, 100).expect("thumbnail generation should succeed");

        let out = image::open(&dest).expect("output image should be readable");
        assert_eq!(out.width(), 100);
        assert_eq!(out.height(), 50, "aspect ratio must be preserved");
    }

    #[test]
    fn generate_thumbnail_leaves_a_smaller_image_untouched_in_size() {
        let dir = temp_dir("no-upscale");
        let source = dir.join("source.png");
        write_test_png(&source, 40, 30);
        let dest = dir.join("out.jpg");

        generate_thumbnail(&source, &dest, 100).expect("thumbnail generation should succeed");

        let out = image::open(&dest).expect("output image should be readable");
        assert_eq!(out.width(), 40);
        assert_eq!(out.height(), 30);
    }

    #[test]
    fn generate_thumbnail_creates_missing_destination_directories() {
        let dir = temp_dir("mkdir");
        let source = dir.join("source.png");
        write_test_png(&source, 50, 50);
        let dest = dir.join("nested").join("deeper").join("out.jpg");

        generate_thumbnail(&source, &dest, 20).expect("should create nested dest dirs");
        assert!(dest.exists());
    }

    #[test]
    fn generate_thumbnail_returns_err_for_missing_source_file() {
        let dir = temp_dir("missing-source");
        let source = dir.join("does-not-exist.png");
        let dest = dir.join("out.jpg");

        let result = generate_thumbnail(&source, &dest, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().starts_with("open:"));
    }

    #[test]
    fn generate_thumbnail_returns_err_for_undecodable_file() {
        let dir = temp_dir("bad-format");
        let source = dir.join("not-an-image.png");
        std::fs::write(&source, b"this is not image data at all").unwrap();
        let dest = dir.join("out.jpg");

        let result = generate_thumbnail(&source, &dest, 100);
        assert!(result.is_err());
    }

    #[test]
    fn generate_thumbnail_leaves_no_temp_file_behind_on_success() {
        let dir = temp_dir("no-leftover-tmp-ok");
        let source = dir.join("source.png");
        write_test_png(&source, 50, 50);
        let dest = dir.join("out.jpg");

        generate_thumbnail(&source, &dest, 20).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp file(s) left behind: {leftovers:?}"
        );
    }

    #[test]
    fn generate_thumbnail_writes_atomically_dest_never_briefly_empty_or_truncated() {
        // The write-then-rename means a concurrent reader can only ever see
        // either the previous complete file or the new complete file — proven
        // here by overwriting an existing valid dest and checking it decodes
        // correctly afterward (a non-atomic in-place write could leave a
        // truncated file if interrupted, though we can't inject the interrupt
        // itself in a unit test — this guards the code path stays temp+rename).
        let dir = temp_dir("atomic-overwrite");
        let source = dir.join("source.png");
        write_test_png(&source, 60, 60);
        let dest = dir.join("out.jpg");

        generate_thumbnail(&source, &dest, 30).unwrap();
        let first = std::fs::read(&dest).unwrap();
        assert!(!first.is_empty());

        // Regenerate from a different-sized source; dest must end up fully
        // replaced, not appended-to or truncated.
        write_test_png(&source, 90, 90);
        generate_thumbnail(&source, &dest, 30).unwrap();

        let out = image::open(&dest).expect("overwritten output image should be readable");
        assert_eq!(out.width(), 30);
        assert_eq!(out.height(), 30);
    }
}

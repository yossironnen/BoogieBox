//! Generates the shareable "story image" PNG for a rendered BoogieMix
//! (wip/boogiemix-story-timeline-plan.md §4.4.2, Phase 3) — a poster-style
//! recap: cover collage, name/stats, a colored+waveform timeline strip, and
//! the ordered tracklist, all baked into one PNG a user can download and
//! send to someone else.
//!
//! Pure `image`/`imageproc`/`ab_glyph` (no C bindings, Windows+Linux safe).
//! Text uses Cascadia Mono, embedded at compile time — it's already the
//! app's own CSS monospace fallback (`client/src/base.css`), and its SIL
//! Open Font License explicitly permits bundling (see
//! `assets/fonts/LICENSE-CascadiaMono.txt`).

use ab_glyph::{FontRef, PxScale};
use boogiebox_db::boogiemix::{MixOutputRow, MixOutputTrackRow};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use std::path::{Path, PathBuf};

static FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/CascadiaMono.ttf");

const WIDTH: u32 = 1200;
const MARGIN: i32 = 40;
const COLLAGE_SIZE: u32 = 140;
const TIMELINE_HEIGHT: i32 = 130;
const TRACK_ROW_HEIGHT: i32 = 28;
const MAX_TRACK_ROWS: usize = 12;

const BG: Rgba<u8> = Rgba([9, 9, 11, 255]);
const SURFACE_SUBTLE: Rgba<u8> = Rgba([24, 24, 27, 255]);
const TEXT: Rgba<u8> = Rgba([228, 228, 231, 255]);
const TEXT_MUTED: Rgba<u8> = Rgba([113, 113, 122, 255]);
const ACCENT: Rgba<u8> = Rgba([99, 102, 241, 255]);
const WAVEFORM_COLOR: Rgba<u8> = Rgba([255, 255, 255, 255]);
const TRACK_THUMB_SIZE: u32 = 20;

// Same fixed palette + rolling hash as the client's `hashToFallbackColor`
// (client/src/components/MixStoryView.tsx) — bit-identical results for ASCII
// keys (track ids are UUIDs), so the downloaded PNG and the in-app view pick
// the same fallback color for the same track.
const FALLBACK_PALETTE: [[u8; 3]; 8] = [
    [0x6d, 0x5c, 0xe0],
    [0xc2, 0x47, 0x7d],
    [0xd9, 0x77, 0x06],
    [0x1c, 0x9c, 0x6d],
    [0x2b, 0x8b, 0xd1],
    [0xa3, 0x4c, 0xd6],
    [0xc2, 0x41, 0x0c],
    [0x0e, 0x94, 0x88],
];

fn hash_to_fallback_color(key: &str) -> Rgba<u8> {
    let mut hash: i32 = 0;
    for c in key.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(c as i32);
    }
    let idx = (hash.unsigned_abs() as usize) % FALLBACK_PALETTE.len();
    let [r, g, b] = FALLBACK_PALETTE[idx];
    Rgba([r, g, b, 255])
}

/// A track with no resolvable album id snapshots as `track_id::None` far
/// more often than as a missing title/artist, so the fallback key prefers
/// the track id (stable, ASCII) and only falls back to title+artist for the
/// rare legacy row that has neither.
fn fallback_key(track: &MixOutputTrackRow) -> String {
    match &track.track_id {
        Some(id) => id.to_string(),
        None => format!("{}::{}", track.title, track.artist_name),
    }
}

/// Manually alpha-blends `color` over `canvas` within the given rect and
/// writes back a fully opaque pixel. Plain `imageproc` drawing functions on
/// an `RgbaImage` overwrite pixels rather than blending them, so a
/// translucent rect drawn that way would erase the artwork underneath
/// instead of darkening it — this is what makes the scrim actually a scrim.
fn blend_rect_alpha(
    canvas: &mut RgbaImage,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: [u8; 3],
    alpha: f32,
) {
    let (cw, ch) = canvas.dimensions();
    let a = alpha.clamp(0.0, 1.0);
    for row in 0..h {
        let py = y + row;
        if py < 0 || py as u32 >= ch {
            continue;
        }
        for col in 0..w {
            let px = x + col;
            if px < 0 || px as u32 >= cw {
                continue;
            }
            let pixel = canvas.get_pixel_mut(px as u32, py as u32);
            for c in 0..3 {
                pixel[c] = ((color[c] as f32) * a + (pixel[c] as f32) * (1.0 - a)).round() as u8;
            }
            pixel[3] = 255;
        }
    }
}

/// Vertical scrim gradient (lighter at top, darker at bottom) over a rect —
/// mirrors the client's per-segment
/// `linear-gradient(180deg, rgba(0,0,0,.15) 0%, rgba(0,0,0,.55) 100%)`.
fn draw_vertical_scrim(canvas: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32) {
    const ALPHA_TOP: f32 = 0.15;
    const ALPHA_BOTTOM: f32 = 0.55;
    for row in 0..h {
        let t = if h > 1 {
            row as f32 / (h - 1) as f32
        } else {
            0.0
        };
        let alpha = ALPHA_TOP + (ALPHA_BOTTOM - ALPHA_TOP) * t;
        blend_rect_alpha(canvas, x, y + row, w, 1, [0, 0, 0], alpha);
    }
}

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BYTES).expect("bundled CascadiaMono.ttf must parse")
}

fn text_w(font: &FontRef, scale: f32, text: &str) -> i32 {
    text_size(PxScale::from(scale), font, text).0 as i32
}

/// Root directory for cached 300px album art thumbnails
/// (`{db_folder}/art/album/thumb/300`) — mirrors `artwork_routes.rs`'s
/// private `album_art_thumb_root`, duplicated here rather than shared since
/// that helper takes a `SharedState` this module has no reason to depend on.
pub fn album_art_thumb_root(db_folder: &Path) -> PathBuf {
    db_folder
        .join("art")
        .join("album")
        .join("thumb")
        .join("300")
}

fn load_album_thumb(thumb_root: &Path, album_id: &str) -> Option<RgbaImage> {
    let cache_key = crate::artwork_cache::build_album_art_cache_key(album_id);
    let item_dir = crate::artwork_cache::cache_item_dir(thumb_root, &cache_key);
    let path = crate::artwork_cache::find_existing_cached_image(&item_dir)?;
    Some(image::open(path).ok()?.into_rgba8())
}

fn draw_collage(canvas: &mut RgbaImage, thumb_root: &Path, album_ids: &[String], x: i32, y: i32) {
    draw_filled_rect_mut(
        canvas,
        Rect::at(x, y).of_size(COLLAGE_SIZE, COLLAGE_SIZE),
        SURFACE_SUBTLE,
    );
    if album_ids.is_empty() {
        return;
    }
    let tile = COLLAGE_SIZE / 2;
    for (i, album_id) in album_ids.iter().take(4).enumerate() {
        let Some(thumb) = load_album_thumb(thumb_root, album_id) else {
            continue;
        };
        let resized =
            image::imageops::resize(&thumb, tile, tile, image::imageops::FilterType::Triangle);
        let tx = x + (i as i32 % 2) * tile as i32;
        let ty = y + (i as i32 / 2) * tile as i32;
        image::imageops::overlay(canvas, &resized, tx as i64, ty as i64);
    }
}

fn draw_timeline(
    canvas: &mut RgbaImage,
    thumb_root: &Path,
    tracks: &[MixOutputTrackRow],
    total_duration: f64,
    x: i32,
    y: i32,
    width: i32,
) {
    draw_filled_rect_mut(
        canvas,
        Rect::at(x, y).of_size(width.max(0) as u32, TIMELINE_HEIGHT as u32),
        SURFACE_SUBTLE,
    );
    if total_duration <= 0.0 || tracks.is_empty() {
        return;
    }

    // Pass 1: each segment's real artwork (cover-fit), or a deterministic
    // fallback color when no cached thumbnail resolves.
    let mut cursor_x = x;
    let mut segments: Vec<(i32, i32)> = Vec::with_capacity(tracks.len());
    for track in tracks {
        let span = (track.output_end_sec - track.output_start_sec).max(0.0);
        let seg_w = ((span / total_duration) * width as f64).round() as i32;
        let seg_w = seg_w.max(1).min(x + width - cursor_x);
        if seg_w <= 0 {
            break;
        }

        let thumb = track
            .album_id
            .as_ref()
            .and_then(|id| load_album_thumb(thumb_root, &id.to_string()));
        match thumb {
            Some(img) => {
                let resized = image::imageops::resize(
                    &img,
                    seg_w.max(1) as u32,
                    TIMELINE_HEIGHT as u32,
                    image::imageops::FilterType::Triangle,
                );
                image::imageops::overlay(canvas, &resized, cursor_x as i64, y as i64);
            }
            None => {
                let color = hash_to_fallback_color(&fallback_key(track));
                draw_filled_rect_mut(
                    canvas,
                    Rect::at(cursor_x, y).of_size(seg_w as u32, TIMELINE_HEIGHT as u32),
                    color,
                );
            }
        }
        segments.push((cursor_x, seg_w));
        cursor_x += seg_w;
    }

    // Pass 2: one scrim gradient across the whole drawn width so text and
    // waveform bars stay legible over busy photos — done once here (not
    // per segment) since it's visually identical either way and alpha
    // blending must run after every segment's artwork is already in place.
    draw_vertical_scrim(canvas, x, y, cursor_x - x, TIMELINE_HEIGHT);

    // Pass 3: waveform bars + separators, on top of the scrim.
    for (track, &(seg_x, seg_w)) in tracks.iter().zip(segments.iter()) {
        if let Some(peaks) = track
            .waveform_peaks_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<Vec<f64>>(j).ok())
            .filter(|p| !p.is_empty())
        {
            let max_peak = peaks.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
            let bar_count = peaks.len().min(seg_w.max(1) as usize).max(1);
            let bar_w = (seg_w as f64 / bar_count as f64).max(1.0);
            for i in 0..bar_count {
                let peak = peaks[i * peaks.len() / bar_count];
                let bar_h = ((peak / max_peak) * (TIMELINE_HEIGHT as f64 - 8.0)).max(2.0) as u32;
                let bx = seg_x + (i as f64 * bar_w).round() as i32;
                let bw = bar_w.round().max(1.0) as u32;
                draw_filled_rect_mut(
                    canvas,
                    Rect::at(bx, y + TIMELINE_HEIGHT - bar_h as i32).of_size(bw, bar_h),
                    WAVEFORM_COLOR,
                );
            }
        }

        // Thin separator between segments.
        draw_filled_rect_mut(
            canvas,
            Rect::at(seg_x + seg_w - 1, y).of_size(1, TIMELINE_HEIGHT as u32),
            Rgba([0, 0, 0, 140]),
        );
    }
}

fn format_duration(sec: f64) -> String {
    if sec <= 0.0 {
        return "0:00".to_string();
    }
    let total = sec.round() as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Renders the full story-image PNG for one mix. Synchronous/blocking (file
/// IO + pixel math) — callers should run it via `spawn_blocking`.
pub fn render_story_image(
    output: &MixOutputRow,
    tracks: &[MixOutputTrackRow],
    db_folder: &Path,
) -> Vec<u8> {
    let thumb_root = album_art_thumb_root(db_folder);
    let font = font();

    let shown_rows = tracks.len().min(MAX_TRACK_ROWS);
    let truncated = tracks.len() > MAX_TRACK_ROWS;
    let track_list_h =
        shown_rows as i32 * TRACK_ROW_HEIGHT + if truncated { TRACK_ROW_HEIGHT } else { 0 };
    let header_h = 190;
    let footer_h = 50;
    let height = (MARGIN * 2 + header_h + 20 + TIMELINE_HEIGHT + 20 + track_list_h + footer_h)
        .max(400) as u32;

    let mut canvas = RgbaImage::from_pixel(WIDTH, height, BG);

    // ── Header ──────────────────────────────────────────────────────────
    let album_ids: Vec<String> = output
        .cover_album_ids
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
        .unwrap_or_default();
    draw_collage(&mut canvas, &thumb_root, &album_ids, MARGIN, MARGIN);

    let text_x = MARGIN + COLLAGE_SIZE as i32 + 24;
    draw_text_mut(
        &mut canvas,
        ACCENT,
        text_x,
        MARGIN,
        PxScale::from(15.0),
        &font,
        "BOOGIEMIX STORY",
    );
    draw_text_mut(
        &mut canvas,
        TEXT,
        text_x,
        MARGIN + 26,
        PxScale::from(30.0),
        &font,
        &output.name,
    );

    let total_duration = tracks
        .last()
        .map(|t| t.output_end_sec)
        .unwrap_or(output.duration_sec);
    let created = output.created_at.as_deref().unwrap_or("");
    let stats = format!(
        "{}  ·  {} track{}  ·  {}",
        format_duration(total_duration),
        tracks.len(),
        if tracks.len() == 1 { "" } else { "s" },
        created.split(' ').next().unwrap_or(created),
    );
    draw_text_mut(
        &mut canvas,
        TEXT_MUTED,
        text_x,
        MARGIN + 72,
        PxScale::from(16.0),
        &font,
        &stats,
    );
    if let Some(playlist_name) = output.playlist_name.as_deref() {
        draw_text_mut(
            &mut canvas,
            TEXT_MUTED,
            text_x,
            MARGIN + 98,
            PxScale::from(14.0),
            &font,
            playlist_name,
        );
    }

    // ── Timeline ────────────────────────────────────────────────────────
    let timeline_y = MARGIN + header_h + 20;
    draw_timeline(
        &mut canvas,
        &thumb_root,
        tracks,
        total_duration,
        MARGIN,
        timeline_y,
        (WIDTH as i32) - MARGIN * 2,
    );

    // ── Track list ──────────────────────────────────────────────────────
    let mut row_y = timeline_y + TIMELINE_HEIGHT + 20;
    let text_x = MARGIN + TRACK_THUMB_SIZE as i32 + 10;
    for (i, track) in tracks.iter().take(MAX_TRACK_ROWS).enumerate() {
        let thumb_y = row_y + (TRACK_ROW_HEIGHT - TRACK_THUMB_SIZE as i32) / 2;
        let thumb = track
            .album_id
            .as_ref()
            .and_then(|id| load_album_thumb(&thumb_root, &id.to_string()));
        match thumb {
            Some(img) => {
                let resized = image::imageops::resize(
                    &img,
                    TRACK_THUMB_SIZE,
                    TRACK_THUMB_SIZE,
                    image::imageops::FilterType::Triangle,
                );
                image::imageops::overlay(&mut canvas, &resized, MARGIN as i64, thumb_y as i64);
            }
            None => {
                draw_filled_rect_mut(
                    &mut canvas,
                    Rect::at(MARGIN, thumb_y).of_size(TRACK_THUMB_SIZE, TRACK_THUMB_SIZE),
                    hash_to_fallback_color(&fallback_key(track)),
                );
            }
        }

        let label = if track.artist_name.is_empty() {
            format!("{}. {}", i + 1, track.title)
        } else {
            format!("{}. {} — {}", i + 1, track.title, track.artist_name)
        };
        draw_text_mut(
            &mut canvas,
            TEXT,
            text_x,
            row_y,
            PxScale::from(15.0),
            &font,
            &label,
        );

        let range = format!(
            "{} – {}",
            format_duration(track.output_start_sec),
            format_duration(track.output_end_sec)
        );
        let rw = text_w(&font, 13.0, &range);
        draw_text_mut(
            &mut canvas,
            TEXT_MUTED,
            WIDTH as i32 - MARGIN - rw,
            row_y + 1,
            PxScale::from(13.0),
            &font,
            &range,
        );
        row_y += TRACK_ROW_HEIGHT;
    }
    if truncated {
        let more = format!("+{} more tracks", tracks.len() - MAX_TRACK_ROWS);
        draw_text_mut(
            &mut canvas,
            TEXT_MUTED,
            MARGIN,
            row_y,
            PxScale::from(14.0),
            &font,
            &more,
        );
    }

    // ── Footer ──────────────────────────────────────────────────────────
    let footer = "Made with BoogieBox";
    let fw = text_w(&font, 13.0, footer);
    draw_text_mut(
        &mut canvas,
        TEXT_MUTED,
        WIDTH as i32 - MARGIN - fw,
        height as i32 - MARGIN,
        PxScale::from(13.0),
        &font,
        footer,
    );
    let mut bytes: Vec<u8> = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    canvas
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("PNG encode of an in-memory RgbaImage cannot fail");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::music::EntityId;

    fn output_row(cover_album_ids: Option<&str>) -> MixOutputRow {
        MixOutputRow {
            id: EntityId::Str("output-1".into()),
            job_id: EntityId::Str("job-1".into()),
            playlist_id: None,
            file_name: "mix.mp3".into(),
            name: "Electronic House".into(),
            playlist_name: Some("House Sessions".into()),
            cover_album_ids: cover_album_ids.map(str::to_string),
            duration_sec: 600.0,
            file_size_bytes: 12345,
            format: "mp3".into(),
            created_at: Some("2026-09-16 12:00:00".into()),
        }
    }

    fn track_row(step_index: i64, title: &str, start: f64, end: f64) -> MixOutputTrackRow {
        MixOutputTrackRow {
            step_index,
            track_id: Some(EntityId::Str(format!("t{step_index}"))),
            album_id: None,
            title: title.to_string(),
            artist_name: "Artist".to_string(),
            album_name: String::new(),
            track_duration_sec: end - start,
            bpm: Some(124.0),
            key_estimate: None,
            output_start_sec: start,
            output_end_sec: end,
            source_trim_start_sec: 0.0,
            source_trim_end_sec: end - start,
            crossfade_in_sec: 0.0,
            crossfade_out_sec: 8.0,
            transition_out_kind: Some("beatmatch".into()),
            transition_out_confidence: None,
            transition_out_phrase_aligned: true,
            transition_out_reason: None,
            waveform_peaks_json: Some("[0.1,0.5,0.9,0.3,0.7]".into()),
            energy_curve_json: None,
            section_markers_json: None,
        }
    }

    fn is_valid_png(bytes: &[u8]) -> bool {
        bytes.len() > 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n"
    }

    #[test]
    fn render_story_image_produces_a_valid_png_with_no_cached_art() {
        let output = output_row(None);
        let tracks = vec![
            track_row(0, "Nightdrive", 0.0, 300.0),
            track_row(1, "Shelter", 292.0, 600.0),
        ];
        let dir = std::env::temp_dir().join(format!("story-image-test-{}", uuid::Uuid::now_v7()));
        let bytes = render_story_image(&output, &tracks, &dir);
        assert!(is_valid_png(&bytes), "output must be a valid PNG");
        assert!(
            bytes.len() > 100,
            "a real image should be more than a bare header"
        );
    }

    #[test]
    fn render_story_image_handles_zero_tracks_without_panicking() {
        let output = output_row(None);
        let dir =
            std::env::temp_dir().join(format!("story-image-test-empty-{}", uuid::Uuid::now_v7()));
        let bytes = render_story_image(&output, &[], &dir);
        assert!(is_valid_png(&bytes));
    }

    #[test]
    fn render_story_image_handles_many_tracks_with_truncation() {
        let output = output_row(None);
        let tracks: Vec<MixOutputTrackRow> = (0..20)
            .map(|i| {
                track_row(
                    i,
                    &format!("Track {i}"),
                    i as f64 * 200.0,
                    (i as f64 + 1.0) * 200.0,
                )
            })
            .collect();
        let dir =
            std::env::temp_dir().join(format!("story-image-test-many-{}", uuid::Uuid::now_v7()));
        let bytes = render_story_image(&output, &tracks, &dir);
        assert!(is_valid_png(&bytes));
    }

    #[test]
    fn album_art_thumb_root_matches_the_artwork_routes_convention() {
        let base = PathBuf::from("D:\\data");
        assert_eq!(
            album_art_thumb_root(&base),
            base.join("art").join("album").join("thumb").join("300")
        );
    }

    #[test]
    fn hash_to_fallback_color_is_deterministic() {
        let a = hash_to_fallback_color("track-123");
        let b = hash_to_fallback_color("track-123");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_to_fallback_color_prefers_the_track_id_over_title_and_artist() {
        let by_id = track_row(0, "Nightdrive", 0.0, 300.0); // track_id = Some("t0")
        let mut no_id = track_row(0, "Nightdrive", 0.0, 300.0);
        no_id.track_id = None;
        assert_eq!(fallback_key(&by_id), "t0");
        assert_eq!(fallback_key(&no_id), "Nightdrive::Artist");
    }

    #[test]
    fn draw_timeline_paints_the_cached_thumbnail_instead_of_a_flat_fallback_color() {
        let mut track = track_row(0, "Nightdrive", 0.0, 300.0);
        track.album_id = Some(EntityId::Str("album-1".into()));

        let dir =
            std::env::temp_dir().join(format!("story-image-timeline-art-{}", uuid::Uuid::now_v7()));
        let cache_key = crate::artwork_cache::build_album_art_cache_key("album-1");
        let item_dir = crate::artwork_cache::cache_item_dir(&dir, &cache_key);
        std::fs::create_dir_all(&item_dir).unwrap();
        RgbaImage::from_pixel(40, 40, Rgba([255, 0, 0, 255]))
            .save(item_dir.join("art.png"))
            .unwrap();

        let mut canvas = RgbaImage::from_pixel(400, TIMELINE_HEIGHT as u32, BG);
        draw_timeline(
            &mut canvas,
            &dir,
            std::slice::from_ref(&track),
            300.0,
            0,
            0,
            400,
        );

        // Top row of the segment, where the scrim is lightest (alpha .15) —
        // a seeded pure-red thumbnail should still read strongly red, unlike
        // any entry in the deterministic fallback palette or the panel's
        // neutral background.
        let sample = canvas.get_pixel(5, 0);
        assert!(
            sample[0] > 180 && sample[1] < 80,
            "expected a reddish pixel from the seeded thumbnail, got {sample:?}"
        );
    }

    #[test]
    fn draw_timeline_falls_back_to_the_deterministic_palette_when_no_thumbnail_resolves() {
        let track = track_row(0, "Nightdrive", 0.0, 300.0); // no album_id
        let dir = std::env::temp_dir().join(format!(
            "story-image-timeline-noart-{}",
            uuid::Uuid::now_v7()
        ));

        let mut canvas = RgbaImage::from_pixel(400, TIMELINE_HEIGHT as u32, BG);
        draw_timeline(
            &mut canvas,
            &dir,
            std::slice::from_ref(&track),
            300.0,
            0,
            0,
            400,
        );

        let expected = hash_to_fallback_color(&fallback_key(&track));
        let sample = canvas.get_pixel(5, 0);
        // Row 0 blends the fallback color toward black at exactly the scrim's
        // top alpha (.15), so the expected value is exact modulo rounding.
        for c in 0..3 {
            let want = (expected[c] as f32 * 0.85).round() as i16;
            assert!(
                (sample[c] as i16 - want).abs() <= 2,
                "channel {c}: expected ~{want}, got {}",
                sample[c]
            );
        }
    }
}

//! `stream`, `download` and `getCoverArt`. Files are only served from inside their library's root folders;
//! transcodes are bounded by a semaphore so offline-download bursts cannot fork dozens of encoders.

use std::{path::PathBuf, time::Duration};

use axum::{
    body::Body,
    extract::{Path as AxPath, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;

use super::{
    handlers::{db_err, valid_id, Ctx},
    model,
    response::SubsonicError,
};
use crate::{ffmpeg, media_serve, routes::artwork_routes};

const TRANSCODE_WAIT: Duration = Duration::from_secs(10);
const BITRATES: [u32; 4] = [128, 192, 256, 320];

/// Largest supported bitrate not above `requested` (at least 128).
pub fn snap_bitrate(requested: u32) -> u32 {
    BITRATES
        .iter()
        .rev()
        .copied()
        .find(|b| *b <= requested)
        .unwrap_or(BITRATES[0])
}

/// Opus bitrates (kbps) and the default when `maxBitRate` is not given.
const OPUS_BITRATES: [u32; 5] = [64, 96, 128, 160, 192];
const OPUS_DEFAULT: u32 = 128;
/// AAC bitrates (kbps) and the default.
const AAC_BITRATES: [u32; 4] = [96, 128, 192, 256];
const AAC_DEFAULT: u32 = 192;

/// Largest bitrate of `ladder` not above `requested` (at least the lowest).
fn snap_to(ladder: &[u32], requested: u32) -> u32 {
    ladder
        .iter()
        .rev()
        .copied()
        .find(|b| *b <= requested)
        .unwrap_or(ladder[0])
}

/// How a stream request is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Original,
    Mp3(u32),
    /// Opus or AAC, streamed without a known length (plan §15.5, 2.5-5).
    Encoded(ffmpeg::StreamCodec, u32),
}

/// `format=opus` / `format=aac` (or `m4a`): that codec at `maxBitRate` (or its default), snapped to its
/// ladder. Without `libopus`, Opus falls back to MP3 at the same cap. `None` for any other format.
pub fn choose_codec(
    format: Option<&str>,
    max_bitrate: u32,
    opus_available: bool,
) -> Option<Delivery> {
    let format = format?.to_ascii_lowercase();
    let cap = |default: u32| {
        if max_bitrate > 0 {
            max_bitrate
        } else {
            default
        }
    };
    match format.as_str() {
        "opus" if opus_available => Some(Delivery::Encoded(
            ffmpeg::StreamCodec::Opus,
            snap_to(&OPUS_BITRATES, cap(OPUS_DEFAULT)),
        )),
        "opus" => Some(Delivery::Mp3(snap_bitrate(cap(OPUS_DEFAULT)))),
        "aac" | "m4a" => Some(Delivery::Encoded(
            ffmpeg::StreamCodec::Aac,
            snap_to(&AAC_BITRATES, cap(AAC_DEFAULT)),
        )),
        _ => None,
    }
}

/// Decision table (plan D12): original unless the client asked for MP3, the file needs transcoding for
/// playback, or `maxBitRate` is below the source bitrate. `default_kbps` comes from `transcodeQuality`.
pub fn choose_delivery(
    download: bool,
    format: Option<&str>,
    max_bitrate: u32,
    source_kbps: Option<i64>,
    needs_transcode: bool,
    source_is_mp3: bool,
    default_kbps: u32,
) -> Delivery {
    let format = format.map(str::to_ascii_lowercase);
    if download || format.as_deref() == Some("raw") {
        return Delivery::Original;
    }
    let cap = |kbps: u32| {
        if max_bitrate > 0 {
            snap_bitrate(kbps.min(max_bitrate))
        } else {
            snap_bitrate(kbps)
        }
    };
    if needs_transcode {
        return Delivery::Mp3(cap(default_kbps));
    }
    if format.as_deref() == Some("mp3") && !source_is_mp3 {
        return Delivery::Mp3(cap(default_kbps));
    }
    match source_kbps {
        Some(src) if max_bitrate > 0 && src > i64::from(max_bitrate) => {
            Delivery::Mp3(snap_bitrate(max_bitrate))
        }
        _ => Delivery::Original,
    }
}

/// True when `file` is inside one of `roots` (both canonicalized; unreachable roots are skipped).
pub fn within_roots(file: &std::path::Path, roots: &[String]) -> bool {
    let Ok(file) = std::fs::canonicalize(file) else {
        return false;
    };
    roots
        .iter()
        .filter(|r| !r.trim().is_empty())
        .filter_map(|r| std::fs::canonicalize(r.trim()).ok())
        .any(|root| file.starts_with(&root))
}

fn content_disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// `stream` (and `download` with `download = true`).
pub async fn stream(ctx: &Ctx, download: bool) -> Result<Response, SubsonicError> {
    let id = valid_id(ctx.params.required("id")?)?;
    if let Some(mix_id) = model::mix_id(&id) {
        return stream_mix(ctx, mix_id.to_owned(), download).await;
    }
    let (song, roots, quality, replay_gain) = ctx
        .db(move |conn| {
            let song = boogiebox_db::subsonic::get_song(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Song"))?;
            let roots = match song.library_id.as_deref() {
                Some(lib) => boogiebox_db::subsonic::library_roots(conn, lib).map_err(db_err)?,
                None => Vec::new(),
            };
            let get = |key: &str| boogiebox_db::artwork::get_setting(conn, key);
            let quality = get("transcodeQuality").unwrap_or_else(|| "low".into());
            let replay_gain = get("replayGainEnabled").as_deref() == Some("true");
            Ok((song, roots, quality, replay_gain))
        })
        .await?;

    let file_path = PathBuf::from(&song.file_path);
    let check_path = file_path.clone();
    let allowed = tokio::task::spawn_blocking(move || within_roots(&check_path, &roots))
        .await
        .unwrap_or(false);
    if !allowed {
        tracing::warn!(track_id = %song.id, "Subsonic stream refused: file missing or outside its library");
        return Err(SubsonicError::not_found("Song file"));
    }

    let ext = model::suffix(&song.file_path);
    let format = ctx.params.text("format");
    let max_bitrate = ctx.params.int("maxBitRate")?.unwrap_or(0).clamp(0, 10_000) as u32;
    let wants_opus = format.is_some_and(|f| f.eq_ignore_ascii_case("opus"));
    let opus_available = if wants_opus && !download {
        tokio::task::spawn_blocking(|| ffmpeg::has_encoder(&ffmpeg::resolve_ffmpeg(), "libopus"))
            .await
            .unwrap_or(false)
    } else {
        false
    };
    let codec = if download {
        None
    } else {
        choose_codec(format, max_bitrate, opus_available)
    };
    let delivery = codec.unwrap_or_else(|| {
        choose_delivery(
            download,
            format,
            max_bitrate,
            song.bitrate,
            ffmpeg::needs_audio_transcode(&file_path),
            ext == "mp3",
            ffmpeg::bitrate_for_quality(&quality),
        )
    });

    match delivery {
        Delivery::Original => {
            let mime = ffmpeg::audio_mime_type(&file_path);
            let mut resp =
                media_serve::serve_file(&file_path, mime, &ctx.headers, ctx.head, &[]).await;
            if download {
                let name = model::virtual_path(&song)
                    .rsplit('/')
                    .next()
                    .unwrap_or("track")
                    .to_owned();
                if let Ok(v) = HeaderValue::from_str(&content_disposition(&name)) {
                    resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
                }
            }
            Ok(resp)
        }
        Delivery::Mp3(kbps) | Delivery::Encoded(_, kbps) => {
            // `timeOffset` (OpenSubsonic `transcodeOffset`) only applies to transcodes; raw files seek by range.
            let start_seconds = ctx.params.int("timeOffset")?.unwrap_or(0).max(0) as f64;
            if song.duration > 0.0 && start_seconds >= song.duration {
                return Ok(past_the_end(&delivery));
            }
            let limiter = ctx.subsonic.transcode_limiter(ctx.settings.max_transcodes);
            let permit = match tokio::time::timeout(TRANSCODE_WAIT, limiter.acquire_owned()).await {
                Ok(Ok(permit)) => permit,
                _ => {
                    return Ok((
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Too many transcodes in progress",
                    )
                        .into_response())
                }
            };
            let resp = if let Delivery::Encoded(codec, kbps) = delivery {
                media_serve::serve_stream_encode(media_serve::StreamEncode {
                    file_path: &file_path,
                    codec,
                    bitrate_kbps: kbps,
                    start_seconds,
                    replay_gain,
                    head_only: ctx.head,
                    log_id: &song.id,
                    log_ext: &ext,
                })
                .await
            } else {
                let range = ctx
                    .headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                media_serve::serve_mp3_transcode(media_serve::Mp3Transcode {
                    file_path: &file_path,
                    duration: (song.duration > 0.0).then_some(song.duration),
                    range: range.as_deref(),
                    bitrate_kbps: kbps,
                    start_seconds,
                    replay_gain,
                    head_only: ctx.head,
                    log_id: &song.id,
                    log_ext: &ext,
                    extra: &[],
                })
                .await
            };
            // Hold the transcode slot until the body is fully sent or the client disconnects.
            let (parts, body) = resp.into_parts();
            let guarded = body.into_data_stream().map(move |chunk| {
                let _slot = &permit;
                chunk
            });
            Ok(Response::from_parts(parts, Body::from_stream(guarded)))
        }
    }
}

/// A `timeOffset` at or past the end: an empty stream of the requested type (players treat it as the end).
fn past_the_end(delivery: &Delivery) -> Response {
    let mime = match delivery {
        Delivery::Encoded(codec, _) => codec.mime(),
        _ => "audio/mpeg",
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, mime), (header::CONTENT_LENGTH, "0")],
    )
        .into_response()
}

/// BoogieMix output folders: the configured one and the default under the data folder (mixes rendered before
/// the setting changed stay playable). Blocking (reads the setting).
fn mix_roots(db: &crate::DbPool, db_folder: Option<&PathBuf>) -> Vec<String> {
    let configured = crate::mix_worker::get_mix_output_dir_fn(db, db_folder);
    let mut roots = vec![configured.to_string_lossy().into_owned()];
    if let Some(folder) = db_folder {
        let default = folder.join("mix-outputs").to_string_lossy().into_owned();
        if !roots.contains(&default) {
            roots.push(default);
        }
    }
    roots
}

/// A BoogieMix render: always the original file (no transcode), owner only, from a mix output folder.
async fn stream_mix(ctx: &Ctx, mix_id: String, download: bool) -> Result<Response, SubsonicError> {
    let user = ctx.user_id();
    let mix = ctx
        .db(move |conn| boogiebox_db::subsonic::get_user_mix(conn, &user, &mix_id).map_err(db_err))
        .await?
        .ok_or_else(|| SubsonicError::not_found("Song"))?;
    let db_folder = ctx
        .state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .db_folder
        .clone();
    let file_path = PathBuf::from(&mix.file_path);
    let (check_path, db) = (file_path.clone(), ctx.db.clone());
    let allowed = tokio::task::spawn_blocking(move || {
        within_roots(&check_path, &mix_roots(&db, db_folder.as_ref()))
    })
    .await
    .unwrap_or(false);
    if !allowed {
        tracing::warn!(mix_id = %mix.id, "Subsonic mix stream refused: file missing or outside the mix folder");
        return Err(SubsonicError::not_found("Song file"));
    }
    let format = if mix.format.is_empty() {
        "mp3".to_owned()
    } else {
        mix.format.clone()
    };
    let mime = ffmpeg::audio_mime_type(std::path::Path::new(&format!("mix.{format}")));
    let mut resp = media_serve::serve_file(&file_path, mime, &ctx.headers, ctx.head, &[]).await;
    if download {
        let name = format!("{}.{format}", mix.name.replace(['/', '\\'], "_"));
        if let Ok(v) = HeaderValue::from_str(&content_disposition(&name)) {
            resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
        }
    }
    Ok(resp)
}

/// What a cover-art id refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverTarget {
    Album(String),
    Artist(String),
}

/// `getCoverArt`: `al-`/`ar-`/`tr-`/`pl-` prefixed ids, or a bare album/artist/song id.
pub async fn cover_art(ctx: &Ctx) -> Result<Response, SubsonicError> {
    let raw = ctx.params.required("id")?.to_owned();
    let size = ctx.params.int("size")?.unwrap_or(0);
    let user = ctx.user.id.clone();
    let target = ctx
        .db(move |conn| resolve_cover(conn, &raw, &user))
        .await?
        .ok_or_else(|| SubsonicError::not_found("Cover art"))?;
    // Thumbnails come from the shared artwork cache (300 or 800 px); originals can be very large.
    let thumb = if size > 0 && size <= 300 {
        "300"
    } else {
        "800"
    };
    let params = artwork_routes::ArtParams {
        size: Some(thumb.to_owned()),
        refresh: None,
    };
    let resp = match target {
        CoverTarget::Album(id) => {
            artwork_routes::album_art_handler(
                State(ctx.state.clone()),
                AxPath(id),
                Query(params),
                ctx.headers.clone(),
            )
            .await
        }
        CoverTarget::Artist(id) => {
            artwork_routes::artist_photo_handler(
                State(ctx.state.clone()),
                AxPath(id),
                Query(params),
                ctx.headers.clone(),
            )
            .await
        }
    };
    if resp.status().is_success() || resp.status() == StatusCode::NOT_MODIFIED {
        Ok(resp)
    } else {
        Err(SubsonicError::not_found("Cover art"))
    }
}

fn resolve_cover(
    conn: &rusqlite::Connection,
    raw: &str,
    user_id: &str,
) -> Result<Option<CoverTarget>, SubsonicError> {
    use boogiebox_db::subsonic as sdb;
    if raw == super::model::BOOGIEMIX_PLAYLIST_ID || raw == "pl-bm-mixes" {
        return Ok(sdb::list_user_mixes(conn, user_id)
            .map_err(db_err)?
            .into_iter()
            .find_map(|m| m.cover_album_id)
            .map(CoverTarget::Album));
    }
    let (prefix, id) = match raw.split_once('-') {
        Some((p @ ("al" | "ar" | "tr" | "pl" | "mx"), rest)) => (Some(p), rest),
        _ => (None, raw),
    };
    let id = valid_id(id)?;
    let album_of_song = |song_id: &str| -> Result<Option<CoverTarget>, SubsonicError> {
        Ok(sdb::get_song(conn, song_id)
            .map_err(db_err)?
            .and_then(|s| s.album_id)
            .map(CoverTarget::Album))
    };
    match prefix {
        Some("al") => Ok(Some(CoverTarget::Album(id))),
        Some("ar") => Ok(Some(CoverTarget::Artist(id))),
        Some("tr") => album_of_song(&id),
        Some("mx") => Ok(sdb::get_user_mix(conn, user_id, &id)
            .map_err(db_err)?
            .and_then(|m| m.cover_album_id)
            .map(CoverTarget::Album)),
        Some("pl") => {
            let entity = boogiebox_db::music::coerce_entity_id(&id);
            Ok(
                boogiebox_db::playlists::get_playlist(conn, &entity, user_id)
                    .map_err(db_err)?
                    .and_then(|p| p.art_album_ids.into_iter().next())
                    .map(CoverTarget::Album),
            )
        }
        _ => {
            if sdb::get_album(conn, &id).map_err(db_err)?.is_some() {
                return Ok(Some(CoverTarget::Album(id)));
            }
            if sdb::get_artist(conn, &id).map_err(db_err)?.is_some() {
                return Ok(Some(CoverTarget::Artist(id)));
            }
            album_of_song(&id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrates_snap_down_with_a_floor() {
        assert_eq!(snap_bitrate(320), 320);
        assert_eq!(snap_bitrate(300), 256);
        assert_eq!(snap_bitrate(192), 192);
        assert_eq!(snap_bitrate(96), 128);
        assert_eq!(snap_bitrate(10_000), 320);
    }

    #[test]
    fn delivery_decision_table() {
        use Delivery::*;
        // download / raw always original
        assert_eq!(
            choose_delivery(true, Some("mp3"), 128, Some(900), true, false, 192),
            Original
        );
        assert_eq!(
            choose_delivery(false, Some("raw"), 128, Some(900), true, false, 192),
            Original
        );
        // not playable → mp3 at default, capped by maxBitRate
        assert_eq!(
            choose_delivery(false, None, 0, Some(900), true, false, 320),
            Mp3(320)
        );
        assert_eq!(
            choose_delivery(false, None, 160, Some(900), true, false, 320),
            Mp3(128)
        );
        // explicit mp3 for a non-mp3 source
        assert_eq!(
            choose_delivery(false, Some("MP3"), 0, Some(500), false, false, 192),
            Mp3(192)
        );
        // explicit mp3 for an mp3 source within budget → original
        assert_eq!(
            choose_delivery(false, Some("mp3"), 0, Some(320), false, true, 192),
            Original
        );
        // maxBitRate below the source → transcode at that rate
        assert_eq!(
            choose_delivery(false, None, 192, Some(320), false, true, 320),
            Mp3(192)
        );
        // maxBitRate at or above the source, or unknown source rate → original
        assert_eq!(
            choose_delivery(false, None, 320, Some(320), false, true, 320),
            Original
        );
        assert_eq!(
            choose_delivery(false, None, 128, None, false, true, 320),
            Original
        );
    }

    #[test]
    fn opus_and_aac_requests() {
        use ffmpeg::StreamCodec::*;
        use Delivery::*;
        assert_eq!(
            choose_codec(Some("opus"), 0, true),
            Some(Encoded(Opus, 128))
        );
        assert_eq!(
            choose_codec(Some("OPUS"), 100, true),
            Some(Encoded(Opus, 96))
        );
        assert_eq!(
            choose_codec(Some("opus"), 32, true),
            Some(Encoded(Opus, 64))
        );
        assert_eq!(
            choose_codec(Some("opus"), 999, true),
            Some(Encoded(Opus, 192))
        );
        // No libopus: MP3 at the same cap.
        assert_eq!(choose_codec(Some("opus"), 0, false), Some(Mp3(128)));
        assert_eq!(choose_codec(Some("opus"), 256, false), Some(Mp3(256)));
        assert_eq!(choose_codec(Some("aac"), 0, false), Some(Encoded(Aac, 192)));
        assert_eq!(
            choose_codec(Some("m4a"), 160, false),
            Some(Encoded(Aac, 128))
        );
        assert_eq!(choose_codec(Some("mp3"), 0, true), None);
        assert_eq!(choose_codec(None, 0, true), None);
    }

    #[test]
    fn root_confinement() {
        let base = std::env::temp_dir().join(format!("bb-subsonic-roots-{}", uuid::Uuid::now_v7()));
        let inside = base.join("lib");
        std::fs::create_dir_all(&inside).unwrap();
        let file = inside.join("a.mp3");
        std::fs::write(&file, b"x").unwrap();
        let outside = base.join("other.mp3");
        std::fs::write(&outside, b"x").unwrap();
        let roots = vec![
            inside.to_string_lossy().into_owned(),
            "Z:\\does\\not\\exist".into(),
        ];
        assert!(within_roots(&file, &roots));
        assert!(!within_roots(&outside, &roots));
        assert!(!within_roots(&inside.join("..").join("other.mp3"), &roots));
        assert!(!within_roots(&inside.join("missing.mp3"), &roots));
        assert!(!within_roots(&file, &[]));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn content_disposition_is_header_safe() {
        let v = content_disposition("01 - Café \"Live\".flac");
        assert!(HeaderValue::from_str(&v).is_ok());
        assert!(v.contains("filename*=UTF-8''01%20-%20Caf%C3%A9%20%22Live%22.flac"));
        assert!(v.contains("filename=\"01 - Caf_ _Live_.flac\""));
    }
}

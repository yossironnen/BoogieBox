//! Defines Rust API routes for Playback Routes server behavior.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::SeekFrom, path};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::{
    auth::AuthenticatedUser,
    bpm_analysis, ffmpeg,
    providers::{fetch_lrclib_lyrics, fetch_lyricsovh},
    waveform_map, DbPool, ErrorResponse, OkResponse, SharedState,
};

// -- Router --------------------------------------------------------------------

/// Documents the Playback Router public API surface.
pub fn playback_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/user/history", get(user_history_handler))
        .route("/api/tracks/{id}/stream", get(stream_track_handler))
        .route("/api/tracks/{id}/played", post(track_played_handler))
        .route("/api/tracks/{id}/lyrics", get(track_lyrics_handler))
        .route("/api/tracks/{id}/waveform", get(track_waveform_handler))
        .route(
            "/api/tracks/{id}/waveform/generate",
            post(track_waveform_generate_handler),
        )
        .route("/api/tracks/{id}/eq-profile", get(track_eq_profile_handler))
        .route(
            "/api/waveforms/map/status",
            get(waveform_map_status_handler),
        )
        .route("/api/waveforms/map/run", post(waveform_map_run_handler))
        .route("/api/bpm/status", get(bpm_status_handler))
        .route("/api/bpm/run", post(bpm_run_handler))
        .with_state(state)
}

// -- Query params --------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamParams {
    no_transcode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserHistoryParams {
    limit: Option<i64>,
}

// -- Handlers ------------------------------------------------------------------

async fn stream_track_handler(
    State(state): State<SharedState>,
    req_headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<StreamParams>,
) -> Response {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response().into_response(),
    };

    let skip_transcode = params.no_transcode.as_deref() == Some("1");
    let range_header = req_headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let track_id = id.clone();
    let track = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_track_for_stream(&conn, &track_id)
    })
    .await
    {
        Ok(Ok(Some(t))) => t,
        Ok(Ok(None)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Track not found".into(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
        _ => return internal_error().into_response(),
    };

    let file_path = path::Path::new(&track.file_path).to_path_buf();
    let track_ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let needs_transcode = ffmpeg::needs_audio_transcode(&file_path);
    tracing::info!(
        track_id = %id,
        ext = %track_ext,
        skip_transcode,
        needs_transcode,
        has_range = range_header.is_some(),
        range = range_header.as_deref().unwrap_or(""),
        "audio stream request"
    );
    if !file_path.exists() {
        tracing::warn!(track_id = %id, ext = %track_ext, "audio stream file missing on disk");
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not found on disk".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // Transcode path
    if !skip_transcode && needs_transcode {
        let (quality, replay_gain) = {
            let db = match get_db(&state) {
                Some(d) => d,
                None => return setup_required_response().into_response(),
            };
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                let quality = conn
                    .query_row(
                        "SELECT value FROM settings WHERE key='transcodeQuality'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap_or_else(|_| "low".into());
                let rg = conn
                    .query_row(
                        "SELECT value FROM settings WHERE key='replayGainEnabled'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap_or_else(|_| "false".into());
                (quality, rg == "true")
            })
            .await
            .unwrap_or_else(|_| ("low".into(), false))
        };

        let total_bytes = track.duration.and_then(|duration| {
            if duration > 0.0 {
                let total = ffmpeg::transcoded_total_bytes(duration, &quality);
                (total > 0).then_some(total)
            } else {
                None
            }
        });
        let parsed_transcode_range = range_header
            .as_ref()
            .and_then(|rh| total_bytes.and_then(|total| ffmpeg::parse_byte_range(rh, total)));
        let seek_seconds = parsed_transcode_range
            .filter(|(start, _)| *start > 0)
            .map(|(start, _)| ffmpeg::byte_offset_to_seconds(start, &quality))
            .unwrap_or(0.0);
        if range_header.is_some() && total_bytes.is_some() && parsed_transcode_range.is_none() {
            tracing::warn!(
                track_id = %id,
                ext = %track_ext,
                range = range_header.as_deref().unwrap_or(""),
                duration = track.duration.unwrap_or(0.0),
                estimated_total_bytes = total_bytes.unwrap_or(0),
                "invalid audio transcode range request"
            );
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::empty())
                .unwrap_or_else(|_| internal_error().into_response());
        }

        let ffmpeg_path = ffmpeg::resolve_ffmpeg();
        tracing::info!(
            track_id = %id,
            ext = %track_ext,
            quality = %quality,
            replay_gain,
            seek_seconds,
            duration = track.duration.unwrap_or(0.0),
            estimated_total_bytes = total_bytes.unwrap_or(0),
            ffmpeg = %ffmpeg_path.display(),
            "audio stream using ffmpeg transcode"
        );
        let mut child = match ffmpeg::spawn_transcode(
            &ffmpeg_path,
            &file_path,
            seek_seconds,
            &quality,
            replay_gain,
        ) {
            Ok(c) => c,
            Err(error) => {
                tracing::error!(
                    track_id = %id,
                    ext = %track_ext,
                    ffmpeg = %ffmpeg_path.display(),
                    error = %error,
                    "failed to spawn ffmpeg audio transcode"
                );
                return internal_error().into_response();
            }
        };
        tracing::info!(
            track_id = %id,
            ext = %track_ext,
            pid = child.id(),
            "spawned ffmpeg audio transcode"
        );

        // Bridge sync std::process::Child stdout → async DuplexStream.
        // std::process avoids IOCP-named-pipe incompatibility with FFmpeg 8.x
        // async muxer tasks (caused EINVAL after ~1 KB on Tokio async pipes).
        let (read_side, write_side) = tokio::io::duplex(64 * 1024);
        let mut child_stdout = child.stdout.take().expect("stdout piped");
        let mut child_stderr = child.stderr.take();
        let log_track_id = id.clone();
        let log_ext = track_ext.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};
            let mut sync_writer = tokio_util::io::SyncIoBridge::new(write_side);
            let mut buf = [0u8; 8192];
            let mut bytes_written: u64 = 0;
            loop {
                match child_stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if sync_writer.write_all(&buf[..n]).is_err() {
                            break;
                        }
                        bytes_written += n as u64;
                    }
                    Err(_) => break,
                }
            }
            drop(sync_writer);
            let mut stderr_text = String::new();
            if let Some(ref mut e) = child_stderr {
                let _ = e.read_to_string(&mut stderr_text);
            }
            match child.wait() {
                Ok(s) if !s.success() => {
                    tracing::warn!(
                        track_id = %log_track_id,
                        ext = %log_ext,
                        status = ?s,
                        bytes_written,
                        stderr = %stderr_text.trim(),
                        "ffmpeg audio transcode exited unsuccessfully"
                    )
                }
                Ok(s) => {
                    tracing::info!(
                        track_id = %log_track_id,
                        ext = %log_ext,
                        status = ?s,
                        bytes_written,
                        "ffmpeg audio transcode completed"
                    )
                }
                Err(error) => {
                    tracing::warn!(
                        track_id = %log_track_id,
                        ext = %log_ext,
                        bytes_written,
                        error = %error,
                        stderr = %stderr_text.trim(),
                        "failed waiting for ffmpeg audio transcode"
                    )
                }
            }
        });

        let start_byte = ffmpeg::transcoded_byte_offset(seek_seconds, &quality);

        let mut builder = Response::builder()
            .header(header::CONTENT_TYPE, "audio/mpeg")
            .header(header::ACCEPT_RANGES, "bytes")
            .header("Cache-Control", "no-cache");

        if range_header.is_some() {
            if let (Some(total), Some((range_start, range_end))) =
                (total_bytes, parsed_transcode_range)
            {
                let response_start = start_byte.max(range_start);
                let response_end = range_end.max(response_start).min(total.saturating_sub(1));
                let remaining = response_end.saturating_sub(response_start) + 1;
                tracing::info!(
                    track_id = %id,
                    ext = %track_ext,
                    status = 206,
                    content_range = %format!("bytes {response_start}-{response_end}/{total}"),
                    content_length = remaining,
                    "audio transcode range response"
                );
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {response_start}-{response_end}/{total}"),
                    )
                    .header(header::CONTENT_LENGTH, remaining.to_string());
            } else {
                tracing::info!(
                    track_id = %id,
                    ext = %track_ext,
                    status = 200,
                    duration = track.duration.unwrap_or(0.0),
                    "audio transcode range requested without known duration"
                );
                builder = builder.status(StatusCode::OK);
            }
        } else {
            tracing::info!(
                track_id = %id,
                ext = %track_ext,
                status = 200,
                content_length = total_bytes.unwrap_or(0),
                "audio transcode full response"
            );
            builder = builder.status(StatusCode::OK);
            if let Some(total) = total_bytes {
                builder = builder.header(header::CONTENT_LENGTH, total.to_string());
            }
        }

        return builder
            .body(Body::from_stream(ReaderStream::new(read_side)))
            .unwrap_or_else(|_| internal_error().into_response());
    }

    // Native file stream
    tracing::info!(
        track_id = %id,
        ext = %track_ext,
        skip_transcode,
        needs_transcode,
        "audio stream using native file response"
    );
    let meta = match tokio::fs::metadata(&file_path).await {
        Ok(m) => m,
        Err(_) => return internal_error().into_response(),
    };
    let file_size = meta.len();
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = format!("\"{file_size}-{modified_secs}\"");
    let last_modified = http_date_secs(modified_secs);

    // M-03: Handle conditional requests so browsers/clients get 304 on unchanged files.
    let if_none_match = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if let Some(ref inm) = if_none_match {
        if inm.as_str() == etag || inm == "*" {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .header("Last-Modified", &last_modified)
                .body(Body::empty())
                .unwrap_or_else(|_| internal_error().into_response());
        }
    }

    let mime = ffmpeg::audio_mime_type(&file_path);

    if let Some(ref rh) = range_header {
        match ffmpeg::parse_byte_range(rh, file_size) {
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::empty())
                    .unwrap_or_else(|_| internal_error().into_response());
            }
            Some((start, end)) => {
                let mut file = match tokio::fs::File::open(&file_path).await {
                    Ok(f) => f,
                    Err(_) => return internal_error().into_response(),
                };
                if file.seek(SeekFrom::Start(start)).await.is_err() {
                    return internal_error().into_response();
                }
                let len = end - start + 1;
                let reader = file.take(len);
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{file_size}"),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_LENGTH, len.to_string())
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::ETAG, &etag)
                    .header("Last-Modified", &last_modified)
                    .body(Body::from_stream(ReaderStream::new(reader)))
                    .unwrap_or_else(|_| internal_error().into_response());
            }
        }
    }

    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => return internal_error().into_response(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &etag)
        .header("Last-Modified", &last_modified)
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|_| internal_error().into_response())
}

async fn user_history_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(params): Query<UserHistoryParams>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };
    let user_id = user.id;
    let limit = params.limit.unwrap_or(50).clamp(1, 200);

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::list_user_history(&conn, &user_id, limit)
    })
    .await;

    match result {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn track_played_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let user_id = user.id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let updated = boogiebox_db::playback::increment_track_play_count(&conn, &id)?;
        if updated {
            let _ = boogiebox_db::playback::insert_play_history(&conn, &user_id, &id);
        }
        Ok::<bool, rusqlite::Error>(updated)
    })
    .await;

    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Track not found".into(),
                setup_required: None,
            }),
        )
            .into_response(),
        _ => internal_error(),
    }
}

async fn track_lyrics_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking({
        let id = id.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::playback::get_track_lyrics_cached(&conn, &id)
        }
    })
    .await;

    match result {
        Ok(Ok(Some(row))) => {
            let mut obj = serde_json::json!({
                "lyrics": row.lyrics,
                "source": row.source,
            });
            if let Some(synced_raw) = row.synced_lyrics {
                if let Ok(parsed) = serde_json::from_str::<Value>(&synced_raw) {
                    obj["syncedLyrics"] = parsed;
                }
            }
            return (StatusCode::OK, Json(obj)).into_response();
        }
        Ok(Ok(None)) => {}
        _ => return internal_error(),
    }

    // Cache miss - try LRCLIB then lyrics.ovh
    let db2 = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };
    let track_info = tokio::task::spawn_blocking({
        let id = id.clone();
        move || {
            let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_track_for_lyrics(&conn, &id)
        }
    })
    .await
    .ok()
    .flatten();

    let Some(info) = track_info else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Lyrics not found".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let artist = info.artist.clone();
    let title = info.title.clone();

    // Try LRCLIB
    if let Some(result) = fetch_lrclib_lyrics(&http_client, &artist, &title).await {
        let synced_json = result
            .synced
            .as_ref()
            .and_then(|s| serde_json::to_string(s).ok());
        let db3 = match get_db(&state) {
            Some(d) => d,
            None => return internal_error(),
        };
        tokio::task::spawn_blocking({
            let id = id.clone();
            let lyrics = result.lyrics.clone();
            let source = result.source.clone();
            let artist = artist.clone();
            let title = title.clone();
            move || {
                let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::artwork::upsert_cached_lyrics(
                    &conn,
                    &id,
                    &artist,
                    &title,
                    &lyrics,
                    &source,
                    synced_json.as_deref(),
                );
            }
        })
        .await
        .ok();

        let mut obj = serde_json::json!({
            "lyrics": result.lyrics,
            "source": result.source,
        });
        if let Some(synced) = result.synced {
            if let Ok(parsed) = serde_json::to_value(synced) {
                obj["syncedLyrics"] = parsed;
            }
        }
        return (StatusCode::OK, Json(obj)).into_response();
    }

    // Try lyrics.ovh
    if let Some(result) = fetch_lyricsovh(&http_client, &artist, &title).await {
        let db4 = match get_db(&state) {
            Some(d) => d,
            None => return internal_error(),
        };
        tokio::task::spawn_blocking({
            let lyrics = result.lyrics.clone();
            let source = result.source.clone();
            move || {
                let conn = db4.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::artwork::upsert_cached_lyrics(
                    &conn, &id, &artist, &title, &lyrics, &source, None,
                );
            }
        })
        .await
        .ok();

        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "lyrics": result.lyrics,
                "source": result.source,
            })),
        )
            .into_response();
    }

    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Lyrics not found".into(),
            setup_required: None,
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct WaveformResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    waveform: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn track_waveform_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let id_clone = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let waveform = boogiebox_db::playback::get_track_waveform(&conn, &id_clone)?;
        let settings = boogiebox_db::playback::get_waveform_settings(&conn);
        Ok::<_, rusqlite::Error>((waveform, settings))
    })
    .await;

    match result {
        Ok(Ok((Some(row), _))) => {
            let waveform_val =
                serde_json::from_str::<Value>(&row.waveform_json).unwrap_or(Value::Null);
            (
                StatusCode::OK,
                Json(WaveformResponse {
                    status: "ready".into(),
                    waveform: Some(waveform_val),
                    error: None,
                }),
            )
                .into_response()
        }
        Ok(Ok((None, settings))) => {
            if settings.generate_on_missing {
                // Spawn background generation, return 'generating' immediately
                let db2 = match get_db(&state) {
                    Some(d) => d,
                    None => return setup_required_response(),
                };
                let ffmpeg_path = ffmpeg::resolve_ffmpeg();
                tokio::spawn(async move {
                    let id_bg = id.clone();
                    let db3 = db2.clone();
                    let file_path_opt = tokio::task::spawn_blocking(move || {
                        let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
                        boogiebox_db::playback::get_track_for_stream(&conn, &id_bg)
                            .ok()
                            .flatten()
                            .map(|t| t.file_path)
                    })
                    .await
                    .ok()
                    .flatten();
                    if let Some(fp) = file_path_opt {
                        let fpath = path::Path::new(&fp);
                        if let Ok(peaks) = ffmpeg::generate_waveform(&ffmpeg_path, fpath).await {
                            let json =
                                serde_json::to_string(&peaks).unwrap_or_else(|_| "[]".into());
                            let len = peaks.len() as i64;
                            let _ = tokio::task::spawn_blocking(move || {
                                let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
                                boogiebox_db::playback::save_track_waveform(
                                    &conn, &id, len, None, &json,
                                )
                            })
                            .await;
                        }
                    }
                });
                (
                    StatusCode::OK,
                    Json(WaveformResponse {
                        status: "generating".into(),
                        waveform: None,
                        error: None,
                    }),
                )
                    .into_response()
            } else {
                (
                    StatusCode::OK,
                    Json(WaveformResponse {
                        status: "missing".into(),
                        waveform: None,
                        error: None,
                    }),
                )
                    .into_response()
            }
        }
        _ => internal_error(),
    }
}

async fn track_waveform_generate_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let id_clone = id.clone();
    let track = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_track_for_stream(&conn, &id_clone)
    })
    .await
    {
        Ok(Ok(Some(t))) => t,
        Ok(Ok(None)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(WaveformResponse {
                    status: "missing".into(),
                    waveform: None,
                    error: Some("Track not found or file is unavailable".into()),
                }),
            )
                .into_response();
        }
        _ => return internal_error(),
    };

    let file_path = path::Path::new(&track.file_path).to_path_buf();
    if !file_path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(WaveformResponse {
                status: "missing".into(),
                waveform: None,
                error: Some("Track not found or file is unavailable".into()),
            }),
        )
            .into_response();
    }

    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    match ffmpeg::generate_waveform(&ffmpeg_path, &file_path).await {
        Ok(peaks) => {
            let json = serde_json::to_string(&peaks).unwrap_or_else(|_| "[]".into());
            let waveform_val = serde_json::to_value(&peaks).unwrap_or(Value::Null);
            let db2 = match get_db(&state) {
                Some(d) => d,
                None => return setup_required_response(),
            };
            let id_clone2 = id.clone();
            let len = peaks.len() as i64;
            let dur = track.duration;
            let json_save = json.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::playback::save_track_waveform(&conn, &id_clone2, len, dur, &json_save)
            })
            .await;
            (
                StatusCode::OK,
                Json(WaveformResponse {
                    status: "ready".into(),
                    waveform: Some(waveform_val),
                    error: None,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WaveformResponse {
                status: "error".into(),
                waveform: None,
                error: Some(e),
            }),
        )
            .into_response(),
    }
}

async fn track_eq_profile_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_track_eq_profile(&conn, &id)
    })
    .await;

    match result {
        Ok(Ok(profile)) => (StatusCode::OK, Json(profile)).into_response(),
        _ => internal_error(),
    }
}

use crate::auth::AdminUser;

async fn waveform_map_status_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_waveform_map_status(&conn)
    })
    .await;

    match result {
        Ok(Ok(status)) => (StatusCode::OK, Json(status)).into_response(),
        _ => internal_error(),
    }
}

async fn waveform_map_run_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    match waveform_map::run_waveform_map_batch(db, "manual").await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error,
                setup_required: None,
            }),
        )
            .into_response(),
    }
}

async fn bpm_status_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_bpm_analysis_status(&conn)
    })
    .await;

    match result {
        Ok(Ok(status)) => (StatusCode::OK, Json(status)).into_response(),
        _ => internal_error(),
    }
}

async fn bpm_run_handler(State(state): State<SharedState>, _admin: AdminUser) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    match bpm_analysis::run_bpm_analysis_batch(db, "manual").await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error,
                setup_required: None,
            }),
        )
            .into_response(),
    }
}

// -- Helpers -------------------------------------------------------------------

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn setup_required_response() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

fn internal_error() -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Internal server error".into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn http_date_secs(secs: u64) -> String {
    use time::{Month, OffsetDateTime, Weekday};
    let dt = OffsetDateTime::from_unix_timestamp(secs as i64).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let wd = match dt.weekday() {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    };
    let mo = match dt.month() {
        Month::January => "Jan",
        Month::February => "Feb",
        Month::March => "Mar",
        Month::April => "Apr",
        Month::May => "May",
        Month::June => "Jun",
        Month::July => "Jul",
        Month::August => "Aug",
        Month::September => "Sep",
        Month::October => "Oct",
        Month::November => "Nov",
        Month::December => "Dec",
    };
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        wd,
        dt.day(),
        mo,
        dt.year(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

#[cfg(test)]
mod tests {
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use crate::DbPool;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rusqlite::params;
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Seeds one library/artist/album/track whose file lives on real disk with real
    /// bytes and a `.mp3` extension, so `stream_track_handler` takes the native-file
    /// streaming path (no ffmpeg transcode spawn — see `ffmpeg::needs_audio_transcode`
    /// / the accepted-gap note in wip/server-rust-coverage-gap-plan.md).
    fn seed_streamable_track(pool: &DbPool) -> (String, std::path::PathBuf) {
        let conn = pool.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("playback-route-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let track_path = dir.join("track.mp3");
        std::fs::write(&track_path, b"fake mp3 bytes for streaming tests").unwrap();

        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, ?, 'Lib')",
            params![library_id, dir.to_string_lossy()],
        )
        .unwrap();
        let artist_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Artist')",
            params![artist_id],
        )
        .unwrap();
        let album_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Album', ?)",
            params![album_id, artist_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
             VALUES (?, ?, ?, ?, 'Track', ?)",
            params![
                track_id,
                library_id,
                artist_id,
                album_id,
                track_path.to_string_lossy()
            ],
        )
        .unwrap();
        (track_id, track_path)
    }

    #[tokio::test]
    async fn stream_track_serves_full_file_and_404s_for_missing_track() {
        let (app, pool) = new_test_app_with_pool("playback-stream");
        let (track_id, _path) = seed_streamable_track(&pool);

        let (status, body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/stream"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"fake mp3 bytes for streaming tests");

        let (missing_status, _) = send(
            app,
            Request::builder()
                .uri("/api/tracks/does-not-exist/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stream_track_supports_byte_ranges_and_conditional_requests() {
        let (app, pool) = new_test_app_with_pool("playback-stream-range");
        let (track_id, _path) = seed_streamable_track(&pool);

        let (range_status, range_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/stream"))
                .header("range", "bytes=0-3")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(range_status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(range_body, b"fake");

        let (full_status, full_resp_headers) = {
            let resp = axum::Router::clone(&app)
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/tracks/{track_id}/stream"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            (resp.status(), resp.headers().clone())
        };
        assert_eq!(full_status, StatusCode::OK);
        let etag = full_resp_headers
            .get(axum::http::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        let not_modified = axum::Router::clone(&app)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tracks/{track_id}/stream"))
                    .header("if-none-match", etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn track_played_increments_count_and_history_and_404s_for_missing() {
        let (app, pool) = new_test_app_with_pool("playback-played");
        let cookie = seed_user_session(&pool, "u1");
        let (track_id, _path) = seed_streamable_track(&pool);

        let (status, body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/tracks/{track_id}/played"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["ok"], true);

        let (history_status, history_body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/user/history")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(history_status, StatusCode::OK);
        assert_eq!(json_body(&history_body).as_array().unwrap().len(), 1);

        let (missing_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/tracks/does-not-exist/played")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn track_lyrics_returns_cached_value_and_404s_for_missing_track_without_network() {
        let (app, pool) = new_test_app_with_pool("playback-lyrics");
        let (track_id, _path) = seed_streamable_track(&pool);

        pool.lock()
            .unwrap()
            .execute(
                "INSERT INTO lyrics_cache(track_id, artist, title, lyrics, source) \
                 VALUES (?, 'Artist', 'Track', 'la la la', 'lrclib')",
                params![track_id],
            )
            .unwrap();

        let (status, body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/lyrics"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["lyrics"], "la la la");

        // A track that doesn't exist at all returns 404 immediately (no cache to check,
        // no network fetch attempted) — this is the only lyrics path safe to exercise
        // without a live LRCLIB/lyrics.ovh call (see accepted gaps in the coverage plan).
        let (missing_status, _) = send(
            app,
            Request::builder()
                .uri("/api/tracks/does-not-exist/lyrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn track_waveform_reports_missing_then_ready_after_saving() {
        let (app, pool) = new_test_app_with_pool("playback-waveform");
        let (track_id, _path) = seed_streamable_track(&pool);
        // Fresh DBs seed waveformGenerateOnMissing=true (see seed_default_settings in
        // boogiebox-db/lib.rs); disable it so the "missing" branch is deterministic and
        // doesn't fire a background ffmpeg waveform-generation spawn.
        pool.lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value='false' WHERE key='waveformGenerateOnMissing'",
                [],
            )
            .unwrap();

        let (missing_status, missing_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/waveform"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::OK);
        assert_eq!(json_body(&missing_body)["status"], "missing");

        boogiebox_db::playback::save_track_waveform(
            &pool.lock().unwrap(),
            &track_id,
            3,
            Some(1.5),
            "[1,2,3]",
        )
        .unwrap();

        let (ready_status, ready_body) = send(
            app,
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/waveform"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(ready_status, StatusCode::OK);
        assert_eq!(json_body(&ready_body)["status"], "ready");
    }

    #[tokio::test]
    async fn track_waveform_generate_404s_when_track_or_file_is_missing() {
        let (app, pool) = new_test_app_with_pool("playback-waveform-generate");
        let (track_id, path) = seed_streamable_track(&pool);
        std::fs::remove_file(&path).unwrap();

        let (missing_file_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/tracks/{track_id}/waveform/generate"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_file_status, StatusCode::NOT_FOUND);

        let (missing_track_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/tracks/does-not-exist/waveform/generate")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_track_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn track_eq_profile_returns_default_for_seeded_track() {
        let (app, pool) = new_test_app_with_pool("playback-eq-profile");
        let (track_id, _path) = seed_streamable_track(&pool);
        let (status, body) = send(
            app,
            Request::builder()
                .uri(format!("/api/tracks/{track_id}/eq-profile"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["eqProfile"], "Rock");
    }

    #[tokio::test]
    async fn waveform_map_and_bpm_admin_routes_are_reachable_on_an_empty_library() {
        let (app, pool) = new_test_app_with_pool("playback-admin-batches");
        let cookie = seed_admin_session(&pool, "admin1");

        for (method, path) in [
            ("GET", "/api/waveforms/map/status"),
            ("POST", "/api/waveforms/map/run"),
            ("GET", "/api/bpm/status"),
            ("POST", "/api/bpm/run"),
        ] {
            let (status, _) = send(
                app.clone(),
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("cookie", cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{method} {path}");
        }

        let (forbidden_status, _) = send(
            app,
            Request::builder()
                .uri("/api/waveforms/map/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::UNAUTHORIZED);
    }
}

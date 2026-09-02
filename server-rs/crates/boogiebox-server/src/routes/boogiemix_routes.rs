//! Defines Rust API routes for Boogiemix Routes server behavior.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use boogiebox_db::{
    boogiemix::{
        cancel_mix_job, clear_deep_analysis_cache, count_playlist_deep_analysis_ready,
        enqueue_mix_job, get_deep_analysis_cache_status, get_deep_analysis_queue_status,
        get_mix_job, get_mix_job_logs, get_mix_output_file, get_mix_transitions,
        get_playlist_deep_analysis_progress, get_setting, list_mix_outputs,
        queue_library_deep_analysis, queue_playlist_deep_analysis, DeepAnalysisCacheStatus,
        DeepAnalysisQueueStatus, MixJobLogRow, MixJobRow, MixTransitionRow,
    },
    jobs::JobError,
    music::coerce_entity_id,
    upsert_setting,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::fs::File;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::io::ReaderStream;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    ffmpeg::{parse_byte_range, resolve_ffmpeg},
    DbPool, ErrorResponse, OkResponse, SharedState,
};

/// Body of a BoogieMix job request.
///
/// Fields carry an explicit snake_case alias alongside the struct-wide
/// camelCase rename: the client posts `default_crossfade_sec`, so without the
/// alias the field never deserialized and every mix silently fell back to the
/// 8 s default — the playlist UI's blend-length selector (up to "45s blend")
/// had no effect on the rendered mix at all.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueRequest {
    #[serde(default, alias = "playlist_id")]
    playlist_id: Option<serde_json::Value>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default, alias = "default_crossfade_sec")]
    default_crossfade_sec: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepRuntimeStatus {
    python_available: bool,
    ffmpeg_available: bool,
    demucs_callable: bool,
    torch_available: bool,
    gpu_available: bool,
    madmom_available: bool,
    enabled: bool,
    details: Vec<String>,
    missing_capabilities: Vec<String>,
    summary: String,
    python: RuntimeComponentStatus,
    ffmpeg: RuntimeComponentStatus,
    demucs: RuntimeComponentStatus,
    torch: RuntimeComponentStatus,
    gpu: RuntimeComponentStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeComponentStatus {
    available: bool,
    version: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeepAnalysisStatusResponse {
    enabled: bool,
    runtime: DeepRuntimeStatus,
    queue: DeepAnalysisQueueStatus,
    cache: DeepAnalysisCacheStatus,
    controls: DeepAnalysisControls,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepAnalysisControls {
    background_mode: String,
    pause_background: bool,
}

/// Documents the Boogiemix Router public API surface.
pub fn boogiemix_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/playlists/{id}/boogiemix/jobs",
            post(enqueue_for_playlist_handler),
        )
        .route("/api/boogiemix/create", post(create_handler))
        .route("/api/boogiemix/jobs/{jobId}", get(get_job_handler))
        .route(
            "/api/boogiemix/jobs/{jobId}/cancel",
            post(cancel_job_handler),
        )
        .route(
            "/api/playlists/{id}/boogiemix/outputs",
            get(list_outputs_handler),
        )
        .route(
            "/api/boogiemix/outputs/{outputId}/file",
            get(download_output_handler),
        )
        .route(
            "/api/boogiemix/outputs/{outputId}/play",
            get(play_output_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/status",
            get(deep_analysis_status_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/playlists/{playlistId}/queue",
            post(queue_playlist_deep_analysis_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/playlists/{playlistId}/progress",
            get(playlist_deep_analysis_progress_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/libraries/{libraryId}/queue",
            post(queue_library_deep_analysis_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/pause",
            post(pause_deep_analysis_background_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/resume",
            post(resume_deep_analysis_background_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/cache/clear",
            post(clear_deep_analysis_cache_handler),
        )
        .with_state(state)
}

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn db_not_configured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

fn not_found(msg: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn internal_error(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn resolve_style(s: Option<&str>) -> &'static str {
    match s {
        Some("chill_blend") => "chill_blend",
        Some("long_build") => "long_build",
        Some("safe_mix") => "safe_mix",
        _ => "club_blend",
    }
}

fn resolve_quality(s: Option<&str>) -> &'static str {
    if s == Some("high_quality") {
        "high_quality"
    } else {
        "standard"
    }
}

// -- Handlers ------------------------------------------------------------------

async fn enqueue_for_playlist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(playlist_id_raw): Path<String>,
    Json(body): Json<EnqueueRequest>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let playlist_id = coerce_entity_id(&playlist_id_raw);
    let user_id = coerce_entity_id(&user.id);
    let crossfade = body.default_crossfade_sec.unwrap_or(8).clamp(4, 60);
    let style = resolve_style(body.style.as_deref());
    let quality = resolve_quality(body.quality.as_deref());

    let result = match db.lock() {
        Ok(conn) => match enqueue_mix_job(&conn, &playlist_id, &user_id, crossfade, style, quality)
        {
            Ok(job_id) => (
                StatusCode::CREATED,
                Json(serde_json::json!({ "jobId": job_id })),
            )
                .into_response(),
            Err(e) => {
                // Match the JobError variant directly rather than sniffing its Display
                // text: `EmptyFolders` (reused here for "playlist needs >= 2 tracks")
                // renders as "At least one folder is required", which never contained
                // the literal substrings this branch used to check for — so this path
                // always 500'd instead of 400ing (found via coverage testing 2026-08-21).
                let status = match e {
                    JobError::LibraryNotFound | JobError::EmptyFolders => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    Json(ErrorResponse {
                        error: e.to_string(),
                        setup_required: None,
                    }),
                )
                    .into_response()
            }
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn create_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(body): Json<EnqueueRequest>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let Some(playlist_id_str) = body.playlist_id.as_ref().and_then(|v| {
        v.as_str()
            .map(str::to_owned)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    }) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "playlist_id is required".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let playlist_id = coerce_entity_id(&playlist_id_str);
    let user_id = coerce_entity_id(&user.id);
    let crossfade = body.default_crossfade_sec.unwrap_or(8).clamp(4, 60);
    let style = resolve_style(body.style.as_deref());
    let quality = resolve_quality(body.quality.as_deref());

    let result = match db.lock() {
        Ok(conn) => match enqueue_mix_job(&conn, &playlist_id, &user_id, crossfade, style, quality)
        {
            Ok(job_id) => (
                StatusCode::CREATED,
                Json(serde_json::json!({ "jobId": job_id })),
            )
                .into_response(),
            Err(e) => {
                // See the matching note in `enqueue_for_playlist_handler`: match the
                // JobError variant directly, not its Display text.
                let status = match e {
                    JobError::LibraryNotFound | JobError::EmptyFolders => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    Json(ErrorResponse {
                        error: e.to_string(),
                        setup_required: None,
                    }),
                )
                    .into_response()
            }
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn get_job_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(job_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let job_id = coerce_entity_id(&job_id_raw);
    let user_id = coerce_entity_id(&user.id);

    let result = match db.lock() {
        Ok(conn) => match get_mix_job(&conn, &job_id, &user_id) {
            Ok(Some(job)) => {
                let deep_counts =
                    count_playlist_deep_analysis_ready(&conn, &job.playlist_id).unwrap_or((0, 0));
                let transitions = get_mix_transitions(&conn, &job_id).unwrap_or_default();
                let logs = get_mix_job_logs(&conn, &job_id).unwrap_or_default();
                let plan_summary: Option<serde_json::Value> = conn
                    .query_row(
                        "SELECT normalized_plan FROM boogiemix_plans \
                         WHERE playlist_id=?1 ORDER BY id DESC LIMIT 1",
                        rusqlite::params![job.playlist_id],
                        |r| r.get::<_, String>(0),
                    )
                    .ok()
                    .and_then(|raw| {
                        let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
                        let plan = &parsed["plan"];
                        Some(serde_json::json!({
                            "style": plan["style"],
                            "energyCurvePhases": plan.get("energyCurvePhases").unwrap_or(&plan["energy_curve_phases"]),
                            "orderedTrackIds": plan.get("orderedTrackIds").unwrap_or(&plan["ordered_track_ids"]),
                            "anthemTrackId": plan.get("anthemTrackId").unwrap_or(&plan["anthem_track_id"]),
                        }))
                    });

                (
                    StatusCode::OK,
                    Json(job_response(
                        job,
                        transitions,
                        logs,
                        plan_summary,
                        deep_counts,
                    )),
                )
                    .into_response()
            }
            Ok(None) => not_found("Job not found"),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn cancel_job_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(job_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let job_id = coerce_entity_id(&job_id_raw);
    let user_id = coerce_entity_id(&user.id);

    let result = match db.lock() {
        Ok(conn) => match cancel_mix_job(&conn, &job_id, &user_id) {
            Ok(true) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
            Ok(false) => (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "Job cannot be canceled in current state".into(),
                    setup_required: None,
                }),
            )
                .into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn list_outputs_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(playlist_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let playlist_id = coerce_entity_id(&playlist_id_raw);
    let user_id = coerce_entity_id(&user.id);

    let result = match db.lock() {
        Ok(conn) => {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM playlists WHERE id=?1 AND user_id=?2",
                    rusqlite::params![playlist_id, user_id],
                    |_| Ok(()),
                )
                .is_ok();
            if !exists {
                not_found("Playlist not found")
            } else {
                match list_mix_outputs(&conn, &playlist_id, &user_id) {
                    Ok(rows) => (StatusCode::OK, Json(rows)).into_response(),
                    Err(e) => internal_error(&e.to_string()),
                }
            }
        }
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

/// Looks up a mix output's file path + name for `user_id`, and verifies the
/// resolved path lives inside the configured mix-output folder. Shared by the
/// download and inline-play handlers so both enforce the same ownership and
/// containment checks.
async fn resolve_output_for_serving(
    state: &SharedState,
    db: &DbPool,
    output_id_raw: &str,
    user_id_raw: &str,
) -> Result<(PathBuf, String), Response> {
    let db_folder = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        s.db_folder.clone()
    };

    let output_id = coerce_entity_id(output_id_raw);
    let user_id = coerce_entity_id(user_id_raw);

    let row = match db.lock() {
        Ok(conn) => get_mix_output_file(&conn, &output_id, &user_id),
        Err(_) => return Err(internal_error("DB lock failed")),
    };

    let (file_path, file_name) = match row {
        Ok(Some(r)) => r,
        Ok(None) => return Err(not_found("Output not found")),
        Err(e) => return Err(internal_error(&e.to_string())),
    };

    // Security: output must be inside the known mix-output folder
    let configured = match db.lock() {
        Ok(conn) => boogiebox_db::boogiemix::get_mix_output_dir_from_db(&conn),
        Err(_) => None,
    };
    let out_dir = resolve_output_dir(configured, db_folder.as_ref());
    let canonical_out = std::fs::canonicalize(&out_dir).unwrap_or(out_dir);
    let canonical_file =
        std::fs::canonicalize(&file_path).unwrap_or_else(|_| PathBuf::from(&file_path));
    if !canonical_file.starts_with(&canonical_out) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Output file is outside the BoogieMix output folder".into(),
                setup_required: None,
            }),
        )
            .into_response());
    }

    Ok((PathBuf::from(file_path), file_name))
}

async fn download_output_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(output_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };

    let (file_path, file_name) =
        match resolve_output_for_serving(&state, &db, &output_id_raw, &user.id).await {
            Ok(v) => v,
            Err(resp) => return resp,
        };

    let file = match File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => return not_found("Output file missing on disk"),
    };

    let safe_name = sanitize_filename(&file_name);
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{safe_name}\""),
        )
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Streams a finished BoogieMix render inline (no forced download), with
/// `Range` support so the client's `<audio>` element can seek/scrub exactly
/// as it does for library tracks (`playback_routes.rs::stream_track_handler`).
async fn play_output_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(output_id_raw): Path<String>,
    req_headers: axum::http::HeaderMap,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };

    let (file_path, _file_name) =
        match resolve_output_for_serving(&state, &db, &output_id_raw, &user.id).await {
            Ok(v) => v,
            Err(resp) => return resp,
        };

    let meta = match tokio::fs::metadata(&file_path).await {
        Ok(m) => m,
        Err(_) => return not_found("Output file missing on disk"),
    };
    let file_size = meta.len();

    let range_header = req_headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if let Some(rh) = range_header {
        return match parse_byte_range(&rh, file_size) {
            None => Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::empty())
                .unwrap_or_else(|_| internal_error("range build failed")),
            Some((start, end)) => {
                let mut file = match File::open(&file_path).await {
                    Ok(f) => f,
                    Err(_) => return internal_error("Failed to open output file"),
                };
                if tokio::io::AsyncSeekExt::seek(&mut file, std::io::SeekFrom::Start(start))
                    .await
                    .is_err()
                {
                    return internal_error("Failed to seek output file");
                }
                let len = end - start + 1;
                let reader = tokio::io::AsyncReadExt::take(file, len);
                Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{file_size}"),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_LENGTH, len.to_string())
                    .header(header::CONTENT_TYPE, "audio/mpeg")
                    .body(Body::from_stream(ReaderStream::new(reader)))
                    .unwrap_or_else(|_| internal_error("range build failed"))
            }
        };
    }

    let file = match File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => return not_found("Output file missing on disk"),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|_| internal_error("response build failed"))
}

async fn deep_analysis_status_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let (enabled, controls, queue, cache) = match db.lock() {
        Ok(conn) => match get_deep_analysis_queue_status(&conn) {
            Ok(queue) => {
                let enabled = parse_bool_setting(
                    get_setting(&conn, "boogiemixDeepAnalysisEnabled").as_deref(),
                    true,
                );
                let controls = DeepAnalysisControls {
                    background_mode: normalize_background_mode(
                        get_setting(&conn, "boogiemixDeepAnalysisBackgroundMode").as_deref(),
                    ),
                    pause_background: parse_bool_setting(
                        get_setting(&conn, "boogiemixDeepAnalysisPauseBackground").as_deref(),
                        false,
                    ),
                };
                let cache = match get_deep_analysis_cache_status(&conn) {
                    Ok(cache) => cache,
                    Err(e) => return internal_error(&e.to_string()),
                };
                (enabled, controls, queue, cache)
            }
            Err(e) => return internal_error(&e.to_string()),
        },
        Err(_) => return internal_error("DB lock failed"),
    };
    let runtime = detect_deep_runtime().await;
    (
        StatusCode::OK,
        Json(DeepAnalysisStatusResponse {
            enabled,
            runtime,
            queue,
            cache,
            controls,
        }),
    )
        .into_response()
}

async fn queue_playlist_deep_analysis_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
    Path(playlist_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let playlist_id = coerce_entity_id(&playlist_id_raw);
    let result = match db.lock() {
        Ok(conn) => match queue_playlist_deep_analysis(&conn, &playlist_id, true) {
            Ok(queued) => (
                StatusCode::OK,
                Json(serde_json::json!({ "queued": queued })),
            )
                .into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn playlist_deep_analysis_progress_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
    Path(playlist_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let playlist_id = coerce_entity_id(&playlist_id_raw);
    let result = match db.lock() {
        Ok(conn) => match get_playlist_deep_analysis_progress(&conn, &playlist_id) {
            Ok(progress) => (StatusCode::OK, Json(progress)).into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn queue_library_deep_analysis_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
    Path(library_id_raw): Path<String>,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let library_id = coerce_entity_id(&library_id_raw);
    let result = match db.lock() {
        Ok(conn) => match queue_library_deep_analysis(&conn, &library_id, true) {
            Ok(queued) => (
                StatusCode::OK,
                Json(serde_json::json!({ "queued": queued })),
            )
                .into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn pause_deep_analysis_background_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
) -> Response {
    update_deep_analysis_pause(state, true)
}

async fn resume_deep_analysis_background_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
) -> Response {
    update_deep_analysis_pause(state, false)
}

fn update_deep_analysis_pause(state: SharedState, paused: bool) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let result = match db.lock() {
        Ok(conn) => match upsert_setting(
            &conn,
            "boogiemixDeepAnalysisPauseBackground",
            if paused { "true" } else { "false" },
        ) {
            Ok(()) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

async fn clear_deep_analysis_cache_handler(
    State(state): State<SharedState>,
    AdminUser(_user): AdminUser,
) -> Response {
    let Some(db) = get_db(&state) else {
        return db_not_configured();
    };
    let result = match db.lock() {
        Ok(conn) => match clear_deep_analysis_cache(&conn) {
            Ok((deleted_cache_rows, deleted_job_rows)) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "deletedCacheRows": deleted_cache_rows,
                    "deletedJobRows": deleted_job_rows,
                })),
            )
                .into_response(),
            Err(e) => internal_error(&e.to_string()),
        },
        Err(_) => internal_error("DB lock failed"),
    };
    result
}

fn resolve_output_dir(
    configured: Option<String>,
    db_folder: Option<&std::path::PathBuf>,
) -> std::path::PathBuf {
    if let Some(dir) = configured {
        let dir = dir.trim().to_string();
        if !dir.is_empty() {
            let p = std::path::PathBuf::from(&dir);
            if p.is_absolute() {
                return p;
            }
            if let Some(base) = db_folder {
                return base.join(p);
            }
            return p;
        }
    }
    if let Some(base) = db_folder {
        return base.join("mix-outputs");
    }
    std::path::PathBuf::from("mix-outputs")
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '"' | '\'' | '\\' | '/' | '\0' | '\r' | '\n' => '_',
            c => c,
        })
        .collect()
}

fn job_response(
    job: MixJobRow,
    transitions: Vec<MixTransitionRow>,
    logs: Vec<MixJobLogRow>,
    plan_summary: Option<serde_json::Value>,
    deep_counts: (i64, i64),
) -> serde_json::Value {
    let missing_reason = deep_analysis_missing_reason(&job, deep_counts);
    let mut value = serde_json::to_value(job).unwrap_or_else(|_| serde_json::json!({}));
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "transitions".to_string(),
            serde_json::to_value(transitions).unwrap_or_else(|_| serde_json::json!([])),
        );
        map.insert(
            "logs".to_string(),
            serde_json::to_value(logs).unwrap_or_else(|_| serde_json::json!([])),
        );
        map.insert(
            "plan_summary".to_string(),
            plan_summary.unwrap_or(serde_json::Value::Null),
        );
        map.insert(
            "deep_analysis_ready_count".to_string(),
            serde_json::json!(deep_counts.0),
        );
        map.insert(
            "deep_analysis_total_count".to_string(),
            serde_json::json!(deep_counts.1),
        );
        map.insert(
            "deep_analysis_missing_reason".to_string(),
            missing_reason
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
    }
    value
}

fn deep_analysis_missing_reason(job: &MixJobRow, deep_counts: (i64, i64)) -> Option<String> {
    if job.mix_quality != "high_quality" || job.used_deep_analysis {
        return None;
    }
    match job.deep_analysis_status.as_deref() {
        Some("disabled") => Some("Deep analysis is disabled.".to_string()),
        Some("unavailable") => Some(
            "Deep analysis dependencies are unavailable; standard analysis was used.".to_string(),
        ),
        Some("timeout") => Some(
            "Deep analysis did not finish before planning; standard analysis was used.".to_string(),
        ),
        Some("partial") => Some(format!(
            "Deep analysis was partially ready ({}/{} tracks); standard analysis filled the gaps.",
            deep_counts.0, deep_counts.1
        )),
        Some("fallback_standard") | None => {
            if deep_counts.0 == 0 {
                Some(
                    "Deep analysis was not ready for this playlist; standard analysis was used."
                        .to_string(),
                )
            } else {
                Some(format!(
                    "Deep analysis was partially ready ({}/{} tracks); standard analysis filled the gaps.",
                    deep_counts.0, deep_counts.1
                ))
            }
        }
        Some(other) => Some(format!("Deep analysis status: {other}.")),
    }
}

fn parse_bool_setting(raw: Option<&str>, default_value: bool) -> bool {
    match raw.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "true" => true,
        Some(v) if v == "false" => false,
        _ => default_value,
    }
}

async fn detect_deep_runtime() -> DeepRuntimeStatus {
    let mut details = Vec::new();
    let mut missing_capabilities = Vec::new();
    let python = detect_python().await;
    let python_available = python.is_some();
    let python_version = if let Some(invocation) = python.as_ref() {
        python_string(
            invocation,
            "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}')",
        )
        .await
    } else {
        None
    };
    let python_detail = match python.as_ref() {
        Some(invocation) => Some(format!("Using {}", invocation.display_name)),
        None => {
            missing_capabilities.push("python".to_string());
            Some("Python 3.10+ was not found.".to_string())
        }
    };
    details.push(if python_available {
        "python:ok".to_string()
    } else {
        "python:missing".to_string()
    });

    let ffmpeg_available =
        command_success(resolve_ffmpeg(), ["-version"], Duration::from_secs(5)).await;
    if !ffmpeg_available {
        missing_capabilities.push("ffmpeg".to_string());
    }
    details.push(
        if ffmpeg_available {
            "ffmpeg:ok"
        } else {
            "ffmpeg:missing"
        }
        .to_string(),
    );

    let mut demucs_callable = false;
    let mut torch_available = false;
    let mut gpu_available = false;
    let mut madmom_available = false;
    let mut demucs_version = None;
    let mut torch_version = None;
    if let Some(invocation) = python.as_ref() {
        // Query a real version string rather than importlib.util.find_spec():
        // find_spec only confirms the package's files are on disk, not that
        // `import` actually succeeds — a package present but broken (e.g. a
        // syntax/runtime error, an ABI-incompatible compiled extension, or an
        // incompatible Python/numpy version) still reports find_spec() truthy
        // while every real import fails. python_string() returns None when
        // the subprocess raises, so a successful version fetch is proof the
        // import genuinely works.
        demucs_version = python_string(
            invocation,
            "import demucs; print(getattr(demucs, '__version__', 'installed'))",
        )
        .await;
        demucs_callable = demucs_version.is_some();
        if !demucs_callable {
            missing_capabilities.push("demucs".to_string());
        }
        details.push(
            if demucs_callable {
                "demucs:ok"
            } else {
                "demucs:missing"
            }
            .to_string(),
        );

        torch_version = python_string(
            invocation,
            "import torch; print(getattr(torch, '__version__', 'installed'))",
        )
        .await;
        torch_available = torch_version.is_some();
        if !torch_available {
            missing_capabilities.push("torch".to_string());
        }
        details.push(
            if torch_available {
                "torch:ok"
            } else {
                "torch:missing"
            }
            .to_string(),
        );

        if torch_available {
            gpu_available = python_bool(
                invocation,
                "import torch; print('true' if torch.cuda.is_available() else 'false')",
            )
            .await;
        }
        details.push(if gpu_available { "gpu:cuda" } else { "gpu:cpu" }.to_string());

        // madmom 0.16.1 needs the same collections/numpy compatibility shims
        // boogiemix_demucs_worker.py applies before importing it, so this
        // check reflects whether the worker's actual import will succeed.
        madmom_available = python_bool(
            invocation,
            "import collections, collections.abc\n\
             if not hasattr(collections, 'MutableSequence'):\n\
             \x20   collections.MutableSequence = collections.abc.MutableSequence\n\
             import numpy as _np\n\
             if not hasattr(_np, 'float'):\n\
             \x20   _np.float = float\n\
             if not hasattr(_np, 'int'):\n\
             \x20   _np.int = int\n\
             if not hasattr(_np, 'bool'):\n\
             \x20   _np.bool = bool\n\
             import madmom\n\
             print('true')",
        )
        .await;
        details.push(
            if madmom_available {
                "madmom:ok"
            } else {
                "madmom:missing"
            }
            .to_string(),
        );
    }
    let enabled = python_available && ffmpeg_available && demucs_callable && torch_available;
    let summary = runtime_summary(enabled, &missing_capabilities, gpu_available);

    DeepRuntimeStatus {
        python_available,
        ffmpeg_available,
        demucs_callable,
        torch_available,
        gpu_available,
        madmom_available,
        enabled,
        details,
        missing_capabilities,
        summary,
        python: RuntimeComponentStatus {
            available: python_available,
            version: python_version,
            detail: python_detail,
        },
        ffmpeg: RuntimeComponentStatus {
            available: ffmpeg_available,
            version: None,
            detail: if ffmpeg_available {
                Some("FFmpeg command is available.".to_string())
            } else {
                Some("FFmpeg command is unavailable.".to_string())
            },
        },
        demucs: RuntimeComponentStatus {
            available: demucs_callable,
            version: demucs_version,
            detail: if demucs_callable {
                Some("Demucs can be imported by the selected Python runtime.".to_string())
            } else {
                Some("Demucs is missing from the selected Python runtime.".to_string())
            },
        },
        torch: RuntimeComponentStatus {
            available: torch_available,
            version: torch_version,
            detail: if torch_available {
                Some("Torch can be imported by the selected Python runtime.".to_string())
            } else {
                Some("Torch is missing from the selected Python runtime.".to_string())
            },
        },
        gpu: RuntimeComponentStatus {
            available: gpu_available,
            version: None,
            detail: if gpu_available {
                Some("CUDA is available to Torch.".to_string())
            } else {
                Some("CUDA is unavailable; CPU deep analysis can still run.".to_string())
            },
        },
    }
}

fn normalize_background_mode(raw: Option<&str>) -> String {
    match raw.unwrap_or("off") {
        "playlists_only" | "favorites_and_playlists" | "all_music" => raw.unwrap().to_string(),
        _ => "off".to_string(),
    }
}

fn runtime_summary(enabled: bool, missing: &[String], gpu_available: bool) -> String {
    if enabled {
        if gpu_available {
            "Ready with GPU acceleration.".to_string()
        } else {
            "Ready using CPU analysis.".to_string()
        }
    } else if missing.is_empty() {
        "Deep analysis runtime is unavailable.".to_string()
    } else {
        format!("Missing: {}.", missing.join(", "))
    }
}

#[derive(Debug, Clone)]
struct PythonInvocation {
    command: PathBuf,
    base_args: Vec<String>,
    display_name: String,
}

async fn detect_python() -> Option<PythonInvocation> {
    let mut candidates = python_candidates();
    candidates.push(PythonInvocation {
        command: PathBuf::from("python"),
        base_args: Vec::new(),
        display_name: "python".to_string(),
    });
    candidates.push(PythonInvocation {
        command: PathBuf::from("py"),
        base_args: vec!["-3".to_string()],
        display_name: "py -3".to_string(),
    });

    for candidate in candidates {
        if python_min_version(&candidate).await {
            return Some(candidate);
        }
    }
    None
}

fn python_candidates() -> Vec<PythonInvocation> {
    // Check both the dev layout (repo-root/Services/...) and the installed layout
    // (exe-dir/resources/Services/...) for the managed venv Python.
    // Windows venv: .venv/Scripts/python.exe  Linux venv: .venv/bin/python
    #[cfg(windows)]
    let venv_python: &[&str] = &["Scripts", "python.exe"];
    #[cfg(not(windows))]
    let venv_python: &[&str] = &["bin", "python"];

    let base_rels: &[&[&str]] = &[
        &["Services", "boogiemix", "python", ".venv"],
        &["resources", "Services", "boogiemix", "python", ".venv"],
    ];
    let mut dirs = Vec::new();
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    dirs.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".."),
    );

    let mut candidates = Vec::new();
    for dir in dirs {
        for base in base_rels {
            let venv_root = base.iter().fold(dir.clone(), |p, s| p.join(s));
            let path = venv_python.iter().fold(venv_root, |p, s| p.join(s));
            if path.is_file() {
                candidates.push(PythonInvocation {
                    display_name: path.display().to_string(),
                    command: path,
                    base_args: Vec::new(),
                });
            }
        }
    }
    candidates
}

async fn python_min_version(invocation: &PythonInvocation) -> bool {
    let script = "import sys; print('true' if sys.version_info >= (3, 10) else 'false')";
    python_bool(invocation, script).await
}

async fn python_bool(invocation: &PythonInvocation, script: &str) -> bool {
    let mut args = invocation.base_args.clone();
    args.push("-c".to_string());
    args.push(script.to_string());
    let output = command_output(&invocation.command, args, Duration::from_secs(10)).await;
    output
        .map(|stdout| stdout.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

async fn python_string(invocation: &PythonInvocation, script: &str) -> Option<String> {
    let mut args = invocation.base_args.clone();
    args.push("-c".to_string());
    args.push(script.to_string());
    command_output(&invocation.command, args, Duration::from_secs(10))
        .await
        .map(|stdout| stdout.trim().to_string())
        .filter(|stdout| !stdout.is_empty())
}

async fn command_success<I, S>(command: PathBuf, args: I, duration: Duration) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    command_output(&command, args, duration).await.is_some()
}

async fn command_output<I, S>(command: &PathBuf, args: I, duration: Duration) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = timeout(duration, cmd.output()).await.ok()?.ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::music::EntityId;

    #[test]
    fn enqueue_request_accepts_the_snake_case_body_the_client_sends() {
        // Regression: the struct-wide camelCase rename meant the client's
        // `default_crossfade_sec` never bound, so every mix rendered at the 8s
        // default and the playlist UI's blend-length selector did nothing.
        let snake: EnqueueRequest =
            serde_json::from_str(r#"{"style":"long_build","default_crossfade_sec":45}"#).unwrap();
        assert_eq!(snake.default_crossfade_sec, Some(45));
        assert_eq!(snake.style.as_deref(), Some("long_build"));

        let camel: EnqueueRequest = serde_json::from_str(r#"{"defaultCrossfadeSec":32}"#).unwrap();
        assert_eq!(camel.default_crossfade_sec, Some(32));

        let omitted: EnqueueRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(omitted.default_crossfade_sec, None);
    }

    #[test]
    fn boogiemix_job_response_is_flat_client_contract() {
        let job = MixJobRow {
            id: EntityId::Str("job-1".to_string()),
            playlist_id: EntityId::Str("playlist-1".to_string()),
            user_id: EntityId::Str("user-1".to_string()),
            status: "pending".to_string(),
            progress_percent: 0,
            current_step: "queued".to_string(),
            last_message: None,
            default_crossfade_sec: 8,
            mix_style: "club_blend".to_string(),
            mix_quality: "standard".to_string(),
            mix_strategy: None,
            planner_provider: None,
            used_deep_analysis: false,
            deep_analysis_status: None,
            cancel_requested: false,
            output_id: None,
            started_at: None,
            finished_at: None,
            created_at: Some("2026-05-24 12:00:00".to_string()),
            updated_at: Some("2026-05-24 12:00:00".to_string()),
        };

        let value = job_response(
            job,
            Vec::new(),
            Vec::new(),
            Some(serde_json::json!({"style":"club_blend"})),
            (0, 0),
        );

        assert_eq!(value["id"], "job-1");
        assert!(value.get("job").is_none());
        assert_eq!(value["transitions"], serde_json::json!([]));
        assert_eq!(value["logs"], serde_json::json!([]));
        assert_eq!(value["plan_summary"]["style"], "club_blend");
    }

    #[test]
    fn parse_bool_setting_matches_node_defaults() {
        assert!(parse_bool_setting(Some(" true "), false));
        assert!(!parse_bool_setting(Some("FALSE"), true));
        assert!(parse_bool_setting(Some("bogus"), true));
        assert!(!parse_bool_setting(None, false));
    }

    #[test]
    fn high_quality_fallback_reason_mentions_standard_path() {
        let job = MixJobRow {
            id: EntityId::Str("job-1".to_string()),
            playlist_id: EntityId::Str("playlist-1".to_string()),
            user_id: EntityId::Str("user-1".to_string()),
            status: "done".to_string(),
            progress_percent: 100,
            current_step: "done".to_string(),
            last_message: None,
            default_crossfade_sec: 8,
            mix_style: "club_blend".to_string(),
            mix_quality: "high_quality".to_string(),
            mix_strategy: None,
            planner_provider: None,
            used_deep_analysis: false,
            deep_analysis_status: Some("fallback_standard".to_string()),
            cancel_requested: false,
            output_id: None,
            started_at: None,
            finished_at: None,
            created_at: None,
            updated_at: None,
        };

        let reason = deep_analysis_missing_reason(&job, (0, 4)).unwrap();

        assert!(reason.contains("standard analysis"));
    }

    #[test]
    fn runtime_summary_lists_missing_capabilities() {
        let missing = vec!["torch".to_string(), "demucs".to_string()];

        assert_eq!(
            runtime_summary(false, &missing, false),
            "Missing: torch, demucs."
        );
        assert_eq!(
            runtime_summary(true, &[], false),
            "Ready using CPU analysis."
        );
    }
}

#[cfg(test)]
mod route_tests {
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use crate::DbPool;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rusqlite::params;
    use uuid::Uuid;

    /// Seeds a playlist owned by `user_id` with 2 tracks (the minimum `enqueue_mix_job`
    /// requires), returning the playlist id.
    fn seed_playlist_with_two_tracks(pool: &DbPool, user_id: &str) -> String {
        let conn = pool.lock().unwrap();
        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, '/music', 'Lib')",
            params![library_id],
        )
        .unwrap();
        let artist_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Artist')",
            params![artist_id],
        )
        .unwrap();
        let playlist_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO playlists(id, user_id, name) VALUES (?, ?, 'Mix Source')",
            params![playlist_id, user_id],
        )
        .unwrap();
        for n in 0..2 {
            let track_id = Uuid::now_v7().to_string();
            conn.execute(
                "INSERT INTO tracks(id, library_id, artist_id, title, file_path) \
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    track_id,
                    library_id,
                    artist_id,
                    format!("Track {n}"),
                    format!("/music/{track_id}.mp3")
                ],
            )
            .unwrap();
            let pt_id = Uuid::now_v7().to_string();
            conn.execute(
                "INSERT INTO playlist_tracks(id, playlist_id, track_id, position) VALUES (?, ?, ?, ?)",
                params![pt_id, playlist_id, track_id, n],
            )
            .unwrap();
        }
        playlist_id
    }

    #[tokio::test]
    async fn enqueue_for_playlist_requires_two_tracks_and_a_real_playlist() {
        let (app, pool) = new_test_app_with_pool("boogiemix-enqueue");
        let cookie = seed_user_session(&pool, "u1");

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/playlists/does-not-exist/boogiemix/jobs")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::BAD_REQUEST);

        // Playlist with 0 tracks -> EmptyFolders (reused as "needs at least 2 tracks").
        pool.lock()
            .unwrap()
            .execute(
                "INSERT INTO playlists(id, user_id, name) VALUES ('empty-pl', 'u1', 'Empty')",
                [],
            )
            .unwrap();
        let (too_few_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/playlists/empty-pl/boogiemix/jobs")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(too_few_status, StatusCode::BAD_REQUEST);

        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/boogiemix/jobs"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"style":"long_build","quality":"high_quality"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(json_body(&body)["jobId"].is_string());
    }

    #[tokio::test]
    async fn create_handler_requires_playlist_id_in_body() {
        let (app, pool) = new_test_app_with_pool("boogiemix-create");
        let cookie = seed_user_session(&pool, "u1");
        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/boogiemix/create")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::BAD_REQUEST);

        let (status, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/boogiemix/create")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"playlistId":"{playlist_id}"}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(json_body(&body)["jobId"].is_string());
    }

    #[tokio::test]
    async fn get_job_returns_full_response_and_404s_for_missing_job() {
        let (app, pool) = new_test_app_with_pool("boogiemix-get-job");
        let cookie = seed_user_session(&pool, "u1");
        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");

        let (_, create_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/boogiemix/jobs"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        let job_id = json_body(&create_body)["jobId"]
            .as_str()
            .unwrap()
            .to_owned();

        let (status, body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/boogiemix/jobs/{job_id}"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["status"], "pending");

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/boogiemix/jobs/does-not-exist")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);

        let (cancel_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/boogiemix/jobs/{job_id}/cancel"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(cancel_status, StatusCode::OK);

        // Cancelling an already-cancelled job is a conflict, not success.
        let (recancel_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/boogiemix/jobs/{job_id}/cancel"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(recancel_status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn list_outputs_404s_for_unowned_playlist_and_ok_for_owned() {
        let (app, pool) = new_test_app_with_pool("boogiemix-outputs");
        let cookie = seed_user_session(&pool, "u1");
        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/playlists/does-not-exist/boogiemix/outputs")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);

        let (status, body) = send(
            app,
            Request::builder()
                .uri(format!("/api/playlists/{playlist_id}/boogiemix/outputs"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json_body(&body).as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn download_output_404s_for_unknown_output_id() {
        let (app, pool) = new_test_app_with_pool("boogiemix-download");
        let cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/boogiemix/outputs/does-not-exist/file")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn play_output_404s_for_unknown_output_id() {
        let (app, pool) = new_test_app_with_pool("boogiemix-play");
        let cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/boogiemix/outputs/does-not-exist/play")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn play_output_serves_inline_and_supports_range() {
        let (app, pool) = new_test_app_with_pool("boogiemix-play-range");
        let cookie = seed_user_session(&pool, "u1");
        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");

        // Write a fake mix output file under a temp out-dir and register a DB row.
        let out_dir = std::env::temp_dir().join(format!("boogiemix-play-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&out_dir).unwrap();
        let file_path = out_dir.join("mix.mp3");
        std::fs::write(&file_path, vec![0u8; 1000]).unwrap();

        let output_id = Uuid::now_v7().to_string();
        let job_id = Uuid::now_v7().to_string();
        {
            let conn = pool.lock().unwrap();
            conn.execute(
                "INSERT INTO mix_jobs(id, playlist_id, user_id, status, default_crossfade_sec, mix_style, mix_quality) \
                 VALUES (?, ?, 'u1', 'done', 8, 'club_blend', 'standard')",
                params![job_id, playlist_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO mix_outputs(id, job_id, playlist_id, user_id, file_path, file_name, duration_sec, file_size_bytes) \
                 VALUES (?, ?, ?, 'u1', ?, 'mix.mp3', 60.0, 1000)",
                params![
                    output_id,
                    job_id,
                    playlist_id,
                    file_path.to_string_lossy().to_string()
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES ('boogiemixOutputFolder', ?)",
                params![out_dir.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let (status, headers, body) = crate::test_support::send_full(
            app.clone(),
            Request::builder()
                .uri(format!("/api/boogiemix/outputs/{output_id}/play"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("content-disposition").is_none());
        assert_eq!(body.len(), 1000);

        let (range_status, range_headers, range_body) = crate::test_support::send_full(
            app,
            Request::builder()
                .uri(format!("/api/boogiemix/outputs/{output_id}/play"))
                .header("cookie", cookie)
                .header("range", "bytes=0-99")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(range_status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(range_body.len(), 100);
        assert_eq!(
            range_headers
                .get("content-range")
                .unwrap()
                .to_str()
                .unwrap(),
            "bytes 0-99/1000"
        );

        std::fs::remove_dir_all(&out_dir).ok();
    }

    #[tokio::test]
    async fn deep_analysis_status_route_requires_auth_and_returns_ok() {
        let (app, pool) = new_test_app_with_pool("boogiemix-deep-status");
        let (unauth_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/boogiemix/deep-analysis/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(unauth_status, StatusCode::UNAUTHORIZED);

        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/boogiemix/deep-analysis/status")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json_body(&body)["runtime"].is_object());
    }

    #[tokio::test]
    async fn deep_analysis_admin_routes_are_admin_gated_and_reachable() {
        let (app, pool) = new_test_app_with_pool("boogiemix-deep-admin");
        let user_cookie = seed_user_session(&pool, "u1");
        let playlist_id = seed_playlist_with_two_tracks(&pool, "u1");
        let library_id: String = pool
            .lock()
            .unwrap()
            .query_row("SELECT id FROM libraries LIMIT 1", [], |r| r.get(0))
            .unwrap();

        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/boogiemix/deep-analysis/playlists/{playlist_id}/queue"
                ))
                .header("cookie", user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        let admin_cookie = seed_admin_session(&pool, "admin1");

        let (queue_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/boogiemix/deep-analysis/playlists/{playlist_id}/queue"
                ))
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(queue_status, StatusCode::OK);

        let (progress_status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!(
                    "/api/boogiemix/deep-analysis/playlists/{playlist_id}/progress"
                ))
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(progress_status, StatusCode::OK);

        let (lib_queue_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/boogiemix/deep-analysis/libraries/{library_id}/queue"
                ))
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(lib_queue_status, StatusCode::OK);

        let (pause_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/boogiemix/deep-analysis/pause")
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(pause_status, StatusCode::OK);

        let (resume_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/boogiemix/deep-analysis/resume")
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(resume_status, StatusCode::OK);

        let (clear_status, clear_body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/boogiemix/deep-analysis/cache/clear")
                .header("cookie", admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(clear_status, StatusCode::OK);
        assert_eq!(json_body(&clear_body)["ok"], true);
    }
}

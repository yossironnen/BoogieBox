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
        get_mix_job, get_mix_job_logs, get_mix_output_file, get_mix_transitions, get_setting,
        list_mix_outputs, queue_library_deep_analysis, queue_playlist_deep_analysis,
        DeepAnalysisCacheStatus, DeepAnalysisQueueStatus, MixJobLogRow, MixJobRow,
        MixTransitionRow,
    },
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
    ffmpeg::resolve_ffmpeg,
    DbPool, ErrorResponse, OkResponse, SharedState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueRequest {
    #[serde(default)]
    playlist_id: Option<serde_json::Value>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
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
            "/api/boogiemix/deep-analysis/status",
            get(deep_analysis_status_handler),
        )
        .route(
            "/api/boogiemix/deep-analysis/playlists/{playlistId}/queue",
            post(queue_playlist_deep_analysis_handler),
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
                let msg = e.to_string();
                let status = if msg.contains("not found") || msg.contains("EmptyFolders") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (
                    status,
                    Json(ErrorResponse {
                        error: msg,
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
                let msg = e.to_string();
                let status = if msg.contains("not found")
                    || msg.contains("EmptyFolders")
                    || msg.contains("playlist_id")
                {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (
                    status,
                    Json(ErrorResponse {
                        error: msg,
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

async fn download_output_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(output_id_raw): Path<String>,
) -> Response {
    let (db, db_folder) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.db_folder.clone())
    };
    let Some(db) = db else {
        return db_not_configured();
    };

    let output_id = coerce_entity_id(&output_id_raw);
    let user_id = coerce_entity_id(&user.id);

    let row = {
        let result = match db.lock() {
            Ok(conn) => get_mix_output_file(&conn, &output_id, &user_id),
            Err(_) => return internal_error("DB lock failed"),
        };
        result
    };

    let (file_path, file_name) = match row {
        Ok(Some(r)) => r,
        Ok(None) => return not_found("Output not found"),
        Err(e) => return internal_error(&e.to_string()),
    };

    // Security: output must be inside the known mix-output folder
    let configured = {
        let c = match db.lock() {
            Ok(conn) => boogiebox_db::boogiemix::get_mix_output_dir_from_db(&conn),
            Err(_) => None,
        };
        c
    };
    let out_dir = resolve_output_dir(configured, db_folder.as_ref());
    let canonical_out = std::fs::canonicalize(&out_dir).unwrap_or(out_dir);
    let canonical_file =
        std::fs::canonicalize(&file_path).unwrap_or_else(|_| std::path::PathBuf::from(&file_path));
    if !canonical_file.starts_with(&canonical_out) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Output file is outside the BoogieMix output folder".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

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
        demucs_callable = python_bool(
            invocation,
            "import importlib.util; print('true' if importlib.util.find_spec('demucs') else 'false')",
        )
        .await;
        if demucs_callable {
            demucs_version = python_string(
                invocation,
                "import demucs; print(getattr(demucs, '__version__', 'installed'))",
            )
            .await;
        } else {
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

        torch_available = python_bool(
            invocation,
            "import importlib.util; print('true' if importlib.util.find_spec('torch') else 'false')",
        )
        .await;
        if torch_available {
            torch_version = python_string(
                invocation,
                "import torch; print(getattr(torch, '__version__', 'installed'))",
            )
            .await;
        } else {
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

        madmom_available = python_bool(
            invocation,
            "import importlib.util; print('true' if importlib.util.find_spec('madmom') else 'false')",
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
    let exe = PathBuf::from("Services")
        .join("boogiemix")
        .join("python")
        .join(".venv")
        .join("Scripts")
        .join("python.exe");
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

    dirs.into_iter()
        .map(|dir| dir.join(&exe))
        .filter(|path| path.is_file())
        .map(|path| PythonInvocation {
            display_name: "app-local Python runtime".to_string(),
            command: path,
            base_args: Vec::new(),
        })
        .collect()
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

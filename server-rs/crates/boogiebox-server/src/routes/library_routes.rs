//! Defines Rust API routes for Library Routes server behavior.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use boogiebox_db::{
    jobs::{CreateLibraryInput, JobError},
    music::coerce_entity_id,
};
use serde::Deserialize;
use std::fs;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    DbPool, ErrorResponse, OkResponse, SharedState,
};

/// Documents the Library Router public API surface.
pub fn library_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/libraries",
            get(list_libraries_handler).post(create_library_handler),
        )
        .route(
            "/api/libraries/{id}",
            put(rename_library_handler).delete(delete_library_handler),
        )
        .route("/api/libraries/{id}/folders", post(add_folder_handler))
        .route(
            "/api/libraries/{id}/folders/{folder_id}",
            delete(remove_folder_handler),
        )
        .route("/api/libraries/{id}/scan", post(enqueue_scan_handler))
        .route(
            "/api/libraries/{id}/scan-jobs",
            get(library_scan_jobs_handler),
        )
        .route("/api/scan-jobs/active", get(active_scan_jobs_handler))
        .route("/api/scan-jobs/{job_id}", get(scan_job_handler))
        .route("/api/schedules", get(list_schedules_handler))
        .route(
            "/api/schedules/{library_id}",
            get(get_schedule_handler)
                .put(upsert_schedule_handler)
                .delete(delete_schedule_handler),
        )
        .route("/api/admin/queues", get(queue_snapshot_handler))
        .route(
            "/api/admin/queues/scan/{job_id}/cancel",
            post(cancel_scan_job_handler),
        )
        .route(
            "/api/admin/queues/post-scan/{job_id}/fail",
            post(fail_post_scan_job_handler),
        )
        .route(
            "/api/admin/queues/post-scan/{job_id}/cancel",
            post(cancel_post_scan_job_handler),
        )
        .route(
            "/api/admin/queues/post-scan/{job_id}/retry",
            post(retry_post_scan_job_handler),
        )
        .route(
            "/api/admin/libraries/{library_id}/post-scan",
            post(enqueue_post_scan_handler),
        )
        .route("/api/stats", get(stats_handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateLibraryRequest {
    path: Option<String>,
    folders: Option<Vec<String>>,
    name: Option<String>,
    library_type: Option<String>,
    scanner_profile: Option<String>,
    metadata_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenameLibraryRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AddFolderRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
struct ScheduleRequest {
    enabled: Option<bool>,
    frequency_hours: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostScanRequest {
    job_type: Option<String>,
}

async fn list_libraries_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::music::list_libraries(&conn)
    })
    .await;

    match result {
        Ok(Ok(libs)) => (StatusCode::OK, Json(libs)).into_response(),
        _ => internal_error(),
    }
}

async fn stats_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::music::get_stats(&conn)
    })
    .await;

    match result {
        Ok(Ok(stats)) => (StatusCode::OK, Json(stats)).into_response(),
        _ => internal_error(),
    }
}

async fn create_library_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(payload): Json<CreateLibraryRequest>,
) -> impl IntoResponse {
    let folders = payload
        .folders
        .unwrap_or_else(|| payload.path.into_iter().collect());
    if let Err(response) = validate_folders(&folders).map_err(validation_error_response) {
        return response;
    }
    with_db(state, move |conn| {
        boogiebox_db::jobs::create_library(
            conn,
            CreateLibraryInput {
                folders,
                name: payload.name,
                library_type: payload.library_type,
                scanner_profile: payload.scanner_profile,
                metadata_mode: payload.metadata_mode,
            },
        )
    })
    .await
    .map(|library| (StatusCode::CREATED, Json(library)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn rename_library_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(payload): Json<RenameLibraryRequest>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::rename_library(conn, &id, &payload.name)
    })
    .await
    .map(|library| (StatusCode::OK, Json(library)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn delete_library_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::delete_library(conn, &id)
    })
    .await
    .map(|_| (StatusCode::OK, Json(OkResponse { ok: true })).into_response())
    .unwrap_or_else(map_job_error)
}

async fn add_folder_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(payload): Json<AddFolderRequest>,
) -> impl IntoResponse {
    if let Err(response) =
        validate_folders(std::slice::from_ref(&payload.path)).map_err(validation_error_response)
    {
        return response;
    }
    with_db(state, move |conn| {
        boogiebox_db::jobs::add_library_folder(conn, &id, &payload.path)
    })
    .await
    .map(|library| (StatusCode::CREATED, Json(library)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn remove_folder_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path((id, folder_id)): Path<(String, String)>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::remove_library_folder(conn, &id, &folder_id)
    })
    .await
    .map(|library| (StatusCode::OK, Json(library)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn enqueue_scan_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !user.is_admin() && !user.can_scan {
        return forbidden();
    }
    let ps_state = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        s.db.as_ref().map(|db| crate::post_scan::PostScanState {
            db: db.clone(),
            http_client: s.http_client.clone(),
            db_folder: s.db_folder.clone(),
        })
    };
    with_db(state, move |conn| {
        let library_id = coerce_entity_id(&id);
        let exists = conn
            .query_row(
                "SELECT id FROM libraries WHERE id=?1",
                [&library_id],
                |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return Err(JobError::LibraryNotFound);
        }
        boogiebox_db::jobs::enqueue_scan_job(conn, &id)
    })
    .await
    .map(|job_id| {
        if let Some(state) = ps_state {
            tokio::spawn(crate::scanner::run_one_pending_scan(state));
        }
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            job_id: boogiebox_db::music::EntityId,
        }
        (StatusCode::OK, Json(Response { job_id })).into_response()
    })
    .unwrap_or_else(map_job_error)
}

async fn active_scan_jobs_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    with_db(state, |conn| {
        boogiebox_db::jobs::list_active_scan_jobs(conn).map_err(JobError::Db)
    })
    .await
    .map(|jobs| (StatusCode::OK, Json(jobs)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn scan_job_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::get_scan_job_detail(conn, &job_id)
    })
    .await
    .map(|job| (StatusCode::OK, Json(job)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn library_scan_jobs_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::list_library_scan_jobs(conn, &id).map_err(JobError::Db)
    })
    .await
    .map(|jobs| (StatusCode::OK, Json(jobs)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn list_schedules_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    with_db(state, |conn| {
        boogiebox_db::jobs::list_schedules(conn).map_err(JobError::Db)
    })
    .await
    .map(|schedules| (StatusCode::OK, Json(schedules)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn get_schedule_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(library_id): Path<String>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::get_schedule(conn, &coerce_entity_id(&library_id)).map_err(JobError::Db)
    })
    .await
    .map(|schedule| (StatusCode::OK, Json(schedule)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn upsert_schedule_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(library_id): Path<String>,
    Json(payload): Json<ScheduleRequest>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        let id = coerce_entity_id(&library_id);
        let exists = conn
            .query_row("SELECT id FROM libraries WHERE id=?1", [&id], |_| Ok(()))
            .is_ok();
        if !exists {
            return Err(JobError::LibraryNotFound);
        }
        boogiebox_db::jobs::upsert_schedule(
            conn,
            &id,
            payload.enabled.unwrap_or(true),
            payload.frequency_hours.unwrap_or(24.0),
        )
    })
    .await
    .map(|schedule| (StatusCode::OK, Json(schedule)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn delete_schedule_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(library_id): Path<String>,
) -> impl IntoResponse {
    with_db(state, move |conn| {
        boogiebox_db::jobs::delete_schedule(conn, &library_id).map_err(JobError::Db)
    })
    .await
    .map(|_| (StatusCode::OK, Json(OkResponse { ok: true })).into_response())
    .unwrap_or_else(map_job_error)
}

async fn queue_snapshot_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    with_db(state, |conn| {
        boogiebox_db::jobs::queue_snapshot(conn).map_err(JobError::Db)
    })
    .await
    .map(|snapshot| (StatusCode::OK, Json(snapshot)).into_response())
    .unwrap_or_else(map_job_error)
}

async fn cancel_scan_job_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let id_for_db = job_id.clone();
    with_db(state, move |conn| {
        boogiebox_db::jobs::cancel_scan_job(conn, &id_for_db)
    })
    .await
    .map(|_| status_response(&job_id, "cancelled"))
    .unwrap_or_else(map_job_error)
}

async fn fail_post_scan_job_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let id_for_db = job_id.clone();
    with_db(state, move |conn| {
        boogiebox_db::jobs::fail_post_scan_job(conn, &id_for_db)
    })
    .await
    .map(|_| status_response(&job_id, "failed"))
    .unwrap_or_else(map_job_error)
}

async fn cancel_post_scan_job_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let id_for_db = job_id.clone();
    with_db(state, move |conn| {
        boogiebox_db::jobs::cancel_post_scan_job(conn, &id_for_db)
    })
    .await
    .map(|_| status_response(&job_id, "cancelled"))
    .unwrap_or_else(map_job_error)
}

async fn retry_post_scan_job_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let id_for_db = job_id.clone();
    with_db(state, move |conn| {
        boogiebox_db::jobs::retry_post_scan_job(conn, &id_for_db)
    })
    .await
    .map(|_| status_response(&job_id, "pending"))
    .unwrap_or_else(map_job_error)
}

async fn enqueue_post_scan_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(library_id): Path<String>,
    Json(payload): Json<PostScanRequest>,
) -> impl IntoResponse {
    let ps_state_worker = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        s.db.as_ref().map(|db| crate::post_scan::PostScanState {
            db: db.clone(),
            http_client: s.http_client.clone(),
            db_folder: s.db_folder.clone(),
        })
    };
    let job_type = payload.job_type.unwrap_or_default();
    let library_id_for_db = library_id.clone();
    let job_type_for_db = job_type.clone();
    with_db(state, move |conn| {
        boogiebox_db::jobs::enqueue_post_scan_job(conn, &library_id_for_db, &job_type_for_db, None)
    })
    .await
    .map(|job_id| {
        if let Some(state) = ps_state_worker {
            tokio::spawn(crate::post_scan::run_one_pending_music_post_scan_owned(
                state,
            ));
        }
        #[derive(serde::Serialize)]
        struct Response {
            ok: bool,
            id: boogiebox_db::music::EntityId,
            status: &'static str,
            job_type: String,
            library_id: String,
        }
        (
            StatusCode::OK,
            Json(Response {
                ok: true,
                id: job_id,
                status: "pending",
                job_type,
                library_id,
            }),
        )
            .into_response()
    })
    .unwrap_or_else(map_job_error)
}

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

async fn with_db<F, T>(state: SharedState, f: F) -> Result<T, JobError>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T, JobError> + Send + 'static,
    T: Send + 'static,
{
    let db = get_db(&state).ok_or_else(|| JobError::Db(rusqlite::Error::InvalidQuery))?;
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        f(&conn)
    })
    .await
    .map_err(|_| JobError::Db(rusqlite::Error::InvalidQuery))?
}

fn validate_folders(folders: &[String]) -> Result<(), (StatusCode, String)> {
    if folders.iter().all(|folder| folder.trim().is_empty()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "At least one folder is required".into(),
        ));
    }
    for folder in folders {
        let normalized = folder.trim();
        match fs::metadata(normalized) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("Path is not a directory: {normalized}"),
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("Path does not exist: {normalized}"),
                ));
            }
            Err(err) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("Cannot access path: {normalized} - {err}"),
                ));
            }
        }
    }
    Ok(())
}

fn validation_error_response((status, error): (StatusCode, String)) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error,
            setup_required: None,
        }),
    )
        .into_response()
}

fn setup_required() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

fn forbidden() -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse {
            error: "Forbidden".into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn status_response(id: &str, status: &'static str) -> axum::response::Response {
    #[derive(serde::Serialize)]
    struct Response<'a> {
        ok: bool,
        id: &'a str,
        status: &'static str,
    }
    (
        StatusCode::OK,
        Json(Response {
            ok: true,
            id,
            status,
        }),
    )
        .into_response()
}

fn map_job_error(err: JobError) -> axum::response::Response {
    let status = match err {
        JobError::EmptyLibraryName | JobError::EmptyFolders | JobError::LastFolder => {
            StatusCode::BAD_REQUEST
        }
        JobError::DuplicateLibraryName | JobError::DuplicateFolder => StatusCode::CONFLICT,
        JobError::LibraryNotFound | JobError::FolderNotFound | JobError::JobNotFound => {
            StatusCode::NOT_FOUND
        }
        JobError::ScanJobNotActive
        | JobError::PostScanJobNotActive
        | JobError::PostScanJobNotPending
        | JobError::PostScanJobNotRetryable => StatusCode::NOT_FOUND,
        JobError::UnsupportedPostScanJob(_) | JobError::ScheduleTooFrequent => {
            StatusCode::BAD_REQUEST
        }
        JobError::ScanCancelled => StatusCode::CONFLICT,
        JobError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ErrorResponse {
            error: err.to_string(),
            setup_required: None,
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

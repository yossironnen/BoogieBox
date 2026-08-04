//! Defines Rust API routes for Library Routes server behavior.

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use boogiebox_db::{
    jobs::{CreateLibraryInput, JobError},
    music::coerce_entity_id,
};
use serde::{Deserialize, Serialize};
use std::{fs, net::SocketAddr};

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    pick_folder, DbPool, ErrorResponse, FolderPicker, OkResponse, SharedState,
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
        .route("/api/admin/browse-folder", post(browse_folder_handler))
        .route("/api/admin/fs/browse", get(fs_browse_handler))
        .route("/api/admin/fs/mkdir", post(fs_mkdir_handler))
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
            cancel: s.worker_cancel.clone(),
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
            tokio::spawn(crate::scanner::run_scan_job(state, job_id.clone()));
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
            cancel: s.worker_cancel.clone(),
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

#[derive(Debug, Deserialize)]
struct FsBrowseParams {
    path: Option<String>,
}

#[derive(Serialize)]
struct FsBrowseEntry {
    name: String,
    path: String,
}

#[derive(Serialize)]
struct FsBrowseResponse {
    path: String,
    parent: Option<String>,
    entries: Vec<FsBrowseEntry>,
}

async fn fs_browse_handler(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    auth: Result<AdminUser, (StatusCode, Json<ErrorResponse>)>,
    Query(params): Query<FsBrowseParams>,
) -> impl IntoResponse {
    let setup_mode = state.read().expect("state lock").setup_required;
    if let Some(response) = filesystem_access_denied(auth.is_err(), setup_mode, &addr) {
        return response;
    }
    let requested = params
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // On Windows, an empty/absent path means "show drive list".
    #[cfg(windows)]
    if requested.is_none() {
        let drives: Vec<FsBrowseEntry> = ('A'..='Z')
            .filter_map(|c| {
                let path = format!("{c}:\\");
                if std::path::Path::new(&path).is_dir() {
                    Some(FsBrowseEntry {
                        name: path.clone(),
                        path,
                    })
                } else {
                    None
                }
            })
            .collect();
        return (
            StatusCode::OK,
            Json(FsBrowseResponse {
                path: String::new(),
                parent: None,
                entries: drives,
            }),
        )
            .into_response();
    }

    let dir = match requested {
        Some(p) => std::path::PathBuf::from(p),
        #[cfg(not(windows))]
        None => {
            // Default to / so the browser always has a navigable starting point.
            // Using $HOME is unreliable for system service users (no home dir).
            std::path::PathBuf::from("/")
        }
        #[cfg(windows)]
        None => unreachable!("handled above"),
    };

    if !dir.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Not a directory: {}", dir.display()),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // On Windows, a drive root (e.g. C:\) has no real parent; use "" as the
    // sentinel so the client can navigate back to the drive list.
    #[cfg(windows)]
    let parent: Option<String> = {
        let p = dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && *p != dir.as_path());
        match p {
            Some(p) => Some(p.to_string_lossy().into_owned()),
            // Drive root — parent sentinel tells the client to go back to the drive list.
            None => Some(String::new()),
        }
    };
    #[cfg(not(windows))]
    let parent = dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && *p != dir.as_path())
        .map(|p| p.to_string_lossy().into_owned());

    let mut entries: Vec<FsBrowseEntry> = match std::fs::read_dir(&dir) {
        Ok(iter) => iter
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| {
                // Skip hidden directories (dot-prefixed on Linux) and Windows system junctions.
                let name = e.file_name();
                let name_str = name.to_str().unwrap_or("");
                !name_str.starts_with('.')
            })
            .map(|e| FsBrowseEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path().to_string_lossy().into_owned(),
            })
            .collect(),
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Cannot read directory: {err}"),
                    setup_required: None,
                }),
            )
                .into_response();
        }
    };

    entries.sort_by_key(|a| a.name.to_lowercase());

    (
        StatusCode::OK,
        Json(FsBrowseResponse {
            path: dir.to_string_lossy().into_owned(),
            parent,
            entries,
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsMkdirRequest {
    parent: String,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FsMkdirResponse {
    path: String,
}

async fn fs_mkdir_handler(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    auth: Result<AdminUser, (StatusCode, Json<ErrorResponse>)>,
    Json(payload): Json<FsMkdirRequest>,
) -> impl IntoResponse {
    let setup_mode = state.read().expect("state lock").setup_required;
    if let Some(response) = filesystem_access_denied(auth.is_err(), setup_mode, &addr) {
        return response;
    }
    let parent = payload.parent.trim();
    let name = payload.name.trim();

    if parent.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "parent path is required".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Folder name cannot be empty".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // Reject names with path separators — must be a single component.
    if name.contains('/') || name.contains('\\') {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Folder name cannot contain path separators".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // Reject dot-only names and names with null bytes.
    if name == "." || name == ".." || name.contains('\0') {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid folder name".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // Windows: reject reserved device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9).
    #[cfg(windows)]
    {
        let upper = name.to_uppercase();
        let stem = upper.split('.').next().unwrap_or(&upper);
        let reserved = matches!(
            stem,
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        );
        if reserved {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("'{name}' is a reserved Windows device name"),
                    setup_required: None,
                }),
            )
                .into_response();
        }

        // Reject characters illegal on Windows: \ / : * ? " < > |
        let illegal: &[char] = &[':', '*', '?', '"', '<', '>', '|'];
        if name.chars().any(|c| illegal.contains(&c)) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error:
                        "Folder name contains a character not allowed on Windows (: * ? \" < > |)"
                            .to_string(),
                    setup_required: None,
                }),
            )
                .into_response();
        }

        // Reject trailing dot or space (silently stripped by Windows, causes confusion).
        if name.ends_with('.') || name.ends_with(' ') {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Folder name cannot end with a dot or space on Windows".to_string(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
    }

    let parent_path = std::path::Path::new(parent);
    if !parent_path.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Parent is not a directory: {parent}"),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let new_path = parent_path.join(name);

    if new_path.exists() {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("'{name}' already exists"),
                setup_required: None,
            }),
        )
            .into_response();
    }

    match fs::create_dir(&new_path) {
        Ok(()) => (
            StatusCode::OK,
            Json(FsMkdirResponse {
                path: new_path.to_string_lossy().into_owned(),
            }),
        )
            .into_response(),
        Err(err) => {
            let msg = match err.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    format!("Permission denied: cannot create folder in {parent}")
                }
                std::io::ErrorKind::NotFound => {
                    format!("Parent directory not found: {parent}")
                }
                std::io::ErrorKind::AlreadyExists => {
                    format!("'{name}' already exists")
                }
                _ => format!("Failed to create folder: {err}"),
            };
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: msg,
                    setup_required: None,
                }),
            )
                .into_response()
        }
    }
}

fn filesystem_access_denied(
    unauthenticated: bool,
    setup_mode: bool,
    addr: &SocketAddr,
) -> Option<axum::response::Response> {
    if !unauthenticated {
        return None;
    }
    if !setup_mode {
        return Some(forbidden());
    }
    if !crate::is_loopback_addr(addr) {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "Setup filesystem access is only available from the server machine"
                        .to_string(),
                    setup_required: None,
                }),
            )
                .into_response(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_filesystem_access_allows_loopback_without_auth() {
        let response =
            filesystem_access_denied(true, true, &SocketAddr::from(([127, 0, 0, 1], 4000)));

        assert!(response.is_none());
    }

    #[test]
    fn setup_filesystem_access_rejects_lan_without_auth() {
        let response =
            filesystem_access_denied(true, true, &SocketAddr::from(([192, 168, 1, 20], 4000)))
                .expect("expected forbidden response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn configured_filesystem_access_requires_auth() {
        let response =
            filesystem_access_denied(true, false, &SocketAddr::from(([127, 0, 0, 1], 4000)))
                .expect("expected forbidden response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn authenticated_filesystem_access_is_not_loopback_limited() {
        let response =
            filesystem_access_denied(false, false, &SocketAddr::from(([192, 168, 1, 20], 4000)));

        assert!(response.is_none());
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowseFolderRequest {
    initial_dir: Option<String>,
}

#[derive(Serialize)]
struct BrowseFolderResponse {
    folder: Option<String>,
}

async fn browse_folder_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(payload): Json<BrowseFolderRequest>,
) -> impl IntoResponse {
    let picker = state
        .read()
        .map(|s| s.folder_picker.clone())
        .unwrap_or(FolderPicker::System);
    if !cfg!(windows) {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorResponse {
                error: "Folder picker is not available on Linux — type the path manually"
                    .to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }
    match pick_folder(
        picker,
        payload.initial_dir.as_deref(),
        "Choose a music library folder",
    )
    .await
    {
        Ok(folder) => (StatusCode::OK, Json(BrowseFolderResponse { folder })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Folder picker failed: {err}"),
                setup_required: None,
            }),
        )
            .into_response(),
    }
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

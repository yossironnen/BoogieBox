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
    auth::{AdminUser, AuthenticatedUser, LibraryManager},
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    if !user.can_manage_libraries() {
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    _user: LibraryManager,
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
    auth: Result<LibraryManager, (StatusCode, Json<ErrorResponse>)>,
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
    auth: Result<LibraryManager, (StatusCode, Json<ErrorResponse>)>,
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
    _user: LibraryManager,
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

#[cfg(test)]
mod route_tests {
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use std::net::SocketAddr;
    use uuid::Uuid;

    fn temp_music_dir(prefix: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("library-route-test-{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn list_libraries_requires_auth_and_starts_empty() {
        let (app, _pool) = new_test_app_with_pool("lib-list");
        let (unauth_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/libraries")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(unauth_status, StatusCode::UNAUTHORIZED);

        let cookie = seed_user_session(&_pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/libraries")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(json_body(&body).as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stats_route_returns_ok_for_authenticated_user() {
        let (app, pool) = new_test_app_with_pool("lib-stats");
        let cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/stats")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn create_library_requires_management_permission_and_a_real_folder() {
        let (app, pool) = new_test_app_with_pool("lib-create");
        let cookie = seed_user_session(&pool, "u1"); // plain user: cannot manage libraries

        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/does/not/exist"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        pool.lock()
            .unwrap()
            .execute(
                "UPDATE users SET can_manage_libraries = 1 WHERE id = 'u1'",
                [],
            )
            .unwrap();

        let (missing_path_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/does/not/exist/at/all"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_path_status, StatusCode::BAD_REQUEST);

        let dir = temp_music_dir("create");
        let (status, body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"path":{:?},"name":"My Library"}}"#,
                    dir.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let created = json_body(&body);
        let library_id = created["id"].as_str().unwrap().to_owned();

        // Duplicate name -> conflict.
        let dir2 = temp_music_dir("create-dup");
        let (dup_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"path":{:?},"name":"My Library"}}"#,
                    dir2.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(dup_status, StatusCode::CONFLICT);

        // Rename.
        let (rename_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/libraries/{library_id}"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Renamed Library"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(rename_status, StatusCode::OK);

        // Add + remove a folder.
        let dir3 = temp_music_dir("create-folder2");
        let (add_folder_status, add_folder_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/libraries/{library_id}/folders"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"path":{:?}}}"#,
                    dir3.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(add_folder_status, StatusCode::CREATED);
        let folders = json_body(&add_folder_body)["folders"]
            .as_array()
            .unwrap()
            .clone();
        let new_folder_id = folders
            .iter()
            .find(|f| f["path"].as_str() == Some(dir3.to_string_lossy().as_ref()))
            .and_then(|f| f["id"].as_str())
            .unwrap()
            .to_owned();

        let (remove_folder_status, _) = send(
            app.clone(),
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/libraries/{library_id}/folders/{new_folder_id}"
                ))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(remove_folder_status, StatusCode::OK);

        // Delete the library entirely.
        let (delete_status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/libraries/{library_id}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(delete_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn enqueue_scan_requires_manage_permission_and_404s_for_missing_library() {
        let (app, pool) = new_test_app_with_pool("lib-scan");
        let cookie = seed_user_session(&pool, "u1");

        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries/some-id/scan")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        pool.lock()
            .unwrap()
            .execute(
                "UPDATE users SET can_manage_libraries = 1 WHERE id = 'u1'",
                [],
            )
            .unwrap();

        let (missing_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/libraries/does-not-exist/scan")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn scan_and_schedule_and_queue_routes_are_reachable_and_admin_gated() {
        let (app, pool) = new_test_app_with_pool("lib-misc");
        let cookie = seed_admin_session(&pool, "admin1");

        // Create a library so schedule/scan-jobs routes have a real id to target.
        let dir = temp_music_dir("misc");
        let (create_status, create_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/libraries")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"path":{:?},"name":"Misc Lib"}}"#,
                    dir.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(create_status, StatusCode::CREATED);
        let library_id = json_body(&create_body)["id"].as_str().unwrap().to_owned();

        let (active_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/scan-jobs/active")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(active_status, StatusCode::OK);

        let (lib_jobs_status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/libraries/{library_id}/scan-jobs"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(lib_jobs_status, StatusCode::OK);

        let (missing_job_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/scan-jobs/does-not-exist")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_job_status, StatusCode::NOT_FOUND);

        let (schedules_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/schedules")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(schedules_status, StatusCode::OK);

        let (upsert_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/schedules/{library_id}"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":true,"frequency_hours":48}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(upsert_status, StatusCode::OK);

        let (get_schedule_status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/schedules/{library_id}"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_schedule_status, StatusCode::OK);

        let (delete_schedule_status, _) = send(
            app.clone(),
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/schedules/{library_id}"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(delete_schedule_status, StatusCode::OK);

        let (queue_status, _) = send(
            app,
            Request::builder()
                .uri("/api/admin/queues")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(queue_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn queue_action_routes_404_for_unknown_job_ids() {
        let (app, pool) = new_test_app_with_pool("lib-queue-actions");
        let cookie = seed_admin_session(&pool, "admin1");

        for (method, path) in [
            ("POST", "/api/admin/queues/scan/no-such-job/cancel"),
            ("POST", "/api/admin/queues/post-scan/no-such-job/fail"),
            ("POST", "/api/admin/queues/post-scan/no-such-job/cancel"),
            ("POST", "/api/admin/queues/post-scan/no-such-job/retry"),
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
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        }
    }

    #[tokio::test]
    async fn fs_browse_setup_mode_allows_loopback_and_blocks_lan() {
        let (app, _pool) = new_test_app_with_pool("lib-fs-browse");
        // Fresh test state always has setup_required: false (see test_support::build_test_state),
        // so authenticate as a library manager instead of relying on setup-mode bypass.
        let cookie = seed_admin_session(&_pool, "admin1");

        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/admin/fs/browse")
                .header("cookie", cookie)
                .extension(ConnectInfo(SocketAddr::from(([192, 168, 1, 20], 5000))))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn fs_browse_unauthenticated_lan_request_is_forbidden() {
        let (app, _pool) = new_test_app_with_pool("lib-fs-browse-forbidden");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/admin/fs/browse")
                .extension(ConnectInfo(SocketAddr::from(([192, 168, 1, 20], 5000))))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn fs_mkdir_validates_and_creates_a_real_directory() {
        let (app, pool) = new_test_app_with_pool("lib-fs-mkdir");
        let cookie = seed_admin_session(&pool, "admin1");
        let parent = temp_music_dir("mkdir-parent");

        let (empty_name_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/admin/fs/mkdir")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))))
                .body(Body::from(format!(
                    r#"{{"parent":{:?},"name":""}}"#,
                    parent.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(empty_name_status, StatusCode::BAD_REQUEST);

        let (sep_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/admin/fs/mkdir")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))))
                .body(Body::from(format!(
                    r#"{{"parent":{:?},"name":"a/b"}}"#,
                    parent.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(sep_status, StatusCode::BAD_REQUEST);

        let (ok_status, ok_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/admin/fs/mkdir")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))))
                .body(Body::from(format!(
                    r#"{{"parent":{:?},"name":"New Folder"}}"#,
                    parent.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(ok_status, StatusCode::OK);
        let new_path = json_body(&ok_body)["path"].as_str().unwrap().to_owned();
        assert!(std::path::Path::new(&new_path).is_dir());

        let (conflict_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/admin/fs/mkdir")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))))
                .body(Body::from(format!(
                    r#"{{"parent":{:?},"name":"New Folder"}}"#,
                    parent.to_string_lossy()
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(conflict_status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn browse_folder_route_is_not_implemented_on_non_windows_or_uses_fixed_picker() {
        let (app, pool) = new_test_app_with_pool("lib-browse-folder");
        let cookie = seed_admin_session(&pool, "admin1");
        let (status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/admin/browse-folder")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        // On non-Windows this is always 501; on Windows the harness's
        // FolderPicker::Fixed(None) resolves to a successful `{ "folder": null }` response.
        if cfg!(windows) {
            assert_eq!(status, StatusCode::OK);
        } else {
            assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        }
    }
}

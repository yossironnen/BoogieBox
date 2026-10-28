//! Backup and restore API: admin Settings › Advanced › Backup, and first-run "Restore from a backup".

use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use boogiebox_db::backup::{self as db_backup, BackupReason, PathRemap};
use serde::{Deserialize, Serialize};

use crate::{
    auth::AdminUser,
    backup::{self, BackupFailure, BackupSettings, BackupSummary, RestoreError, RunningJob},
    server_config, SharedState,
};

/// Backup routes.
pub fn backup_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/admin/backups", get(list_handler).post(create_handler))
        .route("/api/admin/backups/settings", put(settings_handler))
        .route("/api/admin/backups/health", get(health_handler))
        .route("/api/admin/backups/inspect", post(inspect_handler))
        .route(
            "/api/admin/backups/restore-from",
            post(restore_from_handler),
        )
        .route("/api/admin/backups/{name}", delete(delete_handler))
        .route("/api/admin/backups/{name}/restore", post(restore_handler))
        .route("/api/system/backups/inspect", post(setup_inspect_handler))
        .route("/api/system/setup-restore", post(setup_restore_handler))
        .with_state(state)
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

fn no_db() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupsResponse {
    folder: String,
    settings: BackupSettings,
    running: Option<RunningJob>,
    last_failure: Option<BackupFailure>,
    next_run: Option<String>,
    stale: bool,
    backups: Vec<BackupSummary>,
}

async fn overview(state: &SharedState) -> Result<BackupsResponse, Response> {
    let (db, paths, hub) = backup::active(state).ok_or_else(no_db)?;
    tokio::task::spawn_blocking(move || {
        let (settings, folder, last_failure, next_run) = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let settings = BackupSettings::load(&conn);
            let next_run = backup::next_run(&conn, &settings);
            (
                settings,
                backup::backup_dir(&conn, &paths),
                backup::last_failure(&conn),
                next_run,
            )
        };
        let entries = db_backup::list_backups(&folder);
        let newest = entries
            .iter()
            .find(|e| {
                matches!(
                    e.manifest.reason,
                    BackupReason::Scheduled | BackupReason::Manual
                )
            })
            .map(|e| e.manifest.created_at.clone());
        let stale = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            backup::is_stale(&conn, &settings, newest.as_deref())
        };
        BackupsResponse {
            folder: folder.to_string_lossy().into_owned(),
            settings,
            running: hub.running(),
            last_failure,
            next_run,
            stale,
            backups: entries.iter().map(BackupSummary::from).collect(),
        }
    })
    .await
    .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Backup listing failed"))
}

async fn list_handler(_admin: AdminUser, State(state): State<SharedState>) -> Response {
    match overview(&state).await {
        Ok(body) => Json(body).into_response(),
        Err(resp) => resp,
    }
}

async fn settings_handler(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Json(settings): Json<BackupSettings>,
) -> Response {
    if let Err(message) = settings.validate() {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let Some((db, _, _)) = backup::active(&state) else {
        return no_db();
    };
    let saved = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        settings.save(&conn)
    })
    .await;
    if !matches!(saved, Ok(Ok(()))) {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not save the backup settings",
        );
    }
    list_handler(_admin, State(state)).await
}

async fn create_handler(_admin: AdminUser, State(state): State<SharedState>) -> Response {
    let Some((db, paths, hub)) = backup::active(&state) else {
        return no_db();
    };
    if !backup::spawn_backup(db, paths, hub, BackupReason::Manual) {
        return error(
            StatusCode::CONFLICT,
            "A backup or restore is already running",
        );
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    /// `ok`, `failed`, `stale`, `off`.
    state: &'static str,
    schedule: backup::Schedule,
    last_backup_at: Option<String>,
    last_failure: Option<BackupFailure>,
}

async fn health_handler(_admin: AdminUser, State(state): State<SharedState>) -> Response {
    match overview(&state).await {
        Ok(o) => {
            let last_backup_at = o
                .backups
                .iter()
                .find(|b| matches!(b.reason, BackupReason::Scheduled | BackupReason::Manual))
                .map(|b| b.created_at.clone());
            let state = if o.last_failure.is_some() {
                "failed"
            } else if o.stale {
                "stale"
            } else if o.settings.schedule == backup::Schedule::Off {
                "off"
            } else {
                "ok"
            };
            Json(HealthResponse {
                state,
                schedule: o.settings.schedule,
                last_backup_at,
                last_failure: o.last_failure,
            })
            .into_response()
        }
        Err(resp) => resp,
    }
}

async fn delete_handler(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Path(name): Path<String>,
) -> Response {
    if !db_backup::is_bundle_name(&name) {
        return error(StatusCode::BAD_REQUEST, "Not a backup name");
    }
    let Some((db, paths, _)) = backup::active(&state) else {
        return no_db();
    };
    let removed = tokio::task::spawn_blocking(move || {
        let folder = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            backup::backup_dir(&conn, &paths)
        };
        db_backup::find_backup(&folder, &name).map(|entry| std::fs::remove_dir_all(entry.path))
    })
    .await;
    match removed {
        Ok(Some(Ok(()))) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "Backup not found"),
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not delete the backup",
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectRequest {
    path: String,
}

async fn inspect_path(path: String) -> Response {
    let path = PathBuf::from(path.trim());
    let result = tokio::task::spawn_blocking(move || {
        backup::inspect(&path).map(|manifest| {
            let size = std::fs::metadata(path.join(db_backup::BUNDLE_DB_FILE))
                .map(|m| m.len())
                .unwrap_or(0);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            backup::summarize(&name, &path, &manifest, size)
        })
    })
    .await;
    match result {
        Ok(Ok(summary)) => Json(summary).into_response(),
        Ok(Err(err)) => error(StatusCode::BAD_REQUEST, err.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the backup",
        ),
    }
}

async fn inspect_handler(_admin: AdminUser, Json(body): Json<InspectRequest>) -> Response {
    inspect_path(body.path).await
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreRequest {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    remaps: Vec<PathRemap>,
}

fn check_remaps(remaps: &[PathRemap]) -> Result<(), Box<Response>> {
    for remap in remaps {
        if remap.from.trim().is_empty() || !FsPath::new(remap.to.trim()).is_dir() {
            return Err(Box::new(error(
                StatusCode::BAD_REQUEST,
                format!(
                    "Choose a folder that exists on this machine for {}",
                    remap.from
                ),
            )));
        }
    }
    Ok(())
}

fn restore_response(result: Result<PathBuf, RestoreError>) -> Response {
    match result {
        Ok(saved) => Json(serde_json::json!({
            "ok": true,
            "savedTo": saved.to_string_lossy(),
        }))
        .into_response(),
        Err(RestoreError::Busy) => error(StatusCode::CONFLICT, RestoreError::Busy.to_string()),
        Err(err @ RestoreError::Invalid(_)) => error(StatusCode::BAD_REQUEST, err.to_string()),
        Err(err @ RestoreError::Pending) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "ok": false, "pending": true, "error": err.to_string() })),
        )
            .into_response(),
        Err(err @ RestoreError::Unavailable(_)) => {
            error(StatusCode::SERVICE_UNAVAILABLE, err.to_string())
        }
        Err(err @ RestoreError::Failed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        }
    }
}

async fn restore_handler(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Path(name): Path<String>,
    body: Option<Json<RestoreRequest>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    if let Err(resp) = check_remaps(&body.remaps) {
        return *resp;
    }
    let Some((db, paths, _)) = backup::active(&state) else {
        return no_db();
    };
    let found = tokio::task::spawn_blocking(move || {
        let folder = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            backup::backup_dir(&conn, &paths)
        };
        db_backup::find_backup(&folder, &name).map(|e| e.path)
    })
    .await
    .ok()
    .flatten();
    let Some(bundle) = found else {
        return error(StatusCode::NOT_FOUND, "Backup not found");
    };
    restore_response(backup::restore_live(&state, bundle, body.remaps).await)
}

async fn restore_from_handler(
    _admin: AdminUser,
    State(state): State<SharedState>,
    Json(body): Json<RestoreRequest>,
) -> Response {
    let Some(path) = body
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "Choose a backup folder");
    };
    if let Err(resp) = check_remaps(&body.remaps) {
        return *resp;
    }
    restore_response(backup::restore_live(&state, PathBuf::from(path), body.remaps).await)
}

fn setup_only(state: &SharedState) -> Result<(), Box<Response>> {
    if state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .setup_required
    {
        Ok(())
    } else {
        Err(Box::new(error(
            StatusCode::FORBIDDEN,
            "BoogieBox is already set up",
        )))
    }
}

async fn setup_inspect_handler(
    State(state): State<SharedState>,
    Json(body): Json<InspectRequest>,
) -> Response {
    if let Err(resp) = setup_only(&state) {
        return *resp;
    }
    inspect_path(body.path).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupRestoreRequest {
    backup_path: String,
    db_folder: String,
    #[serde(default)]
    remaps: Vec<PathRemap>,
}

async fn setup_restore_handler(
    State(state): State<SharedState>,
    Json(body): Json<SetupRestoreRequest>,
) -> Response {
    if let Err(resp) = setup_only(&state) {
        return *resp;
    }
    let db_folder = PathBuf::from(body.db_folder.trim());
    if body.db_folder.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "Choose a database folder");
    }
    let hub = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .backup
        .clone();
    let Some(_guard) = hub.try_begin("restore", "Restoring the backup") else {
        return error(StatusCode::CONFLICT, RestoreError::Busy.to_string());
    };
    let bundle = PathBuf::from(body.backup_path.trim());
    let folder = db_folder.clone();
    let remaps = body.remaps;
    let restored = tokio::task::spawn_blocking(move || {
        backup::restore_into_new_folder(&bundle, &folder, &remaps).and_then(|_| {
            boogiebox_db::init_db(&folder).map_err(|e| RestoreError::Failed(e.to_string()))
        })
    })
    .await;
    let initialized = match restored {
        Ok(Ok(initialized)) => initialized,
        Ok(Err(err)) => return restore_response(Err(err)),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "Restore failed"),
    };
    let db_config_path = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .db_config_path
        .clone();
    if let Err(err) = server_config::write_db_config_to(&db_config_path, &db_folder) {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Setup failed: {err}"),
        );
    }
    let pool = Arc::new(Mutex::new(initialized.connection));
    let mut s = state.write().unwrap_or_else(|p| p.into_inner());
    s.setup_required = false;
    crate::activate_db(&mut s, pool, db_folder);
    Json(serde_json::json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{build_test_state, seed_admin_session, temp_db_path};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn call(
        app: Router,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            req = req.header("cookie", cookie);
        }
        let resp = app
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn admin_backup_lifecycle_and_live_restore() {
        let db_path = temp_db_path("backup-routes");
        let state: SharedState = Arc::new(std::sync::RwLock::new(build_test_state(&db_path)));
        let cookie = {
            let db = state.read().unwrap().db.clone().unwrap();
            seed_admin_session(&db, "admin-1")
        };
        let app = || backup_router(state.clone());

        // Regular users can't see backups.
        let (status, _) = call(
            app(),
            "GET",
            "/api/admin/backups",
            None,
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = call(
            app(),
            "GET",
            "/api/admin/backups",
            Some(&cookie),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["settings"]["schedule"], "weekly");
        assert_eq!(body["settings"]["time"], "03:30");
        assert_eq!(body["backups"].as_array().unwrap().len(), 0);
        assert!(body["nextRun"].as_str().unwrap().ends_with("T03:30"));

        let (status, _) = call(
            app(),
            "PUT",
            "/api/admin/backups/settings",
            Some(&cookie),
            serde_json::json!({"schedule":"daily","day":2,"time":"25:00","keep":4,"path":"","includeAllArtwork":false,"includeMixes":false}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Back up now, then wait for it.
        let (db, paths, hub) = backup::active(&state).unwrap();
        {
            let conn = db.lock().unwrap();
            conn.execute("INSERT INTO playlists (id, name, user_id) SELECT 'p-1', 'Road trip', id FROM users LIMIT 1", [])
                .unwrap();
        }
        let (status, _) = call(
            app(),
            "POST",
            "/api/admin/backups",
            Some(&cookie),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        for _ in 0..100 {
            if hub.running().is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let (_, body) = call(
            app(),
            "GET",
            "/api/admin/backups",
            Some(&cookie),
            serde_json::Value::Null,
        )
        .await;
        let backups = body["backups"].as_array().unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0]["reason"], "manual");
        assert_eq!(backups[0]["counts"]["playlists"], 1);
        let name = backups[0]["name"].as_str().unwrap().to_string();

        // Change the data after the backup, then restore it live.
        {
            let conn = db.lock().unwrap();
            conn.execute("DELETE FROM playlists", []).unwrap();
        }
        drop(db);
        let (status, body) = call(
            app(),
            "POST",
            &format!("/api/admin/backups/{name}/restore"),
            Some(&cookie),
            serde_json::json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let saved = PathBuf::from(body["savedTo"].as_str().unwrap());
        assert!(
            saved.join(boogiebox_db::DB_FILE_NAME).is_file(),
            "previous data saved aside"
        );
        assert!(saved.join(db_backup::MANIFEST_FILE).is_file());

        let (db, _, _) = backup::active(&state).unwrap();
        let (playlists, sessions) = {
            let conn = db.lock().unwrap();
            let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
            (
                count("SELECT COUNT(*) FROM playlists"),
                count("SELECT COUNT(*) FROM sessions"),
            )
        };
        assert_eq!(playlists, 1, "the playlist is back");
        assert_eq!(sessions, 0, "everyone signs in again");
        assert!(!state.read().unwrap().backup.is_restoring());

        // The old sign-in no longer works; deleting needs a valid name.
        let (status, _) = call(
            app(),
            "GET",
            "/api/admin/backups",
            Some(&cookie),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let _ = paths;
    }

    #[tokio::test]
    async fn setup_endpoints_only_work_during_setup() {
        let db_path = temp_db_path("backup-routes-setup");
        let state: SharedState = Arc::new(std::sync::RwLock::new(build_test_state(&db_path)));
        let app = backup_router(state.clone());
        let (status, _) = call(
            app,
            "POST",
            "/api/system/backups/inspect",
            None,
            serde_json::json!({"path": "x"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

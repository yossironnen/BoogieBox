//! Shared test-only helpers for building a real (temp-SQLite-backed) `AppState` and
//! full `axum::Router`, so route-module tests can drive handlers end-to-end with
//! `tower::ServiceExt::oneshot` instead of re-deriving `AppState` construction per file.
//!
//! Only compiled under `#[cfg(test)]` (see the `mod test_support;` declaration in `lib.rs`).

use crate::{auth::LoginAttemptTracker, dlna, ffmpeg, AppState, DbPool, FolderPicker};
use axum::{
    body::{to_bytes, Body},
    http::{Request, Response, StatusCode},
    Router,
};
use boogiebox_db::init_db;
use rusqlite::params;
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tower::ServiceExt;
use uuid::Uuid;

/// Creates a fresh temp directory (unique per call) and returns the `boogiebox.db` path
/// inside it, without creating the database file itself.
pub fn temp_db_path(prefix: &str) -> PathBuf {
    let mut dir = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    dir.push(format!("boogiebox-test-{prefix}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir.join("boogiebox.db")
}

/// Builds a real `AppState` backed by a freshly-initialized temp SQLite database.
/// FFmpeg and DLNA are left in their inert/unavailable state so tests never spawn
/// real subprocesses or open real sockets (see `wip/server-rust-coverage-gap-plan.md`
/// for the accepted-gap rationale).
pub fn build_test_state(db_path: &std::path::Path) -> AppState {
    let folder = db_path.parent().expect("db path has a parent dir");
    let initialized = init_db(folder).expect("init test db");
    let db_pool = Arc::new(Mutex::new(initialized.connection));

    AppState {
        setup_required: false,
        ffmpeg_available: false,
        ffmpeg_path: PathBuf::from("ffmpeg"),
        log_file: None,
        scan_debug_log_file: None,
        deep_debug_log_file: None,
        suggested_db_folder: PathBuf::from("C:\\BoogieBox"),
        db_config_path: PathBuf::from("boogiebox-config.json"),
        folder_picker: FolderPicker::Fixed(None),
        db: Some(db_pool),
        login_attempts: LoginAttemptTracker::default(),
        http_client: reqwest::Client::default(),
        db_folder: Some(folder.to_path_buf()),
        dlna_manager: dlna::new_dlna_manager(),
        worker_cancel: tokio_util::sync::CancellationToken::new(),
    }
}

/// Builds the full application router (every route module merged, matching production
/// wiring in `lib.rs::build_app`) against a fresh temp-SQLite-backed state.
pub fn build_test_app(db_path: &std::path::Path) -> Router {
    let state = build_test_state(db_path);
    crate::build_app(state, None)
}

/// Convenience: builds a fresh temp DB + full app router in one call, keyed by `prefix`
/// so parallel tests never collide on the same temp directory.
pub fn new_test_app(prefix: &str) -> Router {
    build_test_app(&temp_db_path(prefix))
}

/// Like `new_test_app`, but also returns the shared `DbPool` so the caller can seed
/// fixtures (users/sessions/library rows) or call `seed_session` before/between
/// requests, without re-deriving `AppState` construction.
pub fn new_test_app_with_pool(prefix: &str) -> (Router, DbPool) {
    let state = build_test_state(&temp_db_path(prefix));
    let pool = state.db.clone().expect("test state always has a db");
    (crate::build_app(state, None), pool)
}

/// Inserts a user (if not already present) and a live session for them, returning the
/// `cookie` header value to attach to a request so it authenticates as that user —
/// mirrors the pattern already used by `routes::music_routes`'s
/// `similar_artist_route_tests` module (session row directly in the `sessions` table,
/// checked via the `bb_session` cookie extractor in `auth.rs`).
pub fn seed_session(pool: &DbPool, user_id: &str, username: &str, role: &str) -> String {
    let conn = pool.lock().unwrap_or_else(|p| p.into_inner());
    conn.execute(
        "INSERT OR IGNORE INTO users(id, username, role) VALUES (?, ?, ?)",
        params![user_id, username, role],
    )
    .expect("insert test user");
    let token = format!("test-session-{}", Uuid::now_v7());
    conn.execute(
        "INSERT INTO sessions(token, user_id, expires_at) VALUES (?, ?, datetime('now', '+1 day'))",
        params![token, user_id],
    )
    .expect("insert test session");
    format!("bb_session={token}")
}

/// Convenience: `seed_session` for a regular (`user`) role.
pub fn seed_user_session(pool: &DbPool, user_id: &str) -> String {
    seed_session(pool, user_id, user_id, "user")
}

/// Convenience: `seed_session` for an `admin` role, needed by `AdminUser`-gated routes.
pub fn seed_admin_session(pool: &DbPool, user_id: &str) -> String {
    seed_session(pool, user_id, user_id, "admin")
}

/// Confirms `ffmpeg::ffmpeg_available()` isn't accidentally relied upon by callers of
/// `build_test_state` (documents the deliberate `ffmpeg_available: false` choice above).
#[allow(dead_code)]
fn _assert_ffmpeg_helper_still_exists() -> bool {
    ffmpeg::ffmpeg_available()
}

/// Sends a request through `app` and returns the response with its body already collected,
/// so callers can inspect `status()` and parse the body without repeating the `oneshot` +
/// `to_bytes` boilerplate in every test.
pub async fn send(app: Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp: Response<Body> = app.oneshot(req).await.expect("router did not respond");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    (status, body.to_vec())
}

/// Like `send`, but also returns the response headers — for tests that need to
/// assert on `Content-Range`/`Content-Disposition`/etc. rather than just body+status.
pub async fn send_full(
    app: Router,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let resp: Response<Body> = app.oneshot(req).await.expect("router did not respond");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    (status, headers, body.to_vec())
}

/// Parses a `send()` body as JSON, panicking with the raw body text on failure (much more
/// useful for debugging a failing assertion than serde's default error).
pub fn json_body(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).unwrap_or_else(|e| {
        panic!(
            "response body was not valid JSON ({e}): {:?}",
            String::from_utf8_lossy(body)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn build_test_app_serves_system_status() {
        let app = new_test_app("test-support-status");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/system/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["server"], "boogiebox");
    }

    #[tokio::test]
    async fn build_test_state_has_no_real_ffmpeg_and_a_working_db() {
        let db_path = temp_db_path("test-support-state");
        let state = build_test_state(&db_path);
        assert!(!state.ffmpeg_available);
        assert!(state.db.is_some());
    }
}

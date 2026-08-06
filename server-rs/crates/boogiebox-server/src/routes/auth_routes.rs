//! Defines Rust API routes for Auth Routes server behavior.

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use serde::{Deserialize, Serialize};
use time::Duration as TimeDuration;

use crate::{
    auth::{expires_at_string, new_uuid, verify_pin, AuthenticatedUser},
    DbPool, ErrorResponse, OkResponse, SharedState,
};

// -- Request / response shapes -------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest {
    user_id: Option<serde_json::Value>, // accepts string or number for compat
    pin: Option<String>,
    stay_logged_in: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginUser {
    id: String,
    username: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    user: LoginUserFull,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginUserFull {
    id: String,
    username: String,
    role: String,
    can_manage_libraries: bool,
    can_edit_metadata: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    id: String,
    username: String,
    role: String,
    can_manage_libraries: bool,
    can_edit_metadata: bool,
}

// -- Session constants ---------------------------------------------------------

const SESSION_STAY_SECS: u64 = 30 * 24 * 3600; // 30 days
const SESSION_DEFAULT_SECS: u64 = 24 * 3600; // 24 hours

// -- Router --------------------------------------------------------------------

/// Documents the Auth Router public API surface.
pub fn auth_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/auth/users", get(list_users_handler))
        .route("/api/auth/login", axum::routing::post(login_handler))
        .route("/api/auth/logout", axum::routing::post(logout_handler))
        .route("/api/auth/me", get(me_handler))
        .with_state(state)
}

// -- Handlers ------------------------------------------------------------------

async fn list_users_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::list_login_users(&conn)
    })
    .await;

    match result {
        Ok(Ok(users)) => {
            let body: Vec<LoginUser> = users
                .into_iter()
                .map(|u| LoginUser {
                    id: u.id,
                    username: u.username,
                })
                .collect();
            (StatusCode::OK, Json(body)).into_response()
        }
        _ => internal_error(),
    }
}

async fn login_handler(
    State(state): State<SharedState>,
    jar: CookieJar,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required_response(),
    };

    let tracker = {
        state
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .login_attempts
            .clone()
    };

    // Normalize userId to string
    let user_id = match &payload.user_id {
        Some(serde_json::Value::String(s)) => s.trim().to_owned(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid credentials".into(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
    };

    if user_id.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    // Brute-force check
    if tracker.is_locked(&user_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorResponse {
                error: "Too many login attempts. Try again in 60 seconds.".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let pin_input = payload.pin.clone().unwrap_or_default();
    let user_id_clone = user_id.clone();
    let user_result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::get_user_for_auth(&conn, &user_id_clone)
    })
    .await;

    let user = match user_result {
        Ok(Ok(Some(u))) => u,
        Ok(Ok(None)) => {
            tracker.record_failure(&user_id);
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid credentials".into(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
        _ => return internal_error(),
    };

    // PIN verification
    match &user.pin_hash {
        None => {
            // No PIN set - only accept empty PIN input
            if !pin_input.is_empty() {
                tracker.record_failure(&user_id);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "Invalid credentials".into(),
                        setup_required: None,
                    }),
                )
                    .into_response();
            }
        }
        Some(stored_hash) => {
            if pin_input.is_empty() || !verify_pin(stored_hash, &pin_input) {
                tracker.record_failure(&user_id);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrorResponse {
                        error: "Invalid credentials".into(),
                        setup_required: None,
                    }),
                )
                    .into_response();
            }
        }
    }

    // Auth OK - create session
    tracker.clear_attempts(&user_id);
    let stay_logged_in = payload.stay_logged_in.unwrap_or(false);
    let expiry_secs = if stay_logged_in {
        SESSION_STAY_SECS
    } else {
        SESSION_DEFAULT_SECS
    };
    let token = new_uuid();
    let expires_at = expires_at_string(expiry_secs);

    let db2 = get_db(&state).expect("db present after prior check");
    let token_clone = token.clone();
    let user_id_clone = user.id.clone();
    let session_result = tokio::task::spawn_blocking(move || {
        let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
        // Purge expired sessions for this user first
        let _ = boogiebox_db::delete_expired_sessions_for_user(&conn, &user_id_clone);
        boogiebox_db::create_session(&conn, &token_clone, &user_id_clone, &expires_at)
    })
    .await;

    if session_result.is_err() || session_result.unwrap().is_err() {
        return internal_error();
    }

    let max_age = TimeDuration::seconds(expiry_secs as i64);
    let cookie = Cookie::build(("bb_session", token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false) // BoogieBox serves over HTTP on LAN; Secure would block the cookie
        .max_age(max_age)
        .path("/")
        .build();
    let new_jar = jar.add(cookie);

    let response_body = LoginResponse {
        user: LoginUserFull {
            id: user.id,
            username: user.username,
            role: user.role,
            can_manage_libraries: user.can_manage_libraries,
            can_edit_metadata: user.can_edit_metadata,
        },
    };
    let body = (StatusCode::OK, Json(response_body)).into_response();
    (new_jar, body).into_response()
}

async fn logout_handler(State(state): State<SharedState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(token_cookie) = jar.get("bb_session") {
        let token = token_cookie.value().to_owned();
        if let Some(db) = get_db(&state) {
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::delete_session(&conn, &token)
            })
            .await;
        }
    }

    let removal = Cookie::build(Cookie::from("bb_session"))
        .path("/")
        .max_age(TimeDuration::ZERO)
        .build();
    let new_jar = jar.remove(removal);
    let body = (StatusCode::OK, Json(OkResponse { ok: true })).into_response();
    (new_jar, body).into_response()
}

async fn me_handler(user: AuthenticatedUser) -> impl IntoResponse {
    Json(MeResponse {
        id: user.id,
        username: user.username,
        role: user.role,
        can_manage_libraries: user.can_manage_libraries,
        can_edit_metadata: user.can_edit_metadata,
    })
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

// -- Tests ---------------------------------------------------------------------

#[cfg(test)]
pub use test_helpers::build_test_app_with_db;

#[cfg(test)]
mod test_helpers {
    use super::*;
    use crate::{auth::LoginAttemptTracker, AppState, FolderPicker};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex, RwLock},
    };

    /// Documents the Build Test App With DB public API surface.
    pub fn build_test_app_with_db(db_path: &std::path::Path) -> axum::Router {
        use boogiebox_db::init_db;
        let folder = db_path.parent().unwrap();
        let initialized = init_db(folder).expect("test db");
        let db_pool = Arc::new(Mutex::new(initialized.connection));
        let state = AppState {
            setup_required: false,
            ffmpeg_available: false,
            ffmpeg_path: PathBuf::from("ffmpeg"),
            log_file: None,
            suggested_db_folder: PathBuf::from("C:\\BoogieBox"),
            db_config_path: PathBuf::from("boogiebox-config.json"),
            folder_picker: FolderPicker::Fixed(None),
            db: Some(db_pool),
            login_attempts: LoginAttemptTracker::default(),
            http_client: reqwest::Client::default(),
            db_folder: Some(folder.to_path_buf()),
            dlna_manager: crate::dlna::new_dlna_manager(),
            worker_cancel: tokio_util::sync::CancellationToken::new(),
        };
        let shared = Arc::new(RwLock::new(state));
        auth_router(shared)
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::build_test_app_with_db;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::{env, time::SystemTime};
    use tower::ServiceExt;

    fn temp_db_path(prefix: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        p.push(format!("{prefix}-{nanos}"));
        std::fs::create_dir_all(&p).expect("temp dir");
        p.join("boogiebox.db")
    }

    #[tokio::test]
    async fn list_users_returns_seed_admin() {
        let db_path = temp_db_path("auth-list");
        let app = build_test_app_with_db(&db_path);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let users = json.as_array().unwrap();
        assert!(!users.is_empty());
        assert_eq!(users[0]["username"], "admin");
    }

    #[tokio::test]
    async fn login_no_pin_succeeds_for_seed_admin() {
        let db_path = temp_db_path("auth-login");
        let app = build_test_app_with_db(&db_path);

        // Get admin id
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let users: Value = serde_json::from_slice(&body).unwrap();
        let admin_id = users[0]["id"].as_str().unwrap().to_owned();

        let login_body = format!(r#"{{"userId":"{admin_id}"}}"#);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(login_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let cookie_header = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(cookie_header.contains("bb_session"));
    }

    #[tokio::test]
    async fn login_wrong_user_returns_401() {
        let db_path = temp_db_path("auth-wrong");
        let app = build_test_app_with_db(&db_path);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"userId":"nonexistent-id"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_without_session_returns_401() {
        let db_path = temp_db_path("auth-me");
        let app = build_test_app_with_db(&db_path);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

//! Defines Rust API routes for Admin Routes server behavior.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{hash_pin, new_uuid, AdminUser},
    DbPool, ErrorResponse, OkResponse, SharedState,
};

// -- Request / response shapes -------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserEntry {
    id: String,
    username: String,
    role: String,
    has_pin: bool,
    can_manage_libraries: bool,
    can_edit_metadata: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUserRequest {
    username: Option<String>,
    role: Option<String>,
    pin: Option<String>,
    can_manage_libraries: Option<bool>,
    can_edit_metadata: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdatePermissionsRequest {
    can_manage_libraries: Option<bool>,
    can_edit_metadata: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SetPinRequest {
    pin: Option<serde_json::Value>, // null or string
}

// -- Router --------------------------------------------------------------------

/// Documents the Admin Router public API surface.
pub fn admin_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/admin/users",
            get(list_users_handler).post(create_user_handler),
        )
        .route(
            "/api/admin/users/{id}/permissions",
            axum::routing::put(update_permissions_handler),
        )
        .route(
            "/api/admin/users/{id}/pin",
            axum::routing::put(update_pin_handler),
        )
        .route(
            "/api/admin/users/{id}",
            axum::routing::delete(delete_user_handler),
        )
        .with_state(state)
}

// -- Handlers ------------------------------------------------------------------

async fn list_users_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::list_admin_users(&conn)
    })
    .await;

    match result {
        Ok(Ok(users)) => {
            let body: Vec<AdminUserEntry> = users
                .into_iter()
                .map(|u| AdminUserEntry {
                    id: u.id,
                    username: u.username,
                    role: u.role,
                    has_pin: u.has_pin,
                    can_manage_libraries: u.can_manage_libraries,
                    can_edit_metadata: u.can_edit_metadata,
                    created_at: u.created_at,
                })
                .collect();
            (StatusCode::OK, Json(body)).into_response()
        }
        _ => internal_error(),
    }
}

async fn create_user_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let username = match payload
        .username
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        Some(u) => u.to_owned(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "username required".into(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
    };

    let role = payload.role.as_deref().unwrap_or("user").trim().to_owned();
    if role != "admin" && role != "user" {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "role must be admin or user".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let pin_hash = payload
        .pin
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(hash_pin);
    let can_manage_libraries = payload.can_manage_libraries.unwrap_or(false);
    let can_edit_metadata = payload.can_edit_metadata.unwrap_or(false);
    let new_id = new_uuid();
    let new_id_clone = new_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::create_user(
            &conn,
            &new_id_clone,
            &username,
            &role,
            pin_hash.as_deref(),
            can_manage_libraries,
            can_edit_metadata,
        )
        .and_then(|_| boogiebox_db::list_admin_users(&conn))
    })
    .await;

    match result {
        Ok(Ok(users)) => {
            if let Some(created) = users.into_iter().find(|u| u.id == new_id) {
                (
                    StatusCode::CREATED,
                    Json(AdminUserEntry {
                        id: created.id,
                        username: created.username,
                        role: created.role,
                        has_pin: created.has_pin,
                        can_manage_libraries: created.can_manage_libraries,
                        can_edit_metadata: created.can_edit_metadata,
                        created_at: created.created_at,
                    }),
                )
                    .into_response()
            } else {
                internal_error()
            }
        }
        Ok(Err(rusqlite::Error::SqliteFailure(err, _)))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "Username already exists".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
        _ => internal_error(),
    }
}

async fn update_permissions_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(user_id): Path<String>,
    Json(payload): Json<UpdatePermissionsRequest>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        // Fetch current permissions as defaults
        let current = boogiebox_db::get_user_for_auth(&conn, &user_id)?;
        let Some(user) = current else {
            return Ok::<bool, rusqlite::Error>(false);
        };
        let can_manage_libraries = payload
            .can_manage_libraries
            .unwrap_or(user.can_manage_libraries);
        let can_edit = payload.can_edit_metadata.unwrap_or(user.can_edit_metadata);
        boogiebox_db::update_user_permissions(&conn, &user_id, can_manage_libraries, can_edit)
    })
    .await;

    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => not_found_response(),
        _ => internal_error(),
    }
}

async fn update_pin_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(user_id): Path<String>,
    Json(payload): Json<SetPinRequest>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let pin_hash = match &payload.pin {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) if s.is_empty() => None,
        Some(serde_json::Value::String(s)) => Some(hash_pin(s)),
        _ => None,
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::update_user_pin(&conn, &user_id, pin_hash.as_deref())
    })
    .await;

    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => not_found_response(),
        _ => internal_error(),
    }
}

async fn delete_user_handler(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if admin.0.id == user_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Cannot delete yourself".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        // Guard: last admin cannot be deleted
        let admin_count = boogiebox_db::count_admin_users(&conn)?;
        // Check if the target user is admin
        let target = boogiebox_db::get_user_for_auth(&conn, &user_id)?;
        if let Some(ref t) = target {
            if t.role == "admin" && admin_count <= 1 {
                return Ok::<_, rusqlite::Error>(Err("last_admin"));
            }
        }
        if target.is_none() {
            return Ok(Err("not_found"));
        }
        boogiebox_db::delete_user_by_id(&conn, &user_id)?;
        Ok(Ok(()))
    })
    .await;

    match result {
        Ok(Ok(Ok(()))) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(Err("last_admin"))) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Cannot delete the last admin account".into(),
                setup_required: None,
            }),
        )
            .into_response(),
        Ok(Ok(Err("not_found"))) | Ok(Ok(Err(_))) => not_found_response(),
        _ => internal_error(),
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

fn not_found_response() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "User not found".into(),
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

#[cfg(test)]
mod tests {
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

    fn seed_admin_id(pool: &crate::DbPool) -> String {
        pool.lock()
            .unwrap()
            .query_row("SELECT id FROM users WHERE role='admin' LIMIT 1", [], |r| {
                r.get::<_, String>(0)
            })
            .expect("seed admin exists")
    }

    #[tokio::test]
    async fn list_users_requires_admin() {
        let (app, pool) = new_test_app_with_pool("admin-list-forbidden");
        let user_cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/admin/users")
                .header("cookie", user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_users_returns_seed_admin() {
        let (app, pool) = new_test_app_with_pool("admin-list");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/admin/users")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert!(!json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_user_then_list_includes_it_and_duplicate_username_conflicts() {
        let (app, pool) = new_test_app_with_pool("admin-create");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);

        let (status, body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"newbie","role":"user"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let json = json_body(&body);
        assert_eq!(json["username"], "newbie");
        assert_eq!(json["hasPin"], false);

        let (dup_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"newbie","role":"user"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(dup_status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn create_user_rejects_missing_username_and_bad_role() {
        let (app, pool) = new_test_app_with_pool("admin-create-invalid");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);

        let (status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"  "}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (role_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"x","role":"superuser"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(role_status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_permissions_updates_existing_and_404s_for_missing() {
        let (app, pool) = new_test_app_with_pool("admin-permissions");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);

        let (status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/users/{admin_id}/permissions"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"canManageLibraries":true}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (missing_status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri("/api/admin/users/no-such-user/permissions")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"canManageLibraries":true}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_pin_sets_and_clears_pin() {
        let (app, pool) = new_test_app_with_pool("admin-pin");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);

        let (set_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/users/{admin_id}/pin"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"pin":"1234"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(set_status, StatusCode::OK);

        let (clear_status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/users/{admin_id}/pin"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"pin":null}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(clear_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn delete_user_blocks_self_delete_and_404s_for_missing() {
        let (app, pool) = new_test_app_with_pool("admin-delete");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);

        let (self_status, _) = send(
            app.clone(),
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/admin/users/{admin_id}"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(self_status, StatusCode::BAD_REQUEST);

        let (missing_status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri("/api/admin/users/no-such-user")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_user_removes_a_non_admin_user() {
        let (app, pool) = new_test_app_with_pool("admin-delete-other");
        let admin_id = seed_admin_id(&pool);
        let cookie = seed_admin_session(&pool, &admin_id);
        let other_cookie = seed_user_session(&pool, "u-other");
        let _ = other_cookie; // just needed the user row created

        let (status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri("/api/admin/users/u-other")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
}

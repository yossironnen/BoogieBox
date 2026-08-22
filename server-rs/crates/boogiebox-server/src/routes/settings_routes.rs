//! Defines Rust API routes for Settings Routes server behavior.

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    settings::{
        normalize_settings_payload, validate_user_setting_value, ALLOWED_USER_SETTING_KEYS,
        PLAYBACK_SETTINGS_KEYS, USER_SETTING_MAX_VALUE_LEN,
    },
    DbPool, ErrorResponse, OkResponse, SharedState,
};

// -- Router --------------------------------------------------------------------

/// Documents the Settings Router public API surface.
pub fn settings_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/settings",
            get(get_settings_handler).put(put_settings_handler),
        )
        .route("/api/playback-settings", get(playback_settings_handler))
        .route(
            "/api/user/settings",
            get(get_user_settings_handler).put(put_user_settings_handler),
        )
        .with_state(state)
}

// -- Handlers ------------------------------------------------------------------

async fn get_settings_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::get_all_settings(&conn)
    })
    .await;

    match result {
        Ok(Ok(map)) => (StatusCode::OK, Json(map)).into_response(),
        _ => internal_error(),
    }
}

async fn put_settings_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(payload): Json<HashMap<String, Value>>,
) -> impl IntoResponse {
    // Convert JSON values to strings (settings are stored as TEXT)
    let str_map: HashMap<String, String> = payload
        .into_iter()
        .map(|(k, v)| {
            let s = match &v {
                Value::String(s) => s.clone(),
                other => other.to_string().trim_matches('"').to_owned(),
            };
            (k, s)
        })
        .collect();

    let validated = match normalize_settings_payload(&str_map) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e,
                    setup_required: None,
                }),
            )
                .into_response();
        }
    };

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    const DLNA_KEYS: &[&str] = &[
        "dlnaEnabled",
        "dlnaFriendlyName",
        "dlnaPort",
        "dlnaMediaMode",
    ];
    let dlna_changed = validated.keys().any(|k| DLNA_KEYS.contains(&k.as_str()));
    let scan_debug_toggle = validated
        .get("scanDebugLoggingEnabled")
        .map(|v| v == "true");
    let deep_debug_toggle = validated
        .get("deepmixDebugLoggingEnabled")
        .map(|v| v == "true");

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        for (key, value) in &validated {
            boogiebox_db::upsert_setting(&conn, key, value)?;
        }
        Ok::<_, rusqlite::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            if let Some(enabled) = scan_debug_toggle {
                crate::logging::set_scan_debug_enabled(enabled);
            }
            if let Some(enabled) = deep_debug_toggle {
                crate::logging::set_deep_debug_enabled(enabled);
            }
            if dlna_changed {
                let (dlna_manager, dlna_db) = {
                    let s = state.read().unwrap_or_else(|p| p.into_inner());
                    (s.dlna_manager.clone(), s.db.clone())
                };
                if let Some(dlna_db) = dlna_db {
                    tokio::spawn(crate::dlna::restart_dlna(dlna_manager, dlna_db));
                }
            }
            (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
        }
        _ => internal_error(),
    }
}

async fn playback_settings_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::get_settings_by_keys(&conn, PLAYBACK_SETTINGS_KEYS)
    })
    .await;

    match result {
        Ok(Ok(map)) => {
            let mut public = HashMap::new();
            public.insert(
                "transcodeQuality".to_string(),
                map.get("transcodeQuality")
                    .cloned()
                    .unwrap_or_else(|| "low".into()),
            );
            public.insert(
                "replayGainEnabled".to_string(),
                map.get("replayGainEnabled")
                    .cloned()
                    .unwrap_or_else(|| "false".into()),
            );
            public.insert(
                "vinylMode".to_string(),
                map.get("vinylMode")
                    .cloned()
                    .unwrap_or_else(|| "standard".into()),
            );
            // Return boolean presence flag, not the actual key
            let lastfm_configured = map.get("lastfmKey").map(|v| !v.is_empty()).unwrap_or(false);
            public.insert(
                "lastfmConfigured".to_string(),
                lastfm_configured.to_string(),
            );
            (StatusCode::OK, Json(public)).into_response()
        }
        _ => internal_error(),
    }
}

async fn get_user_settings_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::get_user_settings(&conn, &user.id)
    })
    .await;

    match result {
        Ok(Ok(map)) => (StatusCode::OK, Json(map)).into_response(),
        _ => internal_error(),
    }
}

async fn put_user_settings_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<HashMap<String, Value>>,
) -> impl IntoResponse {
    // Validate allowed keys and value length before writing
    for (key, val) in &payload {
        if !ALLOWED_USER_SETTING_KEYS.contains(&key.as_str()) {
            // Silently skip unknown keys (matches Node behavior)
            continue;
        }
        let v_str = match val {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if v_str.len() > USER_SETTING_MAX_VALUE_LEN {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Value too long for key: {key}"),
                    setup_required: None,
                }),
            )
                .into_response();
        }
        if let Err(error) = validate_user_setting_value(key, &v_str) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error,
                    setup_required: None,
                }),
            )
                .into_response();
        }
    }

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let user_id = user.id.clone();
    let payload_clone: Vec<(String, String)> = payload
        .into_iter()
        .filter(|(k, _)| ALLOWED_USER_SETTING_KEYS.contains(&k.as_str()))
        .map(|(k, v)| {
            let s = match &v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k, s)
        })
        .collect();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        for (key, value) in &payload_clone {
            boogiebox_db::upsert_user_setting(&conn, &user_id, key, value)?;
        }
        Ok::<_, rusqlite::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
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

    #[tokio::test]
    async fn get_settings_requires_admin() {
        let (app, pool) = new_test_app_with_pool("settings-admin-only");
        let user_cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/settings")
                .header("cookie", user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_can_get_and_put_settings() {
        let (app, pool) = new_test_app_with_pool("settings-admin");
        let admin_cookie = seed_admin_session(&pool, "admin1");

        let (get_status, body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/settings")
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_status, StatusCode::OK);
        let json = json_body(&body);
        assert!(json.is_object());

        let (put_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri("/api/settings")
                .header("cookie", admin_cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"lastfmKey":"abc123"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(put_status, StatusCode::OK);

        let (get2_status, body2) = send(
            app,
            Request::builder()
                .uri("/api/settings")
                .header("cookie", admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get2_status, StatusCode::OK);
        let json2 = json_body(&body2);
        assert_eq!(json2["lastfmKey"], "abc123");
    }

    #[tokio::test]
    async fn put_settings_rejects_invalid_payload() {
        let (app, pool) = new_test_app_with_pool("settings-invalid");
        let admin_cookie = seed_admin_session(&pool, "admin1");
        let (status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri("/api/settings")
                .header("cookie", admin_cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"dlnaPort":"not-a-number"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn playback_settings_route_has_public_defaults_and_no_auth_required() {
        let (app, _pool) = new_test_app_with_pool("settings-playback");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/playback-settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["transcodeQuality"], "low");
        assert_eq!(json["lastfmConfigured"], "false");
    }

    #[tokio::test]
    async fn user_settings_round_trip_and_reject_bad_values() {
        let (app, pool) = new_test_app_with_pool("settings-user");
        let cookie = seed_user_session(&pool, "u1");

        let (put_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri("/api/user/settings")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"uiThemeMode":"dark","unknownKey":"ignored-not-error"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(put_status, StatusCode::OK);

        let (get_status, body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/user/settings")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["uiThemeMode"], "dark");
        assert!(json.get("unknownKey").is_none());

        let (bad_status, bad_body) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri("/api/user/settings")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"uiThemeMode":"neon"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(bad_status, StatusCode::BAD_REQUEST);
        let bad_json = json_body(&bad_body);
        assert!(bad_json["error"].as_str().unwrap().contains("uiThemeMode"));
    }

    #[tokio::test]
    async fn user_settings_require_authentication() {
        let (app, _pool) = new_test_app_with_pool("settings-user-auth");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/user/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

//! Defines Rust API routes for Settings Routes server behavior.

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    settings::{
        normalize_settings_payload, ALLOWED_USER_SETTING_KEYS, PLAYBACK_SETTINGS_KEYS,
        USER_SETTING_MAX_VALUE_LEN,
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

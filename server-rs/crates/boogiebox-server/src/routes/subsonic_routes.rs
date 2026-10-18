//! Subsonic password management for the BoogieBox UI (cookie auth): each user sets, changes or clears their
//! own Subsonic password; admins can set or clear it for anyone. The `/rest` API itself is `crate::subsonic`.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    subsonic::crypto::{self, PasswordRule},
    ErrorResponse, OkResponse, SharedState,
};

/// Documents the Subsonic Routes Router public API surface.
pub fn subsonic_routes_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/subsonic/credentials", get(get_credentials_handler))
        .route(
            "/api/subsonic/credentials/password",
            put(set_own_password_handler).delete(clear_own_password_handler),
        )
        .route(
            "/api/admin/users/{id}/subsonic-password",
            put(admin_set_password_handler).delete(admin_clear_password_handler),
        )
        .with_state(state)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsResponse {
    enabled: bool,
    has_password: bool,
    updated_at: Option<String>,
    last_used_at: Option<String>,
    last_client: Option<String>,
    server_urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PasswordRequest {
    password: Option<String>,
}

#[derive(Debug, Serialize)]
struct RuleError {
    error: &'static str,
    rule: &'static str,
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_owned(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn rule_error(rule: PasswordRule) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(RuleError {
            error: rule.message(),
            rule: rule.code(),
        }),
    )
        .into_response()
}

fn server_urls(port: u16) -> Vec<String> {
    let port = if port == 0 { crate::DEFAULT_PORT } else { port };
    crate::upnp::discovery::candidate_ipv4_interfaces(None)
        .into_iter()
        .map(|ip| format!("http://{ip}:{port}"))
        .collect()
}

async fn get_credentials_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
) -> Response {
    let (db, subsonic, port) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.subsonic.clone(), s.remote.http_port())
    };
    let Some(db) = db else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured");
    };
    let result = tokio::task::spawn_blocking(move || {
        let enabled = subsonic.settings(&db).enabled;
        let status = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::subsonic::get_credential_status(&conn, &user.id)
        }?;
        Ok::<_, rusqlite::Error>((enabled, status, server_urls(port)))
    })
    .await;
    match result {
        Ok(Ok((enabled, status, urls))) => Json(CredentialsResponse {
            enabled,
            has_password: status.has_password,
            updated_at: status.updated_at,
            last_used_at: status.last_used_at,
            last_client: status.last_client,
            server_urls: urls,
        })
        .into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

/// Validates, encrypts and stores `password` for `target_id`. PBKDF2 (the "not your PIN" check) runs
/// without holding the database lock.
async fn set_password(
    state: &SharedState,
    target_id: String,
    actor_id: String,
    password: String,
) -> Response {
    if let Err(rule) = crypto::check_password_shape(&password) {
        return rule_error(rule);
    }
    let (db, db_folder, subsonic) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.db_folder.clone(), s.subsonic.clone())
    };
    let Some(db) = db else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured");
    };
    enum Outcome {
        Saved,
        NotFound,
        Rule(PasswordRule),
        NoKey,
    }
    let result = tokio::task::spawn_blocking(move || -> rusqlite::Result<Outcome> {
        let target = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::get_user_for_auth(&conn, &target_id)?
        };
        let Some(target) = target else {
            return Ok(Outcome::NotFound);
        };
        if let Err(rule) = crypto::check_password(&password, target.pin_hash.as_deref()) {
            return Ok(Outcome::Rule(rule));
        }
        let Some(key) = subsonic.key(db_folder.as_deref(), &db) else {
            return Ok(Outcome::NoKey);
        };
        let Some((ciphertext, nonce)) = crypto::encrypt(&key, &target.id, &password) else {
            return Ok(Outcome::NoKey);
        };
        let saved = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::subsonic::set_subsonic_secret(
                &conn,
                &target.id,
                &ciphertext,
                &nonce,
                &actor_id,
            )?
        };
        subsonic.invalidate_accounts();
        Ok(if saved {
            Outcome::Saved
        } else {
            Outcome::NotFound
        })
    })
    .await;
    match result {
        Ok(Ok(Outcome::Saved)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(Outcome::NotFound)) => error(StatusCode::NOT_FOUND, "User not found"),
        Ok(Ok(Outcome::Rule(rule))) => rule_error(rule),
        Ok(Ok(Outcome::NoKey)) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The Subsonic key file could not be created in the data folder",
        ),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

async fn clear_password(state: &SharedState, target_id: String, actor_id: String) -> Response {
    let (db, subsonic) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.subsonic.clone())
    };
    let Some(db) = db else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured");
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::subsonic::clear_subsonic_secret(&conn, &target_id, &actor_id)
    })
    .await;
    subsonic.invalidate_accounts();
    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "User not found"),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

async fn set_own_password_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<PasswordRequest>,
) -> Response {
    let password = payload.password.unwrap_or_default();
    set_password(&state, user.id.clone(), user.id, password).await
}

async fn clear_own_password_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
) -> Response {
    clear_password(&state, user.id.clone(), user.id).await
}

async fn admin_set_password_handler(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(user_id): Path<String>,
    Json(payload): Json<PasswordRequest>,
) -> Response {
    let password = payload.password.unwrap_or_default();
    set_password(&state, user_id, admin.0.id, password).await
}

async fn admin_clear_password_handler(
    State(state): State<SharedState>,
    admin: AdminUser,
    Path(user_id): Path<String>,
) -> Response {
    clear_password(&state, user_id, admin.0.id).await
}

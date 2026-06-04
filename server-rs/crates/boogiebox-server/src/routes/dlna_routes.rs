//! Defines Rust API routes for Dlna Routes server behavior.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

use crate::{dlna, ErrorResponse, OkResponse, SharedState};

/// Documents the DLNA Router public API surface.
pub fn dlna_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/dlna/status", get(dlna_status_handler))
        .route("/api/dlna/restart", post(dlna_restart_handler))
        .with_state(state)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DlnaStatusResponse {
    running: bool,
    port: Option<u16>,
    friendly_name: Option<String>,
}

async fn dlna_status_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let state_r = state.read().unwrap_or_else(|p| p.into_inner());
    let mgr = state_r.dlna_manager.lock().expect("dlna lock");
    (
        StatusCode::OK,
        Json(DlnaStatusResponse {
            running: mgr.running,
            port: mgr.port,
            friendly_name: mgr.friendly_name.clone(),
        }),
    )
}

async fn dlna_restart_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let (db, dlna_manager) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.dlna_manager.clone())
    };
    match db {
        Some(db) => {
            dlna::restart_dlna(dlna_manager, db).await;
            (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Database not configured".into(),
                setup_required: Some(true),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::sync::{Arc, RwLock};
    use tower::ServiceExt;

    #[tokio::test]
    async fn dlna_status_uses_client_api_prefix() {
        let state = Arc::new(RwLock::new(AppState::default()));
        let response = dlna_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/dlna/status")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("status request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json["running"], false);
    }
}

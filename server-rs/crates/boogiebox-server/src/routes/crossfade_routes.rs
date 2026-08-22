//! Defines Rust API routes for Crossfade Routes server behavior.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;

use crate::{auth::AuthenticatedUser, DbPool, ErrorResponse, OkResponse, SharedState};

// -- Router --------------------------------------------------------------------

/// Documents the Crossfade Router public API surface.
pub fn crossfade_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/crossfade/config", get(crossfade_config_handler))
        .route(
            "/api/crossfade/overrides",
            get(get_crossfade_overrides_handler).put(put_crossfade_override_handler),
        )
        .route(
            "/api/crossfade/overrides/{entity_type}/{entity_id}",
            delete(delete_crossfade_override_handler),
        )
        .with_state(state)
}

// -- Query / body params -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CrossfadeConfigParams {
    entity_type: Option<String>,
    entity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossfadeOverridesParams {
    entity_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpsertCrossfadeBody {
    entity_type: Option<String>,
    entity_id: Option<String>,
    mode: Option<String>,
    duration: Option<serde_json::Value>,
}

// -- Handlers ------------------------------------------------------------------

async fn crossfade_config_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<CrossfadeConfigParams>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let entity_type = params.entity_type.clone();
    let entity_id = params.entity_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_crossfade_config(
            &conn,
            entity_type.as_deref(),
            entity_id.as_deref(),
        )
    })
    .await;

    match result {
        Ok(Ok(config)) => (StatusCode::OK, Json(config)).into_response(),
        _ => internal_error(),
    }
}

async fn get_crossfade_overrides_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<CrossfadeOverridesParams>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let entity_type = params.entity_type.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::get_crossfade_overrides(&conn, entity_type.as_deref())
    })
    .await;

    match result {
        Ok(Ok(overrides)) => (StatusCode::OK, Json(overrides)).into_response(),
        _ => internal_error(),
    }
}

async fn put_crossfade_override_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Json(body): Json<UpsertCrossfadeBody>,
) -> impl IntoResponse {
    let entity_type = body.entity_type.unwrap_or_default();
    let entity_id = body.entity_id.unwrap_or_default();
    let mode = body.mode.unwrap_or_default();
    let duration: i64 = match &body.duration {
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    };

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::upsert_crossfade_override(
            &conn,
            &entity_type,
            &entity_id,
            &mode,
            duration,
        )
    })
    .await;

    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
                setup_required: None,
            }),
        )
            .into_response(),
        _ => internal_error(),
    }
}

async fn delete_crossfade_override_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path((entity_type, entity_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::playback::delete_crossfade_override(&conn, &entity_type, &entity_id)
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
    use crate::test_support::{json_body, new_test_app_with_pool, seed_user_session, send};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

    #[tokio::test]
    async fn config_route_requires_authentication() {
        let (app, _pool) = new_test_app_with_pool("crossfade-auth");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/crossfade/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn config_route_returns_global_default_when_no_override() {
        let (app, pool) = new_test_app_with_pool("crossfade-global");
        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/crossfade/config")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["mode"], "off");
        assert_eq!(json["source"], "global");
    }

    #[tokio::test]
    async fn put_then_get_override_round_trips() {
        let (app, pool) = new_test_app_with_pool("crossfade-put");
        let cookie = seed_user_session(&pool, "u1");

        let (put_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri("/api/crossfade/overrides")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"entity_type":"album","entity_id":"a1","mode":"crossfade","duration":5}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(put_status, StatusCode::OK);

        let (get_status, body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/crossfade/config?entity_type=album&entity_id=a1")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["mode"], "crossfade");
        assert_eq!(json["duration"], 5);
        assert_eq!(json["source"], "override");

        let (list_status, list_body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/crossfade/overrides?entity_type=album")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(list_status, StatusCode::OK);
        let list_json = json_body(&list_body);
        assert_eq!(list_json.as_array().unwrap().len(), 1);

        let (del_status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri("/api/crossfade/overrides/album/a1")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(del_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn put_override_with_invalid_mode_returns_bad_request() {
        let (app, pool) = new_test_app_with_pool("crossfade-invalid");
        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri("/api/crossfade/overrides")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"entity_type":"album","entity_id":"a1","mode":"bogus","duration":5}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let json = json_body(&body);
        assert!(json["error"].as_str().unwrap().contains("mode must be"));
    }

    #[tokio::test]
    async fn put_override_with_missing_body_fields_defaults_and_rejects_invalid_entity_type() {
        let (app, pool) = new_test_app_with_pool("crossfade-defaults");
        let cookie = seed_user_session(&pool, "u1");
        // No entity_type at all in the body -> defaults to "" which is not a valid entity type.
        let (status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri("/api/crossfade/overrides")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

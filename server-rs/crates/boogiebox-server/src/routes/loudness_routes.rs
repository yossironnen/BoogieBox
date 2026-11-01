//! Loudness levelling data (wip/loudness-levelling-plan.md §6.2).
//!
//! - `POST /api/tracks/loudness` `{ ids }`: gains and peaks for up to 200 tracks or `boogiemix:<id>`
//!   mixes; ids without data are left out.
//! - `GET /api/admin/loudness/status`: how much of the library has values, and the lane's state.

use std::collections::HashMap;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use boogiebox_db::loudness::{self, TrackLoudness};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    loudness_analysis, ErrorResponse, SharedState,
};

/// Most ids one request may ask for.
pub const MAX_IDS: usize = 200;
const MIX_PREFIX: &str = "boogiemix:";

pub fn loudness_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/tracks/loudness", post(tracks_handler))
        .route("/api/admin/loudness/status", get(status_handler))
        .with_state(state)
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

fn db(state: &SharedState) -> Option<crate::DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

#[derive(Deserialize)]
struct TracksBody {
    ids: Vec<String>,
}

async fn tracks_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Json(body): Json<TracksBody>,
) -> Response {
    if body.ids.len() > MAX_IDS {
        return error(StatusCode::BAD_REQUEST, "Too many ids (at most 200)");
    }
    let Some(db) = db(&state) else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Database not ready");
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let (mixes, tracks): (Vec<String>, Vec<String>) = body
            .ids
            .into_iter()
            .partition(|id| id.starts_with(MIX_PREFIX));
        let mut found: HashMap<String, TrackLoudness> =
            loudness::loudness_for_tracks(&conn, &tracks)?;
        for id in mixes {
            if let Some(mix) = loudness::mix_loudness(&conn, &id[MIX_PREFIX.len()..])? {
                found.insert(id, mix);
            }
        }
        Ok::<_, rusqlite::Error>(found)
    })
    .await;
    match result {
        Ok(Ok(found)) => Json(found).into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Could not read loudness"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusBody {
    #[serde(flatten)]
    counts: loudness::LoudnessStatus,
    failed: usize,
    enabled: bool,
    running: bool,
}

async fn status_handler(State(state): State<SharedState>, _admin: AdminUser) -> Response {
    let Some(db) = db(&state) else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Database not ready");
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        Ok::<_, rusqlite::Error>((
            loudness::loudness_status(&conn)?,
            loudness_analysis::is_enabled(&conn),
        ))
    })
    .await;
    match result {
        Ok(Ok((counts, enabled))) => Json(StatusBody {
            counts,
            failed: loudness_analysis::failed_count(),
            enabled,
            running: loudness_analysis::is_running(),
        })
        .into_response(),
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read loudness status",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use axum::{body::Body, http::Request};

    fn request(
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: serde_json::Value,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            builder = builder.header("cookie", cookie);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn returns_known_tracks_and_mixes_and_limits_the_request() {
        let (app, pool) = new_test_app_with_pool("loudness-tracks");
        let user = seed_user_session(&pool, "u1");
        pool.lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
                 INSERT INTO tracks(id, library_id, title, file_path, duration)
                   VALUES ('t1', 'lib1', 'Loud', '/m/1.flac', 240), ('t2', 'lib1', 'New', '/m/2.flac', 200);
                 INSERT INTO track_loudness(track_id, track_gain_db, track_peak, source, file_size, analyzed_at)
                   VALUES ('t1', -7.5, 0.9, 'tag', 1, 'x');",
            )
            .unwrap();
        let ids = serde_json::json!({ "ids": ["t1", "t2", "nope", "boogiemix:none"] });
        let (status, body) = send(
            app.clone(),
            request("POST", "/api/tracks/loudness", Some(&user), ids),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["t1"]["trackGainDb"], -7.5);
        assert_eq!(body["t1"]["trackPeak"], 0.9);
        assert_eq!(body.as_object().unwrap().len(), 1);

        let many: Vec<String> = (0..201).map(|i| i.to_string()).collect();
        let (status, _) = send(
            app.clone(),
            request(
                "POST",
                "/api/tracks/loudness",
                Some(&user),
                serde_json::json!({ "ids": many }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = send(
            app,
            request(
                "POST",
                "/api/tracks/loudness",
                None,
                serde_json::json!({ "ids": [] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn status_is_admin_only() {
        let (app, pool) = new_test_app_with_pool("loudness-status");
        let admin = seed_admin_session(&pool, "a1");
        let user = seed_user_session(&pool, "u1");
        let get = |cookie: &str| {
            Request::builder()
                .uri("/api/admin/loudness/status")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap()
        };
        let (status, body) = send(app.clone(), get(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["enabled"], true);
        assert_eq!(body["remaining"], 0);
        assert!(body["failed"].is_number());
        let (status, _) = send(app, get(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

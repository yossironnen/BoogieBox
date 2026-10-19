//! Favourites (hearts) in the BoogieBox UI. They are the same per-user stars Subsonic apps set and read.
//! See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-4).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use boogiebox_db::favourites as fdb;
use serde::Deserialize;

use crate::{auth::AuthenticatedUser, DbPool, ErrorResponse, OkResponse, SharedState};

/// Documents the Favourites Router public API surface.
pub fn favourites_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/favourites", get(list_handler))
        .route("/api/favourites/ids", get(ids_handler))
        .route(
            "/api/favourites/{kind}/{id}",
            put(add_handler).delete(remove_handler),
        )
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

fn db_of(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn not_ready() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

async fn ids_handler(State(state): State<SharedState>, user: AuthenticatedUser) -> Response {
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        fdb::favourite_ids(&conn, &user.id)
    })
    .await;
    match result {
        Ok(Ok(ids)) => Json(ids).into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<i64>,
}

async fn list_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(query): Query<ListQuery>,
) -> Response {
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    let limit = query.limit.unwrap_or(fdb::MAX_FAVOURITES);
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        fdb::list_favourites(&conn, &user.id, limit)
    })
    .await;
    match result {
        Ok(Ok(favourites)) => Json(favourites).into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

async fn set(
    state: SharedState,
    user: AuthenticatedUser,
    kind: String,
    id: String,
    on: bool,
) -> Response {
    let Some(kind) = fdb::parse_kind(&kind) else {
        return error(StatusCode::NOT_FOUND, "Unknown favourite type");
    };
    if !valid_id(&id) {
        return error(StatusCode::BAD_REQUEST, "Invalid id");
    }
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        fdb::set_favourite(&conn, &user.id, kind, &id, on)
    })
    .await;
    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "Not found"),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

async fn add_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    set(state, user, kind, id, true).await
}

async fn remove_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path((kind, id)): Path<(String, String)>,
) -> Response {
    set(state, user, kind, id, false).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{json_body, new_test_app_with_pool, seed_user_session, send};
    use axum::{body::Body, http::Request};

    fn req(method: &str, uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn hearts_round_trip_per_user() {
        let (app, pool) = new_test_app_with_pool("favourites-routes");
        let alice = seed_user_session(&pool, "u1");
        let bob = seed_user_session(&pool, "u2");
        {
            let conn = pool.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
                 INSERT INTO artists(id, name) VALUES ('ar1', 'The Band');
                 INSERT INTO albums(id, title, artist_id) VALUES ('al1', 'First', 'ar1');
                 INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) VALUES
                   ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac');",
            )
            .unwrap();
        }
        for uri in [
            "/api/favourites/track/t1",
            "/api/favourites/album/al1",
            "/api/favourites/artist/ar1",
        ] {
            assert_eq!(
                send(app.clone(), req("PUT", uri, &alice)).await.0,
                StatusCode::OK,
                "{uri}"
            );
        }
        assert_eq!(
            send(
                app.clone(),
                req("PUT", "/api/favourites/track/missing", &alice)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(
                app.clone(),
                req("PUT", "/api/favourites/playlist/p1", &alice)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(
                app.clone(),
                req("PUT", "/api/favourites/track/bad%20id", &alice)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );

        let (status, body) = send(app.clone(), req("GET", "/api/favourites/ids", &alice)).await;
        assert_eq!(status, StatusCode::OK);
        let ids = json_body(&body);
        assert_eq!(ids["tracks"], serde_json::json!(["t1"]));
        assert_eq!(ids["albums"], serde_json::json!(["al1"]));
        assert_eq!(ids["artists"], serde_json::json!(["ar1"]));

        let (_, body) = send(app.clone(), req("GET", "/api/favourites?limit=10", &alice)).await;
        let full = json_body(&body);
        assert_eq!(full["tracks"][0]["title"], "Opening");
        assert_eq!(full["albums"][0]["title"], "First");
        assert_eq!(full["artists"][0]["name"], "The Band");

        let (_, body) = send(app.clone(), req("GET", "/api/favourites/ids", &bob)).await;
        assert_eq!(json_body(&body)["tracks"], serde_json::json!([]));

        assert_eq!(
            send(
                app.clone(),
                req("DELETE", "/api/favourites/track/t1", &alice)
            )
            .await
            .0,
            StatusCode::OK
        );
        let (_, body) = send(app.clone(), req("GET", "/api/favourites/ids", &alice)).await;
        assert_eq!(json_body(&body)["tracks"], serde_json::json!([]));
    }
}

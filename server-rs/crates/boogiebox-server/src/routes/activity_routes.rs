//! Admin Activity page (wip/activity-dashboard-plan.md): the live snapshot, its SSE feed, the web player's
//! heartbeat and an admin cancel for any user's BoogieMix job.
//!
//! - `GET /api/activity` (admin): [`activity::ActivitySnapshot`].
//! - `GET /api/activity/events` (admin): SSE `snapshot` events, sent when something changed, at most once a
//!   second; job tables are re-read every 2 s while connected.
//! - `POST /api/activity/heartbeat` (any signed-in user): the web player's position, pause and stop.
//! - `POST /api/activity/mix-jobs/{jobId}/cancel` (admin): cancel a queued or analysing/planning mix.

use std::{
    convert::Infallible,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use tokio::sync::{broadcast, watch};

use crate::{
    activity::{self, Heartbeat},
    auth::{AdminUser, AuthenticatedUser},
    remote::RemoteEvent,
    ErrorResponse, OkResponse, SharedState,
};

const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// Fewest milliseconds between two SSE snapshots.
const MIN_SEND_GAP: Duration = Duration::from_secs(1);
/// Job tables and estimated positions are re-checked this often while a page is open.
const POLL_EVERY: Duration = Duration::from_secs(2);

pub fn activity_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/activity", get(snapshot_handler))
        .route("/api/activity/events", get(events_handler))
        .route("/api/activity/heartbeat", post(heartbeat_handler))
        .route(
            "/api/activity/mix-jobs/{jobId}/cancel",
            post(cancel_mix_job_handler),
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

fn not_ready() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

async fn snapshot_handler(State(state): State<SharedState>, _admin: AdminUser) -> Response {
    match activity::snapshot(&state).await {
        Some(snap) => Json(snap).into_response(),
        None => not_ready(),
    }
}

async fn events_handler(State(state): State<SharedState>, _admin: AdminUser) -> Response {
    let (hub, remote, ready) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.activity.clone(), s.remote.clone(), s.db.is_some())
    };
    if !ready {
        return not_ready();
    }
    let feed = Feed {
        state,
        hub_rx: hub.subscribe(),
        remote_rx: remote.subscribe(),
        last_json: None,
        last_sent: None,
    };
    Sse::new(event_stream(feed))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
        .into_response()
}

struct Feed {
    state: SharedState,
    hub_rx: watch::Receiver<u64>,
    remote_rx: broadcast::Receiver<RemoteEvent>,
    /// Last snapshot sent, with its timestamp zeroed (to spot real changes).
    last_json: Option<String>,
    last_sent: Option<Instant>,
}

impl Feed {
    /// Waits for a change, a remote event or the poll interval.
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.hub_rx.changed() => {}
            r = self.remote_rx.recv() => {
                if matches!(r, Err(broadcast::error::RecvError::Closed)) {
                    tokio::time::sleep(POLL_EVERY).await;
                }
            }
            _ = tokio::time::sleep(POLL_EVERY) => {}
        }
    }

    /// The next snapshot that differs from the last one sent; `None` when the database went away.
    async fn next(&mut self) -> Option<Event> {
        loop {
            if self.last_json.is_some() {
                self.wait().await;
            }
            if let Some(gap) = self
                .last_sent
                .map(|t| MIN_SEND_GAP.saturating_sub(t.elapsed()))
                .filter(|g| !g.is_zero())
            {
                tokio::time::sleep(gap).await;
            }
            let mut snap = activity::snapshot(&self.state).await?;
            let generated = snap.generated_at_ms;
            snap.generated_at_ms = 0;
            let json = serde_json::to_string(&snap).unwrap_or_default();
            if self.last_json.as_deref() == Some(json.as_str()) {
                continue;
            }
            self.last_json = Some(json);
            self.last_sent = Some(Instant::now());
            snap.generated_at_ms = generated;
            let data = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".into());
            return Some(Event::default().event("snapshot").data(data));
        }
    }
}

fn event_stream(feed: Feed) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(feed, |mut feed| async move {
        let event = feed.next().await?;
        Some((Ok(event), feed))
    })
}

async fn heartbeat_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(beat): Json<Heartbeat>,
) -> Response {
    let track_ok = beat
        .track_id
        .as_deref()
        .is_none_or(|t| t.is_empty() || valid_id(t));
    if !valid_id(&beat.player_id) || !track_ok || !beat.position_sec.is_finite() {
        return error(StatusCode::BAD_REQUEST, "Invalid heartbeat");
    }
    activity::hub_of(&state).heartbeat(&user.id, &user.username, beat);
    StatusCode::NO_CONTENT.into_response()
}

async fn cancel_mix_job_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(job_id): Path<String>,
) -> Response {
    if !valid_id(&job_id) {
        return error(StatusCode::NOT_FOUND, "Job not found");
    }
    let Some(db) = state.read().unwrap_or_else(|p| p.into_inner()).db.clone() else {
        return not_ready();
    };
    let job = boogiebox_db::music::coerce_entity_id(&job_id);
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::boogiemix::cancel_mix_job_as_admin(&conn, &job)
    })
    .await;
    match result {
        Ok(Ok(true)) => {
            activity::hub_of(&state).touch();
            (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
        }
        Ok(Ok(false)) => error(
            StatusCode::CONFLICT,
            "Job cannot be canceled in current state",
        ),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use tower::ServiceExt;

    fn get(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    fn post_json(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn endpoints_are_admin_only() {
        let (app, pool) = new_test_app_with_pool("activity-auth");
        let user = seed_user_session(&pool, "u1");
        let admin = seed_admin_session(&pool, "a1");
        for uri in ["/api/activity", "/api/activity/events"] {
            let anonymous = Request::builder().uri(uri).body(Body::empty()).unwrap();
            assert_eq!(
                send(app.clone(), anonymous).await.0,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                send(app.clone(), get(uri, &user)).await.0,
                StatusCode::FORBIDDEN
            );
        }
        let cancel = post_json("/api/activity/mix-jobs/j1/cancel", &user, json!({}));
        assert_eq!(send(app.clone(), cancel).await.0, StatusCode::FORBIDDEN);

        let (status, body) = send(app.clone(), get("/api/activity", &admin)).await;
        assert_eq!(status, StatusCode::OK);
        let snap = json_body(&body);
        assert_eq!(snap["totals"]["playing"], 0);
        assert!(snap["playing"].as_array().unwrap().is_empty());
        assert!(snap["serverJobs"]["scans"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn heartbeats_show_web_playback_and_play_history_appears_in_recent() {
        let (app, pool) = new_test_app_with_pool("activity-heartbeat");
        let user = seed_user_session(&pool, "u1");
        let admin = seed_admin_session(&pool, "a1");
        pool.lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
                 INSERT INTO tracks(id, library_id, title, file_path, duration)
                   VALUES ('t1', 'lib1', 'Opening', '/m/1.flac', 240);",
            )
            .unwrap();
        let beat = json!({ "playerId": "tab-1", "trackId": "t1", "positionSec": 42.5,
                           "durationSec": 240, "paused": false });
        let (status, _) = send(
            app.clone(),
            post_json("/api/activity/heartbeat", &user, beat),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let bad = json!({ "playerId": "../x", "trackId": "t1" });
        assert_eq!(
            send(
                app.clone(),
                post_json("/api/activity/heartbeat", &user, bad)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        // `/played` records a web play for history.
        let played = Request::builder()
            .method("POST")
            .uri("/api/tracks/t1/played")
            .header("cookie", &user)
            .body(Body::empty())
            .unwrap();
        assert!(send(app.clone(), played).await.0.is_success());

        let snap = json_body(&send(app.clone(), get("/api/activity", &admin)).await.1);
        let row = &snap["playing"][0];
        assert_eq!(row["source"], "web");
        assert_eq!(row["user"], "u1");
        assert_eq!(row["title"], "Opening");
        assert_eq!(row["positionSec"], 42.5);
        assert_eq!(row["estimated"], false);
        assert_eq!(
            snap["playing"].as_array().unwrap().len(),
            1,
            "heartbeat wins over the notice"
        );
        let recent = snap["recent"].as_array().unwrap();
        assert_eq!(recent[0]["kind"], "play");
        assert_eq!(recent[0]["source"], "web");
        assert_eq!(recent[0]["title"], "Opening");

        let stop = json!({ "playerId": "tab-1", "trackId": "t1", "stopped": true });
        send(
            app.clone(),
            post_json("/api/activity/heartbeat", &user, stop),
        )
        .await;
        let snap = json_body(&send(app.clone(), get("/api/activity", &admin)).await.1);
        assert_eq!(
            snap["playing"][0]["estimated"], true,
            "the play notice remains, now estimated"
        );
    }

    #[tokio::test]
    async fn admin_can_cancel_any_queued_mix_job() {
        let (app, pool) = new_test_app_with_pool("activity-cancel");
        let admin = seed_admin_session(&pool, "a1");
        seed_user_session(&pool, "u1");
        pool.lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO playlists(id, name, user_id) VALUES ('p1', 'Party', 'u1');
                 INSERT INTO mix_jobs(id, playlist_id, user_id, status) VALUES
                   ('j1', 'p1', 'u1', 'pending'), ('j2', 'p1', 'u1', 'rendering');",
            )
            .unwrap();
        let snap = json_body(&send(app.clone(), get("/api/activity", &admin)).await.1);
        assert_eq!(snap["mixJobs"].as_array().unwrap().len(), 2);
        assert_eq!(snap["totals"]["mixJobs"], 2);

        let cancel = |id: &str| {
            post_json(
                &format!("/api/activity/mix-jobs/{id}/cancel"),
                &admin,
                json!({}),
            )
        };
        assert_eq!(send(app.clone(), cancel("j1")).await.0, StatusCode::OK);
        assert_eq!(
            send(app.clone(), cancel("j2")).await.0,
            StatusCode::CONFLICT
        );
        let status: String = pool
            .lock()
            .unwrap()
            .query_row("SELECT status FROM mix_jobs WHERE id = 'j1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "canceled");
    }

    #[tokio::test]
    async fn an_open_web_stream_is_listed_until_its_body_is_dropped() {
        let (app, pool) = new_test_app_with_pool("activity-stream");
        let user = seed_user_session(&pool, "u1");
        let admin = seed_admin_session(&pool, "a1");
        let dir = std::env::temp_dir().join(format!("bb-activity-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("song.mp3");
        std::fs::write(&file, vec![0u8; 64 * 1024]).unwrap();
        {
            let conn = pool.lock().unwrap();
            conn.execute(
                "INSERT INTO libraries(id, path, name) VALUES ('lib1', ?1, 'Music')",
                [dir.to_string_lossy()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tracks(id, library_id, title, file_path, duration)
                 VALUES ('t1', 'lib1', 'Opening', ?1, 240)",
                [file.to_string_lossy()],
            )
            .unwrap();
        }

        let resp = app
            .clone()
            .oneshot(get("/api/tracks/t1/stream", &user))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let snap = json_body(&send(app.clone(), get("/api/activity", &admin)).await.1);
        let row = &snap["playing"][0];
        assert_eq!(row["source"], "web");
        assert_eq!(row["status"], "streaming");
        assert_eq!(row["title"], "Opening");
        assert_eq!(row["delivery"]["transcoded"], false);
        assert_eq!(row["delivery"]["sourceFormat"], "MP3");

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.len(), 64 * 1024);
        // Finished, it lingers briefly (no flicker between range requests) and is swept later.
        let snap = json_body(&send(app.clone(), get("/api/activity", &admin)).await.1);
        assert_eq!(
            snap["playing"].as_array().unwrap().len(),
            0,
            "closed streams are not playing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sse_sends_an_initial_snapshot() {
        let (app, pool) = new_test_app_with_pool("activity-sse");
        let admin = seed_admin_session(&pool, "a1");
        let resp = app
            .oneshot(get("/api/activity/events", &admin))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let mut body = resp.into_body().into_data_stream();
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            futures_util::StreamExt::next(&mut body),
        )
        .await
        .expect("an event within 5 s")
        .unwrap()
        .unwrap();
        let text = String::from_utf8_lossy(&first);
        assert!(text.starts_with("event: snapshot"), "{text}");
        assert!(text.contains("\"totals\""));
    }
}

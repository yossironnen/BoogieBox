//! Admin Logs view (wip/admin-log-viewer-plan.md §5). Every route is admin-only; files are named by key
//! (see [`crate::logs`]), never by a client path.
//!
//! - `GET /api/admin/logs`: the log folder and every file, by source.
//! - `GET /api/admin/logs/{key}/lines?before=&limit=&level=&q=`: a page of lines ending at `before`.
//! - `GET /api/admin/logs/{key}/follow?from=`: SSE `lines` events (`{lines, end}`) for lines appended after `from`, and
//!   `rotated` (with the key now followed) when the file was replaced or a debug log moved to a new day.
//! - `GET /api/admin/logs/{key}/download`: the whole file, secrets masked.

use std::{
    convert::Infallible,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;

use crate::{
    auth::AdminUser,
    logs::{self, Filter, Level},
    ErrorResponse, SharedState,
};

const KEEP_ALIVE: Duration = Duration::from_secs(15);
const FOLLOW_EVERY: Duration = Duration::from_secs(1);
/// Most open "Follow" streams at once.
const MAX_FOLLOWERS: usize = 4;
static FOLLOWERS: AtomicUsize = AtomicUsize::new(0);

pub fn log_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/admin/logs", get(list_handler))
        .route("/api/admin/logs/{key}/lines", get(lines_handler))
        .route("/api/admin/logs/{key}/follow", get(follow_handler))
        .route("/api/admin/logs/{key}/download", get(download_handler))
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

fn server_log(state: &SharedState) -> Option<PathBuf> {
    state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .log_file
        .clone()
}

/// The server log and the file `key` names.
fn file_for(state: &SharedState, key: &str) -> Option<(PathBuf, PathBuf)> {
    let server = server_log(state)?;
    let path = logs::resolve(&server, key)?;
    Some((server, path))
}

fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "Log not found")
}

fn debug_toggles(state: &SharedState) -> (bool, bool) {
    let db = state.read().unwrap_or_else(|p| p.into_inner()).db.clone();
    let Some(db) = db else {
        return (false, false);
    };
    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
    let on =
        |key: &str| boogiebox_db::boogiemix::get_setting(&conn, key).as_deref() == Some("true");
    (
        on("scanDebugLoggingEnabled"),
        on("deepmixDebugLoggingEnabled"),
    )
}

async fn list_handler(State(state): State<SharedState>, _admin: AdminUser) -> Response {
    let Some(server) = server_log(&state) else {
        return Json(logs::LogList {
            folder: None,
            sources: Vec::new(),
        })
        .into_response();
    };
    let result = tokio::task::spawn_blocking(move || {
        let (scan, deep) = debug_toggles(&state);
        logs::list(&server, scan, deep)
    })
    .await;
    match result {
        Ok(list) => Json(list).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "Couldn't read the logs"),
    }
}

#[derive(Debug, Deserialize)]
struct LinesQuery {
    before: Option<u64>,
    limit: Option<usize>,
    level: Option<String>,
    q: Option<String>,
}

/// The filter, or the reason it is refused.
fn filter_from(level: Option<&str>, q: Option<&str>) -> Result<Filter, &'static str> {
    let min_level = match level.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => Some(Level::from_query(l).ok_or("Unknown level")?),
        None => None,
    };
    let query = q.map(str::trim).filter(|q| !q.is_empty());
    if query.is_some_and(|q| q.chars().count() > logs::MAX_QUERY_CHARS) {
        return Err("Search is too long");
    }
    Ok(Filter {
        min_level,
        query: query.map(str::to_lowercase),
    })
}

async fn lines_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(key): Path<String>,
    Query(q): Query<LinesQuery>,
) -> Response {
    let filter = match filter_from(q.level.as_deref(), q.q.as_deref()) {
        Ok(f) => f,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let Some((_, path)) = file_for(&state, &key) else {
        return not_found();
    };
    let limit = q.limit.unwrap_or(logs::DEFAULT_LIMIT);
    match tokio::task::spawn_blocking(move || logs::read_page(&path, q.before, limit, &filter))
        .await
    {
        Ok(Ok(page)) => Json(page).into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Couldn't read this log"),
    }
}

async fn download_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(key): Path<String>,
) -> Response {
    let Some((_, path)) = file_for(&state, &key) else {
        return not_found();
    };
    // Built from fixed prefixes and a validated date, so it is safe in the header as is.
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "boogiebox.log".into());
    match tokio::task::spawn_blocking(move || logs::read_redacted(&path)).await {
        Ok(Ok(text)) => (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{name}\""),
                ),
            ],
            text,
        )
            .into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Couldn't read this log"),
    }
}

#[derive(Debug, Deserialize)]
struct FollowQuery {
    from: Option<u64>,
}

/// Holds one of the [`MAX_FOLLOWERS`] places while its stream is open.
struct FollowerSlot;

impl FollowerSlot {
    fn take() -> Option<Self> {
        FOLLOWERS
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_FOLLOWERS).then_some(n + 1)
            })
            .ok()
            .map(|_| FollowerSlot)
    }
}

impl Drop for FollowerSlot {
    fn drop(&mut self) {
        FOLLOWERS.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Follow {
    server: PathBuf,
    key: String,
    offset: u64,
    /// `None` only in tests.
    _slot: Option<FollowerSlot>,
}

enum Tick {
    Lines(Vec<logs::LogLine>),
    Rotated,
    Nothing,
}

impl Follow {
    fn tick(&mut self) -> Tick {
        let Some(path) = logs::resolve(&self.server, &self.key) else {
            // Briefly missing while it is being rotated.
            return Tick::Nothing;
        };
        match logs::read_since(&path, self.offset) {
            Ok(Some((lines, next))) => {
                self.offset = next;
                if !lines.is_empty() {
                    return Tick::Lines(lines);
                }
                match logs::newer_key(&self.server, &self.key) {
                    Some(newer) => {
                        self.key = newer;
                        self.offset = 0;
                        Tick::Rotated
                    }
                    None => Tick::Nothing,
                }
            }
            Ok(None) => {
                self.offset = 0;
                Tick::Rotated
            }
            Err(_) => Tick::Nothing,
        }
    }
}

async fn follow_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(key): Path<String>,
    Query(q): Query<FollowQuery>,
) -> Response {
    let Some((server, path)) = file_for(&state, &key) else {
        return not_found();
    };
    let Some(slot) = FollowerSlot::take() else {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many logs are being followed",
        );
    };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let follow = Follow {
        server,
        key,
        offset: q.from.unwrap_or(size).min(size),
        _slot: Some(slot),
    };
    Sse::new(follow_stream(follow))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
        .into_response()
}

fn follow_stream(follow: Follow) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(follow, |follow| async move {
        let mut follow = follow;
        loop {
            tokio::time::sleep(FOLLOW_EVERY).await;
            let (tick, back) = tokio::task::spawn_blocking(move || {
                let tick = follow.tick();
                (tick, follow)
            })
            .await
            .ok()?;
            follow = back;
            let event = match tick {
                Tick::Lines(lines) => Event::default()
                    .event("lines")
                    .data(serde_json::json!({ "lines": lines, "end": follow.offset }).to_string()),
                Tick::Rotated => Event::default()
                    .event("rotated")
                    .data(serde_json::json!({ "key": follow.key }).to_string()),
                Tick::Nothing => continue,
            };
            return Some((Ok(event), follow));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        build_test_state, json_body, seed_admin_session, seed_user_session, send, send_full,
        temp_db_path,
    };
    use axum::body::Body;
    use axum::http::Request;

    fn get(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn admins_list_read_search_and_download_logs() {
        let db_path = temp_db_path("log-routes");
        let dir = db_path.parent().unwrap().join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        let server = dir.join("boogiebox-server.log");
        std::fs::write(
            &server,
            "BoogieBox server log initialized\n\
             2026-10-31T10:00:01.000000Z  INFO boogiebox_server: listening on http://localhost:8899\n\
             2026-10-31T10:00:02.000000Z  WARN boogiebox_server::post_scan: claim failed token=abc\n",
        )
        .unwrap();
        std::fs::write(dir.join("boogiebox-deepmix-debug.2026-10-31.log"), "").unwrap();
        let mut state = build_test_state(&db_path);
        state.log_file = Some(server.clone());
        let pool = state.db.clone().unwrap();
        let app = crate::build_app(state, None);
        let admin = seed_admin_session(&pool, "admin1");
        let user = seed_user_session(&pool, "user1");

        for uri in [
            "/api/admin/logs",
            "/api/admin/logs/server/lines",
            "/api/admin/logs/server/download",
            "/api/admin/logs/server/follow",
        ] {
            let (status, _) = send(app.clone(), get(uri, &user)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        }

        let (status, body) = send(app.clone(), get("/api/admin/logs", &admin)).await;
        assert_eq!(status, StatusCode::OK);
        let list = json_body(&body);
        assert_eq!(list["folder"], dir.display().to_string());
        assert_eq!(list["sources"][0]["files"][0]["key"], "server");
        assert_eq!(list["sources"][1]["enabled"], false);
        assert_eq!(list["sources"][2]["files"][0]["key"], "deep.2026-10-31");

        let (status, body) = send(app.clone(), get("/api/admin/logs/server/lines", &admin)).await;
        assert_eq!(status, StatusCode::OK);
        let page = json_body(&body);
        assert_eq!(page["lines"].as_array().unwrap().len(), 3);
        assert_eq!(page["lines"][2]["level"], "warn");
        assert_eq!(page["lines"][2]["text"], "claim failed token=•••••");

        let (_, body) = send(
            app.clone(),
            get("/api/admin/logs/server/lines?level=warn", &admin),
        )
        .await;
        assert_eq!(json_body(&body)["lines"].as_array().unwrap().len(), 1);
        let (_, body) = send(
            app.clone(),
            get("/api/admin/logs/server/lines?q=LISTENING", &admin),
        )
        .await;
        assert_eq!(json_body(&body)["lines"].as_array().unwrap().len(), 1);
        let (status, _) = send(
            app.clone(),
            get("/api/admin/logs/server/lines?level=loud", &admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let long = "x".repeat(logs::MAX_QUERY_CHARS + 1);
        let (status, _) = send(
            app.clone(),
            get(&format!("/api/admin/logs/server/lines?q={long}"), &admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        for key in ["nope", "server.2", "scan.2026-10-31", "..%2Fsecret"] {
            let (status, _) = send(
                app.clone(),
                get(&format!("/api/admin/logs/{key}/lines"), &admin),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{key}");
        }

        let (status, headers, body) =
            send_full(app.clone(), get("/api/admin/logs/server/download", &admin)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers[header::CONTENT_DISPOSITION],
            "attachment; filename=\"boogiebox-server.log\""
        );
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("listening on") && text.contains("token=•••••"));
        assert!(!text.contains("token=abc"));

        // Public status no longer names log files.
        let (_, body) = send(app.clone(), get("/api/system/status", "")).await;
        let status = json_body(&body);
        assert!(status.get("logFile").is_none());
        assert!(status.get("scanDebugLogFile").is_none());
    }

    #[test]
    fn follow_ticks_report_lines_rotation_and_a_new_day() {
        let dir = temp_db_path("log-follow").parent().unwrap().join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        let server = dir.join("boogiebox-server.log");
        std::fs::write(&server, "").unwrap();
        let day1 = dir.join("boogiebox-scan-debug.2026-10-30.log");
        std::fs::write(&day1, "first\n").unwrap();

        let mut follow = Follow {
            server: server.clone(),
            key: "scan.2026-10-30".into(),
            offset: 0,
            _slot: None,
        };
        assert!(matches!(follow.tick(), Tick::Lines(l) if l.len() == 1));
        assert!(matches!(follow.tick(), Tick::Nothing));
        std::fs::write(
            dir.join("boogiebox-scan-debug.2026-10-31.log"),
            "next day\n",
        )
        .unwrap();
        assert!(matches!(follow.tick(), Tick::Rotated));
        assert_eq!(follow.key, "scan.2026-10-31");
        assert!(matches!(follow.tick(), Tick::Lines(l) if l[0].text == "next day"));

        let mut server_follow = Follow {
            server: server.clone(),
            key: "server".into(),
            offset: 10,
            _slot: None,
        };
        assert!(
            matches!(server_follow.tick(), Tick::Rotated),
            "a shorter file was replaced"
        );
        assert_eq!(server_follow.offset, 0);
    }

    #[test]
    fn at_most_four_followers() {
        let mut slots = Vec::new();
        while let Some(slot) = FollowerSlot::take() {
            slots.push(slot);
        }
        assert_eq!(slots.len(), MAX_FOLLOWERS);
        assert!(FollowerSlot::take().is_none());
        drop(slots);
        assert!(
            FollowerSlot::take().is_some(),
            "places come back when streams close"
        );
    }
}

//! Play On session routes and the live event stream (`wip/play-on-renderers-plan.md` §6).
//!
//! Every route requires a logged-in user and returns 404 while `remotePlaybackEnabled` is off. Commands that
//! would act on another user's active session return **409 `session_in_use`** unless `takeover: true` is sent.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;

use crate::{
    auth::AuthenticatedUser,
    remote::{
        queue::{EnqueueMode, QueueItem, Repeat},
        read_remote_settings,
        session::{Command, Controller, SessionError},
        RemoteEvent, RemoteManager,
    },
    routes::remote_routes::valid_udn,
    DbPool, ErrorResponse, SharedState,
};

/// Max serialized size of a passed-through `radioReason`.
const MAX_REASON_BYTES: usize = 1024;
/// SSE keep-alive interval.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Session router.
pub fn remote_session_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/remote/sessions/{udn}",
            get(get_session_handler).delete(release_handler),
        )
        .route("/api/remote/sessions/{udn}/load", post(load_handler))
        .route("/api/remote/sessions/{udn}/command", post(command_handler))
        .route("/api/remote/sessions/{udn}/queue", post(queue_handler))
        .route(
            "/api/remote/sessions/{udn}/transfer",
            post(transfer_handler),
        )
        .route("/api/remote/events", get(events_handler))
        .with_state(state)
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

/// Maps session errors to HTTP (`InUse` → 409 with the takeover-prompt details).
pub fn session_error_response(e: SessionError) -> Response {
    match e {
        SessionError::InUse(info) => (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "session_in_use",
                "error": "This speaker is in use by another user",
                "udn": info.udn,
                "deviceName": info.device_name,
                "ownerUserId": info.owner_user_id,
                "ownerName": info.owner_name,
                "status": info.status,
                "nowPlaying": info.now_playing.map(|title| json!({ "title": title })),
            })),
        )
            .into_response(),
        SessionError::BadRequest(m) => error(StatusCode::BAD_REQUEST, &m),
        SessionError::Unsupported(what) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "code": "unsupported", "error": format!("This speaker does not support {what}") })),
        )
            .into_response(),
        SessionError::Renderer(m) => error(StatusCode::BAD_GATEWAY, &m),
        SessionError::Ended => error(StatusCode::NOT_FOUND, "No active session"),
    }
}

/// Resolves state + checks the feature switch and per-user rate limit.
async fn gate(
    state: &SharedState,
    user: &AuthenticatedUser,
    udn: Option<&str>,
    rate_limited: bool,
) -> Result<(DbPool, RemoteManager), Response> {
    if udn.is_some_and(|u| !valid_udn(u)) {
        return Err(error(StatusCode::BAD_REQUEST, "Invalid renderer id"));
    }
    let (db, remote) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        match s.db.clone() {
            Some(db) => (db, s.remote.clone()),
            None => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: "Database not configured".into(),
                        setup_required: Some(true),
                    }),
                )
                    .into_response())
            }
        }
    };
    let enabled = {
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            read_remote_settings(&conn).enabled
        })
        .await
        .unwrap_or(false)
    };
    if !enabled {
        return Err(error(StatusCode::NOT_FOUND, "Play On is disabled"));
    }
    if rate_limited && !remote.allow_request(&user.id) {
        return Err(error(StatusCode::TOO_MANY_REQUESTS, "Too many requests"));
    }
    Ok((db, remote))
}

fn controller(user: &AuthenticatedUser) -> Controller {
    Controller {
        user_id: user.id.clone(),
        username: user.username.clone(),
    }
}

fn reply(result: Result<crate::remote::session::SessionState, SessionError>) -> Response {
    match result {
        Ok(state) => Json(state).into_response(),
        Err(e) => session_error_response(e),
    }
}

async fn get_session_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
) -> Response {
    let (_, remote) = match gate(&state, &user, Some(&udn), false).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match remote.session(&udn) {
        Some(h) => Json(h.snapshot()).into_response(),
        None => error(StatusCode::NOT_FOUND, "No active session"),
    }
}

/// One item as sent by clients; the server resolves titles/durations itself.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    /// `track` | `mix`.
    pub kind: String,
    /// Track or mix-output id.
    pub id: Value,
    /// Artist Radio reason, passed through for the queue UI.
    #[serde(default)]
    pub radio_reason: Option<Value>,
}

fn id_string(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    ok.then_some(s)
}

/// Resolves client item refs into queue items (unknown/unauthorized ids are dropped).
pub async fn resolve_items(
    db: &DbPool,
    refs: Vec<ItemRef>,
    user_id: &str,
) -> Result<Vec<QueueItem>, String> {
    if refs.is_empty() || refs.len() > crate::remote::session::MAX_ITEMS {
        return Err("1–5000 items required".into());
    }
    let mut parsed = Vec::with_capacity(refs.len());
    for r in refs {
        let id = id_string(&r.id).ok_or("invalid item id")?;
        let reason = r
            .radio_reason
            .filter(|v| v.is_object() && v.to_string().len() <= MAX_REASON_BYTES);
        match r.kind.as_str() {
            "track" | "mix" => parsed.push((r.kind, id, reason)),
            _ => return Err("item kind must be 'track' or 'mix'".into()),
        }
    }
    let db = db.clone();
    let user = user_id.to_string();
    let items = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        parsed
            .into_iter()
            .filter_map(|(kind, id, reason)| {
                if kind == "track" {
                    let m = boogiebox_db::remote::get_track_media_meta(&conn, &id).ok()??;
                    Some(QueueItem::Track {
                        id: m.id,
                        title: m.title,
                        artist: m.artist,
                        album: m.album,
                        album_id: m.album_id,
                        duration: m.duration.filter(|d| *d > 0.0),
                        radio_reason: reason,
                    })
                } else {
                    let row = boogiebox_db::boogiemix::get_mix_output(
                        &conn,
                        &boogiebox_db::music::coerce_entity_id(&id),
                        &boogiebox_db::music::coerce_entity_id(&user),
                    )
                    .ok()??;
                    Some(QueueItem::Mix {
                        id,
                        title: if row.name.trim().is_empty() {
                            "BoogieMix".into()
                        } else {
                            row.name
                        },
                        duration: Some(row.duration_sec).filter(|d| *d > 0.0),
                    })
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|_| "internal error".to_string())?;
    if items.is_empty() {
        return Err("none of the items could be found".into());
    }
    Ok(items)
}

/// `POST /load` body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBody {
    /// Items in order.
    pub items: Vec<ItemRef>,
    /// Index into `items` to start at.
    #[serde(default)]
    pub start_index: usize,
    /// Start position (s).
    #[serde(default)]
    pub start_at_sec: Option<f64>,
    /// Opaque client queue source.
    #[serde(default)]
    pub queue_source: Option<Value>,
    /// Shuffle.
    #[serde(default)]
    pub shuffle: Option<bool>,
    /// Repeat.
    #[serde(default)]
    pub repeat: Option<Repeat>,
    /// Confirmed takeover.
    #[serde(default)]
    pub takeover: bool,
}

async fn load_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
    Json(body): Json<LoadBody>,
) -> Response {
    let (db, remote) = match gate(&state, &user, Some(&udn), true).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let requested = body.items.len();
    let items = match resolve_items(&db, body.items, &user.id).await {
        Ok(i) => i,
        Err(m) => return error(StatusCode::BAD_REQUEST, &m),
    };
    // Dropped items shift indices; clamp rather than fail.
    let start_index = body.start_index.min(items.len() - 1);
    if requested != items.len() {
        tracing::info!(target: "remote", requested, resolved = items.len(), "Play On load dropped unknown items");
    }
    let queue_source = body
        .queue_source
        .filter(|v| v.is_object() && v.to_string().len() <= MAX_REASON_BYTES);
    reply(
        remote
            .command(
                &db,
                &udn,
                Command::Load {
                    items,
                    start_index,
                    start_at: body
                        .start_at_sec
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .unwrap_or(0.0),
                    queue_source,
                    shuffle: body.shuffle,
                    repeat: body.repeat,
                },
                controller(&user),
                body.takeover,
            )
            .await,
    )
}

/// `POST /command` body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandBody {
    /// `play|pause|toggle|stop|next|prev|seek|jump|shuffle|repeat|volume|mute`.
    pub cmd: String,
    /// Argument for `seek` (s), `jump` (qid), `shuffle`/`mute` (bool), `repeat` (`off|one|all`), `volume` (0–100).
    #[serde(default)]
    pub value: Option<Value>,
    /// Confirmed takeover.
    #[serde(default)]
    pub takeover: bool,
}

/// Parses a `/command` body into a session command.
pub fn parse_command(body: &CommandBody) -> Result<Command, String> {
    let v = body.value.as_ref();
    let num = || v.and_then(Value::as_f64).ok_or("numeric value required");
    let flag = || v.and_then(Value::as_bool).ok_or("boolean value required");
    Ok(match body.cmd.as_str() {
        "play" => Command::Play,
        "pause" => Command::Pause,
        "toggle" => Command::Toggle,
        "stop" => Command::Stop,
        "next" => Command::Next,
        "prev" => Command::Prev,
        "seek" => Command::Seek(num()?),
        "jump" => Command::Jump(v.and_then(Value::as_u64).ok_or("queue id required")?),
        "shuffle" => Command::SetShuffle(flag()?),
        "mute" => Command::SetMute(flag()?),
        "volume" => {
            let n = num()?;
            if !(0.0..=100.0).contains(&n) {
                return Err("volume must be 0–100".into());
            }
            Command::SetVolume(n.round() as u8)
        }
        "repeat" => Command::SetRepeat(
            v.cloned()
                .and_then(|v| serde_json::from_value::<Repeat>(v).ok())
                .ok_or("repeat must be off, one or all")?,
        ),
        other => return Err(format!("unknown command '{other}'")),
    })
}

async fn command_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
    Json(body): Json<CommandBody>,
) -> Response {
    let (db, remote) = match gate(&state, &user, Some(&udn), true).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let cmd = match parse_command(&body) {
        Ok(c) => c,
        Err(m) => return error(StatusCode::BAD_REQUEST, &m),
    };
    // Plain transport commands need an existing session; only `load` creates one.
    if remote.session(&udn).is_none() {
        return session_error_response(SessionError::Ended);
    }
    reply(
        remote
            .command(&db, &udn, cmd, controller(&user), body.takeover)
            .await,
    )
}

/// `POST /queue` body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueBody {
    /// `enqueue|remove|move|clear`.
    pub op: String,
    /// Items for `enqueue`.
    #[serde(default)]
    pub items: Vec<ItemRef>,
    /// `end|next` for `enqueue`.
    #[serde(default)]
    pub mode: Option<EnqueueMode>,
    /// Entry for `remove`/`move`.
    #[serde(default)]
    pub qid: Option<u64>,
    /// Target index for `move`.
    #[serde(default)]
    pub to_index: Option<usize>,
    /// Confirmed takeover.
    #[serde(default)]
    pub takeover: bool,
}

async fn queue_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
    Json(body): Json<QueueBody>,
) -> Response {
    let (db, remote) = match gate(&state, &user, Some(&udn), true).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let cmd = match body.op.as_str() {
        "enqueue" => match resolve_items(&db, body.items, &user.id).await {
            Ok(items) => Command::Enqueue {
                items,
                mode: body.mode.unwrap_or(EnqueueMode::End),
            },
            Err(m) => return error(StatusCode::BAD_REQUEST, &m),
        },
        "remove" => match body.qid {
            Some(q) => Command::Remove(q),
            None => return error(StatusCode::BAD_REQUEST, "qid required"),
        },
        "move" => match (body.qid, body.to_index) {
            (Some(qid), Some(to)) => Command::Move { qid, to },
            _ => return error(StatusCode::BAD_REQUEST, "qid and toIndex required"),
        },
        "clear" => Command::Clear,
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "op must be enqueue, remove, move or clear",
            )
        }
    };
    if remote.session(&udn).is_none() {
        return session_error_response(SessionError::Ended);
    }
    reply(
        remote
            .command(&db, &udn, cmd, controller(&user), body.takeover)
            .await,
    )
}

/// `POST /transfer` body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferBody {
    /// Target renderer.
    pub to_udn: String,
    /// Confirmed takeover.
    #[serde(default)]
    pub takeover: bool,
}

async fn transfer_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
    Json(body): Json<TransferBody>,
) -> Response {
    if !valid_udn(&body.to_udn) {
        return error(StatusCode::BAD_REQUEST, "Invalid renderer id");
    }
    let (db, remote) = match gate(&state, &user, Some(&udn), true).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    reply(
        remote
            .transfer(&db, &udn, &body.to_udn, controller(&user), body.takeover)
            .await,
    )
}

/// `DELETE` query.
#[derive(Debug, Deserialize)]
pub struct ReleaseQuery {
    /// Stop the device (`true`/`false`); defaults to `remoteReleaseStopsRenderer`.
    stop: Option<bool>,
    /// Confirmed takeover.
    #[serde(default)]
    takeover: bool,
}

async fn release_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(udn): Path<String>,
    Query(q): Query<ReleaseQuery>,
) -> Response {
    let (db, remote) = match gate(&state, &user, Some(&udn), true).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let stop = match q.stop {
        Some(s) => s,
        None => {
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                read_remote_settings(&conn).release_stops_renderer
            })
            .await
            .unwrap_or(true)
        }
    };
    reply(
        remote
            .command(
                &db,
                &udn,
                Command::Release { stop },
                controller(&user),
                q.takeover,
            )
            .await,
    )
}

fn sse_event(e: &RemoteEvent) -> Event {
    let name = match e {
        RemoteEvent::Renderers => "renderers",
        RemoteEvent::Session { .. } => "session",
        RemoteEvent::Position { .. } => "position",
        RemoteEvent::Takeover { .. } => "takeover",
        RemoteEvent::Notice { .. } => "notice",
        RemoteEvent::SessionEnded { .. } => "sessionEnded",
    };
    Event::default()
        .event(name)
        .data(serde_json::to_string(e).unwrap_or_else(|_| "{}".into()))
}

/// Initial burst for a new subscriber: renderer list hint plus every live session.
fn initial_events(remote: &RemoteManager) -> Vec<RemoteEvent> {
    std::iter::once(RemoteEvent::Renderers)
        .chain(
            remote
                .session_snapshots()
                .into_iter()
                .map(|s| RemoteEvent::Session { state: Box::new(s) }),
        )
        .collect()
}

async fn events_handler(State(state): State<SharedState>, user: AuthenticatedUser) -> Response {
    let (_, remote) = match gate(&state, &user, None, false).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let rx = remote.subscribe();
    let initial = initial_events(&remote);
    let stream = event_stream(remote, rx, initial);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive"))
        .into_response()
}

/// Turns the broadcast receiver into an SSE stream. A lagging client gets fresh snapshots instead of the
/// missed events.
fn event_stream(
    remote: RemoteManager,
    rx: tokio::sync::broadcast::Receiver<RemoteEvent>,
    initial: Vec<RemoteEvent>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let pending: std::collections::VecDeque<RemoteEvent> = initial.into();
    stream::unfold(
        (remote, rx, pending),
        |(remote, mut rx, mut pending)| async move {
            loop {
                if let Some(e) = pending.pop_front() {
                    return Some((Ok(sse_event(&e)), (remote, rx, pending)));
                }
                match rx.recv().await {
                    Ok(e) => return Some((Ok(sse_event(&e)), (remote, rx, pending))),
                    Err(RecvError::Lagged(_)) => pending.extend(initial_events(&remote)),
                    Err(RecvError::Closed) => return None,
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::session::{InUseInfo, SessionStatus};

    fn body(cmd: &str, value: Option<Value>) -> CommandBody {
        CommandBody {
            cmd: cmd.into(),
            value,
            takeover: false,
        }
    }

    #[test]
    fn command_parsing() {
        assert_eq!(parse_command(&body("play", None)).unwrap(), Command::Play);
        for c in ["pause", "toggle", "stop", "next", "prev"] {
            assert!(parse_command(&body(c, None)).is_ok(), "{c}");
        }
        assert_eq!(
            parse_command(&body("seek", Some(json!(12.5)))).unwrap(),
            Command::Seek(12.5)
        );
        assert_eq!(
            parse_command(&body("jump", Some(json!(4)))).unwrap(),
            Command::Jump(4)
        );
        assert_eq!(
            parse_command(&body("shuffle", Some(json!(true)))).unwrap(),
            Command::SetShuffle(true)
        );
        assert_eq!(
            parse_command(&body("mute", Some(json!(false)))).unwrap(),
            Command::SetMute(false)
        );
        assert_eq!(
            parse_command(&body("volume", Some(json!(33.4)))).unwrap(),
            Command::SetVolume(33)
        );
        assert_eq!(
            parse_command(&body("repeat", Some(json!("all")))).unwrap(),
            Command::SetRepeat(Repeat::All)
        );
        for bad in [
            body("seek", None),
            body("jump", Some(json!(-1))),
            body("shuffle", Some(json!("yes"))),
            body("volume", Some(json!(101))),
            body("repeat", Some(json!("twice"))),
            body("dance", None),
        ] {
            assert!(parse_command(&bad).is_err(), "{:?}", bad.cmd);
        }
    }

    #[test]
    fn error_mapping() {
        let r = session_error_response(SessionError::InUse(Box::new(InUseInfo {
            udn: "u".into(),
            device_name: "WiiM".into(),
            owner_user_id: "alice".into(),
            owner_name: Some("Alice".into()),
            status: SessionStatus::Playing,
            now_playing: Some("Angel".into()),
        })));
        assert_eq!(r.status(), StatusCode::CONFLICT);
        assert_eq!(
            session_error_response(SessionError::BadRequest("x".into())).status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            session_error_response(SessionError::Unsupported("seek")).status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            session_error_response(SessionError::Renderer("x".into())).status(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            session_error_response(SessionError::Ended).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn id_and_rate_limit_rules() {
        assert_eq!(id_string(&json!("ab-1_2")).as_deref(), Some("ab-1_2"));
        assert_eq!(id_string(&json!(42)).as_deref(), Some("42"));
        assert_eq!(id_string(&json!("../x")), None);
        assert_eq!(id_string(&json!("")), None);
        assert_eq!(id_string(&json!(true)), None);
        assert_eq!(id_string(&json!("x".repeat(65))), None);

        let rl = crate::remote::RateLimiter::default();
        for _ in 0..crate::remote::RATE_LIMIT_PER_SEC {
            assert!(rl.allow("u"));
        }
        assert!(!rl.allow("u"));
        assert!(rl.allow("other"));
    }

    mod routes {
        use super::super::*;
        use crate::remote::registry::{tests::description, RendererCaps};
        use crate::test_support::{build_test_state, json_body, seed_session, send, temp_db_path};
        use axum::body::Body;
        use axum::http::Request;

        fn ok(action: &str, inner: &str) -> String {
            format!(
                r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:x">{inner}</u:{action}Response></s:Body></s:Envelope>"#
            )
        }

        async fn setup(prefix: &str) -> (Router, DbPool, RemoteManager) {
            let state = build_test_state(&temp_db_path(prefix));
            let db = state.db.clone().unwrap();
            let remote = state.remote.clone();
            let dir = std::env::temp_dir().join(format!("bb-{prefix}-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&dir).unwrap();
            {
                let conn = db.lock().unwrap();
                conn.execute_batch(
                    "INSERT INTO libraries(id, path, name) VALUES('lib1', 'D:\\Music', 'Music');",
                )
                .unwrap();
                for (id, title) in [("t1", "Angel"), ("t2", "Teardrop")] {
                    let file = dir.join(format!("{id}.flac"));
                    std::fs::write(&file, b"flac").unwrap();
                    conn.execute(
                        "INSERT INTO tracks(id, library_id, file_path, title, duration) VALUES(?1, 'lib1', ?2, ?3, 300)",
                        rusqlite::params![id, file.to_string_lossy(), title],
                    )
                    .unwrap();
                }
            }
            for udn in ["a", "b"] {
                let (url, _) =
                    crate::upnp::soap::tests::spawn_mock(|action: &str, _: &str| match action {
                        "GetTransportInfo" => (
                            200,
                            ok(
                                action,
                                "<CurrentTransportState>TRANSITIONING</CurrentTransportState>",
                            ),
                        ),
                        "GetPositionInfo" => (200, ok(action, "<TrackURI></TrackURI>")),
                        "GetVolume" => (200, ok(action, "<CurrentVolume>20</CurrentVolume>")),
                        _ => (200, ok(action, "")),
                    })
                    .await;
                let mut d = description(udn, "127.0.0.1");
                d.av_transport.control_url = url;
                remote.registry().insert_described(
                    d,
                    RendererCaps {
                        set_next: true,
                        seek: true,
                        ..Default::default()
                    },
                    "http://127.0.0.1:1/d.xml",
                    60,
                    std::time::Instant::now(),
                );
                boogiebox_db::remote::upsert_renderer(
                    &db.lock().unwrap(),
                    &boogiebox_db::remote::RendererUpsert {
                        udn,
                        friendly_name: udn,
                        manufacturer: None,
                        model_name: None,
                        vendor: "generic",
                        location_url: "http://127.0.0.1:1/d.xml",
                        capabilities: "{}",
                    },
                )
                .unwrap();
            }
            (crate::build_app(state, None), db, remote)
        }

        fn post(uri: &str, cookie: &str, body: Value) -> Request<Body> {
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        }

        fn get(uri: &str, cookie: &str) -> Request<Body> {
            Request::builder()
                .uri(uri)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap()
        }

        #[tokio::test]
        async fn session_routes_end_to_end() {
            let (app, db, _remote) = setup("remote-session-routes").await;
            let alice = seed_session(&db, "alice", "Alice", "user");
            let bob = seed_session(&db, "bob", "Bob", "user");

            let (status, _) = send(
                app.clone(),
                Request::builder()
                    .uri("/api/remote/sessions/a")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            let (status, _) = send(app.clone(), get("/api/remote/sessions/a", &alice)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "no session yet");
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/command",
                    &alice,
                    json!({"cmd":"play"}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "commands need a session");
            let (status, _) = send(app.clone(), get("/api/remote/sessions/bad%20id", &alice)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            // Load (one unknown id is dropped; a radio reason passes through).
            let (status, body) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/load",
                    &alice,
                    json!({"items":[{"kind":"track","id":"t1","radioReason":{"kind":"seed"}},{"kind":"track","id":"nope"},{"kind":"track","id":"t2"}],"startIndex":0,"queueSource":{"type":"album","id":"x"}}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let s = json_body(&body);
            assert_eq!(s["status"], "loading");
            assert_eq!(s["queue"].as_array().unwrap().len(), 2);
            assert_eq!(s["queue"][0]["title"], "Angel");
            assert_eq!(s["queue"][0]["radioReason"]["kind"], "seed");
            assert_eq!(s["ownerName"], "Alice");
            for bad in [
                json!({"items":[]}),
                json!({"items":[{"kind":"song","id":"t1"}]}),
                json!({"items":[{"kind":"track","id":"../x"}]}),
                json!({"items":[{"kind":"track","id":"nope"}]}),
            ] {
                let (status, _) = send(
                    app.clone(),
                    post("/api/remote/sessions/a/load", &alice, bad.clone()),
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
            }

            // Bob sees "in use" and gets 409 until he confirms.
            let (_, body) = send(app.clone(), get("/api/remote/renderers", &bob)).await;
            let list = json_body(&body)["renderers"].as_array().unwrap().clone();
            let a = list.iter().find(|r| r["udn"] == "a").unwrap();
            assert_eq!(a["session"]["heldByOther"], true);
            assert_eq!(a["session"]["ownerName"], "Alice");
            assert_eq!(a["session"]["nowPlaying"], "Angel");
            let (status, body) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/command",
                    &bob,
                    json!({"cmd":"pause"}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            let conflict = json_body(&body);
            assert_eq!(conflict["code"], "session_in_use");
            assert_eq!(conflict["ownerName"], "Alice");
            assert_eq!(conflict["nowPlaying"]["title"], "Angel");
            let (status, body) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/command",
                    &bob,
                    json!({"cmd":"pause","takeover":true}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(json_body(&body)["ownerName"], "Bob");

            // Command/queue validation.
            for bad in [json!({"cmd":"seek"}), json!({"cmd":"fly"})] {
                let (status, _) = send(
                    app.clone(),
                    post("/api/remote/sessions/a/command", &bob, bad),
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST);
            }
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/command",
                    &bob,
                    json!({"cmd":"volume","value":30}),
                ),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "no RenderingControl on this mock"
            );
            let (status, body) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/queue",
                    &bob,
                    json!({"op":"enqueue","items":[{"kind":"track","id":"t1"}],"mode":"next"}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let q = json_body(&body)["queue"].as_array().unwrap().clone();
            assert_eq!(q.len(), 3);
            let last_qid = q[2]["qid"].as_u64().unwrap();
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/queue",
                    &bob,
                    json!({"op":"move","qid":last_qid,"toIndex":0}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/queue",
                    &bob,
                    json!({"op":"remove","qid":last_qid}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            for bad in [
                json!({"op":"remove"}),
                json!({"op":"move","qid":1}),
                json!({"op":"shake"}),
                json!({"op":"enqueue","items":[]}),
            ] {
                let (status, _) = send(
                    app.clone(),
                    post("/api/remote/sessions/a/queue", &bob, bad.clone()),
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
            }

            // Transfer a → b, then release b.
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/transfer",
                    &bob,
                    json!({"toUdn":"bad id"}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            let (status, body) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/transfer",
                    &bob,
                    json!({"toUdn":"b"}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            assert_eq!(json_body(&body)["udn"], "b");
            tokio::time::sleep(Duration::from_millis(100)).await;
            let (status, _) = send(app.clone(), get("/api/remote/sessions/a", &bob)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "source released");
            let del = Request::builder()
                .method("DELETE")
                .uri("/api/remote/sessions/b")
                .header("cookie", &bob)
                .body(Body::empty())
                .unwrap();
            let (status, _) = send(app.clone(), del).await;
            assert_eq!(status, StatusCode::OK);
            tokio::time::sleep(Duration::from_millis(100)).await;
            let (status, _) = send(app.clone(), get("/api/remote/sessions/b", &bob)).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            let del = Request::builder()
                .method("DELETE")
                .uri("/api/remote/sessions/b?stop=false")
                .header("cookie", &bob)
                .body(Body::empty())
                .unwrap();
            let (status, _) = send(app.clone(), del).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "nothing left to release");

            // Feature switch.
            boogiebox_db::upsert_setting(&db.lock().unwrap(), "remotePlaybackEnabled", "false")
                .unwrap();
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/load",
                    &alice,
                    json!({"items":[{"kind":"track","id":"t1"}]}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn event_stream_starts_with_snapshots() {
            use futures_util::StreamExt;
            let (app, db, remote) = setup("remote-events").await;
            let alice = seed_session(&db, "alice", "Alice", "user");
            let (status, _) = send(
                app.clone(),
                post(
                    "/api/remote/sessions/a/load",
                    &alice,
                    json!({"items":[{"kind":"track","id":"t1"}]}),
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let resp = tower::ServiceExt::oneshot(app.clone(), get("/api/remote/events", &alice))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert!(resp.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/event-stream"));
            let mut body = resp.into_body().into_data_stream();
            let mut text = String::new();
            while !text.contains("event: session") {
                let chunk = tokio::time::timeout(Duration::from_secs(5), body.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                text.push_str(&String::from_utf8_lossy(&chunk));
            }
            assert!(text.contains("event: renderers"));
            assert!(text.contains("\"udn\":\"a\""));
            // Live events follow.
            remote.notify(RemoteEvent::Notice {
                udn: "a".into(),
                level: "info",
                message: "hello".into(),
            });
            let mut more = String::new();
            while !more.contains("hello") {
                let chunk = tokio::time::timeout(Duration::from_secs(5), body.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                more.push_str(&String::from_utf8_lossy(&chunk));
            }
            assert!(more.contains("event: notice"));

            let (status, _) = send(
                app,
                Request::builder()
                    .uri("/api/remote/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn rate_limit_applies_to_state_changing_routes() {
            let (app, db, _) = setup("remote-rate").await;
            let alice = seed_session(&db, "alice", "Alice", "user");
            let mut limited = false;
            for _ in 0..(crate::remote::RATE_LIMIT_PER_SEC + 5) {
                let (status, _) = send(
                    app.clone(),
                    post(
                        "/api/remote/sessions/a/command",
                        &alice,
                        json!({"cmd":"play"}),
                    ),
                )
                .await;
                limited |= status == StatusCode::TOO_MANY_REQUESTS;
            }
            assert!(limited);
        }
    }

    #[test]
    fn sse_event_names() {
        let e = sse_event(&RemoteEvent::SessionEnded { udn: "u".into() });
        let text = format!("{e:?}");
        assert!(text.contains("sessionEnded"));
        assert!(format!("{:?}", sse_event(&RemoteEvent::Renderers)).contains("renderers"));
    }
}

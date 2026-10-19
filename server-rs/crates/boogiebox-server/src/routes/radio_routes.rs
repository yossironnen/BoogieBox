//! Internet radio stations for the BoogieBox UI: everyone lists and plays them, admins add, edit and delete
//! them. `/stream` relays the station through the server so the web player's audio graph (EQ, meters) works
//! without CORS. Only admin-entered URLs (and one level of `.pls`/`.m3u` they point to) are fetched.
//! See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-6).

use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use boogiebox_db::radio_stations::{self as rdb, StationInput};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    DbPool, ErrorResponse, OkResponse, SharedState,
};

/// Most stations relayed at once (each holds an upstream connection).
pub const MAX_RELAYS: usize = 8;
/// Largest playlist file read when a station URL is a `.pls` / `.m3u`.
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;

static RELAY_SLOTS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_RELAYS)));

/// Live streams never end, so no total timeout; a stalled one is dropped after 30 s without data.
static RELAY_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent(concat!("BoogieBox/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

/// Documents the Radio Router public API surface.
pub fn radio_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/radio-stations",
            get(list_handler).post(create_handler),
        )
        .route(
            "/api/radio-stations/{id}",
            axum::routing::put(update_handler).delete(delete_handler),
        )
        .route("/api/radio-stations/{id}/stream", get(stream_handler))
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
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

async fn run<T, F>(db: DbPool, f: F) -> Result<T, Response>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        f(&conn)
    })
    .await
    {
        Ok(Ok(v)) => Ok(v),
        _ => Err(error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error",
        )),
    }
}

async fn list_handler(State(state): State<SharedState>, _user: AuthenticatedUser) -> Response {
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    match run(db, rdb::list_stations).await {
        Ok(stations) => Json(stations).into_response(),
        Err(r) => r,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StationBody {
    name: Option<String>,
    stream_url: Option<String>,
    homepage_url: Option<String>,
}

fn parse_body(body: &StationBody) -> Result<StationInput, rdb::StationError> {
    StationInput::new(
        body.name.as_deref().unwrap_or(""),
        body.stream_url.as_deref().unwrap_or(""),
        body.homepage_url.as_deref(),
    )
}

async fn create_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Json(body): Json<StationBody>,
) -> Response {
    let input = match parse_body(&body) {
        Ok(i) => i,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.message()),
    };
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    match run(db, move |conn| rdb::create_station(conn, &input)).await {
        Ok(station) => (StatusCode::CREATED, Json(station)).into_response(),
        Err(r) => r,
    }
}

async fn update_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<StationBody>,
) -> Response {
    if !valid_id(&id) {
        return error(StatusCode::NOT_FOUND, "Station not found");
    }
    let input = match parse_body(&body) {
        Ok(i) => i,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.message()),
    };
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    match run(db, move |conn| rdb::update_station(conn, &id, &input)).await {
        Ok(Some(station)) => Json(station).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "Station not found"),
        Err(r) => r,
    }
}

async fn delete_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Response {
    if !valid_id(&id) {
        return error(StatusCode::NOT_FOUND, "Station not found");
    }
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    match run(db, move |conn| rdb::delete_station(conn, &id)).await {
        Ok(true) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "Station not found"),
        Err(r) => r,
    }
}

// ── Relay ──────────────────────────────────────────────────────────────────────

/// How a station's playlist file reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playlist {
    /// The first stream URL in it.
    Stream(String),
    /// An HLS playlist (segments; not a single stream).
    Hls,
    /// Nothing usable.
    Empty,
}

/// Reads a `.pls` (`FileN=url`) or `.m3u` (one URL per line) playlist.
pub fn parse_playlist(text: &str) -> Playlist {
    if text.contains("#EXT-X-") {
        return Playlist::Hls;
    }
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('['))
        .filter_map(|l| match l.split_once('=') {
            Some((k, v)) if k.trim().to_ascii_lowercase().starts_with("file") => Some(v.trim()),
            Some(_) if !l.contains("://") => None,
            _ => Some(l),
        })
        .find(|u| rdb::valid_http_url(u))
        .map(|u| Playlist::Stream(u.to_owned()))
        .unwrap_or(Playlist::Empty)
}

/// True when a response is a playlist file rather than audio.
pub fn is_playlist(content_type: &str, url: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    ct.contains("mpegurl")
        || ct.contains("scpls")
        || ct.contains("x-pls")
        || ((path.ends_with(".pls") || path.ends_with(".m3u") || path.ends_with(".m3u8"))
            && !ct.starts_with("audio/mpeg")
            && !ct.starts_with("audio/aac"))
}

/// Content types the relay passes on (anything else is not a radio stream).
pub fn is_audio(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.starts_with("audio/")
        || ct.starts_with("application/ogg")
        || ct.starts_with("application/octet-stream")
}

fn content_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

async fn read_limited(resp: reqwest::Response) -> Option<String> {
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk.ok()?);
        if buf.len() > MAX_PLAYLIST_BYTES {
            return None;
        }
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Opens a station: follows one playlist level and returns the audio response.
pub async fn open_station(
    client: &reqwest::Client,
    url: &str,
) -> Result<reqwest::Response, (StatusCode, &'static str)> {
    let unreachable = (StatusCode::BAD_GATEWAY, "The station could not be reached");
    let mut current = url.to_owned();
    for depth in 0..2 {
        let resp = client.get(&current).send().await.map_err(|_| unreachable)?;
        if !resp.status().is_success() {
            return Err(unreachable);
        }
        let ct = content_type(&resp);
        if depth == 0 && is_playlist(&ct, &current) {
            let text = read_limited(resp).await.ok_or(unreachable)?;
            match parse_playlist(&text) {
                Playlist::Stream(next) => {
                    current = next;
                    continue;
                }
                Playlist::Hls => {
                    return Err((
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "HLS radio streams are not supported",
                    ))
                }
                Playlist::Empty => return Err(unreachable),
            }
        }
        if !is_audio(&ct) {
            return Err((StatusCode::BAD_GATEWAY, "The station did not send audio"));
        }
        return Ok(resp);
    }
    Err(unreachable)
}

async fn stream_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Response {
    if !valid_id(&id) {
        return error(StatusCode::NOT_FOUND, "Station not found");
    }
    let Some(db) = db_of(&state) else {
        return not_ready();
    };
    let station = match run(db, move |conn| rdb::get_station(conn, &id)).await {
        Ok(Some(s)) => s,
        Ok(None) => return error(StatusCode::NOT_FOUND, "Station not found"),
        Err(r) => return r,
    };
    let Ok(permit) = RELAY_SLOTS.clone().try_acquire_owned() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Too many radio stations are playing",
        );
    };
    let upstream = match open_station(&RELAY_CLIENT, &station.stream_url).await {
        Ok(r) => r,
        Err((status, message)) => {
            tracing::info!(station = %station.id, %status, "Radio station relay failed");
            return error(status, message);
        }
    };
    let ct = content_type(&upstream);
    let body = upstream.bytes_stream().map(move |chunk| {
        let _slot = &permit;
        chunk
    });
    let mut resp = Response::new(Body::from_stream(body));
    let headers = resp.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&ct).unwrap_or(HeaderValue::from_static("audio/mpeg")),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("none"));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send, send_full,
    };
    use axum::http::Request;
    use serde_json::json;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn req(
        method: &str,
        uri: &str,
        cookie: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let r = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", cookie);
        match body {
            Some(b) => r
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => r.body(Body::empty()).unwrap(),
        }
    }

    #[test]
    fn playlists_parse() {
        assert_eq!(
            parse_playlist(
                "[playlist]\nNumberOfEntries=1\nFile1=http://ice.example/live\nTitle1=X\n"
            ),
            Playlist::Stream("http://ice.example/live".into())
        );
        assert_eq!(
            parse_playlist("#EXTM3U\n#EXTINF:-1,Radio\nhttps://a.example/s.mp3\n"),
            Playlist::Stream("https://a.example/s.mp3".into())
        );
        assert_eq!(
            parse_playlist("#EXTM3U\n#EXT-X-VERSION:3\nseg1.ts\n"),
            Playlist::Hls
        );
        assert_eq!(parse_playlist("File1=ftp://nope\n"), Playlist::Empty);
        assert_eq!(parse_playlist(""), Playlist::Empty);
    }

    #[test]
    fn content_kinds() {
        assert!(is_playlist("audio/x-scpls", "http://a/b"));
        assert!(is_playlist("audio/x-mpegurl", "http://a/b"));
        assert!(is_playlist("text/plain", "http://a/listen.pls?x=1"));
        assert!(!is_playlist("audio/mpeg", "http://a/stream.m3u"));
        assert!(!is_playlist("audio/mpeg", "http://a/stream"));
        assert!(is_audio("audio/aacp"));
        assert!(is_audio("application/ogg"));
        assert!(!is_audio("text/html; charset=utf-8"));
    }

    #[tokio::test]
    async fn stations_are_admin_edited_and_user_listed() {
        let (app, pool) = new_test_app_with_pool("radio-routes");
        let admin = seed_admin_session(&pool, "a1");
        let user = seed_user_session(&pool, "u1");
        let station =
            json!({"name": "Jazz", "streamUrl": "http://jazz.example/live", "homepageUrl": ""});
        let (status, _) = send(
            app.clone(),
            req("POST", "/api/radio-stations", &user, Some(station.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, body) = send(
            app.clone(),
            req("POST", "/api/radio-stations", &admin, Some(station)),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = json_body(&body)["id"].as_str().unwrap().to_owned();
        let (status, _) = send(
            app.clone(),
            req(
                "POST",
                "/api/radio-stations",
                &admin,
                Some(json!({"name": "Bad", "streamUrl": "file:///etc"})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, body) =
            send(app.clone(), req("GET", "/api/radio-stations", &user, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)[0]["streamUrl"], "http://jazz.example/live");

        let edit = json!({"name": "Jazz 24", "streamUrl": "https://jazz.example/24"});
        let uri = format!("/api/radio-stations/{id}");
        assert_eq!(
            send(app.clone(), req("PUT", &uri, &user, Some(edit.clone())))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let (status, body) = send(app.clone(), req("PUT", &uri, &admin, Some(edit.clone()))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["name"], "Jazz 24");
        assert_eq!(
            send(
                app.clone(),
                req("PUT", "/api/radio-stations/nope", &admin, Some(edit))
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(app.clone(), req("DELETE", &uri, &user, None)).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(app.clone(), req("DELETE", &uri, &admin, None)).await.0,
            StatusCode::OK
        );
        assert_eq!(
            send(app.clone(), req("DELETE", &uri, &admin, None)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(
                app.clone(),
                req("GET", "/api/radio-stations/missing/stream", &user, None)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn relay_follows_a_playlist_and_refuses_non_audio() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/listen.pls"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/x-scpls")
                    .set_body_string(format!("[playlist]\nFile1={}/live\n", server.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/live"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/mpeg")
                    .set_body_bytes(vec![0xFFu8, 0xFB, 0x90, 0x00]),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string("<html>"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/hls.m3u8"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/vnd.apple.mpegurl")
                    .set_body_string("#EXTM3U\n#EXT-X-TARGETDURATION:6\nseg.ts\n"),
            )
            .mount(&server)
            .await;

        let (app, pool) = new_test_app_with_pool("radio-relay");
        let user = seed_user_session(&pool, "u1");
        let ids: Vec<String> = {
            let conn = pool.lock().unwrap();
            ["listen.pls", "page", "hls.m3u8"]
                .iter()
                .map(|p| {
                    rdb::create_station(
                        &conn,
                        &StationInput::new(p, &format!("{}/{p}", server.uri()), None).unwrap(),
                    )
                    .unwrap()
                    .id
                })
                .collect()
        };
        let (status, headers, body) = send_full(
            app.clone(),
            req(
                "GET",
                &format!("/api/radio-stations/{}/stream", ids[0]),
                &user,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("content-type").unwrap(), "audio/mpeg");
        assert_eq!(headers.get("accept-ranges").unwrap(), "none");
        assert_eq!(body, vec![0xFFu8, 0xFB, 0x90, 0x00]);
        let (status, _) = send(
            app.clone(),
            req(
                "GET",
                &format!("/api/radio-stations/{}/stream", ids[1]),
                &user,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        let (status, _) = send(
            app.clone(),
            req(
                "GET",
                &format!("/api/radio-stations/{}/stream", ids[2]),
                &user,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}

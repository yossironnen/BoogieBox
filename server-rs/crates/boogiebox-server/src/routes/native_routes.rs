//! Native stream links for the desktop app (`wip/native-hires-engine-plan.md` §6.2):
//! `POST /api/native/stream-tokens` (signed in) and `GET|HEAD /media/n/{token}` (the token is the authorization).

use std::path::PathBuf;
use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};

use crate::native::{self, Lookup, NativeDelivery, NativeLink};
use crate::{auth::AuthenticatedUser, ffmpeg, media_serve, ErrorResponse, SharedState};

/// Native link routes.
pub fn native_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/native/stream-tokens", post(issue_handler))
        .route("/media/n/{token}", get(media_handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueBody {
    /// Track ids, or `boogiemix:{outputId}` for rendered mixes.
    ids: Vec<String>,
    /// The desktop app's machine name (shown on the Activity page).
    client: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct IssuedLink {
    id: String,
    /// `/media/n/{token}` (relative to the server origin).
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// Milliseconds since the epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    /// Format of the stream at `url` (`FLAC`, `M4A`, …, or `WAV` for a server decode).
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bits_per_sample: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channels: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<f64>,
    /// `native` (the app decodes the original) or `server_wav` (the server decodes to WAV).
    #[serde(skip_serializing_if = "Option::is_none")]
    decode: Option<&'static str>,
    /// `not_found`, `file_missing` or `unsupported` (no link issued).
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
}

impl IssuedLink {
    fn failed(id: String, error: &'static str) -> Self {
        Self {
            id,
            url: None,
            expires_at: None,
            format: None,
            sample_rate: None,
            bits_per_sample: None,
            channels: None,
            duration: None,
            decode: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
struct IssueResponse {
    tokens: Vec<IssuedLink>,
}

/// One requested item, resolved from the database.
struct Resolved {
    id: String,
    item_id: String,
    file_path: PathBuf,
    duration: Option<f64>,
    sample_rate: Option<u32>,
    bits: Option<u16>,
    channels: Option<u16>,
    /// An internet radio station (relayed live, no file).
    station: bool,
}

fn bad_request(error: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: error.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn resolve(
    conn: &rusqlite::Connection,
    user_id: &str,
    id: &str,
) -> Result<Option<Resolved>, boogiebox_db::jobs::JobError> {
    if let Some(output) = id.strip_prefix("boogiemix:") {
        let output_id = boogiebox_db::music::coerce_entity_id(output);
        let user = boogiebox_db::music::coerce_entity_id(user_id);
        let row = boogiebox_db::boogiemix::get_mix_output(conn, &output_id, &user)?;
        let file = boogiebox_db::boogiemix::get_mix_output_file(conn, &output_id, &user)?;
        return Ok(row.zip(file).map(|(row, (path, _))| Resolved {
            id: id.to_owned(),
            item_id: format!("mx-{output}"),
            file_path: PathBuf::from(path),
            duration: Some(row.duration_sec).filter(|d| *d > 0.0),
            sample_rate: None,
            bits: None,
            channels: None,
            station: false,
        }));
    }
    if let Some(station_id) = id.strip_prefix("radio:") {
        let station = boogiebox_db::radio_stations::get_station(conn, station_id)?;
        return Ok(station.map(|st| Resolved {
            id: id.to_owned(),
            item_id: st.id,
            file_path: PathBuf::new(),
            duration: None,
            sample_rate: None,
            bits: None,
            channels: None,
            station: true,
        }));
    }
    let meta = boogiebox_db::remote::get_track_media_meta(conn, id)?;
    Ok(meta.map(|m| Resolved {
        id: id.to_owned(),
        item_id: m.id,
        file_path: PathBuf::from(m.file_path),
        duration: m.duration.filter(|d| *d > 0.0),
        sample_rate: m.sample_rate.and_then(|r| u32::try_from(r).ok()),
        bits: m.bits_per_sample.and_then(|b| u16::try_from(b).ok()),
        channels: m.channels.and_then(|c| u16::try_from(c).ok()),
        station: false,
    }))
}

/// `POST /api/native/stream-tokens` — `{ ids, client? }` → one entry per id, in order.
async fn issue_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    jar: CookieJar,
    Json(body): Json<IssueBody>,
) -> Response {
    if body.ids.is_empty() || body.ids.len() > native::MAX_IDS {
        return bad_request("ids must hold 1 to 50 items");
    }
    // The extractor already checked the cookie; the session key ties the links to this sign-in.
    let Some(session) = jar
        .get("bb_session")
        .map(|c| native::session_key(c.value()))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (db, links) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (s.db.clone(), s.native.clone())
    };
    let Some(db) = db else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let client = crate::activity::short_label(body.client.as_deref(), 64).unwrap_or_default();
    let ids = body.ids;
    let user_id = user.id.clone();
    // Database reads under one short lock; file checks (possibly on a NAS) after it is released.
    let resolved = tokio::task::spawn_blocking(move || {
        let found = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            ids.iter()
                .map(|id| resolve(&conn, &user_id, id).map(|r| (id.clone(), r)))
                .collect::<Result<Vec<_>, _>>()
        }?;
        Ok::<_, boogiebox_db::jobs::JobError>(
            found
                .into_iter()
                .map(|(id, r)| {
                    let checked = r.map(|r| {
                        let present = r.station || r.file_path.is_file();
                        let server =
                            !r.station && present && native::needs_server_decode(&r.file_path);
                        (r, present, server)
                    });
                    (id, checked)
                })
                .collect::<Vec<_>>(),
        )
    })
    .await;
    let resolved = match resolved {
        Ok(Ok(r)) => r,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let now = Instant::now();
    links.sweep(now);
    let tokens = resolved
        .into_iter()
        .map(|(id, checked)| match checked {
            None => IssuedLink::failed(id, "not_found"),
            Some((_, false, _)) => IssuedLink::failed(id, "file_missing"),
            // A server WAV needs the duration up front (its length is in the header).
            Some((r, true, true)) if r.duration.is_none() => IssuedLink::failed(id, "unsupported"),
            Some((r, true, server)) => {
                let delivery = if r.station {
                    NativeDelivery::Radio
                } else if server {
                    NativeDelivery::Wav
                } else {
                    NativeDelivery::Original
                };
                let ttl = native::link_ttl(r.duration);
                let mime = ffmpeg::audio_mime_type(&r.file_path);
                let (format, sample_rate, bits, channels) = match delivery {
                    NativeDelivery::Original => (
                        r.file_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_ascii_uppercase(),
                        r.sample_rate,
                        r.bits,
                        r.channels,
                    ),
                    NativeDelivery::Wav => {
                        let layout = wav_layout(r.duration, r.sample_rate, r.channels);
                        (
                            "WAV".to_owned(),
                            Some(layout.sample_rate),
                            Some(layout.bits),
                            Some(layout.channels),
                        )
                    }
                    // The station decides (MP3 or AAC); the app probes the stream.
                    NativeDelivery::Radio => (String::new(), None, None, None),
                };
                let token = links.issue(NativeLink {
                    session: session.clone(),
                    user_id: user.id.clone(),
                    username: user.username.clone(),
                    client: client.clone(),
                    item_id: r.item_id,
                    file_path: r.file_path,
                    mime,
                    duration: r.duration,
                    sample_rate: r.sample_rate,
                    channels: r.channels,
                    delivery,
                    expires_at: now + ttl,
                });
                IssuedLink {
                    id: r.id,
                    url: Some(format!("/media/n/{token}")),
                    expires_at: Some(epoch_ms() + ttl.as_millis() as u64),
                    format: Some(format).filter(|f| !f.is_empty()),
                    sample_rate,
                    bits_per_sample: bits,
                    channels,
                    duration: r.duration,
                    decode: Some(delivery.decode()),
                    error: None,
                }
            }
        })
        .collect();
    Json(IssueResponse { tokens }).into_response()
}

/// The WAV the server streams for a source the app can't decode: 24-bit at the source rate.
fn wav_layout(
    duration: Option<f64>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
) -> media_serve::WavLayout {
    media_serve::WavLayout::new(
        duration.unwrap_or(0.0),
        sample_rate.unwrap_or(48_000),
        channels.unwrap_or(2),
        24,
    )
}

/// `GET|HEAD /media/n/{token}` — 404 unknown, 410 expired (ask for a new link).
async fn media_handler(
    State(state): State<SharedState>,
    Path(token): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if !native::is_token_shaped(&token) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let links = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .native
        .clone();
    let link = match links.lookup(&token, Instant::now()) {
        Lookup::Live(link) => link,
        Lookup::Expired => return StatusCode::GONE.into_response(),
        Lookup::Missing => return StatusCode::NOT_FOUND.into_response(),
    };
    if link.delivery == NativeDelivery::Radio {
        return crate::routes::radio_routes::relay_station(
            &state,
            link.item_id.clone(),
            &link.user_id,
            &link.username,
            crate::activity::Source::Desktop,
            link.client.clone(),
        )
        .await;
    }
    if !link.file_path.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let head_only = method == Method::HEAD;
    let tracked = crate::activity::TransferMeta {
        source: crate::activity::Source::Desktop,
        kind: crate::activity::TransferKind::Stream,
        user_id: Some(link.user_id.clone()),
        username: Some(link.username.clone()),
        client: link.client.clone(),
        item_id: link.item_id.clone(),
        delivery: Some(match link.delivery {
            NativeDelivery::Original => crate::activity::DeliveryInfo::original(&link.file_path),
            NativeDelivery::Wav => {
                crate::activity::DeliveryInfo::transcode(&link.file_path, "WAV", None)
            }
            // Relayed above.
            NativeDelivery::Radio => return StatusCode::NOT_FOUND.into_response(),
        }),
    };
    let resp = match link.delivery {
        NativeDelivery::Radio => return StatusCode::NOT_FOUND.into_response(),
        NativeDelivery::Original => {
            media_serve::serve_file(&link.file_path, link.mime, &headers, head_only, &[]).await
        }
        NativeDelivery::Wav => {
            media_serve::serve_wav_transcode(media_serve::WavTranscode {
                file_path: &link.file_path,
                layout: wav_layout(link.duration, link.sample_rate, link.channels),
                range: headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
                loudness: ffmpeg::LoudnessFilter::None,
                head_only,
                log_id: &link.item_id,
                extra: &[],
            })
            .await
        }
    };
    crate::activity::hub_of(&state).track_response(tracked, resp, head_only)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{build_test_state, json_body, send, send_full, temp_db_path};
    use axum::body::Body;
    use axum::http::Request;

    struct Fixture {
        app: Router,
        links: native::NativeLinks,
        db: crate::DbPool,
        dir: PathBuf,
        library: boogiebox_db::music::EntityId,
    }

    fn fixture(prefix: &str) -> Fixture {
        let state = build_test_state(&temp_db_path(prefix));
        let links = state.native.clone();
        let db = state.db.clone().unwrap();
        let dir = std::env::temp_dir().join(format!("bb-native-{prefix}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let library = {
            let conn = db.lock().unwrap();
            boogiebox_db::jobs::create_library(
                &conn,
                boogiebox_db::jobs::CreateLibraryInput {
                    folders: vec![dir.to_string_lossy().into_owned()],
                    name: Some("Music".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap()
            .id
        };
        let app = crate::build_app(state, None);
        Fixture {
            app,
            links,
            db,
            dir,
            library,
        }
    }

    impl Fixture {
        /// A library track at `name` (the file is written when `bytes` is given).
        fn track(
            &self,
            name: &str,
            bytes: Option<&[u8]>,
            duration: f64,
            bits: Option<i64>,
        ) -> String {
            let path = self.dir.join(name);
            if let Some(b) = bytes {
                std::fs::write(&path, b).unwrap();
            }
            let conn = self.db.lock().unwrap();
            let id = boogiebox_db::jobs::upsert_scanned_track(
                &conn,
                &boogiebox_db::jobs::ScannedTrackInput {
                    library_id: self.library.clone(),
                    file_path: path.to_string_lossy().into_owned(),
                    file_name: name.into(),
                    file_size: bytes.map_or(0, |b| b.len() as i64),
                    format: "FLAC".into(),
                    title: name.into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    album_artist: "Artist".into(),
                    genre: String::new(),
                    composer: String::new(),
                    track_number: None,
                    disc_number: None,
                    year: None,
                    comment: None,
                    bpm: None,
                    duration,
                    bitrate: None,
                    sample_rate: Some(96_000),
                    channels: Some(2),
                    bits_per_sample: bits,
                    loudness: None,
                    musicbrainz: Default::default(),
                },
            )
            .unwrap();
            id.to_string()
        }

        async fn issue(
            &self,
            cookie: Option<&str>,
            body: serde_json::Value,
        ) -> (StatusCode, Vec<u8>) {
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/native/stream-tokens")
                .header("content-type", "application/json");
            if let Some(c) = cookie {
                req = req.header("cookie", c);
            }
            send(
                self.app.clone(),
                req.body(Body::from(body.to_string())).unwrap(),
            )
            .await
        }
    }

    fn get(uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
        let mut b = Request::builder().uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(Body::empty()).unwrap()
    }

    fn live(links: &native::NativeLinks, url: &str) -> NativeLink {
        match links.lookup(url.trim_start_matches("/media/n/"), Instant::now()) {
            Lookup::Live(link) => *link,
            other => panic!("expected a live link, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn issues_links_that_serve_the_original_file_with_range() {
        let f = fixture("native-issue");
        let cookie = crate::test_support::seed_user_session(&f.db, "u1");
        let hires = f.track("a.flac", Some(b"0123456789"), 200.0, Some(24));
        let gone = f.track("gone.flac", None, 200.0, Some(16));
        let wma = f.track("b.wma", Some(b"wma"), 0.0, None);

        let (status, body) = f
            .issue(
                Some(&cookie),
                serde_json::json!({
                    "ids": [hires, "nope", gone, wma, "boogiemix:42"],
                    "client": "  STUDIO-PC  "
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let tokens = json_body(&body)["tokens"].as_array().unwrap().clone();
        assert_eq!(tokens.len(), 5, "one entry per id, in order");
        let first = &tokens[0];
        assert_eq!(first["id"], hires.as_str());
        assert_eq!(first["format"], "FLAC");
        assert_eq!(first["sampleRate"], 96_000);
        assert_eq!(first["bitsPerSample"], 24);
        assert_eq!(first["channels"], 2);
        assert_eq!(first["decode"], "native");
        assert!(first["expiresAt"].as_u64().unwrap() > epoch_ms() + 6 * 3_600_000);
        assert_eq!(tokens[1]["error"], "not_found");
        assert_eq!(tokens[2]["error"], "file_missing");
        assert_eq!(
            tokens[3]["error"], "unsupported",
            "server WAV needs a duration"
        );
        assert_eq!(tokens[4]["error"], "not_found", "no such mix for this user");
        assert!(tokens[1].get("url").is_none());

        // No cookie on the media request: the link is the authorization.
        let url = first["url"].as_str().unwrap().to_owned();
        assert!(url.starts_with("/media/n/"));
        let (status, headers, body) =
            send_full(f.app.clone(), get(&url, &[("range", "bytes=2-4")])).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(body, b"234");
        assert_eq!(headers["content-type"], "audio/flac");
        let head = Request::builder()
            .method("HEAD")
            .uri(&url)
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = send_full(f.app.clone(), head).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-length"], "10");
        assert!(body.is_empty());

        // The machine name reached the link (Activity shows it).
        let link = live(&f.links, &url);
        assert_eq!(
            (link.client.as_str(), link.user_id.as_str()),
            ("STUDIO-PC", "u1")
        );
    }

    #[tokio::test]
    async fn rejects_signed_out_and_badly_sized_requests() {
        let f = fixture("native-reject");
        let (status, _) = f.issue(None, serde_json::json!({ "ids": ["1"] })).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let cookie = crate::test_support::seed_user_session(&f.db, "u1");
        let (status, _) = f
            .issue(Some(&cookie), serde_json::json!({ "ids": [] }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let many: Vec<String> = (0..51).map(|i| i.to_string()).collect();
        let (status, _) = f
            .issue(Some(&cookie), serde_json::json!({ "ids": many }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        for bad in ["nope".to_string(), "0".repeat(48)] {
            let (status, _) = send(f.app.clone(), get(&format!("/media/n/{bad}"), &[])).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn expired_links_answer_gone_and_sign_out_revokes() {
        let f = fixture("native-expiry");
        let cookie = crate::test_support::seed_user_session(&f.db, "u1");
        let other = crate::test_support::seed_session(&f.db, "u1", "u1", "user");
        let id = f.track("a.flac", Some(b"0123456789"), 200.0, Some(16));
        let url_of = |body: &[u8]| {
            json_body(body)["tokens"][0]["url"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let (_, body) = f
            .issue(Some(&cookie), serde_json::json!({ "ids": [id] }))
            .await;
        let mine = url_of(&body);
        let (_, body) = f
            .issue(Some(&other), serde_json::json!({ "ids": [id] }))
            .await;
        let theirs = url_of(&body);

        // Expired: 410, so the app knows to ask again.
        let expired = f.links.issue(NativeLink {
            expires_at: Instant::now() - std::time::Duration::from_secs(1),
            ..live(&f.links, &mine)
        });
        let (status, _) = send(f.app.clone(), get(&format!("/media/n/{expired}"), &[])).await;
        assert_eq!(status, StatusCode::GONE);

        // Signing out ends this sign-in's links only.
        let logout = Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(f.app.clone(), logout).await.0, StatusCode::OK);
        assert_eq!(
            send(f.app.clone(), get(&mine, &[])).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(f.app.clone(), get(&theirs, &[])).await.0,
            StatusCode::OK
        );
    }

    /// A station link is live and relays the station's stream, labelled as the desktop app.
    #[tokio::test]
    async fn radio_station_links_relay_the_live_stream() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/mpeg")
                    .set_body_bytes(b"ID3radio-bytes".to_vec()),
            )
            .mount(&upstream)
            .await;
        let f = fixture("native-radio");
        let station = {
            let conn = f.db.lock().unwrap();
            boogiebox_db::radio_stations::create_station(
                &conn,
                &boogiebox_db::radio_stations::StationInput {
                    name: "Test FM".into(),
                    stream_url: format!("{}/live", upstream.uri()),
                    homepage_url: None,
                },
            )
            .unwrap()
        };
        let cookie = crate::test_support::seed_user_session(&f.db, "u1");
        let id = format!("radio:{}", station.id);
        let (_, body) = f
            .issue(
                Some(&cookie),
                serde_json::json!({ "ids": [id, "radio:nope"], "client": "STUDIO-PC" }),
            )
            .await;
        let tokens = json_body(&body)["tokens"].clone();
        assert_eq!(tokens[0]["decode"], "live");
        assert!(tokens[0].get("format").is_none(), "the station decides");
        assert_eq!(tokens[1]["error"], "not_found");
        let url = tokens[0]["url"].as_str().unwrap();
        let (status, headers, body) = send_full(f.app.clone(), get(url, &[])).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["content-type"], "audio/mpeg");
        assert_eq!(body, b"ID3radio-bytes");
    }

    /// Opus is decoded by the server into a 24-bit WAV at the source rate (needs FFmpeg).
    #[tokio::test]
    async fn opus_is_served_as_a_seekable_24_bit_wav() {
        let f = fixture("native-wav");
        let opus = f.dir.join("made.opus");
        let made = std::process::Command::new(ffmpeg::resolve_ffmpeg()) // nosemgrep: boogiebox-rust-dynamic-command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("sine=frequency=440:duration=2:sample_rate=48000")
            .args(["-ac", "2", "-c:a", "libopus"])
            .arg(&opus)
            .status();
        if !matches!(made, Ok(s) if s.success()) {
            eprintln!("ffmpeg not available; skipping the server WAV check");
            return;
        }
        let bytes = std::fs::read(&opus).unwrap();
        let id = f.track("o.opus", Some(&bytes), 2.0, None);
        {
            let conn = f.db.lock().unwrap();
            conn.execute("UPDATE tracks SET sample_rate = 48000 WHERE id = ?1", [&id])
                .unwrap();
        }
        let cookie = crate::test_support::seed_user_session(&f.db, "u1");
        let (_, body) = f
            .issue(Some(&cookie), serde_json::json!({ "ids": [id] }))
            .await;
        let link = &json_body(&body)["tokens"][0];
        assert_eq!(link["decode"], "server_wav");
        assert_eq!(link["format"], "WAV");
        assert_eq!(link["sampleRate"], 48_000);
        assert_eq!(link["bitsPerSample"], 24);

        let url = link["url"].as_str().unwrap();
        let (status, headers, body) =
            send_full(f.app.clone(), get(url, &[("range", "bytes=0-43")])).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(&body[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(body[24..28].try_into().unwrap()), 48_000);
        assert_eq!(u16::from_le_bytes(body[34..36].try_into().unwrap()), 24);
        let total: u64 = headers["content-range"]
            .to_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            total,
            44 + 2 * 48_000 * 2 * 3,
            "2 s of 24-bit stereo plus the header"
        );
    }
}

//! Defines Rust API routes for Provider Routes server behavior.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    providers::{
        fetch_deezer_artist_top, fetch_listenbrainz_top_recordings, fetch_lrclib_lyrics,
        get_spotify_access_token, search_deezer_artist_match, search_metadata,
    },
    DbPool, ErrorResponse, SharedState,
};

// -- Router --------------------------------------------------------------------

/// Documents the Provider Router public API surface.
pub fn provider_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/integrations/spotify/test", get(spotify_test_handler))
        .route("/api/integrations/genius/test", post(genius_test_handler))
        .route("/api/integrations/lyrics", get(lyrics_search_handler))
        .route(
            "/api/integrations/metadata-search",
            get(metadata_search_handler),
        )
        .route("/api/admin/provider-usage", get(provider_usage_handler))
        .route("/api/artists/{id}/top-songs", get(artist_top_songs_handler))
        .route("/api/artists/{id}/about", get(artist_about_handler))
        .route("/api/albums/{id}/about", get(album_about_handler))
        .with_state(state)
}

// -- Query params --------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LyricsSearchParams {
    artist: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataSearchParams {
    artist: Option<String>,
    album: Option<String>,
}

// -- Handlers ------------------------------------------------------------------

async fn spotify_test_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let (client_id, client_secret) = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        (
            boogiebox_db::artwork::get_setting(&conn, "spotifyClientId"),
            boogiebox_db::artwork::get_setting(&conn, "spotifyClientSecret"),
        )
    })
    .await
    .unwrap_or((None, None));

    let (Some(cid), Some(csecret)) = (client_id, client_secret) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Set spotifyClientId and spotifyClientSecret first" }),
            ),
        )
            .into_response();
    };

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    match get_spotify_access_token(&http_client, &cid, &csecret).await {
        Some(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Spotify auth failed" })),
        )
            .into_response(),
    }
}

async fn genius_test_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let body_id = body["clientId"].as_str().unwrap_or("").trim().to_owned();
    let body_secret = body["clientSecret"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_owned();

    let (stored_id, stored_secret) = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        (
            boogiebox_db::artwork::get_setting(&conn, "geniusClientId"),
            boogiebox_db::artwork::get_setting(&conn, "geniusClientSecret"),
        )
    })
    .await
    .unwrap_or((None, None));

    let client_id = if !body_id.is_empty() {
        body_id
    } else {
        stored_id.unwrap_or_default()
    };
    let client_secret = if !body_secret.is_empty() {
        body_secret
    } else {
        stored_secret.unwrap_or_default()
    };

    if client_id.is_empty() || client_secret.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Set geniusClientId and geniusClientSecret first" })),
        )
            .into_response();
    }

    use base64::{engine::general_purpose::STANDARD, Engine};
    let credentials = STANDARD.encode(format!("{client_id}:{client_secret}"));

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let resp = http_client
        .post("https://api.genius.com/oauth/token")
        .header("Authorization", format!("Basic {credentials}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await.unwrap_or(serde_json::Value::Null);
            if data["access_token"].is_string() {
                (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Genius auth failed" })),
                )
                    .into_response()
            }
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Genius auth failed" })),
        )
            .into_response(),
    }
}

async fn lyrics_search_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<LyricsSearchParams>,
) -> impl IntoResponse {
    let artist = params.artist.as_deref().unwrap_or("").trim().to_owned();
    let title = params.title.as_deref().unwrap_or("").trim().to_owned();

    if artist.is_empty() || title.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "artist and title are required" })),
        )
            .into_response();
    }

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();

    if let Some(result) = fetch_lrclib_lyrics(&http_client, &artist, &title).await {
        let synced_json = result
            .synced
            .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null));
        let mut obj = serde_json::json!({
            "lyrics": result.lyrics,
            "source": result.source,
        });
        if let Some(synced) = synced_json {
            obj["syncedLyrics"] = synced;
        }
        return (StatusCode::OK, Json(obj)).into_response();
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "Lyrics not found" })),
    )
        .into_response()
}

async fn metadata_search_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<MetadataSearchParams>,
) -> impl IntoResponse {
    let artist = params.artist.as_deref().unwrap_or("").trim().to_owned();
    if artist.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "artist is required" })),
        )
            .into_response();
    }
    let album = params
        .album
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let (musicbrainz, discogs_token, spotify_id, spotify_secret) =
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            (
                boogiebox_db::artwork::get_setting(&conn, "radioKeylessProviders")
                    .is_none_or(|value| value != "false"),
                boogiebox_db::artwork::get_setting(&conn, "discogsToken"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientId"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientSecret"),
            )
        })
        .await
        .unwrap_or((true, None, None, None));

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let (results, provider_warnings) = search_metadata(
        &http_client,
        musicbrainz,
        discogs_token.as_deref(),
        spotify_id.as_deref(),
        spotify_secret.as_deref(),
        &artist,
        album.as_deref(),
    )
    .await;

    let providers: Vec<String> = results
        .iter()
        .map(|r| r.provider.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if !providers.is_empty() {
        if let Some(db2) = get_db(&state) {
            let entity_type = if album.is_some() {
                "album_metadata_search"
            } else {
                "artist_metadata_search"
            };
            let entity_type = entity_type.to_owned();
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
                for prov in &providers {
                    boogiebox_db::artwork::record_provider_usage(
                        &conn,
                        prov,
                        &entity_type,
                        "display_result",
                    );
                }
            })
            .await;
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "results": results, "provider_warnings": provider_warnings })),
    )
        .into_response()
}

async fn provider_usage_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let snapshot = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::list_provider_usage(&conn)
    })
    .await;

    match snapshot {
        Ok(snap) => (StatusCode::OK, Json(snap)).into_response(),
        _ => internal_error(),
    }
}

/// An artist's bio: `{ source, summary, full, url, stats }` (see `about.rs`); `source` is `null` when no source
/// had a text.
async fn artist_about_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    about_response(&state, &id, true).await
}

/// An album's description, same shape as the artist bio (no stats).
async fn album_about_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    about_response(&state, &id, false).await
}

async fn about_response(state: &SharedState, id: &str, artist: bool) -> axum::response::Response {
    let Some(db) = get_db(state) else {
        return setup_required_response();
    };
    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let entity = boogiebox_db::music::coerce_entity_id(id);
    let about = if artist {
        crate::about::artist_about(&db, &http_client, &entity).await
    } else {
        crate::about::album_about(&db, &http_client, &entity).await
    };
    match about {
        Some(about) => (StatusCode::OK, Json(about)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": if artist { "Artist not found" } else { "Album not found" } })),
        )
            .into_response(),
    }
}

/// Songs shown in an artist's "Top 5 Songs".
const TOP_SONGS: usize = 5;

/// An artist's most played songs: ListenBrainz listen counts by MusicBrainz id, else Deezer's order (no key
/// either way). Answers `{ source, tracks: [{ name, listens, url }] }`; `source` is `listenbrainz`, `deezer` or
/// `null` (nothing found). Cached a week, an empty answer a day; a failed lookup isn't cached.
async fn artist_top_songs_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(db) = get_db(&state) else {
        return setup_required_response();
    };
    let artist_id = boogiebox_db::music::coerce_entity_id(&id);
    let cache_key = format!("topsongs:{id}");
    let lookup = {
        let (db, key, artist_id) = (db.clone(), cache_key.clone(), artist_id.clone());
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let cached = boogiebox_db::artwork::get_lastfm_cached(&conn, &key);
            let identity = boogiebox_db::music::get_artist_external_identity(&conn, &artist_id)
                .ok()
                .flatten();
            let mbid = boogiebox_db::radio::get_artist_musicbrainz_id(&conn, &artist_id)
                .ok()
                .flatten();
            (cached, identity, mbid)
        })
        .await
    };
    let Ok((cached, identity, mbid)) = lookup else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Internal server error" })),
        )
            .into_response();
    };
    if let Some(value) =
        cached.and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
    {
        return (StatusCode::OK, Json(value)).into_response();
    }
    let Some(identity) = identity else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Artist not found" })),
        )
            .into_response();
    };
    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();

    let mut failed = false;
    let mut source = None;
    let mut tracks = Vec::new();
    if let Some(mbid) = mbid.as_deref() {
        match fetch_listenbrainz_top_recordings(&http_client, mbid).await {
            Ok(songs) if !songs.is_empty() => {
                source = Some("listenbrainz");
                tracks = songs.into_iter().take(TOP_SONGS).collect();
            }
            Ok(_) => {}
            Err(_) => failed = true,
        }
    }
    if source.is_none() {
        let deezer_id = match identity.deezer_artist_id {
            Some(id) => Some(id),
            None => search_deezer_artist_match(&http_client, &identity.name)
                .await
                .map(|m| m.external_id),
        };
        if let Some(deezer_id) = deezer_id {
            match fetch_deezer_artist_top(&http_client, &deezer_id, TOP_SONGS).await {
                Ok(songs) if !songs.is_empty() => {
                    source = Some("deezer");
                    tracks = songs;
                    failed = false;
                }
                Ok(_) => {}
                Err(_) => failed = true,
            }
        }
    }
    let body = serde_json::json!({ "source": source, "tracks": tracks });
    if !failed || source.is_some() {
        let days = if source.is_some() { 7 } else { 1 };
        if let Ok(data) = serde_json::to_string(&body) {
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::artwork::save_lastfm_cache(&conn, &cache_key, &data, days);
            })
            .await;
        }
    }
    (StatusCode::OK, Json(body)).into_response()
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
    use crate::test_support::{
        json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};

    // These tests cover validation, auth-gating, and cache-hit paths that don't require
    // a real network call. The actual provider-fetch branches (Spotify/Genius/Last.fm/
    // LRCLIB HTTP requests) are deferred to Phase 3 (wiremock-backed tests), per
    // wip/server-rust-coverage-gap-plan.md.

    #[tokio::test]
    async fn spotify_test_requires_auth_and_configured_credentials() {
        let (app, pool) = new_test_app_with_pool("provider-spotify");
        let cookie = seed_user_session(&pool, "u1");

        let (unauth_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/integrations/spotify/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(unauth_status, StatusCode::UNAUTHORIZED);

        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/integrations/spotify/test")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_body(&body)["error"]
            .as_str()
            .unwrap()
            .contains("Set spotifyClientId"));
    }

    #[tokio::test]
    async fn genius_test_requires_credentials_from_body_or_settings() {
        let (app, pool) = new_test_app_with_pool("provider-genius");
        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/integrations/genius/test")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_body(&body)["error"]
            .as_str()
            .unwrap()
            .contains("Set geniusClientId"));
    }

    #[tokio::test]
    async fn lyrics_search_requires_artist_and_title() {
        let (app, pool) = new_test_app_with_pool("provider-lyrics");
        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/integrations/lyrics?artist=Artist")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json_body(&body)["error"]
            .as_str()
            .unwrap()
            .contains("artist and title"));
    }

    #[tokio::test]
    async fn metadata_search_requires_artist() {
        let (app, pool) = new_test_app_with_pool("provider-metadata");
        let cookie = seed_user_session(&pool, "u1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/integrations/metadata-search")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn metadata_search_surfaces_provider_warnings_alongside_results() {
        // Regression coverage: a rate-limited provider used to look
        // identical to "no matches" — see conversation / providers.rs's
        // search_metadata failover tests for the underlying logic. This
        // confirms the route actually threads the warning through.
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/artist/"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "artists": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/token"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/search"))
            .respond_with(wiremock::ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let (app, pool) = new_test_app_with_pool("provider-metadata-warnings");
        let cookie = seed_user_session(&pool, "u1");
        {
            let conn = pool.lock().unwrap();
            conn.execute(
                "INSERT INTO settings(key, value) VALUES ('spotifyClientId', 'id'), ('spotifyClientSecret', 'secret')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
        }

        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/integrations/metadata-search?artist=Madonna")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["results"].as_array().unwrap().len(), 0);
        let warnings = json_body(&body)["provider_warnings"].clone();
        assert_eq!(warnings[0]["provider"], "spotify");
        assert_eq!(warnings[0]["reason"], "rate_limited");

        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_API_BASE");
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn provider_usage_requires_admin_and_lists_snapshot() {
        let (app, pool) = new_test_app_with_pool("provider-usage");
        let user_cookie = seed_user_session(&pool, "u1");
        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/admin/provider-usage")
                .header("cookie", user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        let admin_cookie = seed_admin_session(&pool, "admin1");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/admin/provider-usage")
                .header("cookie", admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn about_comes_from_wikipedia_then_theaudiodb_then_old_last_fm_and_is_cached() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = MockServer::start().await;
        for var in [
            "BOOGIEBOX_MUSICBRAINZ_API_BASE",
            "BOOGIEBOX_WIKIDATA_API_BASE",
            "BOOGIEBOX_WIKIPEDIA_API_BASE",
            "BOOGIEBOX_THEAUDIODB_API_BASE",
            "BOOGIEBOX_LISTENBRAINZ_API_BASE",
        ] {
            std::env::set_var(var, server.uri());
        }
        let wiki_mbid = "10adbe5e-a2c0-4bf3-8249-2b4cbf6e6ca8";
        let tadb_mbid = "0383dadf-2a4e-4d10-a46a-e9e041da8eb3";
        Mock::given(method("GET"))
            .and(path(format!("/artist/{wiki_mbid}")))
            .and(query_param("inc", "url-rels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "relations": [
                    { "type": "discogs", "url": { "resource": "https://www.discogs.com/artist/1" } },
                    { "type": "wikidata", "url": { "resource": "https://www.wikidata.org/wiki/Q187134" } }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .and(query_param("ids", "Q187134"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entities": { "Q187134": { "sitelinks": { "enwiki": { "title": "Massive Attack" } } } }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/rest_v1/page/summary/Massive_Attack"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "type": "standard",
                "extract": "Massive Attack are an English trip hop collective.",
                "content_urls": { "desktop": { "page": "https://en.wikipedia.org/wiki/Massive_Attack" } }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/1/popularity/artist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "artist_mbid": wiki_mbid, "total_listen_count": 12400, "total_user_count": 3100 }
            ])))
            .mount(&server)
            .await;
        // The second artist: MusicBrainz has no Wikipedia link, TheAudioDB knows it by its id.
        Mock::given(method("GET"))
            .and(path(format!("/artist/{tadb_mbid}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "relations": [] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/artist-mb.php"))
            .and(query_param("i", tadb_mbid))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [{ "idArtist": "111239", "strArtist": "Portishead",
                              "strBiographyEN": "Portishead are a band from Bristol.\n\nThey formed in 1991." }]
            })))
            .mount(&server)
            .await;
        // An album found by artist and title.
        Mock::given(method("GET"))
            .and(path("/searchalbum.php"))
            .and(query_param("s", "Portishead"))
            .and(query_param("a", "Dummy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "album": [{ "idAlbum": "2115", "strAlbum": "Dummy", "strDescriptionEN": "Dummy is the debut album." }]
            })))
            .mount(&server)
            .await;
        // Nothing for the third artist anywhere: an old Last.fm answer isn't shown any more.
        Mock::given(method("GET"))
            .and(path("/search.php"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "artists": null })),
            )
            .mount(&server)
            .await;

        let (app, pool) = new_test_app_with_pool("provider-about");
        let cookie = seed_user_session(&pool, "u1");
        {
            let conn = pool.lock().unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO artists(id, name, musicbrainz_artist_id) VALUES ('ar-wiki', 'Massive Attack', '{wiki_mbid}');
                 INSERT INTO artists(id, name, musicbrainz_artist_id) VALUES ('ar-tadb', 'Portishead', '{tadb_mbid}');
                 INSERT INTO artists(id, name) VALUES ('ar-old', 'Old Band');
                 INSERT INTO albums(id, title, album_artist, artist_id) VALUES ('al-1', 'Dummy', 'Portishead', 'ar-tadb');
                 INSERT INTO lastfm_cache(cache_key, data, fetched_at, expires_at)
                   VALUES ('artist:old band', '{{\"summary\":\"Old bio.\",\"full\":\"Old bio.\",\"url\":\"https://www.last.fm/music/Old+Band\"}}',
                           datetime('now', '-60 days'), datetime('now', '-53 days'));"
            ))
            .unwrap();
        }
        let get = |uri: &str| {
            Request::builder()
                .uri(uri)
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap()
        };

        let (status, body) = send(app.clone(), get("/api/artists/ar-wiki/about")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["source"], "wikipedia");
        assert_eq!(
            body["summary"],
            "Massive Attack are an English trip hop collective."
        );
        assert_eq!(body["url"], "https://en.wikipedia.org/wiki/Massive_Attack");
        assert_eq!(body["stats"]["listens"], 12400);
        assert_eq!(body["stats"]["listeners"], 3100);
        // Cached: MusicBrainz is asked once (`expect(1)`).
        let (_, again) = send(app.clone(), get("/api/artists/ar-wiki/about")).await;
        assert_eq!(json_body(&again)["source"], "wikipedia");

        let (_, body) = send(app.clone(), get("/api/artists/ar-tadb/about")).await;
        let body = json_body(&body);
        assert_eq!(body["source"], "theaudiodb");
        assert_eq!(body["summary"], "Portishead are a band from Bristol.");
        assert_eq!(
            body["full"],
            "Portishead are a band from Bristol.\n\nThey formed in 1991."
        );
        assert_eq!(body["url"], "https://www.theaudiodb.com/artist/111239");

        let (_, body) = send(app.clone(), get("/api/albums/al-1/about")).await;
        let body = json_body(&body);
        assert_eq!(body["source"], "theaudiodb");
        assert_eq!(body["summary"], "Dummy is the debut album.");
        assert!(body["stats"].is_null());

        let (_, body) = send(app.clone(), get("/api/artists/ar-old/about")).await;
        let body = json_body(&body);
        assert!(body["source"].is_null());
        assert_eq!(body["summary"], "");

        let (status, _) = send(app, get("/api/artists/missing/about")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        for var in [
            "BOOGIEBOX_MUSICBRAINZ_API_BASE",
            "BOOGIEBOX_WIKIDATA_API_BASE",
            "BOOGIEBOX_WIKIPEDIA_API_BASE",
            "BOOGIEBOX_THEAUDIODB_API_BASE",
            "BOOGIEBOX_LISTENBRAINZ_API_BASE",
        ] {
            std::env::remove_var(var);
        }
    }

    #[tokio::test]
    async fn artist_top_songs_come_from_listenbrainz_then_deezer_and_are_cached() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());
        let mbid = "10adbe5e-a2c0-4bf3-8249-2b4cbf6e6ca8";
        let rows: Vec<serde_json::Value> = (1..=7)
            .map(|i| {
                serde_json::json!({
                    "recording_mbid": format!("00000000-0000-4000-8000-00000000000{i}"),
                    "recording_name": format!("Song {i}"),
                    "total_listen_count": 1000 - i * 10,
                })
            })
            .collect();
        Mock::given(method("GET"))
            .and(path(format!(
                "/1/popularity/top-recordings-for-artist/{mbid}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::Value::Array(rows)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/artist/42/top"))
            .and(query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "title": "Deezer Hit", "link": "https://www.deezer.com/track/1", "rank": 900000 },
                    { "title": "Second", "link": "https://www.deezer.com/track/2", "rank": 800000 }
                ]
            })))
            .mount(&server)
            .await;

        let (app, pool) = new_test_app_with_pool("provider-top-songs");
        let cookie = seed_user_session(&pool, "u1");
        {
            let conn = pool.lock().unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO artists(id, name, musicbrainz_artist_id) VALUES ('ar-lb', 'Massive Attack', '{mbid}');
                 INSERT INTO artists(id, name, deezer_artist_id) VALUES ('ar-dz', 'Someone', '42');"
            ))
            .unwrap();
        }
        let get = |uri: &str| {
            Request::builder()
                .uri(uri)
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap()
        };

        let (status, body) = send(app.clone(), get("/api/artists/ar-lb/top-songs")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["source"], "listenbrainz");
        let tracks = body["tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 5);
        assert_eq!(tracks[0]["name"], "Song 1");
        assert_eq!(tracks[0]["listens"], 990);
        assert_eq!(
            tracks[0]["url"],
            "https://musicbrainz.org/recording/00000000-0000-4000-8000-000000000001"
        );
        // From the cache: ListenBrainz is asked once (`expect(1)`).
        let (_, again) = send(app.clone(), get("/api/artists/ar-lb/top-songs")).await;
        assert_eq!(json_body(&again)["tracks"][4]["name"], "Song 5");

        let (status, body) = send(app.clone(), get("/api/artists/ar-dz/top-songs")).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["source"], "deezer");
        assert_eq!(body["tracks"][0]["name"], "Deezer Hit");
        assert!(body["tracks"][0]["listens"].is_null());
        assert_eq!(body["tracks"][0]["url"], "https://www.deezer.com/track/1");

        let (status, _) = send(app, get("/api/artists/missing/top-songs")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_API_BASE");
        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }
}

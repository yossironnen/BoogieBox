//! Defines Rust API routes for Provider Routes server behavior.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    providers::{
        fetch_lastfm_album_info, fetch_lastfm_artist_info, fetch_lastfm_top_tracks,
        fetch_lrclib_lyrics, get_spotify_access_token, search_metadata,
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
        .route("/api/lastfm/info", get(lastfm_info_handler))
        .route("/api/lastfm/top-tracks", get(lastfm_top_tracks_handler))
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

#[derive(Debug, Deserialize)]
struct LastFmInfoParams {
    artist: Option<String>,
    album: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LastFmTopTracksParams {
    artist: Option<String>,
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

    let (lastfm_key, discogs_token, spotify_id, spotify_secret) =
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            (
                boogiebox_db::artwork::get_setting(&conn, "lastfmKey"),
                boogiebox_db::artwork::get_setting(&conn, "discogsToken"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientId"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientSecret"),
            )
        })
        .await
        .unwrap_or((None, None, None, None));

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let (results, provider_warnings) = search_metadata(
        &http_client,
        lastfm_key.as_deref(),
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

async fn lastfm_info_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<LastFmInfoParams>,
) -> impl IntoResponse {
    let artist = params.artist.as_deref().unwrap_or("").trim().to_owned();
    let album = params
        .album
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    if artist.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "artist is required" })),
        )
            .into_response();
    }

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let cache_key = if let Some(ref alb) = album {
        format!("album:{}:{}", artist.to_lowercase(), alb.to_lowercase())
    } else {
        format!("artist:{}", artist.to_lowercase())
    };

    // Check cache
    let cached = {
        let ck = cache_key.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_lastfm_cached(&conn, &ck)
        })
        .await
        .unwrap_or(None)
    };

    if let Some(data) = cached {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            return (StatusCode::OK, Json(v)).into_response();
        }
    }

    // Fetch API key
    let db2 = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };
    let api_key = tokio::task::spawn_blocking(move || {
        let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::get_setting(&conn, "lastfmKey").unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    if api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no-key" })),
        )
            .into_response();
    }

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let payload = if let Some(ref alb) = album {
        fetch_lastfm_album_info(&http_client, &api_key, &artist, alb).await
    } else {
        fetch_lastfm_artist_info(&http_client, &api_key, &artist).await
    };

    let Some(info) = payload else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Not found" })),
        )
            .into_response();
    };

    let obj = serde_json::json!({
        "summary": info.summary,
        "full": info.full,
        "listeners": info.listeners,
        "playcount": info.playcount,
        "url": info.url,
        "image": info.image,
        "tags": info.tags,
    });

    // Save to cache (7 days)
    if let Ok(data_str) = serde_json::to_string(&obj) {
        if let Some(db3) = get_db(&state) {
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::artwork::save_lastfm_cache(&conn, &cache_key, &data_str, 7);
            })
            .await;
        }
    }

    (StatusCode::OK, Json(obj)).into_response()
}

async fn lastfm_top_tracks_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(params): Query<LastFmTopTracksParams>,
) -> impl IntoResponse {
    let artist = params.artist.as_deref().unwrap_or("").trim().to_owned();
    if artist.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "artist is required" })),
        )
            .into_response();
    }

    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let cache_key = format!("toptracks:{}", artist.to_lowercase());

    let cached = {
        let ck = cache_key.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_lastfm_cached(&conn, &ck)
        })
        .await
        .unwrap_or(None)
    };

    if let Some(data) = cached {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            return (StatusCode::OK, Json(v)).into_response();
        }
    }

    let db2 = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };
    let api_key = tokio::task::spawn_blocking(move || {
        let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::get_setting(&conn, "lastfmKey").unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    if api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no-key" })),
        )
            .into_response();
    }

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    match fetch_lastfm_top_tracks(&http_client, &api_key, &artist).await {
        Ok(tracks) => {
            if let Ok(data_str) = serde_json::to_string(&tracks) {
                if let Some(db3) = get_db(&state) {
                    let _ = tokio::task::spawn_blocking(move || {
                        let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
                        boogiebox_db::artwork::save_lastfm_cache(&conn, &cache_key, &data_str, 7);
                    })
                    .await;
                }
            }
            (StatusCode::OK, Json(serde_json::json!(tracks))).into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
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
    async fn lastfm_info_requires_artist_and_returns_cached_value_without_a_network_call() {
        let (app, pool) = new_test_app_with_pool("provider-lastfm-info");
        let cookie = seed_user_session(&pool, "u1");

        let (missing_artist_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/lastfm/info")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_artist_status, StatusCode::BAD_REQUEST);

        boogiebox_db::artwork::save_lastfm_cache(
            &pool.lock().unwrap(),
            "artist:cached artist",
            r#"{"summary":"cached summary"}"#,
            7,
        );

        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/lastfm/info?artist=Cached%20Artist")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["summary"], "cached summary");
    }

    #[tokio::test]
    async fn lastfm_info_returns_bad_request_when_no_key_configured_and_cache_misses() {
        let (app, pool) = new_test_app_with_pool("provider-lastfm-no-key");
        let cookie = seed_user_session(&pool, "u1");
        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/lastfm/info?artist=Uncached%20Artist")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json_body(&body)["error"], "no-key");
    }

    #[tokio::test]
    async fn lastfm_top_tracks_requires_artist_and_returns_cached_value() {
        let (app, pool) = new_test_app_with_pool("provider-lastfm-top");
        let cookie = seed_user_session(&pool, "u1");

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/lastfm/top-tracks")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::BAD_REQUEST);

        boogiebox_db::artwork::save_lastfm_cache(
            &pool.lock().unwrap(),
            "toptracks:cached artist",
            r#"[{"name":"Song One"}]"#,
            7,
        );

        let (status, body) = send(
            app,
            Request::builder()
                .uri("/api/lastfm/top-tracks?artist=Cached%20Artist")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)[0]["name"], "Song One");
    }
}

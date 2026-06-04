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

    let (discogs_token, spotify_id, spotify_secret) = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        (
            boogiebox_db::artwork::get_setting(&conn, "discogsToken"),
            boogiebox_db::artwork::get_setting(&conn, "spotifyClientId"),
            boogiebox_db::artwork::get_setting(&conn, "spotifyClientSecret"),
        )
    })
    .await
    .unwrap_or((None, None, None));

    let http_client = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .http_client
        .clone();
    let results = search_metadata(
        &http_client,
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
        Json(serde_json::json!({ "results": results })),
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

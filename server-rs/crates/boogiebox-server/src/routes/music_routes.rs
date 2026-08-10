//! Defines Rust API routes for Music Routes server behavior.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use boogiebox_db::boogiemix::get_track_sonic_fingerprint;
use boogiebox_db::music::{
    coerce_entity_id, get_album, get_artist, get_artist_name, get_home_top_rated, get_track,
    interleave_by_artist, list_album_tracks, list_albums, list_albums_by_group_tracks,
    list_albums_latest, list_artist_albums, list_artist_appears_on, list_artist_own_random_tracks,
    list_artist_radio_candidates, list_artist_radio_tags, list_artists, list_artists_most_played,
    list_auto_dj_candidates, list_genres, list_home_genre_summaries, list_recently_played,
    list_top_played, normalize_artist_release_types, search_music, ArtistList, EntityId,
    ListAlbumsParams, ListArtistsParams, SearchMusicParams,
};
use serde::{Deserialize, Serialize};

use crate::{auth::AuthenticatedUser, DbPool, ErrorResponse, SharedState};

/// Documents the Music Router public API surface.
pub fn music_router(state: SharedState) -> Router {
    Router::new()
        // Search
        .route("/api/search", get(search_handler))
        // Genres
        .route("/api/genres", get(genres_handler))
        .route("/api/home/genres", get(home_genres_handler))
        // Home
        .route("/api/home/top-rated", get(home_top_rated_handler))
        // Artists - specific routes BEFORE parameterized ones
        .route("/api/artists/most-played", get(artists_most_played_handler))
        .route("/api/artists", get(list_artists_handler))
        .route("/api/artists/{id}", get(get_artist_handler))
        .route("/api/artists/{id}/albums", get(artist_albums_handler))
        .route(
            "/api/artists/{id}/appears-on",
            get(artist_appears_on_handler),
        )
        .route("/api/artists/{id}/radio", get(artist_radio_handler))
        .route(
            "/api/artists/{id}/release-types/resolve",
            post(resolve_artist_release_types_handler),
        )
        // Albums - specific routes BEFORE parameterized ones
        .route("/api/albums/latest", get(albums_latest_handler))
        .route(
            "/api/albums/by-group/tracks",
            get(albums_by_group_tracks_handler),
        )
        .route("/api/albums", get(list_albums_handler))
        .route("/api/albums/{id}", get(get_album_handler))
        .route("/api/albums/{id}/tracks", get(album_tracks_handler))
        // Tracks — specific sub-routes BEFORE parameterized {id}
        .route("/api/tracks/recently-played", get(recently_played_handler))
        .route("/api/tracks/top-played", get(top_played_handler))
        .route(
            "/api/tracks/{id}/sonic-fingerprint",
            get(track_sonic_fingerprint_handler),
        )
        .route("/api/tracks/{id}", get(get_track_handler))
        // Auto-DJ
        .route("/api/auto-dj/tracks", get(auto_dj_handler))
        .with_state(state)
}

// -- Query param structs -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    library_id: Option<String>,
    genre: Option<String>,
    year: Option<String>,
    format: Option<String>,
    sort: Option<String>,
    order: Option<String>,
    page: Option<String>,
    limit: Option<String>,
    search_mode: Option<String>,
    include_artists: Option<String>,
    include_albums: Option<String>,
    include_total: Option<String>,
    sonic_fingerprint_only: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LimitQuery {
    limit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistBrowseQuery {
    library_id: Option<String>,
    library_ids: Option<String>,
    genres: Option<String>,
    limit: Option<String>,
    offset: Option<String>,
    q: Option<String>,
    starts_with: Option<String>,
    order: Option<String>,
    view: Option<String>,
    paged: Option<String>,
    sonic_fingerprint_only: Option<String>,
    hide_compilation_only: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlbumBrowseQuery {
    library_id: Option<String>,
    library_ids: Option<String>,
    genres: Option<String>,
    group_by: Option<String>,
    sonic_fingerprint_only: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlbumByGroupQuery {
    title: Option<String>,
    album_artist: Option<String>,
    library_id: Option<String>,
    library_ids: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistAlbumsQuery {
    library_id: Option<String>,
    library_ids: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AutoDjQuery {
    genres: Option<String>,
    library_id: Option<String>,
    limit: Option<String>,
}

#[derive(Debug, Serialize)]
struct ArtistRadioResponse {
    artist: String,
    tags: Vec<String>,
    tracks: Vec<boogiebox_db::music::TrackRow>,
}

// -- Helpers -------------------------------------------------------------------

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn parse_limit(raw: Option<&str>, default: i64, max: i64) -> i64 {
    raw.and_then(|s| s.parse::<i64>().ok())
        .map(|n| n.clamp(1, max))
        .unwrap_or(default)
}

fn parse_genres(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return vec![] };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for part in raw.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let lower = part.to_lowercase();
        if seen.insert(lower) {
            out.push(part.to_owned());
        }
    }
    out
}

fn parse_library_ids(raw_ids: Option<&str>, raw_id: Option<&str>) -> Vec<EntityId> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for src in [raw_id, raw_ids].iter().flatten() {
        for part in src.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
            if seen.insert(part.to_owned()) {
                out.push(coerce_entity_id(part));
            }
        }
    }
    out
}

fn bad_request(msg: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn not_found(msg: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn setup_required() -> axum::response::Response {
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

// -- Search --------------------------------------------------------------------

async fn search_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };

    let query_str = q.q.as_deref().unwrap_or("").to_owned();
    let sort = q.sort.as_deref().unwrap_or("title").to_owned();
    let order = q.order.as_deref().unwrap_or("asc").to_owned();
    let page = q
        .page
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .clamp(1, 200);
    let search_mode = q.search_mode.as_deref().unwrap_or("default");
    let is_mobile_tracks = search_mode == "mobile_tracks";
    let include_artists = q
        .include_artists
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(!is_mobile_tracks);
    let include_albums = q
        .include_albums
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(!is_mobile_tracks);
    let include_total = q
        .include_total
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(!is_mobile_tracks);
    let library_id = q.library_id.as_deref().map(|s| {
        s.parse::<i64>()
            .map(EntityId::Int)
            .unwrap_or_else(|_| EntityId::Str(s.to_owned()))
    });
    let genre = q.genre.clone();
    let year: Option<i64> = q.year.as_deref().and_then(|s| s.parse().ok());
    let format = q.format.clone();
    let sonic_fingerprint_only = q
        .sonic_fingerprint_only
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let user_id = user.id.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        search_music(
            &conn,
            SearchMusicParams {
                user_id: &user_id,
                q: &query_str,
                library_id,
                genre: genre.as_deref(),
                year,
                format: format.as_deref(),
                sort: &sort,
                order: &order,
                page,
                limit,
                include_artists,
                include_albums,
                include_total,
                mobile_tracks_mode: is_mobile_tracks,
                sonic_fingerprint_only,
            },
        )
    })
    .await;

    match result {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        _ => internal_error(),
    }
}

// -- Genres --------------------------------------------------------------------

async fn genres_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    match tokio::task::spawn_blocking(move || list_genres(&db.lock().expect("db"))).await {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn home_genres_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 12, 12) as usize;
    match tokio::task::spawn_blocking(move || {
        list_home_genre_summaries(&db.lock().expect("db"), limit)
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

// -- Home top-rated ------------------------------------------------------------

async fn home_top_rated_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 5, 500);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        get_home_top_rated(&db.lock().expect("db"), &user_id, limit)
    })
    .await
    {
        Ok(Ok(result)) => (StatusCode::OK, Json(result)).into_response(),
        _ => internal_error(),
    }
}

// -- Artists -------------------------------------------------------------------

async fn list_artists_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<ArtistBrowseQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };

    let genres = parse_genres(q.genres.as_deref());
    let library_ids = parse_library_ids(q.library_ids.as_deref(), q.library_id.as_deref());
    let page_limit: Option<i64> = match q.limit.as_deref() {
        Some(s) => match s.parse::<i64>() {
            Ok(n) if (1..=500).contains(&n) => Some(n),
            Ok(_) => return bad_request("limit must be between 1 and 500"),
            Err(_) => return bad_request("limit must be an integer"),
        },
        None => None,
    };
    let page_offset: i64 = match q.offset.as_deref() {
        Some(s) => match s.parse::<i64>() {
            Ok(n) if n >= 0 => n,
            _ => return bad_request("offset must be a non-negative integer"),
        },
        None => 0,
    };
    let order_dir = match q.order.as_deref().unwrap_or("asc") {
        "asc" => "ASC",
        "desc" => "DESC",
        _ => return bad_request("order must be asc or desc"),
    };
    let full_view = match q.view.as_deref().unwrap_or("full") {
        "full" => true,
        "summary" => false,
        _ => return bad_request("view must be full or summary"),
    };
    let starts_with: Option<String> = match q.starts_with.as_deref() {
        Some(s) => {
            let n = s.trim().to_uppercase();
            if n.len() != 1 || !n.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                return bad_request("starts_with must be a single letter A-Z");
            }
            Some(n)
        }
        None => None,
    };
    let name_query: Option<String> = match q.q.as_deref() {
        Some(s) => {
            let t = s.trim().to_owned();
            if t.is_empty() {
                None
            } else if t.len() > 120 {
                return bad_request("q must be 120 characters or fewer");
            } else {
                Some(t)
            }
        }
        None => None,
    };
    let wants_paged = q
        .paged
        .as_deref()
        .is_some_and(|v| matches!(v, "1" | "true" | "yes"))
        || page_limit.is_some()
        || page_offset > 0;
    let sonic_fingerprint_only = q
        .sonic_fingerprint_only
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let hide_compilation_only = q
        .hide_compilation_only
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);

    let user_id = user.id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        list_artists(
            &conn,
            ListArtistsParams {
                user_id: &user_id,
                genres: &genres,
                library_ids: &library_ids,
                starts_with: starts_with.as_deref(),
                name_query: name_query.as_deref(),
                order_dir,
                full_view,
                page_limit: if wants_paged {
                    page_limit.or(Some(100))
                } else {
                    page_limit
                },
                page_offset,
                sonic_fingerprint_only,
                hide_compilation_only,
            },
        )
    })
    .await;

    match result {
        Ok(Ok(ArtistList::Page(page))) => (StatusCode::OK, Json(page)).into_response(),
        Ok(Ok(ArtistList::All(rows))) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn get_artist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        get_artist(&db.lock().expect("db"), &user_id, &artist_id)
    })
    .await
    {
        Ok(Ok(Some(artist))) => (StatusCode::OK, Json(artist)).into_response(),
        Ok(Ok(None)) => not_found("Artist not found"),
        _ => internal_error(),
    }
}

async fn artists_most_played_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 10, 500);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_artists_most_played(&db.lock().expect("db"), &user_id, limit)
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn artist_albums_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<ArtistAlbumsQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    let library_ids = parse_library_ids(q.library_ids.as_deref(), q.library_id.as_deref());
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_artist_albums(&db.lock().expect("db"), &user_id, &artist_id, &library_ids)
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn artist_appears_on_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<ArtistAlbumsQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    let library_ids = parse_library_ids(q.library_ids.as_deref(), q.library_id.as_deref());
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        let artist_name = conn
            .query_row(
                "SELECT name FROM artists WHERE id=?",
                rusqlite::params![&artist_id],
                |r| r.get::<_, String>(0),
            )
            .ok();
        match artist_name {
            None => Ok(None),
            Some(name) => {
                list_artist_appears_on(&conn, &user_id, &artist_id, &name, &library_ids).map(Some)
            }
        }
    })
    .await
    {
        Ok(Ok(Some(rows))) => (StatusCode::OK, Json(rows)).into_response(),
        Ok(Ok(None)) => not_found("Artist not found"),
        _ => internal_error(),
    }
}

// -- Albums --------------------------------------------------------------------

async fn artist_radio_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    let limit = parse_limit(q.limit.as_deref(), 100, 300).max(10);

    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        let Some(artist) = get_artist_name(&conn, &artist_id)? else {
            return Ok::<_, rusqlite::Error>(None);
        };
        let tags = list_artist_radio_tags(&conn, &artist_id)?;
        if tags.is_empty() {
            return Ok(Some(Err(
                "No saved style tags for this artist yet. Run a scan after configuring Last.fm key."
                    .to_string(),
            )));
        }

        let mut tracks = list_artist_radio_candidates(&conn, &artist_id, tags.len(), limit)?;
        if tracks.len() < std::cmp::min(20, (limit / 2) as usize) {
            let fill_limit = limit - tracks.len() as i64;
            if fill_limit > 0 {
                tracks.extend(list_artist_own_random_tracks(
                    &conn, &artist_id, fill_limit,
                )?);
            }
        }
        let tracks = interleave_by_artist(tracks, limit as usize);
        Ok(Some(Ok(ArtistRadioResponse {
            artist,
            tags,
            tracks,
        })))
    })
    .await
    {
        Ok(Ok(Some(Ok(body)))) => (StatusCode::OK, Json(body)).into_response(),
        Ok(Ok(Some(Err(message)))) => not_found(message),
        Ok(Ok(None)) => not_found("Artist not found"),
        _ => internal_error(),
    }
}

async fn resolve_artist_release_types_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);

    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        if get_artist_name(&conn, &artist_id)?.is_none() {
            return Ok::<_, rusqlite::Error>(None);
        }
        let updated = normalize_artist_release_types(&conn, &artist_id)?;
        Ok(Some(updated))
    })
    .await
    {
        Ok(Ok(Some(updated))) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "updated": updated })),
        )
            .into_response(),
        Ok(Ok(None)) => not_found("Artist not found"),
        _ => internal_error(),
    }
}

async fn list_albums_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<AlbumBrowseQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let genres = parse_genres(q.genres.as_deref());
    let library_ids = parse_library_ids(q.library_ids.as_deref(), q.library_id.as_deref());
    let by_album_artist = q.group_by.as_deref() == Some("album_artist");
    let sonic_fingerprint_only = q
        .sonic_fingerprint_only
        .as_deref()
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_albums(
            &db.lock().expect("db"),
            ListAlbumsParams {
                user_id: &user_id,
                library_ids: &library_ids,
                genres: &genres,
                by_album_artist,
                sonic_fingerprint_only,
            },
        )
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn albums_latest_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 60, 200);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_albums_latest(&db.lock().expect("db"), &user_id, limit)
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn albums_by_group_tracks_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Query(q): Query<AlbumByGroupQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let title = match q.title.as_deref() {
        None | Some("") => return bad_request("title required"),
        Some(t) if t.len() > 500 => return bad_request("title must be 500 characters or fewer"),
        Some(t) => t.to_owned(),
    };
    let album_artist = q.album_artist.as_deref().unwrap_or("").to_owned();
    let library_ids = parse_library_ids(q.library_ids.as_deref(), q.library_id.as_deref());
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_albums_by_group_tracks(
            &db.lock().expect("db"),
            &user_id,
            &title,
            &album_artist,
            &library_ids,
        )
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn get_album_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let album_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        get_album(&db.lock().expect("db"), &user_id, &album_id)
    })
    .await
    {
        Ok(Ok(Some(album))) => (StatusCode::OK, Json(album)).into_response(),
        Ok(Ok(None)) => not_found("Album not found"),
        _ => internal_error(),
    }
}

async fn album_tracks_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let album_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        list_album_tracks(&db.lock().expect("db"), &user_id, &album_id)
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

// -- Tracks --------------------------------------------------------------------

async fn recently_played_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 500, 500);
    match tokio::task::spawn_blocking(move || list_recently_played(&db.lock().expect("db"), limit))
        .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn top_played_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let limit = parse_limit(q.limit.as_deref(), 10, 500);
    match tokio::task::spawn_blocking(move || list_top_played(&db.lock().expect("db"), limit)).await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn get_track_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let track_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        get_track(&db.lock().expect("db"), &user_id, &track_id)
    })
    .await
    {
        Ok(Ok(Some(track))) => (StatusCode::OK, Json(track)).into_response(),
        Ok(Ok(None)) => not_found("Not found"),
        _ => internal_error(),
    }
}

async fn track_sonic_fingerprint_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    let track_id = coerce_entity_id(&id);
    match tokio::task::spawn_blocking(move || {
        get_track_sonic_fingerprint(&db.lock().expect("db"), &track_id)
    })
    .await
    {
        Ok(Ok(Some(fp))) => (StatusCode::OK, Json(fp)).into_response(),
        Ok(Ok(None)) => not_found("not_analyzed"),
        _ => internal_error(),
    }
}

// -- Auto-DJ -------------------------------------------------------------------

async fn auto_dj_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Query(q): Query<AutoDjQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };

    let genres = parse_genres(q.genres.as_deref());
    let library_id = q
        .library_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(coerce_entity_id);
    if genres.is_empty() && library_id.is_none() {
        return bad_request("At least one genre is required");
    }

    let limit = parse_limit(q.limit.as_deref(), 200, 500);
    let candidate_limit = (limit * 4).max(80);

    match tokio::task::spawn_blocking(move || {
        let candidates = list_auto_dj_candidates(
            &db.lock().expect("db"),
            &genres,
            library_id.as_ref(),
            candidate_limit,
        )?;
        Ok::<_, rusqlite::Error>(interleave_by_artist(candidates, limit as usize))
    })
    .await
    {
        Ok(Ok(tracks)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "tracks": tracks })),
        )
            .into_response(),
        _ => internal_error(),
    }
}

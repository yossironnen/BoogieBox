//! Defines Rust API routes for Music Routes server behavior.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use boogiebox_db::artwork::{
    get_lastfm_cached, get_lastfm_cached_stale, get_setting, save_lastfm_cache,
};
use boogiebox_db::boogiemix::get_track_sonic_fingerprint;
use boogiebox_db::music::{
    album_change_cursor, coerce_entity_id, get_album, get_artist, get_artist_external_identity,
    get_artist_merge_info, get_artist_name, get_home_top_rated, get_track, interleave_by_artist,
    list_album_tracks, list_albums, list_albums_by_group_tracks, list_albums_latest,
    list_artist_albums, list_artist_appears_on, list_artist_own_random_tracks,
    list_artist_radio_candidates, list_artist_radio_tags, list_artists, list_artists_most_played,
    list_auto_dj_candidates, list_genres, list_home_genre_summaries, list_recently_played,
    list_top_played, lock_artist_identity, merge_artists, normalize_artist_release_types,
    search_music, unmerge_artists, update_track_metadata, ArtistList, ArtistMergeError, EntityId,
    ListAlbumsParams, ListArtistsParams, SearchMusicParams, TrackMetadataUpdate,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::AuthenticatedUser,
    providers::{
        fetch_deezer_related_artists, fetch_lastfm_similar_artists, RelatedArtistCandidate,
    },
    similar_artists::{resolve_local_similar_artists, SimilarArtistResult},
    DbPool, ErrorResponse, SharedState,
};

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
        .route("/api/artists/merge", post(merge_artists_handler))
        .route("/api/artists", get(list_artists_handler))
        .route("/api/artists/{id}", get(get_artist_handler))
        .route("/api/artists/{id}/similar", get(similar_artists_handler))
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
        .route(
            "/api/artists/{id}/merge-info",
            get(artist_merge_info_handler),
        )
        .route("/api/artists/{id}/unmerge", post(unmerge_artist_handler))
        .route(
            "/api/artists/{id}/lock-identity",
            post(lock_artist_identity_handler),
        )
        // Albums - specific routes BEFORE parameterized ones
        .route("/api/albums/latest", get(albums_latest_handler))
        .route(
            "/api/albums/by-group/tracks",
            get(albums_by_group_tracks_handler),
        )
        .route(
            "/api/albums/change-cursor",
            get(album_change_cursor_handler),
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
        .route("/api/tracks/{id}/metadata", put(track_metadata_handler))
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimilarArtistsResponse {
    source_artist_id: EntityId,
    artists: Vec<SimilarArtistResult>,
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
    after_album_rowid: Option<u64>,
    through_album_rowid: Option<u64>,
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

fn forbidden(msg: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

/// Admins, and users granted the "Allow metadata editing" permission, may edit
/// track/album/artist tag metadata. Mirrors the same check in
/// `playlist_routes.rs` (album/artist metadata edit) — kept as a local copy
/// since each route module owns its small helpers here (see `get_db`,
/// `setup_required`, etc. above).
fn can_edit_metadata(user: &AuthenticatedUser) -> bool {
    user.is_admin() || user.can_edit_metadata
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

fn conflict(msg: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: msg.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

/// Maps artist merge/unmerge validation errors to HTTP responses. Matches
/// the variant directly (not its rendered `Display` text) — see the
/// `boogiemix_routes.rs` bug this pattern was adopted to avoid.
fn map_merge_error(err: ArtistMergeError) -> axum::response::Response {
    match err {
        ArtistMergeError::TooFewArtists
        | ArtistMergeError::EmptyName
        | ArtistMergeError::VariousArtistsNotMergeable
        | ArtistMergeError::InvalidMaster
        | ArtistMergeError::NoMembersSelected
        | ArtistMergeError::IdentityNotPending => bad_request(err.to_string()),
        ArtistMergeError::ArtistNotFound | ArtistMergeError::NotAMergeMaster => {
            not_found(err.to_string())
        }
        ArtistMergeError::AlreadyMerged => conflict(err.to_string()),
        ArtistMergeError::Db(_) => internal_error(),
    }
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

// -- Artist consolidation (merge/unmerge) --------------------------------------
// See wip/artist-consolidation-implementation-plan.md.

#[derive(Debug, Deserialize)]
struct MergeArtistsPayload {
    artist_ids: Vec<String>,
    master_name: String,
    master_artist_id: Option<String>,
}

/// `POST /api/artists/merge` — merges 2+ artist rows into one. Requires
/// "Allow metadata editing" (or admin).
async fn merge_artists_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<MergeArtistsPayload>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return forbidden("Forbidden");
    }
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let artist_ids: Vec<EntityId> = payload
        .artist_ids
        .iter()
        .map(|id| coerce_entity_id(id))
        .collect();
    let master_artist_id = payload.master_artist_id.as_deref().map(coerce_entity_id);
    let master_name = payload.master_name;
    let acting_user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        merge_artists(
            &db.lock().expect("db"),
            &artist_ids,
            &master_name,
            master_artist_id.as_ref(),
            &acting_user_id,
        )
    })
    .await
    {
        Ok(Ok(artist)) => (StatusCode::OK, Json(artist)).into_response(),
        Ok(Err(e)) => map_merge_error(e),
        Err(_) => internal_error(),
    }
}

/// `GET /api/artists/{id}/merge-info` — whether `id` is a merge master and,
/// if so, which names were absorbed into it. Powers the "Merged from" panel
/// and the unmerge modal's member checklist. No permission gate — read-only.
async fn artist_merge_info_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    match tokio::task::spawn_blocking(move || {
        get_artist_merge_info(&db.lock().expect("db"), &artist_id)
    })
    .await
    {
        Ok(Ok(info)) => (StatusCode::OK, Json(info)).into_response(),
        _ => internal_error(),
    }
}

#[derive(Debug, Deserialize)]
struct UnmergePayload {
    member_ids: Vec<String>,
}

/// `POST /api/artists/{id}/unmerge` — splits the given merge members back
/// out into their own artist rows. Requires "Allow metadata editing" (or
/// admin).
async fn unmerge_artist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<UnmergePayload>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return forbidden("Forbidden");
    }
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let master_id = coerce_entity_id(&id);
    let member_ids: Vec<EntityId> = payload
        .member_ids
        .iter()
        .map(|id| coerce_entity_id(id))
        .collect();
    let acting_user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        unmerge_artists(
            &db.lock().expect("db"),
            &master_id,
            &member_ids,
            &acting_user_id,
        )
    })
    .await
    {
        Ok(Ok(result)) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Err(e)) => map_merge_error(e),
        Err(_) => internal_error(),
    }
}

/// `POST /api/artists/{id}/lock-identity` — manually locks a merge master
/// still waiting on its post-merge online identity match (§6.5's "Lock
/// now"). Requires "Allow metadata editing" (or admin).
async fn lock_artist_identity_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return forbidden("Forbidden");
    }
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let artist_id = coerce_entity_id(&id);
    let acting_user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        lock_artist_identity(&db.lock().expect("db"), &artist_id, &acting_user_id)
    })
    .await
    {
        Ok(Ok(artist)) => (StatusCode::OK, Json(artist)).into_response(),
        Ok(Err(e)) => map_merge_error(e),
        Err(_) => internal_error(),
    }
}

fn decode_related_cache(payload: Option<String>) -> Option<Vec<RelatedArtistCandidate>> {
    payload.and_then(|raw| serde_json::from_str(&raw).ok())
}

async fn similar_artists_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<LimitQuery>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let http_client = state
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .http_client
        .clone();
    let artist_id = coerce_entity_id(&id);
    let limit = parse_limit(q.limit.as_deref(), 12, 50) as usize;
    let context_db = db.clone();
    let source_id = artist_id.clone();
    let context = tokio::task::spawn_blocking(move || {
        let conn = context_db.lock().expect("db");
        let Some(identity) = get_artist_external_identity(&conn, &source_id)? else {
            return Ok::<_, rusqlite::Error>(None);
        };
        let source_key = identity.artist_id.to_string();
        let lastfm_cache_key = format!("artist-similar:lastfm:{source_key}");
        let deezer_cache_key = format!("artist-similar:deezer:{source_key}");
        Ok(Some((
            identity,
            get_setting(&conn, "lastfmKey"),
            lastfm_cache_key.clone(),
            get_lastfm_cached(&conn, &lastfm_cache_key),
            get_lastfm_cached_stale(&conn, &lastfm_cache_key),
            deezer_cache_key.clone(),
            get_lastfm_cached(&conn, &deezer_cache_key),
            get_lastfm_cached_stale(&conn, &deezer_cache_key),
        )))
    })
    .await;
    let Some((
        identity,
        lastfm_key,
        lastfm_cache_key,
        lastfm_fresh,
        lastfm_stale,
        deezer_cache_key,
        deezer_fresh,
        deezer_stale,
    )) = (match context {
        Ok(Ok(value)) => value,
        _ => return internal_error(),
    })
    else {
        return not_found("Artist not found");
    };

    let mut cache_updates: Vec<(String, String)> = Vec::new();
    let lastfm = if let Some(cached) = decode_related_cache(lastfm_fresh) {
        cached
    } else if let Some(api_key) = lastfm_key.as_deref() {
        match fetch_lastfm_similar_artists(
            &http_client,
            api_key,
            &identity.name,
            identity.lastfm_mbid.as_deref(),
            100,
        )
        .await
        {
            Ok(candidates) => {
                if let Ok(payload) = serde_json::to_string(&candidates) {
                    cache_updates.push((lastfm_cache_key, payload));
                }
                candidates
            }
            Err(_) => decode_related_cache(lastfm_stale).unwrap_or_default(),
        }
    } else {
        decode_related_cache(lastfm_stale).unwrap_or_default()
    };

    let deezer = if let Some(cached) = decode_related_cache(deezer_fresh) {
        cached
    } else if let Some(deezer_id) = identity.deezer_artist_id.as_deref() {
        match fetch_deezer_related_artists(&http_client, deezer_id, 100).await {
            Ok(candidates) => {
                if let Ok(payload) = serde_json::to_string(&candidates) {
                    cache_updates.push((deezer_cache_key, payload));
                }
                candidates
            }
            Err(_) => decode_related_cache(deezer_stale).unwrap_or_default(),
        }
    } else {
        decode_related_cache(deezer_stale).unwrap_or_default()
    };

    if !cache_updates.is_empty() {
        let cache_db = db.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = cache_db.lock().expect("db");
            for (key, payload) in cache_updates {
                save_lastfm_cache(&conn, &key, &payload, 7);
            }
        })
        .await;
    }

    let user_id = user.id;
    let response_source_id = artist_id.clone();
    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        resolve_local_similar_artists(&conn, &user_id, &artist_id, &lastfm, &deezer, limit)
    })
    .await
    {
        Ok(Ok(artists)) => (
            StatusCode::OK,
            Json(SimilarArtistsResponse {
                source_artist_id: response_source_id,
                artists,
            }),
        )
            .into_response(),
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
    let after_album_rowid = q
        .after_album_rowid
        .map(|value| value.min(i64::MAX as u64) as i64);
    let through_album_rowid = q
        .through_album_rowid
        .map(|value| value.min(i64::MAX as u64) as i64);
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
                after_album_rowid,
                through_album_rowid,
            },
        )
    })
    .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn album_change_cursor_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required(),
    };
    match tokio::task::spawn_blocking(move || album_change_cursor(&db.lock().expect("db"))).await {
        Ok(Ok(cursor)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "cursor": cursor })),
        )
            .into_response(),
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackMetadataPayload {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
    composer: Option<String>,
    comment: Option<String>,
    track_number: Option<serde_json::Value>,
    disc_number: Option<serde_json::Value>,
    year: Option<serde_json::Value>,
}

fn parse_metadata_int(value: Option<serde_json::Value>) -> Option<i64> {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_i64(),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `PUT /api/tracks/{id}/metadata` — edits a track's tag metadata (title, artist,
/// album, genre, composer, comment, track/disc number, year) from the Track Info
/// popup. Database-only: never touches the source file's own tags. Renaming
/// artist/album resolves-or-creates the target row the same way the scanner
/// does (see `update_track_metadata`).
async fn track_metadata_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<TrackMetadataPayload>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return forbidden("Forbidden");
    }
    if let Some(title) = &payload.title {
        if title.trim().is_empty() {
            return bad_request("Title is required");
        }
    }
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let track_id = coerce_entity_id(&id);
    let update = TrackMetadataUpdate {
        title: payload.title,
        artist: payload.artist,
        album: payload.album,
        genre: payload.genre,
        composer: payload.composer,
        comment: payload.comment,
        track_number: parse_metadata_int(payload.track_number),
        disc_number: parse_metadata_int(payload.disc_number),
        year: parse_metadata_int(payload.year),
    };
    match tokio::task::spawn_blocking(move || {
        update_track_metadata(&db.lock().expect("db"), &track_id, update)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => not_found("Track not found"),
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

#[cfg(test)]
mod similar_artist_route_tests {
    use super::*;
    use crate::AppState;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use boogiebox_db::{artwork::save_lastfm_cache, initialize_schema};
    use rusqlite::Connection;
    use serde_json::Value;
    use std::sync::{Arc, Mutex, RwLock};
    use tower::ServiceExt;

    fn candidate(external_id: &str, name: &str, rank: usize) -> RelatedArtistCandidate {
        RelatedArtistCandidate {
            external_id: Some(external_id.to_owned()),
            name: name.to_owned(),
            url: None,
            image_url: None,
            match_score: Some(0.9),
            rank,
        }
    }

    fn test_app() -> axum::Router {
        let conn = Connection::open_in_memory().expect("memory db");
        initialize_schema(&conn).expect("schema");
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES('user-1', 'user');
             INSERT INTO sessions(token, user_id, expires_at)
               VALUES('session-1', 'user-1', datetime('now', '+1 day'));
             INSERT INTO artists(id, name, lastfm_mbid, deezer_artist_id) VALUES
               ('source', 'Source', 'source-mbid', 'source-deezer'),
               ('related', 'Related', 'related-mbid', 'related-deezer'),
               ('unowned', 'Unowned', 'unowned-mbid', 'unowned-deezer');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('source-album', 'Source Album', 'Source', 'source'),
               ('related-album', 'Related Album', 'Related', 'related');",
        )
        .expect("fixtures");
        let lastfm = serde_json::to_string(&vec![
            candidate("related-mbid", "Related", 1),
            candidate("unowned-mbid", "Unowned", 2),
        ])
        .expect("lastfm cache");
        let deezer = serde_json::to_string(&vec![candidate("related-deezer", "Related", 1)])
            .expect("deezer cache");
        save_lastfm_cache(&conn, "artist-similar:lastfm:source", &lastfm, 7);
        save_lastfm_cache(&conn, "artist-similar:deezer:source", &deezer, 7);

        let shared = Arc::new(RwLock::new(AppState {
            setup_required: false,
            db: Some(Arc::new(Mutex::new(conn))),
            ..AppState::default()
        }));
        music_router(shared)
    }

    #[tokio::test]
    async fn similar_artists_route_requires_authentication() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/artists/source/similar")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn album_change_cursor_route_returns_authenticated_snapshot() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/albums/change-cursor")
                    .header("cookie", "bb_session=session-1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["cursor"], 2);
    }

    #[tokio::test]
    async fn similar_artists_route_returns_only_owned_cached_matches() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/artists/source/similar?limit=1")
                    .header("cookie", "bb_session=session-1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["sourceArtistId"], "source");
        assert_eq!(json["artists"].as_array().expect("artists").len(), 1);
        assert_eq!(json["artists"][0]["id"], "related");
        assert_eq!(
            json["artists"][0]["providers"],
            serde_json::json!(["lastfm", "deezer"])
        );
    }
}

#[cfg(test)]
mod browse_route_tests {
    use crate::test_support::{json_body, new_test_app_with_pool, seed_user_session, send};
    use crate::DbPool;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rusqlite::params;
    use uuid::Uuid;

    /// Seeds one library/artist/album/track with a genre tag, returning their ids.
    fn seed_music(pool: &DbPool) -> (String, String, String) {
        let conn = pool.lock().unwrap();
        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, '/music', 'Lib')",
            params![library_id],
        )
        .unwrap();
        let artist_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Artist')",
            params![artist_id],
        )
        .unwrap();
        let album_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO albums(id, title, artist_id, album_artist, genre) VALUES (?, 'Album', ?, 'Artist', 'Rock')",
            params![album_id, artist_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, genre, album_artist) \
             VALUES (?, ?, ?, ?, 'Track', ?, 'Rock', 'Artist')",
            params![
                track_id,
                library_id,
                artist_id,
                album_id,
                format!("/music/{track_id}.mp3")
            ],
        )
        .unwrap();
        (artist_id, album_id, track_id)
    }

    #[tokio::test]
    async fn search_and_browse_routes_require_authentication() {
        let (app, _pool) = new_test_app_with_pool("music-auth");
        for path in ["/api/search", "/api/genres", "/api/artists", "/api/albums"] {
            let (status, _) = send(
                app.clone(),
                Request::builder().uri(path).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn browse_and_lookup_routes_cover_the_seeded_music() {
        let (app, pool) = new_test_app_with_pool("music-browse");
        let cookie = seed_user_session(&pool, "u1");
        let (artist_id, album_id, track_id) = seed_music(&pool);

        let get = |path: String, app: axum::Router, cookie: String| async move {
            send(
                app,
                Request::builder()
                    .uri(path)
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
        };

        let (search_status, search_body) =
            get("/api/search?q=Track".into(), app.clone(), cookie.clone()).await;
        assert_eq!(search_status, StatusCode::OK);
        assert!(search_body.len() > 2);

        let (genres_status, _) = get("/api/genres".into(), app.clone(), cookie.clone()).await;
        assert_eq!(genres_status, StatusCode::OK);

        let (home_genres_status, _) =
            get("/api/home/genres".into(), app.clone(), cookie.clone()).await;
        assert_eq!(home_genres_status, StatusCode::OK);

        let (home_top_status, _) =
            get("/api/home/top-rated".into(), app.clone(), cookie.clone()).await;
        assert_eq!(home_top_status, StatusCode::OK);

        let (artists_status, artists_body) =
            get("/api/artists".into(), app.clone(), cookie.clone()).await;
        assert_eq!(artists_status, StatusCode::OK);
        assert_eq!(json_body(&artists_body).as_array().unwrap().len(), 1);

        let (most_played_status, _) = get(
            "/api/artists/most-played".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(most_played_status, StatusCode::OK);

        let (artist_status, artist_body) = get(
            format!("/api/artists/{artist_id}"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(artist_status, StatusCode::OK);
        assert_eq!(json_body(&artist_body)["name"], "Artist");

        let (artist_missing_status, _) = get(
            "/api/artists/does-not-exist".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(artist_missing_status, StatusCode::NOT_FOUND);

        let (artist_albums_status, artist_albums_body) = get(
            format!("/api/artists/{artist_id}/albums"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(artist_albums_status, StatusCode::OK);
        assert_eq!(json_body(&artist_albums_body).as_array().unwrap().len(), 1);

        let (appears_on_status, _) = get(
            format!("/api/artists/{artist_id}/appears-on"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(appears_on_status, StatusCode::OK);

        let (appears_on_missing_status, _) = get(
            "/api/artists/does-not-exist/appears-on".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(appears_on_missing_status, StatusCode::NOT_FOUND);

        // No saved Last.fm style tags yet -> radio 404s with a helpful message rather
        // than 500ing.
        let (radio_status, _) = get(
            format!("/api/artists/{artist_id}/radio"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(radio_status, StatusCode::NOT_FOUND);

        let (release_types_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{artist_id}/release-types/resolve"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(release_types_status, StatusCode::OK);

        let (albums_status, albums_body) =
            get("/api/albums".into(), app.clone(), cookie.clone()).await;
        assert_eq!(albums_status, StatusCode::OK);
        assert_eq!(json_body(&albums_body).as_array().unwrap().len(), 1);

        let (albums_latest_status, _) =
            get("/api/albums/latest".into(), app.clone(), cookie.clone()).await;
        assert_eq!(albums_latest_status, StatusCode::OK);

        let (cursor_status, _) = get(
            "/api/albums/change-cursor".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(cursor_status, StatusCode::OK);

        let (by_group_status, by_group_body) = get(
            "/api/albums/by-group/tracks?title=Album&album_artist=Artist".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(by_group_status, StatusCode::OK);
        assert_eq!(json_body(&by_group_body).as_array().unwrap().len(), 1);

        let (by_group_missing_title_status, _) = get(
            "/api/albums/by-group/tracks".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(by_group_missing_title_status, StatusCode::BAD_REQUEST);

        let (album_status, album_body) = get(
            format!("/api/albums/{album_id}"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(album_status, StatusCode::OK);
        assert_eq!(json_body(&album_body)["title"], "Album");

        let (album_missing_status, _) = get(
            "/api/albums/does-not-exist".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(album_missing_status, StatusCode::NOT_FOUND);

        let (album_tracks_status, album_tracks_body) = get(
            format!("/api/albums/{album_id}/tracks"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(album_tracks_status, StatusCode::OK);
        assert_eq!(json_body(&album_tracks_body).as_array().unwrap().len(), 1);

        let (recently_played_status, _) = get(
            "/api/tracks/recently-played".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(recently_played_status, StatusCode::OK);

        let (top_played_status, _) =
            get("/api/tracks/top-played".into(), app.clone(), cookie.clone()).await;
        assert_eq!(top_played_status, StatusCode::OK);

        let (track_status, track_body) = get(
            format!("/api/tracks/{track_id}"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(track_status, StatusCode::OK);
        assert_eq!(json_body(&track_body)["title"], "Track");

        let (track_missing_status, _) = get(
            "/api/tracks/does-not-exist".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(track_missing_status, StatusCode::NOT_FOUND);

        // Not analyzed yet -> 404, not 500.
        let (fingerprint_status, _) = get(
            format!("/api/tracks/{track_id}/sonic-fingerprint"),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(fingerprint_status, StatusCode::NOT_FOUND);

        let (auto_dj_missing_genre_status, _) =
            get("/api/auto-dj/tracks".into(), app.clone(), cookie.clone()).await;
        assert_eq!(auto_dj_missing_genre_status, StatusCode::BAD_REQUEST);

        let (auto_dj_status, auto_dj_body) = get(
            "/api/auto-dj/tracks?genres=Rock".into(),
            app.clone(),
            cookie.clone(),
        )
        .await;
        assert_eq!(auto_dj_status, StatusCode::OK);
        assert_eq!(
            json_body(&auto_dj_body)["tracks"].as_array().unwrap().len(),
            1
        );

        // Track metadata edit requires the edit-metadata permission.
        let (metadata_forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/tracks/{track_id}/metadata"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"New Title"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(metadata_forbidden_status, StatusCode::FORBIDDEN);

        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 1 WHERE id = 'u1'", [])
            .unwrap();

        let (metadata_status, metadata_body) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/tracks/{track_id}/metadata"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"New Title"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(metadata_status, StatusCode::OK);
        assert_eq!(json_body(&metadata_body)["ok"], true);

        let updated_title: String = pool
            .lock()
            .unwrap()
            .query_row(
                "SELECT title FROM tracks WHERE id = ?",
                params![track_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(updated_title, "New Title");

        let (metadata_bad_title_status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri(format!("/api/tracks/{track_id}/metadata"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"  "}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(metadata_bad_title_status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_artists_validates_query_params() {
        let (app, pool) = new_test_app_with_pool("music-artists-validate");
        let cookie = seed_user_session(&pool, "u1");

        for (query, label) in [
            ("limit=0", "limit out of range"),
            ("offset=-1", "negative offset"),
            ("order=sideways", "bad order"),
            ("view=huge", "bad view"),
            ("starts_with=AB", "multi-char starts_with"),
        ] {
            let (status, _) = send(
                app.clone(),
                Request::builder()
                    .uri(format!("/api/artists?{query}"))
                    .header("cookie", cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{label}");
        }
    }

    // -- Artist consolidation (merge/unmerge) routes -----------------------
    // See wip/artist-consolidation-implementation-plan.md.

    /// Seeds a library (so a merge has somewhere to scope its post-merge
    /// enrichment job to — see §6.5) plus two duplicate artist rows.
    fn seed_duplicate_artists(pool: &DbPool) -> (String, String) {
        let conn = pool.lock().unwrap();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES ('lib-merge-test', '/music-merge', 'Music')",
            [],
        )
        .unwrap();
        let a1 = Uuid::now_v7().to_string();
        let a2 = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Madonna')",
            params![a1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Madonna Ciccone')",
            params![a2],
        )
        .unwrap();
        (a1, a2)
    }

    #[tokio::test]
    async fn merge_artists_route_requires_edit_metadata_permission() {
        let (app, pool) = new_test_app_with_pool("music-merge-forbidden");
        let cookie = seed_user_session(&pool, "u1");
        let (a1, a2) = seed_duplicate_artists(&pool);

        let (status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/artists/merge")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"artist_ids":["{a1}","{a2}"],"master_name":"Madonna","master_artist_id":"{a1}"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn merge_artists_route_merges_and_powers_merge_info_and_unmerge() {
        let (app, pool) = new_test_app_with_pool("music-merge-happy");
        let cookie = seed_user_session(&pool, "u1");
        let (a1, a2) = seed_duplicate_artists(&pool);
        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 1 WHERE id = 'u1'", [])
            .unwrap();

        let (merge_status, merge_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/artists/merge")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"artist_ids":["{a1}","{a2}"],"master_name":"Madonna","master_artist_id":"{a1}"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(merge_status, StatusCode::OK);
        let merged = json_body(&merge_body);
        assert_eq!(merged["name"], "Madonna");
        assert_eq!(merged["album_count"], 0);

        let (info_status, info_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/artists/{a1}/merge-info"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(info_status, StatusCode::OK);
        let info = json_body(&info_body);
        assert_eq!(info["merged"], true);
        assert_eq!(info["members"].as_array().unwrap().len(), 1);
        let member_id = info["members"][0]["id"].as_str().unwrap().to_string();

        // Fewer than 2 artist_ids is rejected with 400.
        let (bad_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/artists/merge")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"artist_ids":["{a1}"],"master_name":"Madonna","master_artist_id":"{a1}"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(bad_status, StatusCode::BAD_REQUEST);

        // Unmerge without permission is rejected.
        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 0 WHERE id = 'u1'", [])
            .unwrap();
        let (unmerge_forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{a1}/unmerge"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"member_ids":["{member_id}"]}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(unmerge_forbidden_status, StatusCode::FORBIDDEN);

        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 1 WHERE id = 'u1'", [])
            .unwrap();
        let (unmerge_status, unmerge_body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{a1}/unmerge"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"member_ids":["{member_id}"]}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(unmerge_status, StatusCode::OK);
        let unmerged = json_body(&unmerge_body);
        assert_eq!(unmerged["new_artist_ids"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn lock_artist_identity_route_requires_permission_and_pending_state() {
        let (app, pool) = new_test_app_with_pool("music-lock-identity");
        let cookie = seed_user_session(&pool, "u1");
        let (a1, a2) = seed_duplicate_artists(&pool);

        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{a1}/lock-identity"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 1 WHERE id = 'u1'", [])
            .unwrap();

        // Not (yet) a merge master waiting on a match — nothing pending.
        let (not_pending_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{a1}/lock-identity"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(not_pending_status, StatusCode::BAD_REQUEST);

        send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/artists/merge")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"artist_ids":["{a1}","{a2}"],"master_name":"Madonna","master_artist_id":"{a1}"}}"#
                )))
                .unwrap(),
        )
        .await;

        let (locked_status, locked_body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{a1}/lock-identity"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(locked_status, StatusCode::OK);
        assert_eq!(json_body(&locked_body)["metadata_locked"].as_i64(), Some(1));
    }
}

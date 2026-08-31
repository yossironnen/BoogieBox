//! Defines Rust API routes for Artwork Routes server behavior.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

use crate::{
    artwork_cache::{
        build_album_art_cache_key, build_artist_art_cache_key, cache_item_dir,
        cached_copy_is_fresh, clear_cached_image_files, ext_from_content_type, file_etag,
        find_existing_cached_image, find_folder_cover_image, get_assigned_cache_file_path,
        http_date_secs, mime_from_path,
    },
    auth::AdminUser,
    image_thumb::generate_thumbnail,
    providers::{
        download_image, search_deezer_artist_image, search_discogs_album_cover,
        search_discogs_artist_image, search_spotify_artist_image,
    },
    DbPool, ErrorResponse, OkResponse, SharedState,
};

const ALBUM_THUMB_SIZES: &[u32] = &[300, 800];
const ARTIST_THUMB_SIZES: &[u32] = &[300, 800];

// -- Router --------------------------------------------------------------------

/// Documents the Artwork Router public API surface.
pub fn artwork_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/albums/{id}/cover", get(album_cover_handler))
        .route("/api/albums/{id}/art", get(album_art_handler))
        .route(
            "/api/albums/{id}/artwork",
            post(album_artwork_upload_handler),
        )
        .route("/api/artists/{id}/photo", get(artist_photo_handler))
        .route(
            "/api/artists/{id}/artwork",
            post(artist_artwork_upload_handler),
        )
        .with_state(state)
}

// -- Query params --------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ArtParams {
    size: Option<String>,
    refresh: Option<String>,
}

// -- Album cover (original) ----------------------------------------------------

async fn album_cover_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(params): Query<ArtParams>,
) -> Response {
    let (db, http_client, _db_folder) = match get_art_state(&state) {
        Some(t) => t,
        None => return setup_required_response(),
    };

    let refresh = params.refresh.as_deref() == Some("1");
    let _art_root_for_purge = album_art_original_root(&state);
    let state_for_purge = state.clone();
    let album_id = id.clone();

    let row = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        if refresh {
            let db_folder = state_for_purge
                .read()
                .expect("state lock")
                .db_folder
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            purge_album_image_cache_blocking(&conn, &album_id, &db_folder);
        }
        boogiebox_db::artwork::get_album_for_art(&conn, &album_id)
    })
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Album not found".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
        _ => return internal_error(),
    };

    let art_root = album_art_original_root(&state);
    let cache_key = build_album_art_cache_key(&id);
    let item_dir = cache_item_dir(&art_root, &cache_key);

    if let Some(cached_path) = find_existing_cached_image(&item_dir) {
        return stream_image_file(cached_path).await;
    }

    // Try folder.jpg
    let folder_path = find_folder_cover_image(std::path::Path::new(&row.file_path));
    if let Some(folder_img) = folder_path {
        let dest =
            match get_assigned_cache_file_path(&art_root, &cache_key, "original", ".jpg", true) {
                Some(p) => p,
                None => return internal_error(),
            };
        let src = folder_img.clone();
        if tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest))
            .await
            .is_ok()
        {
            let cached_path = find_existing_cached_image(&item_dir).unwrap_or(folder_img);
            return stream_image_file(cached_path).await;
        }
    }

    // Try Discogs
    let discogs_token = {
        let db2 = match get_db(&state) {
            Some(d) => d,
            None => return internal_error(),
        };
        tokio::task::spawn_blocking(move || {
            let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_setting(&conn, "discogsToken")
        })
        .await
        .ok()
        .flatten()
    };

    if let Some(token) = discogs_token {
        if let Some(image_url) =
            search_discogs_album_cover(&http_client, &token, &row.artist, &row.title).await
        {
            if let Some((bytes, ext)) = download_image(&http_client, &image_url).await {
                if let Some(dest) =
                    get_assigned_cache_file_path(&art_root, &cache_key, "original", &ext, true)
                {
                    if tokio::task::spawn_blocking(move || std::fs::write(&dest, &bytes))
                        .await
                        .is_ok()
                    {
                        if let Some(cached_path) = find_existing_cached_image(&item_dir) {
                            let db3 = match get_db(&state) {
                                Some(d) => d,
                                None => return stream_image_file(cached_path).await,
                            };
                            let idc = id.clone();
                            tokio::task::spawn_blocking(move || {
                                let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
                                boogiebox_db::artwork::record_provider_usage(
                                    &conn,
                                    "discogs",
                                    "album_art",
                                    "fetch",
                                );
                                let _ = idc; // suppress unused
                            })
                            .await
                            .ok();
                            return stream_image_file(cached_path).await;
                        }
                    }
                }
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "No album cover found from cache, folder image, or integrations".into(),
            setup_required: None,
        }),
    )
        .into_response()
}

// -- Album art thumbnail -------------------------------------------------------

async fn album_art_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(params): Query<ArtParams>,
    headers: HeaderMap,
) -> Response {
    let size = match params.size.as_deref().and_then(|s| s.parse::<u32>().ok()) {
        Some(s) if ALBUM_THUMB_SIZES.contains(&s) => s,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Unsupported size. Allowed: 300, 800".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
    };

    let (db, http_client, _db_folder2) = match get_art_state(&state) {
        Some(t) => t,
        None => return setup_required_response(),
    };

    let handler_start = std::time::Instant::now();
    let art_root = album_art_original_root(&state);
    let thumb_root = album_art_thumb_root(&state, size);
    let cache_key = build_album_art_cache_key(&id);
    let thumb_slot = format!("thumb-{size}");

    // Fast path (plan Phase 3.1): resolve and serve an existing cached thumbnail
    // — or an already-cached original, generating just the thumbnail from it —
    // before ever acquiring the application's single SQLite mutex. Only a true
    // cold miss (neither cached) needs the album row from the database at all.
    let disk_cache_start = std::time::Instant::now();
    if let Some(thumb_path) =
        get_assigned_cache_file_path(&thumb_root, &cache_key, &thumb_slot, ".jpg", false)
    {
        if thumb_path.is_file() {
            tracing::debug!(
                route = "album_art",
                disk_cache_ms = disk_cache_start.elapsed().as_secs_f64() * 1000.0,
                total_ms = handler_start.elapsed().as_secs_f64() * 1000.0,
                hit = "thumb",
                "album art timing"
            );
            return stream_image_file_conditional(thumb_path, &headers).await;
        }
    }

    let item_dir = cache_item_dir(&art_root, &cache_key);
    let disk_cache_ms = disk_cache_start.elapsed().as_secs_f64() * 1000.0;

    if let Some(original_path) = find_existing_cached_image(&item_dir) {
        let thumb_path = match get_assigned_cache_file_path(
            &thumb_root,
            &cache_key,
            &thumb_slot,
            ".jpg",
            true,
        ) {
            Some(p) => p,
            None => return internal_error(),
        };
        let src = original_path.clone();
        let dst = thumb_path.clone();
        let result =
            match tokio::task::spawn_blocking(move || generate_thumbnail(&src, &dst, size)).await {
                Ok(Ok(())) => stream_image_file_conditional(thumb_path, &headers).await,
                _ => stream_image_file_conditional(original_path, &headers).await,
            };
        tracing::debug!(
            route = "album_art",
            disk_cache_ms,
            total_ms = handler_start.elapsed().as_secs_f64() * 1000.0,
            hit = "original",
            "album art timing"
        );
        return result;
    }

    // Cold miss (plan Phase 3.2): neither the album row lookup nor folder/
    // provider discovery ever runs on the request's critical path. Check the
    // durable negative cache first (near-zero cost for an album that keeps
    // coming up empty); otherwise hand the fill off to a single-flight
    // background task — bounded by a small worker semaphore, deduplicated by
    // (cache_key, size) so concurrent misses for the same art never repeat the
    // work — and return promptly with a non-cacheable "not yet available"
    // response. The client's bounded retry (`ArtImage.tsx`) picks up the
    // result once the background fill completes.
    let fill_key = format!("{cache_key}:{size}");
    if art_negative_cache_is_fresh(&db, &fill_key).await {
        tracing::debug!(
            route = "album_art",
            disk_cache_ms,
            total_ms = handler_start.elapsed().as_secs_f64() * 1000.0,
            hit = "negative-cached",
            "album art timing"
        );
        return art_pending_response();
    }

    if claim_art_fill_slot(&fill_key) {
        tokio::spawn(fill_album_art_in_background(
            db.clone(),
            http_client.clone(),
            art_root.clone(),
            thumb_root.clone(),
            cache_key.clone(),
            thumb_slot.clone(),
            id.clone(),
            size,
            fill_key.clone(),
        ));
        tracing::debug!(
            route = "album_art",
            disk_cache_ms,
            total_ms = handler_start.elapsed().as_secs_f64() * 1000.0,
            hit = "miss-fill-scheduled",
            "album art timing"
        );
    } else {
        tracing::debug!(
            route = "album_art",
            disk_cache_ms,
            total_ms = handler_start.elapsed().as_secs_f64() * 1000.0,
            hit = "miss-fill-in-flight",
            "album art timing"
        );
    }
    art_pending_response()
}

/// True if a durable negative-cache entry says `fill_key` was recently tried
/// and found unavailable (reused Last.fm/provider cache table, namespaced —
/// see `art_negative_cache_key`). A fixed TTL, not exponential backoff: a
/// simplification accepted for this phase (see the performance plan).
async fn art_negative_cache_is_fresh(db: &DbPool, fill_key: &str) -> bool {
    let db = db.clone();
    let key = art_negative_cache_key(fill_key);
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::get_lastfm_cached(&conn, &key).is_some()
    })
    .await
    .unwrap_or(false)
}

fn art_negative_cache_key(fill_key: &str) -> String {
    format!("art-miss:{fill_key}")
}

const ART_NEGATIVE_CACHE_TTL_DAYS: i64 = 1;
const ART_FILL_MAX_CONCURRENT: usize = 4;

fn art_fill_inflight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static REGISTRY: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn art_fill_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(ART_FILL_MAX_CONCURRENT))
}

/// Claims the single-flight slot for `fill_key`, returning `true` only if this
/// call is the one that should do the fill (no other request/task is already
/// filling the same cache key + size).
fn claim_art_fill_slot(fill_key: &str) -> bool {
    let mut inflight = art_fill_inflight()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    inflight.insert(fill_key.to_string())
}

fn release_art_fill_slot(fill_key: &str) {
    let mut inflight = art_fill_inflight()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    inflight.remove(fill_key);
}

/// A non-cacheable "not ready yet" response for a cache miss whose fill is
/// scheduled (or already in flight) in the background.
fn art_pending_response() -> Response {
    let mut response = (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Album art not yet available; retry shortly".into(),
            setup_required: None,
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

/// Runs one album-art fill attempt under a bounded worker permit: album row
/// lookup, local folder-art discovery, then rate-limited provider fallback —
/// exactly the sequence the request path used to run inline. Always releases
/// the single-flight slot; records a negative-cache entry only on total
/// failure (found no album row, or found no art from any source).
#[allow(clippy::too_many_arguments)]
async fn fill_album_art_in_background(
    db: DbPool,
    http_client: reqwest::Client,
    art_root: PathBuf,
    thumb_root: PathBuf,
    cache_key: String,
    thumb_slot: String,
    album_id: String,
    size: u32,
    fill_key: String,
) {
    let _permit = art_fill_semaphore().acquire().await;
    let filled = fill_album_art_once(
        &db,
        &http_client,
        &art_root,
        &thumb_root,
        &cache_key,
        &thumb_slot,
        &album_id,
        size,
    )
    .await;

    if !filled {
        let db = db.clone();
        let key = art_negative_cache_key(&fill_key);
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::save_lastfm_cache(&conn, &key, "1", ART_NEGATIVE_CACHE_TTL_DAYS);
        })
        .await;
    }

    release_art_fill_slot(&fill_key);
}

/// True once an original is cached from any source (folder art or a
/// provider) — a thumbnail is then also generated from it, but even if that
/// sub-step fails, the next request's cache-first path (Phase 3.1) generates
/// it inline from the now-cached original, so it still counts as a fill.
#[allow(clippy::too_many_arguments)]
async fn fill_album_art_once(
    db: &DbPool,
    http_client: &reqwest::Client,
    art_root: &std::path::Path,
    thumb_root: &std::path::Path,
    cache_key: &str,
    thumb_slot: &str,
    album_id: &str,
    size: u32,
) -> bool {
    let db_row = db.clone();
    let aid = album_id.to_string();
    let row = match tokio::task::spawn_blocking(move || {
        let conn = db_row.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::get_album_for_art(&conn, &aid)
    })
    .await
    {
        Ok(Some(r)) => r,
        _ => return false,
    };

    let item_dir = cache_item_dir(art_root, cache_key);

    // Local folder-art discovery (highest priority: no network, usually fast —
    // though plan §3.3 found an unreachable media directory can still be slow,
    // which is exactly why this whole fill now runs off the request path).
    if let Some(folder_img) = find_folder_cover_image(std::path::Path::new(&row.file_path)) {
        if let Some(dest) =
            get_assigned_cache_file_path(art_root, cache_key, "original", ".jpg", true)
        {
            let src = folder_img;
            let _ = tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest)).await;
        }
    }

    // Rate-limited provider fallback, only once local discovery has failed.
    if find_existing_cached_image(&item_dir).is_none() {
        let db_tok = db.clone();
        let token = tokio::task::spawn_blocking(move || {
            let conn = db_tok.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_setting(&conn, "discogsToken")
        })
        .await
        .ok()
        .flatten();
        if let Some(tok) = token {
            if let Some(url) =
                search_discogs_album_cover(http_client, &tok, &row.artist, &row.title).await
            {
                if let Some((bytes, ext)) = download_image(http_client, &url).await {
                    if let Some(dest) =
                        get_assigned_cache_file_path(art_root, cache_key, "original", &ext, true)
                    {
                        let _ = tokio::task::spawn_blocking(move || std::fs::write(&dest, &bytes))
                            .await;
                    }
                }
            }
        }
    }

    let original_path = match find_existing_cached_image(&item_dir) {
        Some(p) => p,
        None => return false,
    };
    if let Some(thumb_path) =
        get_assigned_cache_file_path(thumb_root, cache_key, thumb_slot, ".jpg", true)
    {
        let src = original_path;
        let _ =
            tokio::task::spawn_blocking(move || generate_thumbnail(&src, &thumb_path, size)).await;
    }
    true
}

// -- Artist photo --------------------------------------------------------------

async fn artist_photo_handler(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(params): Query<ArtParams>,
) -> Response {
    let size = params
        .size
        .as_deref()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|s| ARTIST_THUMB_SIZES.contains(s));

    if params.size.is_some() && size.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Unsupported size. Allowed: 300, 800".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let (db, http_client, _db_folder3) = match get_art_state(&state) {
        Some(t) => t,
        None => return setup_required_response(),
    };

    let refresh = params.refresh.as_deref() == Some("1");
    let artist_id = id.clone();
    let state_for_artist_purge = state.clone();

    let artist = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        if refresh {
            let db_folder = state_for_artist_purge
                .read()
                .expect("state lock")
                .db_folder
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let art_root = db_folder.join("art").join("artist").join("original");
            let ck = build_artist_art_cache_key(&artist_id);
            let item_dir = cache_item_dir(&art_root, &ck);
            clear_cached_image_files(&item_dir);
        }
        boogiebox_db::artwork::get_artist_for_art(&conn, &artist_id)
    })
    .await
    {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Artist not found".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
        _ => return internal_error(),
    };

    let art_root = artist_art_original_root(&state);
    let cache_key = build_artist_art_cache_key(&id);
    let item_dir = cache_item_dir(&art_root, &cache_key);

    // Check cache
    if let Some(cached_path) = find_existing_cached_image(&item_dir) {
        if let Some(sz) = size {
            return serve_artist_thumb(&state, &id, &cached_path, sz).await;
        }
        return stream_image_file(cached_path).await;
    }

    // Fetch from providers: Deezer -> Discogs -> Spotify
    let settings = {
        let db2 = match get_db(&state) {
            Some(d) => d,
            None => return internal_error(),
        };
        tokio::task::spawn_blocking(move || {
            let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
            (
                boogiebox_db::artwork::get_setting(&conn, "discogsToken"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientId"),
                boogiebox_db::artwork::get_setting(&conn, "spotifyClientSecret"),
            )
        })
        .await
        .ok()
    };

    let (discogs_token, spotify_id, spotify_secret) = match settings {
        Some(t) => t,
        None => return internal_error(),
    };

    let mut image_url: Option<String> = None;
    let mut provider_used: Option<&str> = None;

    // 1. Deezer
    if image_url.is_none() {
        if let Some(url) = search_deezer_artist_image(&http_client, &artist.name).await {
            image_url = Some(url);
            provider_used = Some("deezer");
        }
    }

    // 2. Discogs
    if image_url.is_none() {
        if let Some(token) = &discogs_token {
            if let Some(url) = search_discogs_artist_image(&http_client, token, &artist.name).await
            {
                image_url = Some(url);
                provider_used = Some("discogs");
            }
        }
    }

    // 3. Spotify
    if image_url.is_none() {
        if let (Some(cid), Some(csecret)) = (&spotify_id, &spotify_secret) {
            if let Some(url) =
                search_spotify_artist_image(&http_client, cid, csecret, &artist.name).await
            {
                image_url = Some(url);
                provider_used = Some("spotify");
            }
        }
    }

    let Some(url) = image_url else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No artist image found from local cache or integrations".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let Some((bytes, ext)) = download_image(&http_client, &url).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No artist image found from local cache or integrations".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let dest = match get_assigned_cache_file_path(&art_root, &cache_key, "original", &ext, true) {
        Some(p) => p,
        None => return internal_error(),
    };
    let bytes_clone = bytes.clone();
    let dest_clone = dest.clone();
    if tokio::task::spawn_blocking(move || std::fs::write(&dest_clone, &bytes_clone))
        .await
        .is_err()
    {
        return internal_error();
    }

    // Record usage
    if let Some(prov) = provider_used {
        let db3 = match get_db(&state) {
            Some(d) => d,
            None => return internal_error(),
        };
        let prov = prov.to_owned();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db3.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::record_provider_usage(&conn, &prov, "artist_art", "fetch");
        })
        .await;
    }

    let cached_path = find_existing_cached_image(&item_dir).unwrap_or(dest);

    if let Some(sz) = size {
        return serve_artist_thumb(&state, &id, &cached_path, sz).await;
    }
    stream_image_file(cached_path).await
}

async fn serve_artist_thumb(
    state: &SharedState,
    id: &str,
    original: &std::path::Path,
    size: u32,
) -> Response {
    let thumb_root = artist_art_thumb_root(state, size);
    let cache_key = build_artist_art_cache_key(id);
    let slot = format!("thumb-{size}");

    if let Some(tp) = get_assigned_cache_file_path(&thumb_root, &cache_key, &slot, ".jpg", false) {
        if tp.is_file() {
            return stream_image_file(tp).await;
        }
    }

    let thumb_path =
        match get_assigned_cache_file_path(&thumb_root, &cache_key, &slot, ".jpg", true) {
            Some(p) => p,
            None => return internal_error(),
        };

    let src = original.to_path_buf();
    let orig_buf = original.to_path_buf();
    let dst = thumb_path.clone();
    match tokio::task::spawn_blocking(move || generate_thumbnail(&src, &dst, size)).await {
        Ok(Ok(())) => stream_image_file(thumb_path).await,
        _ => stream_image_file(orig_buf).await,
    }
}

// -- Artwork upload ------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtworkUploadBody {
    image_base64: Option<String>,
    mime_type: Option<String>,
}

async fn album_artwork_upload_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<ArtworkUploadBody>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let (Some(b64), Some(mime)) = (body.image_base64, body.mime_type) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "imageBase64 and mimeType required".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let bytes = match STANDARD.decode(b64.trim()) {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid base64 image".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
    };

    let album_id = id.clone();
    let exists = tokio::task::spawn_blocking({
        let db2 = db.clone();
        move || {
            let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_album_for_art(&conn, &album_id).is_some()
        }
    })
    .await
    .unwrap_or(false);

    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Album not found".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let art_root = album_art_original_root(&state);
    let cache_key = build_album_art_cache_key(&id);
    let ext = ext_from_content_type(&mime);

    // Purge existing
    let item_dir = cache_item_dir(&art_root, &cache_key);
    let _ = tokio::task::spawn_blocking({
        let thumb_root_300 = album_art_thumb_root(&state, 300);
        let thumb_root_800 = album_art_thumb_root(&state, 800);
        let ck = cache_key.clone();
        move || {
            clear_cached_image_files(&item_dir);
            for (tr, sz) in [(&thumb_root_300, 300u32), (&thumb_root_800, 800)] {
                if let Some(p) =
                    get_assigned_cache_file_path(tr, &ck, &format!("thumb-{sz}"), ".jpg", false)
                {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    })
    .await;

    let dest = match get_assigned_cache_file_path(&art_root, &cache_key, "original", ext, true) {
        Some(p) => p,
        None => return internal_error(),
    };

    match tokio::task::spawn_blocking(move || std::fs::write(&dest, &bytes)).await {
        Ok(Ok(())) => {}
        _ => return internal_error(),
    }

    let album_id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::set_album_metadata_locked(&conn, &album_id2);
    })
    .await
    .ok();

    (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
}

async fn artist_artwork_upload_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<ArtworkUploadBody>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(d) => d,
        None => return setup_required_response(),
    };

    let (Some(b64), Some(mime)) = (body.image_base64, body.mime_type) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "imageBase64 and mimeType required".into(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let bytes = match STANDARD.decode(b64.trim()) {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid base64 image".into(),
                    setup_required: None,
                }),
            )
                .into_response()
        }
    };

    let artist_id = id.clone();
    let exists = tokio::task::spawn_blocking({
        let db2 = db.clone();
        move || {
            let conn = db2.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::artwork::get_artist_for_art(&conn, &artist_id).is_some()
        }
    })
    .await
    .unwrap_or(false);

    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Artist not found".into(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let art_root = artist_art_original_root(&state);
    let cache_key = build_artist_art_cache_key(&id);
    let ext = ext_from_content_type(&mime);

    let item_dir = cache_item_dir(&art_root, &cache_key);
    let _ = tokio::task::spawn_blocking({
        let thumb_root_300 = artist_art_thumb_root(&state, 300);
        let thumb_root_800 = artist_art_thumb_root(&state, 800);
        let ck = cache_key.clone();
        move || {
            clear_cached_image_files(&item_dir);
            for (tr, sz) in [(&thumb_root_300, 300u32), (&thumb_root_800, 800)] {
                if let Some(p) =
                    get_assigned_cache_file_path(tr, &ck, &format!("thumb-{sz}"), ".jpg", false)
                {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    })
    .await;

    let dest = match get_assigned_cache_file_path(&art_root, &cache_key, "original", ext, true) {
        Some(p) => p,
        None => return internal_error(),
    };

    match tokio::task::spawn_blocking(move || std::fs::write(&dest, &bytes)).await {
        Ok(Ok(())) => {}
        _ => return internal_error(),
    }

    let artist_id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::artwork::set_artist_metadata_locked(&conn, &artist_id2);
    })
    .await
    .ok();

    (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
}

// -- Helpers -------------------------------------------------------------------

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn get_art_state(state: &SharedState) -> Option<(DbPool, reqwest::Client, std::path::PathBuf)> {
    let s = state.read().unwrap_or_else(|p| p.into_inner());
    let db = s.db.clone()?;
    let client = s.http_client.clone();
    let folder = s
        .db_folder
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    Some((db, client, folder))
}

fn album_art_original_root(state: &SharedState) -> std::path::PathBuf {
    let folder = state
        .read()
        .expect("state lock")
        .db_folder
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    folder.join("art").join("album").join("original")
}

fn album_art_thumb_root(state: &SharedState, size: u32) -> std::path::PathBuf {
    let folder = state
        .read()
        .expect("state lock")
        .db_folder
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    folder
        .join("art")
        .join("album")
        .join("thumb")
        .join(size.to_string())
}

fn artist_art_original_root(state: &SharedState) -> std::path::PathBuf {
    let folder = state
        .read()
        .expect("state lock")
        .db_folder
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    folder.join("art").join("artist").join("original")
}

fn artist_art_thumb_root(state: &SharedState, size: u32) -> std::path::PathBuf {
    let folder = state
        .read()
        .expect("state lock")
        .db_folder
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    folder
        .join("art")
        .join("artist")
        .join("thumb")
        .join(size.to_string())
}

fn purge_album_image_cache_blocking(
    _conn: &rusqlite::Connection,
    album_id: &str,
    db_folder: &std::path::Path,
) {
    let art_root = db_folder.join("art").join("album").join("original");
    let cache_key = build_album_art_cache_key(album_id);
    let item_dir = cache_item_dir(&art_root, &cache_key);
    clear_cached_image_files(&item_dir);

    for size in ALBUM_THUMB_SIZES {
        let thumb_root = db_folder
            .join("art")
            .join("album")
            .join("thumb")
            .join(size.to_string());
        if let Some(p) = get_assigned_cache_file_path(
            &thumb_root,
            &cache_key,
            &format!("thumb-{size}"),
            ".jpg",
            false,
        ) {
            let _ = std::fs::remove_file(p);
        }
    }
}

async fn stream_image_file(path: PathBuf) -> Response {
    let mime = mime_from_path(&path).to_owned();
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return internal_error(),
    };
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return internal_error(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .header(
            header::CACHE_CONTROL,
            "public, max-age=86400, stale-while-revalidate=43200",
        )
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|_| internal_error())
}

/// Same as `stream_image_file`, plus `ETag`/`Last-Modified` and `If-None-Match`/
/// `If-Modified-Since` handling — a `304` short-circuits the body entirely.
/// Used by the album-art thumbnail route (plan Phase 3.1); other cached-image
/// routes keep the plain `stream_image_file` until proven there too.
async fn stream_image_file_conditional(path: PathBuf, req_headers: &header::HeaderMap) -> Response {
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return internal_error(),
    };
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = file_etag(metadata.len(), modified_secs);
    let last_modified = http_date_secs(modified_secs);

    let if_none_match = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok());
    let if_modified_since = req_headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok());

    if cached_copy_is_fresh(if_none_match, &etag, if_modified_since, &last_modified) {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, &etag)
            .header(header::LAST_MODIFIED, &last_modified)
            .header(
                header::CACHE_CONTROL,
                "public, max-age=86400, stale-while-revalidate=43200",
            )
            .body(Body::empty())
            .unwrap_or_else(|_| internal_error());
    }

    let mime = mime_from_path(&path).to_owned();
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return internal_error(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, metadata.len().to_string())
        .header(
            header::CACHE_CONTROL,
            "public, max-age=86400, stale-while-revalidate=43200",
        )
        .header(header::ETAG, &etag)
        .header(header::LAST_MODIFIED, &last_modified)
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|_| internal_error())
}

fn setup_required_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

fn internal_error() -> Response {
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
    use super::{claim_art_fill_slot, release_art_fill_slot};
    use crate::test_support::{json_body, new_test_app_with_pool, seed_admin_session, send};
    use crate::DbPool;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use rusqlite::params;
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Seeds one library/artist/album/track whose `file_path` lives inside a real temp
    /// directory, so `find_folder_cover_image` can find a `folder.jpg` placed alongside
    /// it. Returns (artist_id, album_id, track_dir).
    fn seed_music(pool: &DbPool) -> (String, String, std::path::PathBuf) {
        let conn = pool.lock().unwrap();
        let track_dir = std::env::temp_dir().join(format!("artwork-route-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&track_dir).unwrap();

        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, ?, 'Lib')",
            params![library_id, track_dir.to_string_lossy()],
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
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Album', ?)",
            params![album_id, artist_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        let track_path = track_dir.join("track.mp3");
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
             VALUES (?, ?, ?, ?, 'Track', ?)",
            params![
                track_id,
                library_id,
                artist_id,
                album_id,
                track_path.to_string_lossy()
            ],
        )
        .unwrap();
        (artist_id, album_id, track_dir)
    }

    fn tiny_png_bytes() -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(4, 4, Rgb([10, 20, 30]));
        let mut out = Vec::new();
        buf.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[tokio::test]
    async fn album_cover_404s_for_missing_album() {
        let (app, _pool) = new_test_app_with_pool("artwork-cover-404");
        let (status, _) = send(
            app,
            Request::builder()
                .uri("/api/albums/does-not-exist/cover")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn album_cover_image_response_is_never_compressed() {
        // Plan Phase 4 item 1: the app-wide compression layer must exclude
        // image bodies even when the client advertises gzip support.
        let (app, pool) = new_test_app_with_pool("artwork-cover-no-compress");
        let (_artist_id, album_id, track_dir) = seed_music(&pool);
        std::fs::write(track_dir.join("folder.jpg"), tiny_png_bytes()).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/albums/{album_id}/cover"))
                    .header(axum::http::header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(axum::http::header::CONTENT_ENCODING)
                .is_none(),
            "an image response must never be compressed"
        );
    }

    #[tokio::test]
    async fn album_cover_finds_folder_jpg_next_to_track_file() {
        let (app, pool) = new_test_app_with_pool("artwork-cover-folder");
        let (_artist_id, album_id, track_dir) = seed_music(&pool);
        std::fs::write(track_dir.join("folder.jpg"), tiny_png_bytes()).unwrap();

        let (status, body) = send(
            app,
            Request::builder()
                .uri(format!("/api/albums/{album_id}/cover"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn album_art_rejects_unsupported_size_and_404s_for_missing_album() {
        let (app, pool) = new_test_app_with_pool("artwork-art-validate");
        let (_artist_id, album_id, _dir) = seed_music(&pool);

        let (bad_size_status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/albums/{album_id}/art?size=42"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(bad_size_status, StatusCode::BAD_REQUEST);

        let (missing_status, _) = send(
            app,
            Request::builder()
                .uri("/api/albums/does-not-exist/art?size=300")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn album_art_thumb_cache_hit_completes_promptly_while_db_mutex_is_held() {
        // Plan Phase 3.1 regression guard: a cached-thumbnail request must never
        // wait on the application's single SQLite mutex.
        let db_path = crate::test_support::temp_db_path("artwork-art-cache-hit-mutex");
        let db_folder = db_path.parent().unwrap().to_path_buf();
        let state = crate::test_support::build_test_state(&db_path);
        let pool = state.db.clone().unwrap();
        let app = crate::build_app(state, None);
        let (_artist_id, album_id, _dir) = seed_music(&pool);

        let thumb_root = db_folder
            .join("art")
            .join("album")
            .join("thumb")
            .join("300");
        let cache_key = crate::artwork_cache::build_album_art_cache_key(&album_id);
        let thumb_path = crate::artwork_cache::get_assigned_cache_file_path(
            &thumb_root,
            &cache_key,
            "thumb-300",
            ".jpg",
            true,
        )
        .unwrap();
        std::fs::write(&thumb_path, tiny_png_bytes()).unwrap();

        let holder_pool = pool.clone();
        let holder = std::thread::spawn(move || {
            let _guard = holder_pool.lock().unwrap_or_else(|p| p.into_inner());
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        std::thread::sleep(std::time::Duration::from_millis(20));

        let start = std::time::Instant::now();
        let (status, body) = send(
            app,
            Request::builder()
                .uri(format!("/api/albums/{album_id}/art?size=300"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let elapsed = start.elapsed();
        holder.join().unwrap();

        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "cached-thumb request should not wait on the db mutex, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn album_art_original_cache_hit_generates_thumb_without_touching_the_database() {
        // A cached original (no thumb yet) must generate the thumbnail and serve
        // it without ever calling `get_album_for_art` — proven here by using an
        // album id with no matching row at all; a database lookup would 404.
        let db_path = crate::test_support::temp_db_path("artwork-art-original-hit-no-db");
        let db_folder = db_path.parent().unwrap().to_path_buf();
        let state = crate::test_support::build_test_state(&db_path);
        let app = crate::build_app(state, None);

        let album_id = "no-such-album-row";
        let original_root = db_folder.join("art").join("album").join("original");
        let cache_key = crate::artwork_cache::build_album_art_cache_key(album_id);
        let original_path = crate::artwork_cache::get_assigned_cache_file_path(
            &original_root,
            &cache_key,
            "original",
            ".jpg",
            true,
        )
        .unwrap();
        std::fs::write(&original_path, tiny_png_bytes()).unwrap();

        let (status, body) = send(
            app,
            Request::builder()
                .uri(format!("/api/albums/{album_id}/art?size=300"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());
    }

    /// Polls the art route until the background fill (Phase 3.2) has produced
    /// a `200`, or panics after a generous timeout. Cache misses now return a
    /// non-cacheable "pending" `404` immediately while the fill runs off the
    /// request path — this is how a test observes the fill actually landing.
    async fn wait_for_art_ok(app: &axum::Router, uri: &str) -> Vec<u8> {
        for _ in 0..100 {
            let (status, body) = send(
                app.clone(),
                Request::builder().uri(uri).body(Body::empty()).unwrap(),
            )
            .await;
            if status == StatusCode::OK {
                return body;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("background art fill for {uri} did not complete in time");
    }

    #[tokio::test]
    async fn album_art_conditional_request_returns_304_with_empty_body() {
        let (app, pool) = new_test_app_with_pool("artwork-art-304");
        let (_artist_id, album_id, track_dir) = seed_music(&pool);
        std::fs::write(track_dir.join("folder.jpg"), tiny_png_bytes()).unwrap();

        let uri = format!("/api/albums/{album_id}/art?size=300");
        let body = wait_for_art_ok(&app, &uri).await;
        assert!(!body.is_empty());

        // Fetch again to read the ETag this server would send.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/albums/{album_id}/art?size=300"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = response
            .headers()
            .get(axum::http::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        let conditional = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/albums/{album_id}/art?size=300"))
                    .header(axum::http::header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
        let conditional_body = axum::body::to_bytes(conditional.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(conditional_body.is_empty());
    }

    #[tokio::test]
    async fn album_art_generates_thumbnail_from_folder_jpg() {
        let (app, pool) = new_test_app_with_pool("artwork-art-thumb");
        let (_artist_id, album_id, track_dir) = seed_music(&pool);
        std::fs::write(track_dir.join("folder.jpg"), tiny_png_bytes()).unwrap();

        let uri = format!("/api/albums/{album_id}/art?size=300");
        let body = wait_for_art_ok(&app, &uri).await;
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn album_art_miss_returns_pending_response_then_fills_in_the_background() {
        let (app, pool) = new_test_app_with_pool("artwork-art-pending");
        let (_artist_id, album_id, track_dir) = seed_music(&pool);
        std::fs::write(track_dir.join("folder.jpg"), tiny_png_bytes()).unwrap();

        // The very first request is a cold miss: it must return promptly
        // (never wait for the fill) with a non-cacheable "pending" response.
        let (status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/albums/{album_id}/art?size=300"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let uri = format!("/api/albums/{album_id}/art?size=300");
        let body = wait_for_art_ok(&app, &uri).await;
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn album_art_repeated_miss_for_an_unfillable_album_is_negative_cached() {
        // No folder.jpg and no Discogs token configured: the background fill
        // can never succeed. After it gives up, repeated requests must stay a
        // fast miss (served from the negative cache) rather than re-spawning
        // a fill every time.
        let (app, pool) = new_test_app_with_pool("artwork-art-negative-cache");
        let (_artist_id, album_id, _dir) = seed_music(&pool);
        let uri = format!("/api/albums/{album_id}/art?size=300");

        let (status, _) = send(
            app.clone(),
            Request::builder().uri(&uri).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Give the background fill time to fail and negative-cache the key.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let (status, _) = send(
            app,
            Request::builder().uri(&uri).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn art_fill_single_flight_slot_is_claimed_exactly_once_until_released() {
        // Unique key per test run: the registry is process-global (`static`),
        // so a fixed literal would collide with a concurrently-running test.
        let key = format!("single-flight-test-{}", Uuid::now_v7());

        assert!(
            claim_art_fill_slot(&key),
            "first claim on an unclaimed key must succeed"
        );
        assert!(
            !claim_art_fill_slot(&key),
            "a second concurrent claim on the same key must be refused"
        );

        release_art_fill_slot(&key);
        assert!(
            claim_art_fill_slot(&key),
            "after release, the key can be claimed again"
        );
        release_art_fill_slot(&key);
    }

    #[tokio::test]
    async fn artist_photo_404s_for_missing_artist_and_rejects_bad_size() {
        let (app, pool) = new_test_app_with_pool("artwork-photo-404");
        let (artist_id, _album_id, _dir) = seed_music(&pool);

        let (missing_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/artists/does-not-exist/photo")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);

        let (bad_size_status, _) = send(
            app,
            Request::builder()
                .uri(format!("/api/artists/{artist_id}/photo?size=999"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(bad_size_status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn album_artwork_upload_requires_admin_fields_and_valid_base64() {
        let (app, pool) = new_test_app_with_pool("artwork-upload-album");
        let admin_cookie = seed_admin_session(&pool, "admin1");
        let (_artist_id, album_id, _dir) = seed_music(&pool);

        let (unauth_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/albums/{album_id}/artwork"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(unauth_status, StatusCode::UNAUTHORIZED);

        let (missing_fields_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/albums/{album_id}/artwork"))
                .header("cookie", admin_cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_fields_status, StatusCode::BAD_REQUEST);

        let (bad_b64_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/albums/{album_id}/artwork"))
                .header("cookie", admin_cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"imageBase64":"not-valid-base64!!","mimeType":"image/png"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(bad_b64_status, StatusCode::BAD_REQUEST);

        let png_b64 = STANDARD.encode(tiny_png_bytes());
        let (missing_album_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/albums/does-not-exist/artwork")
                .header("cookie", admin_cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"imageBase64":"{png_b64}","mimeType":"image/png"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_album_status, StatusCode::NOT_FOUND);

        let (ok_status, ok_body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/albums/{album_id}/artwork"))
                .header("cookie", admin_cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"imageBase64":"{png_b64}","mimeType":"image/png"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(ok_status, StatusCode::OK);
        assert_eq!(json_body(&ok_body)["ok"], true);

        let locked: i64 = pool
            .lock()
            .unwrap()
            .query_row(
                "SELECT metadata_locked FROM albums WHERE id = ?",
                params![album_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(locked, 1);
    }

    #[tokio::test]
    async fn artist_artwork_upload_succeeds_and_locks_metadata() {
        let (app, pool) = new_test_app_with_pool("artwork-upload-artist");
        let admin_cookie = seed_admin_session(&pool, "admin1");
        let (artist_id, _album_id, _dir) = seed_music(&pool);
        let png_b64 = STANDARD.encode(tiny_png_bytes());

        let (status, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri(format!("/api/artists/{artist_id}/artwork"))
                .header("cookie", admin_cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"imageBase64":"{png_b64}","mimeType":"image/png"}}"#
                )))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["ok"], true);

        let locked: i64 = pool
            .lock()
            .unwrap()
            .query_row(
                "SELECT metadata_locked FROM artists WHERE id = ?",
                params![artist_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(locked, 1);
    }
}

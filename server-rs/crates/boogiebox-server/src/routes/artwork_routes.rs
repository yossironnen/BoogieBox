//! Defines Rust API routes for Artwork Routes server behavior.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
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
        clear_cached_image_files, ext_from_content_type, find_existing_cached_image,
        find_folder_cover_image, get_assigned_cache_file_path, mime_from_path,
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

    let album_id = id.clone();
    let row = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
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
    let thumb_root = album_art_thumb_root(&state, size);
    let cache_key = build_album_art_cache_key(&id);
    let thumb_slot = format!("thumb-{size}");

    // Check if thumbnail exists
    if let Some(thumb_path) =
        get_assigned_cache_file_path(&thumb_root, &cache_key, &thumb_slot, ".jpg", false)
    {
        if thumb_path.is_file() {
            return stream_image_file(thumb_path).await;
        }
    }

    // Ensure original is cached
    let item_dir = cache_item_dir(&art_root, &cache_key);
    let original_path = if let Some(p) = find_existing_cached_image(&item_dir) {
        p
    } else {
        // Try folder.jpg
        let folder_img = find_folder_cover_image(std::path::Path::new(&row.file_path));
        if let Some(fi) = folder_img {
            if let Some(dest) =
                get_assigned_cache_file_path(&art_root, &cache_key, "original", ".jpg", true)
            {
                let src = fi.clone();
                let _ = tokio::task::spawn_blocking(move || std::fs::copy(&src, &dest)).await;
            }
        }
        // Try Discogs if still not cached
        let cached = find_existing_cached_image(&item_dir);
        if cached.is_none() {
            let token = {
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
            if let Some(tok) = token {
                if let Some(url) =
                    search_discogs_album_cover(&http_client, &tok, &row.artist, &row.title).await
                {
                    if let Some((bytes, ext)) = download_image(&http_client, &url).await {
                        if let Some(dest) = get_assigned_cache_file_path(
                            &art_root, &cache_key, "original", &ext, true,
                        ) {
                            let _ =
                                tokio::task::spawn_blocking(move || std::fs::write(&dest, &bytes))
                                    .await;
                        }
                    }
                }
            }
        }
        match find_existing_cached_image(&item_dir) {
            Some(p) => p,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: "No album art available".into(),
                        setup_required: None,
                    }),
                )
                    .into_response()
            }
        }
    };

    // Generate thumbnail
    let thumb_path =
        match get_assigned_cache_file_path(&thumb_root, &cache_key, &thumb_slot, ".jpg", true) {
            Some(p) => p,
            None => return internal_error(),
        };

    let src = original_path.clone();
    let dst = thumb_path.clone();
    match tokio::task::spawn_blocking(move || generate_thumbnail(&src, &dst, size)).await {
        Ok(Ok(())) => stream_image_file(thumb_path).await,
        _ => stream_image_file(original_path).await,
    }
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

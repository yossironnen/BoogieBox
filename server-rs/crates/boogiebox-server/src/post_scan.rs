//! Defines Rust server support logic for Post Scan.

use crate::{
    artwork_cache::{
        build_album_art_cache_key, build_artist_art_cache_key, cache_item_dir,
        ext_from_content_type, find_existing_cached_image, find_folder_cover_image,
        get_assigned_cache_file_path,
    },
    image_thumb::generate_thumbnail,
    providers::{
        download_image, fetch_lastfm_album_info, fetch_lastfm_artist_info,
        fetch_lastfm_artist_top_tags, fetch_lrclib_lyrics, fetch_lyricsovh,
        get_spotify_access_token, search_deezer_artist_match, search_discogs_album_cover,
        search_discogs_artist_match, search_metadata, search_spotify_artist_match,
        search_spotify_artist_match_with_token, ArtistProviderMatch, LastFmInfoPayload,
        MetadataSearchResult,
    },
    DbPool,
};
use boogiebox_db::{
    artwork::{
        get_album_for_art, get_album_label, get_album_release_type, get_album_year,
        get_artist_for_art, get_lastfm_cached, get_setting, get_track_for_lyrics,
        save_lastfm_cache, set_album_label, set_album_release_type, set_album_year,
        upsert_cached_lyrics,
    },
    jobs::{ClaimedPostScanJob, JobError, PostScanLane},
    music::{
        coerce_entity_id, get_artist_external_identity, list_artists_needing_external_identity,
        persist_artist_identity_if_missing, ArtistExternalIdentity, ArtistIdentityProvider,
        EntityId,
    },
    playlists::normalize_release_type,
};
use reqwest::Client;
use rusqlite::OptionalExtension;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const ARTIST_THUMB_SIZES: &[u32] = &[300, 800];
const ALBUM_THUMB_SIZES: &[u32] = &[300, 800];
const LASTFM_CACHE_DAYS: i64 = 7;
const MIN_LASTFM_TAG_COUNT: u64 = 20;
const MAX_ARTIST_STYLE_AGE_HOURS: i64 = 24 * 7;
const STYLE_TAG_BLACKLIST: &[&str] = &[
    "seen live",
    "favorites",
    "favourites",
    "favorite",
    "favourite",
    "my favorites",
    "my favourites",
];

// -- State ---------------------------------------------------------------------

/// Public Post Scan State data shape used by BoogieBox.
#[derive(Clone)]
pub struct PostScanState {
    /// Documents the DB public API surface.
    pub db: DbPool,
    /// Documents the Http Client public API surface.
    pub http_client: Client,
    /// Documents the DB Folder public API surface.
    pub db_folder: Option<PathBuf>,
    /// Cancelled when the active database is switched, so stale workers stop touching the old pool.
    pub cancel: CancellationToken,
}

// -- Scheduler -----------------------------------------------------------------

/// Documents the Start Post Scan Scheduler public API surface.
pub fn start_post_scan_scheduler(state: PostScanState) {
    {
        let conn = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = boogiebox_db::jobs::enqueue_missing_artist_identity_backfill_jobs(&conn);
    }
    tokio::spawn(async move {
        run_one_pending_music_post_scan(&state).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = state.cancel.cancelled() => break,
                _ = interval.tick() => run_one_pending_music_post_scan(&state).await,
            }
        }
    });
}

/// Documents the Run One Pending Music Post Scan Owned public API surface.
pub async fn run_one_pending_music_post_scan_owned(state: PostScanState) {
    run_one_pending_music_post_scan(&state).await;
}

/// Documents the Run One Pending Music Post Scan public API surface.
pub async fn run_one_pending_music_post_scan(state: &PostScanState) {
    run_one_pending_post_scan_in_lane(state, PostScanLane::Music).await;
}

async fn run_one_pending_post_scan_in_lane(state: &PostScanState, lane: PostScanLane) {
    let db = state.db.clone();
    let claimed = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::jobs::claim_next_post_scan_job(&conn, lane)
    })
    .await
    {
        Ok(Ok(Some(job))) => job,
        Ok(Ok(None)) => return,
        Ok(Err(err)) => {
            tracing::warn!("post-scan claim failed: {err}");
            return;
        }
        Err(err) => {
            tracing::error!("post-scan spawn failed: {err}");
            return;
        }
    };

    tracing::debug!(
        "post-scan started: {} ({:?})",
        claimed.job_type,
        claimed.job_id
    );
    let result = process_post_scan_job(state, &claimed).await;
    let failed_msg = result.as_ref().err().map(|e| e.to_string());

    let db = state.db.clone();
    let job_id = claimed.job_id.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        match failed_msg {
            None => boogiebox_db::jobs::mark_post_scan_done(&conn, &job_id),
            Some(ref msg) => boogiebox_db::jobs::mark_post_scan_failed(&conn, &job_id, msg),
        }
    })
    .await;

    if let Err(err) = result {
        tracing::warn!("post-scan job {} failed: {}", claimed.job_type, err);
    }
}

async fn process_post_scan_job(
    state: &PostScanState,
    job: &ClaimedPostScanJob,
) -> Result<(), JobError> {
    // Heartbeat
    {
        let db = state.db.clone();
        let job_id = job.job_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::jobs::touch_post_scan_job(&conn, &job_id)
        })
        .await;
    }

    match job.job_type.as_str() {
        "refresh_library_mappings" => {
            let db = state.db.clone();
            let library_id = job.library_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::jobs::refresh_library_entity_mappings(&conn, &library_id)
            })
            .await
            .expect("spawn ok")
        }
        "cache_artist_images" => {
            run_cache_artist_images(state, &job.library_id, job.payload.as_deref()).await
        }
        "cache_album_images" => {
            run_cache_album_images(state, &job.library_id, job.payload.as_deref()).await
        }
        "warm_lastfm_artist_info" => {
            run_warm_lastfm_artist_info(state, &job.library_id, job.payload.as_deref()).await
        }
        "enrich_artist_external_ids" => {
            run_enrich_artist_external_ids(
                state,
                &job.job_id,
                &job.library_id,
                job.payload.as_deref(),
            )
            .await
        }
        "warm_lastfm_album_info" => {
            run_warm_lastfm_album_info(state, &job.library_id, job.payload.as_deref()).await
        }
        "warm_track_lyrics" => {
            run_warm_track_lyrics(state, &job.library_id, job.payload.as_deref()).await
        }
        "sync_artist_styles" => run_sync_artist_styles(state, &job.library_id).await,
        "sync_discogs_album_metadata" => {
            run_sync_discogs_album_metadata(state, &job.library_id, job.payload.as_deref()).await
        }
        other => Err(JobError::UnsupportedPostScanJob(other.to_owned())),
    }
}

// -- Helpers -------------------------------------------------------------------

async fn get_setting_value(db: &DbPool, key: &str) -> Option<String> {
    let db = db.clone();
    let key = key.to_owned();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        get_setting(&conn, &key)
    })
    .await
    .ok()
    .flatten()
}

async fn persist_artist_provider_match(
    db: &DbPool,
    artist_id: &str,
    provider: ArtistIdentityProvider,
    matched: &ArtistProviderMatch,
) {
    let db = db.clone();
    let artist_id = coerce_entity_id(artist_id);
    let external_id = matched.external_id.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        persist_artist_identity_if_missing(&conn, &artist_id, provider, Some(&external_id), None)
    })
    .await;
}

async fn persist_lastfm_identity(db: &DbPool, artist_id: &str, info: &LastFmInfoPayload) {
    let db = db.clone();
    let artist_id = coerce_entity_id(artist_id);
    let mbid = info.mbid.clone();
    let canonical_name = info.canonical_name.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        persist_artist_identity_if_missing(
            &conn,
            &artist_id,
            ArtistIdentityProvider::LastFm,
            mbid.as_deref(),
            canonical_name.as_deref(),
        )
    })
    .await;
}

async fn get_library_entity_ids(
    db: &DbPool,
    query: &'static str,
    library_id: EntityId,
) -> Vec<String> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = match conn.prepare(query) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([&library_id], |row| row.get::<_, EntityId>(0))
            .ok()
            .map(|it| {
                it.flatten()
                    .map(|id| match id {
                        EntityId::Int(n) => n.to_string(),
                        EntityId::Str(s) => s,
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

fn parse_entity_ids_from_payload(payload: Option<&str>, key: &str) -> Vec<String> {
    let payload = match payload {
        Some(p) if !p.is_empty() => p,
        _ => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v[key]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|id| {
                    id.as_str()
                        .map(str::to_owned)
                        .or_else(|| id.as_u64().map(|n| n.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn art_original_root(db_folder: &Path, entity: &str) -> PathBuf {
    db_folder.join("art").join(entity).join("original")
}

fn art_thumb_root(db_folder: &Path, entity: &str, size: u32) -> PathBuf {
    db_folder
        .join("art")
        .join(entity)
        .join("thumb")
        .join(size.to_string())
}

fn generate_cached_thumbs(
    src: &Path,
    thumb_root_fn: impl Fn(u32) -> PathBuf,
    cache_key: &str,
    sizes: &[u32],
) {
    for &size in sizes {
        let root = thumb_root_fn(size);
        if let Some(dest) =
            get_assigned_cache_file_path(&root, cache_key, &format!("thumb-{size}"), ".jpg", true)
        {
            let _ = generate_thumbnail(src, &dest, size);
        }
    }
}

// -- cache_artist_images -------------------------------------------------------

async fn run_cache_artist_images(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let db_folder = match &state.db_folder {
        Some(f) => f.clone(),
        None => return Ok(()),
    };

    let artist_ids = parse_entity_ids_from_payload(payload, "artistIds");
    let artist_ids = if artist_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT DISTINCT artist_id FROM tracks WHERE library_id=?1 AND artist_id IS NOT NULL",
            library_id.clone(),
        )
        .await
    } else {
        artist_ids
    };

    let discogs_token = get_setting_value(&state.db, "discogsToken").await;

    for artist_id in &artist_ids {
        let cache_key = build_artist_art_cache_key(artist_id);
        let art_root = art_original_root(&db_folder, "artist");
        let item_dir = cache_item_dir(&art_root, &cache_key);

        // Already cached - ensure thumbnails exist
        if let Some(cached) = find_existing_cached_image(&item_dir) {
            let ck = cache_key.clone();
            let df = db_folder.clone();
            let cached_path = cached.clone();
            tokio::task::spawn_blocking(move || {
                generate_cached_thumbs(
                    &cached_path,
                    |sz| art_thumb_root(&df, "artist", sz),
                    &ck,
                    ARTIST_THUMB_SIZES,
                );
            })
            .await
            .ok();
            continue;
        }

        let artist = {
            let db = state.db.clone();
            let id = artist_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_artist_for_art(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(artist) = artist else { continue };
        if artist.metadata_locked {
            continue;
        }

        // Try providers in priority order. Persist each validated identity
        // from the same response that supplied the selected image.
        let mut image_url = None;
        if let Some(matched) = search_deezer_artist_match(&state.http_client, &artist.name).await {
            persist_artist_provider_match(
                &state.db,
                artist_id,
                ArtistIdentityProvider::Deezer,
                &matched,
            )
            .await;
            image_url = matched.image_url;
        }

        if image_url.is_none() {
            if let Some(token) = &discogs_token {
                if let Some(matched) =
                    search_discogs_artist_match(&state.http_client, token, &artist.name).await
                {
                    persist_artist_provider_match(
                        &state.db,
                        artist_id,
                        ArtistIdentityProvider::Discogs,
                        &matched,
                    )
                    .await;
                    image_url = matched.image_url;
                }
            }
        }

        if image_url.is_none() {
            if let Some(matched) = try_spotify_artist_match(state, &artist.name).await {
                persist_artist_provider_match(
                    &state.db,
                    artist_id,
                    ArtistIdentityProvider::Spotify,
                    &matched,
                )
                .await;
                image_url = matched.image_url;
            }
        }

        let Some(url) = image_url else { continue };

        if let Some((bytes, content_type)) = download_image(&state.http_client, &url).await {
            let ext = ext_from_content_type(&content_type);
            let ck = cache_key.clone();
            let df = db_folder.clone();
            let art_r = art_root.clone();
            tokio::task::spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&item_dir);
                if let Some(dest) = get_assigned_cache_file_path(&art_r, &ck, "original", ext, true)
                {
                    if std::fs::write(&dest, &bytes).is_ok() {
                        generate_cached_thumbs(
                            &dest,
                            |sz| art_thumb_root(&df, "artist", sz),
                            &ck,
                            ARTIST_THUMB_SIZES,
                        );
                    }
                }
            })
            .await
            .ok();
        }
    }
    Ok(())
}

async fn try_spotify_artist_match(
    state: &PostScanState,
    artist_name: &str,
) -> Option<ArtistProviderMatch> {
    let client_id = get_setting_value(&state.db, "spotifyClientId").await?;
    let client_secret = get_setting_value(&state.db, "spotifyClientSecret").await?;
    search_spotify_artist_match(&state.http_client, &client_id, &client_secret, artist_name).await
}

// -- cache_album_images --------------------------------------------------------

async fn run_cache_album_images(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let db_folder = match &state.db_folder {
        Some(f) => f.clone(),
        None => return Ok(()),
    };

    let album_ids = parse_entity_ids_from_payload(payload, "albumIds");
    let album_ids = if album_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT DISTINCT album_id FROM tracks WHERE library_id=?1 AND album_id IS NOT NULL",
            library_id.clone(),
        )
        .await
    } else {
        album_ids
    };

    let discogs_token = get_setting_value(&state.db, "discogsToken").await;

    for album_id in &album_ids {
        let cache_key = build_album_art_cache_key(album_id);
        let art_root = art_original_root(&db_folder, "album");
        let item_dir = cache_item_dir(&art_root, &cache_key);

        if let Some(cached) = find_existing_cached_image(&item_dir) {
            let ck = cache_key.clone();
            let df = db_folder.clone();
            let cached_path = cached.clone();
            tokio::task::spawn_blocking(move || {
                generate_cached_thumbs(
                    &cached_path,
                    |sz| art_thumb_root(&df, "album", sz),
                    &ck,
                    ALBUM_THUMB_SIZES,
                );
            })
            .await
            .ok();
            continue;
        }

        let album = {
            let db = state.db.clone();
            let id = album_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_album_for_art(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(album) = album else { continue };
        if album.metadata_locked {
            continue;
        }

        // Try folder.jpg first
        if let Some(folder_img) = find_folder_cover_image(Path::new(&album.file_path)) {
            let ck = cache_key.clone();
            let df = db_folder.clone();
            let art_r = art_root.clone();
            let src = folder_img.clone();
            tokio::task::spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&item_dir);
                let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
                if let Some(dest) =
                    get_assigned_cache_file_path(&art_r, &ck, "original", &format!(".{ext}"), true)
                {
                    if std::fs::copy(&src, &dest).is_ok() {
                        generate_cached_thumbs(
                            &dest,
                            |sz| art_thumb_root(&df, "album", sz),
                            &ck,
                            ALBUM_THUMB_SIZES,
                        );
                    }
                }
            })
            .await
            .ok();
            continue;
        }

        // Try Discogs
        let image_url = if let Some(token) = &discogs_token {
            search_discogs_album_cover(&state.http_client, token, &album.artist, &album.title).await
        } else {
            None
        };

        let Some(url) = image_url else { continue };
        if let Some((bytes, content_type)) = download_image(&state.http_client, &url).await {
            let ext = ext_from_content_type(&content_type);
            let ck = cache_key.clone();
            let df = db_folder.clone();
            let art_r = art_root.clone();
            tokio::task::spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&item_dir);
                if let Some(dest) = get_assigned_cache_file_path(&art_r, &ck, "original", ext, true)
                {
                    if std::fs::write(&dest, &bytes).is_ok() {
                        generate_cached_thumbs(
                            &dest,
                            |sz| art_thumb_root(&df, "album", sz),
                            &ck,
                            ALBUM_THUMB_SIZES,
                        );
                    }
                }
            })
            .await
            .ok();
        }
    }
    Ok(())
}

// -- warm_lastfm_artist_info ---------------------------------------------------

async fn run_warm_lastfm_artist_info(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let api_key = match get_setting_value(&state.db, "lastfmKey").await {
        Some(k) => k,
        None => return Ok(()),
    };

    let artist_ids = parse_entity_ids_from_payload(payload, "artistIds");
    let artist_ids = if artist_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT DISTINCT artist_id FROM tracks WHERE library_id=?1 AND artist_id IS NOT NULL",
            library_id.clone(),
        )
        .await
    } else {
        artist_ids
    };

    for artist_id in &artist_ids {
        let artist = {
            let db = state.db.clone();
            let id = artist_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_artist_for_art(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(artist) = artist else { continue };

        let cache_key = format!("artist:{}", artist.name.to_lowercase());
        let cached = {
            let db = state.db.clone();
            let ck = cache_key.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_lastfm_cached(&conn, &ck)
            })
            .await
            .ok()
            .flatten()
        };
        if let Some(cached) = cached {
            if let Ok(info) = serde_json::from_str::<LastFmInfoPayload>(&cached) {
                if info.canonical_name.is_some() {
                    persist_lastfm_identity(&state.db, artist_id, &info).await;
                    continue;
                }
            }
        }

        let Some(info) = fetch_lastfm_artist_info(&state.http_client, &api_key, &artist.name).await
        else {
            continue;
        };
        persist_lastfm_identity(&state.db, artist_id, &info).await;

        let data = serde_json::to_string(&info).unwrap_or_default();
        if !data.is_empty() {
            let db = state.db.clone();
            let ck = cache_key.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                save_lastfm_cache(&conn, &ck, &data, LASTFM_CACHE_DAYS);
            })
            .await;
        }
    }
    Ok(())
}

// -- enrich_artist_external_ids ----------------------------------------------

async fn run_enrich_artist_external_ids(
    state: &PostScanState,
    job_id: &EntityId,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let requested_ids = parse_entity_ids_from_payload(payload, "artistIds");
    let artists: Vec<ArtistExternalIdentity> = if requested_ids.is_empty() {
        let db = state.db.clone();
        let library_id = library_id.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let stale_before: String =
                conn.query_row("SELECT datetime('now', '-30 days')", [], |row| row.get(0))?;
            list_artists_needing_external_identity(&conn, &library_id, &stale_before)
        })
        .await
        .expect("identity selection task")?
    } else {
        let db = state.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            requested_ids
                .iter()
                .map(|id| get_artist_external_identity(&conn, &coerce_entity_id(id)))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|rows| rows.into_iter().flatten().collect())
        })
        .await
        .expect("targeted identity task")?
    };

    let lastfm_key = get_setting_value(&state.db, "lastfmKey").await;
    let discogs_token = get_setting_value(&state.db, "discogsToken").await;
    let spotify_id = get_setting_value(&state.db, "spotifyClientId").await;
    let spotify_secret = get_setting_value(&state.db, "spotifyClientSecret").await;
    let spotify_token = match (spotify_id.as_deref(), spotify_secret.as_deref()) {
        (Some(id), Some(secret)) => get_spotify_access_token(&state.http_client, id, secret).await,
        _ => None,
    };

    for artist in artists {
        if artist.lastfm_mbid.is_none() {
            if let Some(api_key) = lastfm_key.as_deref() {
                if let Some(info) =
                    fetch_lastfm_artist_info(&state.http_client, api_key, &artist.name).await
                {
                    persist_lastfm_identity(&state.db, &artist.artist_id.to_string(), &info).await;
                }
            }
        }

        if artist.deezer_artist_id.is_none() {
            if let Some(matched) =
                search_deezer_artist_match(&state.http_client, &artist.name).await
            {
                persist_artist_provider_match(
                    &state.db,
                    &artist.artist_id.to_string(),
                    ArtistIdentityProvider::Deezer,
                    &matched,
                )
                .await;
            }
        }

        if artist.discogs_artist_id.is_none() {
            if let Some(token) = discogs_token.as_deref() {
                if let Some(matched) =
                    search_discogs_artist_match(&state.http_client, token, &artist.name).await
                {
                    persist_artist_provider_match(
                        &state.db,
                        &artist.artist_id.to_string(),
                        ArtistIdentityProvider::Discogs,
                        &matched,
                    )
                    .await;
                }
            }
        }

        if artist.spotify_artist_id.is_none() {
            if let Some(token) = spotify_token.as_deref() {
                if let Some(matched) =
                    search_spotify_artist_match_with_token(&state.http_client, token, &artist.name)
                        .await
                {
                    persist_artist_provider_match(
                        &state.db,
                        &artist.artist_id.to_string(),
                        ArtistIdentityProvider::Spotify,
                        &matched,
                    )
                    .await;
                }
            }
        }

        finalize_pending_identity_lock(&state.db, &artist.artist_id).await;

        let db = state.db.clone();
        let job_id = job_id.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            boogiebox_db::jobs::touch_post_scan_job(&conn, &job_id)
        })
        .await;
    }
    Ok(())
}

/// After a provider lookup pass for one artist, locks it if it was a merge
/// master waiting on its one-shot post-merge identity match (§6.5) and now
/// has one. See `music::finalize_pending_identity_lock`.
async fn finalize_pending_identity_lock(db: &DbPool, artist_id: &EntityId) {
    let db = db.clone();
    let artist_id = artist_id.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        boogiebox_db::music::finalize_pending_identity_lock(&conn, &artist_id)
    })
    .await;
}

// -- warm_lastfm_album_info ----------------------------------------------------

async fn run_warm_lastfm_album_info(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let api_key = match get_setting_value(&state.db, "lastfmKey").await {
        Some(k) => k,
        None => return Ok(()),
    };

    let album_ids = parse_entity_ids_from_payload(payload, "albumIds");
    let album_ids = if album_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT DISTINCT album_id FROM tracks WHERE library_id=?1 AND album_id IS NOT NULL",
            library_id.clone(),
        )
        .await
    } else {
        album_ids
    };

    for album_id in &album_ids {
        let album = {
            let db = state.db.clone();
            let id = album_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_album_for_art(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(album) = album else { continue };
        if album.title.is_empty() || album.artist.is_empty() {
            continue;
        }

        let cache_key = format!(
            "album:{}:{}",
            album.artist.to_lowercase(),
            album.title.to_lowercase()
        );
        let is_cached = {
            let db = state.db.clone();
            let ck = cache_key.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_lastfm_cached(&conn, &ck).is_some()
            })
            .await
            .unwrap_or(false)
        };
        if is_cached {
            continue;
        }

        let Some(info) =
            fetch_lastfm_album_info(&state.http_client, &api_key, &album.artist, &album.title)
                .await
        else {
            continue;
        };

        let data = serde_json::to_string(&info).unwrap_or_default();
        if !data.is_empty() {
            let db = state.db.clone();
            let ck = cache_key.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                save_lastfm_cache(&conn, &ck, &data, LASTFM_CACHE_DAYS);
            })
            .await;
        }
    }
    Ok(())
}

// -- warm_track_lyrics ---------------------------------------------------------

async fn run_warm_track_lyrics(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let track_ids = parse_entity_ids_from_payload(payload, "trackIds");
    let track_ids = if track_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT id FROM tracks WHERE library_id=?1",
            library_id.clone(),
        )
        .await
    } else {
        track_ids
    };

    for track_id in &track_ids {
        let track = {
            let db = state.db.clone();
            let id = track_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_track_for_lyrics(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(track) = track else { continue };

        // Check if already cached
        let is_cached = {
            let db = state.db.clone();
            let id = track_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                conn.query_row(
                    "SELECT 1 FROM lyrics_cache WHERE track_id=?",
                    rusqlite::params![id],
                    |_| Ok(true),
                )
                .optional()
                .ok()
                .flatten()
                .unwrap_or(false)
            })
            .await
            .unwrap_or(false)
        };
        if is_cached {
            continue;
        }

        // Try LRCLIB first, then lyrics.ovh
        let lr = fetch_lrclib_lyrics(&state.http_client, &track.artist, &track.title).await;
        let lr = if lr.is_none() {
            fetch_lyricsovh(&state.http_client, &track.artist, &track.title).await
        } else {
            lr
        };
        let Some(lr) = lr else { continue };

        let synced_json = lr
            .synced
            .as_ref()
            .and_then(|s| serde_json::to_string(s).ok());

        let db = state.db.clone();
        let id = track_id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        let lyrics_text = lr.lyrics.clone();
        let source = lr.source.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            upsert_cached_lyrics(
                &conn,
                &id,
                &artist,
                &title,
                &lyrics_text,
                &source,
                synced_json.as_deref(),
            );
        })
        .await;
    }
    Ok(())
}

// -- sync_artist_styles --------------------------------------------------------

async fn run_sync_artist_styles(
    state: &PostScanState,
    library_id: &EntityId,
) -> Result<(), JobError> {
    let api_key = match get_setting_value(&state.db, "lastfmKey").await {
        Some(k) => k,
        None => return Ok(()),
    };

    // Artists in library whose styles are stale or missing
    let artist_rows: Vec<(String, String)> = {
        let db = state.db.clone();
        let library_id = library_id.clone();
        let age_hours = MAX_ARTIST_STYLE_AGE_HOURS;
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let mut stmt = conn.prepare(
                "SELECT DISTINCT ar.id, ar.name \
                 FROM artists ar \
                 JOIN tracks t ON t.artist_id = ar.id \
                 WHERE t.library_id = ?1 \
                   AND ar.name IS NOT NULL AND ar.name != '' \
                   AND (NOT EXISTS (SELECT 1 FROM artist_styles WHERE artist_id = ar.id) \
                        OR (SELECT MAX(updated_at) FROM artist_styles WHERE artist_id = ar.id) \
                             < datetime('now', '-' || ?2 || ' hours')) \
                 ORDER BY ar.name",
            )?;
            let rows = stmt.query_map(rusqlite::params![library_id, age_hours], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default()
    };

    for (artist_id, artist_name) in &artist_rows {
        let tags = fetch_lastfm_artist_top_tags(&state.http_client, &api_key, artist_name).await;

        let valid_styles: Vec<String> = tags
            .iter()
            .filter(|(name, count)| {
                let n = name.to_lowercase();
                *count >= MIN_LASTFM_TAG_COUNT
                    && !STYLE_TAG_BLACKLIST.contains(&n.as_str())
                    && !n.contains("seen live")
                    && !n.contains("under ")
                    && !n.contains("my ")
                    && n.len() >= 3
            })
            .map(|(name, _)| name.to_lowercase())
            .take(12)
            .collect();

        if valid_styles.is_empty() {
            continue;
        }

        let db = state.db.clone();
        let id = artist_id.clone();
        let styles = valid_styles.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let _ = conn.execute(
                "DELETE FROM artist_styles WHERE artist_id = ?",
                rusqlite::params![id],
            );
            for style in styles {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO artist_styles(artist_id, style, updated_at) \
                     VALUES(?, ?, datetime('now'))",
                    rusqlite::params![id, style],
                );
            }
        })
        .await;
    }
    Ok(())
}

// -- sync_discogs_album_metadata (release type + record label) ------------------

/// Between-album delay for this lane. Deliberately slower than other post-scan
/// lanes since it can call both Discogs and Spotify per album and needs to stay
/// conservative against provider rate limits during a full-library backfill.
const DISCOGS_ALBUM_METADATA_SYNC_DELAY: Duration = Duration::from_millis(1500);

/// Discogs' search results list the label credit inline (no release-detail
/// fetch needed) but mix in pressing/distribution/mastering credits after the
/// releasing label itself, so this takes the first entry as a heuristic.
fn extract_discogs_label(extra: &serde_json::Value) -> Option<String> {
    extra["label"].as_array()?.iter().find_map(|l| {
        let name = l.as_str()?.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("not on label") {
            None
        } else {
            Some(name.to_owned())
        }
    })
}

/// Providers report the release year in two shapes: Discogs sends a bare year
/// ("1995") while Spotify sends a full release date ("1995-06-12"), so take the
/// leading four digits of either. Values outside the era of recorded music are
/// provider junk rather than real metadata.
fn parse_provider_year(value: &str) -> Option<i64> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() != 4 {
        return None;
    }
    digits.parse::<i64>().ok().filter(|y| *y >= 1860)
}

fn normalize_match_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let stripped = regex_strip_parens(&lower);
    let alphanum: String = stripped
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    alphanum.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn regex_strip_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

async fn run_sync_discogs_album_metadata(
    state: &PostScanState,
    library_id: &EntityId,
    payload: Option<&str>,
) -> Result<(), JobError> {
    let discogs_token = get_setting_value(&state.db, "discogsToken").await;
    let spotify_client_id = get_setting_value(&state.db, "spotifyClientId").await;
    let spotify_client_secret = get_setting_value(&state.db, "spotifyClientSecret").await;
    if discogs_token.is_none() && (spotify_client_id.is_none() || spotify_client_secret.is_none()) {
        return Ok(());
    }

    let album_ids = parse_entity_ids_from_payload(payload, "albumIds");
    let album_ids = if album_ids.is_empty() {
        get_library_entity_ids(
            &state.db,
            "SELECT DISTINCT album_id FROM tracks WHERE library_id=?1 AND album_id IS NOT NULL",
            library_id.clone(),
        )
        .await
    } else {
        album_ids
    };

    for album_id in &album_ids {
        let album = {
            let db = state.db.clone();
            let id = album_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                get_album_for_art(&conn, &id)
            })
            .await
            .ok()
            .flatten()
        };
        let Some(album) = album else { continue };
        if album.title.is_empty() || album.artist.is_empty() || album.metadata_locked {
            continue;
        }

        let (needs_release_type, needs_label, needs_year) = {
            let db = state.db.clone();
            let id = album_id.clone();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                (
                    get_album_release_type(&conn, &id).is_none(),
                    get_album_label(&conn, &id).is_none(),
                    get_album_year(&conn, &id).is_none(),
                )
            })
            .await
            .unwrap_or((false, false, false))
        };
        if !needs_release_type && !needs_label && !needs_year {
            continue;
        }

        let results = search_metadata(
            &state.http_client,
            discogs_token.as_deref(),
            spotify_client_id.as_deref(),
            spotify_client_secret.as_deref(),
            &album.artist,
            Some(&album.title),
        )
        .await;

        let target_artist = normalize_match_text(&album.artist);
        let target_title = normalize_match_text(&album.title);
        let matched: Vec<&MetadataSearchResult> = results
            .iter()
            .filter(|r| {
                let artist_ok = r
                    .artist
                    .as_deref()
                    .map(|a| normalize_match_text(a) == target_artist)
                    .unwrap_or(false);
                let title_ok = r
                    .title
                    .as_deref()
                    .map(|t| normalize_match_text(t) == target_title)
                    .unwrap_or(false);
                artist_ok && title_ok
            })
            .collect();

        // Only act when every confidently-matched result agrees; otherwise
        // leave the album for manual resolution rather than guess.
        if needs_release_type {
            let release_types: Vec<&str> = matched
                .iter()
                .filter_map(|r| r.release_type.as_deref())
                .filter_map(|rt| normalize_release_type(Some(rt)))
                .collect();
            let resolved = match release_types.split_first() {
                Some((first, rest)) if rest.iter().all(|v| v == first) => Some(*first),
                _ => None,
            };
            if let Some(release_type) = resolved {
                let db = state.db.clone();
                let id = album_id.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    set_album_release_type(&conn, &id, release_type);
                })
                .await;
            }
        }

        if needs_label {
            let labels: Vec<String> = matched
                .iter()
                .filter(|r| r.provider == "discogs")
                .filter_map(|r| r.extra.as_ref())
                .filter_map(extract_discogs_label)
                .collect();
            // Unlike release type, different pressings of the same album legitimately carry
            // different labels, so take the top (most relevant per Discogs' own ranking) match
            // rather than requiring every candidate to agree.
            if let Some(label) = labels.into_iter().next() {
                let db = state.db.clone();
                let id = album_id.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    set_album_label(&conn, &id, &label);
                })
                .await;
            }
        }

        // Only reached for albums whose own tracks carry no usable year tag, since
        // the tag-derived value always wins. Reissues and remasters give the same
        // album several provider years, so take the earliest to match the original
        // release year that tag-derived years resolve to.
        if needs_year {
            let resolved = matched
                .iter()
                .filter_map(|r| r.year.as_deref())
                .filter_map(parse_provider_year)
                .min();
            if let Some(year) = resolved {
                let db = state.db.clone();
                let id = album_id.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    set_album_year(&conn, &id, year);
                })
                .await;
            }
        }

        tokio::time::sleep(DISCOGS_ALBUM_METADATA_SYNC_DELAY).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_provider_year, persist_artist_provider_match, persist_lastfm_identity};
    use crate::{providers::ArtistProviderMatch, DbPool};
    use boogiebox_db::{
        initialize_schema,
        music::{coerce_entity_id, get_artist_external_identity, ArtistIdentityProvider},
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn parses_bare_and_dated_provider_years() {
        // Discogs sends a bare year, Spotify a full release date.
        assert_eq!(parse_provider_year("1995"), Some(1995));
        assert_eq!(parse_provider_year("1995-06-12"), Some(1995));
        assert_eq!(parse_provider_year("  2001  "), Some(2001));
    }

    #[test]
    fn rejects_provider_junk_years() {
        assert_eq!(parse_provider_year(""), None);
        assert_eq!(parse_provider_year("0"), None);
        assert_eq!(parse_provider_year("0000"), None);
        assert_eq!(parse_provider_year("95"), None);
        assert_eq!(parse_provider_year("unknown"), None);
        // A leading run shorter or longer than four digits is not a year.
        assert_eq!(parse_provider_year("19952"), None);
    }

    #[tokio::test]
    async fn opportunistic_helpers_persist_selected_provider_and_lastfm_identity() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES('artist-1', 'Massive Attack')",
            [],
        )
        .unwrap();
        let db: DbPool = Arc::new(Mutex::new(conn));
        let artist_id = coerce_entity_id("artist-1");
        let matched = ArtistProviderMatch {
            external_id: "deezer-1".to_owned(),
            canonical_name: "Massive Attack".to_owned(),
            image_url: Some("https://img/artist.jpg".to_owned()),
            confidence: 0.9,
        };
        persist_artist_provider_match(&db, "artist-1", ArtistIdentityProvider::Deezer, &matched)
            .await;
        let info = crate::providers::LastFmInfoPayload {
            mbid: Some("mbid-1".to_owned()),
            canonical_name: Some("Massive Attack".to_owned()),
            summary: String::new(),
            full: String::new(),
            listeners: None,
            playcount: None,
            url: None,
            image: None,
            tags: Vec::new(),
        };
        persist_lastfm_identity(&db, "artist-1", &info).await;

        let identity = {
            let conn = db.lock().unwrap();
            get_artist_external_identity(&conn, &artist_id)
                .unwrap()
                .unwrap()
        };
        assert_eq!(identity.deezer_artist_id.as_deref(), Some("deezer-1"));
        assert_eq!(identity.lastfm_mbid.as_deref(), Some("mbid-1"));
        assert_eq!(
            identity.lastfm_canonical_name.as_deref(),
            Some("Massive Attack")
        );
    }

    // -- offline pure-logic tests ------------------------------------------------

    #[test]
    fn parse_entity_ids_from_payload_handles_all_shapes() {
        use super::parse_entity_ids_from_payload;
        assert!(parse_entity_ids_from_payload(None, "artistIds").is_empty());
        assert!(parse_entity_ids_from_payload(Some(""), "artistIds").is_empty());
        assert!(parse_entity_ids_from_payload(Some("not json"), "artistIds").is_empty());
        assert!(parse_entity_ids_from_payload(Some("{}"), "artistIds").is_empty());
        assert!(parse_entity_ids_from_payload(Some(r#"{"artistIds":[]}"#), "artistIds").is_empty());
        let ids = parse_entity_ids_from_payload(Some(r#"{"artistIds":["a1","a2"]}"#), "artistIds");
        assert_eq!(ids, vec!["a1".to_string(), "a2".to_string()]);
        // numeric ids coerce to strings too.
        let ids = parse_entity_ids_from_payload(Some(r#"{"artistIds":[5,6]}"#), "artistIds");
        assert_eq!(ids, vec!["5".to_string(), "6".to_string()]);
    }

    #[test]
    fn art_root_helpers_build_expected_paths() {
        use super::{art_original_root, art_thumb_root};
        let base = std::path::Path::new("/data");
        assert_eq!(
            art_original_root(base, "artist"),
            std::path::PathBuf::from("/data/art/artist/original")
        );
        assert_eq!(
            art_thumb_root(base, "album", 300),
            std::path::PathBuf::from("/data/art/album/thumb/300")
        );
    }

    #[test]
    fn extract_discogs_label_skips_blank_and_not_on_label() {
        use super::extract_discogs_label;
        assert_eq!(
            extract_discogs_label(
                &serde_json::json!({"label": ["Not On Label", "  ", "Real Records"]})
            ),
            Some("Real Records".to_string())
        );
        assert_eq!(
            extract_discogs_label(&serde_json::json!({"label": ["Not on label"]})),
            None
        );
        assert_eq!(extract_discogs_label(&serde_json::json!({})), None);
    }

    #[test]
    fn normalize_match_text_strips_parens_and_punctuation() {
        use super::normalize_match_text;
        assert_eq!(
            normalize_match_text("The Beatles (Remastered) [2009]"),
            "the beatles"
        );
        assert_eq!(normalize_match_text("Sigur Rós"), "sigur r s");
    }

    // -- end-to-end fixture -------------------------------------------------------

    struct Fixture {
        state: super::PostScanState,
        library_id: String,
        artist_id: String,
        album_id: String,
        track_id: String,
        dir: std::path::PathBuf,
    }

    fn fixture(prefix: &str) -> Fixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("post-scan-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = boogiebox_db::init_db(&dir).unwrap().connection;

        let library_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, ?, 'Lib')",
            rusqlite::params![library_id, dir.to_string_lossy()],
        )
        .unwrap();
        let artist_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Test Artist')",
            rusqlite::params![artist_id],
        )
        .unwrap();
        let album_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Test Album', ?)",
            rusqlite::params![album_id, artist_id],
        )
        .unwrap();
        let track_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
             VALUES (?, ?, ?, ?, 'Test Track', ?)",
            rusqlite::params![
                track_id,
                library_id,
                artist_id,
                album_id,
                dir.join("Test Track.mp3").to_string_lossy()
            ],
        )
        .unwrap();

        let db: DbPool = Arc::new(Mutex::new(conn));
        let state = super::PostScanState {
            db,
            http_client: reqwest::Client::new(),
            db_folder: Some(dir.clone()),
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        Fixture {
            state,
            library_id,
            artist_id,
            album_id,
            track_id,
            dir,
        }
    }

    fn set_setting(f: &Fixture, key: &str, value: &str) {
        let conn = f.state.db.lock().unwrap();
        let changed = conn
            .execute(
                "UPDATE settings SET value = ? WHERE key = ?",
                rusqlite::params![value, key],
            )
            .unwrap();
        if changed == 0 {
            conn.execute(
                "INSERT INTO settings(key, value) VALUES (?, ?)",
                rusqlite::params![key, value],
            )
            .unwrap();
        }
    }

    // -- process_post_scan_job dispatch ------------------------------------------

    #[tokio::test]
    async fn process_post_scan_job_dispatches_refresh_library_mappings() {
        let f = fixture("dispatch-refresh");
        let job = boogiebox_db::jobs::ClaimedPostScanJob {
            job_id: boogiebox_db::music::coerce_entity_id("job-1"),
            library_id: boogiebox_db::music::coerce_entity_id(&f.library_id),
            job_type: "refresh_library_mappings".to_string(),
            payload: None,
        };
        let result = super::process_post_scan_job(&f.state, &job).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn process_post_scan_job_rejects_unsupported_job_type() {
        let f = fixture("dispatch-unsupported");
        let job = boogiebox_db::jobs::ClaimedPostScanJob {
            job_id: boogiebox_db::music::coerce_entity_id("job-1"),
            library_id: boogiebox_db::music::coerce_entity_id(&f.library_id),
            job_type: "totally_unknown_job".to_string(),
            payload: None,
        };
        let result = super::process_post_scan_job(&f.state, &job).await;
        assert!(matches!(
            result,
            Err(boogiebox_db::jobs::JobError::UnsupportedPostScanJob(_))
        ));
    }

    #[tokio::test]
    async fn run_one_pending_music_post_scan_runs_a_queued_job_to_completion() {
        let f = fixture("lane-e2e");
        {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::jobs::enqueue_post_scan_job(
                &conn,
                &f.library_id,
                "refresh_library_mappings",
                None,
            )
            .unwrap();
        }
        super::run_one_pending_music_post_scan(&f.state).await;
        let status: String = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT status FROM post_scan_jobs WHERE library_id = ?",
                rusqlite::params![f.library_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(status, "done");
    }

    #[tokio::test]
    async fn run_one_pending_music_post_scan_marks_unsupported_job_failed() {
        let f = fixture("lane-failed");
        {
            let conn = f.state.db.lock().unwrap();
            // `claim_next_post_scan_job` only ever selects from the fixed
            // `MUSIC_POST_SCAN_JOB_TYPES` allow-list, so an arbitrary job_type
            // (even inserted directly, bypassing `enqueue_post_scan_job`'s own
            // check) is simply never claimed — it can't reach this failure path.
            // `refresh_library_mappings` itself no-ops when `artist_libraries`/
            // `album_libraries` don't exist, so force a real failure via
            // `enrich_artist_external_ids`'s auto-select query instead, which
            // propagates a real SQL error (`?`) when its `artists` table is gone.
            boogiebox_db::jobs::enqueue_post_scan_job(
                &conn,
                &f.library_id,
                "enrich_artist_external_ids",
                None,
            )
            .unwrap();
            conn.execute("DROP TABLE artists", []).unwrap();
        }
        super::run_one_pending_music_post_scan(&f.state).await;
        let (status, error_log): (String, Option<String>) = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT status, error_log FROM post_scan_jobs WHERE library_id = ?",
                rusqlite::params![f.library_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(status, "failed");
        assert!(error_log.is_some());
    }

    #[tokio::test]
    async fn run_one_pending_music_post_scan_is_a_noop_with_nothing_queued() {
        let f = fixture("lane-empty");
        // Should return promptly without panicking when the queue is empty.
        super::run_one_pending_music_post_scan(&f.state).await;
    }

    // -- run_cache_artist_images ---------------------------------------------------

    #[tokio::test]
    async fn cache_artist_images_noop_without_db_folder() {
        let mut f = fixture("artist-images-no-folder");
        f.state.db_folder = None;
        let result = super::run_cache_artist_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cache_artist_images_skips_metadata_locked_artist() {
        let f = fixture("artist-images-locked");
        {
            let conn = f.state.db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET metadata_locked = 1 WHERE id = ?",
                rusqlite::params![f.artist_id],
            )
            .unwrap();
        }
        let result = super::run_cache_artist_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        // No art directory should have been created since the artist was skipped.
        assert!(!f.dir.join("art").join("artist").exists());
    }

    #[tokio::test]
    async fn cache_artist_images_downloads_from_deezer_match() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search/artist"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": 99, "name": "Test Artist", "picture_xl": format!("{}/cover.png", server.uri())}]
            })))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/cover.png"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_bytes(vec![1u8, 2, 3, 4])
                    .insert_header("content-type", "image/png"),
            )
            .mount(&server)
            .await;

        let f = fixture("artist-images-deezer");
        let result = super::run_cache_artist_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        assert!(f.dir.join("art").join("artist").join("original").exists());

        let identity = {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::music::get_artist_external_identity(
                &conn,
                &boogiebox_db::music::coerce_entity_id(&f.artist_id),
            )
            .unwrap()
            .unwrap()
        };
        assert_eq!(identity.deezer_artist_id.as_deref(), Some("99"));

        // Running again should hit the "already cached" branch instead of
        // re-fetching from the provider.
        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
        let result = super::run_cache_artist_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cache_artist_images_with_no_provider_match_leaves_no_art() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search/artist"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let f = fixture("artist-images-nomatch");
        let payload = format!(r#"{{"artistIds":["{}"]}}"#, f.artist_id);
        let result =
            super::run_cache_artist_images(&f.state, &f.library_id_entity(), Some(&payload)).await;
        assert!(result.is_ok());
        assert!(!f.dir.join("art").join("artist").join("original").exists());

        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }

    // -- run_cache_album_images ------------------------------------------------

    #[tokio::test]
    async fn cache_album_images_uses_folder_jpg_when_present() {
        let f = fixture("album-images-folder-jpg");
        let folder = f.dir.clone();
        std::fs::write(folder.join("folder.jpg"), vec![9u8, 9, 9]).unwrap();

        let result = super::run_cache_album_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        assert!(f.dir.join("art").join("album").join("original").exists());
    }

    #[tokio::test]
    async fn cache_album_images_skips_metadata_locked_album() {
        let f = fixture("album-images-locked");
        {
            let conn = f.state.db.lock().unwrap();
            conn.execute(
                "UPDATE albums SET metadata_locked = 1 WHERE id = ?",
                rusqlite::params![f.album_id],
            )
            .unwrap();
        }
        let result = super::run_cache_album_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        assert!(!f.dir.join("art").join("album").exists());
    }

    #[tokio::test]
    async fn cache_album_images_falls_back_to_discogs_when_no_folder_jpg() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DISCOGS_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/database/search"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [{ "cover_image": format!("{}/cover.png", server.uri()) }]
                })),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/cover.png"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_bytes(vec![1u8, 2, 3])
                    .insert_header("content-type", "image/png"),
            )
            .mount(&server)
            .await;

        let f = fixture("album-images-discogs");
        set_setting(&f, "discogsToken", "tok123");

        let result = super::run_cache_album_images(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        assert!(f.dir.join("art").join("album").join("original").exists());

        std::env::remove_var("BOOGIEBOX_DISCOGS_API_BASE");
    }

    // -- run_warm_lastfm_artist_info ---------------------------------------------

    #[tokio::test]
    async fn warm_lastfm_artist_info_noop_without_api_key() {
        let f = fixture("lastfm-artist-nokey");
        let result =
            super::run_warm_lastfm_artist_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn warm_lastfm_artist_info_fetches_and_caches() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "artist": {
                        "name": "Test Artist",
                        "mbid": "mbid-xyz",
                        "bio": { "summary": "sum", "content": "full" },
                        "tags": { "tag": [] }
                    }
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("lastfm-artist-fetch");
        set_setting(&f, "lastfmKey", "key1");
        let result =
            super::run_warm_lastfm_artist_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());

        let identity = {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::music::get_artist_external_identity(
                &conn,
                &boogiebox_db::music::coerce_entity_id(&f.artist_id),
            )
            .unwrap()
            .unwrap()
        };
        assert_eq!(identity.lastfm_mbid.as_deref(), Some("mbid-xyz"));

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn warm_lastfm_artist_info_uses_valid_cache_without_refetching() {
        let f = fixture("lastfm-artist-cached");
        set_setting(&f, "lastfmKey", "key1");
        let cache_key = "artist:test artist";
        let payload = crate::providers::LastFmInfoPayload {
            mbid: Some("cached-mbid".to_string()),
            canonical_name: Some("Test Artist".to_string()),
            summary: String::new(),
            full: String::new(),
            listeners: None,
            playcount: None,
            url: None,
            image: None,
            tags: Vec::new(),
        };
        let data = serde_json::to_string(&payload).unwrap();
        {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::artwork::save_lastfm_cache(&conn, cache_key, &data, 7);
        }
        // No mock server mounted at all — a network call here would fail/hang,
        // proving the cached branch short-circuits before any HTTP fetch.
        let result =
            super::run_warm_lastfm_artist_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        let identity = {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::music::get_artist_external_identity(
                &conn,
                &boogiebox_db::music::coerce_entity_id(&f.artist_id),
            )
            .unwrap()
            .unwrap()
        };
        assert_eq!(identity.lastfm_mbid.as_deref(), Some("cached-mbid"));
    }

    // -- run_enrich_artist_external_ids -------------------------------------------

    #[tokio::test]
    async fn enrich_artist_external_ids_targets_requested_ids() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search/artist"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let f = fixture("enrich-targeted");
        let payload = format!(r#"{{"artistIds":["{}"]}}"#, f.artist_id);
        let result = super::run_enrich_artist_external_ids(
            &f.state,
            &boogiebox_db::music::coerce_entity_id("job-1"),
            &f.library_id_entity(),
            Some(&payload),
        )
        .await;
        assert!(result.is_ok());

        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }

    #[tokio::test]
    async fn enrich_artist_external_ids_locks_a_pending_merge_master_on_a_provider_hit() {
        // § artist consolidation, §6.5: a merge master with no adoptable
        // identity is left `identity_lock_pending` instead of
        // `metadata_locked`; this lane must flip it to locked once a
        // provider match lands, and leave it pending on a miss.
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search/artist"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": 42, "name": "Test Artist", "picture_xl": "https://img/test.jpg"}]
            })))
            .mount(&server)
            .await;

        let f = fixture("enrich-pending-merge-hit");
        {
            let conn = f.state.db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET identity_lock_pending = 1 WHERE id = ?1",
                [&f.artist_id],
            )
            .unwrap();
        }

        let payload = format!(r#"{{"artistIds":["{}"]}}"#, f.artist_id);
        let result = super::run_enrich_artist_external_ids(
            &f.state,
            &boogiebox_db::music::coerce_entity_id("job-1"),
            &f.library_id_entity(),
            Some(&payload),
        )
        .await;
        assert!(result.is_ok());

        let (locked, pending): (i64, i64) = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT metadata_locked, identity_lock_pending FROM artists WHERE id = ?1",
                [&f.artist_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(locked, 1, "a provider match landed — must lock now");
        assert_eq!(pending, 0);

        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }

    #[tokio::test]
    async fn enrich_artist_external_ids_leaves_a_pending_merge_master_unlocked_on_a_miss() {
        // A guaranteed, deterministic miss (never a real network call): the
        // mocked Deezer search returns no results, and no Last.fm/Discogs/
        // Spotify keys are configured, so every provider branch is skipped.
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search/artist"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": [] })),
            )
            .mount(&server)
            .await;

        let f = fixture("enrich-pending-merge-miss");
        {
            let conn = f.state.db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET identity_lock_pending = 1 WHERE id = ?1",
                [&f.artist_id],
            )
            .unwrap();
        }

        let payload = format!(r#"{{"artistIds":["{}"]}}"#, f.artist_id);
        let result = super::run_enrich_artist_external_ids(
            &f.state,
            &boogiebox_db::music::coerce_entity_id("job-1"),
            &f.library_id_entity(),
            Some(&payload),
        )
        .await;
        assert!(result.is_ok());
        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");

        let (locked, pending): (i64, i64) = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT metadata_locked, identity_lock_pending FROM artists WHERE id = ?1",
                [&f.artist_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(locked, 0);
        assert_eq!(
            pending, 1,
            "a miss must leave identity_lock_pending set so the periodic backfill sweep retries"
        );
    }

    #[tokio::test]
    async fn enrich_artist_external_ids_falls_back_to_stale_scan_when_no_payload() {
        let f = fixture("enrich-stale");
        // No provider keys configured at all, so every provider branch is
        // skipped and the job only exercises the stale-artist selection query.
        let result = super::run_enrich_artist_external_ids(
            &f.state,
            &boogiebox_db::music::coerce_entity_id("job-1"),
            &f.library_id_entity(),
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    // -- run_warm_lastfm_album_info ------------------------------------------------

    #[tokio::test]
    async fn warm_lastfm_album_info_noop_without_api_key() {
        let f = fixture("lastfm-album-nokey");
        let result =
            super::run_warm_lastfm_album_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn warm_lastfm_album_info_skips_when_already_cached() {
        let f = fixture("lastfm-album-cached");
        set_setting(&f, "lastfmKey", "key1");
        let cache_key = "album:test artist:test album";
        {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::artwork::save_lastfm_cache(&conn, cache_key, "{}", 7);
        }
        // No mock server mounted — proves the cached branch short-circuits.
        let result =
            super::run_warm_lastfm_album_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn warm_lastfm_album_info_fetches_and_caches() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "album": { "name": "Test Album", "artist": "Test Artist" }
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("lastfm-album-fetch");
        set_setting(&f, "lastfmKey", "key1");
        let result =
            super::run_warm_lastfm_album_info(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        let cache_key = "album:test artist:test album";
        let cached = {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::artwork::get_lastfm_cached(&conn, cache_key)
        };
        assert!(cached.is_some());

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    // -- run_warm_track_lyrics -----------------------------------------------------

    #[tokio::test]
    async fn warm_track_lyrics_skips_unknown_and_already_cached_tracks() {
        let f = fixture("lyrics-skip");
        {
            let conn = f.state.db.lock().unwrap();
            conn.execute(
                "INSERT INTO lyrics_cache(track_id, artist, title, lyrics, source, fetched_at, updated_at) \
                 VALUES (?, 'Test Artist', 'Test Track', 'la', 'lrclib', datetime('now'), datetime('now'))",
                rusqlite::params![f.track_id],
            )
            .unwrap();
        }
        let payload = format!(r#"{{"trackIds":["{}", "does-not-exist"]}}"#, f.track_id);
        // No mock server mounted — proves both the missing-track and
        // already-cached branches short-circuit before any HTTP fetch.
        let result =
            super::run_warm_track_lyrics(&f.state, &f.library_id_entity(), Some(&payload)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn warm_track_lyrics_fetches_from_lrclib_then_falls_back_to_lyricsovh() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LRCLIB_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_LYRICS_OVH_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/get"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "lyrics": "fallback lyrics"
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("lyrics-fallback");
        let result = super::run_warm_track_lyrics(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
        let stored: String = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT lyrics FROM lyrics_cache WHERE track_id = ?",
                rusqlite::params![f.track_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(stored, "fallback lyrics");

        std::env::remove_var("BOOGIEBOX_LRCLIB_API_BASE");
        std::env::remove_var("BOOGIEBOX_LYRICS_OVH_API_BASE");
    }

    // -- run_sync_artist_styles ----------------------------------------------------

    #[tokio::test]
    async fn sync_artist_styles_noop_without_api_key() {
        let f = fixture("styles-nokey");
        let result = super::run_sync_artist_styles(&f.state, &f.library_id_entity()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sync_artist_styles_filters_low_count_and_blacklisted_tags() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "toptags": { "tag": [
                        { "name": "seen live", "count": 999 },
                        { "name": "hi", "count": 999 },
                        { "name": "rock", "count": 5 }
                    ] }
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("styles-filtered");
        set_setting(&f, "lastfmKey", "key1");
        let result = super::run_sync_artist_styles(&f.state, &f.library_id_entity()).await;
        assert!(result.is_ok());
        let count: i64 = {
            let conn = f.state.db.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM artist_styles WHERE artist_id = ?",
                rusqlite::params![f.artist_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(count, 0, "all tags fail the filter, so none should persist");

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn sync_artist_styles_persists_valid_tags() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "toptags": { "tag": [
                        { "name": "trip-hop", "count": 100 },
                        { "name": "electronic", "count": 50 }
                    ] }
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("styles-valid");
        set_setting(&f, "lastfmKey", "key1");
        let result = super::run_sync_artist_styles(&f.state, &f.library_id_entity()).await;
        assert!(result.is_ok());
        let styles: Vec<String> = {
            let conn = f.state.db.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT style FROM artist_styles WHERE artist_id = ? ORDER BY style")
                .unwrap();
            stmt.query_map(rusqlite::params![f.artist_id], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            styles,
            vec!["electronic".to_string(), "trip-hop".to_string()]
        );

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    // -- run_sync_discogs_album_metadata --------------------------------------------

    #[tokio::test]
    async fn sync_discogs_album_metadata_noop_without_any_provider_creds() {
        let f = fixture("discogs-meta-nocreds");
        let result =
            super::run_sync_discogs_album_metadata(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sync_discogs_album_metadata_skips_album_that_already_has_everything() {
        let f = fixture("discogs-meta-complete");
        set_setting(&f, "discogsToken", "tok");
        {
            let conn = f.state.db.lock().unwrap();
            boogiebox_db::artwork::set_album_label(&conn, &f.album_id, "Real Label");
            boogiebox_db::artwork::set_album_year(&conn, &f.album_id, 2000);
            boogiebox_db::artwork::set_album_release_type(&conn, &f.album_id, "compilation");
        }
        // No mock server mounted — proves the "nothing needed" branch
        // short-circuits before calling `search_metadata`.
        let result =
            super::run_sync_discogs_album_metadata(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sync_discogs_album_metadata_resolves_label_year_and_release_type() {
        let _guard = crate::providers::provider_fetch_tests::ENV_LOCK
            .lock()
            .await;
        let server = wiremock::MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DISCOGS_API_BASE", server.uri());
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/database/search"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [{
                        "title": "Test Artist - Test Album",
                        "year": "1999",
                        "label": ["Real Records"],
                        "type": "release"
                    }]
                })),
            )
            .mount(&server)
            .await;

        let f = fixture("discogs-meta-resolve");
        set_setting(&f, "discogsToken", "tok");

        let result =
            super::run_sync_discogs_album_metadata(&f.state, &f.library_id_entity(), None).await;
        assert!(result.is_ok());

        let (label, year): (Option<String>, Option<i64>) = {
            let conn = f.state.db.lock().unwrap();
            (
                boogiebox_db::artwork::get_album_label(&conn, &f.album_id),
                boogiebox_db::artwork::get_album_year(&conn, &f.album_id),
            )
        };
        assert_eq!(label.as_deref(), Some("Real Records"));
        assert_eq!(year, Some(1999));

        std::env::remove_var("BOOGIEBOX_DISCOGS_API_BASE");
    }

    impl Fixture {
        fn library_id_entity(&self) -> boogiebox_db::music::EntityId {
            boogiebox_db::music::coerce_entity_id(&self.library_id)
        }
    }
}

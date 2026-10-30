//! Defines Rust API routes for Playlist Routes server behavior.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use boogiebox_db::{
    backup::PathRemap,
    music::coerce_entity_id,
    playlist_import::{
        decode_playlist, load_library_snapshot, parse_playlist, preview_import, MAX_IMPORT_BYTES,
        MAX_IMPORT_ENTRIES,
    },
    playlists::{
        add_track_to_playlist, batch_add_tracks, create_playlist, create_playlist_with_tracks,
        delete_playlist, get_playlist, get_playlist_export_tracks, list_playlist_tracks,
        list_playlists, remove_track_from_playlist, reorder_playlist_tracks, set_album_rating,
        set_artist_rating, set_track_rating, update_album_metadata, update_artist_metadata,
        update_playlist, update_track_progress, AddTrackError, AlbumMetadataUpdate,
        ArtistMetadataUpdate, CreatePlaylistError, EditError, UpdatePlaylistError,
    },
    smart_playlists::{
        self as smart, create_smart_playlist, ensure_fresh, parse_definition, playlist_snapshot,
        save_as_ordinary, smart_definition, update_smart_definition, RuleError, SmartError,
        ValidDefinition,
    },
};
use serde::Deserialize;

use crate::{auth::AuthenticatedUser, DbPool, ErrorResponse, OkResponse, SharedState};

/// Documents the Playlist Router public API surface.
pub fn playlist_router(state: SharedState) -> Router {
    Router::new()
        .route(
            "/api/playlists",
            get(list_playlists_handler).post(create_playlist_handler),
        )
        .route(
            "/api/playlists/{id}",
            get(get_playlist_handler)
                .put(update_playlist_handler)
                .delete(delete_playlist_handler),
        )
        .route(
            "/api/playlists/{id}/tracks",
            get(list_tracks_handler).post(add_track_handler),
        )
        .route(
            "/api/playlists/{id}/tracks/batch",
            post(batch_add_tracks_handler),
        )
        .route(
            "/api/playlists/{id}/tracks/{track_id}",
            delete(remove_track_handler),
        )
        .route(
            "/api/playlists/{id}/tracks/order",
            put(reorder_tracks_handler),
        )
        .route(
            "/api/playlists/{id}/tracks/{track_id}/progress",
            patch(update_track_progress_handler),
        )
        .route("/api/playlists/{id}/export.m3u", get(export_m3u_handler))
        .route(
            "/api/playlists/import/preview",
            post(import_preview_handler),
        )
        .route("/api/playlists/import/commit", post(import_commit_handler))
        .route("/api/playlists/smart-preview", post(smart_preview_handler))
        .route(
            "/api/playlists/{id}/smart-definition",
            get(get_smart_definition_handler).put(put_smart_definition_handler),
        )
        .route("/api/playlists/{id}/snapshot", get(snapshot_handler))
        .route("/api/playlists/{id}/refresh", post(refresh_handler))
        .route("/api/playlists/{id}/static-copy", post(static_copy_handler))
        .route("/api/albums/{id}/rating", patch(album_rating_handler))
        .route("/api/artists/{id}/rating", patch(artist_rating_handler))
        .route("/api/tracks/{id}/rating", patch(track_rating_handler))
        .route("/api/albums/{id}/metadata", put(album_metadata_handler))
        .route("/api/artists/{id}/metadata", put(artist_metadata_handler))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct PlaylistPayload {
    name: Option<String>,
    description: Option<String>,
    remember_progress: Option<i64>,
    /// `manual` (default) or `smart`; only read on create.
    kind: Option<String>,
    /// The rules, required when `kind` is `smart`.
    definition: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmartDefinitionPayload {
    definition: Option<serde_json::Value>,
    expected_edit_revision: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct StaticCopyPayload {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddTrackPayload {
    track_id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BatchAddPayload {
    track_ids: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct ReorderPayload {
    track_ids: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportPreviewPayload {
    content_base64: Option<String>,
    #[serde(default)]
    remaps: Vec<PathRemap>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportCommitPayload {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    track_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProgressPayload {
    seconds: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RatingPayload {
    rating: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlbumMetadataPayload {
    title: Option<String>,
    #[serde(alias = "album_artist")]
    album_artist: Option<String>,
    year: Option<serde_json::Value>,
    genre: Option<String>,
    description: Option<String>,
    label: Option<String>,
    release_type: Option<String>,
    discogs_release_type: Option<String>,
    spotify_release_type: Option<String>,
    #[serde(alias = "reset_lock")]
    reset_lock: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ArtistMetadataPayload {
    name: Option<String>,
    description: Option<String>,
    #[serde(alias = "resetLock")]
    reset_lock: Option<bool>,
}

fn get_db(state: &SharedState) -> Option<DbPool> {
    state.read().unwrap_or_else(|p| p.into_inner()).db.clone()
}

fn setup_required() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn internal_error() -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
}

fn can_edit_metadata(user: &AuthenticatedUser) -> bool {
    user.is_admin() || user.can_edit_metadata
}

fn parse_entity(
    value: &serde_json::Value,
    field: &str,
) -> Result<boogiebox_db::music::EntityId, String> {
    match value {
        serde_json::Value::String(s) if !s.trim().is_empty() => Ok(coerce_entity_id(s.trim())),
        serde_json::Value::Number(n) => Ok(coerce_entity_id(&n.to_string())),
        _ => Err(format!("{field} is required")),
    }
}

fn parse_rating(payload: RatingPayload) -> Result<Option<f64>, &'static str> {
    let Some(value) = payload.rating else {
        return Err("rating must be null or a 0.5-step value between 0.5 and 5");
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => {
            let Some(rating) = n.as_f64() else {
                return Err("rating must be null or a 0.5-step value between 0.5 and 5");
            };
            if (0.5..=5.0).contains(&rating)
                && ((rating * 2.0).round() - rating * 2.0).abs() < f64::EPSILON
            {
                Ok(Some(rating))
            } else {
                Err("rating must be null or a 0.5-step value between 0.5 and 5")
            }
        }
        _ => Err("rating must be null or a 0.5-step value between 0.5 and 5"),
    }
}

fn parse_metadata_year(value: Option<serde_json::Value>) -> Option<i64> {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_i64(),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn sanitize_attachment_filename(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| match ch {
            '"' | '\\' | '/' | ':' | '*' | '?' | '<' | '>' | '|' | '\r' | '\n' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned();
    if sanitized.is_empty() {
        "playlist.m3u".to_owned()
    } else {
        sanitized
    }
}

async fn list_playlists_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || list_playlists(&db.lock().expect("db"), &user_id))
        .await
    {
        Ok(Ok(rows)) => (StatusCode::OK, Json(rows)).into_response(),
        _ => internal_error(),
    }
}

async fn create_playlist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<PlaylistPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(name) = payload
        .name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "Name is required");
    };
    let description = payload.description.unwrap_or_default().trim().to_owned();
    let user_id = user.id;
    match payload.kind.as_deref() {
        None | Some("manual") => {}
        Some("smart") => {
            let def = match rules_from(payload.definition) {
                Ok(def) => def,
                Err(response) => return *response,
            };
            return match tokio::task::spawn_blocking(move || {
                create_smart_playlist(&db.lock().expect("db"), &name, &description, &user_id, &def)
            })
            .await
            {
                Ok(Ok(row)) => (StatusCode::CREATED, Json(row)).into_response(),
                Ok(Err(e)) => smart_error(e),
                Err(_) => internal_error(),
            };
        }
        Some(_) => return error(StatusCode::BAD_REQUEST, "kind must be manual or smart"),
    }
    match tokio::task::spawn_blocking(move || {
        create_playlist(&db.lock().expect("db"), &name, &description, &user_id)
    })
    .await
    {
        Ok(Ok(row)) => (StatusCode::CREATED, Json(row)).into_response(),
        Ok(Err(CreatePlaylistError::NameTaken)) => error(
            StatusCode::CONFLICT,
            "A playlist with this name already exists",
        ),
        _ => internal_error(),
    }
}

async fn get_playlist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        get_playlist(&db.lock().expect("db"), &playlist_id, &user_id)
    })
    .await
    {
        Ok(Ok(Some(row))) => (StatusCode::OK, Json(row)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        _ => internal_error(),
    }
}

async fn update_playlist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<PlaylistPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(name) = payload
        .name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "Name is required");
    };
    let description = payload.description.unwrap_or_default().trim().to_owned();
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        update_playlist(
            &db.lock().expect("db"),
            &playlist_id,
            &name,
            &description,
            payload.remember_progress,
            &user_id,
        )
    })
    .await
    {
        Ok(Ok(Some(row))) => (StatusCode::OK, Json(row)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        Ok(Err(UpdatePlaylistError::NameTaken)) => error(
            StatusCode::CONFLICT,
            "A playlist with this name already exists",
        ),
        _ => internal_error(),
    }
}

async fn delete_playlist_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        delete_playlist(&db.lock().expect("db"), &playlist_id, &user_id)
    })
    .await
    {
        Ok(Ok(())) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        _ => internal_error(),
    }
}

async fn list_tracks_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        // A read: when the refresh fails, the last result is still served.
        if let Err(e) = ensure_fresh(&conn, &playlist_id, &user_id, false) {
            tracing::warn!("Smart playlist refresh failed before listing tracks: {e:?}");
        }
        list_playlist_tracks(&conn, &playlist_id, &user_id)
    })
    .await
    {
        Ok(Ok(Some(rows))) => (StatusCode::OK, Json(rows)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        _ => internal_error(),
    }
}

async fn add_track_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<AddTrackPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(raw_track_id) = payload.track_id else {
        return error(StatusCode::BAD_REQUEST, "track_id is required");
    };
    let track_id = match parse_entity(&raw_track_id, "track_id") {
        Ok(id) => id,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        add_track_to_playlist(&db.lock().expect("db"), &playlist_id, &track_id, &user_id)
    })
    .await
    {
        Ok(Ok(position)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "ok": true, "position": position })),
        )
            .into_response(),
        Ok(Err(AddTrackError::PlaylistNotFound)) => {
            error(StatusCode::NOT_FOUND, "Playlist not found")
        }
        Ok(Err(AddTrackError::TrackAlreadyInPlaylist)) => {
            error(StatusCode::CONFLICT, "Track already in playlist")
        }
        Ok(Err(AddTrackError::SmartPlaylist)) => smart_playlist_refusal(),
        _ => internal_error(),
    }
}

async fn batch_add_tracks_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<BatchAddPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(raw_ids) = payload.track_ids.filter(|ids| !ids.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "track_ids array is required");
    };
    let mut track_ids = Vec::with_capacity(raw_ids.len());
    for raw_id in &raw_ids {
        match parse_entity(raw_id, "track_ids") {
            Ok(id) => track_ids.push(id),
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        }
    }
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        batch_add_tracks(&db.lock().expect("db"), &playlist_id, &track_ids, &user_id)
    })
    .await
    {
        Ok(Ok(Some(added))) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "added": added })),
        )
            .into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        Ok(Err(EditError::SmartPlaylist)) => smart_playlist_refusal(),
        _ => internal_error(),
    }
}

async fn remove_track_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path((id, track_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let track_id = coerce_entity_id(&track_id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        remove_track_from_playlist(&db.lock().expect("db"), &playlist_id, &track_id, &user_id)
    })
    .await
    {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        Ok(Err(EditError::SmartPlaylist)) => smart_playlist_refusal(),
        _ => internal_error(),
    }
}

async fn reorder_tracks_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<ReorderPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(raw_ids) = payload.track_ids else {
        return error(StatusCode::BAD_REQUEST, "track_ids required");
    };
    let mut track_ids = Vec::with_capacity(raw_ids.len());
    for raw_id in &raw_ids {
        match parse_entity(raw_id, "track_ids") {
            Ok(id) => track_ids.push(id),
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        }
    }
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        reorder_playlist_tracks(&db.lock().expect("db"), &playlist_id, &track_ids, &user_id)
    })
    .await
    {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        Ok(Err(EditError::SmartPlaylist)) => smart_playlist_refusal(),
        _ => internal_error(),
    }
}

async fn update_track_progress_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path((id, track_id)): Path<(String, String)>,
    Json(payload): Json<ProgressPayload>,
) -> impl IntoResponse {
    let Some(seconds) = payload
        .seconds
        .filter(|value| value.is_finite() && *value >= 0.0)
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "seconds must be a non-negative number",
        );
    };
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let track_id = coerce_entity_id(&track_id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        update_track_progress(
            &db.lock().expect("db"),
            &playlist_id,
            &track_id,
            seconds,
            &user_id,
        )
    })
    .await
    {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        _ => internal_error(),
    }
}

async fn export_m3u_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        let conn = db.lock().expect("db");
        ensure_fresh(&conn, &playlist_id, &user_id, false)?;
        Ok::<_, SmartError>(get_playlist_export_tracks(&conn, &playlist_id, &user_id)?)
    })
    .await
    {
        Ok(Err(e)) => smart_error(e),
        Ok(Ok(Some((name, tracks)))) => {
            let mut lines = vec!["#EXTM3U".to_owned()];
            for track in tracks {
                let duration = track
                    .duration
                    .filter(|value| value.is_finite())
                    .map(|value| value.round().max(-1.0) as i64)
                    .unwrap_or(-1);
                let label = [track.artist, track.title]
                    .into_iter()
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" - ");
                lines.push(format!("#EXTINF:{duration},{label}"));
                lines.push(track.file_path);
            }
            let body = format!("{}\r\n", lines.join("\r\n"));
            let filename = sanitize_attachment_filename(&format!("{name}.m3u"));
            (
                StatusCode::OK,
                [
                    (
                        header::CONTENT_TYPE,
                        "audio/x-mpegurl; charset=utf-8".to_owned(),
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{filename}\""),
                    ),
                ],
                body,
            )
                .into_response()
        }
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        _ => internal_error(),
    }
}

/// An import error the client shows with its own copy, keyed by `code`.
fn import_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": message, "code": code })),
    )
        .into_response()
}

async fn import_preview_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Json(payload): Json<ImportPreviewPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(bytes) = payload
        .content_base64
        .as_deref()
        .and_then(|raw| STANDARD.decode(raw.trim()).ok())
    else {
        return error(StatusCode::BAD_REQUEST, "contentBase64 is required");
    };
    if bytes.len() > MAX_IMPORT_BYTES {
        return import_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_big",
            "This file is too big",
        );
    }
    let entries = parse_playlist(&decode_playlist(&bytes));
    if entries.is_empty() {
        return import_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "empty",
            "No tracks in this file",
        );
    }
    if entries.len() > MAX_IMPORT_ENTRIES {
        return import_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_long",
            "This playlist is too long",
        );
    }
    let remaps = payload.remaps;
    match tokio::task::spawn_blocking(move || {
        // Read once under the lock; match without holding it.
        let snapshot = load_library_snapshot(&db.lock().expect("db"))?;
        Ok::<_, rusqlite::Error>(preview_import(&snapshot, &entries, &remaps))
    })
    .await
    {
        Ok(Ok(preview)) => (StatusCode::OK, Json(preview)).into_response(),
        _ => internal_error(),
    }
}

async fn import_commit_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<ImportCommitPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(name) = payload
        .name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "Name is required");
    };
    if payload.track_ids.is_empty() || payload.track_ids.len() > MAX_IMPORT_ENTRIES {
        return error(
            StatusCode::BAD_REQUEST,
            "trackIds must hold 1 to 10,000 track ids",
        );
    }
    let description = payload.description.unwrap_or_default().trim().to_owned();
    let track_ids = payload.track_ids;
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        create_playlist_with_tracks(
            &db.lock().expect("db"),
            &name,
            &description,
            &user_id,
            &track_ids,
        )
    })
    .await
    {
        Ok(Ok((playlist, added))) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "playlist": playlist, "added": added })),
        )
            .into_response(),
        Ok(Err(CreatePlaylistError::NameTaken)) => error(
            StatusCode::CONFLICT,
            "A playlist with this name already exists",
        ),
        _ => internal_error(),
    }
}

// ── Smart playlists ───────────────────────────────────────────────────────────

/// The refusal a manual track edit gets on a smart playlist (approved copy).
const SMART_PLAYLIST_REFUSAL: &str =
    "Smart playlists pick their own tracks. Edit the rules instead.";

/// Previews waiting for the database at once; more get 429 `busy` (the editor previews again on its next
/// change) instead of queueing without bound.
const MAX_PENDING_PREVIEWS: usize = 4;

fn preview_permits() -> std::sync::Arc<tokio::sync::Semaphore> {
    static PERMITS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    PERMITS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_PREVIEWS)))
        .clone()
}

fn smart_playlist_refusal() -> Response {
    import_error(
        StatusCode::CONFLICT,
        "smart_playlist",
        SMART_PLAYLIST_REFUSAL,
    )
}

fn invalid_rules(errors: Vec<RuleError>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": "These rules are not valid",
            "code": "invalid_rules",
            "errors": errors,
        })),
    )
        .into_response()
}

/// The validated rules, or the response refusing them (boxed: a `Response` is large).
fn rules_from(value: Option<serde_json::Value>) -> Result<ValidDefinition, Box<Response>> {
    let Some(value) = value else {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "definition is required",
        )));
    };
    parse_definition(&value.to_string()).map_err(|errors| Box::new(invalid_rules(errors)))
}

fn smart_error(e: SmartError) -> Response {
    match e {
        SmartError::NotFound => error(StatusCode::NOT_FOUND, "Playlist not found"),
        SmartError::NameTaken => error(
            StatusCode::CONFLICT,
            "A playlist with this name already exists",
        ),
        SmartError::Invalid(errors) => invalid_rules(errors),
        SmartError::Conflict { edit_revision } => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "The rules changed since they were loaded",
                "code": "edit_conflict",
                "editRevision": edit_revision,
            })),
        )
            .into_response(),
        SmartError::Unsupported => import_error(
            StatusCode::CONFLICT,
            "unsupported",
            "These rules were made by a newer version of BoogieBox",
        ),
        SmartError::TimedOut => import_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "timed_out",
            "The rules took too long to evaluate",
        ),
        SmartError::Db(e) => {
            tracing::warn!("Smart playlist database error: {e}");
            internal_error()
        }
    }
}

async fn smart_preview_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(payload): Json<SmartDefinitionPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let def = match rules_from(payload.definition) {
        Ok(def) => def,
        Err(response) => return *response,
    };
    let Ok(permit) = preview_permits().try_acquire_owned() else {
        return import_error(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "Too many previews at once",
        );
    };
    let user_id = user.id;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        smart::preview(&db.lock().expect("db"), &user_id, &def)
    })
    .await;
    match result {
        Ok(Ok(preview)) => (StatusCode::OK, Json(preview)).into_response(),
        Ok(Err(e)) => smart_error(e),
        Err(_) => internal_error(),
    }
}

async fn get_smart_definition_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        smart_definition(&db.lock().expect("db"), &playlist_id, &user_id)
    })
    .await
    {
        Ok(Ok(Some(view))) => (StatusCode::OK, Json(view)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Smart playlist not found"),
        _ => internal_error(),
    }
}

async fn put_smart_definition_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<SmartDefinitionPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(expected) = payload.expected_edit_revision else {
        return error(StatusCode::BAD_REQUEST, "expectedEditRevision is required");
    };
    let def = match rules_from(payload.definition) {
        Ok(def) => def,
        Err(response) => return *response,
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        update_smart_definition(
            &db.lock().expect("db"),
            &playlist_id,
            &user_id,
            &def,
            expected,
        )
    })
    .await
    {
        Ok(Ok(view)) => (StatusCode::OK, Json(view)).into_response(),
        Ok(Err(e)) => smart_error(e),
        Err(_) => internal_error(),
    }
}

async fn snapshot_response(
    state: SharedState,
    user: AuthenticatedUser,
    id: String,
    force: bool,
) -> Response {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        playlist_snapshot(&db.lock().expect("db"), &playlist_id, &user_id, force)
    })
    .await
    {
        Ok(Ok(Some(snapshot))) => (StatusCode::OK, Json(snapshot)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Playlist not found"),
        _ => internal_error(),
    }
}

/// A playlist and its tracks from one read; smart playlists are brought up to date first.
async fn snapshot_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    snapshot_response(state, user, id, false).await
}

/// Re-evaluates a smart playlist now and returns its snapshot.
async fn refresh_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> impl IntoResponse {
    snapshot_response(state, user, id, true).await
}

async fn static_copy_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<StaticCopyPayload>,
) -> impl IntoResponse {
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let Some(name) = payload
        .name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "Name is required");
    };
    let description = payload.description.unwrap_or_default().trim().to_owned();
    let playlist_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        save_as_ordinary(
            &db.lock().expect("db"),
            &playlist_id,
            &user_id,
            &name,
            &description,
        )
    })
    .await
    {
        Ok(Ok((playlist, added))) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "playlist": playlist, "added": added })),
        )
            .into_response(),
        Ok(Err(e)) => smart_error(e),
        Err(_) => internal_error(),
    }
}

async fn artist_rating_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<RatingPayload>,
) -> impl IntoResponse {
    let rating = match parse_rating(payload) {
        Ok(rating) => rating,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let entity_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        set_artist_rating(&db.lock().expect("db"), &entity_id, &user_id, rating)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Artist not found"),
        _ => internal_error(),
    }
}

async fn album_rating_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<RatingPayload>,
) -> impl IntoResponse {
    let rating = match parse_rating(payload) {
        Ok(rating) => rating,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let entity_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        set_album_rating(&db.lock().expect("db"), &entity_id, &user_id, rating)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Album not found"),
        _ => internal_error(),
    }
}

async fn track_rating_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<RatingPayload>,
) -> impl IntoResponse {
    let rating = match parse_rating(payload) {
        Ok(rating) => rating,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let entity_id = coerce_entity_id(&id);
    let user_id = user.id;
    match tokio::task::spawn_blocking(move || {
        set_track_rating(&db.lock().expect("db"), &entity_id, &user_id, rating)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Track not found"),
        _ => internal_error(),
    }
}

async fn album_metadata_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<AlbumMetadataPayload>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return error(StatusCode::FORBIDDEN, "Forbidden");
    }
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let entity_id = coerce_entity_id(&id);
    let update = AlbumMetadataUpdate {
        title: payload.title,
        album_artist: payload.album_artist,
        year: parse_metadata_year(payload.year),
        genre: payload.genre,
        description: payload.description,
        label: payload.label,
        release_type: payload.release_type,
        discogs_release_type: payload.discogs_release_type,
        spotify_release_type: payload.spotify_release_type,
        reset_lock: payload.reset_lock.unwrap_or(false),
    };
    match tokio::task::spawn_blocking(move || {
        update_album_metadata(&db.lock().expect("db"), &entity_id, update)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Album not found"),
        _ => internal_error(),
    }
}

async fn artist_metadata_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(payload): Json<ArtistMetadataPayload>,
) -> impl IntoResponse {
    if !can_edit_metadata(&user) {
        return error(StatusCode::FORBIDDEN, "Forbidden");
    }
    let Some(name) = payload
        .name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return error(StatusCode::BAD_REQUEST, "name is required");
    };
    let db = match get_db(&state) {
        Some(db) => db,
        None => return setup_required(),
    };
    let entity_id = coerce_entity_id(&id);
    let update = ArtistMetadataUpdate {
        name,
        description: payload.description,
        reset_lock: payload.reset_lock.unwrap_or(false),
    };
    match tokio::task::spawn_blocking(move || {
        update_artist_metadata(&db.lock().expect("db"), &entity_id, update)
    })
    .await
    {
        Ok(Ok(Some(result))) => (StatusCode::OK, Json(result)).into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Artist not found"),
        _ => internal_error(),
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::{json_body, new_test_app_with_pool, seed_user_session, send};
    use crate::DbPool;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use rusqlite::params;
    use uuid::Uuid;

    /// Seeds one library/artist/album/track, returning their ids.
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
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Album', ?)",
            params![album_id, artist_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
             VALUES (?, ?, ?, ?, 'Track', ?)",
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
    async fn create_list_get_update_delete_playlist_lifecycle() {
        let (app, pool) = new_test_app_with_pool("playlist-lifecycle");
        let cookie = seed_user_session(&pool, "u1");

        let (create_status, create_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/playlists")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"My Mix"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(create_status, StatusCode::CREATED);
        let created = json_body(&create_body);
        let playlist_id = created["id"].as_str().unwrap().to_owned();

        // duplicate name -> conflict
        let (dup_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/playlists")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"My Mix"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(dup_status, StatusCode::CONFLICT);

        // missing name -> bad request
        let (missing_name_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri("/api/playlists")
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_name_status, StatusCode::BAD_REQUEST);

        let (list_status, list_body) = send(
            app.clone(),
            Request::builder()
                .uri("/api/playlists")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(list_status, StatusCode::OK);
        assert_eq!(json_body(&list_body).as_array().unwrap().len(), 1);

        let (get_status, _) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/playlists/{playlist_id}"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_status, StatusCode::OK);

        let (get_missing_status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/playlists/does-not-exist")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(get_missing_status, StatusCode::NOT_FOUND);

        let (update_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/playlists/{playlist_id}"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Renamed Mix"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(update_status, StatusCode::OK);

        let (delete_status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/playlists/{playlist_id}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(delete_status, StatusCode::OK);
    }

    async fn create_playlist_id(app: axum::Router, cookie: &str) -> String {
        let (status, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/playlists")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Tracks Test"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        json_body(&body)["id"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn add_batch_add_reorder_progress_remove_and_export_tracks() {
        let (app, pool) = new_test_app_with_pool("playlist-tracks");
        let cookie = seed_user_session(&pool, "u1");
        let (_artist_id, _album_id, track_id) = seed_music(&pool);
        let playlist_id = create_playlist_id(app.clone(), &cookie).await;

        let (add_status, add_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/tracks"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"track_id":"{track_id}"}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(add_status, StatusCode::CREATED);
        assert_eq!(json_body(&add_body)["ok"], true);

        // Already in playlist -> conflict.
        let (conflict_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/tracks"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"track_id":"{track_id}"}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(conflict_status, StatusCode::CONFLICT);

        let (list_status, list_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/playlists/{playlist_id}/tracks"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(list_status, StatusCode::OK);
        assert_eq!(json_body(&list_body).as_array().unwrap().len(), 1);

        let (progress_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/playlists/{playlist_id}/tracks/{track_id}/progress"
                ))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"seconds":12.5}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(progress_status, StatusCode::OK);

        let (reorder_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/playlists/{playlist_id}/tracks/order"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{track_id}"]}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(reorder_status, StatusCode::OK);

        let (export_status, export_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/playlists/{playlist_id}/export.m3u"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(export_status, StatusCode::OK);
        let export_text = String::from_utf8(export_body).unwrap();
        assert!(export_text.starts_with("#EXTM3U"));

        let (remove_status, _) = send(
            app,
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/playlists/{playlist_id}/tracks/{track_id}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(remove_status, StatusCode::OK);
    }

    #[tokio::test]
    async fn batch_add_tracks_requires_nonempty_array_and_404s_for_missing_playlist() {
        let (app, pool) = new_test_app_with_pool("playlist-batch");
        let cookie = seed_user_session(&pool, "u1");
        let (_artist_id, _album_id, track_id) = seed_music(&pool);
        let playlist_id = create_playlist_id(app.clone(), &cookie).await;

        let (empty_status, _) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/tracks/batch"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"track_ids":[]}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(empty_status, StatusCode::BAD_REQUEST);

        let (ok_status, ok_body) = send(
            app.clone(),
            Request::builder()
                .method("POST")
                .uri(format!("/api/playlists/{playlist_id}/tracks/batch"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{track_id}"]}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(ok_status, StatusCode::OK);
        assert_eq!(json_body(&ok_body)["added"], 1);

        let (missing_status, _) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/playlists/does-not-exist/tracks/batch")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"track_ids":["{track_id}"]}}"#)))
                .unwrap(),
        )
        .await;
        assert_eq!(missing_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ratings_round_trip_validate_and_404_on_missing_entity() {
        let (app, pool) = new_test_app_with_pool("playlist-ratings");
        let cookie = seed_user_session(&pool, "u1");
        let (artist_id, album_id, track_id) = seed_music(&pool);

        for (kind, id) in [
            ("artists", &artist_id),
            ("albums", &album_id),
            ("tracks", &track_id),
        ] {
            let (status, body) = send(
                app.clone(),
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/{kind}/{id}/rating"))
                    .header("cookie", cookie.clone())
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating":4.5}"#))
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{kind} rating should succeed");
            assert_eq!(json_body(&body)["rating"], 4.5);

            let (bad_status, _) = send(
                app.clone(),
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/{kind}/{id}/rating"))
                    .header("cookie", cookie.clone())
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating":4.3}"#))
                    .unwrap(),
            )
            .await;
            assert_eq!(bad_status, StatusCode::BAD_REQUEST, "{kind} bad rating");

            let (missing_status, _) = send(
                app.clone(),
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/{kind}/does-not-exist/rating"))
                    .header("cookie", cookie.clone())
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"rating":3}"#))
                    .unwrap(),
            )
            .await;
            assert_eq!(
                missing_status,
                StatusCode::NOT_FOUND,
                "{kind} missing entity"
            );
        }
    }

    #[tokio::test]
    async fn album_and_artist_metadata_require_edit_permission_and_validate() {
        let (app, pool) = new_test_app_with_pool("playlist-metadata");
        let cookie = seed_user_session(&pool, "u1"); // plain user: cannot edit metadata
        let (artist_id, album_id, _track_id) = seed_music(&pool);

        let (forbidden_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/albums/{album_id}/metadata"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"New Title"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(forbidden_status, StatusCode::FORBIDDEN);

        // Grant metadata-edit permission directly and retry.
        pool.lock()
            .unwrap()
            .execute("UPDATE users SET can_edit_metadata = 1 WHERE id = 'u1'", [])
            .unwrap();

        let (album_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/albums/{album_id}/metadata"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"New Title","year":1999}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(album_status, StatusCode::OK);

        let (artist_status, _) = send(
            app.clone(),
            Request::builder()
                .method("PUT")
                .uri(format!("/api/artists/{artist_id}/metadata"))
                .header("cookie", cookie.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Renamed Artist"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(artist_status, StatusCode::OK);

        let (bad_name_status, _) = send(
            app,
            Request::builder()
                .method("PUT")
                .uri(format!("/api/artists/{artist_id}/metadata"))
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"  "}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(bad_name_status, StatusCode::BAD_REQUEST);
    }

    fn post_json(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn encoded(text: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine};
        STANDARD.encode(text)
    }

    #[tokio::test]
    async fn import_preview_matches_with_remaps_and_commit_keeps_order() {
        let (app, pool) = new_test_app_with_pool("playlist-import");
        let cookie = seed_user_session(&pool, "u1");
        let (artist_id, album_id, first) = seed_music(&pool);
        let second = Uuid::now_v7().to_string();
        pool.lock()
            .unwrap()
            .execute(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
                 SELECT ?, library_id, ?, ?, 'Second', ? FROM tracks WHERE id = ?",
                params![
                    second,
                    artist_id,
                    album_id,
                    format!("/music/{second}.mp3"),
                    first
                ],
            )
            .unwrap();
        let m3u = format!(
            "#EXTM3U\r\nD:\\Old\\{second}.mp3\r\n/music/{first}.mp3\r\nhttps://radio.example/live\r\nmissing.mp3\r\n"
        );

        let (status, body) = send(
            app.clone(),
            post_json(
                "/api/playlists/import/preview",
                &cookie,
                serde_json::json!({
                    "contentBase64": encoded(&m3u),
                    "remaps": [{ "from": "D:\\Old", "to": "/music" }]
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let preview = json_body(&body);
        let statuses: Vec<&str> = preview["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, ["matched", "matched", "remote", "unmatched"]);
        assert_eq!(preview["entries"][0]["trackId"], second.as_str());
        assert_eq!(preview["appliedRemaps"][0]["gain"], 1);
        assert_eq!(preview["totals"]["matched"], 2);

        let (status, body) = send(
            app.clone(),
            post_json(
                "/api/playlists/import/commit",
                &cookie,
                serde_json::json!({
                    "name": "Imported",
                    "trackIds": [second, first, second, "gone"]
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let created = json_body(&body);
        assert_eq!(created["added"], 2);
        assert_eq!(created["playlist"]["track_count"], 2);
        let playlist_id = created["playlist"]["id"].as_str().unwrap().to_owned();

        let (_, list_body) = send(
            app.clone(),
            Request::builder()
                .uri(format!("/api/playlists/{playlist_id}/tracks"))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let ids: Vec<String> = json_body(&list_body)
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(ids, [second.clone(), first.clone()]);

        let (status, _) = send(
            app,
            post_json(
                "/api/playlists/import/commit",
                &cookie,
                serde_json::json!({ "name": "imported", "trackIds": [first] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn import_preview_rejects_empty_oversized_and_missing_content() {
        let (app, pool) = new_test_app_with_pool("playlist-import-errors");
        let cookie = seed_user_session(&pool, "u1");
        let cases = [
            (
                serde_json::json!({ "contentBase64": encoded("#EXTM3U\n# nothing\n") }),
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("empty"),
            ),
            (
                serde_json::json!({ "contentBase64": encoded(&"a.mp3\n".repeat(10_001)) }),
                StatusCode::PAYLOAD_TOO_LARGE,
                Some("too_long"),
            ),
            (
                serde_json::json!({ "contentBase64": encoded(&"x".repeat(2 * 1024 * 1024 + 1)) }),
                StatusCode::PAYLOAD_TOO_LARGE,
                Some("too_big"),
            ),
            (serde_json::json!({}), StatusCode::BAD_REQUEST, None),
        ];
        for (body, want_status, want_code) in cases {
            let (status, response) = send(
                app.clone(),
                post_json("/api/playlists/import/preview", &cookie, body),
            )
            .await;
            assert_eq!(status, want_status);
            if let Some(code) = want_code {
                assert_eq!(json_body(&response)["code"], code);
            }
        }
        let (status, _) = send(
            app,
            post_json(
                "/api/playlists/import/commit",
                &cookie,
                serde_json::json!({ "name": "Empty", "trackIds": [] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    fn get_req(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    fn put_json(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn genre_rules(genre: &str) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "root": { "mode": "all", "children": [
                { "field": "genre", "operator": "is", "value": genre }
            ]}
        })
    }

    /// Seeds two Rock tracks and one Jazz track; returns their ids in title order (A, B, C).
    fn seed_genres(pool: &DbPool) -> Vec<String> {
        let (artist_id, album_id, first) = seed_music(pool);
        let conn = pool.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET title = 'A Rock', genre = 'Rock' WHERE id = ?",
            params![first],
        )
        .unwrap();
        let mut ids = vec![first.clone()];
        for (title, genre) in [("B Rock", "Rock"), ("C Jazz", "Jazz")] {
            let id = Uuid::now_v7().to_string();
            conn.execute(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, genre, file_path, duration) \
                 SELECT ?, library_id, ?, ?, ?, ?, ?, 200 FROM tracks WHERE id = ?",
                params![
                    id,
                    artist_id,
                    album_id,
                    title,
                    genre,
                    format!("/music/{id}.mp3"),
                    first
                ],
            )
            .unwrap();
            ids.push(id);
        }
        ids
    }

    #[tokio::test]
    async fn smart_playlist_lifecycle_over_the_api() {
        let (app, pool) = new_test_app_with_pool("smart-playlists");
        let cookie = seed_user_session(&pool, "u1");
        let other = seed_user_session(&pool, "u2");
        let ids = seed_genres(&pool);

        // Invalid rules are refused with their problems.
        let (status, body) = send(
            app.clone(),
            post_json(
                "/api/playlists",
                &cookie,
                serde_json::json!({ "name": "Bad", "kind": "smart", "definition": {
                    "schemaVersion": 1, "root": { "mode": "all", "children": [] } } }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let bad = json_body(&body);
        assert_eq!(bad["code"], "invalid_rules");
        assert_eq!(bad["errors"][0]["code"], "empty_group");

        // Preview stores nothing.
        let (status, body) = send(
            app.clone(),
            post_json(
                "/api/playlists/smart-preview",
                &cookie,
                serde_json::json!({ "definition": genre_rules("Rock") }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let preview = json_body(&body);
        assert_eq!(preview["trackCount"], 2);
        assert_eq!(preview["tracks"][0]["title"], "A Rock");

        let (status, body) = send(
            app.clone(),
            post_json(
                "/api/playlists",
                &cookie,
                serde_json::json!({ "name": "Rock", "kind": "smart", "definition": genre_rules("Rock") }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let created = json_body(&body);
        assert_eq!(created["kind"], "smart");
        assert_eq!(created["track_count"], 2);
        let id = created["id"].as_str().unwrap().to_owned();

        let (_, body) = send(app.clone(), get_req("/api/playlists", &cookie)).await;
        assert_eq!(json_body(&body)[0]["kind"], "smart");

        // Manual edits are refused.
        let (status, body) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{id}/tracks"),
                &cookie,
                serde_json::json!({ "track_id": ids[2] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json_body(&body)["code"], "smart_playlist");
        let (status, _) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{id}/tracks/batch"),
                &cookie,
                serde_json::json!({ "track_ids": [ids[2]] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = send(
            app.clone(),
            put_json(
                &format!("/api/playlists/{id}/tracks/order"),
                &cookie,
                serde_json::json!({ "track_ids": [ids[1], ids[0]] }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = send(
            app.clone(),
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/playlists/{id}/tracks/{}", ids[0]))
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        // A library change shows on the next read.
        pool.lock()
            .unwrap()
            .execute(
                "UPDATE tracks SET genre = 'Rock' WHERE id = ?",
                params![ids[2]],
            )
            .unwrap();
        let (status, body) = send(
            app.clone(),
            get_req(&format!("/api/playlists/{id}/snapshot"), &cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let snapshot = json_body(&body);
        assert_eq!(snapshot["tracks"].as_array().unwrap().len(), 3);
        assert_eq!(snapshot["membershipRevision"], 2);
        assert!(snapshot["refreshError"].is_null());
        let (status, _) = send(
            app.clone(),
            get_req(&format!("/api/playlists/{id}/snapshot"), &other),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "other users can't read it");

        // Rules edit: revision checked.
        let (_, body) = send(
            app.clone(),
            get_req(&format!("/api/playlists/{id}/smart-definition"), &cookie),
        )
        .await;
        let view = json_body(&body);
        assert_eq!(view["editRevision"], 1);
        assert_eq!(view["supported"], true);
        let (status, body) = send(
            app.clone(),
            put_json(
                &format!("/api/playlists/{id}/smart-definition"),
                &cookie,
                serde_json::json!({ "definition": genre_rules("Jazz"), "expectedEditRevision": 1 }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["editRevision"], 2);
        let (status, body) = send(
            app.clone(),
            put_json(
                &format!("/api/playlists/{id}/smart-definition"),
                &cookie,
                serde_json::json!({ "definition": genre_rules("Rock"), "expectedEditRevision": 1 }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let conflict = json_body(&body);
        assert_eq!(conflict["code"], "edit_conflict");
        assert_eq!(conflict["editRevision"], 2);
        let (status, _) = send(
            app.clone(),
            get_req(&format!("/api/playlists/{id}/smart-definition"), &other),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Refresh, export, ordinary copy.
        let (status, body) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{id}/refresh"),
                &cookie,
                serde_json::json!({}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json_body(&body)["tracks"].as_array().unwrap().len(),
            0,
            "the only Jazz track became Rock"
        );
        pool.lock()
            .unwrap()
            .execute(
                "UPDATE tracks SET genre = 'Jazz' WHERE id = ?",
                params![ids[2]],
            )
            .unwrap();
        let (status, body) = send(
            app.clone(),
            get_req(&format!("/api/playlists/{id}/export.m3u"), &cookie),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(String::from_utf8(body)
            .unwrap()
            .contains(&format!("/music/{}.mp3", ids[2])));
        let (status, body) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{id}/static-copy"),
                &cookie,
                serde_json::json!({ "name": "Jazz (copy)" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let copy = json_body(&body);
        assert_eq!(copy["playlist"]["kind"], "manual");
        assert_eq!(copy["added"], 1);
        let copy_id = copy["playlist"]["id"].as_str().unwrap().to_owned();
        let (status, _) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{copy_id}/tracks"),
                &cookie,
                serde_json::json!({ "track_id": ids[0] }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "the copy is an ordinary playlist"
        );
        let (status, _) = send(
            app.clone(),
            post_json(
                &format!("/api/playlists/{copy_id}/static-copy"),
                &cookie,
                serde_json::json!({ "name": "Nope" }),
            ),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "only smart playlists are copied"
        );

        // Kind is checked on create and stays fixed on rename.
        let (status, _) = send(
            app.clone(),
            post_json(
                "/api/playlists",
                &cookie,
                serde_json::json!({ "name": "X", "kind": "magic" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = send(
            app,
            put_json(
                &format!("/api/playlists/{id}"),
                &cookie,
                serde_json::json!({ "name": "Jazz" }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(&body)["kind"], "smart");
    }
}

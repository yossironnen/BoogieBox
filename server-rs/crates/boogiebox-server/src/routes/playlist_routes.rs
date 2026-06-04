//! Defines Rust API routes for Playlist Routes server behavior.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use boogiebox_db::{
    music::coerce_entity_id,
    playlists::{
        add_track_to_playlist, batch_add_tracks, create_playlist, delete_playlist, get_playlist,
        get_playlist_export_tracks, list_playlist_tracks, list_playlists,
        remove_track_from_playlist, reorder_playlist_tracks, set_album_rating, set_artist_rating,
        set_track_rating, update_album_metadata, update_artist_metadata, update_playlist,
        update_track_progress, AddTrackError, AlbumMetadataUpdate, ArtistMetadataUpdate,
        CreatePlaylistError, UpdatePlaylistError,
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
        list_playlist_tracks(&db.lock().expect("db"), &playlist_id, &user_id)
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
        get_playlist_export_tracks(&db.lock().expect("db"), &playlist_id, &user_id)
    })
    .await
    {
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

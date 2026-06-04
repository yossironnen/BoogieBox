//! Defines SQLite data access and schema helpers for Playlists.

use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::music::EntityId;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Public Playlist Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaylistRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Name public API surface.
    pub name: String,
    /// Documents the Description public API surface.
    pub description: Option<String>,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
    /// Documents the Updated At public API surface.
    pub updated_at: Option<String>,
    /// Documents the Remember Progress public API surface.
    pub remember_progress: Option<i64>,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
    /// Documents the Total Duration public API surface.
    pub total_duration: Option<f64>,
    /// Documents the Art Album Ids public API surface.
    pub art_album_ids: Vec<String>,
}

/// Public Playlist Track Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaylistTrackRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the File Name public API surface.
    pub file_name: Option<String>,
    /// Documents the File Size public API surface.
    pub file_size: Option<i64>,
    /// Documents the Format public API surface.
    pub format: Option<String>,
    /// Documents the Duration public API surface.
    pub duration: Option<f64>,
    /// Documents the Bitrate public API surface.
    pub bitrate: Option<i64>,
    /// Documents the Sample Rate public API surface.
    pub sample_rate: Option<i64>,
    /// Documents the Channels public API surface.
    pub channels: Option<i64>,
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Track Number public API surface.
    pub track_number: Option<i64>,
    /// Documents the Disc Number public API surface.
    pub disc_number: Option<i64>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// Documents the Composer public API surface.
    pub composer: Option<String>,
    /// Documents the Comment public API surface.
    pub comment: Option<String>,
    /// Documents the Bpm public API surface.
    pub bpm: Option<f64>,
    /// Documents the Bpm Detected public API surface.
    pub bpm_detected: Option<f64>,
    /// Documents the Bpm Source public API surface.
    pub bpm_source: Option<String>,
    /// Documents the Bpm Confidence public API surface.
    pub bpm_confidence: Option<f64>,
    /// Documents the Scanned At public API surface.
    pub scanned_at: Option<String>,
    /// Documents the Album Id public API surface.
    pub album_id: Option<EntityId>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Album public API surface.
    pub album: Option<String>,
    /// Documents the Library Name public API surface.
    pub library_name: Option<String>,
    /// Documents the Position public API surface.
    pub position: i64,
    /// Documents the Playlist Track Id public API surface.
    pub playlist_track_id: Option<EntityId>,
    /// Documents the Progress Seconds public API surface.
    pub progress_seconds: Option<f64>,
}

/// Public Export Track data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTrack {
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the Duration public API surface.
    pub duration: Option<f64>,
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn column_exists(conn: &Connection, table: &str, col: &str) -> bool {
    let pragma = format!("PRAGMA table_info({table})");
    conn.prepare(&pragma)
        .and_then(|mut stmt| {
            let names = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(names.iter().any(|n| n.eq_ignore_ascii_case(col)))
        })
        .unwrap_or(false)
}

fn entity_id_to_value(id: &EntityId) -> Value {
    match id {
        EntityId::Int(n) => Value::Integer(*n),
        EntityId::Str(s) => Value::Text(s.clone()),
    }
}

fn new_uuid_v7() -> String {
    Uuid::now_v7().to_string()
}

fn schema_uses_uuid(conn: &Connection) -> bool {
    conn.prepare("PRAGMA table_info(playlists)")
        .and_then(|mut stmt| {
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("id"))
                .is_none_or(|(_, column_type)| !column_type.to_uppercase().contains("INT")))
        })
        .unwrap_or(false)
}

fn find_owned_playlist(conn: &Connection, id: &EntityId, user_id: &str) -> rusqlite::Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM playlists WHERE id=? AND user_id=?",
            rusqlite::params![id, user_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

fn is_playlist_name_taken(
    conn: &Connection,
    name: &str,
    exclude_id: Option<&EntityId>,
    user_id: &str,
) -> rusqlite::Result<bool> {
    let count: i64 = if let Some(eid) = exclude_id {
        conn.query_row(
            "SELECT COUNT(*) FROM playlists WHERE LOWER(name)=LOWER(?) AND id!=? AND user_id=?",
            rusqlite::params![name, eid, user_id],
            |row| row.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM playlists WHERE LOWER(name)=LOWER(?) AND user_id=?",
            rusqlite::params![name, user_id],
            |row| row.get(0),
        )?
    };
    Ok(count > 0)
}

fn map_playlist_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaylistRow> {
    let raw_art: Option<String> = row.get(8)?;
    let art_album_ids = raw_art
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .collect();
    Ok(PlaylistRow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        remember_progress: row.get(5)?,
        track_count: row.get(6)?,
        total_duration: row.get(7)?,
        art_album_ids,
    })
}

fn art_album_ids_expr(has_album_id: bool) -> &'static str {
    if has_album_id {
        "(SELECT GROUP_CONCAT(album_id) FROM (
            SELECT DISTINCT t2.album_id AS album_id
            FROM playlist_tracks pt2
            JOIN tracks t2 ON t2.id = pt2.track_id
            WHERE pt2.playlist_id = p.id AND t2.album_id IS NOT NULL
            ORDER BY pt2.position ASC LIMIT 4
          )) AS art_album_ids"
    } else {
        "NULL AS art_album_ids"
    }
}

fn get_playlist_by_id(conn: &Connection, id: &EntityId) -> rusqlite::Result<Option<PlaylistRow>> {
    let has_album_id = column_exists(conn, "tracks", "album_id");
    let art_expr = art_album_ids_expr(has_album_id);
    let sql = format!(
        "SELECT p.id, p.name, p.description, p.created_at, p.updated_at, p.remember_progress,
                COUNT(pt.track_id) AS track_count,
                ROUND(SUM(t.duration), 0) AS total_duration,
                {art_expr}
         FROM playlists p
         LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
         LEFT JOIN tracks t ON t.id = pt.track_id
         WHERE p.id=?
         GROUP BY p.id"
    );
    conn.query_row(&sql, rusqlite::params![id], map_playlist_row)
        .optional()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Documents the List Playlists public API surface.
pub fn list_playlists(conn: &Connection, user_id: &str) -> rusqlite::Result<Vec<PlaylistRow>> {
    let has_album_id = column_exists(conn, "tracks", "album_id");
    let art_expr = art_album_ids_expr(has_album_id);
    let sql = format!(
        "SELECT p.id, p.name, p.description, p.created_at, p.updated_at, p.remember_progress,
                COUNT(pt.track_id) AS track_count,
                ROUND(SUM(t.duration), 0) AS total_duration,
                {art_expr}
         FROM playlists p
         LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
         LEFT JOIN tracks t ON t.id = pt.track_id
         WHERE p.user_id=?
         GROUP BY p.id
         ORDER BY p.updated_at DESC"
    );
    conn.prepare(&sql)?
        .query_map(rusqlite::params![user_id], map_playlist_row)?
        .collect()
}

/// Public Create Playlist Error data shape used by BoogieBox.
#[derive(Debug)]
pub enum CreatePlaylistError {
    NameTaken,
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for CreatePlaylistError {
    fn from(e: rusqlite::Error) -> Self {
        let msg = e.to_string().to_lowercase();
        if msg.contains("unique") {
            Self::NameTaken
        } else {
            Self::Db(e)
        }
    }
}

/// Documents the Create Playlist public API surface.
pub fn create_playlist(
    conn: &Connection,
    name: &str,
    description: &str,
    user_id: &str,
) -> Result<PlaylistRow, CreatePlaylistError> {
    if is_playlist_name_taken(conn, name, None, user_id)? {
        return Err(CreatePlaylistError::NameTaken);
    }
    let uses_uuid = schema_uses_uuid(conn);
    let playlist_id: EntityId = if uses_uuid {
        let new_id = new_uuid_v7();
        conn.execute(
            "INSERT INTO playlists(id, name, description, user_id) VALUES(?,?,?,?)",
            rusqlite::params![new_id, name, description, user_id],
        )?;
        EntityId::Str(new_id)
    } else {
        conn.execute(
            "INSERT INTO playlists(name, description, user_id) VALUES(?,?,?)",
            rusqlite::params![name, description, user_id],
        )?;
        EntityId::Int(conn.last_insert_rowid())
    };
    get_playlist_by_id(conn, &playlist_id)?
        .ok_or_else(|| CreatePlaylistError::Db(rusqlite::Error::QueryReturnedNoRows))
}

/// Public Update Playlist Error data shape used by BoogieBox.
#[derive(Debug)]
pub enum UpdatePlaylistError {
    NotFound,
    NameTaken,
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for UpdatePlaylistError {
    fn from(e: rusqlite::Error) -> Self {
        let msg = e.to_string().to_lowercase();
        if msg.contains("unique") {
            Self::NameTaken
        } else {
            Self::Db(e)
        }
    }
}

/// Documents the Get Playlist public API surface.
pub fn get_playlist(
    conn: &Connection,
    id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<Option<PlaylistRow>> {
    let has_album_id = column_exists(conn, "tracks", "album_id");
    let art_expr = art_album_ids_expr(has_album_id);
    let sql = format!(
        "SELECT p.id, p.name, p.description, p.created_at, p.updated_at, p.remember_progress,
                COUNT(pt.track_id) AS track_count,
                ROUND(SUM(t.duration), 0) AS total_duration,
                {art_expr}
         FROM playlists p
         LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
         LEFT JOIN tracks t ON t.id = pt.track_id
         WHERE p.id=? AND p.user_id=?
         GROUP BY p.id"
    );
    conn.query_row(&sql, rusqlite::params![id, user_id], map_playlist_row)
        .optional()
}

/// Documents the Update Playlist public API surface.
pub fn update_playlist(
    conn: &Connection,
    id: &EntityId,
    name: &str,
    description: &str,
    remember_progress: Option<i64>,
    user_id: &str,
) -> Result<Option<PlaylistRow>, UpdatePlaylistError> {
    if !find_owned_playlist(conn, id, user_id)? {
        return Ok(None);
    }
    if is_playlist_name_taken(conn, name, Some(id), user_id)? {
        return Err(UpdatePlaylistError::NameTaken);
    }
    let changes = conn.execute(
        "UPDATE playlists SET name=?, description=?, remember_progress=COALESCE(?,remember_progress), updated_at=datetime('now') WHERE id=? AND user_id=?",
        rusqlite::params![name, description, remember_progress, id, user_id],
    )?;
    if changes == 0 {
        return Ok(None);
    }
    Ok(get_playlist_by_id(conn, id)?)
}

/// Documents the Delete Playlist public API surface.
pub fn delete_playlist(conn: &Connection, id: &EntityId, user_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM playlists WHERE id=? AND user_id=?",
        rusqlite::params![id, user_id],
    )?;
    Ok(())
}

/// Documents the List Playlist Tracks public API surface.
pub fn list_playlist_tracks(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<Option<Vec<PlaylistTrackRow>>> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Ok(None);
    }
    let rows = conn
        .prepare(
            "SELECT t.id, t.file_name, t.file_size, t.format,
                    t.duration, t.bitrate, t.sample_rate, t.channels,
                    t.title, t.track_number, t.disc_number, t.year, t.genre,
                    t.composer, t.comment, t.bpm, t.bpm_detected, t.bpm_source, t.bpm_confidence, t.scanned_at,
                    t.album_id, ar.name AS artist, al.title AS album, l.name AS library_name,
                    pt.position, pt.id AS playlist_track_id, pt.progress_seconds
             FROM playlist_tracks pt
             JOIN tracks t ON t.id = pt.track_id
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id
             LEFT JOIN libraries l ON l.id = t.library_id
             WHERE pt.playlist_id=?
             ORDER BY pt.position ASC, pt.id ASC",
        )?
        .query_map(rusqlite::params![playlist_id], |row| {
            Ok(PlaylistTrackRow {
                id: row.get(0)?,
                file_name: row.get(1)?,
                file_size: row.get(2)?,
                format: row.get(3)?,
                duration: row.get(4)?,
                bitrate: row.get(5)?,
                sample_rate: row.get(6)?,
                channels: row.get(7)?,
                title: row.get(8)?,
                track_number: row.get(9)?,
                disc_number: row.get(10)?,
                year: row.get(11)?,
                genre: row.get(12)?,
                composer: row.get(13)?,
                comment: row.get(14)?,
                bpm: row.get(15)?,
                bpm_detected: row.get(16)?,
                bpm_source: row.get(17)?,
                bpm_confidence: row.get(18)?,
                scanned_at: row.get(19)?,
                album_id: row.get(20)?,
                artist: row.get(21)?,
                album: row.get(22)?,
                library_name: row.get(23)?,
                position: row.get(24)?,
                playlist_track_id: row.get(25)?,
                progress_seconds: row.get(26)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(rows))
}

/// Public Add Track Error data shape used by BoogieBox.
#[derive(Debug)]
pub enum AddTrackError {
    PlaylistNotFound,
    TrackAlreadyInPlaylist,
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for AddTrackError {
    fn from(e: rusqlite::Error) -> Self {
        let msg = e.to_string().to_lowercase();
        if msg.contains("unique") {
            Self::TrackAlreadyInPlaylist
        } else {
            Self::Db(e)
        }
    }
}

/// Documents the Add Track To Playlist public API surface.
pub fn add_track_to_playlist(
    conn: &Connection,
    playlist_id: &EntityId,
    track_id: &EntityId,
    user_id: &str,
) -> Result<i64, AddTrackError> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Err(AddTrackError::PlaylistNotFound);
    }
    let max_pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id=?",
        rusqlite::params![playlist_id],
        |row| row.get(0),
    )?;
    let position = max_pos + 1;
    conn.execute(
        "INSERT INTO playlist_tracks(playlist_id, track_id, position) VALUES(?,?,?)",
        rusqlite::params![playlist_id, track_id, position],
    )?;
    conn.execute(
        "UPDATE playlists SET updated_at=datetime('now') WHERE id=?",
        rusqlite::params![playlist_id],
    )?;
    Ok(position)
}

/// Documents the Batch Add Tracks public API surface.
pub fn batch_add_tracks(
    conn: &Connection,
    playlist_id: &EntityId,
    track_ids: &[EntityId],
    user_id: &str,
) -> rusqlite::Result<Option<i64>> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Ok(None);
    }
    let max_pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id=?",
        rusqlite::params![playlist_id],
        |row| row.get(0),
    )?;
    let mut pos = max_pos + 1;
    let mut added: i64 = 0;
    for tid in track_ids {
        let changes = conn.execute(
            "INSERT OR IGNORE INTO playlist_tracks(playlist_id, track_id, position) VALUES(?,?,?)",
            rusqlite::params![playlist_id, tid, pos],
        )?;
        if changes > 0 {
            pos += 1;
            added += 1;
        }
    }
    conn.execute(
        "UPDATE playlists SET updated_at=datetime('now') WHERE id=?",
        rusqlite::params![playlist_id],
    )?;
    Ok(Some(added))
}

/// Documents the Remove Track From Playlist public API surface.
pub fn remove_track_from_playlist(
    conn: &Connection,
    playlist_id: &EntityId,
    track_id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<bool> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id=? AND track_id=?",
        rusqlite::params![playlist_id, track_id],
    )?;
    conn.execute(
        "UPDATE playlists SET updated_at=datetime('now') WHERE id=?",
        rusqlite::params![playlist_id],
    )?;
    Ok(true)
}

/// Documents the Reorder Playlist Tracks public API surface.
pub fn reorder_playlist_tracks(
    conn: &Connection,
    playlist_id: &EntityId,
    track_ids: &[EntityId],
    user_id: &str,
) -> rusqlite::Result<bool> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Ok(false);
    }
    for (pos, tid) in track_ids.iter().enumerate() {
        conn.execute(
            "UPDATE playlist_tracks SET position=? WHERE playlist_id=? AND track_id=?",
            rusqlite::params![pos as i64, playlist_id, tid],
        )?;
    }
    conn.execute(
        "UPDATE playlists SET updated_at=datetime('now') WHERE id=?",
        rusqlite::params![playlist_id],
    )?;
    Ok(true)
}

/// Documents the Update Track Progress public API surface.
pub fn update_track_progress(
    conn: &Connection,
    playlist_id: &EntityId,
    track_id: &EntityId,
    seconds: f64,
    user_id: &str,
) -> rusqlite::Result<bool> {
    if !find_owned_playlist(conn, playlist_id, user_id)? {
        return Ok(false);
    }
    conn.execute(
        "UPDATE playlist_tracks SET progress_seconds=? WHERE playlist_id=? AND track_id=?",
        rusqlite::params![seconds, playlist_id, track_id],
    )?;
    Ok(true)
}

/// Documents the Get Playlist Export Tracks public API surface.
pub fn get_playlist_export_tracks(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<Option<(String, Vec<ExportTrack>)>> {
    let playlist: Option<(EntityId, String)> = conn
        .query_row(
            "SELECT id, name FROM playlists WHERE id=? AND user_id=?",
            rusqlite::params![playlist_id, user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((_, name)) = playlist else {
        return Ok(None);
    };
    let tracks = conn
        .prepare(
            "SELECT t.file_path, t.duration, COALESCE(NULLIF(t.title,''), t.file_name) AS title, ar.name AS artist
             FROM playlist_tracks pt
             JOIN tracks t ON t.id = pt.track_id
             LEFT JOIN artists ar ON ar.id = t.artist_id
             WHERE pt.playlist_id=?
             ORDER BY pt.position ASC, pt.id ASC",
        )?
        .query_map(rusqlite::params![playlist_id], |row| {
            Ok(ExportTrack {
                file_path: row.get(0)?,
                duration: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some((name, tracks)))
}

/// Public Rating Update Result data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct RatingUpdateResult {
    /// Documents the Ok public API surface.
    pub ok: bool,
    /// Documents the Rating public API surface.
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<i64>,
}

/// Public Metadata Update Result data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct MetadataUpdateResult {
    /// Documents the Ok public API surface.
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<EntityId>,
}

/// Public Album Metadata Update data shape used by BoogieBox.
#[derive(Debug, Default)]
pub struct AlbumMetadataUpdate {
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Album Artist public API surface.
    pub album_artist: Option<String>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// Documents the Description public API surface.
    pub description: Option<String>,
    /// Documents the Release Type public API surface.
    pub release_type: Option<String>,
    /// Documents the Discogs Release Type public API surface.
    pub discogs_release_type: Option<String>,
    /// Documents the Spotify Release Type public API surface.
    pub spotify_release_type: Option<String>,
    /// Documents the Reset Lock public API surface.
    pub reset_lock: bool,
}

/// Public Artist Metadata Update data shape used by BoogieBox.
#[derive(Debug, Default)]
pub struct ArtistMetadataUpdate {
    /// Documents the Name public API surface.
    pub name: String,
    /// Documents the Description public API surface.
    pub description: Option<String>,
    /// Documents the Reset Lock public API surface.
    pub reset_lock: bool,
}

fn valid_rating_value(rating: Option<f64>) -> bool {
    match rating {
        None => true,
        Some(value) => {
            (0.5..=5.0).contains(&value)
                && ((value * 2.0).round() - value * 2.0).abs() < f64::EPSILON
        }
    }
}

fn normalize_release_type(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("album") => Some("album"),
        Some("single") => Some("single"),
        Some("compilation") => Some("compilation"),
        _ => None,
    }
}

fn resolve_release_type(
    local_release_type: Option<&str>,
    metadata_locked: i64,
    discogs_release_type: Option<&str>,
    spotify_release_type: Option<&str>,
) -> &'static str {
    let local = normalize_release_type(local_release_type);
    if metadata_locked == 1 {
        if let Some(value) = local {
            return value;
        }
    }
    normalize_release_type(discogs_release_type)
        .or_else(|| normalize_release_type(spotify_release_type))
        .or(local)
        .unwrap_or("album")
}

fn blank_to_none(value: Option<&str>) -> Option<&str> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Documents the Update Artist Metadata public API surface.
pub fn update_artist_metadata(
    conn: &Connection,
    artist_id: &EntityId,
    update: ArtistMetadataUpdate,
) -> rusqlite::Result<Option<MetadataUpdateResult>> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM artists WHERE id=?",
            rusqlite::params![artist_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(None);
    }
    let name = update.name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if let Some(description) = update.description {
        conn.execute(
            "UPDATE artists SET name=?, metadata_locked=?, description=? WHERE id=?",
            rusqlite::params![
                name,
                if update.reset_lock { 0 } else { 1 },
                blank_to_none(Some(&description)),
                artist_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE artists SET name=?, metadata_locked=? WHERE id=?",
            rusqlite::params![name, if update.reset_lock { 0 } else { 1 }, artist_id],
        )?;
    }
    Ok(Some(MetadataUpdateResult {
        ok: true,
        merged_into: None,
    }))
}

/// Documents the Update Album Metadata public API surface.
pub fn update_album_metadata(
    conn: &Connection,
    album_id: &EntityId,
    update: AlbumMetadataUpdate,
) -> rusqlite::Result<Option<MetadataUpdateResult>> {
    let current: Option<(EntityId, String, String, Option<String>, i64)> = conn
        .query_row(
            "SELECT id, title, album_artist, release_type, metadata_locked FROM albums WHERE id=?",
            rusqlite::params![album_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((_, current_title, current_album_artist, current_release_type, metadata_locked)) =
        current
    else {
        return Ok(None);
    };

    let next_release_type = resolve_release_type(
        update
            .release_type
            .as_deref()
            .or(current_release_type.as_deref()),
        metadata_locked,
        update.discogs_release_type.as_deref(),
        update.spotify_release_type.as_deref(),
    );
    let next_title = update
        .title
        .as_deref()
        .map(str::trim)
        .unwrap_or(&current_title)
        .to_owned();
    let next_album_artist = update
        .album_artist
        .as_deref()
        .map(str::trim)
        .unwrap_or(&current_album_artist)
        .to_owned();

    let conflicting_album: Option<EntityId> = conn
        .query_row(
            "SELECT id FROM albums WHERE id != ? AND title = ? AND album_artist = ?",
            rusqlite::params![album_id, next_title, next_album_artist],
            |row| row.get(0),
        )
        .optional()?;

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> rusqlite::Result<Option<EntityId>> {
        let target_id = conflicting_album.as_ref().unwrap_or(album_id);
        let mut sets: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if let Some(title) = update.title {
            sets.push("title=?");
            values.push(match blank_to_none(Some(&title)) {
                Some(value) => Value::Text(value.to_owned()),
                None => Value::Null,
            });
        }
        if let Some(album_artist) = update.album_artist {
            sets.push("album_artist=?");
            values.push(Value::Text(album_artist.trim().to_owned()));
        }
        if let Some(year) = update.year {
            sets.push("year=?");
            values.push(Value::Integer(year));
        }
        if let Some(genre) = update.genre {
            sets.push("genre=?");
            values.push(match blank_to_none(Some(&genre)) {
                Some(value) => Value::Text(value.to_owned()),
                None => Value::Null,
            });
        }
        if let Some(description) = update.description {
            sets.push("description=?");
            values.push(match blank_to_none(Some(&description)) {
                Some(value) => Value::Text(value.to_owned()),
                None => Value::Null,
            });
        }
        sets.push("metadata_locked=?");
        values.push(Value::Integer(if update.reset_lock { 0 } else { 1 }));
        sets.push("release_type=?");
        values.push(Value::Text(next_release_type.to_owned()));
        values.push(entity_id_to_value(target_id));
        let sql = format!("UPDATE albums SET {} WHERE id=?", sets.join(", "));
        conn.execute(&sql, params_from_iter(values))?;
        if let Some(conflict_id) = conflicting_album.clone() {
            conn.execute(
                "UPDATE tracks SET album_id=? WHERE album_id=?",
                rusqlite::params![conflict_id, album_id],
            )?;
            conn.execute("DELETE FROM albums WHERE id=?", rusqlite::params![album_id])?;
            Ok(Some(conflict_id))
        } else {
            Ok(None)
        }
    })();
    match result {
        Ok(merged_into) => {
            conn.execute_batch("COMMIT")?;
            Ok(Some(MetadataUpdateResult {
                ok: true,
                merged_into,
            }))
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// Documents the Set Artist Rating public API surface.
pub fn set_artist_rating(
    conn: &Connection,
    artist_id: &EntityId,
    user_id: &str,
    rating: Option<f64>,
) -> rusqlite::Result<Option<RatingUpdateResult>> {
    if !valid_rating_value(rating) {
        return Ok(None);
    }
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM artists WHERE id=?",
            rusqlite::params![artist_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(None);
    }
    if let Some(value) = rating {
        conn.execute(
            "INSERT INTO artist_ratings(user_id, artist_id, rating, updated_at)
             VALUES(?, ?, ?, datetime('now'))
             ON CONFLICT(user_id, artist_id) DO UPDATE SET
               rating=excluded.rating,
               updated_at=datetime('now')",
            rusqlite::params![user_id, artist_id, value],
        )?;
    } else {
        conn.execute(
            "DELETE FROM artist_ratings WHERE user_id=? AND artist_id=?",
            rusqlite::params![user_id, artist_id],
        )?;
    }
    Ok(Some(RatingUpdateResult {
        ok: true,
        rating,
        updated: None,
    }))
}

/// Documents the Set Track Rating public API surface.
pub fn set_track_rating(
    conn: &Connection,
    track_id: &EntityId,
    user_id: &str,
    rating: Option<f64>,
) -> rusqlite::Result<Option<RatingUpdateResult>> {
    if !valid_rating_value(rating) {
        return Ok(None);
    }
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM tracks WHERE id=?",
            rusqlite::params![track_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(None);
    }
    if let Some(value) = rating {
        conn.execute(
            "INSERT INTO track_ratings(user_id, track_id, rating, updated_at)
             VALUES(?, ?, ?, datetime('now'))
             ON CONFLICT(user_id, track_id) DO UPDATE SET
               rating=excluded.rating,
               updated_at=datetime('now')",
            rusqlite::params![user_id, track_id, value],
        )?;
    } else {
        conn.execute(
            "DELETE FROM track_ratings WHERE user_id=? AND track_id=?",
            rusqlite::params![user_id, track_id],
        )?;
    }
    Ok(Some(RatingUpdateResult {
        ok: true,
        rating,
        updated: None,
    }))
}

/// Documents the Set Album Rating public API surface.
pub fn set_album_rating(
    conn: &Connection,
    album_id: &EntityId,
    user_id: &str,
    rating: Option<f64>,
) -> rusqlite::Result<Option<RatingUpdateResult>> {
    if !valid_rating_value(rating) {
        return Ok(None);
    }
    let album: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT title, album_artist FROM albums WHERE id=?",
            rusqlite::params![album_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((title, album_artist)) = album else {
        return Ok(None);
    };
    let album_ids: Vec<EntityId> = conn
        .prepare(
            "SELECT id
             FROM albums
             WHERE LOWER(title)=LOWER(?)
               AND COALESCE(album_artist, '')=COALESCE(?, '')",
        )?
        .query_map(
            rusqlite::params![title, album_artist.unwrap_or_default()],
            |row| row.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let mut updated = 0;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> rusqlite::Result<()> {
        if let Some(value) = rating {
            let mut stmt = conn.prepare(
                "INSERT INTO album_ratings(user_id, album_id, rating, updated_at)
                 VALUES(?, ?, ?, datetime('now'))
                 ON CONFLICT(user_id, album_id) DO UPDATE SET
                   rating=excluded.rating,
                   updated_at=datetime('now')",
            )?;
            for id in &album_ids {
                updated += stmt.execute(rusqlite::params![user_id, id, value])? as i64;
            }
        } else if !album_ids.is_empty() {
            let placeholders = album_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let sql = format!(
                "DELETE FROM album_ratings WHERE user_id=? AND album_id IN ({placeholders})"
            );
            let mut values = vec![Value::Text(user_id.to_owned())];
            values.extend(album_ids.iter().map(entity_id_to_value));
            updated += conn.execute(&sql, params_from_iter(values))? as i64;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(err);
        }
    }
    Ok(Some(RatingUpdateResult {
        ok: true,
        rating,
        updated: Some(updated),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_db() -> Connection {
        let conn = Connection::open_in_memory().expect("memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE users (
              id TEXT PRIMARY KEY,
              username TEXT NOT NULL
            );
            CREATE TABLE artists (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL
            );
            CREATE TABLE albums (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              album_artist TEXT,
              artist_id TEXT
            );
            CREATE TABLE libraries (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL
            );
            CREATE TABLE tracks (
              id TEXT PRIMARY KEY,
              library_id TEXT,
              artist_id TEXT,
              album_id TEXT,
              file_path TEXT NOT NULL,
              file_name TEXT,
              file_size INTEGER,
              format TEXT,
              duration REAL,
              bitrate INTEGER,
              sample_rate INTEGER,
              channels INTEGER,
              title TEXT,
              track_number INTEGER,
              disc_number INTEGER,
              year INTEGER,
              genre TEXT,
              composer TEXT,
              comment TEXT,
              bpm REAL,
              bpm_detected REAL,
              bpm_source TEXT,
              bpm_confidence REAL,
              scanned_at TEXT
            );
            CREATE TABLE playlists (
              id TEXT PRIMARY KEY,
              user_id TEXT,
              name TEXT NOT NULL,
              description TEXT,
              created_at TEXT NOT NULL DEFAULT (datetime('now')),
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              remember_progress INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE playlist_tracks (
              id TEXT PRIMARY KEY,
              playlist_id TEXT NOT NULL,
              track_id TEXT NOT NULL,
              position INTEGER NOT NULL DEFAULT 0,
              added_at TEXT NOT NULL DEFAULT (datetime('now')),
              progress_seconds REAL NOT NULL DEFAULT 0,
              UNIQUE(playlist_id, track_id)
            );
            CREATE TABLE artist_ratings (
              user_id TEXT NOT NULL,
              artist_id TEXT NOT NULL,
              rating REAL NOT NULL,
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              PRIMARY KEY (user_id, artist_id)
            );
            CREATE TABLE album_ratings (
              user_id TEXT NOT NULL,
              album_id TEXT NOT NULL,
              rating REAL NOT NULL,
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              PRIMARY KEY (user_id, album_id)
            );
            CREATE TABLE track_ratings (
              user_id TEXT NOT NULL,
              track_id TEXT NOT NULL,
              rating REAL NOT NULL,
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              PRIMARY KEY (user_id, track_id)
            );
            INSERT INTO users(id, username) VALUES ('user-1', 'Admin');
            INSERT INTO artists(id, name) VALUES ('artist-1', 'Artist');
            INSERT INTO albums(id, title, album_artist, artist_id) VALUES
              ('album-1', 'Album', 'Artist', 'artist-1'),
              ('album-2', 'Album', 'Artist', 'artist-1');
            INSERT INTO libraries(id, name) VALUES ('lib-1', 'Library');
            INSERT INTO tracks(
              id, library_id, artist_id, album_id, file_path, file_name, duration, title
            ) VALUES
              ('track-1', 'lib-1', 'artist-1', 'album-1', 'D:\Music\one.mp3', 'one.mp3', 180, 'One'),
              ('track-2', 'lib-1', 'artist-1', 'album-2', 'D:\Music\two.mp3', 'two.mp3', 120, 'Two');
            "#,
        )
        .expect("seed fixture");
        conn
    }

    #[test]
    fn create_playlist_rejects_duplicate_names_for_same_user() {
        let conn = fixture_db();
        let first = create_playlist(&conn, "Road Trip", "", "user-1").expect("create first");

        assert!(matches!(
            create_playlist(&conn, "road trip", "", "user-1"),
            Err(CreatePlaylistError::NameTaken)
        ));
        assert_eq!(first.track_count, 0);
    }

    #[test]
    fn playlist_track_mutations_preserve_order_and_progress() {
        let conn = fixture_db();
        let playlist = create_playlist(&conn, "Queue", "", "user-1").expect("playlist");
        let track_1 = EntityId::Str("track-1".to_owned());
        let track_2 = EntityId::Str("track-2".to_owned());

        assert_eq!(
            add_track_to_playlist(&conn, &playlist.id, &track_1, "user-1").expect("add first"),
            0
        );
        batch_add_tracks(
            &conn,
            &playlist.id,
            std::slice::from_ref(&track_2),
            "user-1",
        )
        .expect("batch add")
        .expect("playlist exists");
        reorder_playlist_tracks(
            &conn,
            &playlist.id,
            &[track_2.clone(), track_1.clone()],
            "user-1",
        )
        .expect("reorder");
        update_track_progress(&conn, &playlist.id, &track_2, 42.5, "user-1").expect("progress");

        let tracks = list_playlist_tracks(&conn, &playlist.id, "user-1")
            .expect("tracks")
            .expect("playlist exists");
        assert_eq!(tracks[0].id, track_2);
        assert_eq!(tracks[0].position, 0);
        assert_eq!(tracks[0].progress_seconds, Some(42.5));
    }

    #[test]
    fn list_playlist_tracks_accepts_real_bpm_detected_values() {
        let conn = fixture_db();
        let playlist = create_playlist(&conn, "Real BPM", "", "user-1").expect("playlist");
        let track_id = EntityId::Str("track-1".to_owned());
        conn.execute(
            "UPDATE tracks SET bpm_detected = 118.25 WHERE id = 'track-1'",
            [],
        )
        .expect("set bpm");
        add_track_to_playlist(&conn, &playlist.id, &track_id, "user-1").expect("add track");

        let tracks = list_playlist_tracks(&conn, &playlist.id, "user-1")
            .expect("tracks")
            .expect("playlist exists");

        assert_eq!(tracks[0].bpm_detected, Some(118.25));
    }

    #[test]
    fn album_rating_updates_duplicate_album_rows_together() {
        let conn = fixture_db();
        let album_id = EntityId::Str("album-1".to_owned());
        let result = set_album_rating(&conn, &album_id, "user-1", Some(4.5))
            .expect("rating")
            .expect("album exists");

        assert_eq!(result.updated, Some(2));
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM album_ratings WHERE rating = 4.5",
                [],
                |row| row.get(0),
            )
            .expect("count ratings");
        assert_eq!(count, 2);
    }

    #[test]
    fn artist_and_track_rating_support_clear() {
        let conn = fixture_db();
        let artist_id = EntityId::Str("artist-1".to_owned());
        let track_id = EntityId::Str("track-1".to_owned());

        set_artist_rating(&conn, &artist_id, "user-1", Some(5.0)).expect("artist rating");
        set_track_rating(&conn, &track_id, "user-1", Some(3.5)).expect("track rating");
        set_track_rating(&conn, &track_id, "user-1", None).expect("clear track rating");

        let artist_rating: f64 = conn
            .query_row(
                "SELECT rating FROM artist_ratings WHERE artist_id = 'artist-1'",
                [],
                |row| row.get(0),
            )
            .expect("artist rating row");
        let track_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_ratings", [], |row| row.get(0))
            .expect("track count");
        assert_eq!(artist_rating, 5.0);
        assert_eq!(track_count, 0);
    }

    #[test]
    fn artist_metadata_update_locks_and_updates_description() {
        let conn = Connection::open_in_memory().expect("memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE artists (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              description TEXT,
              metadata_locked INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO artists(id, name) VALUES('artist-1', 'Old Name');
            "#,
        )
        .expect("seed");

        let result = update_artist_metadata(
            &conn,
            &EntityId::Str("artist-1".to_owned()),
            ArtistMetadataUpdate {
                name: "New Name".to_owned(),
                description: Some("Updated bio".to_owned()),
                reset_lock: false,
            },
        )
        .expect("update")
        .expect("found");

        assert!(result.ok);
        let row: (String, String, i64) = conn
            .query_row(
                "SELECT name, description, metadata_locked FROM artists WHERE id = 'artist-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("artist row");
        assert_eq!(row, ("New Name".to_owned(), "Updated bio".to_owned(), 1));
    }

    #[test]
    fn album_metadata_update_merges_conflicting_album() {
        let conn = Connection::open_in_memory().expect("memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE albums (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              album_artist TEXT NOT NULL DEFAULT '',
              year INTEGER,
              genre TEXT,
              description TEXT,
              release_type TEXT NOT NULL DEFAULT 'album',
              metadata_locked INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE tracks (
              id TEXT PRIMARY KEY,
              album_id TEXT
            );
            INSERT INTO albums(id, title, album_artist) VALUES
              ('album-1', 'Old Album', 'Old Artist'),
              ('album-2', 'New Album', 'New Artist');
            INSERT INTO tracks(id, album_id) VALUES('track-1', 'album-1');
            "#,
        )
        .expect("seed");

        let result = update_album_metadata(
            &conn,
            &EntityId::Str("album-1".to_owned()),
            AlbumMetadataUpdate {
                title: Some("New Album".to_owned()),
                album_artist: Some("New Artist".to_owned()),
                genre: Some("Rock".to_owned()),
                reset_lock: true,
                ..AlbumMetadataUpdate::default()
            },
        )
        .expect("update")
        .expect("found");

        assert_eq!(
            result.merged_into,
            Some(EntityId::Str("album-2".to_owned()))
        );
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM albums WHERE id = 'album-1'",
                [],
                |row| row.get(0),
            )
            .expect("remaining");
        let track_album: String = conn
            .query_row(
                "SELECT album_id FROM tracks WHERE id = 'track-1'",
                [],
                |row| row.get(0),
            )
            .expect("track album");
        assert_eq!(remaining, 0);
        assert_eq!(track_album, "album-2");
    }
}

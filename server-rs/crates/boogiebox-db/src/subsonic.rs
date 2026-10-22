//! Subsonic / OpenSubsonic API (`/rest`) data access: the per-user Subsonic password, stars, scrobbles and the
//! flat, paged song/album/artist listings Subsonic clients sync with. Every listing is one statement with
//! batch side-lookups so a sync page holds the shared connection mutex briefly.
//! See wip/opensubsonic-api-implementation-plan.md.

use std::collections::{HashMap, HashSet};

use rusqlite::{
    params, params_from_iter,
    types::{Value, ValueRef},
    Connection, OptionalExtension, Row,
};

use crate::music::{
    build_safe_fts_prefix_query, canonicalize_home_genre, coerce_entity_id, id_to_value,
    like_escape, list_home_genre_summaries, push_genre_filter, tracks_fts_has_track_id,
};

/// Seconds either side of a scrobble's timestamp within which a repeat is treated as the same play
/// (clients resend their offline scrobble cache).
const SCROBBLE_DEDUPE_SECS: i64 = 30;

/// Creates the Subsonic tables (migration `2026-10-18-subsonic-schema`).
pub fn ensure_subsonic_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subsonic_credentials (
           user_id       TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
           secret_enc    BLOB,
           secret_nonce  BLOB,
           created_at    TEXT NOT NULL DEFAULT (datetime('now')),
           updated_at    TEXT,
           updated_by    TEXT,
           last_used_at  TEXT,
           last_client   TEXT
         );
         CREATE TABLE IF NOT EXISTS subsonic_stars (
           user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           item_type   TEXT NOT NULL CHECK(item_type IN ('track','album','artist')),
           item_id     TEXT NOT NULL,
           starred_at  TEXT NOT NULL DEFAULT (datetime('now')),
           PRIMARY KEY (user_id, item_type, item_id)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_subsonic_stars_item ON subsonic_stars(item_type, item_id);",
    )?;
    // Per-user "last played" lookups for a page of songs. Every real database has play_history; minimal
    // legacy fixtures may not.
    if crate::table_exists(conn, "play_history") {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_play_history_user_track
               ON play_history(user_id, track_id, played_at);",
        )?;
    }
    Ok(())
}

/// Phase 2.5 tables (migration `2026-10-19-subsonic-phase-2-5`): per-user play counts, scrobble forwarding and
/// internet radio stations.
pub fn ensure_subsonic_phase_2_5_schema(conn: &Connection) -> rusqlite::Result<()> {
    ensure_user_track_stats_schema(conn)?;
    crate::scrobble::ensure_scrobble_schema(conn)?;
    crate::radio_stations::ensure_radio_station_schema(conn)?;
    Ok(())
}

/// Phase 3 schema (migration `2026-10-22-subsonic-phase-3`): saved play queues, and the API key columns
/// (OpenSubsonic `apiKeyAuthentication`, hash only).
pub fn ensure_subsonic_phase_3_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subsonic_play_queues (
           user_id      TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
           track_ids    TEXT NOT NULL,
           current_idx  INTEGER,
           position_ms  INTEGER NOT NULL DEFAULT 0,
           changed_at   TEXT NOT NULL DEFAULT (datetime('now')),
           changed_by   TEXT
         );",
    )?;
    for (column, definition) in [("api_key_hash", "TEXT"), ("api_key_created_at", "TEXT")] {
        if !crate::column_exists(conn, "subsonic_credentials", column)? {
            conn.execute_batch(&format!(
                "ALTER TABLE subsonic_credentials ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_subsonic_credentials_api_key
           ON subsonic_credentials(api_key_hash) WHERE api_key_hash IS NOT NULL;",
    )
}

/// `user_track_stats`: one row per user and track, kept in step with `play_history` by triggers (so every
/// writer is covered) and backfilled from the existing history. No foreign keys: `play_history` rows are
/// removed by its own cascades, and the delete trigger follows them.
fn ensure_user_track_stats_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_track_stats (
           user_id         TEXT NOT NULL,
           track_id        TEXT NOT NULL,
           play_count      INTEGER NOT NULL DEFAULT 0,
           last_played_at  TEXT,
           PRIMARY KEY (user_id, track_id)
         ) WITHOUT ROWID;",
    )?;
    if !crate::table_exists(conn, "play_history") {
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS trg_play_history_stats_insert AFTER INSERT ON play_history
         BEGIN
           INSERT INTO user_track_stats(user_id, track_id, play_count, last_played_at)
           VALUES (NEW.user_id, NEW.track_id, 1, NEW.played_at)
           ON CONFLICT(user_id, track_id) DO UPDATE SET
             play_count = play_count + 1,
             last_played_at = MAX(COALESCE(last_played_at, ''), COALESCE(excluded.last_played_at, ''));
         END;
         CREATE TRIGGER IF NOT EXISTS trg_play_history_stats_delete AFTER DELETE ON play_history
         BEGIN
           UPDATE user_track_stats SET play_count = play_count - 1
             WHERE user_id = OLD.user_id AND track_id = OLD.track_id;
           DELETE FROM user_track_stats
             WHERE user_id = OLD.user_id AND track_id = OLD.track_id AND play_count <= 0;
         END;
         INSERT INTO user_track_stats(user_id, track_id, play_count, last_played_at)
           SELECT user_id, track_id, COUNT(*), MAX(played_at) FROM play_history GROUP BY user_id, track_id
           ON CONFLICT(user_id, track_id) DO NOTHING;",
    )
}

// ── Lenient column readers ─────────────────────────────────────────────────────
// Legacy databases may hold integer ids and loosely typed numeric tags; read them without failing the page.

fn text_id(row: &Row<'_>, idx: usize) -> rusqlite::Result<String> {
    Ok(opt_text_id(row, idx)?.unwrap_or_default())
}

fn opt_text_id(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    Ok(match row.get_ref(idx)? {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(String::from_utf8_lossy(t).into_owned()),
    })
}

fn opt_text(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    Ok(opt_text_id(row, idx)?.filter(|s| !s.trim().is_empty()))
}

fn opt_i64(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<i64>> {
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(n) => Some(n),
        ValueRef::Real(f) if f.is_finite() => Some(f.round() as i64),
        ValueRef::Text(t) => std::str::from_utf8(t)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|f| f.is_finite())
            .map(|f| f.round() as i64),
        _ => None,
    })
}

fn opt_f64(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<f64>> {
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(n) => Some(n as f64),
        ValueRef::Real(f) if f.is_finite() => Some(f),
        ValueRef::Text(t) => std::str::from_utf8(t)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|f| f.is_finite()),
        _ => None,
    })
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn id_values(ids: &[String]) -> Vec<Value> {
    ids.iter()
        .map(|id| id_to_value(&coerce_entity_id(id)))
        .collect()
}

fn push_library(conds: &mut Vec<String>, params: &mut Vec<Value>, library: Option<&str>) {
    if let Some(lib) = library {
        conds.push("t.library_id = ?".into());
        params.push(id_to_value(&coerce_entity_id(lib)));
    }
}

fn where_clause(conds: &[String]) -> String {
    if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    }
}

// ── Credentials ────────────────────────────────────────────────────────────────

/// The account behind a Subsonic login, plus its encrypted Subsonic password when one is set.
#[derive(Debug, Clone)]
pub struct SubsonicAccount {
    /// `users.id`.
    pub user_id: String,
    /// Username as stored (lookup is case-insensitive).
    pub username: String,
    /// `admin` or `user`.
    pub role: String,
    /// "Allow libraries management" permission.
    pub can_manage_libraries: bool,
    /// "Allow metadata editing" permission.
    pub can_edit_metadata: bool,
    /// AES-256-GCM ciphertext of the Subsonic password.
    pub secret_enc: Option<Vec<u8>>,
    /// Nonce used for `secret_enc`.
    pub secret_nonce: Option<Vec<u8>>,
}

/// Looks up a user (case-insensitive username) with their Subsonic credential columns.
pub fn get_account_by_username(
    conn: &Connection,
    username: &str,
) -> rusqlite::Result<Option<SubsonicAccount>> {
    conn.query_row(
        "SELECT u.id, u.username, u.role, COALESCE(u.can_manage_libraries, 0),
                COALESCE(u.can_edit_metadata, 0), c.secret_enc, c.secret_nonce
         FROM users u LEFT JOIN subsonic_credentials c ON c.user_id = u.id
         WHERE u.username = ?1",
        [username],
        |row| {
            Ok(SubsonicAccount {
                user_id: text_id(row, 0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                can_manage_libraries: row.get::<_, i64>(3)? != 0,
                can_edit_metadata: row.get::<_, i64>(4)? != 0,
                secret_enc: row.get(5)?,
                secret_nonce: row.get(6)?,
            })
        },
    )
    .optional()
}

/// Looks up the user whose API key hashes to `key_hash` (lowercase hex SHA-256).
pub fn get_account_by_api_key(
    conn: &Connection,
    key_hash: &str,
) -> rusqlite::Result<Option<SubsonicAccount>> {
    conn.query_row(
        "SELECT u.id, u.username, u.role, COALESCE(u.can_manage_libraries, 0),
                COALESCE(u.can_edit_metadata, 0), c.secret_enc, c.secret_nonce
         FROM subsonic_credentials c JOIN users u ON u.id = c.user_id
         WHERE c.api_key_hash = ?1",
        [key_hash],
        |row| {
            Ok(SubsonicAccount {
                user_id: text_id(row, 0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                can_manage_libraries: row.get::<_, i64>(3)? != 0,
                can_edit_metadata: row.get::<_, i64>(4)? != 0,
                secret_enc: row.get(5)?,
                secret_nonce: row.get(6)?,
            })
        },
    )
    .optional()
}

/// Non-secret status of a user's Subsonic password and API key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubsonicCredentialStatus {
    /// A Subsonic password is set.
    pub has_password: bool,
    /// When it was last set or changed.
    pub updated_at: Option<String>,
    /// Last successful Subsonic login.
    pub last_used_at: Option<String>,
    /// Client name (`c` parameter) of the last successful login.
    pub last_client: Option<String>,
    /// An API key exists.
    pub has_api_key: bool,
    /// When the API key was created.
    pub api_key_created_at: Option<String>,
}

/// Returns the Subsonic password status for `user_id` (all empty when never set).
pub fn get_credential_status(
    conn: &Connection,
    user_id: &str,
) -> rusqlite::Result<SubsonicCredentialStatus> {
    Ok(conn
        .query_row(
            "SELECT secret_enc IS NOT NULL, updated_at, last_used_at, last_client,
                    api_key_hash IS NOT NULL, api_key_created_at
             FROM subsonic_credentials WHERE user_id = ?1",
            [user_id],
            |row| {
                Ok(SubsonicCredentialStatus {
                    has_password: row.get(0)?,
                    updated_at: row.get(1)?,
                    last_used_at: row.get(2)?,
                    last_client: row.get(3)?,
                    has_api_key: row.get(4)?,
                    api_key_created_at: row.get(5)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

/// Stores the hash of a new API key for `user_id`, replacing any old one. Returns false when the user does
/// not exist.
pub fn set_api_key_hash(
    conn: &Connection,
    user_id: &str,
    key_hash: &str,
) -> rusqlite::Result<bool> {
    let exists = conn
        .query_row("SELECT 1 FROM users WHERE id = ?1", [user_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO subsonic_credentials(user_id, api_key_hash, api_key_created_at)
         VALUES(?1, ?2, datetime('now'))
         ON CONFLICT(user_id) DO UPDATE SET api_key_hash = excluded.api_key_hash,
           api_key_created_at = excluded.api_key_created_at",
        params![user_id, key_hash],
    )?;
    Ok(true)
}

/// Revokes a user's API key. Returns false when the user had none.
pub fn clear_api_key(conn: &Connection, user_id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE subsonic_credentials SET api_key_hash = NULL, api_key_created_at = NULL
         WHERE user_id = ?1 AND api_key_hash IS NOT NULL",
        [user_id],
    )? > 0)
}

/// Ids of users that have an API key.
pub fn list_user_ids_with_api_key(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt =
        conn.prepare("SELECT user_id FROM subsonic_credentials WHERE api_key_hash IS NOT NULL")?;
    let rows = stmt.query_map([], |row| text_id(row, 0))?;
    rows.collect()
}

/// Stores a new encrypted Subsonic password. Returns false when the user does not exist.
pub fn set_subsonic_secret(
    conn: &Connection,
    user_id: &str,
    secret_enc: &[u8],
    secret_nonce: &[u8],
    updated_by: &str,
) -> rusqlite::Result<bool> {
    let exists = conn
        .query_row("SELECT 1 FROM users WHERE id = ?1", [user_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO subsonic_credentials(user_id, secret_enc, secret_nonce, updated_at, updated_by)
         VALUES(?1, ?2, ?3, datetime('now'), ?4)
         ON CONFLICT(user_id) DO UPDATE SET secret_enc = excluded.secret_enc,
           secret_nonce = excluded.secret_nonce, updated_at = excluded.updated_at,
           updated_by = excluded.updated_by",
        params![user_id, secret_enc, secret_nonce, updated_by],
    )?;
    Ok(true)
}

/// Removes a user's Subsonic password. Returns false when the user does not exist.
pub fn clear_subsonic_secret(
    conn: &Connection,
    user_id: &str,
    updated_by: &str,
) -> rusqlite::Result<bool> {
    let exists = conn
        .query_row("SELECT 1 FROM users WHERE id = ?1", [user_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Ok(false);
    }
    conn.execute(
        "UPDATE subsonic_credentials SET secret_enc = NULL, secret_nonce = NULL,
           updated_at = datetime('now'), updated_by = ?2
         WHERE user_id = ?1",
        params![user_id, updated_by],
    )?;
    Ok(true)
}

/// Clears every stored Subsonic password (used when the encryption key file is replaced).
pub fn clear_all_subsonic_secrets(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE subsonic_credentials SET secret_enc = NULL, secret_nonce = NULL
         WHERE secret_enc IS NOT NULL",
        [],
    )
}

/// Records a successful Subsonic login.
pub fn record_subsonic_use(
    conn: &Connection,
    user_id: &str,
    client: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subsonic_credentials SET last_used_at = datetime('now'),
           last_client = COALESCE(?2, last_client)
         WHERE user_id = ?1",
        params![user_id, client],
    )?;
    Ok(())
}

/// Ids of users that have a Subsonic password set.
pub fn list_user_ids_with_subsonic_password(
    conn: &Connection,
) -> rusqlite::Result<HashSet<String>> {
    let mut stmt =
        conn.prepare("SELECT user_id FROM subsonic_credentials WHERE secret_enc IS NOT NULL")?;
    let rows = stmt.query_map([], |row| text_id(row, 0))?;
    rows.collect()
}

// ── Libraries ──────────────────────────────────────────────────────────────────

/// Libraries as `(id, name)` in a stable order; Subsonic music folder ids are 1-based positions in this list
/// (many clients parse folder ids as integers).
pub fn music_folders(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, name FROM libraries ORDER BY added_at, rowid")?;
    let rows = stmt.query_map([], |row| {
        Ok((text_id(row, 0)?, opt_text(row, 1)?.unwrap_or_default()))
    })?;
    rows.collect()
}

/// Root folders of a library (its path plus its extra folders), for confining file access.
pub fn library_roots(conn: &Connection, library_id: &str) -> rusqlite::Result<Vec<String>> {
    let lib = id_to_value(&coerce_entity_id(library_id));
    let mut stmt = conn.prepare(
        "SELECT path FROM libraries WHERE id = ?1
         UNION SELECT path FROM library_folders WHERE library_id = ?1",
    )?;
    let rows = stmt.query_map([lib], |row| opt_text_id(row, 0))?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Most recent library scan as Unix seconds (Subsonic `lastModified`).
pub fn library_last_modified_secs(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT MAX(CAST(strftime('%s', last_scan) AS INTEGER)) FROM libraries",
        [],
        |row| row.get(0),
    )
}

/// A user's starred item ids of one kind, newest first.
pub fn starred_ids(
    conn: &Connection,
    user_id: &str,
    kind: ItemKind,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM subsonic_stars WHERE user_id = ?1 AND item_type = ?2
         ORDER BY starred_at DESC, item_id",
    )?;
    let rows = stmt.query_map(params![user_id, kind.star_type()], |row| text_id(row, 0))?;
    rows.collect()
}

/// Albums for `ids`, in the order of `ids`.
pub fn albums_by_ids(conn: &Connection, ids: &[String]) -> rusqlite::Result<Vec<SubsonicAlbum>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_id: HashMap<String, SubsonicAlbum> = query_albums(
        conn,
        AlbumQuery {
            joins: "",
            join_params: Vec::new(),
            conds: vec![format!("al.id IN ({})", placeholders(ids.len()))],
            params: id_values(ids),
            library: None,
            order: "al.id",
            limit: -1,
            offset: 0,
        },
    )?
    .into_iter()
    .map(|a| (a.id.clone(), a))
    .collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// Artists for `ids`, in the order of `ids`.
pub fn artists_by_ids(conn: &Connection, ids: &[String]) -> rusqlite::Result<Vec<SubsonicArtist>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(a) = get_artist(conn, id)? {
            out.push(a);
        }
    }
    Ok(out)
}

// ── Library rows ───────────────────────────────────────────────────────────────

/// A song as Subsonic clients see it. `file_path` stays server-side (suffix, MIME, streaming only).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubsonicSong {
    pub id: String,
    pub title: String,
    pub album_id: Option<String>,
    pub album: Option<String>,
    pub artist_id: Option<String>,
    pub artist: Option<String>,
    pub album_artist_id: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub file_path: String,
    pub file_size: i64,
    pub duration: f64,
    pub bitrate: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub play_count: i64,
    pub bpm: Option<f64>,
    pub bpm_detected: Option<f64>,
    pub comment: Option<String>,
    pub created: Option<String>,
    pub library_id: Option<String>,
}

/// An album with its song count and duration aggregated from its tracks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubsonicAlbum {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub song_count: i64,
    pub duration: f64,
    pub created: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub play_count: i64,
    pub release_type: Option<String>,
}

/// An album artist with the number of albums they own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubsonicArtist {
    pub id: String,
    pub name: String,
    pub album_count: i64,
}

/// A genre label (compound tags split, same grouping as Home) with counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsonicGenre {
    pub name: String,
    pub song_count: i64,
    pub album_count: i64,
}

const SONG_SELECT: &str =
    "SELECT t.id, t.title, t.album_id, al.title, t.artist_id, ar.name, al.artist_id,
       alar.name, t.album_artist, t.track_number, t.disc_number, t.year, t.genre, t.file_path,
       t.file_size, t.duration, t.bitrate, t.sample_rate, t.channels, t.play_count, t.bpm,
       t.bpm_detected, t.comment, t.scanned_at, t.library_id
     FROM tracks t
     LEFT JOIN albums al ON al.id = t.album_id
     LEFT JOIN artists ar ON ar.id = t.artist_id
     LEFT JOIN artists alar ON alar.id = al.artist_id";

fn map_song(row: &Row<'_>) -> rusqlite::Result<SubsonicSong> {
    Ok(SubsonicSong {
        id: text_id(row, 0)?,
        title: opt_text(row, 1)?.unwrap_or_default(),
        album_id: opt_text_id(row, 2)?,
        album: opt_text(row, 3)?,
        artist_id: opt_text_id(row, 4)?,
        artist: opt_text(row, 5)?,
        album_artist_id: opt_text_id(row, 6)?,
        album_artist: opt_text(row, 7)?.or(opt_text(row, 8)?),
        track_number: opt_i64(row, 9)?,
        disc_number: opt_i64(row, 10)?,
        year: opt_i64(row, 11)?,
        genre: opt_text(row, 12)?,
        file_path: opt_text_id(row, 13)?.unwrap_or_default(),
        file_size: opt_i64(row, 14)?.unwrap_or(0),
        duration: opt_f64(row, 15)?.unwrap_or(0.0),
        bitrate: opt_i64(row, 16)?,
        sample_rate: opt_i64(row, 17)?,
        channels: opt_i64(row, 18)?,
        play_count: opt_i64(row, 19)?.unwrap_or(0),
        bpm: opt_f64(row, 20)?,
        bpm_detected: opt_f64(row, 21)?,
        comment: opt_text(row, 22)?,
        created: opt_text(row, 23)?,
        library_id: opt_text_id(row, 24)?,
    })
}

fn query_songs(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(params), map_song)?;
    rows.collect()
}

/// Album display artist (`artist_name` in album orderings).
const ALBUM_ARTIST_NAME: &str =
    "COALESCE(NULLIF(TRIM(ar.name), ''), NULLIF(TRIM(al.album_artist), ''), '')";

const ALBUM_COLUMNS: &str = "al.id, al.title,
       COALESCE(NULLIF(TRIM(ar.name), ''), NULLIF(TRIM(al.album_artist), ''), '') AS artist_name,
       al.artist_id, COUNT(t.id), COALESCE(SUM(t.duration), 0), al.added_at, al.year, al.genre,
       COALESCE(SUM(t.play_count), 0), al.release_type";

fn map_album(row: &Row<'_>) -> rusqlite::Result<SubsonicAlbum> {
    Ok(SubsonicAlbum {
        id: text_id(row, 0)?,
        name: opt_text(row, 1)?.unwrap_or_default(),
        artist: opt_text(row, 2)?.unwrap_or_default(),
        artist_id: opt_text_id(row, 3)?,
        song_count: opt_i64(row, 4)?.unwrap_or(0),
        duration: opt_f64(row, 5)?.unwrap_or(0.0),
        created: opt_text(row, 6)?,
        year: opt_i64(row, 7)?,
        genre: opt_text(row, 8)?,
        play_count: opt_i64(row, 9)?.unwrap_or(0),
        release_type: opt_text(row, 10)?,
    })
}

/// Shared album query. `joins` may bind parameters (`join_params`), which precede the WHERE parameters.
/// `conds`, `joins` and `order` see `al` (albums), `ar` (its artist) and `artist_name`, never `t` (tracks):
/// albums without tracks (in `library`, when given) are left out.
struct AlbumQuery<'a> {
    joins: &'a str,
    join_params: Vec<Value>,
    conds: Vec<String>,
    params: Vec<Value>,
    library: Option<&'a str>,
    order: &'a str,
    limit: i64,
    offset: i64,
}

/// Two steps (plan P3-5): first the ordered page of album ids, sorted and offset over one row per album; then
/// song counts and totals for just those albums. Grouping every track before sorting cost ~45 ms per page
/// on a 100k-track library, whatever the page.
fn query_albums(conn: &Connection, q: AlbumQuery<'_>) -> rusqlite::Result<Vec<SubsonicAlbum>> {
    let (lib_cond, lib_params) = match q.library {
        Some(lib) => (
            " AND t.library_id = ?",
            vec![id_to_value(&coerce_entity_id(lib))],
        ),
        None => ("", Vec::new()),
    };
    let mut conds = vec![format!(
        "EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id{lib_cond})"
    )];
    conds.extend(q.conds);
    let page_sql = format!(
        "SELECT al.id, {ALBUM_ARTIST_NAME} AS artist_name
         FROM albums al
         LEFT JOIN artists ar ON ar.id = al.artist_id
         {joins}
         {where_sql}
         ORDER BY {order}
         LIMIT ? OFFSET ?",
        joins = q.joins,
        where_sql = where_clause(&conds),
        order = q.order,
    );
    let mut page_params = q.join_params;
    page_params.extend(lib_params.iter().cloned());
    page_params.extend(q.params);
    page_params.push(Value::Integer(q.limit));
    page_params.push(Value::Integer(q.offset.max(0)));
    let ids: Vec<Value> = {
        let mut stmt = conn.prepare(&page_sql)?;
        let rows = stmt.query_map(params_from_iter(page_params), |row| row.get::<_, Value>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let lib_join = if q.library.is_some() {
        " AND t.library_id = ?"
    } else {
        ""
    };
    let totals_sql = format!(
        "SELECT {ALBUM_COLUMNS}
         FROM albums al
         JOIN tracks t ON t.album_id = al.id{lib_join}
         LEFT JOIN artists ar ON ar.id = al.artist_id
         WHERE al.id IN ({})
         GROUP BY al.id",
        placeholders(ids.len())
    );
    let mut totals_params = lib_params;
    totals_params.extend(ids.iter().cloned());
    let mut by_id: HashMap<String, SubsonicAlbum> = {
        let mut stmt = conn.prepare(&totals_sql)?;
        let rows = stmt.query_map(params_from_iter(totals_params), map_album)?;
        rows.map(|r| r.map(|a| (a.id.clone(), a)))
            .collect::<rusqlite::Result<_>>()?
    };
    Ok(ids
        .iter()
        .filter_map(|id| by_id.remove(&text_value(id)))
        .collect())
}

// ── Songs ──────────────────────────────────────────────────────────────────────

/// One song by id.
pub fn get_song(conn: &Connection, id: &str) -> rusqlite::Result<Option<SubsonicSong>> {
    Ok(songs_by_ids(conn, &[id.to_owned()])?.into_iter().next())
}

/// Songs for `ids`, returned in the order of `ids` (unknown ids are skipped).
pub fn songs_by_ids(conn: &Connection, ids: &[String]) -> rusqlite::Result<Vec<SubsonicSong>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!("{SONG_SELECT} WHERE t.id IN ({})", placeholders(ids.len()));
    let mut by_id: HashMap<String, SubsonicSong> = query_songs(conn, &sql, id_values(ids))?
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

/// An album's songs in disc/track order.
pub fn album_songs(conn: &Connection, album_id: &str) -> rusqlite::Result<Vec<SubsonicSong>> {
    let sql = format!(
        "{SONG_SELECT} WHERE t.album_id = ?
         ORDER BY COALESCE(t.disc_number, 1), COALESCE(t.track_number, 0), t.title COLLATE NOCASE, t.id"
    );
    query_songs(conn, &sql, vec![id_to_value(&coerce_entity_id(album_id))])
}

/// Every song, in stable id order — the page shape `search3` with an empty query returns for client sync.
pub fn songs_page(
    conn: &Connection,
    library: Option<&str>,
    offset: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let mut conds = Vec::new();
    let mut params = Vec::new();
    push_library(&mut conds, &mut params, library);
    songs_page_by_id(conn, &conds, params, offset, limit)
}

/// One id-ordered page of songs matching `conds` (which use the `t` alias). The page's ids are chosen in a
/// join-free subquery, so OFFSET skips index entries rather than fully joined rows; the album and artist
/// joins then run for the returned page only (deep sync pages were 2× slower otherwise, plan P3-5).
fn songs_page_by_id(
    conn: &Connection,
    conds: &[String],
    mut params: Vec<Value>,
    offset: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let sql = format!(
        "{SONG_SELECT}
         JOIN (SELECT t.id AS pid FROM tracks t {} ORDER BY t.id LIMIT ? OFFSET ?) page ON page.pid = t.id
         ORDER BY t.id",
        where_clause(conds)
    );
    params.push(Value::Integer(limit));
    params.push(Value::Integer(offset.max(0)));
    query_songs(conn, &sql, params)
}

/// Full-text song search (title/artist/album/genre/composer prefix match), best matches first.
pub fn search_songs(
    conn: &Connection,
    query: &str,
    library: Option<&str>,
    offset: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let Some(fts) = build_safe_fts_prefix_query(query) else {
        return Ok(Vec::new());
    };
    let (fts_col, track_col) = if tracks_fts_has_track_id(conn) {
        ("track_id", "t.id")
    } else {
        ("rowid", "t.rowid")
    };
    let mut conds = Vec::new();
    let mut params = vec![Value::Text(fts)];
    push_library(&mut conds, &mut params, library);
    let sql = format!(
        "{SONG_SELECT}
         JOIN (SELECT {fts_col} AS mid, bm25(tracks_fts) AS score FROM tracks_fts
               WHERE tracks_fts MATCH ?) m ON m.mid = {track_col}
         {} ORDER BY m.score, t.title COLLATE NOCASE, t.id LIMIT ? OFFSET ?",
        where_clause(&conds)
    );
    params.push(Value::Integer(limit));
    params.push(Value::Integer(offset.max(0)));
    query_songs(conn, &sql, params)
}

/// Random songs, optionally filtered. Samples rowids first (narrow scan) and then loads only those rows.
pub fn random_songs(
    conn: &Connection,
    size: i64,
    genre: Option<&str>,
    from_year: Option<i64>,
    to_year: Option<i64>,
    library: Option<&str>,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let mut conds = Vec::new();
    let mut params = Vec::new();
    if let Some(g) = genre.map(str::trim).filter(|g| !g.is_empty()) {
        let labels = genre_raw_labels(conn, g)?;
        push_genre_filter(&mut conds, &mut params, &labels);
    }
    if let Some(y) = from_year {
        conds.push("t.year >= ?".into());
        params.push(Value::Integer(y));
    }
    if let Some(y) = to_year {
        conds.push("t.year <= ?".into());
        params.push(Value::Integer(y));
    }
    push_library(&mut conds, &mut params, library);
    let sql = format!(
        "{SONG_SELECT} WHERE t.rowid IN (
           SELECT t.rowid FROM tracks t {} ORDER BY random() LIMIT ?
         ) ORDER BY random()",
        where_clause(&conds)
    );
    params.push(Value::Integer(size));
    query_songs(conn, &sql, params)
}

/// Songs carrying a genre tag (compound genre strings match on tag boundaries).
pub fn songs_by_genre(
    conn: &Connection,
    genre: &str,
    library: Option<&str>,
    offset: i64,
    count: i64,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let mut conds = Vec::new();
    let mut params = Vec::new();
    let labels = genre_raw_labels(conn, genre)?;
    push_genre_filter(&mut conds, &mut params, &labels);
    push_library(&mut conds, &mut params, library);
    songs_page_by_id(conn, &conds, params, offset, count)
}

/// A playlist's songs in playlist order; `None` when the playlist does not exist or belongs to someone else.
pub fn playlist_songs(
    conn: &Connection,
    playlist_id: &str,
    user_id: &str,
) -> rusqlite::Result<Option<Vec<SubsonicSong>>> {
    let pid = id_to_value(&coerce_entity_id(playlist_id));
    let owned = conn
        .query_row(
            "SELECT 1 FROM playlists WHERE id = ?1 AND user_id = ?2",
            params![pid, user_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !owned {
        return Ok(None);
    }
    let sql = format!(
        "{SONG_SELECT} JOIN playlist_tracks pt ON pt.track_id = t.id
         WHERE pt.playlist_id = ? ORDER BY pt.position, pt.id"
    );
    query_songs(conn, &sql, vec![pid]).map(Some)
}

// ── Albums ─────────────────────────────────────────────────────────────────────

/// One album (with at least one track).
pub fn get_album(conn: &Connection, id: &str) -> rusqlite::Result<Option<SubsonicAlbum>> {
    Ok(query_albums(
        conn,
        AlbumQuery {
            joins: "",
            join_params: Vec::new(),
            conds: vec!["al.id = ?".into()],
            params: vec![id_to_value(&coerce_entity_id(id))],
            library: None,
            order: "al.id",
            limit: 1,
            offset: 0,
        },
    )?
    .into_iter()
    .next())
}

/// Albums owned by an artist, oldest first.
pub fn artist_albums(conn: &Connection, artist_id: &str) -> rusqlite::Result<Vec<SubsonicAlbum>> {
    query_albums(
        conn,
        AlbumQuery {
            joins: "",
            join_params: Vec::new(),
            conds: vec!["al.artist_id = ?".into()],
            params: vec![id_to_value(&coerce_entity_id(artist_id))],
            library: None,
            order: "COALESCE(al.year, 0), al.title COLLATE NOCASE, al.id",
            limit: -1,
            offset: 0,
        },
    )
}

/// Album title search; an empty query lists every album in name order.
pub fn search_albums(
    conn: &Connection,
    query: &str,
    library: Option<&str>,
    offset: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicAlbum>> {
    let mut conds = Vec::new();
    let mut params = Vec::new();
    let q = query.trim();
    if !q.is_empty() {
        conds.push("al.title LIKE ? ESCAPE '\\'".into());
        params.push(Value::Text(format!("%{}%", like_escape(q))));
    }
    query_albums(
        conn,
        AlbumQuery {
            joins: "",
            join_params: Vec::new(),
            conds,
            params,
            library,
            order: "al.title COLLATE NOCASE, al.id",
            limit,
            offset,
        },
    )
}

/// Parameters for `getAlbumList` / `getAlbumList2`.
#[derive(Debug, Clone, Copy)]
pub struct AlbumListQuery<'a> {
    pub list_type: &'a str,
    pub user_id: &'a str,
    pub size: i64,
    pub offset: i64,
    pub from_year: Option<i64>,
    pub to_year: Option<i64>,
    pub genre: Option<&'a str>,
    pub library: Option<&'a str>,
}

/// Album list by Subsonic list type; `Ok(None)` for an unknown type.
pub fn album_list(
    conn: &Connection,
    q: AlbumListQuery<'_>,
) -> rusqlite::Result<Option<Vec<SubsonicAlbum>>> {
    let mut conds = Vec::new();
    let mut params = Vec::new();
    let user = Value::Text(q.user_id.to_owned());
    let mut joins = "";
    let mut join_params = Vec::new();
    let year_order;
    let order: &str = match q.list_type {
        "random" => "random()",
        "newest" => "al.added_at DESC, al.id DESC",
        "alphabeticalByName" => "al.title COLLATE NOCASE, al.id",
        "alphabeticalByArtist" => "artist_name COLLATE NOCASE, al.title COLLATE NOCASE, al.id",
        "frequent" => {
            joins = "JOIN (SELECT t2.album_id AS aid, SUM(us.play_count) AS plays
                           FROM user_track_stats us JOIN tracks t2 ON t2.id = us.track_id
                           WHERE us.user_id = ? GROUP BY t2.album_id) up ON up.aid = al.id";
            join_params.push(user);
            conds.push("up.plays > 0".into());
            "up.plays DESC, al.id"
        }
        "recent" => {
            joins = "JOIN (SELECT t2.album_id AS aid, MAX(us.last_played_at) AS last_played
                           FROM user_track_stats us JOIN tracks t2 ON t2.id = us.track_id
                           WHERE us.user_id = ? GROUP BY t2.album_id) rp ON rp.aid = al.id";
            join_params.push(user);
            "rp.last_played DESC, al.id"
        }
        "highest" => {
            joins = "JOIN album_ratings ur ON ur.album_id = al.id AND ur.user_id = ?";
            join_params.push(user);
            "ur.rating DESC, al.title COLLATE NOCASE, al.id"
        }
        "starred" => {
            joins = "JOIN subsonic_stars st ON st.item_type = 'album' AND st.item_id = al.id
                     AND st.user_id = ?";
            join_params.push(user);
            "st.starred_at DESC, al.id"
        }
        "byYear" => {
            let from = q.from_year.unwrap_or(0);
            let to = q.to_year.unwrap_or(9999);
            conds.push("al.year BETWEEN ? AND ?".into());
            params.push(Value::Integer(from.min(to)));
            params.push(Value::Integer(from.max(to)));
            year_order = if from > to {
                "al.year DESC, al.title COLLATE NOCASE, al.id"
            } else {
                "al.year ASC, al.title COLLATE NOCASE, al.id"
            };
            year_order
        }
        "byGenre" => {
            let Some(genre) = q.genre.map(str::trim).filter(|g| !g.is_empty()) else {
                return Ok(Some(Vec::new()));
            };
            let values = genre_values(conn, genre)?;
            if values.is_empty() {
                return Ok(Some(Vec::new()));
            }
            conds.push(format!(
                "al.id IN (SELECT t.album_id FROM tracks t WHERE t.genre IN ({}))",
                placeholders(values.len())
            ));
            params.extend(values.into_iter().map(Value::Text));
            "al.title COLLATE NOCASE, al.id"
        }
        _ => return Ok(None),
    };
    query_albums(
        conn,
        AlbumQuery {
            joins,
            join_params,
            conds,
            params,
            library: q.library,
            order,
            limit: q.size,
            offset: q.offset,
        },
    )
    .map(Some)
}

// ── Artists ────────────────────────────────────────────────────────────────────

fn map_artist(row: &Row<'_>) -> rusqlite::Result<SubsonicArtist> {
    Ok(SubsonicArtist {
        id: text_id(row, 0)?,
        name: opt_text(row, 1)?.unwrap_or_default(),
        album_count: opt_i64(row, 2)?.unwrap_or(0),
    })
}

/// Album artists (artists owning at least one album with tracks), name order. `name_query` filters by
/// substring; `limit` of -1 lists all.
pub fn list_artists(
    conn: &Connection,
    library: Option<&str>,
    name_query: Option<&str>,
    offset: i64,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicArtist>> {
    let mut track_cond = String::new();
    let mut params = Vec::new();
    if let Some(lib) = library {
        track_cond.push_str(" AND t.library_id = ?");
        params.push(id_to_value(&coerce_entity_id(lib)));
    }
    let mut name_cond = String::new();
    if let Some(q) = name_query.map(str::trim).filter(|q| !q.is_empty()) {
        name_cond.push_str(" AND ar.name LIKE ? ESCAPE '\\'");
        params.push(Value::Text(format!("%{}%", like_escape(q))));
    }
    let sql = format!(
        "SELECT ar.id, ar.name, COUNT(DISTINCT al.id)
         FROM artists ar JOIN albums al ON al.artist_id = ar.id
         WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id{track_cond}){name_cond}
         GROUP BY ar.id
         ORDER BY ar.name COLLATE NOCASE, ar.id
         LIMIT ? OFFSET ?"
    );
    params.push(Value::Integer(limit));
    params.push(Value::Integer(offset.max(0)));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), map_artist)?;
    rows.collect()
}

/// One artist by id (including artists that own no album).
pub fn get_artist(conn: &Connection, id: &str) -> rusqlite::Result<Option<SubsonicArtist>> {
    conn.query_row(
        "SELECT ar.id, ar.name,
                (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id
                   AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id))
         FROM artists ar WHERE ar.id = ?1",
        [id_to_value(&coerce_entity_id(id))],
        map_artist,
    )
    .optional()
}

// ── Genres ─────────────────────────────────────────────────────────────────────

/// All genres (compound tags split and grouped like Home), alphabetical.
pub fn genres(conn: &Connection) -> rusqlite::Result<Vec<SubsonicGenre>> {
    let mut out: Vec<SubsonicGenre> = list_home_genre_summaries(conn, usize::MAX)?
        .into_iter()
        .map(|g| SubsonicGenre {
            name: g.label,
            song_count: g.track_count,
            album_count: g.album_count,
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.name.cmp(&b.name))
    });
    Ok(out)
}

/// Raw tag spellings that belong to a genre label (e.g. "Hip-Hop" and "Hip Hop"), for tag filters.
pub fn genre_raw_labels(conn: &Connection, label: &str) -> rusqlite::Result<Vec<String>> {
    let mut raw: Vec<String> = Vec::new();
    for_genre_tags(conn, label, |_, part| {
        if !raw.iter().any(|r| r == part) {
            raw.push(part.to_owned());
        }
    })?;
    if raw.is_empty() {
        raw.push(label.trim().to_owned());
    }
    Ok(raw)
}

/// The stored `tracks.genre` strings carrying the tag `label` (for an indexed `t.genre IN (…)` filter).
fn genre_values(conn: &Connection, label: &str) -> rusqlite::Result<Vec<String>> {
    let mut values: Vec<String> = Vec::new();
    for_genre_tags(conn, label, |genre, _| {
        if values.last().map(String::as_str) != Some(genre) {
            values.push(genre.to_owned());
        }
    })?;
    Ok(values)
}

/// Calls `found(genre string, tag)` for every tag of every distinct genre string whose canonical key is
/// `label`'s. Reads the distinct genre strings (an index scan) rather than the full Home summary with its
/// distinct artist/album counts, which cost ~45 ms per call on 100k tracks (plan P3-5). Tags match as on
/// Home: split on commas, compare canonical keys.
fn for_genre_tags(
    conn: &Connection,
    label: &str,
    mut found: impl FnMut(&str, &str),
) -> rusqlite::Result<()> {
    let key = canonicalize_home_genre(label);
    if key.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT DISTINCT genre FROM tracks WHERE genre IS NOT NULL AND TRIM(genre) != ''",
    )?;
    let rows = stmt.query_map([], |row| opt_text(row, 0))?;
    for genre in rows {
        let Some(genre) = genre? else { continue };
        for part in genre.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if canonicalize_home_genre(part) == key {
                found(&genre, part);
            }
        }
    }
    Ok(())
}

// ── Per-user annotations ───────────────────────────────────────────────────────

/// The entity type an annotation lookup is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Track,
    Album,
    Artist,
}

impl ItemKind {
    fn star_type(self) -> &'static str {
        match self {
            ItemKind::Track => "track",
            ItemKind::Album => "album",
            ItemKind::Artist => "artist",
        }
    }

    fn rating_table(self) -> (&'static str, &'static str) {
        match self {
            ItemKind::Track => ("track_ratings", "track_id"),
            ItemKind::Album => ("album_ratings", "album_id"),
            ItemKind::Artist => ("artist_ratings", "artist_id"),
        }
    }
}

/// A user's ratings, stars, play counts and (tracks only) last-played times for a page of items, keyed by
/// item id. Play counts are the user's own (`user_track_stats`); albums sum their tracks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations {
    pub ratings: HashMap<String, f64>,
    pub starred: HashMap<String, String>,
    pub played: HashMap<String, String>,
    pub plays: HashMap<String, i64>,
}

fn collect_pairs<T: rusqlite::types::FromSql>(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
) -> rusqlite::Result<HashMap<String, T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((text_id(row, 0)?, row.get::<_, T>(1)?))
    })?;
    rows.collect()
}

/// Batch-loads annotations for `ids` (one statement per annotation kind).
pub fn annotations(
    conn: &Connection,
    user_id: &str,
    kind: ItemKind,
    ids: &[String],
) -> rusqlite::Result<Annotations> {
    if ids.is_empty() {
        return Ok(Annotations::default());
    }
    let ph = placeholders(ids.len());
    let with_user = |extra: Vec<Value>| {
        let mut p = vec![Value::Text(user_id.to_owned())];
        p.extend(extra);
        p
    };
    let (table, column) = kind.rating_table();
    let ratings = collect_pairs::<f64>(
        conn,
        &format!("SELECT {column}, rating FROM {table} WHERE user_id = ? AND {column} IN ({ph})"),
        with_user(id_values(ids)),
    )?;
    let mut star_params = with_user(vec![Value::Text(kind.star_type().to_owned())]);
    star_params.extend(ids.iter().map(|id| Value::Text(id.clone())));
    let starred = collect_pairs::<String>(
        conn,
        &format!(
            "SELECT item_id, starred_at FROM subsonic_stars
             WHERE user_id = ? AND item_type = ? AND item_id IN ({ph})"
        ),
        star_params,
    )?;
    let mut played = HashMap::new();
    let mut plays = HashMap::new();
    match kind {
        ItemKind::Track => {
            let mut stmt = conn.prepare(&format!(
                "SELECT track_id, play_count, last_played_at FROM user_track_stats
                 WHERE user_id = ? AND track_id IN ({ph})"
            ))?;
            let rows = stmt.query_map(params_from_iter(with_user(id_values(ids))), |row| {
                Ok((
                    text_id(row, 0)?,
                    opt_i64(row, 1)?.unwrap_or(0),
                    opt_text(row, 2)?,
                ))
            })?;
            for row in rows {
                let (id, count, last) = row?;
                plays.insert(id.clone(), count);
                if let Some(last) = last {
                    played.insert(id, last);
                }
            }
        }
        ItemKind::Album => {
            plays = collect_pairs::<i64>(
                conn,
                &format!(
                    "SELECT t.album_id, SUM(us.play_count) FROM tracks t
                     JOIN user_track_stats us ON us.user_id = ? AND us.track_id = t.id
                     WHERE t.album_id IN ({ph}) GROUP BY t.album_id"
                ),
                with_user(id_values(ids)),
            )?;
        }
        ItemKind::Artist => {}
    }
    Ok(Annotations {
        ratings,
        starred,
        played,
        plays,
    })
}

// ── Stars and item kinds ───────────────────────────────────────────────────────

/// What a bare id names: a track, else an album, else an artist (folder-mode clients pass any of them as `id`).
pub fn resolve_item_kind(conn: &Connection, id: &str) -> rusqlite::Result<Option<ItemKind>> {
    let value = id_to_value(&coerce_entity_id(id));
    for (kind, table) in [
        (ItemKind::Track, "tracks"),
        (ItemKind::Album, "albums"),
        (ItemKind::Artist, "artists"),
    ] {
        let found = conn
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE id = ?1"),
                [&value],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if found {
            return Ok(Some(kind));
        }
    }
    Ok(None)
}

/// Stars (`star = true`) or unstars items for a user in one transaction.
pub fn set_stars(
    conn: &Connection,
    user_id: &str,
    items: &[(ItemKind, String)],
    star: bool,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (kind, id) in items {
        if star {
            tx.execute(
                "INSERT OR IGNORE INTO subsonic_stars(user_id, item_type, item_id) VALUES(?1, ?2, ?3)",
                params![user_id, kind.star_type(), id],
            )?;
        } else {
            tx.execute(
                "DELETE FROM subsonic_stars WHERE user_id = ?1 AND item_type = ?2 AND item_id = ?3",
                params![user_id, kind.star_type(), id],
            )?;
        }
    }
    tx.commit()
}

/// Removes stars whose track/album/artist no longer exists. Returns how many were removed.
pub fn sweep_stale_stars(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM subsonic_stars
         WHERE (item_type = 'track' AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.id = subsonic_stars.item_id))
            OR (item_type = 'album' AND NOT EXISTS (SELECT 1 FROM albums a WHERE a.id = subsonic_stars.item_id))
            OR (item_type = 'artist' AND NOT EXISTS (SELECT 1 FROM artists r WHERE r.id = subsonic_stars.item_id))",
        [],
    )
}

/// Moves artist stars from a merged-away artist onto the merge master (called inside the merge transaction).
pub(crate) fn repoint_artist_stars(
    conn: &Connection,
    master_id: &crate::music::EntityId,
    source_id: &crate::music::EntityId,
) -> rusqlite::Result<()> {
    if !crate::table_exists(conn, "subsonic_stars") {
        return Ok(());
    }
    let (master, source) = (master_id.to_string(), source_id.to_string());
    conn.execute(
        "INSERT OR IGNORE INTO subsonic_stars(user_id, item_type, item_id, starred_at)
         SELECT user_id, 'artist', ?1, starred_at FROM subsonic_stars
         WHERE item_type = 'artist' AND item_id = ?2",
        params![master, source],
    )?;
    conn.execute(
        "DELETE FROM subsonic_stars WHERE item_type = 'artist' AND item_id = ?1",
        params![source],
    )?;
    Ok(())
}

// ── Playlist edits ─────────────────────────────────────────────────────────────

fn owns_playlist(conn: &Connection, playlist_id: &Value, user_id: &str) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM playlists WHERE id = ?1 AND user_id = ?2",
            params![playlist_id, user_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// A free playlist name for `user_id`: `name`, else `name (2)`, `name (3)`… (BoogieBox names are unique per
/// user; Subsonic apps don't expect that).
pub fn unique_playlist_name(
    conn: &Connection,
    user_id: &str,
    name: &str,
) -> rusqlite::Result<String> {
    let taken = |candidate: &str| -> rusqlite::Result<bool> {
        Ok(conn
            .query_row(
                "SELECT 1 FROM playlists WHERE user_id = ?1 AND LOWER(TRIM(name)) = LOWER(TRIM(?2))",
                params![user_id, candidate],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    };
    if !taken(name)? {
        return Ok(name.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{name} ({n})");
        if !taken(&candidate)? {
            return Ok(candidate);
        }
    }
    Ok(format!("{name} ({})", uuid::Uuid::now_v7()))
}

/// Track ids of a playlist in order (owner only; `None` otherwise).
pub fn playlist_track_ids(
    conn: &Connection,
    playlist_id: &str,
    user_id: &str,
) -> rusqlite::Result<Option<Vec<String>>> {
    let pid = id_to_value(&coerce_entity_id(playlist_id));
    if !owns_playlist(conn, &pid, user_id)? {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt.query_map([pid], |row| text_id(row, 0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map(Some)
}

/// Replaces a playlist's entries with `track_ids` (unknown and repeated ids are dropped — a track appears once
/// per BoogieBox playlist). Owner only; false otherwise.
pub fn set_playlist_tracks(
    conn: &Connection,
    playlist_id: &str,
    user_id: &str,
    track_ids: &[String],
) -> rusqlite::Result<bool> {
    let pid = id_to_value(&coerce_entity_id(playlist_id));
    if !owns_playlist(conn, &pid, user_id)? {
        return Ok(false);
    }
    let existing: HashSet<String> = songs_by_ids(conn, track_ids)?
        .into_iter()
        .map(|s| s.id)
        .collect();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [&pid])?;
    let mut seen = HashSet::new();
    let mut position = 0i64;
    for id in track_ids {
        if existing.contains(id) && seen.insert(id.clone()) {
            // Same shape as `playlists::batch_add_tracks` (no explicit id).
            tx.execute(
                "INSERT INTO playlist_tracks(playlist_id, track_id, position) VALUES(?1, ?2, ?3)",
                params![pid, id_to_value(&coerce_entity_id(id)), position],
            )?;
            position += 1;
        }
    }
    tx.execute(
        "UPDATE playlists SET updated_at = datetime('now') WHERE id = ?1",
        [&pid],
    )?;
    tx.commit()?;
    Ok(true)
}

/// Subsonic `updatePlaylist` entry edits: remove by 0-based index, then append. Owner only; false otherwise.
pub fn edit_playlist_entries(
    conn: &Connection,
    playlist_id: &str,
    user_id: &str,
    remove_indexes: &[usize],
    add_ids: &[String],
) -> rusqlite::Result<bool> {
    let Some(current) = playlist_track_ids(conn, playlist_id, user_id)? else {
        return Ok(false);
    };
    let remove: HashSet<usize> = remove_indexes.iter().copied().collect();
    let mut next: Vec<String> = current
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !remove.contains(i))
        .map(|(_, id)| id)
        .collect();
    next.extend(add_ids.iter().cloned());
    set_playlist_tracks(conn, playlist_id, user_id, &next)
}

// ── BoogieMix outputs ──────────────────────────────────────────────────────────

/// A finished BoogieMix render owned by a user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubsonicMix {
    pub id: String,
    pub name: String,
    pub file_path: String,
    pub format: String,
    pub duration: f64,
    pub file_size: i64,
    pub cover_album_id: Option<String>,
    pub source_playlist_name: Option<String>,
    pub created: Option<String>,
}

const MIX_SELECT: &str =
    "SELECT id, COALESCE(NULLIF(TRIM(name), ''), file_name), file_path, format,
       duration_sec, file_size_bytes, cover_album_ids, source_playlist_name, created_at
     FROM mix_outputs";

fn map_mix(row: &Row<'_>) -> rusqlite::Result<SubsonicMix> {
    let covers = opt_text(row, 6)?;
    Ok(SubsonicMix {
        id: text_id(row, 0)?,
        name: opt_text(row, 1)?.unwrap_or_else(|| "BoogieMix".to_owned()),
        file_path: opt_text_id(row, 2)?.unwrap_or_default(),
        format: opt_text(row, 3)?
            .unwrap_or_else(|| "mp3".to_owned())
            .to_ascii_lowercase(),
        duration: opt_f64(row, 4)?.unwrap_or(0.0),
        file_size: opt_i64(row, 5)?.unwrap_or(0),
        cover_album_id: covers.and_then(|c| {
            c.split(',')
                .map(str::trim)
                .find(|s| !s.is_empty())
                .map(str::to_owned)
        }),
        source_playlist_name: opt_text(row, 7)?,
        created: opt_text(row, 8)?,
    })
}

/// A user's mixes, newest first.
pub fn list_user_mixes(conn: &Connection, user_id: &str) -> rusqlite::Result<Vec<SubsonicMix>> {
    let mut stmt = conn.prepare(&format!(
        "{MIX_SELECT} WHERE user_id = ?1 ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([user_id], map_mix)?;
    rows.collect()
}

/// One of the user's mixes.
pub fn get_user_mix(
    conn: &Connection,
    user_id: &str,
    mix_id: &str,
) -> rusqlite::Result<Option<SubsonicMix>> {
    conn.query_row(
        &format!("{MIX_SELECT} WHERE id = ?1 AND user_id = ?2"),
        params![id_to_value(&coerce_entity_id(mix_id)), user_id],
        map_mix,
    )
    .optional()
}

// ── Info lookups ───────────────────────────────────────────────────────────────

/// An artist's name, local description and MusicBrainz id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistInfo {
    pub name: String,
    pub description: Option<String>,
    pub musicbrainz_id: Option<String>,
}

/// Name, local description and MusicBrainz id of an artist.
pub fn artist_info(conn: &Connection, id: &str) -> rusqlite::Result<Option<ArtistInfo>> {
    conn.query_row(
        "SELECT name, description, COALESCE(musicbrainz_artist_id, lastfm_mbid) FROM artists WHERE id = ?1",
        [id_to_value(&coerce_entity_id(id))],
        |row| {
            Ok(ArtistInfo {
                name: opt_text(row, 0)?.unwrap_or_default(),
                description: opt_text(row, 1)?,
                musicbrainz_id: opt_text(row, 2)?,
            })
        },
    )
    .optional()
}

/// An album's title, owning artist name and local description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumInfo {
    pub title: String,
    pub artist: String,
    pub description: Option<String>,
}

/// Title, owning artist name and local description of an album.
pub fn album_info(conn: &Connection, id: &str) -> rusqlite::Result<Option<AlbumInfo>> {
    conn.query_row(
        "SELECT al.title, COALESCE(NULLIF(TRIM(ar.name), ''), al.album_artist, ''), al.description
         FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id WHERE al.id = ?1",
        [id_to_value(&coerce_entity_id(id))],
        |row| {
            Ok(AlbumInfo {
                title: opt_text(row, 0)?.unwrap_or_default(),
                artist: opt_text(row, 1)?.unwrap_or_default(),
                description: opt_text(row, 2)?,
            })
        },
    )
    .optional()
}

/// Cached lyrics text for an artist + title (no network).
pub fn cached_lyrics(
    conn: &Connection,
    artist: &str,
    title: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT lyrics FROM lyrics_cache
         WHERE LOWER(TRIM(artist)) = LOWER(TRIM(?1)) AND LOWER(TRIM(title)) = LOWER(TRIM(?2))
           AND TRIM(COALESCE(lyrics, '')) != ''
         ORDER BY updated_at DESC LIMIT 1",
        params![artist, title],
        |row| row.get(0),
    )
    .optional()
}

/// An artist id for a name: the unique release-owning match, else the first exact name match.
pub fn artist_id_by_name(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    if let Some(id) = crate::music::find_owned_artist_by_name(conn, name)? {
        return Ok(Some(id.to_string()));
    }
    conn.query_row(
        "SELECT id FROM artists WHERE LOWER(TRIM(name)) = LOWER(TRIM(?1)) ORDER BY id LIMIT 1",
        [name],
        |row| text_id(row, 0),
    )
    .optional()
}

/// An artist's songs, most played first.
pub fn artist_songs_by_plays(
    conn: &Connection,
    artist_id: &str,
    limit: i64,
) -> rusqlite::Result<Vec<SubsonicSong>> {
    let sql = format!(
        "{SONG_SELECT} WHERE t.artist_id = ? ORDER BY t.play_count DESC, t.title COLLATE NOCASE, t.id LIMIT ?"
    );
    query_songs(
        conn,
        &sql,
        vec![
            id_to_value(&coerce_entity_id(artist_id)),
            Value::Integer(limit),
        ],
    )
}

// ── Scrobbles ──────────────────────────────────────────────────────────────────

/// Result of recording one Subsonic scrobble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrobbleOutcome {
    /// Counted: play count incremented and a history row written.
    Recorded,
    /// Already recorded within the dedupe window (a client resending its offline cache).
    Duplicate,
    /// No such track.
    NotFound,
}

/// The time a scrobble is recorded at: the client's timestamp when plausible (from 2000 up to a day ahead,
/// a clock-skew guard), otherwise now.
pub fn effective_scrobble_time(
    conn: &Connection,
    played_at_secs: Option<i64>,
) -> rusqlite::Result<i64> {
    let now: i64 = conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| {
        r.get(0)
    })?;
    Ok(played_at_secs
        .filter(|s| *s >= 946_684_800 && *s <= now + 86_400)
        .unwrap_or(now))
}

/// Records a finished play at `played_at_secs` (Unix seconds; `None` = now), skipping repeats of the same
/// user+track within ±30 s.
pub fn record_scrobble(
    conn: &Connection,
    user_id: &str,
    track_id: &str,
    played_at_secs: Option<i64>,
) -> rusqlite::Result<ScrobbleOutcome> {
    let tid = id_to_value(&coerce_entity_id(track_id));
    let exists = conn
        .query_row("SELECT 1 FROM tracks WHERE id = ?1", [&tid], |_| Ok(()))
        .optional()?
        .is_some();
    if !exists {
        return Ok(ScrobbleOutcome::NotFound);
    }
    let at = effective_scrobble_time(conn, played_at_secs)?;
    let duplicate = conn
        .query_row(
            "SELECT 1 FROM play_history
             WHERE user_id = ?1 AND track_id = ?2
               AND played_at BETWEEN datetime(?3, 'unixepoch') AND datetime(?4, 'unixepoch')
             LIMIT 1",
            params![
                user_id,
                tid,
                at - SCROBBLE_DEDUPE_SECS,
                at + SCROBBLE_DEDUPE_SECS
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if duplicate {
        return Ok(ScrobbleOutcome::Duplicate);
    }
    crate::playback::increment_track_play_count(conn, &text_value(&tid))?;
    conn.execute(
        "INSERT INTO play_history(id, user_id, track_id, played_at)
         VALUES(?1, ?2, ?3, datetime(?4, 'unixepoch'))",
        params![uuid::Uuid::now_v7().to_string(), user_id, tid, at],
    )?;
    Ok(ScrobbleOutcome::Recorded)
}

fn text_value(v: &Value) -> String {
    match v {
        Value::Integer(n) => n.to_string(),
        Value::Text(s) => s.clone(),
        _ => String::new(),
    }
}

// ── Play queue ─────────────────────────────────────────────────────────────────

/// Most entries a saved play queue may hold.
pub const MAX_PLAY_QUEUE: usize = 1000;
/// Longest `changed_by` (client name) stored.
const MAX_QUEUE_CLIENT_CHARS: usize = 64;

/// A user's saved play queue, as stored (ids may no longer exist; readers filter them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayQueue {
    /// Song ids in queue order; a song may appear more than once.
    pub track_ids: Vec<String>,
    /// Index of the current entry.
    pub current_idx: Option<usize>,
    pub position_ms: i64,
    /// `YYYY-MM-DD HH:MM:SS`, UTC.
    pub changed_at: String,
    /// The app that saved it.
    pub changed_by: Option<String>,
}

/// Replaces the user's saved queue. An empty `track_ids` deletes it.
pub fn save_play_queue(
    conn: &Connection,
    user_id: &str,
    track_ids: &[String],
    current_idx: Option<usize>,
    position_ms: i64,
    client: Option<&str>,
) -> rusqlite::Result<()> {
    if track_ids.is_empty() {
        conn.execute(
            "DELETE FROM subsonic_play_queues WHERE user_id = ?1",
            [user_id],
        )?;
        return Ok(());
    }
    let stored = &track_ids[..track_ids.len().min(MAX_PLAY_QUEUE)];
    let ids = serde_json::to_string(stored).unwrap_or_else(|_| "[]".to_owned());
    let current = current_idx.filter(|i| *i < stored.len()).map(|i| i as i64);
    let client: Option<String> = client.map(|c| c.chars().take(MAX_QUEUE_CLIENT_CHARS).collect());
    conn.execute(
        "INSERT INTO subsonic_play_queues(user_id, track_ids, current_idx, position_ms, changed_at, changed_by)
         VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5)
         ON CONFLICT(user_id) DO UPDATE SET
           track_ids = excluded.track_ids, current_idx = excluded.current_idx,
           position_ms = excluded.position_ms, changed_at = excluded.changed_at, changed_by = excluded.changed_by",
        params![user_id, ids, current, position_ms.max(0), client],
    )?;
    Ok(())
}

/// The user's saved queue, if any.
pub fn get_play_queue(conn: &Connection, user_id: &str) -> rusqlite::Result<Option<PlayQueue>> {
    conn.query_row(
        "SELECT track_ids, current_idx, position_ms, changed_at, changed_by
         FROM subsonic_play_queues WHERE user_id = ?1",
        [user_id],
        |row| {
            let raw: String = row.get(0)?;
            Ok(PlayQueue {
                track_ids: serde_json::from_str(&raw).unwrap_or_default(),
                current_idx: row
                    .get::<_, Option<i64>>(1)?
                    .and_then(|i| usize::try_from(i).ok()),
                position_ms: row.get(2)?,
                changed_at: row.get(3)?,
                changed_by: row.get(4)?,
            })
        },
    )
    .optional()
}

/// Songs for `ids` keyed by id (each id looked up once; unknown ids are absent).
pub fn songs_by_id_map(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<HashMap<String, SubsonicSong>> {
    let unique: Vec<String> = ids
        .iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    Ok(songs_by_ids(conn, &unique)?
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect())
}

/// Library scan state for `getScanStatus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanStatus {
    /// A scan is pending or running.
    pub scanning: bool,
    /// Files scanned so far by the active scans, or the library's track count when idle.
    pub count: i64,
    /// When the last scan finished (`YYYY-MM-DD HH:MM:SS`, UTC).
    pub last_scan: Option<String>,
}

/// Current scan state, in one statement.
pub fn scan_status(conn: &Connection) -> rusqlite::Result<ScanStatus> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM scan_jobs WHERE status IN ('pending','running')),
                (SELECT COALESCE(SUM(files_scanned), 0) FROM scan_jobs WHERE status IN ('pending','running')),
                (SELECT COUNT(*) FROM tracks),
                (SELECT MAX(finished_at) FROM scan_jobs WHERE status = 'done')",
        [],
        |row| {
            let scanning: bool = row.get(0)?;
            let active: i64 = row.get(1)?;
            let tracks: i64 = row.get(2)?;
            Ok(ScanStatus {
                scanning,
                count: if scanning { active } else { tracks },
                last_scan: row.get(3)?,
            })
        },
    )
}

#[cfg(test)]
mod scale_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username, role) VALUES ('u1', 'Alice', 'user'), ('u2', 'bob', 'admin');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'One'), ('lib2', '/m2', 'Two');
             INSERT INTO artists(id, name) VALUES ('ar1', 'The Band'), ('ar2', 'Solo'), ('ar3', 'Guest');
             INSERT INTO albums(id, title, artist_id, year, added_at) VALUES
               ('al1', 'First Album', 'ar1', 1999, '2026-01-01 00:00:00'),
               ('al2', 'Second Album', 'ar1', 2005, '2026-02-01 00:00:00'),
               ('al3', 'Lonely', 'ar2', 2010, '2026-03-01 00:00:00');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration, track_number,
                                disc_number, genre, year, bitrate, play_count, bpm_detected) VALUES
               ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m1/a/01.flac', 200.4, 1, 1, 'Rock, Alternative', 1999, 900, 3, 121.6),
               ('t2', 'lib1', 'ar3', 'al1', 'Closing', '/m1/a/02.flac', 100, 2, 1, 'Rock', 1999, 900, 0, NULL),
               ('t3', 'lib1', 'ar1', 'al2', 'Middle', '/m1/b/01.mp3', 150, 1, 1, 'Prog Rock', 2005, 320, 7, NULL),
               ('t4', 'lib2', 'ar2', 'al3', 'Alone', '/m2/c/01.mp3', 50, 1, 1, 'Jazz', 2010, 192, 0, NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn play_queue_saves_replaces_clears_and_follows_user_delete() {
        let conn = db();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        ensure_subsonic_phase_3_schema(&conn).unwrap();
        // Idempotent (fresh databases also get it from initialize_schema).
        ensure_subsonic_phase_3_schema(&conn).unwrap();
        assert_eq!(get_play_queue(&conn, "u1").unwrap(), None);

        let ids: Vec<String> = ["t1", "t2", "t1"].map(String::from).to_vec();
        save_play_queue(&conn, "u1", &ids, Some(2), 61_500, Some("Amperfy")).unwrap();
        let q = get_play_queue(&conn, "u1").unwrap().unwrap();
        assert_eq!(q.track_ids, ids);
        assert_eq!(q.current_idx, Some(2));
        assert_eq!(q.position_ms, 61_500);
        assert_eq!(q.changed_by.as_deref(), Some("Amperfy"));

        // Replace: an out-of-range index is dropped, a negative position clamps to 0.
        save_play_queue(&conn, "u1", &ids[..1], Some(5), -3, None).unwrap();
        let q = get_play_queue(&conn, "u1").unwrap().unwrap();
        assert_eq!(
            (q.current_idx, q.position_ms, q.changed_by),
            (None, 0, None)
        );

        let map = songs_by_id_map(&conn, &ids).unwrap();
        assert_eq!(map.len(), 2);

        save_play_queue(&conn, "u1", &[], None, 0, None).unwrap();
        assert_eq!(get_play_queue(&conn, "u1").unwrap(), None);

        save_play_queue(&conn, "u2", &ids, Some(0), 0, None).unwrap();
        conn.execute("DELETE FROM users WHERE id = 'u2'", [])
            .unwrap();
        assert_eq!(get_play_queue(&conn, "u2").unwrap(), None);
    }

    #[test]
    fn phase_3_migration_adds_api_key_columns_once() {
        let conn = db();
        ensure_subsonic_phase_3_schema(&conn).unwrap();
        assert!(crate::column_exists(&conn, "subsonic_credentials", "api_key_hash").unwrap());
        assert!(crate::column_exists(&conn, "subsonic_credentials", "api_key_created_at").unwrap());
    }

    #[test]
    fn scan_status_reports_active_scans_or_the_track_count() {
        let conn = db();
        let idle = scan_status(&conn).unwrap();
        assert_eq!(
            idle,
            ScanStatus {
                scanning: false,
                count: 4,
                last_scan: None
            }
        );
        conn.execute_batch(
            "INSERT INTO scan_jobs(id, library_id, status, finished_at, files_scanned) VALUES
               ('j1', 'lib1', 'done', '2026-05-01 10:00:00', 9),
               ('j2', 'lib2', 'done', '2026-05-02 10:00:00', 9),
               ('j3', 'lib1', 'failed', '2026-05-03 10:00:00', 9),
               ('j4', 'lib1', 'running', NULL, 120),
               ('j5', 'lib2', 'pending', NULL, 0);",
        )
        .unwrap();
        let busy = scan_status(&conn).unwrap();
        assert!(busy.scanning);
        assert_eq!(busy.count, 120);
        assert_eq!(busy.last_scan.as_deref(), Some("2026-05-02 10:00:00"));
    }

    #[test]
    fn folders_roots_stars_and_last_modified() {
        let conn = db();
        let folders = music_folders(&conn).unwrap();
        assert_eq!(folders.len(), 2);
        conn.execute_batch(
            "INSERT INTO library_folders(id, library_id, path) VALUES ('lf1', 'lib1', '/extra');
             UPDATE libraries SET last_scan = '2026-05-01 00:00:00' WHERE id = 'lib1';
             INSERT INTO subsonic_stars(user_id, item_type, item_id, starred_at) VALUES
               ('u1', 'album', 'al1', '2026-01-01 00:00:00'), ('u1', 'album', 'al3', '2026-02-01 00:00:00'),
               ('u1', 'artist', 'ar2', '2026-01-01 00:00:00');",
        )
        .unwrap();
        let mut roots = library_roots(&conn, "lib1").unwrap();
        roots.sort();
        assert_eq!(roots, ["/extra", "/m1"]);
        assert_eq!(
            library_last_modified_secs(&conn).unwrap(),
            Some(1_777_593_600)
        );
        let starred = starred_ids(&conn, "u1", ItemKind::Album).unwrap();
        assert_eq!(starred, ["al3", "al1"]);
        assert_eq!(
            albums_by_ids(&conn, &starred)
                .unwrap()
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["al3", "al1"]
        );
        assert_eq!(
            artists_by_ids(&conn, &["ar2".into(), "zz".into()])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn credentials_round_trip_and_status() {
        let conn = db();
        assert_eq!(
            get_credential_status(&conn, "u1").unwrap(),
            SubsonicCredentialStatus::default()
        );
        let account = get_account_by_username(&conn, "alice").unwrap().unwrap();
        assert_eq!(account.user_id, "u1");
        assert!(account.secret_enc.is_none());

        assert!(set_subsonic_secret(&conn, "u1", b"cipher", b"nonce", "u1").unwrap());
        assert!(!set_subsonic_secret(&conn, "missing", b"c", b"n", "u1").unwrap());
        let account = get_account_by_username(&conn, "ALICE").unwrap().unwrap();
        assert_eq!(account.secret_enc.as_deref(), Some(&b"cipher"[..]));
        assert_eq!(account.secret_nonce.as_deref(), Some(&b"nonce"[..]));
        assert!(list_user_ids_with_subsonic_password(&conn)
            .unwrap()
            .contains("u1"));

        record_subsonic_use(&conn, "u1", Some("Symfonium")).unwrap();
        let status = get_credential_status(&conn, "u1").unwrap();
        assert!(status.has_password);
        assert!(status.updated_at.is_some() && status.last_used_at.is_some());
        assert_eq!(status.last_client.as_deref(), Some("Symfonium"));

        assert!(clear_subsonic_secret(&conn, "u1", "u2").unwrap());
        assert!(!get_credential_status(&conn, "u1").unwrap().has_password);
        assert!(list_user_ids_with_subsonic_password(&conn)
            .unwrap()
            .is_empty());
        assert!(!clear_subsonic_secret(&conn, "missing", "u2").unwrap());

        set_subsonic_secret(&conn, "u1", b"c", b"n", "u1").unwrap();
        assert_eq!(clear_all_subsonic_secrets(&conn).unwrap(), 1);
    }

    #[test]
    fn api_keys_are_hash_only_unique_and_independent_of_passwords() {
        let conn = db();
        assert!(get_account_by_api_key(&conn, "h1").unwrap().is_none());
        // A user without a Subsonic password gets a row with no secret.
        assert!(set_api_key_hash(&conn, "u1", "h1").unwrap());
        assert!(!set_api_key_hash(&conn, "missing", "h9").unwrap());
        let account = get_account_by_api_key(&conn, "h1").unwrap().unwrap();
        assert_eq!(account.username, "Alice");
        assert!(account.secret_enc.is_none());
        let status = get_credential_status(&conn, "u1").unwrap();
        assert!(status.has_api_key && !status.has_password);
        assert!(status.api_key_created_at.is_some());
        assert!(list_user_ids_with_api_key(&conn).unwrap().contains("u1"));

        // Unique across users.
        assert!(set_api_key_hash(&conn, "u2", "h1").is_err());
        // Replacing drops the old key.
        set_api_key_hash(&conn, "u1", "h2").unwrap();
        assert!(get_account_by_api_key(&conn, "h1").unwrap().is_none());

        // Clearing every password (key file replaced) leaves API keys alone.
        set_subsonic_secret(&conn, "u1", b"c", b"n", "u1").unwrap();
        clear_all_subsonic_secrets(&conn).unwrap();
        assert!(get_account_by_api_key(&conn, "h2").unwrap().is_some());

        assert!(clear_api_key(&conn, "u1").unwrap());
        assert!(!clear_api_key(&conn, "u1").unwrap());
        assert!(get_account_by_api_key(&conn, "h2").unwrap().is_none());
        assert!(!get_credential_status(&conn, "u1").unwrap().has_api_key);
    }

    #[test]
    fn songs_map_fields_and_keep_requested_order() {
        let conn = db();
        let songs = songs_by_ids(&conn, &["t3".into(), "missing".into(), "t1".into()]).unwrap();
        assert_eq!(
            songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["t3", "t1"]
        );
        let t1 = &songs[1];
        assert_eq!(t1.album.as_deref(), Some("First Album"));
        assert_eq!(t1.artist.as_deref(), Some("The Band"));
        assert_eq!(t1.album_artist.as_deref(), Some("The Band"));
        assert_eq!(t1.genre.as_deref(), Some("Rock, Alternative"));
        assert_eq!(t1.bpm_detected, Some(121.6));
        assert_eq!(t1.library_id.as_deref(), Some("lib1"));
        assert!(get_song(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn album_and_artist_listings() {
        let conn = db();
        let album = get_album(&conn, "al1").unwrap().unwrap();
        assert_eq!((album.song_count, album.play_count), (2, 3));
        assert_eq!(album.artist, "The Band");
        assert!((album.duration - 300.4).abs() < 1e-9);
        assert_eq!(
            album_songs(&conn, "al1")
                .unwrap()
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            ["t1", "t2"]
        );
        assert_eq!(
            artist_albums(&conn, "ar1")
                .unwrap()
                .iter()
                .map(|a| a.id.clone())
                .collect::<Vec<_>>(),
            ["al1", "al2"]
        );
        let artists = list_artists(&conn, None, None, 0, -1).unwrap();
        assert_eq!(
            artists
                .iter()
                .map(|a| (a.name.as_str(), a.album_count))
                .collect::<Vec<_>>(),
            [("Solo", 1), ("The Band", 2)]
        );
        assert_eq!(
            list_artists(&conn, Some("lib2"), None, 0, -1)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_artists(&conn, None, Some("ban"), 0, -1).unwrap()[0].id,
            "ar1"
        );
        // Guest owns no album: not an album artist, but still fetchable by id.
        assert_eq!(get_artist(&conn, "ar3").unwrap().unwrap().album_count, 0);
    }

    #[test]
    fn album_lists_by_type() {
        let conn = db();
        let ids = |list_type: &str, from: Option<i64>, to: Option<i64>, genre: Option<&str>| {
            album_list(
                &conn,
                AlbumListQuery {
                    list_type,
                    user_id: "u1",
                    size: 10,
                    offset: 0,
                    from_year: from,
                    to_year: to,
                    genre,
                    library: None,
                },
            )
            .unwrap()
            .map(|v| v.into_iter().map(|a| a.id).collect::<Vec<_>>())
        };
        assert_eq!(
            ids("newest", None, None, None).unwrap(),
            ["al3", "al2", "al1"]
        );
        assert_eq!(
            ids("alphabeticalByName", None, None, None).unwrap(),
            ["al1", "al3", "al2"]
        );
        assert_eq!(
            ids("alphabeticalByArtist", None, None, None).unwrap(),
            ["al3", "al1", "al2"]
        );
        // `frequent` ranks by the user's own plays (none yet).
        assert!(ids("frequent", None, None, None).unwrap().is_empty());
        assert_eq!(
            ids("byYear", Some(2010), Some(1999), None).unwrap(),
            ["al3", "al2", "al1"]
        );
        assert_eq!(
            ids("byYear", Some(2000), Some(2010), None).unwrap(),
            ["al2", "al3"]
        );
        assert_eq!(ids("byGenre", None, None, Some("rock")).unwrap(), ["al1"]);
        assert_eq!(ids("random", None, None, None).unwrap().len(), 3);
        assert!(ids("highest", None, None, None).unwrap().is_empty());
        assert!(ids("starred", None, None, None).unwrap().is_empty());
        assert!(ids("recent", None, None, None).unwrap().is_empty());
        assert!(ids("bogus", None, None, None).is_none());

        conn.execute_batch(
            "INSERT INTO album_ratings(user_id, album_id, rating) VALUES ('u1', 'al3', 4.5), ('u1', 'al1', 2);
             INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES ('u1', 'album', 'al2');
             INSERT INTO play_history(id, user_id, track_id, played_at) VALUES ('h1', 'u1', 't4', '2026-05-01 10:00:00');",
        )
        .unwrap();
        assert_eq!(ids("highest", None, None, None).unwrap(), ["al3", "al1"]);
        assert_eq!(ids("starred", None, None, None).unwrap(), ["al2"]);
        assert_eq!(ids("recent", None, None, None).unwrap(), ["al3"]);
        assert_eq!(ids("frequent", None, None, None).unwrap(), ["al3"]);
    }

    #[test]
    fn song_listings_paging_search_genre_random() {
        let conn = db();
        let page1 = songs_page(&conn, None, 0, 2).unwrap();
        let page2 = songs_page(&conn, None, 2, 2).unwrap();
        let mut all: Vec<String> = page1.into_iter().chain(page2).map(|s| s.id).collect();
        all.sort();
        assert_eq!(all, ["t1", "t2", "t3", "t4"]);
        assert_eq!(songs_page(&conn, Some("lib2"), 0, 10).unwrap().len(), 1);

        // Genre tags match on boundaries: "Rock" does not pick up "Prog Rock".
        let rock: Vec<String> = songs_by_genre(&conn, "Rock", None, 0, 10)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(rock, ["t1", "t2"]);
        let names: Vec<String> = genres(&conn).unwrap().into_iter().map(|g| g.name).collect();
        assert!(names.contains(&"Rock".to_string()) && names.contains(&"Alternative".to_string()));

        let random = random_songs(&conn, 10, Some("Rock"), None, None, None).unwrap();
        assert_eq!(random.len(), 2);
        assert_eq!(
            random_songs(&conn, 1, None, None, None, None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            random_songs(&conn, 10, None, Some(2005), Some(2010), None)
                .unwrap()
                .len(),
            2
        );

        assert_eq!(search_albums(&conn, "album", None, 0, 10).unwrap().len(), 2);
        assert_eq!(search_albums(&conn, "", None, 0, 10).unwrap().len(), 3);
        assert_eq!(search_albums(&conn, "100%", None, 0, 10).unwrap().len(), 0);
        assert!(search_songs(&conn, "  ", None, 0, 10).unwrap().is_empty());

        conn.execute_batch(
            "INSERT INTO tracks_fts(track_id, title, artist, album, genre, composer, file_path) VALUES
               ('t1', 'Opening', 'The Band', 'First Album', 'Rock', '', ''),
               ('t4', 'Alone', 'Solo', 'Lonely', 'Jazz', '', '');",
        )
        .unwrap();
        let hits: Vec<String> = search_songs(&conn, "open", None, 0, 10)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(hits, ["t1"]);
        assert!(search_songs(&conn, "open", Some("lib2"), 0, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn playlist_songs_are_owner_scoped_and_ordered() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO playlists(id, user_id, name) VALUES ('p1', 'u1', 'Mine');
             INSERT INTO playlist_tracks(id, playlist_id, track_id, position) VALUES
               ('pt1', 'p1', 't3', 0), ('pt2', 'p1', 't1', 1);",
        )
        .unwrap();
        let songs = playlist_songs(&conn, "p1", "u1").unwrap().unwrap();
        assert_eq!(
            songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["t3", "t1"]
        );
        assert!(playlist_songs(&conn, "p1", "u2").unwrap().is_none());
    }

    #[test]
    fn annotations_are_per_user_and_batched() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('u1', 't1', 3.5), ('u2', 't2', 5);
             INSERT INTO subsonic_stars(user_id, item_type, item_id, starred_at) VALUES
               ('u1', 'track', 't2', '2026-04-01 00:00:00'), ('u1', 'album', 't1', '2026-04-01 00:00:00');
             INSERT INTO play_history(id, user_id, track_id, played_at) VALUES
               ('h1', 'u1', 't1', '2026-05-01 10:00:00'), ('h2', 'u1', 't1', '2026-05-02 10:00:00');",
        )
        .unwrap();
        let a = annotations(&conn, "u1", ItemKind::Track, &["t1".into(), "t2".into()]).unwrap();
        assert_eq!(a.ratings.get("t1"), Some(&3.5));
        assert!(!a.ratings.contains_key("t2"));
        assert_eq!(a.starred.len(), 1);
        assert!(a.starred.contains_key("t2"));
        assert_eq!(
            a.played.get("t1").map(String::as_str),
            Some("2026-05-02 10:00:00")
        );
        assert_eq!(a.plays.get("t1"), Some(&2));
        assert!(!a.plays.contains_key("t2"));
        let other = annotations(&conn, "u2", ItemKind::Track, &["t1".into()]).unwrap();
        assert!(other.plays.is_empty(), "play counts are per user");
        let albums =
            annotations(&conn, "u1", ItemKind::Album, &["al1".into(), "al2".into()]).unwrap();
        assert_eq!(albums.plays.get("al1"), Some(&2));
        assert!(!albums.plays.contains_key("al2"));
        assert_eq!(
            annotations(&conn, "u1", ItemKind::Album, &[]).unwrap(),
            Annotations::default()
        );
    }

    #[test]
    fn user_track_stats_follow_play_history_and_backfill() {
        let conn = db();
        let stats = |user: &str, track: &str| -> Option<(i64, String)> {
            conn.query_row(
                "SELECT play_count, last_played_at FROM user_track_stats WHERE user_id = ?1 AND track_id = ?2",
                params![user, track],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
        };
        conn.execute_batch(
            "INSERT INTO play_history(id, user_id, track_id, played_at) VALUES
               ('h1', 'u1', 't1', '2026-05-02 10:00:00'), ('h2', 'u1', 't1', '2026-05-01 10:00:00');",
        )
        .unwrap();
        // An older play does not move last_played_at back.
        assert_eq!(stats("u1", "t1"), Some((2, "2026-05-02 10:00:00".into())));
        crate::playback::insert_play_history(&conn, "u2", "t1").unwrap();
        assert_eq!(stats("u2", "t1").map(|s| s.0), Some(1));
        conn.execute("DELETE FROM play_history WHERE id = 'h1'", [])
            .unwrap();
        assert_eq!(stats("u1", "t1").map(|s| s.0), Some(1));
        conn.execute("DELETE FROM play_history WHERE id = 'h2'", [])
            .unwrap();
        assert_eq!(stats("u1", "t1"), None);

        // Backfill: rebuild from existing history (as the migration does on an upgraded database).
        conn.execute_batch(
            "DROP TRIGGER trg_play_history_stats_insert; DROP TRIGGER trg_play_history_stats_delete;
             DROP TABLE user_track_stats;
             INSERT INTO play_history(id, user_id, track_id, played_at) VALUES
               ('h3', 'u1', 't2', '2026-06-01 10:00:00'), ('h4', 'u1', 't2', '2026-06-03 10:00:00');",
        )
        .unwrap();
        ensure_user_track_stats_schema(&conn).unwrap();
        assert_eq!(stats("u1", "t2"), Some((2, "2026-06-03 10:00:00".into())));
        assert_eq!(stats("u2", "t1").map(|s| s.0), Some(1));
        // Idempotent: running it again does not double counts.
        ensure_user_track_stats_schema(&conn).unwrap();
        assert_eq!(stats("u1", "t2").map(|s| s.0), Some(2));
    }

    #[test]
    fn stars_resolve_kinds_sweep_and_follow_artist_merges() {
        let conn = db();
        assert_eq!(
            resolve_item_kind(&conn, "t1").unwrap(),
            Some(ItemKind::Track)
        );
        assert_eq!(
            resolve_item_kind(&conn, "al2").unwrap(),
            Some(ItemKind::Album)
        );
        assert_eq!(
            resolve_item_kind(&conn, "ar3").unwrap(),
            Some(ItemKind::Artist)
        );
        assert_eq!(resolve_item_kind(&conn, "zz").unwrap(), None);

        let items = vec![
            (ItemKind::Track, "t1".to_owned()),
            (ItemKind::Album, "al1".to_owned()),
            (ItemKind::Artist, "ar2".to_owned()),
        ];
        set_stars(&conn, "u1", &items, true).unwrap();
        set_stars(&conn, "u1", &items, true).unwrap(); // idempotent
        assert_eq!(starred_ids(&conn, "u1", ItemKind::Track).unwrap(), ["t1"]);
        set_stars(&conn, "u1", &items[..1], false).unwrap();
        assert!(starred_ids(&conn, "u1", ItemKind::Track)
            .unwrap()
            .is_empty());

        // Merging ar2 into ar1 moves the star.
        crate::music::merge_artists(
            &conn,
            &[coerce_entity_id("ar1"), coerce_entity_id("ar2")],
            "The Band",
            Some(&coerce_entity_id("ar1")),
            "u1",
        )
        .unwrap();
        assert_eq!(starred_ids(&conn, "u1", ItemKind::Artist).unwrap(), ["ar1"]);

        conn.execute_batch(
            "INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES ('u1', 'track', 'gone'), ('u1', 'album', 'gone');",
        )
        .unwrap();
        assert_eq!(sweep_stale_stars(&conn).unwrap(), 2);
        assert_eq!(starred_ids(&conn, "u1", ItemKind::Album).unwrap(), ["al1"]);
    }

    #[test]
    fn playlist_edits_replace_remove_append_and_dedupe() {
        let conn = db();
        conn.execute_batch("INSERT INTO playlists(id, user_id, name) VALUES ('p1', 'u1', 'Mix');")
            .unwrap();
        assert_eq!(unique_playlist_name(&conn, "u1", "Mix").unwrap(), "Mix (2)");
        assert_eq!(unique_playlist_name(&conn, "u1", "Other").unwrap(), "Other");
        assert_eq!(unique_playlist_name(&conn, "u2", "Mix").unwrap(), "Mix");

        let ids = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(
            set_playlist_tracks(&conn, "p1", "u1", &ids(&["t3", "t1", "nope", "t3", "t2"]))
                .unwrap()
        );
        assert_eq!(
            playlist_track_ids(&conn, "p1", "u1").unwrap().unwrap(),
            ["t3", "t1", "t2"]
        );
        assert!(
            edit_playlist_entries(&conn, "p1", "u1", &[0, 2, 99], &ids(&["t4", "t1"])).unwrap()
        );
        assert_eq!(
            playlist_track_ids(&conn, "p1", "u1").unwrap().unwrap(),
            ["t1", "t4"]
        );
        assert!(!set_playlist_tracks(&conn, "p1", "u2", &ids(&["t1"])).unwrap());
        assert!(playlist_track_ids(&conn, "p1", "u2").unwrap().is_none());
        assert!(!edit_playlist_entries(&conn, "p1", "u2", &[0], &[]).unwrap());
    }

    #[test]
    fn mixes_info_lyrics_and_top_songs() {
        let conn = db();
        // Mix rows only need their own columns here; skip seeding parent jobs.
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO mix_outputs(id, job_id, playlist_id, user_id, file_path, file_name, name, cover_album_ids,
                                     source_playlist_name, duration_sec, file_size_bytes, format, created_at) VALUES
               ('m1', 'j1', NULL, 'u1', '/mixes/a.mp3', 'a.mp3', 'Friday', 'al2,al1', 'Party', 3600, 900, 'MP3', '2026-01-01 00:00:00'),
               ('m2', 'j2', NULL, 'u1', '/mixes/b.flac', 'b.flac', '', NULL, '', 60, 10, 'flac', '2026-02-01 00:00:00'),
               ('m3', 'j3', NULL, 'u2', '/mixes/c.mp3', 'c.mp3', 'Theirs', NULL, '', 60, 10, 'mp3', '2026-03-01 00:00:00');
             UPDATE artists SET description = 'Local bio', lastfm_mbid = 'mbid-1' WHERE id = 'ar1';
             UPDATE albums SET description = 'Liner notes' WHERE id = 'al1';
             INSERT INTO lyrics_cache(track_id, artist, title, lyrics) VALUES ('t1', 'The Band', 'Opening', 'la la');
             PRAGMA foreign_keys = ON;",
        )
        .unwrap();
        let mixes = list_user_mixes(&conn, "u1").unwrap();
        assert_eq!(
            mixes.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["m2", "m1"]
        );
        assert_eq!(mixes[0].name, "b.flac");
        assert_eq!(mixes[1].format, "mp3");
        assert_eq!(mixes[1].cover_album_id.as_deref(), Some("al2"));
        assert_eq!(mixes[1].source_playlist_name.as_deref(), Some("Party"));
        assert!(get_user_mix(&conn, "u1", "m3").unwrap().is_none());
        assert_eq!(
            get_user_mix(&conn, "u2", "m3").unwrap().unwrap().name,
            "Theirs"
        );

        assert_eq!(
            artist_info(&conn, "ar1").unwrap().unwrap(),
            ArtistInfo {
                name: "The Band".into(),
                description: Some("Local bio".into()),
                musicbrainz_id: Some("mbid-1".into()),
            }
        );
        assert_eq!(
            album_info(&conn, "al1").unwrap().unwrap(),
            AlbumInfo {
                title: "First Album".into(),
                artist: "The Band".into(),
                description: Some("Liner notes".into()),
            }
        );
        assert_eq!(
            cached_lyrics(&conn, " the band", "OPENING")
                .unwrap()
                .as_deref(),
            Some("la la")
        );
        assert!(cached_lyrics(&conn, "The Band", "Closing")
            .unwrap()
            .is_none());
        assert_eq!(
            artist_id_by_name(&conn, "the band").unwrap().as_deref(),
            Some("ar1")
        );
        assert_eq!(
            artist_id_by_name(&conn, "Guest").unwrap().as_deref(),
            Some("ar3")
        );
        assert!(artist_id_by_name(&conn, "Nobody").unwrap().is_none());
        let top: Vec<String> = artist_songs_by_plays(&conn, "ar1", 10)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(top, ["t3", "t1"]);
    }

    #[test]
    fn scrobbles_count_once_within_the_window() {
        let conn = db();
        let at = 1_780_000_000;
        assert_eq!(
            record_scrobble(&conn, "u1", "t2", Some(at)).unwrap(),
            ScrobbleOutcome::Recorded
        );
        assert_eq!(
            record_scrobble(&conn, "u1", "t2", Some(at + 10)).unwrap(),
            ScrobbleOutcome::Duplicate
        );
        assert_eq!(
            record_scrobble(&conn, "u1", "t2", Some(at + 200)).unwrap(),
            ScrobbleOutcome::Recorded
        );
        assert_eq!(
            record_scrobble(&conn, "u2", "t2", Some(at)).unwrap(),
            ScrobbleOutcome::Recorded
        );
        assert_eq!(
            record_scrobble(&conn, "u1", "nope", None).unwrap(),
            ScrobbleOutcome::NotFound
        );
        // Far-future timestamps fall back to now instead of being stored.
        assert_eq!(
            record_scrobble(&conn, "u1", "t1", Some(i64::MAX / 2)).unwrap(),
            ScrobbleOutcome::Recorded
        );
        let plays: i64 = conn
            .query_row("SELECT play_count FROM tracks WHERE id = 't2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(plays, 3);
        let stored: String = conn
            .query_row(
                "SELECT MIN(played_at) FROM play_history WHERE user_id = 'u1' AND track_id = 't2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "2026-05-28 20:26:40");
    }
}

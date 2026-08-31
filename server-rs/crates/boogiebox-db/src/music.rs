//! Defines SQLite data access and schema helpers for Music.

use rusqlite::{
    params_from_iter,
    types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, Value, ValueRef},
    Connection, OptionalExtension,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{collections::HashMap, fmt};

// ── EntityId ──────────────────────────────────────────────────────────────────

/// Public Entity Id data shape used by BoogieBox.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityId {
    Int(i64),
    Str(String),
}

impl FromSql for EntityId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Integer(n) => Ok(EntityId::Int(n)),
            ValueRef::Text(s) => Ok(EntityId::Str(
                std::str::from_utf8(s)
                    .map_err(|_| FromSqlError::InvalidType)?
                    .to_owned(),
            )),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl ToSql for EntityId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            EntityId::Int(n) => Ok(ToSqlOutput::Owned(Value::Integer(*n))),
            EntityId::Str(s) => Ok(ToSqlOutput::Owned(Value::Text(s.clone()))),
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntityId::Int(n) => write!(f, "{n}"),
            EntityId::Str(s) => write!(f, "{s}"),
        }
    }
}

/// Documents the Coerce Entity Id public API surface.
pub fn coerce_entity_id(raw: &str) -> EntityId {
    match raw.parse::<i64>() {
        Ok(n) if n > 0 => EntityId::Int(n),
        _ => EntityId::Str(raw.to_owned()),
    }
}

fn id_to_value(id: &EntityId) -> Value {
    match id {
        EntityId::Int(n) => Value::Integer(*n),
        EntityId::Str(s) => Value::Text(s.clone()),
    }
}

// ── Schema helpers ────────────────────────────────────────────────────────────

fn table_exists_local(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn column_exists_local(conn: &Connection, table: &str, col: &str) -> rusqlite::Result<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row?.eq_ignore_ascii_case(col) {
            return Ok(true);
        }
    }
    Ok(false)
}

// ── Library ───────────────────────────────────────────────────────────────────

/// Public Library Folder data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct LibraryFolder {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Path public API surface.
    pub path: String,
    /// Documents the Position public API surface.
    pub position: i64,
    /// Documents the Added At public API surface.
    pub added_at: Option<String>,
}

/// Public Library Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct LibraryRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Name public API surface.
    pub name: String,
    /// Documents the Path public API surface.
    pub path: Option<String>,
    /// Documents the Library Type public API surface.
    pub library_type: Option<String>,
    /// Documents the Scanner Profile public API surface.
    pub scanner_profile: Option<String>,
    /// Documents the Metadata Mode public API surface.
    pub metadata_mode: Option<String>,
    /// Documents the Added At public API surface.
    pub added_at: Option<String>,
    /// Documents the Last Scan public API surface.
    pub last_scan: Option<String>,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
    /// Documents the Primary Path public API surface.
    pub primary_path: Option<String>,
    /// Documents the Folder Count public API surface.
    pub folder_count: i64,
    /// Documents the Folders public API surface.
    pub folders: Vec<LibraryFolder>,
}

/// Documents the List Libraries public API surface.
pub fn list_libraries(conn: &Connection) -> rusqlite::Result<Vec<LibraryRow>> {
    let has_library_type = column_exists_local(conn, "libraries", "library_type")?;
    let has_folders_table = table_exists_local(conn, "library_folders");

    let scanner_col = if column_exists_local(conn, "libraries", "scanner_profile")? {
        "l.scanner_profile"
    } else {
        "NULL"
    };
    let metadata_col = if column_exists_local(conn, "libraries", "metadata_mode")? {
        "l.metadata_mode"
    } else {
        "NULL"
    };
    let added_at_col = if column_exists_local(conn, "libraries", "added_at")? {
        "l.added_at"
    } else {
        "NULL"
    };
    let last_scan_col = if column_exists_local(conn, "libraries", "last_scan")? {
        "l.last_scan"
    } else {
        "NULL"
    };

    let sql = if has_library_type {
        format!(
            "SELECT l.id, l.name, l.path, l.library_type, {scanner_col}, {metadata_col}, {added_at_col}, {last_scan_col},
             (SELECT COUNT(*) FROM tracks t WHERE t.library_id = l.id) AS track_count
             FROM libraries l ORDER BY l.name"
        )
    } else {
        format!(
            "SELECT l.id, l.name, l.path, NULL AS library_type, {scanner_col}, {metadata_col}, {added_at_col}, {last_scan_col},
             COUNT(t.id) AS track_count
             FROM libraries l LEFT JOIN tracks t ON t.library_id = l.id
             GROUP BY l.id ORDER BY l.name"
        )
    };

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        EntityId,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    )> = conn
        .prepare(&sql)?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut libs = Vec::with_capacity(rows.len());
    for (
        id,
        name,
        path,
        library_type,
        scanner_profile,
        metadata_mode,
        added_at,
        last_scan,
        track_count,
    ) in rows
    {
        let folders: Vec<LibraryFolder> = if has_folders_table {
            conn.prepare(
                "SELECT id, library_id, path, position, added_at FROM library_folders WHERE library_id=? ORDER BY position ASC, id ASC",
            )?
            .query_map([&id], |row| {
                Ok(LibraryFolder {
                    id: row.get(0)?,
                    library_id: row.get(1)?,
                    path: row.get(2)?,
                    position: row.get(3)?,
                    added_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?
        } else {
            match &path {
                Some(p) => vec![LibraryFolder {
                    id: id.clone(),
                    library_id: id.clone(),
                    path: p.clone(),
                    position: 0,
                    added_at: None,
                }],
                None => vec![],
            }
        };

        let primary_path = folders
            .first()
            .map(|f| f.path.clone())
            .or_else(|| path.clone());
        let folder_count = if folders.is_empty() && primary_path.is_some() {
            1
        } else {
            folders.len() as i64
        };

        libs.push(LibraryRow {
            id,
            name,
            path: primary_path.clone(),
            library_type,
            scanner_profile,
            metadata_mode,
            added_at,
            last_scan,
            track_count,
            primary_path,
            folder_count,
            folders,
        });
    }

    Ok(libs)
}

// ── Stats ─────────────────────────────────────────────────────────────────────

/// Public Stats Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct StatsRow {
    /// Documents the Total Tracks public API surface.
    pub total_tracks: i64,
    /// Documents the Total Artists public API surface.
    pub total_artists: i64,
    /// Documents the Total Albums public API surface.
    pub total_albums: i64,
    /// Documents the Total Libraries public API surface.
    pub total_libraries: i64,
    /// Documents the Total Hours public API surface.
    pub total_hours: Option<f64>,
    /// Documents the Total Gb public API surface.
    pub total_gb: Option<f64>,
}

/// Documents the Get Stats public API surface.
pub fn get_stats(conn: &Connection) -> rusqlite::Result<StatsRow> {
    // stats_cache fast path
    let cached = conn.query_row(
        "SELECT total_tracks, total_artists, total_albums, total_libraries, total_hours, total_gb FROM stats_cache WHERE id = 1",
        [],
        |row| Ok(StatsRow {
            total_tracks: row.get(0)?,
            total_artists: row.get(1)?,
            total_albums: row.get(2)?,
            total_libraries: row.get(3)?,
            total_hours: row.get(4)?,
            total_gb: row.get(5)?,
        }),
    );
    if let Ok(row) = cached {
        return Ok(row);
    }

    conn.query_row(
        "SELECT
          (SELECT COUNT(*) FROM tracks),
          (SELECT COUNT(*) FROM artists ar WHERE EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = ar.id)),
          (SELECT COUNT(DISTINCT album_id) FROM tracks WHERE album_id IS NOT NULL),
          (SELECT COUNT(*) FROM libraries),
          (SELECT ROUND(COALESCE(SUM(duration),0)/3600.0,1) FROM tracks),
          (SELECT ROUND(COALESCE(SUM(file_size),0)/1073741824.0,2) FROM tracks)",
        [],
        |row| {
            Ok(StatsRow {
                total_tracks: row.get(0)?,
                total_artists: row.get(1)?,
                total_albums: row.get(2)?,
                total_libraries: row.get(3)?,
                total_hours: row.get(4)?,
                total_gb: row.get(5)?,
            })
        },
    )
}

// ── Genre ─────────────────────────────────────────────────────────────────────

/// Public Genre Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct GenreRow {
    /// Documents the Genre public API surface.
    pub genre: String,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
}

/// Public Home Genre Summary data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct HomeGenreSummary {
    /// Documents the Label public API surface.
    pub label: String,
    /// Documents the Canonical Key public API surface.
    pub canonical_key: String,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
    /// Documents the Artist Count public API surface.
    pub artist_count: i64,
    /// Documents the Album Count public API surface.
    pub album_count: i64,
    /// Documents the Raw Labels public API surface.
    pub raw_labels: Vec<String>,
}

/// Documents the List Genres public API surface.
pub fn list_genres(conn: &Connection) -> rusqlite::Result<Vec<GenreRow>> {
    conn.prepare(
        "SELECT genre, COUNT(*) AS track_count FROM tracks WHERE genre IS NOT NULL AND genre != '' GROUP BY genre ORDER BY genre",
    )?
    .query_map([], |row| Ok(GenreRow { genre: row.get(0)?, track_count: row.get(1)? }))?
    .collect()
}

/// Documents the List Home Genre Summaries public API surface.
pub fn list_home_genre_summaries(
    conn: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<HomeGenreSummary>> {
    let raw: Vec<(String, i64, i64, i64)> = conn
        .prepare(
            "SELECT genre, COUNT(*) AS track_count, COUNT(DISTINCT artist_id) AS artist_count, COUNT(DISTINCT album_id) AS album_count
             FROM tracks WHERE genre IS NOT NULL AND TRIM(genre) != '' GROUP BY genre",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;

    Ok(build_home_genre_summaries(raw, limit))
}

fn canonicalize_home_genre(label: &str) -> String {
    label
        .trim()
        .to_lowercase()
        .replace('&', "and")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn build_home_genre_summaries(
    rows: Vec<(String, i64, i64, i64)>,
    limit: usize,
) -> Vec<HomeGenreSummary> {
    let mut agg: HashMap<String, (i64, i64, i64, HashMap<String, i64>)> = HashMap::new();
    for (genre, track_count, artist_count, album_count) in rows {
        for part in genre.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let key = canonicalize_home_genre(part);
            if key.is_empty() {
                continue;
            }
            let e = agg.entry(key).or_insert((0, 0, 0, HashMap::new()));
            e.0 += track_count;
            e.1 += artist_count;
            e.2 += album_count;
            *e.3.entry(part.to_owned()).or_insert(0) += track_count;
        }
    }

    let mut summaries: Vec<HomeGenreSummary> = agg
        .into_iter()
        .map(
            |(canonical_key, (track_count, artist_count, album_count, label_map))| {
                let mut labels: Vec<(String, i64)> = label_map.into_iter().collect();
                labels.sort_by(|a, b| {
                    if b.1 != a.1 {
                        return b.1.cmp(&a.1);
                    }
                    let pa = a.0.chars().filter(|c| !c.is_alphanumeric()).count();
                    let pb = b.0.chars().filter(|c| !c.is_alphanumeric()).count();
                    if pa != pb {
                        return pa.cmp(&pb);
                    }
                    if a.0.len() != b.0.len() {
                        return a.0.len().cmp(&b.0.len());
                    }
                    a.0.cmp(&b.0)
                });
                let label = labels
                    .first()
                    .map(|(l, _)| l.clone())
                    .unwrap_or_else(|| canonical_key.clone());
                HomeGenreSummary {
                    label,
                    canonical_key,
                    track_count,
                    artist_count,
                    album_count,
                    raw_labels: labels.into_iter().map(|(l, _)| l).collect(),
                }
            },
        )
        .collect();

    summaries.sort_by(|a, b| {
        if b.track_count != a.track_count {
            return b.track_count.cmp(&a.track_count);
        }
        if b.artist_count != a.artist_count {
            return b.artist_count.cmp(&a.artist_count);
        }
        a.label.cmp(&b.label)
    });
    summaries.truncate(limit.max(1));
    summaries
}

// ── Artist ────────────────────────────────────────────────────────────────────

/// Public Artist Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ArtistRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Name public API surface.
    pub name: String,
    /// Documents the Rating public API surface.
    pub rating: Option<f64>,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
    /// Documents the Album Count public API surface.
    pub album_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_locked: Option<i64>,
    /// Set while a merge master (§ artist consolidation) is waiting on its
    /// one-shot post-merge online identity match — see `merge_artists`.
    /// `None` outside the artist-detail fetch, same as `metadata_locked`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_lock_pending: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<String>,
}

/// External artist namespaces supported by the metadata enrichment pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistIdentityProvider {
    LastFm,
    Deezer,
    Spotify,
    Discogs,
}

/// Optional provider identities stored for one local artist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistExternalIdentity {
    pub artist_id: EntityId,
    pub name: String,
    pub lastfm_mbid: Option<String>,
    pub lastfm_canonical_name: Option<String>,
    pub lastfm_identity_checked_at: Option<String>,
    pub deezer_artist_id: Option<String>,
    pub deezer_identity_checked_at: Option<String>,
    pub spotify_artist_id: Option<String>,
    pub spotify_identity_checked_at: Option<String>,
    pub discogs_artist_id: Option<String>,
    pub discogs_identity_checked_at: Option<String>,
}

/// Loads the complete optional external identity state for one artist.
pub fn get_artist_external_identity(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<Option<ArtistExternalIdentity>> {
    conn.query_row(
        "SELECT id, name, lastfm_mbid, lastfm_canonical_name, lastfm_identity_checked_at,
                deezer_artist_id, deezer_identity_checked_at,
                spotify_artist_id, spotify_identity_checked_at,
                discogs_artist_id, discogs_identity_checked_at
         FROM artists WHERE id=?1",
        [artist_id],
        |row| {
            Ok(ArtistExternalIdentity {
                artist_id: row.get(0)?,
                name: row.get(1)?,
                lastfm_mbid: row.get(2)?,
                lastfm_canonical_name: row.get(3)?,
                lastfm_identity_checked_at: row.get(4)?,
                deezer_artist_id: row.get(5)?,
                deezer_identity_checked_at: row.get(6)?,
                spotify_artist_id: row.get(7)?,
                spotify_identity_checked_at: row.get(8)?,
                discogs_artist_id: row.get(9)?,
                discogs_identity_checked_at: row.get(10)?,
            })
        },
    )
    .optional()
}

fn nonempty_identity(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Fills a provider identity only when the corresponding local field is null.
/// A successful provider check is timestamped even when no confident ID exists.
pub fn persist_artist_identity_if_missing(
    conn: &Connection,
    artist_id: &EntityId,
    provider: ArtistIdentityProvider,
    external_id: Option<&str>,
    canonical_name: Option<&str>,
) -> rusqlite::Result<bool> {
    let external_id = nonempty_identity(external_id);
    let canonical_name = nonempty_identity(canonical_name);
    let changed = match provider {
        ArtistIdentityProvider::LastFm => conn.execute(
            "UPDATE artists
             SET lastfm_mbid=COALESCE(lastfm_mbid, ?1),
                 lastfm_canonical_name=COALESCE(lastfm_canonical_name, ?2),
                 lastfm_identity_checked_at=datetime('now')
             WHERE id=?3
               AND (lastfm_mbid IS NULL OR lastfm_canonical_name IS NULL
                    OR lastfm_identity_checked_at IS NULL)",
            rusqlite::params![external_id, canonical_name, artist_id],
        )?,
        ArtistIdentityProvider::Deezer => conn.execute(
            "UPDATE artists
             SET deezer_artist_id=COALESCE(deezer_artist_id, ?1),
                 deezer_identity_checked_at=datetime('now')
             WHERE id=?2 AND (deezer_artist_id IS NULL OR deezer_identity_checked_at IS NULL)",
            rusqlite::params![external_id, artist_id],
        )?,
        ArtistIdentityProvider::Spotify => conn.execute(
            "UPDATE artists
             SET spotify_artist_id=COALESCE(spotify_artist_id, ?1),
                 spotify_identity_checked_at=datetime('now')
             WHERE id=?2 AND (spotify_artist_id IS NULL OR spotify_identity_checked_at IS NULL)",
            rusqlite::params![external_id, artist_id],
        )?,
        ArtistIdentityProvider::Discogs => conn.execute(
            "UPDATE artists
             SET discogs_artist_id=COALESCE(discogs_artist_id, ?1),
                 discogs_identity_checked_at=datetime('now')
             WHERE id=?2 AND (discogs_artist_id IS NULL OR discogs_identity_checked_at IS NULL)",
            rusqlite::params![external_id, artist_id],
        )?,
    };
    Ok(changed > 0)
}

/// Returns library artists with at least one missing identity whose last
/// confirmed check is absent or older than the supplied SQLite timestamp.
pub fn list_artists_needing_external_identity(
    conn: &Connection,
    library_id: &EntityId,
    stale_before: &str,
) -> rusqlite::Result<Vec<ArtistExternalIdentity>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT ar.id, ar.name, ar.lastfm_mbid, ar.lastfm_canonical_name,
                ar.lastfm_identity_checked_at, ar.deezer_artist_id,
                ar.deezer_identity_checked_at, ar.spotify_artist_id,
                ar.spotify_identity_checked_at, ar.discogs_artist_id,
                ar.discogs_identity_checked_at
         FROM artists ar
         JOIN tracks t ON t.artist_id=ar.id
         WHERE t.library_id=?1 AND (
           (ar.lastfm_mbid IS NULL AND (ar.lastfm_identity_checked_at IS NULL OR ar.lastfm_identity_checked_at < ?2)) OR
           (ar.deezer_artist_id IS NULL AND (ar.deezer_identity_checked_at IS NULL OR ar.deezer_identity_checked_at < ?2)) OR
           (ar.spotify_artist_id IS NULL AND (ar.spotify_identity_checked_at IS NULL OR ar.spotify_identity_checked_at < ?2)) OR
           (ar.discogs_artist_id IS NULL AND (ar.discogs_identity_checked_at IS NULL OR ar.discogs_identity_checked_at < ?2))
         )
         ORDER BY ar.name, ar.id",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![library_id, stale_before], |row| {
            Ok(ArtistExternalIdentity {
                artist_id: row.get(0)?,
                name: row.get(1)?,
                lastfm_mbid: row.get(2)?,
                lastfm_canonical_name: row.get(3)?,
                lastfm_identity_checked_at: row.get(4)?,
                deezer_artist_id: row.get(5)?,
                deezer_identity_checked_at: row.get(6)?,
                spotify_artist_id: row.get(7)?,
                spotify_identity_checked_at: row.get(8)?,
                discogs_artist_id: row.get(9)?,
                discogs_identity_checked_at: row.get(10)?,
            })
        })?
        .collect();
    rows
}

/// Resolves an external identifier to exactly one local artist that owns at
/// least one release. Ambiguous identifiers intentionally produce no match.
pub fn find_owned_artist_by_external_identity(
    conn: &Connection,
    provider: ArtistIdentityProvider,
    external_id: &str,
) -> rusqlite::Result<Option<EntityId>> {
    let sql = match provider {
        ArtistIdentityProvider::LastFm => "SELECT ar.id FROM artists ar WHERE ar.lastfm_mbid=?1",
        ArtistIdentityProvider::Deezer => {
            "SELECT ar.id FROM artists ar WHERE ar.deezer_artist_id=?1"
        }
        ArtistIdentityProvider::Spotify => {
            "SELECT ar.id FROM artists ar WHERE ar.spotify_artist_id=?1"
        }
        ArtistIdentityProvider::Discogs => {
            "SELECT ar.id FROM artists ar WHERE ar.discogs_artist_id=?1"
        }
    };
    let sql =
        format!("{sql} AND EXISTS (SELECT 1 FROM albums al WHERE al.artist_id=ar.id) LIMIT 2");
    let matches: Vec<EntityId> = conn
        .prepare(&sql)?
        .query_map([external_id.trim()], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok((matches.len() == 1).then(|| matches[0].clone()))
}

/// Resolves an exact normalized local/canonical name to one release-owning
/// artist. Multiple local homonyms intentionally produce no match.
pub fn find_owned_artist_by_name(
    conn: &Connection,
    name: &str,
) -> rusqlite::Result<Option<EntityId>> {
    let matches: Vec<EntityId> = conn
        .prepare(
            "SELECT ar.id FROM artists ar
             WHERE (LOWER(TRIM(ar.name))=LOWER(TRIM(?1))
                    OR LOWER(TRIM(COALESCE(ar.lastfm_canonical_name,'')))=LOWER(TRIM(?1)))
               AND EXISTS (SELECT 1 FROM albums al WHERE al.artist_id=ar.id)
             LIMIT 2",
        )?
        .query_map([name], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok((matches.len() == 1).then(|| matches[0].clone()))
}

/// Public Artist Browse Page data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ArtistBrowsePage {
    /// Documents the Items public API surface.
    pub items: Vec<ArtistRow>,
    /// Documents the Total public API surface.
    pub total: i64,
    /// Documents the Limit public API surface.
    pub limit: i64,
    /// Documents the Offset public API surface.
    pub offset: i64,
    /// Documents the Has More public API surface.
    pub has_more: bool,
}

/// Documents the Artist List public API surface.
pub enum ArtistList {
    Page(ArtistBrowsePage),
    All(Vec<ArtistRow>),
}

/// Documents the List Artists Params public API surface.
pub struct ListArtistsParams<'a> {
    /// Documents the User Id public API surface.
    pub user_id: &'a str,
    /// Documents the Genres public API surface.
    pub genres: &'a [String],
    /// Documents the Library Ids public API surface.
    pub library_ids: &'a [EntityId],
    /// Documents the Starts With public API surface.
    pub starts_with: Option<&'a str>,
    /// Documents the Name Query public API surface.
    pub name_query: Option<&'a str>,
    /// Documents the Order Dir public API surface.
    pub order_dir: &'a str,
    /// Documents the Full View public API surface.
    pub full_view: bool,
    /// Documents the Page Limit public API surface.
    pub page_limit: Option<i64>,
    /// Documents the Page Offset public API surface.
    pub page_offset: i64,
    /// Filter to artists with at least one Sonic Fingerprint (deep analysis) track.
    pub sonic_fingerprint_only: bool,
    /// Hide artists that own no album directly (they only appear via tracks on
    /// other artists' albums, e.g. Various Artists compilations). Ignored when
    /// `name_query` is set, so a direct name search always finds these artists.
    pub hide_compilation_only: bool,
}

/// Documents the List Artists public API surface.
pub fn list_artists(conn: &Connection, p: ListArtistsParams<'_>) -> rusqlite::Result<ArtistList> {
    let mut conditions: Vec<String> = Vec::new();
    let mut filter_params: Vec<Value> = Vec::new();

    if !p.library_ids.is_empty() {
        let ph = p
            .library_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        conditions.push(format!("t.library_id IN ({ph})"));
        for id in p.library_ids {
            filter_params.push(id_to_value(id));
        }
    }
    if !p.genres.is_empty() {
        let ph = p.genres.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        conditions.push(format!("LOWER(TRIM(COALESCE(t.genre,''))) IN ({ph})"));
        for g in p.genres {
            filter_params.push(Value::Text(g.to_lowercase()));
        }
    }
    if let Some(sw) = p.starts_with {
        conditions.push("UPPER(SUBSTR(TRIM(COALESCE(ar.name,'')),1,1)) = ?".into());
        filter_params.push(Value::Text(sw.to_uppercase()));
    }
    if let Some(q) = p.name_query {
        conditions.push("LOWER(TRIM(COALESCE(ar.name,''))) LIKE ?".into());
        filter_params.push(Value::Text(format!("%{}%", q.to_lowercase())));
    }
    if p.sonic_fingerprint_only {
        conditions
            .push("EXISTS (SELECT 1 FROM track_deep_analysis da WHERE da.track_id = t.id AND da.confidence > 0.25)".into());
    }
    if p.hide_compilation_only && p.name_query.is_none() {
        conditions.push("EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = ar.id)".into());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let wants_paged = p.page_limit.is_some() || p.page_offset > 0;

    let total: Option<i64> = if wants_paged {
        let count_sql = format!(
            "SELECT COUNT(DISTINCT ar.id) FROM artists ar JOIN tracks t ON t.artist_id = ar.id {where_clause}"
        );
        Some(conn.query_row(
            &count_sql,
            params_from_iter(filter_params.iter().cloned()),
            |row| row.get(0),
        )?)
    } else {
        None
    };

    // track_count/album_count reflect the artist's owned albums (albums.artist_id),
    // not just tracks tagged with this artist's own id — for a container artist like
    // "Various Artists" those diverge sharply: it owns many compilations whose tracks
    // are individually credited to other performers, so counting via t.artist_id alone
    // would wildly undercount.
    let owned_counts_sql = "(SELECT COUNT(*) FROM tracks ot JOIN albums oal ON oal.id = ot.album_id WHERE oal.artist_id = ar.id) AS track_count, \
         (SELECT COUNT(*) FROM albums oal2 WHERE oal2.artist_id = ar.id) AS album_count";
    let meta_fields = if p.full_view {
        format!("ar.id, ar.name, MAX(arr.rating) AS rating, ar.metadata_locked, ar.identity_lock_pending, ar.description, {owned_counts_sql}")
    } else {
        format!("ar.id, ar.name, MAX(arr.rating) AS rating, NULL AS metadata_locked, NULL AS identity_lock_pending, NULL AS description, {owned_counts_sql}")
    };
    let order = p.order_dir;
    let limit_clause = if wants_paged { "LIMIT ? OFFSET ?" } else { "" };

    let sql = format!(
        "SELECT {meta_fields}
         FROM artists ar JOIN tracks t ON t.artist_id = ar.id
         LEFT JOIN artist_ratings arr ON arr.artist_id = ar.id AND arr.user_id = ?
         {where_clause}
         GROUP BY ar.id ORDER BY ar.name {order}, ar.id {order}
         {limit_clause}"
    );

    let mut full_params: Vec<Value> = vec![Value::Text(p.user_id.to_owned())];
    full_params.extend(filter_params.iter().cloned());
    if wants_paged {
        full_params.push(Value::Integer(p.page_limit.unwrap_or(100)));
        full_params.push(Value::Integer(p.page_offset));
    }

    let rows: Vec<ArtistRow> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(full_params), |row| {
            Ok(ArtistRow {
                id: row.get(0)?,
                name: row.get(1)?,
                rating: row.get(2)?,
                metadata_locked: row.get(3)?,
                identity_lock_pending: row.get(4)?,
                description: row.get(5)?,
                track_count: row.get(6)?,
                album_count: row.get(7)?,
                play_count: None,
                styles: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    if wants_paged {
        let limit = p.page_limit.unwrap_or(100);
        let total_val = total.unwrap_or(rows.len() as i64);
        Ok(ArtistList::Page(ArtistBrowsePage {
            has_more: p.page_offset + (rows.len() as i64) < total_val,
            total: total_val,
            limit,
            offset: p.page_offset,
            items: rows,
        }))
    } else {
        Ok(ArtistList::All(rows))
    }
}

/// Documents the Get Artist public API surface.
pub fn get_artist(
    conn: &Connection,
    user_id: &str,
    artist_id: &EntityId,
) -> rusqlite::Result<Option<ArtistRow>> {
    let artist = conn
        .query_row(
            "SELECT ar.id, ar.name, MAX(arr.rating) AS rating, ar.metadata_locked,
                ar.identity_lock_pending, ar.description,
                (SELECT COUNT(*) FROM tracks t JOIN albums al ON al.id = t.album_id WHERE al.artist_id = ar.id) AS track_count,
                (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id) AS album_count
         FROM artists ar
         LEFT JOIN artist_ratings arr ON arr.artist_id = ar.id AND arr.user_id = ?
         WHERE ar.id = ? GROUP BY ar.id",
            rusqlite::params![user_id, artist_id],
            |row| {
                Ok(ArtistRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    rating: row.get(2)?,
                    metadata_locked: row.get(3)?,
                    identity_lock_pending: row.get(4)?,
                    description: row.get(5)?,
                    track_count: row.get(6)?,
                    album_count: row.get(7)?,
                    play_count: None,
                    styles: Vec::new(),
                })
            },
        )
        .optional()?;

    let Some(mut artist) = artist else {
        return Ok(None);
    };
    artist.styles = list_artist_radio_tags(conn, artist_id)?;
    Ok(Some(artist))
}

/// Documents the List Artists Most Played public API surface.
pub fn list_artists_most_played(
    conn: &Connection,
    user_id: &str,
    limit: i64,
) -> rusqlite::Result<Vec<ArtistRow>> {
    conn.prepare(
        "SELECT ar.id, ar.name, MAX(arr.rating) AS rating, NULL AS metadata_locked, NULL AS description,
                (SELECT COUNT(*) FROM tracks t JOIN albums al ON al.id = t.album_id WHERE al.artist_id = ar.id) AS track_count,
                (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id) AS album_count,
                COALESCE(ar.play_count, 0) AS play_count
         FROM artists ar
         LEFT JOIN artist_ratings arr ON arr.artist_id = ar.id AND arr.user_id = ?
         WHERE COALESCE(ar.play_count, 0) > 0
         GROUP BY ar.id
         ORDER BY COALESCE(ar.play_count, 0) DESC, LOWER(ar.name) ASC
         LIMIT ?",
    )?
    .query_map(rusqlite::params![user_id, limit], |row| {
        Ok(ArtistRow {
            id: row.get(0)?,
            name: row.get(1)?,
            rating: row.get(2)?,
            metadata_locked: row.get(3)?,
            identity_lock_pending: None,
            description: row.get(4)?,
            track_count: row.get(5)?,
            album_count: row.get(6)?,
            play_count: Some(row.get(7)?),
            styles: Vec::new(),
        })
    })?
    .collect()
}

// ── Artist Merge / Unmerge ───────────────────────────────────────────────────
// Consolidates duplicate artist rows (see
// wip/artist-consolidation-implementation-plan.md). A merge reassigns every
// selected artist's albums/tracks onto one chosen "master" identity, records
// exactly what moved so an unmerge can reverse it precisely, and registers
// each absorbed name in `artist_name_aliases` so a rescan whose tag text
// still says a pre-merge name resolves back to the master instead of
// recreating the old duplicate (see `jobs::upsert_artist`).

/// Errors from merging or splitting artist rows apart.
#[derive(Debug, thiserror::Error)]
pub enum ArtistMergeError {
    #[error("At least two artists are required to merge")]
    TooFewArtists,
    #[error("A name is required")]
    EmptyName,
    #[error("Artist not found")]
    ArtistNotFound,
    #[error("Various Artists cannot be merged")]
    VariousArtistsNotMergeable,
    #[error("Artist is already a merged identity — unmerge it first")]
    AlreadyMerged,
    #[error("The selected master must be one of the merged artists")]
    InvalidMaster,
    #[error("Artist is not a merged identity")]
    NotAMergeMaster,
    #[error("No matching merge members were selected")]
    NoMembersSelected,
    #[error("Artist identity is not pending an online match")]
    IdentityNotPending,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// One name absorbed into a merge master, as shown on the artist detail
/// page's "Merged from" panel and the unmerge modal's member checklist.
#[derive(Debug, Serialize)]
pub struct ArtistMergeMember {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Original Name public API surface.
    pub original_name: String,
    /// Documents the Album Count public API surface.
    pub album_count: i64,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
}

/// Whether — and from what — an artist is currently a merge master.
#[derive(Debug, Serialize)]
pub struct ArtistMergeInfo {
    /// Documents the Merged public API surface.
    pub merged: bool,
    /// Documents the Members public API surface.
    pub members: Vec<ArtistMergeMember>,
}

/// Result of splitting one or more merge members back out.
#[derive(Debug, Serialize)]
pub struct UnmergeResult {
    /// The master artist after unmerging, or `None` if it was a synthetic
    /// (custom-name) master that got fully dissolved.
    pub master: Option<ArtistRow>,
    /// Documents the New Artist Ids public API surface.
    pub new_artist_ids: Vec<EntityId>,
}

fn new_merge_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn is_various_artists(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("various artists")
}

/// True if `artist_id` is currently the master of an active merge.
fn is_merge_master(conn: &Connection, artist_id: &EntityId) -> rusqlite::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM artist_merges WHERE master_artist_id = ?1 LIMIT 1",
            [artist_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Snapshot of one artist's provider identity fields, used by
/// `adopt_external_identity` — a plain struct (rather than reusing
/// `ArtistExternalIdentity` directly) so the merge logic below stays
/// independent of that struct's `artist_id`/`name` fields.
#[derive(Debug, Clone, Default)]
struct IdentitySnapshot {
    lastfm_mbid: Option<String>,
    lastfm_canonical_name: Option<String>,
    lastfm_identity_checked_at: Option<String>,
    deezer_artist_id: Option<String>,
    deezer_identity_checked_at: Option<String>,
    spotify_artist_id: Option<String>,
    spotify_identity_checked_at: Option<String>,
    discogs_artist_id: Option<String>,
    discogs_identity_checked_at: Option<String>,
}

impl From<ArtistExternalIdentity> for IdentitySnapshot {
    fn from(v: ArtistExternalIdentity) -> Self {
        IdentitySnapshot {
            lastfm_mbid: v.lastfm_mbid,
            lastfm_canonical_name: v.lastfm_canonical_name,
            lastfm_identity_checked_at: v.lastfm_identity_checked_at,
            deezer_artist_id: v.deezer_artist_id,
            deezer_identity_checked_at: v.deezer_identity_checked_at,
            spotify_artist_id: v.spotify_artist_id,
            spotify_identity_checked_at: v.spotify_identity_checked_at,
            discogs_artist_id: v.discogs_artist_id,
            discogs_identity_checked_at: v.discogs_identity_checked_at,
        }
    }
}

/// Picks, per provider, the first non-null identity across `snapshots`
/// (master's own pre-merge identity first at index 0, then sources in
/// selection order). A value adopted from anything but index 0 has its
/// paired `*_identity_checked_at` cleared instead of carried over, so a
/// background refresh re-verifies it rather than trusting a stale
/// timestamp inherited from a differently-named source artist.
fn adopt_external_identity(snapshots: &[IdentitySnapshot]) -> IdentitySnapshot {
    fn winner(values: &[Option<&str>]) -> Option<usize> {
        values.iter().position(|v| v.is_some())
    }

    let lastfm_vals: Vec<Option<&str>> =
        snapshots.iter().map(|s| s.lastfm_mbid.as_deref()).collect();
    let (lastfm_mbid, lastfm_canonical_name, lastfm_identity_checked_at) =
        match winner(&lastfm_vals) {
            Some(0) => (
                snapshots[0].lastfm_mbid.clone(),
                snapshots[0].lastfm_canonical_name.clone(),
                snapshots[0].lastfm_identity_checked_at.clone(),
            ),
            Some(i) => (
                snapshots[i].lastfm_mbid.clone(),
                snapshots[i].lastfm_canonical_name.clone(),
                None,
            ),
            None => (None, None, None),
        };

    let deezer_vals: Vec<Option<&str>> = snapshots
        .iter()
        .map(|s| s.deezer_artist_id.as_deref())
        .collect();
    let (deezer_artist_id, deezer_identity_checked_at) = match winner(&deezer_vals) {
        Some(0) => (
            snapshots[0].deezer_artist_id.clone(),
            snapshots[0].deezer_identity_checked_at.clone(),
        ),
        Some(i) => (snapshots[i].deezer_artist_id.clone(), None),
        None => (None, None),
    };

    let spotify_vals: Vec<Option<&str>> = snapshots
        .iter()
        .map(|s| s.spotify_artist_id.as_deref())
        .collect();
    let (spotify_artist_id, spotify_identity_checked_at) = match winner(&spotify_vals) {
        Some(0) => (
            snapshots[0].spotify_artist_id.clone(),
            snapshots[0].spotify_identity_checked_at.clone(),
        ),
        Some(i) => (snapshots[i].spotify_artist_id.clone(), None),
        None => (None, None),
    };

    let discogs_vals: Vec<Option<&str>> = snapshots
        .iter()
        .map(|s| s.discogs_artist_id.as_deref())
        .collect();
    let (discogs_artist_id, discogs_identity_checked_at) = match winner(&discogs_vals) {
        Some(0) => (
            snapshots[0].discogs_artist_id.clone(),
            snapshots[0].discogs_identity_checked_at.clone(),
        ),
        Some(i) => (snapshots[i].discogs_artist_id.clone(), None),
        None => (None, None),
    };

    IdentitySnapshot {
        lastfm_mbid,
        lastfm_canonical_name,
        lastfm_identity_checked_at,
        deezer_artist_id,
        deezer_identity_checked_at,
        spotify_artist_id,
        spotify_identity_checked_at,
        discogs_artist_id,
        discogs_identity_checked_at,
    }
}

/// Picks a library to scope a merge master's one-shot post-merge enrichment
/// jobs to (`post_scan_jobs` requires one) — the library any of its tracks
/// already belong to, or else any configured library as a last resort.
fn library_id_for_enrichment_scope(
    conn: &Connection,
    master_id: &EntityId,
) -> rusqlite::Result<Option<EntityId>> {
    if let Some(lib) = conn
        .query_row(
            "SELECT t.library_id FROM tracks t
             JOIN albums al ON al.id = t.album_id
             WHERE al.artist_id = ?1 LIMIT 1",
            [master_id],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(Some(lib));
    }
    conn.query_row("SELECT id FROM libraries LIMIT 1", [], |row| row.get(0))
        .optional()
}

/// Merges 2+ artist rows into one. `master_artist_id`, when present, must be
/// one of `artist_ids` and keeps that artist's row (renamed to
/// `master_name`); when absent, a brand-new row is created for
/// `master_name` and every one of `artist_ids` becomes a merge member.
/// Reassigns every merged artist's albums and direct track credits, unions
/// their radio-tag styles, carries over per-user ratings (keeping the
/// higher rating on a conflict), adopts the first available external
/// identity in selection order (§ Post-merge online identity matching:
/// when none is found, the result stays unlocked and pending a one-shot
/// online match instead of locking immediately), and registers each
/// absorbed name as an alias so a rescan can't recreate it. Records enough
/// in `artist_merge_members`/`artist_merge_moves` for `unmerge_artists` to
/// reverse this exactly.
pub fn merge_artists(
    conn: &Connection,
    artist_ids: &[EntityId],
    master_name: &str,
    master_artist_id: Option<&EntityId>,
    acting_user_id: &str,
) -> Result<ArtistRow, ArtistMergeError> {
    let master_name = master_name.trim();
    if master_name.is_empty() {
        return Err(ArtistMergeError::EmptyName);
    }
    let mut ids: Vec<EntityId> = Vec::new();
    for candidate_id in artist_ids {
        if !ids.contains(candidate_id) {
            ids.push(candidate_id.clone());
        }
    }
    if ids.len() < 2 {
        return Err(ArtistMergeError::TooFewArtists);
    }
    if let Some(master_id) = master_artist_id {
        if !ids.contains(master_id) {
            return Err(ArtistMergeError::InvalidMaster);
        }
    }

    // Validate every selected artist up front, before mutating anything.
    for candidate_id in &ids {
        let name: Option<String> = conn
            .query_row(
                "SELECT name FROM artists WHERE id = ?1",
                [candidate_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(name) = name else {
            return Err(ArtistMergeError::ArtistNotFound);
        };
        if is_various_artists(&name) {
            return Err(ArtistMergeError::VariousArtistsNotMergeable);
        }
        if is_merge_master(conn, candidate_id)? {
            return Err(ArtistMergeError::AlreadyMerged);
        }
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = merge_artists_tx(conn, &ids, master_name, master_artist_id, acting_user_id);
    match result {
        Ok(row) => {
            conn.execute_batch("COMMIT")?;
            Ok(row)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn merge_artists_tx(
    conn: &Connection,
    ids: &[EntityId],
    master_name: &str,
    master_artist_id: Option<&EntityId>,
    acting_user_id: &str,
) -> Result<ArtistRow, ArtistMergeError> {
    let master_is_new = master_artist_id.is_none();
    let master_id: EntityId = match master_artist_id {
        Some(existing_id) => existing_id.clone(),
        None => {
            let new_id = EntityId::Str(new_merge_id());
            conn.execute(
                "INSERT INTO artists(id, name) VALUES(?1, ?2)",
                rusqlite::params![new_id, master_name],
            )?;
            new_id
        }
    };

    let mut identity_snapshots: Vec<IdentitySnapshot> =
        vec![get_artist_external_identity(conn, &master_id)?
            .map(IdentitySnapshot::from)
            .unwrap_or_default()];

    let merge_id = new_merge_id();
    // Inserted now, before any `artist_merge_members` row, since those
    // reference `artist_merges.id` via a foreign key.
    conn.execute(
        "INSERT INTO artist_merges(id, master_artist_id, merged_by_user_id, merged_at, master_is_new)
         VALUES(?1, ?2, ?3, datetime('now'), ?4)",
        rusqlite::params![merge_id, master_id, acting_user_id, master_is_new as i64],
    )?;

    let sources: Vec<EntityId> = ids
        .iter()
        .filter(|candidate_id| **candidate_id != master_id)
        .cloned()
        .collect();

    for source_id in &sources {
        let source_name: String = conn.query_row(
            "SELECT name FROM artists WHERE id = ?1",
            [source_id],
            |row| row.get(0),
        )?;

        if let Some(identity) = get_artist_external_identity(conn, source_id)? {
            identity_snapshots.push(IdentitySnapshot::from(identity));
        }

        let album_ids: Vec<EntityId> = conn
            .prepare("SELECT id FROM albums WHERE artist_id = ?1")?
            .query_map([source_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let track_ids: Vec<EntityId> = conn
            .prepare("SELECT id FROM tracks WHERE artist_id = ?1")?
            .query_map([source_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        conn.execute(
            "UPDATE albums SET artist_id = ?1 WHERE artist_id = ?2",
            rusqlite::params![master_id, source_id],
        )?;
        conn.execute(
            "UPDATE tracks SET artist_id = ?1 WHERE artist_id = ?2",
            rusqlite::params![master_id, source_id],
        )?;

        conn.execute(
            "INSERT OR IGNORE INTO artist_styles(artist_id, style)
             SELECT ?1, style FROM artist_styles WHERE artist_id = ?2",
            rusqlite::params![master_id, source_id],
        )?;
        conn.execute(
            "DELETE FROM artist_styles WHERE artist_id = ?1",
            [source_id],
        )?;

        conn.execute(
            "INSERT INTO artist_ratings(user_id, artist_id, rating, updated_at)
             SELECT user_id, ?1, rating, datetime('now') FROM artist_ratings WHERE artist_id = ?2
             ON CONFLICT(user_id, artist_id) DO UPDATE SET
               rating = MAX(artist_ratings.rating, excluded.rating),
               updated_at = datetime('now')",
            rusqlite::params![master_id, source_id],
        )?;

        let alias_key = source_name.trim().to_lowercase();
        if alias_key != master_name.to_lowercase() {
            conn.execute(
                "INSERT OR REPLACE INTO artist_name_aliases(alias_name_normalized, artist_id)
                 VALUES(?1, ?2)",
                rusqlite::params![alias_key, master_id],
            )?;
        }

        let member_id = new_merge_id();
        conn.execute(
            "INSERT INTO artist_merge_members(
                id, merge_id, original_name, original_artist_id,
                album_count_at_merge, track_count_at_merge
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                member_id,
                merge_id,
                source_name,
                source_id.to_string(),
                album_ids.len() as i64,
                track_ids.len() as i64,
            ],
        )?;
        for album_id in &album_ids {
            conn.execute(
                "INSERT OR IGNORE INTO artist_merge_moves(merge_member_id, entity_type, entity_id)
                 VALUES(?1, 'album', ?2)",
                rusqlite::params![member_id, album_id.to_string()],
            )?;
        }
        for track_id in &track_ids {
            conn.execute(
                "INSERT OR IGNORE INTO artist_merge_moves(merge_member_id, entity_type, entity_id)
                 VALUES(?1, 'track', ?2)",
                rusqlite::params![member_id, track_id.to_string()],
            )?;
        }

        conn.execute("DELETE FROM artists WHERE id = ?1", [source_id])?;
    }

    let adopted = adopt_external_identity(&identity_snapshots);
    let has_identity = adopted.lastfm_mbid.is_some()
        || adopted.deezer_artist_id.is_some()
        || adopted.spotify_artist_id.is_some()
        || adopted.discogs_artist_id.is_some();

    conn.execute(
        "UPDATE artists SET
            name = ?1,
            metadata_locked = ?2,
            identity_lock_pending = ?3,
            lastfm_mbid = ?4,
            lastfm_canonical_name = ?5,
            lastfm_identity_checked_at = ?6,
            deezer_artist_id = ?7,
            deezer_identity_checked_at = ?8,
            spotify_artist_id = ?9,
            spotify_identity_checked_at = ?10,
            discogs_artist_id = ?11,
            discogs_identity_checked_at = ?12
         WHERE id = ?13",
        rusqlite::params![
            master_name,
            has_identity as i64,
            (!has_identity) as i64,
            adopted.lastfm_mbid,
            adopted.lastfm_canonical_name,
            adopted.lastfm_identity_checked_at,
            adopted.deezer_artist_id,
            adopted.deezer_identity_checked_at,
            adopted.spotify_artist_id,
            adopted.spotify_identity_checked_at,
            adopted.discogs_artist_id,
            adopted.discogs_identity_checked_at,
            master_id,
        ],
    )?;

    if !has_identity {
        if let Some(lib_id) = library_id_for_enrichment_scope(conn, &master_id)? {
            let payload = serde_json::json!({ "artistIds": [master_id.to_string()] }).to_string();
            let _ = crate::jobs::enqueue_post_scan_job(
                conn,
                &lib_id.to_string(),
                "enrich_artist_external_ids",
                Some(payload.clone()),
            );
            let _ = crate::jobs::enqueue_post_scan_job(
                conn,
                &lib_id.to_string(),
                "cache_artist_images",
                Some(payload),
            );
        } else {
            // Nothing to scope an online lookup to (no libraries configured
            // at all) — lock immediately rather than wait forever.
            conn.execute(
                "UPDATE artists SET metadata_locked = 1, identity_lock_pending = 0 WHERE id = ?1",
                [&master_id],
            )?;
        }
    }

    get_artist(conn, acting_user_id, &master_id)?.ok_or(ArtistMergeError::ArtistNotFound)
}

/// Returns whether `artist_id` is currently a merge master and, if so, the
/// names absorbed into it — powers the "Merged from" panel and the unmerge
/// modal's member checklist.
pub fn get_artist_merge_info(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<ArtistMergeInfo> {
    let merge_id: Option<String> = conn
        .query_row(
            "SELECT id FROM artist_merges WHERE master_artist_id = ?1",
            [artist_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(merge_id) = merge_id else {
        return Ok(ArtistMergeInfo {
            merged: false,
            members: Vec::new(),
        });
    };
    let members = conn
        .prepare(
            "SELECT id, original_name, album_count_at_merge, track_count_at_merge
             FROM artist_merge_members WHERE merge_id = ?1 ORDER BY original_name",
        )?
        .query_map([&merge_id], |row| {
            Ok(ArtistMergeMember {
                id: row.get(0)?,
                original_name: row.get(1)?,
                album_count: row.get(2)?,
                track_count: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ArtistMergeInfo {
        merged: true,
        members,
    })
}

/// Splits the given merge members back out into their own artist rows,
/// reversing exactly what `merge_artists` moved (via `artist_merge_moves`).
/// If every member of the merge is split out and the master was a
/// brand-new row minted for a custom name (`master_is_new`), the now-empty
/// master row is deleted too; otherwise it's unlocked and kept. Returns the
/// ids of the newly created artists plus the (possibly now-unlocked, or
/// deleted → `None`) master.
pub fn unmerge_artists(
    conn: &Connection,
    master_artist_id: &EntityId,
    member_ids: &[EntityId],
    acting_user_id: &str,
) -> Result<UnmergeResult, ArtistMergeError> {
    if member_ids.is_empty() {
        return Err(ArtistMergeError::NoMembersSelected);
    }
    let merge_row: Option<(String, i64)> = conn
        .query_row(
            "SELECT id, master_is_new FROM artist_merges WHERE master_artist_id = ?1",
            [master_artist_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((merge_id, master_is_new)) = merge_row else {
        return Err(ArtistMergeError::NotAMergeMaster);
    };

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = unmerge_artists_tx(
        conn,
        master_artist_id,
        &merge_id,
        master_is_new != 0,
        member_ids,
    );
    let new_ids = match result {
        Ok(ids) => {
            conn.execute_batch("COMMIT")?;
            ids
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };

    let master = get_artist(conn, acting_user_id, master_artist_id)?;
    Ok(UnmergeResult {
        master,
        new_artist_ids: new_ids,
    })
}

fn unmerge_artists_tx(
    conn: &Connection,
    master_artist_id: &EntityId,
    merge_id: &str,
    master_is_new: bool,
    member_ids: &[EntityId],
) -> Result<Vec<EntityId>, ArtistMergeError> {
    let mut new_ids = Vec::new();

    for member_id in member_ids {
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT original_name, merge_id FROM artist_merge_members WHERE id = ?1",
                [member_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((original_name, row_merge_id)) = row else {
            continue; // already split out or unknown id
        };
        if row_merge_id != merge_id {
            continue; // belongs to a different merge — not this master's to split
        }

        let new_id = EntityId::Str(new_merge_id());
        conn.execute(
            "INSERT INTO artists(id, name) VALUES(?1, ?2)",
            rusqlite::params![new_id, original_name],
        )?;

        let moves: Vec<(String, String)> = conn
            .prepare(
                "SELECT entity_type, entity_id FROM artist_merge_moves WHERE merge_member_id = ?1",
            )?
            .query_map([member_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (entity_type, entity_id) in moves {
            match entity_type.as_str() {
                "album" => {
                    conn.execute(
                        "UPDATE albums SET artist_id = ?1 WHERE id = ?2",
                        rusqlite::params![new_id, entity_id],
                    )?;
                }
                "track" => {
                    conn.execute(
                        "UPDATE tracks SET artist_id = ?1 WHERE id = ?2",
                        rusqlite::params![new_id, entity_id],
                    )?;
                }
                _ => {}
            }
        }

        conn.execute(
            "DELETE FROM artist_name_aliases WHERE alias_name_normalized = LOWER(TRIM(?1))",
            [&original_name],
        )?;
        conn.execute(
            "DELETE FROM artist_merge_moves WHERE merge_member_id = ?1",
            [member_id],
        )?;
        conn.execute(
            "DELETE FROM artist_merge_members WHERE id = ?1",
            [member_id],
        )?;

        new_ids.push(new_id);
    }

    if new_ids.is_empty() {
        return Err(ArtistMergeError::NoMembersSelected);
    }

    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artist_merge_members WHERE merge_id = ?1",
        [merge_id],
        |row| row.get(0),
    )?;

    if remaining == 0 {
        conn.execute("DELETE FROM artist_merges WHERE id = ?1", [merge_id])?;
        if master_is_new {
            conn.execute(
                "DELETE FROM artist_styles WHERE artist_id = ?1",
                [master_artist_id],
            )?;
            conn.execute("DELETE FROM artists WHERE id = ?1", [master_artist_id])?;
        } else {
            conn.execute(
                "UPDATE artists SET metadata_locked = 0, identity_lock_pending = 0 WHERE id = ?1",
                [master_artist_id],
            )?;
        }
    }

    Ok(new_ids)
}

/// Called after `post_scan::run_enrich_artist_external_ids` attempts a
/// provider lookup for one artist: if that artist was waiting on a
/// post-merge identity match (`identity_lock_pending`) and now has at least
/// one external identity, locks it (same protection any other artist with
/// `metadata_locked` already gets). A miss leaves `identity_lock_pending`
/// set so the next periodic backfill sweep tries again (§6.5, §8 decision
/// #5) — this only ever locks on a hit, never on a miss.
pub fn finalize_pending_identity_lock(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<bool> {
    let updated = conn.execute(
        "UPDATE artists SET metadata_locked = 1, identity_lock_pending = 0
         WHERE id = ?1
           AND identity_lock_pending = 1
           AND (lastfm_mbid IS NOT NULL OR deezer_artist_id IS NOT NULL
                OR spotify_artist_id IS NOT NULL OR discogs_artist_id IS NOT NULL)",
        [artist_id],
    )?;
    Ok(updated > 0)
}

/// Manually locks a merge master that's still waiting on its post-merge
/// online identity match (§6.5's "Lock now" override) — for an artist that
/// will never get a match (a local/unreleased act, say).
pub fn lock_artist_identity(
    conn: &Connection,
    artist_id: &EntityId,
    acting_user_id: &str,
) -> Result<ArtistRow, ArtistMergeError> {
    let pending: Option<i64> = conn
        .query_row(
            "SELECT identity_lock_pending FROM artists WHERE id = ?1",
            [artist_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(pending) = pending else {
        return Err(ArtistMergeError::ArtistNotFound);
    };
    if pending == 0 {
        return Err(ArtistMergeError::IdentityNotPending);
    }
    conn.execute(
        "UPDATE artists SET metadata_locked = 1, identity_lock_pending = 0 WHERE id = ?1",
        [artist_id],
    )?;
    get_artist(conn, acting_user_id, artist_id)?.ok_or(ArtistMergeError::ArtistNotFound)
}

// ── Album ─────────────────────────────────────────────────────────────────────

/// Public Album Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct AlbumRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Title public API surface.
    pub title: String,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Album Artist public API surface.
    pub album_artist: Option<String>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// Documents the Rating public API surface.
    pub rating: Option<f64>,
    #[serde(rename = "releaseType")]
    pub release_type: Option<String>,
    /// Documents the Track Count public API surface.
    pub track_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_locked: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_scanned_at: Option<String>,
}

/// Documents the List Albums Params public API surface.
pub struct ListAlbumsParams<'a> {
    /// Documents the User Id public API surface.
    pub user_id: &'a str,
    /// Documents the Library Ids public API surface.
    pub library_ids: &'a [EntityId],
    /// Documents the Genres public API surface.
    pub genres: &'a [String],
    /// Documents the By Album Artist public API surface.
    pub by_album_artist: bool,
    /// Filter to albums with at least one Sonic Fingerprint (deep analysis) track.
    pub sonic_fingerprint_only: bool,
    /// Return only album rows created after this SQLite row id.
    pub after_album_rowid: Option<i64>,
    /// Bound incremental results to this SQLite row id so cursor advancement cannot skip rows.
    pub through_album_rowid: Option<i64>,
}

/// Returns the current album insertion cursor used by incremental Browse refreshes.
pub fn album_change_cursor(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COALESCE(MAX(rowid), 0) FROM albums", [], |row| {
        row.get(0)
    })
}

/// Documents the List Albums public API surface.
pub fn list_albums(conn: &Connection, p: ListAlbumsParams<'_>) -> rusqlite::Result<Vec<AlbumRow>> {
    let mut conditions: Vec<String> = Vec::new();
    let mut filter_params: Vec<Value> = vec![Value::Text(p.user_id.to_owned())];

    if !p.library_ids.is_empty() {
        let ph = p
            .library_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        conditions.push(format!("t.library_id IN ({ph})"));
        for id in p.library_ids {
            filter_params.push(id_to_value(id));
        }
    }
    if !p.genres.is_empty() {
        let ph = p.genres.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        conditions.push(format!("LOWER(TRIM(COALESCE(t.genre,''))) IN ({ph})"));
        for g in p.genres {
            filter_params.push(Value::Text(g.to_lowercase()));
        }
    }
    if p.sonic_fingerprint_only {
        conditions
            .push("EXISTS (SELECT 1 FROM track_deep_analysis da WHERE da.track_id = t.id AND da.confidence > 0.25)".into());
    }
    if let Some(after) = p.after_album_rowid {
        if p.by_album_artist {
            let upper_bound = if p.through_album_rowid.is_some() {
                " AND changed_al.rowid <= ?"
            } else {
                ""
            };
            conditions.push(format!(
                "EXISTS (
                   SELECT 1 FROM albums changed_al
                   WHERE changed_al.rowid > ?{upper_bound}
                     AND changed_al.title = al.title
                     AND COALESCE(changed_al.album_artist,'') = COALESCE(al.album_artist,'')
                 )"
            ));
        } else {
            conditions.push(if p.through_album_rowid.is_some() {
                "al.rowid > ? AND al.rowid <= ?".into()
            } else {
                "al.rowid > ?".into()
            });
        }
        filter_params.push(Value::Integer(after));
        if let Some(through) = p.through_album_rowid {
            filter_params.push(Value::Integer(through));
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = if p.by_album_artist {
        format!(
            "SELECT MIN(al.id), al.title, al.album_artist, al.album_artist AS artist,
                    MIN(al.year), al.genre, MAX(alr.rating), MIN(al.release_type),
                    COUNT(t.id), ROUND(SUM(t.duration),0), MAX(al.metadata_locked), MIN(al.description),
                    MIN(al.label)
             FROM albums al JOIN tracks t ON t.album_id = al.id
             LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
             {where_clause}
             GROUP BY al.title, COALESCE(al.album_artist,'')
             ORDER BY LOWER(COALESCE(al.album_artist,'')), MIN(al.year), al.title"
        )
    } else {
        format!(
            "SELECT al.id, al.title, al.album_artist, ar.name AS artist,
                    al.year, al.genre, alr.rating, al.release_type,
                    COUNT(t.id), ROUND(SUM(t.duration),0), al.metadata_locked, al.description,
                    al.label
             FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id
             JOIN tracks t ON t.album_id = al.id
             LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
             {where_clause}
             GROUP BY al.id
             ORDER BY LOWER(COALESCE(ar.name,'')), al.year, al.title"
        )
    };

    conn.prepare(&sql)?
        .query_map(params_from_iter(filter_params), |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: row.get(10)?,
                description: row.get(11)?,
                label: row.get(12)?,
                added_at: None,
                latest_scanned_at: None,
            })
        })?
        .collect()
}

/// Documents the List Albums Latest public API surface.
///
/// Runs as three bounded stages instead of aggregating the entire `tracks`/`albums`
/// set before applying `LIMIT`:
/// 1. Find the latest `limit` grouped `(title, album_artist)` identities from a
///    lightweight group-by (no `tracks` join at all when `added_at` is present).
/// 2. Resolve every album row id belonging to those groups (duplicate album rows
///    sharing the same title/album_artist collapse into one group, same as before).
/// 3. Aggregate `tracks` only for that bounded id set to produce the final rows,
///    preserving the original representative id, rating, counts, duration, label,
///    `added_at`/`latest_scanned_at`, and tie-break ordering.
pub fn list_albums_latest(
    conn: &Connection,
    user_id: &str,
    limit: i64,
) -> rusqlite::Result<Vec<AlbumRow>> {
    let has_added_at = column_exists_local(conn, "albums", "added_at")?;
    let sort_col = if has_added_at {
        "MIN(al.added_at)"
    } else {
        "MIN(t.scanned_at)"
    };
    let added_at_select = sort_col;

    if limit <= 0 {
        return Ok(Vec::new());
    }

    // Stage 1: bounded group identities, latest first.
    let group_sql = if has_added_at {
        "SELECT al.title, COALESCE(al.album_artist,''), MIN(al.added_at) AS sort_val
         FROM albums al
         GROUP BY al.title, COALESCE(al.album_artist,'')
         ORDER BY sort_val DESC, LOWER(COALESCE(al.album_artist,'')), al.title
         LIMIT ?"
    } else {
        "SELECT al.title, COALESCE(al.album_artist,''), MIN(t.scanned_at) AS sort_val
         FROM albums al JOIN tracks t ON t.album_id = al.id
         GROUP BY al.title, COALESCE(al.album_artist,'')
         ORDER BY sort_val DESC, LOWER(COALESCE(al.album_artist,'')), al.title
         LIMIT ?"
    };
    let groups: Vec<(String, String)> = conn
        .prepare(group_sql)?
        .query_map(rusqlite::params![limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    if groups.is_empty() {
        return Ok(Vec::new());
    }

    // Stage 2: resolve every album row id belonging to those bounded groups.
    let group_placeholders = groups
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let ids_sql = format!(
        "SELECT al.id FROM albums al
         WHERE (al.title, COALESCE(al.album_artist,'')) IN (VALUES {group_placeholders})"
    );
    let mut group_params: Vec<Value> = Vec::with_capacity(groups.len() * 2);
    for (title, album_artist) in &groups {
        group_params.push(Value::Text(title.clone()));
        group_params.push(Value::Text(album_artist.clone()));
    }
    let album_ids: Vec<String> = conn
        .prepare(&ids_sql)?
        .query_map(params_from_iter(group_params), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<_>>()?;

    if album_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Stage 3: aggregate tracks only for the resolved, bounded id set.
    let id_placeholders = album_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT MIN(al.id), al.title, al.album_artist, al.album_artist AS artist,
                MIN(al.year), al.genre, MAX(alr.rating), MIN(al.release_type),
                COUNT(t.id), ROUND(SUM(t.duration),0), {added_at_select} AS added_at, MAX(t.scanned_at) AS latest_scanned_at,
                MIN(al.label)
         FROM albums al JOIN tracks t ON t.album_id = al.id
         LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
         WHERE al.id IN ({id_placeholders})
         GROUP BY al.title, COALESCE(al.album_artist,'')
         ORDER BY {sort_col} DESC, LOWER(COALESCE(al.album_artist,'')), al.title"
    );
    let mut final_params: Vec<Value> = Vec::with_capacity(1 + album_ids.len());
    final_params.push(Value::Text(user_id.to_owned()));
    final_params.extend(album_ids.into_iter().map(Value::Text));

    conn.prepare(&sql)?
        .query_map(params_from_iter(final_params), |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: None,
                description: None,
                added_at: row.get(10)?,
                latest_scanned_at: row.get(11)?,
                label: row.get(12)?,
            })
        })?
        .collect()
}

/// Documents the Get Album public API surface.
pub fn get_album(
    conn: &Connection,
    user_id: &str,
    album_id: &EntityId,
) -> rusqlite::Result<Option<AlbumRow>> {
    conn.query_row(
        "SELECT al.id, al.title, al.album_artist, ar.name AS artist,
                al.year, al.genre, alr.rating, al.release_type,
                COUNT(t.id) AS track_count, NULL, al.metadata_locked, al.description,
                al.label
         FROM albums al
         LEFT JOIN artists ar ON ar.id = al.artist_id
         LEFT JOIN tracks t ON t.album_id = al.id
         LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
         WHERE al.id = ? GROUP BY al.id",
        rusqlite::params![user_id, album_id],
        |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: row.get(10)?,
                description: row.get(11)?,
                label: row.get(12)?,
                added_at: None,
                latest_scanned_at: None,
            })
        },
    )
    .optional()
}

/// Documents the List Artist Albums public API surface.
pub fn list_artist_albums(
    conn: &Connection,
    user_id: &str,
    artist_id: &EntityId,
    library_ids: &[EntityId],
) -> rusqlite::Result<Vec<AlbumRow>> {
    let lib_where = if library_ids.is_empty() {
        String::new()
    } else {
        let ph = library_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        format!("AND t.library_id IN ({ph})")
    };

    let sql = format!(
        "SELECT al.id, al.title, al.album_artist, ar.name AS artist,
                al.year, al.genre, alr.rating, al.release_type,
                COUNT(t.id), ROUND(SUM(t.duration),0), al.metadata_locked, al.description,
                al.label
         FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id
         JOIN tracks t ON t.album_id = al.id
         LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
         WHERE al.artist_id = ? {lib_where}
         GROUP BY al.id
         ORDER BY al.year ASC, al.title ASC"
    );

    let mut params: Vec<Value> = vec![Value::Text(user_id.to_owned()), id_to_value(artist_id)];
    for id in library_ids {
        params.push(id_to_value(id));
    }

    conn.prepare(&sql)?
        .query_map(params_from_iter(params), |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: row.get(10)?,
                description: row.get(11)?,
                label: row.get(12)?,
                added_at: None,
                latest_scanned_at: None,
            })
        })?
        .collect()
}

/// Documents the List Artist Appears On public API surface.
pub fn list_artist_appears_on(
    conn: &Connection,
    user_id: &str,
    artist_id: &EntityId,
    artist_name: &str,
    library_ids: &[EntityId],
) -> rusqlite::Result<Vec<AlbumRow>> {
    let lib_where = if library_ids.is_empty() {
        String::new()
    } else {
        let ph = library_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        format!("AND t.library_id IN ({ph})")
    };

    let sql = format!(
        "SELECT al.id, al.title, al.album_artist, al.album_artist AS artist,
                al.year, al.genre, alr.rating, al.release_type,
                COUNT(DISTINCT t.id), ROUND(SUM(t.duration),0), NULL, NULL, al.label
         FROM albums al JOIN tracks t ON t.album_id = al.id
         LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
         WHERE t.artist_id = ?
           AND al.album_artist IS NOT NULL AND al.album_artist != ''
           AND LOWER(al.album_artist) != LOWER(?)
           {lib_where}
         GROUP BY al.id
         ORDER BY al.year ASC, al.title ASC"
    );

    let mut params: Vec<Value> = vec![
        Value::Text(user_id.to_owned()),
        id_to_value(artist_id),
        Value::Text(artist_name.to_owned()),
    ];
    for id in library_ids {
        params.push(id_to_value(id));
    }

    conn.prepare(&sql)?
        .query_map(params_from_iter(params), |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: row.get(10)?,
                description: row.get(11)?,
                label: row.get(12)?,
                added_at: None,
                latest_scanned_at: None,
            })
        })?
        .collect()
}

// ── Track ─────────────────────────────────────────────────────────────────────

/// Public Track Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct TrackRow {
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
    /// Documents the Last Played At public API surface.
    pub last_played_at: Option<String>,
    /// Documents the Play Count public API surface.
    pub play_count: Option<i64>,
    /// Documents the Album Id public API surface.
    pub album_id: Option<EntityId>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Album public API surface.
    pub album: Option<String>,
    /// Documents the Library Name public API surface.
    pub library_name: Option<String>,
    /// Documents the Rating public API surface.
    pub rating: Option<f64>,
    /// True when real Demucs stem analysis (confidence > 0.25) exists for this track.
    pub has_deep_analysis: bool,
    /// On-disk path of the source file. Only populated by `get_track` (single-track
    /// detail fetch) — left `None` on every list/search query to keep those payloads
    /// small, since nothing besides the track detail popup needs it.
    pub file_path: Option<String>,
}

fn map_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
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
        last_played_at: row.get(20)?,
        play_count: row.get(21)?,
        album_id: row.get(22)?,
        artist: row.get(23)?,
        album: row.get(24)?,
        library_name: row.get(25)?,
        has_deep_analysis: row.get(26)?,
        rating: row.get(27)?,
        file_path: None,
    })
}

const TRACK_COLS: &str = "t.id, t.file_name, t.file_size, t.format,
     t.duration, t.bitrate, t.sample_rate, t.channels,
     t.title, t.track_number, t.disc_number, t.year, t.genre,
     t.composer, t.comment, t.bpm, t.bpm_detected, t.bpm_source, t.bpm_confidence, t.scanned_at,
     t.last_played_at, t.play_count, t.album_id,
     ar.name AS artist, al.title AS album, l.name AS library_name,
     (EXISTS (SELECT 1 FROM track_deep_analysis da WHERE da.track_id=t.id AND da.confidence>0.25))\
     AS has_deep_analysis";

/// Documents the List Album Tracks public API surface.
pub fn list_album_tracks(
    conn: &Connection,
    user_id: &str,
    album_id: &EntityId,
) -> rusqlite::Result<Vec<TrackRow>> {
    let sql = format!(
        "SELECT {TRACK_COLS}, trr.rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE t.album_id = ?
         ORDER BY COALESCE(t.disc_number,1) ASC, COALESCE(t.track_number,9999) ASC, t.title ASC"
    );
    conn.prepare(&sql)?
        .query_map(rusqlite::params![user_id, album_id], map_track)?
        .collect()
}

/// Documents the List Albums By Group Tracks public API surface.
pub fn list_albums_by_group_tracks(
    conn: &Connection,
    user_id: &str,
    title: &str,
    album_artist: &str,
    library_ids: &[EntityId],
) -> rusqlite::Result<Vec<TrackRow>> {
    let lib_where = if library_ids.is_empty() {
        String::new()
    } else {
        let ph = library_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        format!("AND t.library_id IN ({ph})")
    };

    // Driven from `albums` (filtered by the WHERE clause) rather than `tracks`: SQLite has no
    // index usable for `LOWER(al.title) = ...`, so starting the scan from `tracks` — the much
    // larger table — made the planner iterate every track in the library for every album click
    // (measured ~350ms on a 63k-track/9k-album library) before joining out to the one matching
    // album. Starting from `albums` and joining tracks via the indexed `idx_tracks_album` cuts
    // that to a few ms even without a supporting index, since the WHERE now only scans albums.
    let sql = format!(
        "SELECT {TRACK_COLS}, trr.rating
         FROM albums al
         JOIN tracks t ON t.album_id = al.id
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE LOWER(al.title) = LOWER(?) AND al.album_artist = COALESCE(?,'')
           {lib_where}
         ORDER BY COALESCE(t.disc_number,1) ASC, COALESCE(t.track_number,9999) ASC, t.title ASC"
    );

    let mut params: Vec<Value> = vec![
        Value::Text(user_id.to_owned()),
        Value::Text(title.to_owned()),
        Value::Text(album_artist.to_owned()),
    ];
    for id in library_ids {
        params.push(id_to_value(id));
    }

    conn.prepare(&sql)?
        .query_map(params_from_iter(params), map_track)?
        .collect()
}

/// Documents the Get Track public API surface.
pub fn get_track(
    conn: &Connection,
    user_id: &str,
    track_id: &EntityId,
) -> rusqlite::Result<Option<TrackRow>> {
    let sql = format!(
        "SELECT {TRACK_COLS}, trr.rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE t.id = ?"
    );
    let mut track = conn
        .query_row(&sql, rusqlite::params![user_id, track_id], map_track)
        .optional()?;
    if let Some(row) = track.as_mut() {
        row.file_path = conn
            .query_row(
                "SELECT file_path FROM tracks WHERE id = ?",
                rusqlite::params![track_id],
                |r| r.get(0),
            )
            .optional()?;
    }
    Ok(track)
}

fn blank_to_none(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// `upsert_artist`/`upsert_album` only ever fail via a propagated `rusqlite::Error`
/// (they carry no other fallible step), so this always unwraps the `Db` arm in
/// practice; the fallback exists purely so the match stays exhaustive.
fn job_err_to_rusqlite(err: crate::jobs::JobError) -> rusqlite::Error {
    match err {
        crate::jobs::JobError::Db(inner) => inner,
        other => rusqlite::Error::InvalidParameterName(other.to_string()),
    }
}

/// Public Track Metadata Update data shape used by BoogieBox.
#[derive(Debug, Default)]
pub struct TrackMetadataUpdate {
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Album public API surface.
    pub album: Option<String>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// Documents the Composer public API surface.
    pub composer: Option<String>,
    /// Documents the Comment public API surface.
    pub comment: Option<String>,
    /// Documents the Track Number public API surface.
    pub track_number: Option<i64>,
    /// Documents the Disc Number public API surface.
    pub disc_number: Option<i64>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
}

/// Updates a track's tag metadata (title/artist/album/genre/composer/comment/
/// track & disc number/year) in the database only — it never touches the audio
/// file's own tags. Renaming `artist` or `album` resolves-or-creates the target
/// row the same way the scanner does (see [`crate::jobs::upsert_artist`] /
/// [`crate::jobs::upsert_album`]) and moves the track onto it, so a typo can
/// create a new artist/album rather than editing the intended one — callers
/// should treat these two fields with the same care as a scanner tag edit.
/// Returns `Ok(None)` when the track does not exist.
pub fn update_track_metadata(
    conn: &Connection,
    track_id: &EntityId,
    update: TrackMetadataUpdate,
) -> rusqlite::Result<Option<crate::playlists::MetadataUpdateResult>> {
    let current_artist_name: Option<String> = conn
        .query_row(
            "SELECT COALESCE(ar.name, '') FROM tracks t
             LEFT JOIN artists ar ON ar.id = t.artist_id
             WHERE t.id = ?",
            rusqlite::params![track_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(current_artist_name) = current_artist_name else {
        return Ok(None);
    };

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> rusqlite::Result<()> {
        let mut sets: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();

        if let Some(title) = update.title.as_deref().and_then(blank_to_none) {
            sets.push("title=?");
            values.push(Value::Text(title.to_owned()));
        }

        // Resolve the artist first: a simultaneous album rename is owned by
        // whichever artist name applies after this update, not the old one.
        let mut effective_artist_name = current_artist_name;
        if let Some(artist_name) = update.artist.as_deref().and_then(blank_to_none) {
            let artist_id =
                crate::jobs::upsert_artist(conn, artist_name).map_err(job_err_to_rusqlite)?;
            sets.push("artist_id=?");
            values.push(id_to_value(&artist_id));
            effective_artist_name = artist_name.to_owned();
        }

        if let Some(album_title) = update.album.as_deref().and_then(blank_to_none) {
            let album_id = crate::jobs::upsert_album(conn, album_title, &effective_artist_name)
                .map_err(job_err_to_rusqlite)?;
            sets.push("album_id=?");
            values.push(id_to_value(&album_id));
        }

        if let Some(genre) = &update.genre {
            sets.push("genre=?");
            values.push(Value::Text(blank_to_none(genre).unwrap_or("").to_owned()));
        }
        if let Some(composer) = &update.composer {
            sets.push("composer=?");
            values.push(Value::Text(
                blank_to_none(composer).unwrap_or("").to_owned(),
            ));
        }
        if let Some(comment) = &update.comment {
            sets.push("comment=?");
            values.push(Value::Text(blank_to_none(comment).unwrap_or("").to_owned()));
        }
        if let Some(track_number) = update.track_number {
            sets.push("track_number=?");
            values.push(Value::Integer(track_number));
        }
        if let Some(disc_number) = update.disc_number {
            sets.push("disc_number=?");
            values.push(Value::Integer(disc_number));
        }
        if let Some(year) = update.year {
            sets.push("year=?");
            values.push(Value::Integer(year));
        }

        if !sets.is_empty() {
            values.push(id_to_value(track_id));
            let sql = format!("UPDATE tracks SET {} WHERE id=?", sets.join(", "));
            conn.execute(&sql, params_from_iter(values))?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(Some(crate::playlists::MetadataUpdateResult {
                ok: true,
                merged_into: None,
            }))
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// Documents the List Recently Played public API surface.
pub fn list_recently_played(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<TrackRow>> {
    let sql = format!(
        "SELECT {TRACK_COLS}, NULL AS rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE t.last_played_at IS NOT NULL AND t.last_played_at != ''
         ORDER BY datetime(t.last_played_at) DESC, t.id DESC
         LIMIT ?"
    );
    conn.prepare(&sql)?
        .query_map(rusqlite::params![limit], map_track)?
        .collect()
}

/// Documents the List Top Played public API surface.
pub fn list_top_played(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<TrackRow>> {
    let sql = format!(
        "SELECT {TRACK_COLS}, NULL AS rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE COALESCE(t.play_count,0) > 0
         ORDER BY COALESCE(t.play_count,0) DESC, datetime(t.last_played_at) DESC, t.id DESC
         LIMIT ?"
    );
    conn.prepare(&sql)?
        .query_map(rusqlite::params![limit], map_track)?
        .collect()
}

// ── Home top-rated ────────────────────────────────────────────────────────────

/// Public Home Top Rated data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct HomeTopRated {
    /// Documents the Artists public API surface.
    pub artists: Vec<ArtistRow>,
    /// Documents the Albums public API surface.
    pub albums: Vec<AlbumRow>,
    /// Documents the Tracks public API surface.
    pub tracks: Vec<TrackRow>,
}

/// Documents the Get Home Top Rated public API surface.
pub fn get_home_top_rated(
    conn: &Connection,
    user_id: &str,
    limit: i64,
) -> rusqlite::Result<HomeTopRated> {
    let artists: Vec<ArtistRow> = conn
        .prepare(
            "SELECT ar.id, ar.name, art.rating, NULL, NULL,
                    COUNT(DISTINCT t.id), COUNT(DISTINCT t.album_id), COALESCE(ar.play_count,0)
             FROM artist_ratings art
             INNER JOIN artists ar ON ar.id = art.artist_id
             LEFT JOIN tracks t ON t.artist_id = ar.id
             WHERE art.user_id = ?
             GROUP BY ar.id
             ORDER BY art.rating DESC, LOWER(ar.name) ASC, ar.id ASC
             LIMIT ?",
        )?
        .query_map(rusqlite::params![user_id, limit], |row| {
            Ok(ArtistRow {
                id: row.get(0)?,
                name: row.get(1)?,
                rating: row.get(2)?,
                metadata_locked: row.get(3)?,
                identity_lock_pending: None,
                description: row.get(4)?,
                track_count: row.get(5)?,
                album_count: row.get(6)?,
                play_count: Some(row.get(7)?),
                styles: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let albums: Vec<AlbumRow> = conn
        .prepare(
            "SELECT MIN(al.id), al.title, al.album_artist, ar.name AS artist,
                    al.year, al.genre, MAX(alr.rating), MIN(al.release_type),
                    COUNT(DISTINCT t.id), SUM(COALESCE(t.duration,0)), NULL, NULL
             FROM album_ratings alr
             INNER JOIN albums al ON al.id = alr.album_id
             LEFT JOIN artists ar ON ar.id = al.artist_id
             LEFT JOIN tracks t ON t.album_id = al.id
             WHERE alr.user_id = ?
             GROUP BY LOWER(al.title), LOWER(COALESCE(NULLIF(al.album_artist,''),ar.name,''))
             ORDER BY MAX(alr.rating) DESC, LOWER(al.title) ASC, MIN(al.id) ASC
             LIMIT ?",
        )?
        .query_map(rusqlite::params![user_id, limit], |row| {
            Ok(AlbumRow {
                id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                rating: row.get(6)?,
                release_type: row.get(7)?,
                track_count: row.get(8)?,
                total_duration: row.get(9)?,
                metadata_locked: row.get(10)?,
                description: row.get(11)?,
                label: None,
                added_at: None,
                latest_scanned_at: None,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let tracks: Vec<TrackRow> = conn
        .prepare(&format!(
            "SELECT {TRACK_COLS}, tr.rating
             FROM track_ratings tr
             INNER JOIN tracks t ON t.id = tr.track_id
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id
             LEFT JOIN libraries l ON l.id = t.library_id
             WHERE tr.user_id = ?
             ORDER BY tr.rating DESC,
                      LOWER(COALESCE(NULLIF(t.title,''),t.file_name)) ASC,
                      t.id ASC
             LIMIT ?"
        ))?
        .query_map(rusqlite::params![user_id, limit], map_track)?
        .collect::<rusqlite::Result<_>>()?;

    Ok(HomeTopRated {
        artists,
        albums,
        tracks,
    })
}

// ── Auto-DJ ───────────────────────────────────────────────────────────────────

/// Documents the List Auto Dj Candidates public API surface.
pub fn list_auto_dj_candidates(
    conn: &Connection,
    genres: &[String],
    library_id: Option<&EntityId>,
    candidate_limit: i64,
) -> rusqlite::Result<Vec<TrackRow>> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    if let Some(lid) = library_id {
        conditions.push("t.library_id = ?".into());
        params.push(id_to_value(lid));
    }

    if !genres.is_empty() {
        let ph = genres.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        conditions.push(format!("LOWER(TRIM(COALESCE(t.genre,''))) IN ({ph})"));
        for g in genres {
            params.push(Value::Text(g.to_lowercase()));
        }
    }

    params.push(Value::Integer(candidate_limit));

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT {TRACK_COLS}, NULL AS rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN libraries l ON l.id = t.library_id
         {where_clause}
         ORDER BY RANDOM()
         LIMIT ?"
    );

    conn.prepare(&sql)?
        .query_map(params_from_iter(params), map_track)?
        .collect()
}

// ── Interleave by artist (auto-dj / radio) ───────────────────────────────────

/// Documents the Get Artist Name public API surface.
pub fn get_artist_name(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT name FROM artists WHERE id = ?",
        rusqlite::params![artist_id],
        |row| row.get(0),
    )
    .optional()
}

/// Documents the List Artist Radio Tags public API surface.
pub fn list_artist_radio_tags(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT style FROM artist_styles WHERE artist_id = ? ORDER BY style")?
        .query_map(rusqlite::params![artist_id], |row| row.get::<_, String>(0))?
        .collect()
}

/// Documents the List Artist Radio Candidates public API surface.
pub fn list_artist_radio_candidates(
    conn: &Connection,
    artist_id: &EntityId,
    tags_len: usize,
    limit: i64,
) -> rusqlite::Result<Vec<TrackRow>> {
    let min_overlap = if tags_len >= 4 { 2 } else { 1 };
    let per_artist_cap = ((limit + 11) / 12).clamp(2, 8);
    let sql = format!(
        "WITH target_styles AS (
           SELECT style FROM artist_styles WHERE artist_id = ?
         ),
         style_popularity AS (
           SELECT style, COUNT(DISTINCT artist_id) AS artist_count
           FROM artist_styles
           GROUP BY style
         ),
         target_weights AS (
           SELECT ts.style AS style,
                  COALESCE(1.0 / (1 + sp.artist_count), 1.0) AS weight
           FROM target_styles ts
           LEFT JOIN style_popularity sp ON sp.style = ts.style
         ),
         similar_artists AS (
           SELECT s2.artist_id AS artist_id,
                  COUNT(*) AS overlap,
                  SUM(tw.weight) AS score
           FROM artist_styles s2
           JOIN target_weights tw ON tw.style = s2.style
           WHERE s2.artist_id != ?
           GROUP BY s2.artist_id
           HAVING COUNT(*) >= ?
         ),
         ranked_tracks AS (
           SELECT {TRACK_COLS}, NULL AS rating,
                  sa.score AS score,
                  sa.overlap AS overlap,
                  ROW_NUMBER() OVER (PARTITION BY t.artist_id ORDER BY RANDOM()) AS artist_pick
           FROM tracks t
           JOIN similar_artists sa ON sa.artist_id = t.artist_id
           LEFT JOIN artists ar ON ar.id = t.artist_id
           LEFT JOIN albums al ON al.id = t.album_id
           LEFT JOIN libraries l ON l.id = t.library_id
         )
         SELECT id, file_name, file_size, format, duration, bitrate, sample_rate, channels,
                title, track_number, disc_number, year, genre, composer, comment,
                bpm, bpm_detected, bpm_source, bpm_confidence, scanned_at,
                last_played_at, play_count, album_id, artist, album, library_name,
                has_deep_analysis, rating
         FROM ranked_tracks
         WHERE artist_pick <= ?
         ORDER BY score DESC, overlap DESC, RANDOM()
         LIMIT ?"
    );
    conn.prepare(&sql)?
        .query_map(
            params_from_iter([
                id_to_value(artist_id),
                id_to_value(artist_id),
                Value::Integer(min_overlap),
                Value::Integer(per_artist_cap),
                Value::Integer(limit),
            ]),
            map_track,
        )?
        .collect()
}

/// Documents the List Artist Own Random Tracks public API surface.
pub fn list_artist_own_random_tracks(
    conn: &Connection,
    artist_id: &EntityId,
    limit: i64,
) -> rusqlite::Result<Vec<TrackRow>> {
    let sql = format!(
        "SELECT {TRACK_COLS}, NULL AS rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN libraries l ON l.id = t.library_id
         WHERE t.artist_id = ?
         ORDER BY RANDOM()
         LIMIT ?"
    );
    conn.prepare(&sql)?
        .query_map(
            params_from_iter([id_to_value(artist_id), Value::Integer(limit)]),
            map_track,
        )?
        .collect()
}

/// Documents the Normalize Artist Release Types public API surface.
pub fn normalize_artist_release_types(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE albums
         SET release_type = 'album'
         WHERE artist_id = ?
           AND COALESCE(metadata_locked, 0) = 0
           AND (
             release_type IS NULL
             OR TRIM(release_type) = ''
             OR LOWER(TRIM(release_type)) NOT IN ('album', 'single', 'compilation')
           )",
        rusqlite::params![artist_id],
    )
}

// ── Search ────────────────────────────────────────────────────────────────────

fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Port of Node's `buildSafeFtsPrefixQuery`. Returns None if no usable words remain.
/// Documents the Build Safe Fts Prefix Query public API surface.
pub fn build_safe_fts_prefix_query(raw: &str) -> Option<String> {
    const RESERVED: &[&str] = &["and", "or", "not", "near"];
    let cleaned = raw
        .trim()
        .replace(['"', '\'', '*', '(', ')', '^', '-', ':', ','], " ");
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !RESERVED.contains(&w.to_lowercase().as_str()))
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(
        words
            .iter()
            .map(|w| format!("\"{}\"*", w))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Returns true when the FTS table has populated UUID track ids.
fn tracks_fts_has_track_id(conn: &Connection) -> bool {
    if !column_exists_local(conn, "tracks_fts", "track_id").unwrap_or(false) {
        return false;
    }

    conn.query_row(
        "SELECT 1 FROM tracks_fts WHERE track_id IS NOT NULL LIMIT 1",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

/// Documents the Search Music Params public API surface.
pub struct SearchMusicParams<'a> {
    /// Documents the User Id public API surface.
    pub user_id: &'a str,
    /// Documents the Q public API surface.
    pub q: &'a str,
    /// Documents the Library Id public API surface.
    pub library_id: Option<EntityId>,
    /// Documents the Genre public API surface.
    pub genre: Option<&'a str>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
    /// Documents the Format public API surface.
    pub format: Option<&'a str>,
    /// Documents the Sort public API surface.
    pub sort: &'a str,
    /// Documents the Order public API surface.
    pub order: &'a str,
    /// Documents the Page public API surface.
    pub page: i64,
    /// Documents the Limit public API surface.
    pub limit: i64,
    /// Documents the Include Artists public API surface.
    pub include_artists: bool,
    /// Documents the Include Albums public API surface.
    pub include_albums: bool,
    /// Documents the Include Total public API surface.
    pub include_total: bool,
    /// Documents the Mobile Tracks Mode public API surface.
    pub mobile_tracks_mode: bool,
    /// Filter track results to those with a Sonic Fingerprint (deep analysis).
    pub sonic_fingerprint_only: bool,
}

/// Documents the Search Music public API surface.
pub fn search_music(conn: &Connection, p: SearchMusicParams<'_>) -> rusqlite::Result<JsonValue> {
    let trimmed = p.q.trim();
    let has_query = !trimmed.is_empty();
    let offset = (p.page - 1) * p.limit;
    let allowed_sorts = [
        "title",
        "artist",
        "album",
        "year",
        "duration",
        "bitrate",
        "genre",
        "rating",
        "scanned_at",
        "relevance",
    ];
    let sort_col = if allowed_sorts.contains(&p.sort) {
        p.sort
    } else if p.mobile_tracks_mode {
        "relevance"
    } else {
        "title"
    };
    let order_dir = if p.order == "desc" { "DESC" } else { "ASC" };

    let fts_query = if has_query {
        build_safe_fts_prefix_query(trimmed)
    } else {
        None
    };
    let suppress_tracks = has_query && fts_query.is_none();
    let use_fts = has_query && fts_query.is_some();
    let uses_relevance = use_fts && sort_col == "relevance";

    let use_track_id_col = tracks_fts_has_track_id(conn);
    let fts_join = if use_track_id_col {
        "JOIN tracks_fts fts ON fts.track_id = t.id"
    } else {
        "JOIN tracks_fts fts ON fts.rowid = t.rowid"
    };

    // ── Artist search ──────────────────────────────────────────────────────────
    let artists: Vec<JsonValue> = if has_query && p.include_artists {
        let prefix = format!("{}%", escape_like_pattern(trimmed));
        let sql = format!(
            "SELECT ar.id, ar.name,
                    MAX(arr.rating) AS rating,
                    (SELECT COUNT(*) FROM tracks t JOIN albums al ON al.id = t.album_id WHERE al.artist_id = ar.id) AS track_count,
                    (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id) AS album_count
             FROM artists ar
             LEFT JOIN artist_ratings arr ON arr.artist_id = ar.id AND arr.user_id = ?1
             WHERE ar.name LIKE ?2 ESCAPE '\\'
             GROUP BY ar.id
             ORDER BY ar.name COLLATE NOCASE {order_dir}
             LIMIT 10"
        );
        let mut stmt = conn.prepare(&sql)?;
        let raw: Vec<rusqlite::Result<JsonValue>> = stmt
            .query_map(rusqlite::params![p.user_id, prefix], |row| {
                let id: EntityId = row.get(0)?;
                let name: String = row.get(1)?;
                let rating: Option<f64> = row.get(2)?;
                let track_count: i64 = row.get(3)?;
                let album_count: i64 = row.get(4)?;
                Ok(serde_json::json!({
                    "id": id,
                    "name": name,
                    "rating": rating,
                    "track_count": track_count,
                    "album_count": album_count,
                }))
            })?
            .collect();
        raw.into_iter().filter_map(|r| r.ok()).collect()
    } else {
        vec![]
    };

    // ── Album search ───────────────────────────────────────────────────────────
    let albums: Vec<JsonValue> = if has_query && p.include_albums {
        let prefix = format!("{}%", escape_like_pattern(trimmed));
        let sql = format!(
            "SELECT MIN(al.id) AS id, al.title, al.album_artist,
                    al.album_artist AS artist,
                    MIN(al.year) AS year, al.genre,
                    MAX(alr.rating) AS rating,
                    MIN(al.release_type) AS releaseType,
                    COUNT(t.id) AS track_count,
                    ROUND(SUM(t.duration), 0) AS total_duration
             FROM albums al
             JOIN tracks t ON t.album_id = al.id
             LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?1
             WHERE al.title LIKE ?2 ESCAPE '\\' OR al.album_artist LIKE ?2 ESCAPE '\\'
             GROUP BY al.title, COALESCE(al.album_artist, '')
             ORDER BY al.title COLLATE NOCASE {order_dir}
             LIMIT 20"
        );
        let mut stmt = conn.prepare(&sql)?;
        let raw: Vec<rusqlite::Result<JsonValue>> = stmt
            .query_map(rusqlite::params![p.user_id, prefix], |row| {
                let id: EntityId = row.get(0)?;
                let title: String = row.get(1)?;
                let album_artist: Option<String> = row.get(2)?;
                let artist: Option<String> = row.get(3)?;
                let year: Option<i64> = row.get(4)?;
                let genre: Option<String> = row.get(5)?;
                let rating: Option<f64> = row.get(6)?;
                let release_type: Option<String> = row.get(7)?;
                let track_count: i64 = row.get(8)?;
                let total_duration: Option<f64> = row.get(9)?;
                Ok(serde_json::json!({
                    "id": id,
                    "title": title,
                    "album_artist": album_artist,
                    "artist": artist,
                    "year": year,
                    "genre": genre,
                    "rating": rating,
                    "releaseType": release_type,
                    "track_count": track_count,
                    "total_duration": total_duration,
                }))
            })?
            .collect();
        raw.into_iter().filter_map(|r| r.ok()).collect()
    } else {
        vec![]
    };

    // ── Track search ───────────────────────────────────────────────────────────
    let (tracks, total, has_more) = if suppress_tracks || !has_query {
        (vec![], 0i64, false)
    } else if use_fts {
        let fts_str = fts_query.as_deref().unwrap_or("");
        let mut conds: Vec<String> = vec!["tracks_fts MATCH ?".to_owned()];
        let mut bind: Vec<Box<dyn ToSql>> = vec![Box::new(fts_str.to_owned())];

        if let Some(ref lib_id) = p.library_id {
            conds.push("t.library_id = ?".to_owned());
            match lib_id {
                EntityId::Int(n) => bind.push(Box::new(*n)),
                EntityId::Str(s) => bind.push(Box::new(s.clone())),
            }
        }
        if let Some(g) = p.genre {
            conds.push("t.genre LIKE ?".to_owned());
            bind.push(Box::new(format!("%{}%", g)));
        }
        if let Some(y) = p.year {
            conds.push("t.year = ?".to_owned());
            bind.push(Box::new(y));
        }
        if let Some(f) = p.format {
            conds.push("t.format LIKE ?".to_owned());
            bind.push(Box::new(format!("%{}%", f)));
        }
        if p.sonic_fingerprint_only {
            conds.push(
                "EXISTS (SELECT 1 FROM track_deep_analysis da WHERE da.track_id = t.id AND da.confidence > 0.25)".to_owned(),
            );
        }

        let where_clause = format!("WHERE {}", conds.join(" AND "));
        let sort_expr = if uses_relevance {
            format!("bm25(tracks_fts), t.title COLLATE NOCASE {order_dir}")
        } else {
            match sort_col {
                "artist" => format!("ar.name {order_dir}"),
                "album" => format!("al.title {order_dir}"),
                "rating" => {
                    format!("COALESCE(trr.rating, 0) {order_dir}, t.title COLLATE NOCASE ASC")
                }
                _ => format!("t.{sort_col} {order_dir}"),
            }
        };

        let track_fetch_limit = if p.include_total {
            p.limit
        } else {
            p.limit + 1
        };

        let mut total_count = 0i64;
        if p.include_total {
            let count_sql = format!(
                "SELECT COUNT(*) FROM tracks t {fts_join}
                 LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id
                 {where_clause}"
            );
            let uid = p.user_id.to_owned();
            let fts_for_count = fts_str.to_owned();
            let mut count_bind: Vec<Box<dyn ToSql>> = vec![Box::new(uid)];
            count_bind.push(Box::new(fts_for_count));
            if let Some(ref lib_id) = p.library_id {
                match lib_id {
                    EntityId::Int(n) => count_bind.push(Box::new(*n)),
                    EntityId::Str(s) => count_bind.push(Box::new(s.clone())),
                }
            }
            if let Some(g) = p.genre {
                count_bind.push(Box::new(format!("%{}%", g)));
            }
            if let Some(y) = p.year {
                count_bind.push(Box::new(y));
            }
            if let Some(f) = p.format {
                count_bind.push(Box::new(format!("%{}%", f)));
            }
            total_count = conn
                .query_row(&count_sql, params_from_iter(count_bind.iter()), |r| {
                    r.get(0)
                })
                .unwrap_or(0);
        }

        let track_sql = format!(
            "SELECT {TRACK_COLS}, trr.rating AS rating
             FROM tracks t {fts_join}
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id
             LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
             LEFT JOIN libraries l ON l.id = t.library_id
             {where_clause}
             ORDER BY {sort_expr}
             LIMIT ? OFFSET ?"
        );
        let uid = p.user_id.to_owned();
        let fts_final = fts_str.to_owned();
        let mut track_bind: Vec<Box<dyn ToSql>> = vec![Box::new(uid)];
        track_bind.push(Box::new(fts_final));
        if let Some(ref lib_id) = p.library_id {
            match lib_id {
                EntityId::Int(n) => track_bind.push(Box::new(*n)),
                EntityId::Str(s) => track_bind.push(Box::new(s.clone())),
            }
        }
        if let Some(g) = p.genre {
            track_bind.push(Box::new(format!("%{}%", g)));
        }
        if let Some(y) = p.year {
            track_bind.push(Box::new(y));
        }
        if let Some(f) = p.format {
            track_bind.push(Box::new(format!("%{}%", f)));
        }
        track_bind.push(Box::new(track_fetch_limit));
        track_bind.push(Box::new(offset));

        let mut stmt = conn.prepare(&track_sql)?;
        let fetched: Vec<TrackRow> = stmt
            .query_map(params_from_iter(track_bind.iter()), map_track)?
            .filter_map(|r| r.ok())
            .collect();

        let more = !p.include_total && fetched.len() > p.limit as usize;
        let page_tracks: Vec<TrackRow> = if more {
            fetched.into_iter().take(p.limit as usize).collect()
        } else {
            fetched
        };
        if !p.include_total {
            total_count = (offset + page_tracks.len() as i64) + if more { 1 } else { 0 };
        }

        (page_tracks, total_count, more)
    } else {
        (vec![], 0, false)
    };

    let tracks_json: Vec<JsonValue> = tracks
        .into_iter()
        .map(|t| serde_json::to_value(t).unwrap_or(JsonValue::Null))
        .collect();

    Ok(serde_json::json!({
        "tracks": tracks_json,
        "total": total,
        "page": p.page,
        "limit": p.limit,
        "hasMore": has_more,
        "artists": artists,
        "albums": albums,
        "top_results": [],
        "groups": {
            "tracks": { "total": total, "has_more": has_more },
            "artists": { "total": artists.len(), "has_more": false },
            "albums": { "total": albums.len(), "has_more": false },
        }
    }))
}

/// Documents the Interleave By Artist public API surface.
pub fn interleave_by_artist(tracks: Vec<TrackRow>, limit: usize) -> Vec<TrackRow> {
    let mut buckets: Vec<(String, Vec<TrackRow>)> = Vec::new();
    let mut bucket_index: HashMap<String, usize> = HashMap::new();

    for track in tracks {
        let key = track.artist.as_deref().unwrap_or("").trim().to_owned();
        let key = if key.is_empty() {
            "__unknown__".to_owned()
        } else {
            key
        };
        if let Some(&idx) = bucket_index.get(&key) {
            buckets[idx].1.push(track);
        } else {
            bucket_index.insert(key.clone(), buckets.len());
            buckets.push((key, vec![track]));
        }
    }

    // Sort largest bucket first
    buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.1.len()));

    let mut result: Vec<TrackRow> = Vec::with_capacity(limit);
    let mut last_artist = String::new();

    while result.len() < limit && !buckets.is_empty() {
        let pick_idx = if buckets.len() > 1 && buckets[0].0 == last_artist {
            1
        } else {
            0
        };
        let track = buckets[pick_idx].1.remove(0);
        last_artist = buckets[pick_idx].0.clone();
        result.push(track);

        if buckets[pick_idx].1.is_empty() {
            buckets.remove(pick_idx);
        } else {
            // Re-sort to maintain descending bucket size
            let remaining = buckets[pick_idx].1.len();
            let mut i = pick_idx;
            while i > 0 && buckets[i - 1].1.len() < remaining {
                buckets.swap(i - 1, i);
                i -= 1;
            }
        }
    }

    result
}

/// Write algorithmically-detected BPM back to the tracks table.
/// Only updates if the track currently has no detected BPM, so manually-set
/// values (from tags or previous detection) are never overwritten.
/// Documents the Set Track Bpm Detected public API surface.
pub fn set_track_bpm_detected(
    conn: &Connection,
    track_id: &EntityId,
    bpm: f64,
    source: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET bpm_detected=?1, bpm_source=?2, bpm_confidence=0.75
          WHERE id=?3 AND (bpm_detected IS NULL OR bpm_detected=0)",
        rusqlite::params![bpm, source, track_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_search_schema(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE TABLE artists (id TEXT PRIMARY KEY, name TEXT, metadata_locked INTEGER, description TEXT, play_count INTEGER);
            CREATE TABLE albums (
                id TEXT PRIMARY KEY,
                title TEXT,
                album_artist TEXT,
                artist_id TEXT,
                year INTEGER,
                genre TEXT,
                release_type TEXT,
                metadata_locked INTEGER,
                description TEXT,
                label TEXT,
                added_at TEXT
            );
            CREATE TABLE libraries (id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE tracks (
                id TEXT PRIMARY KEY,
                library_id TEXT,
                artist_id TEXT,
                album_id TEXT,
                title TEXT,
                file_name TEXT,
                album_artist TEXT,
                genre TEXT,
                composer TEXT,
                duration REAL,
                format TEXT,
                bitrate INTEGER,
                sample_rate INTEGER,
                channels INTEGER,
                file_path TEXT,
                file_size INTEGER,
                track_number INTEGER,
                disc_number INTEGER,
                year INTEGER,
                comment TEXT,
                bpm REAL,
                bpm_detected REAL,
                bpm_source TEXT,
                bpm_confidence REAL,
                scanned_at TEXT,
                last_played_at TEXT,
                play_count INTEGER
            );
            CREATE TABLE artist_ratings (artist_id TEXT, user_id TEXT, rating REAL);
            CREATE TABLE album_ratings (album_id TEXT, user_id TEXT, rating REAL);
            CREATE TABLE track_ratings (track_id TEXT, user_id TEXT, rating REAL);
            CREATE TABLE artist_name_aliases (alias_name_normalized TEXT PRIMARY KEY, artist_id TEXT);
            CREATE TABLE track_deep_analysis (
                track_id TEXT PRIMARY KEY,
                confidence REAL NOT NULL DEFAULT 0.0
            );
            CREATE VIRTUAL TABLE tracks_fts USING fts5(
                track_id UNINDEXED,
                title,
                artist,
                album,
                genre,
                composer,
                file_path,
                content='',
                tokenize='unicode61'
            );
            ",
        )
        .unwrap();
    }

    /// Same shape as `create_search_schema` but omits `albums.added_at`, exercising
    /// `list_albums_latest`'s pre-`added_at` fallback (older, unmigrated schemas).
    fn create_search_schema_without_added_at(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE TABLE artists (id TEXT PRIMARY KEY, name TEXT, metadata_locked INTEGER, description TEXT, play_count INTEGER);
            CREATE TABLE albums (
                id TEXT PRIMARY KEY,
                title TEXT,
                album_artist TEXT,
                artist_id TEXT,
                year INTEGER,
                genre TEXT,
                release_type TEXT,
                metadata_locked INTEGER,
                description TEXT,
                label TEXT
            );
            CREATE TABLE libraries (id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE tracks (
                id TEXT PRIMARY KEY,
                library_id TEXT,
                artist_id TEXT,
                album_id TEXT,
                title TEXT,
                file_name TEXT,
                album_artist TEXT,
                genre TEXT,
                composer TEXT,
                duration REAL,
                format TEXT,
                bitrate INTEGER,
                sample_rate INTEGER,
                channels INTEGER,
                file_path TEXT,
                file_size INTEGER,
                track_number INTEGER,
                disc_number INTEGER,
                year INTEGER,
                comment TEXT,
                bpm REAL,
                bpm_detected REAL,
                bpm_source TEXT,
                bpm_confidence REAL,
                scanned_at TEXT,
                last_played_at TEXT,
                play_count INTEGER
            );
            CREATE TABLE album_ratings (album_id TEXT, user_id TEXT, rating REAL);
            ",
        )
        .unwrap();
    }

    #[test]
    fn search_music_uses_rowid_when_fts_track_id_column_is_empty() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute(
            "INSERT INTO artists (id, name) VALUES ('artist-1', 'New Order')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO albums (id, title, album_artist) VALUES ('album-1', 'Singles', 'New Order')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO libraries (id, name) VALUES ('library-1', 'Music')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, genre, format,
                duration, file_size, scanned_at
             ) VALUES (
                '019e6b35-708b-7b47-9ccd-aef6fa2b948c', 'library-1', 'artist-1', 'album-1',
                'Whole New Way', 'whole-new-way.flac', 'Electronic', 'flac', 240, 12345, '2026-06-01'
             )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks_fts (rowid, title, artist, album, genre, composer, file_path)
             SELECT rowid, title, 'New Order', 'Singles', genre, '', file_path FROM tracks",
            [],
        )
        .unwrap();

        let result = search_music(
            &conn,
            SearchMusicParams {
                user_id: "user-1",
                q: "new",
                library_id: None,
                genre: None,
                year: None,
                format: None,
                sort: "title",
                order: "asc",
                page: 1,
                limit: 10,
                include_artists: true,
                include_albums: true,
                include_total: true,
                mobile_tracks_mode: false,
                sonic_fingerprint_only: false,
            },
        )
        .unwrap();

        assert_eq!(result["total"], 1);
        assert_eq!(result["tracks"][0]["title"], "Whole New Way");
    }

    #[test]
    fn auto_dj_candidates_allow_library_scope_without_genres() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES
                ('library-1', 'First'),
                ('library-2', 'Second');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'Artist');
            INSERT INTO albums (id, title, album_artist) VALUES ('album-1', 'Album', 'Artist');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, genre, format,
                duration, file_size, scanned_at
            ) VALUES
                ('track-1', 'library-1', 'artist-1', 'album-1', 'First Track', 'first.flac', '', 'flac', 180, 1, '2026-08-10'),
                ('track-2', 'library-2', 'artist-1', 'album-1', 'Second Track', 'second.flac', 'Rock', 'flac', 180, 1, '2026-08-10');
            ",
        )
        .unwrap();

        let library_id = coerce_entity_id("library-1");
        let tracks = list_auto_dj_candidates(&conn, &[], Some(&library_id), 10).unwrap();

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, coerce_entity_id("track-1"));
    }

    fn seed_one_track(conn: &Connection) {
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES ('library-1', 'Home Library');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'deadmau5');
            INSERT INTO albums (id, title, album_artist, artist_id)
                VALUES ('album-1', 'For Lack of a Better Name', 'deadmau5', 'artist-1');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, genre, format,
                duration, file_size, file_path, scanned_at
            ) VALUES (
                'track-1', 'library-1', 'artist-1', 'album-1', 'Strobe', '02 - Strobe.flac',
                'Progressive House', 'flac', 637, 82246041, 'D:\\Music\\deadmau5\\For Lack of a Better Name\\02 - Strobe.flac',
                '2026-08-10 14:22:00'
            );
            ",
        )
        .unwrap();
    }

    #[test]
    fn get_track_includes_file_path() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        seed_one_track(&conn);

        let track = get_track(&conn, "user-1", &coerce_entity_id("track-1"))
            .unwrap()
            .expect("track exists");

        assert_eq!(
            track.file_path.as_deref(),
            Some("D:\\Music\\deadmau5\\For Lack of a Better Name\\02 - Strobe.flac")
        );
    }

    #[test]
    fn list_queries_leave_file_path_unset() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        seed_one_track(&conn);

        // TRACK_COLS-based list queries (unlike get_track) never select
        // file_path, to keep list payloads small — map_track defaults it to
        // None, and this must stay that way.
        let tracks = list_album_tracks(&conn, "user-1", &coerce_entity_id("album-1")).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].file_path, None);
    }

    #[test]
    fn list_albums_by_group_tracks_matches_title_and_album_artist_only() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES ('library-1', 'Home Library');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'Weezer'), ('artist-2', 'Green Day');
            -- Two different releases happen to share a title, distinguished only by album_artist —
            -- exactly the case list_albums_by_group_tracks groups on (client's group_by=album_artist mode).
            INSERT INTO albums (id, title, album_artist, artist_id) VALUES
                ('album-1', 'Greatest Hits', 'Weezer', 'artist-1'),
                ('album-2', 'Greatest Hits', 'Green Day', 'artist-2');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, genre, format,
                duration, file_size, track_number, scanned_at
            ) VALUES
                ('track-1', 'library-1', 'artist-1', 'album-1', 'Buddy Holly', 'buddy-holly.flac', 'Rock', 'flac', 160, 1, 1, '2026-08-10'),
                ('track-2', 'library-1', 'artist-2', 'album-2', 'Basket Case', 'basket-case.flac', 'Rock', 'flac', 180, 1, 1, '2026-08-10');
            ",
        )
        .unwrap();

        let tracks =
            list_albums_by_group_tracks(&conn, "user-1", "Greatest Hits", "Weezer", &[]).unwrap();

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, coerce_entity_id("track-1"));
    }

    #[test]
    fn list_albums_returns_bounded_incremental_rows_and_group_aggregates() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES ('library-1', 'Home Library');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'Artist One');
            INSERT INTO albums (id, title, album_artist, artist_id) VALUES
                ('album-1', 'Existing Album', 'Artist One', 'artist-1'),
                ('album-2', 'Shared Album', 'Artist One', 'artist-1');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, duration, scanned_at
            ) VALUES
                ('track-1', 'library-1', 'artist-1', 'album-1', 'Existing Track', 'existing.flac', 180, '2026-08-19'),
                ('track-2', 'library-1', 'artist-1', 'album-2', 'Shared One', 'shared-1.flac', 200, '2026-08-19');
            ",
        )
        .unwrap();
        let after = album_change_cursor(&conn).unwrap();

        conn.execute_batch(
            "
            INSERT INTO albums (id, title, album_artist, artist_id) VALUES
                ('album-3', 'Shared Album', 'Artist One', 'artist-1'),
                ('album-4', 'New Album', 'Artist One', 'artist-1');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, duration, scanned_at
            ) VALUES
                ('track-3', 'library-1', 'artist-1', 'album-3', 'Shared Two', 'shared-2.flac', 220, '2026-08-19'),
                ('track-4', 'library-1', 'artist-1', 'album-4', 'New Track', 'new.flac', 240, '2026-08-19');
            ",
        )
        .unwrap();
        let through = album_change_cursor(&conn).unwrap();

        conn.execute_batch(
            "
            INSERT INTO albums (id, title, album_artist, artist_id)
              VALUES ('album-5', 'Later Album', 'Artist One', 'artist-1');
            INSERT INTO tracks (
                id, library_id, artist_id, album_id, title, file_name, duration, scanned_at
            ) VALUES
                ('track-5', 'library-1', 'artist-1', 'album-5', 'Later Track', 'later.flac', 260, '2026-08-19');
            ",
        )
        .unwrap();

        let rows = list_albums(
            &conn,
            ListAlbumsParams {
                user_id: "user-1",
                library_ids: &[],
                genres: &[],
                by_album_artist: true,
                sonic_fingerprint_only: false,
                after_album_rowid: Some(after),
                through_album_rowid: Some(through),
            },
        )
        .unwrap();

        assert_eq!(
            rows.iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            vec!["New Album", "Shared Album"]
        );
        let shared = rows
            .iter()
            .find(|row| row.title == "Shared Album")
            .expect("changed grouped album");
        assert_eq!(shared.track_count, 2);
        assert_eq!(shared.total_duration, Some(420.0));
    }

    #[test]
    fn list_albums_by_group_tracks_scans_albums_not_tracks() {
        // Regression guard for the query-plan bug where the query was driven from `tracks`
        // (the larger table) instead of `albums`, forcing a full scan of every track in the
        // library per album click. Filtering by title/album_artist has no supporting index, so
        // the driving table must be `albums` — asserting that keeps the planner from
        // regressing to a `tracks`-first scan even without measuring wall-clock time.
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        // Matches the real schema's idx_tracks_album (see boogiebox-db::lib::initialize_schema)
        // — the minimal test fixture otherwise has no track index, which changes which table
        // the planner prefers to drive from and would make this guard test meaningless.
        conn.execute_batch("CREATE INDEX idx_tracks_album ON tracks(album_id)")
            .unwrap();

        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN \
                 SELECT t.id FROM albums al \
                 JOIN tracks t ON t.album_id = al.id \
                 WHERE LOWER(al.title) = LOWER(?) AND al.album_artist = COALESCE(?,'')",
            )
            .unwrap();
        let details: Vec<String> = stmt
            .query_map(rusqlite::params!["Greatest Hits", "Weezer"], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        // The first step in the plan is the driving (outermost) table; it must be `albums`
        // (the smaller, filtered table), not `tracks` (the whole-library scan this regression
        // test guards against).
        let driving_step = details.first().expect("plan has at least one step");
        assert!(
            driving_step.starts_with("SCAN al") || driving_step.starts_with("SEARCH al"),
            "expected albums to be the driving table, got: {details:?}"
        );
        assert!(
            details.iter().any(|d| d.contains("SEARCH t")),
            "expected tracks to be joined via an index seek, got: {details:?}"
        );
    }

    #[test]
    fn list_albums_latest_bounded_groups_aggregate_duplicates_rate_per_user_and_break_ties() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch("CREATE INDEX idx_tracks_album ON tracks(album_id)")
            .unwrap();
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES ('library-1', 'Home Library');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'Artist One');

            -- Duplicate album rows sharing the same (title, album_artist) group must
            -- aggregate into a single row, with the lexicographically-smallest id
            -- as the representative and MIN(added_at) across the group's rows as
            -- its sort value (existing behavior, preserved).
            INSERT INTO albums (id, title, album_artist, artist_id, added_at) VALUES
                ('album-1a', 'Shared Album', 'Artist One', 'artist-1', '2026-08-20T00:00:00Z'),
                ('album-1b', 'Shared Album', 'Artist One', 'artist-1', '2026-08-19T00:00:00Z');
            INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_name, duration, scanned_at) VALUES
                ('track-1a', 'library-1', 'artist-1', 'album-1a', 'Track A', 'a.flac', 180, '2026-08-20'),
                ('track-1b', 'library-1', 'artist-1', 'album-1b', 'Track B', 'b.flac', 200, '2026-08-19');

            -- Two groups with an identical sort value exercise the deterministic
            -- album_artist/title tie-break.
            INSERT INTO albums (id, title, album_artist, artist_id, added_at) VALUES
                ('album-2', 'Zeta Album', 'Zed Artist', 'artist-1', '2026-08-21T00:00:00Z'),
                ('album-3', 'Alpha Album', 'Zed Artist', 'artist-1', '2026-08-21T00:00:00Z');
            INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_name, duration, scanned_at) VALUES
                ('track-2', 'library-1', 'artist-1', 'album-2', 'Track Z', 'z.flac', 210, '2026-08-21'),
                ('track-3', 'library-1', 'artist-1', 'album-3', 'Track Al', 'al.flac', 190, '2026-08-21');

            -- Null album_artist must still group/sort deterministically via COALESCE.
            INSERT INTO albums (id, title, album_artist, artist_id, added_at) VALUES
                ('album-4', 'No Artist Album', NULL, 'artist-1', '2026-08-18T00:00:00Z');
            INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_name, duration, scanned_at) VALUES
                ('track-4', 'library-1', 'artist-1', 'album-4', 'Track N', 'n.flac', 150, '2026-08-18');

            -- Oldest group, excluded once the limit trims the bounded group stage.
            INSERT INTO albums (id, title, album_artist, artist_id, added_at) VALUES
                ('album-5', 'Oldest Album', 'Artist One', 'artist-1', '2026-08-01T00:00:00Z');
            INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_name, duration, scanned_at) VALUES
                ('track-5', 'library-1', 'artist-1', 'album-5', 'Track O', 'o.flac', 300, '2026-08-01');

            INSERT INTO album_ratings (album_id, user_id, rating) VALUES
                ('album-1a', 'user-1', 4.5),
                ('album-1a', 'user-2', 1.0);
            ",
        )
        .unwrap();

        let rows = list_albums_latest(&conn, "user-1", 4).unwrap();

        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec![
                "Alpha Album",
                "Zeta Album",
                "Shared Album",
                "No Artist Album"
            ],
            "expected added_at-desc order with album_artist/title tie-break, bounded to the limit"
        );

        let shared = rows.iter().find(|r| r.title == "Shared Album").unwrap();
        assert_eq!(shared.id, EntityId::Str("album-1a".to_string()));
        assert_eq!(shared.track_count, 2);
        assert_eq!(shared.total_duration, Some(380.0));
        // Rating is scoped to the requesting user, not just any rating on the album.
        assert_eq!(shared.rating, Some(4.5));

        assert!(
            rows.iter().all(|r| r.title != "Oldest Album"),
            "oldest group must be excluded by the limit"
        );

        let other_user_rows = list_albums_latest(&conn, "user-2", 4).unwrap();
        let shared_for_other = other_user_rows
            .iter()
            .find(|r| r.title == "Shared Album")
            .unwrap();
        assert_eq!(shared_for_other.rating, Some(1.0));
    }

    #[test]
    fn list_albums_latest_falls_back_to_track_scanned_at_when_added_at_column_is_absent() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema_without_added_at(&conn);
        conn.execute_batch("CREATE INDEX idx_tracks_album ON tracks(album_id)")
            .unwrap();
        conn.execute_batch(
            "
            INSERT INTO libraries (id, name) VALUES ('library-1', 'Home Library');
            INSERT INTO artists (id, name) VALUES ('artist-1', 'Artist One');
            INSERT INTO albums (id, title, album_artist, artist_id) VALUES
                ('album-1', 'Older Album', 'Artist One', 'artist-1'),
                ('album-2', 'Newer Album', 'Artist One', 'artist-1');
            INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_name, duration, scanned_at) VALUES
                ('track-1', 'library-1', 'artist-1', 'album-1', 'Track One', 'one.flac', 200, '2026-08-01'),
                ('track-2', 'library-1', 'artist-1', 'album-2', 'Track Two', 'two.flac', 210, '2026-08-20');
            ",
        )
        .unwrap();

        let rows = list_albums_latest(&conn, "user-1", 10).unwrap();

        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["Newer Album", "Older Album"]
        );
        assert_eq!(rows[0].added_at.as_deref(), Some("2026-08-20"));
    }

    #[test]
    fn list_albums_latest_group_stage_never_touches_tracks_when_added_at_present() {
        // Regression guard for the original bug: finding the latest bounded group
        // identities must not reference `tracks` at all when `added_at` is present,
        // so `LIMIT` is applied before any track-level work happens (previously the
        // whole library's tracks/albums were aggregated first).
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch(
            "CREATE INDEX idx_tracks_album ON tracks(album_id);
             CREATE INDEX idx_albums_added_at ON albums(added_at DESC)",
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN \
                 SELECT al.title, COALESCE(al.album_artist,''), MIN(al.added_at) AS sort_val \
                 FROM albums al \
                 GROUP BY al.title, COALESCE(al.album_artist,'') \
                 ORDER BY sort_val DESC, LOWER(COALESCE(al.album_artist,'')), al.title \
                 LIMIT ?",
            )
            .unwrap();
        let details: Vec<String> = stmt
            .query_map(rusqlite::params![60], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(
            details.iter().all(|d| !d.to_uppercase().contains("TRACKS")),
            "expected the bounded group-identity stage to never reference tracks, got: {details:?}"
        );
    }

    #[test]
    fn list_albums_latest_track_aggregation_uses_indexed_album_id_lookup() {
        // Regression guard: once the bounded group's album ids are known, aggregating
        // their tracks must go through an indexed `album_id` seek, not a full `tracks`
        // scan — this is the step that previously touched every track in the library.
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        conn.execute_batch("CREATE INDEX idx_tracks_album ON tracks(album_id)")
            .unwrap();

        let mut stmt = conn
            .prepare(
                "EXPLAIN QUERY PLAN \
                 SELECT MIN(al.id), al.title, al.album_artist, al.album_artist AS artist, \
                        MIN(al.year), al.genre, MAX(alr.rating), MIN(al.release_type), \
                        COUNT(t.id), ROUND(SUM(t.duration),0), MIN(al.added_at) AS added_at, \
                        MAX(t.scanned_at) AS latest_scanned_at, MIN(al.label) \
                 FROM albums al JOIN tracks t ON t.album_id = al.id \
                 LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ? \
                 WHERE al.id IN (?, ?, ?) \
                 GROUP BY al.title, COALESCE(al.album_artist,'') \
                 ORDER BY MIN(al.added_at) DESC, LOWER(COALESCE(al.album_artist,'')), al.title",
            )
            .unwrap();
        let details: Vec<String> = stmt
            .query_map(
                rusqlite::params!["user-1", "album-1", "album-2", "album-3"],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(
            details
                .iter()
                .any(|d| d.contains("SEARCH t") && d.contains("idx_tracks_album")),
            "expected tracks to be joined via idx_tracks_album, not a full scan, got: {details:?}"
        );
        assert!(
            details.iter().all(|d| !d.contains("SCAN t ")),
            "expected no full scan of tracks, got: {details:?}"
        );
    }

    #[test]
    fn update_track_metadata_updates_scalar_fields() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        seed_one_track(&conn);

        let track_id = coerce_entity_id("track-1");
        let result = update_track_metadata(
            &conn,
            &track_id,
            TrackMetadataUpdate {
                genre: Some("House".into()),
                year: Some(2009),
                track_number: Some(2),
                ..Default::default()
            },
        )
        .unwrap()
        .expect("track exists");
        assert!(result.ok);

        let track = get_track(&conn, "user-1", &track_id).unwrap().unwrap();
        assert_eq!(track.genre.as_deref(), Some("House"));
        assert_eq!(track.year, Some(2009));
        assert_eq!(track.track_number, Some(2));
        // Untouched fields survive.
        assert_eq!(track.title.as_deref(), Some("Strobe"));
    }

    #[test]
    fn update_track_metadata_rename_artist_resolves_or_creates() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);
        seed_one_track(&conn);

        let track_id = coerce_entity_id("track-1");
        update_track_metadata(
            &conn,
            &track_id,
            TrackMetadataUpdate {
                artist: Some("Joel Zimmerman".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .expect("track exists");

        let track = get_track(&conn, "user-1", &track_id).unwrap().unwrap();
        assert_eq!(track.artist.as_deref(), Some("Joel Zimmerman"));

        // Renaming to a name that already exists as another artist resolves
        // onto that existing row rather than creating a duplicate.
        update_track_metadata(
            &conn,
            &track_id,
            TrackMetadataUpdate {
                artist: Some("Joel Zimmerman".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let artist_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artists WHERE LOWER(name)=LOWER('Joel Zimmerman')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(artist_count, 1);
    }

    #[test]
    fn update_track_metadata_rejects_missing_track() {
        let conn = Connection::open_in_memory().unwrap();
        create_search_schema(&conn);

        let result = update_track_metadata(
            &conn,
            &coerce_entity_id("does-not-exist"),
            TrackMetadataUpdate::default(),
        )
        .unwrap();
        assert!(result.is_none());
    }

    fn seed_identity_library(conn: &Connection) {
        crate::initialize_schema(conn).expect("identity schema");
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES('library-1', 'D:/Music', 'Music');
             INSERT INTO artists(id, name) VALUES
               ('artist-owned', 'Owned Artist'),
               ('artist-duplicate', 'Duplicate Artist'),
               ('artist-appears', 'Appears Only');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('album-owned', 'Owned Release', 'Owned Artist', 'artist-owned'),
               ('album-duplicate', 'Duplicate Release', 'Duplicate Artist', 'artist-duplicate');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) VALUES
               ('track-owned', 'library-1', 'artist-owned', 'album-owned', 'Owned Track', 'D:/Music/owned.flac'),
               ('track-duplicate', 'library-1', 'artist-duplicate', 'album-duplicate', 'Duplicate Track', 'D:/Music/duplicate.flac'),
               ('track-appears', 'library-1', 'artist-appears', 'album-owned', 'Guest Track', 'D:/Music/guest.flac');",
        )
        .expect("identity fixtures");
    }

    #[test]
    fn artist_identity_persistence_is_optional_and_never_overwrites() {
        let conn = Connection::open_in_memory().unwrap();
        seed_identity_library(&conn);
        let artist_id = coerce_entity_id("artist-owned");

        assert!(persist_artist_identity_if_missing(
            &conn,
            &artist_id,
            ArtistIdentityProvider::LastFm,
            Some("mbid-1"),
            Some("Owned Artist Canonical"),
        )
        .unwrap());
        persist_artist_identity_if_missing(
            &conn,
            &artist_id,
            ArtistIdentityProvider::LastFm,
            Some("mbid-replacement"),
            Some("Replacement Name"),
        )
        .unwrap();
        persist_artist_identity_if_missing(
            &conn,
            &artist_id,
            ArtistIdentityProvider::Deezer,
            None,
            None,
        )
        .unwrap();

        let identity = get_artist_external_identity(&conn, &artist_id)
            .unwrap()
            .expect("artist identity");
        assert_eq!(identity.lastfm_mbid.as_deref(), Some("mbid-1"));
        assert_eq!(
            identity.lastfm_canonical_name.as_deref(),
            Some("Owned Artist Canonical")
        );
        assert!(identity.lastfm_identity_checked_at.is_some());
        assert_eq!(identity.deezer_artist_id, None);
        assert!(identity.deezer_identity_checked_at.is_some());
    }

    #[test]
    fn identity_selection_respects_missing_and_fresh_checks() {
        let conn = Connection::open_in_memory().unwrap();
        seed_identity_library(&conn);
        conn.execute_batch(
            "UPDATE artists SET
               lastfm_identity_checked_at='2026-08-17 12:00:00',
               deezer_identity_checked_at='2026-08-17 12:00:00',
               spotify_identity_checked_at='2026-08-17 12:00:00',
               discogs_identity_checked_at='2026-08-17 12:00:00'
             WHERE id='artist-owned';",
        )
        .unwrap();

        let pending = list_artists_needing_external_identity(
            &conn,
            &coerce_entity_id("library-1"),
            "2026-08-01 00:00:00",
        )
        .unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .all(|row| row.artist_id != coerce_entity_id("artist-owned")));
    }

    #[test]
    fn local_identity_resolution_requires_one_release_owning_artist() {
        let conn = Connection::open_in_memory().unwrap();
        seed_identity_library(&conn);
        conn.execute_batch(
            "UPDATE artists SET lastfm_mbid='shared-mbid' WHERE id IN ('artist-owned','artist-duplicate');
             UPDATE artists SET deezer_artist_id='deezer-owned' WHERE id='artist-owned';
             UPDATE artists SET deezer_artist_id='deezer-appears' WHERE id='artist-appears';
             UPDATE artists SET lastfm_canonical_name='Canonical Owned' WHERE id='artist-owned';",
        )
        .unwrap();

        assert_eq!(
            find_owned_artist_by_external_identity(
                &conn,
                ArtistIdentityProvider::Deezer,
                "deezer-owned"
            )
            .unwrap(),
            Some(coerce_entity_id("artist-owned"))
        );
        assert_eq!(
            find_owned_artist_by_external_identity(
                &conn,
                ArtistIdentityProvider::LastFm,
                "shared-mbid"
            )
            .unwrap(),
            None
        );
        assert_eq!(
            find_owned_artist_by_external_identity(
                &conn,
                ArtistIdentityProvider::Deezer,
                "deezer-appears"
            )
            .unwrap(),
            None
        );
        assert_eq!(
            find_owned_artist_by_name(&conn, " canonical owned ").unwrap(),
            Some(coerce_entity_id("artist-owned"))
        );
    }

    // ── Artist Merge / Unmerge ──────────────────────────────────────────────
    // Real, fully-migrated temp database per test (not a hand-rolled schema)
    // so merge/unmerge exercise the production FK/CASCADE/CHECK behavior —
    // mirrors the `Fixture`/`fixture()` pattern established in `jobs.rs`.

    fn merge_test_conn(prefix: &str) -> Connection {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("music-merge-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        crate::init_db(&dir).expect("init test db").connection
    }

    fn insert_test_library(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES(?1, ?2, 'Music')",
            rusqlite::params![id, format!("D:\\Music\\{id}")],
        )
        .unwrap();
    }

    fn insert_test_artist(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO artists(id, name) VALUES(?1, ?2)",
            rusqlite::params![id, name],
        )
        .unwrap();
    }

    fn insert_test_album(
        conn: &Connection,
        id: &str,
        title: &str,
        artist_id: &str,
        album_artist: &str,
    ) {
        conn.execute(
            "INSERT INTO albums(id, title, artist_id, album_artist) VALUES(?1, ?2, ?3, ?4)",
            rusqlite::params![id, title, artist_id, album_artist],
        )
        .unwrap();
    }

    fn insert_test_track(
        conn: &Connection,
        id: &str,
        library_id: &str,
        artist_id: &str,
        album_id: &str,
        title: &str,
    ) {
        conn.execute(
            "INSERT INTO tracks(
                id, library_id, artist_id, album_id, title, file_name, album_artist,
                genre, composer, duration, file_path, file_size
             ) VALUES(?1, ?2, ?3, ?4, ?5, 'file.flac', '', '', '', 0, ?6, 1)",
            rusqlite::params![
                id,
                library_id,
                artist_id,
                album_id,
                title,
                format!("D:\\Music\\{id}.flac")
            ],
        )
        .unwrap();
    }

    #[test]
    fn adopt_external_identity_keeps_masters_own_value() {
        let master = IdentitySnapshot {
            deezer_artist_id: Some("d1".into()),
            deezer_identity_checked_at: Some("t1".into()),
            ..Default::default()
        };
        let source = IdentitySnapshot {
            deezer_artist_id: Some("d2".into()),
            deezer_identity_checked_at: Some("t2".into()),
            ..Default::default()
        };
        let adopted = adopt_external_identity(&[master, source]);
        assert_eq!(adopted.deezer_artist_id.as_deref(), Some("d1"));
        assert_eq!(adopted.deezer_identity_checked_at.as_deref(), Some("t1"));
    }

    #[test]
    fn adopt_external_identity_adopts_first_non_null_source_and_clears_checked_at() {
        let master = IdentitySnapshot::default();
        let source1 = IdentitySnapshot::default();
        let source2 = IdentitySnapshot {
            spotify_artist_id: Some("s2".into()),
            spotify_identity_checked_at: Some("t2".into()),
            ..Default::default()
        };
        let adopted = adopt_external_identity(&[master, source1, source2]);
        assert_eq!(adopted.spotify_artist_id.as_deref(), Some("s2"));
        assert_eq!(
            adopted.spotify_identity_checked_at, None,
            "adopting from anything but index 0 must clear checked_at to force re-verification"
        );
    }

    #[test]
    fn adopt_external_identity_pairs_lastfm_mbid_with_its_own_canonical_name() {
        let master = IdentitySnapshot::default();
        let source = IdentitySnapshot {
            lastfm_mbid: Some("mbid-1".into()),
            lastfm_canonical_name: Some("Canonical".into()),
            lastfm_identity_checked_at: Some("t".into()),
            ..Default::default()
        };
        let adopted = adopt_external_identity(&[master, source]);
        assert_eq!(adopted.lastfm_mbid.as_deref(), Some("mbid-1"));
        assert_eq!(adopted.lastfm_canonical_name.as_deref(), Some("Canonical"));
        assert_eq!(adopted.lastfm_identity_checked_at, None);
    }

    #[test]
    fn adopt_external_identity_all_null_stays_null() {
        let adopted =
            adopt_external_identity(&[IdentitySnapshot::default(), IdentitySnapshot::default()]);
        assert!(adopted.lastfm_mbid.is_none());
        assert!(adopted.deezer_artist_id.is_none());
        assert!(adopted.spotify_artist_id.is_none());
        assert!(adopted.discogs_artist_id.is_none());
    }

    #[test]
    fn is_various_artists_matches_case_and_whitespace_insensitively() {
        assert!(is_various_artists("Various Artists"));
        assert!(is_various_artists("  various artists  "));
        assert!(!is_various_artists("Various"));
        assert!(!is_various_artists("Madonna"));
    }

    #[test]
    fn merge_artists_requires_at_least_two() {
        let conn = merge_test_conn("too-few");
        insert_test_artist(&conn, "a1", "Solo");
        let err =
            merge_artists(&conn, &[coerce_entity_id("a1")], "Solo", None, "user-1").unwrap_err();
        assert!(matches!(err, ArtistMergeError::TooFewArtists));
    }

    #[test]
    fn merge_artists_requires_a_name() {
        let conn = merge_test_conn("empty-name");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        let err = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "   ",
            None,
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::EmptyName));
    }

    #[test]
    fn merge_artists_rejects_unknown_artist() {
        let conn = merge_test_conn("not-found");
        insert_test_artist(&conn, "a1", "One");
        let err = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("missing")],
            "One",
            None,
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::ArtistNotFound));
    }

    #[test]
    fn merge_artists_rejects_various_artists() {
        let conn = merge_test_conn("various");
        insert_test_artist(&conn, "va", "Various Artists");
        insert_test_artist(&conn, "a2", "Two");
        let err = merge_artists(
            &conn,
            &[coerce_entity_id("va"), coerce_entity_id("a2")],
            "Two",
            None,
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::VariousArtistsNotMergeable));
    }

    #[test]
    fn merge_artists_rejects_master_id_not_in_selection() {
        let conn = merge_test_conn("invalid-master");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        insert_test_artist(&conn, "a3", "Three");
        let err = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a3")),
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::InvalidMaster));
    }

    #[test]
    fn merge_artists_rejects_already_merged_artist() {
        let conn = merge_test_conn("already-merged");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        insert_test_artist(&conn, "a3", "Three");
        merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        let err = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a3")],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::AlreadyMerged));
    }

    #[test]
    fn merge_artists_dedupes_repeated_ids() {
        let conn = merge_test_conn("dedupe");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        let result = merge_artists(
            &conn,
            &[
                coerce_entity_id("a1"),
                coerce_entity_id("a2"),
                coerce_entity_id("a1"),
            ],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        assert_eq!(result.name, "One");
    }

    #[test]
    fn merge_artists_moves_albums_tracks_styles_and_creates_alias() {
        let conn = merge_test_conn("happy-path");
        insert_test_library(&conn, "lib1");
        insert_test_artist(&conn, "a1", "Madonna");
        insert_test_artist(&conn, "a2", "Madonna Ciccone");
        insert_test_album(&conn, "alb1", "True Blue", "a1", "Madonna");
        insert_test_album(&conn, "alb2", "Erotica", "a2", "Madonna Ciccone");
        insert_test_track(&conn, "t1", "lib1", "a1", "alb1", "Papa Don't Preach");
        insert_test_track(&conn, "t2", "lib1", "a2", "alb2", "Deeper and Deeper");
        conn.execute(
            "INSERT INTO artist_styles(artist_id, style) VALUES('a1','pop')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO artist_styles(artist_id, style) VALUES('a2','dance')",
            [],
        )
        .unwrap();

        let result = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "Madonna",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();

        assert_eq!(result.name, "Madonna");
        assert_eq!(result.album_count, 2);
        assert_eq!(result.track_count, 2);
        // no external identity anywhere -> pending an online match, not yet locked
        assert_eq!(result.metadata_locked, Some(0));
        assert_eq!(result.identity_lock_pending, Some(1));

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM artists WHERE id='a2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0, "source artist row should be deleted");

        let styles = list_artist_radio_tags(&conn, &coerce_entity_id("a1")).unwrap();
        assert_eq!(styles, vec!["dance".to_string(), "pop".to_string()]);

        let alias_target: EntityId = conn
            .query_row(
                "SELECT artist_id FROM artist_name_aliases WHERE alias_name_normalized = 'madonna ciccone'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alias_target, coerce_entity_id("a1"));

        let info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(info.merged);
        assert_eq!(info.members.len(), 1);
        assert_eq!(info.members[0].original_name, "Madonna Ciccone");
        assert_eq!(info.members[0].album_count, 1);
        assert_eq!(info.members[0].track_count, 1);

        // A rescan resolving the old, pre-merge name must resolve back to the
        // master, not create a duplicate (§6.3 — exercised end-to-end here;
        // see jobs.rs for the focused upsert_artist unit tests).
        let resolved = crate::jobs::upsert_artist(&conn, "Madonna Ciccone").unwrap();
        assert_eq!(resolved, coerce_entity_id("a1"));
        let artist_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM artists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(artist_count, 1);

        // A scoped post-scan job was enqueued for the one-shot online match.
        let job_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM post_scan_jobs WHERE job_type='enrich_artist_external_ids'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(job_count, 1);
    }

    #[test]
    fn merge_artists_with_custom_name_creates_new_master_and_absorbs_both() {
        let conn = merge_test_conn("custom-name");
        insert_test_artist(&conn, "a1", "Prince");
        insert_test_artist(&conn, "a2", "TAFKAP");

        let result = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "The Artist",
            None,
            "user-1",
        )
        .unwrap();

        assert_eq!(result.name, "The Artist");
        let info = get_artist_merge_info(&conn, &result.id).unwrap();
        assert!(info.merged);
        assert_eq!(info.members.len(), 2);
        let mut names: Vec<_> = info
            .members
            .iter()
            .map(|m| m.original_name.clone())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Prince".to_string(), "TAFKAP".to_string()]);
    }

    #[test]
    fn merge_artists_keeps_masters_existing_identity_over_source() {
        let conn = merge_test_conn("identity-keep");
        insert_test_artist(&conn, "a1", "Beyonce");
        insert_test_artist(&conn, "a2", "Beyonce Knowles");
        conn.execute(
            "UPDATE artists SET deezer_artist_id='d-master', deezer_identity_checked_at='2020-01-01' WHERE id='a1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE artists SET deezer_artist_id='d-source', deezer_identity_checked_at='2021-01-01' WHERE id='a2'",
            [],
        )
        .unwrap();

        let result = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "Beyonce",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();

        assert_eq!(result.metadata_locked, Some(1));
        assert_eq!(result.identity_lock_pending, Some(0));
        let deezer: String = conn
            .query_row(
                "SELECT deezer_artist_id FROM artists WHERE id='a1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deezer, "d-master");
    }

    #[test]
    fn merge_artists_keeps_higher_rating_per_user() {
        let conn = merge_test_conn("ratings");
        conn.execute(
            "INSERT INTO users(id, username, role) VALUES('u1','tester','user')",
            [],
        )
        .unwrap();
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        conn.execute(
            "INSERT INTO artist_ratings(user_id, artist_id, rating) VALUES('u1','a1',3.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO artist_ratings(user_id, artist_id, rating) VALUES('u1','a2',4.5)",
            [],
        )
        .unwrap();

        merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a1")),
            "u1",
        )
        .unwrap();

        let rating: f64 = conn
            .query_row(
                "SELECT rating FROM artist_ratings WHERE user_id='u1' AND artist_id='a1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rating, 4.5);
    }

    #[test]
    fn unmerge_artists_splits_member_back_out_and_restores_moves() {
        let conn = merge_test_conn("unmerge-full");
        insert_test_library(&conn, "lib1");
        insert_test_artist(&conn, "a1", "Madonna");
        insert_test_artist(&conn, "a2", "Madonna Ciccone");
        insert_test_album(&conn, "alb2", "Erotica", "a2", "Madonna Ciccone");
        insert_test_track(&conn, "t2", "lib1", "a2", "alb2", "Deeper and Deeper");

        merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "Madonna",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        let info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        let member_id = info.members[0].id.clone();

        let result =
            unmerge_artists(&conn, &coerce_entity_id("a1"), &[member_id], "user-1").unwrap();
        assert_eq!(result.new_artist_ids.len(), 1);
        let new_id = &result.new_artist_ids[0];

        let restored_name: String = conn
            .query_row("SELECT name FROM artists WHERE id = ?1", [new_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(restored_name, "Madonna Ciccone");

        let album_owner: EntityId = conn
            .query_row("SELECT artist_id FROM albums WHERE id='alb2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(&album_owner, new_id);
        let track_owner: EntityId = conn
            .query_row("SELECT artist_id FROM tracks WHERE id='t2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(&track_owner, new_id);

        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artist_name_aliases WHERE alias_name_normalized='madonna ciccone'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alias_count, 0);

        let master = result
            .master
            .expect("master should survive (had a real prior identity)");
        assert_eq!(master.metadata_locked, Some(0));

        let merge_info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(!merge_info.merged);
    }

    #[test]
    fn unmerge_artists_deletes_synthetic_master_when_fully_dissolved() {
        let conn = merge_test_conn("unmerge-dissolve");
        insert_test_artist(&conn, "a1", "Prince");
        insert_test_artist(&conn, "a2", "TAFKAP");
        let merged = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "The Artist",
            None,
            "user-1",
        )
        .unwrap();
        let master_id = merged.id.clone();
        let info = get_artist_merge_info(&conn, &master_id).unwrap();
        let all_member_ids: Vec<EntityId> = info.members.iter().map(|m| m.id.clone()).collect();

        let result = unmerge_artists(&conn, &master_id, &all_member_ids, "user-1").unwrap();
        assert_eq!(result.new_artist_ids.len(), 2);
        assert!(
            result.master.is_none(),
            "a synthetic (custom-name) master with nothing left should be deleted, not kept"
        );

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artists WHERE id = ?1",
                [&master_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn unmerge_artists_partial_keeps_master_locked_with_remaining_members() {
        let conn = merge_test_conn("unmerge-partial");
        insert_test_artist(&conn, "a1", "Madonna");
        insert_test_artist(&conn, "a2", "Madonna Ciccone");
        insert_test_artist(&conn, "a3", "M.D.N.A.");
        merge_artists(
            &conn,
            &[
                coerce_entity_id("a1"),
                coerce_entity_id("a2"),
                coerce_entity_id("a3"),
            ],
            "Madonna",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();

        let info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        assert_eq!(info.members.len(), 2);
        let one_member = info.members[0].id.clone();

        let result =
            unmerge_artists(&conn, &coerce_entity_id("a1"), &[one_member], "user-1").unwrap();
        assert_eq!(result.new_artist_ids.len(), 1);
        assert!(result.master.is_some());

        let remaining_info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(remaining_info.merged);
        assert_eq!(remaining_info.members.len(), 1);
    }

    #[test]
    fn unmerge_artists_rejects_non_master() {
        let conn = merge_test_conn("unmerge-not-master");
        insert_test_artist(&conn, "a1", "Solo");
        let err = unmerge_artists(
            &conn,
            &coerce_entity_id("a1"),
            &[coerce_entity_id("whatever")],
            "user-1",
        )
        .unwrap_err();
        assert!(matches!(err, ArtistMergeError::NotAMergeMaster));
    }

    #[test]
    fn unmerge_artists_requires_member_ids() {
        let conn = merge_test_conn("unmerge-empty");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        let err = unmerge_artists(&conn, &coerce_entity_id("a1"), &[], "user-1").unwrap_err();
        assert!(matches!(err, ArtistMergeError::NoMembersSelected));
    }

    #[test]
    fn lock_artist_identity_locks_when_pending() {
        let conn = merge_test_conn("lock-pending");
        // A library must exist for the merge to have somewhere to scope its
        // one-shot enrichment job to — otherwise (§6.5's edge case) it locks
        // immediately instead of going pending, which isn't what this test
        // wants to exercise.
        insert_test_library(&conn, "lib1");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        let merged = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        assert_eq!(merged.identity_lock_pending, Some(1));

        let locked = lock_artist_identity(&conn, &coerce_entity_id("a1"), "user-1").unwrap();
        assert_eq!(locked.metadata_locked, Some(1));
        assert_eq!(locked.identity_lock_pending, Some(0));
    }

    #[test]
    fn merge_artists_locks_immediately_when_no_library_exists_to_scope_enrichment_to() {
        let conn = merge_test_conn("lock-no-library");
        insert_test_artist(&conn, "a1", "One");
        insert_test_artist(&conn, "a2", "Two");
        let merged = merge_artists(
            &conn,
            &[coerce_entity_id("a1"), coerce_entity_id("a2")],
            "One",
            Some(&coerce_entity_id("a1")),
            "user-1",
        )
        .unwrap();
        assert_eq!(
            merged.metadata_locked,
            Some(1),
            "nothing to scope a post-merge online lookup to — must lock immediately"
        );
        assert_eq!(merged.identity_lock_pending, Some(0));
    }

    #[test]
    fn lock_artist_identity_rejects_when_not_pending() {
        let conn = merge_test_conn("lock-not-pending");
        insert_test_artist(&conn, "a1", "Solo");
        let err = lock_artist_identity(&conn, &coerce_entity_id("a1"), "user-1").unwrap_err();
        assert!(matches!(err, ArtistMergeError::IdentityNotPending));
    }

    #[test]
    fn lock_artist_identity_rejects_unknown_artist() {
        let conn = merge_test_conn("lock-unknown");
        let err = lock_artist_identity(&conn, &coerce_entity_id("missing"), "user-1").unwrap_err();
        assert!(matches!(err, ArtistMergeError::ArtistNotFound));
    }

    #[test]
    fn finalize_pending_identity_lock_locks_on_hit_and_leaves_pending_on_miss() {
        let conn = merge_test_conn("finalize-lock");
        insert_test_artist(&conn, "a1", "One");
        conn.execute(
            "UPDATE artists SET identity_lock_pending = 1 WHERE id='a1'",
            [],
        )
        .unwrap();

        let locked = finalize_pending_identity_lock(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(!locked, "no identity yet — must not lock");
        let pending: i64 = conn
            .query_row(
                "SELECT identity_lock_pending FROM artists WHERE id='a1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);

        conn.execute("UPDATE artists SET deezer_artist_id='d1' WHERE id='a1'", [])
            .unwrap();
        let locked = finalize_pending_identity_lock(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(locked, "a provider match landed — must lock now");
        let (locked_flag, pending): (i64, i64) = conn
            .query_row(
                "SELECT metadata_locked, identity_lock_pending FROM artists WHERE id='a1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(locked_flag, 1);
        assert_eq!(pending, 0);
    }

    #[test]
    fn get_artist_merge_info_reports_unmerged_for_a_plain_artist() {
        let conn = merge_test_conn("merge-info-plain");
        insert_test_artist(&conn, "a1", "Solo");
        let info = get_artist_merge_info(&conn, &coerce_entity_id("a1")).unwrap();
        assert!(!info.merged);
        assert!(info.members.is_empty());
    }
}

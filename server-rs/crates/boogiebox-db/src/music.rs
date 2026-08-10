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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<String>,
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
        format!("ar.id, ar.name, MAX(arr.rating) AS rating, ar.metadata_locked, ar.description, {owned_counts_sql}")
    } else {
        format!("ar.id, ar.name, MAX(arr.rating) AS rating, NULL AS metadata_locked, NULL AS description, {owned_counts_sql}")
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
                description: row.get(4)?,
                track_count: row.get(5)?,
                album_count: row.get(6)?,
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
            "SELECT ar.id, ar.name, MAX(arr.rating) AS rating, ar.metadata_locked, ar.description,
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
                    description: row.get(4)?,
                    track_count: row.get(5)?,
                    album_count: row.get(6)?,
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
            description: row.get(4)?,
            track_count: row.get(5)?,
            album_count: row.get(6)?,
            play_count: Some(row.get(7)?),
            styles: Vec::new(),
        })
    })?
    .collect()
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
    let added_at_select = if has_added_at {
        "MIN(al.added_at)"
    } else {
        "MIN(t.scanned_at)"
    };

    let sql = format!(
        "SELECT MIN(al.id), al.title, al.album_artist, al.album_artist AS artist,
                MIN(al.year), al.genre, MAX(alr.rating), MIN(al.release_type),
                COUNT(t.id), ROUND(SUM(t.duration),0), {added_at_select} AS added_at, MAX(t.scanned_at) AS latest_scanned_at,
                MIN(al.label)
         FROM albums al JOIN tracks t ON t.album_id = al.id
         LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = ?
         GROUP BY al.title, COALESCE(al.album_artist,'')
         ORDER BY {sort_col} DESC, LOWER(COALESCE(al.album_artist,'')), al.title
         LIMIT ?"
    );

    conn.prepare(&sql)?
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

    let sql = format!(
        "SELECT {TRACK_COLS}, trr.rating
         FROM tracks t
         LEFT JOIN artists ar ON ar.id = t.artist_id
         JOIN albums al ON al.id = t.album_id
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
    conn.query_row(&sql, rusqlite::params![user_id, track_id], map_track)
        .optional()
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
}

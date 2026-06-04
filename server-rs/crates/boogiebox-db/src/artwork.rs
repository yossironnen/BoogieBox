//! Defines SQLite data access and schema helpers for Artwork.

use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

// ── Settings ──────────────────────────────────────────────────────────────────

/// Documents the Get Setting public API surface.
pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?",
        params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| {
        let v = v.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    })
}

// ── Provider usage ────────────────────────────────────────────────────────────

/// Public Provider Usage Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ProviderUsageRow {
    /// Documents the Provider public API surface.
    pub provider: String,
    /// Documents the Entity Type public API surface.
    pub entity_type: String,
    /// Documents the Usage Type public API surface.
    pub usage_type: String,
    /// Documents the Count public API surface.
    pub count: i64,
    /// Documents the Last Used At public API surface.
    pub last_used_at: String,
}

/// Public Provider Usage Provider Summary data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ProviderUsageProviderSummary {
    /// Documents the Provider public API surface.
    pub provider: String,
    /// Documents the Total Count public API surface.
    pub total_count: i64,
    /// Documents the Last Used At public API surface.
    pub last_used_at: Option<String>,
    /// Documents the Usage Breakdown public API surface.
    pub usage_breakdown: HashMap<String, i64>,
}

/// Public Provider Usage Snapshot data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ProviderUsageSnapshot {
    /// Documents the Fetched At public API surface.
    pub fetched_at: String,
    /// Documents the Providers public API surface.
    pub providers: Vec<ProviderUsageProviderSummary>,
    /// Documents the Rows public API surface.
    pub rows: Vec<ProviderUsageRow>,
}

/// Documents the Record Provider Usage public API surface.
pub fn record_provider_usage(
    conn: &Connection,
    provider: &str,
    entity_type: &str,
    usage_type: &str,
) {
    let provider = provider.trim().to_lowercase();
    let entity_type = entity_type.trim().to_lowercase();
    let usage_type = usage_type.trim().to_lowercase();
    if provider.is_empty() || entity_type.is_empty() || usage_type.is_empty() {
        return;
    }
    let _ = conn.execute(
        "INSERT INTO provider_usage_stats(provider, entity_type, usage_type, count, last_used_at) \
         VALUES(?, ?, ?, 1, datetime('now')) \
         ON CONFLICT(provider, entity_type, usage_type) DO UPDATE SET \
           count = provider_usage_stats.count + 1, \
           last_used_at = datetime('now')",
        params![provider, entity_type, usage_type],
    );
}

/// Documents the List Provider Usage public API surface.
pub fn list_provider_usage(conn: &Connection) -> ProviderUsageSnapshot {
    let rows: Vec<ProviderUsageRow> = conn
        .prepare(
            "SELECT provider, entity_type, usage_type, count, last_used_at \
             FROM provider_usage_stats \
             ORDER BY provider ASC, count DESC, entity_type ASC, usage_type ASC",
        )
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |r| {
                Ok(ProviderUsageRow {
                    provider: r.get(0)?,
                    entity_type: r.get(1)?,
                    usage_type: r.get(2)?,
                    count: r.get(3)?,
                    last_used_at: r.get(4)?,
                })
            })
            .ok()
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
        })
        .unwrap_or_default();

    let mut provider_map: HashMap<String, ProviderUsageProviderSummary> = HashMap::new();
    for row in &rows {
        let entry = provider_map.entry(row.provider.clone()).or_insert_with(|| {
            ProviderUsageProviderSummary {
                provider: row.provider.clone(),
                total_count: 0,
                last_used_at: None,
                usage_breakdown: HashMap::new(),
            }
        });
        entry.total_count += row.count;
        *entry
            .usage_breakdown
            .entry(row.usage_type.clone())
            .or_default() += row.count;
        if entry.last_used_at.as_deref() < Some(row.last_used_at.as_str()) {
            entry.last_used_at = Some(row.last_used_at.clone());
        }
    }

    let mut providers: Vec<ProviderUsageProviderSummary> = provider_map.into_values().collect();
    providers.sort_by(|a, b| {
        b.total_count
            .cmp(&a.total_count)
            .then(a.provider.cmp(&b.provider))
    });

    let fetched_at = conn
        .query_row("SELECT datetime('now')", [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|_| "unknown".to_string());

    ProviderUsageSnapshot {
        fetched_at,
        providers,
        rows,
    }
}

// ── Album art ─────────────────────────────────────────────────────────────────

/// Documents the Album Art Row public API surface.
pub struct AlbumArtRow {
    /// Documents the Title public API surface.
    pub title: String,
    /// Documents the Artist public API surface.
    pub artist: String,
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the Metadata Locked public API surface.
    pub metadata_locked: bool,
}

/// Documents the Get Album For Art public API surface.
pub fn get_album_for_art(conn: &Connection, album_id: &str) -> Option<AlbumArtRow> {
    conn.query_row(
        "SELECT al.title, \
                COALESCE(NULLIF(al.album_artist, ''), ar.name, '') AS artist, \
                t.file_path, COALESCE(al.metadata_locked, 0) \
         FROM albums al \
         LEFT JOIN artists ar ON ar.id = al.artist_id \
         JOIN tracks t ON t.album_id = al.id \
         WHERE al.id = ? \
         ORDER BY t.track_number ASC, t.id ASC LIMIT 1",
        params![album_id],
        |r| {
            Ok(AlbumArtRow {
                title: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                artist: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                file_path: r.get(2)?,
                metadata_locked: r.get::<_, i64>(3)? != 0,
            })
        },
    )
    .ok()
}

/// Documents the Set Album Metadata Locked public API surface.
pub fn set_album_metadata_locked(conn: &Connection, album_id: &str) {
    let _ = conn.execute(
        "UPDATE albums SET metadata_locked = 1 WHERE id = ?",
        params![album_id],
    );
}

// ── Artist art ────────────────────────────────────────────────────────────────

/// Documents the Artist Art Row public API surface.
pub struct ArtistArtRow {
    /// Documents the Name public API surface.
    pub name: String,
    /// Documents the Metadata Locked public API surface.
    pub metadata_locked: bool,
}

/// Documents the Get Artist For Art public API surface.
pub fn get_artist_for_art(conn: &Connection, artist_id: &str) -> Option<ArtistArtRow> {
    conn.query_row(
        "SELECT name, COALESCE(metadata_locked, 0) FROM artists WHERE id = ?",
        params![artist_id],
        |r| {
            Ok(ArtistArtRow {
                name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                metadata_locked: r.get::<_, i64>(1)? != 0,
            })
        },
    )
    .ok()
}

/// Documents the Set Artist Metadata Locked public API surface.
pub fn set_artist_metadata_locked(conn: &Connection, artist_id: &str) {
    let _ = conn.execute(
        "UPDATE artists SET metadata_locked = 1 WHERE id = ?",
        params![artist_id],
    );
}

// ── Lyrics ────────────────────────────────────────────────────────────────────

/// Documents the Track For Lyrics public API surface.
pub struct TrackForLyrics {
    /// Documents the Artist public API surface.
    pub artist: String,
    /// Documents the Title public API surface.
    pub title: String,
}

/// Documents the Get Track For Lyrics public API surface.
pub fn get_track_for_lyrics(conn: &Connection, track_id: &str) -> Option<TrackForLyrics> {
    conn.query_row(
        "SELECT COALESCE(ar.name, ''), COALESCE(t.title, '') \
         FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id WHERE t.id = ?",
        params![track_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .ok()
    .and_then(|(artist, title)| {
        let a = artist.trim().to_string();
        let t = title.trim().to_string();
        if a.is_empty() || t.is_empty() {
            None
        } else {
            Some(TrackForLyrics {
                artist: a,
                title: t,
            })
        }
    })
}

/// Documents the Upsert Cached Lyrics public API surface.
pub fn upsert_cached_lyrics(
    conn: &Connection,
    track_id: &str,
    artist: &str,
    title: &str,
    lyrics: &str,
    source: &str,
    synced_json: Option<&str>,
) {
    let _ = conn.execute(
        "INSERT INTO lyrics_cache(track_id, artist, title, lyrics, synced_lyrics, source, \
                                  fetched_at, updated_at) \
         VALUES(?, ?, ?, ?, ?, ?, datetime('now'), datetime('now')) \
         ON CONFLICT(track_id) DO UPDATE SET \
           artist=excluded.artist, title=excluded.title, lyrics=excluded.lyrics, \
           synced_lyrics=excluded.synced_lyrics, source=excluded.source, \
           fetched_at=excluded.fetched_at, updated_at=excluded.updated_at",
        params![
            track_id,
            artist.trim(),
            title.trim(),
            lyrics,
            synced_json,
            source
        ],
    );
}

// ── Last.fm cache ─────────────────────────────────────────────────────────────

/// Documents the Get Lastfm Cached public API surface.
pub fn get_lastfm_cached(conn: &Connection, cache_key: &str) -> Option<String> {
    conn.query_row(
        "SELECT data FROM lastfm_cache WHERE cache_key = ? AND expires_at > datetime('now')",
        params![cache_key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Documents the Save Lastfm Cache public API surface.
pub fn save_lastfm_cache(conn: &Connection, cache_key: &str, data: &str, expires_days: i64) {
    let _ = conn.execute(
        "INSERT INTO lastfm_cache(cache_key, data, fetched_at, expires_at) \
         VALUES(?, ?, datetime('now'), datetime('now', '+' || ? || ' days')) \
         ON CONFLICT(cache_key) DO UPDATE SET \
           data=excluded.data, \
           fetched_at=excluded.fetched_at, \
           expires_at=excluded.expires_at",
        params![cache_key, data, expires_days],
    );
}

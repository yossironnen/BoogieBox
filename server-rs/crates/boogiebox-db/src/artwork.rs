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

/// Returns the album's stored record label, if any non-empty value is set.
pub fn get_album_label(conn: &Connection, album_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT label FROM albums WHERE id = ?",
        params![album_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .filter(|s| !s.trim().is_empty())
}

/// Stores the record label for an album.
pub fn set_album_label(conn: &Connection, album_id: &str, label: &str) {
    let _ = conn.execute(
        "UPDATE albums SET label = ? WHERE id = ?",
        params![label, album_id],
    );
}

/// Returns the album's stored release year, if a plausible one is set.
pub fn get_album_year(conn: &Connection, album_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT year FROM albums WHERE id = ?",
        params![album_id],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .filter(|y| *y > 0)
}

/// Stores the provider-resolved release year for an album.
pub fn set_album_year(conn: &Connection, album_id: &str, year: i64) {
    let _ = conn.execute(
        "UPDATE albums SET year = ? WHERE id = ?",
        params![year, album_id],
    );
}

/// Returns the album's stored release_type, if set to a non-default value.
pub fn get_album_release_type(conn: &Connection, album_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT release_type FROM albums WHERE id = ?",
        params![album_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .filter(|s| !s.trim().is_empty() && s != "album")
}

/// Stores the auto-classified release_type for an album.
pub fn set_album_release_type(conn: &Connection, album_id: &str, release_type: &str) {
    let _ = conn.execute(
        "UPDATE albums SET release_type = ? WHERE id = ?",
        params![release_type, album_id],
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

/// Returns a Last.fm/provider cache payload regardless of expiry so discovery
/// routes can degrade to stale data when a remote provider is unavailable.
pub fn get_lastfm_cached_stale(conn: &Connection, cache_key: &str) -> Option<String> {
    conn.query_row(
        "SELECT data FROM lastfm_cache WHERE cache_key = ?",
        params![cache_key],
        |row| row.get::<_, String>(0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_db;
    use std::time::SystemTime;
    use uuid::Uuid;

    #[test]
    fn stale_provider_cache_is_available_only_through_explicit_fallback() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE lastfm_cache(
               cache_key TEXT PRIMARY KEY, data TEXT NOT NULL,
               fetched_at TEXT NOT NULL, expires_at TEXT NOT NULL
             );
             INSERT INTO lastfm_cache(cache_key, data, fetched_at, expires_at)
               VALUES('similar:test', '[1]', datetime('now','-2 days'), datetime('now','-1 day'));",
        )
        .unwrap();
        assert_eq!(get_lastfm_cached(&conn, "similar:test"), None);
        assert_eq!(
            get_lastfm_cached_stale(&conn, "similar:test").as_deref(),
            Some("[1]")
        );
    }

    struct Fixture {
        conn: Connection,
        artist_id: String,
        album_id: String,
        track_id: String,
    }

    fn fixture(prefix: &str) -> Fixture {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("artwork-db-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = init_db(&dir).unwrap().connection;

        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, ?, 'Lib')",
            params![library_id, dir.to_string_lossy()],
        )
        .unwrap();
        let artist_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Test Artist')",
            params![artist_id],
        )
        .unwrap();
        let album_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Test Album', ?)",
            params![album_id, artist_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) \
             VALUES (?, ?, ?, ?, 'Test Track', ?)",
            params![
                track_id,
                library_id,
                artist_id,
                album_id,
                format!("/music/{track_id}.mp3")
            ],
        )
        .unwrap();

        Fixture {
            conn,
            artist_id,
            album_id,
            track_id,
        }
    }

    #[test]
    fn get_setting_returns_none_for_missing_key() {
        let f = fixture("get-setting");
        assert!(get_setting(&f.conn, "noSuchSetting").is_none());
        // Seeded to an empty string by default (see seed_default_settings in
        // lib.rs) — an empty value reads back as None, same as an unset key.
        assert!(get_setting(&f.conn, "lastfmKey").is_none());
        f.conn
            .execute(
                "UPDATE settings SET value = 'abc123' WHERE key = 'lastfmKey'",
                [],
            )
            .unwrap();
        assert_eq!(get_setting(&f.conn, "lastfmKey").as_deref(), Some("abc123"));
    }

    #[test]
    fn record_and_list_provider_usage() {
        let f = fixture("provider-usage");
        record_provider_usage(&f.conn, "discogs", "album_art", "fetch");
        record_provider_usage(&f.conn, "discogs", "album_art", "fetch");
        record_provider_usage(&f.conn, "deezer", "artist_art", "fetch");
        let snapshot = list_provider_usage(&f.conn);
        assert_eq!(snapshot.providers.len(), 2);
        assert!(!snapshot.rows.is_empty());
    }

    #[test]
    fn album_label_year_and_release_type_round_trip_and_reject_defaults() {
        let f = fixture("album-metadata");
        assert!(get_album_label(&f.conn, &f.album_id).is_none());
        set_album_label(&f.conn, &f.album_id, "Real Label Records");
        assert_eq!(
            get_album_label(&f.conn, &f.album_id).as_deref(),
            Some("Real Label Records")
        );

        assert!(get_album_year(&f.conn, &f.album_id).is_none());
        set_album_year(&f.conn, &f.album_id, 1999);
        assert_eq!(get_album_year(&f.conn, &f.album_id), Some(1999));
        set_album_year(&f.conn, &f.album_id, 0);
        assert!(
            get_album_year(&f.conn, &f.album_id).is_none(),
            "year 0 is not plausible and must read back as None"
        );

        // Default release_type "album" (set by the schema) reads back as None —
        // only a non-default classification is considered "set".
        assert!(get_album_release_type(&f.conn, &f.album_id).is_none());
        set_album_release_type(&f.conn, &f.album_id, "compilation");
        assert_eq!(
            get_album_release_type(&f.conn, &f.album_id).as_deref(),
            Some("compilation")
        );
    }

    #[test]
    fn album_and_artist_art_lookup_and_metadata_lock() {
        let f = fixture("art-lookup");
        let album = get_album_for_art(&f.conn, &f.album_id).expect("album should be found");
        assert_eq!(album.title, "Test Album");
        assert!(!album.metadata_locked);
        set_album_metadata_locked(&f.conn, &f.album_id);
        let locked_album = get_album_for_art(&f.conn, &f.album_id).unwrap();
        assert!(locked_album.metadata_locked);

        assert!(get_album_for_art(&f.conn, "does-not-exist").is_none());

        let artist = get_artist_for_art(&f.conn, &f.artist_id).expect("artist should be found");
        assert_eq!(artist.name, "Test Artist");
        assert!(!artist.metadata_locked);
        set_artist_metadata_locked(&f.conn, &f.artist_id);
        let locked_artist = get_artist_for_art(&f.conn, &f.artist_id).unwrap();
        assert!(locked_artist.metadata_locked);

        assert!(get_artist_for_art(&f.conn, "does-not-exist").is_none());
    }

    #[test]
    fn get_track_for_lyrics_requires_both_artist_and_title() {
        let f = fixture("track-lyrics-lookup");
        let info = get_track_for_lyrics(&f.conn, &f.track_id).expect("should resolve artist+title");
        assert_eq!(info.artist, "Test Artist");
        assert_eq!(info.title, "Test Track");

        assert!(get_track_for_lyrics(&f.conn, "does-not-exist").is_none());
    }

    #[test]
    fn upsert_cached_lyrics_inserts_then_updates() {
        let f = fixture("upsert-lyrics");
        upsert_cached_lyrics(
            &f.conn,
            &f.track_id,
            "Test Artist",
            "Test Track",
            "la la la",
            "lrclib",
            None,
        );
        let lyrics: String = f
            .conn
            .query_row(
                "SELECT lyrics FROM lyrics_cache WHERE track_id = ?",
                params![f.track_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lyrics, "la la la");

        upsert_cached_lyrics(
            &f.conn,
            &f.track_id,
            "Test Artist",
            "Test Track",
            "updated lyrics",
            "ovh",
            Some("[]"),
        );
        let (updated_lyrics, source): (String, String) = f
            .conn
            .query_row(
                "SELECT lyrics, source FROM lyrics_cache WHERE track_id = ?",
                params![f.track_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(updated_lyrics, "updated lyrics");
        assert_eq!(source, "ovh");

        let count: i64 = f
            .conn
            .query_row("SELECT COUNT(*) FROM lyrics_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "same track_id must update, not duplicate");
    }

    #[test]
    fn lastfm_cache_save_and_get_round_trips_and_respects_expiry() {
        let f = fixture("lastfm-cache");
        assert!(get_lastfm_cached(&f.conn, "artist:test").is_none());
        save_lastfm_cache(&f.conn, "artist:test", "{\"a\":1}", 7);
        assert_eq!(
            get_lastfm_cached(&f.conn, "artist:test").as_deref(),
            Some("{\"a\":1}")
        );

        // Simulate a cache entry that has passed its TTL: get_lastfm_cached
        // excludes it, but get_lastfm_cached_stale (used to degrade gracefully
        // when a provider is unavailable) still finds it regardless of expiry.
        save_lastfm_cache(&f.conn, "artist:expired", "{\"b\":2}", 7);
        f.conn
            .execute(
                "UPDATE lastfm_cache SET expires_at = datetime('now', '-1 day') WHERE cache_key = 'artist:expired'",
                [],
            )
            .unwrap();
        assert!(get_lastfm_cached(&f.conn, "artist:expired").is_none());
        assert_eq!(
            get_lastfm_cached_stale(&f.conn, "artist:expired").as_deref(),
            Some("{\"b\":2}")
        );
    }
}

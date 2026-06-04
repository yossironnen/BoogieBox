//! Defines SQLite data access and schema helpers for Playback.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::music::EntityId;

// ── Track streaming ───────────────────────────────────────────────────────────

/// Public Track Stream Row data shape used by BoogieBox.
#[derive(Debug)]
pub struct TrackStreamRow {
    /// Documents the Id public API surface.
    pub id: String,
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the Format public API surface.
    pub format: Option<String>,
    /// Documents the Duration public API surface.
    pub duration: Option<f64>,
}

/// Documents the Get Track For Stream public API surface.
pub fn get_track_for_stream(
    conn: &Connection,
    track_id: &str,
) -> rusqlite::Result<Option<TrackStreamRow>> {
    conn.query_row(
        "SELECT id, file_path, format, duration FROM tracks WHERE id = ?",
        params![track_id],
        |row| {
            Ok(TrackStreamRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                format: row.get(2)?,
                duration: row.get(3)?,
            })
        },
    )
    .optional()
}

// ── Play count / history ──────────────────────────────────────────────────────

/// Documents the Increment Track Play Count public API surface.
pub fn increment_track_play_count(conn: &Connection, track_id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "UPDATE tracks SET play_count = COALESCE(play_count, 0) + 1, last_played_at = datetime('now') WHERE id = ?",
        params![track_id],
    )?;
    if changed > 0 {
        let _ = conn.execute(
            "UPDATE artists SET play_count = COALESCE(play_count, 0) + 1 WHERE id = (SELECT artist_id FROM tracks WHERE id = ?)",
            params![track_id],
        );
    }
    Ok(changed > 0)
}

/// Documents the Insert Play History public API surface.
pub fn insert_play_history(
    conn: &Connection,
    user_id: &str,
    track_id: &str,
) -> rusqlite::Result<()> {
    let id = Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO play_history(id, user_id, track_id) VALUES(?, ?, ?)",
        params![id, user_id, track_id],
    )?;
    Ok(())
}

/// Public History Entry Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct HistoryEntryRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the Played At public API surface.
    pub played_at: String,
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Album public API surface.
    pub album: Option<String>,
}

/// Documents the List User History public API surface.
pub fn list_user_history(
    conn: &Connection,
    user_id: &str,
    limit: i64,
) -> rusqlite::Result<Vec<HistoryEntryRow>> {
    let limit = limit.clamp(1, 200);
    let mut stmt = conn.prepare(
        "SELECT ph.id, ph.track_id, ph.played_at,
                t.title, ar.name AS artist, al.title AS album
         FROM play_history ph
         JOIN tracks t ON t.id = ph.track_id
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE ph.user_id = ?
         ORDER BY ph.played_at DESC
         LIMIT ?",
    )?;

    let rows = stmt
        .query_map(params![user_id, limit], |row| {
            Ok(HistoryEntryRow {
                id: row.get(0)?,
                track_id: row.get(1)?,
                played_at: row.get(2)?,
                title: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })?
        .collect();
    rows
}

// ── Lyrics ────────────────────────────────────────────────────────────────────

/// Public Track Lyrics Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLyricsRow {
    /// Documents the Lyrics public API surface.
    pub lyrics: String,
    /// Raw JSON string — parse to Value in the route layer if needed
    /// Documents the Synced Lyrics public API surface.
    pub synced_lyrics: Option<String>,
    /// Documents the Source public API surface.
    pub source: Option<String>,
}

/// Documents the Get Track Lyrics Cached public API surface.
pub fn get_track_lyrics_cached(
    conn: &Connection,
    track_id: &str,
) -> rusqlite::Result<Option<TrackLyricsRow>> {
    // Primary: look up by track_id
    let row = conn
        .query_row(
            "SELECT lyrics, synced_lyrics, source FROM lyrics_cache \
             WHERE track_id = ? AND TRIM(COALESCE(lyrics, '')) != ''",
            params![track_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;

    if let Some((lyrics, synced_lyrics, source)) = row {
        return Ok(Some(TrackLyricsRow {
            lyrics,
            synced_lyrics,
            source,
        }));
    }

    // Fallback: look up by artist + title from the track row
    let track_info = conn
        .query_row(
            "SELECT COALESCE(ar.name, ''), COALESCE(t.title, '') \
             FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id WHERE t.id = ?",
            params![track_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;

    if let Some((artist, title)) = track_info {
        if !artist.trim().is_empty() && !title.trim().is_empty() {
            let row2 = conn
                .query_row(
                    "SELECT lyrics, synced_lyrics, source FROM lyrics_cache \
                     WHERE LOWER(TRIM(artist)) = LOWER(TRIM(?)) \
                       AND LOWER(TRIM(title))  = LOWER(TRIM(?)) \
                       AND TRIM(COALESCE(lyrics, '')) != ''",
                    params![artist, title],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((lyrics, synced_lyrics, source)) = row2 {
                return Ok(Some(TrackLyricsRow {
                    lyrics,
                    synced_lyrics,
                    source,
                }));
            }
        }
    }

    Ok(None)
}

// ── Waveforms ─────────────────────────────────────────────────────────────────

/// Public Track Waveform Row data shape used by BoogieBox.
#[derive(Debug)]
pub struct TrackWaveformRow {
    /// Documents the Track Id public API surface.
    pub track_id: String,
    /// Documents the Sample Count public API surface.
    pub sample_count: i64,
    /// Documents the Duration Seconds public API surface.
    pub duration_seconds: Option<f64>,
    /// Raw JSON array string, e.g. "[12,34,…]"
    /// Documents the Waveform Json public API surface.
    pub waveform_json: String,
    /// Documents the Version public API surface.
    pub version: i64,
    /// Documents the Updated At public API surface.
    pub updated_at: String,
}

/// Documents the Get Track Waveform public API surface.
pub fn get_track_waveform(
    conn: &Connection,
    track_id: &str,
) -> rusqlite::Result<Option<TrackWaveformRow>> {
    conn.query_row(
        "SELECT track_id, sample_count, duration_seconds, waveform_json, version, updated_at \
         FROM track_waveforms WHERE track_id = ?",
        params![track_id],
        |r| {
            Ok(TrackWaveformRow {
                track_id: r.get(0)?,
                sample_count: r.get(1)?,
                duration_seconds: r.get(2)?,
                waveform_json: r.get(3)?,
                version: r.get(4)?,
                updated_at: r.get(5)?,
            })
        },
    )
    .optional()
}

/// Documents the Save Track Waveform public API surface.
pub fn save_track_waveform(
    conn: &Connection,
    track_id: &str,
    sample_count: i64,
    duration_seconds: Option<f64>,
    waveform_json: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO track_waveforms(track_id, sample_count, duration_seconds, waveform_json, version, updated_at) \
         VALUES(?, ?, ?, ?, 1, datetime('now')) \
         ON CONFLICT(track_id) DO UPDATE SET \
           sample_count=excluded.sample_count, \
           duration_seconds=excluded.duration_seconds, \
           waveform_json=excluded.waveform_json, \
           version=excluded.version, \
           updated_at=excluded.updated_at",
        params![track_id, sample_count, duration_seconds, waveform_json],
    )?;
    Ok(())
}

/// Documents the List Tracks Missing Waveforms public API surface.
pub fn list_tracks_missing_waveforms(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<TrackStreamRow>> {
    let limit = limit.clamp(1, 1000);
    let mut stmt = conn.prepare(
        "SELECT t.id, t.file_path, t.format, t.duration
         FROM tracks t
         LEFT JOIN track_waveforms tw ON tw.track_id = t.id
         WHERE tw.track_id IS NULL
           AND TRIM(COALESCE(t.file_path, '')) != ''
         ORDER BY t.id
         LIMIT ?",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(TrackStreamRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                format: row.get(2)?,
                duration: row.get(3)?,
            })
        })?
        .collect();
    rows
}

/// Documents the Mark Waveform Map Run Complete public API surface.
pub fn mark_waveform_map_run_complete(
    conn: &Connection,
    frequency_hours: f64,
) -> rusqlite::Result<()> {
    let hours = frequency_hours.clamp(1.0, 8760.0);
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('waveformBackgroundLastRun', datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('waveformBackgroundNextRun', datetime('now', ?))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![format!("+{hours} hours")],
    )?;
    Ok(())
}

/// Public Waveform Settings data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformSettings {
    /// Documents the Generate On Missing public API surface.
    pub generate_on_missing: bool,
    /// Documents the Background Enabled public API surface.
    pub background_enabled: bool,
    /// Documents the Frequency Hours public API surface.
    pub frequency_hours: f64,
    /// Documents the Batch Size public API surface.
    pub batch_size: i64,
    /// Documents the Last Run public API surface.
    pub last_run: Option<String>,
    /// Documents the Next Run public API surface.
    pub next_run: Option<String>,
}

/// Documents the Get Waveform Settings public API surface.
pub fn get_waveform_settings(conn: &Connection) -> WaveformSettings {
    let keys = [
        "waveformGenerateOnMissing",
        "waveformBackgroundEnabled",
        "waveformBackgroundFrequencyHours",
        "waveformBackgroundBatchSize",
        "waveformBackgroundLastRun",
        "waveformBackgroundNextRun",
    ];
    let mut map = std::collections::HashMap::new();
    for key in &keys {
        if let Ok(Some(val)) = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
        {
            map.insert(*key, val);
        }
    }
    WaveformSettings {
        generate_on_missing: map
            .get("waveformGenerateOnMissing")
            .map(|v| v == "true")
            .unwrap_or(false),
        background_enabled: map
            .get("waveformBackgroundEnabled")
            .map(|v| v == "true")
            .unwrap_or(false),
        frequency_hours: map
            .get("waveformBackgroundFrequencyHours")
            .and_then(|v| v.parse().ok())
            .unwrap_or(24.0),
        batch_size: map
            .get("waveformBackgroundBatchSize")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100),
        last_run: map
            .get("waveformBackgroundLastRun")
            .cloned()
            .filter(|v| !v.is_empty()),
        next_run: map
            .get("waveformBackgroundNextRun")
            .cloned()
            .filter(|v| !v.is_empty()),
    }
}

/// Public Waveform Map Status data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformMapStatus {
    /// Documents the Enabled public API surface.
    pub enabled: bool,
    /// Documents the Generate On Missing public API surface.
    pub generate_on_missing: bool,
    /// Documents the Frequency Hours public API surface.
    pub frequency_hours: f64,
    /// Documents the Batch Size public API surface.
    pub batch_size: i64,
    /// Documents the Last Run public API surface.
    pub last_run: Option<String>,
    /// Documents the Next Run public API surface.
    pub next_run: Option<String>,
    /// Documents the In Progress public API surface.
    pub in_progress: bool,
    /// Documents the Total Tracks public API surface.
    pub total_tracks: i64,
    /// Documents the Mapped Tracks public API surface.
    pub mapped_tracks: i64,
    /// Documents the Missing Tracks public API surface.
    pub missing_tracks: i64,
}

/// Documents the Get Waveform Map Status public API surface.
pub fn get_waveform_map_status(conn: &Connection) -> rusqlite::Result<WaveformMapStatus> {
    let settings = get_waveform_settings(conn);
    let total_tracks: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0);
    let mapped_tracks: i64 = conn
        .query_row("SELECT COUNT(*) FROM track_waveforms", [], |r| r.get(0))
        .unwrap_or(0);
    let missing_tracks = (total_tracks - mapped_tracks).max(0);
    Ok(WaveformMapStatus {
        enabled: settings.background_enabled,
        generate_on_missing: settings.generate_on_missing,
        frequency_hours: settings.frequency_hours,
        batch_size: settings.batch_size,
        last_run: settings.last_run,
        next_run: settings.next_run,
        in_progress: false,
        total_tracks,
        mapped_tracks,
        missing_tracks,
    })
}

// ── BPM analysis ──────────────────────────────────────────────────────────────

/// Public Bpm Analysis Status data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmAnalysisStatus {
    /// Documents the Enabled public API surface.
    pub enabled: bool,
    /// Documents the Background Enabled public API surface.
    pub background_enabled: bool,
    /// Documents the Frequency Hours public API surface.
    pub frequency_hours: f64,
    /// Documents the Last Run public API surface.
    pub last_run: Option<String>,
    /// Documents the Next Run public API surface.
    pub next_run: Option<String>,
    /// Documents the Spotify Fallback Enabled public API surface.
    pub spotify_fallback_enabled: bool,
    /// Documents the Total Tracks public API surface.
    pub total_tracks: i64,
    /// Documents the Analyzed Tracks public API surface.
    pub analyzed_tracks: i64,
    /// Documents the Missing Tracks public API surface.
    pub missing_tracks: i64,
    /// Documents the In Progress public API surface.
    pub in_progress: bool,
    /// Documents the Active Run public API surface.
    pub active_run: Option<BpmActiveRun>,
}

/// Public Bpm Active Run data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmActiveRun {
    /// Documents the Processed public API surface.
    pub processed: i64,
    /// Documents the Analyzed public API surface.
    pub analyzed: i64,
    /// Documents the Skipped public API surface.
    pub skipped: i64,
    /// Documents the Errors public API surface.
    pub errors: i64,
}

/// Public Bpm Settings data shape used by BoogieBox.
#[derive(Debug)]
pub struct BpmSettings {
    /// Documents the Enabled public API surface.
    pub enabled: bool,
    /// Documents the Background Enabled public API surface.
    pub background_enabled: bool,
    /// Documents the Frequency Hours public API surface.
    pub frequency_hours: f64,
    /// Documents the Last Run public API surface.
    pub last_run: Option<String>,
    /// Documents the Next Run public API surface.
    pub next_run: Option<String>,
}

/// Documents the Get Bpm Analysis Status public API surface.
pub fn get_bpm_analysis_status(conn: &Connection) -> rusqlite::Result<BpmAnalysisStatus> {
    let settings = get_bpm_settings(conn);
    let total_tracks: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0);
    let analyzed_tracks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks
             WHERE COALESCE(bpm_detected, 0) > 0 OR COALESCE(bpm, 0) > 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(BpmAnalysisStatus {
        enabled: settings.enabled,
        background_enabled: settings.background_enabled,
        frequency_hours: settings.frequency_hours,
        last_run: settings.last_run,
        next_run: settings.next_run,
        spotify_fallback_enabled: spotify_configured(conn),
        total_tracks,
        analyzed_tracks,
        missing_tracks: (total_tracks - analyzed_tracks).max(0),
        in_progress: false,
        active_run: None,
    })
}

/// Documents the Get Bpm Settings public API surface.
pub fn get_bpm_settings(conn: &Connection) -> BpmSettings {
    let keys = [
        "bpmAnalysisEnabled",
        "bpmBackgroundEnabled",
        "bpmBackgroundFrequencyHours",
        "bpmBackgroundLastRun",
        "bpmBackgroundNextRun",
    ];
    let mut map = std::collections::HashMap::new();
    for key in &keys {
        if let Ok(Some(val)) = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
        {
            map.insert(*key, val);
        }
    }

    BpmSettings {
        enabled: map
            .get("bpmAnalysisEnabled")
            .map(|v| v == "true")
            .unwrap_or(false),
        background_enabled: map
            .get("bpmBackgroundEnabled")
            .map(|v| v == "true")
            .unwrap_or(false),
        frequency_hours: map
            .get("bpmBackgroundFrequencyHours")
            .and_then(|v| v.parse().ok())
            .unwrap_or(24.0),
        last_run: map
            .get("bpmBackgroundLastRun")
            .cloned()
            .filter(|v| !v.is_empty()),
        next_run: map
            .get("bpmBackgroundNextRun")
            .cloned()
            .filter(|v| !v.is_empty()),
    }
}

fn spotify_configured(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'spotifyClientId'",
        [],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .is_some_and(|v| !v.trim().is_empty())
}

/// Documents the List Tracks Missing Bpm public API surface.
pub fn list_tracks_missing_bpm(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<TrackStreamRow>> {
    let limit = limit.clamp(1, 1000);
    let mut stmt = conn.prepare(
        "SELECT id, file_path, format, duration
         FROM tracks
         WHERE (bpm_detected IS NULL OR bpm_detected <= 0)
           AND (bpm IS NULL OR bpm <= 0)
           AND TRIM(COALESCE(file_path, '')) != ''
         ORDER BY id
         LIMIT ?",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(TrackStreamRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                format: row.get(2)?,
                duration: row.get(3)?,
            })
        })?
        .collect();
    rows
}

/// Documents the Save Track Bpm Detected public API surface.
pub fn save_track_bpm_detected(
    conn: &Connection,
    track_id: &str,
    bpm: f64,
    source: &str,
    confidence: f64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks
         SET bpm_detected = ?1,
             bpm_source = ?2,
             bpm_confidence = ?3,
             bpm_analyzed_at = datetime('now')
         WHERE id = ?4",
        params![bpm, source, confidence.clamp(0.0, 1.0), track_id],
    )?;
    Ok(())
}

/// Documents the Mark Bpm Analysis Run Complete public API surface.
pub fn mark_bpm_analysis_run_complete(
    conn: &Connection,
    frequency_hours: f64,
) -> rusqlite::Result<()> {
    let hours = frequency_hours.clamp(1.0, 8760.0);
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('bpmBackgroundLastRun', datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('bpmBackgroundNextRun', datetime('now', ?))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![format!("+{hours} hours")],
    )?;
    Ok(())
}

// ── EQ profile ────────────────────────────────────────────────────────────────

/// Public Eq Profile Resolution data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqProfileResolution {
    /// Documents the Eq Profile public API surface.
    pub eq_profile: String,
    /// Documents the Source public API surface.
    pub source: String,
}

const GENRE_EQ_MAP: &[(&str, &str)] = &[
    ("jazz", "Jazz"),
    ("classical", "Classical"),
    ("electronic", "Electronic"),
    ("edm", "Electronic"),
    ("dance", "Electronic"),
    ("hip-hop", "Hip-Hop"),
    ("hip hop", "Hip-Hop"),
    ("rap", "Hip-Hop"),
    ("pop", "Pop"),
    ("r&b", "R&B"),
    ("soul", "R&B"),
    ("country", "Country"),
    ("rock", "Rock"),
    ("metal", "Rock"),
    ("punk", "Rock"),
    ("folk", "Acoustic"),
    ("acoustic", "Acoustic"),
    ("blues", "Blues"),
    ("reggae", "Reggae"),
    ("latin", "Latin"),
];

fn genre_to_eq_profile(genre: &str) -> Option<&'static str> {
    let lower = genre.to_lowercase();
    GENRE_EQ_MAP
        .iter()
        .find(|(key, _)| lower.contains(key))
        .map(|(_, profile)| *profile)
}

/// Documents the Get Track Eq Profile public API surface.
pub fn get_track_eq_profile(
    conn: &Connection,
    track_id: &str,
) -> rusqlite::Result<EqProfileResolution> {
    let track_info = conn
        .query_row(
            "SELECT COALESCE(ar.name, ''), COALESCE(t.genre, ''), COALESCE(ar.genres, '') \
             FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id WHERE t.id = ?",
            params![track_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;

    let (artist_name, track_genre, artist_genres) = match track_info {
        None => {
            return Ok(EqProfileResolution {
                eq_profile: "Rock".into(),
                source: "default".into(),
            })
        }
        Some(v) => v,
    };

    // Check artist_eq_cache (table may not exist on all DB versions)
    if !artist_name.trim().is_empty() {
        let cached = conn
            .query_row(
                "SELECT eq_profile FROM artist_eq_cache \
                 WHERE LOWER(TRIM(artist)) = LOWER(TRIM(?)) LIMIT 1",
                params![artist_name],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .unwrap_or(None);
        if let Some(profile) = cached.filter(|p| !p.is_empty()) {
            return Ok(EqProfileResolution {
                eq_profile: profile,
                source: "artist_eq_cache".into(),
            });
        }
    }

    // Track genre tag
    if let Some(profile) = genre_to_eq_profile(track_genre.trim()) {
        return Ok(EqProfileResolution {
            eq_profile: profile.to_string(),
            source: "track_tags".into(),
        });
    }

    // Artist genres (comma-separated or JSON array)
    let genres: Vec<String> = if artist_genres.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<String>>(&artist_genres).unwrap_or_default()
    } else {
        artist_genres
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };
    for g in &genres {
        if let Some(profile) = genre_to_eq_profile(g) {
            return Ok(EqProfileResolution {
                eq_profile: profile.to_string(),
                source: "artist_tags".into(),
            });
        }
    }

    Ok(EqProfileResolution {
        eq_profile: "Rock".into(),
        source: "default".into(),
    })
}

// ── Crossfade ─────────────────────────────────────────────────────────────────

const VALID_CF_ENTITY_TYPES: &[&str] = &["album", "playlist", "autodj"];
const VALID_CF_MODES: &[&str] = &["off", "zerogap", "crossfade"];

/// Public Crossfade Config data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossfadeConfig {
    /// Documents the Mode public API surface.
    pub mode: String,
    /// Documents the Duration public API surface.
    pub duration: i64,
    /// Documents the Source public API surface.
    pub source: String,
}

/// Public Crossfade Override data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossfadeOverride {
    /// Documents the Entity Type public API surface.
    pub entity_type: String,
    /// Documents the Entity Id public API surface.
    pub entity_id: String,
    /// Documents the Mode public API surface.
    pub mode: String,
    /// Documents the Duration public API surface.
    pub duration: i64,
}

/// Documents the Get Crossfade Config public API surface.
pub fn get_crossfade_config(
    conn: &Connection,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
) -> rusqlite::Result<CrossfadeConfig> {
    if let (Some(etype), Some(eid)) = (entity_type, entity_id) {
        if VALID_CF_ENTITY_TYPES.contains(&etype) && !eid.trim().is_empty() {
            let ov = conn
                .query_row(
                    "SELECT mode, duration FROM crossfade_overrides WHERE entity_type=? AND entity_id=?",
                    params![etype, eid],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?;
            if let Some((mode, duration)) = ov {
                return Ok(CrossfadeConfig {
                    mode,
                    duration,
                    source: "override".into(),
                });
            }
        }
    }

    let mode = conn
        .query_row(
            "SELECT value FROM settings WHERE key='crossfadeMode'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| "off".into());
    let duration: i64 = conn
        .query_row(
            "SELECT value FROM settings WHERE key='crossfadeDuration'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    Ok(CrossfadeConfig {
        mode,
        duration,
        source: "global".into(),
    })
}

/// Documents the Get Crossfade Overrides public API surface.
pub fn get_crossfade_overrides(
    conn: &Connection,
    entity_type: Option<&str>,
) -> rusqlite::Result<Vec<CrossfadeOverride>> {
    let filtered = entity_type.filter(|t| VALID_CF_ENTITY_TYPES.contains(t));
    let rows: Vec<CrossfadeOverride> = if let Some(etype) = filtered {
        let mut stmt = conn.prepare(
            "SELECT entity_type, entity_id, mode, duration \
             FROM crossfade_overrides WHERE entity_type=?",
        )?;
        let r = stmt
            .query_map(params![etype], map_cf_override)?
            .collect::<rusqlite::Result<_>>()?;
        r
    } else {
        let mut stmt =
            conn.prepare("SELECT entity_type, entity_id, mode, duration FROM crossfade_overrides")?;
        let r = stmt
            .query_map([], map_cf_override)?
            .collect::<rusqlite::Result<_>>()?;
        r
    };
    Ok(rows)
}

fn map_cf_override(row: &rusqlite::Row<'_>) -> rusqlite::Result<CrossfadeOverride> {
    Ok(CrossfadeOverride {
        entity_type: row.get(0)?,
        entity_id: row.get(1)?,
        mode: row.get(2)?,
        duration: row.get(3)?,
    })
}

/// Public Crossfade Upsert Error data shape used by BoogieBox.
#[derive(Debug)]
pub enum CrossfadeUpsertError {
    InvalidEntityType,
    InvalidMode,
    InvalidDuration,
    Db(rusqlite::Error),
}

impl std::fmt::Display for CrossfadeUpsertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEntityType => {
                write!(f, "entity_type must be one of: album, playlist, autodj")
            }
            Self::InvalidMode => write!(f, "mode must be one of: off, zerogap, crossfade"),
            Self::InvalidDuration => write!(f, "duration must be an integer between 1 and 10"),
            Self::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

fn crossfade_id_column_is_text(conn: &Connection) -> bool {
    conn.prepare("PRAGMA table_info(crossfade_overrides)")
        .and_then(|mut stmt| {
            let rows: Vec<(String, String)> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows
                .iter()
                .find(|(col, _)| col.eq_ignore_ascii_case("id"))
                .is_none_or(|(_, typ)| !typ.to_uppercase().contains("INT")))
        })
        .unwrap_or(false)
}

/// Documents the Upsert Crossfade Override public API surface.
pub fn upsert_crossfade_override(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    mode: &str,
    duration: i64,
) -> Result<(), CrossfadeUpsertError> {
    if !VALID_CF_ENTITY_TYPES.contains(&entity_type) {
        return Err(CrossfadeUpsertError::InvalidEntityType);
    }
    if entity_id.trim().is_empty() {
        return Err(CrossfadeUpsertError::InvalidDuration);
    }
    if !VALID_CF_MODES.contains(&mode) {
        return Err(CrossfadeUpsertError::InvalidMode);
    }
    if !(1..=10).contains(&duration) {
        return Err(CrossfadeUpsertError::InvalidDuration);
    }

    // Delete existing row (works for both schemas)
    conn.execute(
        "DELETE FROM crossfade_overrides WHERE entity_type=? AND entity_id=?",
        params![entity_type, entity_id],
    )
    .map_err(CrossfadeUpsertError::Db)?;

    if crossfade_id_column_is_text(conn) {
        let id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO crossfade_overrides(id, entity_type, entity_id, mode, duration) \
             VALUES(?, ?, ?, ?, ?)",
            params![id, entity_type, entity_id, mode, duration],
        )
        .map_err(CrossfadeUpsertError::Db)?;
    } else {
        conn.execute(
            "INSERT INTO crossfade_overrides(entity_type, entity_id, mode, duration) \
             VALUES(?, ?, ?, ?)",
            params![entity_type, entity_id, mode, duration],
        )
        .map_err(CrossfadeUpsertError::Db)?;
    }

    Ok(())
}

/// Documents the Delete Crossfade Override public API surface.
pub fn delete_crossfade_override(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM crossfade_overrides WHERE entity_type=? AND entity_id=?",
        params![entity_type, entity_id],
    )?;
    Ok(())
}

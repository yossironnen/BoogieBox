//! Track loudness for levelling: ReplayGain values read from tags at scan time, or measured by the
//! loudness lane, plus album gain computed from measured tracks. Plan: `wip/loudness-levelling-plan.md`.

use crate::music::EntityId;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// ReplayGain 2.0 reference loudness; every stored gain is relative to it.
pub const REFERENCE_LUFS: f64 = -18.0;
/// Measurement method version; rows measured with an older one are measured again.
pub const MEASURE_VERSION: i64 = 1;

/// ReplayGain values found in a file's tags (gains in dB relative to −18 LUFS, peaks linear).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoudnessTags {
    /// Track gain; a tag row needs at least this.
    pub track_gain_db: Option<f64>,
    /// Track sample peak.
    pub track_peak: Option<f64>,
    /// Album gain.
    pub album_gain_db: Option<f64>,
    /// Album sample peak.
    pub album_peak: Option<f64>,
}

/// One FFmpeg `ebur128` result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// Integrated loudness in LUFS; `None` for digital silence.
    pub integrated_lufs: Option<f64>,
    /// True peak in dBTP.
    pub true_peak_dbtp: Option<f64>,
    /// Loudness range in LU.
    pub lra: Option<f64>,
}

/// The values a player needs for one track.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLoudness {
    /// Track gain in dB.
    pub track_gain_db: f64,
    /// Track peak, linear.
    pub track_peak: Option<f64>,
    /// Album gain in dB.
    pub album_gain_db: Option<f64>,
    /// Album peak, linear.
    pub album_peak: Option<f64>,
}

/// Positive gain allowed on a server transcode when the peak is unknown.
pub const MAX_GAIN_WITHOUT_PEAK_DB: f64 = 6.0;

/// Room left under full scale: the gain never lifts the peak above −1 dBTP.
pub const PEAK_HEADROOM_DB: f64 = 1.0;

/// Gain a server path applies for `mode` (`off`, `track`, `album`, `auto`): album gain in album mode,
/// or in auto mode with `album_context`; never lifts the peak above −1 dBTP (capped at +6 dB without one). Server
/// paths have no pre-amp. Mirrors `pickGainDb` in `client/src/audio/loudness.ts`.
pub fn applied_gain_db(mode: &str, data: Option<&TrackLoudness>, album_context: bool) -> f64 {
    let Some(data) = data.filter(|_| mode != "off") else {
        return 0.0;
    };
    let use_album =
        (mode == "album" || (mode == "auto" && album_context)) && data.album_gain_db.is_some();
    let (gain, peak) = if use_album {
        (
            data.album_gain_db.unwrap_or(data.track_gain_db),
            data.album_peak.or(data.track_peak),
        )
    } else {
        (data.track_gain_db, data.track_peak)
    };
    let applied = match peak.filter(|p| *p > 0.0) {
        Some(peak) => gain.min(-PEAK_HEADROOM_DB - 20.0 * peak.log10()),
        None => gain.min(MAX_GAIN_WITHOUT_PEAK_DB),
    };
    (applied * 100.0).round() / 100.0
}

/// Library-wide progress of the loudness data.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessStatus {
    /// Tracks with values from their tags.
    pub tagged: i64,
    /// Tracks measured by the lane.
    pub measured: i64,
    /// Tracks still waiting to be measured.
    pub remaining: i64,
}

/// Migration `2026-11-01-track-loudness`: the loudness table, mix loudness columns, and the old
/// `replayGainEnabled` setting carried over to every user's `loudnessMode`.
pub fn ensure_loudness_schema(conn: &Connection) -> rusqlite::Result<()> {
    if crate::table_exists(conn, "tracks") {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS track_loudness (
               track_id        TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
               track_gain_db   REAL,
               track_peak      REAL,
               album_gain_db   REAL,
               album_peak      REAL,
               integrated_lufs REAL,
               true_peak_dbtp  REAL,
               lra             REAL,
               source          TEXT NOT NULL CHECK (source IN ('tag','analysis')),
               album_source    TEXT CHECK (album_source IN ('tag','computed')),
               file_size       INTEGER NOT NULL,
               analyzed_at     TEXT NOT NULL,
               version         INTEGER NOT NULL DEFAULT 1
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS idx_track_loudness_source
               ON track_loudness(source, analyzed_at);",
        )?;
    }
    if crate::table_exists(conn, "mix_outputs") {
        for column in ["integrated_lufs", "true_peak_dbtp"] {
            if !crate::column_exists(conn, "mix_outputs", column)? {
                conn.execute_batch(&format!("ALTER TABLE mix_outputs ADD COLUMN {column} REAL"))?;
            }
        }
    }
    let replay_gain_on = crate::table_exists(conn, "settings")
        && conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'replayGainEnabled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some("true");
    if replay_gain_on && crate::table_exists(conn, "users") {
        conn.execute(
            "INSERT OR IGNORE INTO user_settings(user_id, key, value, updated_at)
             SELECT id, 'loudnessMode', 'auto', datetime('now') FROM users",
            [],
        )?;
    }
    Ok(())
}

/// Stores (or clears) one scanned file's tag values. Tags replace a measured row; a file that lost
/// its tags, or whose size changed since it was measured, loses its row so the lane measures it.
pub fn record_scanned_tags(
    conn: &Connection,
    track_id: &EntityId,
    file_size: i64,
    tags: Option<&LoudnessTags>,
) -> rusqlite::Result<()> {
    let Some(tags) = tags.filter(|tags| tags.track_gain_db.is_some()) else {
        let removed = conn.execute(
            "DELETE FROM track_loudness
             WHERE track_id = ?1 AND (source = 'tag' OR file_size != ?2)",
            params![track_id, file_size],
        )?;
        if removed > 0 {
            reset_computed_album_gain(conn, track_id)?;
        }
        return Ok(());
    };
    let album_source = tags.album_gain_db.map(|_| "tag");
    let changed = conn.execute(
        "INSERT INTO track_loudness(track_id, track_gain_db, track_peak, album_gain_db, album_peak,
                                    source, album_source, file_size, analyzed_at, version)
         VALUES (?1, ?2, ?3, ?4, ?5, 'tag', ?6, ?7, datetime('now'), ?8)
         ON CONFLICT(track_id) DO UPDATE SET
           track_gain_db = excluded.track_gain_db,
           track_peak = excluded.track_peak,
           album_gain_db = CASE WHEN excluded.album_source IS NOT NULL THEN excluded.album_gain_db
                                WHEN track_loudness.album_source = 'computed' THEN track_loudness.album_gain_db END,
           album_peak = CASE WHEN excluded.album_source IS NOT NULL THEN excluded.album_peak
                             WHEN track_loudness.album_source = 'computed' THEN track_loudness.album_peak END,
           album_source = CASE WHEN excluded.album_source IS NOT NULL THEN excluded.album_source
                               WHEN track_loudness.album_source = 'computed' THEN 'computed' END,
           integrated_lufs = NULL, true_peak_dbtp = NULL, lra = NULL,
           source = 'tag',
           file_size = excluded.file_size,
           analyzed_at = excluded.analyzed_at,
           version = excluded.version
         WHERE track_loudness.source != 'tag'
            OR track_loudness.track_gain_db IS NOT excluded.track_gain_db
            OR track_loudness.track_peak IS NOT excluded.track_peak
            OR (excluded.album_source IS NOT NULL
                AND (track_loudness.album_gain_db IS NOT excluded.album_gain_db
                     OR track_loudness.album_peak IS NOT excluded.album_peak
                     OR track_loudness.album_source IS NOT 'tag'))
            OR (excluded.album_source IS NULL AND track_loudness.album_source = 'tag')
            OR track_loudness.file_size != excluded.file_size",
        params![
            track_id,
            tags.track_gain_db,
            tags.track_peak,
            tags.album_gain_db,
            tags.album_peak,
            album_source,
            file_size,
            MEASURE_VERSION,
        ],
    )?;
    if changed > 0 && album_source.is_none() {
        reset_computed_album_gain(conn, track_id)?;
    }
    Ok(())
}

/// Clears computed album gain on `track_id`'s album, so it is computed again from its current tracks.
fn reset_computed_album_gain(conn: &Connection, track_id: &EntityId) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE track_loudness SET album_gain_db = NULL, album_peak = NULL, album_source = NULL
         WHERE album_source = 'computed'
           AND track_id IN (SELECT id FROM tracks
                            WHERE album_id = (SELECT album_id FROM tracks WHERE id = ?1))",
        [track_id],
    )?;
    Ok(())
}

/// Up to `limit` tracks the lane should measure: no row yet, or measured by an older method.
/// `skip` holds ids that already failed in this run.
pub fn tracks_needing_measurement(
    conn: &Connection,
    limit: usize,
    skip: &std::collections::HashSet<String>,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.id, t.file_path FROM tracks t
         LEFT JOIN track_loudness l ON l.track_id = t.id
         WHERE l.track_id IS NULL OR (l.source = 'analysis' AND l.version < ?1)
         ORDER BY t.added_at, t.id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![MEASURE_VERSION, (limit + skip.len()) as i64],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if !skip.contains(&row.0) {
            out.push(row);
        }
        if out.len() == limit {
            break;
        }
    }
    Ok(out)
}

/// Stores measured results. A row that gained tags meanwhile is left alone.
pub fn record_measurements(
    conn: &Connection,
    rows: &[(String, Measurement)],
) -> rusqlite::Result<usize> {
    let mut written = 0;
    for (track_id, m) in rows {
        let gain = m.integrated_lufs.map(|lufs| REFERENCE_LUFS - lufs);
        let peak = m.true_peak_dbtp.map(|db| 10_f64.powf(db / 20.0));
        let changed = conn.execute(
            "INSERT INTO track_loudness(track_id, track_gain_db, track_peak, integrated_lufs,
                                        true_peak_dbtp, lra, source, file_size, analyzed_at, version)
             SELECT id, ?2, ?3, ?4, ?5, ?6, 'analysis', file_size, datetime('now'), ?7
             FROM tracks WHERE id = ?1
             ON CONFLICT(track_id) DO UPDATE SET
               track_gain_db = excluded.track_gain_db, track_peak = excluded.track_peak,
               integrated_lufs = excluded.integrated_lufs, true_peak_dbtp = excluded.true_peak_dbtp,
               lra = excluded.lra, file_size = excluded.file_size,
               analyzed_at = excluded.analyzed_at, version = excluded.version,
               album_gain_db = NULL, album_peak = NULL, album_source = NULL
             WHERE track_loudness.source = 'analysis'",
            params![
                track_id,
                gain,
                peak,
                m.integrated_lufs,
                m.true_peak_dbtp,
                m.lra,
                MEASURE_VERSION
            ],
        )?;
        if changed > 0 {
            reset_computed_album_gain(conn, &EntityId::Str(track_id.clone()))?;
            written += 1;
        }
    }
    Ok(written)
}

/// Computes album gain for up to `limit` albums whose tracks all have a row and some lack album
/// gain: the duration-weighted power mean of the track loudness, peak = loudest track peak.
pub fn compute_album_gains(conn: &Connection, limit: usize) -> rusqlite::Result<usize> {
    let albums: Vec<String> = {
        let mut stmt = conn.prepare_cached(
            "SELECT t.album_id FROM tracks t
             LEFT JOIN track_loudness l ON l.track_id = t.id
             WHERE t.album_id IS NOT NULL
             GROUP BY t.album_id
             HAVING SUM(l.track_id IS NULL) = 0
                AND SUM(l.album_source IS NULL AND l.track_gain_db IS NOT NULL) > 0
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for album_id in &albums {
        let tracks: Vec<(f64, Option<f64>, f64)> = {
            let mut stmt = conn.prepare_cached(
                "SELECT l.track_gain_db, l.track_peak, t.duration FROM tracks t
                 JOIN track_loudness l ON l.track_id = t.id
                 WHERE t.album_id = ?1 AND l.track_gain_db IS NOT NULL",
            )?;
            let rows = stmt.query_map([album_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                ))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let Some((gain, peak)) = album_gain(&tracks) else {
            continue;
        };
        conn.execute(
            "UPDATE track_loudness SET album_gain_db = ?2, album_peak = ?3, album_source = 'computed'
             WHERE album_source IS NULL AND track_gain_db IS NOT NULL
               AND track_id IN (SELECT id FROM tracks WHERE album_id = ?1)",
            params![album_id, gain, peak],
        )?;
    }
    Ok(albums.len())
}

/// Album gain from `(track gain dB, track peak, duration s)` rows; tracks without a duration count
/// as one second so they still weigh in.
pub fn album_gain(tracks: &[(f64, Option<f64>, f64)]) -> Option<(f64, Option<f64>)> {
    if tracks.is_empty() {
        return None;
    }
    let (mut energy, mut weight) = (0.0, 0.0);
    for (gain, _, duration) in tracks {
        let d = if *duration > 0.0 { *duration } else { 1.0 };
        energy += d * 10_f64.powf((REFERENCE_LUFS - gain) / 10.0);
        weight += d;
    }
    let lufs = 10.0 * (energy / weight).log10();
    let peak = tracks
        .iter()
        .filter_map(|(_, peak, _)| *peak)
        .fold(None, |max: Option<f64>, p| {
            Some(max.map_or(p, |m| m.max(p)))
        });
    Some((REFERENCE_LUFS - lufs, peak))
}

/// Loudness for the given track ids; tracks without a usable value are left out.
pub fn loudness_for_tracks(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<HashMap<String, TrackLoudness>> {
    let mut out = HashMap::new();
    let mut stmt = conn.prepare_cached(
        "SELECT track_gain_db, track_peak, album_gain_db, album_peak FROM track_loudness
         WHERE track_id = ?1 AND track_gain_db IS NOT NULL",
    )?;
    for id in ids {
        let row = stmt
            .query_row([id], |row| {
                Ok(TrackLoudness {
                    track_gain_db: row.get(0)?,
                    track_peak: row.get(1)?,
                    album_gain_db: row.get(2)?,
                    album_peak: row.get(3)?,
                })
            })
            .optional()?;
        if let Some(row) = row {
            out.insert(id.clone(), row);
        }
    }
    Ok(out)
}

/// Loudness of a rendered BoogieMix, as a track gain.
pub fn mix_loudness(conn: &Connection, mix_id: &str) -> rusqlite::Result<Option<TrackLoudness>> {
    let row: Option<(Option<f64>, Option<f64>)> = conn
        .query_row(
            "SELECT integrated_lufs, true_peak_dbtp FROM mix_outputs WHERE id = ?1",
            [mix_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(lufs, peak)| {
        lufs.map(|lufs| TrackLoudness {
            track_gain_db: REFERENCE_LUFS - lufs,
            track_peak: peak.map(|db| 10_f64.powf(db / 20.0)),
            album_gain_db: None,
            album_peak: None,
        })
    }))
}

/// Stores a rendered mix's measured loudness.
pub fn record_mix_loudness(
    conn: &Connection,
    mix_id: &str,
    integrated_lufs: f64,
    true_peak_dbtp: Option<f64>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE mix_outputs SET integrated_lufs = ?2, true_peak_dbtp = ?3 WHERE id = ?1",
        params![mix_id, integrated_lufs, true_peak_dbtp],
    )?;
    Ok(())
}

/// Up to `limit` rendered mixes without a loudness value, as `(id, file_path)`.
pub fn mixes_needing_measurement(
    conn: &Connection,
    limit: usize,
    skip: &std::collections::HashSet<String>,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, file_path FROM mix_outputs WHERE integrated_lufs IS NULL
         ORDER BY created_at LIMIT ?1",
    )?;
    let rows = stmt.query_map([(limit + skip.len()) as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if !skip.contains(&row.0) && out.len() < limit {
            out.push(row);
        }
    }
    Ok(out)
}

/// Library-wide counts for the admin status.
pub fn loudness_status(conn: &Connection) -> rusqlite::Result<LoudnessStatus> {
    conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM track_loudness WHERE source = 'tag'),
           (SELECT COUNT(*) FROM track_loudness WHERE source = 'analysis' AND version >= ?1),
           (SELECT COUNT(*) FROM tracks t LEFT JOIN track_loudness l ON l.track_id = t.id
             WHERE l.track_id IS NULL OR (l.source = 'analysis' AND l.version < ?1))",
        [MEASURE_VERSION],
        |row| {
            Ok(LoudnessStatus {
                tagged: row.get(0)?,
                measured: row.get(1)?,
                remaining: row.get(2)?,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE users (id TEXT PRIMARY KEY);
             CREATE TABLE user_settings (user_id TEXT, key TEXT, value TEXT, updated_at TEXT,
               PRIMARY KEY (user_id, key));
             CREATE TABLE tracks (id TEXT PRIMARY KEY, album_id TEXT, file_path TEXT,
               file_size INTEGER, duration REAL, added_at TEXT);
             CREATE TABLE mix_outputs (id TEXT PRIMARY KEY, file_path TEXT, created_at TEXT);
             INSERT INTO tracks VALUES
               ('t1', 'a1', '/m/1.flac', 100, 200, '1'),
               ('t2', 'a1', '/m/2.flac', 100, 100, '2'),
               ('t3', 'a2', '/m/3.flac', 100, 300, '3');",
        )
        .unwrap();
        ensure_loudness_schema(&conn).unwrap();
        conn
    }

    fn id(value: &str) -> EntityId {
        EntityId::Str(value.into())
    }

    fn tags(track: f64, album: Option<f64>) -> LoudnessTags {
        LoudnessTags {
            track_gain_db: Some(track),
            track_peak: Some(0.9),
            album_gain_db: album,
            album_peak: album.map(|_| 0.95),
        }
    }

    /// `(source, track gain, album gain, album source)`.
    type Row = (String, Option<f64>, Option<f64>, Option<String>);

    fn row(conn: &Connection, track: &str) -> Option<Row> {
        conn.query_row(
            "SELECT source, track_gain_db, album_gain_db, album_source FROM track_loudness WHERE track_id = ?1",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .unwrap()
    }

    fn measured(lufs: f64) -> Measurement {
        Measurement {
            integrated_lufs: Some(lufs),
            true_peak_dbtp: Some(-1.0),
            lra: Some(6.0),
        }
    }

    #[test]
    fn tags_are_stored_replace_a_measurement_and_go_when_the_tags_go() {
        let conn = db();
        record_measurements(&conn, &[("t1".into(), measured(-10.0))]).unwrap();
        assert_eq!(row(&conn, "t1").unwrap().1, Some(-8.0));

        record_scanned_tags(&conn, &id("t1"), 100, Some(&tags(-6.5, Some(-7.0)))).unwrap();
        assert_eq!(
            row(&conn, "t1"),
            Some(("tag".into(), Some(-6.5), Some(-7.0), Some("tag".into())))
        );
        // A later measurement never overwrites tag values.
        record_measurements(&conn, &[("t1".into(), measured(-20.0))]).unwrap();
        assert_eq!(row(&conn, "t1").unwrap().1, Some(-6.5));

        record_scanned_tags(&conn, &id("t1"), 100, None).unwrap();
        assert_eq!(row(&conn, "t1"), None);
        record_scanned_tags(&conn, &id("t1"), 100, Some(&LoudnessTags::default())).unwrap();
        assert_eq!(row(&conn, "t1"), None);
    }

    #[test]
    fn a_size_change_drops_a_measurement_but_not_tags() {
        let conn = db();
        record_measurements(&conn, &[("t1".into(), measured(-12.0))]).unwrap();
        record_scanned_tags(&conn, &id("t1"), 100, None).unwrap();
        assert!(
            row(&conn, "t1").is_some(),
            "same size keeps the measurement"
        );
        record_scanned_tags(&conn, &id("t1"), 999, None).unwrap();
        assert_eq!(row(&conn, "t1"), None);

        record_scanned_tags(&conn, &id("t2"), 100, Some(&tags(-3.0, None))).unwrap();
        record_scanned_tags(&conn, &id("t2"), 555, Some(&tags(-3.0, None))).unwrap();
        let size: i64 = conn
            .query_row(
                "SELECT file_size FROM track_loudness WHERE track_id='t2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(size, 555);
    }

    #[test]
    fn a_rescan_with_the_same_tags_writes_nothing() {
        let conn = db();
        record_scanned_tags(&conn, &id("t1"), 100, Some(&tags(-6.5, Some(-7.0)))).unwrap();
        conn.execute("UPDATE track_loudness SET analyzed_at = 'old'", [])
            .unwrap();
        record_scanned_tags(&conn, &id("t1"), 100, Some(&tags(-6.5, Some(-7.0)))).unwrap();
        let at: String = conn
            .query_row("SELECT analyzed_at FROM track_loudness", [], |r| r.get(0))
            .unwrap();
        assert_eq!(at, "old");
    }

    #[test]
    fn measures_missing_tracks_and_skips_failures() {
        let conn = db();
        record_scanned_tags(&conn, &id("t2"), 100, Some(&tags(-1.0, None))).unwrap();
        let none = HashSet::new();
        let todo = tracks_needing_measurement(&conn, 10, &none).unwrap();
        assert_eq!(
            todo.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
            ["t1", "t3"]
        );
        let skip: HashSet<String> = ["t1".to_string()].into();
        let todo = tracks_needing_measurement(&conn, 1, &skip).unwrap();
        assert_eq!(todo, [("t3".to_string(), "/m/3.flac".to_string())]);

        record_measurements(&conn, &[("t1".into(), measured(-9.0))]).unwrap();
        conn.execute(
            "UPDATE track_loudness SET version = 0 WHERE track_id = 't1'",
            [],
        )
        .unwrap();
        let todo = tracks_needing_measurement(&conn, 10, &none).unwrap();
        assert_eq!(todo.len(), 2, "an older method is measured again");
        assert_eq!(
            loudness_status(&conn).unwrap(),
            LoudnessStatus {
                tagged: 1,
                measured: 0,
                remaining: 2
            }
        );
    }

    #[test]
    fn silence_is_stored_without_a_gain_and_left_out_of_lookups() {
        let conn = db();
        let silent = Measurement {
            integrated_lufs: None,
            true_peak_dbtp: None,
            lra: None,
        };
        record_measurements(&conn, &[("t3".into(), silent)]).unwrap();
        assert!(tracks_needing_measurement(&conn, 10, &HashSet::new())
            .unwrap()
            .iter()
            .all(|r| r.0 != "t3"));
        assert!(loudness_for_tracks(&conn, &["t3".into()])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn album_gain_is_the_duration_weighted_power_mean() {
        // Two equal tracks: the album equals them.
        let (gain, peak) =
            album_gain(&[(-6.0, Some(0.5), 100.0), (-6.0, Some(0.8), 100.0)]).unwrap();
        assert!((gain + 6.0).abs() < 1e-9);
        assert_eq!(peak, Some(0.8));
        // −8 LUFS for 200 s and −18 LUFS for 100 s: 10·log10((2·10^-0.8 + 10^-1.8)/3) ≈ −9.55 LUFS.
        let (gain, _) = album_gain(&[(-10.0, None, 200.0), (0.0, None, 100.0)]).unwrap();
        assert!((gain - (-18.0 + 9.549)).abs() < 0.01, "{gain}");
        assert_eq!(album_gain(&[]), None);
        assert_eq!(album_gain(&[(-3.0, None, 0.0)]).unwrap().0, -3.0);
    }

    #[test]
    fn album_gain_waits_for_every_track_and_resets_when_the_album_changes() {
        let conn = db();
        record_measurements(&conn, &[("t1".into(), measured(-12.0))]).unwrap();
        assert_eq!(
            compute_album_gains(&conn, 10).unwrap(),
            0,
            "t2 not measured yet"
        );
        record_scanned_tags(&conn, &id("t2"), 100, Some(&tags(-6.0, None))).unwrap();
        compute_album_gains(&conn, 10).unwrap();
        let (_, _, album, source) = row(&conn, "t1").unwrap();
        assert_eq!(source.as_deref(), Some("computed"));
        assert!((album.unwrap() + 6.0).abs() < 1e-9);
        assert_eq!(row(&conn, "t2").unwrap().2, album);
        assert_eq!(
            compute_album_gains(&conn, 10).unwrap(),
            0,
            "nothing left to do"
        );

        // A rescan with unchanged tags keeps the computed album gain.
        record_scanned_tags(&conn, &id("t2"), 100, Some(&tags(-6.0, None))).unwrap();
        assert_eq!(row(&conn, "t2").unwrap().3.as_deref(), Some("computed"));
        // New tag values on one track reset the album, and it is computed again.
        record_scanned_tags(&conn, &id("t2"), 100, Some(&tags(-2.0, None))).unwrap();
        assert_eq!(row(&conn, "t1").unwrap().3, None);
        compute_album_gains(&conn, 10).unwrap();
        assert_eq!(row(&conn, "t1").unwrap().3.as_deref(), Some("computed"));

        let found = loudness_for_tracks(&conn, &["t1".into(), "t2".into(), "nope".into()]).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found["t1"].album_gain_db.is_some());
    }

    #[test]
    fn server_gain_follows_mode_album_context_and_peak() {
        let data = TrackLoudness {
            track_gain_db: -8.0,
            track_peak: Some(0.9),
            album_gain_db: Some(-6.0),
            album_peak: Some(1.0),
        };
        assert_eq!(applied_gain_db("off", Some(&data), true), 0.0);
        assert_eq!(applied_gain_db("track", None, false), 0.0);
        assert_eq!(applied_gain_db("track", Some(&data), true), -8.0);
        assert_eq!(applied_gain_db("album", Some(&data), false), -6.0);
        assert_eq!(applied_gain_db("auto", Some(&data), true), -6.0);
        assert_eq!(applied_gain_db("auto", Some(&data), false), -8.0);
        let quiet = TrackLoudness {
            track_gain_db: 9.0,
            track_peak: Some(0.5),
            album_gain_db: None,
            album_peak: None,
        };
        assert_eq!(applied_gain_db("album", Some(&quiet), true), 5.02);
        let no_peak = TrackLoudness {
            track_peak: None,
            ..quiet
        };
        assert_eq!(applied_gain_db("track", Some(&no_peak), false), 6.0);
    }

    #[test]
    fn mixes_get_a_track_gain_from_their_measurement() {
        let conn = db();
        conn.execute(
            "INSERT INTO mix_outputs(id, file_path, created_at) VALUES ('m1', '/mix/1.mp3', '1')",
            [],
        )
        .unwrap();
        assert_eq!(mix_loudness(&conn, "m1").unwrap(), None);
        assert_eq!(
            mixes_needing_measurement(&conn, 5, &HashSet::new())
                .unwrap()
                .len(),
            1
        );
        record_mix_loudness(&conn, "m1", -16.0, Some(-1.5)).unwrap();
        let mix = mix_loudness(&conn, "m1").unwrap().unwrap();
        assert!((mix.track_gain_db + 2.0).abs() < 1e-9);
        assert!(mixes_needing_measurement(&conn, 5, &HashSet::new())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn migration_carries_replay_gain_over_to_every_user_and_reruns_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE users (id TEXT PRIMARY KEY);
             CREATE TABLE user_settings (user_id TEXT, key TEXT, value TEXT, updated_at TEXT,
               PRIMARY KEY (user_id, key));
             INSERT INTO users VALUES ('u1'), ('u2');
             INSERT INTO user_settings VALUES ('u2', 'loudnessMode', 'track', 'x');
             INSERT INTO settings VALUES ('replayGainEnabled', 'true');",
        )
        .unwrap();
        ensure_loudness_schema(&conn).unwrap();
        ensure_loudness_schema(&conn).unwrap();
        let modes: Vec<(String, String)> = conn
            .prepare("SELECT user_id, value FROM user_settings ORDER BY user_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            modes,
            [("u1".into(), "auto".into()), ("u2".into(), "track".into())]
        );

        let off = db();
        let count: i64 = off
            .query_row("SELECT COUNT(*) FROM user_settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // A legacy database without the tables is fine.
        ensure_loudness_schema(&Connection::open_in_memory().unwrap()).unwrap();
    }
}

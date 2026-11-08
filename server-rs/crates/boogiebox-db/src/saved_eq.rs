//! Per-user EQ curves saved for a song or an album (`wip/saved-eq-plan.md`). A song's curve wins over its
//! album's. Curves are stored as a copy of the bands, never as a link to a named profile.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::music::EntityId;

/// The player's parametric EQ has exactly this many bands (`DEFAULT_PARAMETRIC_BANDS` in `client/src/audio/eq.ts`).
pub const BAND_COUNT: usize = 7;
/// Limits from `client/src/audio/eq.ts`. Values outside them are refused, not clamped.
const FREQ_RANGE: (f64, f64) = (20.0, 20_000.0);
const GAIN_RANGE: (f64, f64) = (-12.0, 12.0);
const Q_RANGE: (f64, f64) = (0.2, 10.0);
const BAND_TYPES: [&str; 5] = ["peaking", "low_shelf", "high_shelf", "highpass", "lowpass"];
/// Longest band id and label kept. The player's own are a few characters.
const MAX_ID_CHARS: usize = 16;
const MAX_LABEL_CHARS: usize = 32;

/// Creates the table and the triggers that drop a curve with its song or album. User deletes cascade.
pub fn ensure_saved_eq_schema(conn: &Connection) -> rusqlite::Result<()> {
    if !crate::table_exists(conn, "users") {
        return Ok(());
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_saved_eq (
           user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           scope       TEXT NOT NULL CHECK (scope IN ('track', 'album')),
           entity_id   TEXT NOT NULL,
           bands_json  TEXT NOT NULL,
           updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
           PRIMARY KEY (user_id, scope, entity_id)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_user_saved_eq_entity ON user_saved_eq(scope, entity_id);",
    )?;
    if crate::table_exists(conn, "tracks") {
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS trg_saved_eq_track_delete AFTER DELETE ON tracks BEGIN
               DELETE FROM user_saved_eq WHERE scope = 'track' AND entity_id = CAST(OLD.id AS TEXT);
             END;",
        )?;
    }
    if crate::table_exists(conn, "albums") {
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS trg_saved_eq_album_delete AFTER DELETE ON albums BEGIN
               DELETE FROM user_saved_eq WHERE scope = 'album' AND entity_id = CAST(OLD.id AS TEXT);
             END;",
        )?;
    }
    Ok(())
}

/// One band as the player sends and stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedBand {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    #[serde(rename = "type")]
    pub band_type: String,
    pub frequency_hz: f64,
    pub gain_db: f64,
    pub q: f64,
}

/// Checks a curve sent by the player. The message names the first thing wrong.
pub fn validate_bands(raw: &Value) -> Result<Vec<SavedBand>, String> {
    let Some(list) = raw.as_array() else {
        return Err("bands must be a list".into());
    };
    if list.len() != BAND_COUNT {
        return Err(format!("bands must have exactly {BAND_COUNT} entries"));
    }
    let mut bands = Vec::with_capacity(BAND_COUNT);
    for (i, item) in list.iter().enumerate() {
        let band: SavedBand = serde_json::from_value(item.clone())
            .map_err(|_| format!("band {} is not a valid band", i + 1))?;
        let n = i + 1;
        let id = band.id.trim();
        let label = band.label.trim();
        if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Err(format!("band {n} has a bad id"));
        }
        if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
            return Err(format!("band {n} has a bad label"));
        }
        if !BAND_TYPES.contains(&band.band_type.as_str()) {
            return Err(format!("band {n} has an unknown type"));
        }
        let within = |v: f64, (lo, hi): (f64, f64)| v.is_finite() && v >= lo && v <= hi;
        if !within(band.frequency_hz, FREQ_RANGE) {
            return Err(format!("band {n} frequency is out of range"));
        }
        if !within(band.gain_db, GAIN_RANGE) {
            return Err(format!("band {n} gain is out of range"));
        }
        if !within(band.q, Q_RANGE) {
            return Err(format!("band {n} Q is out of range"));
        }
        bands.push(SavedBand {
            id: id.to_owned(),
            label: label.to_owned(),
            ..band
        });
    }
    Ok(bands)
}

/// The curve a song plays with for one user, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedEqView {
    /// `track`, `album`, or none when neither has a curve.
    pub scope: Option<&'static str>,
    pub bands: Option<Vec<SavedBand>>,
    pub has_track: bool,
    pub has_album: bool,
    /// The song's album, for saving or removing the album's curve.
    pub album_id: Option<String>,
}

fn id_text(id: &EntityId) -> String {
    match id {
        EntityId::Int(n) => n.to_string(),
        EntityId::Str(s) => s.clone(),
    }
}

fn read(
    conn: &Connection,
    user_id: &str,
    scope: &str,
    entity_id: &str,
) -> rusqlite::Result<Option<Vec<SavedBand>>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT bands_json FROM user_saved_eq WHERE user_id = ?1 AND scope = ?2 AND entity_id = ?3",
            params![user_id, scope, entity_id],
            |r| r.get(0),
        )
        .optional()?;
    // A row that no longer reads as a curve is ignored rather than played.
    Ok(raw
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .and_then(|value| validate_bands(&value).ok()))
}

/// What a song plays with: its own curve, else its album's. `None` when the song doesn't exist.
pub fn saved_eq_for_track(
    conn: &Connection,
    user_id: &str,
    track_id: &EntityId,
) -> rusqlite::Result<Option<SavedEqView>> {
    let album: Option<Option<String>> = conn
        .query_row(
            "SELECT CAST(album_id AS TEXT) FROM tracks WHERE id = ?1",
            params![track_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(album_id) = album else {
        return Ok(None);
    };
    let track = read(conn, user_id, "track", &id_text(track_id))?;
    let album = match &album_id {
        Some(id) => read(conn, user_id, "album", id)?,
        None => None,
    };
    let (scope, bands) = match (&track, &album) {
        (Some(bands), _) => (Some("track"), Some(bands.clone())),
        (None, Some(bands)) => (Some("album"), Some(bands.clone())),
        (None, None) => (None, None),
    };
    Ok(Some(SavedEqView {
        scope,
        bands,
        has_track: track.is_some(),
        has_album: album.is_some(),
        album_id,
    }))
}

/// A song or an album.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Track,
    Album,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Scope::Track => "track",
            Scope::Album => "album",
        }
    }

    fn table(self) -> &'static str {
        match self {
            Scope::Track => "tracks",
            Scope::Album => "albums",
        }
    }
}

fn exists(conn: &Connection, scope: Scope, id: &EntityId) -> rusqlite::Result<bool> {
    // The table name comes from the enum, never from input.
    let sql = format!("SELECT 1 FROM {} WHERE id = ?1", scope.table());
    Ok(conn
        .query_row(&sql, params![id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Saves (replaces) a curve. `false` when the song or album doesn't exist.
pub fn save(
    conn: &Connection,
    user_id: &str,
    scope: Scope,
    entity_id: &EntityId,
    bands: &[SavedBand],
) -> rusqlite::Result<bool> {
    if !exists(conn, scope, entity_id)? {
        return Ok(false);
    }
    let json = serde_json::to_string(bands).expect("bands serialize");
    conn.execute(
        "INSERT INTO user_saved_eq(user_id, scope, entity_id, bands_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(user_id, scope, entity_id) DO UPDATE SET
           bands_json = excluded.bands_json,
           updated_at = excluded.updated_at",
        params![user_id, scope.name(), id_text(entity_id), json],
    )?;
    Ok(true)
}

/// Removes a curve. `false` when the song or album doesn't exist; removing one that isn't there is fine.
pub fn remove(
    conn: &Connection,
    user_id: &str,
    scope: Scope,
    entity_id: &EntityId,
) -> rusqlite::Result<bool> {
    if !exists(conn, scope, entity_id)? {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM user_saved_eq WHERE user_id = ?1 AND scope = ?2 AND entity_id = ?3",
        params![user_id, scope.name(), id_text(entity_id)],
    )?;
    Ok(true)
}

/// Moves album curves to the album another one merges into, before the old album is deleted. A curve the
/// remaining album already has for that user stays.
pub(crate) fn carry_album_curves(
    conn: &Connection,
    from: &EntityId,
    into: &EntityId,
) -> rusqlite::Result<()> {
    if !crate::table_exists(conn, "user_saved_eq") {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO user_saved_eq(user_id, scope, entity_id, bands_json, updated_at)
         SELECT user_id, 'album', ?2, bands_json, updated_at FROM user_saved_eq
         WHERE scope = 'album' AND entity_id = ?1",
        params![id_text(from), id_text(into)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn band(i: usize) -> Value {
        json!({ "id": format!("b{i}"), "label": format!("Band {i}"), "enabled": true, "type": "peaking",
                "frequencyHz": 100.0 * i as f64, "gainDb": 1.5, "q": 1.0 })
    }

    fn curve(gain: f64) -> Vec<SavedBand> {
        let mut list: Vec<Value> = (1..=BAND_COUNT).map(band).collect();
        list[1]["gainDb"] = json!(gain);
        validate_bands(&Value::Array(list)).unwrap()
    }

    struct Fx {
        conn: Connection,
        user: String,
        track: EntityId,
        album: EntityId,
        _dir: std::path::PathBuf,
    }

    fn fixture(name: &str) -> Fx {
        let dir = std::env::temp_dir().join(format!("saved-eq-{name}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = crate::init_db(&dir).unwrap().connection;
        let user: String = conn
            .query_row("SELECT id FROM users LIMIT 1", [], |r| r.get(0))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES ('lib', 'C:/music', 'Music');
             INSERT INTO artists(id, name) VALUES ('ar', 'Artist');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES ('al1', 'One', 'Artist', 'ar'), ('al2', 'Two', 'Artist', 'ar');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path)
               VALUES ('t1', 'lib', 'ar', 'al1', 'Song', 'C:/music/1.mp3'),
                      ('t2', 'lib', 'ar', 'al1', 'Other', 'C:/music/2.mp3');",
        )
        .unwrap();
        Fx {
            conn,
            user,
            track: EntityId::Str("t1".into()),
            album: EntityId::Str("al1".into()),
            _dir: dir,
        }
    }

    #[test]
    fn validation_names_what_is_wrong() {
        let good: Vec<Value> = (1..=BAND_COUNT).map(band).collect();
        assert!(validate_bands(&Value::Array(good.clone())).is_ok());
        assert_eq!(
            validate_bands(&json!({})).unwrap_err(),
            "bands must be a list"
        );
        assert_eq!(
            validate_bands(&Value::Array(good[..6].to_vec())).unwrap_err(),
            "bands must have exactly 7 entries"
        );
        let with = |field: &str, value: Value| {
            let mut list = good.clone();
            list[2][field] = value;
            validate_bands(&Value::Array(list)).unwrap_err()
        };
        assert_eq!(with("gainDb", json!(12.5)), "band 3 gain is out of range");
        assert_eq!(
            with("frequencyHz", json!(10)),
            "band 3 frequency is out of range"
        );
        assert_eq!(with("q", json!(0.1)), "band 3 Q is out of range");
        assert_eq!(with("type", json!("notch")), "band 3 has an unknown type");
        assert_eq!(with("label", json!("  ")), "band 3 has a bad label");
        assert_eq!(with("id", json!("x".repeat(17))), "band 3 has a bad id");
        assert_eq!(with("gainDb", json!("loud")), "band 3 is not a valid band");
    }

    #[test]
    fn the_song_wins_over_its_album_and_each_user_has_their_own() {
        let f = fixture("order");
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(
            (view.scope, view.has_track, view.has_album),
            (None, false, false)
        );
        assert_eq!(view.album_id.as_deref(), Some("al1"));

        assert!(save(&f.conn, &f.user, Scope::Album, &f.album, &curve(3.0)).unwrap());
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(view.scope, Some("album"));
        assert_eq!(view.bands.unwrap()[1].gain_db, 3.0);

        assert!(save(&f.conn, &f.user, Scope::Track, &f.track, &curve(-4.0)).unwrap());
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(
            (view.scope, view.has_track, view.has_album),
            (Some("track"), true, true)
        );
        assert_eq!(view.bands.unwrap()[1].gain_db, -4.0);
        // The album's other song still plays the album's curve.
        let other = saved_eq_for_track(&f.conn, &f.user, &EntityId::Str("t2".into()))
            .unwrap()
            .unwrap();
        assert_eq!(other.scope, Some("album"));

        // Saving again replaces; another user sees nothing.
        assert!(save(&f.conn, &f.user, Scope::Track, &f.track, &curve(6.0)).unwrap());
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(view.bands.unwrap()[1].gain_db, 6.0);
        f.conn
            .execute(
                "INSERT INTO users(id, username, role) VALUES ('u2', 'second', 'user')",
                [],
            )
            .unwrap();
        let theirs = saved_eq_for_track(&f.conn, "u2", &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(theirs.scope, None);

        assert!(remove(&f.conn, &f.user, Scope::Track, &f.track).unwrap());
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(view.scope, Some("album"));
        assert!(
            remove(&f.conn, &f.user, Scope::Track, &f.track).unwrap(),
            "removing twice is fine"
        );
    }

    #[test]
    fn unknown_songs_and_albums_are_refused() {
        let f = fixture("unknown");
        let missing = EntityId::Str("nope".into());
        assert_eq!(
            saved_eq_for_track(&f.conn, &f.user, &missing).unwrap(),
            None
        );
        assert!(!save(&f.conn, &f.user, Scope::Track, &missing, &curve(1.0)).unwrap());
        assert!(!save(&f.conn, &f.user, Scope::Album, &missing, &curve(1.0)).unwrap());
        assert!(!remove(&f.conn, &f.user, Scope::Album, &missing).unwrap());
    }

    #[test]
    fn curves_go_with_their_song_album_or_user() {
        let f = fixture("cleanup");
        save(&f.conn, &f.user, Scope::Track, &f.track, &curve(1.0)).unwrap();
        save(&f.conn, &f.user, Scope::Album, &f.album, &curve(2.0)).unwrap();
        let count = || -> i64 {
            f.conn
                .query_row("SELECT COUNT(*) FROM user_saved_eq", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(), 2);
        f.conn
            .execute("DELETE FROM tracks WHERE id = 't1'", [])
            .unwrap();
        assert_eq!(count(), 1);
        f.conn
            .execute("DELETE FROM tracks WHERE album_id = 'al1'", [])
            .unwrap();
        f.conn
            .execute("DELETE FROM albums WHERE id = 'al1'", [])
            .unwrap();
        assert_eq!(count(), 0);

        f.conn
            .execute(
                "INSERT INTO users(id, username, role) VALUES ('u2', 'second', 'user')",
                [],
            )
            .unwrap();
        save(
            &f.conn,
            "u2",
            Scope::Album,
            &EntityId::Str("al2".into()),
            &curve(2.0),
        )
        .unwrap();
        f.conn
            .execute("DELETE FROM users WHERE id = 'u2'", [])
            .unwrap();
        assert_eq!(count(), 0);
    }

    #[test]
    fn an_album_merged_by_a_rename_keeps_its_curve() {
        let f = fixture("merge");
        let two = EntityId::Str("al2".into());
        save(&f.conn, &f.user, Scope::Album, &f.album, &curve(5.0)).unwrap();
        // Renaming "One" to "Two" merges it into the existing album.
        let result = crate::playlists::update_album_metadata(
            &f.conn,
            &f.album,
            crate::playlists::AlbumMetadataUpdate {
                title: Some("Two".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.merged_into, Some(two.clone()));
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!(view.scope, Some("album"));
        assert_eq!(view.album_id.as_deref(), Some("al2"));
        assert_eq!(view.bands.unwrap()[1].gain_db, 5.0);
        let left: i64 = f
            .conn
            .query_row(
                "SELECT COUNT(*) FROM user_saved_eq WHERE entity_id = 'al1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_rescan_keeps_the_song_and_album_ids() {
        let f = fixture("rescan");
        save(&f.conn, &f.user, Scope::Track, &f.track, &curve(1.0)).unwrap();
        save(&f.conn, &f.user, Scope::Album, &f.album, &curve(2.0)).unwrap();
        let input = crate::jobs::ScannedTrackInput {
            library_id: EntityId::Str("lib".into()),
            file_path: "C:/music/1.mp3".into(),
            file_name: "1.mp3".into(),
            file_size: 1000,
            format: "mp3".into(),
            title: "Song (remastered tag)".into(),
            artist: "Artist".into(),
            album: "One".into(),
            album_artist: "Artist".into(),
            genre: String::new(),
            composer: String::new(),
            track_number: Some(1),
            disc_number: None,
            year: None,
            comment: None,
            bpm: None,
            duration: 200.0,
            bitrate: None,
            sample_rate: None,
            channels: None,
            loudness: None,
        };
        crate::jobs::upsert_scanned_track(&f.conn, &input).unwrap();
        let view = saved_eq_for_track(&f.conn, &f.user, &f.track)
            .unwrap()
            .unwrap();
        assert_eq!((view.has_track, view.has_album), (true, true));
        assert_eq!(view.album_id.as_deref(), Some("al1"));
    }
}

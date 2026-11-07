//! Global popularity per track, for the "Popular" steering pill: Last.fm scrobble counts, filled artist by
//! artist by the background metadata lane (`radio_metadata.rs`). One request covers all of an artist's tracks,
//! so a library takes one request per artist instead of one per track.

use crate::music::EntityId;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Re-read an artist's counts after this long; they change slowly.
pub const POPULARITY_OK_TTL_DAYS: i64 = 90;
/// An artist Last.fm didn't know may appear there later.
pub const POPULARITY_EMPTY_TTL_DAYS: i64 = 30;
const SOURCE_LASTFM: &str = "lastfm";

/// Suffix words that mark another release of the same song ("Song - 2011 Remaster").
const RELEASE_WORDS: [&str; 7] = [
    "remaster", "version", "mix", "edit", "live", "mono", "stereo",
];

/// Migration `2026-11-06-track-popularity`.
pub fn ensure_track_popularity_schema(conn: &Connection) -> rusqlite::Result<()> {
    if crate::table_exists(conn, "tracks") {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS track_popularity (
               track_id    TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
               scrobbles   INTEGER NOT NULL,
               listeners   INTEGER,
               source      TEXT NOT NULL,
               checked_at  TEXT NOT NULL DEFAULT (datetime('now'))
             ) WITHOUT ROWID;",
        )?;
    }
    if crate::table_exists(conn, "artists") {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS artist_popularity_sync (
               artist_id   TEXT PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
               status      TEXT NOT NULL,
               checked_at  TEXT NOT NULL DEFAULT (datetime('now'))
             ) WITHOUT ROWID;",
        )?;
    }
    Ok(())
}

/// One song in an artist's Last.fm list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopularityEntry {
    pub title: String,
    pub scrobbles: u64,
    pub listeners: Option<u64>,
}

fn letters_and_digits(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A song title reduced to what two releases of it share: no case, punctuation, bracketed notes, or a suffix
/// like " - 2011 Remaster". On a sample of the dev library exact titles matched 80% of tracks and these 99%.
pub fn normalize_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let without_suffix = lower
        .match_indices(" - ")
        .find(|(at, _)| RELEASE_WORDS.iter().any(|word| lower[*at..].contains(word)))
        .map_or(lower.as_str(), |(at, _)| &lower[..at]);
    let mut depth = 0usize;
    let without_brackets: String = without_suffix
        .chars()
        .filter(|c| match c {
            '(' | '[' => {
                depth += 1;
                false
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect();
    let reduced = letters_and_digits(&without_brackets);
    // A title that is all brackets, like "(Untitled)".
    if reduced.is_empty() {
        letters_and_digits(&lower)
    } else {
        reduced
    }
}

/// Artists with tracks whose counts are missing or due for another look, never-checked ones first.
pub fn list_artists_needing_popularity(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<(EntityId, String)>> {
    let mut stmt = conn.prepare(
        "SELECT ar.id, ar.name
         FROM artists ar
         LEFT JOIN artist_popularity_sync s ON s.artist_id = ar.id
         WHERE TRIM(ar.name) <> ''
           AND EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = ar.id)
           AND (s.artist_id IS NULL
                OR (s.status = 'ok' AND s.checked_at < datetime('now', ?1))
                OR (s.status <> 'ok' AND s.checked_at < datetime('now', ?2)))
         ORDER BY s.artist_id IS NOT NULL, s.checked_at, ar.id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![
            format!("-{POPULARITY_OK_TTL_DAYS} days"),
            format!("-{POPULARITY_EMPTY_TTL_DAYS} days"),
            limit
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    rows.collect()
}

/// Replaces an artist's counts with what Last.fm listed, matched to their tracks by title, and marks the artist
/// checked. A track with no match has no count. Returns how many tracks matched.
pub fn store_artist_popularity(
    conn: &Connection,
    artist_id: &EntityId,
    entries: &[PopularityEntry],
) -> rusqlite::Result<usize> {
    // Several releases can reduce to one title; the biggest stands for the song.
    let mut by_title: HashMap<String, &PopularityEntry> = HashMap::new();
    for entry in entries {
        let slot = by_title
            .entry(normalize_title(&entry.title))
            .or_insert(entry);
        if entry.scrobbles > slot.scrobbles {
            *slot = entry;
        }
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM track_popularity WHERE source = ?1
           AND track_id IN (SELECT id FROM tracks WHERE artist_id = ?2)",
        params![SOURCE_LASTFM, artist_id],
    )?;
    let tracks: Vec<(EntityId, String)> = tx
        .prepare("SELECT id, COALESCE(title, '') FROM tracks WHERE artist_id = ?1")?
        .query_map([artist_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut matched = 0;
    {
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO track_popularity(track_id, scrobbles, listeners, source)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (track_id, title) in &tracks {
            if let Some(entry) = by_title.get(&normalize_title(title)) {
                insert.execute(params![
                    track_id,
                    entry.scrobbles as i64,
                    entry.listeners.map(|n| n as i64),
                    SOURCE_LASTFM
                ])?;
                matched += 1;
            }
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO artist_popularity_sync(artist_id, status, checked_at)
         VALUES (?1, ?2, datetime('now'))",
        params![artist_id, if entries.is_empty() { "empty" } else { "ok" }],
    )?;
    tx.commit()?;
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::coerce_entity_id;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES ('lib', '/m', 'Music');
             INSERT INTO artists(id, name) VALUES ('a1', 'Erasure'), ('a2', 'Yello'), ('a3', 'No Tracks'), ('a4', '  ');
             INSERT INTO tracks(id, library_id, artist_id, title, file_path) VALUES
               ('t1', 'lib', 'a1', 'A Little Respect', '/m/1'),
               ('t2', 'lib', 'a1', 'A Little Respect (Remastered 2009)', '/m/2'),
               ('t3', 'lib', 'a1', 'Sometimes - 12\" Mix', '/m/3'),
               ('t4', 'lib', 'a1', 'Rare B-Side', '/m/4'),
               ('t5', 'lib', 'a2', 'The Race', '/m/5'),
               ('t6', 'lib', 'a4', 'Blank Artist', '/m/6');",
        )
        .unwrap();
        conn
    }

    fn entry(title: &str, scrobbles: u64) -> PopularityEntry {
        PopularityEntry {
            title: title.to_owned(),
            scrobbles,
            listeners: Some(scrobbles / 10),
        }
    }

    fn scrobbles(conn: &Connection, track: &str) -> Option<i64> {
        conn.query_row(
            "SELECT scrobbles FROM track_popularity WHERE track_id = ?1",
            [track],
            |r| r.get(0),
        )
        .ok()
    }

    #[test]
    fn titles_reduce_to_what_releases_share() {
        for (title, expected) in [
            ("A Little Respect", "a little respect"),
            ("A Little Respect (Remastered 2009)", "a little respect"),
            ("Sometimes - 12\" Mix", "sometimes"),
            ("Song - 2011 Remaster", "song"),
            ("Song [Live at Wembley]", "song"),
            ("Don't Stop!", "don t stop"),
            ("Ça Plane Pour Moi", "ça plane pour moi"),
            // A dash that isn't a release note stays.
            ("Part One - Part Two", "part one part two"),
            ("(Untitled)", "untitled"),
            ("", ""),
        ] {
            assert_eq!(normalize_title(title), expected, "{title}");
        }
    }

    #[test]
    fn counts_reach_every_release_of_a_song_and_unmatched_tracks_have_none() {
        let conn = db();
        let matched = store_artist_popularity(
            &conn,
            &coerce_entity_id("a1"),
            &[
                entry("A Little Respect", 900),
                entry("A Little Respect - 2009 Remaster", 100),
                entry("Sometimes", 500),
                entry("Not In The Library", 7),
            ],
        )
        .unwrap();
        assert_eq!(matched, 3);
        assert_eq!(scrobbles(&conn, "t1"), Some(900), "the biggest release");
        assert_eq!(scrobbles(&conn, "t2"), Some(900));
        assert_eq!(scrobbles(&conn, "t3"), Some(500));
        assert_eq!(scrobbles(&conn, "t4"), None);
        let listeners: i64 = conn
            .query_row(
                "SELECT listeners FROM track_popularity WHERE track_id = 't3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(listeners, 50);
    }

    #[test]
    fn a_new_look_replaces_the_old_counts() {
        let conn = db();
        let artist = coerce_entity_id("a1");
        store_artist_popularity(&conn, &artist, &[entry("Rare B-Side", 5)]).unwrap();
        assert_eq!(scrobbles(&conn, "t4"), Some(5));
        store_artist_popularity(&conn, &artist, &[entry("Sometimes", 8)]).unwrap();
        assert_eq!(scrobbles(&conn, "t4"), None);
        assert_eq!(scrobbles(&conn, "t3"), Some(8));
    }

    #[test]
    fn counts_go_with_the_track() {
        let conn = db();
        store_artist_popularity(&conn, &coerce_entity_id("a2"), &[entry("The Race", 5)]).unwrap();
        conn.execute("DELETE FROM tracks WHERE id = 't5'", [])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_popularity", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn artists_are_listed_until_checked_and_again_when_due() {
        let conn = db();
        let names = |conn: &Connection| -> Vec<String> {
            list_artists_needing_popularity(conn, 10)
                .unwrap()
                .into_iter()
                .map(|(_, name)| name)
                .collect()
        };
        assert_eq!(
            names(&conn),
            ["Erasure", "Yello"],
            "no blank names, no artists without tracks"
        );
        assert_eq!(list_artists_needing_popularity(&conn, 1).unwrap().len(), 1);

        store_artist_popularity(&conn, &coerce_entity_id("a1"), &[entry("Sometimes", 1)]).unwrap();
        store_artist_popularity(&conn, &coerce_entity_id("a2"), &[]).unwrap();
        assert!(names(&conn).is_empty());

        // An artist Last.fm didn't know comes back sooner than one it did.
        conn.execute_batch(
            "UPDATE artist_popularity_sync SET checked_at = datetime('now', '-40 days');",
        )
        .unwrap();
        assert_eq!(names(&conn), ["Yello"]);
        conn.execute_batch(
            "UPDATE artist_popularity_sync SET checked_at = datetime('now', '-100 days');",
        )
        .unwrap();
        assert_eq!(names(&conn), ["Erasure", "Yello"]);
    }

    #[test]
    fn the_migration_can_run_again() {
        let conn = db();
        ensure_track_popularity_schema(&conn).unwrap();
        ensure_track_popularity_schema(&conn).unwrap();
    }
}

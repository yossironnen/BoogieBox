//! Favourites in the web UI: the same per-user stars Subsonic apps set (`subsonic_stars`), read back in the
//! BoogieBox JSON shapes. See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-4).

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::music::{
    coerce_entity_id, id_to_value, map_track, AlbumRow, ArtistRow, TrackRow, TRACK_COLS,
};
use crate::subsonic::{self as sdb, ItemKind};

/// Most favourites of each kind returned at once.
pub const MAX_FAVOURITES: i64 = 1000;

/// The ids a user has marked as favourites, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FavouriteIds {
    pub tracks: Vec<String>,
    pub albums: Vec<String>,
    pub artists: Vec<String>,
}

/// Favourites as full rows, newest first.
#[derive(Debug, Serialize)]
pub struct Favourites {
    pub artists: Vec<ArtistRow>,
    pub albums: Vec<AlbumRow>,
    pub tracks: Vec<TrackRow>,
}

/// Parses `track` / `album` / `artist`.
pub fn parse_kind(raw: &str) -> Option<ItemKind> {
    match raw {
        "track" => Some(ItemKind::Track),
        "album" => Some(ItemKind::Album),
        "artist" => Some(ItemKind::Artist),
        _ => None,
    }
}

/// All favourite ids of a user.
pub fn favourite_ids(conn: &Connection, user_id: &str) -> rusqlite::Result<FavouriteIds> {
    Ok(FavouriteIds {
        tracks: sdb::starred_ids(conn, user_id, ItemKind::Track)?,
        albums: sdb::starred_ids(conn, user_id, ItemKind::Album)?,
        artists: sdb::starred_ids(conn, user_id, ItemKind::Artist)?,
    })
}

/// Adds or removes a favourite. `Ok(false)` when the item does not exist.
pub fn set_favourite(
    conn: &Connection,
    user_id: &str,
    kind: ItemKind,
    id: &str,
    on: bool,
) -> rusqlite::Result<bool> {
    let table = match kind {
        ItemKind::Track => "tracks",
        ItemKind::Album => "albums",
        ItemKind::Artist => "artists",
    };
    let exists = conn
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            [id_to_value(&coerce_entity_id(id))],
            |_| Ok(()),
        )
        .map(|_| true)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            other => Err(other),
        })?;
    if !exists && on {
        return Ok(false);
    }
    // Removing works even for an item deleted since (cleans up its star).
    sdb::set_stars(conn, user_id, &[(kind, id.to_owned())], on)?;
    Ok(exists || !on)
}

/// Full favourite rows (each list newest first, at most `limit` items).
pub fn list_favourites(
    conn: &Connection,
    user_id: &str,
    limit: i64,
) -> rusqlite::Result<Favourites> {
    let limit = limit.clamp(1, MAX_FAVOURITES);
    let tracks_sql = format!(
        "SELECT {TRACK_COLS}, trr.rating
         FROM subsonic_stars st
         JOIN tracks t ON t.id = st.item_id
         LEFT JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN libraries l ON l.id = t.library_id
         LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = st.user_id
         WHERE st.user_id = ?1 AND st.item_type = 'track'
         ORDER BY st.starred_at DESC, t.id
         LIMIT ?2"
    );
    let tracks = conn
        .prepare(&tracks_sql)?
        .query_map(params![user_id, limit], map_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let albums = conn
        .prepare(
            "SELECT al.id, al.title, al.album_artist, ar.name, al.year, al.genre, alr.rating, al.release_type,
                    (SELECT COUNT(*) FROM tracks t WHERE t.album_id = al.id),
                    (SELECT ROUND(SUM(t.duration), 0) FROM tracks t WHERE t.album_id = al.id),
                    al.metadata_locked, al.description, al.label
             FROM subsonic_stars st
             JOIN albums al ON al.id = st.item_id
             LEFT JOIN artists ar ON ar.id = al.artist_id
             LEFT JOIN album_ratings alr ON alr.album_id = al.id AND alr.user_id = st.user_id
             WHERE st.user_id = ?1 AND st.item_type = 'album'
             ORDER BY st.starred_at DESC, al.id
             LIMIT ?2",
        )?
        .query_map(params![user_id, limit], |row| {
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
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let artists = conn
        .prepare(
            "SELECT ar.id, ar.name, arr.rating,
                    (SELECT COUNT(*) FROM tracks t JOIN albums al ON al.id = t.album_id WHERE al.artist_id = ar.id),
                    (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id),
                    ar.play_count
             FROM subsonic_stars st
             JOIN artists ar ON ar.id = st.item_id
             LEFT JOIN artist_ratings arr ON arr.artist_id = ar.id AND arr.user_id = st.user_id
             WHERE st.user_id = ?1 AND st.item_type = 'artist'
             ORDER BY st.starred_at DESC, ar.id
             LIMIT ?2",
        )?
        .query_map(params![user_id, limit], |row| {
            Ok(ArtistRow {
                id: row.get(0)?,
                name: row.get(1)?,
                rating: row.get(2)?,
                track_count: row.get(3)?,
                album_count: row.get(4)?,
                play_count: row.get(5)?,
                metadata_locked: None,
                identity_lock_pending: None,
                description: None,
                styles: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Favourites {
        artists,
        albums,
        tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username, role) VALUES ('u1', 'alice', 'user'), ('u2', 'bob', 'user');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
             INSERT INTO artists(id, name) VALUES ('ar1', 'The Band'), ('ar2', 'Solo');
             INSERT INTO albums(id, title, artist_id) VALUES ('al1', 'First', 'ar1'), ('al2', 'Second', 'ar2');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration) VALUES
               ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac', 200),
               ('t2', 'lib1', 'ar2', 'al2', 'Second Song', '/m/2.flac', 100);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn kinds_parse() {
        assert_eq!(parse_kind("track"), Some(ItemKind::Track));
        assert_eq!(parse_kind("album"), Some(ItemKind::Album));
        assert_eq!(parse_kind("artist"), Some(ItemKind::Artist));
        assert_eq!(parse_kind("playlist"), None);
    }

    #[test]
    fn set_list_and_remove_favourites_per_user() {
        let conn = db();
        assert!(set_favourite(&conn, "u1", ItemKind::Track, "t1", true).unwrap());
        assert!(set_favourite(&conn, "u1", ItemKind::Album, "al2", true).unwrap());
        assert!(set_favourite(&conn, "u1", ItemKind::Artist, "ar1", true).unwrap());
        assert!(!set_favourite(&conn, "u1", ItemKind::Track, "missing", true).unwrap());
        // Idempotent.
        assert!(set_favourite(&conn, "u1", ItemKind::Track, "t1", true).unwrap());
        assert!(set_favourite(&conn, "u2", ItemKind::Track, "t2", true).unwrap());

        let ids = favourite_ids(&conn, "u1").unwrap();
        assert_eq!(ids.tracks, ["t1"]);
        assert_eq!(ids.albums, ["al2"]);
        assert_eq!(ids.artists, ["ar1"]);

        let full = list_favourites(&conn, "u1", 50).unwrap();
        assert_eq!(full.tracks.len(), 1);
        assert_eq!(full.tracks[0].title.as_deref(), Some("Opening"));
        assert_eq!(full.tracks[0].artist.as_deref(), Some("The Band"));
        assert_eq!(full.albums[0].title, "Second");
        assert_eq!(full.albums[0].track_count, 1);
        assert_eq!(full.albums[0].artist.as_deref(), Some("Solo"));
        assert_eq!(full.artists[0].name, "The Band");
        assert_eq!(full.artists[0].album_count, 1);

        assert!(set_favourite(&conn, "u1", ItemKind::Track, "t1", false).unwrap());
        // Removing a star on a vanished item still succeeds.
        assert!(set_favourite(&conn, "u1", ItemKind::Track, "gone", false).unwrap());
        assert!(favourite_ids(&conn, "u1").unwrap().tracks.is_empty());
        assert_eq!(favourite_ids(&conn, "u2").unwrap().tracks, ["t2"]);
    }
}

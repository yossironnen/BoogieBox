//! Primes the SQLite page cache with the queries the home page fires on
//! first load, so that load doesn't pay for cold disk I/O in the user's
//! browser. Runs once, off the async runtime, right after a database opens
//! (startup, first-run setup, and library switch).

use std::time::Instant;

use rusqlite::Connection;

use crate::DbPool;

const WARM_LIMIT: i64 = 12;
const WARM_USER_CAP: usize = 25;

/// Spawns a background task that reads the home page's core queries once,
/// pulling their pages into the OS/SQLite cache. Best-effort: failures are
/// logged and otherwise ignored, since this never affects correctness.
pub fn warm_in_background(db: DbPool) {
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let started = Instant::now();
        warm_cache(&conn);
        tracing::info!("Database cache warm-up finished in {:?}", started.elapsed());
    });
}

fn warm_cache(conn: &Connection) {
    let _ = boogiebox_db::music::list_home_genre_summaries(conn, WARM_LIMIT as usize);
    let _ = boogiebox_db::music::list_genres(conn);
    let _ = boogiebox_db::music::list_recently_played(conn, WARM_LIMIT);
    let _ = boogiebox_db::music::list_top_played(conn, WARM_LIMIT);

    let user_ids: Vec<String> = conn
        .prepare("SELECT id FROM users LIMIT ?")
        .and_then(|mut stmt| {
            stmt.query_map([WARM_USER_CAP as i64], |row| row.get(0))?
                .collect()
        })
        .unwrap_or_default();

    for user_id in user_ids {
        let _ = boogiebox_db::music::get_home_top_rated(conn, &user_id, WARM_LIMIT);
        let _ = boogiebox_db::music::list_albums_latest(conn, &user_id, WARM_LIMIT);
        let _ = boogiebox_db::music::list_artists_most_played(conn, &user_id, WARM_LIMIT);
        let _ = boogiebox_db::playlists::list_playlists(conn, &user_id);
    }
}

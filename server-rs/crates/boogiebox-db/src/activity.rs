//! Admin Activity page data: the persisted recent-activity history (`activity_events`, 7-day retention) and the
//! short, indexed reads the live snapshot needs (active BoogieMix jobs, deep-analysis queue counts, titles for the
//! items being played). See wip/activity-dashboard-plan.md.

use std::collections::HashMap;

use rusqlite::{params, params_from_iter, types::ValueRef, Connection, OptionalExtension};
use serde::Serialize;

/// Days of history kept in `activity_events`.
pub const ACTIVITY_RETENTION_DAYS: i64 = 7;
/// Most ids resolved in one title lookup.
const MAX_LOOKUP_IDS: usize = 200;

/// Migration `2026-10-24-activity-events`: the recent-activity history.
pub fn ensure_activity_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS activity_events (
           id        INTEGER PRIMARY KEY AUTOINCREMENT,
           at        TEXT NOT NULL,
           kind      TEXT NOT NULL,
           user_id   TEXT REFERENCES users(id) ON DELETE CASCADE,
           username  TEXT,
           source    TEXT,
           client    TEXT,
           item_id   TEXT,
           title     TEXT,
           count     INTEGER,
           bytes     INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_activity_events_at ON activity_events(at);",
    )
}

/// One event to store. `at` is Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewActivityEvent {
    pub at: i64,
    pub kind: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub source: Option<String>,
    pub client: Option<String>,
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub count: Option<i64>,
    pub bytes: Option<i64>,
}

/// Stores events in one transaction. Rows naming a user that no longer exists keep the name but lose the id.
pub fn insert_activity_events(
    conn: &Connection,
    events: &[NewActivityEvent],
) -> rusqlite::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO activity_events(at, kind, user_id, username, source, client, item_id, title, count, bytes)
             VALUES (datetime(?1, 'unixepoch'), ?2,
                     (SELECT id FROM users WHERE id = ?3), ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for e in events {
            stmt.execute(params![
                e.at, e.kind, e.user_id, e.username, e.source, e.client, e.item_id, e.title,
                e.count, e.bytes
            ])?;
        }
    }
    tx.commit()
}

/// Deletes history older than the retention window. Returns the rows removed.
pub fn prune_activity_events(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM activity_events WHERE at < datetime('now', ?1)",
        [format!("-{ACTIVITY_RETENTION_DAYS} days")],
    )
}

/// One "Recent" row: a stored event, a finished BoogieMix job or a finished scan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentActivity {
    /// `YYYY-MM-DD HH:MM:SS` (UTC).
    pub at: String,
    /// `play`, `download`, `stream`, `playOnStopped`, `mixFinished`, `mixFailed`, `scanFinished`.
    pub kind: String,
    pub user: Option<String>,
    pub source: Option<String>,
    pub client: Option<String>,
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub count: Option<i64>,
    pub bytes: Option<i64>,
}

/// Recent activity newer than `since` (`YYYY-MM-DD HH:MM:SS`), newest first.
pub fn list_recent_activity(
    conn: &Connection,
    since: &str,
    limit: usize,
) -> rusqlite::Result<Vec<RecentActivity>> {
    let mut stmt = conn.prepare(
        "SELECT at, kind, user, source, client, item_id, title, count, bytes FROM (
           SELECT e.at AS at, e.kind AS kind, COALESCE(u.username, e.username) AS user, e.source AS source,
                  e.client AS client, e.item_id AS item_id, e.title AS title, e.count AS count, e.bytes AS bytes
             FROM activity_events e LEFT JOIN users u ON u.id = e.user_id
            WHERE e.at >= ?1
           UNION ALL
           SELECT j.finished_at, CASE WHEN j.status = 'done' THEN 'mixFinished' ELSE 'mixFailed' END,
                  u.username, NULL, NULL, j.output_id,
                  COALESCE(NULLIF(TRIM(o.name), ''), p.name), NULL, NULL
             FROM mix_jobs j
             LEFT JOIN users u ON u.id = j.user_id
             LEFT JOIN mix_outputs o ON o.id = j.output_id
             LEFT JOIN playlists p ON p.id = j.playlist_id
            WHERE j.finished_at >= ?1 AND j.status IN ('done', 'failed')
           UNION ALL
           SELECT s.finished_at, 'scanFinished', NULL, NULL, NULL, s.library_id, l.name, s.files_scanned, NULL
             FROM scan_jobs s LEFT JOIN libraries l ON l.id = s.library_id
            WHERE s.finished_at >= ?1 AND s.status = 'done'
         ) ORDER BY at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![since, limit as i64], |r| {
        Ok(RecentActivity {
            at: r.get(0)?,
            kind: r.get(1)?,
            user: r.get(2)?,
            source: r.get(3)?,
            client: r.get(4)?,
            item_id: r.get(5)?,
            title: r.get(6)?,
            count: r.get(7)?,
            bytes: r.get(8)?,
        })
    })?;
    rows.collect()
}

/// A BoogieMix job that has not finished.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMixJob {
    pub id: String,
    pub user: Option<String>,
    pub playlist_name: Option<String>,
    pub style: String,
    pub track_count: i64,
    pub status: String,
    pub progress_percent: i64,
    pub step: String,
    pub message: Option<String>,
    pub started_at: Option<String>,
    /// Only queued and analysing/planning jobs can be cancelled (a render runs to the end).
    pub can_cancel: bool,
}

/// Every unfinished BoogieMix job, oldest first.
pub fn list_active_mix_jobs(conn: &Connection) -> rusqlite::Result<Vec<ActiveMixJob>> {
    let mut stmt = conn.prepare(
        "SELECT j.id, u.username, p.name, j.mix_style,
                (SELECT COUNT(*) FROM playlist_tracks pt WHERE pt.playlist_id = j.playlist_id),
                j.status, j.progress_percent, j.current_step, j.last_message, j.started_at
           FROM mix_jobs j
           LEFT JOIN users u ON u.id = j.user_id
           LEFT JOIN playlists p ON p.id = j.playlist_id
          WHERE j.status IN ('pending', 'analyzing', 'planning', 'rendering')
          ORDER BY j.created_at, j.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let status: String = r.get(5)?;
        Ok(ActiveMixJob {
            id: r.get(0)?,
            user: r.get(1)?,
            playlist_name: r.get(2)?,
            style: r.get(3)?,
            track_count: r.get(4)?,
            can_cancel: status != "rendering",
            status,
            progress_percent: r.get(6)?,
            step: r.get(7)?,
            message: r.get(8)?,
            started_at: r.get(9)?,
        })
    })?;
    rows.collect()
}

/// Pending and running deep-analysis jobs (index `idx_deep_analysis_jobs_status`; never a full-table sum).
pub fn deep_analysis_active_counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let count = |status: &str| -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status = ?1",
            [status],
            |r| r.get(0),
        )
    };
    Ok((count("pending")?, count("running")?))
}

/// Display details of a played or transferred item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemSummary {
    pub title: String,
    pub artist: Option<String>,
    pub album_id: Option<String>,
    pub duration: Option<f64>,
}

/// Looks up titles for item ids: library track ids, `mx-{outputId}` mixes and `st-{stationId}` radio stations.
/// Unknown ids are left out.
pub fn item_summaries(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<HashMap<String, ItemSummary>> {
    let mut out = HashMap::new();
    let (mut tracks, mut mixes, mut stations) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids.iter().take(MAX_LOOKUP_IDS) {
        if let Some(mix) = id.strip_prefix("mx-") {
            mixes.push((id.clone(), mix.to_owned()));
        } else if let Some(station) = id.strip_prefix("st-") {
            stations.push((id.clone(), station.to_owned()));
        } else {
            tracks.push((id.clone(), id.clone()));
        }
    }
    lookup(
        conn,
        &tracks,
        "SELECT t.id, t.title, COALESCE(ar.name, NULLIF(t.album_artist, '')), t.album_id, t.duration
           FROM tracks t LEFT JOIN artists ar ON ar.id = t.artist_id WHERE t.id IN",
        &mut out,
    )?;
    lookup(
        conn,
        &mixes,
        "SELECT id, COALESCE(NULLIF(TRIM(name), ''), file_name), 'BoogieMix', NULL, duration_sec
           FROM mix_outputs WHERE id IN",
        &mut out,
    )?;
    lookup(
        conn,
        &stations,
        "SELECT id, name, NULL, NULL, NULL FROM internet_radio_stations WHERE id IN",
        &mut out,
    )?;
    Ok(out)
}

/// Runs `select … IN (?, …)` for `(public id, row id)` pairs and stores the rows under their public ids.
fn lookup(
    conn: &Connection,
    pairs: &[(String, String)],
    select_in: &str,
    out: &mut HashMap<String, ItemSummary>,
) -> rusqlite::Result<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let by_row: HashMap<&str, &str> = pairs
        .iter()
        .map(|(public, row)| (row.as_str(), public.as_str()))
        .collect();
    let marks = vec!["?"; pairs.len()].join(",");
    let mut stmt = conn.prepare(&format!("{select_in} ({marks})"))?;
    let rows = stmt.query_map(params_from_iter(pairs.iter().map(|(_, row)| row)), |r| {
        Ok((
            lenient_text(r.get_ref(0)?).unwrap_or_default(),
            ItemSummary {
                title: lenient_text(r.get_ref(1)?).unwrap_or_default(),
                artist: lenient_text(r.get_ref(2)?),
                album_id: lenient_text(r.get_ref(3)?),
                duration: r.get::<_, Option<f64>>(4)?.filter(|d| *d > 0.0),
            },
        ))
    })?;
    for row in rows {
        let (id, summary) = row?;
        if let Some(public) = by_row.get(id.as_str()) {
            out.insert((*public).to_owned(), summary);
        }
    }
    Ok(())
}

/// Text of a loosely typed column (legacy databases may hold integer ids).
fn lenient_text(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(String::from_utf8_lossy(t).into_owned()),
    }
}

/// Name of a user, if they exist.
pub fn username(conn: &Connection, user_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT username FROM users WHERE id = ?1", [user_id], |r| {
        r.get(0)
    })
    .optional()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username, role) VALUES ('u1', 'alice', 'admin'), ('u2', 'bob', 'user');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
             INSERT INTO artists(id, name) VALUES ('ar1', 'The Band');
             INSERT INTO albums(id, title, artist_id) VALUES ('al1', 'First', 'ar1');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration)
               VALUES ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac', 245.0);
             INSERT INTO playlists(id, name, user_id) VALUES ('p1', 'Party', 'u1');
             INSERT INTO playlist_tracks(id, playlist_id, track_id, position) VALUES ('pt1', 'p1', 't1', 0);",
        )
        .unwrap();
        conn
    }

    fn event(kind: &str, at: i64) -> NewActivityEvent {
        NewActivityEvent {
            at,
            kind: kind.into(),
            user_id: Some("u2".into()),
            username: Some("bob".into()),
            source: Some("subsonic".into()),
            client: Some("Amperfy".into()),
            item_id: Some("t1".into()),
            title: Some("Opening".into()),
            count: None,
            bytes: None,
        }
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn schema_migration_is_idempotent() {
        let conn = db();
        ensure_activity_schema(&conn).unwrap();
        ensure_activity_schema(&conn).unwrap();
    }

    #[test]
    fn events_round_trip_prune_and_merge_with_jobs() {
        let conn = db();
        let mut unknown_user = event("download", now() - 60);
        unknown_user.user_id = Some("gone".into());
        unknown_user.count = Some(37);
        insert_activity_events(
            &conn,
            &[
                event("play", now() - 30),
                unknown_user,
                event("play", now() - 9 * 86_400),
            ],
        )
        .unwrap();
        assert_eq!(prune_activity_events(&conn).unwrap(), 1);
        conn.execute_batch(
            "INSERT INTO mix_jobs(id, playlist_id, user_id, status, finished_at) VALUES
               ('j1', 'p1', 'u1', 'done', datetime('now', '-10 minutes')),
               ('j2', 'p1', 'u1', 'canceled', datetime('now', '-5 minutes'));
             INSERT INTO scan_jobs(id, library_id, status, finished_at, files_scanned) VALUES
               ('s1', 'lib1', 'done', datetime('now', '-20 minutes'), 12);",
        )
        .unwrap();
        let rows = list_recent_activity(&conn, "2000-01-01 00:00:00", 50).unwrap();
        let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["play", "download", "mixFinished", "scanFinished"]);
        assert_eq!(rows[0].user.as_deref(), Some("bob"));
        assert_eq!(
            rows[1].user.as_deref(),
            Some("bob"),
            "name kept without the id"
        );
        assert_eq!(rows[1].count, Some(37));
        assert_eq!(rows[2].title.as_deref(), Some("Party"));
        assert_eq!(rows[3].title.as_deref(), Some("Music"));
        assert_eq!(rows[3].count, Some(12));
        assert_eq!(
            list_recent_activity(&conn, "2000-01-01 00:00:00", 2)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn deleting_a_user_deletes_their_history() {
        let conn = db();
        insert_activity_events(&conn, &[event("play", now())]).unwrap();
        conn.execute("DELETE FROM users WHERE id = 'u2'", [])
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn active_mix_jobs_and_deep_counts() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO mix_jobs(id, playlist_id, user_id, status, progress_percent, current_step) VALUES
               ('j1', 'p1', 'u1', 'rendering', 64, 'Rendering transitions'),
               ('j2', 'p1', 'u1', 'pending', 0, 'queued'),
               ('j3', 'p1', 'u1', 'done', 100, 'done');
             INSERT INTO deep_analysis_jobs(track_id, status) VALUES ('t1', 'pending');",
        )
        .unwrap();
        let jobs = list_active_mix_jobs(&conn).unwrap();
        assert_eq!(jobs.len(), 2);
        let render = jobs.iter().find(|j| j.id == "j1").unwrap();
        assert_eq!(render.user.as_deref(), Some("alice"));
        assert_eq!(render.playlist_name.as_deref(), Some("Party"));
        assert_eq!(render.track_count, 1);
        assert_eq!(render.progress_percent, 64);
        assert!(!render.can_cancel);
        assert!(jobs.iter().find(|j| j.id == "j2").unwrap().can_cancel);
        assert_eq!(deep_analysis_active_counts(&conn).unwrap(), (1, 0));
    }

    #[test]
    fn item_summaries_cover_tracks_mixes_and_stations() {
        let conn = db();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO mix_outputs(id, job_id, playlist_id, user_id, file_path, file_name, name, duration_sec)
               VALUES ('m1', 'j1', 'p1', 'u1', '/x/a.mp3', 'a.mp3', 'Friday', 3600);
             INSERT INTO internet_radio_stations(id, name, stream_url) VALUES ('r1', 'Jazz FM', 'http://x');
             PRAGMA foreign_keys = ON;",
        )
        .unwrap();
        let ids = ["t1", "mx-m1", "st-r1", "missing"].map(String::from);
        let found = item_summaries(&conn, &ids).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(found["t1"].title, "Opening");
        assert_eq!(found["t1"].artist.as_deref(), Some("The Band"));
        assert_eq!(found["t1"].album_id.as_deref(), Some("al1"));
        assert_eq!(found["t1"].duration, Some(245.0));
        assert_eq!(found["mx-m1"].title, "Friday");
        assert_eq!(found["mx-m1"].duration, Some(3600.0));
        assert_eq!(found["st-r1"].title, "Jazz FM");
        assert_eq!(username(&conn, "u1").unwrap().as_deref(), Some("alice"));
    }
}

//! Scrobble forwarding storage: each user's connected Last.fm / ListenBrainz account (encrypted secret) and the
//! delivery queue the server's forwarder drains. Times are Unix seconds supplied by the caller.
//! See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-3).

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};

use crate::music::{coerce_entity_id, id_to_value};

/// Most queued plays kept per user and service (the oldest are dropped beyond this).
pub const MAX_QUEUED_PER_ACCOUNT: i64 = 1000;
/// Queued plays older than this are dropped (services refuse very old timestamps anyway).
pub const QUEUE_MAX_AGE_SECS: i64 = 14 * 24 * 3600;
/// Queued plays are dropped after this many failed attempts.
pub const MAX_ATTEMPTS: i64 = 20;

/// A scrobbling service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrobbleService {
    LastFm,
    ListenBrainz,
}

impl ScrobbleService {
    pub const ALL: [ScrobbleService; 2] = [ScrobbleService::LastFm, ScrobbleService::ListenBrainz];

    pub fn as_str(self) -> &'static str {
        match self {
            ScrobbleService::LastFm => "lastfm",
            ScrobbleService::ListenBrainz => "listenbrainz",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lastfm" => Some(ScrobbleService::LastFm),
            "listenbrainz" => Some(ScrobbleService::ListenBrainz),
            _ => None,
        }
    }
}

/// Creates the scrobble tables (part of migration `2026-10-19-subsonic-phase-2-5`).
pub fn ensure_scrobble_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scrobble_accounts (
           user_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           service           TEXT NOT NULL CHECK(service IN ('lastfm','listenbrainz')),
           secret_enc        BLOB NOT NULL,
           secret_nonce      BLOB NOT NULL,
           account_name      TEXT,
           needs_reconnect   INTEGER NOT NULL DEFAULT 0,
           connected_at      TEXT NOT NULL DEFAULT (datetime('now')),
           last_scrobble_at  TEXT,
           PRIMARY KEY (user_id, service)
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS scrobble_queue (
           id               INTEGER PRIMARY KEY AUTOINCREMENT,
           user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
           service          TEXT NOT NULL,
           artist           TEXT NOT NULL,
           title            TEXT NOT NULL,
           album            TEXT,
           duration_secs    INTEGER,
           played_at        INTEGER NOT NULL,
           attempts         INTEGER NOT NULL DEFAULT 0,
           next_attempt_at  INTEGER NOT NULL DEFAULT 0,
           created_at       INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_scrobble_queue_due ON scrobble_queue(next_attempt_at);
         CREATE INDEX IF NOT EXISTS idx_scrobble_queue_account ON scrobble_queue(user_id, service, id);",
    )
}

// ── Accounts ───────────────────────────────────────────────────────────────────

/// A stored account with its encrypted secret (Last.fm session key or ListenBrainz token).
#[derive(Debug, Clone, PartialEq)]
pub struct ScrobbleAccount {
    pub user_id: String,
    pub service: ScrobbleService,
    pub secret_enc: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    pub account_name: Option<String>,
    pub needs_reconnect: bool,
}

/// What the Settings UI shows for one connected account (never the secret).
#[derive(Debug, Clone, PartialEq)]
pub struct ScrobbleAccountStatus {
    pub service: ScrobbleService,
    pub account_name: Option<String>,
    pub needs_reconnect: bool,
    pub connected_at: String,
    pub last_scrobble_at: Option<String>,
    pub pending: i64,
}

/// Stores (or replaces) a user's account and clears any reconnect flag.
pub fn upsert_account(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
    secret_enc: &[u8],
    secret_nonce: &[u8],
    account_name: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO scrobble_accounts(user_id, service, secret_enc, secret_nonce, account_name)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(user_id, service) DO UPDATE SET
           secret_enc = excluded.secret_enc, secret_nonce = excluded.secret_nonce,
           account_name = excluded.account_name, needs_reconnect = 0, connected_at = datetime('now')",
        params![user_id, service.as_str(), secret_enc, secret_nonce, account_name],
    )?;
    Ok(())
}

/// Disconnects an account and drops its queued plays. True when an account existed.
pub fn delete_account(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
) -> rusqlite::Result<bool> {
    conn.execute(
        "DELETE FROM scrobble_queue WHERE user_id = ?1 AND service = ?2",
        params![user_id, service.as_str()],
    )?;
    let n = conn.execute(
        "DELETE FROM scrobble_accounts WHERE user_id = ?1 AND service = ?2",
        params![user_id, service.as_str()],
    )?;
    Ok(n > 0)
}

/// Drops every stored account and queued play (a new encryption key makes the secrets unreadable).
pub fn clear_all_accounts(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM scrobble_queue", [])?;
    conn.execute("DELETE FROM scrobble_accounts", [])
}

/// One account with its encrypted secret.
pub fn get_account(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
) -> rusqlite::Result<Option<ScrobbleAccount>> {
    conn.query_row(
        "SELECT secret_enc, secret_nonce, account_name, needs_reconnect
         FROM scrobble_accounts WHERE user_id = ?1 AND service = ?2",
        params![user_id, service.as_str()],
        |row| {
            Ok(ScrobbleAccount {
                user_id: user_id.to_owned(),
                service,
                secret_enc: row.get(0)?,
                secret_nonce: row.get(1)?,
                account_name: row.get(2)?,
                needs_reconnect: row.get::<_, i64>(3)? != 0,
            })
        },
    )
    .optional()
}

/// The user's accounts for the Settings UI, in service order.
pub fn list_accounts(
    conn: &Connection,
    user_id: &str,
) -> rusqlite::Result<Vec<ScrobbleAccountStatus>> {
    let mut stmt = conn.prepare(
        "SELECT a.service, a.account_name, a.needs_reconnect, a.connected_at, a.last_scrobble_at,
                (SELECT COUNT(*) FROM scrobble_queue q WHERE q.user_id = a.user_id AND q.service = a.service)
         FROM scrobble_accounts a WHERE a.user_id = ?1 ORDER BY a.service",
    )?;
    let rows = stmt.query_map([user_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ScrobbleAccountStatus {
                service: ScrobbleService::LastFm,
                account_name: row.get(1)?,
                needs_reconnect: row.get::<_, i64>(2)? != 0,
                connected_at: row.get(3)?,
                last_scrobble_at: row.get(4)?,
                pending: row.get(5)?,
            },
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (service, mut status) = row?;
        if let Some(service) = ScrobbleService::parse(&service) {
            status.service = service;
            out.push(status);
        }
    }
    Ok(out)
}

/// Services a user can currently scrobble to (connected and not waiting for a reconnect).
pub fn active_services(conn: &Connection, user_id: &str) -> rusqlite::Result<Vec<ScrobbleService>> {
    let mut stmt = conn.prepare(
        "SELECT service FROM scrobble_accounts WHERE user_id = ?1 AND needs_reconnect = 0 ORDER BY service",
    )?;
    let rows = stmt.query_map([user_id], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(s) = ScrobbleService::parse(&row?) {
            out.push(s);
        }
    }
    Ok(out)
}

/// Flags an account whose credentials the service rejected.
pub fn mark_needs_reconnect(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE scrobble_accounts SET needs_reconnect = 1 WHERE user_id = ?1 AND service = ?2",
        params![user_id, service.as_str()],
    )?;
    Ok(())
}

/// Records a successful delivery time.
pub fn mark_delivered(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE scrobble_accounts SET last_scrobble_at = datetime('now') WHERE user_id = ?1 AND service = ?2",
        params![user_id, service.as_str()],
    )?;
    Ok(())
}

// ── Queue ──────────────────────────────────────────────────────────────────────

/// What a scrobble says about a play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrobbleTrack {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_secs: Option<i64>,
}

/// One queued play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedScrobble {
    pub id: i64,
    pub user_id: String,
    pub service: ScrobbleService,
    pub track: ScrobbleTrack,
    pub played_at: i64,
    pub attempts: i64,
}

/// Artist, title, album and duration of a library track (`None` without a title or artist).
pub fn track_meta(conn: &Connection, track_id: &str) -> rusqlite::Result<Option<ScrobbleTrack>> {
    let id = id_to_value(&coerce_entity_id(track_id));
    let row = conn
        .query_row(
            "SELECT ar.name, t.title, al.title, t.duration FROM tracks t
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE t.id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((artist, title, album, duration)) = row else {
        return Ok(None);
    };
    let clean = |s: Option<String>| s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let (Some(artist), Some(title)) = (clean(artist), clean(title)) else {
        return Ok(None);
    };
    Ok(Some(ScrobbleTrack {
        artist,
        title,
        album: clean(album),
        duration_secs: duration
            .filter(|d| d.is_finite() && *d > 0.0)
            .map(|d| d.round() as i64),
    }))
}

/// Queues a play for delivery and trims the account's queue to [`MAX_QUEUED_PER_ACCOUNT`].
pub fn enqueue(
    conn: &Connection,
    user_id: &str,
    service: ScrobbleService,
    track: &ScrobbleTrack,
    played_at: i64,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO scrobble_queue(user_id, service, artist, title, album, duration_secs, played_at,
                                    next_attempt_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            user_id,
            service.as_str(),
            track.artist,
            track.title,
            track.album,
            track.duration_secs,
            played_at,
            now
        ],
    )?;
    conn.execute(
        "DELETE FROM scrobble_queue WHERE user_id = ?1 AND service = ?2 AND id NOT IN
           (SELECT id FROM scrobble_queue WHERE user_id = ?1 AND service = ?2 ORDER BY id DESC LIMIT ?3)",
        params![user_id, service.as_str(), MAX_QUEUED_PER_ACCOUNT],
    )?;
    Ok(())
}

/// Plays due for delivery, grouped by account (user, service) and oldest first within each.
pub fn due(conn: &Connection, now: i64, limit: i64) -> rusqlite::Result<Vec<QueuedScrobble>> {
    let mut stmt = conn.prepare(
        "SELECT q.id, q.user_id, q.service, q.artist, q.title, q.album, q.duration_secs, q.played_at, q.attempts
         FROM scrobble_queue q
         JOIN scrobble_accounts a ON a.user_id = q.user_id AND a.service = q.service AND a.needs_reconnect = 0
         WHERE q.next_attempt_at <= ?1
         ORDER BY q.user_id, q.service, q.played_at, q.id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![now, limit], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            ScrobbleTrack {
                artist: row.get(3)?,
                title: row.get(4)?,
                album: row.get(5)?,
                duration_secs: row.get(6)?,
            },
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, user_id, service, track, played_at, attempts) = row?;
        if let Some(service) = ScrobbleService::parse(&service) {
            out.push(QueuedScrobble {
                id,
                user_id,
                service,
                track,
                played_at,
                attempts,
            });
        }
    }
    Ok(out)
}

fn id_list(ids: &[i64]) -> (String, Vec<Value>) {
    (
        vec!["?"; ids.len()].join(","),
        ids.iter().map(|id| Value::Integer(*id)).collect(),
    )
}

/// Removes delivered (or permanently refused) plays.
pub fn remove(conn: &Connection, ids: &[i64]) -> rusqlite::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let (ph, values) = id_list(ids);
    conn.execute(
        &format!("DELETE FROM scrobble_queue WHERE id IN ({ph})"),
        params_from_iter(values),
    )
}

/// Schedules another attempt for plays that failed temporarily.
pub fn defer(conn: &Connection, ids: &[i64], next_attempt_at: i64) -> rusqlite::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let (ph, mut values) = id_list(ids);
    values.insert(0, Value::Integer(next_attempt_at));
    conn.execute(
        &format!(
            "UPDATE scrobble_queue SET attempts = attempts + 1, next_attempt_at = ? WHERE id IN ({ph})"
        ),
        params_from_iter(values),
    )
}

/// Drops plays that are too old or failed too often.
pub fn prune(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM scrobble_queue WHERE created_at < ?1 OR attempts >= ?2",
        params![now - QUEUE_MAX_AGE_SECS, MAX_ATTEMPTS],
    )
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
             INSERT INTO artists(id, name) VALUES ('ar1', 'The Band');
             INSERT INTO albums(id, title, artist_id) VALUES ('al1', 'First', 'ar1');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration) VALUES
               ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac', 200.4),
               ('t2', 'lib1', NULL, 'al1', 'No Artist', '/m/2.flac', 10);",
        )
        .unwrap();
        conn
    }

    fn track() -> ScrobbleTrack {
        ScrobbleTrack {
            artist: "The Band".into(),
            title: "Opening".into(),
            album: Some("First".into()),
            duration_secs: Some(200),
        }
    }

    #[test]
    fn service_names_round_trip() {
        for s in ScrobbleService::ALL {
            assert_eq!(ScrobbleService::parse(s.as_str()), Some(s));
        }
        assert_eq!(ScrobbleService::parse("spotify"), None);
    }

    #[test]
    fn accounts_upsert_list_reconnect_and_delete() {
        let conn = db();
        assert!(get_account(&conn, "u1", ScrobbleService::LastFm)
            .unwrap()
            .is_none());
        upsert_account(
            &conn,
            "u1",
            ScrobbleService::LastFm,
            b"ct",
            b"nonce",
            Some("al"),
        )
        .unwrap();
        upsert_account(
            &conn,
            "u1",
            ScrobbleService::ListenBrainz,
            b"ct2",
            b"n2",
            None,
        )
        .unwrap();
        let acct = get_account(&conn, "u1", ScrobbleService::LastFm)
            .unwrap()
            .unwrap();
        assert_eq!(acct.secret_enc, b"ct");
        assert_eq!(acct.account_name.as_deref(), Some("al"));
        assert!(!acct.needs_reconnect);
        assert_eq!(
            active_services(&conn, "u1").unwrap(),
            [ScrobbleService::LastFm, ScrobbleService::ListenBrainz]
        );

        mark_needs_reconnect(&conn, "u1", ScrobbleService::LastFm).unwrap();
        assert_eq!(
            active_services(&conn, "u1").unwrap(),
            [ScrobbleService::ListenBrainz]
        );
        enqueue(
            &conn,
            "u1",
            ScrobbleService::ListenBrainz,
            &track(),
            100,
            100,
        )
        .unwrap();
        mark_delivered(&conn, "u1", ScrobbleService::ListenBrainz).unwrap();
        let list = list_accounts(&conn, "u1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].service, ScrobbleService::LastFm);
        assert!(list[0].needs_reconnect);
        assert_eq!(list[1].pending, 1);
        assert!(list[1].last_scrobble_at.is_some());

        // Reconnecting clears the flag.
        upsert_account(
            &conn,
            "u1",
            ScrobbleService::LastFm,
            b"ct3",
            b"n3",
            Some("al"),
        )
        .unwrap();
        assert!(
            !get_account(&conn, "u1", ScrobbleService::LastFm)
                .unwrap()
                .unwrap()
                .needs_reconnect
        );

        assert!(delete_account(&conn, "u1", ScrobbleService::ListenBrainz).unwrap());
        assert!(!delete_account(&conn, "u1", ScrobbleService::ListenBrainz).unwrap());
        assert_eq!(list_accounts(&conn, "u1").unwrap()[0].pending, 0);
        assert_eq!(clear_all_accounts(&conn).unwrap(), 1);
        assert!(list_accounts(&conn, "u1").unwrap().is_empty());
    }

    #[test]
    fn track_meta_needs_artist_and_title() {
        let conn = db();
        let meta = track_meta(&conn, "t1").unwrap().unwrap();
        assert_eq!(meta, track());
        assert!(track_meta(&conn, "t2").unwrap().is_none());
        assert!(track_meta(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn queue_due_defer_remove_prune_and_cap() {
        let conn = db();
        upsert_account(&conn, "u1", ScrobbleService::LastFm, b"c", b"n", None).unwrap();
        enqueue(&conn, "u1", ScrobbleService::LastFm, &track(), 300, 1000).unwrap();
        enqueue(&conn, "u1", ScrobbleService::LastFm, &track(), 200, 1000).unwrap();
        // No account → never due.
        enqueue(&conn, "u2", ScrobbleService::LastFm, &track(), 100, 1000).unwrap();
        let due_now = due(&conn, 1000, 50).unwrap();
        assert_eq!(due_now.len(), 2);
        assert_eq!(due_now[0].played_at, 200, "oldest play first");
        assert!(due(&conn, 999, 50).unwrap().is_empty());

        defer(&conn, &[due_now[0].id], 2000).unwrap();
        let later = due(&conn, 1500, 50).unwrap();
        assert_eq!(later.len(), 1);
        assert_eq!(due(&conn, 2000, 50).unwrap()[0].attempts, 1);
        assert_eq!(remove(&conn, &[due_now[1].id]).unwrap(), 1);
        assert_eq!(remove(&conn, &[]).unwrap(), 0);
        assert_eq!(defer(&conn, &[], 1).unwrap(), 0);

        // Too old and too many attempts are pruned.
        conn.execute(
            "UPDATE scrobble_queue SET attempts = ?1 WHERE id = ?2",
            params![MAX_ATTEMPTS, due_now[0].id],
        )
        .unwrap();
        assert_eq!(prune(&conn, 1000).unwrap(), 1);
        assert_eq!(prune(&conn, 1000 + QUEUE_MAX_AGE_SECS + 1).unwrap(), 1);

        for i in 0..(MAX_QUEUED_PER_ACCOUNT + 3) {
            enqueue(&conn, "u1", ScrobbleService::LastFm, &track(), i, 5000).unwrap();
        }
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scrobble_queue WHERE user_id = 'u1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, MAX_QUEUED_PER_ACCOUNT);
        let oldest: i64 = conn
            .query_row(
                "SELECT MIN(played_at) FROM scrobble_queue WHERE user_id = 'u1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(oldest, 3);
    }
}

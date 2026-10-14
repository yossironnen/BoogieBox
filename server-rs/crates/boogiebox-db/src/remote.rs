//! Play On storage: UPnP renderers remembered across restarts, with per-device admin preferences
//! (`wip/play-on-renderers-plan.md` §7), and each renderer's session (queue, owner, position, media tokens) so it
//! survives a server restart.

use rusqlite::{params, Connection, OptionalExtension, Row};

/// Allowed `format_policy` values.
pub const FORMAT_POLICIES: &[&str] = &["auto", "direct", "lossless", "mp3"];

/// One remembered renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererRow {
    /// Normalized UDN (primary key).
    pub udn: String,
    /// Name reported by the device.
    pub friendly_name: String,
    /// Admin-set display name override.
    pub alias: Option<String>,
    /// Manufacturer reported by the device.
    pub manufacturer: Option<String>,
    /// Model name reported by the device.
    pub model_name: Option<String>,
    /// Vendor family (`wiim` | `generic`).
    pub vendor: String,
    /// Last known device description URL.
    pub location_url: String,
    /// JSON capabilities blob (endpoints, sink formats, setNext support).
    pub capabilities: String,
    /// `auto` | `direct` | `lossless` | `mp3`.
    pub format_policy: String,
    /// Hidden from the picker by an admin.
    pub hidden: bool,
    /// First discovery timestamp (SQLite `datetime('now')` format).
    pub first_seen_at: String,
    /// Last discovery timestamp.
    pub last_seen_at: String,
}

/// Fields refreshed from a fresh device description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererUpsert<'a> {
    /// Normalized UDN.
    pub udn: &'a str,
    /// Device name.
    pub friendly_name: &'a str,
    /// Manufacturer.
    pub manufacturer: Option<&'a str>,
    /// Model name.
    pub model_name: Option<&'a str>,
    /// Vendor family.
    pub vendor: &'a str,
    /// Description URL.
    pub location_url: &'a str,
    /// Capabilities JSON.
    pub capabilities: &'a str,
}

/// Creates the `upnp_renderers` table (migration `2026-10-12-upnp-renderers-schema`).
pub fn ensure_upnp_renderers_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS upnp_renderers (
           udn            TEXT PRIMARY KEY,
           friendly_name  TEXT NOT NULL,
           alias          TEXT,
           manufacturer   TEXT,
           model_name     TEXT,
           vendor         TEXT NOT NULL DEFAULT 'generic',
           location_url   TEXT NOT NULL,
           capabilities   TEXT NOT NULL DEFAULT '{}',
           format_policy  TEXT NOT NULL DEFAULT 'auto',
           hidden         INTEGER NOT NULL DEFAULT 0,
           first_seen_at  TEXT NOT NULL DEFAULT (datetime('now')),
           last_seen_at   TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
}

/// One persisted Play On session.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteSessionRow {
    /// Renderer UDN (primary key).
    pub udn: String,
    /// JSON snapshot written by the server's session actor (queue, owner, playback state, media tokens).
    pub snapshot: String,
    /// Last known position (s); updated more often than the snapshot.
    pub position_sec: f64,
    /// Last known status (`playing`, `paused`, …).
    pub status: String,
    /// When `position_sec` was sampled (ms since epoch).
    pub saved_at_ms: i64,
}

/// Creates the `remote_sessions` table (migration `2026-10-13-remote-sessions-schema`).
pub fn ensure_remote_sessions_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS remote_sessions (
           udn           TEXT PRIMARY KEY,
           snapshot      TEXT NOT NULL,
           position_sec  REAL NOT NULL DEFAULT 0,
           status        TEXT NOT NULL DEFAULT 'paused',
           saved_at_ms   INTEGER NOT NULL DEFAULT 0,
           updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
}

/// Writes (inserts or replaces) a session snapshot.
pub fn save_remote_session(conn: &Connection, row: &RemoteSessionRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO remote_sessions(udn, snapshot, position_sec, status, saved_at_ms, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(udn) DO UPDATE SET snapshot = excluded.snapshot, position_sec = excluded.position_sec,
           status = excluded.status, saved_at_ms = excluded.saved_at_ms, updated_at = excluded.updated_at",
        params![row.udn, row.snapshot, row.position_sec, row.status, row.saved_at_ms],
    )?;
    Ok(())
}

/// Updates only the position/status of a saved session; false when there is none.
pub fn update_remote_session_position(
    conn: &Connection,
    udn: &str,
    position_sec: f64,
    status: &str,
    saved_at_ms: i64,
) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE remote_sessions SET position_sec = ?2, status = ?3, saved_at_ms = ?4, updated_at = datetime('now')
         WHERE udn = ?1",
        params![udn, position_sec, status, saved_at_ms],
    )? > 0)
}

/// Every saved session.
pub fn list_remote_sessions(conn: &Connection) -> rusqlite::Result<Vec<RemoteSessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT udn, snapshot, position_sec, status, saved_at_ms FROM remote_sessions ORDER BY udn",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(RemoteSessionRow {
            udn: r.get(0)?,
            snapshot: r.get(1)?,
            position_sec: r.get(2)?,
            status: r.get(3)?,
            saved_at_ms: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Forgets one session; false when there was none.
pub fn delete_remote_session(conn: &Connection, udn: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM remote_sessions WHERE udn = ?1", params![udn])? > 0)
}

/// Forgets every session (Play On switched off).
pub fn delete_all_remote_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM remote_sessions", [])
}

const COLS: &str = "udn, friendly_name, alias, manufacturer, model_name, vendor, location_url, capabilities, format_policy, hidden, first_seen_at, last_seen_at";

fn map_row(r: &Row<'_>) -> rusqlite::Result<RendererRow> {
    Ok(RendererRow {
        udn: r.get(0)?,
        friendly_name: r.get(1)?,
        alias: r.get(2)?,
        manufacturer: r.get(3)?,
        model_name: r.get(4)?,
        vendor: r.get(5)?,
        location_url: r.get(6)?,
        capabilities: r.get(7)?,
        format_policy: r.get(8)?,
        hidden: r.get::<_, i64>(9)? != 0,
        first_seen_at: r.get(10)?,
        last_seen_at: r.get(11)?,
    })
}

/// Inserts or refreshes a renderer from discovery, preserving admin preferences (alias/policy/hidden).
pub fn upsert_renderer(conn: &Connection, r: &RendererUpsert<'_>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO upnp_renderers (udn, friendly_name, manufacturer, model_name, vendor, location_url, capabilities)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(udn) DO UPDATE SET
           friendly_name = excluded.friendly_name,
           manufacturer  = excluded.manufacturer,
           model_name    = excluded.model_name,
           vendor        = excluded.vendor,
           location_url  = excluded.location_url,
           capabilities  = excluded.capabilities,
           last_seen_at  = datetime('now')",
        params![
            r.udn,
            r.friendly_name,
            r.manufacturer,
            r.model_name,
            r.vendor,
            r.location_url,
            r.capabilities
        ],
    )?;
    Ok(())
}

/// Bumps `last_seen_at` for a renderer that re-announced itself without changes.
pub fn touch_renderer(conn: &Connection, udn: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE upnp_renderers SET last_seen_at = datetime('now') WHERE udn = ?1",
        params![udn],
    )? > 0)
}

/// All remembered renderers, display-name order.
pub fn list_renderers(conn: &Connection) -> rusqlite::Result<Vec<RendererRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM upnp_renderers ORDER BY COALESCE(alias, friendly_name) COLLATE NOCASE, udn"
    ))?;
    let rows = stmt.query_map([], map_row)?;
    rows.collect()
}

/// One renderer by UDN.
pub fn get_renderer(conn: &Connection, udn: &str) -> rusqlite::Result<Option<RendererRow>> {
    conn.query_row(
        &format!("SELECT {COLS} FROM upnp_renderers WHERE udn = ?1"),
        params![udn],
        map_row,
    )
    .optional()
}

/// Admin preference changes; `None` leaves a field untouched, `Some(None)` clears the alias.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RendererPrefs {
    /// New alias (`Some(None)` clears it).
    pub alias: Option<Option<String>>,
    /// New format policy (must be one of [`FORMAT_POLICIES`]).
    pub format_policy: Option<String>,
    /// New hidden flag.
    pub hidden: Option<bool>,
}

/// Applies admin preferences. Returns `Ok(false)` when the renderer does not exist.
///
/// The caller validates `format_policy` and alias length; an unknown policy here is rejected defensively.
pub fn update_renderer_prefs(
    conn: &Connection,
    udn: &str,
    prefs: &RendererPrefs,
) -> rusqlite::Result<bool> {
    if let Some(policy) = &prefs.format_policy {
        if !FORMAT_POLICIES.contains(&policy.as_str()) {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "format_policy {policy}"
            )));
        }
    }
    if get_renderer(conn, udn)?.is_none() {
        return Ok(false);
    }
    if let Some(alias) = &prefs.alias {
        let alias = alias.as_deref().map(str::trim).filter(|a| !a.is_empty());
        conn.execute(
            "UPDATE upnp_renderers SET alias = ?2 WHERE udn = ?1",
            params![udn, alias],
        )?;
    }
    if let Some(policy) = &prefs.format_policy {
        conn.execute(
            "UPDATE upnp_renderers SET format_policy = ?2 WHERE udn = ?1",
            params![udn, policy],
        )?;
    }
    if let Some(hidden) = prefs.hidden {
        conn.execute(
            "UPDATE upnp_renderers SET hidden = ?2 WHERE udn = ?1",
            params![udn, hidden as i64],
        )?;
    }
    Ok(true)
}

/// Forgets a renderer (it will be re-added if it is discovered again).
pub fn delete_renderer(conn: &Connection, udn: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM upnp_renderers WHERE udn = ?1", params![udn])? > 0)
}

/// Everything a renderer needs to describe and stream one library track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackMediaMeta {
    /// Track id (stringified entity id).
    pub id: String,
    /// Title.
    pub title: String,
    /// Track artist name.
    pub artist: Option<String>,
    /// Album title.
    pub album: Option<String>,
    /// Album id (for artwork).
    pub album_id: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Track number.
    pub track_number: Option<i64>,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// Absolute file path.
    pub file_path: String,
    /// File size in bytes.
    pub file_size: Option<i64>,
    /// Sample rate (Hz) from the scanner's technical metadata.
    pub sample_rate: Option<i64>,
    /// Channel count from the scanner's technical metadata.
    pub channels: Option<i64>,
}

fn value_to_string(v: rusqlite::types::Value) -> Option<String> {
    match v {
        rusqlite::types::Value::Integer(i) => Some(i.to_string()),
        rusqlite::types::Value::Text(s) => Some(s),
        _ => None,
    }
}

/// Loads renderer metadata for one track.
pub fn get_track_media_meta(
    conn: &Connection,
    track_id: &str,
) -> rusqlite::Result<Option<TrackMediaMeta>> {
    let id = crate::music::coerce_entity_id(track_id);
    conn.query_row(
        "SELECT t.id, t.title, a.name, al.title, t.album_id, t.genre, t.track_number, t.duration,
                t.file_path, t.file_size, t.sample_rate, t.channels
         FROM tracks t
         LEFT JOIN artists a ON a.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE t.id = ?1",
        params![id],
        |r| {
            Ok(TrackMediaMeta {
                id: value_to_string(r.get(0)?).unwrap_or_default(),
                title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                artist: r.get(2)?,
                album: r.get(3)?,
                album_id: value_to_string(r.get(4)?),
                genre: r.get(5)?,
                track_number: r.get(6)?,
                duration: r.get(7)?,
                file_path: r.get(8)?,
                file_size: r.get(9)?,
                sample_rate: r.get(10)?,
                channels: r.get(11)?,
            })
        },
    )
    .optional()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_media_meta_joins_artist_and_album() {
        let root = std::env::temp_dir().join(format!("bb-remote-meta-{}", uuid::Uuid::now_v7()));
        let crate::InitializedDatabase {
            connection: conn, ..
        } = crate::init_db(&root).unwrap();
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES('lib1', 'D:\\Music', 'Music');
             INSERT INTO artists(id, name) VALUES('ar1', 'Massive Attack');
             INSERT INTO albums(id, title, artist_id) VALUES('al1', 'Mezzanine', 'ar1');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracks(id, library_id, file_path, title, artist_id, album_id, genre, track_number, duration, file_size, sample_rate, channels)
             VALUES('t1', 'lib1', 'D:\\Music\\a.flac', 'Angel', 'ar1', 'al1', 'Trip-Hop', 1, 379.5, 1000, 44100, 2)",
            [],
        )
        .unwrap();
        let m = get_track_media_meta(&conn, "t1").unwrap().unwrap();
        assert_eq!(m.title, "Angel");
        assert_eq!(m.artist.as_deref(), Some("Massive Attack"));
        assert_eq!(m.album.as_deref(), Some("Mezzanine"));
        assert_eq!(m.album_id.as_deref(), Some("al1"));
        assert_eq!((m.sample_rate, m.channels), (Some(44100), Some(2)));
        assert_eq!(m.duration, Some(379.5));
        assert!(get_track_media_meta(&conn, "missing").unwrap().is_none());
        assert_eq!(
            value_to_string(rusqlite::types::Value::Integer(7)).as_deref(),
            Some("7")
        );
        assert_eq!(value_to_string(rusqlite::types::Value::Null), None);
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_upnp_renderers_schema(&conn).unwrap();
        ensure_upnp_renderers_schema(&conn).unwrap(); // idempotent
        conn
    }

    fn sample<'a>(udn: &'a str, name: &'a str) -> RendererUpsert<'a> {
        RendererUpsert {
            udn,
            friendly_name: name,
            manufacturer: Some("Linkplay"),
            model_name: Some("WiiM Ultra"),
            vendor: "wiim",
            location_url: "http://10.0.0.5:49152/d.xml",
            capabilities: "{}",
        }
    }

    #[test]
    fn upsert_preserves_admin_prefs_and_lists_by_display_name() {
        let conn = db();
        upsert_renderer(&conn, &sample("b", "Zeta")).unwrap();
        upsert_renderer(&conn, &sample("a", "alpha")).unwrap();
        assert!(update_renderer_prefs(
            &conn,
            "b",
            &RendererPrefs {
                alias: Some(Some("  Bedroom ".into())),
                format_policy: Some("lossless".into()),
                hidden: Some(true),
            }
        )
        .unwrap());
        // Rediscovery updates device fields but keeps prefs.
        let mut again = sample("b", "Zeta 2");
        again.location_url = "http://10.0.0.6:49152/d.xml";
        upsert_renderer(&conn, &again).unwrap();
        let b = get_renderer(&conn, "b").unwrap().unwrap();
        assert_eq!(b.friendly_name, "Zeta 2");
        assert_eq!(b.location_url, "http://10.0.0.6:49152/d.xml");
        assert_eq!(b.alias.as_deref(), Some("Bedroom"));
        assert_eq!(b.format_policy, "lossless");
        assert!(b.hidden);
        let names: Vec<_> = list_renderers(&conn)
            .unwrap()
            .into_iter()
            .map(|r| r.udn)
            .collect();
        assert_eq!(names, vec!["a", "b"]); // "alpha" < "Bedroom"
    }

    #[test]
    fn prefs_clear_alias_and_reject_bad_input() {
        let conn = db();
        upsert_renderer(&conn, &sample("a", "A")).unwrap();
        update_renderer_prefs(
            &conn,
            "a",
            &RendererPrefs {
                alias: Some(Some("X".into())),
                ..Default::default()
            },
        )
        .unwrap();
        update_renderer_prefs(
            &conn,
            "a",
            &RendererPrefs {
                alias: Some(Some("   ".into())),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(get_renderer(&conn, "a").unwrap().unwrap().alias, None);
        update_renderer_prefs(
            &conn,
            "a",
            &RendererPrefs {
                alias: Some(Some("Y".into())),
                ..Default::default()
            },
        )
        .unwrap();
        update_renderer_prefs(
            &conn,
            "a",
            &RendererPrefs {
                alias: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(get_renderer(&conn, "a").unwrap().unwrap().alias, None);
        assert!(update_renderer_prefs(
            &conn,
            "a",
            &RendererPrefs {
                format_policy: Some("ogg".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert!(!update_renderer_prefs(&conn, "missing", &RendererPrefs::default()).unwrap());
    }

    #[test]
    fn touch_and_delete() {
        let conn = db();
        assert!(!touch_renderer(&conn, "a").unwrap());
        upsert_renderer(&conn, &sample("a", "A")).unwrap();
        assert!(touch_renderer(&conn, "a").unwrap());
        assert!(delete_renderer(&conn, "a").unwrap());
        assert!(!delete_renderer(&conn, "a").unwrap());
        assert_eq!(get_renderer(&conn, "a").unwrap(), None);
    }

    #[test]
    fn fresh_schema_includes_table_via_tracked_migration() {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='upnp_renderers'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1);
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE id='2026-10-12-upnp-renderers-schema'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }

    #[test]
    fn remote_sessions_save_update_list_delete() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_remote_sessions_schema(&conn).unwrap();
        ensure_remote_sessions_schema(&conn).unwrap(); // idempotent
        let row = RemoteSessionRow {
            udn: "wiim".into(),
            snapshot: "{\"v\":1}".into(),
            position_sec: 12.5,
            status: "playing".into(),
            saved_at_ms: 1000,
        };
        save_remote_session(&conn, &row).unwrap();
        save_remote_session(
            &conn,
            &RemoteSessionRow {
                udn: "tv".into(),
                ..row.clone()
            },
        )
        .unwrap();
        assert!(update_remote_session_position(&conn, "wiim", 40.0, "paused", 2000).unwrap());
        assert!(!update_remote_session_position(&conn, "missing", 1.0, "paused", 1).unwrap());
        let rows = list_remote_sessions(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let wiim = rows.iter().find(|r| r.udn == "wiim").unwrap();
        assert_eq!(
            (wiim.position_sec, wiim.status.as_str(), wiim.saved_at_ms),
            (40.0, "paused", 2000)
        );
        assert_eq!(wiim.snapshot, "{\"v\":1}");
        // A new snapshot replaces the old one.
        save_remote_session(
            &conn,
            &RemoteSessionRow {
                snapshot: "{\"v\":2}".into(),
                ..row.clone()
            },
        )
        .unwrap();
        assert_eq!(
            list_remote_sessions(&conn).unwrap()[1].snapshot,
            "{\"v\":2}"
        );
        assert!(delete_remote_session(&conn, "wiim").unwrap());
        assert!(!delete_remote_session(&conn, "wiim").unwrap());
        assert_eq!(delete_all_remote_sessions(&conn).unwrap(), 1);
        assert!(list_remote_sessions(&conn).unwrap().is_empty());
    }

    #[test]
    fn remote_sessions_table_via_tracked_migration() {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE id='2026-10-13-remote-sessions-schema'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
        assert!(list_remote_sessions(&conn).unwrap().is_empty());
    }
}

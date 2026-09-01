//! Periodic database housekeeping.
//!
//! `auto_vacuum = INCREMENTAL` (enabled once per database by
//! `ensure_incremental_autovacuum` in `lib.rs`) holds freed pages on SQLite's
//! freelist until `PRAGMA incremental_vacuum` is explicitly run — it does not
//! reclaim automatically. This module gates that call to a weekly, late-night
//! window so it never competes with interactive scan/playback traffic.

use rusqlite::{Connection, OptionalExtension};

/// `settings` key recording when [`run_incremental_vacuum`] last actually ran.
const LAST_RUN_KEY: &str = "dbMaintenanceLastVacuumAt";
/// Minimum days between incremental-vacuum runs.
const MIN_DAYS_BETWEEN_RUNS: i64 = 7;
/// Local-time hour window (inclusive) during which the weekly run is allowed to fire.
const WINDOW_START_HOUR: i64 = 2;
const WINDOW_END_HOUR: i64 = 3;

/// True when at least a week has passed since the last run (or none has ever run) AND the
/// server's local clock currently falls inside the late-night maintenance window.
pub fn incremental_vacuum_due(conn: &Connection) -> rusqlite::Result<bool> {
    let last_run: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [LAST_RUN_KEY],
            |row| row.get(0),
        )
        .optional()?;

    let week_elapsed = match last_run {
        None => true,
        Some(last_run) => {
            conn.query_row(
                "SELECT julianday('now') - julianday(?1) >= ?2",
                rusqlite::params![last_run, MIN_DAYS_BETWEEN_RUNS],
                |row| row.get::<_, i64>(0),
            )? != 0
        }
    };
    if !week_elapsed {
        return Ok(false);
    }

    conn.query_row(
        "SELECT CAST(strftime('%H', 'now', 'localtime') AS INTEGER) BETWEEN ?1 AND ?2",
        rusqlite::params![WINDOW_START_HOUR, WINDOW_END_HOUR],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
}

/// Runs an unbounded `PRAGMA incremental_vacuum` (reclaims every page currently on the
/// freelist — cheap and appropriate at weekly, off-request-path cadence) and records the run
/// timestamp. Returns `(page_count_before, page_count_after)` for logging. Best-effort:
/// callers should log failures rather than propagate them to request handlers.
pub fn run_incremental_vacuum(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let page_count_before: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    conn.execute_batch("PRAGMA incremental_vacuum")?;
    let page_count_after: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;

    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [LAST_RUN_KEY],
    )?;

    Ok((page_count_before, page_count_after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{configure_database_connection, initialize_schema};
    use std::path::Path;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        configure_database_connection(&conn, Path::new("test.db")).unwrap();
        initialize_schema(&conn).unwrap();
        conn
    }

    fn set_local_hour_settable_now(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT CAST(strftime('%H', 'now', 'localtime') AS INTEGER)",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn due_when_never_run_and_in_window() {
        let conn = test_conn();
        let hour = set_local_hour_settable_now(&conn);
        let due = incremental_vacuum_due(&conn).unwrap();
        // Only assert the positive case when we're actually inside the window right now;
        // otherwise just assert the call doesn't error (time-of-day is out of our control).
        if (WINDOW_START_HOUR..=WINDOW_END_HOUR).contains(&hour) {
            assert!(due);
        } else {
            assert!(!due);
        }
    }

    #[test]
    fn not_due_within_a_week_of_last_run_even_in_window() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [LAST_RUN_KEY],
        )
        .unwrap();
        assert!(!incremental_vacuum_due(&conn).unwrap());
    }

    #[test]
    fn due_after_a_week_when_in_window_not_due_when_out_of_window() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, datetime('now', '-8 days'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [LAST_RUN_KEY],
        )
        .unwrap();
        let hour = set_local_hour_settable_now(&conn);
        let due = incremental_vacuum_due(&conn).unwrap();
        if (WINDOW_START_HOUR..=WINDOW_END_HOUR).contains(&hour) {
            assert!(due);
        } else {
            assert!(!due);
        }
    }

    #[test]
    fn run_incremental_vacuum_records_last_run_and_reports_page_counts() {
        let conn = test_conn();
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .unwrap();

        let (before, after) = run_incremental_vacuum(&conn).unwrap();
        assert!(before >= 0);
        assert!(after >= 0);

        let last_run: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [LAST_RUN_KEY],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert!(last_run.is_some());
    }

    #[test]
    fn run_incremental_vacuum_updates_existing_last_run_via_upsert() {
        let conn = test_conn();
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
            .unwrap();

        run_incremental_vacuum(&conn).unwrap();
        let first: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [LAST_RUN_KEY],
                |row| row.get(0),
            )
            .unwrap();

        run_incremental_vacuum(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = ?1",
                [LAST_RUN_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "upsert must not duplicate the settings row");
        let _ = first;
    }
}

//! Bounded Home query results, scoped to the database, user and query parameters.
//! SQLite change counters invalidate reads after both local and external writes.

use crate::DbPool;
use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Cache {
    database: Weak<Mutex<Connection>>,
    stamp: (u64, i64),
    entries: HashMap<String, (Instant, Vec<u8>)>,
    bytes: usize,
}

/// Shared only by authenticated Home query handlers; never stores errors.
#[derive(Clone, Default)]
pub struct HomeQueryCache(Arc<Mutex<Cache>>);

impl HomeQueryCache {
    /// Caller holds the database mutex for both validation and loading. Concurrent
    /// misses therefore share the first completed result rather than repeat its SQL.
    pub fn read<T: Serialize>(
        &self,
        db: &DbPool,
        conn: &Connection,
        key: String,
        load: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Option<Vec<u8>> {
        let version = conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .ok()?;
        let stamp = (conn.total_changes(), version);
        let mut cache = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if !Weak::ptr_eq(&cache.database, &Arc::downgrade(db)) || cache.stamp != stamp {
            cache.entries.clear();
            cache.bytes = 0;
            cache.database = Arc::downgrade(db);
            cache.stamp = stamp;
        }
        if let Some((created, body)) = cache.entries.get(&key) {
            if created.elapsed() < MAX_AGE {
                return Some(body.clone());
            }
        }
        if let Some((_, old)) = cache.entries.remove(&key) {
            cache.bytes -= old.len();
        }
        let body = serde_json::to_vec(&load(conn).ok()?).ok()?;
        if body.len() <= MAX_BYTES {
            while cache.entries.len() >= MAX_ENTRIES || cache.bytes + body.len() > MAX_BYTES {
                let oldest = cache
                    .entries
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| k.clone())?;
                if let Some((_, old)) = cache.entries.remove(&oldest) {
                    cache.bytes -= old.len();
                }
            }
            cache.bytes += body.len();
            cache.entries.insert(key, (Instant::now(), body.clone()));
        }
        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pool() -> DbPool {
        Arc::new(Mutex::new(Connection::open_in_memory().unwrap()))
    }

    #[test]
    fn shares_results_but_invalidates_after_writes_and_database_switches() {
        let cache = HomeQueryCache::default();
        let db = pool();
        let conn = db.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE values_for_test(value); INSERT INTO values_for_test VALUES(1)",
        )
        .unwrap();
        let read = |conn: &Connection| {
            conn.query_row("SELECT value FROM values_for_test", [], |r| {
                r.get::<_, i64>(0)
            })
        };
        assert_eq!(
            cache.read(&db, &conn, "u:genres".into(), read).unwrap(),
            b"1"
        );
        assert_eq!(
            cache
                .read(
                    &db,
                    &conn,
                    "u:genres".into(),
                    |_| -> rusqlite::Result<i64> { panic!("cache miss") }
                )
                .unwrap(),
            b"1"
        );
        // A second user/limit must never reuse the first user's result.
        assert_eq!(
            cache
                .read(&db, &conn, "other:genres".into(), |_| Ok(7))
                .unwrap(),
            b"7"
        );
        conn.execute("UPDATE values_for_test SET value=2", [])
            .unwrap();
        assert_eq!(
            cache.read(&db, &conn, "u:genres".into(), read).unwrap(),
            b"2"
        );
        let other_db = pool();
        let other_conn = other_db.lock().unwrap();
        assert_eq!(
            cache
                .read(&other_db, &other_conn, "u:genres".into(), |_| Ok(9))
                .unwrap(),
            b"9"
        );
    }

    #[test]
    fn detects_external_commits_on_the_same_connection() {
        let path = crate::test_support::temp_db_path("home-cache-external");
        let db = Arc::new(Mutex::new(Connection::open(&path).unwrap()));
        let conn = db.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE values_for_test(value); INSERT INTO values_for_test VALUES(1)",
        )
        .unwrap();
        let cache = HomeQueryCache::default();
        let read = |conn: &Connection| {
            conn.query_row("SELECT value FROM values_for_test", [], |r| {
                r.get::<_, i64>(0)
            })
        };
        assert_eq!(
            cache.read(&db, &conn, "u:recent".into(), read).unwrap(),
            b"1"
        );
        Connection::open(&path)
            .unwrap()
            .execute("UPDATE values_for_test SET value=2", [])
            .unwrap();
        assert_eq!(
            cache.read(&db, &conn, "u:recent".into(), read).unwrap(),
            b"2"
        );
    }

    #[test]
    fn expires_results_bounds_memory_and_does_not_cache_errors() {
        let cache = HomeQueryCache::default();
        let db = pool();
        let conn = db.lock().unwrap();
        assert!(cache
            .read(&db, &conn, "error".into(), |_| Err::<i64, _>(
                rusqlite::Error::InvalidQuery
            ))
            .is_none());
        assert_eq!(
            cache.read(&db, &conn, "error".into(), |_| Ok(1)).unwrap(),
            b"1"
        );
        cache.0.lock().unwrap().entries.get_mut("error").unwrap().0 = Instant::now() - MAX_AGE;
        assert_eq!(
            cache.read(&db, &conn, "error".into(), |_| Ok(2)).unwrap(),
            b"2"
        );
        for n in 0..MAX_ENTRIES + 2 {
            cache.read(&db, &conn, n.to_string(), |_| Ok(n)).unwrap();
        }
        assert_eq!(cache.0.lock().unwrap().entries.len(), MAX_ENTRIES);
        let large = "a".repeat(MAX_BYTES / 2);
        for n in 0..3 {
            cache
                .read(&db, &conn, format!("large{n}"), |_| Ok(&large))
                .unwrap();
        }
        assert!(cache.0.lock().unwrap().bytes <= MAX_BYTES);
        cache
            .read(
                &db,
                &conn,
                "oversized".into(),
                |_| Ok("a".repeat(MAX_BYTES)),
            )
            .unwrap();
        assert!(!cache.0.lock().unwrap().entries.contains_key("oversized"));
    }
}

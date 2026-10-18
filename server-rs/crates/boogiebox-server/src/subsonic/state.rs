//! In-memory Subsonic state kept in `AppState`: the enabled flag, decrypted-credential cache, encryption key
//! and transcode limiter. Caches keep SQLite off the per-request path (a client sync sends thousands of
//! requests); everything is dropped on a database switch.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::sync::Semaphore;

use crate::DbPool;

const SETTINGS_TTL: Duration = Duration::from_secs(10);
const ACCOUNT_TTL: Duration = Duration::from_secs(60);
const MAX_CACHED_ACCOUNTS: usize = 1024;
const USE_RECORD_INTERVAL: Duration = Duration::from_secs(300);
/// How long a "now playing" notice stays listed without a refresh.
const NOW_PLAYING_TTL: Duration = Duration::from_secs(600);
const MAX_NOW_PLAYING: usize = 64;
/// Default for `subsonicMaxTranscodes`.
pub const DEFAULT_MAX_TRANSCODES: usize = 3;
/// Default for `subsonicIgnoredArticles`.
pub const DEFAULT_IGNORED_ARTICLES: &str = "The El La Los Las Le Les";

/// The account a Subsonic request runs as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsonicUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub can_manage_libraries: bool,
    pub can_edit_metadata: bool,
}

impl SubsonicUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// Settings read from the database, cached briefly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsonicSettings {
    pub enabled: bool,
    pub max_transcodes: usize,
    pub ignored_articles: String,
}

impl Default for SubsonicSettings {
    fn default() -> Self {
        SubsonicSettings {
            enabled: false,
            max_transcodes: DEFAULT_MAX_TRANSCODES,
            ignored_articles: DEFAULT_IGNORED_ARTICLES.to_owned(),
        }
    }
}

/// A Subsonic client's "now playing" notice (`scrobble` with `submission=false`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlayingEntry {
    pub user_id: String,
    pub username: String,
    /// Client name (`c` parameter).
    pub client: String,
    /// Song id (`mx-…` for a BoogieMix mix).
    pub track_id: String,
    pub at: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedAccount {
    pub user: Option<SubsonicUser>,
    /// Decrypted Subsonic password; `None` when unset or undecryptable.
    pub secret: Option<String>,
    pub fetched: Instant,
}

#[derive(Debug, Default)]
struct Inner {
    settings: Mutex<Option<(SubsonicSettings, Instant)>>,
    accounts: Mutex<HashMap<String, CachedAccount>>,
    key: Mutex<Option<(PathBuf, [u8; 32])>>,
    last_use: Mutex<HashMap<String, Instant>>,
    transcodes: Mutex<Option<(usize, Arc<Semaphore>)>>,
    now_playing: Mutex<Vec<NowPlayingEntry>>,
}

/// Shared, cheaply cloneable Subsonic state.
#[derive(Debug, Clone, Default)]
pub struct SubsonicState(Arc<Inner>);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl SubsonicState {
    /// Drops every cache (database switched).
    pub fn reset(&self) {
        *lock(&self.0.settings) = None;
        lock(&self.0.accounts).clear();
        *lock(&self.0.key) = None;
        lock(&self.0.last_use).clear();
        lock(&self.0.now_playing).clear();
    }

    /// Records what a user's client is playing (one entry per user + client).
    pub fn set_now_playing(&self, entry: NowPlayingEntry) {
        let mut list = lock(&self.0.now_playing);
        list.retain(|e| {
            e.at.elapsed() < NOW_PLAYING_TTL
                && !(e.user_id == entry.user_id && e.client == entry.client)
        });
        if list.len() >= MAX_NOW_PLAYING {
            list.remove(0);
        }
        list.push(entry);
    }

    /// Current "now playing" notices, newest first.
    pub fn now_playing(&self) -> Vec<NowPlayingEntry> {
        let mut list = lock(&self.0.now_playing);
        list.retain(|e| e.at.elapsed() < NOW_PLAYING_TTL);
        list.iter().rev().cloned().collect()
    }

    /// Forgets cached accounts (password, role or user changes).
    pub fn invalidate_accounts(&self) {
        lock(&self.0.accounts).clear();
    }

    /// Forgets cached settings (a Subsonic setting changed).
    pub fn invalidate_settings(&self) {
        *lock(&self.0.settings) = None;
    }

    /// Current settings (cached for a few seconds).
    pub fn settings(&self, db: &DbPool) -> SubsonicSettings {
        if let Some((s, at)) = lock(&self.0.settings).as_ref() {
            if at.elapsed() < SETTINGS_TTL {
                return s.clone();
            }
        }
        let fresh = {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            read_settings(&conn)
        };
        *lock(&self.0.settings) = Some((fresh.clone(), Instant::now()));
        fresh
    }

    pub(crate) fn cached_account(&self, username_key: &str) -> Option<CachedAccount> {
        let accounts = lock(&self.0.accounts);
        accounts
            .get(username_key)
            .filter(|a| a.fetched.elapsed() < ACCOUNT_TTL)
            .cloned()
    }

    pub(crate) fn store_account(&self, username_key: String, account: CachedAccount) {
        let mut accounts = lock(&self.0.accounts);
        if accounts.len() >= MAX_CACHED_ACCOUNTS {
            accounts.clear();
        }
        accounts.insert(username_key, account);
    }

    /// The encryption key for the active data folder, creating the key file on first use. When a new key
    /// replaces a missing/corrupt one, stored passwords are cleared (they can no longer be decrypted).
    /// Blocking: call from `spawn_blocking`.
    pub fn key(&self, db_folder: Option<&Path>, db: &DbPool) -> Option<[u8; 32]> {
        let dir = super::crypto::key_dir(db_folder?)?;
        if let Some((cached_dir, key)) = lock(&self.0.key).as_ref() {
            if *cached_dir == dir {
                return Some(*key);
            }
        }
        let (key, created) = match super::crypto::load_or_create_key(&dir) {
            Ok(k) => k,
            Err(err) => {
                tracing::error!(error = %err, "Subsonic key file could not be read or created");
                return None;
            }
        };
        if created {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            match boogiebox_db::subsonic::clear_all_subsonic_secrets(&conn) {
                Ok(n) if n > 0 => tracing::warn!(
                    cleared = n,
                    "New Subsonic key file created; existing Subsonic passwords must be set again"
                ),
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "Could not clear stale Subsonic passwords")
                }
            }
            self.invalidate_accounts();
        }
        *lock(&self.0.key) = Some((dir, key));
        Some(key)
    }

    /// True at most once per interval per user, to throttle "last used" writes.
    pub fn should_record_use(&self, user_id: &str) -> bool {
        let mut last = lock(&self.0.last_use);
        match last.get(user_id) {
            Some(at) if at.elapsed() < USE_RECORD_INTERVAL => false,
            _ => {
                last.insert(user_id.to_owned(), Instant::now());
                true
            }
        }
    }

    /// Semaphore bounding concurrent Subsonic transcodes (rebuilt when the limit changes).
    pub fn transcode_limiter(&self, max: usize) -> Arc<Semaphore> {
        let max = max.clamp(1, 8);
        let mut slot = lock(&self.0.transcodes);
        match slot.as_ref() {
            Some((n, sem)) if *n == max => sem.clone(),
            _ => {
                let sem = Arc::new(Semaphore::new(max));
                *slot = Some((max, sem.clone()));
                sem
            }
        }
    }
}

fn read_settings(conn: &rusqlite::Connection) -> SubsonicSettings {
    let get = |key: &str| boogiebox_db::artwork::get_setting(conn, key);
    SubsonicSettings {
        enabled: get("subsonicApiEnabled").as_deref() == Some("true"),
        max_transcodes: get("subsonicMaxTranscodes")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .map(|n| n.clamp(1, 8))
            .unwrap_or(DEFAULT_MAX_TRANSCODES),
        ignored_articles: get("subsonicIgnoredArticles")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_IGNORED_ARTICLES.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcode_limiter_is_rebuilt_only_when_the_limit_changes() {
        let state = SubsonicState::default();
        let a = state.transcode_limiter(3);
        let b = state.transcode_limiter(3);
        assert!(Arc::ptr_eq(&a, &b));
        let c = state.transcode_limiter(5);
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(state.transcode_limiter(99).available_permits(), 8);
    }

    #[test]
    fn now_playing_keeps_one_entry_per_user_and_client() {
        let state = SubsonicState::default();
        let entry = |user: &str, client: &str, track: &str| NowPlayingEntry {
            user_id: user.into(),
            username: user.into(),
            client: client.into(),
            track_id: track.into(),
            at: Instant::now(),
        };
        state.set_now_playing(entry("u1", "phone", "t1"));
        state.set_now_playing(entry("u1", "phone", "t2"));
        state.set_now_playing(entry("u1", "desk", "t3"));
        state.set_now_playing(entry("u2", "phone", "t4"));
        let tracks: Vec<String> = state
            .now_playing()
            .into_iter()
            .map(|e| e.track_id)
            .collect();
        assert_eq!(tracks, ["t4", "t3", "t2"]);
        for i in 0..(MAX_NOW_PLAYING + 5) {
            state.set_now_playing(entry(&format!("x{i}"), "c", "t"));
        }
        assert_eq!(state.now_playing().len(), MAX_NOW_PLAYING);
        state.reset();
        assert!(state.now_playing().is_empty());
    }

    #[test]
    fn use_recording_is_throttled_per_user() {
        let state = SubsonicState::default();
        assert!(state.should_record_use("u1"));
        assert!(!state.should_record_use("u1"));
        assert!(state.should_record_use("u2"));
        state.reset();
        assert!(state.should_record_use("u1"));
    }

    #[test]
    fn account_cache_expires_and_resets() {
        let state = SubsonicState::default();
        state.store_account(
            "alice".into(),
            CachedAccount {
                user: None,
                secret: None,
                fetched: Instant::now(),
            },
        );
        assert!(state.cached_account("alice").is_some());
        state.invalidate_accounts();
        assert!(state.cached_account("alice").is_none());
        if let Some(stale) = Instant::now().checked_sub(ACCOUNT_TTL + Duration::from_secs(1)) {
            state.store_account(
                "bob".into(),
                CachedAccount {
                    user: None,
                    secret: None,
                    fetched: stale,
                },
            );
            assert!(state.cached_account("bob").is_none());
        }
    }
}

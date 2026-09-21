//! Background and on-demand metadata enrichment for Artist Radio
//! (`wip/artist-radio-v2-plan.md` §3).
//!
//! Deliberately its own scheduler rather than a `post_scan` lane: the post-scan
//! lane runs exactly one job at a time, and a whole-library track-tag backfill
//! (hours) would starve artwork caching and every other lane. Like the BPM and
//! waveform sweeps it works in small bounded batches, stands down while a
//! BoogieMix build holds the priority gate, and keeps all of its progress in
//! the database (`track_tag_sync`, `artists.musicbrainz_*`, `lastfm_cache`) so a
//! restart resumes instead of starting over.

use crate::{
    mix_priority_gate,
    providers::{
        fetch_lastfm_track_top_tags, fetch_musicbrainz_artist_tags, is_valid_mbid,
        search_musicbrainz_artist, ProviderFetchError,
    },
    tag_taxonomy::to_tag_inputs,
    DbPool,
};
use boogiebox_db::{
    artwork::{get_setting, save_lastfm_cache},
    music::EntityId,
    radio::{
        insert_artist_tags_if_none, list_artists_needing_musicbrainz_id,
        list_artists_needing_musicbrainz_tags, list_tracks_needing_tags, list_untagged_among,
        replace_track_tags, set_musicbrainz_identity, track_tag_progress, TagCandidate,
        SOURCE_LASTFM,
    },
};
use reqwest::Client;
use serde::Serialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const TRACK_BATCH: i64 = 25;
const ARTIST_BATCH: i64 = 20;
/// Last.fm tag counts are 0–100 relative to the top tag; below this is noise.
const MIN_TRACK_TAG_COUNT: u64 = 10;
const MAX_TRACK_TAGS: usize = 12;
const MB_TAG_CACHE_DAYS: i64 = 30;
const MB_ID_RECHECK_DAYS: i64 = 30;
/// Most tracks a single radio launch will enrich on demand.
pub const LAZY_MAX_TRACKS: usize = 60;
const ERRORS_BEFORE_BACKOFF: usize = 3;

const IDLE_WAIT: Duration = Duration::from_secs(5 * 60);
const BACKOFF_WAIT: Duration = Duration::from_secs(10 * 60);
const GATED_WAIT: Duration = Duration::from_secs(60);

static LAZY_RUNNING: AtomicBool = AtomicBool::new(false);

fn delay_from_env_or(var: &str, default_ms: u64) -> Duration {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(default_ms))
}

/// Between-request delay for Last.fm track-tag lookups.
fn track_tag_delay() -> Duration {
    delay_from_env_or("BOOGIEBOX_TRACK_TAG_SYNC_DELAY_MS", 1000)
}

/// MusicBrainz allows ~1 request/second; stay just under it.
fn musicbrainz_delay() -> Duration {
    delay_from_env_or("BOOGIEBOX_MUSICBRAINZ_DELAY_MS", 1100)
}

/// `radioTrackTagSync` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Off,
    /// Only tracks in a launched radio's candidate pool.
    Lazy,
    /// Lazy plus a whole-library background backfill (the default).
    Full,
}

impl SyncMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("off") => SyncMode::Off,
            Some("lazy") => SyncMode::Lazy,
            _ => SyncMode::Full,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataSettings {
    pub mode: SyncMode,
    pub lastfm_key: Option<String>,
    /// `radioKeylessProviders`: MusicBrainz/ListenBrainz (default on).
    pub keyless: bool,
}

pub fn read_settings(conn: &rusqlite::Connection) -> MetadataSettings {
    MetadataSettings {
        mode: SyncMode::parse(get_setting(conn, "radioTrackTagSync").as_deref()),
        lastfm_key: get_setting(conn, "lastfmKey").filter(|k| !k.trim().is_empty()),
        keyless: get_setting(conn, "radioKeylessProviders").is_none_or(|v| v != "false"),
    }
}

/// What one scheduler pass achieved, which decides how long to sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Did work; there may be more.
    Progress,
    /// Nothing to do right now.
    Idle,
    /// A provider throttled or failed; wait longer before trying again.
    Backoff,
    /// A BoogieMix build has priority.
    Gated,
}

impl Outcome {
    fn wait(self) -> Duration {
        match self {
            Outcome::Progress => Duration::from_millis(500),
            Outcome::Idle => IDLE_WAIT,
            Outcome::Backoff => BACKOFF_WAIT,
            Outcome::Gated => GATED_WAIT,
        }
    }
}

/// Starts the background enrichment loop.
pub fn start_radio_metadata_scheduler(db: DbPool, http_client: Client, cancel: CancellationToken) {
    tokio::spawn(async move {
        loop {
            let outcome = run_one_pass(&db, &http_client, &cancel).await;
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(outcome.wait()) => {}
            }
        }
    });
}

async fn blocking<T, F>(db: &DbPool, work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> T + Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        work(&conn)
    })
    .await
    .ok()
}

/// One bounded pass: a batch of MusicBrainz artist work, then a batch of
/// Last.fm track tags (when the mode is `full`).
pub async fn run_one_pass(db: &DbPool, http: &Client, cancel: &CancellationToken) -> Outcome {
    if cancel.is_cancelled() {
        return Outcome::Idle;
    }
    if mix_priority_gate::is_active() {
        return Outcome::Gated;
    }
    let Some(settings) = blocking(db, read_settings).await else {
        return Outcome::Idle;
    };
    let mut progressed = false;

    if settings.keyless {
        match run_musicbrainz_pass(db, http, cancel).await {
            Ok(did) => progressed |= did,
            Err(error) => {
                tracing::debug!("radio metadata: MusicBrainz pass stopped: {error}");
                return Outcome::Backoff;
            }
        }
    }

    if settings.mode == SyncMode::Full {
        if let Some(key) = settings.lastfm_key.as_deref() {
            let candidates = blocking(db, |conn| {
                list_tracks_needing_tags(conn, SOURCE_LASTFM, TRACK_BATCH)
            })
            .await
            .and_then(Result::ok)
            .unwrap_or_default();
            if !candidates.is_empty() {
                let (done, stop) =
                    process_track_candidates(db, http, key, &candidates, true, cancel).await;
                progressed |= done > 0;
                if let Some(outcome) = stop {
                    return outcome;
                }
            }
        }
    }

    if progressed {
        Outcome::Progress
    } else {
        Outcome::Idle
    }
}

/// Resolves MusicBrainz ids for artists lacking one, then fills artists that
/// have no tags at all from MusicBrainz's genres/tags. Returns whether any
/// artist was processed.
async fn run_musicbrainz_pass(
    db: &DbPool,
    http: &Client,
    cancel: &CancellationToken,
) -> Result<bool, ProviderFetchError> {
    let mut did_work = false;

    let needing_id = blocking(db, |conn| {
        list_artists_needing_musicbrainz_id(conn, MB_ID_RECHECK_DAYS, ARTIST_BATCH)
    })
    .await
    .and_then(Result::ok)
    .unwrap_or_default();
    for (artist_id, name, lastfm_mbid) in needing_id {
        if cancel.is_cancelled() || mix_priority_gate::is_active() {
            return Ok(did_work);
        }
        // Last.fm's mbid is already a MusicBrainz id — no network needed.
        let resolved = match lastfm_mbid.filter(|m| is_valid_mbid(m)) {
            Some(mbid) => Some(mbid),
            None => {
                let found = search_musicbrainz_artist(http, &name).await?;
                tokio::time::sleep(musicbrainz_delay()).await;
                found
            }
        };
        blocking(db, move |conn| {
            set_musicbrainz_identity(conn, &artist_id, resolved.as_deref())
        })
        .await;
        did_work = true;
    }

    let needing_tags = blocking(db, |conn| {
        list_artists_needing_musicbrainz_tags(conn, ARTIST_BATCH)
    })
    .await
    .and_then(Result::ok)
    .unwrap_or_default();
    for (artist_id, mbid) in needing_tags {
        if cancel.is_cancelled() || mix_priority_gate::is_active() {
            return Ok(did_work);
        }
        let raw = fetch_musicbrainz_artist_tags(http, &mbid).await?;
        tokio::time::sleep(musicbrainz_delay()).await;
        // MusicBrainz votes are small integers: keep tags with at least a
        // fifth of the leading tag's votes.
        let tags = to_tag_inputs(&raw, 1, 0.2, 12, &[]);
        let cache_key = format!("mb-artist-tags:{artist_id}");
        let cached = serde_json::to_string(&raw).unwrap_or_else(|_| "[]".to_owned());
        blocking(db, move |conn| {
            let _ = insert_artist_tags_if_none(conn, &artist_id, &tags);
            // Cached even when empty: the cache entry is the "checked" marker.
            save_lastfm_cache(conn, &cache_key, &cached, MB_TAG_CACHE_DAYS);
        })
        .await;
        did_work = true;
    }
    Ok(did_work)
}

/// Fetches and stores Last.fm tags for `candidates`. Returns how many tracks
/// were stamped and, when the batch had to stop early, why.
async fn process_track_candidates(
    db: &DbPool,
    http: &Client,
    api_key: &str,
    candidates: &[TagCandidate],
    honor_gate: bool,
    cancel: &CancellationToken,
) -> (usize, Option<Outcome>) {
    let mut done = 0;
    let mut errors = 0;
    for candidate in candidates {
        if cancel.is_cancelled() {
            return (done, Some(Outcome::Idle));
        }
        if honor_gate && mix_priority_gate::is_active() {
            return (done, Some(Outcome::Gated));
        }
        match fetch_lastfm_track_top_tags(http, api_key, &candidate.artist, &candidate.title).await
        {
            Ok(raw) => {
                let tags = to_tag_inputs(
                    &raw,
                    MIN_TRACK_TAG_COUNT,
                    0.0,
                    MAX_TRACK_TAGS,
                    &[candidate.artist.as_str(), candidate.title.as_str()],
                );
                let track_id = candidate.track_id.clone();
                blocking(db, move |conn| {
                    replace_track_tags(conn, &track_id, SOURCE_LASTFM, &tags)
                })
                .await;
                done += 1;
            }
            // A throttle must never be stamped as "no tags" — leave the track
            // unmarked so the next pass retries it.
            Err(ProviderFetchError::RateLimited) => return (done, Some(Outcome::Backoff)),
            Err(ProviderFetchError::Failed(message)) => {
                tracing::debug!("radio metadata: Last.fm track tags failed: {message}");
                errors += 1;
                if errors >= ERRORS_BEFORE_BACKOFF {
                    return (done, Some(Outcome::Backoff));
                }
            }
        }
        tokio::time::sleep(track_tag_delay()).await;
    }
    (done, None)
}

struct LazyGuard;

impl Drop for LazyGuard {
    fn drop(&mut self) {
        LAZY_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Radio-launch path: enriches up to [`LAZY_MAX_TRACKS`] of the candidate
/// tracks in the background (never blocks the request). No-op when the mode is
/// `off`, no Last.fm key is configured, or a lazy fetch is already running.
pub fn spawn_lazy_tag_fetch(db: DbPool, http_client: Client, track_ids: Vec<EntityId>) {
    if track_ids.is_empty()
        || LAZY_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        let _guard = LazyGuard;
        run_lazy_tag_fetch(&db, &http_client, &track_ids).await;
    });
}

async fn run_lazy_tag_fetch(db: &DbPool, http: &Client, track_ids: &[EntityId]) -> usize {
    let Some(settings) = blocking(db, read_settings).await else {
        return 0;
    };
    let Some(key) = settings
        .lastfm_key
        .as_deref()
        .filter(|_| settings.mode != SyncMode::Off)
    else {
        return 0;
    };
    let ids = track_ids.to_vec();
    let candidates = blocking(db, move |conn| {
        list_untagged_among(conn, SOURCE_LASTFM, &ids, LAZY_MAX_TRACKS)
    })
    .await
    .and_then(Result::ok)
    .unwrap_or_default();
    let cancel = CancellationToken::new();
    process_track_candidates(db, http, key, &candidates, false, &cancel)
        .await
        .0
}

/// Progress/coverage shown in Settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadioMetadataStatus {
    pub mode: SyncMode,
    pub lastfm_configured: bool,
    pub keyless_enabled: bool,
    pub tracks_checked: i64,
    pub tracks_total: i64,
    pub artists_tagged: i64,
    pub artists_total: i64,
}

pub fn radio_metadata_status(conn: &rusqlite::Connection) -> rusqlite::Result<RadioMetadataStatus> {
    let settings = read_settings(conn);
    let (tracks_checked, tracks_total) = track_tag_progress(conn, SOURCE_LASTFM)?;
    let artists_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artists ar WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = ar.id)",
        [],
        |row| row.get(0),
    )?;
    let artists_tagged: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.artist_id) FROM artist_styles s
         JOIN artists ar ON ar.id = s.artist_id",
        [],
        |row| row.get(0),
    )?;
    Ok(RadioMetadataStatus {
        mode: settings.mode,
        lastfm_configured: settings.lastfm_key.is_some(),
        keyless_enabled: settings.keyless,
        tracks_checked,
        tracks_total,
        artists_tagged,
        artists_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::provider_fetch_tests::ENV_LOCK;
    use boogiebox_db::init_db;
    use rusqlite::params;
    use std::{
        sync::{Arc, Mutex},
        time::SystemTime,
    };
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    const MBID: &str = "10adbe5e-a2c0-4bf3-8249-2b4cbf6e6ca8";

    fn temp_db() -> DbPool {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("radio-metadata-test-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Mutex::new(init_db(&dir).unwrap().connection))
    }

    fn set_setting(db: &DbPool, key: &str, value: &str) {
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value, updated_at) VALUES(?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
            .unwrap();
    }

    /// Seeds one artist with one track; returns `(artist_id, track_id)`.
    fn seed(db: &DbPool, artist: &str, title: &str) -> (String, String) {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES ('lib', '/music', 'Lib')
             ON CONFLICT(path) DO NOTHING",
            [],
        )
        .unwrap();
        let artist_id = format!("artist-{artist}");
        conn.execute(
            "INSERT OR IGNORE INTO artists(id, name) VALUES(?1, ?2)",
            params![artist_id, artist],
        )
        .unwrap();
        let track_id = format!("track-{artist}-{title}");
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, title, file_path)
             VALUES(?1, 'lib', ?2, ?3, ?4)",
            params![track_id, artist_id, title, format!("/music/{track_id}.mp3")],
        )
        .unwrap();
        (artist_id, track_id)
    }

    fn fast_delays() {
        std::env::set_var("BOOGIEBOX_TRACK_TAG_SYNC_DELAY_MS", "0");
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_DELAY_MS", "0");
    }

    fn quiet_musicbrainz(db: &DbPool) {
        set_setting(db, "radioKeylessProviders", "false");
    }

    #[test]
    fn sync_mode_parses_with_full_as_the_default() {
        assert_eq!(SyncMode::parse(Some("off")), SyncMode::Off);
        assert_eq!(SyncMode::parse(Some(" lazy ")), SyncMode::Lazy);
        assert_eq!(SyncMode::parse(Some("full")), SyncMode::Full);
        assert_eq!(SyncMode::parse(Some("bogus")), SyncMode::Full);
        assert_eq!(SyncMode::parse(None), SyncMode::Full);
        assert!(Outcome::Backoff.wait() > Outcome::Idle.wait());
        assert!(Outcome::Progress.wait() < Outcome::Gated.wait());
    }

    #[test]
    fn settings_default_to_full_with_keyless_providers_on() {
        let db = temp_db();
        let conn = db.lock().unwrap();
        let s = read_settings(&conn);
        assert_eq!(s.mode, SyncMode::Full);
        assert!(s.keyless);
        assert_eq!(s.lastfm_key, None);
    }

    #[tokio::test]
    async fn full_mode_backfills_tracks_classifies_tags_and_reports_progress() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        Mock::given(method("GET"))
            .and(query_param("method", "track.gettoptags"))
            .and(query_param("track", "Teardrop"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptags": { "tag": [
                    { "name": "Trip Hop", "count": 100 },
                    { "name": "melancholy", "count": 60 },
                    { "name": "Massive Attack", "count": 50 },
                    { "name": "seen live", "count": 40 },
                    { "name": "faint", "count": 3 }
                ]}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("track", "Nothing"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "error": 6, "message": "Track not found" })),
            )
            .mount(&server)
            .await;

        let db = temp_db();
        quiet_musicbrainz(&db);
        set_setting(&db, "lastfmKey", "key");
        let (_, tear) = seed(&db, "Massive Attack", "Teardrop");
        let (_, none) = seed(&db, "Massive Attack", "Nothing");

        let outcome = run_one_pass(&db, &Client::new(), &CancellationToken::new()).await;
        assert_eq!(outcome, Outcome::Progress);

        {
            let conn = db.lock().unwrap();
            let tags: Vec<(String, String, Option<String>)> = conn
            .prepare(
                "SELECT tag, kind, bucket FROM track_tags WHERE track_id=?1 ORDER BY weight DESC",
            )
            .unwrap()
            .query_map([&tear], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
            assert_eq!(
                tags,
                vec![
                    ("trip-hop".to_owned(), "genre".to_owned(), None),
                    (
                        "melancholy".to_owned(),
                        "mood".to_owned(),
                        Some("melancholic".to_owned())
                    ),
                ]
            );
            let status: String = conn
                .query_row(
                    "SELECT status FROM track_tag_sync WHERE track_id=?1",
                    [&none],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(status, "empty");
            let progress = radio_metadata_status(&conn).unwrap();
            assert_eq!((progress.tracks_checked, progress.tracks_total), (2, 2));
            assert!(progress.lastfm_configured);
            assert_eq!(progress.mode, SyncMode::Full);
        }

        // Everything is stamped: the next pass has nothing left to do.
        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Idle
        );
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn a_rate_limit_backs_off_without_stamping_the_track() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let db = temp_db();
        quiet_musicbrainz(&db);
        set_setting(&db, "lastfmKey", "key");
        seed(&db, "Artist", "Song");

        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Backoff
        );
        let n: i64 = db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM track_tag_sync", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "a throttled lookup must stay retryable");
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn repeated_failures_back_off_after_a_few_errors() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let db = temp_db();
        quiet_musicbrainz(&db);
        set_setting(&db, "lastfmKey", "key");
        for title in ["a", "b", "c", "d"] {
            seed(&db, "Artist", title);
        }
        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Backoff
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            ERRORS_BEFORE_BACKOFF
        );
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn lazy_and_off_modes_skip_the_background_backfill() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "toptags": { "tag": [{ "name": "ambient", "count": 90 }] } }),
            ))
            .mount(&server)
            .await;
        let db = temp_db();
        quiet_musicbrainz(&db);
        set_setting(&db, "lastfmKey", "key");
        let (_, track) = seed(&db, "Artist", "Song");

        for mode in ["lazy", "off"] {
            set_setting(&db, "radioTrackTagSync", mode);
            assert_eq!(
                run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
                Outcome::Idle,
                "{mode}"
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());

        // The lazy path itself fetches for the launched radio's tracks in `lazy`...
        set_setting(&db, "radioTrackTagSync", "lazy");
        let id = EntityId::Str(track.clone());
        assert_eq!(
            run_lazy_tag_fetch(&db, &Client::new(), std::slice::from_ref(&id)).await,
            1
        );
        // ...but not when sync is off, and never re-fetches a stamped track.
        set_setting(&db, "radioTrackTagSync", "off");
        assert_eq!(
            run_lazy_tag_fetch(&db, &Client::new(), std::slice::from_ref(&id)).await,
            0
        );
        set_setting(&db, "radioTrackTagSync", "lazy");
        assert_eq!(run_lazy_tag_fetch(&db, &Client::new(), &[id]).await, 0);
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn spawn_lazy_tag_fetch_is_single_flight_and_ignores_empty_input() {
        let db = temp_db();
        spawn_lazy_tag_fetch(db.clone(), Client::new(), Vec::new());
        assert!(!LAZY_RUNNING.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_mix_build_gates_the_pass() {
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        let db = temp_db();
        let guard = crate::mix_priority_gate::PriorityAnalysisGuard::acquire();
        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Gated
        );
        drop(guard);
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            run_one_pass(&db, &Client::new(), &cancelled).await,
            Outcome::Idle
        );
    }

    #[tokio::test]
    async fn musicbrainz_pass_resolves_ids_fills_untagged_artists_and_caches_the_check() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        Mock::given(method("GET"))
            .and(path("/artist/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [{ "id": MBID, "name": "Massive Attack", "score": 100 }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/artist/{MBID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tags": [
                    { "name": "downtempo", "count": 12 },
                    { "name": "chillout", "count": 6 },
                    { "name": "british", "count": 1 }
                ]
            })))
            .mount(&server)
            .await;

        let db = temp_db();
        set_setting(&db, "radioTrackTagSync", "off"); // MB runs independently of the Last.fm mode
        let (artist_id, _) = seed(&db, "Massive Attack", "Teardrop");
        // Already has a Last.fm mbid → resolved without a search call.
        let (with_lfm, _) = seed(&db, "Portishead", "Roads");
        db.lock()
            .unwrap()
            .execute(
                "UPDATE artists SET lastfm_mbid='8f6bd1e4-fbe1-4f50-aa9b-94c450ec0f11' WHERE id=?1",
                [&with_lfm],
            )
            .unwrap();

        let cancel = CancellationToken::new();
        // Pass 1 resolves ids for both artists and fetches tags for the artist
        // that had no tags; the Portishead tag fetch has no mock, so it 404s → empty.
        assert_eq!(
            run_one_pass(&db, &Client::new(), &cancel).await,
            Outcome::Progress
        );

        {
            let conn = db.lock().unwrap();
            let mbid: Option<String> = conn
                .query_row(
                    "SELECT musicbrainz_artist_id FROM artists WHERE id=?1",
                    [&artist_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(mbid.as_deref(), Some(MBID));
            let mbid: Option<String> = conn
                .query_row(
                    "SELECT musicbrainz_artist_id FROM artists WHERE id=?1",
                    [&with_lfm],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                mbid.as_deref(),
                Some("8f6bd1e4-fbe1-4f50-aa9b-94c450ec0f11")
            );
            let styles: Vec<(String, Option<String>)> = conn
            .prepare(
                "SELECT style, bucket FROM artist_styles WHERE artist_id=?1 ORDER BY weight DESC",
            )
            .unwrap()
            .query_map([&artist_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
            assert_eq!(
                styles,
                vec![
                    ("downtempo".to_owned(), None),
                    ("chillout".to_owned(), Some("chill".to_owned())),
                ]
            );
            let cached: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM lastfm_cache WHERE cache_key LIKE 'mb-artist-tags:%'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(cached, 2, "even the empty check is cached");
        }

        // Nothing left: idle.
        assert_eq!(
            run_one_pass(&db, &Client::new(), &cancel).await,
            Outcome::Idle
        );
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn musicbrainz_throttle_backs_off_and_disabling_keyless_makes_no_calls() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let db = temp_db();
        seed(&db, "Artist", "Song");

        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Backoff
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        set_setting(&db, "radioKeylessProviders", "false");
        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Idle
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "no new calls"
        );
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }
}

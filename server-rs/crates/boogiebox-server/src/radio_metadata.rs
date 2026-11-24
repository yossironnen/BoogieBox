//! Background and on-demand metadata enrichment for Artist Radio
//! (`wip/artist-radio-v2-plan.md` §3).
//!
//! Deliberately its own scheduler rather than a `post_scan` lane: the post-scan
//! lane runs exactly one job at a time, and a whole-library track-tag backfill
//! (hours) would starve artwork caching and every other lane. Like the BPM and
//! waveform sweeps it works in small bounded batches, stands down while a
//! BoogieMix build holds the priority gate, and keeps all of its progress in
//! the database (`track_tag_sync`, `artists.musicbrainz_*`, the provider cache) so a
//! restart resumes instead of starting over.

use crate::{
    mix_priority_gate,
    providers::{
        fetch_listenbrainz_top_recordings, fetch_musicbrainz_artist_tags,
        fetch_musicbrainz_recording_tags, is_valid_mbid, search_musicbrainz_artist,
        search_musicbrainz_recording, ProviderFetchError,
    },
    tag_taxonomy::to_tag_inputs,
    DbPool,
};
use boogiebox_db::{
    artwork::{get_setting, save_lastfm_cache},
    music::EntityId,
    popularity::{
        list_artists_needing_listenbrainz_popularity, store_listenbrainz_popularity,
        PopularityEntry,
    },
    radio::{
        insert_artist_tags_if_none, list_artists_needing_musicbrainz_id,
        list_artists_needing_musicbrainz_tags, list_tracks_needing_tags, list_untagged_among,
        replace_track_tags, set_musicbrainz_identity, track_tag_progress, TagCandidate,
        SOURCE_MUSICBRAINZ,
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
/// MusicBrainz recording tags carry a vote or two each (sampled on the dev library), so every tag counts.
const MIN_MUSICBRAINZ_TRACK_TAG_COUNT: u64 = 1;
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

/// ListenBrainz asks for at most one request per second per application.
fn listenbrainz_delay() -> Duration {
    delay_from_env_or("BOOGIEBOX_LISTENBRAINZ_DELAY_MS", 1100)
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
    /// The main metadata switch (see [`keyless_enabled`]).
    pub keyless: bool,
}

/// The main metadata switch (`radioKeylessProviders`, default on). Off: BoogieBox asks none of MusicBrainz,
/// ListenBrainz, Wikipedia/Wikidata or TheAudioDB; answers already saved are still shown.
pub fn keyless_enabled(conn: &rusqlite::Connection) -> bool {
    get_setting(conn, "radioKeylessProviders").is_none_or(|v| v != "false")
}

pub fn read_settings(conn: &rusqlite::Connection) -> MetadataSettings {
    MetadataSettings {
        mode: SyncMode::parse(get_setting(conn, "radioTrackTagSync").as_deref()),
        keyless: keyless_enabled(conn),
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

/// One bounded pass: a batch of MusicBrainz artist work, then (when the mode is
/// `full`) ListenBrainz popularity and MusicBrainz track tags.
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

    // Popularity for the "Popular" pill from ListenBrainz (no key), for artists with a MusicBrainz id.
    if settings.keyless && settings.mode == SyncMode::Full {
        let artists = blocking(db, |conn| {
            list_artists_needing_listenbrainz_popularity(conn, ARTIST_BATCH)
        })
        .await
        .and_then(Result::ok)
        .unwrap_or_default();
        if !artists.is_empty() {
            let (done, stop) = process_listenbrainz_popularity(db, http, &artists, cancel).await;
            progressed |= done > 0;
            if let Some(outcome) = stop {
                return outcome;
            }
        }
    }

    // Track tags from MusicBrainz (no key): find each track's recording, then its tags.
    if settings.keyless && settings.mode == SyncMode::Full {
        let candidates = blocking(db, |conn| {
            list_tracks_needing_tags(conn, SOURCE_MUSICBRAINZ, TRACK_BATCH)
        })
        .await
        .and_then(Result::ok)
        .unwrap_or_default();
        if !candidates.is_empty() {
            let (done, stop) = process_musicbrainz_track_tags(db, http, &candidates, cancel).await;
            progressed |= done > 0;
            if let Some(outcome) = stop {
                return outcome;
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
            None => search_musicbrainz_artist(http, &name).await?,
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

/// MusicBrainz tags for a batch of tracks: the recording from the track's tags, else a search by artist and
/// title (the id found is kept on the track), then the recording's tags. Two requests per track at most.
async fn process_musicbrainz_track_tags(
    db: &DbPool,
    http: &Client,
    candidates: &[TagCandidate],
    cancel: &CancellationToken,
) -> (usize, Option<Outcome>) {
    let mut done = 0;
    let mut errors = 0;
    for candidate in candidates {
        if cancel.is_cancelled() {
            return (done, Some(Outcome::Idle));
        }
        if mix_priority_gate::is_active() {
            return (done, Some(Outcome::Gated));
        }
        let track_id = candidate.track_id.clone();
        let known = blocking(db, move |conn| {
            conn.query_row(
                "SELECT musicbrainz_recording_id FROM tracks WHERE id = ?1",
                [&track_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
        })
        .await
        .flatten()
        .filter(|id| is_valid_mbid(id));
        let recording = match known {
            Some(id) => Ok(Some(id)),
            None => {
                let found =
                    search_musicbrainz_recording(http, &candidate.artist, &candidate.title).await;
                if let Ok(Some(id)) = &found {
                    let (track_id, id) = (candidate.track_id.clone(), id.clone());
                    blocking(db, move |conn| {
                        conn.execute(
                            "UPDATE tracks SET musicbrainz_recording_id = ?1
                             WHERE id = ?2 AND musicbrainz_recording_id IS NULL",
                            rusqlite::params![id, track_id],
                        )
                    })
                    .await;
                }
                found
            }
        };
        let tags = match recording {
            Ok(Some(id)) => fetch_musicbrainz_recording_tags(http, &id).await,
            // No recording to be found: stamped empty, asked again after the negative-cache TTL.
            Ok(None) => Ok(Vec::new()),
            Err(error) => Err(error),
        };
        match tags {
            Ok(raw) => {
                let tags = to_tag_inputs(
                    &raw,
                    MIN_MUSICBRAINZ_TRACK_TAG_COUNT,
                    0.0,
                    MAX_TRACK_TAGS,
                    &[candidate.artist.as_str(), candidate.title.as_str()],
                );
                let track_id = candidate.track_id.clone();
                blocking(db, move |conn| {
                    replace_track_tags(conn, &track_id, SOURCE_MUSICBRAINZ, &tags)
                })
                .await;
                done += 1;
            }
            // A throttle is never stamped as "no tags": the next pass retries the track.
            Err(ProviderFetchError::RateLimited) => return (done, Some(Outcome::Backoff)),
            Err(error) => {
                tracing::debug!("radio metadata: MusicBrainz track tags failed: {error}");
                errors += 1;
                if errors >= ERRORS_BEFORE_BACKOFF {
                    return (done, Some(Outcome::Backoff));
                }
            }
        }
    }
    (done, None)
}

/// ListenBrainz listen counts for a batch of artists (`(artist id, MusicBrainz id)`), one request per artist.
async fn process_listenbrainz_popularity(
    db: &DbPool,
    http: &Client,
    artists: &[(EntityId, String)],
    cancel: &CancellationToken,
) -> (usize, Option<Outcome>) {
    let mut done = 0;
    let mut errors = 0;
    for (artist_id, mbid) in artists {
        if cancel.is_cancelled() {
            return (done, Some(Outcome::Idle));
        }
        if mix_priority_gate::is_active() {
            return (done, Some(Outcome::Gated));
        }
        match fetch_listenbrainz_top_recordings(http, mbid).await {
            Ok(songs) => {
                let entries: Vec<PopularityEntry> = songs
                    .into_iter()
                    .filter_map(|song| {
                        Some(PopularityEntry {
                            title: song.name,
                            scrobbles: song.listens?,
                            listeners: None,
                            recording_mbid: song.recording_mbid,
                        })
                    })
                    .collect();
                let artist_id = artist_id.clone();
                blocking(db, move |conn| {
                    store_listenbrainz_popularity(conn, &artist_id, &entries)
                })
                .await;
                done += 1;
            }
            // Never recorded as "no data": the next pass retries the artist.
            Err(ProviderFetchError::RateLimited) => return (done, Some(Outcome::Backoff)),
            Err(error) => {
                tracing::debug!("radio metadata: ListenBrainz popularity failed: {error}");
                errors += 1;
                if errors >= ERRORS_BEFORE_BACKOFF {
                    return (done, Some(Outcome::Backoff));
                }
            }
        }
        tokio::time::sleep(listenbrainz_delay()).await;
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
/// tracks with MusicBrainz track tags in the background (never blocks the
/// request). No-op when the mode is `off`, MusicBrainz is turned off, or a lazy
/// fetch is already running.
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
    if settings.mode == SyncMode::Off || !settings.keyless {
        return 0;
    }
    let ids = track_ids.to_vec();
    let candidates = blocking(db, move |conn| {
        list_untagged_among(conn, SOURCE_MUSICBRAINZ, &ids, LAZY_MAX_TRACKS)
    })
    .await
    .and_then(Result::ok)
    .unwrap_or_default();
    let cancel = CancellationToken::new();
    process_musicbrainz_track_tags(db, http, &candidates, &cancel)
        .await
        .0
}

/// Progress/coverage shown in Settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RadioMetadataStatus {
    pub mode: SyncMode,
    pub keyless_enabled: bool,
    pub tracks_checked: i64,
    pub tracks_total: i64,
    pub artists_tagged: i64,
    pub artists_total: i64,
}

pub fn radio_metadata_status(conn: &rusqlite::Connection) -> rusqlite::Result<RadioMetadataStatus> {
    let settings = read_settings(conn);
    let (tracks_checked, tracks_total) = track_tag_progress(conn, SOURCE_MUSICBRAINZ)?;
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
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_DELAY_MS", "0");
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_DELAY_MS", "0");
        // Nothing here may reach the real ListenBrainz or MusicBrainz; tests that want them point these at
        // their mock after this.
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_API_BASE", "http://127.0.0.1:9");
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", "http://127.0.0.1:9");
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
    }

    fn scrobbles(db: &DbPool, track_id: &str) -> Option<i64> {
        db.lock()
            .unwrap()
            .query_row(
                "SELECT scrobbles FROM track_popularity WHERE track_id = ?1",
                [track_id],
                |r| r.get(0),
            )
            .ok()
    }

    #[tokio::test]
    async fn lazy_and_off_modes_skip_the_background_backfill() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        let rec = "e8f9b188-f819-4e43-ab0f-4bd26ce9ff56";
        Mock::given(method("GET"))
            .and(path(format!("/recording/{rec}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "tags": [{ "name": "ambient", "count": 2 }] }),
                ),
            )
            .mount(&server)
            .await;
        let db = temp_db();
        let (artist, track) = seed(&db, "Artist", "Song");
        {
            // The artist is done with MusicBrainz, so only track tags would call it.
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET musicbrainz_artist_id = ?1, musicbrainz_identity_checked_at = datetime('now')",
                [MBID],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO artist_styles(artist_id, style, kind, weight) VALUES (?1, 'ambient', 'genre', 1.0)",
                [&artist],
            )
            .unwrap();
            conn.execute(
                "UPDATE tracks SET musicbrainz_recording_id = ?1 WHERE id = ?2",
                params![rec, track],
            )
            .unwrap();
        }

        for mode in ["lazy", "off"] {
            set_setting(&db, "radioTrackTagSync", mode);
            assert_eq!(
                run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
                Outcome::Idle,
                "{mode}"
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());

        // Off, or with MusicBrainz turned off, the radio-start path does nothing...
        let id = EntityId::Str(track.clone());
        assert_eq!(
            run_lazy_tag_fetch(&db, &Client::new(), std::slice::from_ref(&id)).await,
            0
        );
        set_setting(&db, "radioTrackTagSync", "lazy");
        quiet_musicbrainz(&db);
        assert_eq!(
            run_lazy_tag_fetch(&db, &Client::new(), std::slice::from_ref(&id)).await,
            0
        );
        assert!(server.received_requests().await.unwrap().is_empty());
        // ...in `lazy` it tags the launched radio's tracks from MusicBrainz, once.
        set_setting(&db, "radioKeylessProviders", "true");
        assert_eq!(
            run_lazy_tag_fetch(&db, &Client::new(), std::slice::from_ref(&id)).await,
            1
        );
        assert_eq!(run_lazy_tag_fetch(&db, &Client::new(), &[id]).await, 0);
        let status = radio_metadata_status(&db.lock().unwrap()).unwrap();
        assert_eq!((status.tracks_checked, status.tracks_total), (1, 1));
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
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
        set_setting(&db, "radioTrackTagSync", "off"); // MB artist work runs whatever the track-tag mode
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
    async fn listenbrainz_listen_counts_replace_last_fm_counts_by_recording_then_title() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_API_BASE", server.uri());
        // Keyless MusicBrainz calls find nothing new (the artist already has its id).
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        let rec = "e8f9b188-f819-4e43-ab0f-4bd26ce9ff56";
        Mock::given(method("GET"))
            .and(path(format!("/1/popularity/top-recordings-for-artist/{MBID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "recording_mbid": rec, "recording_name": "Retitled On The Release", "total_listen_count": 900 },
                { "recording_mbid": "0383dadf-2a4e-4d10-a46a-e9e041da8eb3", "recording_name": "Angel", "total_listen_count": 300 },
                { "recording_name": "Never Played", "total_listen_count": 0 }
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let db = temp_db();
        let (_, teardrop) = seed(&db, "Massive Attack", "Teardrop");
        let (_, angel) = seed(&db, "Massive Attack", "Angel (2012 Remaster)");
        let (_, other) = seed(&db, "Massive Attack", "Obscure");
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET musicbrainz_artist_id = ?1, musicbrainz_identity_checked_at = datetime('now')",
                [MBID],
            )
            .unwrap();
            conn.execute(
                "UPDATE tracks SET musicbrainz_recording_id = ?1 WHERE id = ?2",
                params![rec, teardrop],
            )
            .unwrap();
            // Last.fm counted "Obscure" before: ListenBrainz replaces the artist's counts.
            conn.execute(
                "INSERT INTO track_popularity(track_id, scrobbles, source) VALUES (?1, 50, 'lastfm')",
                [&other],
            )
            .unwrap();
        }
        assert_eq!(
            run_one_pass(&db, &Client::new(), &CancellationToken::new()).await,
            Outcome::Progress
        );
        assert_eq!(
            scrobbles(&db, &teardrop),
            Some(900),
            "matched by recording id"
        );
        assert_eq!(scrobbles(&db, &angel), Some(300), "matched by title");
        assert_eq!(scrobbles(&db, &other), None);
        let source: String = db
            .lock()
            .unwrap()
            .query_row("SELECT source FROM artist_popularity_sync", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(source, "listenbrainz");
        // Done for this artist: the next pass doesn't ask again.
        run_one_pass(&db, &Client::new(), &CancellationToken::new()).await;
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn musicbrainz_track_tags_find_the_recording_then_its_tags() {
        let _env = ENV_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        fast_delays();
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_API_BASE", server.uri());
        let found = "e8f9b188-f819-4e43-ab0f-4bd26ce9ff56";
        let tagged = "0383dadf-2a4e-4d10-a46a-e9e041da8eb3";
        Mock::given(method("GET"))
            .and(path("/recording/"))
            .and(query_param("query", "artist:\"Massive Attack\" AND recording:\"Teardrop\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "recordings": [
                    { "id": "11111111-1111-4111-8111-111111111111", "score": 100, "title": "Teardrop",
                      "artist-credit": [{ "name": "Someone Else" }] },
                    { "id": found, "score": 100, "title": "Teardrop",
                      "artist-credit": [{ "name": "Massive Attack" }] }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/recording/"))
            .and(query_param(
                "query",
                "artist:\"Massive Attack\" AND recording:\"Unreleased\"",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "recordings": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/recording/{found}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tags": [{ "name": "trip hop", "count": 1 }],
                "genres": [{ "name": "downtempo", "count": 1 }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/recording/{tagged}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "genres": [{ "name": "electronic", "count": 2 }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/1/popularity/top-recordings-for-artist/{MBID}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        let db = temp_db();
        let (artist, teardrop) = seed(&db, "Massive Attack", "Teardrop");
        let (_, angel) = seed(&db, "Massive Attack", "Angel");
        let (_, unreleased) = seed(&db, "Massive Attack", "Unreleased");
        {
            // The artist is done with MusicBrainz (id and tags), so only the track pass asks it.
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE artists SET musicbrainz_artist_id = ?1, musicbrainz_identity_checked_at = datetime('now')",
                [MBID],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO artist_styles(artist_id, style, kind, weight) VALUES (?1, 'trip-hop', 'genre', 1.0)",
                [&artist],
            )
            .unwrap();
            conn.execute(
                "UPDATE tracks SET musicbrainz_recording_id = ?1 WHERE id = ?2",
                params![tagged, angel],
            )
            .unwrap();
        }
        run_one_pass(&db, &Client::new(), &CancellationToken::new()).await;

        let conn = db.lock().unwrap();
        let tags = |track: &str| -> Vec<String> {
            let mut stmt = conn
                .prepare("SELECT tag FROM track_tags WHERE track_id = ?1 AND source = 'musicbrainz' ORDER BY tag")
                .unwrap();
            stmt.query_map([track], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(tags(&teardrop).len(), 2, "{:?}", tags(&teardrop));
        assert_eq!(tags(&angel).len(), 1, "{:?}", tags(&angel));
        assert!(tags(&unreleased).is_empty());
        let recording: Option<String> = conn
            .query_row(
                "SELECT musicbrainz_recording_id FROM tracks WHERE id = ?1",
                [&teardrop],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(recording.as_deref(), Some(found), "the id found is kept");
        let status: String = conn
            .query_row(
                "SELECT status FROM track_tag_sync WHERE track_id = ?1 AND source = 'musicbrainz'",
                [&unreleased],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "empty");
        drop(conn);
        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_API_BASE");
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

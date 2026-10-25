//! Admin Activity page: what is playing, downloading and rendering right now, plus recent history
//! (wip/activity-dashboard-plan.md).
//!
//! [`ActivityHub`] is in memory. Media handlers register each response with [`ActivityHub::track_response`],
//! which counts bytes as the body is sent; range requests for one item collapse into one transfer, which
//! lingers briefly after its last response so it does not flicker. Finished downloads, long DLNA/radio streams
//! and plays are queued as history events and written to `activity_events` by the background task (never on
//! the request path). [`snapshot`] merges the hub with Play On sessions, the now-playing list and the job
//! tables into one [`ActivitySnapshot`].

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{body::Body, http::header, response::Response};
use boogiebox_db::activity::{
    self as adb, ActiveMixJob, ItemSummary, NewActivityEvent, RecentActivity,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::{
    plays::{NowPlayingEntry, PlayHub, PlaySource},
    remote::{session::SessionStatus, RemoteEvent, RemoteManager},
    DbPool, SharedState,
};

/// A transfer with no open response stays listed this long after its last byte.
const LINGER_MS: u64 = 10_000;
/// A download session (an app syncing songs one after another) ends this long after its last transfer.
const DOWNLOAD_SESSION_IDLE_MS: u64 = 60_000;
/// Window for "N songs in the last 5 min".
const DOWNLOAD_WINDOW_MS: u64 = 5 * 60_000;
/// A web player that has not reported for this long is gone (closed tab, lost network).
const WEB_PLAYER_TTL_MS: u64 = 45_000;
/// DLNA and radio streams shorter than this are not written to history.
const MIN_HISTORY_STREAM_MS: u64 = 30_000;
/// A now-playing notice without position counts as playing until the track's length plus this.
const ESTIMATE_GRACE_SEC: f64 = 30.0;
/// Background sweep interval.
const SWEEP_EVERY: Duration = Duration::from_secs(2);
/// History pruning interval.
const PRUNE_EVERY: Duration = Duration::from_secs(3600);
/// "Recent" shows this much history.
const RECENT_HOURS: i64 = 24;
/// Most "Recent" rows in a snapshot.
const RECENT_LIMIT: usize = 50;
/// Most live transfers kept (oldest closed ones are dropped first beyond this).
const MAX_TRANSFERS: usize = 256;
/// Most queued history events before the oldest are dropped (the writer flushes every sweep).
const MAX_PENDING_EVENTS: usize = 2000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Where a transfer comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Web,
    Subsonic,
    PlayOn,
    Dlna,
    Radio,
    MixFile,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Web => "web",
            Source::Subsonic => "subsonic",
            Source::PlayOn => "playOn",
            Source::Dlna => "dlna",
            Source::Radio => "radio",
            Source::MixFile => "mixFile",
        }
    }
}

impl From<PlaySource> for Source {
    fn from(s: PlaySource) -> Self {
        match s {
            PlaySource::Web => Source::Web,
            PlaySource::PlayOn => Source::PlayOn,
            PlaySource::Subsonic => Source::Subsonic,
        }
    }
}

/// Playback or a file fetched to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferKind {
    Stream,
    Download,
}

/// How a response is delivered (the client words it).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryInfo {
    pub transcoded: bool,
    /// Source format, e.g. `FLAC`.
    pub source_format: Option<String>,
    /// Target format when transcoding, e.g. `MP3`.
    pub target_format: Option<String>,
    pub kbps: Option<u32>,
}

impl DeliveryInfo {
    /// The original file, as stored.
    pub fn original(path: &std::path::Path) -> Self {
        DeliveryInfo {
            transcoded: false,
            source_format: format_of(path),
            target_format: None,
            kbps: None,
        }
    }

    /// A transcode of `path` to `target` (e.g. `MP3`).
    pub fn transcode(path: &std::path::Path, target: &str, kbps: Option<u32>) -> Self {
        DeliveryInfo {
            transcoded: true,
            source_format: format_of(path),
            target_format: Some(target.to_owned()),
            kbps,
        }
    }
}

fn format_of(path: &std::path::Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(str::to_ascii_uppercase)
}

/// What a media handler is about to send.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferMeta {
    pub source: Source,
    pub kind: TransferKind,
    pub user_id: Option<String>,
    pub username: Option<String>,
    /// App name (Subsonic `c`), speaker UDN (Play On), peer address (DLNA); empty for the web.
    pub client: String,
    /// Library track id, `mx-{outputId}` or `st-{stationId}`.
    pub item_id: String,
    pub delivery: Option<DeliveryInfo>,
}

impl TransferMeta {
    fn key(&self) -> Key {
        (
            self.source,
            self.user_id.clone().unwrap_or_default(),
            self.client.clone(),
            self.item_id.clone(),
        )
    }
}

type Key = (Source, String, String, String);

#[derive(Debug)]
struct TransferEntry {
    meta: TransferMeta,
    started_ms: u64,
    total_bytes: Option<u64>,
    bytes: AtomicU64,
    open: AtomicU32,
    last_ms: AtomicU64,
}

impl TransferEntry {
    fn is_live(&self, now: u64) -> bool {
        self.open.load(Ordering::Relaxed) > 0
            || now.saturating_sub(self.last_ms.load(Ordering::Relaxed)) < LINGER_MS
    }
}

/// Keeps a transfer open while a response body is being sent.
pub struct TransferGuard {
    hub: ActivityHub,
    entry: Arc<TransferEntry>,
}

impl Drop for TransferGuard {
    fn drop(&mut self) {
        self.entry.last_ms.store(now_ms(), Ordering::Relaxed);
        self.entry.open.fetch_sub(1, Ordering::Relaxed);
        self.hub.bump();
    }
}

/// An app fetching songs one after another (offline sync) or a run of uncached Subsonic streams.
#[derive(Debug, Clone)]
struct DownloadSession {
    source: Source,
    user_id: Option<String>,
    username: Option<String>,
    client: String,
    started_ms: u64,
    last_ms: u64,
    /// Completed items: (finished at, bytes).
    completed: VecDeque<(u64, u64)>,
    total_items: u64,
    total_bytes: u64,
    /// At least one item came from an explicit download request.
    explicit: bool,
    rate_bps: u64,
    rate_sample: Option<(u64, u64)>,
}

/// A web player's last report.
#[derive(Debug, Clone, PartialEq)]
struct WebPlayer {
    user_id: String,
    username: String,
    track_id: String,
    position_sec: f64,
    duration_sec: Option<f64>,
    paused: bool,
    sampled_ms: u64,
}

/// Body of `POST /api/activity/heartbeat`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    /// Random id per browser tab.
    pub player_id: String,
    pub track_id: Option<String>,
    #[serde(default)]
    pub position_sec: f64,
    pub duration_sec: Option<f64>,
    #[serde(default)]
    pub paused: bool,
    /// The tab is closing or playback stopped.
    #[serde(default)]
    pub stopped: bool,
}

#[derive(Debug)]
struct Inner {
    transfers: Mutex<HashMap<Key, Arc<TransferEntry>>>,
    downloads: Mutex<HashMap<(String, String), DownloadSession>>,
    web_players: Mutex<HashMap<(String, String), WebPlayer>>,
    pending: Mutex<Vec<NewActivityEvent>>,
    changed: watch::Sender<u64>,
}

impl Default for Inner {
    fn default() -> Self {
        Inner {
            transfers: Mutex::default(),
            downloads: Mutex::default(),
            web_players: Mutex::default(),
            pending: Mutex::default(),
            changed: watch::channel(0).0,
        }
    }
}

/// Shared, cheaply cloneable activity state kept in `AppState`.
#[derive(Debug, Clone, Default)]
pub struct ActivityHub(Arc<Inner>);

impl ActivityHub {
    /// Forgets everything (database switched). Queued history belongs to the old database and is dropped.
    pub fn reset(&self) {
        lock(&self.0.transfers).clear();
        lock(&self.0.downloads).clear();
        lock(&self.0.web_players).clear();
        lock(&self.0.pending).clear();
        self.bump();
    }

    fn bump(&self) {
        self.0.changed.send_modify(|v| *v = v.wrapping_add(1));
    }

    /// Tells open Activity pages to refresh (e.g. a job changed in the database).
    pub fn touch(&self) {
        self.bump();
    }

    /// Notified on every change (transfers opened or closed, plays, heartbeats).
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    /// Registers a response and counts its body bytes. HEAD requests and non-success responses (304, 416,
    /// errors) are passed through untouched.
    pub fn track_response(&self, meta: TransferMeta, resp: Response, head_only: bool) -> Response {
        if head_only || !resp.status().is_success() {
            return resp;
        }
        let total_bytes = (resp.status() == axum::http::StatusCode::OK)
            .then(|| {
                resp.headers()
                    .get(header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .flatten();
        let guard = self.open(meta, total_bytes);
        let (parts, body) = resp.into_parts();
        let counted = body.into_data_stream().map(move |chunk| {
            if let Ok(bytes) = &chunk {
                let entry = &guard.entry;
                entry.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                entry.last_ms.store(now_ms(), Ordering::Relaxed);
            }
            chunk
        });
        Response::from_parts(parts, Body::from_stream(counted))
    }

    /// Opens (or joins) a transfer without a body to count, e.g. a radio relay; it ends when the guard drops.
    pub fn open(&self, meta: TransferMeta, total_bytes: Option<u64>) -> TransferGuard {
        let now = now_ms();
        let entry = {
            let mut map = lock(&self.0.transfers);
            if map.len() >= MAX_TRANSFERS {
                if let Some(oldest) = map
                    .iter()
                    .filter(|(_, e)| e.open.load(Ordering::Relaxed) == 0)
                    .min_by_key(|(_, e)| e.last_ms.load(Ordering::Relaxed))
                    .map(|(k, _)| k.clone())
                {
                    map.remove(&oldest);
                }
            }
            let entry = map
                .entry(meta.key())
                .or_insert_with(|| {
                    Arc::new(TransferEntry {
                        meta: meta.clone(),
                        started_ms: now,
                        total_bytes,
                        bytes: AtomicU64::new(0),
                        open: AtomicU32::new(0),
                        last_ms: AtomicU64::new(now),
                    })
                })
                .clone();
            entry.open.fetch_add(1, Ordering::Relaxed);
            entry.last_ms.store(now, Ordering::Relaxed);
            entry
        };
        self.bump();
        TransferGuard {
            hub: self.clone(),
            entry,
        }
    }

    /// A player started an item: queue a history event.
    pub fn record_play(
        &self,
        source: PlaySource,
        user_id: &str,
        username: &str,
        client: &str,
        item_id: &str,
    ) {
        self.push_event(NewActivityEvent {
            at: (now_ms() / 1000) as i64,
            kind: "play".into(),
            user_id: Some(user_id.to_owned()),
            username: Some(username.to_owned()),
            source: Some(Source::from(source).as_str().to_owned()),
            client: Some(client.to_owned()).filter(|c| !c.is_empty()),
            item_id: Some(item_id.to_owned()),
            title: None,
            count: None,
            bytes: None,
        });
        self.bump();
    }

    /// A Play On speaker stopped: queue a history event.
    pub fn record_play_on_stopped(
        &self,
        speaker: &str,
        user_id: Option<&str>,
        username: Option<&str>,
    ) {
        self.push_event(NewActivityEvent {
            at: (now_ms() / 1000) as i64,
            kind: "playOnStopped".into(),
            user_id: user_id.map(str::to_owned),
            username: username.map(str::to_owned),
            source: Some(Source::PlayOn.as_str().to_owned()),
            client: Some(speaker.to_owned()),
            item_id: None,
            title: None,
            count: None,
            bytes: None,
        });
        self.bump();
    }

    fn push_event(&self, event: NewActivityEvent) {
        let mut pending = lock(&self.0.pending);
        if pending.len() >= MAX_PENDING_EVENTS {
            pending.remove(0);
        }
        pending.push(event);
    }

    /// Takes the queued history events.
    pub fn take_events(&self) -> Vec<NewActivityEvent> {
        std::mem::take(&mut *lock(&self.0.pending))
    }

    /// A web player's report (position, pause, stop).
    pub fn heartbeat(&self, user_id: &str, username: &str, beat: Heartbeat) {
        let key = (
            user_id.to_owned(),
            beat.player_id.chars().take(64).collect(),
        );
        let mut players = lock(&self.0.web_players);
        match beat.track_id.filter(|t| !t.is_empty() && !beat.stopped) {
            Some(track_id) => {
                players.insert(
                    key,
                    WebPlayer {
                        user_id: user_id.to_owned(),
                        username: username.to_owned(),
                        track_id,
                        position_sec: beat.position_sec.max(0.0),
                        duration_sec: beat.duration_sec.filter(|d| d.is_finite() && *d > 0.0),
                        paused: beat.paused,
                        sampled_ms: now_ms(),
                    },
                );
            }
            None => {
                players.remove(&key);
            }
        }
        drop(players);
        self.bump();
    }

    /// Ends lingering transfers, folds downloads into sessions, ends idle sessions and stale web players.
    /// `now_playing` decides whether a closed Subsonic stream was playback or caching. Returns whether
    /// anything changed.
    pub fn sweep(&self, now: u64, now_playing: &[NowPlayingEntry]) -> bool {
        let ended: Vec<Arc<TransferEntry>> = {
            let mut map = lock(&self.0.transfers);
            let keys: Vec<Key> = map
                .iter()
                .filter(|(_, e)| !e.is_live(now))
                .map(|(k, _)| k.clone())
                .collect();
            keys.into_iter().filter_map(|k| map.remove(&k)).collect()
        };
        let mut changed = !ended.is_empty();
        for entry in &ended {
            self.transfer_ended(entry, now_playing);
        }

        let open_sessions: HashSet<(String, String)> = lock(&self.0.transfers)
            .values()
            .filter(|e| counts_as_download(&e.meta, now_playing))
            .map(|e| session_key(&e.meta))
            .collect();
        let live_bytes = self.live_session_bytes(now_playing);
        let mut finished = Vec::new();
        {
            let mut sessions = lock(&self.0.downloads);
            for (key, s) in sessions.iter_mut() {
                while s
                    .completed
                    .front()
                    .is_some_and(|(at, _)| now.saturating_sub(*at) > DOWNLOAD_WINDOW_MS)
                {
                    s.completed.pop_front();
                }
                let total = s.total_bytes + live_bytes.get(key).copied().unwrap_or(0);
                if let Some((at, bytes)) = s.rate_sample {
                    let dt = now.saturating_sub(at);
                    if dt >= 1000 {
                        s.rate_bps = total.saturating_sub(bytes) * 1000 / dt;
                        s.rate_sample = Some((now, total));
                    }
                } else {
                    s.rate_sample = Some((now, total));
                }
            }
            let idle: Vec<(String, String)> = sessions
                .iter()
                .filter(|(k, s)| {
                    !open_sessions.contains(*k)
                        && now.saturating_sub(s.last_ms) > DOWNLOAD_SESSION_IDLE_MS
                })
                .map(|(k, _)| k.clone())
                .collect();
            for key in idle {
                if let Some(s) = sessions.remove(&key) {
                    finished.push(s);
                }
            }
        }
        for s in finished {
            changed = true;
            if s.explicit || s.total_items >= 2 {
                self.push_event(NewActivityEvent {
                    at: (s.last_ms / 1000) as i64,
                    kind: "download".into(),
                    user_id: s.user_id,
                    username: s.username,
                    source: Some(s.source.as_str().to_owned()),
                    client: Some(s.client).filter(|c| !c.is_empty()),
                    item_id: None,
                    title: None,
                    count: Some(s.total_items as i64),
                    bytes: Some(s.total_bytes as i64),
                });
            }
        }

        let before = lock(&self.0.web_players).len();
        lock(&self.0.web_players)
            .retain(|_, p| now.saturating_sub(p.sampled_ms) < WEB_PLAYER_TTL_MS);
        changed |= lock(&self.0.web_players).len() != before;
        if changed {
            self.bump();
        }
        changed
    }

    fn transfer_ended(&self, entry: &TransferEntry, now_playing: &[NowPlayingEntry]) {
        let meta = &entry.meta;
        let bytes = entry.bytes.load(Ordering::Relaxed);
        let last = entry.last_ms.load(Ordering::Relaxed);
        match meta.source {
            Source::MixFile => self.push_event(NewActivityEvent {
                at: (last / 1000) as i64,
                kind: "download".into(),
                user_id: meta.user_id.clone(),
                username: meta.username.clone(),
                source: Some(meta.source.as_str().to_owned()),
                client: None,
                item_id: Some(meta.item_id.clone()),
                title: None,
                count: Some(1),
                bytes: Some(bytes as i64),
            }),
            Source::Dlna | Source::Radio
                if last.saturating_sub(entry.started_ms) >= MIN_HISTORY_STREAM_MS =>
            {
                self.push_event(NewActivityEvent {
                    at: (entry.started_ms / 1000) as i64,
                    kind: "stream".into(),
                    user_id: meta.user_id.clone(),
                    username: meta.username.clone(),
                    source: Some(meta.source.as_str().to_owned()),
                    client: Some(meta.client.clone()).filter(|c| !c.is_empty()),
                    item_id: Some(meta.item_id.clone()),
                    title: None,
                    count: None,
                    bytes: Some(bytes as i64),
                })
            }
            _ if counts_as_download(meta, now_playing) && bytes > 0 => {
                let mut sessions = lock(&self.0.downloads);
                let s = sessions
                    .entry(session_key(meta))
                    .or_insert_with(|| DownloadSession {
                        source: meta.source,
                        user_id: meta.user_id.clone(),
                        username: meta.username.clone(),
                        client: meta.client.clone(),
                        started_ms: entry.started_ms,
                        last_ms: last,
                        completed: VecDeque::new(),
                        total_items: 0,
                        total_bytes: 0,
                        explicit: false,
                        rate_bps: 0,
                        rate_sample: None,
                    });
                s.completed.push_back((last, bytes));
                s.total_items += 1;
                s.total_bytes += bytes;
                s.last_ms = s.last_ms.max(last);
                s.explicit |= meta.kind == TransferKind::Download;
            }
            _ => {}
        }
    }

    /// Bytes of open download transfers per session.
    fn live_session_bytes(
        &self,
        now_playing: &[NowPlayingEntry],
    ) -> HashMap<(String, String), u64> {
        let mut out = HashMap::new();
        for e in lock(&self.0.transfers).values() {
            if counts_as_download(&e.meta, now_playing) {
                *out.entry(session_key(&e.meta)).or_insert(0) += e.bytes.load(Ordering::Relaxed);
            }
        }
        out
    }

    fn live(&self, now: u64, now_playing: &[NowPlayingEntry]) -> Live {
        let transfers: Vec<LiveTransfer> = lock(&self.0.transfers)
            .values()
            .filter(|e| e.is_live(now))
            .map(|e| LiveTransfer {
                meta: e.meta.clone(),
                started_ms: e.started_ms,
                total_bytes: e.total_bytes,
                bytes: e.bytes.load(Ordering::Relaxed),
                open: e.open.load(Ordering::Relaxed) > 0,
                is_download: counts_as_download(&e.meta, now_playing),
            })
            .collect();
        Live {
            transfers,
            sessions: lock(&self.0.downloads).values().cloned().collect(),
            web_players: lock(&self.0.web_players).values().cloned().collect(),
        }
    }
}

fn session_key(meta: &TransferMeta) -> (String, String) {
    (
        meta.user_id.clone().unwrap_or_default(),
        format!("{}:{}", meta.source.as_str(), meta.client),
    )
}

/// Explicit Subsonic downloads, and Subsonic streams of something the app is not playing (offline caching).
fn counts_as_download(meta: &TransferMeta, now_playing: &[NowPlayingEntry]) -> bool {
    match (meta.source, meta.kind) {
        (Source::Subsonic, TransferKind::Download) => true,
        (Source::Subsonic, TransferKind::Stream) => !now_playing.iter().any(|n| {
            n.source == PlaySource::Subsonic
                && Some(&n.user_id) == meta.user_id.as_ref()
                && n.client == meta.client
                && n.track_id == meta.item_id
        }),
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct LiveTransfer {
    meta: TransferMeta,
    started_ms: u64,
    total_bytes: Option<u64>,
    bytes: u64,
    open: bool,
    is_download: bool,
}

#[derive(Debug, Clone)]
struct Live {
    transfers: Vec<LiveTransfer>,
    sessions: Vec<DownloadSession>,
    web_players: Vec<WebPlayer>,
}

// ── Snapshot ───────────────────────────────────────────────────────────────────

/// Everything the Activity page shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub generated_at_ms: u64,
    pub totals: Totals,
    pub playing: Vec<PlayingRow>,
    pub downloads: Vec<DownloadRow>,
    pub mix_jobs: Vec<ActiveMixJob>,
    pub server_jobs: ServerJobs,
    pub recent: Vec<RecentRow>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub playing: usize,
    pub downloading: usize,
    pub mix_jobs: usize,
    pub transcodes: usize,
    pub subsonic_transcode_limit: Option<u32>,
    pub active_users: usize,
    pub server_jobs: usize,
}

/// One row of "Now playing".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingRow {
    pub id: String,
    pub source: Source,
    pub user: Option<String>,
    /// App name, speaker name or DLNA device name.
    pub client: Option<String>,
    pub item_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<String>,
    pub position_sec: Option<f64>,
    pub duration_sec: Option<f64>,
    /// The position is worked out from the start time, not reported by the player.
    pub estimated: bool,
    pub sampled_at_ms: u64,
    /// `playing`, `paused`, `loading` or `streaming` (bytes flowing, no player report).
    pub status: &'static str,
    pub delivery: Option<DeliveryInfo>,
}

/// One row of "Downloads".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    pub id: String,
    pub source: Source,
    pub user: Option<String>,
    pub client: Option<String>,
    /// `offlineSync` (many songs) or `file` (one file, e.g. a BoogieMix).
    pub label: &'static str,
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub items_completed: u64,
    pub bytes: u64,
    pub in_progress: usize,
    pub rate_bps: u64,
    pub total_bytes: Option<u64>,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerJobs {
    pub scans: Vec<ScanRow>,
    /// Pending or running post-scan jobs by type (`artwork`, `waveforms`, …).
    pub post_scan_queued: Vec<(String, i64)>,
    pub deep_analysis_queued: i64,
    pub deep_analysis_running: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRow {
    pub id: String,
    pub library: Option<String>,
    pub status: String,
    pub files_scanned: Option<i64>,
    pub files_found: Option<i64>,
    pub started_at: Option<String>,
}

/// One "Recent" row; `at` is ms since epoch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentRow {
    pub at_ms: u64,
    pub kind: String,
    pub user: Option<String>,
    pub source: Option<String>,
    pub client: Option<String>,
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub count: Option<i64>,
    pub bytes: Option<i64>,
}

/// What the snapshot reads from the database in one lock hold.
struct DbPart {
    items: HashMap<String, ItemSummary>,
    mix_jobs: Vec<ActiveMixJob>,
    server_jobs: ServerJobs,
    recent: Vec<RecentActivity>,
    transcode_limit: Option<u32>,
}

/// The activity hub held in `AppState`.
pub fn hub_of(state: &SharedState) -> ActivityHub {
    state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .activity
        .clone()
}

/// DLNA requests carry the peer address as their client; show the device's name when it is a known renderer,
/// and never the address.
fn name_dlna_peers(
    transfers: &mut [LiveTransfer],
    renderers: &[crate::remote::registry::RendererEntry],
) {
    let names: HashMap<String, String> = renderers
        .iter()
        .filter_map(|r| {
            let host = reqwest::Url::parse(&r.location)
                .ok()?
                .host_str()?
                .to_owned();
            Some((host, r.description.friendly_name.clone()))
        })
        .collect();
    for t in transfers
        .iter_mut()
        .filter(|t| t.meta.source == Source::Dlna)
    {
        t.meta.client = names.get(&t.meta.client).cloned().unwrap_or_default();
    }
}

/// Builds the Activity snapshot. `None` before setup.
pub async fn snapshot(state: &SharedState) -> Option<ActivitySnapshot> {
    let (db, hub, plays, remote) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (
            s.db.clone()?,
            s.activity.clone(),
            s.plays.clone(),
            s.remote.clone(),
        )
    };
    let now = now_ms();
    let now_playing = plays.now_playing();
    let mut live = hub.live(now, &now_playing);
    name_dlna_peers(&mut live.transfers, &remote.registry().snapshot());
    let sessions = remote.session_snapshots();

    let mut ids: Vec<String> = now_playing.iter().map(|n| n.track_id.clone()).collect();
    ids.extend(live.web_players.iter().map(|p| p.track_id.clone()));
    ids.extend(live.transfers.iter().map(|t| t.meta.item_id.clone()));
    let pending = hub.take_events();
    let db_part = tokio::task::spawn_blocking(move || read_db(&db, ids, pending))
        .await
        .ok()?;
    Some(assemble(now, now_playing, live, sessions, db_part))
}

fn read_db(db: &DbPool, mut ids: Vec<String>, pending: Vec<NewActivityEvent>) -> DbPart {
    let started = std::time::Instant::now();
    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
    if let Err(e) = adb::insert_activity_events(&conn, &pending) {
        tracing::warn!("Activity history write failed: {e}");
    }
    let since_secs = (now_ms() / 1000) as i64 - RECENT_HOURS * 3600;
    let since: String = conn
        .query_row("SELECT datetime(?1, 'unixepoch')", [since_secs], |r| {
            r.get(0)
        })
        .unwrap_or_else(|_| "1970-01-01 00:00:00".into());
    let recent = adb::list_recent_activity(&conn, &since, RECENT_LIMIT).unwrap_or_default();
    ids.extend(recent.iter().filter_map(|r| {
        r.item_id.clone().filter(|_| {
            r.title.is_none() && matches!(r.kind.as_str(), "play" | "stream" | "download")
        })
    }));
    ids.sort();
    ids.dedup();
    let items = adb::item_summaries(&conn, &ids).unwrap_or_default();
    let mix_jobs = adb::list_active_mix_jobs(&conn).unwrap_or_default();
    let queues = boogiebox_db::jobs::queue_snapshot(&conn).ok();
    let (deep_queued, deep_running) = adb::deep_analysis_active_counts(&conn).unwrap_or((0, 0));
    let transcode_limit = boogiebox_db::artwork::get_setting(&conn, "subsonicMaxTranscodes")
        .and_then(|v| v.parse::<u32>().ok())
        .or(Some(3));
    drop(conn);
    let held = started.elapsed();
    if held > Duration::from_millis(50) {
        tracing::warn!(
            held_ms = held.as_millis() as u64,
            "Activity snapshot held the database lock"
        );
    } else {
        tracing::debug!(
            held_us = held.as_micros() as u64,
            "Activity snapshot lock hold"
        );
    }

    let mut server_jobs = ServerJobs {
        deep_analysis_queued: deep_queued,
        deep_analysis_running: deep_running,
        ..ServerJobs::default()
    };
    if let Some(q) = queues {
        server_jobs.scans = q
            .queues
            .scan
            .into_iter()
            .map(|r| ScanRow {
                id: r.id.to_string(),
                library: r.library_name,
                status: r.status,
                files_scanned: r.files_scanned,
                files_found: r.files_found,
                started_at: r.started_at,
            })
            .collect();
        let mut by_type: Vec<(String, i64)> = Vec::new();
        for row in q.queues.post_scan {
            let kind = row.job_type.unwrap_or_else(|| "other".into());
            match by_type.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, n)) => *n += 1,
                None => by_type.push((kind, 1)),
            }
        }
        server_jobs.post_scan_queued = by_type;
    }
    DbPart {
        items,
        mix_jobs,
        server_jobs,
        recent,
        transcode_limit,
    }
}

fn sqlite_time_ms(at: &str) -> u64 {
    // `YYYY-MM-DD HH:MM:SS` (UTC) → ms since epoch; 0 when unreadable.
    let num = |range: std::ops::Range<usize>| at.get(range).and_then(|v| v.parse::<u32>().ok());
    let parsed = (|| {
        let month = time::Month::try_from(num(5..7)? as u8).ok()?;
        let date =
            time::Date::from_calendar_date(num(0..4)? as i32, month, num(8..10)? as u8).ok()?;
        let clock =
            time::Time::from_hms(num(11..13)? as u8, num(14..16)? as u8, num(17..19)? as u8)
                .ok()?;
        Some(
            time::PrimitiveDateTime::new(date, clock)
                .assume_utc()
                .unix_timestamp(),
        )
    })();
    parsed.map_or(0, |secs| secs.max(0) as u64 * 1000)
}

fn assemble(
    now: u64,
    now_playing: Vec<NowPlayingEntry>,
    live: Live,
    sessions: Vec<crate::remote::session::SessionState>,
    db: DbPart,
) -> ActivitySnapshot {
    let item = |id: &str| db.items.get(id);
    let delivery_for = |source: Source, user: Option<&str>, client: Option<&str>, item_id: &str| {
        live.transfers
            .iter()
            .find(|t| {
                t.meta.source == source
                    && (user.is_none() || t.meta.user_id.as_deref() == user)
                    && client.is_none_or(|c| t.meta.client == c)
                    && (item_id.is_empty() || t.meta.item_id == item_id)
            })
            .and_then(|t| t.meta.delivery.clone())
    };
    let mut playing = Vec::new();

    // 1. Play On sessions: real positions.
    let mut play_on_users: HashSet<(String, String)> = HashSet::new();
    for s in &sessions {
        let status = match s.status {
            SessionStatus::Playing => "playing",
            SessionStatus::Paused => "paused",
            SessionStatus::Loading => "loading",
            _ => continue,
        };
        let Some(current) = s
            .current_qid
            .and_then(|q| s.queue.iter().find(|e| e.qid == q))
        else {
            continue;
        };
        let (item_id, title, artist, album_id) = match &current.item {
            crate::remote::queue::QueueItem::Track {
                id,
                title,
                artist,
                album_id,
                ..
            } => (id.clone(), title.clone(), artist.clone(), album_id.clone()),
            crate::remote::queue::QueueItem::Mix { id, title, .. } => (
                format!("mx-{id}"),
                title.clone(),
                Some("BoogieMix".into()),
                None,
            ),
        };
        if let Some(owner) = &s.owner_user_id {
            play_on_users.insert((owner.clone(), s.renderer_name.clone()));
        }
        playing.push(PlayingRow {
            id: format!("playOn:{}", s.udn),
            source: Source::PlayOn,
            user: s.owner_name.clone(),
            client: Some(s.renderer_name.clone()),
            delivery: delivery_for(Source::PlayOn, None, Some(&s.udn), ""),
            item_id,
            title: Some(title),
            artist,
            album_id,
            position_sec: Some(s.position_sec),
            duration_sec: s.duration_sec.or(current.item.duration()),
            estimated: false,
            sampled_at_ms: s.sampled_at_ms,
            status,
        });
    }

    // 2. Web players that report their position.
    let web_users: HashSet<&str> = live
        .web_players
        .iter()
        .map(|p| p.user_id.as_str())
        .collect();
    for p in &live.web_players {
        let info = item(&p.track_id);
        playing.push(PlayingRow {
            id: format!("web:{}:{}", p.user_id, p.track_id),
            source: Source::Web,
            user: Some(p.username.clone()),
            client: None,
            item_id: p.track_id.clone(),
            title: info.map(|i| i.title.clone()),
            artist: info.and_then(|i| i.artist.clone()),
            album_id: info.and_then(|i| i.album_id.clone()),
            position_sec: Some(p.position_sec),
            duration_sec: p.duration_sec.or(info.and_then(|i| i.duration)),
            estimated: false,
            sampled_at_ms: p.sampled_ms,
            status: if p.paused { "paused" } else { "playing" },
            delivery: delivery_for(Source::Web, Some(&p.user_id), None, &p.track_id),
        });
    }

    // 3. Now-playing notices without a position (Subsonic apps, older web clients): estimated.
    for n in &now_playing {
        if n.source == PlaySource::PlayOn
            || (n.source == PlaySource::Web && web_users.contains(n.user_id.as_str()))
            || play_on_users.contains(&(n.user_id.clone(), n.client.clone()))
        {
            continue;
        }
        let info = item(&n.track_id);
        let duration = info.and_then(|i| i.duration);
        let elapsed = n.at.elapsed().as_secs_f64();
        if duration.is_some_and(|d| elapsed > d + ESTIMATE_GRACE_SEC) {
            continue;
        }
        let source = Source::from(n.source);
        let client = (source == Source::Subsonic).then(|| n.client.clone());
        playing.push(PlayingRow {
            id: format!("{}:{}:{}", source.as_str(), n.user_id, n.client),
            source,
            user: Some(n.username.clone()),
            delivery: delivery_for(source, Some(&n.user_id), client.as_deref(), &n.track_id),
            client,
            item_id: n.track_id.clone(),
            title: info.map(|i| i.title.clone()),
            artist: info.and_then(|i| i.artist.clone()),
            album_id: info.and_then(|i| i.album_id.clone()),
            position_sec: Some(duration.map_or(elapsed, |d| elapsed.min(d))),
            duration_sec: duration,
            estimated: true,
            sampled_at_ms: now,
            status: "playing",
        });
    }

    // 4. Streams no player reported: DLNA, radio, web streams without a heartbeat.
    for t in live.transfers.iter().filter(|t| t.open && !t.is_download) {
        let m = &t.meta;
        if !matches!(m.source, Source::Dlna | Source::Radio | Source::Web)
            || m.kind != TransferKind::Stream
        {
            continue;
        }
        let claimed = playing.iter().any(|p| {
            p.source == m.source
                && p.item_id == m.item_id
                && p.user.as_deref() == m.username.as_deref()
        });
        if claimed
            || (m.source == Source::Web
                && m.user_id.as_deref().is_some_and(|u| web_users.contains(u)))
        {
            continue;
        }
        let info = item(&m.item_id);
        playing.push(PlayingRow {
            id: format!(
                "{}:{}:{}:{}",
                m.source.as_str(),
                m.user_id.as_deref().unwrap_or(""),
                m.client,
                m.item_id
            ),
            source: m.source,
            user: m.username.clone(),
            client: Some(m.client.clone()).filter(|c| !c.is_empty()),
            item_id: m.item_id.clone(),
            title: info.map(|i| i.title.clone()),
            artist: info.and_then(|i| i.artist.clone()),
            album_id: info.and_then(|i| i.album_id.clone()),
            position_sec: None,
            duration_sec: info.and_then(|i| i.duration),
            estimated: false,
            sampled_at_ms: now,
            status: "streaming",
            delivery: m.delivery.clone(),
        });
    }

    // Downloads: single files, then download sessions (finished items plus the ones in flight).
    let mut downloads = Vec::new();
    for t in live
        .transfers
        .iter()
        .filter(|t| t.meta.source == Source::MixFile && t.open)
    {
        let m = &t.meta;
        downloads.push(DownloadRow {
            id: format!("file:{}:{}", m.user_id.as_deref().unwrap_or(""), m.item_id),
            source: m.source,
            user: m.username.clone(),
            client: None,
            label: "file",
            item_id: Some(m.item_id.clone()),
            title: item(&m.item_id).map(|i| i.title.clone()),
            items_completed: 0,
            bytes: t.bytes,
            in_progress: 1,
            rate_bps: 0,
            total_bytes: t.total_bytes,
            started_at_ms: t.started_ms,
        });
    }
    let mut session_rows: HashMap<(String, String), DownloadRow> = HashMap::new();
    for s in &live.sessions {
        let recent_items = s.completed.len() as u64;
        let recent_bytes: u64 = s.completed.iter().map(|(_, b)| b).sum();
        session_rows.insert(
            (
                s.user_id.clone().unwrap_or_default(),
                format!("{}:{}", s.source.as_str(), s.client),
            ),
            DownloadRow {
                id: format!("sync:{}:{}", s.user_id.as_deref().unwrap_or(""), s.client),
                source: s.source,
                user: s.username.clone(),
                client: Some(s.client.clone()).filter(|c| !c.is_empty()),
                label: "offlineSync",
                item_id: None,
                title: None,
                items_completed: recent_items,
                bytes: recent_bytes,
                in_progress: 0,
                rate_bps: s.rate_bps,
                total_bytes: None,
                started_at_ms: s.started_ms,
            },
        );
    }
    let explicit_sessions: HashSet<(String, String)> = live
        .sessions
        .iter()
        .filter(|s| s.explicit)
        .map(|s| {
            (
                s.user_id.clone().unwrap_or_default(),
                format!("{}:{}", s.source.as_str(), s.client),
            )
        })
        .collect();
    let mut open_explicit: HashSet<(String, String)> = HashSet::new();
    for t in live.transfers.iter().filter(|t| t.is_download && t.open) {
        let key = session_key(&t.meta);
        if t.meta.kind == TransferKind::Download {
            open_explicit.insert(key.clone());
        }
        let row = session_rows.entry(key).or_insert_with(|| DownloadRow {
            id: format!(
                "sync:{}:{}",
                t.meta.user_id.as_deref().unwrap_or(""),
                t.meta.client
            ),
            source: t.meta.source,
            user: t.meta.username.clone(),
            client: Some(t.meta.client.clone()).filter(|c| !c.is_empty()),
            label: "offlineSync",
            item_id: None,
            title: None,
            items_completed: 0,
            bytes: 0,
            in_progress: 0,
            rate_bps: 0,
            total_bytes: None,
            started_at_ms: t.started_ms,
        });
        row.in_progress += 1;
        row.bytes += t.bytes;
    }
    let mut syncs: Vec<DownloadRow> = session_rows
        .into_iter()
        .filter(|(key, row)| {
            // One cached stream (e.g. the next song prefetched) is not a sync.
            row.items_completed + row.in_progress as u64 >= 2
                || explicit_sessions.contains(key)
                || open_explicit.contains(key)
        })
        .map(|(_, row)| row)
        .collect();
    syncs.sort_by_key(|r| r.started_at_ms);
    downloads.extend(syncs);

    // Totals.
    let transcodes = live
        .transfers
        .iter()
        .filter(|t| t.open && t.meta.delivery.as_ref().is_some_and(|d| d.transcoded))
        .count();
    let mut users: HashSet<String> = HashSet::new();
    users.extend(playing.iter().filter_map(|p| p.user.clone()));
    users.extend(downloads.iter().filter_map(|d| d.user.clone()));
    users.extend(db.mix_jobs.iter().filter_map(|j| j.user.clone()));
    let server_job_count = db.server_jobs.scans.len()
        + usize::from(!db.server_jobs.post_scan_queued.is_empty())
        + usize::from(db.server_jobs.deep_analysis_running > 0);

    let recent = db
        .recent
        .into_iter()
        .map(|r| {
            let title = r.title.clone().or_else(|| {
                r.item_id
                    .as_deref()
                    .and_then(|id| db.items.get(id))
                    .map(|i| i.title.clone())
            });
            RecentRow {
                at_ms: sqlite_time_ms(&r.at),
                kind: r.kind,
                user: r.user,
                source: r.source,
                client: r.client,
                item_id: r.item_id,
                title,
                count: r.count,
                bytes: r.bytes,
            }
        })
        .collect();

    ActivitySnapshot {
        generated_at_ms: now,
        totals: Totals {
            playing: playing.len(),
            downloading: downloads.len(),
            mix_jobs: db.mix_jobs.len(),
            transcodes,
            subsonic_transcode_limit: db.transcode_limit,
            active_users: users.len(),
            server_jobs: server_job_count,
        },
        playing,
        downloads,
        mix_jobs: db.mix_jobs,
        server_jobs: db.server_jobs,
        recent,
    }
}

// ── Background work ────────────────────────────────────────────────────────────

/// Starts the sweeper / history writer and the Play On "stopped" listener for one database. Both stop with
/// `cancel` (database switch or shutdown).
pub fn start(
    hub: ActivityHub,
    plays: PlayHub,
    remote: RemoteManager,
    db: DbPool,
    cancel: CancellationToken,
) {
    {
        let (hub, db, cancel) = (hub.clone(), db.clone(), cancel.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SWEEP_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut last_prune: Option<std::time::Instant> = None;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => {}
                }
                hub.sweep(now_ms(), &plays.now_playing());
                let events = hub.take_events();
                let prune = last_prune.is_none_or(|t| t.elapsed() >= PRUNE_EVERY);
                if events.is_empty() && !prune {
                    continue;
                }
                if prune {
                    last_prune = Some(std::time::Instant::now());
                }
                let db = db.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    if let Err(e) = adb::insert_activity_events(&conn, &events) {
                        tracing::warn!("Activity history write failed: {e}");
                    }
                    if prune {
                        match adb::prune_activity_events(&conn) {
                            Ok(n) if n > 0 => {
                                tracing::debug!(removed = n, "Pruned activity history")
                            }
                            Ok(_) => {}
                            Err(e) => tracing::warn!("Activity history prune failed: {e}"),
                        }
                    }
                })
                .await;
            }
        });
    }
    tokio::spawn(async move {
        let mut rx = remote.subscribe();
        // Last known speaker name and holder per renderer, so "stopped" can name them.
        let mut speakers: HashMap<String, (String, Option<String>, Option<String>, bool)> =
            HashMap::new();
        loop {
            let event = tokio::select! {
                _ = cancel.cancelled() => break,
                e = rx.recv() => e,
            };
            match event {
                Ok(RemoteEvent::Session { state }) => {
                    let active = matches!(
                        state.status,
                        SessionStatus::Playing | SessionStatus::Paused | SessionStatus::Loading
                    );
                    let entry = speakers.entry(state.udn.clone()).or_insert((
                        state.renderer_name.clone(),
                        None,
                        None,
                        false,
                    ));
                    entry.0 = state.renderer_name.clone();
                    if state.owner_user_id.is_some() {
                        entry.1 = state.owner_user_id.clone();
                        entry.2 = state.owner_name.clone();
                    }
                    if active {
                        entry.3 = true;
                    } else if entry.3 && state.status == SessionStatus::Stopped {
                        entry.3 = false;
                        hub.record_play_on_stopped(
                            &entry.0,
                            entry.1.as_deref(),
                            entry.2.as_deref(),
                        );
                    }
                    hub.bump();
                }
                Ok(RemoteEvent::SessionEnded { udn }) => {
                    if let Some((name, user, username, was_active)) = speakers.remove(&udn) {
                        if was_active {
                            hub.record_play_on_stopped(&name, user.as_deref(), username.as_deref());
                        }
                    }
                    hub.bump();
                }
                Ok(RemoteEvent::Takeover { .. }) => hub.bump(),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => hub.bump(),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::StatusCode};
    use std::time::Instant;

    fn meta(
        source: Source,
        kind: TransferKind,
        user: &str,
        client: &str,
        item: &str,
    ) -> TransferMeta {
        TransferMeta {
            source,
            kind,
            user_id: Some(user.into()),
            username: Some(user.into()),
            client: client.into(),
            item_id: item.into(),
            delivery: None,
        }
    }

    fn body_response(bytes: &'static [u8]) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes))
            .unwrap()
    }

    fn np(source: PlaySource, user: &str, client: &str, track: &str) -> NowPlayingEntry {
        NowPlayingEntry {
            user_id: user.into(),
            username: user.into(),
            client: client.into(),
            track_id: track.into(),
            source,
            at: Instant::now(),
        }
    }

    fn entry_count(hub: &ActivityHub) -> usize {
        lock(&hub.0.transfers).len()
    }

    fn age_all(hub: &ActivityHub, by_ms: u64) {
        for e in lock(&hub.0.transfers).values() {
            e.last_ms.fetch_sub(by_ms, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn counts_body_bytes_and_collapses_range_requests() {
        let hub = ActivityHub::default();
        let m = meta(Source::Web, TransferKind::Stream, "u1", "", "t1");
        let a = hub.track_response(m.clone(), body_response(b"hello"), false);
        let b = hub.track_response(m, body_response(b"world!"), false);
        assert_eq!(entry_count(&hub), 1);
        let e = lock(&hub.0.transfers).values().next().unwrap().clone();
        assert_eq!(e.open.load(Ordering::Relaxed), 2);
        assert_eq!(
            to_bytes(a.into_body(), usize::MAX).await.unwrap().as_ref(),
            b"hello"
        );
        assert_eq!(to_bytes(b.into_body(), usize::MAX).await.unwrap().len(), 6);
        assert_eq!(e.bytes.load(Ordering::Relaxed), 11);
        assert_eq!(
            e.open.load(Ordering::Relaxed),
            0,
            "guards dropped with the bodies"
        );
        assert_eq!(e.total_bytes, Some(5));
    }

    #[tokio::test]
    async fn head_and_non_success_responses_are_not_tracked() {
        let hub = ActivityHub::default();
        let m = meta(Source::Web, TransferKind::Stream, "u1", "", "t1");
        let _ = hub.track_response(m.clone(), body_response(b"x"), true);
        let not_modified = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .unwrap();
        let _ = hub.track_response(m, not_modified, false);
        assert_eq!(entry_count(&hub), 0);
    }

    #[tokio::test]
    async fn a_dropped_body_ends_the_transfer_after_the_linger() {
        let hub = ActivityHub::default();
        let resp = hub.track_response(
            meta(Source::Web, TransferKind::Stream, "u1", "", "t1"),
            body_response(b"abc"),
            false,
        );
        drop(resp);
        let now = now_ms();
        assert!(!hub.sweep(now, &[]), "still lingering");
        assert_eq!(entry_count(&hub), 1);
        assert!(hub.sweep(now + LINGER_MS + 1, &[]));
        assert_eq!(entry_count(&hub), 0);
    }

    #[tokio::test]
    async fn subsonic_downloads_become_one_session_and_one_history_event() {
        let hub = ActivityHub::default();
        for track in ["t1", "t2", "t3"] {
            let r = hub.track_response(
                meta(
                    Source::Subsonic,
                    TransferKind::Download,
                    "u1",
                    "Amperfy",
                    track,
                ),
                body_response(b"0123456789"),
                false,
            );
            to_bytes(r.into_body(), usize::MAX).await.unwrap();
        }
        age_all(&hub, LINGER_MS + 1);
        let now = now_ms();
        hub.sweep(now, &[]);
        let live = hub.live(now, &[]);
        assert_eq!(live.sessions.len(), 1);
        assert_eq!(live.sessions[0].total_items, 3);
        assert_eq!(live.sessions[0].total_bytes, 30);
        assert!(hub.take_events().is_empty(), "the session is still open");

        hub.sweep(now + DOWNLOAD_SESSION_IDLE_MS + 1, &[]);
        let events = hub.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "download");
        assert_eq!(events[0].count, Some(3));
        assert_eq!(events[0].bytes, Some(30));
        assert_eq!(events[0].client.as_deref(), Some("Amperfy"));
    }

    #[tokio::test]
    async fn a_subsonic_stream_of_the_playing_song_is_playback_not_a_download() {
        let hub = ActivityHub::default();
        let playing = [np(PlaySource::Subsonic, "u1", "Amperfy", "t1")];
        let m = meta(
            Source::Subsonic,
            TransferKind::Stream,
            "u1",
            "Amperfy",
            "t1",
        );
        assert!(!counts_as_download(&m, &playing));
        assert!(counts_as_download(&m, &[]));
        let r = hub.track_response(m, body_response(b"abc"), false);
        to_bytes(r.into_body(), usize::MAX).await.unwrap();
        age_all(&hub, LINGER_MS + 1);
        hub.sweep(now_ms(), &playing);
        assert!(hub.live(now_ms(), &playing).sessions.is_empty());
    }

    #[test]
    fn heartbeats_add_update_and_remove_web_players() {
        let hub = ActivityHub::default();
        let beat = |track: Option<&str>, stopped: bool| Heartbeat {
            player_id: "tab1".into(),
            track_id: track.map(String::from),
            position_sec: 12.0,
            duration_sec: Some(200.0),
            paused: true,
            stopped,
        };
        hub.heartbeat("u1", "alice", beat(Some("t1"), false));
        hub.heartbeat("u1", "alice", beat(Some("t2"), false));
        let players = hub.live(now_ms(), &[]).web_players;
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].track_id, "t2");
        assert!(players[0].paused);
        hub.heartbeat("u1", "alice", beat(Some("t2"), true));
        assert!(hub.live(now_ms(), &[]).web_players.is_empty());

        hub.heartbeat("u1", "alice", beat(Some("t3"), false));
        assert!(hub.sweep(now_ms() + WEB_PLAYER_TTL_MS + 1, &[]));
        assert!(hub.live(now_ms(), &[]).web_players.is_empty());
    }

    #[test]
    fn plays_and_long_streams_are_queued_for_history() {
        let hub = ActivityHub::default();
        hub.record_play(PlaySource::Subsonic, "u1", "alice", "Amperfy", "t1");
        let mut dlna = meta(Source::Dlna, TransferKind::Stream, "", "Bedroom TV", "t2");
        dlna.user_id = None;
        dlna.username = None;
        let guard = hub.open(dlna, None);
        let started = guard.entry.started_ms;
        drop(guard);
        {
            let map = lock(&hub.0.transfers);
            let e = map.values().next().unwrap();
            e.last_ms
                .store(started + MIN_HISTORY_STREAM_MS + 5, Ordering::Relaxed);
        }
        hub.sweep(started + MIN_HISTORY_STREAM_MS + LINGER_MS + 10, &[]);
        let events = hub.take_events();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["play", "stream"]);
        assert_eq!(events[1].client.as_deref(), Some("Bedroom TV"));
        assert_eq!(events[0].source.as_deref(), Some("subsonic"));
    }

    #[test]
    fn reset_clears_everything() {
        let hub = ActivityHub::default();
        let _guard = hub.open(
            meta(Source::Radio, TransferKind::Stream, "u1", "", "st-1"),
            None,
        );
        hub.record_play(PlaySource::Web, "u1", "alice", "", "t1");
        hub.reset();
        assert_eq!(entry_count(&hub), 0);
        assert!(hub.take_events().is_empty());
    }

    fn db_part() -> DbPart {
        let mut items = HashMap::new();
        items.insert(
            "t1".to_owned(),
            ItemSummary {
                title: "Opening".into(),
                artist: Some("The Band".into()),
                album_id: Some("al1".into()),
                duration: Some(200.0),
            },
        );
        DbPart {
            items,
            mix_jobs: Vec::new(),
            server_jobs: ServerJobs::default(),
            recent: Vec::new(),
            transcode_limit: Some(3),
        }
    }

    #[test]
    fn estimated_rows_expire_after_the_track_and_web_heartbeats_win() {
        let now = now_ms();
        let mut stale = np(PlaySource::Subsonic, "u2", "Amperfy", "t1");
        stale.at = Instant::now() - Duration::from_secs(300);
        let fresh = np(PlaySource::Subsonic, "u1", "Amperfy", "t1");
        let web_notice = np(PlaySource::Web, "u3", crate::plays::WEB_CLIENT, "t1");
        let live = Live {
            transfers: Vec::new(),
            sessions: Vec::new(),
            web_players: vec![WebPlayer {
                user_id: "u3".into(),
                username: "carol".into(),
                track_id: "t1".into(),
                position_sec: 50.0,
                duration_sec: None,
                paused: false,
                sampled_ms: now,
            }],
        };
        let snap = assemble(
            now,
            vec![stale, fresh, web_notice],
            live,
            Vec::new(),
            db_part(),
        );
        assert_eq!(snap.playing.len(), 2);
        let web = snap
            .playing
            .iter()
            .find(|p| p.source == Source::Web)
            .unwrap();
        assert!(!web.estimated);
        assert_eq!(web.position_sec, Some(50.0));
        assert_eq!(web.duration_sec, Some(200.0));
        let app = snap
            .playing
            .iter()
            .find(|p| p.source == Source::Subsonic)
            .unwrap();
        assert!(app.estimated);
        assert_eq!(app.user.as_deref(), Some("u1"));
        assert_eq!(app.client.as_deref(), Some("Amperfy"));
        assert_eq!(app.title.as_deref(), Some("Opening"));
        assert_eq!(snap.totals.playing, 2);
        assert_eq!(snap.totals.active_users, 2);
    }

    #[test]
    fn unreported_streams_show_and_single_prefetches_are_not_syncs() {
        let now = now_ms();
        let transfer = |m: TransferMeta, is_download: bool| LiveTransfer {
            meta: m,
            started_ms: now,
            total_bytes: None,
            bytes: 100,
            open: true,
            is_download,
        };
        let mut dlna = meta(Source::Dlna, TransferKind::Stream, "", "Bedroom TV", "t1");
        dlna.user_id = None;
        dlna.username = None;
        let mut transcoded = meta(Source::Web, TransferKind::Stream, "u1", "", "t1");
        transcoded.delivery = Some(DeliveryInfo {
            transcoded: true,
            source_format: Some("FLAC".into()),
            target_format: Some("MP3".into()),
            kbps: Some(192),
        });
        let live = Live {
            transfers: vec![
                transfer(dlna, false),
                transfer(transcoded, false),
                transfer(
                    meta(
                        Source::Subsonic,
                        TransferKind::Stream,
                        "u2",
                        "Amperfy",
                        "t9",
                    ),
                    true,
                ),
            ],
            sessions: Vec::new(),
            web_players: Vec::new(),
        };
        let snap = assemble(now, Vec::new(), live, Vec::new(), db_part());
        let sources: Vec<Source> = snap.playing.iter().map(|p| p.source).collect();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&Source::Dlna) && sources.contains(&Source::Web));
        assert!(snap.playing.iter().all(|p| p.status == "streaming"));
        assert!(
            snap.downloads.is_empty(),
            "one prefetched stream is not a sync"
        );
        assert_eq!(snap.totals.transcodes, 1);
    }

    #[test]
    fn sqlite_times_convert_to_ms() {
        assert_eq!(sqlite_time_ms("1970-01-01 00:00:10"), 10_000);
        assert_eq!(sqlite_time_ms("garbage"), 0);
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let snap = assemble(
            now_ms(),
            Vec::new(),
            Live {
                transfers: Vec::new(),
                sessions: Vec::new(),
                web_players: Vec::new(),
            },
            Vec::new(),
            db_part(),
        );
        let v = serde_json::to_value(&snap).unwrap();
        assert!(v.get("generatedAtMs").is_some());
        assert!(v["totals"].get("subsonicTranscodeLimit").is_some());
        assert!(v["serverJobs"].get("postScanQueued").is_some());
        assert!(v.get("mixJobs").is_some());
    }
}

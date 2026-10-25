//! One actor per renderer (`wip/play-on-renderers-plan.md` §5): owns the server-side queue, drives the device,
//! polls its state, keeps gapless prefetch (`SetNext`) in sync, records plays, and enforces takeover protection.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;

use super::link::RendererLink;
use super::media::{PersistedToken, PrepareError, PreparedItem};
use super::queue::{EnqueueMode, Queue, QueueEntry, QueueItem, QueueSnapshot, Repeat};
use super::RemoteEvent;
use crate::upnp::soap::TransportState;
use crate::upnp::UpnpError;

/// A session is "held" by its owner while active and touched within this window (§5.5).
pub const HOLD_WINDOW: Duration = Duration::from_secs(30 * 60);
/// Sessions idle this long are torn down.
pub const IDLE_TEARDOWN: Duration = Duration::from_secs(6 * 60 * 60);
/// Poll interval while paused/stopped/lost.
pub const SLOW_POLL: Duration = Duration::from_secs(5);
/// Stop polling after this long stopped/idle (until the next command).
pub const POLL_SLEEP_AFTER: Duration = Duration::from_secs(30 * 60);
/// Position ticks are ignored this long after a local seek/load (devices report stale positions).
pub const SEEK_SETTLE: Duration = Duration::from_millis(1500);
/// `STOPPED` right after a load is the device still settling, not a natural end.
pub const LOAD_GRACE: Duration = Duration::from_secs(3);
/// While playing, the saved position is refreshed this often (the full snapshot is saved on every change).
pub const POSITION_SAVE_EVERY: Duration = Duration::from_secs(15);
/// Volume/mute are re-read at most this often.
pub const VOLUME_POLL: Duration = Duration::from_secs(5);
/// Start position error that triggers a corrective seek after a load.
pub const START_TOLERANCE: f64 = 2.0;
/// Max items in one load/enqueue.
pub const MAX_ITEMS: usize = 5000;

/// Session status shown to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    /// Nothing loaded yet.
    Idle,
    /// Loading/buffering.
    Loading,
    /// Playing.
    Playing,
    /// Paused.
    Paused,
    /// Stopped (end of queue or stopped).
    Stopped,
    /// Another app took over the device.
    LostControl,
    /// The device stopped answering.
    Offline,
}

impl SessionStatus {
    /// Wire name (also stored in `remote_sessions.status`).
    pub fn name(self) -> &'static str {
        match self {
            SessionStatus::Idle => "idle",
            SessionStatus::Loading => "loading",
            SessionStatus::Playing => "playing",
            SessionStatus::Paused => "paused",
            SessionStatus::Stopped => "stopped",
            SessionStatus::LostControl => "lostControl",
            SessionStatus::Offline => "offline",
        }
    }

    /// Statuses during which another user must confirm a takeover.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            SessionStatus::Loading
                | SessionStatus::Playing
                | SessionStatus::Paused
                | SessionStatus::LostControl
        )
    }
}

/// Client-facing session snapshot.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    /// Renderer UDN.
    pub udn: String,
    /// Renderer display name.
    pub renderer_name: String,
    /// Status.
    pub status: SessionStatus,
    /// Queue in play order.
    pub queue: Vec<QueueEntry>,
    /// Current entry.
    pub current_qid: Option<u64>,
    /// Position (seconds) at `sampled_at_ms`.
    pub position_sec: f64,
    /// Current item duration.
    pub duration_sec: Option<f64>,
    /// Wall-clock time of the position sample (ms since epoch) for client interpolation.
    pub sampled_at_ms: u64,
    /// Device volume 0–100.
    pub volume: Option<u8>,
    /// Device mute.
    pub muted: Option<bool>,
    /// Shuffle.
    pub shuffle: bool,
    /// Repeat.
    pub repeat: Repeat,
    /// Opaque client queue source (playlist/album/radio...).
    pub queue_source: Option<serde_json::Value>,
    /// User currently holding the session.
    pub owner_user_id: Option<String>,
    /// Their name.
    pub owner_name: Option<String>,
    /// Last user who sent a command.
    pub last_controller_user_id: Option<String>,
    /// Seeking supported.
    pub can_seek: bool,
    /// Volume control supported.
    pub can_set_volume: bool,
    /// Monotonic change counter.
    pub revision: u64,
    /// Last error/notice, if any.
    pub error: Option<String>,
    /// Last owner activity (not serialized).
    #[serde(skip)]
    pub last_activity: Option<Instant>,
}

impl SessionState {
    /// Fresh idle state.
    pub fn new(udn: &str, renderer_name: &str, can_seek: bool, can_set_volume: bool) -> Self {
        Self {
            udn: udn.into(),
            renderer_name: renderer_name.into(),
            status: SessionStatus::Idle,
            queue: Vec::new(),
            current_qid: None,
            position_sec: 0.0,
            duration_sec: None,
            sampled_at_ms: now_ms(),
            volume: None,
            muted: None,
            shuffle: false,
            repeat: Repeat::Off,
            queue_source: None,
            owner_user_id: None,
            owner_name: None,
            last_controller_user_id: None,
            can_seek,
            can_set_volume,
            // Seeded from the clock so a later session on the same renderer never reuses a lower revision.
            revision: now_ms(),
            error: None,
            last_activity: None,
        }
    }

    /// Title of the current entry.
    pub fn now_playing_title(&self) -> Option<String> {
        let qid = self.current_qid?;
        self.queue
            .iter()
            .find(|e| e.qid == qid)
            .map(|e| e.item.title().to_string())
    }
}

/// True when `requester` must confirm before acting on this session (§5.5).
pub fn held_by_other(state: &SessionState, requester: &str, now: Instant) -> bool {
    let Some(owner) = state.owner_user_id.as_deref() else {
        return false;
    };
    owner != requester
        && state.status.is_active()
        && state
            .last_activity
            .is_some_and(|t| now.saturating_duration_since(t) < HOLD_WINDOW)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Who is sending a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controller {
    /// User id.
    pub user_id: String,
    /// Username (shown to others).
    pub username: String,
}

/// Session commands (§5.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Replace the queue and start playing.
    Load {
        /// Items in the order given.
        items: Vec<QueueItem>,
        /// Index into `items` to start at.
        start_index: usize,
        /// Start position in seconds.
        start_at: f64,
        /// Opaque client queue source.
        queue_source: Option<serde_json::Value>,
        /// Shuffle (unchanged when `None`).
        shuffle: Option<bool>,
        /// Repeat (unchanged when `None`).
        repeat: Option<Repeat>,
        /// The start item was already playing elsewhere (handoff/transfer), so its play is not recorded again.
        resumed: bool,
    },
    /// Resume/start.
    Play,
    /// Pause.
    Pause,
    /// Play/pause toggle.
    Toggle,
    /// Stop.
    Stop,
    /// Next item.
    Next,
    /// Previous item (or restart when > 3 s in).
    Prev,
    /// Seek to seconds.
    Seek(f64),
    /// Play a specific entry.
    Jump(u64),
    /// Add items.
    Enqueue {
        /// Items.
        items: Vec<QueueItem>,
        /// Where.
        mode: EnqueueMode,
    },
    /// Remove an entry.
    Remove(u64),
    /// Move an entry.
    Move {
        /// Entry.
        qid: u64,
        /// Target play-order index.
        to: usize,
    },
    /// Empty the queue and stop.
    Clear,
    /// Shuffle on/off.
    SetShuffle(bool),
    /// Repeat mode.
    SetRepeat(Repeat),
    /// Device volume.
    SetVolume(u8),
    /// Device mute.
    SetMute(bool),
    /// End the session (optionally stopping the device).
    Release {
        /// Stop the device first.
        stop: bool,
    },
}

/// Details for the takeover prompt (409 `session_in_use`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InUseInfo {
    /// Renderer UDN.
    pub udn: String,
    /// Renderer name.
    pub device_name: String,
    /// Holder's user id.
    pub owner_user_id: String,
    /// Holder's name.
    pub owner_name: Option<String>,
    /// Session status.
    pub status: SessionStatus,
    /// Current title.
    pub now_playing: Option<String>,
}

/// Command failures.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    /// Held by another user; resend with `takeover`.
    InUse(Box<InUseInfo>),
    /// Invalid request.
    BadRequest(String),
    /// The renderer can't do this (no seek / no volume).
    Unsupported(&'static str),
    /// The renderer refused or is unreachable.
    Renderer(String),
    /// The session has ended.
    Ended,
}

/// Message to the actor.
#[derive(Debug)]
pub struct Request {
    /// Command.
    pub cmd: Command,
    /// Sender.
    pub by: Controller,
    /// Confirmed takeover.
    pub takeover: bool,
    /// Reply channel.
    pub reply: oneshot::Sender<Result<SessionState, SessionError>>,
}

/// A session saved to `remote_sessions` so it survives a server restart. The speaker's current and next stream
/// tokens are saved with it, so a renderer that is still playing keeps working when the server comes back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSession {
    /// Format version.
    pub v: u32,
    /// Queue, in play order.
    pub queue: QueueSnapshot,
    /// Opaque client queue source.
    pub queue_source: Option<serde_json::Value>,
    /// Holder.
    pub owner_user_id: Option<String>,
    /// Holder's name.
    pub owner_name: Option<String>,
    /// Last controller (plays are attributed to them).
    pub last_controller_user_id: Option<String>,
    /// Status when saved.
    pub status: SessionStatus,
    /// Position (s) when saved.
    pub position_sec: f64,
    /// Current item duration.
    pub duration_sec: Option<f64>,
    /// Entry whose play was already recorded.
    pub played_qid: Option<u64>,
    /// URL the renderer is playing.
    pub current_url: Option<String>,
    /// URL prefetched with `SetNext`.
    pub next_url: Option<String>,
    /// Entry behind `next_url`.
    pub next_qid: Option<u64>,
    /// Volume control works on this device.
    pub can_set_volume: bool,
    /// Tokens behind `current_url`/`next_url` (filled in by the manager when saving).
    #[serde(default)]
    pub tokens: Vec<PersistedToken>,
}

/// The media token in one of our renderer URLs (`…/media/r/<token>`).
pub fn token_in_url(url: &str) -> Option<&str> {
    url.rsplit_once("/media/r/").map(|(_, token)| token)
}

/// Item preparation and play recording, abstracted so tests can run without a database.
#[derive(Clone)]
pub(crate) enum SessionEnv {
    /// Production: prepare through the manager (tokens, DB), record plays in the DB.
    Real {
        /// Manager.
        manager: super::RemoteManager,
        /// Database.
        db: crate::DbPool,
    },
    /// Tests: scripted preparation and an in-memory play log.
    #[cfg(test)]
    Test(Arc<tests::TestEnv>),
}

impl SessionEnv {
    async fn prepare(
        &self,
        udn: &str,
        item: &QueueItem,
        owner: &str,
    ) -> Result<PreparedItem, PrepareError> {
        match self {
            SessionEnv::Real { manager, db } => match item {
                QueueItem::Track { id, .. } => manager.prepare_track(db, udn, id).await,
                QueueItem::Mix { id, .. } => manager.prepare_mix(db, udn, id, owner).await,
            },
            #[cfg(test)]
            SessionEnv::Test(t) => t.prepare(item),
        }
    }

    /// Counts a play that just started and lists it as now playing (player name = the speaker).
    async fn record_play(&self, user_id: &str, username: &str, speaker: &str, item: &QueueItem) {
        let QueueItem::Track { id, .. } = item else {
            return; // BoogieMix outputs are not library tracks.
        };
        match self {
            SessionEnv::Real { manager, db } => {
                let db = db.clone();
                let (user, track) = (user_id.to_string(), id.clone());
                let counted = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    let counted = boogiebox_db::playback::increment_track_play_count(&conn, &track)
                        .unwrap_or(false);
                    if counted {
                        let _ = boogiebox_db::playback::insert_play_history(&conn, &user, &track);
                    }
                    counted
                })
                .await
                .unwrap_or(false);
                if let (true, Some(env)) = (counted, manager.play_env()) {
                    crate::plays::play_started(
                        &env,
                        crate::plays::PlaySource::PlayOn,
                        user_id,
                        username,
                        id,
                        speaker,
                    );
                }
            }
            #[cfg(test)]
            SessionEnv::Test(t) => t.plays.lock().unwrap().push((user_id.into(), id.clone())),
        }
    }

    /// The current track played long enough to count: queue it for the user's scrobbling services.
    async fn record_scrobble(&self, user_id: &str, item: &QueueItem, started_at: i64) {
        let QueueItem::Track { id, .. } = item else {
            return;
        };
        match self {
            SessionEnv::Real { manager, .. } => {
                if let Some(env) = manager.play_env() {
                    crate::plays::play_finished(&env, user_id, id, started_at).await;
                }
            }
            #[cfg(test)]
            SessionEnv::Test(t) => t
                .scrobbles
                .lock()
                .unwrap()
                .push((user_id.into(), id.clone())),
        }
    }

    /// Saves the full session (queue, owner, playback state and the renderer's stream tokens).
    async fn persist(&self, udn: &str, mut session: PersistedSession) {
        match self {
            SessionEnv::Real { manager, db } => {
                session.tokens = [&session.current_url, &session.next_url]
                    .into_iter()
                    .flatten()
                    .filter_map(|url| token_in_url(url))
                    .filter_map(|token| manager.media().export(token))
                    .collect();
                let Ok(snapshot) = serde_json::to_string(&session) else {
                    return;
                };
                let row = boogiebox_db::remote::RemoteSessionRow {
                    udn: udn.to_string(),
                    snapshot,
                    position_sec: session.position_sec,
                    status: session.status.name().to_string(),
                    saved_at_ms: now_ms() as i64,
                };
                let db = db.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    if let Err(e) = boogiebox_db::remote::save_remote_session(&conn, &row) {
                        tracing::warn!(target: "remote", udn = %row.udn, "Could not save Play On session: {e}");
                    }
                })
                .await;
            }
            #[cfg(test)]
            SessionEnv::Test(t) => t.saved.lock().unwrap().push((udn.into(), session)),
        }
    }

    /// Refreshes only the saved position/status.
    async fn persist_position(&self, udn: &str, position_sec: f64, status: SessionStatus) {
        match self {
            SessionEnv::Real { db, .. } => {
                let (db, udn) = (db.clone(), udn.to_string());
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    let _ = boogiebox_db::remote::update_remote_session_position(
                        &conn,
                        &udn,
                        position_sec,
                        status.name(),
                        now_ms() as i64,
                    );
                })
                .await;
            }
            #[cfg(test)]
            SessionEnv::Test(t) => {
                t.positions
                    .lock()
                    .unwrap()
                    .push((udn.into(), position_sec, status))
            }
        }
    }

    /// Deletes the saved session (it really ended).
    async fn forget(&self, udn: &str) {
        match self {
            SessionEnv::Real { db, .. } => {
                let (db, udn) = (db.clone(), udn.to_string());
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    let _ = boogiebox_db::remote::delete_remote_session(&conn, &udn);
                })
                .await;
            }
            #[cfg(test)]
            SessionEnv::Test(t) => t.forgotten.lock().unwrap().push(udn.into()),
        }
    }
}

/// Static configuration for a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Renderer UDN.
    pub udn: String,
    /// Renderer name.
    pub name: String,
    /// Poll interval while playing.
    pub poll: Duration,
    /// `SetNextAVTransportURI` supported.
    pub set_next: bool,
    /// `Seek` supported.
    pub seek: bool,
    /// Shuffle seed.
    pub seed: u64,
}

/// Handle kept by the manager.
#[derive(Clone, Debug)]
pub struct SessionHandle {
    /// Unique per spawned actor (identifies it in the end callback).
    pub id: u64,
    /// Command channel. The actor ends once every handle is dropped.
    pub tx: mpsc::Sender<Request>,
    /// Latest snapshot.
    pub shared: Arc<RwLock<SessionState>>,
}

impl SessionHandle {
    /// Latest snapshot.
    pub fn snapshot(&self) -> SessionState {
        self.shared
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Sends a command and waits for the result.
    pub async fn send(
        &self,
        cmd: Command,
        by: Controller,
        takeover: bool,
    ) -> Result<SessionState, SessionError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Request {
                cmd,
                by,
                takeover,
                reply,
            })
            .await
            .map_err(|_| SessionError::Ended)?;
        rx.await.map_err(|_| SessionError::Ended)?
    }
}

/// Callback run once when a session actor exits (manager cleanup); receives the session's [`SessionHandle::id`].
pub type OnEnd = Box<dyn FnOnce(u64) + Send>;

static NEXT_SESSION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Spawns a session actor, optionally continuing a saved session. `on_end` runs once when it exits.
pub(crate) fn spawn_session(
    cfg: SessionConfig,
    link: RendererLink,
    env: SessionEnv,
    events: broadcast::Sender<RemoteEvent>,
    on_end: OnEnd,
    restore: Option<PersistedSession>,
) -> SessionHandle {
    let (tx, rx) = mpsc::channel(32);
    let state = SessionState::new(&cfg.udn, &cfg.name, cfg.seek, link.has_volume());
    let shared = Arc::new(RwLock::new(state.clone()));
    let mut actor = Actor {
        queue: Queue::with_seed(cfg.seed),
        cfg,
        link,
        env,
        events,
        shared: shared.clone(),
        state,
        current_url: None,
        next_url: None,
        next_qid: None,
        pending_start: None,
        resync_next: false,
        ignore_ticks_until: None,
        load_grace_until: None,
        played_qid: None,
        scrobble_armed: None,
        foreign_polls: 0,
        failures: 0,
        last_rel: None,
        last_volume_poll: None,
        stopped_since: None,
        created: Instant::now(),
        saved: false,
        dirty: false,
        last_position_saved: None,
        restoring: false,
        resume_at: None,
    };
    if let Some(saved) = restore {
        actor.restore(saved);
    }
    // Hold no sender here: the actor must see its channel close when the manager drops the handles.
    let id = NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(async move {
        actor.run(rx).await;
        on_end(id);
    });
    SessionHandle { id, tx, shared }
}

struct Actor {
    cfg: SessionConfig,
    link: RendererLink,
    env: SessionEnv,
    events: broadcast::Sender<RemoteEvent>,
    shared: Arc<RwLock<SessionState>>,
    state: SessionState,
    queue: Queue,
    current_url: Option<String>,
    next_url: Option<String>,
    next_qid: Option<u64>,
    pending_start: Option<f64>,
    resync_next: bool,
    ignore_ticks_until: Option<Instant>,
    load_grace_until: Option<Instant>,
    played_qid: Option<u64>,
    /// Entry seen playing below its scrobble threshold; it scrobbles when the position passes it.
    scrobble_armed: Option<u64>,
    foreign_polls: u32,
    failures: u32,
    last_rel: Option<f64>,
    last_volume_poll: Option<Instant>,
    stopped_since: Option<Instant>,
    created: Instant,
    /// A saved row exists for this session.
    saved: bool,
    /// A structural change has not been saved yet.
    dirty: bool,
    last_position_saved: Option<Instant>,
    /// Continuing a saved session: the first poll decides whether the device still plays our stream.
    restoring: bool,
    /// Where a restored session resumes once the device lost our stream.
    resume_at: Option<f64>,
}

/// Why the actor stopped.
enum Exit {
    /// The manager dropped the session (shutdown, Play On restarted): keep the saved row.
    Dropped,
    /// Released or idle: the session is over.
    Ended,
}

fn is_fault(e: &UpnpError, code: u32) -> bool {
    e.fault_code() == Some(code)
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::Receiver<Request>) {
        self.refresh_volume(true).await;
        self.publish(true);
        let exit = loop {
            let delay = self.poll_delay();
            // Not polling: still wake up when the idle-teardown deadline passes.
            let sleep_for = delay.unwrap_or_else(|| self.idle_remaining());
            tokio::select! {
                req = rx.recv() => {
                    let Some(req) = req else { break Exit::Dropped };
                    if self.handle(req).await {
                        break Exit::Ended;
                    }
                }
                _ = tokio::time::sleep(sleep_for) => {
                    if delay.is_some() {
                        self.poll().await;
                    }
                }
            }
            self.save().await;
            if self.idle_expired() {
                tracing::info!(target: "remote", udn = %self.cfg.udn, "Play On session idle; ending");
                break Exit::Ended;
            }
        };
        match exit {
            // Keep the latest position for the restore.
            Exit::Dropped if self.saved => {
                self.env
                    .persist_position(&self.cfg.udn, self.state.position_sec, self.state.status)
                    .await;
            }
            Exit::Dropped => {}
            Exit::Ended => self.env.forget(&self.cfg.udn).await,
        }
        let _ = self.events.send(RemoteEvent::SessionEnded {
            udn: self.cfg.udn.clone(),
        });
    }

    /// The saveable form of this session.
    fn persisted(&self) -> PersistedSession {
        PersistedSession {
            v: 1,
            queue: self.queue.snapshot(),
            queue_source: self.state.queue_source.clone(),
            owner_user_id: self.state.owner_user_id.clone(),
            owner_name: self.state.owner_name.clone(),
            last_controller_user_id: self.state.last_controller_user_id.clone(),
            status: self.state.status,
            position_sec: self.state.position_sec,
            duration_sec: self.state.duration_sec,
            played_qid: self.played_qid,
            current_url: self.current_url.clone(),
            next_url: self.next_url.clone(),
            next_qid: self.next_qid,
            can_set_volume: self.state.can_set_volume,
            tokens: Vec::new(),
        }
    }

    /// Saves after a structural change; refreshes the position every [`POSITION_SAVE_EVERY`] while playing.
    async fn save(&mut self) {
        if self.queue.is_empty() {
            if self.saved {
                self.env.forget(&self.cfg.udn).await;
                self.saved = false;
            }
            self.dirty = false;
            return;
        }
        let now = Instant::now();
        if self.dirty {
            self.dirty = false;
            self.last_position_saved = Some(now);
            self.env.persist(&self.cfg.udn, self.persisted()).await;
            self.saved = true;
        } else if self.saved
            && matches!(
                self.state.status,
                SessionStatus::Playing | SessionStatus::Loading
            )
            && self
                .last_position_saved
                .is_none_or(|t| now.duration_since(t) >= POSITION_SAVE_EVERY)
        {
            self.last_position_saved = Some(now);
            self.env
                .persist_position(&self.cfg.udn, self.state.position_sec, self.state.status)
                .await;
        }
    }

    /// Continues a saved session. The device is checked on the first poll.
    fn restore(&mut self, saved: PersistedSession) {
        self.queue = Queue::from_snapshot(saved.queue, self.cfg.seed);
        self.state.queue_source = saved.queue_source;
        self.state.owner_user_id = saved.owner_user_id;
        self.state.owner_name = saved.owner_name;
        self.state.last_controller_user_id = saved.last_controller_user_id;
        self.state.can_set_volume = saved.can_set_volume && self.link.has_volume();
        self.state.duration_sec = saved.duration_sec;
        self.set_position(saved.position_sec);
        self.state.last_activity = Some(Instant::now());
        self.current_url = saved.current_url;
        self.next_url = saved.next_url;
        self.next_qid = saved.next_qid;
        self.played_qid = saved.played_qid;
        self.resync_next = true;
        self.restoring = true;
        self.saved = true;
        let status = match saved.status {
            _ if self.queue.is_empty() => SessionStatus::Idle,
            SessionStatus::Loading | SessionStatus::Offline => SessionStatus::Playing,
            s => s,
        };
        self.set_status(status);
        self.sync_snapshot();
    }

    fn poll_delay(&self) -> Option<Duration> {
        match self.state.status {
            SessionStatus::Idle => None,
            SessionStatus::Playing | SessionStatus::Loading => Some(self.cfg.poll),
            SessionStatus::Offline => Some(match self.failures {
                0..=3 => Duration::from_secs(5),
                4..=5 => Duration::from_secs(15),
                _ => Duration::from_secs(60),
            }),
            SessionStatus::Stopped
                if self
                    .stopped_since
                    .is_some_and(|t| t.elapsed() >= POLL_SLEEP_AFTER) =>
            {
                None
            }
            SessionStatus::Paused | SessionStatus::Stopped | SessionStatus::LostControl => {
                Some(SLOW_POLL)
            }
        }
    }

    fn idle_expired(&self) -> bool {
        let since = self.state.last_activity.unwrap_or(self.created);
        !self.state.status.is_active() && since.elapsed() >= IDLE_TEARDOWN
    }

    fn idle_remaining(&self) -> Duration {
        let since = self.state.last_activity.unwrap_or(self.created);
        IDLE_TEARDOWN
            .saturating_sub(since.elapsed())
            .max(Duration::from_secs(1))
    }

    fn set_status(&mut self, status: SessionStatus) {
        if status == SessionStatus::Stopped && self.state.status != SessionStatus::Stopped {
            self.stopped_since = Some(Instant::now());
        } else if status != SessionStatus::Stopped {
            self.stopped_since = None;
        }
        self.state.status = status;
    }

    /// Copies queue state into the snapshot and the shared handle copy.
    fn sync_snapshot(&mut self) {
        self.state.queue = self.queue.entries().to_vec();
        self.state.current_qid = self.queue.current().map(|e| e.qid);
        self.state.shuffle = self.queue.shuffle();
        self.state.repeat = self.queue.repeat();
        *self.shared.write().unwrap_or_else(|p| p.into_inner()) = self.state.clone();
    }

    /// Copies queue state into the snapshot and notifies clients. `full` = structural change.
    fn publish(&mut self, full: bool) {
        if full {
            self.state.revision += 1;
            self.dirty = true;
        }
        self.sync_snapshot();
        let event = if full {
            RemoteEvent::Session {
                state: Box::new(self.state.clone()),
            }
        } else {
            RemoteEvent::Position {
                udn: self.cfg.udn.clone(),
                status: self.state.status,
                position_sec: self.state.position_sec,
                duration_sec: self.state.duration_sec,
                sampled_at_ms: self.state.sampled_at_ms,
            }
        };
        let _ = self.events.send(event);
    }

    fn notice(&self, level: &'static str, message: String) {
        let _ = self.events.send(RemoteEvent::Notice {
            udn: self.cfg.udn.clone(),
            level,
            message,
        });
    }

    fn set_position(&mut self, sec: f64) {
        self.state.position_sec = sec.max(0.0);
        self.state.sampled_at_ms = now_ms();
    }

    // ── commands ─────────────────────────────────────────────────────────────

    /// Returns true when the session should end.
    async fn handle(&mut self, req: Request) -> bool {
        let Request {
            cmd,
            by,
            takeover,
            reply,
        } = req;
        let now = Instant::now();
        if held_by_other(&self.state, &by.user_id, now) {
            if !takeover {
                let info = InUseInfo {
                    udn: self.cfg.udn.clone(),
                    device_name: self.cfg.name.clone(),
                    owner_user_id: self.state.owner_user_id.clone().unwrap_or_default(),
                    owner_name: self.state.owner_name.clone(),
                    status: self.state.status,
                    now_playing: self.state.now_playing_title(),
                };
                let _ = reply.send(Err(SessionError::InUse(Box::new(info))));
                return false;
            }
            let _ = self.events.send(RemoteEvent::Takeover {
                udn: self.cfg.udn.clone(),
                from_user_id: self.state.owner_user_id.clone().unwrap_or_default(),
                by_user_id: by.user_id.clone(),
                by_user_name: by.username.clone(),
            });
            tracing::info!(
                target: "remote",
                udn = %self.cfg.udn,
                from = %self.state.owner_user_id.as_deref().unwrap_or(""),
                to = %by.user_id,
                "Play On session taken over"
            );
        }
        self.state.owner_user_id = Some(by.user_id.clone());
        self.state.owner_name = Some(by.username.clone());
        self.state.last_controller_user_id = Some(by.user_id.clone());
        self.state.last_activity = Some(now);

        if let Command::Release { stop } = cmd {
            if stop {
                let _ = self.link.stop().await;
            }
            self.set_status(SessionStatus::Stopped);
            self.publish(true);
            let _ = reply.send(Ok(self.state.clone()));
            return true;
        }

        let result = self.execute(cmd).await;
        self.state.error = result.as_ref().err().and_then(|e| match e {
            SessionError::Renderer(m) => Some(m.clone()),
            _ => None,
        });
        self.publish(true);
        let _ = reply.send(result.map(|_| self.state.clone()));
        false
    }

    async fn execute(&mut self, cmd: Command) -> Result<(), SessionError> {
        match cmd {
            Command::Load {
                items,
                start_index,
                start_at,
                queue_source,
                shuffle,
                repeat,
                resumed,
            } => {
                if items.is_empty() || items.len() > MAX_ITEMS {
                    return Err(SessionError::BadRequest("1–5000 items required".into()));
                }
                if start_index >= items.len() {
                    return Err(SessionError::BadRequest("startIndex out of range".into()));
                }
                if let Some(r) = repeat {
                    self.queue.set_repeat(r);
                }
                let shuffle = shuffle.unwrap_or(self.queue.shuffle());
                self.queue.load(items, start_index, shuffle);
                self.state.queue_source = queue_source;
                let start_qid = self.queue.current().map(|e| e.qid);
                self.play_current(start_at.max(0.0)).await?;
                // Only when the start item itself is playing (an unplayable one may have been skipped).
                if resumed && self.queue.current().map(|e| e.qid) == start_qid {
                    self.played_qid = start_qid;
                }
                Ok(())
            }
            Command::Play => self.resume().await,
            Command::Pause => self.pause().await,
            Command::Toggle => {
                if matches!(
                    self.state.status,
                    SessionStatus::Playing | SessionStatus::Loading
                ) {
                    self.pause().await
                } else {
                    self.resume().await
                }
            }
            Command::Stop => {
                if let Err(e) = self.link.stop().await {
                    if !is_fault(&e, 701) {
                        return Err(SessionError::Renderer(e.to_string()));
                    }
                }
                self.set_status(SessionStatus::Stopped);
                self.set_position(0.0);
                Ok(())
            }
            Command::Next => {
                if self.queue.skip_next() {
                    self.play_current(0.0).await
                } else {
                    self.stop_at_end().await;
                    Ok(())
                }
            }
            Command::Prev => {
                if self.state.position_sec > 3.0 && self.current_url.is_some() {
                    self.seek(0.0).await
                } else if self.queue.skip_prev() {
                    self.play_current(0.0).await
                } else if self.current_url.is_some() {
                    self.seek(0.0).await
                } else {
                    Ok(())
                }
            }
            Command::Seek(sec) => {
                if !sec.is_finite() || sec < 0.0 {
                    return Err(SessionError::BadRequest("invalid position".into()));
                }
                if self.resume_at.is_some() {
                    // Waiting to reload after a restart: just move the resume point.
                    self.resume_at = Some(sec);
                    self.set_position(sec);
                    return Ok(());
                }
                self.seek(sec).await
            }
            Command::Jump(qid) => {
                if !self.queue.jump(qid) {
                    return Err(SessionError::BadRequest("unknown queue entry".into()));
                }
                self.play_current(0.0).await
            }
            Command::Enqueue { items, mode } => {
                if items.is_empty() || items.len() > MAX_ITEMS {
                    return Err(SessionError::BadRequest("1–5000 items required".into()));
                }
                self.queue.enqueue(items, mode);
                self.resync_upcoming().await;
                Ok(())
            }
            Command::Remove(qid) => match self.queue.remove(qid) {
                None => Err(SessionError::BadRequest("unknown queue entry".into())),
                Some(true) if self.playing_or_paused() => {
                    if self.queue.is_empty() {
                        self.stop_at_end().await;
                        Ok(())
                    } else {
                        self.play_current(0.0).await
                    }
                }
                Some(_) => {
                    self.resync_upcoming().await;
                    Ok(())
                }
            },
            Command::Move { qid, to } => {
                if !self.queue.move_to(qid, to) {
                    return Err(SessionError::BadRequest("unknown queue entry".into()));
                }
                self.resync_upcoming().await;
                Ok(())
            }
            Command::Clear => {
                self.queue.clear();
                let _ = self.link.stop().await;
                self.current_url = None;
                self.next_url = None;
                self.next_qid = None;
                self.state.duration_sec = None;
                self.set_position(0.0);
                self.set_status(SessionStatus::Stopped);
                Ok(())
            }
            Command::SetShuffle(on) => {
                self.queue.set_shuffle(on);
                self.resync_upcoming().await;
                Ok(())
            }
            Command::SetRepeat(r) => {
                self.queue.set_repeat(r);
                self.resync_upcoming().await;
                Ok(())
            }
            Command::SetVolume(v) => {
                if !self.link.has_volume() || !self.state.can_set_volume {
                    return Err(SessionError::Unsupported("volume"));
                }
                let v = v.min(100);
                self.link
                    .set_volume(v)
                    .await
                    .map_err(|e| SessionError::Renderer(e.to_string()))?;
                // Fixed-volume outputs (e.g. a WiiM feeding an amplifier) accept SetVolume and ignore it.
                if let Ok((actual, muted)) = self.link.volume().await {
                    self.state.muted = Some(muted);
                    if actual.abs_diff(v) > 2 {
                        self.state.volume = Some(actual);
                        self.state.can_set_volume = false;
                        self.notice(
                            "info",
                            format!("{}'s volume is fixed. Use your amplifier", self.cfg.name),
                        );
                        return Err(SessionError::Unsupported("volume (it is fixed)"));
                    }
                }
                self.state.volume = Some(v);
                Ok(())
            }
            Command::SetMute(m) => {
                if !self.link.has_volume() {
                    return Err(SessionError::Unsupported("volume"));
                }
                self.link
                    .set_mute(m)
                    .await
                    .map_err(|e| SessionError::Renderer(e.to_string()))?;
                self.state.muted = Some(m);
                Ok(())
            }
            Command::Release { .. } => Ok(()), // handled in `handle`
        }
    }

    /// Scrobbles the current entry once its position passes the threshold, but only after it was seen below it
    /// (so a resumed or restored session that starts past the threshold is not scrobbled twice).
    async fn check_scrobble(&mut self, user: &str, entry: &QueueEntry, rel: f64) {
        let Some(threshold) =
            crate::plays::scrobble_threshold(entry.item.duration().or(self.state.duration_sec))
        else {
            return;
        };
        if rel < threshold {
            self.scrobble_armed = Some(entry.qid);
        } else if self.scrobble_armed == Some(entry.qid) {
            self.scrobble_armed = None;
            let started_at = crate::plays::unix_now() - rel.max(0.0) as i64;
            self.env
                .record_scrobble(user, &entry.item, started_at)
                .await;
        }
    }

    fn playing_or_paused(&self) -> bool {
        matches!(
            self.state.status,
            SessionStatus::Playing | SessionStatus::Paused | SessionStatus::Loading
        )
    }

    async fn resume(&mut self) -> Result<(), SessionError> {
        if self.queue.is_empty() {
            return Err(SessionError::BadRequest("queue is empty".into()));
        }
        if let Some(at) = self.resume_at.take() {
            return self.continue_current_at(at).await;
        }
        match self.state.status {
            SessionStatus::LostControl => {
                let at = self.state.position_sec;
                self.continue_current_at(at).await
            }
            SessionStatus::Paused | SessionStatus::Stopped if self.current_url.is_some() => {
                self.link
                    .play()
                    .await
                    .map_err(|e| SessionError::Renderer(e.to_string()))?;
                self.load_grace_until = Some(Instant::now() + LOAD_GRACE);
                self.set_status(SessionStatus::Loading);
                Ok(())
            }
            SessionStatus::Playing | SessionStatus::Loading => Ok(()),
            _ => self.play_current(0.0).await,
        }
    }

    /// Reloads the current entry mid-track (after another app took the speaker, or the device lost our stream
    /// while the server was away). It was already counted, so its play is not recorded again.
    async fn continue_current_at(&mut self, at: f64) -> Result<(), SessionError> {
        let current = self.queue.current().map(|e| e.qid);
        let counted = self.played_qid.filter(|q| Some(*q) == current);
        self.play_current(at).await?;
        if counted.is_some() && self.queue.current().map(|e| e.qid) == current {
            self.played_qid = counted;
        }
        Ok(())
    }

    async fn pause(&mut self) -> Result<(), SessionError> {
        match self.link.pause().await {
            Ok(()) => {
                self.set_status(SessionStatus::Paused);
                Ok(())
            }
            Err(e) if is_fault(&e, 701) => Ok(()), // not playing: nothing to pause
            Err(e) => Err(SessionError::Renderer(e.to_string())),
        }
    }

    async fn seek(&mut self, sec: f64) -> Result<(), SessionError> {
        if !self.cfg.seek {
            return Err(SessionError::Unsupported("seek"));
        }
        if self.current_url.is_none() {
            return Err(SessionError::BadRequest("nothing loaded".into()));
        }
        let sec = match self.state.duration_sec {
            Some(d) => sec.min((d - 1.0).max(0.0)),
            None => sec,
        };
        self.link
            .seek(sec)
            .await
            .map_err(|e| SessionError::Renderer(e.to_string()))?;
        self.set_position(sec);
        self.ignore_ticks_until = Some(Instant::now() + SEEK_SETTLE);
        self.pending_start = None;
        // WiiM drops the prefetched next item on seek (Phase 0.3): re-issue once playing again.
        self.resync_next = true;
        Ok(())
    }

    async fn stop_at_end(&mut self) {
        let _ = self.link.stop().await;
        self.current_url = None;
        self.next_url = None;
        self.next_qid = None;
        self.set_status(SessionStatus::Stopped);
        self.set_position(0.0);
    }

    /// Loads and plays the current entry, skipping entries that can't be prepared or that the device refuses.
    async fn play_current(&mut self, start_at: f64) -> Result<(), SessionError> {
        self.resume_at = None;
        let mut attempts = self.queue.entries().len().max(1);
        let owner = self.state.owner_user_id.clone().unwrap_or_default();
        while attempts > 0 {
            attempts -= 1;
            let Some(entry) = self.queue.current().cloned() else {
                self.stop_at_end().await;
                return Ok(());
            };
            let prepared = match self.env.prepare(&self.cfg.udn, &entry.item, &owner).await {
                Ok(p) => p,
                Err(PrepareError::NotFound | PrepareError::FileMissing) => {
                    self.skip_unplayable(&entry, "file is missing").await;
                    if self.current_url.is_none() && self.queue.current().is_none() {
                        return Ok(());
                    }
                    continue;
                }
                Err(e) => return Err(SessionError::Renderer(format!("{e:?}"))),
            };
            match self.link.set_uri(&prepared.url, &prepared.didl).await {
                Ok(()) => {}
                Err(e) if e.fault_code().is_some() => {
                    self.skip_unplayable(&entry, "the speaker can't play it")
                        .await;
                    continue;
                }
                Err(e) => return Err(SessionError::Renderer(e.to_string())),
            }
            self.link
                .play()
                .await
                .map_err(|e| SessionError::Renderer(e.to_string()))?;
            self.current_url = Some(prepared.url);
            self.next_url = None;
            self.next_qid = None;
            self.played_qid = None;
            self.pending_start = Some(start_at);
            self.resync_next = true;
            self.foreign_polls = 0;
            let now = Instant::now();
            self.load_grace_until = Some(now + LOAD_GRACE);
            self.ignore_ticks_until = Some(now + SEEK_SETTLE);
            self.state.duration_sec = prepared.duration.or(entry.item.duration());
            self.set_position(start_at);
            self.set_status(SessionStatus::Loading);
            return Ok(());
        }
        self.stop_at_end().await;
        Ok(())
    }

    async fn skip_unplayable(&mut self, entry: &QueueEntry, why: &str) {
        self.notice("warning", format!("Skipped {}: {why}", entry.item.title()));
        if !self.queue.skip_next() {
            self.stop_at_end().await;
        }
    }

    /// Re-issues (or clears) the gapless prefetch for whatever now plays next.
    async fn resync_upcoming(&mut self) {
        if self.current_url.is_some() && self.playing_or_paused() {
            self.set_next_for_upcoming().await;
        }
    }

    async fn set_next_for_upcoming(&mut self) {
        if !self.cfg.set_next {
            return;
        }
        let owner = self.state.owner_user_id.clone().unwrap_or_default();
        let next = self.queue.peek_next().cloned();
        match next {
            Some(entry) => match self.env.prepare(&self.cfg.udn, &entry.item, &owner).await {
                Ok(p) => {
                    if self.link.set_next(&p.url, &p.didl).await.is_ok() {
                        self.next_url = Some(p.url);
                        self.next_qid = Some(entry.qid);
                    }
                }
                Err(_) => self.clear_next().await,
            },
            None => self.clear_next().await,
        }
    }

    async fn clear_next(&mut self) {
        if self.next_url.take().is_some() {
            let _ = self.link.set_next("", "").await;
        }
        self.next_qid = None;
    }

    async fn refresh_volume(&mut self, force: bool) {
        if !self.link.has_volume() {
            return;
        }
        let due = force
            || self
                .last_volume_poll
                .is_none_or(|t| t.elapsed() >= VOLUME_POLL);
        if !due {
            return;
        }
        self.last_volume_poll = Some(Instant::now());
        if let Ok((v, m)) = self.link.volume().await {
            self.state.volume = Some(v);
            self.state.muted = Some(m);
        }
    }

    // ── polling ──────────────────────────────────────────────────────────────

    async fn poll(&mut self) {
        let before = (
            self.state.status,
            self.state.current_qid,
            self.state.volume,
            self.state.muted,
            self.state.duration_sec,
        );
        let info = match self.link.transport_info().await {
            Ok(ti) => self.link.position_info().await.map(|pi| (ti, pi)),
            Err(e) => Err(e),
        };
        let (ti, pi) = match info {
            Ok(v) => v,
            Err(e) => {
                self.failures += 1;
                if e.is_transport_failure()
                    && self.failures >= 3
                    && self.state.status != SessionStatus::Offline
                {
                    self.set_status(SessionStatus::Offline);
                    self.notice("error", format!("{} is unreachable", self.cfg.name));
                    self.publish(true);
                }
                return;
            }
        };
        if self.state.status == SessionStatus::Offline {
            self.notice("info", format!("{} is back", self.cfg.name));
            self.set_status(SessionStatus::Paused);
        }
        self.failures = 0;
        let now = Instant::now();

        if std::mem::take(&mut self.restoring)
            && matches!(ti.state, TransportState::Stopped | TransportState::NoMedia)
            && matches!(
                self.state.status,
                SessionStatus::Playing | SessionStatus::Paused | SessionStatus::LostControl
            )
        {
            // The device dropped our stream while the server was away: come back paused where it was. Its
            // (stale) URI stays "ours", so this isn't mistaken for another app; WiiM even reports
            // RelTime == duration once stopped, which must not read as a natural end.
            self.next_url = None;
            self.next_qid = None;
            self.resume_at = Some(self.state.position_sec);
            self.set_status(SessionStatus::Paused);
            self.finish_poll(before);
            return;
        }

        // Gapless transition: the device moved onto the URI we prefetched.
        if let (Some(next), Some(uri)) = (&self.next_url, &pi.track_uri) {
            if uri == next {
                if let Some(q) = self.next_qid {
                    self.queue.jump(q);
                }
                self.current_url = self.next_url.take();
                self.next_qid = None;
                self.played_qid = None;
                self.pending_start = None;
                self.resync_next = true;
                self.state.duration_sec = self.queue.current().and_then(|e| e.item.duration());
            }
        }

        let ours = match &pi.track_uri {
            None => true,
            Some(u) => self.current_url.as_deref() == Some(u.as_str()),
        };
        if !ours {
            self.foreign_polls += 1;
            if self.foreign_polls >= 2 && self.state.status != SessionStatus::LostControl {
                self.set_status(SessionStatus::LostControl);
                self.next_url = None;
                self.next_qid = None;
                self.notice(
                    "info",
                    format!("{} is playing another source", self.cfg.name),
                );
            }
            self.finish_poll(before);
            return;
        }
        self.foreign_polls = 0;
        if self.state.status == SessionStatus::LostControl {
            self.finish_poll(before);
            return;
        }

        let settling = self.ignore_ticks_until.is_some_and(|t| now < t);
        match ti.state {
            TransportState::Playing => {
                self.set_status(SessionStatus::Playing);
                self.state.last_activity = Some(now);
                if let (Some(start), Some(rel)) = (self.pending_start, pi.rel_time) {
                    if !settling {
                        self.pending_start = None;
                        if self.cfg.seek && (rel - start).abs() > START_TOLERANCE {
                            // WiiM can start a fresh load at an earlier seek target (Phase 0.3).
                            let _ = self.link.seek(start).await;
                            self.set_position(start);
                            self.ignore_ticks_until = Some(now + SEEK_SETTLE);
                            self.resync_next = true;
                        }
                    }
                }
                let current = self.queue.current().cloned();
                if let Some(entry) = current {
                    let user = self
                        .state
                        .last_controller_user_id
                        .clone()
                        .unwrap_or_default();
                    if self.played_qid != Some(entry.qid) {
                        self.played_qid = Some(entry.qid);
                        let username = self.state.owner_name.clone().unwrap_or_default();
                        self.env
                            .record_play(&user, &username, &self.cfg.name, &entry.item)
                            .await;
                    }
                    // Positions are stale while a start/seek settles (including a corrective seek just sent).
                    let settled = self.pending_start.is_none()
                        && self.ignore_ticks_until.is_none_or(|t| now >= t);
                    if let (Some(rel), true) = (pi.rel_time, settled) {
                        self.check_scrobble(&user, &entry, rel).await;
                    }
                }
                if self.resync_next && !settling {
                    self.resync_next = false;
                    self.set_next_for_upcoming().await;
                }
            }
            TransportState::Paused => self.set_status(SessionStatus::Paused),
            TransportState::Transitioning => self.set_status(SessionStatus::Loading),
            TransportState::Stopped | TransportState::NoMedia => {
                let in_grace = self.load_grace_until.is_some_and(|t| now < t);
                if in_grace {
                    // Still settling after a load/resume.
                } else if matches!(
                    self.state.status,
                    SessionStatus::Playing | SessionStatus::Loading
                ) && self.current_url.is_some()
                {
                    let dur = pi.duration.or(self.state.duration_sec);
                    let rel = pi.rel_time.or(self.last_rel).unwrap_or(0.0);
                    let natural_end = match dur {
                        Some(d) => rel >= d - 3.0 || self.last_rel.is_some_and(|l| l >= d - 3.0),
                        None => true,
                    };
                    if natural_end {
                        if self.queue.advance() {
                            let _ = self.play_current(0.0).await;
                        } else {
                            self.stop_at_end().await;
                        }
                        self.finish_poll(before);
                        return;
                    }
                    self.set_status(SessionStatus::Stopped);
                } else if self.state.status != SessionStatus::Idle && self.resume_at.is_none() {
                    self.set_status(SessionStatus::Stopped);
                }
            }
            TransportState::Other(_) => {}
        }

        // Re-check: a corrective seek above starts a new settle window, and this poll's position is stale.
        // While waiting to resume after a restart the device's position isn't ours.
        let settling = self.ignore_ticks_until.is_some_and(|t| now < t) || self.resume_at.is_some();
        if !settling {
            if let Some(rel) = pi.rel_time {
                self.set_position(rel);
                self.last_rel = Some(rel);
            }
            if let Some(d) = pi.duration {
                self.state.duration_sec = Some(d);
            }
        }
        self.refresh_volume(false).await;
        self.finish_poll(before);
    }

    fn finish_poll(
        &mut self,
        before: (
            SessionStatus,
            Option<u64>,
            Option<u8>,
            Option<bool>,
            Option<f64>,
        ),
    ) {
        let current_qid = self.queue.current().map(|e| e.qid);
        let after = (
            self.state.status,
            current_qid,
            self.state.volume,
            self.state.muted,
            self.state.duration_sec,
        );
        self.publish(after != before);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::remote::link::mock::MockRenderer;
    use crate::remote::media::Delivery;
    use crate::remote::queue::tests::track;
    use std::sync::Mutex;

    /// Scripted environment: items whose id is in `missing` fail to prepare.
    #[derive(Default)]
    pub(crate) struct TestEnv {
        pub(crate) plays: Mutex<Vec<(String, String)>>,
        pub(crate) scrobbles: Mutex<Vec<(String, String)>>,
        pub(crate) missing: Mutex<Vec<String>>,
        counter: Mutex<u64>,
        pub(crate) saved: Mutex<Vec<(String, PersistedSession)>>,
        pub(crate) positions: Mutex<Vec<(String, f64, SessionStatus)>>,
        pub(crate) forgotten: Mutex<Vec<String>>,
    }

    impl TestEnv {
        pub(crate) fn prepare(&self, item: &QueueItem) -> Result<PreparedItem, PrepareError> {
            let (id, title) = match item {
                QueueItem::Track { id, title, .. } | QueueItem::Mix { id, title, .. } => {
                    (id.clone(), title.clone())
                }
            };
            if self.missing.lock().unwrap().contains(&id) {
                return Err(PrepareError::FileMissing);
            }
            let mut c = self.counter.lock().unwrap();
            *c += 1;
            let duration = item.duration();
            Ok(PreparedItem {
                url: format!("http://mock/{id}/{c}"),
                didl: format!(
                    r#"<res duration="{}">x</res>"#,
                    crate::upnp::didl::format_duration(duration.unwrap_or(10.0))
                ),
                delivery: Delivery::Direct,
                duration,
                title,
            })
        }
    }

    fn timed(id: &str, secs: f64) -> QueueItem {
        match track(id) {
            QueueItem::Track {
                id,
                title,
                artist,
                album,
                album_id,
                radio_reason,
                ..
            } => QueueItem::Track {
                id,
                title,
                artist,
                album,
                album_id,
                duration: Some(secs),
                radio_reason,
            },
            other => other,
        }
    }

    pub(crate) fn alice() -> Controller {
        Controller {
            user_id: "alice".into(),
            username: "Alice".into(),
        }
    }
    fn bob() -> Controller {
        Controller {
            user_id: "bob".into(),
            username: "Bob".into(),
        }
    }

    struct Harness {
        handle: SessionHandle,
        mock: Arc<MockRenderer>,
        env: Arc<TestEnv>,
        events: broadcast::Receiver<RemoteEvent>,
        ended: Arc<Mutex<bool>>,
    }

    fn harness(set_next: bool) -> Harness {
        spawn_with(
            Arc::new(MockRenderer::wiim()),
            Arc::new(TestEnv::default()),
            set_next,
            None,
        )
    }

    /// A session on an existing mock/env (restart tests reuse both), optionally continuing a saved session.
    fn spawn_with(
        mock: Arc<MockRenderer>,
        env: Arc<TestEnv>,
        set_next: bool,
        restore: Option<PersistedSession>,
    ) -> Harness {
        let (events_tx, events) = broadcast::channel(1024);
        let ended = Arc::new(Mutex::new(false));
        let ended2 = ended.clone();
        let handle = spawn_session(
            SessionConfig {
                udn: "wiim".into(),
                name: "WiiM Ultra".into(),
                poll: Duration::from_millis(500),
                set_next,
                seek: true,
                seed: 5,
            },
            RendererLink::Mock(mock.clone()),
            SessionEnv::Test(env.clone()),
            events_tx,
            Box::new(move |_| *ended2.lock().unwrap() = true),
            restore,
        );
        Harness {
            handle,
            mock,
            env,
            events,
            ended,
        }
    }

    fn load(items: Vec<QueueItem>, start: usize) -> Command {
        Command::Load {
            items,
            start_index: start,
            start_at: 0.0,
            queue_source: None,
            shuffle: None,
            repeat: None,
            resumed: false,
        }
    }

    async fn sleep_s(s: f64) {
        tokio::time::sleep(Duration::from_secs_f64(s)).await;
    }

    fn current_title(h: &Harness) -> Option<String> {
        h.handle.snapshot().now_playing_title()
    }

    fn last_saved(env: &TestEnv) -> PersistedSession {
        env.saved.lock().unwrap().last().unwrap().1.clone()
    }

    #[tokio::test(start_paused = true)]
    async fn saves_changes_and_position_and_forgets_on_release() {
        let h = harness(true);
        h.handle
            .send(
                load(vec![timed("a", 60.0), timed("b", 60.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(3.0).await;
        let saved = last_saved(&h.env);
        assert_eq!(saved.queue.entries.len(), 2);
        assert_eq!(saved.owner_name.as_deref(), Some("Alice"));
        assert_eq!(saved.status, SessionStatus::Playing);
        assert!(saved
            .current_url
            .as_deref()
            .is_some_and(|u| u.starts_with("http://mock/a/")));
        assert_eq!(saved.played_qid, Some(saved.queue.entries[0].qid));
        sleep_s(20.0).await;
        assert!(
            h.env
                .positions
                .lock()
                .unwrap()
                .iter()
                .any(|(_, pos, s)| *pos > 10.0 && *s == SessionStatus::Playing),
            "position refreshed while playing"
        );
        h.handle
            .send(Command::Release { stop: true }, alice(), false)
            .await
            .unwrap();
        sleep_s(0.5).await;
        assert_eq!(*h.env.forgotten.lock().unwrap(), vec!["wiim".to_string()]);
    }

    #[tokio::test(start_paused = true)]
    async fn scrobbles_once_the_threshold_passes_and_never_short_or_resumed_tracks() {
        let h = harness(true);
        h.handle
            .send(
                load(vec![timed("a", 60.0), timed("b", 20.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(20.0).await;
        assert!(
            h.env.scrobbles.lock().unwrap().is_empty(),
            "a is below half its length"
        );
        sleep_s(15.0).await;
        assert_eq!(
            *h.env.scrobbles.lock().unwrap(),
            vec![("alice".to_string(), "a".to_string())]
        );
        // b (20 s) is too short to scrobble.
        sleep_s(50.0).await;
        assert_eq!(h.env.scrobbles.lock().unwrap().len(), 1);
        assert_eq!(h.env.plays.lock().unwrap().len(), 2);

        // A handoff that starts past the threshold was already scrobbled by the other player.
        let h = harness(true);
        let mut cmd = load(vec![timed("c", 60.0)], 0);
        if let Command::Load {
            start_at, resumed, ..
        } = &mut cmd
        {
            *start_at = 40.0;
            *resumed = true;
        }
        h.handle.send(cmd, alice(), false).await.unwrap();
        sleep_s(15.0).await;
        assert!(h.env.scrobbles.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn clearing_the_queue_forgets_and_dropping_keeps_the_row() {
        let h = harness(true);
        h.handle
            .send(load(vec![timed("a", 60.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(2.0).await;
        h.handle.send(Command::Clear, alice(), false).await.unwrap();
        sleep_s(0.5).await;
        assert_eq!(
            h.env.forgotten.lock().unwrap().len(),
            1,
            "empty queue: nothing to restore"
        );

        let Harness {
            handle, env, ended, ..
        } = harness(true);
        handle
            .send(load(vec![timed("a", 60.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(2.0).await;
        drop(handle);
        sleep_s(0.5).await;
        assert!(
            *ended.lock().unwrap(),
            "the actor ends once the manager drops its handles"
        );
        assert!(
            env.forgotten.lock().unwrap().is_empty(),
            "a dropped session keeps its saved row"
        );
        assert!(
            !env.positions.lock().unwrap().is_empty(),
            "and saves its last position"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn restored_session_continues_on_the_renderers_current_stream() {
        let first = harness(true);
        first
            .handle
            .send(
                load(vec![timed("a", 8.0), timed("b", 8.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(3.0).await;
        let saved = last_saved(&first.env);
        let Harness {
            handle, mock, env, ..
        } = first;
        drop(handle);
        sleep_s(0.2).await;

        // Server back: same device, still playing our URL.
        let h = spawn_with(mock, env, true, Some(saved));
        sleep_s(1.0).await;
        let s = h.handle.snapshot();
        assert_eq!(s.status, SessionStatus::Playing);
        assert_eq!(s.owner_name.as_deref(), Some("Alice"));
        assert_eq!(current_title(&h).as_deref(), Some("Title a"));
        assert_eq!(s.queue.len(), 2);
        h.handle.send(Command::Pause, alice(), false).await.unwrap();
        sleep_s(0.6).await;
        h.handle.send(Command::Play, alice(), false).await.unwrap();
        sleep_s(1.5).await;
        assert_eq!(
            h.env.plays.lock().unwrap().len(),
            1,
            "pause/resume after a restore is not a new play"
        );
        sleep_s(8.0).await;
        assert_eq!(
            current_title(&h).as_deref(),
            Some("Title b"),
            "gapless handover still works"
        );
        let plays: Vec<String> = h
            .env
            .plays
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(plays, vec!["a", "b"], "a was not counted twice");
    }

    #[tokio::test(start_paused = true)]
    async fn restored_handoff_session_does_not_count_its_track_again() {
        let first = harness(true);
        let mut cmd = load(vec![timed("a", 60.0), timed("b", 60.0)], 0);
        if let Command::Load {
            start_at, resumed, ..
        } = &mut cmd
        {
            *start_at = 4.0;
            *resumed = true;
        }
        first.handle.send(cmd, alice(), false).await.unwrap();
        sleep_s(6.0).await;
        let saved = last_saved(&first.env);
        assert!(saved.played_qid.is_some(), "saved: {saved:?}");
        let Harness {
            handle, mock, env, ..
        } = first;
        drop(handle);
        sleep_s(0.2).await;
        let h = spawn_with(mock, env, true, Some(saved));
        sleep_s(2.0).await;
        h.handle.send(Command::Pause, alice(), false).await.unwrap();
        sleep_s(0.6).await;
        h.handle.send(Command::Play, alice(), false).await.unwrap();
        sleep_s(2.0).await;
        assert!(
            h.env.plays.lock().unwrap().is_empty(),
            "plays: {:?}",
            h.env.plays.lock().unwrap()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn restored_session_comes_back_paused_when_the_device_stopped() {
        let first = harness(true);
        first
            .handle
            .send(
                load(vec![timed("a", 60.0), timed("b", 60.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(8.0).await;
        let saved = last_saved(&first.env);
        let Harness {
            handle, mock, env, ..
        } = first;
        drop(handle);
        sleep_s(0.2).await;
        let mut saved = saved;
        saved.position_sec = 7.0; // the row's position column is newer than the snapshot
        mock.stop_reporting_end(); // the stream died while the server was away (WiiM-style report)

        let h = spawn_with(mock, env, true, Some(saved));
        sleep_s(16.0).await;
        let s = h.handle.snapshot();
        assert_eq!(
            s.status,
            SessionStatus::Paused,
            "not lost control, not advanced"
        );
        assert_eq!(s.position_sec, 7.0);
        assert_eq!(current_title(&h).as_deref(), Some("Title a"));
        h.handle
            .send(Command::Seek(9.0), alice(), false)
            .await
            .unwrap();
        assert_eq!(
            h.handle.snapshot().position_sec,
            9.0,
            "seeking moves the resume point"
        );
        h.handle
            .send(Command::Seek(7.0), alice(), false)
            .await
            .unwrap();

        h.handle.send(Command::Play, alice(), false).await.unwrap();
        sleep_s(3.0).await;
        assert_eq!(h.handle.snapshot().status, SessionStatus::Playing);
        assert!(
            h.mock.calls().iter().any(|c| c == "Seek 7"),
            "resumes at the saved position: {:?}",
            h.mock.calls()
        );
        assert_eq!(
            h.env.plays.lock().unwrap().len(),
            1,
            "continuing the same track is not a new play"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn gapless_queue_plays_through_and_records_plays() {
        let h = harness(true);
        let s = h
            .handle
            .send(
                load(vec![timed("a", 6.0), timed("b", 6.0), timed("c", 6.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(s.status, SessionStatus::Loading);
        assert_eq!(s.owner_name.as_deref(), Some("Alice"));
        sleep_s(3.0).await;
        assert_eq!(h.handle.snapshot().status, SessionStatus::Playing);
        assert!(
            h.mock
                .calls()
                .iter()
                .any(|c| c.starts_with("SetNextAVTransportURI http://mock/b/")),
            "prefetched b"
        );
        sleep_s(5.0).await;
        assert_eq!(
            current_title(&h).as_deref(),
            Some("Title b"),
            "gapless transition detected"
        );
        sleep_s(6.0).await;
        assert_eq!(current_title(&h).as_deref(), Some("Title c"));
        sleep_s(8.0).await;
        assert_eq!(
            h.handle.snapshot().status,
            SessionStatus::Stopped,
            "end of queue"
        );
        let plays: Vec<String> = h
            .env
            .plays
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(plays, vec!["a", "b", "c"]);
        assert!(h
            .env
            .plays
            .lock()
            .unwrap()
            .iter()
            .all(|(u, _)| u == "alice"));
        // Only the gapless path was used: one SetAVTransportURI for the whole queue.
        assert_eq!(
            h.mock
                .calls()
                .iter()
                .filter(|c| c.starts_with("SetAVTransportURI"))
                .count(),
            1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn resumed_load_does_not_record_the_start_item_again() {
        let h = harness(true);
        let mut cmd = load(vec![timed("a", 6.0), timed("b", 6.0)], 0);
        if let Command::Load {
            start_at, resumed, ..
        } = &mut cmd
        {
            *start_at = 2.0;
            *resumed = true;
        }
        h.handle.send(cmd, alice(), false).await.unwrap();
        sleep_s(12.0).await;
        assert_eq!(current_title(&h).as_deref(), Some("Title b"));
        let plays: Vec<String> = h
            .env
            .plays
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| t.clone())
            .collect();
        assert_eq!(plays, vec!["b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn without_set_next_falls_back_to_load_on_stop() {
        let h = harness(false);
        h.handle
            .send(
                load(vec![timed("a", 5.0), timed("b", 5.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(9.0).await;
        assert_eq!(current_title(&h).as_deref(), Some("Title b"));
        assert!(!h.mock.calls().iter().any(|c| c.starts_with("SetNext")));
        assert_eq!(
            h.mock
                .calls()
                .iter()
                .filter(|c| c.starts_with("SetAVTransportURI"))
                .count(),
            2
        );
    }

    #[tokio::test(start_paused = true)]
    async fn seek_reissues_set_next_so_gapless_survives() {
        let h = harness(true);
        h.handle
            .send(
                load(vec![timed("a", 20.0), timed("b", 10.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(3.0).await;
        let next_calls = |h: &Harness| {
            h.mock
                .calls()
                .iter()
                .filter(|c| c.starts_with("SetNextAVTransportURI http"))
                .count()
        };
        assert_eq!(next_calls(&h), 1);
        h.handle
            .send(Command::Seek(17.0), alice(), false)
            .await
            .unwrap();
        sleep_s(2.5).await;
        assert_eq!(next_calls(&h), 2, "re-issued after seek");
        sleep_s(3.0).await;
        assert_eq!(current_title(&h).as_deref(), Some("Title b"));
        assert_eq!(
            h.mock
                .calls()
                .iter()
                .filter(|c| c.starts_with("SetAVTransportURI"))
                .count(),
            1,
            "still gapless"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn corrects_carried_over_start_position() {
        let h = harness(true);
        h.mock.carry_seek(8.0);
        h.handle
            .send(load(vec![timed("a", 30.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(3.0).await;
        assert!(
            h.mock.calls().contains(&"Seek 0".to_string()),
            "corrective seek to 0: {:?}",
            h.mock.calls()
        );
        assert!(h.handle.snapshot().position_sec < 3.0);
    }

    #[tokio::test(start_paused = true)]
    async fn transport_commands_and_volume() {
        let h = harness(true);
        h.handle
            .send(
                load(
                    vec![timed("a", 60.0), timed("b", 60.0), timed("c", 60.0)],
                    1,
                ),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(2.0).await;
        let s = h.handle.send(Command::Pause, alice(), false).await.unwrap();
        assert_eq!(s.status, SessionStatus::Paused);
        assert!(
            h.handle.send(Command::Pause, alice(), false).await.is_ok(),
            "701 on pause is ignored"
        );
        h.handle
            .send(Command::Toggle, alice(), false)
            .await
            .unwrap();
        sleep_s(4.0).await;
        assert_eq!(h.handle.snapshot().status, SessionStatus::Playing);
        h.handle
            .send(Command::Toggle, alice(), false)
            .await
            .unwrap();
        assert_eq!(h.handle.snapshot().status, SessionStatus::Paused);
        h.handle.send(Command::Play, alice(), false).await.unwrap();

        h.handle.send(Command::Next, alice(), false).await.unwrap();
        assert_eq!(current_title(&h).as_deref(), Some("Title c"));
        sleep_s(5.0).await;
        h.handle.send(Command::Prev, alice(), false).await.unwrap();
        assert!(
            h.mock.calls().contains(&"Seek 0".to_string()),
            "prev > 3 s restarts"
        );
        h.handle.send(Command::Prev, alice(), false).await.unwrap();
        assert_eq!(current_title(&h).as_deref(), Some("Title b"));
        let first = h.handle.snapshot().queue[0].qid;
        h.handle
            .send(Command::Jump(first), alice(), false)
            .await
            .unwrap();
        assert_eq!(current_title(&h).as_deref(), Some("Title a"));
        h.handle.send(Command::Prev, alice(), false).await.unwrap();
        h.handle.send(Command::Next, alice(), false).await.unwrap();
        h.handle.send(Command::Next, alice(), false).await.unwrap();
        let end = h.handle.send(Command::Next, alice(), false).await.unwrap();
        assert_eq!(end.status, SessionStatus::Stopped, "next at the end stops");

        let v = h
            .handle
            .send(Command::SetVolume(150), alice(), false)
            .await
            .unwrap();
        assert_eq!(v.volume, Some(100));
        h.handle
            .send(Command::SetMute(true), alice(), false)
            .await
            .unwrap();
        assert_eq!(h.mock.current_volume(), (100, true));
        h.mock.external_volume(20);
        h.handle.send(Command::Play, alice(), false).await.unwrap();
        sleep_s(7.0).await;
        assert_eq!(
            h.handle.snapshot().volume,
            Some(20),
            "device-side volume change reflected"
        );

        assert!(matches!(
            h.handle.send(Command::Seek(-1.0), alice(), false).await,
            Err(SessionError::BadRequest(_))
        ));
        assert!(matches!(
            h.handle.send(Command::Jump(999), alice(), false).await,
            Err(SessionError::BadRequest(_))
        ));
        let st = h.handle.send(Command::Stop, alice(), false).await.unwrap();
        assert_eq!(st.status, SessionStatus::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn fixed_volume_output_is_detected() {
        let mut h = harness(true);
        h.mock.fix_volume(100);
        h.handle
            .send(load(vec![timed("a", 60.0)], 0), alice(), false)
            .await
            .unwrap();
        assert_eq!(h.handle.snapshot().volume, Some(100));
        assert!(h.handle.snapshot().can_set_volume);
        assert_eq!(
            h.handle.send(Command::SetVolume(15), alice(), false).await,
            Err(SessionError::Unsupported("volume (it is fixed)"))
        );
        let s = h.handle.snapshot();
        assert!(!s.can_set_volume, "volume control switched off");
        assert_eq!(s.volume, Some(100));
        assert!(matches!(
            h.handle.send(Command::SetVolume(20), alice(), false).await,
            Err(SessionError::Unsupported("volume"))
        ));
        let mut fixed_notice = false;
        while let Ok(e) = h.events.try_recv() {
            if let RemoteEvent::Notice { message, .. } = e {
                fixed_notice |= message.contains("volume is fixed");
            }
        }
        assert!(fixed_notice);
    }

    #[tokio::test(start_paused = true)]
    async fn queue_edits_resync_prefetch() {
        let h = harness(true);
        h.handle
            .send(
                load(vec![timed("a", 60.0), timed("b", 60.0)], 0),
                alice(),
                false,
            )
            .await
            .unwrap();
        sleep_s(3.0).await;
        h.handle
            .send(
                Command::Enqueue {
                    items: vec![timed("x", 30.0)],
                    mode: EnqueueMode::Next,
                },
                alice(),
                false,
            )
            .await
            .unwrap();
        assert!(h
            .mock
            .calls()
            .iter()
            .any(|c| c.starts_with("SetNextAVTransportURI http://mock/x/")));
        let qx = h.handle.snapshot().queue[1].qid;
        h.handle
            .send(Command::Move { qid: qx, to: 2 }, alice(), false)
            .await
            .unwrap();
        assert!(h
            .mock
            .calls()
            .last()
            .unwrap()
            .starts_with("SetNextAVTransportURI http://mock/b/"));
        let qb = h.handle.snapshot().queue[1].qid;
        h.handle
            .send(Command::Remove(qb), alice(), false)
            .await
            .unwrap();
        h.handle
            .send(Command::SetRepeat(Repeat::One), alice(), false)
            .await
            .unwrap();
        assert!(
            h.mock
                .calls()
                .last()
                .unwrap()
                .starts_with("SetNextAVTransportURI http://mock/a/"),
            "repeat-one prefetches the same item"
        );
        h.handle
            .send(Command::SetRepeat(Repeat::Off), alice(), false)
            .await
            .unwrap();
        h.handle
            .send(Command::SetShuffle(true), alice(), false)
            .await
            .unwrap();
        // Removing the current entry plays the following one.
        let qa = h.handle.snapshot().current_qid.unwrap();
        h.handle
            .send(Command::Remove(qa), alice(), false)
            .await
            .unwrap();
        assert_eq!(current_title(&h).as_deref(), Some("Title x"));
        let qx = h.handle.snapshot().current_qid.unwrap();
        let s = h
            .handle
            .send(Command::Remove(qx), alice(), false)
            .await
            .unwrap();
        assert_eq!(
            s.status,
            SessionStatus::Stopped,
            "removing the last current entry stops"
        );
        assert!(matches!(
            h.handle.send(Command::Remove(4242), alice(), false).await,
            Err(SessionError::BadRequest(_))
        ));
        assert!(matches!(
            h.handle
                .send(Command::Move { qid: 4242, to: 0 }, alice(), false)
                .await,
            Err(SessionError::BadRequest(_))
        ));
        assert!(matches!(
            h.handle
                .send(
                    Command::Enqueue {
                        items: vec![],
                        mode: EnqueueMode::End
                    },
                    alice(),
                    false
                )
                .await,
            Err(SessionError::BadRequest(_))
        ));
        h.handle
            .send(
                Command::Enqueue {
                    items: vec![timed("z", 5.0)],
                    mode: EnqueueMode::End,
                },
                alice(),
                false,
            )
            .await
            .unwrap();
        let c = h.handle.send(Command::Clear, alice(), false).await.unwrap();
        assert!(c.queue.is_empty() && c.status == SessionStatus::Stopped);
        assert!(matches!(
            h.handle.send(Command::Play, alice(), false).await,
            Err(SessionError::BadRequest(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn unplayable_items_are_skipped_with_a_notice() {
        let mut h = harness(true);
        h.env.missing.lock().unwrap().push("a".into());
        h.mock.fault_on("/b/");
        h.handle
            .send(
                load(
                    vec![timed("a", 10.0), timed("b", 10.0), timed("c", 10.0)],
                    0,
                ),
                alice(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(current_title(&h).as_deref(), Some("Title c"));
        let mut notices = 0;
        while let Ok(e) = h.events.try_recv() {
            if matches!(e, RemoteEvent::Notice { .. }) {
                notices += 1;
            }
        }
        assert_eq!(notices, 2);
        h.env.missing.lock().unwrap().push("c".into());
        let s = h
            .handle
            .send(
                Command::Jump(h.handle.snapshot().queue[2].qid),
                alice(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(s.status, SessionStatus::Stopped, "nothing playable left");
    }

    #[tokio::test(start_paused = true)]
    async fn takeover_requires_confirmation_and_notifies() {
        let mut h = harness(true);
        h.handle
            .send(load(vec![timed("a", 60.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(2.0).await;
        // Same user on another device: never prompted.
        assert!(h.handle.send(Command::Pause, alice(), false).await.is_ok());
        match h.handle.send(Command::Play, bob(), false).await {
            Err(SessionError::InUse(info)) => {
                assert_eq!(info.owner_user_id, "alice");
                assert_eq!(info.owner_name.as_deref(), Some("Alice"));
                assert_eq!(info.now_playing.as_deref(), Some("Title a"));
                assert_eq!(info.status, SessionStatus::Paused);
            }
            other => panic!("expected InUse, got {other:?}"),
        }
        let s = h.handle.send(Command::Play, bob(), true).await.unwrap();
        assert_eq!(s.owner_user_id.as_deref(), Some("bob"));
        let mut saw_takeover = false;
        while let Ok(e) = h.events.try_recv() {
            if let RemoteEvent::Takeover {
                by_user_name,
                from_user_id,
                ..
            } = e
            {
                saw_takeover = by_user_name == "Bob" && from_user_id == "alice";
            }
        }
        assert!(saw_takeover);
        // Now Alice needs to confirm.
        assert!(matches!(
            h.handle.send(Command::Pause, alice(), false).await,
            Err(SessionError::InUse(_))
        ));
    }

    #[test]
    fn held_by_other_rules() {
        let now = Instant::now();
        let mut s = SessionState::new("u", "n", true, true);
        assert!(!held_by_other(&s, "bob", now), "no owner");
        s.owner_user_id = Some("alice".into());
        s.last_activity = Some(now);
        s.status = SessionStatus::Playing;
        assert!(held_by_other(&s, "bob", now));
        assert!(!held_by_other(&s, "alice", now));
        for free in [
            SessionStatus::Stopped,
            SessionStatus::Idle,
            SessionStatus::Offline,
        ] {
            s.status = free;
            assert!(!held_by_other(&s, "bob", now), "{free:?} is free");
        }
        s.status = SessionStatus::Paused;
        assert!(held_by_other(&s, "bob", now));
        assert!(
            !held_by_other(&s, "bob", now + HOLD_WINDOW),
            "paused 30 min is free"
        );
        s.status = SessionStatus::LostControl;
        assert!(held_by_other(&s, "bob", now));
    }

    #[tokio::test(start_paused = true)]
    async fn external_takeover_offline_and_recovery() {
        let h = harness(true);
        h.handle
            .send(load(vec![timed("a", 120.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(3.0).await;
        h.mock.foreign_play("http://plex/x.mp3");
        sleep_s(2.0).await;
        assert_eq!(h.handle.snapshot().status, SessionStatus::LostControl);
        let resumed = h.handle.send(Command::Play, alice(), false).await.unwrap();
        assert_eq!(
            resumed.status,
            SessionStatus::Loading,
            "resume here reloads our item"
        );
        assert!(h.mock.current().unwrap().starts_with("http://mock/a/"));
        sleep_s(3.0).await;
        assert_eq!(
            h.env.plays.lock().unwrap().len(),
            1,
            "resume here continues the same play"
        );

        h.mock.external_pause();
        sleep_s(1.0).await;
        assert_eq!(
            h.handle.snapshot().status,
            SessionStatus::Paused,
            "device-side pause reflected"
        );

        h.mock.set_offline(true);
        sleep_s(20.0).await;
        assert_eq!(h.handle.snapshot().status, SessionStatus::Offline);
        assert!(matches!(
            h.handle.send(Command::Seek(5.0), alice(), false).await,
            Err(SessionError::Renderer(_))
        ));
        assert!(h.handle.snapshot().error.is_some());
        h.mock.set_offline(false);
        sleep_s(20.0).await;
        assert_ne!(h.handle.snapshot().status, SessionStatus::Offline);
    }

    #[tokio::test(start_paused = true)]
    async fn release_and_external_stop() {
        let h = harness(true);
        h.handle
            .send(load(vec![timed("a", 120.0)], 0), alice(), false)
            .await
            .unwrap();
        sleep_s(3.0).await;
        h.mock.stop().unwrap(); // stopped from the device mid-track: not a natural end
        sleep_s(1.0).await;
        let s = h.handle.snapshot();
        assert_eq!(s.status, SessionStatus::Stopped);
        assert_eq!(
            s.now_playing_title().as_deref(),
            Some("Title a"),
            "did not advance"
        );
        let r = h
            .handle
            .send(Command::Release { stop: true }, alice(), false)
            .await
            .unwrap();
        assert_eq!(r.status, SessionStatus::Stopped);
        sleep_s(0.1).await;
        assert!(*h.ended.lock().unwrap());
        assert!(matches!(
            h.handle.send(Command::Play, alice(), false).await,
            Err(SessionError::Ended)
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_sessions_end_and_validation() {
        let h = harness(true);
        assert!(matches!(
            h.handle.send(load(vec![], 0), alice(), false).await,
            Err(SessionError::BadRequest(_))
        ));
        assert!(matches!(
            h.handle
                .send(load(vec![timed("a", 1.0)], 5), alice(), false)
                .await,
            Err(SessionError::BadRequest(_))
        ));
        let s = h
            .handle
            .send(
                Command::Load {
                    items: vec![timed("a", 5.0), timed("b", 5.0)],
                    start_index: 0,
                    start_at: 2.0,
                    queue_source: Some(serde_json::json!({"type":"album","id":"al1"})),
                    shuffle: Some(false),
                    repeat: Some(Repeat::All),
                    resumed: false,
                },
                alice(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(s.repeat, Repeat::All);
        assert_eq!(s.queue_source.unwrap()["type"], "album");
        h.handle.send(Command::Stop, alice(), false).await.unwrap();
        assert!(matches!(
            h.handle.send(Command::Seek(1.0), alice(), false).await,
            Ok(_) | Err(SessionError::BadRequest(_))
        ));
        sleep_s(IDLE_TEARDOWN.as_secs_f64() + 60.0).await;
        assert!(*h.ended.lock().unwrap(), "idle session torn down");
    }

    #[test]
    fn status_and_state_helpers() {
        assert!(SessionStatus::Loading.is_active());
        assert!(!SessionStatus::Offline.is_active());
        let mut s = SessionState::new("u", "n", false, false);
        assert_eq!(s.now_playing_title(), None);
        s.current_qid = Some(9);
        assert_eq!(s.now_playing_title(), None);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["status"], "idle");
        assert!(json.get("lastActivity").is_none());
        assert!(is_fault(
            &UpnpError::Fault {
                code: 701,
                description: String::new()
            },
            701
        ));
    }
}

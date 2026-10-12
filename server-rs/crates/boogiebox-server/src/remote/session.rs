//! One actor per renderer (`wip/play-on-renderers-plan.md` §5): owns the server-side queue, drives the device,
//! polls its state, keeps gapless prefetch (`SetNext`) in sync, records plays, and enforces takeover protection.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;

use super::link::RendererLink;
use super::media::{PrepareError, PreparedItem};
use super::queue::{EnqueueMode, Queue, QueueEntry, QueueItem, Repeat};
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
/// Volume/mute are re-read at most this often.
pub const VOLUME_POLL: Duration = Duration::from_secs(5);
/// Start position error that triggers a corrective seek after a load.
pub const START_TOLERANCE: f64 = 2.0;
/// Max items in one load/enqueue.
pub const MAX_ITEMS: usize = 5000;

/// Session status shown to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

    async fn record_play(&self, user_id: &str, item: &QueueItem) {
        let QueueItem::Track { id, .. } = item else {
            return; // BoogieMix outputs are not library tracks.
        };
        match self {
            SessionEnv::Real { db, .. } => {
                let db = db.clone();
                let (user, track) = (user_id.to_string(), id.clone());
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    if boogiebox_db::playback::increment_track_play_count(&conn, &track)
                        .unwrap_or(false)
                    {
                        let _ = boogiebox_db::playback::insert_play_history(&conn, &user, &track);
                    }
                })
                .await;
            }
            #[cfg(test)]
            SessionEnv::Test(t) => t.plays.lock().unwrap().push((user_id.into(), id.clone())),
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
    /// Command channel.
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

/// Callback run once when a session actor exits (manager cleanup); receives the session's own sender.
pub type OnEnd = Box<dyn FnOnce(&mpsc::Sender<Request>) + Send>;

/// Spawns a session actor. `on_end` runs once when it exits (manager cleanup).
pub(crate) fn spawn_session(
    cfg: SessionConfig,
    link: RendererLink,
    env: SessionEnv,
    events: broadcast::Sender<RemoteEvent>,
    on_end: OnEnd,
) -> SessionHandle {
    let (tx, rx) = mpsc::channel(32);
    let state = SessionState::new(&cfg.udn, &cfg.name, cfg.seek, link.has_volume());
    let shared = Arc::new(RwLock::new(state.clone()));
    let actor = Actor {
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
        foreign_polls: 0,
        failures: 0,
        last_rel: None,
        last_volume_poll: None,
        stopped_since: None,
        created: Instant::now(),
    };
    let tx_for_end = tx.clone();
    tokio::spawn(async move {
        actor.run(rx).await;
        on_end(&tx_for_end);
    });
    SessionHandle { tx, shared }
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
    foreign_polls: u32,
    failures: u32,
    last_rel: Option<f64>,
    last_volume_poll: Option<Instant>,
    stopped_since: Option<Instant>,
    created: Instant,
}

fn is_fault(e: &UpnpError, code: u32) -> bool {
    e.fault_code() == Some(code)
}

impl Actor {
    async fn run(mut self, mut rx: mpsc::Receiver<Request>) {
        self.refresh_volume(true).await;
        self.publish(true);
        loop {
            let delay = self.poll_delay();
            // Not polling: still wake up when the idle-teardown deadline passes.
            let sleep_for = delay.unwrap_or_else(|| self.idle_remaining());
            tokio::select! {
                req = rx.recv() => {
                    let Some(req) = req else { break };
                    if self.handle(req).await {
                        break;
                    }
                }
                _ = tokio::time::sleep(sleep_for) => {
                    if delay.is_some() {
                        self.poll().await;
                    }
                }
            }
            if self.idle_expired() {
                tracing::info!(target: "remote", udn = %self.cfg.udn, "Play On session idle; ending");
                break;
            }
        }
        let _ = self.events.send(RemoteEvent::SessionEnded {
            udn: self.cfg.udn.clone(),
        });
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

    /// Copies queue state into the snapshot and notifies clients. `full` = structural change.
    fn publish(&mut self, full: bool) {
        self.state.queue = self.queue.entries().to_vec();
        self.state.current_qid = self.queue.current().map(|e| e.qid);
        self.state.shuffle = self.queue.shuffle();
        self.state.repeat = self.queue.repeat();
        if full {
            self.state.revision += 1;
        }
        *self.shared.write().unwrap_or_else(|p| p.into_inner()) = self.state.clone();
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
        match self.state.status {
            SessionStatus::LostControl => {
                let at = self.state.position_sec;
                self.play_current(at).await
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
                    if self.played_qid != Some(entry.qid) {
                        self.played_qid = Some(entry.qid);
                        let user = self
                            .state
                            .last_controller_user_id
                            .clone()
                            .unwrap_or_default();
                        self.env.record_play(&user, &entry.item).await;
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
                } else if self.state.status != SessionStatus::Idle {
                    self.set_status(SessionStatus::Stopped);
                }
            }
            TransportState::Other(_) => {}
        }

        // Re-check: a corrective seek above starts a new settle window, and this poll's position is stale.
        let settling = self.ignore_ticks_until.is_some_and(|t| now < t);
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
        pub(crate) missing: Mutex<Vec<String>>,
        counter: Mutex<u64>,
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
        let mock = Arc::new(MockRenderer::wiim());
        let env = Arc::new(TestEnv::default());
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

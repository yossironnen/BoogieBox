//! Play On: playback on UPnP/DLNA renderers (`wip/play-on-renderers-plan.md`).
//!
//! [`RemoteManager`] owns renderer discovery and the renderer registry. It lives in `AppState`, starts after
//! setup (and again after a database switch), and is reconfigured when the `remote*` settings change.

pub mod link;
pub mod media;
pub mod queue;
pub mod registry;
pub mod session;

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::upnp::discovery::{spawn_discovery, DiscoveryEvent, DiscoveryHandle};
use crate::upnp::soap::SoapClient;
use crate::DbPool;
use registry::{describe_renderer, RendererRegistry, StoredCapabilities};

/// Default poll interval while a renderer is playing.
pub const DEFAULT_POLL_MS: u64 = 1000;
const EXPIRY_SWEEP: Duration = Duration::from_secs(30);
/// Largest icon accepted from a device.
pub const MAX_ICON_BYTES: usize = 256 * 1024;

/// Global Play On settings (admin-editable, see `settings.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSettings {
    /// Master switch (`remotePlaybackEnabled`, default on).
    pub enabled: bool,
    /// Interface override (`remoteInterfaceIp`, empty = auto).
    pub interface_ip: Option<Ipv4Addr>,
    /// Poll interval while playing (`remotePollMs`).
    pub poll_ms: u64,
    /// Stop the renderer when playback moves back to a browser (`remoteReleaseStopsRenderer`).
    pub release_stops_renderer: bool,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interface_ip: None,
            poll_ms: DEFAULT_POLL_MS,
            release_stops_renderer: true,
        }
    }
}

fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Reads the Play On settings, falling back to defaults for missing or invalid values.
pub fn read_remote_settings(conn: &Connection) -> RemoteSettings {
    let d = RemoteSettings::default();
    RemoteSettings {
        enabled: setting(conn, "remotePlaybackEnabled").map_or(d.enabled, |v| v != "false"),
        interface_ip: setting(conn, "remoteInterfaceIp").and_then(|v| v.trim().parse().ok()),
        poll_ms: setting(conn, "remotePollMs")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|v| v.clamp(500, 5000))
            .unwrap_or(d.poll_ms),
        release_stops_renderer: setting(conn, "remoteReleaseStopsRenderer")
            .map_or(d.release_stops_renderer, |v| v != "false"),
    }
}

/// Events pushed to clients over the SSE stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RemoteEvent {
    /// The renderer list or a renderer's online state changed.
    Renderers,
    /// Full session snapshot (structural change).
    Session {
        /// Snapshot.
        state: Box<session::SessionState>,
    },
    /// Lightweight position tick.
    #[serde(rename_all = "camelCase")]
    Position {
        /// Renderer.
        udn: String,
        /// Status.
        status: session::SessionStatus,
        /// Position (s) at `sampled_at_ms`.
        position_sec: f64,
        /// Duration (s).
        duration_sec: Option<f64>,
        /// Sample time (ms since epoch).
        sampled_at_ms: u64,
    },
    /// Another user took over a session (plan 5.5).
    #[serde(rename_all = "camelCase")]
    Takeover {
        /// Renderer.
        udn: String,
        /// Previous holder.
        from_user_id: String,
        /// New holder.
        by_user_id: String,
        /// New holder's name.
        by_user_name: String,
    },
    /// Inline notice for the dock/mini player (plan 8.7).
    Notice {
        /// Renderer.
        udn: String,
        /// `info` | `warning` | `error`.
        level: &'static str,
        /// Text.
        message: String,
    },
    /// A session ended (released, idle, or Play On stopped).
    SessionEnded {
        /// Renderer.
        udn: String,
    },
}

struct Runtime {
    cancel: CancellationToken,
    discovery: DiscoveryHandle,
    settings: RemoteSettings,
}

struct Inner {
    registry: RendererRegistry,
    soap: SoapClient,
    events: broadcast::Sender<RemoteEvent>,
    runtime: Mutex<Option<Runtime>>,
    icons: Mutex<std::collections::HashMap<String, (String, Vec<u8>)>>,
    media: media::MediaTokens,
    http_port: AtomicU16,
    sessions: Mutex<std::collections::HashMap<String, session::SessionHandle>>,
    rate: RateLimiter,
}

/// Play On coordinator stored in `AppState`.
#[derive(Clone)]
pub struct RemoteManager {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for RemoteManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteManager")
            .field("running", &self.is_running())
            .finish()
    }
}

impl Default for RemoteManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteManager {
    /// Creates an idle manager (nothing runs until [`RemoteManager::start`]).
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(Inner {
                registry: RendererRegistry::default(),
                soap: SoapClient::new(),
                events,
                runtime: Mutex::new(None),
                icons: Mutex::new(Default::default()),
                media: media::MediaTokens::default(),
                http_port: AtomicU16::new(crate::DEFAULT_PORT),
                sessions: Mutex::new(Default::default()),
                rate: RateLimiter::default(),
            }),
        }
    }

    /// Renderer registry.
    pub fn registry(&self) -> &RendererRegistry {
        &self.inner.registry
    }

    /// LAN SOAP/HTTP client.
    pub fn soap(&self) -> &SoapClient {
        &self.inner.soap
    }

    /// Media token registry.
    pub fn media(&self) -> &media::MediaTokens {
        &self.inner.media
    }

    /// Records the main HTTP port renderers fetch media from.
    pub fn set_http_port(&self, port: u16) {
        self.inner.http_port.store(port, Ordering::Relaxed);
    }

    /// Main HTTP port renderers fetch media from.
    pub fn http_port(&self) -> u16 {
        self.inner.http_port.load(Ordering::Relaxed)
    }

    /// Subscribes to client-facing events.
    pub fn subscribe(&self) -> broadcast::Receiver<RemoteEvent> {
        self.inner.events.subscribe()
    }

    fn emit(&self, event: RemoteEvent) {
        let _ = self.inner.events.send(event);
    }

    /// Publishes an event to connected clients (no-op without subscribers).
    pub fn notify(&self, event: RemoteEvent) {
        self.emit(event);
    }

    /// True while discovery is running.
    pub fn is_running(&self) -> bool {
        self.inner
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Settings the running instance was started with.
    pub fn running_settings(&self) -> Option<RemoteSettings> {
        self.inner
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|r| r.settings.clone())
    }

    /// Discovery diagnostics, when running.
    pub fn discovery_status(&self) -> Option<crate::upnp::discovery::DiscoveryStatus> {
        self.inner
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|r| r.discovery.status())
    }

    /// Requests an immediate search. Returns false when Play On is not running.
    pub fn refresh(&self) -> bool {
        match self
            .inner
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            Some(rt) => {
                rt.discovery.refresh();
                true
            }
            None => false,
        }
    }

    /// Stops discovery and forgets in-memory state.
    pub fn stop(&self) {
        if let Some(rt) = self
            .inner
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            rt.cancel.cancel();
        }
        // Dropping the handles closes every session's command channel, which ends the actors.
        let ended: Vec<String> = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .map(|(udn, _)| udn)
            .collect();
        for udn in &ended {
            self.inner.media.revoke_owner(udn);
        }
        self.inner.registry.clear();
        self.inner
            .icons
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.emit(RemoteEvent::Renderers);
    }

    /// (Re)starts Play On against `db`, honoring the current settings. `parent` is the app's worker token so
    /// a database switch stops discovery automatically.
    pub async fn start(&self, db: DbPool, parent: &CancellationToken) {
        self.stop();
        let loaded = tokio::task::spawn_blocking({
            let db = db.clone();
            move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                let settings = read_remote_settings(&conn);
                let rows = boogiebox_db::remote::list_renderers(&conn).unwrap_or_default();
                (settings, rows)
            }
        })
        .await;
        let Ok((settings, rows)) = loaded else {
            return;
        };
        if !settings.enabled {
            tracing::info!(target: "remote", "Play On disabled; discovery not started");
            return;
        }
        self.inner.registry.load_persisted(&rows, Instant::now());

        let cancel = parent.child_token();
        let (tx, rx) = mpsc::channel(64);
        let discovery = spawn_discovery(settings.interface_ip, tx, cancel.clone());
        *self.inner.runtime.lock().unwrap_or_else(|p| p.into_inner()) = Some(Runtime {
            cancel: cancel.clone(),
            discovery,
            settings,
        });
        tokio::spawn(self.clone().consume(rx, db, cancel));
        self.emit(RemoteEvent::Renderers);
    }

    async fn consume(
        self,
        mut rx: mpsc::Receiver<DiscoveryEvent>,
        db: DbPool,
        cancel: CancellationToken,
    ) {
        let in_flight: Arc<Mutex<HashSet<String>>> = Arc::default();
        let mut sweep = tokio::time::interval(EXPIRY_SWEEP);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = sweep.tick() => {
                    self.inner.media.sweep(Instant::now());
                    if !self.inner.registry.expire(Instant::now()).is_empty() {
                        self.emit(RemoteEvent::Renderers);
                    }
                }
                event = rx.recv() => match event {
                    None => break,
                    Some(event) => self.handle_discovery(event, &db, &in_flight),
                },
            }
        }
    }

    fn handle_discovery(
        &self,
        event: DiscoveryEvent,
        db: &DbPool,
        in_flight: &Arc<Mutex<HashSet<String>>>,
    ) {
        match event {
            DiscoveryEvent::Gone { udn } => {
                if self.inner.registry.mark_gone(&udn) {
                    self.emit(RemoteEvent::Renderers);
                }
            }
            DiscoveryEvent::Seen {
                location,
                udn,
                max_age,
            } => {
                let outcome =
                    self.inner
                        .registry
                        .mark_seen(&udn, &location, max_age, Instant::now());
                if outcome.came_online {
                    self.emit(RemoteEvent::Renderers);
                }
                if outcome.persist_touch && !outcome.needs_description {
                    let db = db.clone();
                    let udn = udn.clone();
                    tokio::task::spawn_blocking(move || {
                        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = boogiebox_db::remote::touch_renderer(&conn, &udn);
                    });
                }
                if outcome.needs_description
                    && in_flight
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(udn.clone())
                {
                    let this = self.clone();
                    let db = db.clone();
                    let in_flight = in_flight.clone();
                    tokio::spawn(async move {
                        this.describe_and_store(&location, max_age, &db).await;
                        in_flight
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .remove(&udn);
                    });
                }
            }
        }
    }

    async fn describe_and_store(&self, location: &str, max_age: u64, db: &DbPool) {
        let (description, caps) = match describe_renderer(&self.inner.soap, location).await {
            Ok(v) => v,
            Err(e) => {
                tracing::info!(target: "remote", "Ignoring renderer at {location}: {e}");
                return;
            }
        };
        let stored = StoredCapabilities {
            description: description.clone(),
            caps: caps.clone(),
        };
        let json = serde_json::to_string(&stored).unwrap_or_else(|_| "{}".into());
        let persist = {
            let db = db.clone();
            let d = description.clone();
            let location = location.to_string();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::remote::upsert_renderer(
                    &conn,
                    &boogiebox_db::remote::RendererUpsert {
                        udn: &d.udn,
                        friendly_name: &d.friendly_name,
                        manufacturer: d.manufacturer.as_deref(),
                        model_name: d.model_name.as_deref(),
                        vendor: d.vendor.as_str(),
                        location_url: &location,
                        capabilities: &json,
                    },
                )
            })
            .await
        };
        if !matches!(persist, Ok(Ok(()))) {
            tracing::warn!(target: "remote", udn = %description.udn, "Could not persist renderer");
        }
        tracing::info!(
            target: "remote",
            udn = %description.udn,
            name = %description.friendly_name,
            set_next = caps.set_next,
            "Renderer described"
        );
        self.inner
            .icons
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&description.udn);
        if self.inner.registry.insert_described(
            description,
            caps,
            location,
            max_age,
            Instant::now(),
        ) {
            self.emit(RemoteEvent::Renderers);
        }
    }

    /// Returns the renderer's best icon `(mime, bytes)`, fetched once from the device and cached.
    pub async fn icon(&self, udn: &str) -> Option<(String, Vec<u8>)> {
        if let Some(hit) = self
            .inner
            .icons
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(udn)
        {
            return Some(hit.clone());
        }
        let entry = self.inner.registry.get(udn)?;
        let icon = entry.description.best_icon()?.clone();
        let mime = icon.mime.to_ascii_lowercase();
        if mime != "image/png" && mime != "image/jpeg" {
            return None;
        }
        let bytes = self
            .inner
            .soap
            .get_bytes(&icon.url, MAX_ICON_BYTES)
            .await
            .ok()?;
        self.inner
            .icons
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(udn.to_string(), (mime.clone(), bytes.clone()));
        Some((mime, bytes))
    }
}

type SessionMap = std::collections::HashMap<String, session::SessionHandle>;

impl RemoteManager {
    fn sessions(&self) -> std::sync::MutexGuard<'_, SessionMap> {
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Per-user rate limit for state-changing requests.
    pub fn allow_request(&self, user_id: &str) -> bool {
        self.inner.rate.allow(user_id)
    }

    /// Live session for `udn`.
    pub fn session(&self, udn: &str) -> Option<session::SessionHandle> {
        self.sessions()
            .get(udn)
            .filter(|h| !h.tx.is_closed())
            .cloned()
    }

    /// Latest snapshot of every live session.
    pub fn session_snapshots(&self) -> Vec<session::SessionState> {
        self.sessions()
            .values()
            .filter(|h| !h.tx.is_closed())
            .map(|h| h.snapshot())
            .collect()
    }

    fn open_session(
        &self,
        db: &DbPool,
        udn: &str,
    ) -> Result<session::SessionHandle, session::SessionError> {
        let mut sessions = self.sessions();
        if let Some(h) = sessions.get(udn).filter(|h| !h.tx.is_closed()) {
            return Ok(h.clone());
        }
        let entry = self
            .inner
            .registry
            .get(udn)
            .ok_or_else(|| session::SessionError::BadRequest("unknown renderer".into()))?;
        if !entry.online {
            return Err(session::SessionError::Renderer(format!(
                "{} is offline",
                entry.description.friendly_name
            )));
        }
        let poll_ms = self
            .running_settings()
            .map(|s| s.poll_ms)
            .unwrap_or(DEFAULT_POLL_MS);
        let link = link::RendererLink::Soap {
            soap: self.inner.soap.clone(),
            avt: entry.description.av_transport.clone(),
            rc: entry.description.rendering_control.clone(),
        };
        let manager = self.clone();
        let end_udn = udn.to_string();
        let handle = session::spawn_session(
            session::SessionConfig {
                udn: udn.to_string(),
                name: entry.description.friendly_name.clone(),
                poll: Duration::from_millis(poll_ms),
                set_next: entry.caps.set_next,
                seek: entry.caps.seek,
                seed: rand::random(),
            },
            link,
            session::SessionEnv::Real {
                manager: self.clone(),
                db: db.clone(),
            },
            self.inner.events.clone(),
            Box::new(move |tx| {
                let mut sessions = manager.sessions();
                if sessions
                    .get(&end_udn)
                    .is_some_and(|h| h.tx.same_channel(tx))
                {
                    sessions.remove(&end_udn);
                    manager.inner.media.revoke_owner(&end_udn);
                }
            }),
        );
        sessions.insert(udn.to_string(), handle.clone());
        Ok(handle)
    }

    /// Sends a command to `udn`'s session, creating the session on first use.
    pub async fn command(
        &self,
        db: &DbPool,
        udn: &str,
        cmd: session::Command,
        by: session::Controller,
        takeover: bool,
    ) -> Result<session::SessionState, session::SessionError> {
        let handle = match self.session(udn) {
            Some(h) => h,
            None if matches!(cmd, session::Command::Release { .. }) => {
                return Err(session::SessionError::Ended)
            }
            None => self.open_session(db, udn)?,
        };
        handle.send(cmd, by, takeover).await
    }

    /// Moves `from`'s queue and position to `to`. The target is loaded first, so a refused target leaves
    /// `from` playing.
    pub async fn transfer(
        &self,
        db: &DbPool,
        from: &str,
        to: &str,
        by: session::Controller,
        takeover: bool,
    ) -> Result<session::SessionState, session::SessionError> {
        if from == to {
            return Err(session::SessionError::BadRequest("same renderer".into()));
        }
        let source = self.session(from).ok_or(session::SessionError::Ended)?;
        let snap = source.snapshot();
        if snap.queue.is_empty() {
            return Err(session::SessionError::BadRequest("queue is empty".into()));
        }
        if !takeover && session::held_by_other(&snap, &by.user_id, tokio::time::Instant::now()) {
            return Err(session::SessionError::InUse(Box::new(session::InUseInfo {
                udn: snap.udn.clone(),
                device_name: snap.renderer_name.clone(),
                owner_user_id: snap.owner_user_id.clone().unwrap_or_default(),
                owner_name: snap.owner_name.clone(),
                status: snap.status,
                now_playing: snap.now_playing_title(),
            })));
        }
        let start_index = snap
            .current_qid
            .and_then(|q| snap.queue.iter().position(|e| e.qid == q))
            .unwrap_or(0);
        let elapsed = if snap.status == session::SessionStatus::Playing {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(snap.sampled_at_ms);
            now_ms.saturating_sub(snap.sampled_at_ms) as f64 / 1000.0
        } else {
            0.0
        };
        let target = self
            .command(
                db,
                to,
                session::Command::Load {
                    items: snap.queue.iter().map(|e| e.item.clone()).collect(),
                    start_index,
                    start_at: snap.position_sec + elapsed,
                    queue_source: snap.queue_source.clone(),
                    shuffle: Some(false),
                    repeat: Some(snap.repeat),
                    resumed: true,
                },
                by.clone(),
                takeover,
            )
            .await?;
        let _ = source
            .send(session::Command::Release { stop: true }, by, true)
            .await;
        Ok(target)
    }
}

/// Per-user request counter used by the rate limiter (requests per one-second window).
#[derive(Debug, Default)]
pub struct RateLimiter {
    windows: std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, u32)>>,
}

/// Max state-changing Play On requests per user per second.
pub const RATE_LIMIT_PER_SEC: u32 = 20;

impl RateLimiter {
    /// Records a request; false when the user exceeded the limit in the current window.
    pub fn allow(&self, user_id: &str) -> bool {
        let now = std::time::Instant::now();
        let mut w = self.windows.lock().unwrap_or_else(|p| p.into_inner());
        if w.len() > 10_000 {
            w.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(1));
        }
        let entry = w.entry(user_id.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= Duration::from_secs(1) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= RATE_LIMIT_PER_SEC
    }
}

/// Starts Play On for `db` in the background (called after setup and after a database switch).
pub fn start_in_background(manager: RemoteManager, db: DbPool, parent: CancellationToken) {
    tokio::spawn(async move { manager.start(db, &parent).await });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upnp::device::DeviceIcon;
    use registry::{tests::description, RendererCaps};

    fn conn_with_settings(pairs: &[(&str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        for (k, v) in pairs {
            conn.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)",
                rusqlite::params![k, v],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn settings_defaults_and_overrides() {
        assert_eq!(
            read_remote_settings(&conn_with_settings(&[])),
            RemoteSettings::default()
        );
        let s = read_remote_settings(&conn_with_settings(&[
            ("remotePlaybackEnabled", "false"),
            ("remoteInterfaceIp", "192.168.1.4"),
            ("remotePollMs", "99999"),
            ("remoteReleaseStopsRenderer", "false"),
        ]));
        assert!(!s.enabled);
        assert_eq!(s.interface_ip, Some(Ipv4Addr::new(192, 168, 1, 4)));
        assert_eq!(s.poll_ms, 5000);
        assert!(!s.release_stops_renderer);
        let bad = read_remote_settings(&conn_with_settings(&[
            ("remoteInterfaceIp", ""),
            ("remotePollMs", "x"),
        ]));
        assert_eq!(bad.interface_ip, None);
        assert_eq!(bad.poll_ms, DEFAULT_POLL_MS);
    }

    fn pool(prefix: &str) -> DbPool {
        let path = crate::test_support::temp_db_path(prefix);
        let state = crate::test_support::build_test_state(&path);
        state.db.unwrap()
    }

    #[tokio::test]
    async fn start_respects_disabled_setting_and_stop_is_idempotent() {
        let db = pool("remote-disabled");
        {
            let conn = db.lock().unwrap();
            boogiebox_db::upsert_setting(&conn, "remotePlaybackEnabled", "false").unwrap();
        }
        let m = RemoteManager::new();
        m.start(db, &CancellationToken::new()).await;
        assert!(!m.is_running());
        assert!(m.running_settings().is_none());
        assert!(!m.refresh());
        m.stop();
        assert!(format!("{m:?}").contains("running: false"));
    }

    #[tokio::test]
    async fn start_loads_persisted_renderers_and_handles_discovery_events() {
        let db = pool("remote-start");
        {
            let conn = db.lock().unwrap();
            boogiebox_db::upsert_setting(&conn, "remoteInterfaceIp", "127.0.0.1").unwrap();
            let caps = serde_json::to_string(&StoredCapabilities {
                description: description("old", "10.0.0.8"),
                caps: RendererCaps::default(),
            })
            .unwrap();
            boogiebox_db::remote::upsert_renderer(
                &conn,
                &boogiebox_db::remote::RendererUpsert {
                    udn: "old",
                    friendly_name: "Old",
                    manufacturer: None,
                    model_name: None,
                    vendor: "generic",
                    location_url: "http://10.0.0.8/d",
                    capabilities: &caps,
                },
            )
            .unwrap();
        }
        let m = RemoteManager::default();
        let mut events = m.subscribe();
        let parent = CancellationToken::new();
        m.start(db.clone(), &parent).await;
        assert!(m.is_running());
        assert!(m.refresh());
        assert_eq!(
            m.running_settings().unwrap().interface_ip,
            Some(Ipv4Addr::LOCALHOST)
        );
        assert!(m.registry().get("old").is_some());
        assert!(!m.registry().is_online("old"));
        assert_eq!(events.recv().await.unwrap(), RemoteEvent::Renderers);

        // A byebye for an offline device changes nothing; an unreachable location is ignored.
        let in_flight = Arc::default();
        m.handle_discovery(DiscoveryEvent::Gone { udn: "old".into() }, &db, &in_flight);
        m.describe_and_store("http://127.0.0.1:9/d.xml", 60, &db)
            .await;
        assert!(m.registry().get("127.0.0.1").is_none());

        parent.cancel();
        m.stop();
        assert!(!m.is_running());
        assert!(m.registry().snapshot().is_empty());
    }

    #[tokio::test]
    async fn seen_event_describes_persists_and_serves_icon() {
        use axum::{
            routing::{get, post},
            Router,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let device = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Den</friendlyName><UDN>uuid:DEN</UDN>
<iconList><icon><mimetype>image/png</mimetype><width>48</width><height>48</height><url>/i.png</url></icon></iconList>
<serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/avt</controlURL></service></serviceList></device></root>"#;
        let app = Router::new()
            .route("/d.xml", get(move || async move { device }))
            .route("/i.png", get(|| async { vec![137u8, 80, 78, 71] }))
            .route("/avt", post(|| async { "" }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let db = pool("remote-seen");
        let m = RemoteManager::new();
        let mut events = m.subscribe();
        let in_flight = Arc::default();
        m.handle_discovery(
            DiscoveryEvent::Seen {
                location: format!("http://{addr}/d.xml"),
                udn: "den".into(),
                max_age: 1800,
            },
            &db,
            &in_flight,
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
            RemoteEvent::Renderers
        );
        assert!(m.registry().is_online("den"));
        let row = boogiebox_db::remote::get_renderer(&db.lock().unwrap(), "den")
            .unwrap()
            .unwrap();
        assert_eq!(row.friendly_name, "Den");
        assert!(serde_json::from_str::<StoredCapabilities>(&row.capabilities).is_ok());

        let (mime, bytes) = m.icon("den").await.unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes.len(), 4);
        assert!(m.icon("den").await.is_some(), "cached");
        assert!(m.icon("nobody").await.is_none());

        // Re-seen soon after: no new description fetch, persisted touch is debounced.
        m.handle_discovery(
            DiscoveryEvent::Seen {
                location: format!("http://{addr}/d.xml"),
                udn: "den".into(),
                max_age: 1800,
            },
            &db,
            &in_flight,
        );
        assert!(m.handle_gone_for_test("den"));
    }

    /// Live LAN check (read-only: M-SEARCH, description GETs, GetProtocolInfo). Run manually:
    /// `cargo test -p boogiebox-server --lib live_lan_discovery -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "needs a UPnP renderer on the LAN"]
    async fn live_lan_discovery_finds_renderers() {
        let db = pool("remote-live");
        let m = RemoteManager::new();
        let mut events = m.subscribe();
        let parent = CancellationToken::new();
        m.start(db, &parent).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < deadline {
            if tokio::time::timeout(Duration::from_secs(1), events.recv())
                .await
                .is_ok()
                && m.registry().snapshot().iter().any(|e| e.online)
            {
                break;
            }
        }
        let found = m.registry().snapshot();
        for e in &found {
            println!(
                "{} ({:?}) online={} setNext={} seek={} flac={} rc={}",
                e.description.friendly_name,
                e.description.vendor,
                e.online,
                e.caps.set_next,
                e.caps.seek,
                e.caps.sink.accepts("audio/flac"),
                e.description.rendering_control.is_some()
            );
        }
        parent.cancel();
        assert!(found.iter().any(|e| e.online), "no renderer found");
    }

    /// Two scripted SOAP renderers + a real DB track: sessions over the real link, takeover, transfer, release.
    #[tokio::test]
    async fn manager_sessions_over_soap_transfer_and_release() {
        use crate::upnp::soap::tests::spawn_mock;
        use session::{Command, Controller, SessionError, SessionStatus};

        let ok = |action: &str, inner: &str| {
            format!(
                r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:x">{inner}</u:{action}Response></s:Body></s:Envelope>"#
            )
        };
        let responder = move |action: &str, _body: &str| {
            match action {
            "GetTransportInfo" => (200, ok(action, "<CurrentTransportState>TRANSITIONING</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus>")),
            "GetPositionInfo" => (200, ok(action, "<TrackURI></TrackURI><RelTime>0:00:00</RelTime><TrackDuration>0:00:00</TrackDuration>")),
            "GetVolume" => (200, ok(action, "<CurrentVolume>25</CurrentVolume>")),
            "GetMute" => (200, ok(action, "<CurrentMute>0</CurrentMute>")),
            _ => (200, ok(action, "")),
        }
        };
        let (url_a, log_a) = spawn_mock(responder).await;
        let (url_b, log_b) = spawn_mock(responder).await;

        let db = pool("remote-sessions");
        let dir = std::env::temp_dir().join(format!("bb-sessions-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.flac");
        std::fs::write(&file, b"flac").unwrap();
        {
            let conn = db.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES('lib1', 'D:\\Music', 'Music');",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tracks(id, library_id, file_path, title, duration) VALUES('t1', 'lib1', ?1, 'Angel', 300)",
                rusqlite::params![file.to_string_lossy()],
            )
            .unwrap();
        }
        let m = RemoteManager::new();
        let mut events = m.subscribe();
        for (udn, url) in [("a", &url_a), ("b", &url_b)] {
            let mut d = description(udn, "127.0.0.1");
            d.av_transport.control_url = url.clone();
            d.rendering_control = Some(d.av_transport.clone());
            m.registry().insert_described(
                d,
                RendererCaps {
                    sink: crate::upnp::device::SinkCaps::parse("http-get:*:audio/flac:*"),
                    set_next: true,
                    seek: true,
                },
                "http://127.0.0.1:1/d.xml",
                60,
                Instant::now(),
            );
        }
        let alice = Controller {
            user_id: "alice".into(),
            username: "Alice".into(),
        };
        let bob = Controller {
            user_id: "bob".into(),
            username: "Bob".into(),
        };
        let load = || Command::Load {
            items: vec![queue::QueueItem::Track {
                id: "t1".into(),
                title: "Angel".into(),
                artist: None,
                album: None,
                album_id: None,
                duration: Some(300.0),
                radio_reason: None,
            }],
            start_index: 0,
            start_at: 42.0,
            queue_source: None,
            shuffle: None,
            repeat: None,
            resumed: false,
        };

        assert!(matches!(
            m.command(&db, "zzz", Command::Play, alice.clone(), false)
                .await,
            Err(SessionError::BadRequest(_))
        ));
        assert_eq!(
            m.command(
                &db,
                "a",
                Command::Release { stop: true },
                alice.clone(),
                false
            )
            .await,
            Err(SessionError::Ended),
            "release without a session"
        );

        let s = m
            .command(&db, "a", load(), alice.clone(), false)
            .await
            .unwrap();
        assert_eq!(s.status, SessionStatus::Loading);
        assert_eq!(s.volume, Some(25), "initial volume read");
        let set_uri = log_a
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| a == "SetAVTransportURI")
            .map(|(_, b)| b.clone())
            .unwrap();
        assert!(set_uri.contains("/media/r/"), "real media token URL sent");
        assert!(
            set_uri.contains("&lt;dc:title&gt;Angel&lt;/dc:title&gt;"),
            "double-escaped DIDL"
        );
        assert_eq!(m.session_snapshots().len(), 1);
        assert!(!m.media().is_empty());

        // Bob needs to confirm; transfer by Bob without takeover is refused the same way.
        assert!(matches!(
            m.command(&db, "a", Command::Pause, bob.clone(), false)
                .await,
            Err(SessionError::InUse(_))
        ));
        assert!(matches!(
            m.transfer(&db, "a", "b", bob.clone(), false).await,
            Err(SessionError::InUse(_))
        ));
        assert!(matches!(
            m.transfer(&db, "a", "a", alice.clone(), false).await,
            Err(SessionError::BadRequest(_))
        ));
        assert!(matches!(
            m.transfer(&db, "b", "a", alice.clone(), false).await,
            Err(SessionError::Ended)
        ));

        // Alice moves playback to b at the same position; a is stopped and its session ends.
        let moved = m
            .transfer(&db, "a", "b", alice.clone(), false)
            .await
            .unwrap();
        assert_eq!(moved.udn, "b");
        assert!(moved.position_sec >= 42.0);
        assert!(log_b
            .lock()
            .unwrap()
            .iter()
            .any(|(a, _)| a == "SetAVTransportURI"));
        assert!(log_a.lock().unwrap().iter().any(|(a, _)| a == "Stop"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(m.session("a").is_none(), "source session ended");
        assert_eq!(m.session_snapshots().len(), 1);

        let mut saw_ended = false;
        while let Ok(e) = events.try_recv() {
            saw_ended |= matches!(e, RemoteEvent::SessionEnded { ref udn } if udn == "a");
        }
        assert!(saw_ended);

        // Offline renderers can't start a session.
        m.registry().mark_gone("a");
        assert!(matches!(
            m.command(&db, "a", load(), alice.clone(), false).await,
            Err(SessionError::Renderer(_))
        ));

        // Stopping Play On ends every session and revokes tokens.
        m.stop();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(m.session("b").is_none());
        assert_eq!(m.media().len(), 0);
    }

    #[test]
    fn non_image_icons_are_refused() {
        let m = RemoteManager::new();
        let mut d = description("x", "10.0.0.9");
        d.icons.push(DeviceIcon {
            mime: "image/svg+xml".into(),
            width: 48,
            height: 48,
            url: "http://10.0.0.9/i.svg".into(),
        });
        m.registry().insert_described(
            d,
            RendererCaps::default(),
            "http://10.0.0.9/d",
            60,
            Instant::now(),
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(rt.block_on(m.icon("x")).is_none());
    }

    impl RemoteManager {
        fn handle_gone_for_test(&self, udn: &str) -> bool {
            let was = self.registry().is_online(udn);
            self.handle_discovery(
                DiscoveryEvent::Gone { udn: udn.into() },
                &pool("remote-gone"),
                &Arc::default(),
            );
            was && !self.registry().is_online(udn)
        }
    }
}

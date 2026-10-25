//! Play events shared by every player: the BoogieBox web player (`/api/tracks/{id}/played`, `/scrobble`),
//! Play On sessions and Subsonic apps (`scrobble`). Starting a track updates the "now playing" list
//! (`getNowPlaying`) and tells connected scrobbling services; a finished play is queued for them.
//! Play counts themselves are recorded by the callers (`play_history`; per-user counts follow by trigger).

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rand::Rng;
use tokio::sync::Notify;

use crate::{activity::ActivityHub, subsonic::SubsonicState, DbPool, SharedState};

/// How long a "now playing" notice stays listed without a refresh.
const NOW_PLAYING_TTL: Duration = Duration::from_secs(600);
const MAX_NOW_PLAYING: usize = 64;
/// How long a Last.fm sign-in may take.
pub const LASTFM_STATE_TTL: Duration = Duration::from_secs(600);
const MAX_LASTFM_STATES: usize = 256;
/// Client name shown for the BoogieBox web player.
pub const WEB_CLIENT: &str = "BoogieBox";

/// Which kind of player reported a play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaySource {
    /// The BoogieBox web player.
    Web,
    /// A Play On speaker session.
    PlayOn,
    /// A Subsonic app.
    Subsonic,
}

/// Someone's player reporting what it is playing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlayingEntry {
    pub user_id: String,
    pub username: String,
    /// Player name: the Subsonic `c` parameter, "BoogieBox" or a Play On speaker's name.
    pub client: String,
    /// Song id (`mx-…` for a BoogieMix mix).
    pub track_id: String,
    pub source: PlaySource,
    pub at: Instant,
}

#[derive(Debug, Default)]
struct Inner {
    now_playing: Mutex<Vec<NowPlayingEntry>>,
    lastfm_states: Mutex<HashMap<String, (String, Instant)>>,
    wake: Notify,
}

/// Shared, cheaply cloneable play state kept in `AppState`.
#[derive(Debug, Clone, Default)]
pub struct PlayHub(Arc<Inner>);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl PlayHub {
    /// Forgets everything (database switched).
    pub fn reset(&self) {
        lock(&self.0.now_playing).clear();
        lock(&self.0.lastfm_states).clear();
    }

    /// Records what a player is playing (one entry per user + player).
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

    /// A one-time, unguessable value naming `user_id` for the Last.fm sign-in callback.
    pub fn issue_lastfm_state(&self, user_id: &str) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let state = hex::encode(bytes);
        let mut states = lock(&self.0.lastfm_states);
        states.retain(|_, (_, at)| at.elapsed() < LASTFM_STATE_TTL);
        if states.len() >= MAX_LASTFM_STATES {
            states.clear();
        }
        states.insert(state.clone(), (user_id.to_owned(), Instant::now()));
        state
    }

    /// The user a sign-in state was issued to; each state works once and expires.
    pub fn take_lastfm_state(&self, state: &str) -> Option<String> {
        let (user, at) = lock(&self.0.lastfm_states).remove(state)?;
        (at.elapsed() < LASTFM_STATE_TTL).then_some(user)
    }

    /// Wakes the scrobble forwarder (new plays were queued).
    pub fn wake(&self) {
        self.0.wake.notify_one();
    }

    /// Resolves when [`PlayHub::wake`] is called.
    pub async fn woken(&self) {
        self.0.wake.notified().await;
    }
}

/// Everything a play event needs, taken from `AppState` once.
#[derive(Clone)]
pub struct PlayEnv {
    pub db: DbPool,
    pub http: reqwest::Client,
    pub hub: PlayHub,
    pub subsonic: SubsonicState,
    pub db_folder: Option<PathBuf>,
    pub activity: ActivityHub,
}

impl PlayEnv {
    /// `None` before setup (no database).
    pub fn from_state(state: &SharedState) -> Option<Self> {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        Some(PlayEnv {
            db: s.db.clone()?,
            http: s.http_client.clone(),
            hub: s.plays.clone(),
            subsonic: s.subsonic.clone(),
            db_folder: s.db_folder.clone(),
            activity: s.activity.clone(),
        })
    }
}

/// A player started `track_id`: list it as now playing, add it to the activity history and tell the user's
/// scrobbling services (best effort, in the background).
pub fn play_started(
    env: &PlayEnv,
    source: PlaySource,
    user_id: &str,
    username: &str,
    track_id: &str,
    client: &str,
) {
    let client: String = client.chars().take(64).collect();
    env.activity
        .record_play(source, user_id, username, &client, track_id);
    env.hub.set_now_playing(NowPlayingEntry {
        user_id: user_id.to_owned(),
        username: username.to_owned(),
        client,
        track_id: track_id.to_owned(),
        source,
        at: Instant::now(),
    });
    if crate::subsonic::model::mix_id(track_id).is_some() {
        return;
    }
    let (env, user, track) = (env.clone(), user_id.to_owned(), track_id.to_owned());
    tokio::spawn(async move {
        crate::scrobbling::forward_now_playing(
            &env,
            &crate::scrobbling::Endpoints::production(),
            &user,
            &track,
        )
        .await;
    });
}

/// A play counts (played long enough): queue it for the user's scrobbling services. `played_at` is the Unix
/// start time. Blocking work runs off the async runtime.
pub async fn play_finished(env: &PlayEnv, user_id: &str, track_id: &str, played_at: i64) {
    let (db, user, track) = (env.db.clone(), user_id.to_owned(), track_id.to_owned());
    let queued = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        crate::scrobbling::enqueue_play(&conn, &user, &track, played_at, unix_now())
    })
    .await
    .unwrap_or(0);
    if queued > 0 {
        env.hub.wake();
    }
}

/// Current Unix time in seconds.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Seconds a track must play before it counts as a scrobble: half its length or 4 minutes, whichever is
/// less. `None` for tracks of 30 s or less (Last.fm rule) or with no known length.
pub fn scrobble_threshold(duration_secs: Option<f64>) -> Option<f64> {
    let d = duration_secs.filter(|d| d.is_finite())?;
    (d > 30.0).then(|| (d / 2.0).min(240.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(user: &str, client: &str, track: &str) -> NowPlayingEntry {
        NowPlayingEntry {
            user_id: user.into(),
            username: user.into(),
            client: client.into(),
            track_id: track.into(),
            source: PlaySource::Subsonic,
            at: Instant::now(),
        }
    }

    #[test]
    fn now_playing_keeps_one_entry_per_user_and_client() {
        let hub = PlayHub::default();
        hub.set_now_playing(entry("u1", "phone", "t1"));
        hub.set_now_playing(entry("u1", "phone", "t2"));
        hub.set_now_playing(entry("u1", "desk", "t3"));
        hub.set_now_playing(entry("u2", "phone", "t4"));
        let tracks: Vec<String> = hub.now_playing().into_iter().map(|e| e.track_id).collect();
        assert_eq!(tracks, ["t4", "t3", "t2"]);
        for i in 0..(MAX_NOW_PLAYING + 5) {
            hub.set_now_playing(entry(&format!("x{i}"), "c", "t"));
        }
        assert_eq!(hub.now_playing().len(), MAX_NOW_PLAYING);
        hub.reset();
        assert!(hub.now_playing().is_empty());
    }

    #[test]
    fn lastfm_states_are_single_use_and_bound_to_a_user() {
        let hub = PlayHub::default();
        let a = hub.issue_lastfm_state("u1");
        let b = hub.issue_lastfm_state("u2");
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(hub.take_lastfm_state(&a).as_deref(), Some("u1"));
        assert_eq!(hub.take_lastfm_state(&a), None, "one use only");
        assert_eq!(hub.take_lastfm_state("forged"), None);
        hub.reset();
        assert_eq!(hub.take_lastfm_state(&b), None);
        for i in 0..(MAX_LASTFM_STATES + 1) {
            hub.issue_lastfm_state(&format!("u{i}"));
        }
        assert!(lock(&hub.0.lastfm_states).len() <= MAX_LASTFM_STATES);
    }

    #[test]
    fn thresholds_follow_the_scrobbling_rules() {
        assert_eq!(scrobble_threshold(Some(30.0)), None);
        assert_eq!(scrobble_threshold(None), None);
        assert_eq!(scrobble_threshold(Some(f64::NAN)), None);
        assert_eq!(scrobble_threshold(Some(100.0)), Some(50.0));
        assert_eq!(scrobble_threshold(Some(1200.0)), Some(240.0));
    }

    #[tokio::test]
    async fn wake_resolves_a_waiter() {
        let hub = PlayHub::default();
        hub.wake();
        tokio::time::timeout(Duration::from_secs(1), hub.woken())
            .await
            .expect("stored permit wakes the next waiter");
    }
}

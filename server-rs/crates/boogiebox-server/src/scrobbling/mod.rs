//! Scrobble forwarding to each user's own Last.fm and ListenBrainz accounts (plan §15.5, 2.5-3).
//! Plays are queued in `scrobble_queue` and sent by a background forwarder, outside the database lock, with
//! retries. "Now playing" is sent directly (best effort). Account secrets are AES-256-GCM encrypted with the
//! data-folder key Subsonic passwords use, bound to user and service.

pub mod lastfm;
pub mod listenbrainz;

use std::{collections::BTreeMap, time::Duration};

use boogiebox_db::scrobble::{self as qdb, QueuedScrobble, ScrobbleService, ScrobbleTrack};
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;

use crate::plays::{unix_now, PlayEnv};

/// How often the forwarder looks for due plays when nothing wakes it.
const TICK: Duration = Duration::from_secs(30);
/// Most queued plays handled per pass.
const PASS_LIMIT: i64 = 500;
/// Longest wait between retries.
const MAX_BACKOFF_SECS: i64 = 6 * 3600;
/// Longest a "now playing" notice may take.
const NOW_PLAYING_BUDGET: Duration = Duration::from_secs(5);

/// Why a send failed, and therefore what happens to the play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The service rejected the user's credentials: the account needs reconnecting (plays are kept).
    Auth,
    /// Try again later.
    Temporary,
    /// The service refused these plays: drop them.
    Rejected,
}

/// Service API roots (tests point them at mock servers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub lastfm: String,
    pub listenbrainz: String,
}

impl Endpoints {
    pub fn production() -> Self {
        Endpoints {
            lastfm: crate::providers::lastfm_api_base(),
            listenbrainz: listenbrainz::api_base(),
        }
    }
}

fn aad(user_id: &str, service: ScrobbleService) -> String {
    format!("scrobble:{user_id}:{}", service.as_str())
}

/// Encrypts an account secret for storage.
pub fn seal(
    key: &[u8; 32],
    user_id: &str,
    service: ScrobbleService,
    secret: &str,
) -> Option<(Vec<u8>, Vec<u8>)> {
    crate::subsonic::crypto::encrypt(key, &aad(user_id, service), secret)
}

/// Decrypts a stored account secret.
pub fn open(key: &[u8; 32], account: &qdb::ScrobbleAccount) -> Option<String> {
    crate::subsonic::crypto::decrypt(
        key,
        &aad(&account.user_id, account.service),
        &account.secret_enc,
        &account.secret_nonce,
    )
}

/// The admin's Last.fm app credentials, when both key and shared secret are set.
pub fn lastfm_app(conn: &Connection) -> Option<lastfm::AppCredentials> {
    let get = |k: &str| {
        boogiebox_db::artwork::get_setting(conn, k)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    Some(lastfm::AppCredentials {
        api_key: get("lastfmKey")?,
        secret: get("lastfmSecret")?,
    })
}

/// Queues a finished play for each of the user's active services; returns how many were queued.
pub fn enqueue_play(
    conn: &Connection,
    user_id: &str,
    track_id: &str,
    played_at: i64,
    now: i64,
) -> usize {
    let services = match qdb::active_services(conn, user_id) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return 0,
        Err(err) => {
            tracing::warn!(error = %err, "Could not read scrobble accounts");
            return 0;
        }
    };
    let Ok(Some(track)) = qdb::track_meta(conn, track_id) else {
        return 0;
    };
    services
        .into_iter()
        .filter(|s| match qdb::enqueue(conn, user_id, *s, &track, played_at, now) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(error = %err, service = s.as_str(), "Could not queue a scrobble");
                false
            }
        })
        .count()
}

/// Accounts with their decrypted secrets, read in one blocking task (one DB hold, then the key, outside the lock).
struct Unlocked {
    accounts: Vec<(qdb::ScrobbleAccount, String)>,
    lastfm: Option<lastfm::AppCredentials>,
}

async fn unlock<F>(env: &PlayEnv, select: F) -> Option<Unlocked>
where
    F: FnOnce(&Connection) -> rusqlite::Result<Vec<qdb::ScrobbleAccount>> + Send + 'static,
{
    let env = env.clone();
    tokio::task::spawn_blocking(move || {
        let (accounts, lastfm) = {
            let conn = env.db.lock().unwrap_or_else(|p| p.into_inner());
            match select(&conn) {
                Ok(a) => (a, lastfm_app(&conn)),
                Err(err) => {
                    tracing::warn!(error = %err, "Could not read scrobble accounts");
                    return None;
                }
            }
        };
        if accounts.is_empty() {
            return Some(Unlocked {
                accounts: Vec::new(),
                lastfm,
            });
        }
        // Only now touch the key file (it is created on first use; never for users without accounts).
        let key = env.subsonic.key(env.db_folder.as_deref(), &env.db)?;
        Some(Unlocked {
            accounts: accounts
                .into_iter()
                .filter_map(|a| open(&key, &a).map(|secret| (a, secret)))
                .collect(),
            lastfm,
        })
    })
    .await
    .ok()
    .flatten()
}

/// Tells the user's services that `track_id` started (best effort, bounded time).
pub async fn forward_now_playing(
    env: &PlayEnv,
    endpoints: &Endpoints,
    user_id: &str,
    track_id: &str,
) {
    let (user, track_id) = (user_id.to_owned(), track_id.to_owned());
    let track_lookup = track_id.clone();
    let Some(unlocked) = unlock(env, move |conn| {
        let mut accounts = Vec::new();
        for service in qdb::active_services(conn, &user)? {
            if let Some(a) = qdb::get_account(conn, &user, service)? {
                accounts.push(a);
            }
        }
        Ok(accounts)
    })
    .await
    else {
        return;
    };
    if unlocked.accounts.is_empty() {
        return;
    }
    let db = env.db.clone();
    let track = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        qdb::track_meta(&conn, &track_lookup).ok().flatten()
    })
    .await
    .ok()
    .flatten();
    let Some(track) = track else { return };
    for (account, secret) in &unlocked.accounts {
        let send = async {
            match account.service {
                ScrobbleService::LastFm => match &unlocked.lastfm {
                    Some(app) => {
                        lastfm::update_now_playing(
                            &env.http,
                            &endpoints.lastfm,
                            app,
                            secret,
                            &track,
                        )
                        .await
                    }
                    None => Err(SendError::Temporary),
                },
                ScrobbleService::ListenBrainz => {
                    listenbrainz::playing_now(&env.http, &endpoints.listenbrainz, secret, &track)
                        .await
                }
            }
        };
        if let Ok(Err(err)) = tokio::time::timeout(NOW_PLAYING_BUDGET, send).await {
            tracing::debug!(
                service = account.service.as_str(),
                ?err,
                "Now playing not sent"
            );
        }
    }
}

/// Retry delay after `attempts` failures: one minute, doubling, at most six hours.
pub fn backoff_secs(attempts: i64) -> i64 {
    let shift = attempts.clamp(0, 20) as u32;
    60i64.saturating_mul(1i64 << shift).min(MAX_BACKOFF_SECS)
}

/// What one delivery pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PassReport {
    pub sent: usize,
    pub deferred: usize,
    pub dropped: usize,
    pub reconnect: usize,
}

async fn send_batch(
    env: &PlayEnv,
    endpoints: &Endpoints,
    service: ScrobbleService,
    secret: &str,
    app: Option<&lastfm::AppCredentials>,
    plays: &[(ScrobbleTrack, i64)],
) -> Result<(), SendError> {
    match service {
        ScrobbleService::LastFm => match app {
            Some(app) => lastfm::scrobble(&env.http, &endpoints.lastfm, app, secret, plays).await,
            None => Err(SendError::Temporary),
        },
        ScrobbleService::ListenBrainz => {
            listenbrainz::submit_listens(&env.http, &endpoints.listenbrainz, secret, plays).await
        }
    }
}

/// Sends every due play (grouped per account, in service-sized batches) and records the outcome.
pub async fn deliver_due(env: &PlayEnv, endpoints: &Endpoints) -> PassReport {
    let now = unix_now();
    let db = env.db.clone();
    let due = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(err) = qdb::prune(&conn, now) {
            tracing::warn!(error = %err, "Could not prune the scrobble queue");
        }
        qdb::due(&conn, now, PASS_LIMIT).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let mut report = PassReport::default();
    if due.is_empty() {
        return report;
    }
    let mut groups: BTreeMap<(String, &'static str), Vec<QueuedScrobble>> = BTreeMap::new();
    for row in due {
        groups
            .entry((row.user_id.clone(), row.service.as_str()))
            .or_default()
            .push(row);
    }
    let keys: Vec<(String, ScrobbleService)> = groups
        .values()
        .filter_map(|rows| rows.first().map(|r| (r.user_id.clone(), r.service)))
        .collect();
    let Some(unlocked) = unlock(env, move |conn| {
        let mut accounts = Vec::new();
        for (user, service) in &keys {
            if let Some(a) = qdb::get_account(conn, user, *service)? {
                accounts.push(a);
            }
        }
        Ok(accounts)
    })
    .await
    else {
        return report;
    };

    let mut sent_ids = Vec::new();
    let mut dropped_ids = Vec::new();
    let mut deferred: Vec<(i64, i64)> = Vec::new();
    let mut reconnect = Vec::new();
    let mut delivered = Vec::new();
    for rows in groups.values() {
        let Some(first) = rows.first() else { continue };
        let Some((_, secret)) = unlocked
            .accounts
            .iter()
            .find(|(a, _)| a.user_id == first.user_id && a.service == first.service)
        else {
            // Secret unreadable (key replaced): the rows go when the account is cleared.
            continue;
        };
        let batch_size = match first.service {
            ScrobbleService::LastFm => lastfm::MAX_BATCH,
            ScrobbleService::ListenBrainz => listenbrainz::MAX_BATCH,
        };
        for chunk in rows.chunks(batch_size) {
            let plays: Vec<(ScrobbleTrack, i64)> = chunk
                .iter()
                .map(|r| (r.track.clone(), r.played_at))
                .collect();
            let ids = chunk.iter().map(|r| r.id);
            match send_batch(
                env,
                endpoints,
                first.service,
                secret,
                unlocked.lastfm.as_ref(),
                &plays,
            )
            .await
            {
                Ok(()) => {
                    sent_ids.extend(ids);
                    delivered.push((first.user_id.clone(), first.service));
                }
                Err(SendError::Rejected) => dropped_ids.extend(ids),
                Err(SendError::Temporary) => {
                    deferred.extend(chunk.iter().map(|r| (r.id, now + backoff_secs(r.attempts))))
                }
                Err(SendError::Auth) => {
                    reconnect.push((first.user_id.clone(), first.service));
                    break;
                }
            }
        }
    }
    report.sent = sent_ids.len();
    report.dropped = dropped_ids.len();
    report.deferred = deferred.len();
    report.reconnect = reconnect.len();
    let db = env.db.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let _ = qdb::remove(&conn, &sent_ids);
        let _ = qdb::remove(&conn, &dropped_ids);
        for (id, next) in deferred {
            let _ = qdb::defer(&conn, &[id], next);
        }
        for (user, service) in reconnect {
            let _ = qdb::mark_needs_reconnect(&conn, &user, service);
        }
        delivered.dedup();
        for (user, service) in delivered {
            let _ = qdb::mark_delivered(&conn, &user, service);
        }
    })
    .await;
    if report.reconnect > 0 {
        tracing::warn!(
            accounts = report.reconnect,
            "A scrobbling service rejected an account; it needs reconnecting"
        );
    }
    report
}

/// Runs the forwarder until `cancel` fires (database switch or shutdown).
pub fn start_forwarder(env: PlayEnv, cancel: CancellationToken) {
    tokio::spawn(async move {
        let endpoints = Endpoints::production();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(TICK) => {}
                _ = env.hub.woken() => {}
            }
            deliver_due(&env, &endpoints).await;
        }
    });
}

// ── Connecting accounts ────────────────────────────────────────────────────────

/// Why connecting an account failed (shown to the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectError {
    /// The service did not accept the token / sign-in.
    Rejected,
    /// The service could not be reached.
    Unavailable,
    /// The Last.fm key and shared secret are not both set.
    NotConfigured,
    /// The sign-in link expired or was already used.
    Expired,
    /// The encryption key or database failed.
    Internal,
}

impl ConnectError {
    pub fn message(self) -> &'static str {
        match self {
            ConnectError::Rejected => "The service did not accept this account",
            ConnectError::Unavailable => "The service could not be reached. Try again later",
            ConnectError::NotConfigured => "Last.fm is not set up on this server",
            ConnectError::Expired => "This sign-in link has expired. Start again from Settings",
            ConnectError::Internal => "The account could not be saved",
        }
    }
}

fn connect_error(err: SendError) -> ConnectError {
    match err {
        SendError::Auth | SendError::Rejected => ConnectError::Rejected,
        SendError::Temporary => ConnectError::Unavailable,
    }
}

async fn store_account(
    env: &PlayEnv,
    user_id: &str,
    service: ScrobbleService,
    secret: String,
    name: Option<String>,
) -> Result<(), ConnectError> {
    let (env, user) = (env.clone(), user_id.to_owned());
    tokio::task::spawn_blocking(move || {
        let key = env
            .subsonic
            .key(env.db_folder.as_deref(), &env.db)
            .ok_or(ConnectError::Internal)?;
        let (enc, nonce) = seal(&key, &user, service, &secret).ok_or(ConnectError::Internal)?;
        let conn = env.db.lock().unwrap_or_else(|p| p.into_inner());
        qdb::upsert_account(&conn, &user, service, &enc, &nonce, name.as_deref()).map_err(|err| {
            tracing::error!(error = %err, "Could not save a scrobble account");
            ConnectError::Internal
        })
    })
    .await
    .map_err(|_| ConnectError::Internal)?
}

/// Validates a ListenBrainz token and stores it; returns the ListenBrainz user name.
pub async fn connect_listenbrainz(
    env: &PlayEnv,
    endpoints: &Endpoints,
    user_id: &str,
    token: &str,
) -> Result<String, ConnectError> {
    let token = token.trim();
    if !listenbrainz::plausible_token(token) {
        return Err(ConnectError::Rejected);
    }
    let name = listenbrainz::validate_token(&env.http, &endpoints.listenbrainz, token)
        .await
        .map_err(connect_error)?;
    store_account(
        env,
        user_id,
        ScrobbleService::ListenBrainz,
        token.to_owned(),
        Some(name.clone()).filter(|n| !n.is_empty()),
    )
    .await?;
    Ok(name)
}

/// `scheme://host[:port]` with nothing after it.
pub fn valid_origin(origin: &str) -> bool {
    let Some((_, rest)) = origin.split_once("://") else {
        return false;
    };
    boogiebox_db::radio_stations::valid_http_url(origin) && !rest.contains(['/', '?', '#', '@'])
}

/// Starts the Last.fm sign-in: the page to open, with a one-time callback state for `user_id`.
pub async fn begin_lastfm_connect(
    env: &PlayEnv,
    user_id: &str,
    origin: &str,
) -> Result<String, ConnectError> {
    let origin = origin.trim().trim_end_matches('/');
    if !valid_origin(origin) {
        return Err(ConnectError::Rejected);
    }
    let db = env.db.clone();
    let app = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        lastfm_app(&conn)
    })
    .await
    .map_err(|_| ConnectError::Internal)?
    .ok_or(ConnectError::NotConfigured)?;
    let state = env.hub.issue_lastfm_state(user_id);
    let callback = format!("{origin}/api/scrobble/lastfm/callback?state={state}");
    Ok(lastfm::auth_url(&app.api_key, &callback))
}

/// Finishes the Last.fm sign-in from the callback (`state` names the user; it works once).
pub async fn finish_lastfm_connect(
    env: &PlayEnv,
    endpoints: &Endpoints,
    state: &str,
    token: &str,
) -> Result<String, ConnectError> {
    let user = env
        .hub
        .take_lastfm_state(state)
        .ok_or(ConnectError::Expired)?;
    let token = token.trim();
    // Last.fm callback tokens are URL-safe: letters, digits, `-` and `_`.
    if token.is_empty()
        || token.len() > 128
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ConnectError::Rejected);
    }
    let db = env.db.clone();
    let app = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        lastfm_app(&conn)
    })
    .await
    .map_err(|_| ConnectError::Internal)?
    .ok_or(ConnectError::NotConfigured)?;
    let (session_key, name) = lastfm::get_session(&env.http, &endpoints.lastfm, &app, token)
        .await
        .map_err(connect_error)?;
    store_account(
        env,
        &user,
        ScrobbleService::LastFm,
        session_key,
        Some(name.clone()).filter(|n| !n.is_empty()),
    )
    .await?;
    Ok(name)
}

#[cfg(test)]
mod tests;

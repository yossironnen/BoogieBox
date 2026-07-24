//! Defines Rust server support logic for Auth.

use axum::{
    extract::{FromRef, FromRequestParts},
    http::StatusCode,
};
use axum_extra::extract::CookieJar;
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde::Serialize;
use sha2::Sha512;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{DbPool, ErrorResponse, SharedState};

// -- PIN hashing --------------------------------------------------------------

const PBKDF2_ITERATIONS: u32 = 600_000;
const HASH_OUTPUT_LEN: usize = 64;
const SALT_LEN: usize = 32;

/// Documents the Hash Pin public API surface.
pub fn hash_pin(pin: &str) -> String {
    let mut rng = rand::rng();
    let mut salt = [0u8; SALT_LEN];
    rng.fill_bytes(&mut salt);
    let mut hash = [0u8; HASH_OUTPUT_LEN];
    pbkdf2_hmac::<Sha512>(pin.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut hash);
    format!("{}:{}", hex::encode(salt), hex::encode(hash))
}

/// Returns true if `pin` matches the stored `"salt:hash"` string.
/// Documents the Verify Pin public API surface.
pub fn verify_pin(stored: &str, pin: &str) -> bool {
    let Some((salt_hex, hash_hex)) = stored.split_once(':') else {
        return false;
    };
    let Ok(salt) = hex::decode(salt_hex) else {
        return false;
    };
    let Ok(expected) = hex::decode(hash_hex) else {
        return false;
    };
    if expected.len() != HASH_OUTPUT_LEN {
        return false;
    }
    let mut actual = [0u8; HASH_OUTPUT_LEN];
    pbkdf2_hmac::<Sha512>(pin.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut actual);
    // Constant-time comparison
    expected
        .iter()
        .zip(actual.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

// -- UUID generation ----------------------------------------------------------

/// Documents the New Uuid public API surface.
pub fn new_uuid() -> String {
    Uuid::now_v7().to_string()
}

// -- Session expiry formatting -------------------------------------------------

/// Returns an SQLite-compatible datetime string `YYYY-MM-DD HH:MM:SS` offset
/// by `seconds` from now (UTC). Uses the `time` crate for correctness.
/// Documents the Expires At String public API surface.
pub fn expires_at_string(seconds: u64) -> String {
    let dt = OffsetDateTime::now_utc() + time::Duration::seconds(seconds as i64);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    )
}

// -- Brute-force login tracker ------------------------------------------------

const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_SECS: u64 = 60;
const MAX_TRACKER_ENTRIES: usize = 10_000;

#[derive(Debug)]
struct LoginAttemptEntry {
    count: u32,
    locked_until: Option<Instant>,
}

/// Public Login Attempt Tracker data shape used by BoogieBox.
#[derive(Debug, Clone, Default)]
pub struct LoginAttemptTracker(Arc<Mutex<HashMap<String, LoginAttemptEntry>>>);

impl LoginAttemptTracker {
    /// Documents the Is Locked public API surface.
    pub fn is_locked(&self, user_id: &str) -> bool {
        let mut map = self.0.lock().unwrap();
        if let Some(entry) = map.get(user_id) {
            if let Some(until) = entry.locked_until {
                if Instant::now() < until {
                    return true;
                }
                // Lockout expired - clear it so next attempt starts fresh
                map.remove(user_id);
            }
        }
        false
    }

    /// Documents the Record Failure public API surface.
    pub fn record_failure(&self, user_id: &str) {
        let mut map = self.0.lock().unwrap();
        // Evict expired entries to bound memory before inserting a new key.
        if map.len() >= MAX_TRACKER_ENTRIES {
            let now = Instant::now();
            map.retain(|_, e| e.locked_until.is_some_and(|until| until > now));
            // If still at capacity after eviction, drop the whole map to prevent DoS.
            if map.len() >= MAX_TRACKER_ENTRIES {
                map.clear();
            }
        }
        let entry = map.entry(user_id.to_string()).or_insert(LoginAttemptEntry {
            count: 0,
            locked_until: None,
        });
        entry.count += 1;
        if entry.count >= MAX_LOGIN_ATTEMPTS {
            entry.locked_until = Some(Instant::now() + Duration::from_secs(LOCKOUT_SECS));
        }
    }

    /// Documents the Clear Attempts public API surface.
    pub fn clear_attempts(&self, user_id: &str) {
        self.0.lock().unwrap().remove(user_id);
    }
}

// -- Authenticated user type ---------------------------------------------------

/// Public Authenticated User data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedUser {
    /// Documents the Id public API surface.
    pub id: String,
    /// Documents the Username public API surface.
    pub username: String,
    /// Documents the Role public API surface.
    pub role: String,
    /// Documents the Can Scan public API surface.
    pub can_scan: bool,
    /// Documents the Can Edit Metadata public API surface.
    pub can_edit_metadata: bool,
}

impl AuthenticatedUser {
    /// Documents the Is Admin public API surface.
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// Axum extractor: resolves the authenticated user from the `bb_session` cookie.
/// Rejects with 401 if the cookie is missing or the session is expired/invalid.
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, axum::Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let shared_state = SharedState::from_ref(state);

        let db: DbPool = {
            shared_state
                .read()
                .expect("state lock")
                .db
                .clone()
                .ok_or_else(|| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        axum::Json(ErrorResponse {
                            error: "Database not configured".into(),
                            setup_required: Some(true),
                        }),
                    )
                })?
        };

        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get("bb_session")
            .map(|c| c.value().to_owned())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(ErrorResponse {
                        error: "Unauthorized".into(),
                        setup_required: None,
                    }),
                )
            })?;

        let session = tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::get_session_user(&conn, &token)
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(ErrorResponse {
                    error: "Internal error".into(),
                    setup_required: None,
                }),
            )
        })?
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(ErrorResponse {
                    error: "Database error".into(),
                    setup_required: None,
                }),
            )
        })?;

        session
            .map(|u| AuthenticatedUser {
                id: u.user_id,
                username: u.username,
                role: u.role,
                can_scan: u.can_scan,
                can_edit_metadata: u.can_edit_metadata,
            })
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(ErrorResponse {
                        error: "Unauthorized".into(),
                        setup_required: None,
                    }),
                )
            })
    }
}

/// Axum extractor: same as `AuthenticatedUser` but also requires the `admin` role.
/// Rejects with 403 if the user is not an admin.
/// Documents the Admin User public API surface.
pub struct AdminUser(pub AuthenticatedUser);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, axum::Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err((
                StatusCode::FORBIDDEN,
                axum::Json(ErrorResponse {
                    error: "Forbidden".into(),
                    setup_required: None,
                }),
            ));
        }
        Ok(AdminUser(user))
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_correct_pin() {
        let h = hash_pin("secret123");
        assert!(h.contains(':'));
        assert!(verify_pin(&h, "secret123"));
    }

    #[test]
    fn verify_rejects_wrong_pin() {
        let h = hash_pin("correct");
        assert!(!verify_pin(&h, "wrong"));
    }

    #[test]
    fn verify_rejects_malformed_hash() {
        assert!(!verify_pin("not-a-valid-hash", "anything"));
        assert!(!verify_pin("", "anything"));
    }

    #[test]
    fn new_uuid_produces_different_values() {
        assert_ne!(new_uuid(), new_uuid());
    }

    #[test]
    fn expires_at_string_is_in_future() {
        let s = expires_at_string(86400);
        // Format: YYYY-MM-DD HH:MM:SS
        assert_eq!(s.len(), 19);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
    }

    #[test]
    fn lockout_after_five_failures() {
        let tracker = LoginAttemptTracker::default();
        for _ in 0..5 {
            tracker.record_failure("user1");
        }
        assert!(tracker.is_locked("user1"));
    }

    #[test]
    fn clear_attempts_removes_lockout() {
        let tracker = LoginAttemptTracker::default();
        for _ in 0..5 {
            tracker.record_failure("user1");
        }
        tracker.clear_attempts("user1");
        assert!(!tracker.is_locked("user1"));
    }
}

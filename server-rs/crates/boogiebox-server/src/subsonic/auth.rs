//! Subsonic authentication: `u`+`p` (plain or `enc:<hex>`) or `u`+`t`+`s` (token) against the user's
//! Subsonic password — never the PIN — or an OpenSubsonic `apiKey` alone. Password failures share one
//! message so usernames cannot be probed, and the web login's lockout tracker is reused under a separate
//! `subsonic:` key.

use std::path::Path;

use super::{
    crypto,
    params::Params,
    response::SubsonicError,
    state::{CachedAccount, SubsonicState, SubsonicUser},
};
use crate::{auth::LoginAttemptTracker, DbPool};

const MAX_USERNAME_LEN: usize = 128;
const MAX_CLIENT_LEN: usize = 64;

/// Everything authentication needs; owned so it can move into `spawn_blocking`.
pub struct AuthContext<'a> {
    pub subsonic: &'a SubsonicState,
    pub db: &'a DbPool,
    pub db_folder: Option<&'a Path>,
    pub tracker: &'a LoginAttemptTracker,
}

/// Authenticates a request. Blocking (SQLite, key file): call from `spawn_blocking`.
pub fn authenticate(ctx: &AuthContext<'_>, params: &Params) -> Result<SubsonicUser, SubsonicError> {
    if !ctx.subsonic.settings(ctx.db).enabled {
        return Err(SubsonicError::generic(
            "The Subsonic API is turned off on this server",
        ));
    }

    let has_key = params.text("apiKey").is_some();
    let has_p = params.text("p").is_some();
    let has_token = params.text("t").is_some() || params.text("s").is_some();
    // OpenSubsonic: an API key names its user, so `u` alongside it is a conflict too.
    let has_user = params.text("u").is_some();
    if has_key && (has_p || has_token || has_user) || has_p && has_token {
        return Err(SubsonicError::new(
            43,
            "Multiple conflicting authentication mechanisms provided",
        ));
    }
    if has_key {
        return authenticate_api_key(ctx, params);
    }
    let username = params.required("u")?;
    if !has_p {
        params.required("t")?;
        params.required("s")?;
    }
    if username.chars().count() > MAX_USERNAME_LEN {
        return Err(SubsonicError::wrong_credentials());
    }

    let lock_key = format!("subsonic:{}", username.to_lowercase());
    if ctx.tracker.is_locked(&lock_key) {
        return Err(SubsonicError::new(
            40,
            "Too many failed login attempts. Try again in a minute",
        ));
    }

    let account = load_account(ctx, username)?;
    let secret = account.secret.as_deref();
    let valid = if has_p {
        let given = decode_password(params.get("p").unwrap_or(""));
        let expected = secret.unwrap_or("\u{0}no-password");
        given.is_some_and(|g| crypto::constant_time_eq(g.as_bytes(), expected.as_bytes()))
            && secret.is_some()
    } else {
        let salt = params.get("s").unwrap_or("");
        let given = params.get("t").unwrap_or("").to_ascii_lowercase();
        let salt_ok = (6..=64).contains(&salt.len());
        // Always hash, so unknown users and wrong passwords take the same path.
        let expected = crypto::token(secret.unwrap_or("\u{0}no-password"), salt);
        salt_ok
            && crypto::constant_time_eq(given.as_bytes(), expected.as_bytes())
            && secret.is_some()
    };

    match (valid, account.user) {
        (true, Some(user)) => {
            ctx.tracker.clear_attempts(&lock_key);
            record_use(ctx, &user, params);
            Ok(user)
        }
        _ => {
            ctx.tracker.record_failure(&lock_key);
            Err(SubsonicError::wrong_credentials())
        }
    }
}

fn invalid_api_key() -> SubsonicError {
    SubsonicError::new(44, "Invalid API key")
}

/// `apiKey` (OpenSubsonic `apiKeyAuthentication`): the key's SHA-256 names the user. No lockout: a 256-bit
/// key cannot be guessed, and a per-key counter would not slow a guesser down.
fn authenticate_api_key(
    ctx: &AuthContext<'_>,
    params: &Params,
) -> Result<SubsonicUser, SubsonicError> {
    let key = params.text("apiKey").unwrap_or("");
    if key.len() > crypto::MAX_API_KEY_LEN {
        return Err(invalid_api_key());
    }
    let hash = crypto::hash_api_key(key);
    let user = match ctx.subsonic.cached_api_key(&hash).and_then(|a| a.user) {
        Some(user) => user,
        None => {
            let row = {
                let conn = ctx.db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::subsonic::get_account_by_api_key(&conn, &hash)
            }
            .map_err(|err| {
                tracing::error!(error = %err, "Subsonic API key lookup failed");
                SubsonicError::generic("Database error")
            })?;
            let Some(row) = row else {
                tracing::debug!("Subsonic API key not recognised");
                return Err(invalid_api_key());
            };
            let user = user_from(row);
            ctx.subsonic.store_api_key(
                hash,
                CachedAccount {
                    user: Some(user.clone()),
                    secret: None,
                    fetched: std::time::Instant::now(),
                },
            );
            user
        }
    };
    record_use(ctx, &user, params);
    Ok(user)
}

/// Records "last used" and the client name, at most once per interval per user.
fn record_use(ctx: &AuthContext<'_>, user: &SubsonicUser, params: &Params) {
    if !ctx.subsonic.should_record_use(&user.id) {
        return;
    }
    let client: Option<String> = params
        .text("c")
        .map(|c| c.chars().take(MAX_CLIENT_LEN).collect());
    let conn = ctx.db.lock().unwrap_or_else(|p| p.into_inner());
    if let Err(err) =
        boogiebox_db::subsonic::record_subsonic_use(&conn, &user.id, client.as_deref())
    {
        tracing::warn!(error = %err, "Could not record Subsonic login time");
    }
}

fn user_from(row: boogiebox_db::subsonic::SubsonicAccount) -> SubsonicUser {
    SubsonicUser {
        id: row.user_id,
        username: row.username,
        role: row.role,
        can_manage_libraries: row.can_manage_libraries,
        can_edit_metadata: row.can_edit_metadata,
    }
}

/// `p` is plain text or `enc:` + hex of the UTF-8 bytes.
fn decode_password(raw: &str) -> Option<String> {
    match raw.strip_prefix("enc:") {
        Some(hex_part) => hex::decode(hex_part)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(raw.to_owned()),
    }
}

fn load_account(ctx: &AuthContext<'_>, username: &str) -> Result<CachedAccount, SubsonicError> {
    let cache_key = username.to_lowercase();
    if let Some(hit) = ctx.subsonic.cached_account(&cache_key) {
        return Ok(hit);
    }
    let row = {
        let conn = ctx.db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::subsonic::get_account_by_username(&conn, username)
    }
    .map_err(|err| {
        tracing::error!(error = %err, "Subsonic account lookup failed");
        SubsonicError::generic("Database error")
    })?;
    let account = match row {
        None => CachedAccount {
            user: None,
            secret: None,
            fetched: std::time::Instant::now(),
        },
        Some(row) => {
            let secret = match (&row.secret_enc, &row.secret_nonce) {
                (Some(ct), Some(nonce)) => ctx
                    .subsonic
                    .key(ctx.db_folder, ctx.db)
                    .and_then(|key| crypto::decrypt(&key, &row.user_id, ct, nonce)),
                _ => None,
            };
            CachedAccount {
                user: Some(user_from(row)),
                secret,
                fetched: std::time::Instant::now(),
            }
        }
    };
    ctx.subsonic.store_account(cache_key, account.clone());
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fixture {
        state: SubsonicState,
        db: DbPool,
        dir: std::path::PathBuf,
        tracker: LoginAttemptTracker,
    }

    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("bb-subsonic-auth-{}", uuid::Uuid::now_v7()));
            let db = Arc::new(Mutex::new(boogiebox_db::init_db(&dir).unwrap().connection));
            {
                let conn = db.lock().unwrap();
                conn.execute_batch(
                    "INSERT INTO users(id, username, role) VALUES ('u1', 'Alice', 'user'), ('u2', 'nopass', 'admin');
                     INSERT INTO settings(key, value) VALUES ('subsonicApiEnabled', 'true');",
                )
                .unwrap();
            }
            let fx = Fixture {
                state: SubsonicState::default(),
                db,
                dir,
                tracker: LoginAttemptTracker::default(),
            };
            let key = fx.state.key(Some(&fx.dir), &fx.db).unwrap();
            let (ct, nonce) = crypto::encrypt(&key, "u1", "sesame!!").unwrap();
            boogiebox_db::subsonic::set_subsonic_secret(
                &fx.db.lock().unwrap(),
                "u1",
                &ct,
                &nonce,
                "u1",
            )
            .unwrap();
            fx
        }

        fn auth(&self, query: &str) -> Result<SubsonicUser, SubsonicError> {
            let ctx = AuthContext {
                subsonic: &self.state,
                db: &self.db,
                db_folder: Some(&self.dir),
                tracker: &self.tracker,
            };
            authenticate(&ctx, &Params::parse(Some(query), b""))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn accepts_plain_hex_and_token_logins() {
        let fx = Fixture::new();
        assert_eq!(fx.auth("u=alice&p=sesame!!&c=test").unwrap().id, "u1");
        let enc = hex::encode("sesame!!");
        assert_eq!(
            fx.auth(&format!("u=Alice&p=enc:{enc}")).unwrap().username,
            "Alice"
        );
        let t = crypto::token("sesame!!", "abc123");
        assert!(fx
            .auth(&format!("u=alice&t={}&s=abc123", t.to_uppercase()))
            .is_ok());
        let status =
            boogiebox_db::subsonic::get_credential_status(&fx.db.lock().unwrap(), "u1").unwrap();
        assert_eq!(status.last_client.as_deref(), Some("test"));
    }

    #[test]
    fn rejects_wrong_missing_and_conflicting_credentials() {
        let fx = Fixture::new();
        assert_eq!(fx.auth("u=alice&p=wrong").unwrap_err().code, 40);
        assert_eq!(fx.auth("u=ghost&p=sesame!!").unwrap_err().code, 40);
        assert_eq!(fx.auth("u=nopass&p=").unwrap_err().code, 10);
        assert_eq!(fx.auth("u=nopass&p=anything").unwrap_err().code, 40);
        assert_eq!(fx.auth("u=alice&t=abc&s=12").unwrap_err().code, 40);
        assert_eq!(fx.auth("p=sesame!!").unwrap_err().code, 10);
        assert_eq!(fx.auth("u=alice&t=abc").unwrap_err().code, 10);
        assert_eq!(fx.auth("u=alice&p=x&t=y&s=zzzzzz").unwrap_err().code, 43);
        assert_eq!(fx.auth("apiKey=k&u=alice&p=x").unwrap_err().code, 43);
        assert_eq!(fx.auth("apiKey=k&u=alice").unwrap_err().code, 43);
        assert_eq!(fx.auth("apiKey=k").unwrap_err().code, 44);
        // Unknown users and wrong passwords share the message.
        assert_eq!(
            fx.auth("u=ghost&p=x").unwrap_err().message,
            fx.auth("u=alice&p=x").unwrap_err().message
        );
    }

    #[test]
    fn api_key_signs_in_alone_and_stops_when_revoked() {
        let fx = Fixture::new();
        let key = crypto::generate_api_key();
        // u2 has no Subsonic password: the key still works, password sign-in still fails with 40.
        boogiebox_db::subsonic::set_api_key_hash(
            &fx.db.lock().unwrap(),
            "u2",
            &crypto::hash_api_key(&key),
        )
        .unwrap();
        let user = fx.auth(&format!("apiKey={key}&c=keyapp")).unwrap();
        assert_eq!(user.username, "nopass");
        assert!(user.is_admin());
        assert_eq!(fx.auth("u=nopass&p=anything").unwrap_err().code, 40);
        let status =
            boogiebox_db::subsonic::get_credential_status(&fx.db.lock().unwrap(), "u2").unwrap();
        assert_eq!(status.last_client.as_deref(), Some("keyapp"));

        // Cached until invalidated, like passwords.
        boogiebox_db::subsonic::clear_api_key(&fx.db.lock().unwrap(), "u2").unwrap();
        assert!(fx.auth(&format!("apiKey={key}")).is_ok());
        fx.state.invalidate_accounts();
        assert_eq!(fx.auth(&format!("apiKey={key}")).unwrap_err().code, 44);
        assert_eq!(
            fx.auth(&format!("apiKey={}", "x".repeat(200)))
                .unwrap_err()
                .code,
            44
        );
    }

    #[test]
    fn locks_out_after_repeated_failures() {
        let fx = Fixture::new();
        for _ in 0..5 {
            assert!(fx.auth("u=alice&p=nope").is_err());
        }
        let err = fx.auth("u=alice&p=sesame!!").unwrap_err();
        assert_eq!(err.code, 40);
        assert!(err.message.contains("Too many"));
    }

    #[test]
    fn disabled_api_and_cache_invalidation() {
        let fx = Fixture::new();
        assert!(fx.auth("u=alice&p=sesame!!").is_ok());
        boogiebox_db::subsonic::clear_subsonic_secret(&fx.db.lock().unwrap(), "u1", "u1").unwrap();
        // Cached for a minute unless invalidated.
        assert!(fx.auth("u=alice&p=sesame!!").is_ok());
        fx.state.invalidate_accounts();
        assert_eq!(fx.auth("u=alice&p=sesame!!").unwrap_err().code, 40);

        fx.db
            .lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value = 'false' WHERE key = 'subsonicApiEnabled'",
                [],
            )
            .unwrap();
        fx.state.invalidate_settings();
        assert_eq!(fx.auth("u=alice&p=sesame!!").unwrap_err().code, 0);
    }
}

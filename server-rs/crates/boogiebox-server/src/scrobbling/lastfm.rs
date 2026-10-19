//! Last.fm Scrobbling 2.0: web authentication, `track.updateNowPlaying` and `track.scrobble`.
//! Every call is signed with the app's shared secret (`api_sig`). Keys and session keys are never logged.

use std::time::Duration;

use md5::{Digest, Md5};
use serde_json::Value;

use super::SendError;
use crate::providers::urlencoding::encode;
use boogiebox_db::scrobble::ScrobbleTrack;

/// Where users approve BoogieBox on Last.fm.
pub const AUTH_PAGE: &str = "https://www.last.fm/api/auth/";
/// Most plays per `track.scrobble` call (Last.fm limit).
pub const MAX_BATCH: usize = 50;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The app credentials an admin entered (`lastfmKey`, `lastfmSecret`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCredentials {
    pub api_key: String,
    pub secret: String,
}

/// `api_sig`: md5 of the parameters sorted by name and concatenated as name+value, then the secret.
/// `format` and `callback` are not signed.
pub fn api_sig(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<&(String, String)> = params
        .iter()
        .filter(|(k, _)| k != "format" && k != "callback")
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Md5::new();
    for (k, v) in sorted {
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    hasher.update(secret.as_bytes());
    hex::encode(hasher.finalize())
}

/// The page that asks the user to allow BoogieBox; Last.fm redirects to `callback` with `token`.
pub fn auth_url(api_key: &str, callback: &str) -> String {
    format!(
        "{AUTH_PAGE}?api_key={}&cb={}",
        encode(api_key),
        encode(callback)
    )
}

fn form_body(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Maps a Last.fm error code to how the forwarder reacts.
pub fn classify(code: i64) -> SendError {
    match code {
        // Invalid/expired session, authentication failed, unauthorised token.
        4 | 9 | 14 => SendError::Auth,
        // Invalid parameters / resource: the play itself is bad.
        6 | 7 => SendError::Rejected,
        // Offline, temporary error, rate limit, and app-key problems the admin can fix.
        _ => SendError::Temporary,
    }
}

async fn call(
    client: &reqwest::Client,
    base: &str,
    app: &AppCredentials,
    mut params: Vec<(String, String)>,
) -> Result<Value, SendError> {
    params.push(("api_key".into(), app.api_key.clone()));
    let sig = api_sig(&params, &app.secret);
    params.push(("api_sig".into(), sig));
    params.push(("format".into(), "json".into()));
    let response = client
        .post(base)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .timeout(TIMEOUT)
        .body(form_body(&params))
        .send()
        .await
        .map_err(|_| SendError::Temporary)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if let Some(code) = body.get("error").and_then(Value::as_i64) {
        return Err(classify(code));
    }
    if !status.is_success() {
        return Err(if status.is_client_error() && status.as_u16() != 429 {
            SendError::Rejected
        } else {
            SendError::Temporary
        });
    }
    Ok(body)
}

/// Exchanges the callback token for a session key; returns `(session_key, user_name)`.
pub async fn get_session(
    client: &reqwest::Client,
    base: &str,
    app: &AppCredentials,
    token: &str,
) -> Result<(String, String), SendError> {
    let body = call(
        client,
        base,
        app,
        vec![
            ("method".into(), "auth.getSession".into()),
            ("token".into(), token.to_owned()),
        ],
    )
    .await?;
    let session = body.get("session").ok_or(SendError::Temporary)?;
    let key = session
        .get("key")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
        .ok_or(SendError::Temporary)?;
    let name = session.get("name").and_then(Value::as_str).unwrap_or("");
    Ok((key.to_owned(), name.to_owned()))
}

fn track_params(track: &ScrobbleTrack, suffix: &str) -> Vec<(String, String)> {
    let mut p = vec![
        (format!("artist{suffix}"), track.artist.clone()),
        (format!("track{suffix}"), track.title.clone()),
    ];
    if let Some(album) = &track.album {
        p.push((format!("album{suffix}"), album.clone()));
    }
    if let Some(d) = track.duration_secs {
        p.push((format!("duration{suffix}"), d.to_string()));
    }
    p
}

/// `track.updateNowPlaying`.
pub async fn update_now_playing(
    client: &reqwest::Client,
    base: &str,
    app: &AppCredentials,
    session_key: &str,
    track: &ScrobbleTrack,
) -> Result<(), SendError> {
    let mut params = vec![
        ("method".into(), "track.updateNowPlaying".into()),
        ("sk".into(), session_key.to_owned()),
    ];
    params.extend(track_params(track, ""));
    call(client, base, app, params).await.map(|_| ())
}

/// `track.scrobble` for up to [`MAX_BATCH`] plays (`played_at` = Unix start time).
pub async fn scrobble(
    client: &reqwest::Client,
    base: &str,
    app: &AppCredentials,
    session_key: &str,
    plays: &[(ScrobbleTrack, i64)],
) -> Result<(), SendError> {
    let mut params = vec![
        ("method".into(), "track.scrobble".into()),
        ("sk".into(), session_key.to_owned()),
    ];
    for (i, (track, at)) in plays.iter().take(MAX_BATCH).enumerate() {
        let suffix = format!("[{i}]");
        params.extend(track_params(track, &suffix));
        params.push((format!("timestamp{suffix}"), at.to_string()));
    }
    call(client, base, app, params).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_string_contains, method},
        Mock, MockServer, ResponseTemplate,
    };

    fn app() -> AppCredentials {
        AppCredentials {
            api_key: "key123".into(),
            secret: "sekrit".into(),
        }
    }

    fn track() -> ScrobbleTrack {
        ScrobbleTrack {
            artist: "Björk".into(),
            title: "Army of Me".into(),
            album: Some("Post".into()),
            duration_secs: Some(234),
        }
    }

    #[test]
    fn signature_sorts_params_and_skips_format() {
        let params = vec![
            ("token".to_string(), "tok".to_string()),
            ("method".to_string(), "auth.getSession".to_string()),
            ("api_key".to_string(), "key123".to_string()),
            ("format".to_string(), "json".to_string()),
        ];
        // Sorted name+value pairs, then the secret; `format` is not signed.
        let mut expected = Md5::new();
        expected.update(b"api_keykey123methodauth.getSessiontokentoksekrit");
        assert_eq!(api_sig(&params, "sekrit"), hex::encode(expected.finalize()));
    }

    #[test]
    fn auth_url_encodes_the_callback() {
        let url = auth_url(
            "key123",
            "http://box.local:3000/api/scrobble/lastfm/callback?state=ab",
        );
        assert_eq!(
            url,
            "https://www.last.fm/api/auth/?api_key=key123&cb=http%3A%2F%2Fbox.local%3A3000%2Fapi%2Fscrobble%2Flastfm%2Fcallback%3Fstate%3Dab"
        );
    }

    #[test]
    fn error_codes_classify() {
        assert_eq!(classify(9), SendError::Auth);
        assert_eq!(classify(14), SendError::Auth);
        assert_eq!(classify(6), SendError::Rejected);
        assert_eq!(classify(11), SendError::Temporary);
        assert_eq!(classify(29), SendError::Temporary);
        assert_eq!(classify(26), SendError::Temporary);
    }

    #[tokio::test]
    async fn session_scrobble_and_now_playing_round_trip() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("method=auth.getSession"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"session": {"name": "alice", "key": "sk-1", "subscriber": 0}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_string_contains("method=track.scrobble"))
            .and(body_string_contains("artist%5B0%5D=Bj%C3%B6rk"))
            .and(body_string_contains("timestamp%5B1%5D=200"))
            .and(body_string_contains("api_sig="))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"scrobbles": {"@attr": {"accepted": 2, "ignored": 0}}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_string_contains("method=track.updateNowPlaying"))
            .and(body_string_contains("sk=sk-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"nowplaying": {}})),
            )
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let base = server.uri();
        assert_eq!(
            get_session(&client, &base, &app(), "tok").await.unwrap(),
            ("sk-1".to_string(), "alice".to_string())
        );
        scrobble(
            &client,
            &base,
            &app(),
            "sk-1",
            &[(track(), 100), (track(), 200)],
        )
        .await
        .unwrap();
        update_now_playing(&client, &base, &app(), "sk-1", &track())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn errors_map_to_send_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("sk=expired"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({"error": 9, "message": "Invalid session key"}),
                ),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_string_contains("sk=busy"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_string_contains("method=auth.getSession"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"session": {}})),
            )
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let base = server.uri();
        assert_eq!(
            scrobble(&client, &base, &app(), "expired", &[(track(), 1)]).await,
            Err(SendError::Auth)
        );
        assert_eq!(
            update_now_playing(&client, &base, &app(), "busy", &track()).await,
            Err(SendError::Temporary)
        );
        assert_eq!(
            get_session(&client, &base, &app(), "tok").await,
            Err(SendError::Temporary)
        );
        // Unreachable server.
        assert_eq!(
            scrobble(&client, "http://127.0.0.1:9", &app(), "sk", &[(track(), 1)]).await,
            Err(SendError::Temporary)
        );
    }
}

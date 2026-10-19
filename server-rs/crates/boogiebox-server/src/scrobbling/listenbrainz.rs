//! ListenBrainz: token validation, `playing_now` and listen submission. Tokens are never logged.

use std::time::Duration;

use serde_json::{json, Value};

use super::SendError;
use boogiebox_db::scrobble::ScrobbleTrack;

/// Most listens per `import` submission.
pub const MAX_BATCH: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(15);

/// API root (overridable for tests).
pub fn api_base() -> String {
    std::env::var("BOOGIEBOX_LISTENBRAINZ_API_BASE")
        .unwrap_or_else(|_| "https://api.listenbrainz.org".to_owned())
}

/// Tokens are UUID-like; reject anything that could not be one before sending it anywhere.
pub fn plausible_token(token: &str) -> bool {
    (8..=128).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn metadata(track: &ScrobbleTrack) -> Value {
    let mut info = json!({
        "submission_client": "BoogieBox",
        "submission_client_version": env!("CARGO_PKG_VERSION"),
        "media_player": "BoogieBox",
    });
    if let Some(d) = track.duration_secs {
        info["duration_ms"] = json!(d * 1000);
    }
    let mut meta = json!({
        "artist_name": track.artist,
        "track_name": track.title,
        "additional_info": info,
    });
    if let Some(album) = &track.album {
        meta["release_name"] = json!(album);
    }
    meta
}

fn status_error(status: reqwest::StatusCode) -> SendError {
    match status.as_u16() {
        401 | 403 => SendError::Auth,
        429 => SendError::Temporary,
        400..=499 => SendError::Rejected,
        _ => SendError::Temporary,
    }
}

/// Checks a user token; returns the ListenBrainz user name.
pub async fn validate_token(
    client: &reqwest::Client,
    base: &str,
    token: &str,
) -> Result<String, SendError> {
    let response = client
        .get(format!("{base}/1/validate-token"))
        .header("Authorization", format!("Token {token}"))
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| SendError::Temporary)?;
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    let body: Value = response.json().await.map_err(|_| SendError::Temporary)?;
    if body.get("valid").and_then(Value::as_bool) != Some(true) {
        return Err(SendError::Auth);
    }
    Ok(body
        .get("user_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned())
}

async fn submit(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    body: Value,
) -> Result<(), SendError> {
    let response = client
        .post(format!("{base}/1/submit-listens"))
        .header("Authorization", format!("Token {token}"))
        .header("Content-Type", "application/json")
        .timeout(TIMEOUT)
        .body(body.to_string())
        .send()
        .await
        .map_err(|_| SendError::Temporary)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(status_error(response.status()))
    }
}

/// `playing_now` for a track that just started.
pub async fn playing_now(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    track: &ScrobbleTrack,
) -> Result<(), SendError> {
    submit(
        client,
        base,
        token,
        json!({"listen_type": "playing_now", "payload": [{"track_metadata": metadata(track)}]}),
    )
    .await
}

/// Submits up to [`MAX_BATCH`] finished plays (`listened_at` = Unix start time).
pub async fn submit_listens(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    plays: &[(ScrobbleTrack, i64)],
) -> Result<(), SendError> {
    let payload: Vec<Value> = plays
        .iter()
        .take(MAX_BATCH)
        .map(|(track, at)| json!({"listened_at": at, "track_metadata": metadata(track)}))
        .collect();
    let listen_type = if payload.len() == 1 {
        "single"
    } else {
        "import"
    };
    submit(
        client,
        base,
        token,
        json!({"listen_type": listen_type, "payload": payload}),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn track() -> ScrobbleTrack {
        ScrobbleTrack {
            artist: "Massive Attack".into(),
            title: "Teardrop".into(),
            album: Some("Mezzanine".into()),
            duration_secs: Some(330),
        }
    }

    #[test]
    fn token_shape() {
        assert!(plausible_token("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"));
        assert!(!plausible_token("short"));
        assert!(!plausible_token("has space in it"));
        assert!(!plausible_token(&"a".repeat(129)));
    }

    #[test]
    fn metadata_includes_optional_fields() {
        let m = metadata(&track());
        assert_eq!(m["release_name"], "Mezzanine");
        assert_eq!(m["additional_info"]["duration_ms"], 330_000);
        let bare = metadata(&ScrobbleTrack {
            album: None,
            duration_secs: None,
            ..track()
        });
        assert!(bare.get("release_name").is_none());
        assert!(bare["additional_info"].get("duration_ms").is_none());
    }

    #[tokio::test]
    async fn validate_and_submit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/1/validate-token"))
            .and(header("Authorization", "Token good-token-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"code": 200, "message": "Token valid.", "valid": true, "user_name": "lb_user"}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/1/validate-token"))
            .and(header("Authorization", "Token bad-token-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"valid": false})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/1/submit-listens"))
            .and(header("Authorization", "Token good-token-1"))
            .and(body_partial_json(json!({"listen_type": "import"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/1/submit-listens"))
            .and(body_partial_json(json!({"listen_type": "playing_now"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/1/submit-listens"))
            .and(body_partial_json(json!({"listen_type": "single"})))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let base = server.uri();
        assert_eq!(
            validate_token(&client, &base, "good-token-1")
                .await
                .unwrap(),
            "lb_user"
        );
        assert_eq!(
            validate_token(&client, &base, "bad-token-1").await,
            Err(SendError::Auth)
        );
        submit_listens(
            &client,
            &base,
            "good-token-1",
            &[(track(), 1), (track(), 2)],
        )
        .await
        .unwrap();
        playing_now(&client, &base, "good-token-1", &track())
            .await
            .unwrap();
        assert_eq!(
            submit_listens(&client, &base, "good-token-1", &[(track(), 1)]).await,
            Err(SendError::Auth)
        );
    }

    #[test]
    fn statuses_classify() {
        use reqwest::StatusCode;
        assert_eq!(status_error(StatusCode::UNAUTHORIZED), SendError::Auth);
        assert_eq!(
            status_error(StatusCode::TOO_MANY_REQUESTS),
            SendError::Temporary
        );
        assert_eq!(status_error(StatusCode::BAD_REQUEST), SendError::Rejected);
        assert_eq!(status_error(StatusCode::BAD_GATEWAY), SendError::Temporary);
    }
}

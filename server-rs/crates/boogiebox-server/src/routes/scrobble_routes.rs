//! Scrobbling accounts for the BoogieBox UI (cookie auth, own account only): connect ListenBrainz with a
//! token, connect Last.fm through its sign-in page, see status, disconnect. Secrets are never returned.
//! See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-3).

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use boogiebox_db::scrobble::{self as qdb, ScrobbleService};
use serde::{Deserialize, Serialize};

use crate::{
    auth::AuthenticatedUser,
    plays::PlayEnv,
    scrobbling::{self, ConnectError, Endpoints},
    ErrorResponse, OkResponse, SharedState,
};

/// Documents the Scrobble Router public API surface.
pub fn scrobble_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/scrobble/accounts", get(accounts_handler))
        .route("/api/scrobble/lastfm/connect", post(lastfm_connect_handler))
        .route(
            "/api/scrobble/lastfm/callback",
            get(lastfm_callback_handler),
        )
        // `PUT` connects ListenBrainz (token); Last.fm connects through its sign-in page instead.
        .route(
            "/api/scrobble/{service}",
            put(connect_listenbrainz_handler).delete(disconnect_handler),
        )
        .with_state(state)
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_owned(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn not_ready() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "Database not configured")
}

fn connect_failed(err: ConnectError) -> Response {
    let status = match err {
        ConnectError::Rejected | ConnectError::Expired => StatusCode::BAD_REQUEST,
        ConnectError::NotConfigured => StatusCode::CONFLICT,
        ConnectError::Unavailable => StatusCode::BAD_GATEWAY,
        ConnectError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, err.message())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView {
    service: &'static str,
    account_name: Option<String>,
    needs_reconnect: bool,
    connected_at: String,
    last_scrobble_at: Option<String>,
    pending: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountsResponse {
    /// The admin entered both the Last.fm key and shared secret.
    lastfm_available: bool,
    accounts: Vec<AccountView>,
}

async fn accounts_handler(State(state): State<SharedState>, user: AuthenticatedUser) -> Response {
    let Some(env) = PlayEnv::from_state(&state) else {
        return not_ready();
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = env.db.lock().unwrap_or_else(|p| p.into_inner());
        let accounts = qdb::list_accounts(&conn, &user.id)?;
        Ok::<_, rusqlite::Error>((scrobbling::lastfm_app(&conn).is_some(), accounts))
    })
    .await;
    match result {
        Ok(Ok((lastfm_available, accounts))) => Json(AccountsResponse {
            lastfm_available,
            accounts: accounts
                .into_iter()
                .map(|a| AccountView {
                    service: a.service.as_str(),
                    account_name: a.account_name,
                    needs_reconnect: a.needs_reconnect,
                    connected_at: a.connected_at,
                    last_scrobble_at: a.last_scrobble_at,
                    pending: a.pending,
                })
                .collect(),
        })
        .into_response(),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

#[derive(Debug, Deserialize)]
struct TokenBody {
    token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectedResponse {
    ok: bool,
    account_name: String,
}

async fn connect_listenbrainz_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(service): Path<String>,
    Json(body): Json<TokenBody>,
) -> Response {
    if service != ScrobbleService::ListenBrainz.as_str() {
        return error(StatusCode::NOT_FOUND, "Unknown service");
    }
    let Some(env) = PlayEnv::from_state(&state) else {
        return not_ready();
    };
    let token = body.token.unwrap_or_default();
    match scrobbling::connect_listenbrainz(&env, &Endpoints::production(), &user.id, &token).await {
        Ok(account_name) => Json(ConnectedResponse {
            ok: true,
            account_name,
        })
        .into_response(),
        Err(err) => connect_failed(err),
    }
}

#[derive(Debug, Deserialize)]
struct ConnectBody {
    /// The web UI's own origin (`window.location.origin`); Last.fm sends the user back there.
    origin: Option<String>,
}

#[derive(Debug, Serialize)]
struct UrlResponse {
    url: String,
}

async fn lastfm_connect_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Json(body): Json<ConnectBody>,
) -> Response {
    let Some(env) = PlayEnv::from_state(&state) else {
        return not_ready();
    };
    let origin = body.origin.unwrap_or_default();
    match scrobbling::begin_lastfm_connect(&env, &user.id, &origin).await {
        Ok(url) => Json(UrlResponse { url }).into_response(),
        Err(err) => connect_failed(err),
    }
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    token: Option<String>,
}

fn escape_html(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

/// The page Last.fm returns the user to (in a separate window or the system browser).
fn callback_page(ok: bool, message: &str) -> Response {
    let title = if ok {
        "Last.fm connected"
    } else {
        "Last.fm not connected"
    };
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>{title}</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#111;color:#eee;display:flex;\
         min-height:100vh;align-items:center;justify-content:center;margin:0;padding:16px}}\
         main{{max-width:28rem;text-align:center}}h1{{font-size:1.4rem}}</style></head>\
         <body><main><h1>{title}</h1><p>{}</p></main></body></html>",
        escape_html(message)
    );
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, [(header::CACHE_CONTROL, "no-store")], Html(body)).into_response()
}

/// No session cookie needed: the one-time `state` names the user (the page may open in another browser).
async fn lastfm_callback_handler(
    State(state): State<SharedState>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(env) = PlayEnv::from_state(&state) else {
        return not_ready();
    };
    let (Some(sign_in), Some(token)) = (query.state, query.token) else {
        return callback_page(false, ConnectError::Expired.message());
    };
    match scrobbling::finish_lastfm_connect(&env, &Endpoints::production(), &sign_in, &token).await
    {
        Ok(_) => callback_page(
            true,
            "BoogieBox will now scrobble your plays to Last.fm. You can close this window.",
        ),
        Err(err) => callback_page(false, err.message()),
    }
}

async fn disconnect_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
    Path(service): Path<String>,
) -> Response {
    let Some(service) = ScrobbleService::parse(&service) else {
        return error(StatusCode::NOT_FOUND, "Unknown service");
    };
    let Some(env) = PlayEnv::from_state(&state) else {
        return not_ready();
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = env.db.lock().unwrap_or_else(|p| p.into_inner());
        qdb::delete_account(&conn, &user.id, service)
    })
    .await;
    match result {
        Ok(Ok(true)) => (StatusCode::OK, Json(OkResponse { ok: true })).into_response(),
        Ok(Ok(false)) => error(StatusCode::NOT_FOUND, "Not connected"),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{json_body, new_test_app_with_pool, seed_user_session, send};
    use axum::{body::Body, http::Request};
    use serde_json::{json, Value};

    fn req(method: &str, uri: &str, body: Option<Value>, cookie: Option<&str>) -> Request<Body> {
        let mut r = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            r = r.header("cookie", c);
        }
        match body {
            Some(b) => r
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => r.body(Body::empty()).unwrap(),
        }
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            escape_html("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[tokio::test]
    async fn accounts_connect_and_disconnect_without_exposing_secrets() {
        let (app, pool) = new_test_app_with_pool("scrobble-routes");
        let cookie = seed_user_session(&pool, "u1");
        let c = Some(cookie.as_str());

        let (status, _) = send(
            app.clone(),
            req("GET", "/api/scrobble/accounts", None, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, body) = send(app.clone(), req("GET", "/api/scrobble/accounts", None, c)).await;
        assert_eq!(status, StatusCode::OK);
        let body = json_body(&body);
        assert_eq!(body["lastfmAvailable"], false);
        assert_eq!(body["accounts"], json!([]));

        // Last.fm not set up → conflict; a bad origin is rejected first.
        let connect = |origin: &str| {
            req(
                "POST",
                "/api/scrobble/lastfm/connect",
                Some(json!({ "origin": origin })),
                c,
            )
        };
        assert_eq!(
            send(app.clone(), connect("http://box.local:3000")).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            send(app.clone(), connect("http://box/evil")).await.0,
            StatusCode::BAD_REQUEST
        );

        // A malformed ListenBrainz token never leaves the server.
        let (status, _) = send(
            app.clone(),
            req(
                "PUT",
                "/api/scrobble/listenbrainz",
                Some(json!({"token": "not a token"})),
                c,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Callback without a valid state → an error page, no account.
        let (status, page) = send(
            app.clone(),
            req(
                "GET",
                "/api/scrobble/lastfm/callback?state=forged&token=abc",
                None,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&page).contains("expired"));
        let (status, _) = send(
            app.clone(),
            req("GET", "/api/scrobble/lastfm/callback", None, None),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Seed an account directly, then list and disconnect it.
        {
            let conn = pool.lock().unwrap();
            qdb::upsert_account(
                &conn,
                "u1",
                ScrobbleService::ListenBrainz,
                b"secret-bytes",
                b"nonce",
                Some("lb_u1"),
            )
            .unwrap();
        }
        let (_, body) = send(app.clone(), req("GET", "/api/scrobble/accounts", None, c)).await;
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(text.contains("\"accountName\":\"lb_u1\""), "{text}");
        assert!(
            !text.contains("secret"),
            "no secret in the response: {text}"
        );

        let delete = |uri: &str| req("DELETE", uri, None, c);
        assert_eq!(
            send(app.clone(), delete("/api/scrobble/listenbrainz"))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            send(app.clone(), delete("/api/scrobble/listenbrainz"))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            send(app.clone(), delete("/api/scrobble/spotify")).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

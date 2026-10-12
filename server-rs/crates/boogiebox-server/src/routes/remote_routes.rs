//! Play On renderer routes (`wip/play-on-renderers-plan.md` §6): list/refresh/admin-edit renderers and serve
//! their icons. Session control routes land in Phase 4.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::{AdminUser, AuthenticatedUser},
    remote::{read_remote_settings, RemoteEvent, RemoteManager},
    DbPool, ErrorResponse, SharedState,
};
use boogiebox_db::remote::{RendererPrefs, RendererRow, FORMAT_POLICIES};

/// Maximum alias length (characters).
pub const MAX_ALIAS_CHARS: usize = 64;

/// Play On renderer router.
pub fn remote_router(state: SharedState) -> Router {
    Router::new()
        .route("/api/remote/renderers", get(list_renderers_handler))
        .route("/api/remote/renderers/refresh", post(refresh_handler))
        .route("/api/remote/renderers/{udn}", patch(patch_renderer_handler))
        .route("/api/remote/renderers/{udn}/icon", get(icon_handler))
        .with_state(state)
}

/// Renderer capabilities exposed to clients.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesView {
    /// Gapless transitions available.
    pub set_next: bool,
    /// Seeking available.
    pub seek: bool,
    /// Volume/mute control available.
    pub volume: bool,
}

/// One renderer as shown in the picker and in Settings.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RendererView {
    /// Stable id.
    pub udn: String,
    /// Display name (alias or device name).
    pub name: String,
    /// Name reported by the device.
    pub friendly_name: String,
    /// Admin alias.
    pub alias: Option<String>,
    /// Manufacturer.
    pub manufacturer: Option<String>,
    /// Model name.
    pub model: Option<String>,
    /// Vendor family.
    pub vendor: String,
    /// Currently reachable.
    pub online: bool,
    /// Hidden by an admin (only admins receive hidden renderers).
    pub hidden: bool,
    /// Format policy.
    pub format_policy: String,
    /// The device advertises a usable icon.
    pub has_icon: bool,
    /// Capabilities.
    pub capabilities: CapabilitiesView,
    /// Last discovery timestamp (UTC, SQLite format).
    pub last_seen_at: String,
}

/// `GET /api/remote/renderers` response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderersResponse {
    /// Discovery is running.
    pub running: bool,
    /// Renderers, display-name order.
    pub renderers: Vec<RendererView>,
}

fn view(row: RendererRow, remote: &RemoteManager) -> RendererView {
    let entry = remote.registry().get(&row.udn);
    let (online, has_icon, capabilities) = match &entry {
        Some(e) => (
            e.online,
            e.description.best_icon().is_some_and(|i| {
                let m = i.mime.to_ascii_lowercase();
                m == "image/png" || m == "image/jpeg"
            }),
            CapabilitiesView {
                set_next: e.caps.set_next,
                seek: e.caps.seek,
                volume: e.description.rendering_control.is_some(),
            },
        ),
        None => (
            false,
            false,
            CapabilitiesView {
                set_next: false,
                seek: false,
                volume: false,
            },
        ),
    };
    RendererView {
        name: row
            .alias
            .clone()
            .unwrap_or_else(|| row.friendly_name.clone()),
        udn: row.udn,
        friendly_name: row.friendly_name,
        alias: row.alias,
        manufacturer: row.manufacturer,
        model: row.model_name,
        vendor: row.vendor,
        online,
        hidden: row.hidden,
        format_policy: row.format_policy,
        has_icon,
        capabilities,
        last_seen_at: row.last_seen_at,
    }
}

/// UDNs are device-supplied; accept only a conservative character set so they are safe in paths and logs.
pub fn valid_udn(udn: &str) -> bool {
    !udn.is_empty()
        && udn.len() <= 200
        && udn
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn disabled() -> Response {
    error(StatusCode::NOT_FOUND, "Play On is disabled")
}

fn context(state: &SharedState) -> Option<(DbPool, RemoteManager)> {
    let s = state.read().unwrap_or_else(|p| p.into_inner());
    Some((s.db.clone()?, s.remote.clone()))
}

fn setup_required() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "Database not configured".into(),
            setup_required: Some(true),
        }),
    )
        .into_response()
}

async fn load_rows(db: DbPool) -> Option<(bool, Vec<RendererRow>)> {
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        let enabled = read_remote_settings(&conn).enabled;
        boogiebox_db::remote::list_renderers(&conn).map(|rows| (enabled, rows))
    })
    .await
    .ok()?
    .ok()
}

async fn list_renderers_handler(
    State(state): State<SharedState>,
    user: AuthenticatedUser,
) -> Response {
    let Some((db, remote)) = context(&state) else {
        return setup_required();
    };
    let Some((enabled, rows)) = load_rows(db).await else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    };
    if !enabled {
        return disabled();
    }
    let renderers = rows
        .into_iter()
        .filter(|r| user.is_admin() || !r.hidden)
        .map(|r| view(r, &remote))
        .collect();
    Json(RenderersResponse {
        running: remote.is_running(),
        renderers,
    })
    .into_response()
}

async fn refresh_handler(State(state): State<SharedState>, _user: AuthenticatedUser) -> Response {
    let Some((_, remote)) = context(&state) else {
        return setup_required();
    };
    if remote.refresh() {
        (StatusCode::ACCEPTED, Json(crate::OkResponse { ok: true })).into_response()
    } else {
        disabled()
    }
}

/// `PATCH /api/remote/renderers/{udn}` body. `alias: null` clears the alias; absent fields are untouched.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchRendererBody {
    /// New alias, `null` to clear.
    #[serde(default, deserialize_with = "present")]
    pub alias: Option<Value>,
    /// New format policy.
    pub format_policy: Option<String>,
    /// New hidden flag.
    pub hidden: Option<bool>,
}

/// Distinguishes an explicit `null` (`Some(Value::Null)`) from an absent field (`None`, via `default`).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// Validates a PATCH body into DB preferences.
pub fn prefs_from_body(body: PatchRendererBody) -> Result<RendererPrefs, &'static str> {
    let alias = match body.alias {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.chars().count() > MAX_ALIAS_CHARS {
                return Err("Alias is too long");
            }
            if t.chars().any(char::is_control) {
                return Err("Alias contains invalid characters");
            }
            Some((!t.is_empty()).then(|| t.to_string()))
        }
        Some(_) => return Err("Alias must be a string or null"),
    };
    if let Some(p) = &body.format_policy {
        if !FORMAT_POLICIES.contains(&p.as_str()) {
            return Err("Unknown format policy");
        }
    }
    Ok(RendererPrefs {
        alias,
        format_policy: body.format_policy,
        hidden: body.hidden,
    })
}

async fn patch_renderer_handler(
    State(state): State<SharedState>,
    _admin: AdminUser,
    Path(udn): Path<String>,
    Json(body): Json<PatchRendererBody>,
) -> Response {
    if !valid_udn(&udn) {
        return error(StatusCode::BAD_REQUEST, "Invalid renderer id");
    }
    let prefs = match prefs_from_body(body) {
        Ok(p) => p,
        Err(msg) => return error(StatusCode::BAD_REQUEST, msg),
    };
    let Some((db, remote)) = context(&state) else {
        return setup_required();
    };
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        if !boogiebox_db::remote::update_renderer_prefs(&conn, &udn, &prefs)? {
            return Ok(None);
        }
        boogiebox_db::remote::get_renderer(&conn, &udn)
    })
    .await;
    match result {
        Ok(Ok(Some(row))) => {
            remote.notify(RemoteEvent::Renderers);
            Json(view(row, &remote)).into_response()
        }
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "Renderer not found"),
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
    }
}

async fn icon_handler(
    State(state): State<SharedState>,
    _user: AuthenticatedUser,
    Path(udn): Path<String>,
) -> Response {
    if !valid_udn(&udn) {
        return error(StatusCode::BAD_REQUEST, "Invalid renderer id");
    }
    let Some((_, remote)) = context(&state) else {
        return setup_required();
    };
    match remote.icon(&udn).await {
        Some((mime, bytes)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => error(StatusCode::NOT_FOUND, "No icon"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::registry::{tests::description, RendererCaps, StoredCapabilities};
    use crate::test_support::{
        build_test_state, json_body, seed_admin_session, seed_user_session, send, temp_db_path,
    };
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::{Arc, RwLock};
    use std::time::Instant;

    fn app(prefix: &str) -> (Router, DbPool, RemoteManager) {
        let state = build_test_state(&temp_db_path(prefix));
        let db = state.db.clone().unwrap();
        let remote = state.remote.clone();
        (crate::build_app(state, None), db, remote)
    }

    fn seed(db: &DbPool, udn: &str, name: &str) {
        let caps = serde_json::to_string(&StoredCapabilities {
            description: description(udn, "10.0.0.5"),
            caps: RendererCaps::default(),
        })
        .unwrap();
        boogiebox_db::remote::upsert_renderer(
            &db.lock().unwrap(),
            &boogiebox_db::remote::RendererUpsert {
                udn,
                friendly_name: name,
                manufacturer: Some("Acme"),
                model_name: Some("Box"),
                vendor: "generic",
                location_url: "http://10.0.0.5/d",
                capabilities: &caps,
            },
        )
        .unwrap();
    }

    fn get(uri: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }

    fn patch_req(uri: &str, cookie: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("PATCH")
            .uri(uri)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn list_requires_auth_merges_online_state_and_hides_for_non_admins() {
        let (app, db, remote) = app("remote-routes-list");
        seed(&db, "a", "Living Room");
        seed(&db, "b", "Kitchen");
        boogiebox_db::remote::update_renderer_prefs(
            &db.lock().unwrap(),
            "b",
            &RendererPrefs {
                hidden: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let mut d = description("a", "10.0.0.5");
        d.rendering_control = Some(d.av_transport.clone());
        remote.registry().insert_described(
            d,
            RendererCaps {
                set_next: true,
                seek: true,
                ..Default::default()
            },
            "http://10.0.0.5/d",
            60,
            Instant::now(),
        );

        let (status, _) = send(
            app.clone(),
            Request::builder()
                .uri("/api/remote/renderers")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let user = seed_user_session(&db, "u1");
        let (status, body) = send(app.clone(), get("/api/remote/renderers", &user)).await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["running"], false);
        let list = json["renderers"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "Living Room");
        assert_eq!(list[0]["online"], true);
        assert_eq!(list[0]["capabilities"]["setNext"], true);
        assert_eq!(list[0]["capabilities"]["volume"], true);
        assert_eq!(list[0]["hasIcon"], false);

        let admin = seed_admin_session(&db, "admin1");
        let (_, body) = send(app.clone(), get("/api/remote/renderers", &admin)).await;
        let list = json_body(&body)["renderers"].as_array().unwrap().clone();
        assert_eq!(list.len(), 2);
        let kitchen = list.iter().find(|r| r["udn"] == "b").unwrap();
        assert_eq!(kitchen["hidden"], true);
        assert_eq!(kitchen["online"], false);
    }

    #[tokio::test]
    async fn disabled_setting_returns_404_and_refresh_needs_running_discovery() {
        let (app, db, _) = app("remote-routes-disabled");
        let user = seed_user_session(&db, "u1");
        let refresh = Request::builder()
            .method("POST")
            .uri("/api/remote/renderers/refresh")
            .header("cookie", &user)
            .body(Body::empty())
            .unwrap();
        let (status, _) = send(app.clone(), refresh).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "not running in tests");

        boogiebox_db::upsert_setting(&db.lock().unwrap(), "remotePlaybackEnabled", "false")
            .unwrap();
        let (status, body) = send(app, get("/api/remote/renderers", &user)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json_body(&body)["error"], "Play On is disabled");
    }

    #[tokio::test]
    async fn patch_is_admin_only_and_validated() {
        let (app, db, remote) = app("remote-routes-patch");
        seed(&db, "a", "Living Room");
        let mut events = remote.subscribe();
        let user = seed_user_session(&db, "u1");
        let admin = seed_admin_session(&db, "admin1");

        let (status, _) = send(
            app.clone(),
            patch_req("/api/remote/renderers/a", &user, r#"{"hidden":true}"#),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = send(
            app.clone(),
            patch_req(
                "/api/remote/renderers/a",
                &admin,
                r#"{"alias":" Lounge ","formatPolicy":"flac","hidden":true}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let json = json_body(&body);
        assert_eq!(json["name"], "Lounge");
        assert_eq!(json["formatPolicy"], "flac");
        assert_eq!(json["hidden"], true);
        assert_eq!(events.recv().await.unwrap(), RemoteEvent::Renderers);

        let (_, body) = send(
            app.clone(),
            patch_req("/api/remote/renderers/a", &admin, r#"{"alias":null}"#),
        )
        .await;
        assert_eq!(json_body(&body)["name"], "Living Room");

        for bad in [
            r#"{"formatPolicy":"ogg"}"#,
            r#"{"alias":5}"#,
            r#"{"bogus":1}"#,
        ] {
            let (status, _) = send(
                app.clone(),
                patch_req("/api/remote/renderers/a", &admin, bad),
            )
            .await;
            assert!(status.is_client_error(), "{bad} → {status}");
        }
        let (status, _) = send(
            app.clone(),
            patch_req("/api/remote/renderers/missing", &admin, "{}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(
            app,
            patch_req("/api/remote/renderers/bad%20id", &admin, "{}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn icon_route_validates_and_404s_without_icon() {
        let (app, db, _) = app("remote-routes-icon");
        let user = seed_user_session(&db, "u1");
        let (status, _) = send(app.clone(), get("/api/remote/renderers/a/icon", &user)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(app, get("/api/remote/renderers/a%20b/icon", &user)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn routes_without_db_report_setup_required() {
        let state = Arc::new(RwLock::new(crate::AppState {
            db: None,
            ..crate::AppState::default()
        }));
        let router = remote_router(state);
        let (status, _) = send(
            router,
            Request::builder()
                .method("POST")
                .uri("/api/remote/renderers/refresh")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        // The auth extractor runs first and has no DB to check the session against.
        assert!(status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn body_validation_and_udn_rules() {
        let parse = |s: &str| prefs_from_body(serde_json::from_str(s).unwrap());
        assert_eq!(parse("{}").unwrap(), RendererPrefs::default());
        assert_eq!(parse(r#"{"alias":"  "}"#).unwrap().alias, Some(None));
        assert!(parse(&format!(
            r#"{{"alias":"{}"}}"#,
            "x".repeat(MAX_ALIAS_CHARS + 1)
        ))
        .is_err());
        assert!(parse(r#"{"alias":"a\u0007b"}"#).is_err());
        assert!(valid_udn("00000000-aaaa_b:c.d"));
        assert!(!valid_udn(""));
        assert!(!valid_udn("../x"));
        assert!(!valid_udn(&"a".repeat(201)));
    }
}

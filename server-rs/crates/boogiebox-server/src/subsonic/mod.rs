//! OpenSubsonic-compatible API at `/rest/{method}` (and `/rest/{method}.view`), so third-party Subsonic apps
//! can browse, search, stream and scrobble. Off by default (`subsonicApiEnabled`); apps sign in with the
//! user's Subsonic password, never the PIN. See wip/opensubsonic-api-implementation-plan.md.

pub mod auth;
pub mod crypto;
pub mod handlers;
pub mod media;
pub mod model;
pub mod params;
pub mod response;
pub mod state;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{HeaderMap, Method},
    response::Response,
    routing::get,
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub use state::{SubsonicState, SubsonicUser};

use self::{
    handlers::{Ctx, Reply},
    params::Params,
    response::{error_response, ok_response, Format, SubsonicError},
};
use crate::SharedState;

/// Form bodies only carry parameters.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// The `/rest` router. Credentials travel in parameters, not cookies, so CORS allows any origin without
/// credentials (web Subsonic clients such as Feishin web).
pub fn router(state: SharedState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);
    Router::new()
        .route("/rest/{method}", get(dispatch).post(dispatch))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(cors)
        .with_state(state)
}

async fn dispatch(
    State(state): State<SharedState>,
    method: Method,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let params = Params::parse(query.as_deref(), &body);
    let format = Format::from_params(params.get("f"), params.get("callback"));
    let name = name.strip_suffix(".view").unwrap_or(&name).to_owned();

    let (db, db_folder, subsonic, tracker) = {
        let s = state.read().unwrap_or_else(|p| p.into_inner());
        (
            s.db.clone(),
            s.db_folder.clone(),
            s.subsonic.clone(),
            s.login_attempts.clone(),
        )
    };
    let Some(db) = db else {
        return error_response(
            &format,
            &SubsonicError::generic("Server setup is not complete"),
        );
    };

    // OpenSubsonic: extensions are discoverable before signing in (still only while the API is on).
    if name == "getOpenSubsonicExtensions" {
        let (check_state, check_db) = (subsonic.clone(), db.clone());
        let enabled = tokio::task::spawn_blocking(move || check_state.settings(&check_db).enabled)
            .await
            .unwrap_or(false);
        return if enabled {
            ok_response(&format, handlers::open_subsonic_extensions())
        } else {
            error_response(
                &format,
                &SubsonicError::generic("The Subsonic API is turned off on this server"),
            )
        };
    }

    let auth_params = params.clone();
    let auth_db = db.clone();
    let auth_state = subsonic.clone();
    let authed = tokio::task::spawn_blocking(move || {
        let ctx = auth::AuthContext {
            subsonic: &auth_state,
            db: &auth_db,
            db_folder: db_folder.as_deref(),
            tracker: &tracker,
        };
        let user = auth::authenticate(&ctx, &auth_params)?;
        Ok::<_, SubsonicError>((user, auth_state.settings(&auth_db)))
    })
    .await;
    let (user, settings) = match authed {
        Ok(Ok(v)) => v,
        Ok(Err(err)) => return error_response(&format, &err),
        Err(_) => return error_response(&format, &SubsonicError::generic("Internal error")),
    };

    let ctx = Ctx {
        state,
        db,
        subsonic,
        user,
        params,
        headers,
        head: method == Method::HEAD,
        settings,
    };
    match handlers::handle(&name, &ctx).await {
        Ok(Reply::Payload(node)) => ok_response(&format, node),
        Ok(Reply::Raw(resp)) => resp,
        Err(err) => error_response(&format, &err),
    }
}

#[cfg(test)]
mod route_tests;

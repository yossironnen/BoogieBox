//! Axum application assembly, setup flow, logging, and server runtime wiring.

use axum::{
    extract::{ConnectInfo, State},
    http::{header, Method, StatusCode},
    response::IntoResponse,
    routing::{any, get, post},
    Json, Router,
};
use boogiebox_db::{database_exists, init_db, InitDbError};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env, fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use thiserror::Error;
use tokio::{process::Command, time::timeout};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
};
pub mod artwork_cache;
pub mod auth;
pub mod bpm_analysis;
pub mod cors;
pub mod deep_analysis;
pub mod dlna;
pub mod event_log;
pub mod ffmpeg;
pub mod image_thumb;
pub mod logging;
pub mod mix_worker;
pub mod post_scan;
pub mod providers;
pub mod routes;
pub mod scanner;
pub mod server_config;
pub mod settings;
pub mod similar_artists;
pub mod waveform_map;

const DEFAULT_PORT: u16 = 3001;

/// Shared SQLite connection wrapper used by route handlers and background workers.
pub type DbPool = Arc<Mutex<rusqlite::Connection>>;
/// Shared mutable application state used by Axum extractors.
pub type SharedState = Arc<RwLock<AppState>>;

/// Public Server Error data shape used by BoogieBox.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("invalid PORT value: {0}")]
    InvalidPort(String),
    #[error("failed to bind server: {0}")]
    Bind(#[from] std::io::Error),
}

/// Public Server Config data shape used by BoogieBox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    /// TCP port the Axum server should bind.
    pub port: u16,
    /// Optional Vite build directory served as the web client.
    pub client_build_dir: Option<PathBuf>,
}

impl ServerConfig {
    /// Builds server configuration from environment variables and client-build discovery.
    pub fn from_env() -> Result<Self, ServerError> {
        let port = match env::var("PORT") {
            Ok(raw) if !raw.trim().is_empty() => raw
                .trim()
                .parse::<u16>()
                .map_err(|_| ServerError::InvalidPort(raw))?,
            _ => DEFAULT_PORT,
        };

        Ok(Self {
            port,
            client_build_dir: resolve_client_build_dir(),
        })
    }
}

/// Public App State data shape used by BoogieBox.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Whether first-run setup is still required before API routes can serve data.
    pub setup_required: bool,
    /// Whether the resolved FFmpeg executable is available to the server.
    pub ffmpeg_available: bool,
    /// Resolved FFmpeg executable path used by transcoding and analysis jobs.
    pub ffmpeg_path: PathBuf,
    /// Optional durable log file path reported by system status.
    pub log_file: Option<PathBuf>,
    /// Scan/post-scan debug log path (populated when `scanDebugLoggingEnabled` is on).
    pub scan_debug_log_file: Option<PathBuf>,
    /// BoogieMix deep-analysis debug log path (populated when `deepmixDebugLoggingEnabled` is on).
    pub deep_debug_log_file: Option<PathBuf>,
    /// Suggested local database folder shown during first-run setup.
    pub suggested_db_folder: PathBuf,
    /// Writable locator path for the persisted database-folder config.
    pub db_config_path: PathBuf,
    /// Folder picker implementation used by setup-only local folder selection.
    pub folder_picker: FolderPicker,
    /// Open database pool after setup has completed.
    pub db: Option<DbPool>,
    /// In-memory login throttling state shared by auth extractors.
    pub login_attempts: auth::LoginAttemptTracker,
    /// Shared HTTP client for provider lookups and background jobs.
    pub http_client: reqwest::Client,
    /// Configured database folder, retained for jobs that need asset paths.
    pub db_folder: Option<PathBuf>,
    /// DLNA manager used to start, stop, and report the optional music server.
    pub dlna_manager: dlna::DlnaManager,
    /// Cancelled and replaced whenever the active database is switched, so background
    /// workers bound to the previous pool stop instead of continuing to run against it.
    pub worker_cancel: tokio_util::sync::CancellationToken,
}

impl Default for AppState {
    fn default() -> Self {
        let db_config = server_config::read_db_config();
        let setup_required = is_setup_required(db_config.as_ref());

        let db_folder = db_config
            .as_ref()
            .map(|config| PathBuf::from(&config.db_folder));

        let db = if !setup_required {
            db_config.as_ref().and_then(|config| {
                init_db(Path::new(&config.db_folder))
                    .ok()
                    .map(|initialized| Arc::new(Mutex::new(initialized.connection)))
            })
        } else {
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("BoogieBox/1.0")
            .build()
            .unwrap_or_default();

        let ffmpeg_path = ffmpeg::resolve_ffmpeg();
        let log_file = resolve_log_file();
        let log_paths = log_file.as_deref().and_then(logging::resolve_log_paths);

        Self {
            setup_required,
            ffmpeg_available: ffmpeg::ffmpeg_available(),
            ffmpeg_path,
            log_file,
            scan_debug_log_file: log_paths.as_ref().map(|p| p.scan_debug_log.clone()),
            deep_debug_log_file: log_paths.as_ref().map(|p| p.deep_debug_log.clone()),
            suggested_db_folder: suggested_db_folder(),
            db_config_path: server_config::get_writable_db_locator_path(),
            folder_picker: FolderPicker::System,
            db,
            login_attempts: auth::LoginAttemptTracker::default(),
            http_client,
            db_folder,
            dlna_manager: dlna::new_dlna_manager(),
            worker_cancel: tokio_util::sync::CancellationToken::new(),
        }
    }
}

/// Public Folder Picker data shape used by BoogieBox.
#[derive(Clone, Debug)]
pub enum FolderPicker {
    System,
    Fixed(Option<String>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    app: &'static str,
    server: &'static str,
    version: &'static str,
    discovery: bool,
    ffmpeg_available: bool,
    ffmpeg_path: String,
    ffprobe_available: bool,
    log_file: Option<String>,
    scan_debug_log_file: Option<String>,
    deep_debug_log_file: Option<String>,
    setup_required: bool,
    suggested_db_folder: String,
    db_folder: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupRequest {
    db_folder: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectFolderRequest {
    initial_dir: Option<String>,
}

#[derive(Debug, Serialize)]
struct SelectFolderResponse {
    folder: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DebugTestPathRequest {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugTestPathResponse {
    input: String,
    normalized: String,
    exists: bool,
    is_directory: Option<bool>,
    display_name: String,
    exists_error: Option<String>,
    stat_error: Option<String>,
}

/// Public Ok Response data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct OkResponse {
    /// Documents the Ok public API surface.
    pub ok: bool,
}

/// Public Error Response data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    /// Documents the Error public API surface.
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_required: Option<bool>,
}

/// Documents the Run From Env public API surface.
pub async fn run_from_env() -> Result<(), ServerError> {
    let log_file = init_logging();
    let config = ServerConfig::from_env()?;
    let state = AppState::default();
    let startup_banner = format!(
        "BoogieBox Rust server starting; version={}; log_file={}; ffmpeg_path={}; ffmpeg_available={}; ffprobe_available={}",
        env!("CARGO_PKG_VERSION"),
        log_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string()),
        state.ffmpeg_path.display(),
        state.ffmpeg_available,
        ffmpeg::ffprobe_available()
    );
    // The startup banner must reach the console, server.log, and the Windows
    // Event Log no matter how logging is configured — a stray
    // `BOOGIEBOX_LOG_LEVEL=error` or a disabled debug toggle shouldn't be
    // able to hide it. All three writes bypass the tracing filter pipeline
    // entirely (a `tracing::info!` here would double-print to the console,
    // since the server layer already tees its writes to stdout).
    println!("{startup_banner}");
    event_log::write_startup_event(&startup_banner);
    if let Some(path) = log_file.as_ref() {
        logging::append_log_marker(path, &startup_banner);
    }
    if let Some(db) = state.db.clone() {
        {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let scan_debug = boogiebox_db::boogiemix::get_setting(&conn, "scanDebugLoggingEnabled")
                .map(|v| v == "true")
                .unwrap_or(false);
            let deep_debug =
                boogiebox_db::boogiemix::get_setting(&conn, "deepmixDebugLoggingEnabled")
                    .map(|v| v == "true")
                    .unwrap_or(false);
            logging::sync_debug_toggles(scan_debug, deep_debug);
            match boogiebox_db::jobs::reset_orphaned_scan_jobs(&conn) {
                Ok(n) if n > 0 => tracing::info!("Startup: recovered {n} orphaned scan job(s)"),
                Ok(_) => {}
                Err(err) => tracing::warn!("Startup scan-job recovery failed: {err}"),
            }
            match boogiebox_db::jobs::reset_orphaned_post_scan_jobs(&conn) {
                Ok(n) if n > 0 => {
                    tracing::info!("Startup: recovered {n} orphaned post-scan job(s)")
                }
                Ok(_) => {}
                Err(err) => tracing::warn!("Startup post-scan-job recovery failed: {err}"),
            }
            match boogiebox_db::boogiemix::reset_orphaned_mix_jobs(&conn) {
                Ok(n) if n > 0 => tracing::info!("Startup: recovered {n} orphaned mix job(s)"),
                Ok(_) => {}
                Err(err) => tracing::warn!("Startup mix-job recovery failed: {err}"),
            }
        }
        let dlna_db = db.clone();
        let cancel = state.worker_cancel.clone();
        let ps_state = post_scan::PostScanState {
            db,
            http_client: state.http_client.clone(),
            db_folder: state.db_folder.clone(),
            cancel: cancel.clone(),
        };
        scanner::start_scan_scheduler(ps_state.clone());
        post_scan::start_post_scan_scheduler(ps_state.clone());
        waveform_map::start_waveform_map_scheduler(ps_state.db.clone(), cancel.clone());
        bpm_analysis::start_bpm_analysis_scheduler(ps_state.db.clone(), cancel.clone());
        deep_analysis::start_deep_analysis_worker(ps_state.clone());
        mix_worker::start_mix_worker(ps_state);
        let dlna_mgr = state.dlna_manager.clone();
        tokio::spawn(dlna::start_dlna_if_enabled(dlna_mgr, dlna_db));
    }
    let app = build_app(state, config.client_build_dir.clone());
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(
        "BoogieBox Rust server listening on http://localhost:{}",
        config.port
    );
    if let Some(client_dir) = config.client_build_dir {
        tracing::info!("Serving client from: {}", client_dir.display());
    } else {
        tracing::warn!("Client build not found; API status endpoint only");
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Initializes the three-way log split (`server.log` + the two opt-in debug
/// logs, see [`logging`]). `scan_enabled`/`deep_enabled` reflect the last
/// known settings state (env-var override only at this point — the DB isn't
/// open yet); [`logging::sync_debug_toggles`] reconciles against the real
/// settings once it is.
fn init_logging() -> Option<PathBuf> {
    let server_log = resolve_log_file().filter(|path| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).is_ok()
        } else {
            false
        }
    })?;
    let paths = logging::resolve_log_paths(&server_log)?;
    let scan_enabled = env::var("BOOGIEBOX_SCAN_DEBUG_LOGGING")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let deep_enabled = env::var("BOOGIEBOX_DEEPMIX_DEBUG_LOGGING")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    logging::init(&paths, scan_enabled, deep_enabled);
    Some(paths.server_log)
}

/// Documents the Resolve Log File public API surface.
pub fn resolve_log_file() -> Option<PathBuf> {
    if let Ok(raw) = env::var("BOOGIEBOX_LOG_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    if let Ok(raw) = env::var("BOOGIEBOX_LOG_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed).join("boogiebox-server.log"));
        }
    }

    if let Ok(raw) = env::var("BOOGIEBOX_DEBUG_LOG_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    if let Ok(program_data) = env::var("PROGRAMDATA") {
        let trimmed = program_data.trim();
        if !trimmed.is_empty() {
            return Some(
                PathBuf::from(trimmed)
                    .join("BoogieBox")
                    .join("logs")
                    .join("boogiebox-server.log"),
            );
        }
    }

    // Linux equivalent of the PROGRAMDATA branch above: the packaged systemd
    // unit (installer/linux/boogiebox.service) already sets this to
    // /var/lib/boogiebox, which is owned by the service user, so logs land
    // in /var/lib/boogiebox/logs instead of inside the install directory
    // (/opt/boogiebox) that the exe-adjacent fallback below would otherwise use.
    if let Ok(config_dir) = env::var("BOOGIEBOX_CONFIG_DIR") {
        let trimmed = config_dir.trim();
        if !trimmed.is_empty() {
            return Some(
                PathBuf::from(trimmed)
                    .join("logs")
                    .join("boogiebox-server.log"),
            );
        }
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            return Some(dir.join("logs").join("boogiebox-server.log"));
        }
    }

    env::current_dir()
        .ok()
        .map(|dir| dir.join("logs").join("boogiebox-server.log"))
}

/// Documents the Build App public API surface.
pub fn build_app(state: AppState, client_build_dir: Option<PathBuf>) -> Router {
    let shared_state: SharedState = Arc::new(RwLock::new(state));
    let allowed_origins = cors::allowed_origins_from_env();

    let cors_layer = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _req| {
            cors::is_allowed_origin_with_config(origin.to_str().unwrap_or(""), &allowed_origins)
        }))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::COOKIE, header::AUTHORIZATION]);

    // State-erase the core system routes first, then merge the sub-routers
    // (which are also state-erased via .with_state inside their builder fn).
    let api = Router::new()
        .route("/api/system/status", get(status_handler))
        .route("/api/system/setup", post(setup_handler))
        .route("/api/system/switch-db", post(switch_db_handler))
        .route("/api/system/select-folder", post(select_folder_handler))
        .route("/api/debug/test-path", post(debug_test_path_handler))
        .route("/api/{*path}", any(api_fallback_handler))
        .with_state(shared_state.clone())
        .merge(routes::auth_routes::auth_router(shared_state.clone()))
        .merge(routes::admin_routes::admin_router(shared_state.clone()))
        .merge(routes::settings_routes::settings_router(
            shared_state.clone(),
        ))
        .merge(routes::library_routes::library_router(shared_state.clone()))
        .merge(routes::music_routes::music_router(shared_state.clone()))
        .merge(routes::playlist_routes::playlist_router(
            shared_state.clone(),
        ))
        .merge(routes::playback_routes::playback_router(
            shared_state.clone(),
        ))
        .merge(routes::artwork_routes::artwork_router(shared_state.clone()))
        .merge(routes::provider_routes::provider_router(
            shared_state.clone(),
        ))
        .merge(routes::crossfade_routes::crossfade_router(
            shared_state.clone(),
        ))
        .merge(routes::boogiemix_routes::boogiemix_router(
            shared_state.clone(),
        ))
        .merge(routes::dlna_routes::dlna_router(shared_state))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(cors_layer);

    if let Some(client_dir) = client_build_dir {
        let index = client_dir.join("index.html");
        api.fallback_service(ServeDir::new(client_dir).fallback(ServeFile::new(index)))
    } else {
        api
    }
}

async fn debug_test_path_handler(
    _admin: auth::AdminUser,
    Json(payload): Json<DebugTestPathRequest>,
) -> impl IntoResponse {
    let raw_path = payload.path.trim();
    if raw_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "path required" })),
        )
            .into_response();
    }

    let path = PathBuf::from(raw_path);
    let normalized_path = path.components().collect::<PathBuf>();
    let normalized = normalized_path.to_string_lossy().into_owned();

    let exists = normalized_path.exists();
    let mut stat_error = None;
    let is_directory = if exists {
        match fs::metadata(&normalized_path) {
            Ok(metadata) => Some(metadata.is_dir()),
            Err(err) => {
                stat_error = Some(err.to_string());
                None
            }
        }
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(DebugTestPathResponse {
            input: payload.path,
            normalized: normalized.clone(),
            exists,
            is_directory,
            display_name: normalized_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&normalized)
                .to_string(),
            exists_error: None,
            stat_error,
        }),
    )
        .into_response()
}

async fn select_folder_handler(
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<SelectFolderRequest>,
) -> impl IntoResponse {
    let state_snapshot = state
        .read()
        .expect("state lock should not be poisoned")
        .clone();
    if !state_snapshot.setup_required {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Already configured".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    if !is_loopback_addr(&addr) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Folder picker is only available from the server machine".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    if !cfg!(windows) {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorResponse {
                error: "Folder picker is not available on Linux — type the path manually"
                    .to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    match pick_database_folder(
        state_snapshot.folder_picker,
        payload.initial_dir.as_deref(),
        &state_snapshot.suggested_db_folder,
    )
    .await
    {
        Ok(folder) => (StatusCode::OK, Json(SelectFolderResponse { folder })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Folder picker failed: {err}"),
                setup_required: None,
            }),
        )
            .into_response(),
    }
}

async fn status_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let state = state.read().unwrap_or_else(|p| p.into_inner());
    (
        StatusCode::OK,
        [("X-BoogieBox-Server", "1")],
        Json(StatusResponse {
            app: "BoogieBox",
            server: "boogiebox",
            version: env!("CARGO_PKG_VERSION"),
            discovery: true,
            ffmpeg_available: state.ffmpeg_available,
            ffmpeg_path: state.ffmpeg_path.to_string_lossy().into_owned(),
            ffprobe_available: ffmpeg::ffprobe_available(),
            log_file: state
                .log_file
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            scan_debug_log_file: state
                .scan_debug_log_file
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            deep_debug_log_file: state
                .deep_debug_log_file
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            setup_required: state.setup_required,
            suggested_db_folder: state.suggested_db_folder.to_string_lossy().into_owned(),
            db_folder: state
                .db_folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }),
    )
}

async fn setup_handler(
    State(state): State<SharedState>,
    Json(payload): Json<SetupRequest>,
) -> impl IntoResponse {
    if !state
        .read()
        .expect("state lock should not be poisoned")
        .setup_required
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Already configured".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    }

    let Some(db_folder) = payload
        .db_folder
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "dbFolder is required".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let db_config_path = state
        .read()
        .expect("state lock should not be poisoned")
        .db_config_path
        .clone();

    match server_config::write_db_config_to(&db_config_path, Path::new(db_folder)) {
        Ok(_) => match init_db(Path::new(db_folder)) {
            Ok(initialized) => {
                let db_pool = Arc::new(Mutex::new(initialized.connection));
                let mut s = state.write().unwrap_or_else(|p| p.into_inner());
                s.setup_required = false;
                s.db = Some(db_pool);
                s.db_folder = Some(PathBuf::from(db_folder));
                s.worker_cancel.cancel();
                let cancel = tokio_util::sync::CancellationToken::new();
                s.worker_cancel = cancel.clone();
                let ps_state = post_scan::PostScanState {
                    db: s.db.as_ref().expect("db set").clone(),
                    http_client: s.http_client.clone(),
                    db_folder: s.db_folder.clone(),
                    cancel: cancel.clone(),
                };
                scanner::start_scan_scheduler(ps_state.clone());
                post_scan::start_post_scan_scheduler(ps_state.clone());
                waveform_map::start_waveform_map_scheduler(ps_state.db.clone(), cancel.clone());
                bpm_analysis::start_bpm_analysis_scheduler(ps_state.db.clone(), cancel.clone());
                deep_analysis::start_deep_analysis_worker(ps_state.clone());
                mix_worker::start_mix_worker(ps_state);
                let dlna_mgr = s.dlna_manager.clone();
                let dlna_db = s.db.as_ref().expect("db set").clone();
                tokio::spawn(dlna::start_dlna_if_enabled(dlna_mgr, dlna_db));
                (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
            }
            Err(err) => map_init_db_error(err).into_response(),
        },
        Err(server_config::DbConfigWriteError::CreateDir { source, .. }) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Cannot create folder: {source}"),
                setup_required: None,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Setup failed: {err}"),
                setup_required: None,
            }),
        )
            .into_response(),
    }
}

async fn switch_db_handler(
    State(state): State<SharedState>,
    Json(payload): Json<SetupRequest>,
) -> impl IntoResponse {
    {
        let s = state.read().expect("state lock");
        if s.setup_required {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Server not configured yet; use /api/system/setup first".to_string(),
                    setup_required: Some(true),
                }),
            )
                .into_response();
        }
        if s.db.is_none() {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "No active database".to_string(),
                    setup_required: None,
                }),
            )
                .into_response();
        }
    }

    let Some(db_folder) = payload
        .db_folder
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "dbFolder is required".to_string(),
                setup_required: None,
            }),
        )
            .into_response();
    };

    let db_config_path = state.read().expect("state lock").db_config_path.clone();

    match server_config::write_db_config_to(&db_config_path, Path::new(db_folder)) {
        Ok(_) => match init_db(Path::new(db_folder)) {
            Ok(initialized) => {
                let db_pool = Arc::new(Mutex::new(initialized.connection));
                let mut s = state.write().unwrap_or_else(|p| p.into_inner());
                s.db = Some(db_pool);
                s.db_folder = Some(PathBuf::from(db_folder));
                s.worker_cancel.cancel();
                let cancel = tokio_util::sync::CancellationToken::new();
                s.worker_cancel = cancel.clone();
                let ps_state = post_scan::PostScanState {
                    db: s.db.as_ref().expect("db set").clone(),
                    http_client: s.http_client.clone(),
                    db_folder: s.db_folder.clone(),
                    cancel: cancel.clone(),
                };
                scanner::start_scan_scheduler(ps_state.clone());
                post_scan::start_post_scan_scheduler(ps_state.clone());
                waveform_map::start_waveform_map_scheduler(ps_state.db.clone(), cancel.clone());
                bpm_analysis::start_bpm_analysis_scheduler(ps_state.db.clone(), cancel.clone());
                deep_analysis::start_deep_analysis_worker(ps_state.clone());
                mix_worker::start_mix_worker(ps_state);
                let dlna_mgr = s.dlna_manager.clone();
                let dlna_db = s.db.as_ref().expect("db set").clone();
                tokio::spawn(dlna::start_dlna_if_enabled(dlna_mgr, dlna_db));
                (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
            }
            Err(err) => map_init_db_error(err).into_response(),
        },
        Err(server_config::DbConfigWriteError::CreateDir { source, .. }) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Cannot create folder: {source}"),
                setup_required: None,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Switch failed: {err}"),
                setup_required: None,
            }),
        )
            .into_response(),
    }
}

fn is_setup_required(db_config: Option<&server_config::DbConfig>) -> bool {
    db_config.is_none_or(|config| !database_exists(Path::new(&config.db_folder)))
}

fn map_init_db_error(err: InitDbError) -> (StatusCode, Json<ErrorResponse>) {
    match err {
        InitDbError::EmptyDbFolder => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "dbFolder is required".to_string(),
                setup_required: None,
            }),
        ),
        InitDbError::CreateDir { source, .. } => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Cannot create folder: {source}"),
                setup_required: None,
            }),
        ),
        other => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Setup failed: {other}"),
                setup_required: None,
            }),
        ),
    }
}

async fn api_fallback_handler(State(state): State<SharedState>) -> impl IntoResponse {
    if state
        .read()
        .expect("state lock should not be poisoned")
        .setup_required
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Database not configured".to_string(),
                setup_required: Some(true),
            }),
        )
            .into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutdown signal received");
}

/// Documents the Suggested DB Folder public API surface.
pub fn suggested_db_folder() -> PathBuf {
    #[cfg(not(windows))]
    {
        // Prefer an explicit data-dir override (set by the systemd unit via
        // BOOGIEBOX_DATA_DIR, or conventionally by a package manager install).
        if let Ok(dir) = env::var("BOOGIEBOX_DATA_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }

        // If running as a system service user (no real HOME, or HOME is / or .),
        // fall back to the standard system data directory.
        let home = env::var("HOME").unwrap_or_default();
        let home = home.trim();
        if home.is_empty() || home == "." || home == "/" {
            return PathBuf::from("/var/lib/boogiebox/data");
        }

        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("BoogieBox")
    }
    #[cfg(windows)]
    {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            let trimmed = local_app_data.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("BoogieBox");
            }
        }
        let home = env::var("USERPROFILE")
            .or_else(|_| env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join("AppData")
            .join("Local")
            .join("BoogieBox")
    }
}

/// Documents the Client Build Candidates public API surface.
pub fn client_build_candidates() -> Vec<PathBuf> {
    let exec_dir = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut candidates = Vec::new();
    if let Ok(env_client_build) = env::var("BOOGIEBOX_CLIENT_BUILD_DIR") {
        let trimmed = env_client_build.trim();
        if !trimmed.is_empty() {
            candidates.push(to_absolute_path(trimmed, &cwd));
        }
    }

    candidates.extend([
        exec_dir.join("client").join("build"),
        exec_dir.join("resources").join("client").join("build"),
        cwd.join("client").join("build"),
        cwd.join("..").join("client").join("build"),
        manifest_dir
            .join("..")
            .join("..")
            .join("..")
            .join("client")
            .join("build"),
    ]);

    unique_paths(candidates)
}

/// Documents the Resolve Client Build Dir public API surface.
pub fn resolve_client_build_dir() -> Option<PathBuf> {
    client_build_candidates()
        .into_iter()
        .find(|candidate| candidate.join("index.html").is_file())
}

fn to_absolute_path(raw: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_lowercase();
        if seen.insert(key) {
            result.push(path);
        }
    }
    result
}

pub(crate) fn is_loopback_addr(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
    }
}

pub async fn pick_database_folder(
    picker: FolderPicker,
    initial_dir: Option<&str>,
    suggested_db_folder: &Path,
) -> Result<Option<String>, String> {
    match picker {
        FolderPicker::Fixed(folder) => Ok(folder),
        FolderPicker::System => pick_database_folder_from_system(initial_dir, suggested_db_folder)
            .await
            .map_err(|err| err.to_string()),
    }
}

pub async fn pick_folder(
    picker: FolderPicker,
    initial_dir: Option<&str>,
    description: &str,
) -> Result<Option<String>, String> {
    match picker {
        FolderPicker::Fixed(folder) => Ok(folder),
        FolderPicker::System => pick_folder_from_system(initial_dir, description)
            .await
            .map_err(|err| err.to_string()),
    }
}

async fn pick_folder_from_system(
    initial_dir: Option<&str>,
    description: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let selected_path = initial_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let script = build_folder_picker_script_with_description(&selected_path, description);
    let output = timeout(
        Duration::from_secs(10 * 60),
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-STA",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .stdin(Stdio::null())
            .output(),
    )
    .await??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("powershell exited with status {}", output.status).into()
        } else {
            stderr.into()
        });
    }

    let folder = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!folder.is_empty()).then_some(folder))
}

async fn pick_database_folder_from_system(
    initial_dir: Option<&str>,
    suggested_db_folder: &Path,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    if !cfg!(windows) {
        return Ok(None);
    }

    let selected_path = initial_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| suggested_db_folder.to_string_lossy().into_owned());
    let script = build_folder_picker_script(&selected_path);
    let output = timeout(
        Duration::from_secs(10 * 60),
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-STA",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .stdin(Stdio::null())
            .output(),
    )
    .await??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("powershell exited with status {}", output.status).into()
        } else {
            stderr.into()
        });
    }

    let folder = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!folder.is_empty()).then_some(folder))
}

fn build_folder_picker_script(selected_path: &str) -> String {
    build_folder_picker_script_with_description(selected_path, "Choose BoogieBox database folder")
}

pub fn build_folder_picker_script_with_description(
    selected_path: &str,
    description: &str,
) -> String {
    [
        "Add-Type -AssemblyName System.Windows.Forms".to_string(),
        "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog".to_string(),
        format!(
            "$dialog.Description = '{}'",
            escape_powershell_single_quoted(description)
        ),
        "$dialog.ShowNewFolderButton = $true".to_string(),
        format!(
            "$dialog.SelectedPath = '{}'",
            escape_powershell_single_quoted(selected_path)
        ),
        "if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::Out.Write($dialog.SelectedPath) }".to_string(),
    ]
    .join("; ")
}

fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::{fs, sync::Mutex, time::SystemTime};
    use tower::ServiceExt;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn status_endpoint_matches_boogiebox_discovery_contract() {
        let app = build_app(
            AppState {
                setup_required: true,
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/system/status")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("status request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("X-BoogieBox-Server").unwrap(), "1");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(json["app"], "BoogieBox");
        assert_eq!(json["server"], "boogiebox");
        assert_eq!(json["discovery"], true);
        assert_eq!(json["setupRequired"], true);
        assert!(json["ffmpegPath"].as_str().unwrap().contains("ffmpeg"));
        assert!(json.get("ffprobeAvailable").is_some());
        assert!(json.get("logFile").is_some());
        assert!(json["suggestedDbFolder"]
            .as_str()
            .unwrap()
            .contains("BoogieBox"));
    }

    #[tokio::test]
    async fn setup_endpoint_writes_config_and_clears_setup_required() {
        let root = temp_dir("boogiebox-setup");
        let locator = root.join("config").join("boogiebox-config.json");
        let db_folder = root.join("db");
        let db_file = db_folder.join("boogiebox.db");

        let app = build_app(
            AppState {
                setup_required: true,
                db_config_path: locator.clone(),
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/system/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"dbFolder":"{}"}}"#,
                        db_folder.to_string_lossy().replace('\\', "\\\\")
                    )))
                    .expect("request should build"),
            )
            .await
            .expect("setup request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(locator.is_file());
        assert!(db_folder.is_dir());
        assert!(db_file.is_file());
    }

    #[tokio::test]
    async fn setup_endpoint_rejects_blank_folder() {
        let app = build_app(
            AppState {
                setup_required: true,
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/system/setup")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"dbFolder":"   "}"#))
                    .expect("request should build"),
            )
            .await
            .expect("setup request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn select_folder_requires_loopback_request() {
        let app = build_app(
            AppState {
                setup_required: true,
                folder_picker: FolderPicker::Fixed(Some("C:\\Selected".to_string())),
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/system/select-folder")
                    .header("content-type", "application/json")
                    .extension(ConnectInfo(SocketAddr::from(([192, 168, 1, 20], 5000))))
                    .body(Body::from("{}"))
                    .expect("request should build"),
            )
            .await
            .expect("select-folder request should succeed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn select_folder_returns_picker_result_for_loopback_request() {
        let app = build_app(
            AppState {
                setup_required: true,
                folder_picker: FolderPicker::Fixed(Some("D:\\BoogieBox".to_string())),
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/system/select-folder")
                    .header("content-type", "application/json")
                    .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5000))))
                    .body(Body::from(r#"{"initialDir":"C:\\Start"}"#))
                    .expect("request should build"),
            )
            .await
            .expect("select-folder request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(json["folder"], "D:\\BoogieBox");
    }

    #[tokio::test]
    async fn unconfigured_api_routes_return_setup_required() {
        let app = build_app(
            AppState {
                setup_required: true,
                db: None,
                ..AppState::default()
            },
            None,
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/albums")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("api request should succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: Value = serde_json::from_slice(&body).expect("json body");

        assert_eq!(json["error"], "Database not configured");
        assert_eq!(json["setupRequired"], true);
    }

    #[test]
    fn setup_is_required_until_locator_points_to_real_database_file() {
        let root = temp_dir("app-state-missing-db");
        let db_folder = root.join("db");
        fs::create_dir_all(&db_folder).expect("db folder");

        let config = server_config::DbConfig {
            db_folder: db_folder.to_string_lossy().into_owned(),
        };
        assert!(is_setup_required(Some(&config)));

        fs::write(db_folder.join("boogiebox.db"), []).expect("db file");
        assert!(!is_setup_required(Some(&config)));
    }

    #[test]
    fn resolves_client_build_dir_from_env_override() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let dir = temp_dir("boogiebox-client-build");
        fs::create_dir_all(&dir).expect("temp client dir");
        fs::write(dir.join("index.html"), "<html></html>").expect("index");

        let previous = env::var("BOOGIEBOX_CLIENT_BUILD_DIR").ok();
        env::set_var("BOOGIEBOX_CLIENT_BUILD_DIR", &dir);
        let resolved = resolve_client_build_dir();
        restore_env("BOOGIEBOX_CLIENT_BUILD_DIR", previous);

        assert_eq!(resolved, Some(dir));
    }

    #[test]
    fn resolves_log_file_from_env_override() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let dir = temp_dir("boogiebox-logs");
        let previous_path = env::var("BOOGIEBOX_LOG_PATH").ok();
        let previous_dir = env::var("BOOGIEBOX_LOG_DIR").ok();
        env::set_var("BOOGIEBOX_LOG_DIR", &dir);
        env::remove_var("BOOGIEBOX_LOG_PATH");

        let resolved = resolve_log_file();

        restore_env("BOOGIEBOX_LOG_PATH", previous_path);
        restore_env("BOOGIEBOX_LOG_DIR", previous_dir);

        assert_eq!(resolved, Some(dir.join("boogiebox-server.log")));
    }

    #[test]
    fn rejects_invalid_port_values() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous = env::var("PORT").ok();
        env::set_var("PORT", "not-a-port");
        let result = ServerConfig::from_env();
        restore_env("PORT", previous);

        assert!(matches!(result, Err(ServerError::InvalidPort(_))));
    }

    #[test]
    fn detects_loopback_ipv4_ipv6_and_mapped_ipv4_addresses() {
        assert!(is_loopback_addr(&SocketAddr::from(([127, 0, 0, 1], 1))));
        assert!(is_loopback_addr(&SocketAddr::from((
            [0, 0, 0, 0, 0, 0, 0, 1],
            1
        ))));
        assert!(is_loopback_addr(&"[::ffff:127.0.0.1]:1".parse().unwrap()));
        assert!(!is_loopback_addr(&SocketAddr::from(([192, 168, 1, 5], 1))));
    }

    #[test]
    fn escapes_folder_picker_powershell_single_quotes() {
        let script = build_folder_picker_script("C:\\Yossi's Music");

        assert!(script.contains("C:\\Yossi''s Music"));
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("{prefix}-{nanos}"));
        path
    }

    fn restore_env(key: &str, value: Option<String>) {
        if let Some(value) = value {
            env::set_var(key, value);
        } else {
            env::remove_var(key);
        }
    }
}

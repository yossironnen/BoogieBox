//! Defines Tauri desktop shell logic for Commands.

use crate::{
    config::{self, DesktopConfig},
    server,
    server_process::{self, PackagedServerStatus, ServerProcessStore},
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

// ─── Server ──────────────────────────────────────────────────────────────────

/// Probe the BoogieBox server and return reachability + version info.
#[tauri::command]
pub async fn probe_server(app: AppHandle, url: Option<String>) -> server::ServerProbeResult {
    let target = url.or_else(|| config::load(&app).server_url);
    match target {
        Some(u) => server::probe(&u).await,
        None => server::ServerProbeResult {
            reachable: false,
            url: String::new(),
            version: None,
            app: None,
            setup_required: None,
            error: Some("No server URL configured".to_string()),
        },
    }
}

/// Probe localhost and the local IPv4 network for running BoogieBox servers.
#[tauri::command]
pub async fn discover_servers() -> server::ServerDiscoveryResult {
    server::discover().await
}

/// Start a packaged boogiebox-server.exe if one can be resolved.
#[tauri::command]
pub fn start_packaged_server(
    app: AppHandle,
    store: State<ServerProcessStore>,
    path: Option<String>,
) -> PackagedServerStatus {
    server_process::start(&app, &store, path)
}

/// Stop the packaged server process started by this desktop shell.
#[tauri::command]
pub fn stop_packaged_server(store: State<ServerProcessStore>) -> PackagedServerStatus {
    server_process::stop(&store)
}

/// Return the current packaged server process state.
#[tauri::command]
pub fn packaged_server_status(store: State<ServerProcessStore>) -> PackagedServerStatus {
    server_process::status(&store)
}

// ─── Config ──────────────────────────────────────────────────────────────────

/// Return the current desktop configuration.
#[tauri::command]
pub fn get_config(app: AppHandle) -> DesktopConfig {
    config::load(&app)
}

/// Update the configured server URL and save to disk.
/// Navigates the main window to the new URL if provided.
#[tauri::command]
pub fn set_server_url(app: AppHandle, url: String) -> Result<(), String> {
    let mut cfg = config::load(&app);
    cfg.server_url = if url.is_empty() {
        None
    } else {
        Some(url.clone())
    };
    config::save(&app, &cfg)?;

    if !url.is_empty() {
        if let Some(window) = app.get_webview_window("main") {
            let nav_url: url::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
            window.navigate(nav_url).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ─── Video ───────────────────────────────────────────────────────────────────

/// Open a native folder picker and return the selected path.
#[tauri::command]
pub async fn select_folder(app: AppHandle, initial_dir: Option<String>) -> Option<String> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose BoogieBox database folder");

    if let Some(dir) = initial_dir {
        let path = PathBuf::from(dir);
        if path.exists() && path.is_dir() {
            dialog = dialog.set_directory(path);
        }
    }

    dialog
        .blocking_pick_folder()
        .and_then(|folder| folder.into_path().ok())
        .map(|path| path.to_string_lossy().to_string())
}


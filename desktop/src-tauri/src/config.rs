//! Persistent desktop-shell configuration for server discovery and startup.

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::AppHandle;

const CONFIG_FILE: &str = "desktop-config.json";

/// Public Desktop Config data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfig {
    /// URL of the BoogieBox server, for example `http://localhost:3001`.
    pub server_url: Option<String>,
    /// Window width persisted from a previous desktop session.
    pub window_width: Option<u32>,
    /// Window height persisted from a previous desktop session.
    pub window_height: Option<u32>,
    /// Whether the shell should restore as maximized when startup handling supports it.
    pub start_maximized: Option<bool>,
    /// Optional absolute path to a packaged `boogiebox-server.exe`.
    pub server_exe_path: Option<String>,
    /// Whether the desktop shell should auto-start the configured packaged server.
    pub auto_start_packaged_server: Option<bool>,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            server_url: Some("http://localhost:3001".to_string()),
            window_width: None,
            window_height: None,
            start_maximized: None,
            server_exe_path: None,
            auto_start_packaged_server: Some(false),
        }
    }
}

/// Resolves the per-user desktop configuration file path.
pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "BoogieBox", "BoogieBox")
        .map(|dirs| dirs.config_dir().join(CONFIG_FILE))
}

/// Ensures the desktop config directory exists and seeds default config if missing.
pub fn init(_app: &AppHandle) {
    // Ensure config directory exists
    if let Some(path) = config_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // Write defaults if missing
        if !path.exists() {
            let defaults = DesktopConfig::default();
            if let Ok(json) = serde_json::to_string_pretty(&defaults) {
                let _ = fs::write(&path, json);
            }
        }
    }
}

/// Loads desktop configuration from disk, falling back to defaults on read errors.
pub fn load(_app: &AppHandle) -> DesktopConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return DesktopConfig::default(),
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return DesktopConfig::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Persists desktop configuration to the resolved config path.
pub fn save(_app: &AppHandle, cfg: &DesktopConfig) -> Result<(), String> {
    let path = config_path().ok_or("cannot determine config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

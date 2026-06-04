//! Defines Tauri desktop shell logic for Lib.

pub mod commands;
pub mod config;
pub mod server;
pub mod server_process;
pub mod windows;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

/// Documents the Run public API surface.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "BoogieBox desktop starting"
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .manage(server_process::ServerProcessStore::default())
        .setup(|app| {
            config::init(app.handle());
            let cfg = config::load(app.handle());
            tracing::info!(
                server_url = cfg.server_url.as_deref().unwrap_or("(not configured)"),
                "desktop config loaded"
            );
            if cfg.auto_start_packaged_server.unwrap_or(false) {
                let status = server_process::start(
                    app.handle(),
                    app.state::<server_process::ServerProcessStore>().inner(),
                    None,
                );
                tracing::info!(
                    running = status.running,
                    pid = status.pid,
                    error = status.error,
                    "packaged server auto-start attempted"
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::probe_server,
            commands::discover_servers,
            commands::start_packaged_server,
            commands::stop_packaged_server,
            commands::packaged_server_status,
            commands::get_config,
            commands::set_server_url,
            commands::select_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error running BoogieBox desktop");
}

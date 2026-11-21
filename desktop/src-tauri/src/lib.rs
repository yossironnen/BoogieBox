//! Defines Tauri desktop shell logic for Lib.

pub mod audio;
pub mod commands;
pub mod config;
pub mod media_controls;
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
        .manage(audio::AudioState::default())
        .manage(media_controls::MediaControls::default())
        .setup(|app| {
            config::init(app.handle());
            let cfg = config::load(app.handle());
            if let Some(url) = cfg.server_url.as_deref() {
                audio::allow_page(app.handle(), url);
            }
            media_controls::start(app.handle());
            audio::start(app.handle(), app.state::<audio::AudioState>().inner());
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
            audio::audio_command,
            audio::audio_devices,
            audio::audio_settings,
            audio::audio_save_settings,
            audio::machine_name,
            media_controls::media_now_playing,
            media_controls::media_clear,
        ])
        .build(tauri::generate_context!())
        .expect("error building BoogieBox desktop")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                audio::stop(app.state::<audio::AudioState>().inner());
            }
        });
}

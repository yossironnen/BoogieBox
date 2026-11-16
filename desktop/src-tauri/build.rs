//! Defines Build behavior for BoogieBox.

/// Every app command: listing them gives each an `allow-…` permission, so the bundled pages
/// (`capabilities/default.json`) and the server's page (runtime capability in `audio::allow_page`) can be given
/// different sets.
const COMMANDS: &[&str] = &[
    "probe_server",
    "discover_servers",
    "start_packaged_server",
    "stop_packaged_server",
    "packaged_server_status",
    "get_config",
    "set_server_url",
    "select_folder",
    "audio_command",
    "audio_devices",
    "audio_settings",
    "audio_save_settings",
    "machine_name",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build");
}

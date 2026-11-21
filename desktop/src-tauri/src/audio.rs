//! The Hi-Res audio engine inside the desktop app (`wip/native-hires-engine-plan.md` §9).
//!
//! The page (the server's web UI) sends JSON commands through `audio_command` and listens for engine events
//! on `audio-event`. Only these audio commands are open to the server's page (capability `server-page`); the
//! shell's other powers stay with the bundled connection screen.

use std::sync::Mutex;

use boogiebox_audio::{Command, Device, EngineConfig, EngineHandle};
use tauri::{AppHandle, Emitter, State};

use crate::config::{self, AudioSettings};

/// Tauri event name for engine events.
pub const EVENT: &str = "audio-event";

/// The running engine (Windows only; elsewhere the commands answer "not available").
#[derive(Default)]
pub struct AudioState {
    engine: Mutex<Option<EngineHandle>>,
}

impl AudioState {
    fn send(&self, command: Command) -> Result<(), String> {
        let guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(engine) if engine.send(command) => Ok(()),
            Some(_) => Err("the audio engine stopped".into()),
            None => Err("the audio engine is not available".into()),
        }
    }
}

fn output_command(settings: &AudioSettings) -> Command {
    Command::SetOutput {
        device_id: settings.device_id.clone(),
        mode: settings.mode,
        buffer_ms: settings.buffer_ms,
        nearest_rate: settings.nearest_rate,
    }
}

/// Starts the engine with the saved output settings and forwards its events to the window.
pub fn start(app: &AppHandle, state: &AudioState) {
    #[cfg(windows)]
    {
        let (engine, events) =
            boogiebox_audio::start(boogiebox_audio::default_factory(), EngineConfig::default());
        let settings = config::load(app).audio;
        let _ = engine.send(output_command(&settings));
        *state.engine.lock().unwrap_or_else(|p| p.into_inner()) = Some(engine);
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("bb-audio-events".into())
            .spawn(move || {
                use tauri::Manager;
                for event in events {
                    if let Err(e) = app.emit(EVENT, &event) {
                        tracing::warn!("audio event not delivered: {e}");
                    }
                    // Windows' media controls follow the engine (when "This PC" plays); never blocks.
                    crate::media_controls::on_engine_event(
                        &app,
                        app.state::<crate::media_controls::MediaControls>().inner(),
                        &event,
                    );
                }
            });
        if let Err(e) = spawned {
            tracing::error!("audio event thread did not start: {e}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state, EngineConfig::default());
        tracing::info!("native audio engine is Windows-only");
    }
}

/// Commands the server's page may call (everything else stays with the bundled connection screen).
const PAGE_PERMISSIONS: [&str; 8] = [
    "allow-audio-command",
    "allow-audio-devices",
    "allow-audio-settings",
    "allow-audio-save-settings",
    "allow-machine-name",
    "allow-media-now-playing",
    "allow-media-clear",
    "core:event:default",
];

/// The `scheme://host:port` of a server URL.
pub fn origin(server_url: &str) -> Option<String> {
    let url = url::Url::parse(server_url).ok()?;
    match url.scheme() {
        "http" | "https" => Some(url.origin().ascii_serialization()),
        _ => None,
    }
}

/// Lets the server's page (at `server_url`) use the audio commands and engine events, and nothing else.
pub fn allow_page(app: &AppHandle, server_url: &str) {
    use tauri::Manager;
    let Some(origin) = origin(server_url) else {
        return;
    };
    let id: String = origin
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut capability = tauri::ipc::CapabilityBuilder::new(format!("server-page-{id}"))
        .remote(format!("{origin}/*"))
        .window("main");
    for permission in PAGE_PERMISSIONS {
        capability = capability.permission(permission);
    }
    if let Err(e) = app.add_capability(capability) {
        tracing::warn!(origin, "server page capability not added: {e}");
    }
}

/// Stops the engine (app exit).
pub fn stop(state: &AudioState) {
    if let Some(engine) = state
        .engine
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = engine.send(Command::Shutdown);
    }
}

/// One engine command, as JSON (`{"type": "load", ...}`). `shutdown` is the shell's, not the page's.
#[tauri::command]
pub fn audio_command(state: State<AudioState>, command: serde_json::Value) -> Result<(), String> {
    let command: Command =
        serde_json::from_value(command).map_err(|e| format!("bad audio command: {e}"))?;
    if matches!(command, Command::Shutdown) {
        return Err("not allowed".into());
    }
    state.send(command)
}

/// Output devices on this PC.
#[tauri::command]
pub fn audio_devices() -> Vec<Device> {
    boogiebox_audio::list_devices()
}

/// The saved output settings.
#[tauri::command]
pub fn audio_settings(app: AppHandle) -> AudioSettings {
    config::load(&app).audio
}

/// Saves the output settings and applies them to the engine.
#[tauri::command]
pub fn audio_save_settings(
    app: AppHandle,
    state: State<AudioState>,
    settings: AudioSettings,
) -> Result<(), String> {
    let mut cfg = config::load(&app);
    cfg.audio = AudioSettings {
        buffer_ms: settings.buffer_ms.clamp(20, 200),
        ..settings
    };
    config::save(&app, &cfg)?;
    state.send(output_command(&cfg.audio))
}

/// This PC's name (shown on the server's Activity page).
#[tauri::command]
pub fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_become_an_output_command() {
        let s = AudioSettings {
            device_id: Some("dev".into()),
            mode: boogiebox_audio::OutputMode::Shared,
            buffer_ms: 150,
            nearest_rate: false,
            pure: Some(false),
        };
        assert_eq!(
            output_command(&s),
            Command::SetOutput {
                device_id: Some("dev".into()),
                mode: boogiebox_audio::OutputMode::Shared,
                buffer_ms: 150,
                nearest_rate: false,
            }
        );
        let defaults: AudioSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults, AudioSettings::default());
        assert_eq!(defaults.mode, boogiebox_audio::OutputMode::Exclusive);
        assert_eq!(defaults.pure, None, "follows the mode until chosen");
        let saved: AudioSettings = serde_json::from_str(r#"{"pure":true}"#).unwrap();
        assert_eq!(saved.pure, Some(true));
    }

    #[test]
    fn page_origins_are_http_only() {
        assert_eq!(
            origin("http://192.168.1.5:3001/browse").as_deref(),
            Some("http://192.168.1.5:3001")
        );
        assert_eq!(
            origin("https://boogie.lan").as_deref(),
            Some("https://boogie.lan")
        );
        assert_eq!(origin("file:///c:/x"), None);
        assert_eq!(origin("not a url"), None);
    }

    #[test]
    fn commands_without_an_engine_say_so() {
        let state = AudioState::default();
        assert!(state
            .send(Command::Play)
            .unwrap_err()
            .contains("not available"));
    }
}

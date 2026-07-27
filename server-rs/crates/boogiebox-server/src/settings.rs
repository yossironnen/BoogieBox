//! Defines Rust server support logic for Settings.

use std::collections::HashMap;

// ── Allowed setting keys ──────────────────────────────────────────────────────

/// Documents the ALLOWED GLOBAL SETTINGS KEYS public API surface.
pub const ALLOWED_GLOBAL_SETTINGS_KEYS: &[&str] = &[
    // Provider credentials
    "discogsToken",
    "lastfmKey",
    "spotifyClientId",
    "spotifyClientSecret",
    "geniusClientId",
    "geniusClientSecret",
    // DLNA
    "dlnaEnabled",
    "dlnaFriendlyName",
    "dlnaPort",
    "dlnaMediaMode",
    // BoogieMix
    "boogiemixCrossfadeDuration",
    "boogiemixDefaultStyle",
    "boogiemixGeminiApiKey",
    "boogiemixOpenRouterApiKey",
    "boogiemixOutputFolder",
    "boogiemixDeepAnalysisEnabled",
    "boogiemixDeepAnalysisPreferGpu",
    "boogiemixDeepAnalysisMaxConcurrent",
    "boogiemixDeepAnalysisTimeoutMs",
    "boogiemixDeepAnalysisCleanupTemp",
    "boogiemixDeepAnalysisTempDir",
    "boogiemixDeepAnalysisBackgroundMode",
    "boogiemixDeepAnalysisPauseBackground",
    "boogiemixDeepAnalysisModel",
    "boogiemixHighQualityWaitMs",
    "boogiemixDebugCandidates",
    // Audio / playback
    "crossfadeMode",
    "crossfadeDuration",
    "transcodeQuality",
    "replayGainEnabled",
    "vinylMode",
    // Waveform
    "waveformGenerateOnMissing",
    "waveformBackgroundEnabled",
    "waveformBackgroundFrequencyHours",
    "waveformBackgroundBatchSize",
    "waveformBackgroundLastRun",
    "waveformBackgroundNextRun",
    // BPM
    "bpmAnalysisEnabled",
    "bpmBackgroundEnabled",
    "bpmBackgroundFrequencyHours",
    "bpmBackgroundLastRun",
    "bpmBackgroundNextRun",
    "bpmSpotifyFallbackEnabled",
    // Debug
    "scanDebugLoggingEnabled",
    "deepmixDebugLoggingEnabled",
];

/// Documents the ALLOWED USER SETTING KEYS public API surface.
pub const ALLOWED_USER_SETTING_KEYS: &[&str] = &[
    "theme",
    "adaptiveAccent",
    "uiThemeMode",
    "eqProfiles",
    "autoEqEnabled",
    "eqSelectedProfile",
    "eqGains",
    "eqMode",
    "parametricEqProfiles",
    "parametricEqSelectedProfile",
    "parametricEqBands",
    "volume",
    "muted",
];

/// Documents the USER SETTING MAX VALUE LEN public API surface.
pub const USER_SETTING_MAX_VALUE_LEN: usize = 4096;

/// Validates user-setting values that have a constrained production contract.
pub fn validate_user_setting_value(key: &str, value: &str) -> Result<(), String> {
    if key == "uiThemeMode" && !["light", "dark", "custom"].contains(&value) {
        return Err("Setting 'uiThemeMode' must be 'light', 'dark', or 'custom'".to_string());
    }
    Ok(())
}

/// Documents the PLAYBACK SETTINGS KEYS public API surface.
pub const PLAYBACK_SETTINGS_KEYS: &[&str] = &[
    "transcodeQuality",
    "replayGainEnabled",
    "vinylMode",
    "lastfmKey",
];

// ── Settings validation / normalization ───────────────────────────────────────

/// Validates and normalizes a map of global settings entries.
/// Returns `Err(reason)` for the first invalid key or value.
/// Documents the Normalize Settings Payload public API surface.
pub fn normalize_settings_payload(
    input: &HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::with_capacity(input.len());
    for (key, value) in input {
        if !ALLOWED_GLOBAL_SETTINGS_KEYS.contains(&key.as_str()) {
            return Err(format!("Unknown setting key: {key}"));
        }
        let normalized = normalize_setting_value(key, value)?;
        out.insert(key.clone(), normalized);
    }
    Ok(out)
}

fn normalize_setting_value(key: &str, value: &str) -> Result<String, String> {
    match key {
        // Boolean flags
        "dlnaEnabled"
        | "waveformGenerateOnMissing"
        | "waveformBackgroundEnabled"
        | "bpmAnalysisEnabled"
        | "bpmBackgroundEnabled"
        | "bpmSpotifyFallbackEnabled"
        | "scanDebugLoggingEnabled"
        | "deepmixDebugLoggingEnabled"
        | "replayGainEnabled"
        | "boogiemixDeepAnalysisEnabled"
        | "boogiemixDeepAnalysisPreferGpu"
        | "boogiemixDeepAnalysisCleanupTemp"
        | "boogiemixDeepAnalysisPauseBackground"
        | "boogiemixDebugCandidates" => {
            if value != "true" && value != "false" {
                return Err(format!("Setting '{key}' must be 'true' or 'false'"));
            }
            Ok(value.to_string())
        }

        "boogiemixDeepAnalysisModel" => {
            if !["mdx_extra_q", "htdemucs", "hpss"].contains(&value) {
                return Err(format!(
                    "Setting '{key}' must be 'mdx_extra_q', 'htdemucs', or 'hpss'"
                ));
            }
            Ok(value.to_string())
        }

        "boogiemixDeepAnalysisBackgroundMode" => {
            if ![
                "off",
                "playlists_only",
                "favorites_and_playlists",
                "all_music",
            ]
            .contains(&value)
            {
                return Err(format!("Setting '{key}' has an invalid value"));
            }
            Ok(value.to_string())
        }

        // dlnaPort: integer 1024–65535
        "dlnaPort" => {
            let n: u32 = value
                .trim()
                .parse()
                .map_err(|_| format!("Setting '{key}' must be an integer"))?;
            if !(1024..=65535).contains(&n) {
                return Err(format!("Setting '{key}' must be between 1024 and 65535"));
            }
            Ok(n.to_string())
        }

        // dlnaMediaMode: enum
        "dlnaMediaMode" => {
            if value != "audio" {
                return Err(format!("Setting '{key}' must be 'audio'"));
            }
            Ok(value.to_string())
        }

        // transcodeQuality: enum
        "transcodeQuality" => {
            if !["low", "medium", "high"].contains(&value) {
                return Err(format!(
                    "Setting '{key}' must be 'low', 'medium', or 'high'"
                ));
            }
            Ok(value.to_string())
        }

        // vinylMode: enum
        "vinylMode" => {
            if !["standard", "vinyl"].contains(&value) {
                return Err(format!("Setting '{key}' must be 'standard' or 'vinyl'"));
            }
            Ok(value.to_string())
        }

        // crossfadeMode: known modes
        "crossfadeMode" => {
            if !["off", "crossfade", "zerogap"].contains(&value) {
                return Err(format!(
                    "Setting '{key}' must be 'off', 'crossfade', or 'zerogap'"
                ));
            }
            Ok(value.to_string())
        }

        // Numeric float range for hours: 0.5–720
        "waveformBackgroundFrequencyHours" | "bpmBackgroundFrequencyHours" => {
            let n: f64 = value
                .trim()
                .parse()
                .map_err(|_| format!("Setting '{key}' must be a number"))?;
            if !(0.5..=720.0).contains(&n) {
                return Err(format!("Setting '{key}' must be between 0.5 and 720"));
            }
            Ok(n.to_string())
        }

        // Waveform batch size: 1–5000
        "waveformBackgroundBatchSize" => {
            let n: u32 = value
                .trim()
                .parse()
                .map_err(|_| format!("Setting '{key}' must be an integer"))?;
            if !(1..=5000).contains(&n) {
                return Err(format!("Setting '{key}' must be between 1 and 5000"));
            }
            Ok(n.to_string())
        }

        // defaultVideoQuality: enum
        "defaultVideoQuality" => {
            if !["original", "high", "medium", "low"].contains(&value) {
                return Err(format!(
                    "Setting '{key}' must be 'original', 'high', 'medium', or 'low'"
                ));
            }
            Ok(value.to_string())
        }

        // hardwareAccelerationMode: enum
        "hardwareAccelerationMode" => {
            if !["off", "auto", "nvenc", "qsv", "amf"].contains(&value) {
                return Err(format!("Setting '{key}' has an invalid value"));
            }
            Ok(value.to_string())
        }

        // defaultSubtitleMode: enum
        "defaultSubtitleMode" => {
            if !["off", "forced", "on"].contains(&value) {
                return Err(format!("Setting '{key}' must be 'off', 'forced', or 'on'"));
            }
            Ok(value.to_string())
        }

        // Anything else accepted as-is (strings, paths, API keys, timestamps)
        _ => Ok(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_key() {
        let mut map = HashMap::new();
        map.insert("unknownKey".into(), "value".into());
        assert!(normalize_settings_payload(&map).is_err());
    }

    #[test]
    fn rejects_invalid_boolean() {
        let mut map = HashMap::new();
        map.insert("dlnaEnabled".into(), "yes".into());
        assert!(normalize_settings_payload(&map).is_err());
    }

    #[test]
    fn accepts_valid_boolean() {
        let mut map = HashMap::new();
        map.insert("dlnaEnabled".into(), "true".into());
        assert!(normalize_settings_payload(&map).is_ok());
    }

    #[test]
    fn rejects_invalid_dlna_port() {
        let mut map = HashMap::new();
        map.insert("dlnaPort".into(), "80".into());
        assert!(normalize_settings_payload(&map).is_err());
    }

    #[test]
    fn accepts_valid_transcode_quality() {
        let mut map = HashMap::new();
        map.insert("transcodeQuality".into(), "high".into());
        assert!(normalize_settings_payload(&map).is_ok());
    }

    #[test]
    fn validates_hybrid_theme_modes() {
        assert!(validate_user_setting_value("uiThemeMode", "light").is_ok());
        assert!(validate_user_setting_value("uiThemeMode", "dark").is_ok());
        assert!(validate_user_setting_value("uiThemeMode", "custom").is_ok());
        assert!(validate_user_setting_value("uiThemeMode", "neon").is_err());
    }
}

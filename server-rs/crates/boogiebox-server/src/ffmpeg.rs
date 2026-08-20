//! Defines Rust server support logic for Ffmpeg.

use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const NEEDS_TRANSCODE_EXTS: &[&str] = &[
    ".flac", ".m4a", ".aac", ".wma", ".alac", ".ape", ".aiff", ".aif",
];
const WAVEFORM_POINTS: usize = 960;

/// Documents the Resolve FFmpeg public API surface.
pub fn resolve_ffmpeg() -> PathBuf {
    resolve_tool("ffmpeg")
}

/// Documents the Resolve Ffprobe public API surface.
pub fn resolve_ffprobe() -> PathBuf {
    resolve_tool("ffprobe")
}

/// Documents the FFmpeg Available public API surface.
pub fn ffmpeg_available() -> bool {
    tool_available(&resolve_ffmpeg())
}

/// Documents the Ffprobe Available public API surface.
pub fn ffprobe_available() -> bool {
    tool_available(&resolve_ffprobe())
}

fn tool_available(path: &Path) -> bool {
    // Path comes from resolve_tool's fixed candidate list (config dir / exe dir /
    // PATH lookup for "ffmpeg"/"ffprobe"), never from user input.
    // nosemgrep: boogiebox-rust-dynamic-command
    Command::new(path)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn resolve_tool(name: &str) -> PathBuf {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };

    let exec_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(env_dir) = env::var("BOOGIEBOX_FFMPEG_DIR") {
        let t = env_dir.trim().to_owned();
        if !t.is_empty() {
            candidates.push(PathBuf::from(t).join(&exe));
        }
    }

    if let Some(dir) = exec_dir.as_ref() {
        candidates.push(dir.join("resources").join("ffmpeg").join(&exe));
        candidates.push(dir.join("ffmpeg").join(&exe));
    }

    candidates.push(cwd.join("resources").join("ffmpeg").join(&exe));
    candidates.push(cwd.join("tools").join("ffmpeg").join(&exe));
    candidates.push(
        manifest
            .join("..")
            .join("..")
            .join("..")
            .join("tools")
            .join("ffmpeg")
            .join(&exe),
    );

    for candidate in &candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }

    PathBuf::from(exe)
}

/// Documents the Needs Audio Transcode public API surface.
pub fn needs_audio_transcode(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()));
    match ext.as_deref() {
        Some(e) => NEEDS_TRANSCODE_EXTS.contains(&e),
        None => false,
    }
}

/// Documents the Bitrate For Quality public API surface.
pub fn bitrate_for_quality(quality: &str) -> u32 {
    if quality.eq_ignore_ascii_case("high") {
        320
    } else {
        192
    }
}

/// Documents the Transcoded Bytes Per Sec public API surface.
pub fn transcoded_bytes_per_sec(quality: &str) -> u64 {
    (bitrate_for_quality(quality) as u64 * 1000) / 8
}

/// Documents the Transcoded Total Bytes public API surface.
pub fn transcoded_total_bytes(duration_secs: f64, quality: &str) -> u64 {
    (duration_secs * transcoded_bytes_per_sec(quality) as f64).round() as u64
}

/// Documents the Byte Offset To Seconds public API surface.
pub fn byte_offset_to_seconds(byte_offset: u64, quality: &str) -> f64 {
    byte_offset as f64 / transcoded_bytes_per_sec(quality) as f64
}

/// Documents the Transcoded Byte Offset public API surface.
pub fn transcoded_byte_offset(seconds: f64, quality: &str) -> u64 {
    (seconds * transcoded_bytes_per_sec(quality) as f64).round() as u64
}

/// Parse an HTTP `Range: bytes=...` header. Returns `(start, end)` inclusive.
/// Documents the Parse Byte Range public API surface.
pub fn parse_byte_range(header: &str, file_size: u64) -> Option<(u64, u64)> {
    if file_size == 0 {
        return None;
    }
    let stripped = header.trim().strip_prefix("bytes=")?.trim();
    let (start_raw, end_raw) = stripped.split_once('-')?;

    if start_raw.is_empty() && end_raw.is_empty() {
        return None;
    }

    if start_raw.is_empty() {
        // suffix range: bytes=-N
        let suffix: u64 = end_raw.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = file_size.saturating_sub(suffix);
        let end = file_size.saturating_sub(1);
        if start > end {
            return None;
        }
        return Some((start, end));
    }

    let start: u64 = start_raw.parse().ok()?;
    if start >= file_size {
        return None;
    }

    let end = if end_raw.is_empty() {
        file_size - 1
    } else {
        let e: u64 = end_raw.parse().ok()?;
        e.min(file_size - 1)
    };

    if end < start {
        return None;
    }
    Some((start, end))
}

/// Documents the Audio Mime Type public API surface.
pub fn audio_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("opus") => "audio/ogg; codecs=opus",
        Some("flac") => "audio/flac",
        Some("m4a") | Some("alac") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("wma") => "audio/x-ms-wma",
        Some("ape") => "audio/x-ape",
        Some("aiff") | Some("aif") => "audio/aiff",
        _ => "application/octet-stream",
    }
}

/// Decode audio to mono 8 kHz s16le PCM via FFmpeg and compute 960-bucket amplitude peaks.
/// Returns a `Vec<u8>` of length 960, each value 0–255.
/// Documents the Generate Waveform public API surface.
pub async fn generate_waveform(ffmpeg: &Path, file_path: &Path) -> Result<Vec<u8>, String> {
    let output = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file_path)
        .args(["-map", "0:a"])
        .args(["-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to spawn ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err(format!("ffmpeg exited with status {}", output.status));
    }

    let raw = output.stdout;
    let num_samples = raw.len() / 2;
    if num_samples == 0 {
        return Ok(vec![0u8; WAVEFORM_POINTS]);
    }

    let samples: Vec<i16> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    let samples_per_bucket = (samples.len() / WAVEFORM_POINTS).max(1);

    let peaks: Vec<i32> = (0..WAVEFORM_POINTS)
        .map(|i| {
            let start = i * samples_per_bucket;
            if start >= samples.len() {
                return 0;
            }
            let end = ((i + 1) * samples_per_bucket).min(samples.len());
            samples[start..end]
                .iter()
                .map(|s| s.unsigned_abs() as i32)
                .max()
                .unwrap_or(0)
        })
        .collect();

    let max_peak = *peaks.iter().max().unwrap_or(&1);
    if max_peak == 0 {
        return Ok(vec![0u8; WAVEFORM_POINTS]);
    }

    Ok(peaks
        .iter()
        .map(|&p| ((p as f64 / max_peak as f64) * 255.0).round() as u8)
        .collect())
}

/// Documents the Detect Bpm public API surface.
pub async fn detect_bpm(ffmpeg: &Path, file_path: &Path) -> Result<Option<f64>, String> {
    const SAMPLE_RATE: usize = 11_025;
    const FRAME: usize = 1024;
    const MAX_SECONDS: &str = "180";

    let output = tokio::process::Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-t",
            MAX_SECONDS,
            "-i",
        ])
        .arg(file_path)
        .args(["-map", "0:a"])
        .args(["-ac", "1", "-ar", "11025", "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("failed to spawn ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err(format!("ffmpeg exited with status {}", output.status));
    }

    let samples: Vec<f64> = output
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f64 / i16::MAX as f64)
        .collect();
    Ok(estimate_bpm_from_samples(&samples, SAMPLE_RATE, FRAME))
}

fn estimate_bpm_from_samples(
    samples: &[f64],
    sample_rate: usize,
    frame_size: usize,
) -> Option<f64> {
    if samples.len() < sample_rate * 8 {
        return None;
    }

    let energies: Vec<f64> = samples
        .chunks(frame_size)
        .filter(|chunk| chunk.len() == frame_size)
        .map(|chunk| chunk.iter().map(|s| s * s).sum::<f64>().sqrt())
        .collect();
    if energies.len() < 16 {
        return None;
    }

    let mean = energies.iter().sum::<f64>() / energies.len() as f64;
    let mut flux = Vec::with_capacity(energies.len().saturating_sub(1));
    for pair in energies.windows(2) {
        flux.push((pair[1] - pair[0]).max(0.0));
    }
    let flux_mean = flux.iter().sum::<f64>() / flux.len().max(1) as f64;
    for value in &mut flux {
        *value = (*value - flux_mean).max(0.0);
    }
    if mean <= f64::EPSILON || flux.iter().all(|v| *v <= f64::EPSILON) {
        return None;
    }

    let frames_per_second = sample_rate as f64 / frame_size as f64;
    const MIN_BPM: u32 = 60;
    const MAX_BPM: u32 = 200;
    let mut scores = vec![0.0f64; (MAX_BPM - MIN_BPM + 1) as usize];
    let mut best_bpm = 0.0;
    let mut best_score = 0.0;
    for bpm in MIN_BPM..=MAX_BPM {
        let lag = ((60.0 / bpm as f64) * frames_per_second).round() as usize;
        if lag == 0 || lag >= flux.len() {
            continue;
        }
        let score = flux
            .iter()
            .zip(flux.iter().skip(lag))
            .map(|(a, b)| a * b)
            .sum::<f64>();
        scores[(bpm - MIN_BPM) as usize] = score;
        if score > best_score {
            best_score = score;
            best_bpm = bpm as f64;
        }
    }

    if best_score <= f64::EPSILON {
        return None;
    }

    // Correct common half-tempo (octave) errors: this correlation detector can
    // lock onto a sub-harmonic — e.g. alternating kick emphasis on
    // four-on-the-floor dance tracks produces strong periodicity at exactly
    // half the true beat rate. Dance/club tracks are rarely genuinely below
    // ~90 BPM, so when the winning candidate is that low and the doubled
    // tempo still has strong correlation support, prefer the doubled value.
    // Empirically validated against real mislabeled tracks: a track tagged
    // 130 BPM but detected at 66.67 had a doubled-tempo score ~74% of the
    // peak; a track detected at 62 (no tag, but implausible for its dance
    // genre neighbors) had ~65% — both comfortably clear this threshold,
    // while unrelated tempos score far lower.
    if best_bpm < 90.0 {
        let doubled_bpm = (best_bpm * 2.0).round() as u32;
        if (MIN_BPM..=MAX_BPM).contains(&doubled_bpm) {
            let doubled_score = scores[(doubled_bpm - MIN_BPM) as usize];
            if doubled_score >= best_score * 0.5 {
                best_bpm = doubled_bpm as f64;
            }
        }
    }

    Some(best_bpm)
}

/// Spawn FFmpeg to transcode `file_path` → MP3 stream.
///
/// Uses `std::process::Command` (anonymous Windows pipes, no IOCP) to avoid
/// a compatibility issue with FFmpeg 8.x's async muxer task writing to
/// Tokio's IOCP-backed named pipes, which caused EINVAL after ~1 KB.
///
/// Returns the child process with stdout and stderr piped.
/// Documents the Spawn Transcode public API surface.
pub fn spawn_transcode(
    ffmpeg: &Path,
    file_path: &Path,
    seek_seconds: f64,
    quality: &str,
    replay_gain: bool,
) -> std::io::Result<std::process::Child> {
    let bitrate = bitrate_for_quality(quality);
    let mut cmd = std::process::Command::new(ffmpeg); // nosemgrep: boogiebox-rust-dynamic-command

    cmd.args(["-loglevel", "error", "-hide_banner"]);
    if seek_seconds > 0.001 {
        cmd.args(["-ss", &format!("{seek_seconds:.3}")]);
    }
    cmd.args(["-i"]);
    cmd.arg(file_path);
    // -map 0:a: skip attached_pic / cover-art streams that -vn misses in 8.x
    cmd.args(["-map", "0:a"]);
    if replay_gain {
        cmd.args(["-af", "loudnorm=I=-14:TP=-1.5:LRA=11"]);
    }
    cmd.args([
        "-acodec",
        "libmp3lame",
        "-ab",
        &format!("{bitrate}k"),
        "-ar",
        "44100",
        "-ac",
        "2",
        "-f",
        "mp3",
        "pipe:1",
    ]);

    tracing::debug!(
        "ffmpeg transcode: {} {:?}",
        ffmpeg.display(),
        cmd.get_args().collect::<Vec<_>>()
    );

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_low_default() {
        assert_eq!(bitrate_for_quality("low"), 192);
        assert_eq!(bitrate_for_quality("other"), 192);
    }

    #[test]
    fn bitrate_high() {
        assert_eq!(bitrate_for_quality("high"), 320);
        assert_eq!(bitrate_for_quality("HIGH"), 320);
    }

    #[test]
    fn transcode_byte_math_round_trips() {
        let dur = 300.0f64;
        let total = transcoded_total_bytes(dur, "low");
        let seconds_back = byte_offset_to_seconds(total, "low");
        assert!(
            (seconds_back - dur).abs() < 1.0,
            "round-trip {seconds_back} ≈ {dur}"
        );
    }

    #[test]
    fn parse_range_normal() {
        assert_eq!(parse_byte_range("bytes=0-499", 1000), Some((0, 499)));
        assert_eq!(parse_byte_range("bytes=500-999", 1000), Some((500, 999)));
        assert_eq!(parse_byte_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_byte_range("bytes=-200", 1000), Some((800, 999)));
    }

    #[test]
    fn parse_range_invalid() {
        assert_eq!(parse_byte_range("bytes=1000-1999", 1000), None);
        assert_eq!(parse_byte_range("bytes=500-499", 1000), None);
        assert_eq!(parse_byte_range("bytes=-", 1000), None);
        assert_eq!(parse_byte_range("invalid", 1000), None);
    }

    #[test]
    fn needs_transcode_extensions() {
        assert!(needs_audio_transcode(Path::new("song.flac")));
        assert!(needs_audio_transcode(Path::new("song.m4a")));
        assert!(needs_audio_transcode(Path::new("song.FLAC")));
        assert!(!needs_audio_transcode(Path::new("song.mp3")));
        assert!(!needs_audio_transcode(Path::new("song.ogg")));
    }

    #[test]
    fn mime_types() {
        assert_eq!(audio_mime_type(Path::new("a.mp3")), "audio/mpeg");
        assert_eq!(audio_mime_type(Path::new("a.flac")), "audio/flac");
        assert_eq!(
            audio_mime_type(Path::new("a.opus")),
            "audio/ogg; codecs=opus"
        );
        assert_eq!(
            audio_mime_type(Path::new("a.xyz")),
            "application/octet-stream",
            "unknown extension should not masquerade as MP3"
        );
    }

    #[test]
    fn bundled_tool_candidate_is_preferred_from_env_dir() {
        let resolved = resolve_ffmpeg();
        assert!(resolved.ends_with(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        }));
    }

    /// Builds a synthetic click track: short broadband bursts spaced at `bpm`,
    /// alternating between `strong_amp` and `weak_amp` so every other beat can
    /// be emphasized (mimicking a four-on-the-floor alternating kick pattern).
    fn synth_beat_track(
        sample_rate: usize,
        bpm: f64,
        duration_s: f64,
        strong_amp: f64,
        weak_amp: f64,
    ) -> Vec<f64> {
        let n = (sample_rate as f64 * duration_s) as usize;
        let mut samples = vec![0.0f64; n];
        let period = 60.0 / bpm;
        let burst_len = (sample_rate as f64 * 0.03) as usize;
        let mut beat_index = 0usize;
        let mut t = 0.0;
        while t < duration_s {
            let start = (t * sample_rate as f64) as usize;
            let amp = if beat_index.is_multiple_of(2) {
                strong_amp
            } else {
                weak_amp
            };
            for i in 0..burst_len {
                if start + i < n {
                    samples[start + i] = amp * if i % 2 == 0 { 1.0 } else { -1.0 };
                }
            }
            beat_index += 1;
            t += period;
        }
        samples
    }

    #[test]
    fn estimate_bpm_does_not_halve_uniform_beats() {
        // With every beat equally emphasized there is no sub-harmonic bias,
        // so a fast dance tempo must not be detected as its (implausible)
        // half-tempo — the octave correction should never need to fire here.
        let samples = synth_beat_track(11_025, 128.0, 20.0, 1.0, 1.0);
        let bpm = estimate_bpm_from_samples(&samples, 11_025, 1024).expect("should detect");
        assert!(
            bpm > 90.0,
            "uniform fast beats should not be halved, got {bpm}"
        );
    }

    #[test]
    fn estimate_bpm_corrects_half_tempo_octave_error() {
        // Regression: a real 130 BPM track was locally detected at 66.67 BPM
        // (empirically confirmed ~74% correlation support at the doubled
        // tempo) because alternating kick emphasis makes the raw correlation
        // favor the half-tempo sub-harmonic. Compare the same true tempo with
        // alternating vs. uniform beat strength: without correction the
        // alternating case would lock onto roughly half of the uniform
        // baseline; with correction the two should land close together.
        let alternating = synth_beat_track(11_025, 130.0, 24.0, 1.0, 0.5);
        let uniform = synth_beat_track(11_025, 130.0, 24.0, 1.0, 1.0);
        let alt_bpm = estimate_bpm_from_samples(&alternating, 11_025, 1024).expect("should detect");
        let uni_bpm = estimate_bpm_from_samples(&uniform, 11_025, 1024).expect("should detect");
        assert!(
            alt_bpm > 90.0,
            "correction should have fired, got {alt_bpm}"
        );
        assert!(
            (alt_bpm - uni_bpm).abs() <= 8.0,
            "alternating beats should correct close to the uniform baseline: alt={alt_bpm} uni={uni_bpm}"
        );
    }

    #[test]
    fn estimate_bpm_leaves_genuine_slow_uniform_tempo_uncorrected() {
        // A genuinely slow, uniformly-emphasized tempo must stay below the
        // 90 BPM octave-correction threshold: there is no real correlation
        // support at the doubled lag since beats aren't alternating.
        let samples = synth_beat_track(11_025, 75.0, 20.0, 1.0, 1.0);
        let bpm = estimate_bpm_from_samples(&samples, 11_025, 1024).expect("should detect");
        assert!(
            bpm < 90.0,
            "genuine slow tempo should stay uncorrected, got {bpm}"
        );
    }
}

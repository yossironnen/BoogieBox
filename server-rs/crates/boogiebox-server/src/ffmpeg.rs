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

/// MP3 bitrate (kbps) for the `transcodeQuality` setting: `high` = 320, anything else 192.
pub fn bitrate_for_quality(quality: &str) -> u32 {
    if quality.eq_ignore_ascii_case("high") {
        320
    } else {
        192
    }
}

/// Bytes per second of a constant-bitrate MP3 at `kbps`.
pub fn transcoded_bytes_per_sec(kbps: u32) -> u64 {
    (kbps.max(1) as u64 * 1000) / 8
}

/// Documents the Transcoded Total Bytes public API surface.
pub fn transcoded_total_bytes(duration_secs: f64, kbps: u32) -> u64 {
    (duration_secs * transcoded_bytes_per_sec(kbps) as f64).round() as u64
}

/// Documents the Byte Offset To Seconds public API surface.
pub fn byte_offset_to_seconds(byte_offset: u64, kbps: u32) -> f64 {
    byte_offset as f64 / transcoded_bytes_per_sec(kbps) as f64
}

/// Documents the Transcoded Byte Offset public API surface.
pub fn transcoded_byte_offset(seconds: f64, kbps: u32) -> u64 {
    (seconds * transcoded_bytes_per_sec(kbps) as f64).round() as u64
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

/// FFmpeg exited cleanly but decoded no audio (e.g. a FLAC whose metadata hides its first frame). An error,
/// so the lanes count it as a failure instead of storing a flat waveform or silence.
pub const NO_AUDIO_DECODED: &str = "ffmpeg decoded no audio";

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
        return Err(NO_AUDIO_DECODED.to_string());
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

    if output.stdout.len() < 2 {
        return Err(NO_AUDIO_DECODED.to_string());
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
/// Static levelling gain on a transcode (wip/loudness-levelling-plan.md §8). Positive gain gets a
/// limiter at about −1.5 dBFS so it cannot clip; negative gain is a plain volume change.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LoudnessFilter {
    /// Leave the audio as it is.
    #[default]
    None,
    /// Apply this many dB.
    StaticDb(f64),
}

impl LoudnessFilter {
    /// A filter for `db`, or `None` when it is too small to matter.
    pub fn from_db(db: f64) -> Self {
        if db.is_finite() && db.abs() >= 0.05 {
            LoudnessFilter::StaticDb(db)
        } else {
            LoudnessFilter::None
        }
    }

    /// The `-af` value, if any.
    pub fn filter_arg(self) -> Option<String> {
        match self {
            LoudnessFilter::None => None,
            LoudnessFilter::StaticDb(db) if db > 0.0 => {
                Some(format!("volume={db:.2}dB,alimiter=limit=0.84:level=false"))
            }
            LoudnessFilter::StaticDb(db) => Some(format!("volume={db:.2}dB")),
        }
    }
}

/// Tokio's IOCP-backed named pipes, which caused EINVAL after ~1 KB.
///
/// Returns the child process with stdout and stderr piped.
/// Documents the Spawn Transcode public API surface.
pub fn spawn_transcode(
    ffmpeg: &Path,
    file_path: &Path,
    seek_seconds: f64,
    bitrate: u32,
    loudness: LoudnessFilter,
) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(ffmpeg); // nosemgrep: boogiebox-rust-dynamic-command

    cmd.args(["-loglevel", "error", "-hide_banner"]);
    if seek_seconds > 0.001 {
        cmd.args(["-ss", &format!("{seek_seconds:.3}")]);
    }
    cmd.args(["-i"]);
    cmd.arg(file_path);
    // -map 0:a: skip attached_pic / cover-art streams that -vn misses in 8.x
    cmd.args(["-map", "0:a"]);
    if let Some(filter) = loudness.filter_arg() {
        cmd.arg("-af").arg(filter);
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

/// Encodings streamed without a known length (Subsonic `format=opus` / `format=aac`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCodec {
    /// Opus in Ogg (`libopus`).
    Opus,
    /// AAC-LC in ADTS (FFmpeg's native `aac`).
    Aac,
}

impl StreamCodec {
    /// Response `Content-Type`.
    pub fn mime(self) -> &'static str {
        match self {
            StreamCodec::Opus => "audio/ogg",
            StreamCodec::Aac => "audio/aac",
        }
    }

    /// Display name (Activity page).
    pub fn label(self) -> &'static str {
        match self {
            StreamCodec::Opus => "Opus",
            StreamCodec::Aac => "AAC",
        }
    }
}

/// FFmpeg arguments for a streamed Opus/AAC encode; the empty string after `-i` is the input path placeholder
/// (kept as an `OsStr` by the caller, as in [`pcm_args`]). `seek_seconds` > 0 seeks the input (`-ss` before `-i`).
pub fn encode_args(
    codec: StreamCodec,
    bitrate: u32,
    loudness: LoudnessFilter,
    seek_seconds: f64,
) -> Vec<String> {
    let mut args: Vec<String> = ["-loglevel", "error", "-hide_banner"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    if seek_seconds > 0.001 {
        args.extend(["-ss".into(), format!("{seek_seconds:.3}")]);
    }
    args.extend(
        ["-i", "", "-map", "0:a", "-vn"]
            .iter()
            .map(|a| a.to_string()),
    );
    if let Some(filter) = loudness.filter_arg() {
        args.extend(["-af".into(), filter]);
    }
    let (encoder, rate, format) = match codec {
        StreamCodec::Opus => ("libopus", "48000", "ogg"),
        StreamCodec::Aac => ("aac", "44100", "adts"),
    };
    for a in [
        "-c:a".to_string(),
        encoder.to_string(),
        "-b:a".to_string(),
        format!("{bitrate}k"),
        "-ar".to_string(),
        rate.to_string(),
        "-ac".to_string(),
        "2".to_string(),
        "-f".to_string(),
        format.to_string(),
        "pipe:1".to_string(),
    ] {
        args.push(a);
    }
    args
}

/// Spawns ffmpeg encoding `file_path` with [`encode_args`] (stdout piped).
pub fn spawn_encode(
    ffmpeg: &Path,
    file_path: &Path,
    codec: StreamCodec,
    bitrate: u32,
    loudness: LoudnessFilter,
    seek_seconds: f64,
) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(ffmpeg); // nosemgrep: boogiebox-rust-dynamic-command
    for arg in encode_args(codec, bitrate, loudness, seek_seconds) {
        if arg.is_empty() {
            cmd.arg(file_path);
        } else {
            cmd.arg(arg);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// True when `ffmpeg -encoders` lists `name` (checked once per process; blocking).
pub fn has_encoder(ffmpeg: &Path, name: &str) -> bool {
    static ENCODERS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let list = ENCODERS.get_or_init(|| {
        Command::new(ffmpeg) // nosemgrep: boogiebox-rust-dynamic-command
            .args(["-hide_banner", "-encoders"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    });
    encoder_listed(list, name)
}

/// Whether an `ffmpeg -encoders` listing contains the encoder `name`.
pub fn encoder_listed(listing: &str, name: &str) -> bool {
    listing
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(name))
}

/// Builds the ffmpeg arguments for raw little-endian PCM output (no container; the caller writes the WAV
/// header so byte offsets stay exact).
pub fn pcm_args(
    seek_seconds: f64,
    sample_rate: u32,
    channels: u16,
    bits: u16,
    loudness: LoudnessFilter,
) -> Vec<String> {
    let (codec, format) = if bits >= 24 {
        ("pcm_s24le", "s24le")
    } else {
        ("pcm_s16le", "s16le")
    };
    let mut args: Vec<String> = vec!["-loglevel".into(), "error".into(), "-hide_banner".into()];
    if seek_seconds > 0.001 {
        args.extend(["-ss".into(), format!("{seek_seconds:.6}")]);
    }
    args.push("-i".into());
    args.push(String::new()); // placeholder for the input path (kept as OsStr by the caller)
    if let Some(filter) = loudness.filter_arg() {
        args.extend(["-af".into(), filter]);
    }
    for a in ["-map", "0:a", "-vn", "-acodec", codec, "-ar"] {
        args.push(a.to_string());
    }
    args.push(sample_rate.to_string());
    args.push("-ac".into());
    args.push(channels.to_string());
    for a in ["-f", format, "pipe:1"] {
        args.push(a.to_string());
    }
    args
}

/// Spawns ffmpeg decoding `file_path` to raw PCM on stdout (see [`pcm_args`]).
pub fn spawn_pcm(
    ffmpeg: &Path,
    file_path: &Path,
    seek_seconds: f64,
    sample_rate: u32,
    channels: u16,
    bits: u16,
    loudness: LoudnessFilter,
) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new(ffmpeg); // nosemgrep: boogiebox-rust-dynamic-command
    for arg in pcm_args(seek_seconds, sample_rate, channels, bits, loudness) {
        if arg.is_empty() {
            cmd.arg(file_path);
        } else {
            cmd.arg(arg);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// Measures a file's loudness with FFmpeg's `ebur128` filter (true peak, mono counted as dual mono,
/// as listeners hear it on two speakers). `timeout` kills a stuck decode.
pub async fn measure_loudness(
    ffmpeg: &Path,
    file_path: &Path,
    timeout: std::time::Duration,
) -> Result<boogiebox_db::loudness::Measurement, String> {
    let child = tokio::process::Command::new(ffmpeg) // nosemgrep: boogiebox-rust-dynamic-command
        .args(["-nostats", "-hide_banner", "-nostdin", "-i"])
        .arg(file_path)
        .args([
            "-map",
            "0:a:0",
            "-af",
            "ebur128=peak=true:dualmono=true:framelog=verbose",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to spawn ffmpeg: {e}"))?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| "ffmpeg timed out".to_string())?
        .map_err(|e| format!("ffmpeg failed: {e}"))?;
    if !output.status.success() {
        return Err(format!("ffmpeg exited with status {}", output.status));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // ebur128 still prints a summary (I −70, peak 0.0) when nothing was decoded.
    if stderr.contains("Output file is empty") {
        return Err(NO_AUDIO_DECODED.to_string());
    }
    parse_ebur128_summary(&stderr)
        .ok_or_else(|| "no ebur128 summary in the ffmpeg output".to_string())
}

/// Reads the `Summary:` block `ebur128` prints at the end. Integrated loudness at the −70 LUFS
/// floor, and `-inf` peaks, mean digital silence and come back as `None`.
pub fn parse_ebur128_summary(stderr: &str) -> Option<boogiebox_db::loudness::Measurement> {
    let summary = &stderr[stderr.rfind("Summary:")?..];
    let value = |label: &str| {
        summary.lines().find_map(|line| {
            let rest = line.trim().strip_prefix(label)?;
            rest.split_whitespace().next()?.parse::<f64>().ok()
        })
    };
    let integrated = value("I:")?;
    Some(boogiebox_db::loudness::Measurement {
        integrated_lufs: (integrated.is_finite() && integrated > -69.5).then_some(integrated),
        true_peak_dbtp: value("Peak:").filter(|peak| peak.is_finite()),
        lra: value("LRA:").filter(|lra| lra.is_finite()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_args_select_codec_and_seek() {
        let a = pcm_args(0.0, 44_100, 2, 16, LoudnessFilter::None);
        assert!(!a.contains(&"-ss".to_string()));
        assert!(a.contains(&"pcm_s16le".to_string()) && a.contains(&"s16le".to_string()));
        assert_eq!(
            a.iter().filter(|s| s.is_empty()).count(),
            1,
            "one input placeholder"
        );
        let b = pcm_args(12.5, 96_000, 2, 24, LoudnessFilter::StaticDb(-4.0));
        assert!(b.windows(2).any(|w| w[0] == "-ss" && w[1] == "12.500000"));
        assert!(b.contains(&"pcm_s24le".to_string()) && b.contains(&"96000".to_string()));
        assert!(b
            .windows(2)
            .any(|w| w[0] == "-af" && w[1] == "volume=-4.00dB"));
        assert!(!a.contains(&"-af".to_string()));
    }

    #[test]
    fn loudness_filter_limits_only_positive_gain() {
        assert_eq!(LoudnessFilter::from_db(0.02), LoudnessFilter::None);
        assert_eq!(LoudnessFilter::from_db(f64::NAN), LoudnessFilter::None);
        assert_eq!(LoudnessFilter::None.filter_arg(), None);
        assert_eq!(
            LoudnessFilter::from_db(-7.5).filter_arg().as_deref(),
            Some("volume=-7.50dB")
        );
        assert_eq!(
            LoudnessFilter::from_db(3.0).filter_arg().as_deref(),
            Some("volume=3.00dB,alimiter=limit=0.84:level=false")
        );
    }

    #[test]
    fn encode_args_seek_the_input() {
        let a = encode_args(StreamCodec::Opus, 128, LoudnessFilter::None, 0.0);
        assert!(!a.contains(&"-ss".to_string()));
        assert_eq!(a.iter().filter(|s| s.is_empty()).count(), 1);
        let b = encode_args(StreamCodec::Aac, 192, LoudnessFilter::StaticDb(-2.0), 30.0);
        let ss = b.iter().position(|s| s == "-ss").expect("seek");
        let input = b.iter().position(|s| s == "-i").expect("input");
        assert_eq!(b[ss + 1], "30.000");
        assert!(ss < input, "-ss before -i seeks the input, not the output");
        assert!(b.contains(&"adts".to_string()) && b.contains(&"volume=-2.00dB".to_string()));
    }

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
        let total = transcoded_total_bytes(dur, 192);
        let seconds_back = byte_offset_to_seconds(total, 192);
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

    #[test]
    fn parses_the_ebur128_summary() {
        let stderr = "[Parsed_ebur128_0 @ 0x1] t: 4.9 TARGET:-23 LUFS M: -27.8 S: -27.8 I: -27.8 LUFS\n\
[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    I:         -9.4 LUFS\n    Threshold: -19.5 LUFS\n\n\
  Loudness range:\n    LRA:         5.2 LU\n    Threshold: -29.6 LUFS\n\n  True peak:\n    Peak:        0.6 dBFS\n";
        let m = parse_ebur128_summary(stderr).unwrap();
        assert_eq!(m.integrated_lufs, Some(-9.4));
        assert_eq!(m.lra, Some(5.2));
        assert_eq!(m.true_peak_dbtp, Some(0.6));

        let silent =
            "Summary:\n  I:         -70.0 LUFS\n  LRA:         0.0 LU\n  Peak:       -inf dBFS\n";
        let m = parse_ebur128_summary(silent).unwrap();
        assert_eq!(m.integrated_lufs, None);
        assert_eq!(m.true_peak_dbtp, None);
        assert!(parse_ebur128_summary("no summary here").is_none());
        assert!(parse_ebur128_summary("Summary:\n  LRA: 1 LU\n").is_none());
    }

    /// A tagger that overflows FLAC's 24-bit block length leaves 16 MiB of picture bytes between the last
    /// metadata block and the first frame. FFmpeg then exits 0 having decoded nothing.
    fn write_flac_with_orphaned_picture_bytes(good: &[u8], out: &Path) {
        assert_eq!(&good[..4], b"fLaC");
        let mut pos = 4;
        let mut blocks: Vec<(u8, &[u8])> = Vec::new();
        loop {
            let header = good[pos];
            let len = u32::from_be_bytes([0, good[pos + 1], good[pos + 2], good[pos + 3]]) as usize;
            blocks.push((header & 0x7f, &good[pos + 4..pos + 4 + len]));
            pos += 4 + len;
            if header & 0x80 != 0 {
                break;
            }
        }
        let mut picture = Vec::new();
        for n in [3_u32, 10] {
            picture.extend_from_slice(&n.to_be_bytes());
        }
        picture.extend_from_slice(b"image/jpeg");
        for n in [0_u32, 500, 500, 24, 0, 5000] {
            picture.extend_from_slice(&n.to_be_bytes());
        }
        let mut seed = 0x2545_f491_u32;
        picture.extend((0..5000 + (1 << 24)).map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        }));
        blocks.push((6, &picture));
        let mut bytes = b"fLaC".to_vec();
        for (i, (kind, body)) in blocks.iter().enumerate() {
            let last = if i == blocks.len() - 1 { 0x80 } else { 0 };
            bytes.push(kind | last);
            bytes.extend_from_slice(&((body.len() as u32) & 0xff_ffff).to_be_bytes()[1..]);
            bytes.extend_from_slice(body);
        }
        bytes.extend_from_slice(&good[pos..]);
        std::fs::write(out, bytes).unwrap();
    }

    #[tokio::test]
    async fn a_file_that_decodes_to_nothing_is_an_error_not_silence() {
        let ffmpeg = resolve_ffmpeg();
        let dir = std::env::temp_dir().join(format!("boogiebox-no-audio-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.flac");
        let made = std::process::Command::new(&ffmpeg) // nosemgrep: boogiebox-rust-dynamic-command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("sine=frequency=440:duration=10")
            .arg(&good)
            .status();
        if !matches!(made, Ok(status) if status.success()) {
            eprintln!("ffmpeg not available; skipping the no-audio check");
            return;
        }
        assert!(generate_waveform(&ffmpeg, &good).await.is_ok());

        let broken = dir.join("broken.flac");
        write_flac_with_orphaned_picture_bytes(&std::fs::read(&good).unwrap(), &broken);
        let timeout = std::time::Duration::from_secs(60);
        assert_eq!(
            generate_waveform(&ffmpeg, &broken).await,
            Err(NO_AUDIO_DECODED.into())
        );
        assert_eq!(
            detect_bpm(&ffmpeg, &broken).await,
            Err(NO_AUDIO_DECODED.into())
        );
        assert_eq!(
            measure_loudness(&ffmpeg, &broken, timeout).await.err(),
            Some(NO_AUDIO_DECODED.into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

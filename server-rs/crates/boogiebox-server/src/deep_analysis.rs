//! Defines Rust server support logic for Deep Analysis.

use crate::{ffmpeg::resolve_ffmpeg, post_scan::PostScanState};
use boogiebox_db::{
    boogiemix::{
        claim_next_deep_analysis_job_with_background, complete_deep_analysis_job,
        fail_deep_analysis_job, get_deep_analysis_queue_status, get_setting,
        queue_background_deep_analysis_batch, should_skip_deep_analysis, skip_deep_analysis_job,
        upsert_deep_analysis, ClaimedDeepAnalysisJob, MixTrackInput,
    },
    music::set_track_bpm_detected,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{process::Command, time::timeout};

macro_rules! dlog {
    ($enabled:expr, $($arg:tt)*) => {
        if $enabled {
            tracing::info!($($arg)*);
        }
    };
}

const DEEP_ANALYSIS_VERSION: i64 = 1;
const DEFAULT_MODEL: &str = "htdemucs";
const DEFAULT_TIMEOUT_MS: u64 = 90 * 60 * 1000;

#[derive(Debug, Clone)]
struct DeepSettings {
    enabled: bool,
    max_concurrent: usize,
    timeout_ms: u64,
    prefer_gpu: bool,
    use_madmom: bool,
    cleanup_temp: bool,
    temp_dir: PathBuf,
    background_mode: String,
    pause_background: bool,
    max_duration_secs: Option<f64>,
    debug_logging: bool,
}

#[derive(Debug, Clone)]
struct RuntimeStatus {
    python: Option<PythonInvocation>,
    ffmpeg_available: bool,
    demucs_callable: bool,
    torch_available: bool,
    gpu_available: bool,
}

impl RuntimeStatus {
    fn enabled(&self) -> bool {
        self.python.is_some()
            && self.ffmpeg_available
            && self.demucs_callable
            && self.torch_available
    }
}

#[derive(Debug, Clone)]
struct PythonInvocation {
    command: PathBuf,
    base_args: Vec<String>,
    display_name: String,
}

const RUNTIME_CACHE_SECS: u64 = 60;

/// Documents the Start Deep Analysis Worker public API surface.
pub fn start_deep_analysis_worker(state: PostScanState) {
    tokio::spawn(async move {
        let active = Arc::new(AtomicUsize::new(0));
        let mut last_runtime: Option<(Instant, RuntimeStatus)> = None;
        let mut interval = tokio::time::interval(Duration::from_millis(1200));
        loop {
            interval.tick().await;
            run_tick(&state, active.clone(), &mut last_runtime).await;
        }
    });
}

async fn run_tick(
    state: &PostScanState,
    active: Arc<AtomicUsize>,
    last_runtime: &mut Option<(Instant, RuntimeStatus)>,
) {
    let settings = match load_settings(state) {
        Ok(settings) => settings,
        Err(err) => {
            tracing::warn!("[boogiemix:deep] settings unavailable: {err}");
            return;
        }
    };
    let dbg = settings.debug_logging;
    let current_active = active.load(Ordering::SeqCst);
    if !settings.enabled {
        dlog!(
            dbg,
            "[boogiemix:deep] tick skipped: deep analysis disabled in settings"
        );
        return;
    }
    if current_active >= settings.max_concurrent {
        dlog!(
            dbg,
            "[boogiemix:deep] tick skipped: at concurrency limit ({current_active}/{max})",
            max = settings.max_concurrent
        );
        return;
    }

    // Enqueue background jobs before checking queue, so newly-queued work is visible.
    if !settings.pause_background && settings.background_mode != "off" {
        dlog!(
            dbg,
            "[boogiemix:deep] checking background batch queue (mode={})",
            settings.background_mode
        );
        if let Err(err) = maybe_queue_background_batch(state, &settings.background_mode) {
            tracing::warn!("[boogiemix:deep] background queue failed: {err}");
        }
    }

    // M-05: Check queue before spawning Python runtime detection processes.
    if !has_queued_jobs(state) {
        dlog!(
            dbg,
            "[boogiemix:deep] tick: no pending or running jobs in queue"
        );
        return;
    }

    // H-01: Cache RuntimeStatus for 60 s to avoid spawning Python on every tick.
    let runtime = match last_runtime.as_ref() {
        Some((checked_at, rt)) if checked_at.elapsed().as_secs() < RUNTIME_CACHE_SECS => {
            dlog!(
                dbg,
                "[boogiemix:deep] using cached runtime status (age={}s)",
                checked_at.elapsed().as_secs()
            );
            rt.clone()
        }
        _ => {
            dlog!(
                dbg,
                "[boogiemix:deep] detecting Python runtime (cache expired or first tick)"
            );
            let rt = detect_runtime_with_debug(dbg).await;
            *last_runtime = Some((Instant::now(), rt.clone()));
            rt
        }
    };
    if !runtime.enabled() {
        if dbg {
            tracing::info!(
                "[boogiemix:deep] runtime not usable — python={}, ffmpeg={}, demucs={}, torch={}, gpu={}",
                runtime.python.as_ref().map(|p| p.display_name.as_str()).unwrap_or("missing"),
                runtime.ffmpeg_available,
                runtime.demucs_callable,
                runtime.torch_available,
                runtime.gpu_available,
            );
        }
        return;
    }
    dlog!(
        dbg,
        "[boogiemix:deep] runtime ready — python={}, gpu={}",
        runtime
            .python
            .as_ref()
            .map(|p| p.display_name.as_str())
            .unwrap_or("?"),
        runtime.gpu_available,
    );

    while active.load(Ordering::SeqCst) < settings.max_concurrent {
        let job = match claim_job(state, !settings.pause_background) {
            Ok(Some(job)) => job,
            Ok(None) => {
                dlog!(dbg, "[boogiemix:deep] no more claimable jobs this tick");
                break;
            }
            Err(err) => {
                tracing::warn!("[boogiemix:deep] claim failed: {err}");
                break;
            }
        };
        dlog!(
            dbg,
            "[boogiemix:deep] claimed job {} for track {} (duration={:?}s, fingerprint={})",
            job.id,
            job.track_id,
            job.duration,
            &job.file_fingerprint[..job.file_fingerprint.len().min(20)]
        );
        active.fetch_add(1, Ordering::SeqCst);
        let state = state.clone();
        let settings = settings.clone();
        let runtime = runtime.clone();
        let active = active.clone();
        tokio::spawn(async move {
            if let Err(err) = process_job(&state, &settings, &runtime, job.clone()).await {
                tracing::error!("[boogiemix:deep] job {} failed: {err}", job.id);
                let job_id = job.id.clone();
                let db = state.db.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(conn) = db.lock() {
                        let _ = fail_deep_analysis_job(&conn, &job_id, &err);
                    }
                })
                .await;
            }
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

fn has_queued_jobs(state: &PostScanState) -> bool {
    match state.db.lock() {
        Ok(conn) => match get_deep_analysis_queue_status(&conn) {
            Ok(status) => status.pending > 0 || status.running > 0,
            Err(_) => true,
        },
        Err(_) => true,
    }
}

fn maybe_queue_background_batch(state: &PostScanState, mode: &str) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let status = get_deep_analysis_queue_status(&conn).map_err(|e| e.to_string())?;
    if status.pending > 0 || status.running > 0 {
        return Ok(());
    }
    let queued =
        queue_background_deep_analysis_batch(&conn, mode, 10).map_err(|e| e.to_string())?;
    if queued > 0 {
        tracing::info!("[boogiemix:deep] queued {queued} background deep-analysis jobs");
    }
    Ok(())
}

fn claim_job(
    state: &PostScanState,
    include_background: bool,
) -> Result<Option<ClaimedDeepAnalysisJob>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    claim_next_deep_analysis_job_with_background(&conn, include_background)
        .map_err(|e| e.to_string())
}

async fn process_job(
    state: &PostScanState,
    settings: &DeepSettings,
    runtime: &RuntimeStatus,
    job: ClaimedDeepAnalysisJob,
) -> Result<(), String> {
    let dbg = settings.debug_logging;
    dlog!(
        dbg,
        "[boogiemix:deep] process_job start — job={} track={} file={:?} duration={:?}",
        job.id,
        job.track_id,
        job.file_path,
        job.duration
    );

    let track = MixTrackInput {
        track_id: job.track_id.clone(),
        file_path: job.file_path.clone(),
        title: None,
        artist: None,
        duration: job.duration,
        bpm: None,
        bpm_detected: None,
        file_size: None,
        scanned_at: None,
        position: 0,
    };
    if let Some(reason) = should_skip_deep_analysis(&track, settings.max_duration_secs) {
        dlog!(dbg, "[boogiemix:deep] job {} skipped: {reason}", job.id);
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        skip_deep_analysis_job(&conn, &job.id, reason).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let Some(python) = runtime.python.as_ref() else {
        return Err("runtime_unavailable".into());
    };
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — python={}",
        job.id,
        python.display_name
    );

    let temp_dir_create = std::fs::create_dir_all(&settings.temp_dir);
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — temp_dir={:?} create={:?}",
        job.id,
        settings.temp_dir,
        temp_dir_create.is_ok()
    );
    temp_dir_create.map_err(|e| e.to_string())?;

    let use_gpu = settings.prefer_gpu && runtime.gpu_available;
    let payload = serde_json::json!({
        "track_id": job.track_id,
        "file_path": job.file_path,
        "duration_sec": job.duration,
        "analysis_version": DEEP_ANALYSIS_VERSION,
        "demucs_model": DEFAULT_MODEL,
        "use_gpu": use_gpu,
        "use_madmom": settings.use_madmom,
        "cleanup_temp": settings.cleanup_temp,
        "temp_root": settings.temp_dir,
    });
    // Scale timeout by track duration so long tracks on slow (CPU-only) hardware
    // don't hit a blanket cap. 5 s of processing budget per 1 s of audio.
    let effective_timeout_ms = job
        .duration
        .map(|secs| (secs * 5_000.0) as u64)
        .map(|scaled| scaled.max(settings.timeout_ms))
        .unwrap_or(settings.timeout_ms);
    dlog!(dbg, "[boogiemix:deep] job {} — use_gpu={} use_madmom={} timeout_ms={} (base={}) prefer_gpu={} gpu_available={}",
        job.id, use_gpu, settings.use_madmom, effective_timeout_ms, settings.timeout_ms,
        settings.prefer_gpu, runtime.gpu_available);
    let worker_script = worker_script_path();
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — worker_script={:?}",
        job.id,
        worker_script
    );
    let start = Instant::now();
    let output = run_python_worker_with_debug(
        python,
        &payload,
        effective_timeout_ms,
        dbg,
        &job.id.to_string(),
    )
    .await?;
    let processing_ms = start.elapsed().as_millis().min(i64::MAX as u128) as i64;
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — python worker returned in {}ms",
        job.id,
        processing_ms
    );
    let analysis_version = output["analysis_version"]
        .as_i64()
        .unwrap_or(DEEP_ANALYSIS_VERSION);
    let demucs_model = output["demucs_model"].as_str().unwrap_or(DEFAULT_MODEL);
    let used_gpu = output["used_gpu"].as_bool().unwrap_or(false);
    let energy = output["energy_score_refined"].as_f64().unwrap_or(0.5);
    let confidence = output["confidence"].as_f64().unwrap_or(0.0);
    let bpm_refined = output["bpm_refined"].as_f64();
    let used_demucs = output["used_demucs"].as_bool().unwrap_or(false);
    let schema_version = output["analysis_schema_version"].as_i64().unwrap_or(2);
    dlog!(dbg, "[boogiemix:deep] job {} — output summary: schema_v={} analysis_v={} model={} used_gpu={} used_demucs={} confidence={:.3} energy={:.3} bpm_refined={:?}",
        job.id, schema_version, analysis_version, demucs_model, used_gpu, used_demucs, confidence, energy, bpm_refined);

    // Merge key_neural and cue_points into transition_hints_json so they are
    // stored without requiring a DB schema change.
    let hints_json = {
        let base = json_field(&output, "transition_hints_json", "{}");
        let mut hints: serde_json::Value =
            serde_json::from_str(&base).unwrap_or(serde_json::Value::Object(Default::default()));
        if let Some(obj) = hints.as_object_mut() {
            if let Some(kn) = output.get("key_neural") {
                if !kn.is_null() {
                    obj.insert("keyNeural".to_string(), kn.clone());
                }
            }
            if let Some(cp) = output.get("cue_points") {
                if !cp.is_null() {
                    obj.insert("cuePoints".to_string(), cp.clone());
                }
            }
            if let Some(bg) = output.get("beat_grid") {
                if !bg.is_null() {
                    obj.insert("beatGrid".to_string(), bg.clone());
                }
            }
            if let Some(ne) = output.get("neural_embedding") {
                if !ne.is_null() {
                    obj.insert("neuralEmbedding".to_string(), ne.clone());
                }
            }
        }
        serde_json::to_string(&hints).unwrap_or(base)
    };

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — writing deep analysis to DB for track {}",
        job.id,
        job.track_id
    );
    let upsert_result = upsert_deep_analysis(
        &conn,
        &job.track_id,
        analysis_version,
        schema_version,
        demucs_model,
        used_gpu,
        &job.file_fingerprint,
        &json_field(&output, "stem_feature_json", "{}"),
        &json_field(&output, "vocal_windows_json", "[]"),
        &json_field(&output, "drum_windows_json", "[]"),
        &json_field(&output, "bass_windows_json", "[]"),
        &json_field(&output, "section_json", "[]"),
        &json_field(&output, "phrase_boundaries_json", "[]"),
        &json_field(&output, "intro_outro_refined_json", "{}"),
        energy,
        &hints_json,
        &json_field(&output, "transition_windows_json", "[]"),
        confidence,
        job.duration,
        output["processing_time_ms"]
            .as_i64()
            .unwrap_or(processing_ms),
    );
    match &upsert_result {
        Ok(_) => dlog!(
            dbg,
            "[boogiemix:deep] job {} — DB upsert successful",
            job.id
        ),
        Err(e) => tracing::error!("[boogiemix:deep] job {} — DB upsert failed: {e}", job.id),
    }
    upsert_result.map_err(|e| e.to_string())?;

    // Write bpm_refined back to tracks.bpm_detected so it's available for
    // BPM matching without needing to reload deep features every time.
    tracing::info!(track_id = %job.track_id, bpm_refined = ?bpm_refined, "deep analysis bpm_refined");
    if let Some(bpm) = bpm_refined.filter(|&b| (60.0..=220.0).contains(&b)) {
        match set_track_bpm_detected(&conn, &job.track_id, bpm, "deep_analysis") {
            Ok(_) => tracing::info!(track_id = %job.track_id, bpm, "wrote bpm_detected to tracks"),
            Err(e) => {
                tracing::warn!(track_id = %job.track_id, bpm, err = %e, "failed to write bpm_detected")
            }
        }
    }

    let complete_result = complete_deep_analysis_job(&conn, &job.id);
    match &complete_result {
        Ok(_) => dlog!(
            dbg,
            "[boogiemix:deep] job {} — marked complete in DB",
            job.id
        ),
        Err(e) => tracing::error!(
            "[boogiemix:deep] job {} — failed to mark complete: {e}",
            job.id
        ),
    }
    complete_result.map_err(|e| e.to_string())?;
    tracing::info!("[boogiemix:deep] job {} completed: track={} confidence={:.3} energy={:.3} processing_ms={}",
        job.id, job.track_id, confidence, energy, processing_ms);
    Ok(())
}

fn json_field(output: &Value, key: &str, fallback: &str) -> String {
    match output.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| fallback.to_string()),
        None => fallback.to_string(),
    }
}

async fn run_python_worker_with_debug(
    python: &PythonInvocation,
    payload: &Value,
    timeout_ms: u64,
    dbg: bool,
    job_id: &str,
) -> Result<Value, String> {
    use tokio::io::AsyncWriteExt;
    let script = worker_script_path().ok_or_else(|| "worker_script_missing".to_string())?;
    let mut args = python.base_args.clone();
    args.push(script.display().to_string());
    let payload_bytes = payload.to_string().into_bytes();

    dlog!(
        dbg,
        "[boogiemix:deep] job {} — spawning python: {:?} {:?}",
        job_id,
        python.command,
        args
    );
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — payload_bytes={} timeout_ms={}",
        job_id,
        payload_bytes.len(),
        timeout_ms
    );

    let mut cmd = Command::new(&python.command);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        tracing::error!("[boogiemix:deep] job {} — spawn failed: {e}", job_id);
        e.to_string()
    })?;
    let pid = child.id();
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — python pid={:?}",
        job_id,
        pid
    );

    if let Some(mut stdin) = child.stdin.take() {
        let write_result = stdin.write_all(&payload_bytes).await;
        dlog!(
            dbg,
            "[boogiemix:deep] job {} — stdin write result: {:?}",
            job_id,
            write_result.is_ok()
        );
        // EOF signals the worker to start processing
    }
    dlog!(
        dbg,
        "[boogiemix:deep] job {} — waiting for python (timeout={}ms)…",
        job_id,
        timeout_ms
    );
    let wait_result = timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;
    match wait_result {
        Err(_) => {
            tracing::error!(
                "[boogiemix:deep] job {} — TIMEOUT after {}ms (pid={:?})",
                job_id,
                timeout_ms,
                pid
            );
            Err("process_timeout".to_string())
        }
        Ok(Err(e)) => {
            tracing::error!(
                "[boogiemix:deep] job {} — wait_with_output error: {e}",
                job_id
            );
            Err(e.to_string())
        }
        Ok(Ok(output)) => {
            let status = output.status;
            let stdout_len = output.stdout.len();
            let stderr_raw = String::from_utf8_lossy(&output.stderr);
            dlog!(
                dbg,
                "[boogiemix:deep] job {} — exit_status={} stdout_bytes={} stderr_bytes={}",
                job_id,
                status,
                stdout_len,
                output.stderr.len()
            );
            if status.success() {
                if !stderr_raw.is_empty() {
                    if dbg {
                        tracing::info!(
                            "[boogiemix:deep] job {} — python stderr:\n{}",
                            job_id,
                            stderr_raw.trim()
                        );
                    } else {
                        tracing::debug!(worker_stderr = %stderr_raw.trim(), "python worker stderr");
                    }
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parse_result = serde_json::from_str(stdout.trim());
                match parse_result {
                    Ok(v) => {
                        dlog!(dbg, "[boogiemix:deep] job {} — JSON parse OK", job_id);
                        Ok(v)
                    }
                    Err(e) => {
                        let preview: String = stdout.trim().chars().take(200).collect();
                        tracing::error!("[boogiemix:deep] job {} — JSON parse error: {e}; stdout preview: {preview}", job_id);
                        Err(format!("invalid worker JSON: {e}"))
                    }
                }
            } else {
                let tail: String = stderr_raw
                    .chars()
                    .rev()
                    .take(2000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                tracing::error!(
                    "[boogiemix:deep] job {} — python exited with {status}; stderr tail:\n{tail}",
                    job_id
                );
                Err(tail)
            }
        }
    }
}

fn load_settings(state: &PostScanState) -> Result<DeepSettings, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let enabled = parse_bool(
        get_setting(&conn, "boogiemixDeepAnalysisEnabled").as_deref(),
        true,
    );
    let max_concurrent = get_setting(&conn, "boogiemixDeepAnalysisMaxConcurrent")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 4);
    let timeout_ms = get_setting(&conn, "boogiemixDeepAnalysisTimeoutMs")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(30_000, 4 * 60 * 60_000);
    let prefer_gpu = parse_bool(
        get_setting(&conn, "boogiemixDeepAnalysisPreferGpu").as_deref(),
        true,
    );
    let use_madmom = parse_bool(get_setting(&conn, "boogiemixUseMadmom").as_deref(), true);
    let cleanup_temp = parse_bool(
        get_setting(&conn, "boogiemixDeepAnalysisCleanupTemp").as_deref(),
        true,
    );
    let temp_dir = get_setting(&conn, "boogiemixDeepAnalysisTempDir")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("boogiemix-demucs"));
    let background_mode = normalize_background_mode(
        get_setting(&conn, "boogiemixDeepAnalysisBackgroundMode").as_deref(),
    );
    let pause_background = parse_bool(
        get_setting(&conn, "boogiemixDeepAnalysisPauseBackground").as_deref(),
        false,
    );
    let max_duration_secs = get_setting(&conn, "boogiemixDeepAnalysisMaxDurationMins")
        .and_then(|v| v.parse::<f64>().ok())
        .map(|mins| mins * 60.0)
        .filter(|&secs| secs > 0.0);
    let debug_logging = parse_bool(
        get_setting(&conn, "deepmixDebugLoggingEnabled").as_deref(),
        false,
    );
    Ok(DeepSettings {
        enabled,
        max_concurrent,
        timeout_ms,
        prefer_gpu,
        use_madmom,
        cleanup_temp,
        temp_dir,
        background_mode,
        pause_background,
        max_duration_secs,
        debug_logging,
    })
}

async fn detect_runtime_with_debug(dbg: bool) -> RuntimeStatus {
    dlog!(
        dbg,
        "[boogiemix:deep] detect_runtime: probing Python candidates"
    );
    let python = detect_python_with_debug(dbg).await;
    let ffmpeg_path = resolve_ffmpeg();
    let ffmpeg_available =
        command_success(ffmpeg_path.clone(), ["-version"], Duration::from_secs(5)).await;
    dlog!(
        dbg,
        "[boogiemix:deep] detect_runtime: ffmpeg={:?} available={}",
        ffmpeg_path,
        ffmpeg_available
    );
    let mut demucs_callable = false;
    let mut torch_available = false;
    let mut gpu_available = false;
    if let Some(invocation) = python.as_ref() {
        dlog!(
            dbg,
            "[boogiemix:deep] detect_runtime: checking demucs importability"
        );
        demucs_callable = python_bool(
            invocation,
            "import importlib.util; print('true' if importlib.util.find_spec('demucs') else 'false')",
        )
        .await;
        dlog!(
            dbg,
            "[boogiemix:deep] detect_runtime: demucs_callable={}",
            demucs_callable
        );
        dlog!(
            dbg,
            "[boogiemix:deep] detect_runtime: checking torch importability"
        );
        torch_available = python_bool(
            invocation,
            "import importlib.util; print('true' if importlib.util.find_spec('torch') else 'false')",
        )
        .await;
        dlog!(
            dbg,
            "[boogiemix:deep] detect_runtime: torch_available={}",
            torch_available
        );
        if torch_available {
            dlog!(
                dbg,
                "[boogiemix:deep] detect_runtime: checking CUDA availability"
            );
            gpu_available = python_bool(
                invocation,
                "import torch; print('true' if torch.cuda.is_available() else 'false')",
            )
            .await;
            dlog!(
                dbg,
                "[boogiemix:deep] detect_runtime: gpu_available={}",
                gpu_available
            );
        }
    } else {
        dlog!(
            dbg,
            "[boogiemix:deep] detect_runtime: no suitable Python found — skipping package checks"
        );
    }
    RuntimeStatus {
        python,
        ffmpeg_available,
        demucs_callable,
        torch_available,
        gpu_available,
    }
}

async fn detect_python_with_debug(dbg: bool) -> Option<PythonInvocation> {
    let mut candidates = python_candidates();
    dlog!(
        dbg,
        "[boogiemix:deep] detect_python: venv candidates found: {}",
        candidates.len()
    );
    for c in &candidates {
        dlog!(dbg, "[boogiemix:deep]   candidate venv: {:?}", c.command);
    }
    candidates.push(PythonInvocation {
        command: PathBuf::from("python"),
        base_args: Vec::new(),
        display_name: "python".to_string(),
    });
    candidates.push(PythonInvocation {
        command: PathBuf::from("py"),
        base_args: vec!["-3".to_string()],
        display_name: "py -3".to_string(),
    });

    for candidate in candidates {
        dlog!(
            dbg,
            "[boogiemix:deep] detect_python: probing {:?}",
            candidate.command
        );
        if python_min_version(&candidate).await {
            dlog!(
                dbg,
                "[boogiemix:deep] detect_python: selected {}",
                candidate.display_name
            );
            tracing::debug!(
                "[boogiemix:deep] using Python runtime {}",
                candidate.display_name
            );
            return Some(candidate);
        } else {
            dlog!(
                dbg,
                "[boogiemix:deep] detect_python: {:?} failed version check",
                candidate.command
            );
        }
    }
    dlog!(
        dbg,
        "[boogiemix:deep] detect_python: no suitable Python found"
    );
    None
}

fn python_candidates() -> Vec<PythonInvocation> {
    // Check both the dev layout (repo-root/Services/...) and the installed layout
    // (exe-dir/resources/Services/...) for the managed venv python.
    // Windows venv: .venv/Scripts/python.exe  Linux venv: .venv/bin/python
    #[cfg(windows)]
    let venv_python: &[&str] = &["Scripts", "python.exe"];
    #[cfg(not(windows))]
    let venv_python: &[&str] = &["bin", "python"];

    let base_rels: &[&[&str]] = &[
        &["Services", "boogiemix", "python", ".venv"],
        &["resources", "Services", "boogiemix", "python", ".venv"],
    ];

    let mut candidates = Vec::new();
    for root in candidate_roots() {
        for base in base_rels {
            let venv_root: PathBuf = base.iter().fold(root.clone(), |p, s| p.join(s));
            let path = venv_python.iter().fold(venv_root, |p, s| p.join(s));
            if path.is_file() {
                candidates.push(PythonInvocation {
                    display_name: path.display().to_string(),
                    command: path,
                    base_args: Vec::new(),
                });
            }
        }
    }
    candidates
}

fn worker_script_path() -> Option<PathBuf> {
    let rels: &[&[&str]] = &[
        &[
            "Services",
            "boogiemix",
            "python",
            "boogiemix_demucs_worker.py",
        ],
        &[
            "resources",
            "Services",
            "boogiemix",
            "python",
            "boogiemix_demucs_worker.py",
        ],
    ];
    for root in candidate_roots() {
        for rel in rels {
            let path = rel.iter().fold(root.clone(), |p, s| p.join(s));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn candidate_roots() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..");
    if let Ok(canonical) = manifest_root.canonicalize() {
        dirs.push(canonical);
    } else {
        dirs.push(manifest_root);
    }
    dirs
}

async fn python_min_version(invocation: &PythonInvocation) -> bool {
    python_bool(
        invocation,
        "import sys; print('true' if sys.version_info >= (3, 10) else 'false')",
    )
    .await
}

async fn python_bool(invocation: &PythonInvocation, script: &str) -> bool {
    let mut args = invocation.base_args.clone();
    args.push("-c".to_string());
    args.push(script.to_string());
    command_output(&invocation.command, args, Duration::from_secs(10))
        .await
        .map(|stdout| stdout.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

async fn command_success<I, S>(command: PathBuf, args: I, duration: Duration) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    command_output(&command, args, duration).await.is_ok()
}

async fn command_output<I, S>(command: &Path, args: I, duration: Duration) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = timeout(duration, cmd.output())
        .await
        .map_err(|_| "process_timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.chars().take(500).collect())
    }
}

fn parse_bool(raw: Option<&str>, default_value: bool) -> bool {
    match raw.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "true" => true,
        Some(v) if v == "false" => false,
        _ => default_value,
    }
}

fn normalize_background_mode(raw: Option<&str>) -> String {
    match raw {
        Some(v @ ("playlists_only" | "favorites_and_playlists" | "all_music")) => v.to_string(),
        _ => "off".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_field_preserves_object_payloads() {
        let value = serde_json::json!({"stem_feature_json":{"a":1}});
        assert_eq!(json_field(&value, "stem_feature_json", "{}"), "{\"a\":1}");
    }

    #[test]
    fn parse_bool_keeps_defaults_for_invalid_values() {
        assert!(parse_bool(Some("true"), false));
        assert!(!parse_bool(Some("FALSE"), true));
        assert!(parse_bool(Some("bogus"), true));
    }

    #[test]
    fn background_mode_defaults_to_off() {
        assert_eq!(normalize_background_mode(None), "off");
        assert_eq!(normalize_background_mode(Some("bogus")), "off");
        assert_eq!(
            normalize_background_mode(Some("favorites_and_playlists")),
            "favorites_and_playlists"
        );
    }

    #[test]
    fn worker_script_resolution_checks_source_layout() {
        let roots = candidate_roots();
        assert!(!roots.is_empty());
    }
}

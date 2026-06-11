//! Defines Rust server support logic for Mix Worker.

use crate::{
    ffmpeg::{resolve_ffmpeg, resolve_ffprobe},
    post_scan::PostScanState,
    DbPool,
};
use boogiebox_db::{
    boogiemix::{
        append_mix_job_log, claim_next_mix_job, complete_mix_job, count_deep_analysis_ready,
        create_mix_output, fail_mix_job, get_cached_mix_analysis, get_mix_output_dir_from_db,
        get_setting, is_mix_job_canceled, load_deep_track_features, load_playlist_tracks_for_mix,
        persist_mix_plan, persist_mix_transitions, queue_missing_deep_analysis_for_tracks,
        set_mix_job_status, touch_deep_analysis_last_used, update_mix_job_plan_info,
        update_mix_job_progress, upsert_mix_analysis, DeepTrackFeatures, MixTrackInput,
        MixTransitionRow, TrackMixAnalysis, TransitionWindow,
    },
    music::{coerce_entity_id, EntityId},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

// ── Types ─────────────────────────────────────────────────────────────────────

struct FfprobeInfo {
    duration_sec: f64,
    key_estimate: Option<String>,
}

struct SilenceInfo {
    intro_end_sec: Option<f64>,
    outro_start_sec: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MixTransition {
    from_track_id: EntityId,
    to_track_id: EntityId,
    crossfade_sec: f64,
    from_outro_start_sec: f64,
    to_intro_start_sec: f64,
    phrase_aware: bool,
    reason: String,
    kind: String,
    intensity: f64,
    eq_duck: f64,
    #[serde(default)]
    bass_duck: f64,
    filter_sweep: bool,
    echo_tail_sec: f64,
    loop_build_sec: f64,
    bpm_adjust_from_pct: f64,
    bpm_adjust_to_pct: f64,
    #[serde(default)]
    deep_used: bool,
    /// Drums RMS on the incoming track's intro window (Phase 1d).
    #[serde(default)]
    drums_rms_incoming: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MixPlan {
    playlist_id: EntityId,
    default_crossfade_sec: f64,
    target_energy_ramp: Vec<f64>,
    transitions: Vec<MixTransition>,
    style: String,
    ordered_track_ids: Vec<EntityId>,
    energy_curve_phases: Vec<String>,
    per_track_energy: HashMap<String, f64>,
    anthem_track_id: Option<EntityId>,
}

struct StylePreset {
    allow_resequence: bool,
    curve_aggressiveness: f64,
    base_crossfade_sec: f64,
    fx_intensity: f64,
}

// ── Worker entry point ────────────────────────────────────────────────────────

/// Documents the Start Mix Worker public API surface.
pub fn start_mix_worker(state: PostScanState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1200));
        loop {
            interval.tick().await;
            if let Err(e) = try_process_next(&state).await {
                tracing::error!("[mix_worker] tick error: {e}");
            }
        }
    });
}

async fn try_process_next(state: &PostScanState) -> Result<(), String> {
    let job_id = {
        let conn = lock_db(&state.db)?;
        claim_next_mix_job(&conn).map_err(|e| e.to_string())?
    };
    let Some(job_id) = job_id else {
        return Ok(());
    };
    process_mix_job(state, job_id).await;
    Ok(())
}

async fn process_mix_job(state: &PostScanState, job_id: EntityId) {
    match do_process_mix_job(state, &job_id).await {
        Ok(()) => {}
        Err(e) => {
            tracing::error!("[mix_worker] job {} failed: {e}", job_id);
            if let Ok(conn) = state.db.lock() {
                let _ = fail_mix_job(&conn, &job_id, &e);
                let _ = append_mix_job_log(&conn, &job_id, "error", &e);
            }
        }
    }
}

async fn do_process_mix_job(state: &PostScanState, job_id: &EntityId) -> Result<(), String> {
    let ffmpeg = resolve_ffmpeg();
    let ffprobe = resolve_ffprobe();

    // ── Read job info ─────────────────────────────────────────────────────────
    let (playlist_id_str, user_id_str, crossfade_sec, mix_style, mix_quality) = {
        let conn = lock_db(&state.db)?;
        if is_mix_job_canceled(&conn, job_id).map_err(|e| e.to_string())? {
            set_mix_job_status(&conn, job_id, "canceled", "canceled").ok();
            return Ok(());
        }
        conn.query_row(
            "SELECT playlist_id, user_id, default_crossfade_sec,
                    COALESCE(mix_style,'club_blend'), COALESCE(mix_quality,'standard')
             FROM mix_jobs WHERE id=?1",
            rusqlite::params![job_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
    };
    let playlist_id = coerce_entity_id(&playlist_id_str);
    let user_id = coerce_entity_id(&user_id_str);
    let crossfade = crossfade_sec.clamp(4, 60) as f64;

    // ── Load tracks ───────────────────────────────────────────────────────────
    let tracks = {
        let conn = lock_db(&state.db)?;
        load_playlist_tracks_for_mix(&conn, &playlist_id).map_err(|e| e.to_string())?
    };
    if tracks.len() < 2 {
        return Err("Playlist needs at least 2 tracks".into());
    }

    let queued = {
        let conn = lock_db(&state.db)?;
        queue_missing_deep_analysis_for_tracks(&conn, &tracks, false).map_err(|e| e.to_string())?
    };
    if queued > 0 {
        let conn = lock_db(&state.db)?;
        append_mix_job_log(
            &conn,
            job_id,
            "info",
            &format!("Queued {queued} deep-analysis track jobs"),
        )
        .ok();
    }

    if mix_quality == "high_quality" {
        let wait_ms = {
            let conn = lock_db(&state.db)?;
            get_setting(&conn, "boogiemixHighQualityWaitMs")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(20_000)
                .min(60_000)
        };
        if wait_ms > 0 {
            let ready =
                wait_for_deep_analysis(state, &tracks, Duration::from_millis(wait_ms)).await?;
            let conn = lock_db(&state.db)?;
            append_mix_job_log(
                &conn,
                job_id,
                "info",
                &format!(
                    "High-quality wait: deep analysis ready for {ready}/{} tracks",
                    tracks.len()
                ),
            )
            .ok();
        }
    }

    // ── Analyze tracks ────────────────────────────────────────────────────────
    let mut analyses: Vec<TrackMixAnalysis> = Vec::with_capacity(tracks.len());

    for (i, track) in tracks.iter().enumerate() {
        {
            let conn = lock_db(&state.db)?;
            if is_mix_job_canceled(&conn, job_id).map_err(|e| e.to_string())? {
                set_mix_job_status(&conn, job_id, "canceled", "canceled").ok();
                return Ok(());
            }
        }

        let cached = {
            let conn = lock_db(&state.db)?;
            get_cached_mix_analysis(&conn, &track.track_id).map_err(|e| e.to_string())?
        };

        let analysis = if let Some(cached) = cached {
            {
                let conn = lock_db(&state.db)?;
                append_mix_job_log(
                    &conn,
                    job_id,
                    "info",
                    &format!("Reused analysis track {}", track.track_id),
                )
                .ok();
            }
            cached
        } else {
            let info = ffprobe_track(&ffprobe, &track.file_path).await;
            let loudness = analyze_loudness(&ffmpeg, &track.file_path).await;
            let silence = analyze_silence(&ffmpeg, &track.file_path).await;
            let a = build_analysis(
                track,
                info,
                loudness,
                silence.intro_end_sec,
                silence.outro_start_sec,
            );
            {
                let conn = lock_db(&state.db)?;
                upsert_mix_analysis(&conn, &a).ok();
                append_mix_job_log(
                    &conn,
                    job_id,
                    "info",
                    &format!("Analyzed track {}", track.track_id),
                )
                .ok();
            }
            a
        };
        analyses.push(analysis);

        let pct = 5 + ((i + 1) as i64 * 45) / tracks.len() as i64;
        {
            let conn = lock_db(&state.db)?;
            update_mix_job_progress(&conn, job_id, "analyzing", pct, None).ok();
        }
    }

    // ── Load deep features (best-effort, used only when present) ──────────────
    let mut deep_features: HashMap<String, DeepTrackFeatures> = HashMap::new();
    {
        let conn = lock_db(&state.db)?;
        for track in &tracks {
            match load_deep_track_features(&conn, &track.track_id) {
                Ok(Some(feat)) => {
                    deep_features.insert(track.track_id.to_string(), feat);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(
                        "[mix_worker] deep features load failed for {}: {err}",
                        track.track_id
                    );
                }
            }
        }
    }

    // ── Merge vocal cue points into analyses (Phase 2) ───────────────────────
    // When deep features contain high-confidence vocal cue points, override the
    // silence-detection intro_end/outro_start so the planner picks transitions
    // at real vocal-free gaps rather than audio silence pads.
    for analysis in &mut analyses {
        let tid = analysis.track_id.to_string();
        if let Some(deep) = deep_features.get(&tid) {
            if let Some(cp) = &deep.cue_points {
                if cp.confidence >= 0.6 {
                    let beats = deep
                        .beat_grid
                        .as_ref()
                        .map(|bg| bg.beats.as_slice())
                        .unwrap_or(&[]);
                    if let Some(ie) = cp.intro_end_sec {
                        if ie > analysis.intro_start_sec && ie < analysis.duration_sec * 0.5 {
                            // Snap to nearest beat within 0.5s for tighter timing
                            let snapped = snap_to_beat(beats, ie, 0.5).unwrap_or(ie);
                            analysis.intro_end_sec = snapped;
                        }
                    }
                    if let Some(os) = cp.outro_start_sec {
                        if os > analysis.duration_sec * 0.25 && os < analysis.duration_sec {
                            let snapped = snap_to_beat(beats, os, 0.5).unwrap_or(os);
                            analysis.outro_start_sec = snapped;
                        }
                    }
                }
            }
        }
    }

    // ── Plan ──────────────────────────────────────────────────────────────────
    {
        let conn = lock_db(&state.db)?;
        set_mix_job_status(&conn, job_id, "planning", "planning").ok();
        update_mix_job_progress(&conn, job_id, "planning", 55, Some("Building mix plan")).ok();
    }

    let debug_candidates_enabled = {
        let conn = lock_db(&state.db)?;
        get_setting(&conn, "boogiemixDebugCandidates")
            .as_deref()
            .map(|v| v == "true")
            .unwrap_or(false)
    };

    let (plan, strategy, provider, debug_steps) = create_plan(
        state,
        &playlist_id,
        &tracks,
        &analyses,
        &deep_features,
        crossfade,
        &mix_style,
        debug_candidates_enabled,
    )
    .await;

    {
        let conn = lock_db(&state.db)?;
        let track_ids: Vec<EntityId> = tracks.iter().map(|track| track.track_id.clone()).collect();
        let ready_count =
            count_deep_analysis_ready(&conn, &track_ids).map_err(|e| e.to_string())?;
        let deep_used_count = plan.transitions.iter().filter(|t| t.deep_used).count();
        let used_deep = deep_used_count > 0;
        if used_deep {
            let consumed: Vec<EntityId> = plan
                .transitions
                .iter()
                .filter(|t| t.deep_used)
                .flat_map(|t| [t.from_track_id.clone(), t.to_track_id.clone()].into_iter())
                .collect();
            let _ = touch_deep_analysis_last_used(&conn, &consumed);
            append_mix_job_log(
                &conn,
                job_id,
                "info",
                &format!(
                    "Deep features applied to {deep_used_count}/{} transitions ({}/{} tracks ready)",
                    plan.transitions.len(),
                    ready_count,
                    track_ids.len()
                ),
            )
            .ok();
        } else if ready_count > 0 {
            append_mix_job_log(
                &conn,
                job_id,
                "info",
                &format!(
                    "Deep rows present for {ready_count}/{} tracks but no transition consumed them",
                    track_ids.len()
                ),
            )
            .ok();
        }
        let plan_json = serde_json::to_string(&serde_json::json!({
            "strategy": strategy,
            "plan": plan,
        }))
        .unwrap_or_default();
        persist_mix_plan(&conn, &playlist_id, &provider, None, &plan_json, "valid").ok();
        update_mix_job_plan_info(&conn, job_id, &strategy, &provider, used_deep).ok();

        let transition_rows: Vec<MixTransitionRow> = plan
            .transitions
            .iter()
            .enumerate()
            .map(|(i, t)| MixTransitionRow {
                step_index: i as i64,
                from_track_id: t.from_track_id.clone(),
                to_track_id: t.to_track_id.clone(),
                crossfade_sec: t.crossfade_sec,
                from_outro_start_sec: t.from_outro_start_sec,
                to_intro_start_sec: t.to_intro_start_sec,
                phrase_aware: t.phrase_aware,
                reason: t.reason.clone(),
            })
            .collect();
        persist_mix_transitions(&conn, job_id, &transition_rows).ok();
        update_mix_job_progress(&conn, job_id, "planning", 60, Some("Planning transitions")).ok();
        append_mix_job_log(
            &conn,
            job_id,
            "info",
            &format!(
                "Built plan with {} transitions ({})",
                plan.transitions.len(),
                provider
            ),
        )
        .ok();
    }

    // ── Render ────────────────────────────────────────────────────────────────
    {
        let conn = lock_db(&state.db)?;
        set_mix_job_status(&conn, job_id, "rendering", "rendering").ok();
        update_mix_job_progress(&conn, job_id, "rendering", 70, Some("Rendering mix")).ok();
    }

    let track_map: HashMap<String, &MixTrackInput> =
        tracks.iter().map(|t| (t.track_id.to_string(), t)).collect();
    let analysis_map: HashMap<String, &TrackMixAnalysis> = analyses
        .iter()
        .map(|a| (a.track_id.to_string(), a))
        .collect();

    let ordered_tracks: Vec<&MixTrackInput> = plan
        .ordered_track_ids
        .iter()
        .filter_map(|id| track_map.get(&id.to_string()).copied())
        .collect();
    let ordered_analyses: Vec<&TrackMixAnalysis> = ordered_tracks
        .iter()
        .filter_map(|t| analysis_map.get(&t.track_id.to_string()).copied())
        .collect();

    if ordered_tracks.len() < 2 || ordered_tracks.len() != ordered_analyses.len() {
        return Err("Track/analysis mismatch after planning".into());
    }

    let out_dir = get_mix_output_dir_fn(&state.db, state.db_folder.as_ref());
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("Create output dir: {e}"))?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let output_name = format!("playlist-{}-mix-{}-{}.mp3", playlist_id, job_id, ts);
    let output_path = out_dir.join(&output_name);

    if debug_candidates_enabled && !debug_steps.is_empty() {
        let debug_name = format!(
            "playlist-{}-mix-{}-{}.candidates.json",
            playlist_id, job_id, ts
        );
        let debug_path = out_dir.join(&debug_name);
        let payload = serde_json::json!({
            "playlist_id": playlist_id.to_string(),
            "job_id": job_id.to_string(),
            "style": &mix_style,
            "default_crossfade_sec": crossfade,
            "ordered_track_ids": plan.ordered_track_ids,
            "transitions": debug_steps,
        });
        match serde_json::to_vec_pretty(&payload) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&debug_path, &bytes) {
                    let conn = lock_db(&state.db)?;
                    append_mix_job_log(
                        &conn,
                        job_id,
                        "warn",
                        &format!("Failed to write candidate debug file: {e}"),
                    )
                    .ok();
                } else {
                    let conn = lock_db(&state.db)?;
                    append_mix_job_log(
                        &conn,
                        job_id,
                        "info",
                        &format!(
                            "Wrote transition candidate debug file: {} ({} steps)",
                            debug_name,
                            debug_steps.len()
                        ),
                    )
                    .ok();
                }
            }
            Err(e) => {
                let conn = lock_db(&state.db)?;
                append_mix_job_log(
                    &conn,
                    job_id,
                    "warn",
                    &format!("Failed to serialize candidate debug: {e}"),
                )
                .ok();
            }
        }
    }

    let file_size = render_mix(
        state,
        job_id,
        &ffmpeg,
        &ordered_tracks,
        &ordered_analyses,
        &deep_features,
        &plan,
        &output_path,
    )
    .await
    .map_err(|e| format!("Render failed: {e}"))?;

    let duration_sec = ordered_analyses.iter().map(|a| a.duration_sec).sum::<f64>()
        - plan
            .transitions
            .iter()
            .map(|t| t.crossfade_sec)
            .sum::<f64>();

    // ── Complete ──────────────────────────────────────────────────────────────
    {
        let conn = lock_db(&state.db)?;
        let output_id = create_mix_output(
            &conn,
            job_id,
            &playlist_id,
            &user_id,
            output_path.to_str().unwrap_or(""),
            &output_name,
            duration_sec.max(1.0),
            file_size,
        )
        .map_err(|e| e.to_string())?;
        complete_mix_job(&conn, job_id, &output_id).map_err(|e| e.to_string())?;
        update_mix_job_progress(&conn, job_id, "done", 100, Some("Mix completed")).ok();
        append_mix_job_log(
            &conn,
            job_id,
            "info",
            &format!("Mix complete: {output_name}"),
        )
        .ok();
    }

    Ok(())
}

// ── Track analysis ────────────────────────────────────────────────────────────

async fn ffprobe_track(ffprobe: &Path, file_path: &str) -> Option<FfprobeInfo> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=sample_rate:format_tags=initial_key,key",
            "-of",
            "default=noprint_wrappers=1:nokey=0",
            file_path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut duration_sec = 0.0f64;
    let mut key_estimate: Option<String> = None;

    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("duration=") {
            if let Ok(d) = line["duration=".len()..].trim().parse::<f64>() {
                if d > 0.0 {
                    duration_sec = d;
                }
            }
        } else if lower.starts_with("tag:initial_key=") {
            let v = line["tag:initial_key=".len()..].trim();
            if !v.is_empty() && v != "N/A" {
                key_estimate = Some(v.to_string());
            }
        } else if key_estimate.is_none() && lower.starts_with("tag:key=") {
            let v = line["tag:key=".len()..].trim();
            if !v.is_empty() && v != "N/A" {
                key_estimate = Some(v.to_string());
            }
        }
    }

    if duration_sec <= 0.0 {
        return None;
    }
    Some(FfprobeInfo {
        duration_sec,
        key_estimate,
    })
}

async fn analyze_loudness(ffmpeg: &Path, file_path: &str) -> Option<f64> {
    let output = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-i",
            file_path,
            "-af",
            "loudnorm=I=-16:TP=-1.5:LRA=11:print_format=summary",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;

    let text = String::from_utf8_lossy(&output.stderr);
    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(rest) = lower.strip_prefix("input integrated:") {
            let v = rest.trim().trim_end_matches("lufs").trim();
            if let Ok(lufs) = v.parse::<f64>() {
                return Some(lufs);
            }
        }
    }
    None
}

async fn analyze_silence(ffmpeg: &Path, file_path: &str) -> SilenceInfo {
    let Ok(output) = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-i",
            file_path,
            "-af",
            "silencedetect=noise=-35dB:d=0.8",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
    else {
        return SilenceInfo {
            intro_end_sec: None,
            outro_start_sec: None,
        };
    };

    let text = String::from_utf8_lossy(&output.stderr).to_string();
    let mut starts: Vec<f64> = Vec::new();
    let mut ends: Vec<f64> = Vec::new();

    for line in text.lines() {
        let lower = line.to_lowercase();
        if let Some(pos) = lower.find("silence_start:") {
            let rest = line[pos + "silence_start:".len()..].trim();
            if let Ok(v) = rest.split_whitespace().next().unwrap_or("").parse::<f64>() {
                starts.push(v);
            }
        }
        if let Some(pos) = lower.find("silence_end:") {
            let rest = line[pos + "silence_end:".len()..].trim();
            let val_str = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('|');
            if let Ok(v) = val_str.parse::<f64>() {
                ends.push(v);
            }
        }
    }

    let intro_end_sec = ends.iter().find(|&&v| (0.0..=30.0).contains(&v)).copied();
    let outro_start_sec = starts.last().copied();
    SilenceInfo {
        intro_end_sec,
        outro_start_sec,
    }
}

fn build_analysis(
    track: &MixTrackInput,
    info: Option<FfprobeInfo>,
    loudness: Option<f64>,
    silence_intro_end: Option<f64>,
    silence_outro_start: Option<f64>,
) -> TrackMixAnalysis {
    let duration = info
        .as_ref()
        .map(|i| i.duration_sec)
        .or(track.duration)
        .unwrap_or(0.0)
        .max(1.0);

    let bpm = track
        .bpm_detected
        .filter(|&b| (60.0..=220.0).contains(&b))
        .or_else(|| track.bpm.filter(|&b| (60.0..=220.0).contains(&b)));

    // For long dance tracks (5-9 min), 12s intro and 18s outro caps are far too
    // conservative — the track has already gone quiet before the crossfade starts.
    // Use proportional defaults so the mix region stays in the energetic body.
    let intro_end_sec = silence_intro_end
        .unwrap_or_else(|| (duration * 0.15).min(60.0))
        .clamp(2.0, duration * 0.2);

    let outro_start_sec = silence_outro_start
        .unwrap_or(duration - duration * 0.22)
        .clamp(intro_end_sec + 10.0, duration - 2.0);

    let low_energy_end = intro_end_sec.min((duration * 0.12).max(4.0));
    let high_energy_start = low_energy_end.max(duration * 0.35);
    let high_energy_end = (high_energy_start + 2.0).max((duration * 0.7).min(duration - 2.0));
    let confidence = if bpm.is_some() { 0.85 } else { 0.55 };

    TrackMixAnalysis {
        track_id: track.track_id.clone(),
        duration_sec: duration,
        bpm_estimate: bpm,
        loudness_lufs: loudness,
        key_estimate: info.and_then(|i| i.key_estimate),
        beat_grid_sec: bpm.map(|b| 60.0 / b),
        phrase_bars: bpm.map(|_| 8i64),
        intro_start_sec: 0.0,
        intro_end_sec,
        outro_start_sec,
        outro_end_sec: duration,
        low_energy_start_sec: Some(0.0),
        low_energy_end_sec: Some(low_energy_end),
        high_energy_start_sec: Some(high_energy_start),
        high_energy_end_sec: Some(high_energy_end),
        confidence,
    }
}

// ── Deterministic planner ─────────────────────────────────────────────────────

fn style_preset(style: &str) -> StylePreset {
    match style {
        "chill_blend" => StylePreset {
            allow_resequence: true,
            curve_aggressiveness: 0.55,
            base_crossfade_sec: 24.0,
            fx_intensity: 0.35,
        },
        "long_build" => StylePreset {
            allow_resequence: true,
            curve_aggressiveness: 0.7,
            base_crossfade_sec: 28.0,
            fx_intensity: 0.55,
        },
        "safe_mix" => StylePreset {
            allow_resequence: false,
            curve_aggressiveness: 0.4,
            base_crossfade_sec: 10.0,
            fx_intensity: 0.2,
        },
        _ => StylePreset {
            // club_blend default: 20s — user setting overrides via default_crossfade
            allow_resequence: true,
            curve_aggressiveness: 0.8,
            base_crossfade_sec: 20.0,
            fx_intensity: 0.7,
        },
    }
}

fn estimate_energy(a: &TrackMixAnalysis) -> f64 {
    let loudness = a
        .loudness_lufs
        .map_or(0.5, |l| ((l + 24.0) / 18.0).clamp(0.0, 1.0));
    let bpm = a
        .bpm_estimate
        .map_or(0.5, |b| ((b - 70.0) / 90.0).clamp(0.0, 1.0));
    let spread = match (a.high_energy_end_sec, a.low_energy_end_sec) {
        (Some(h), Some(l)) if a.duration_sec > 0.0 => ((h - l) / a.duration_sec).clamp(0.0, 1.0),
        _ => 0.5,
    };
    (loudness * 0.45) + (bpm * 0.35) + (spread * 0.2)
}

fn effective_energy(a: &TrackMixAnalysis, deep: Option<&DeepTrackFeatures>) -> f64 {
    let base = estimate_energy(a);
    let Some(d) = deep else { return base };
    // Phase 4c: prefer neural mel energy when available (more perceptually accurate).
    if let Some(ne) = &d.neural_embedding {
        if ne.energy_neural > 0.0 {
            return ne.energy_neural;
        }
    }
    if d.energy_refined > 0.0 {
        let blend = (d.confidence.clamp(0.0, 1.0) * 0.7 + 0.3).clamp(0.0, 1.0);
        return (base * (1.0 - blend) + d.energy_refined * blend).clamp(0.0, 1.0);
    }
    base
}

fn phase_at(index: usize, total: usize) -> &'static str {
    if total <= 1 {
        return "groove";
    }
    let p = index as f64 / (total - 1) as f64;
    if p < 0.16 {
        "warmup"
    } else if p < 0.33 {
        "groove"
    } else if p < 0.55 {
        "lift"
    } else if p < 0.70 {
        "peak"
    } else if p < 0.86 {
        "anthem"
    } else {
        "cooldown"
    }
}

fn phase_target_energy(phase: &str) -> f64 {
    match phase {
        "warmup" => 0.22,
        "groove" => 0.38,
        "lift" => 0.55,
        "peak" => 0.78,
        "anthem" => 0.92,
        "cooldown" => 0.45,
        _ => 0.5,
    }
}

/// Parse a Camelot Wheel code (e.g. "11A", "3B") into (number 1–12, is_major).
fn parse_camelot(code: &str) -> Option<(u8, bool)> {
    let code = code.trim();
    let is_major = code.ends_with('B') || code.ends_with('b');
    let is_minor = code.ends_with('A') || code.ends_with('a');
    if !is_major && !is_minor {
        return None;
    }
    let num_str = &code[..code.len() - 1];
    let num: u8 = num_str.parse().ok()?;
    if !(1..=12).contains(&num) {
        return None;
    }
    Some((num, is_major))
}

/// Camelot Wheel compatibility score. Same key = 1.0, adjacent = 0.8,
/// parallel (same number, different mode) = 0.75, +/-2 steps = 0.5, else = 0.2.
fn camelot_compat(from: &str, to: &str) -> f64 {
    let (Some((fn_, fm)), Some((tn_, tm))) = (parse_camelot(from), parse_camelot(to)) else {
        return 0.5; // unknown keys — neutral
    };
    if fn_ == tn_ && fm == tm {
        return 1.0; // same key
    }
    if fn_ == tn_ {
        return 0.75; // parallel major/minor
    }
    let diff = fn_
        .abs_diff(tn_)
        .min(12u8.saturating_sub(fn_.abs_diff(tn_)));
    if fm == tm {
        // Same mode wheel (all major or all minor)
        match diff {
            1 => 0.8,
            2 => 0.5,
            _ => 0.2,
        }
    } else {
        // Cross-mode — slightly less compatible
        match diff {
            0 => 0.75, // already handled above, keep for clarity
            1 => 0.6,
            _ => 0.2,
        }
    }
}

/// Derive a Camelot code from a raw key string like "Am", "F#", "Bm".
fn key_string_to_camelot(key: &str) -> Option<String> {
    let k = key.trim();
    let is_minor = k.ends_with('m');
    let root = if is_minor { &k[..k.len() - 1] } else { k };
    let code = if is_minor {
        match root {
            "A" => "8A",
            "E" => "9A",
            "B" => "10A",
            "F#" | "Gb" => "11A",
            "C#" | "Db" => "12A",
            "G#" | "Ab" => "1A",
            "D#" | "Eb" => "2A",
            "A#" | "Bb" => "3A",
            "F" => "4A",
            "C" => "5A",
            "G" => "6A",
            "D" => "7A",
            _ => return None,
        }
    } else {
        match root {
            "C" => "8B",
            "G" => "9B",
            "D" => "10B",
            "A" => "11B",
            "E" => "12B",
            "B" => "1B",
            "F#" | "Gb" => "2B",
            "C#" | "Db" => "3B",
            "G#" | "Ab" => "4B",
            "D#" | "Eb" => "5B",
            "A#" | "Bb" => "6B",
            "F" => "7B",
            _ => return None,
        }
    };
    Some(code.to_string())
}

fn harmonic_compat(a: &TrackMixAnalysis, b: &TrackMixAnalysis) -> f64 {
    match (&a.key_estimate, &b.key_estimate) {
        (Some(k1), Some(k2)) if k1 == k2 => 1.0,
        (Some(k1), Some(k2)) if k1.replace('m', "") == k2.replace('m', "") => 0.7,
        (Some(_), Some(_)) => 0.35,
        _ => 0.5,
    }
}

/// Harmonic compatibility using Camelot Wheel when available in deep features,
/// falling back to the legacy key-string heuristic.
fn harmonic_compat_deep(
    a: &TrackMixAnalysis,
    b: &TrackMixAnalysis,
    deep_a: Option<&boogiebox_db::boogiemix::DeepTrackFeatures>,
    deep_b: Option<&boogiebox_db::boogiemix::DeepTrackFeatures>,
) -> f64 {
    // Prefer neural key when confidence >= 0.7
    let camelot_a = deep_a
        .and_then(|d| d.key_neural.as_ref())
        .filter(|kn| kn.confidence >= 0.7)
        .and_then(|kn| kn.camelot.clone())
        .or_else(|| a.key_estimate.as_deref().and_then(key_string_to_camelot));
    let camelot_b = deep_b
        .and_then(|d| d.key_neural.as_ref())
        .filter(|kn| kn.confidence >= 0.7)
        .and_then(|kn| kn.camelot.clone())
        .or_else(|| b.key_estimate.as_deref().and_then(key_string_to_camelot));

    match (camelot_a.as_deref(), camelot_b.as_deref()) {
        (Some(ca), Some(cb)) => camelot_compat(ca, cb),
        _ => harmonic_compat(a, b),
    }
}

/// Adjust `to_start` (entry point of the incoming track) by up to one beat so
/// that its beat phase matches `from_start`'s phase.  Both tracks must have
/// beat-grid data; otherwise `to_start` is returned unchanged.
fn align_beat_phase(
    from_start: f64,
    from_beat: Option<f64>,
    to_start: f64,
    to_beat: Option<f64>,
) -> f64 {
    let (fb, tb) = match (from_beat, to_beat) {
        (Some(f), Some(t)) if f > 0.0 && t > 0.0 => (f, t),
        _ => return to_start,
    };
    // Phase of from_start within its beat cycle (0.0–1.0).
    let from_phase = (from_start % fb) / fb;
    // Nearest beat in to_start's grid that shares the same phase.
    let to_beats_floor = (to_start / tb).floor();
    let candidate = (to_beats_floor * tb) + from_phase * tb;
    // Pick candidate or candidate + one beat, whichever is closest to to_start.
    let alt = candidate + tb;
    if (candidate - to_start).abs() <= (alt - to_start).abs() {
        candidate.max(0.0)
    } else {
        alt.max(0.0)
    }
}

fn quantize_to_phrase(value: f64, beat_grid: Option<f64>, phrase_bars: Option<i64>) -> f64 {
    let beat = match beat_grid {
        Some(b) if b > 0.0 => b,
        _ => return value,
    };
    let bars = match phrase_bars {
        Some(b) if b > 0 => b as f64,
        _ => return value,
    };
    let phrase_len = beat * bars;
    if !phrase_len.is_finite() || phrase_len <= 0.0 {
        return value;
    }
    ((value / phrase_len).round() * phrase_len).max(0.0)
}

/// Returns the best available BPM for a track: neural beat grid > analysis estimate > deep bpm_refined.
fn best_bpm(
    a: &TrackMixAnalysis,
    deep: Option<&boogiebox_db::boogiemix::DeepTrackFeatures>,
) -> Option<f64> {
    deep.and_then(|d| d.beat_grid.as_ref())
        .map(|bg| bg.bpm_neural)
        .filter(|&b| b > 20.0)
        .or(a.bpm_estimate)
        .or_else(|| deep.and_then(|d| d.bpm_refined))
}

/// Returns the nearest beat from a beat grid within `tolerance` seconds, or None.
fn snap_to_beat(beats: &[f64], target: f64, tolerance: f64) -> Option<f64> {
    beats
        .iter()
        .copied()
        .filter(|&b| (b - target).abs() <= tolerance)
        .min_by(|&a, &b| {
            (a - target)
                .abs()
                .partial_cmp(&(b - target).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn select_transition(
    from_a: &TrackMixAnalysis,
    from_t: &MixTrackInput,
    to_a: &TrackMixAnalysis,
    to_t: &MixTrackInput,
    phase: &str,
    preset: &StylePreset,
    default_crossfade: f64,
) -> MixTransition {
    let bpm_diff = match (from_a.bpm_estimate, to_a.bpm_estimate) {
        (Some(f), Some(t)) => (f - t).abs(),
        _ => 999.0,
    };
    let harmonic = harmonic_compat(from_a, to_a);
    let low_conf = from_a.confidence.min(to_a.confidence) < 0.58;

    let mut kind: &str = "blend";
    // Respect the user's crossfade preference; fall back to preset only when unset.
    let mut crossfade = if default_crossfade >= 4.0 {
        default_crossfade
    } else {
        preset.base_crossfade_sec
    };

    match phase {
        "warmup" | "groove" => {
            crossfade += 2.0;
            kind = "blend";
        }
        "lift" => {
            kind = if preset.fx_intensity > 0.45 {
                "filter_mix"
            } else {
                "blend"
            };
        }
        "peak" | "anthem" => {
            crossfade = (crossfade - 2.0).max(6.0);
            kind = if bpm_diff > 10.0 { "echo_out" } else { "cut" };
        }
        _ => {}
    }

    if bpm_diff > 14.0 || low_conf {
        crossfade = (crossfade * 0.7).max(8.0);
        if kind != "echo_out" {
            kind = "cut";
        }
    }
    if harmonic < 0.4 {
        kind = "echo_out";
        crossfade = (crossfade * 0.8).max(8.0);
    }

    let max_dur = (from_a.duration_sec * 0.35).min(to_a.duration_sec * 0.35);
    crossfade = crossfade.clamp(6.0, max_dur.min(60.0));

    // Mix from the track's natural outro start (proportional ~78% through), not
    // from duration - crossfade which pushes the transition into the silent tail.
    let mut from_start = from_a.outro_start_sec;
    from_start = quantize_to_phrase(from_start, from_a.beat_grid_sec, from_a.phrase_bars);

    let mut to_start = to_a.intro_end_sec;
    to_start = quantize_to_phrase(to_start, to_a.beat_grid_sec, to_a.phrase_bars);
    // Snap to_start to match beat phase of from_start so the downbeats align at
    // the mix point. Without this, beats can be offset by a random fraction of a
    // beat even when BPMs match.
    to_start = align_beat_phase(
        from_start,
        from_a.beat_grid_sec,
        to_start,
        to_a.beat_grid_sec,
    );

    let energy_a = estimate_energy(from_a);
    let energy_b = estimate_energy(to_a);
    let intensity = ((energy_a + energy_b) / 2.0
        + if matches!(phase, "peak" | "anthem") {
            0.2
        } else {
            0.0
        })
    .clamp(0.0, 1.0);

    let bpm_adjust_to_pct = match (from_a.bpm_estimate, to_a.bpm_estimate) {
        (Some(f), Some(t)) if f > 0.0 && t > 0.0 => ((f / t - 1.0) * 100.0).clamp(-15.0, 15.0),
        _ => 0.0,
    };

    let mut reason_bits = vec![kind.to_string(), format!("phase:{phase}")];
    if bpm_diff > 10.0 {
        reason_bits.push("bpm_mismatch".into());
    }
    if harmonic < 0.4 {
        reason_bits.push("low_harmonic_compat".into());
    }
    if low_conf {
        reason_bits.push("low_confidence".into());
    }

    let echo_tail = if kind == "echo_out" {
        (0.18 + preset.fx_intensity * 0.25).clamp(0.0, 0.6)
    } else {
        0.0
    };

    MixTransition {
        from_track_id: from_t.track_id.clone(),
        to_track_id: to_t.track_id.clone(),
        crossfade_sec: crossfade,
        from_outro_start_sec: from_start,
        to_intro_start_sec: to_start,
        phrase_aware: from_a.beat_grid_sec.is_some() && to_a.beat_grid_sec.is_some(),
        reason: reason_bits.join("|"),
        kind: kind.to_string(),
        intensity,
        eq_duck: (0.12 + intensity * 0.18).clamp(0.0, 0.35),
        bass_duck: 0.0,
        filter_sweep: kind == "filter_mix",
        echo_tail_sec: echo_tail,
        loop_build_sec: 0.0,
        bpm_adjust_from_pct: 0.0,
        bpm_adjust_to_pct,
        deep_used: false,
        drums_rms_incoming: 0.0,
    }
}

struct StyleWeights {
    vocal: f64,
    drum: f64,
    bass: f64,
    phrase: f64,
    bpm: f64,
    key: f64,
    energy: f64,
    section: f64,
    confidence: f64,
}

fn style_weights(style: &str) -> StyleWeights {
    match style {
        "safe_mix" => StyleWeights {
            vocal: 0.30,
            drum: 0.10,
            bass: 0.18,
            phrase: 0.13,
            bpm: 0.10,
            key: 0.05,
            energy: 0.06,
            section: 0.05,
            confidence: 0.03,
        },
        "club_blend" => StyleWeights {
            vocal: 0.10,
            drum: 0.28,
            bass: 0.14,
            phrase: 0.20,
            bpm: 0.13,
            key: 0.03,
            energy: 0.05,
            section: 0.04,
            confidence: 0.03,
        },
        "chill_blend" => StyleWeights {
            vocal: 0.20,
            drum: 0.06,
            bass: 0.10,
            phrase: 0.05,
            bpm: 0.05,
            key: 0.05,
            energy: 0.28,
            section: 0.18,
            confidence: 0.03,
        },
        "long_build" => StyleWeights {
            vocal: 0.10,
            drum: 0.14,
            bass: 0.10,
            phrase: 0.15,
            bpm: 0.06,
            key: 0.05,
            energy: 0.15,
            section: 0.22,
            confidence: 0.03,
        },
        _ => StyleWeights {
            vocal: 0.15,
            drum: 0.18,
            bass: 0.14,
            phrase: 0.12,
            bpm: 0.10,
            key: 0.05,
            energy: 0.13,
            section: 0.10,
            confidence: 0.03,
        },
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn near_end(end: f64, duration: f64) -> bool {
    duration <= 0.0 || (duration - end) <= 90.0
}

fn near_start(start: f64) -> bool {
    start <= 90.0
}

fn nearest_phrase_boundary(boundaries: &[f64], target: f64, tolerance: f64) -> Option<f64> {
    boundaries
        .iter()
        .copied()
        .filter(|b| (*b - target).abs() <= tolerance)
        .min_by(|a, b| {
            (a - target)
                .abs()
                .partial_cmp(&(b - target).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn section_role(deep: Option<&DeepTrackFeatures>, t: f64) -> Option<&str> {
    let deep = deep?;
    deep.sections
        .iter()
        .find(|s| t >= s.start && t <= s.end)
        .map(|s| s.kind.as_str())
}

#[derive(Debug, Clone)]
struct DeepPair {
    from_start: f64,
    to_start: f64,
    from_window: TransitionWindow,
    to_window: TransitionWindow,
    crossfade: f64,
    phrase_aligned: bool,
    score: f64,
    components: Vec<(&'static str, f64)>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TransitionCandidateLocal {
    /// Documents the Score public API surface.
    pub score: f64,
    /// Documents the Crossfade Sec public API surface.
    pub crossfade_sec: f64,
    /// Documents the Phrase Aligned public API surface.
    pub phrase_aligned: bool,
    /// Documents the From Window public API surface.
    pub from_window: TransitionWindow,
    /// Documents the To Window public API surface.
    pub to_window: TransitionWindow,
    /// Documents the Components public API surface.
    pub components: Vec<(String, f64)>,
    /// Documents the Chosen public API surface.
    pub chosen: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TransitionStepDebug {
    /// Documents the Step Index public API surface.
    pub step_index: usize,
    /// Documents the From Track Id public API surface.
    pub from_track_id: EntityId,
    /// Documents the To Track Id public API surface.
    pub to_track_id: EntityId,
    /// Documents the Style public API surface.
    pub style: String,
    /// Documents the Used Deep public API surface.
    pub used_deep: bool,
    /// Documents the Candidates public API surface.
    pub candidates: Vec<TransitionCandidateLocal>,
}

const TRANSITION_DEBUG_TOP_N: usize = 16;

#[allow(clippy::too_many_arguments)]
fn score_deep_pair(
    from_a: &TrackMixAnalysis,
    from_deep: Option<&DeepTrackFeatures>,
    to_a: &TrackMixAnalysis,
    to_deep: Option<&DeepTrackFeatures>,
    style: &str,
    from_window: &TransitionWindow,
    to_window: &TransitionWindow,
) -> (f64, Vec<(&'static str, f64)>, bool, f64, f64) {
    let weights = style_weights(style);

    let from_dur = (from_window.end - from_window.start).max(1.0);
    let to_dur = (to_window.end - to_window.start).max(1.0);
    let max_cross = from_dur.min(to_dur);
    let max_cross = max_cross
        .min(from_window.recommended_max_crossfade.max(8.0))
        .min(to_window.recommended_max_crossfade.max(8.0));
    let min_cross = from_window
        .recommended_min_crossfade
        .max(to_window.recommended_min_crossfade)
        .max(4.0);
    let crossfade = max_cross.clamp(min_cross, 60.0);

    let from_start = (from_window.end - crossfade).max(from_window.start);
    let to_start = to_window.start;

    let phrase_target_from = from_start;
    let beat_tol = 0.2_f64;
    let aligned_from = from_deep
        .and_then(|d| {
            // Prefer neural phrase boundaries if available, else fall back to autocorrelation
            if let Some(bg) = &d.beat_grid {
                if !bg.phrase_boundaries_neural.is_empty() {
                    return nearest_phrase_boundary(
                        &bg.phrase_boundaries_neural,
                        phrase_target_from,
                        beat_tol,
                    );
                }
            }
            nearest_phrase_boundary(
                &d.phrase_boundaries,
                phrase_target_from,
                d.bpm_refined
                    .or(from_a.bpm_estimate)
                    .map(|b| (60.0 / b) * 2.0)
                    .unwrap_or(1.5),
            )
        })
        .is_some();
    let aligned_to = to_deep
        .and_then(|d| {
            if let Some(bg) = &d.beat_grid {
                if !bg.phrase_boundaries_neural.is_empty() {
                    return nearest_phrase_boundary(
                        &bg.phrase_boundaries_neural,
                        to_start,
                        beat_tol,
                    );
                }
            }
            nearest_phrase_boundary(
                &d.phrase_boundaries,
                to_start,
                d.bpm_refined
                    .or(to_a.bpm_estimate)
                    .map(|b| (60.0 / b) * 2.0)
                    .unwrap_or(1.5),
            )
        })
        .is_some();
    let phrase_aligned = aligned_from && aligned_to;

    let vocal_score = 1.0
        - from_window
            .vocal_risk
            .max(to_window.vocal_risk)
            .clamp(0.0, 1.0);
    let drum_pair = (from_window.drum_continuity + to_window.drum_continuity) / 2.0;
    let drum_score = match style {
        "club_blend" => drum_pair.clamp(0.0, 1.0),
        "chill_blend" => (1.0 - drum_pair).clamp(0.0, 1.0),
        _ => drum_pair.clamp(0.0, 1.0),
    };
    let bass_score = 1.0
        - from_window
            .bass_risk
            .max(to_window.bass_risk)
            .clamp(0.0, 1.0);
    let phrase_score = if phrase_aligned {
        1.0
    } else if aligned_from || aligned_to {
        0.6
    } else {
        0.3
    };

    let bpm_score = match (best_bpm(from_a, from_deep), best_bpm(to_a, to_deep)) {
        (Some(f), Some(t)) => (1.0 - ((f - t).abs() / 12.0).min(1.0)).max(0.0),
        _ => 0.5,
    };
    let key_score = harmonic_compat_deep(from_a, to_a, from_deep, to_deep);

    let from_energy = from_deep
        .map(|d| d.energy_refined)
        .unwrap_or_else(|| estimate_energy(from_a));
    let to_energy = to_deep
        .map(|d| d.energy_refined)
        .unwrap_or_else(|| estimate_energy(to_a));
    let energy_score = match style {
        "chill_blend" => {
            let smooth = 1.0 - (from_energy - to_energy).abs();
            let low = 1.0 - from_energy.max(to_energy);
            (smooth * 0.5 + low * 0.5).clamp(0.0, 1.0)
        }
        "long_build" => {
            let delta = (to_energy - from_energy).clamp(-1.0, 1.0);
            ((delta + 0.2) / 1.2).clamp(0.0, 1.0)
        }
        "safe_mix" => 1.0 - (from_energy - to_energy).abs(),
        _ => 1.0 - (from_energy - to_energy).abs().min(1.0) * 0.6,
    };

    let from_role = section_role(from_deep, from_start);
    let to_role = section_role(to_deep, to_start);
    let section_score = match style {
        "safe_mix" => match (from_role, to_role) {
            (Some("outro"), Some("intro")) => 1.0,
            (Some("outro"), _) | (_, Some("intro")) => 0.75,
            _ => 0.5,
        },
        "club_blend" => match (from_role, to_role) {
            (Some(a), Some(b))
                if matches!(a, "outro" | "verse" | "chorus")
                    && matches!(b, "intro" | "verse" | "build") =>
            {
                1.0
            }
            _ => 0.6,
        },
        "chill_blend" => match (from_role, to_role) {
            (Some("outro" | "breakdown"), Some("intro" | "breakdown")) => 1.0,
            _ => 0.55,
        },
        "long_build" => match (from_role, to_role) {
            (Some("outro" | "breakdown"), Some("build" | "intro")) => 1.0,
            (Some("build"), Some("drop")) => 1.0,
            (Some("breakdown"), Some("drop")) => 0.9,
            _ => 0.5,
        },
        _ => 0.5,
    };

    let confidence_score = match (from_deep, to_deep) {
        (Some(f), Some(t)) => ((f.confidence + t.confidence) / 2.0).clamp(0.0, 1.0),
        (Some(f), None) | (None, Some(f)) => (f.confidence * 0.6).clamp(0.0, 1.0),
        _ => 0.0,
    };

    let total = weights.vocal * vocal_score
        + weights.drum * drum_score
        + weights.bass * bass_score
        + weights.phrase * phrase_score
        + weights.bpm * bpm_score
        + weights.key * key_score
        + weights.energy * energy_score
        + weights.section * section_score
        + weights.confidence * confidence_score;

    let components = vec![
        ("vocal", vocal_score),
        ("drum", drum_score),
        ("bass", bass_score),
        ("phrase", phrase_score),
        ("bpm", bpm_score),
        ("key", key_score),
        ("energy", energy_score),
        ("section", section_score),
        ("conf", confidence_score),
    ];
    (total, components, phrase_aligned, from_start, to_start)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn select_transition_deep(
    from_a: &TrackMixAnalysis,
    from_t: &MixTrackInput,
    from_deep: Option<&DeepTrackFeatures>,
    to_a: &TrackMixAnalysis,
    to_t: &MixTrackInput,
    to_deep: Option<&DeepTrackFeatures>,
    phase: &str,
    style: &str,
    preset: &StylePreset,
    default_crossfade: f64,
) -> MixTransition {
    select_transition_deep_with_debug(
        from_a,
        from_t,
        from_deep,
        to_a,
        to_t,
        to_deep,
        phase,
        style,
        preset,
        default_crossfade,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn select_transition_deep_with_debug(
    from_a: &TrackMixAnalysis,
    from_t: &MixTrackInput,
    from_deep: Option<&DeepTrackFeatures>,
    to_a: &TrackMixAnalysis,
    to_t: &MixTrackInput,
    to_deep: Option<&DeepTrackFeatures>,
    phase: &str,
    style: &str,
    preset: &StylePreset,
    default_crossfade: f64,
    mut debug_sink: Option<&mut Vec<TransitionCandidateLocal>>,
) -> MixTransition {
    if from_deep.is_none() && to_deep.is_none() {
        return select_transition(from_a, from_t, to_a, to_t, phase, preset, default_crossfade);
    }

    let outro_windows: Vec<&TransitionWindow> = from_deep
        .map(|d| {
            d.transition_windows
                .iter()
                .filter(|w| {
                    (w.role == "outro" || w.role == "instrumental")
                        && near_end(w.end, from_a.duration_sec)
                        && (w.end - w.start) >= 4.0
                })
                .collect()
        })
        .unwrap_or_default();
    let intro_windows: Vec<&TransitionWindow> = to_deep
        .map(|d| {
            d.transition_windows
                .iter()
                .filter(|w| {
                    (w.role == "intro" || w.role == "instrumental")
                        && near_start(w.start)
                        && (w.end - w.start) >= 4.0
                })
                .collect()
        })
        .unwrap_or_default();

    let synth_outro;
    let synth_intro;
    let outro_iter: Vec<&TransitionWindow> = if outro_windows.is_empty() {
        synth_outro = TransitionWindow {
            role: "outro".into(),
            start: from_a.outro_start_sec,
            end: from_a.duration_sec,
            score: 0.55,
            vocal_risk: 0.4,
            drum_continuity: 0.4,
            bass_risk: 0.4,
            energy: estimate_energy(from_a),
            recommended_min_crossfade: 6.0,
            recommended_max_crossfade: (from_a.duration_sec - from_a.outro_start_sec).max(8.0),
            vocals_rms: None,
            drums_rms: None,
            bass_rms: None,
            other_rms: None,
        };
        vec![&synth_outro]
    } else {
        outro_windows
    };
    let intro_iter: Vec<&TransitionWindow> = if intro_windows.is_empty() {
        synth_intro = TransitionWindow {
            role: "intro".into(),
            start: 0.0,
            end: to_a.intro_end_sec.max(8.0),
            score: 0.55,
            vocal_risk: 0.4,
            drum_continuity: 0.4,
            bass_risk: 0.4,
            energy: estimate_energy(to_a),
            recommended_min_crossfade: 6.0,
            recommended_max_crossfade: to_a.intro_end_sec.max(8.0),
            vocals_rms: None,
            drums_rms: None,
            bass_rms: None,
            other_rms: None,
        };
        vec![&synth_intro]
    } else {
        intro_windows
    };

    let mut best: Option<DeepPair> = None;
    let mut all_pairs: Vec<DeepPair> = Vec::new();
    let collect_debug = debug_sink.is_some();
    for fw in &outro_iter {
        for iw in &intro_iter {
            let (score, components, phrase_aligned, from_start, to_start) =
                score_deep_pair(from_a, from_deep, to_a, to_deep, style, fw, iw);
            let from_dur = (fw.end - fw.start).max(1.0);
            let to_dur = (iw.end - iw.start).max(1.0);
            let max_cross = from_dur
                .min(to_dur)
                .min(fw.recommended_max_crossfade.max(8.0))
                .min(iw.recommended_max_crossfade.max(8.0));
            let min_cross = fw
                .recommended_min_crossfade
                .max(iw.recommended_min_crossfade)
                .max(4.0);
            let crossfade = max_cross.clamp(min_cross, 60.0);
            let pair = DeepPair {
                from_start,
                to_start,
                from_window: (*fw).clone(),
                to_window: (*iw).clone(),
                crossfade,
                phrase_aligned,
                score,
                components,
            };
            if collect_debug {
                all_pairs.push(pair.clone());
            }
            if best.as_ref().map(|b| pair.score > b.score).unwrap_or(true) {
                best = Some(pair);
            }
        }
    }

    let Some(pair) = best else {
        return select_transition(from_a, from_t, to_a, to_t, phase, preset, default_crossfade);
    };

    if let Some(sink) = debug_sink.as_mut() {
        let chosen_score = pair.score;
        all_pairs.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut chosen_marked = false;
        for p in all_pairs.into_iter().take(TRANSITION_DEBUG_TOP_N) {
            let is_chosen = !chosen_marked && (p.score - chosen_score).abs() < 1e-9;
            if is_chosen {
                chosen_marked = true;
            }
            sink.push(TransitionCandidateLocal {
                score: round3(p.score),
                crossfade_sec: round2(p.crossfade),
                phrase_aligned: p.phrase_aligned,
                from_window: p.from_window,
                to_window: p.to_window,
                components: p
                    .components
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), round3(v)))
                    .collect(),
                chosen: is_chosen,
            });
        }
    }

    // Style adjustments applied to the window-derived crossfade, then capped at
    // the user's preference so the mix respects the configured transition length.
    let style_max = if default_crossfade >= 4.0 {
        default_crossfade
    } else {
        60.0
    };
    let crossfade = match style {
        "chill_blend" => (pair.crossfade + 4.0).min(style_max),
        "long_build" => (pair.crossfade + 2.0).min(style_max),
        "safe_mix" => pair.crossfade.clamp(8.0, 30.0_f64.min(style_max)),
        _ => pair.crossfade.clamp(6.0, style_max),
    };

    let from_start = pair.from_start;
    let from_start = if let Some(deep) = from_deep {
        let beat = from_a.beat_grid_sec.unwrap_or(0.5).max(0.25);
        nearest_phrase_boundary(&deep.phrase_boundaries, from_start, beat * 2.0)
            .unwrap_or(from_start)
    } else {
        from_start
    };
    let to_start = pair.to_start;
    let to_start = if let Some(deep) = to_deep {
        let beat = to_a.beat_grid_sec.unwrap_or(0.5).max(0.25);
        nearest_phrase_boundary(&deep.phrase_boundaries, to_start, beat * 2.0).unwrap_or(to_start)
    } else {
        to_start
    };

    let bpm_adjust_to_pct = match (from_a.bpm_estimate, to_a.bpm_estimate) {
        (Some(f), Some(t)) if f > 0.0 && t > 0.0 => {
            let target_window = match style {
                "club_blend" | "long_build" => 8.0,
                _ => 4.0,
            };
            (((f / t) - 1.0) * 100.0).clamp(-target_window, target_window)
        }
        _ => 0.0,
    };

    let vocal_overlap = pair.from_window.vocal_risk.max(pair.to_window.vocal_risk);
    let bass_overlap = pair.from_window.bass_risk.max(pair.to_window.bass_risk);
    let energy_blend = (pair.from_window.energy + pair.to_window.energy) / 2.0;

    let kind = match style {
        "safe_mix" => "safe_crossfade",
        "club_blend"
            if pair.from_window.drum_continuity > 0.4 && pair.to_window.drum_continuity > 0.4 =>
        {
            "beat_blend"
        }
        "club_blend" => "blend",
        "chill_blend" => "chill_fade",
        "long_build" => "long_build",
        _ => "blend",
    };

    // Use measured bass RMS from stem analysis when available (Phase 1c).
    // Falls back to the overlap-based heuristic for tracks without deep features.
    let bass_duck = if let Some(rms) = pair.from_window.bass_rms.filter(|&r| r > 0.3) {
        rms.clamp(0.0, 1.0)
    } else if bass_overlap > 0.45 {
        (0.45 + (bass_overlap - 0.45) * 0.8).clamp(0.0, 0.8)
    } else {
        0.0
    };
    let eq_duck = (0.10 + vocal_overlap * 0.25).clamp(0.0, 0.4);
    let filter_sweep = matches!(style, "club_blend" | "long_build") && pair.phrase_aligned;
    let echo_tail_sec = if style == "long_build" && vocal_overlap < 0.25 {
        (0.18 + preset.fx_intensity * 0.20).clamp(0.0, 0.5)
    } else {
        0.0
    };

    let components_text = pair
        .components
        .iter()
        .map(|(k, v)| format!("{k}={v:.2}"))
        .collect::<Vec<_>>()
        .join(",");
    let reason = format!(
        "deep:{style}|kind:{kind}|score:{:.2}|phrase:{}|{}",
        pair.score,
        if pair.phrase_aligned {
            "aligned"
        } else {
            "free"
        },
        components_text
    );

    MixTransition {
        from_track_id: from_t.track_id.clone(),
        to_track_id: to_t.track_id.clone(),
        crossfade_sec: crossfade,
        from_outro_start_sec: from_start,
        to_intro_start_sec: to_start,
        phrase_aware: pair.phrase_aligned,
        reason,
        kind: kind.to_string(),
        intensity: energy_blend.clamp(0.0, 1.0),
        eq_duck,
        bass_duck,
        filter_sweep,
        echo_tail_sec,
        loop_build_sec: 0.0,
        bpm_adjust_from_pct: 0.0,
        bpm_adjust_to_pct,
        deep_used: true,
        drums_rms_incoming: pair.to_window.drums_rms.unwrap_or(0.0),
    }
}

/// Cosine similarity between two embedding vectors; returns 0.0 if either is empty.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a < 1e-9 || norm_b < 1e-9 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

fn reorder_for_curve(
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    deep_features: &HashMap<String, DeepTrackFeatures>,
    preset: &StylePreset,
) -> (Vec<usize>, usize) {
    let n = tracks.len();
    let deep_for = |i: usize| deep_features.get(&tracks[i].track_id.to_string());
    let energy_of = |i: usize| effective_energy(&analyses[i], deep_for(i));

    let anthem_source = (0..n)
        .max_by(|&a, &b| energy_of(a).partial_cmp(&energy_of(b)).unwrap())
        .unwrap_or(0);

    let anthem_target = ((n as f64 * 0.72).round() as usize).clamp(1, n.saturating_sub(2).max(1));

    let curve: Vec<f64> = (0..n)
        .map(|i| {
            let phase = phase_at(i, n);
            let base = phase_target_energy(phase);
            let lift = (preset.curve_aggressiveness - 0.5) * 0.2;
            (base + lift).clamp(0.0, 1.0)
        })
        .collect();

    let mut remaining: Vec<usize> = (0..n).collect();
    let mut ordered: Vec<usize> = Vec::with_capacity(n);

    for (slot, target_e) in curve.iter().copied().enumerate().take(n) {
        if slot == anthem_target && remaining.contains(&anthem_source) {
            ordered.push(anthem_source);
            remaining.retain(|&x| x != anthem_source);
            continue;
        }
        let prev_idx = ordered.last().copied();
        let prev_a = prev_idx.map(|i| &analyses[i]);

        let best_idx = remaining
            .iter()
            .enumerate()
            .min_by(|(_, &a), (_, &b)| {
                let ea = energy_of(a);
                let eb = energy_of(b);
                let energy_a = (ea - target_e).abs();
                let energy_b = (eb - target_e).abs();
                let bpm_a = prev_a
                    .and_then(|p| p.bpm_estimate)
                    .and_then(|pb| {
                        analyses[a]
                            .bpm_estimate
                            .map(|b| (pb - b).abs().min(30.0) / 30.0)
                    })
                    .unwrap_or(0.2);
                let bpm_b = prev_a
                    .and_then(|p| p.bpm_estimate)
                    .and_then(|pb| {
                        analyses[b]
                            .bpm_estimate
                            .map(|b| (pb - b).abs().min(30.0) / 30.0)
                    })
                    .unwrap_or(0.2);
                let harm_a = prev_a.map_or(0.0, |p| 1.0 - harmonic_compat(p, &analyses[a]));
                let harm_b = prev_a.map_or(0.0, |p| 1.0 - harmonic_compat(p, &analyses[b]));
                // Phase 4d: cosine similarity boost for embedding-similar adjacent tracks
                let prev_emb = prev_idx.and_then(|pi| {
                    deep_for(pi)
                        .and_then(|d| d.neural_embedding.as_ref())
                        .map(|ne| ne.embedding_16d.as_slice())
                });
                let cosine_bonus_a = prev_emb.map_or(0.0, |prev_e| {
                    let sim = deep_for(a)
                        .and_then(|d| d.neural_embedding.as_ref())
                        .map_or(0.0, |ne| cosine_similarity(prev_e, &ne.embedding_16d));
                    if sim > 0.7 {
                        (sim - 0.7) * 0.5
                    } else {
                        0.0
                    }
                });
                let cosine_bonus_b = prev_emb.map_or(0.0, |prev_e| {
                    let sim = deep_for(b)
                        .and_then(|d| d.neural_embedding.as_ref())
                        .map_or(0.0, |ne| cosine_similarity(prev_e, &ne.embedding_16d));
                    if sim > 0.7 {
                        (sim - 0.7) * 0.5
                    } else {
                        0.0
                    }
                });
                let score_a =
                    energy_a * 0.50 + bpm_a * 0.22 + harm_a * 0.18 - cosine_bonus_a * 0.10;
                let score_b =
                    energy_b * 0.50 + bpm_b * 0.22 + harm_b * 0.18 - cosine_bonus_b * 0.10;
                score_a.partial_cmp(&score_b).unwrap()
            })
            .map(|(idx, _)| idx);

        if let Some(idx) = best_idx {
            let track_idx = remaining[idx];
            ordered.push(track_idx);
            remaining.remove(idx);
        }
    }

    let anthem_pos = ordered
        .iter()
        .position(|&i| i == anthem_source)
        .unwrap_or(0);
    (ordered, anthem_pos)
}

#[allow(clippy::too_many_arguments)]
fn deterministic_plan(
    playlist_id: &EntityId,
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    deep_features: &HashMap<String, DeepTrackFeatures>,
    default_crossfade: f64,
    style: &str,
    collect_debug: bool,
    debug_steps: &mut Vec<TransitionStepDebug>,
) -> MixPlan {
    let preset = style_preset(style);
    let n = tracks.len();

    let (ordered_indices, anthem_pos) = if preset.allow_resequence && n > 3 {
        reorder_for_curve(tracks, analyses, deep_features, &preset)
    } else {
        let anthem = n.saturating_sub(1);
        ((0..n).collect(), anthem)
    };

    let ordered_tracks: Vec<&MixTrackInput> = ordered_indices.iter().map(|&i| &tracks[i]).collect();
    let ordered_analyses: Vec<&TrackMixAnalysis> =
        ordered_indices.iter().map(|&i| &analyses[i]).collect();

    let curve: Vec<(&str, f64)> = (0..n)
        .map(|i| {
            let phase = phase_at(i, n);
            let base = phase_target_energy(phase);
            let lift = (preset.curve_aggressiveness - 0.5) * 0.2;
            (phase, (base + lift).clamp(0.0, 1.0))
        })
        .collect();

    let mut transitions = Vec::new();
    let mut per_track_energy = HashMap::new();

    for (i, t) in ordered_tracks.iter().enumerate() {
        let deep = deep_features.get(&t.track_id.to_string());
        let e = effective_energy(ordered_analyses[i], deep);
        per_track_energy.insert(t.track_id.to_string(), (e * 1000.0).round() / 1000.0);
    }

    for i in 0..n.saturating_sub(1) {
        let phase = curve[i].0;
        let from_deep = deep_features.get(&ordered_tracks[i].track_id.to_string());
        let to_deep = deep_features.get(&ordered_tracks[i + 1].track_id.to_string());
        let mut per_pair: Vec<TransitionCandidateLocal> = Vec::new();
        let sink: Option<&mut Vec<TransitionCandidateLocal>> = if collect_debug {
            Some(&mut per_pair)
        } else {
            None
        };
        let t = select_transition_deep_with_debug(
            ordered_analyses[i],
            ordered_tracks[i],
            from_deep,
            ordered_analyses[i + 1],
            ordered_tracks[i + 1],
            to_deep,
            phase,
            style,
            &preset,
            default_crossfade,
            sink,
        );
        if collect_debug && !per_pair.is_empty() {
            debug_steps.push(TransitionStepDebug {
                step_index: i,
                from_track_id: ordered_tracks[i].track_id.clone(),
                to_track_id: ordered_tracks[i + 1].track_id.clone(),
                style: style.to_string(),
                used_deep: t.deep_used,
                candidates: per_pair,
            });
        }
        transitions.push(t);
    }

    let anthem_track_id = ordered_tracks.get(anthem_pos).map(|t| t.track_id.clone());

    MixPlan {
        playlist_id: playlist_id.clone(),
        default_crossfade_sec: default_crossfade,
        target_energy_ramp: curve.iter().map(|(_, e)| *e).collect(),
        transitions,
        style: style.to_string(),
        ordered_track_ids: ordered_tracks.iter().map(|t| t.track_id.clone()).collect(),
        energy_curve_phases: curve.iter().map(|(p, _)| p.to_string()).collect(),
        per_track_energy,
        anthem_track_id,
    }
}

// ── AI planner ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn create_plan(
    state: &PostScanState,
    playlist_id: &EntityId,
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    deep_features: &HashMap<String, DeepTrackFeatures>,
    default_crossfade: f64,
    style: &str,
    collect_debug: bool,
) -> (MixPlan, String, String, Vec<TransitionStepDebug>) {
    let (gemini_key, openrouter_key) = match state.db.lock() {
        Ok(conn) => (
            get_setting(&conn, "boogiemixGeminiApiKey").unwrap_or_default(),
            get_setting(&conn, "boogiemixOpenRouterApiKey").unwrap_or_default(),
        ),
        Err(_) => (String::new(), String::new()),
    };

    let gemini_key = gemini_key.trim().to_string();
    let openrouter_key = openrouter_key.trim().to_string();

    if !gemini_key.is_empty() || !openrouter_key.is_empty() {
        let prompt = build_ai_prompt(tracks, analyses, default_crossfade, style);

        if !gemini_key.is_empty() {
            if let Some(plan) = try_gemini(
                &state.http_client,
                &gemini_key,
                &prompt,
                playlist_id,
                tracks,
                analyses,
                default_crossfade,
                style,
            )
            .await
            {
                return (plan, "AI Mix Strategy".into(), "gemini".into(), Vec::new());
            }
        }

        if !openrouter_key.is_empty() {
            if let Some(plan) = try_openrouter(
                &state.http_client,
                &openrouter_key,
                &prompt,
                playlist_id,
                tracks,
                analyses,
                default_crossfade,
                style,
            )
            .await
            {
                return (
                    plan,
                    "AI Mix Strategy".into(),
                    "openrouter".into(),
                    Vec::new(),
                );
            }
        }
    }

    let mut debug_steps: Vec<TransitionStepDebug> = Vec::new();
    let plan = deterministic_plan(
        playlist_id,
        tracks,
        analyses,
        deep_features,
        default_crossfade,
        style,
        collect_debug,
        &mut debug_steps,
    );
    let strategy = if plan.transitions.iter().any(|t| t.deep_used) {
        "Deterministic (deep features)".to_string()
    } else {
        "Deterministic fallback".to_string()
    };
    (plan, strategy, "deterministic".into(), debug_steps)
}

fn build_ai_prompt(
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    default_crossfade: f64,
    style: &str,
) -> String {
    let track_list: Vec<Value> = tracks
        .iter()
        .zip(analyses.iter())
        .map(|(t, a)| {
            serde_json::json!({
                "id": t.track_id.to_string(),
                "title": t.title,
                "artist": t.artist,
                "duration": a.duration_sec,
                "bpm": a.bpm_estimate,
                "loudness": a.loudness_lufs,
                "energy": (estimate_energy(a) * 1000.0).round() / 1000.0,
                "introEnd": a.intro_end_sec,
                "outroStart": a.outro_start_sec,
                "key": a.key_estimate,
            })
        })
        .collect();

    format!(
        "You are a professional DJ. Create a mix plan for the following playlist in {} style \
         with approximately {:.0}s crossfades. For each transition pick optimal start/end points \
         based on track structure. Return only a JSON object with: strategy (string), \
         orderedTrackIds (array of id strings), transitions (array of objects with fields: \
         fromTrackId, toTrackId, startA, endA, startB, endB, type, confidence, \
         bpmAdjustA, bpmAdjustB, pitchAdjustA, pitchAdjustB). \
         Types: blend|echo_out|filter_mix|cut|long_build. Confidence 0-1.\n\nTracks:\n{}",
        style,
        default_crossfade,
        serde_json::to_string_pretty(&track_list).unwrap_or_default()
    )
}

#[allow(clippy::too_many_arguments)]
async fn try_gemini(
    client: &reqwest::Client,
    api_key: &str,
    prompt: &str,
    playlist_id: &EntityId,
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    default_crossfade: f64,
    style: &str,
) -> Option<MixPlan> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={api_key}"
    );
    let body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {"responseMimeType": "application/json", "temperature": 0.7}
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(45))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let json: Value = resp.json().await.ok()?;
    let text = json["candidates"][0]["content"]["parts"][0]["text"].as_str()?;
    parse_ai_plan_response(
        text,
        playlist_id,
        tracks,
        analyses,
        default_crossfade,
        style,
    )
}

#[allow(clippy::too_many_arguments)]
async fn try_openrouter(
    client: &reqwest::Client,
    api_key: &str,
    prompt: &str,
    playlist_id: &EntityId,
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    default_crossfade: f64,
    style: &str,
) -> Option<MixPlan> {
    let body = serde_json::json!({
        "model": "google/gemma-3-27b-it:free",
        "messages": [
            {"role": "system", "content": "You are a professional DJ mix planner. Always respond with valid JSON only."},
            {"role": "user", "content": prompt}
        ],
        "response_format": {"type": "json_object"},
        "temperature": 0.7
    });

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("HTTP-Referer", "https://github.com/boogiebox/boogiebox")
        .json(&body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let json: Value = resp.json().await.ok()?;
    let text = json["choices"][0]["message"]["content"].as_str()?;
    parse_ai_plan_response(
        text,
        playlist_id,
        tracks,
        analyses,
        default_crossfade,
        style,
    )
}

fn parse_ai_plan_response(
    text: &str,
    playlist_id: &EntityId,
    tracks: &[MixTrackInput],
    analyses: &[TrackMixAnalysis],
    default_crossfade: f64,
    style: &str,
) -> Option<MixPlan> {
    let json: Value = serde_json::from_str(text).ok().or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')? + 1;
        serde_json::from_str(&text[start..end]).ok()
    })?;

    let transitions_val = json["transitions"].as_array()?;
    if transitions_val.is_empty() {
        return None;
    }

    let track_map: HashMap<String, usize> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.track_id.to_string(), i))
        .collect();

    let preset = style_preset(style);
    let mut transitions: Vec<MixTransition> = Vec::new();

    for t in transitions_val {
        let from_id = t["fromTrackId"]
            .as_str()
            .or_else(|| t["from_track_id"].as_str())?;
        let to_id = t["toTrackId"]
            .as_str()
            .or_else(|| t["to_track_id"].as_str())?;
        let from_idx = *track_map.get(from_id)?;
        let to_idx = *track_map.get(to_id)?;
        let from_a = &analyses[from_idx];
        let to_a = &analyses[to_idx];

        let start_a = t["startA"]
            .as_f64()
            .or_else(|| t["start_a"].as_f64())
            .unwrap_or(from_a.outro_start_sec);
        let end_a = t["endA"]
            .as_f64()
            .or_else(|| t["end_a"].as_f64())
            .unwrap_or(from_a.duration_sec);
        let start_b = t["startB"]
            .as_f64()
            .or_else(|| t["start_b"].as_f64())
            .unwrap_or(to_a.intro_end_sec);

        let kind_str = t["type"].as_str().unwrap_or("blend");
        let kind = match kind_str {
            "echo_out" | "filter_mix" | "cut" | "long_build" => kind_str.to_string(),
            _ => "blend".to_string(),
        };

        let confidence = t["confidence"].as_f64().unwrap_or(0.7).clamp(0.0, 1.0);
        let crossfade = ((end_a - start_a)
            .abs()
            .min((to_a.duration_sec * 0.45).min(from_a.duration_sec * 0.45)))
        .clamp(8.0, 90.0);
        let bpm_adjust_b = t["bpmAdjustB"]
            .as_f64()
            .or_else(|| t["bpm_adjust_b"].as_f64())
            .unwrap_or(0.0);

        let echo_tail = if kind == "echo_out" {
            (0.18 + preset.fx_intensity * 0.25).clamp(0.0, 0.6)
        } else {
            0.0
        };
        let intensity = ((estimate_energy(from_a) + estimate_energy(to_a)) / 2.0).clamp(0.0, 1.0);

        transitions.push(MixTransition {
            from_track_id: tracks[from_idx].track_id.clone(),
            to_track_id: tracks[to_idx].track_id.clone(),
            crossfade_sec: crossfade,
            from_outro_start_sec: start_a,
            to_intro_start_sec: start_b,
            phrase_aware: true,
            reason: format!("ai:{kind}"),
            kind: kind.clone(),
            intensity,
            eq_duck: (0.12 + intensity * 0.18).clamp(0.0, 0.35),
            bass_duck: 0.0,
            filter_sweep: kind == "filter_mix",
            echo_tail_sec: echo_tail,
            loop_build_sec: 0.0,
            bpm_adjust_from_pct: 0.0,
            bpm_adjust_to_pct: bpm_adjust_b,
            deep_used: false,
            drums_rms_incoming: 0.0,
        });
        let _ = confidence;
    }

    if transitions.is_empty() {
        return None;
    }

    let ordered_track_ids = derive_ordered_ids(&transitions, tracks);
    let per_track_energy: HashMap<String, f64> = tracks
        .iter()
        .zip(analyses.iter())
        .map(|(t, a)| {
            (
                t.track_id.to_string(),
                (estimate_energy(a) * 1000.0).round() / 1000.0,
            )
        })
        .collect();
    let n = tracks.len();
    let target_energy_ramp = (0..n)
        .map(|i| phase_target_energy(phase_at(i, n)))
        .collect();
    let energy_curve_phases = (0..n).map(|i| phase_at(i, n).to_string()).collect();

    Some(MixPlan {
        playlist_id: playlist_id.clone(),
        default_crossfade_sec: default_crossfade,
        target_energy_ramp,
        transitions,
        style: style.to_string(),
        ordered_track_ids,
        energy_curve_phases,
        per_track_energy,
        anthem_track_id: None,
    })
}

fn derive_ordered_ids(transitions: &[MixTransition], tracks: &[MixTrackInput]) -> Vec<EntityId> {
    if transitions.is_empty() {
        return tracks.iter().map(|t| t.track_id.clone()).collect();
    }

    let all_ids: std::collections::HashSet<String> =
        tracks.iter().map(|t| t.track_id.to_string()).collect();
    let to_ids: std::collections::HashSet<String> = transitions
        .iter()
        .map(|t| t.to_track_id.to_string())
        .collect();
    let start = transitions
        .iter()
        .map(|t| t.from_track_id.to_string())
        .find(|id| !to_ids.contains(id))
        .unwrap_or_else(|| transitions[0].from_track_id.to_string());

    let next_map: HashMap<String, String> = transitions
        .iter()
        .map(|t| (t.from_track_id.to_string(), t.to_track_id.to_string()))
        .collect();

    let mut ordered: Vec<EntityId> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut current = start;

    while !seen.contains(&current) && all_ids.contains(&current) {
        if let Some(t) = tracks.iter().find(|t| t.track_id.to_string() == current) {
            ordered.push(t.track_id.clone());
        }
        seen.insert(current.clone());
        match next_map.get(&current) {
            Some(next) => current = next.clone(),
            None => break,
        }
    }

    for t in tracks {
        if !seen.contains(&t.track_id.to_string()) {
            ordered.push(t.track_id.clone());
        }
    }

    ordered
}

// ── FFmpeg renderer ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct RenderTiming {
    start: f64,
    trim_dur: f64,
    effective_dur: f64,
    fade_in_sec: f64,
    fade_out_sec: f64,
    gain_db: f64,
    bpm_adjust_pct: f64,
}

fn fade_curves_for_style(style: &str) -> (&'static str, &'static str) {
    // All styles use equal-power curves (qsin = quarter-sine) so the combined
    // loudness stays constant across the crossfade. Linear (tri) or convex (ipar)
    // curves cause a ~3 dB power dip at the midpoint for uncorrelated audio.
    match style {
        "long_build" => ("cub", "ihsin"),
        _ => ("qsin", "qsin"),
    }
}

fn compute_render_timings(
    analyses: &[&TrackMixAnalysis],
    transitions: &[MixTransition],
    bpm_factors: &[f64],
) -> Vec<RenderTiming> {
    let n = analyses.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = analyses[i];
        let incoming = if i > 0 { transitions.get(i - 1) } else { None };
        let outgoing = transitions.get(i);

        let start = incoming.map(|t| t.to_intro_start_sec).unwrap_or(0.0);
        // Trim track A to end at from_outro_start_sec so the acrossfade crossfade
        // region covers the energetic body (just before the quiet outro), not the
        // silent tail.  The quiet outro is discarded — a DJ never plays it.
        let end = outgoing
            .map(|t| t.from_outro_start_sec.min(a.duration_sec))
            .unwrap_or(a.duration_sec);
        let loop_build = outgoing.map(|t| t.loop_build_sec).unwrap_or(0.0);
        let trim_dur = (end - start + loop_build).max(8.0);
        let bpm_factor = bpm_factors[i];
        let effective_dur = trim_dur / bpm_factor;
        let fade_in_sec = incoming
            .map(|t| t.crossfade_sec)
            .unwrap_or(0.0)
            .clamp(0.0, effective_dur.max(1.0));
        let fade_out_sec = outgoing
            .map(|t| t.crossfade_sec)
            .unwrap_or(0.0)
            .clamp(0.0, effective_dur.max(1.0));
        let loudness = a.loudness_lufs.unwrap_or(-14.0);
        let eq_duck = outgoing.map(|t| t.eq_duck).unwrap_or(0.0);
        // Allow up to -12 dB reduction for very hot modern masters (e.g. -8 LUFS),
        // but cap gain boost at +6 dB to avoid clipping quiet tracks.
        let base_gain = (-14.0 - loudness).clamp(-12.0, 6.0);
        let duck = (eq_duck * 6.0).min(3.0);
        out.push(RenderTiming {
            start,
            trim_dur,
            effective_dur,
            fade_in_sec,
            fade_out_sec,
            gain_db: base_gain - duck,
            bpm_adjust_pct: (bpm_factor - 1.0) * 100.0,
        });
    }
    out
}

fn build_filter_complex(plan: &MixPlan, timings: &[RenderTiming], n: usize) -> String {
    let safe_mode = plan.style == "safe_mix";
    let (fc_in_default, fc_out_default) = fade_curves_for_style(&plan.style);
    let transitions = &plan.transitions;

    // capacity: N per-track parts + (N-1) acrossfade parts + 1 limiter
    let mut parts: Vec<String> = Vec::with_capacity(n * 2);

    // ── Per-track pre-processing ──────────────────────────────────────────────
    for (i, t) in timings.iter().enumerate() {
        let outgoing = transitions.get(i);
        let is_first = i == 0;
        let is_last = i == n - 1;

        // Pitch-preserving tempo adjustment. aresample/asetrate also shifts pitch;
        // atempo stretches time only, keeping the original pitch.
        let tempo = if t.bpm_adjust_pct.abs() > 0.01 {
            let factor = 1.0 + t.bpm_adjust_pct / 100.0;
            if factor.is_finite() && (0.5..2.0).contains(&factor) {
                format!(",atempo={factor:.6}")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Gentle open/close fades only on the very first and very last track.
        // Middle-track crossfades are handled by acrossfade below.
        let fade_in = if is_first {
            ",afade=t=in:st=0:d=2:curve=tri".to_string()
        } else {
            String::new()
        };
        let fade_out = if is_last {
            let d = 4.0_f64.min(t.effective_dur);
            let st = (t.effective_dur - d).max(0.0);
            format!(",afade=t=out:st={st:.3}:d={d:.3}:curve=tri")
        } else {
            String::new()
        };

        // Effects are timed to the outgoing crossfade window in the track's own
        // local time (starting from 0 after atrim + asetpts).
        let crossfade_dur = outgoing.map(|o| o.crossfade_sec).unwrap_or(0.0);
        let fade_start = (t.effective_dur - crossfade_dur).max(0.0);
        let fade_end = t.effective_dur;

        let bass_duck_db = outgoing.map(|o| o.bass_duck).unwrap_or(0.0);
        let bass_duck = if !is_last && !safe_mode && bass_duck_db > 0.05 {
            let g = (bass_duck_db * 10.0).clamp(2.0, 10.0);
            format!(",bass=g=-{g:.2}:f=180:enable='between(t,{fade_start:.3},{fade_end:.3})'")
        } else {
            String::new()
        };

        let sweep_on = !safe_mode && !is_last && outgoing.map(|o| o.filter_sweep).unwrap_or(false);
        let sweep = if sweep_on {
            format!(
                ",highpass=f=200:enable='between(t,{fade_start:.3},{fade_end:.3})',\
                 lowpass=f=8000:enable='between(t,{fade_start:.3},{fade_end:.3})'"
            )
        } else {
            String::new()
        };

        let echo_tail = outgoing.map(|o| o.echo_tail_sec).unwrap_or(0.0);
        let echo = if !safe_mode && !is_last && echo_tail > 0.0 {
            format!(",aecho=0.8:0.7:120:{:.3}", echo_tail.min(0.7))
        } else {
            String::new()
        };

        let pad = match outgoing.map(|o| o.loop_build_sec) {
            Some(p) if p > 0.0 => format!(",apad=pad_dur={:.3}", p.min(3.0)),
            _ => String::new(),
        };

        // Two-phase drum entry (Phase 1d): when the incoming track has strong drums,
        // briefly reduce its high-frequency content in the first 40% of the crossfade
        // window to let the outgoing track's outro breathe before the full blend.
        let incoming_transition = if i > 0 { transitions.get(i - 1) } else { None };
        let drum_intro = if !is_first && !safe_mode {
            if let Some(inc_trans) = incoming_transition {
                let drums_rms = inc_trans.drums_rms_incoming;
                if drums_rms > 0.25 {
                    let phase_a_end = (inc_trans.crossfade_sec * 0.40).min(8.0);
                    format!(
                        ",highpass=f=300:enable='lt(t,{phase_a_end:.3})',\
                         volume=0.75:enable='lt(t,{phase_a_end:.3})'"
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        parts.push(format!(
            "[{i}:a]atrim=start={:.3}:duration={:.3},asetpts=PTS-STARTPTS,\
             volume={:.3}dB{tempo}{fade_in}{fade_out}{bass_duck}{sweep}{echo}{drum_intro}{pad}[t{i}]",
            t.start, t.trim_dur, t.gain_db
        ));
    }

    // ── Chain acrossfade transitions ──────────────────────────────────────────
    // acrossfade properly crossfades both tracks simultaneously over the full
    // crossfade window, unlike amix+adelay which can leave both at full volume.
    if n == 1 {
        parts.push("[t0]alimiter=limit=0.95,aresample=44100:resampler=soxr[mixout]".to_string());
    } else {
        let mut cur = "t0".to_string();
        for (i, trans) in transitions.iter().enumerate().take(n - 1) {
            // Must not exceed either track's effective duration (acrossfade limit).
            let max_a = timings.get(i).map(|t| t.effective_dur).unwrap_or(59.0);
            let max_b = timings.get(i + 1).map(|t| t.effective_dur).unwrap_or(59.0);
            let cross_dur = trans.crossfade_sec.clamp(2.0, 120.0).min(max_a).min(max_b);
            let next = format!("t{}", i + 1);
            let out = if i == n - 2 {
                "mixout_raw".to_string()
            } else {
                format!("m{i}")
            };
            parts.push(format!(
                "[{cur}][{next}]acrossfade=d={cross_dur:.3}:c1={fc_out_default}:c2={fc_in_default}[{out}]"
            ));
            cur = out;
        }
        parts.push(
            "[mixout_raw]alimiter=limit=0.95,aresample=44100:resampler=soxr[mixout]".to_string(),
        );
    }

    parts.join(";")
}

#[allow(clippy::too_many_arguments)]
async fn render_mix(
    state: &PostScanState,
    job_id: &EntityId,
    ffmpeg: &Path,
    tracks: &[&MixTrackInput],
    analyses: &[&TrackMixAnalysis],
    deep_features: &HashMap<String, DeepTrackFeatures>,
    plan: &MixPlan,
    out_path: &Path,
) -> Result<u64, String> {
    let n = tracks.len();
    let transitions = &plan.transitions;

    // Resolve BPM for each ordered track using same priority chain as best_bpm():
    // neural beat grid (most accurate) → standard mix estimate → Demucs-refined.
    let bpms: Vec<Option<f64>> = tracks
        .iter()
        .zip(analyses.iter())
        .map(|(t, a)| {
            let deep = deep_features.get(&t.track_id.to_string());
            deep.and_then(|d| d.beat_grid.as_ref())
                .map(|bg| bg.bpm_neural)
                .filter(|&b| (60.0..=220.0).contains(&b))
                .or(a.bpm_estimate)
                .or_else(|| {
                    deep.and_then(|d| d.bpm_refined)
                        .filter(|&b| (60.0..=220.0).contains(&b))
                })
        })
        .collect();

    tracing::debug!(resolved_bpms = ?bpms, "per-track BPM resolution");

    let mut bpm_factors = vec![1.0f64; n];
    for i in 1..n {
        if let (Some(prev_bpm), Some(curr_bpm)) = (bpms[i - 1], bpms[i]) {
            if prev_bpm > 0.0 && curr_bpm > 0.0 {
                let prev_eff = prev_bpm * bpm_factors[i - 1];
                bpm_factors[i] = (prev_eff / curr_bpm).clamp(0.85, 1.15);
            }
        }
    }

    let timings = compute_render_timings(analyses, transitions, &bpm_factors);

    for (i, t) in transitions.iter().enumerate() {
        tracing::debug!(
            track_i = i,
            from_outro_start = t.from_outro_start_sec,
            to_intro_start = t.to_intro_start_sec,
            crossfade_sec = t.crossfade_sec,
            trim_dur = timings[i].trim_dur,
            "transition timing"
        );
    }

    let filter_complex = build_filter_complex(plan, &timings, n);
    tracing::debug!(filter_complex = %filter_complex, "render filter graph");

    let mut input_args: Vec<String> = Vec::new();
    for track in tracks.iter().take(n) {
        input_args.push("-i".into());
        input_args.push(track.file_path.clone());
    }

    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-y"]);
    for arg in &input_args {
        cmd.arg(arg);
    }
    cmd.args([
        "-filter_complex",
        &filter_complex,
        "-map",
        "[mixout]",
        "-c:a",
        "libmp3lame",
        "-b:a",
        "320k",
    ]);
    cmd.arg(out_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;

    let cancel_check = async {
        let mut interval = tokio::time::interval(Duration::from_millis(1500));
        interval.tick().await;
        loop {
            interval.tick().await;
            let canceled = state
                .db
                .lock()
                .ok()
                .map(|conn| is_mix_job_canceled(&conn, job_id).unwrap_or(false))
                .unwrap_or(false);
            if canceled {
                return;
            }
        }
    };

    tokio::pin!(cancel_check);

    let output = tokio::select! {
        res = child.wait_with_output() => {
            res.map_err(|e| format!("ffmpeg wait failed: {e}"))?
        }
        _ = &mut cancel_check => {
            let _ = tokio::fs::remove_file(out_path).await;
            return Err("canceled".into());
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = tokio::fs::remove_file(out_path).await;
        return Err(format!(
            "FFmpeg render failed: {}",
            stderr.chars().take(600).collect::<String>()
        ));
    }

    let meta = std::fs::metadata(out_path).map_err(|e| format!("Output file not found: {e}"))?;
    Ok(meta.len())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn lock_db(db: &DbPool) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, String> {
    db.lock().map_err(|e| e.to_string())
}

async fn wait_for_deep_analysis(
    state: &PostScanState,
    tracks: &[MixTrackInput],
    wait_for: Duration,
) -> Result<usize, String> {
    let track_ids: Vec<EntityId> = tracks.iter().map(|track| track.track_id.clone()).collect();
    let deadline = tokio::time::Instant::now() + wait_for;
    loop {
        let ready = {
            let conn = lock_db(&state.db)?;
            count_deep_analysis_ready(&conn, &track_ids).map_err(|e| e.to_string())?
        };
        if ready >= track_ids.len() || tokio::time::Instant::now() >= deadline {
            return Ok(ready);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

fn get_mix_output_dir_fn(db: &DbPool, db_folder: Option<&PathBuf>) -> PathBuf {
    let configured = db
        .lock()
        .ok()
        .and_then(|conn| get_mix_output_dir_from_db(&conn));

    if let Some(dir) = configured {
        let dir = dir.trim().to_string();
        if !dir.is_empty() {
            let p = PathBuf::from(&dir);
            if p.is_absolute() {
                return p;
            }
            if let Some(base) = db_folder {
                return base.join(p);
            }
            return p;
        }
    }

    if let Some(base) = db_folder {
        return base.join("mix-outputs");
    }

    PathBuf::from("mix-outputs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::boogiemix::{StemSummary, TrackSection};

    fn analysis(id: &str, bpm: f64) -> TrackMixAnalysis {
        TrackMixAnalysis {
            track_id: EntityId::Str(id.into()),
            duration_sec: 240.0,
            bpm_estimate: Some(bpm),
            loudness_lufs: Some(-14.0),
            key_estimate: Some("Am".into()),
            beat_grid_sec: Some(60.0 / bpm),
            phrase_bars: Some(8),
            intro_start_sec: 0.0,
            intro_end_sec: 16.0,
            outro_start_sec: 210.0,
            outro_end_sec: 240.0,
            low_energy_start_sec: Some(0.0),
            low_energy_end_sec: Some(16.0),
            high_energy_start_sec: Some(60.0),
            high_energy_end_sec: Some(200.0),
            confidence: 0.85,
        }
    }

    fn input_track(id: &str) -> MixTrackInput {
        MixTrackInput {
            track_id: EntityId::Str(id.into()),
            file_path: format!("D:\\Music\\{id}.mp3"),
            title: Some(id.into()),
            artist: Some("Artist".into()),
            duration: Some(240.0),
            bpm: Some(120.0),
            bpm_detected: Some(120.0),
            file_size: Some(1234),
            scanned_at: Some("2026-05-26".into()),
            position: 0,
        }
    }

    fn deep_for(id: &str, vocal_risk: f64, drum_cont: f64, bass_risk: f64) -> DeepTrackFeatures {
        DeepTrackFeatures {
            track_id: EntityId::Str(id.into()),
            analysis_schema_version: 2,
            confidence: 0.85,
            energy_refined: 0.55,
            used_gpu: false,
            demucs_model: "htdemucs".into(),
            vocal_windows: Vec::new(),
            drum_windows: Vec::new(),
            bass_windows: Vec::new(),
            sections: vec![
                TrackSection {
                    kind: "intro".into(),
                    start: 0.0,
                    end: 16.0,
                    confidence: 0.7,
                    vocal_density: 0.1,
                    drum_density: 0.3,
                    energy: 0.3,
                },
                TrackSection {
                    kind: "outro".into(),
                    start: 210.0,
                    end: 240.0,
                    confidence: 0.7,
                    vocal_density: 0.1,
                    drum_density: 0.3,
                    energy: 0.3,
                },
            ],
            phrase_boundaries: (0..30).map(|i| (i as f64) * 8.0).collect(),
            transition_windows: vec![
                TransitionWindow {
                    role: "intro".into(),
                    start: 0.0,
                    end: 16.0,
                    score: 0.85,
                    vocal_risk,
                    drum_continuity: drum_cont,
                    bass_risk,
                    energy: 0.35,
                    recommended_min_crossfade: 6.0,
                    recommended_max_crossfade: 16.0,
                    vocals_rms: Some(vocal_risk),
                    drums_rms: Some(drum_cont),
                    bass_rms: Some(bass_risk),
                    other_rms: None,
                },
                TransitionWindow {
                    role: "outro".into(),
                    start: 210.0,
                    end: 240.0,
                    score: 0.85,
                    vocal_risk,
                    drum_continuity: drum_cont,
                    bass_risk,
                    energy: 0.35,
                    recommended_min_crossfade: 6.0,
                    recommended_max_crossfade: 24.0,
                    vocals_rms: Some(vocal_risk),
                    drums_rms: Some(drum_cont),
                    bass_rms: Some(bass_risk),
                    other_rms: None,
                },
            ],
            bpm_refined: Some(120.0),
            summary: StemSummary {
                vocal_density: vocal_risk,
                drum_density: drum_cont,
                bass_density: bass_risk,
                other_density: 0.4,
                instrumental_ratio: 1.0 - vocal_risk,
                has_long_intro: true,
                has_long_outro: true,
            },
            key_neural: None,
            cue_points: None,
            beat_grid: None,
            neural_embedding: None,
        }
    }

    #[test]
    fn safe_mix_penalises_vocal_overlap_in_scoring() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.1, 0.3, 0.2);
        let to_deep = deep_for("b", 0.1, 0.3, 0.2);

        let clean_window = from_deep.transition_windows[1].clone();
        let vocal_window = TransitionWindow {
            vocal_risk: 0.9,
            ..clean_window.clone()
        };

        let (clean_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "safe_mix",
            &clean_window,
            &to_deep.transition_windows[0],
        );
        let (vocal_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "safe_mix",
            &vocal_window,
            &to_deep.transition_windows[0],
        );
        assert!(clean_score > vocal_score);
    }

    #[test]
    fn club_blend_prefers_drum_continuity() {
        let from_a = analysis("a", 124.0);
        let to_a = analysis("b", 124.0);
        let from_deep = deep_for("a", 0.2, 0.85, 0.3);
        let to_deep = deep_for("b", 0.2, 0.85, 0.3);

        let outro = from_deep.transition_windows[1].clone();
        let intro = to_deep.transition_windows[0].clone();

        let (steady_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "club_blend",
            &outro,
            &intro,
        );
        let sparse_outro = TransitionWindow {
            drum_continuity: 0.1,
            ..outro
        };
        let sparse_intro = TransitionWindow {
            drum_continuity: 0.1,
            ..intro
        };
        let (sparse_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "club_blend",
            &sparse_outro,
            &sparse_intro,
        );
        assert!(steady_score > sparse_score);
    }

    #[test]
    fn long_build_rewards_rising_energy() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 124.0);
        let mut from_deep = deep_for("a", 0.2, 0.5, 0.3);
        let mut to_deep = deep_for("b", 0.2, 0.7, 0.3);
        from_deep.energy_refined = 0.30;
        to_deep.energy_refined = 0.85;

        let outro = from_deep.transition_windows[1].clone();
        let intro = to_deep.transition_windows[0].clone();

        let (rising_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "long_build",
            &outro,
            &intro,
        );
        from_deep.energy_refined = 0.85;
        to_deep.energy_refined = 0.30;
        let (falling_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "long_build",
            &outro,
            &intro,
        );
        assert!(rising_score > falling_score);
    }

    #[test]
    fn select_transition_deep_emits_deep_used_when_features_present() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_t = input_track("a");
        let to_t = input_track("b");
        let from_deep = deep_for("a", 0.15, 0.6, 0.3);
        let to_deep = deep_for("b", 0.15, 0.6, 0.3);
        let preset = style_preset("club_blend");

        let trans = select_transition_deep(
            &from_a,
            &from_t,
            Some(&from_deep),
            &to_a,
            &to_t,
            Some(&to_deep),
            "lift",
            "club_blend",
            &preset,
            45.0,
        );
        assert!(trans.deep_used);
        assert!(trans.reason.starts_with("deep:"));
        assert!(trans.from_outro_start_sec >= 200.0);
        assert!(trans.to_intro_start_sec <= 16.0);
    }

    #[test]
    fn select_transition_deep_falls_back_to_legacy_without_deep() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            None,
            &to_a,
            &input_track("b"),
            None,
            "lift",
            "club_blend",
            &preset,
            45.0,
        );
        assert!(!trans.deep_used);
        assert!(!trans.reason.starts_with("deep:"));
    }

    #[test]
    fn safe_mix_produces_safe_crossfade_kind() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.5, 0.8, 0.5);
        let to_deep = deep_for("b", 0.5, 0.8, 0.5);
        let preset_safe = style_preset("safe_mix");
        let preset_club = style_preset("club_blend");

        let safe = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "lift",
            "safe_mix",
            &preset_safe,
            45.0,
        );
        let club = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "lift",
            "club_blend",
            &preset_club,
            45.0,
        );
        assert_eq!(safe.kind, "safe_crossfade");
        assert!(matches!(club.kind.as_str(), "beat_blend" | "blend"));
    }

    fn render_transition(
        from_id: &str,
        to_id: &str,
        crossfade: f64,
        bass_duck: f64,
        filter_sweep: bool,
        echo_tail: f64,
    ) -> MixTransition {
        MixTransition {
            from_track_id: EntityId::Str(from_id.into()),
            to_track_id: EntityId::Str(to_id.into()),
            crossfade_sec: crossfade,
            from_outro_start_sec: 200.0,
            to_intro_start_sec: 4.0,
            phrase_aware: true,
            reason: "test".into(),
            kind: "blend".into(),
            intensity: 0.5,
            eq_duck: 0.2,
            bass_duck,
            filter_sweep,
            echo_tail_sec: echo_tail,
            loop_build_sec: 0.0,
            bpm_adjust_from_pct: 0.0,
            bpm_adjust_to_pct: 0.0,
            deep_used: true,
            drums_rms_incoming: 0.0,
        }
    }

    fn render_plan(style: &str, transition: MixTransition) -> MixPlan {
        MixPlan {
            playlist_id: EntityId::Str("p1".into()),
            default_crossfade_sec: 12.0,
            target_energy_ramp: vec![0.4, 0.5],
            transitions: vec![transition],
            style: style.into(),
            ordered_track_ids: vec![EntityId::Str("a".into()), EntityId::Str("b".into())],
            energy_curve_phases: vec!["groove".into(), "lift".into()],
            per_track_energy: HashMap::new(),
            anthem_track_id: None,
        }
    }

    #[test]
    fn fade_curves_differ_per_style() {
        assert_eq!(fade_curves_for_style("safe_mix"), ("qsin", "qsin"));
        assert_eq!(fade_curves_for_style("club_blend"), ("qsin", "qsin"));
        assert_eq!(fade_curves_for_style("chill_blend"), ("qsin", "qsin"));
        assert_eq!(fade_curves_for_style("long_build"), ("cub", "ihsin"));
    }

    #[test]
    fn build_filter_complex_emits_bass_duck_when_requested() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.5, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = compute_render_timings(&refs, &plan.transitions, &[1.0, 1.0]);
        let filter = build_filter_complex(&plan, &timings, 2);
        assert!(filter.contains("bass=g=-"));
        assert!(filter.contains("enable='between(t,"));
    }

    #[test]
    fn safe_mix_strips_all_effects() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        // Transition asks for bass duck, filter sweep, and echo. Safe mode must strip them.
        let transition = render_transition("a", "b", 12.0, 0.7, true, 0.5);
        let plan = render_plan("safe_mix", transition);
        let timings = compute_render_timings(&refs, &plan.transitions, &[1.0, 1.0]);
        let filter = build_filter_complex(&plan, &timings, 2);
        assert!(!filter.contains("bass=g=-"));
        assert!(!filter.contains("aecho"));
        assert!(!filter.contains("highpass=f=200"));
        assert!(!filter.contains("lowpass=f=8000"));
        // acrossfade uses c1/c2 for curves; safe_mix uses qsin
        assert!(filter.contains("c1=qsin"));
    }

    #[test]
    fn club_blend_uses_linear_curve_and_emits_sweep_when_requested() {
        let analyses = [analysis("a", 124.0), analysis("b", 124.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 16.0, 0.0, true, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = compute_render_timings(&refs, &plan.transitions, &[1.0, 1.0]);
        let filter = build_filter_complex(&plan, &timings, 2);
        // club_blend uses equal-power qsin for acrossfade
        assert!(filter.contains("c1=qsin"));
        // Sweep filter active when filter_sweep=true and not safe-mode.
        assert!(filter.contains("highpass=f=200"));
        assert!(filter.contains("lowpass=f=8000"));
    }

    #[test]
    fn fade_durations_honor_transition_crossfade_length() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 18.0, 0.0, false, 0.0);
        let plan = render_plan("chill_blend", transition);
        let timings = compute_render_timings(&refs, &plan.transitions, &[1.0, 1.0]);
        let filter = build_filter_complex(&plan, &timings, 2);
        // Crossfade is 18s → acrossfade must use exactly 18s with equal-power qsin curves
        assert!(filter.contains("acrossfade=d=18.000:c1=qsin:c2=qsin"));
    }

    #[test]
    fn style_weights_sum_to_unity() {
        for style in ["safe_mix", "club_blend", "chill_blend", "long_build"] {
            let w = style_weights(style);
            let sum = w.vocal
                + w.drum
                + w.bass
                + w.phrase
                + w.bpm
                + w.key
                + w.energy
                + w.section
                + w.confidence;
            assert!(
                (sum - 1.0).abs() < 0.01,
                "style {style} weights must sum to ~1.0 (got {sum})"
            );
        }
    }

    #[test]
    fn style_weight_dominants_match_doc_intent() {
        let safe = style_weights("safe_mix");
        assert!(safe.vocal >= safe.drum, "safe_mix vocal must outrank drum");
        assert!(
            safe.vocal >= safe.energy,
            "safe_mix vocal must outrank energy"
        );

        let club = style_weights("club_blend");
        assert!(
            club.drum >= club.vocal,
            "club_blend drum must outrank vocal"
        );
        assert!(
            club.phrase >= club.energy,
            "club_blend phrase must outrank energy"
        );

        let chill = style_weights("chill_blend");
        assert!(
            chill.energy >= chill.drum,
            "chill_blend energy must outrank drum"
        );
        assert!(
            chill.energy >= chill.phrase,
            "chill_blend energy must outrank phrase"
        );

        let build = style_weights("long_build");
        assert!(
            build.section >= build.bpm,
            "long_build section must outrank bpm"
        );
        assert!(
            build.section >= build.vocal,
            "long_build section must outrank vocal"
        );
    }

    #[test]
    fn chill_blend_rewards_low_drum_continuity() {
        // chill_blend should prefer sparse-drum transitions; club_blend should prefer dense drums.
        let from_a = analysis("a", 110.0);
        let to_a = analysis("b", 110.0);
        let from_deep = deep_for("a", 0.2, 0.5, 0.3);
        let to_deep = deep_for("b", 0.2, 0.5, 0.3);

        let outro = from_deep.transition_windows[1].clone();
        let intro = to_deep.transition_windows[0].clone();

        let dense_outro = TransitionWindow {
            drum_continuity: 0.9,
            ..outro.clone()
        };
        let dense_intro = TransitionWindow {
            drum_continuity: 0.9,
            ..intro.clone()
        };
        let sparse_outro = TransitionWindow {
            drum_continuity: 0.05,
            ..outro
        };
        let sparse_intro = TransitionWindow {
            drum_continuity: 0.05,
            ..intro
        };

        let (chill_sparse, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "chill_blend",
            &sparse_outro,
            &sparse_intro,
        );
        let (chill_dense, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "chill_blend",
            &dense_outro,
            &dense_intro,
        );
        assert!(
            chill_sparse > chill_dense,
            "chill_blend prefers sparse drums ({chill_sparse} vs {chill_dense})"
        );
    }

    #[test]
    fn safe_mix_prefers_low_bpm_delta() {
        let from_a = analysis("a", 120.0);
        let close_to = analysis("b", 121.0);
        let far_to = analysis("c", 138.0);
        let from_deep = deep_for("a", 0.1, 0.5, 0.2);
        let close_deep = deep_for("b", 0.1, 0.5, 0.2);
        let far_deep = deep_for("c", 0.1, 0.5, 0.2);

        let outro = from_deep.transition_windows[1].clone();
        let close_intro = close_deep.transition_windows[0].clone();
        let far_intro = far_deep.transition_windows[0].clone();

        let (close_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &close_to,
            Some(&close_deep),
            "safe_mix",
            &outro,
            &close_intro,
        );
        let (far_score, _, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &far_to,
            Some(&far_deep),
            "safe_mix",
            &outro,
            &far_intro,
        );
        assert!(
            close_score > far_score,
            "safe_mix prefers small bpm delta ({close_score} vs {far_score})"
        );
    }

    #[test]
    fn long_build_prefers_breakdown_to_drop_section() {
        let from_a = analysis("a", 124.0);
        let to_a = analysis("b", 124.0);
        let mut from_deep = deep_for("a", 0.1, 0.6, 0.3);
        let mut to_deep = deep_for("b", 0.1, 0.6, 0.3);

        // section_role iterates in order and stops at the first containing section.
        // Replace the default outro section with a breakdown that fully covers the outro window.
        from_deep.sections = vec![
            TrackSection {
                kind: "intro".into(),
                start: 0.0,
                end: 16.0,
                confidence: 0.7,
                vocal_density: 0.1,
                drum_density: 0.3,
                energy: 0.3,
            },
            TrackSection {
                kind: "breakdown".into(),
                start: 190.0,
                end: 240.0,
                confidence: 0.85,
                vocal_density: 0.1,
                drum_density: 0.2,
                energy: 0.3,
            },
        ];
        to_deep.sections = vec![TrackSection {
            kind: "drop".into(),
            start: 0.0,
            end: 32.0,
            confidence: 0.85,
            vocal_density: 0.1,
            drum_density: 0.9,
            energy: 0.9,
        }];
        from_deep.energy_refined = 0.3;
        to_deep.energy_refined = 0.85;

        let outro = TransitionWindow {
            start: 196.0,
            end: 232.0,
            ..from_deep.transition_windows[1].clone()
        };
        let intro = TransitionWindow {
            start: 4.0,
            end: 32.0,
            ..to_deep.transition_windows[0].clone()
        };

        let (build_score, components, _, _, _) = score_deep_pair(
            &from_a,
            Some(&from_deep),
            &to_a,
            Some(&to_deep),
            "long_build",
            &outro,
            &intro,
        );
        let section_score = components
            .iter()
            .find(|(k, _)| *k == "section")
            .map(|(_, v)| *v)
            .unwrap_or(0.0);
        assert!(
            section_score >= 0.9,
            "long_build breakdown→drop should score ≥0.9 (got {section_score}); total={build_score}"
        );
    }

    #[test]
    fn deterministic_plan_collects_candidate_debug_when_requested() {
        let tracks = vec![input_track("a"), input_track("b"), input_track("c")];
        let analyses = vec![
            analysis("a", 120.0),
            analysis("b", 120.0),
            analysis("c", 122.0),
        ];
        let mut deep_features: HashMap<String, DeepTrackFeatures> = HashMap::new();
        deep_features.insert("a".into(), deep_for("a", 0.2, 0.6, 0.3));
        deep_features.insert("b".into(), deep_for("b", 0.2, 0.6, 0.3));
        deep_features.insert("c".into(), deep_for("c", 0.2, 0.6, 0.3));

        let mut debug = Vec::new();
        let plan = deterministic_plan(
            &EntityId::Str("p1".into()),
            &tracks,
            &analyses,
            &deep_features,
            12.0,
            "club_blend",
            true,
            &mut debug,
        );

        assert_eq!(plan.transitions.len(), 2);
        assert_eq!(debug.len(), 2);
        for step in &debug {
            assert!(!step.candidates.is_empty());
            assert!(step.candidates.iter().filter(|c| c.chosen).count() == 1);
            assert!(step.used_deep);
            assert!(step.candidates.len() <= TRANSITION_DEBUG_TOP_N);
        }
    }

    #[test]
    fn deterministic_plan_skips_debug_when_disabled() {
        let tracks = vec![input_track("a"), input_track("b")];
        let analyses = vec![analysis("a", 120.0), analysis("b", 120.0)];
        let mut deep_features: HashMap<String, DeepTrackFeatures> = HashMap::new();
        deep_features.insert("a".into(), deep_for("a", 0.2, 0.6, 0.3));
        deep_features.insert("b".into(), deep_for("b", 0.2, 0.6, 0.3));

        let mut debug = Vec::new();
        let _plan = deterministic_plan(
            &EntityId::Str("p1".into()),
            &tracks,
            &analyses,
            &deep_features,
            12.0,
            "safe_mix",
            false,
            &mut debug,
        );
        assert!(debug.is_empty());
    }
}

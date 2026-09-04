//! Defines Rust server support logic for Mix Worker.

use crate::{
    beat_grid::{bar_offset, fit_local_grid, fold_bpm_octave, LocalGrid},
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
            tokio::select! {
                _ = state.cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(e) = try_process_next(&state).await {
                        tracing::error!("[mix_worker] tick error: {e}");
                    }
                }
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

    // Playlist name for the rendered output's ID3 tags (title/album) — see
    // render_mix's -metadata args. Falls back to a generic label if somehow
    // missing (e.g. playlist deleted mid-render).
    let playlist_name = {
        let conn = lock_db(&state.db)?;
        conn.query_row(
            "SELECT name FROM playlists WHERE id=?1",
            rusqlite::params![playlist_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "Playlist".to_string())
    };

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

    // ── Adopt the neural tempo for planning, not just for rendering ──────────
    // `build_analysis` can only see the ID3 tag and the local flux detector.
    // On the reference playlist that detector clustered at 118/120 BPM for
    // tracks actually running anywhere from 120 to 133, and read two tracks at
    // half time. The planner ordered and scored tracks on those numbers, which
    // is how a run of neighbours ended up 7-9% apart in tempo; the beat grid is
    // the accurate source and it is already loaded by this point.
    for analysis in &mut analyses {
        let tid = analysis.track_id.to_string();
        let Some(bpm) = beat_grid_bpm(deep_features.get(&tid)) else {
            continue;
        };
        if !(60.0..=220.0).contains(&bpm) {
            continue;
        }
        analysis.bpm_estimate = Some(bpm);
        analysis.beat_grid_sec = Some(60.0 / bpm);
        analysis.confidence = analysis.confidence.max(0.85);
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

    let rendered_date = unix_millis_to_ymd(ts as u64);

    let file_size = render_mix(
        state,
        job_id,
        &ffmpeg,
        &ordered_tracks,
        &ordered_analyses,
        &deep_features,
        &plan,
        &output_path,
        &playlist_name,
        &rendered_date,
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

/// Integrated loudness (LUFS) of one region of a file.
///
/// Whole-file loudness is a poor gain-match target for a DJ mix: the mix only
/// ever plays a trimmed span of each track, and two tracks with identical
/// whole-file loudness routinely differ by several dB over the spans actually
/// used. Measured on a real render, that mismatch left consecutive tracks up to
/// ~5 dB apart, which is exactly what makes a transition audible as a seam.
async fn analyze_region_loudness(
    ffmpeg: &Path,
    file_path: &str,
    start_sec: f64,
    dur_sec: f64,
) -> Option<f64> {
    if !dur_sec.is_finite() || dur_sec < 3.0 {
        return None;
    }
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-ss")
        .arg(format!("{:.3}", start_sec.max(0.0)))
        .arg("-t")
        .arg(format!("{dur_sec:.3}"))
        .arg("-i")
        .arg(file_path)
        .arg("-af")
        .arg("ebur128=peak=none")
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;

    let text = String::from_utf8_lossy(&output.stderr);
    parse_ebur128_integrated(&text)
}

/// Pulls the final `I: <value> LUFS` reading out of an `ebur128` summary block.
fn parse_ebur128_integrated(text: &str) -> Option<f64> {
    let mut last: Option<f64> = None;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("I:") else {
            continue;
        };
        let Some(value) = rest.trim().strip_suffix("LUFS") else {
            continue;
        };
        if let Ok(lufs) = value.trim().parse::<f64>() {
            last = Some(lufs);
        }
    }
    last.filter(|l| l.is_finite() && *l > -70.0)
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

    // Prefer the file's own tag BPM over the app's local flux-autocorrelation
    // detector: the local detector is a lightweight heuristic prone to octave
    // errors (e.g. locking onto half the true tempo for four-on-the-floor
    // dance tracks) and clusters toward a generic value on ambiguous rhythms,
    // while tag BPM is typically curated/verified at encode time.
    let bpm = track
        .bpm
        .filter(|&b| (60.0..=220.0).contains(&b))
        .or_else(|| track.bpm_detected.filter(|&b| (60.0..=220.0).contains(&b)));

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

/// Tempo recovered from the spacing of detected beats, rejecting dropped or
/// doubled beats before averaging the rest.
///
/// `bpm_neural` is a *median* of beat intervals quantised to the beat tracker's
/// 100 fps frame grid, so it lands up to ~1.5% from the true tempo — it reports
/// 127.66 for a track whose ID3 tag and whose own beat spacing both say exactly
/// 127. Averaging the surviving intervals recovers the sub-frame resolution the
/// median throws away, and matches tag BPM exactly on every tagged track in the
/// reference playlist. That precision is what a long blend needs: a 0.5% tempo
/// error drifts half a beat across 45 s, so the drums pull apart well before the
/// crossfade ends even when they start perfectly locked.
fn refined_bpm_from_beats(beats: &[f64]) -> Option<f64> {
    let mut intervals: Vec<f64> = beats
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| d.is_finite() && *d > 0.0)
        .collect();
    if intervals.len() < 8 {
        return None;
    }
    let mut sorted = intervals.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    if median <= 0.0 {
        return None;
    }
    intervals.retain(|d| *d > median * 0.85 && *d < median * 1.15);
    if intervals.len() < 8 {
        return None;
    }
    let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
    let bpm = 60.0 / mean;
    (20.0..=250.0).contains(&bpm).then_some(bpm)
}

/// Tempo from a track's neural beat grid, preferring the refined beat-spacing
/// value over the frame-quantised median the tracker reports.
fn beat_grid_bpm(deep: Option<&boogiebox_db::boogiemix::DeepTrackFeatures>) -> Option<f64> {
    let bg = deep.and_then(|d| d.beat_grid.as_ref())?;
    refined_bpm_from_beats(&bg.beats)
        .or(Some(bg.bpm_neural))
        .filter(|&b| b > 20.0)
}

/// Returns the best available BPM for a track: neural beat grid > analysis estimate > deep bpm_refined.
fn best_bpm(
    a: &TrackMixAnalysis,
    deep: Option<&boogiebox_db::boogiemix::DeepTrackFeatures>,
) -> Option<f64> {
    beat_grid_bpm(deep)
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

    // Mix from the track's musical body when long overlays would otherwise land
    // in a sparse silence-detected tail.
    let mut from_start = outgoing_analysis_end(from_a, crossfade);
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

fn overlap_duration(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    a_end.min(b_end) - a_start.max(b_start)
}

fn has_rhythmic_timing_evidence(deep: Option<&DeepTrackFeatures>) -> bool {
    let Some(deep) = deep else {
        return false;
    };
    if !deep.drum_windows.is_empty() {
        return true;
    }
    if deep
        .transition_windows
        .iter()
        .any(|w| w.drums_rms.is_some() || w.vocals_rms.is_some() || w.bass_rms.is_some())
    {
        return true;
    }
    deep.beat_grid
        .as_ref()
        .map(|grid| grid.beats.len() >= 8)
        .unwrap_or(false)
}

fn drum_score_for_span(deep: Option<&DeepTrackFeatures>, start: f64, end: f64) -> Option<f64> {
    let deep = deep?;
    if end <= start {
        return None;
    }
    let span = end - start;
    deep.drum_windows
        .iter()
        .filter_map(|w| {
            let overlap = overlap_duration(start, end, w.start, w.end);
            if overlap <= 0.0 {
                return None;
            }
            let strength = w.average.max(w.strength).clamp(0.0, 1.0);
            Some((overlap / span).clamp(0.0, 1.0) * strength)
        })
        .fold(None, |best, score| match best {
            Some(current) if current >= score => Some(current),
            _ => Some(score),
        })
}

fn outgoing_crossfade_end(
    deep: Option<&DeepTrackFeatures>,
    window: &TransitionWindow,
    crossfade: f64,
) -> f64 {
    let fallback = window.end.max(crossfade);
    let Some(deep) = deep else {
        return fallback;
    };
    let mut best: Option<(f64, f64)> = None;
    for drum in &deep.drum_windows {
        let overlap = overlap_duration(window.start, window.end, drum.start, drum.end);
        if overlap <= 0.0 {
            continue;
        }
        let end = (drum.end + 2.0).clamp(crossfade, window.end);
        let start = (end - crossfade).max(0.0);
        let score = drum_score_for_span(Some(deep), start, end).unwrap_or(0.0)
            + (end / window.end.max(1.0)) * 0.05;
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((end, score));
        }
    }
    if let Some((end, _)) = best {
        return end;
    }
    // No drums overlap the outro window — the track's energetic body ends before the
    // outro. Find the global last drum hit and trim A just after it so the crossfade
    // region stays in the energetic section rather than a beatless quiet tail.
    let last_drum_end = deep
        .drum_windows
        .iter()
        .filter(|w| w.end < window.end)
        .map(|w| w.end)
        .fold(f64::NEG_INFINITY, f64::max);
    if last_drum_end.is_finite() && last_drum_end > 0.0 {
        // 2-second tail after the last hit gives a clean exit; don't exceed window.end.
        (last_drum_end + 2.0).clamp(crossfade, window.end)
    } else {
        fallback
    }
}

fn outgoing_analysis_end(a: &TrackMixAnalysis, crossfade: f64) -> f64 {
    let latest = (a.duration_sec - 1.0).max(crossfade);
    let earliest = (a.intro_end_sec + crossfade + 4.0)
        .min(latest)
        .max(crossfade);
    let natural = a.outro_start_sec.clamp(earliest, latest);
    if crossfade < 20.0 || a.duration_sec < 240.0 {
        return natural;
    }

    // Long overlay blends need to leave before sparse, beatless DJ outros when
    // stem timing is unavailable. Cap very-late silence-based outros to the body.
    let body_latest = (a.duration_sec * 0.82).clamp(earliest, latest);
    natural.min(body_latest)
}

fn incoming_drum_start(
    deep: Option<&DeepTrackFeatures>,
    window: &TransitionWindow,
    crossfade: f64,
) -> f64 {
    let Some(deep) = deep else {
        return window.start;
    };
    let mut best: Option<(f64, f64)> = None;
    for drum in &deep.drum_windows {
        let overlap = overlap_duration(window.start, window.end, drum.start, drum.end);
        if overlap <= 0.0 {
            continue;
        }
        let start = drum.start.max(window.start);
        let score = drum_score_for_span(Some(deep), start, start + crossfade).unwrap_or(0.0)
            - (start / window.end.max(1.0)) * 0.03;
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((start, score));
        }
    }
    best.map(|(start, _)| start).unwrap_or(window.start)
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

    let from_start = outgoing_crossfade_end(from_deep, from_window, crossfade);
    let to_start = incoming_drum_start(to_deep, to_window, crossfade);

    let phrase_target_from = (from_start - crossfade).max(0.0);
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
    let from_drum = drum_score_for_span(from_deep, (from_start - crossfade).max(0.0), from_start)
        .unwrap_or(from_window.drum_continuity);
    let to_drum = drum_score_for_span(to_deep, to_start, to_start + crossfade)
        .unwrap_or(to_window.drum_continuity);
    let drum_pair = (from_drum + to_drum) / 2.0;
    let drum_score = match style {
        // Beat-centric styles: require BOTH tracks to have drums; a beatless mixout
        // tanks the score via min() so the planner seeks an earlier energetic window.
        "club_blend" | "long_build" => from_drum.min(to_drum).clamp(0.0, 1.0),
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

/// Widens an outro candidate window earlier into the track body when it is
/// narrower than `target`, so `long_build` can reach its configured blend
/// length instead of being capped by whatever "safe" outro span deep analysis
/// happened to detect. Only widens the *start* (the end stays anchored near
/// the track's end); the hold-then-fade render envelope keeps the outgoing
/// track at full volume through most of the window regardless of width, so
/// reaching earlier into the track is low-risk.
fn widen_outro_window_for_long_build(mut w: TransitionWindow, target: f64) -> TransitionWindow {
    if (w.end - w.start) < target {
        w.start = (w.end - target).max(0.0);
        w.recommended_max_crossfade = w.recommended_max_crossfade.max(target);
    }
    w
}

/// Widens an intro candidate window later into the track when narrower than
/// `target`, mirroring `widen_outro_window_for_long_build`. The incoming
/// track's fade-in spans the whole window, so a wider window simply gives it
/// more time to gradually become prominent, per the same reasoning.
fn widen_intro_window_for_long_build(
    mut w: TransitionWindow,
    target: f64,
    track_duration: f64,
) -> TransitionWindow {
    if (w.end - w.start) < target {
        w.end = (w.start + target).min(track_duration.max(w.end));
        w.recommended_max_crossfade = w.recommended_max_crossfade.max(target);
    }
    w
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
    if !has_rhythmic_timing_evidence(from_deep) || !has_rhythmic_timing_evidence(to_deep) {
        return select_transition(from_a, from_t, to_a, to_t, phase, preset, default_crossfade);
    }

    // For beat-centric styles, prefer outro windows that have drum activity.
    // Try with a drum threshold first; if no windows qualify, fall back to all.
    let beat_centric = matches!(style, "club_blend" | "long_build");
    let outro_windows: Vec<&TransitionWindow> = from_deep
        .map(|d| {
            let base: Vec<&TransitionWindow> = d
                .transition_windows
                .iter()
                .filter(|w| {
                    (w.role == "outro" || w.role == "instrumental")
                        && near_end(w.end, from_a.duration_sec)
                        && (w.end - w.start) >= 4.0
                })
                .collect();
            if beat_centric {
                let with_drums: Vec<&TransitionWindow> = base
                    .iter()
                    .copied()
                    .filter(|w| w.drum_continuity > 0.15)
                    .collect();
                if !with_drums.is_empty() {
                    return with_drums;
                }
            }
            base
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

    let outro_iter: Vec<TransitionWindow> = if outro_windows.is_empty() {
        vec![TransitionWindow {
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
        }]
    } else {
        outro_windows.into_iter().cloned().collect()
    };
    let intro_iter: Vec<TransitionWindow> = if intro_windows.is_empty() {
        vec![TransitionWindow {
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
        }]
    } else {
        intro_windows.into_iter().cloned().collect()
    };

    // long_build deliberately wants extended overlaps. The hold-then-fade
    // render envelope keeps the outgoing track at full volume through most of
    // the window regardless of its width, so it's safe to widen windows that
    // deep analysis found too narrow toward the user's configured blend
    // length instead of silently capping the crossfade to whatever "safe"
    // span happened to be detected.
    let (outro_iter, intro_iter) = if style == "long_build" && default_crossfade >= 4.0 {
        (
            outro_iter
                .into_iter()
                .map(|w| widen_outro_window_for_long_build(w, default_crossfade))
                .collect(),
            intro_iter
                .into_iter()
                .map(|w| widen_intro_window_for_long_build(w, default_crossfade, to_a.duration_sec))
                .collect(),
        )
    } else {
        (outro_iter, intro_iter)
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

    let from_start = if (crossfade - pair.crossfade).abs() > 0.01 {
        outgoing_crossfade_end(from_deep, &pair.from_window, crossfade)
    } else {
        pair.from_start
    };
    let from_start = if let Some(deep) = from_deep {
        let beat = from_a.beat_grid_sec.unwrap_or(0.5).max(0.25);
        nearest_phrase_boundary(
            &deep.phrase_boundaries,
            (from_start - crossfade).max(0.0),
            beat * 2.0,
        )
        .map(|phrase_start| phrase_start + crossfade)
        .unwrap_or(from_start)
    } else {
        from_start
    };
    let to_start = if (crossfade - pair.crossfade).abs() > 0.01 {
        incoming_drum_start(to_deep, &pair.to_window, crossfade)
    } else {
        pair.to_start
    };
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
                // Scored against what the renderer can actually close (about
                // 6%, +-3% from each side) rather than a flat 30 BPM ramp: on a
                // 125 BPM track the old scale barely distinguished a 1 BPM
                // neighbour from a 7 BPM one, and 7 BPM is the difference
                // between a locked blend and an unmatched one.
                let bpm_gap = |i: usize| {
                    prev_a.and_then(|p| p.bpm_estimate).and_then(|pb| {
                        analyses[i].bpm_estimate.map(|b| {
                            let folded = fold_bpm_octave(pb, b);
                            ((folded - pb).abs() / pb.max(1.0) / (2.0 * TEMPO_MATCH_LIMIT)).min(1.0)
                        })
                    })
                };
                let bpm_a = bpm_gap(a).unwrap_or(0.5);
                let bpm_b = bpm_gap(b).unwrap_or(0.5);
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
                    energy_a * 0.34 + bpm_a * 0.42 + harm_a * 0.14 - cosine_bonus_a * 0.10;
                let score_b =
                    energy_b * 0.34 + bpm_b * 0.42 + harm_b * 0.14 - cosine_bonus_b * 0.10;
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

/// Largest pitch-preserving tempo change applied to any audio that actually
/// sounds, as a fraction (0.03 = ±3%). This is classic DJ pitch-fader
/// territory: a 125 BPM track plays between 121.3 and 128.8 and nobody hears a
/// tempo change.
///
/// It bounds *every* factor the renderer emits, so a pair of neighbours can be
/// closed from both sides — the outgoing track pushes up to +3% and the
/// incoming pulls down to -3% — giving a reachable gap of about 6%. Wider gaps
/// are left unmatched rather than stretched into sounding wrong.
const TEMPO_MATCH_LIMIT: f64 = 0.03;

/// Fraction of a crossfade window during which the outgoing track holds at its
/// regular volume before it starts fading out (e.g. 0.7 = the outgoing track
/// stays full for the first 70% of the window, then fades over the remaining
/// 30%). The incoming track fades in gradually across the *entire* window.
/// This mirrors how a DJ actually blends two tracks: the new track is slowly
/// layered in underneath the still-playing track, and only once the new track
/// has become prominent does the old one get pulled out. A single symmetric
/// crossfade (both tracks fading for the whole window) reads as the outgoing
/// track visibly fading from the first instant, which sounds abrupt even when
/// the combined loudness is kept constant. Tweak this to hold longer/shorter.
const OUTGOING_HOLD_FRACTION: f64 = 0.7;

/// Largest tempo change taken in a single step of a mid-track tempo move.
/// 1.2% is below the threshold where a step reads as a tempo change rather
/// than as the track simply continuing.
const MAX_SINGLE_TEMPO_STEP: f64 = 0.012;

/// Cap on the intermediate steps in one mid-track tempo move. Every extra step
/// is another `concat` boundary, and each boundary costs a little timing
/// accuracy (see `CONCAT_BOUNDARY_SEC`), so the move stays coarse on purpose.
const MAX_TEMPO_STEPS: usize = 4;

/// Seconds of source audio each intermediate tempo step lasts.
const TEMPO_STEP_SEC: f64 = 4.0;

/// Seconds of margin kept between a blend and a mid-track tempo move, so the
/// tempo is provably constant for the whole of every overlap.
const TEMPO_MOVE_GUARD_SEC: f64 = 4.0;

/// Extra audio ffmpeg emits at each `concat` boundary between independently
/// `atempo`-ed segments, measured at ~1.7 ms per boundary and stable from 2 to
/// 20 segments. Small, but it lands directly on the blend that follows, so the
/// timeline model accounts for it rather than letting it drift the alignment.
const CONCAT_BOUNDARY_SEC: f64 = 0.0017;

/// Seconds either side of a mix point used to fit that point's beat grid.
/// Wide enough for ~60 beats of evidence, narrow enough that a slipped section
/// elsewhere in the track cannot pull the fit off the music.
const GRID_FIT_HALF_WINDOW_SEC: f64 = 30.0;

/// Sample rate every track is resampled to before `atempo`.
///
/// `atempo` swallows a fixed prefix of its input, and the size of that prefix
/// is set by the frame size, so tracks arriving at different sample rates come
/// out shifted by different amounts. Pinning the rate — and emitting `atempo`
/// on *every* track, even at factor 1.0 — makes the offset identical
/// everywhere, where before a track that happened to need no tempo change sat
/// ~21 ms away from its neighbours.
const RENDER_SAMPLE_RATE: u32 = 48000;

/// Residual effective-BPM error below which a blend is treated as beat-locked
/// for its whole length (0.002 = 0.2%).
const TEMPO_DRIFT_TOLERANCE: f64 = 0.002;

/// Piecewise-constant tempo map for one rendered track stream.
///
/// A track is beat-matched to a different neighbour at each end, so its tempo
/// generally has to change somewhere in the middle. Each segment is rendered as
/// its own `atrim`+`atempo` chain and the chain is `concat`ed back together, so
/// the mapping from source time to output time is exactly this arithmetic.
#[derive(Debug, Clone, PartialEq)]
struct TempoMap {
    /// `(source-seconds, tempo factor)` in playback order.
    segments: Vec<(f64, f64)>,
}

impl TempoMap {
    fn constant(src_dur: f64, factor: f64) -> Self {
        TempoMap {
            segments: vec![(src_dur.max(0.0), factor.max(1e-6))],
        }
    }

    fn is_constant(&self) -> bool {
        self.segments.len() <= 1
    }

    /// Tempo factor applied to the very start of the stream.
    fn head_factor(&self) -> f64 {
        self.segments.first().map(|s| s.1).unwrap_or(1.0)
    }

    /// Tempo factor applied to the very end of the stream — the one in force
    /// through the outgoing blend.
    fn tail_factor(&self) -> f64 {
        self.segments.last().map(|s| s.1).unwrap_or(1.0)
    }

    fn src_duration(&self) -> f64 {
        self.segments.iter().map(|(d, _)| *d).sum()
    }

    fn out_duration(&self) -> f64 {
        self.to_output(self.src_duration())
    }

    /// Maps an offset in source seconds (from the trim-in point) to the
    /// corresponding offset in the rendered stream.
    fn to_output(&self, src_offset: f64) -> f64 {
        let mut remaining = src_offset.max(0.0);
        let mut out = 0.0;
        for (idx, (dur, factor)) in self.segments.iter().enumerate() {
            if idx > 0 {
                out += CONCAT_BOUNDARY_SEC;
            }
            if remaining <= *dur {
                return out + remaining / factor.max(1e-6);
            }
            out += dur / factor.max(1e-6);
            remaining -= dur;
        }
        out + remaining / self.tail_factor().max(1e-6)
    }
}

/// Everything the renderer knows about one track's rhythm, fitted at the two
/// points that matter: where the track is mixed in and where it is mixed out.
///
/// The raw neural beat list is never used directly. It carries half-beat slips
/// and dropped beats that would put a blend a full half beat out, so each end
/// of the track gets its own locally-fitted uniform grid instead.
#[derive(Debug, Clone, Default)]
struct TrackRhythm {
    /// Grid fitted around the track's mix-in point.
    in_grid: Option<LocalGrid>,
    /// Grid fitted around the track's mix-out point.
    out_grid: Option<LocalGrid>,
    /// Bar phase at the mix-in point, when real downbeats back it.
    in_bar_offset: Option<i64>,
    /// Bar phase at the mix-out point, when real downbeats back it.
    out_bar_offset: Option<i64>,
    /// Whole-track tempo, used only when no local fit succeeds.
    fallback_bpm: Option<f64>,
}

impl TrackRhythm {
    fn in_bpm(&self) -> Option<f64> {
        self.in_grid.map(|g| g.bpm()).or(self.fallback_bpm)
    }

    fn out_bpm(&self) -> Option<f64> {
        self.out_grid.map(|g| g.bpm()).or(self.fallback_bpm)
    }
}

/// Fits a track's mix-in and mix-out grids.
///
/// `mix_in_sec`/`mix_out_sec` are the planner's entry and exit points in the
/// track's own seconds; the fit is centred on each so the grid is right exactly
/// where the two tracks have to line up.
fn build_track_rhythm(
    deep: Option<&DeepTrackFeatures>,
    fallback_bpm: Option<f64>,
    mix_in_sec: f64,
    mix_out_sec: f64,
) -> TrackRhythm {
    let Some(bg) = deep.and_then(|d| d.beat_grid.as_ref()) else {
        return TrackRhythm {
            fallback_bpm,
            ..Default::default()
        };
    };
    let real_downbeats = bg.downbeats_real.then_some(bg.downbeats.as_slice());
    let fit = |center: f64| {
        let grid = fit_local_grid(&bg.beats, center, GRID_FIT_HALF_WINDOW_SEC)?;
        let bars =
            real_downbeats.and_then(|d| bar_offset(&grid, d, center, GRID_FIT_HALF_WINDOW_SEC));
        Some((grid, bars))
    };
    let mix_in = fit(mix_in_sec);
    let mix_out = fit(mix_out_sec);
    TrackRhythm {
        in_grid: mix_in.map(|(g, _)| g),
        out_grid: mix_out.map(|(g, _)| g),
        in_bar_offset: mix_in.and_then(|(_, b)| b),
        out_bar_offset: mix_out.and_then(|(_, b)| b),
        fallback_bpm,
    }
}

/// The stretch of source audio each track contributes, before any tempo change.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceSpan {
    start: f64,
    dur: f64,
}

/// Picks each track's trim-in and trim-out points and snaps the trim-in onto a
/// real beat (a real bar one, when downbeat evidence allows) so the incoming
/// stream never opens mid-beat.
fn compute_source_spans(
    analyses: &[&TrackMixAnalysis],
    transitions: &[MixTransition],
    rhythms: &[TrackRhythm],
) -> Vec<SourceSpan> {
    let n = analyses.len();
    (0..n)
        .map(|i| {
            let a = analyses[i];
            let incoming = if i > 0 { transitions.get(i - 1) } else { None };
            let outgoing = transitions.get(i);

            let raw_start = incoming.map(|t| t.to_intro_start_sec).unwrap_or(0.0);
            let start = match (incoming, rhythms.get(i).and_then(|r| r.in_grid)) {
                (Some(_), Some(grid)) => {
                    let snapped = match rhythms[i].in_bar_offset {
                        // With real downbeats the entry lands on bar one.
                        Some(bars) => {
                            let bar_len = grid.bar_sec();
                            let base = grid.anchor + bars as f64 * grid.period;
                            base + ((raw_start - base) / bar_len).round() * bar_len
                        }
                        None => grid.nearest_beat(raw_start),
                    };
                    if snapped >= 0.0 && (snapped - raw_start).abs() <= grid.bar_sec() {
                        snapped
                    } else {
                        raw_start
                    }
                }
                _ => raw_start,
            };

            // Trim track A to end at from_outro_start_sec so the crossfade region
            // covers the energetic body (just before the quiet outro), not the
            // silent tail. The quiet outro is discarded — a DJ never plays it.
            let end = outgoing
                .map(|t| t.from_outro_start_sec.min(a.duration_sec))
                .unwrap_or(a.duration_sec);
            let loop_build = outgoing.map(|t| t.loop_build_sec).unwrap_or(0.0);
            SourceSpan {
                start,
                dur: (end - start + loop_build).max(8.0),
            }
        })
        .collect()
}

/// Tempo factors for one track: what it plays at through its incoming blend and
/// what it plays at through its outgoing blend.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TrackTempo {
    head: f64,
    tail: f64,
}

/// Chooses the tempo every blend runs at, and from that each track's head and
/// tail tempo factors.
///
/// Unlike a chained match, this is decided per *boundary* and nothing
/// propagates: transition `i` only has to satisfy the two tracks it joins, so
/// the mix's tempo is free to travel across a set (120 to 134 BPM in the
/// reference playlist) while no individual track is ever bent more than
/// `TEMPO_MATCH_LIMIT` from its own tempo. Chaining every track to the first
/// one, which is what a single constant factor per track mathematically forces,
/// pinned that same playlist's 134 BPM closer at 8.7% slow.
///
/// A boundary whose two tempos cannot meet inside the limit from both sides is
/// reported unmatched: the renderer then blends it without claiming a beat lock
/// rather than stretching a track until it sounds wrong.
fn plan_blend_tempos(
    in_bpms: &[Option<f64>],
    out_bpms: &[Option<f64>],
) -> (Vec<TrackTempo>, Vec<bool>) {
    let n = in_bpms.len();
    let mut tempos = vec![
        TrackTempo {
            head: 1.0,
            tail: 1.0
        };
        n
    ];
    let mut matched = vec![false; n.saturating_sub(1)];
    if n == 0 {
        return (tempos, matched);
    }

    for i in 0..n.saturating_sub(1) {
        let out_bpm = out_bpms[i].filter(|b| *b > 0.0);
        let in_bpm = in_bpms[i + 1].filter(|b| *b > 0.0);
        let (Some(a), Some(raw_b)) = (out_bpm, in_bpm) else {
            tempos[i].tail = tempos[i].head;
            tempos[i + 1].head = 1.0;
            continue;
        };
        let b = fold_bpm_octave(a, raw_b);
        let lo = (1.0 - TEMPO_MATCH_LIMIT) * a.max(b);
        let hi = (1.0 + TEMPO_MATCH_LIMIT) * a.min(b);
        if lo > hi {
            tempos[i].tail = tempos[i].head;
            tempos[i + 1].head = 1.0;
            continue;
        }
        // Meet in the middle so neither track carries the whole adjustment.
        let blend_bpm = (a * b).sqrt().clamp(lo, hi);
        tempos[i].tail = blend_bpm / a;
        tempos[i + 1].head = blend_bpm / b;
        matched[i] = true;
    }
    if let Some(last) = tempos.last_mut() {
        last.tail = last.head;
    }
    (tempos, matched)
}

/// Builds a track's tempo map from its head and tail factors, placing any
/// mid-track tempo move clear of both blends.
///
/// The move is stepped rather than continuous. ffmpeg's runtime `atempo`
/// commands were measured accumulating 780 ms of timing error over a 30 s ramp,
/// which would put the following blend badly out; stepped `concat` segments
/// cost ~1.7 ms per boundary and nothing else, and at ≤1.2% per step the move
/// is inaudible anyway.
fn build_tempo_map(
    span: SourceSpan,
    tempo: TrackTempo,
    head_guard_sec: f64,
    tail_guard_sec: f64,
) -> TempoMap {
    let change = tempo.tail / tempo.head.max(1e-6) - 1.0;
    if !change.is_finite() || change.abs() < 1e-6 {
        return TempoMap::constant(span.dur, tempo.head);
    }

    let steps = ((change.abs() / MAX_SINGLE_TEMPO_STEP).ceil() as usize)
        .saturating_sub(1)
        .min(MAX_TEMPO_STEPS);
    let head_guard = head_guard_sec.max(0.0);
    let tail_guard = tail_guard_sec.max(0.0);
    let move_dur = steps as f64 * TEMPO_STEP_SEC;
    if head_guard + move_dur + tail_guard >= span.dur {
        // No room between the two blends to change tempo at all: keep the
        // incoming blend locked and let the caller drop the outgoing lock.
        return TempoMap::constant(span.dur, tempo.head);
    }

    let mut segments = Vec::with_capacity(steps + 2);
    segments.push((head_guard, tempo.head));
    for k in 0..steps {
        let t = (k + 1) as f64 / (steps + 1) as f64;
        segments.push((TEMPO_STEP_SEC, tempo.head + (tempo.tail - tempo.head) * t));
    }
    segments.push((span.dur - head_guard - move_dur, tempo.tail));
    TempoMap { segments }
}

#[derive(Debug, Clone)]
struct RenderTiming {
    start: f64,
    trim_dur: f64,
    tempo: TempoMap,
    effective_dur: f64,
    gain_db: f64,
}

/// Resolves every track's source span, tempo map and fade lengths.
///
/// Returns the timings alongside a per-transition flag saying whether that
/// boundary really is beat-matched — a boundary can lose its match here when a
/// track turns out to have no room to change tempo between its two blends.
fn compute_render_timings(
    analyses: &[&TrackMixAnalysis],
    transitions: &[MixTransition],
    rhythms: &[TrackRhythm],
    tempos: &[TrackTempo],
    matched: &[bool],
) -> (Vec<RenderTiming>, Vec<bool>) {
    let n = analyses.len();
    let spans = compute_source_spans(analyses, transitions, rhythms);
    let mut matched = matched.to_vec();
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
        let incoming = if i > 0 { transitions.get(i - 1) } else { None };
        let outgoing = transitions.get(i);
        let tempo = tempos[i];
        let head_guard = incoming
            .map(|t| t.crossfade_sec * tempo.head + TEMPO_MOVE_GUARD_SEC)
            .unwrap_or(TEMPO_MOVE_GUARD_SEC);
        let tail_guard = outgoing
            .map(|t| t.crossfade_sec * tempo.tail + TEMPO_MOVE_GUARD_SEC)
            .unwrap_or(TEMPO_MOVE_GUARD_SEC);
        let map = build_tempo_map(spans[i], tempo, head_guard, tail_guard);

        // `build_tempo_map` falls back to a constant head tempo when the track
        // is too short to change tempo clear of both blends. When that happens
        // the outgoing blend is no longer tempo-matched, so say so instead of
        // phase-locking against a tempo the track is not actually playing.
        if map.is_constant() && (tempo.tail - tempo.head).abs() > 1e-6 {
            if let Some(flag) = matched.get_mut(i) {
                *flag = false;
            }
        }

        let effective_dur = map.out_duration();
        let loudness = analyses[i].loudness_lufs.unwrap_or(-14.0);
        // Allow up to -12 dB reduction for very hot modern masters (e.g. -8 LUFS),
        // but cap gain boost at +6 dB to avoid clipping quiet tracks.
        //
        // Deliberately no transition ducking here: `eq_duck` used to be folded
        // into this static gain, which applied up to 2.4 dB across a track's
        // *entire* length and only to tracks that have an outgoing transition —
        // a per-track level offset that made consecutive tracks sit at audibly
        // different volumes. Transition-local shaping lives in the filter graph.
        let base_gain = (-14.0 - loudness).clamp(-12.0, 6.0);
        out.push(RenderTiming {
            start: spans[i].start,
            trim_dur: spans[i].dur,
            tempo: map,
            effective_dur,
            gain_db: base_gain,
        });
    }

    (out, matched)
}

/// Longest overlap over which two not-quite-matched tempos stay usably in sync.
///
/// Any residual effective-BPM error makes the two beat grids drift apart over
/// the blend. Once that drift passes roughly half a beat the drums are audibly
/// flamming, so a long overlay of imperfectly matched tempos is worse than a
/// short one: cap the crossfade at the point where drift is still under half a
/// beat instead of holding a 45 s blend that falls apart halfway through.
fn max_cross_for_tempo_drift(out_bpm: Option<f64>, in_bpm: Option<f64>) -> f64 {
    let (Some(out_bpm), Some(in_bpm)) = (out_bpm, in_bpm) else {
        return f64::INFINITY;
    };
    if !(out_bpm > 0.0 && in_bpm > 0.0) {
        return f64::INFINITY;
    }
    let rel_err = ((in_bpm - out_bpm) / out_bpm).abs();
    if rel_err <= TEMPO_DRIFT_TOLERANCE {
        return f64::INFINITY;
    }
    let beat_sec = 60.0 / out_bpm;
    (0.5 * beat_sec / rel_err).max(6.0)
}

/// Effective tempo each track plays at through its incoming and outgoing blend.
fn blend_bpms(
    rhythms: &[TrackRhythm],
    timings: &[RenderTiming],
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let head = rhythms
        .iter()
        .zip(timings.iter())
        .map(|(r, t)| r.in_bpm().map(|b| b * t.tempo.head_factor()))
        .collect();
    let tail = rhythms
        .iter()
        .zip(timings.iter())
        .map(|(r, t)| r.out_bpm().map(|b| b * t.tempo.tail_factor()))
        .collect();
    (head, tail)
}

/// Crossfade duration actually usable for transition `i` (between track `i`
/// and `i+1`), clamped to both tracks' available (post-tempo) duration and to
/// what the residual tempo mismatch can hold in sync.
fn resolve_cross_durs(
    plan: &MixPlan,
    timings: &[RenderTiming],
    n: usize,
    rhythms: &[TrackRhythm],
) -> Vec<f64> {
    // Compare the tempos the two tracks *play at*, not their native ones: a
    // matched boundary has no residual to cap, and passing native tempos here
    // would shorten exactly the blends that were successfully matched.
    let (in_bpms, out_bpms) = blend_bpms(rhythms, timings);
    (0..n.saturating_sub(1))
        .map(|i| {
            let max_a = timings.get(i).map(|t| t.effective_dur).unwrap_or(59.0);
            let max_b = timings.get(i + 1).map(|t| t.effective_dur).unwrap_or(59.0);
            let drift_cap = max_cross_for_tempo_drift(
                out_bpms.get(i).copied().flatten(),
                in_bpms.get(i + 1).copied().flatten(),
            );
            plan.transitions
                .get(i)
                .map(|t| t.crossfade_sec)
                .unwrap_or(8.0)
                .clamp(2.0, 120.0)
                .min(max_a)
                .min(max_b)
                .min(drift_cap)
        })
        .collect()
}

/// Output-timeline placement for every track, with each overlap locked to a
/// shared beat.
struct AlignedTimeline {
    output_starts: Vec<f64>,
    cross_durs: Vec<f64>,
}

/// Places each track on the output timeline so its first beat lands on a beat
/// of the track it is mixing into — on a *bar one* of it when both tracks carry
/// real downbeat evidence.
///
/// Matching tempo is not enough for the drums to sit on top of each other. The
/// overlap otherwise starts at an arbitrary offset, so the two grids meet at a
/// random phase and stay there: measured on a real 45 s blend whose tempos were
/// matched to 125.00 vs 125.00 BPM, the incoming downbeats sat a persistent
/// ~870 ms — nearly half a bar — away from the outgoing ones.
fn resolve_aligned_timeline(
    timings: &[RenderTiming],
    cross_durs: &[f64],
    rhythms: &[TrackRhythm],
    matched: &[bool],
) -> AlignedTimeline {
    let n = timings.len();
    let mut output_starts = vec![0.0f64; n];
    let mut crosses = cross_durs.to_vec();

    for i in 1..n {
        let prev = i - 1;
        let Some(&nominal_cross) = crosses.get(prev) else {
            continue;
        };
        let lock = matched.get(prev).copied().unwrap_or(false);
        let out_grid = rhythms[prev].out_grid;
        let in_grid = rhythms[i].in_grid;
        let out_factor = timings[prev].tempo.tail_factor().max(1e-6);

        // A bar is only a meaningful unit when both ends know where bar one is;
        // otherwise the quantum is a beat, which still lines the kicks up.
        let bar_locked =
            rhythms[prev].out_bar_offset.is_some() && rhythms[i].in_bar_offset.is_some();
        let quantum = match (lock, out_grid) {
            (true, Some(g)) => {
                let beats = if bar_locked { 4.0 } else { 1.0 };
                g.period * beats / out_factor
            }
            _ => 0.0,
        };

        // Quantise the blend so it also *ends* on a grid edge.
        let mut cross = nominal_cross;
        if quantum > 0.25 {
            let units = (cross / quantum).round().max(1.0);
            cross = (units * quantum)
                .min(timings[prev].effective_dur)
                .min(timings[i].effective_dur);
        }

        let nominal_start = output_starts[prev] + timings[prev].effective_dur - cross;
        let shift = match (lock, out_grid, in_grid) {
            (true, Some(og), Some(ig)) if quantum > 0.25 => blend_phase_shift(
                nominal_start,
                output_starts[prev],
                &timings[prev],
                &og,
                &timings[i],
                &ig,
                quantum,
            ),
            _ => 0.0,
        };

        output_starts[i] = (nominal_start + shift).max(output_starts[prev] + 1.0);
        crosses[prev] =
            (output_starts[prev] + timings[prev].effective_dur - output_starts[i]).max(0.0);
    }

    AlignedTimeline {
        output_starts,
        cross_durs: crosses,
    }
}

/// Sub-quantum nudge that makes the incoming track's first grid beat coincide
/// with an outgoing-track grid beat at the start of the overlap. Returns a value
/// in `[-quantum/2, +quantum/2]`.
#[allow(clippy::too_many_arguments)]
fn blend_phase_shift(
    nominal_start: f64,
    out_track_start: f64,
    out_timing: &RenderTiming,
    out_grid: &LocalGrid,
    in_timing: &RenderTiming,
    in_grid: &LocalGrid,
    quantum: f64,
) -> f64 {
    // Where the incoming stream's first grid beat lands on the output timeline.
    let in_first = in_grid.beat_at_or_after(in_timing.start);
    let in_offset = in_timing.tempo.to_output(in_first - in_timing.start);
    let incoming_beat = nominal_start + in_offset;

    // Any outgoing grid beat near the mix point works as the anchor; the modular
    // wrap below covers the rest of the grid. Both blends run at a constant
    // tempo by construction, so mapping through the tail factor is exact here.
    let out_factor = out_timing.tempo.tail_factor().max(1e-6);
    let mix_point_local = out_timing.start + out_timing.trim_dur
        - (out_timing.effective_dur - nominal_start + out_track_start) * out_factor;
    let anchor_local = out_grid.nearest_beat(mix_point_local);
    let anchor = out_track_start + out_timing.tempo.to_output(anchor_local - out_timing.start);

    let delta = anchor - incoming_beat;
    let shift = delta - quantum * (delta / quantum).round();
    if shift.is_finite() {
        shift
    } else {
        0.0
    }
}

/// Renders one track's `atrim`/`atempo` chain, splitting it into `concat`ed
/// segments when the track changes tempo mid-way.
fn tempo_chain_parts(index: usize, timing: &RenderTiming, label: &str) -> Vec<String> {
    let segments = &timing.tempo.segments;
    let single = segments.len() <= 1;
    let mut parts = Vec::with_capacity(segments.len() + 1);
    let mut labels = Vec::with_capacity(segments.len());
    let mut src_offset = 0.0;
    for (seg, (dur, factor)) in segments.iter().enumerate() {
        let out_label = if single {
            label.to_string()
        } else {
            format!("{label}s{seg}")
        };
        parts.push(format!(
            "[{index}:a]atrim=start={:.4}:duration={:.4},asetpts=PTS-STARTPTS,             aresample={RENDER_SAMPLE_RATE},atempo={:.6}[{out_label}]",
            timing.start + src_offset,
            dur,
            factor.clamp(0.5, 2.0)
        ));
        labels.push(format!("[{out_label}]"));
        src_offset += dur;
    }
    if !single {
        let count = labels.len();
        parts.push(format!(
            "{}concat=n={count}:v=0:a=1[{label}]",
            labels.join("")
        ));
    }
    parts
}

fn build_filter_complex(
    plan: &MixPlan,
    timings: &[RenderTiming],
    n: usize,
    rhythms: &[TrackRhythm],
    matched: &[bool],
) -> String {
    let safe_mode = plan.style == "safe_mix";
    let transitions = &plan.transitions;
    let nominal_durs = resolve_cross_durs(plan, timings, n, rhythms);
    let aligned = resolve_aligned_timeline(timings, &nominal_durs, rhythms, matched);
    let cross_durs = aligned.cross_durs;
    let output_starts = aligned.output_starts;

    let mut parts: Vec<String> = Vec::with_capacity(n * 3);

    // ── Per-track pre-processing ──────────────────────────────────────────────
    for (i, t) in timings.iter().enumerate() {
        let outgoing = transitions.get(i);
        let is_first = i == 0;
        let is_last = i == n - 1;
        let crossfade_dur_in = if i > 0 { cross_durs[i - 1] } else { 0.0 };
        let crossfade_dur_out = if !is_last { cross_durs[i] } else { 0.0 };

        // Pitch-preserving tempo, emitted for every track even at factor 1.0 so
        // `atempo`'s fixed input-prefix cost is identical across the mix.
        let src = format!("p{i}");
        parts.extend(tempo_chain_parts(i, t, &src));

        // Incoming: the new track fades in gradually across the *entire*
        // crossfade window while the outgoing track is still at full/regular
        // volume (see OUTGOING_HOLD_FRACTION below), so it is "layered in"
        // underneath rather than racing to full volume alongside a fade-out.
        let fade_in = if is_first {
            ",afade=t=in:st=0:d=2:curve=tri".to_string()
        } else if crossfade_dur_in > 0.0 {
            format!(",afade=t=in:st=0:d={crossfade_dur_in:.3}:curve=qsin")
        } else {
            String::new()
        };

        // Outgoing: hold at full/regular volume for OUTGOING_HOLD_FRACTION of
        // the crossfade window, then fade out over only the remaining tail, so
        // the previous track doesn't visibly start fading until the incoming
        // track has become prominent.
        let fade_tail_dur = if !is_last && crossfade_dur_out > 0.0 {
            (crossfade_dur_out * (1.0 - OUTGOING_HOLD_FRACTION)).max(0.5)
        } else {
            0.0
        };
        let hold_end = (t.effective_dur - fade_tail_dur).max(0.0);
        let fade_out = if is_last {
            let d = 4.0_f64.min(t.effective_dur);
            let st = (t.effective_dur - d).max(0.0);
            format!(",afade=t=out:st={st:.3}:d={d:.3}:curve=tri")
        } else if fade_tail_dur > 0.0 {
            format!(",afade=t=out:st={hold_end:.3}:d={fade_tail_dur:.3}:curve=qsin")
        } else {
            String::new()
        };

        // ── Bass swap ────────────────────────────────────────────────────────
        // Two full-range tracks stacked for 45 s pile their low ends on top of
        // each other. A DJ answers that by holding the incoming track's bass
        // down through the blend and swapping the low end over in one move at
        // the handover. Both halves of that swap are scheduled at the same
        // output instant, so the two spectral steps cancel and the total low end
        // stays continuous — unlike a one-sided duck, whose step is exposed.
        //
        // Everything else on this chain is now continuous by construction: the
        // previous timeline-gated `highpass=200`/`lowpass=8000` sweep switched a
        // filter on and off mid-music, which reads as a sudden "the sound
        // changed" cue right at the transition and was a direct cause of the
        // handover being easy to notice.
        let bass_swap_db = outgoing.map(|o| o.bass_duck).unwrap_or(0.0);
        let bass_swap_out = if !is_last && !safe_mode && bass_swap_db > 0.05 {
            let g = (bass_swap_db * 10.0).clamp(2.0, 10.0);
            // Outgoing gives its low end up from the handover onwards.
            format!(",bass=g=-{g:.2}:f=180:enable='gte(t,{hold_end:.3})'", g = g)
        } else {
            String::new()
        };
        let bass_swap_in = if !is_first && !safe_mode {
            let incoming_swap = transitions.get(i - 1).map(|tr| tr.bass_duck).unwrap_or(0.0);
            if incoming_swap > 0.05 && crossfade_dur_in > 0.0 {
                let g = (incoming_swap * 10.0).clamp(2.0, 10.0);
                // Incoming holds its low end back until the same handover point.
                let swap_at = crossfade_dur_in * OUTGOING_HOLD_FRACTION;
                format!(",bass=g=-{g:.2}:f=180:enable='lt(t,{swap_at:.3})'")
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // `echo_tail_sec` is still planned and persisted, but no longer rendered.
        // ffmpeg's `aecho` has no timeline support, so it could not be scoped to
        // the handover and instead coloured each outgoing track end to end. That
        // did two measurable kinds of damage on a real render: `aecho=0.8:0.7`
        // attenuates by 4.5 dB (in_gain x out_gain), so the five tracks carrying
        // it played a full 4.5 dB quieter than the rest for their whole length —
        // the single largest source of track-to-track loudness steps — and a
        // 120 ms slapback across an entire track smears exactly the drum
        // transients the beat-locking above exists to line up.

        let pad = match outgoing.map(|o| o.loop_build_sec) {
            Some(p) if p > 0.0 => format!(",apad=pad_dur={:.3}", p.min(3.0)),
            _ => String::new(),
        };

        // Two-phase drum entry: when the incoming track has strong drums, hold it
        // slightly back early in the blend so the outgoing track's outro breathes
        // before the full overlay. Expressed as a continuous ramp rather than the
        // previous step change in level plus a switched 300 Hz highpass, both of
        // which snapped back to normal in one sample at the phase boundary.
        let drum_intro = if !is_first && !safe_mode {
            let drums_rms = transitions
                .get(i - 1)
                .map(|t| t.drums_rms_incoming)
                .unwrap_or(0.0);
            if drums_rms > 0.25 && crossfade_dur_in > 0.0 {
                let phase_a_end = (crossfade_dur_in * 0.40).min(8.0);
                format!(
                    ",volume=volume='if(lt(t,{phase_a_end:.3}),0.75+0.25*t/{phase_a_end:.3},1)':eval=frame"
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        parts.push(format!(
            "[{src}]volume={:.3}dB{fade_in}{fade_out}{bass_swap_out}{bass_swap_in}\
             {drum_intro}{pad}[t{i}]",
            t.gain_db
        ));
    }

    // ── Combine by output-timeline position instead of acrossfade ─────────────
    // ffmpeg's acrossfade forces a single symmetric curve pair spanning the
    // whole window on both streams, which can't express a hold-then-fade
    // handover. Each track above already carries its own gain envelope (full
    // volume, gradual fade-in, or hold-then-fade-out), so tracks only need to
    // be delayed into their output-timeline position and summed.
    if n == 1 {
        parts.push(
            "[t0]alimiter=limit=0.95:level=0:attack=8:release=180,aresample=44100[mixout]"
                .to_string(),
        );
    } else {
        let mut mix_inputs = String::from("[t0]");
        for (i, &output_start) in output_starts.iter().enumerate().skip(1) {
            // Sample-resolution placement: `adelay`'s default millisecond
            // rounding is up to 0.5 ms of avoidable phase error on a join whose
            // whole point is that the drums coincide.
            let delay_samples = (output_start * RENDER_SAMPLE_RATE as f64).round().max(0.0) as i64;
            parts.push(format!("[t{i}]adelay={delay_samples}S:all=1[d{i}]"));
            mix_inputs.push_str(&format!("[d{i}]"));
        }
        parts.push(format!(
            "{mix_inputs}amix=inputs={n}:duration=longest:normalize=0,\
             alimiter=limit=0.95:level=0:attack=8:release=180,aresample=44100[mixout]"
        ));
    }

    parts.join(";")
}

#[allow(clippy::too_many_arguments)]
/// Formats a Unix-epoch millisecond timestamp as `YYYY-MM-DD` (UTC) without
/// pulling in a date/time crate — Howard Hinnant's civil_from_days algorithm.
fn unix_millis_to_ymd(millis: u64) -> String {
    let days = (millis / 86_400_000) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Builds the `ffmpeg` `-map_metadata`/`-metadata` argument pairs that stamp a
/// rendered BoogieMix's own title/album/artist/date onto the output container
/// instead of inheriting the first input track's ID3 tags (ffmpeg's default
/// when `-map_metadata` is omitted is `-map_metadata 0`). Pure/no-ffmpeg so it
/// can be unit-tested directly; `render_mix` passes the result straight to
/// `Command::args`. Title/album format must stay in sync with the client's
/// `mixOutputToTrack` (PlaylistsView.tsx) so the in-app now-playing title and
/// the file's own ID3 title never disagree.
fn render_metadata_args(playlist_name: &str, rendered_date: &str) -> Vec<String> {
    vec![
        "-map_metadata".into(),
        "-1".into(),
        "-metadata".into(),
        format!("title={playlist_name} — BoogieMix"),
        "-metadata".into(),
        format!("album={playlist_name}"),
        "-metadata".into(),
        "artist=BoogieBox BoogieMix".into(),
        "-metadata".into(),
        format!("date={rendered_date}"),
    ]
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
    playlist_name: &str,
    rendered_date: &str,
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
            beat_grid_bpm(deep)
                .filter(|&b| (60.0..=220.0).contains(&b))
                .or(a.bpm_estimate)
                .or_else(|| {
                    deep.and_then(|d| d.bpm_refined)
                        .filter(|&b| (60.0..=220.0).contains(&b))
                })
        })
        .collect();

    tracing::debug!(resolved_bpms = ?bpms, "per-track BPM resolution");

    // Fit each track's grid at the two points it is actually mixed at, rather
    // than trusting one whole-track tempo: real grids slip half a beat and drop
    // beats in places, and a single global BPM is typically 0.03-0.06% off,
    // which walks the grid 200 ms across a track.
    let rhythms: Vec<TrackRhythm> = tracks
        .iter()
        .zip(analyses.iter())
        .zip(bpms.iter())
        .enumerate()
        .map(|(i, ((t, a), bpm))| {
            let mix_in = if i > 0 {
                transitions
                    .get(i - 1)
                    .map(|tr| tr.to_intro_start_sec)
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            let mix_out = transitions
                .get(i)
                .map(|tr| tr.from_outro_start_sec.min(a.duration_sec))
                .unwrap_or(a.duration_sec);
            build_track_rhythm(
                deep_features.get(&t.track_id.to_string()),
                *bpm,
                mix_in,
                mix_out,
            )
        })
        .collect();

    let in_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.in_bpm()).collect();
    let out_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.out_bpm()).collect();
    let (tempos, matched) = plan_blend_tempos(&in_bpms, &out_bpms);
    tracing::debug!(?in_bpms, ?out_bpms, ?tempos, ?matched, "blend tempo plan");

    let (mut timings, matched) =
        compute_render_timings(analyses, transitions, &rhythms, &tempos, &matched);

    // Gain-match the spans the mix actually plays, not the whole files. Whole-file
    // loudness left consecutive tracks up to ~5 dB apart in a measured render,
    // because a track's mixed-from outro sits well below its own integrated level.
    for (i, track) in tracks.iter().enumerate().take(n) {
        let timing = &timings[i];
        let Some(region_lufs) =
            analyze_region_loudness(ffmpeg, &track.file_path, timing.start, timing.trim_dur).await
        else {
            continue;
        };
        timings[i].gain_db = (-14.0 - region_lufs).clamp(-12.0, 6.0);
        tracing::debug!(
            track_i = i,
            region_lufs,
            gain_db = timings[i].gain_db,
            "region loudness gain match"
        );
    }
    let timings = timings;

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

    let filter_complex = build_filter_complex(plan, &timings, n, &rhythms, &matched);
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
    let metadata_args = render_metadata_args(playlist_name, rendered_date);
    cmd.args([
        "-filter_complex",
        filter_complex.as_str(),
        "-map",
        "[mixout]",
    ]);
    cmd.args(metadata_args.iter().map(String::as_str));
    cmd.args(["-c:a", "libmp3lame", "-b:a", "320k"]);
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
        // ffmpeg prints input/metadata info first and the actual error last,
        // so tail-truncate rather than head-truncate or the real reason never
        // survives into last_message.
        let chars: Vec<char> = stderr.chars().collect();
        let tail_start = chars.len().saturating_sub(600);
        let tail: String = chars[tail_start..].iter().collect();
        return Err(format!("FFmpeg render failed: {tail}"));
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
    use boogiebox_db::boogiemix::{KeyNeural, StemSummary, StemWindow, TrackSection};

    #[test]
    fn unix_millis_to_ymd_formats_known_dates() {
        assert_eq!(unix_millis_to_ymd(0), "1970-01-01");
        // 2026-09-02T00:00:00Z
        assert_eq!(unix_millis_to_ymd(1_788_307_200_000), "2026-09-02");
        // 2000-02-29T12:00:00Z (leap day)
        assert_eq!(unix_millis_to_ymd(951_825_600_000), "2000-02-29");
    }

    #[test]
    fn render_metadata_args_strip_inherited_tags_and_stamp_playlist_derived_values() {
        let args = render_metadata_args("Road Trip", "2026-09-02");
        assert_eq!(
            args,
            vec![
                "-map_metadata".to_string(),
                "-1".to_string(),
                "-metadata".to_string(),
                "title=Road Trip — BoogieMix".to_string(),
                "-metadata".to_string(),
                "album=Road Trip".to_string(),
                "-metadata".to_string(),
                "artist=BoogieBox BoogieMix".to_string(),
                "-metadata".to_string(),
                "date=2026-09-02".to_string(),
            ]
        );
    }

    #[test]
    fn render_metadata_args_falls_back_playlist_name() {
        // do_process_mix_job falls back to "Playlist" when the name lookup
        // fails (e.g. playlist deleted mid-render) — confirm that value
        // flows through into both the title and album tags unmodified.
        let args = render_metadata_args("Playlist", "2026-01-01");
        assert!(args.contains(&"title=Playlist — BoogieMix".to_string()));
        assert!(args.contains(&"album=Playlist".to_string()));
    }

    /// Integration check for the concern flagged in the play-on-the-fly plan
    /// (§7.1): confirm real `ffmpeg`/`ffprobe` actually write/read these
    /// `-metadata` args as expected on an mp3 container, and — the actual bug
    /// being fixed — that `-map_metadata -1` stops the first input's tags
    /// (e.g. artist) from leaking onto the output the way ffmpeg's implicit
    /// `-map_metadata 0` default otherwise would. Renders a tiny synthetic
    /// silence clip rather than exercising the full `render_mix` filter graph
    /// (which needs a `MixPlan`/`TrackMixAnalysis`/deep-features fixture well
    /// beyond what this metadata concern needs) — self-skips when ffmpeg
    /// isn't on the machine running the tests, matching how this codebase has
    /// no other real-ffmpeg-invocation test to model an `#[ignore]` gate on.
    #[tokio::test]
    async fn render_metadata_args_survive_a_real_ffmpeg_encode() {
        let ffmpeg = crate::ffmpeg::resolve_ffmpeg();
        let ffprobe = crate::ffmpeg::resolve_ffprobe();
        if !crate::ffmpeg::ffmpeg_available() {
            eprintln!("skipping: ffmpeg not available on this machine");
            return;
        }

        let dir = std::env::temp_dir().join(format!(
            "boogiebox-render-metadata-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let input_path = dir.join("input.mp3");
        let output_path = dir.join("output.mp3");

        // Stand-in "first source track" carrying tags that must NOT survive
        // onto the mix output — this is the exact regression render_mix's
        // `-map_metadata -1` fixes (ffmpeg's default `-map_metadata 0` would
        // otherwise copy these straight through).
        let gen = tokio::process::Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-metadata",
                "title=Wrong Track Title",
                "-metadata",
                "artist=Wrong Source Artist",
                "-c:a",
                "libmp3lame",
            ])
            .arg(&input_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .expect("spawn ffmpeg (generate input)");
        assert!(
            gen.status.success(),
            "ffmpeg failed generating test input: {}",
            String::from_utf8_lossy(&gen.stderr)
        );

        let metadata_args = render_metadata_args("Road Trip", "2026-09-02");
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-hide_banner", "-y", "-i"]).arg(&input_path);
        cmd.args(metadata_args.iter().map(String::as_str));
        cmd.args(["-c:a", "libmp3lame"]).arg(&output_path);
        let render = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .expect("spawn ffmpeg (render with metadata args)");
        assert!(
            render.status.success(),
            "ffmpeg failed rendering with metadata args: {}",
            String::from_utf8_lossy(&render.stderr)
        );

        let probe = tokio::process::Command::new(&ffprobe)
            .args([
                "-v",
                "quiet",
                "-show_entries",
                "format_tags=title,album,artist,date",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&output_path)
            .output()
            .await
            .expect("spawn ffprobe");
        assert!(probe.status.success(), "ffprobe failed reading output tags");
        let tags = String::from_utf8_lossy(&probe.stdout).to_lowercase();

        assert!(
            tags.contains("title=road trip — boogiemix") || tags.contains("title=road trip"),
            "expected mix title tag, got: {tags}"
        );
        assert!(
            tags.contains("album=road trip"),
            "expected album tag, got: {tags}"
        );
        assert!(
            tags.contains("artist=boogiebox boogiemix"),
            "expected fixed BoogieMix artist tag, got: {tags}"
        );
        assert!(
            tags.contains("date=2026-09-02"),
            "expected date tag, got: {tags}"
        );
        assert!(
            !tags.contains("wrong track title") && !tags.contains("wrong source artist"),
            "input track's tags leaked onto the mix output despite -map_metadata -1: {tags}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn build_analysis_prefers_tag_bpm_over_local_detection() {
        // Regression: the local flux-autocorrelation BPM detector is prone to
        // octave errors (half/double the true tempo) and clusters toward a
        // generic value on ambiguous rhythms, while tag BPM is normally
        // curated. A real-world example: Overflow tagged 130 BPM was locally
        // detected at 66.67 (a clean octave error) — using the tag avoids
        // beat-matching against a wrong target, which is what actually causes
        // audible drum desync during a long crossfade.
        let mut track = input_track("a");
        track.bpm = Some(130.0);
        track.bpm_detected = Some(66.67);
        let a = build_analysis(&track, None, None, None, None);
        assert_eq!(a.bpm_estimate, Some(130.0));
    }

    #[test]
    fn build_analysis_falls_back_to_detected_bpm_without_a_tag() {
        let mut track = input_track("a");
        track.bpm = None;
        track.bpm_detected = Some(118.0);
        let a = build_analysis(&track, None, None, None, None);
        assert_eq!(a.bpm_estimate, Some(118.0));
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

    fn stem_window(start: f64, end: f64, average: f64) -> StemWindow {
        StemWindow {
            start,
            end,
            strength: average,
            average,
        }
    }

    fn without_rhythm_timing(mut deep: DeepTrackFeatures) -> DeepTrackFeatures {
        deep.confidence = 0.4;
        deep.drum_windows.clear();
        deep.beat_grid = None;
        for window in &mut deep.transition_windows {
            window.vocals_rms = None;
            window.drums_rms = None;
            window.bass_rms = None;
            window.other_rms = None;
        }
        deep
    }

    #[test]
    fn club_blend_starts_deep_transition_where_drums_are_active() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let mut from_deep = deep_for("a", 0.1, 0.8, 0.2);
        let mut to_deep = deep_for("b", 0.1, 0.8, 0.2);
        from_deep.transition_windows[1].start = 200.0;
        from_deep.transition_windows[1].end = 240.0;
        from_deep.transition_windows[1].recommended_max_crossfade = 24.0;
        to_deep.transition_windows[0].start = 0.0;
        to_deep.transition_windows[0].end = 48.0;
        to_deep.transition_windows[0].recommended_max_crossfade = 24.0;
        from_deep.drum_windows = vec![stem_window(214.0, 226.0, 0.9)];
        to_deep.drum_windows = vec![stem_window(18.0, 42.0, 0.9)];

        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            8.0,
        );

        assert!(trans.deep_used);
        assert!((trans.from_outro_start_sec - 228.0).abs() < 0.01);
        assert!((trans.to_intro_start_sec - 18.0).abs() < 0.01);
        assert_eq!(trans.kind, "beat_blend");
    }

    #[test]
    fn club_blend_cuts_after_last_drum_before_beatless_outro() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let mut from_deep = deep_for("a", 0.1, 0.8, 0.2);
        let mut to_deep = deep_for("b", 0.1, 0.8, 0.2);
        from_deep.transition_windows[1].start = 210.0;
        from_deep.transition_windows[1].end = 240.0;
        from_deep.transition_windows[1].recommended_max_crossfade = 24.0;
        to_deep.transition_windows[0].start = 0.0;
        to_deep.transition_windows[0].end = 48.0;
        to_deep.transition_windows[0].recommended_max_crossfade = 24.0;
        from_deep.drum_windows = vec![stem_window(160.0, 190.0, 0.9)];
        to_deep.drum_windows = vec![stem_window(18.0, 42.0, 0.9)];

        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            45.0,
        );

        assert!(trans.deep_used);
        assert!((trans.from_outro_start_sec - 192.0).abs() < 0.01);
        assert!(trans.from_outro_start_sec < from_deep.transition_windows[1].start);
    }

    #[test]
    fn synthetic_deep_rows_fall_back_to_legacy_transition_timing() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = without_rhythm_timing(deep_for("a", 0.25, 0.35, 0.3));
        let to_deep = without_rhythm_timing(deep_for("b", 0.25, 0.35, 0.3));

        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            8.0,
        );

        assert!(!trans.deep_used);
        assert!(!trans.reason.starts_with("deep:"));
        assert_eq!(trans.to_intro_start_sec, 16.0);
        assert!(trans.from_outro_start_sec < 240.0 - trans.crossfade_sec);
    }

    #[test]
    fn long_overlay_fallback_caps_very_late_outgoing_tail() {
        let mut from_a = analysis("a", 124.0);
        from_a.duration_sec = 600.0;
        from_a.intro_end_sec = 60.0;
        from_a.outro_start_sec = 598.0;
        let mut to_a = analysis("b", 124.0);
        to_a.duration_sec = 600.0;
        to_a.intro_end_sec = 60.0;
        to_a.outro_start_sec = 598.0;

        let preset = style_preset("club_blend");
        let trans = select_transition(
            &from_a,
            &input_track("a"),
            &to_a,
            &input_track("b"),
            "lift",
            &preset,
            45.0,
        );

        assert_eq!(trans.crossfade_sec, 45.0);
        assert!(trans.from_outro_start_sec < 520.0);
        assert!(trans.from_outro_start_sec > 440.0);
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

    /// Renders with no rhythmic evidence, i.e. the pre-deep-analysis path:
    /// timings follow the plan verbatim with no quantisation or phase locking.
    fn no_rhythm(n: usize) -> Vec<TrackRhythm> {
        vec![TrackRhythm::default(); n]
    }

    fn unit_tempos(n: usize) -> Vec<TrackTempo> {
        vec![
            TrackTempo {
                head: 1.0,
                tail: 1.0
            };
            n
        ]
    }

    /// Perfectly regular grid at `bpm` with its first beat at `phase`, and real
    /// bar ones every 4 beats from there.
    fn rhythm_at(bpm: f64, phase: f64, mix_in: f64, mix_out: f64, dur: f64) -> TrackRhythm {
        let beat = 60.0 / bpm;
        let beats: Vec<f64> = (0..(dur / beat) as usize)
            .map(|i| phase + beat * i as f64)
            .collect();
        let downbeats: Vec<f64> = beats.iter().copied().step_by(4).collect();
        let in_grid = fit_local_grid(&beats, mix_in, GRID_FIT_HALF_WINDOW_SEC);
        let out_grid = fit_local_grid(&beats, mix_out, GRID_FIT_HALF_WINDOW_SEC);
        TrackRhythm {
            in_bar_offset: in_grid
                .and_then(|g| bar_offset(&g, &downbeats, mix_in, GRID_FIT_HALF_WINDOW_SEC)),
            out_bar_offset: out_grid
                .and_then(|g| bar_offset(&g, &downbeats, mix_out, GRID_FIT_HALF_WINDOW_SEC)),
            in_grid,
            out_grid,
            fallback_bpm: Some(bpm),
        }
    }

    /// `compute_render_timings` with every track left at its own tempo.
    fn timings_for(
        refs: &[&TrackMixAnalysis],
        transitions: &[MixTransition],
        rhythms: &[TrackRhythm],
    ) -> Vec<RenderTiming> {
        let n = refs.len();
        compute_render_timings(
            refs,
            transitions,
            rhythms,
            &unit_tempos(n),
            &vec![true; n.saturating_sub(1)],
        )
        .0
    }

    fn filter_for(plan: &MixPlan, timings: &[RenderTiming], n: usize) -> String {
        build_filter_complex(
            plan,
            timings,
            n,
            &no_rhythm(n),
            &vec![false; n.saturating_sub(1)],
        )
    }

    #[test]
    fn build_filter_complex_swaps_bass_between_outgoing_and_incoming() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.5, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        // Outgoing gives up its low end from the handover; incoming holds its own
        // back until the same instant. Both steps land together so the total low
        // end stays continuous instead of exposing a one-sided spectral jump.
        assert!(filter.contains("bass=g=-5.00:f=180:enable='gte(t,196.400)'"));
        assert!(filter.contains("bass=g=-5.00:f=180:enable='lt(t,8.400)'"));
        // The old timeline-switched sweep is gone entirely.
        assert!(!filter.contains("highpass=f=200"));
        assert!(!filter.contains("lowpass=f=8000"));
    }

    #[test]
    fn safe_mix_strips_all_effects() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        // Transition asks for bass duck, filter sweep, and echo. Safe mode must strip them.
        let transition = render_transition("a", "b", 12.0, 0.7, true, 0.5);
        let plan = render_plan("safe_mix", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        assert!(!filter.contains("bass=g=-"));
        assert!(!filter.contains("aecho"));
        assert!(!filter.contains("highpass=f=200"));
        assert!(!filter.contains("lowpass=f=8000"));
        // safe_mix strips extra effects but still needs the base hold-then-fade
        // crossfade: 12s crossfade -> 30% tail = 3.6s fade, holding until 196.4s.
        assert!(filter.contains("afade=t=out:st=196.400:d=3.600:curve=qsin"));
        assert!(filter.contains("afade=t=in:st=0:d=12.000:curve=qsin"));
    }

    #[test]
    fn club_blend_holds_outgoing_full_volume_then_fades_without_switched_sweep() {
        let analyses = [analysis("a", 124.0), analysis("b", 124.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 16.0, 0.0, true, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        // 16s crossfade -> 30% tail = 4.8s fade, holding full volume until 195.2s.
        assert!(filter.contains("afade=t=out:st=195.200:d=4.800:curve=qsin"));
        assert!(filter.contains("afade=t=in:st=0:d=16.000:curve=qsin"));
        // The transition asks for a filter sweep, but switching a highpass/lowpass
        // on and off mid-music is a hard timbral step that reads as an abrupt
        // handover. Only continuous shaping survives now.
        assert!(!filter.contains("highpass=f=200"));
        assert!(!filter.contains("lowpass=f=8000"));
        assert!(!filter.contains("enable='between(t,"));
    }

    #[test]
    fn fade_durations_honor_transition_crossfade_length() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 18.0, 0.0, false, 0.0);
        let plan = render_plan("chill_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        // Incoming fades in across the full 18s window; outgoing holds full volume
        // until the last 30% (5.4s) of that window before fading out.
        assert!(filter.contains("afade=t=in:st=0:d=18.000:curve=qsin"));
        assert!(filter.contains("afade=t=out:st=194.600:d=5.400:curve=qsin"));
        // Tracks are combined via delayed amix, not the old symmetric acrossfade.
        assert!(!filter.contains("acrossfade"));
        assert!(filter.contains("amix=inputs=2:duration=longest:normalize=0"));
    }

    #[test]
    fn long_build_45s_holds_outgoing_for_70_percent_before_fading() {
        // Regression: long_build previously produced a real ~6 dB loudness dip
        // (measured via ffmpeg acrossfade + ebur128) from a mismatched cub/ihsin
        // curve pair, and a plain symmetric crossfade still read as "one track
        // fades while the other starts" even after that fix. The renderer now
        // holds the outgoing track at full volume for OUTGOING_HOLD_FRACTION of
        // the window and only fades it over the remaining tail, while the
        // incoming track fades in gradually across the whole window.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 45.0, 0.0, false, 0.0);
        let plan = render_plan("long_build", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        // 45s crossfade -> 30% tail = 13.5s fade, holding full volume until 186.5s.
        assert!(filter.contains("afade=t=out:st=186.500:d=13.500:curve=qsin"));
        assert!(filter.contains("afade=t=in:st=0:d=45.000:curve=qsin"));
        assert!(!filter.contains("acrossfade"));
        assert!(!filter.contains("c1=cub"));
        assert!(!filter.contains("c2=ihsin"));
    }

    #[test]
    fn resolve_output_starts_delays_each_track_by_prior_overlap() {
        let analyses = [
            analysis("a", 120.0),
            analysis("b", 120.0),
            analysis("c", 120.0),
        ];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let t1 = render_transition("a", "b", 20.0, 0.0, false, 0.0);
        let t2 = render_transition("b", "c", 20.0, 0.0, false, 0.0);
        let plan = MixPlan {
            playlist_id: EntityId::Str("p1".into()),
            default_crossfade_sec: 20.0,
            target_energy_ramp: vec![0.4, 0.5, 0.6],
            transitions: vec![t1, t2],
            style: "long_build".into(),
            ordered_track_ids: vec![
                EntityId::Str("a".into()),
                EntityId::Str("b".into()),
                EntityId::Str("c".into()),
            ],
            energy_curve_phases: vec!["groove".into(), "lift".into(), "peak".into()],
            per_track_energy: HashMap::new(),
            anthem_track_id: None,
        };
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(3));
        let cross_durs = resolve_cross_durs(&plan, &timings, 3, &no_rhythm(3));
        let aligned =
            resolve_aligned_timeline(&timings, &cross_durs, &no_rhythm(3), &[false, false]);
        let output_starts = aligned.output_starts;
        assert_eq!(output_starts[0], 0.0);
        // track0 effective_dur=200, minus its 20s crossfade with track1.
        assert!((output_starts[1] - 180.0).abs() < 1e-6);
        // track2 starts after track1's own (trimmed) run, minus their crossfade.
        assert!(output_starts[2] > output_starts[1]);
    }

    #[test]
    fn aligned_timeline_locks_incoming_downbeats_onto_outgoing_ones() {
        // Both tracks run at 120 BPM (2.0s bars) but their downbeat grids are
        // offset by 0.9s — close to the worst case measured on a real render,
        // where matched tempos still left the drums ~870 ms apart for the whole
        // 45 s blend because nothing ever aligned the phase.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 20.0, 0.0, false, 0.0);
        let plan = render_plan("long_build", transition);
        let rhythms = vec![
            rhythm_at(120.0, 0.0, 0.0, 200.0, 260.0),
            rhythm_at(120.0, 0.9, 4.0, 200.0, 260.0),
        ];
        let timings = timings_for(&refs, &plan.transitions, &rhythms);
        let cross_durs = resolve_cross_durs(&plan, &timings, 2, &rhythms);
        let aligned = resolve_aligned_timeline(&timings, &cross_durs, &rhythms, &[true]);

        let bar = 2.0_f64;
        let overlap_start = aligned.output_starts[1];
        // Track A's bar ones sit on multiples of the bar from its own trim start.
        let a_phase = (overlap_start - timings[0].start).rem_euclid(bar);
        // Track B's stream was snapped to its own bar one, so its first bar one
        // is the overlap start itself.
        assert!(
            a_phase < 1e-6 || (bar - a_phase) < 1e-6,
            "overlap must begin on an outgoing bar one, phase was {a_phase}"
        );
        // The nudge stays sub-bar: the blend length is preserved to within half a bar.
        assert!((aligned.cross_durs[0] - cross_durs[0]).abs() <= bar / 2.0 + 1e-6);
    }

    #[test]
    fn aligned_timeline_quantizes_blend_to_whole_bars() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        // 45s at 120 BPM is 22.5 bars — the blend should land on a whole bar so it
        // also ends on a phrase edge rather than mid-bar.
        let transition = render_transition("a", "b", 45.0, 0.0, false, 0.0);
        let plan = render_plan("long_build", transition);
        let rhythms = vec![
            rhythm_at(120.0, 0.0, 0.0, 200.0, 260.0),
            rhythm_at(120.0, 0.0, 4.0, 200.0, 260.0),
        ];
        let timings = timings_for(&refs, &plan.transitions, &rhythms);
        let cross_durs = resolve_cross_durs(&plan, &timings, 2, &rhythms);
        let aligned = resolve_aligned_timeline(&timings, &cross_durs, &rhythms, &[true]);
        let bars = aligned.cross_durs[0] / 2.0;
        assert!(
            (bars - bars.round()).abs() < 1e-6,
            "blend was {} bars",
            bars
        );
    }

    #[test]
    fn trim_start_snaps_to_a_real_bar_one() {
        // Planner entry points land wherever window scoring put them (0.0/0.2/0.5s
        // in real data), which opens the incoming track mid-bar.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 20.0, 0.0, false, 0.0);
        // Bar ones every 2s from 0.35s; the plan asks to come in at 4.0s.
        let rhythms = vec![
            rhythm_at(120.0, 0.0, 0.0, 200.0, 260.0),
            rhythm_at(120.0, 0.35, 4.0, 200.0, 260.0),
        ];
        let spans = compute_source_spans(&refs, &[transition], &rhythms);
        assert!(
            (spans[1].start - 4.35).abs() < 1e-6,
            "entry snapped to {}",
            spans[1].start
        );

        // Without any grid the start is left alone rather than snapped to a
        // synthetic grid anchored at file t=0, which carries no phase information.
        let transition = render_transition("a", "b", 20.0, 0.0, false, 0.0);
        let spans = compute_source_spans(&refs, &[transition], &no_rhythm(2));
        assert!((spans[1].start - 4.0).abs() < 1e-9);
    }

    #[test]
    fn octave_equivalent_tempos_match_instead_of_going_unmatched() {
        // A half-time reading describes the same groove, so folding it first
        // turns an impossible 94% stretch into a 1.5% one.
        let (tempos, matched) =
            plan_blend_tempos(&[Some(130.0), Some(66.0)], &[Some(130.0), Some(66.0)]);
        assert!(matched[0], "a half-time reading must still beat-match");
        // Both sides meet in the middle of 130 and 132.
        let blend = 130.0 * tempos[0].tail;
        assert!((blend - (130.0f64 * 132.0).sqrt()).abs() < 1e-9);
        assert!((66.0 * 2.0 * tempos[1].head - blend).abs() < 1e-9);
    }

    #[test]
    fn residual_tempo_mismatch_caps_the_blend_length() {
        // Perfectly matched tempos are free to blend for as long as asked.
        assert!(max_cross_for_tempo_drift(Some(125.0), Some(125.0)).is_infinite());
        // A 2% residual drifts half a beat in ~12s, so a 45s overlay of the two
        // would fall apart long before it ended.
        let cap = max_cross_for_tempo_drift(Some(125.0), Some(127.5));
        assert!(cap > 11.0 && cap < 13.0, "cap was {cap}");
        assert!(max_cross_for_tempo_drift(None, Some(125.0)).is_infinite());
    }

    #[test]
    fn region_gain_replaces_the_old_whole_track_transition_duck() {
        // eq_duck used to be folded into the static per-track gain, which quietened
        // a track across its entire length and only when it had an outgoing
        // transition — a per-track level offset audible as a loudness step.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.5, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        // Both tracks measure -14 LUFS, so both must land at exactly unity gain.
        assert!(timings[0].gain_db.abs() < 1e-9);
        assert!(timings[1].gain_db.abs() < 1e-9);
    }

    #[test]
    fn no_track_carries_a_whole_track_echo() {
        // Regression: `aecho` has no timeline support, so an echo asked for at
        // one transition was applied across the whole outgoing track. Measured on
        // a real render, `aecho=0.8:0.7` cost those tracks 4.5 dB against the
        // tracks without it — the largest track-to-track loudness step in the mix.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let mut transition = render_transition("a", "b", 45.0, 0.0, false, 0.0);
        transition.echo_tail_sec = 0.4;
        let plan = render_plan("long_build", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        assert!(!filter.contains("aecho"));
    }

    #[test]
    fn refines_tempo_past_the_beat_trackers_frame_quantization() {
        // A grid at exactly 127 BPM whose beat times are quantised to the beat
        // tracker's 10 ms frames, the way madmom emits them. The median interval
        // lands on a frame boundary (0.47s -> 127.66 BPM, 0.5% high); averaging
        // the intervals recovers the true tempo.
        let beat = 60.0 / 127.0;
        let beats: Vec<f64> = (0..200)
            .map(|i| ((i as f64 * beat) * 100.0).round() / 100.0)
            .collect();
        let refined = refined_bpm_from_beats(&beats).expect("refined tempo");
        assert!(
            (refined - 127.0).abs() < 0.05,
            "expected ~127 BPM, got {refined}"
        );

        // A dropped beat must not drag the estimate down.
        let mut gapped = beats.clone();
        gapped.remove(100);
        let refined_gapped = refined_bpm_from_beats(&gapped).expect("refined tempo");
        assert!(
            (refined_gapped - 127.0).abs() < 0.05,
            "expected ~127 BPM despite a dropped beat, got {refined_gapped}"
        );

        assert_eq!(refined_bpm_from_beats(&[0.0, 0.5, 1.0]), None);
    }

    #[test]
    fn parses_integrated_loudness_from_ebur128_summary() {
        let summary = "[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    \
                       I:         -12.4 LUFS\n    Threshold: -22.6 LUFS\n";
        assert_eq!(parse_ebur128_integrated(summary), Some(-12.4));
        assert_eq!(parse_ebur128_integrated("no loudness here"), None);
        // Silence reads as -inf and must not be used as a gain-match target.
        assert_eq!(parse_ebur128_integrated("    I:  -70.5 LUFS"), None);
    }

    /// Output-timeline positions of a track's grid beats inside `[from, to]`.
    fn output_beats(
        rhythm: &TrackRhythm,
        timing: &RenderTiming,
        output_start: f64,
        from: f64,
        to: f64,
        use_out_grid: bool,
    ) -> Vec<f64> {
        let grid = if use_out_grid {
            rhythm.out_grid
        } else {
            rhythm.in_grid
        }
        .expect("grid");
        let mut beats = Vec::new();
        let mut t = grid.beat_at_or_after(timing.start);
        while t <= timing.start + timing.trim_dur {
            let at = output_start + timing.tempo.to_output(t - timing.start);
            if at >= from - 1e-9 && at <= to + 1e-9 {
                beats.push(at);
            }
            t += grid.period;
        }
        beats
    }

    #[test]
    fn a_matched_blend_puts_both_tracks_beats_on_top_of_each_other() {
        // The complaint this whole path exists to answer: two tracks at
        // different tempos, mixed for 30 s, with their grids at unrelated
        // phases. After tempo matching and phase locking every incoming beat
        // must land on an outgoing beat, and stay there for the whole blend.
        let analyses = [analysis("a", 124.0), analysis("b", 127.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 30.0, 0.0, false, 0.0);
        let plan = render_plan("club_blend", transition);

        // Grids deliberately out of phase with each other and with the plan's
        // entry point, and neither tempo is a round number.
        let rhythms = vec![
            rhythm_at(124.3, 0.17, 0.0, 200.0, 280.0),
            rhythm_at(127.1, 0.61, 4.0, 200.0, 280.0),
        ];
        let in_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.in_bpm()).collect();
        let out_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.out_bpm()).collect();
        let (tempos, matched) = plan_blend_tempos(&in_bpms, &out_bpms);
        assert!(matched[0], "124.3 -> 127.1 is 2.3% apart and must match");

        let (timings, matched) =
            compute_render_timings(&refs, &plan.transitions, &rhythms, &tempos, &matched);
        let cross = resolve_cross_durs(&plan, &timings, 2, &rhythms);
        let aligned = resolve_aligned_timeline(&timings, &cross, &rhythms, &matched);

        let overlap_start = aligned.output_starts[1];
        let overlap_end = aligned.output_starts[0] + timings[0].effective_dur;
        assert!(
            overlap_end - overlap_start > 20.0,
            "expected a real overlap, got {}",
            overlap_end - overlap_start
        );

        let out_beats = output_beats(
            &rhythms[0],
            &timings[0],
            aligned.output_starts[0],
            overlap_start,
            overlap_end,
            true,
        );
        let in_beats = output_beats(
            &rhythms[1],
            &timings[1],
            aligned.output_starts[1],
            overlap_start,
            overlap_end,
            false,
        );
        assert!(out_beats.len() > 40 && in_beats.len() > 40);

        let mut worst: f64 = 0.0;
        for b in &in_beats {
            let nearest = out_beats
                .iter()
                .map(|o| (o - b).abs())
                .fold(f64::INFINITY, f64::min);
            worst = worst.max(nearest);
        }
        // 5 ms is far below the ~10 ms where a doubled kick starts to sound
        // like a flam; the pre-fix renderer measured 870 ms on a real blend.
        assert!(
            worst < 0.005,
            "worst beat offset across the blend was {:.1} ms",
            worst * 1000.0
        );
    }

    #[test]
    fn an_unmatched_boundary_is_not_phase_locked_and_gets_a_short_blend() {
        // 120 vs 140 cannot be closed inside the limit. Rather than pretending,
        // the blend keeps its planned position and the drift cap shortens it.
        let analyses = [analysis("a", 120.0), analysis("b", 140.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 45.0, 0.0, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let rhythms = vec![
            rhythm_at(120.0, 0.0, 0.0, 200.0, 280.0),
            rhythm_at(140.0, 0.0, 4.0, 200.0, 280.0),
        ];
        let in_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.in_bpm()).collect();
        let out_bpms: Vec<Option<f64>> = rhythms.iter().map(|r| r.out_bpm()).collect();
        let (tempos, matched) = plan_blend_tempos(&in_bpms, &out_bpms);
        assert!(!matched[0]);
        assert_eq!(tempos[0].tail, 1.0);

        let (timings, matched) =
            compute_render_timings(&refs, &plan.transitions, &rhythms, &tempos, &matched);
        let cross = resolve_cross_durs(&plan, &timings, 2, &rhythms);
        // A 16.7% residual drifts half a beat in well under a second, so the
        // cap floors at 6 s rather than holding a 45 s overlay that falls apart.
        assert!((cross[0] - 6.0).abs() < 1e-6, "cross was {}", cross[0]);
        let aligned = resolve_aligned_timeline(&timings, &cross, &rhythms, &matched);
        // No phase nudge was applied: the blend sits exactly where planned.
        let expected = timings[0].effective_dur - cross[0];
        assert!((aligned.output_starts[1] - expected).abs() < 1e-9);
    }

    #[test]
    fn wide_tempo_gaps_are_left_unmatched_rather_than_stretched() {
        // 100 -> 140 is 40% apart. Even pushed from both sides the limit only
        // closes about 6%, so the boundary must be reported unmatched instead of
        // stretching a track until it sounds wrong.
        let (tempos, matched) =
            plan_blend_tempos(&[Some(100.0), Some(140.0)], &[Some(100.0), Some(140.0)]);
        assert!(!matched[0]);
        assert_eq!(tempos[0].tail, 1.0);
        assert_eq!(tempos[1].head, 1.0);
    }

    #[test]
    fn neighbouring_tempos_meet_in_the_middle_within_the_limit() {
        // 120 -> 123 is 2.5% apart: reachable, and the cost is shared rather
        // than dumped entirely on the incoming track.
        let (tempos, matched) =
            plan_blend_tempos(&[Some(120.0), Some(123.0)], &[Some(120.0), Some(123.0)]);
        assert!(matched[0]);
        let blend = 120.0 * tempos[0].tail;
        assert!((blend - (120.0f64 * 123.0).sqrt()).abs() < 1e-9);
        assert!((123.0 * tempos[1].head - blend).abs() < 1e-9);
        // Neither track moves more than the limit.
        assert!((tempos[0].tail - 1.0).abs() <= TEMPO_MATCH_LIMIT + 1e-9);
        assert!((tempos[1].head - 1.0).abs() <= TEMPO_MATCH_LIMIT + 1e-9);
    }

    #[test]
    fn tempo_travels_across_a_set_instead_of_pinning_to_the_first_track() {
        // The whole point of deciding tempo per boundary: a single constant
        // factor per track mathematically forces one tempo for the entire mix,
        // which pinned a real 120-134 BPM playlist at 122.2 and played its
        // 134 BPM closer 8.7% slow. Here every track stays near its own tempo.
        let bpms = [
            Some(120.0),
            Some(122.0),
            Some(124.0),
            Some(126.5),
            Some(129.0),
            Some(131.5),
            Some(134.0),
        ];
        let (tempos, matched) = plan_blend_tempos(&bpms, &bpms);
        assert!(matched.iter().all(|m| *m), "every hop is inside the limit");
        for (i, t) in tempos.iter().enumerate() {
            assert!(
                (t.head - 1.0).abs() <= TEMPO_MATCH_LIMIT + 1e-9
                    && (t.tail - 1.0).abs() <= TEMPO_MATCH_LIMIT + 1e-9,
                "track {i} bent past the limit: {t:?}"
            );
        }
        // The last track still plays essentially at its own 134 BPM.
        let last = bpms[6].unwrap() * tempos[6].head;
        assert!((last - 134.0).abs() < 134.0 * TEMPO_MATCH_LIMIT);
    }

    #[test]
    fn every_matched_boundary_ends_up_at_one_shared_tempo() {
        let bpms = [Some(124.0), Some(126.0), Some(128.0)];
        let (tempos, matched) = plan_blend_tempos(&bpms, &bpms);
        for i in 0..2 {
            assert!(matched[i]);
            let out = bpms[i].unwrap() * tempos[i].tail;
            let inn = bpms[i + 1].unwrap() * tempos[i + 1].head;
            assert!((out - inn).abs() < 1e-9, "boundary {i}: {out} vs {inn}");
        }
    }

    #[test]
    fn missing_tempo_leaves_a_boundary_unmatched_and_unstretched() {
        let (tempos, matched) = plan_blend_tempos(
            &[Some(120.0), None, Some(120.0)],
            &[Some(120.0), None, Some(120.0)],
        );
        assert_eq!(matched, vec![false, false]);
        assert!(tempos.iter().all(|t| t.head == 1.0 && t.tail == 1.0));
    }

    #[test]
    fn a_tempo_map_maps_source_time_through_a_mid_track_tempo_move() {
        let span = SourceSpan {
            start: 10.0,
            dur: 240.0,
        };
        let tempo = TrackTempo {
            head: 0.99,
            tail: 1.01,
        };
        let map = build_tempo_map(span, tempo, 30.0, 30.0);
        assert!(!map.is_constant(), "a 2% move should be stepped");
        assert!((map.head_factor() - 0.99).abs() < 1e-9);
        assert!((map.tail_factor() - 1.01).abs() < 1e-9);
        assert!((map.src_duration() - 240.0).abs() < 1e-9);
        // Both blends sit in constant-tempo territory, so their mapping is exact.
        assert!((map.to_output(20.0) - 20.0 / 0.99).abs() < 1e-9);
        let from_end = map.out_duration() - map.to_output(240.0 - 20.0);
        assert!((from_end - 20.0 / 1.01).abs() < 1e-9);
        // Monotonic, and the total is the sum of the parts.
        let mut prev = -1.0;
        for k in 0..=240 {
            let v = map.to_output(k as f64);
            assert!(v > prev, "map went backwards at {k}");
            prev = v;
        }
    }

    #[test]
    fn a_tempo_move_with_no_room_between_blends_falls_back_to_one_tempo() {
        // A short track whose two blends leave no clear middle: keep the
        // incoming blend locked rather than changing tempo underneath either.
        let span = SourceSpan {
            start: 0.0,
            dur: 40.0,
        };
        let tempo = TrackTempo {
            head: 0.98,
            tail: 1.02,
        };
        let map = build_tempo_map(span, tempo, 20.0, 20.0);
        assert!(map.is_constant());
        assert!((map.head_factor() - 0.98).abs() < 1e-9);
    }

    #[test]
    fn losing_the_room_to_change_tempo_also_loses_the_beat_lock_claim() {
        // If the tempo move cannot be made, the outgoing blend is no longer
        // running at the tempo the plan assumed, so it must stop claiming a lock
        // instead of phase-locking against a tempo that was never applied.
        // The middle track here has two 90 s blends inside a 196 s body, leaving
        // no clear middle to change tempo in.
        let analyses = [
            analysis("a", 120.0),
            analysis("b", 120.0),
            analysis("c", 120.0),
        ];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transitions = vec![
            render_transition("a", "b", 90.0, 0.0, false, 0.0),
            render_transition("b", "c", 90.0, 0.0, false, 0.0),
        ];
        let tempos = vec![
            TrackTempo {
                head: 1.0,
                tail: 0.98,
            },
            TrackTempo {
                head: 0.98,
                tail: 1.02,
            },
            TrackTempo {
                head: 1.02,
                tail: 1.02,
            },
        ];
        let (timings, matched) =
            compute_render_timings(&refs, &transitions, &no_rhythm(3), &tempos, &[true, true]);
        assert!(timings[1].tempo.is_constant());
        assert!((timings[1].tempo.tail_factor() - 0.98).abs() < 1e-9);
        // The incoming blend keeps its lock; the outgoing one gives it up.
        assert_eq!(matched, vec![true, false]);
    }

    #[test]
    fn every_track_carries_a_tempo_filter_so_the_atempo_offset_is_uniform() {
        // `atrim`+`atempo` swallows a fixed ~21 ms prefix. A track that happened
        // to need no tempo change used to skip the filter entirely and so sat
        // 21 ms away from all of its neighbours - an audible flam on its own.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.0, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        assert_eq!(filter.matches("atempo=").count(), 2);
        assert_eq!(filter.matches("atempo=1.000000").count(), 2);
        // And the rate is pinned so that prefix is the same size everywhere.
        assert_eq!(filter.matches("aresample=48000").count(), 2);
    }

    #[test]
    fn tracks_are_placed_at_sample_resolution() {
        // adelay's default millisecond rounding is up to 0.5 ms of avoidable
        // phase error on a join whose whole point is that the drums coincide.
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.0, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let timings = timings_for(&refs, &plan.transitions, &no_rhythm(2));
        let filter = filter_for(&plan, &timings, 2);
        // Track 0 runs 200 s, minus the 12 s overlap, at 48 kHz.
        assert!(filter.contains("adelay=9024000S:all=1"), "{filter}");
    }

    #[test]
    fn a_stepped_tempo_move_renders_as_concatenated_segments() {
        let analyses = [analysis("a", 120.0), analysis("b", 120.0)];
        let refs: Vec<&TrackMixAnalysis> = analyses.iter().collect();
        let transition = render_transition("a", "b", 12.0, 0.0, false, 0.0);
        let plan = render_plan("club_blend", transition);
        let tempos = vec![
            TrackTempo {
                head: 0.985,
                tail: 1.015,
            },
            TrackTempo {
                head: 1.0,
                tail: 1.0,
            },
        ];
        let (timings, _) =
            compute_render_timings(&refs, &plan.transitions, &no_rhythm(2), &tempos, &[true]);
        let filter = filter_for(&plan, &timings, 2);
        assert!(filter.contains("concat=n="), "{filter}");
        // Steps stay under the audibility threshold for a tempo change.
        let factors: Vec<f64> = timings[0].tempo.segments.iter().map(|s| s.1).collect();
        for w in factors.windows(2) {
            assert!(
                (w[1] / w[0] - 1.0).abs() <= MAX_SINGLE_TEMPO_STEP + 1e-9,
                "step {w:?} too large"
            );
        }
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
    fn long_build_widens_narrow_deep_windows_toward_target_crossfade() {
        // Regression: deep-analysis "safe" outro/intro windows are often much
        // narrower (e.g. 16-30s from `deep_for`) than a long_build user's
        // configured blend length (e.g. 45s), and used to silently cap the
        // crossfade there. Since the hold-then-fade render envelope keeps the
        // outgoing track clean through most of the window regardless of its
        // width, long_build should widen narrow windows toward the user's
        // target instead of capping to whatever "safe" span deep analysis
        // happened to detect.
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.2, 0.5, 0.3);
        let to_deep = deep_for("b", 0.2, 0.5, 0.3);
        let preset = style_preset("long_build");

        let transition = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "lift",
            "long_build",
            &preset,
            45.0,
        );
        assert!(
            transition.crossfade_sec >= 40.0,
            "expected crossfade close to the 45s target, got {}",
            transition.crossfade_sec
        );
    }

    #[test]
    fn other_styles_do_not_widen_narrow_deep_windows() {
        // club_blend deliberately keeps tighter, vocal/drum-risk-aware windows;
        // only long_build should widen toward the user's configured target.
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.2, 0.5, 0.3);
        let to_deep = deep_for("b", 0.2, 0.5, 0.3);
        let preset = style_preset("club_blend");

        let transition = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "lift",
            "club_blend",
            &preset,
            45.0,
        );
        assert!(
            transition.crossfade_sec < 30.0,
            "club_blend should stay bounded by the narrow analyzed window, got {}",
            transition.crossfade_sec
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

    // -- Phase 5 additions: pure-logic helpers, AI-plan parsing, job/db plumbing --

    #[test]
    fn camelot_parsing_and_compat() {
        assert_eq!(parse_camelot("11A"), Some((11, false)));
        assert_eq!(parse_camelot("3B"), Some((3, true)));
        assert_eq!(parse_camelot("13A"), None);
        assert_eq!(parse_camelot("A"), None);
        assert_eq!(parse_camelot("x"), None);

        assert_eq!(camelot_compat("8A", "8A"), 1.0);
        assert_eq!(camelot_compat("8A", "8B"), 0.75);
        assert_eq!(camelot_compat("8A", "9A"), 0.8);
        assert_eq!(camelot_compat("8A", "10A"), 0.5);
        assert_eq!(camelot_compat("8A", "1A"), 0.2);
        assert_eq!(camelot_compat("8A", "9B"), 0.6);
        assert_eq!(camelot_compat("??", "8A"), 0.5);
    }

    #[test]
    fn key_string_to_camelot_covers_all_roots() {
        assert_eq!(key_string_to_camelot("Am"), Some("8A".into()));
        assert_eq!(key_string_to_camelot("C"), Some("8B".into()));
        assert_eq!(key_string_to_camelot("F#"), Some("2B".into()));
        assert_eq!(key_string_to_camelot("Gbm"), Some("11A".into()));
        assert_eq!(key_string_to_camelot("Zz"), None);
        assert_eq!(key_string_to_camelot("Zzm"), None);
    }

    #[test]
    fn harmonic_compat_legacy_and_deep_paths() {
        let a = analysis("a", 120.0);
        let mut b = analysis("b", 120.0);
        assert_eq!(harmonic_compat(&a, &b), 1.0);
        b.key_estimate = Some("A".into());
        assert_eq!(harmonic_compat(&a, &b), 0.7); // "Am" vs "A" -> same root text
        b.key_estimate = Some("C#".into());
        assert_eq!(harmonic_compat(&a, &b), 0.35);
        b.key_estimate = None;
        assert_eq!(harmonic_compat(&a, &b), 0.5);

        // deep path: neural key with high confidence overrides key_estimate.
        let mut deep_a = deep_for("a", 0.1, 0.5, 0.2);
        let mut deep_b = deep_for("b", 0.1, 0.5, 0.2);
        deep_a.key_neural = Some(KeyNeural {
            key: "F#".into(),
            mode: "minor".into(),
            confidence: 0.9,
            camelot: Some("11A".into()),
        });
        deep_b.key_neural = Some(KeyNeural {
            key: "F#".into(),
            mode: "minor".into(),
            confidence: 0.9,
            camelot: Some("11A".into()),
        });
        let a2 = analysis("a", 120.0);
        let b2 = analysis("b", 120.0);
        assert_eq!(
            harmonic_compat_deep(&a2, &b2, Some(&deep_a), Some(&deep_b)),
            1.0
        );

        // low-confidence neural key falls back to legacy key_estimate string compare.
        deep_a.key_neural.as_mut().unwrap().confidence = 0.2;
        deep_b.key_neural.as_mut().unwrap().confidence = 0.2;
        assert_eq!(
            harmonic_compat_deep(&a2, &b2, Some(&deep_a), Some(&deep_b)),
            harmonic_compat(&a2, &b2)
        );

        // no deep features at all -> legacy path.
        assert_eq!(
            harmonic_compat_deep(&a2, &b2, None, None),
            harmonic_compat(&a2, &b2)
        );
    }

    #[test]
    fn phase_progression_and_target_energy() {
        assert_eq!(phase_at(0, 1), "groove");
        assert_eq!(phase_at(0, 10), "warmup");
        assert_eq!(phase_at(2, 10), "groove");
        assert_eq!(phase_at(4, 10), "lift");
        assert_eq!(phase_at(6, 10), "peak");
        assert_eq!(phase_at(7, 10), "anthem");
        assert_eq!(phase_at(9, 10), "cooldown");

        assert_eq!(phase_target_energy("warmup"), 0.22);
        assert_eq!(phase_target_energy("anthem"), 0.92);
        assert_eq!(phase_target_energy("unknown"), 0.5);
    }

    #[test]
    fn energy_estimation_prefers_neural_then_refined_then_base() {
        let a = analysis("a", 120.0);
        let base = estimate_energy(&a);
        assert!((0.0..=1.0).contains(&base));

        // No deep features -> effective_energy == base estimate.
        assert_eq!(effective_energy(&a, None), base);

        // Deep with only energy_refined blends toward it.
        let mut deep = deep_for("a", 0.1, 0.5, 0.2);
        deep.energy_refined = 0.9;
        deep.confidence = 1.0;
        let blended = effective_energy(&a, Some(&deep));
        assert!(blended > base);

        // Neural embedding energy takes priority over energy_refined when > 0.
        deep.neural_embedding = Some(boogiebox_db::boogiemix::NeuralEmbedding {
            energy_neural: 0.33,
            danceability: 0.5,
            valence: None,
            embedding_16d: vec![0.0; 16],
            model_version: "v1".into(),
        });
        assert_eq!(effective_energy(&a, Some(&deep)), 0.33);
    }

    #[test]
    fn rounding_and_boundary_helpers() {
        assert_eq!(round2(1.23456), 1.23);
        assert_eq!(round3(1.23456), 1.235);
        assert!(near_end(230.0, 240.0));
        assert!(!near_end(100.0, 240.0));
        assert!(near_end(10.0, 0.0)); // duration<=0 -> always "near end"
        assert!(near_start(50.0));
        assert!(!near_start(120.0));

        let boundaries = [0.0, 8.0, 16.0, 24.0];
        assert_eq!(nearest_phrase_boundary(&boundaries, 17.0, 2.0), Some(16.0));
        assert_eq!(nearest_phrase_boundary(&boundaries, 50.0, 2.0), None);

        assert_eq!(overlap_duration(0.0, 10.0, 5.0, 15.0), 5.0);
        assert!(overlap_duration(0.0, 10.0, 20.0, 30.0) < 0.0);
    }

    #[test]
    fn rhythmic_timing_evidence_and_drum_scoring() {
        assert!(!has_rhythmic_timing_evidence(None));
        let mut deep = deep_for("a", 0.1, 0.5, 0.2);
        deep.drum_windows.clear();
        deep.beat_grid = None;
        for w in &mut deep.transition_windows {
            w.drums_rms = None;
            w.vocals_rms = None;
            w.bass_rms = None;
        }
        assert!(!has_rhythmic_timing_evidence(Some(&deep)));

        deep.drum_windows = vec![stem_window(0.0, 10.0, 0.8)];
        assert!(has_rhythmic_timing_evidence(Some(&deep)));

        assert_eq!(drum_score_for_span(None, 0.0, 10.0), None);
        assert_eq!(drum_score_for_span(Some(&deep), 10.0, 5.0), None); // end <= start
        let score = drum_score_for_span(Some(&deep), 0.0, 10.0).expect("overlap");
        assert!(score > 0.0);
        assert_eq!(drum_score_for_span(Some(&deep), 100.0, 110.0), None);
    }

    #[test]
    fn section_role_lookup() {
        let deep = deep_for("a", 0.1, 0.5, 0.2);
        assert_eq!(section_role(Some(&deep), 5.0), Some("intro"));
        assert_eq!(section_role(Some(&deep), 220.0), Some("outro"));
        assert_eq!(section_role(Some(&deep), 100.0), None);
        assert_eq!(section_role(None, 5.0), None);
    }

    #[test]
    fn bpm_selection_helpers() {
        let a = analysis("a", 120.0);
        // best_bpm falls back to analysis estimate without deep features.
        assert_eq!(best_bpm(&a, None), Some(120.0));

        let mut deep = deep_for("a", 0.1, 0.5, 0.2);
        deep.bpm_refined = Some(121.0);
        assert_eq!(beat_grid_bpm(Some(&deep)), None); // no beat_grid set
        assert_eq!(best_bpm(&a, Some(&deep)), Some(120.0)); // analysis wins over bpm_refined

        let beats: Vec<f64> = (0..20).map(|i| i as f64 * 0.5).collect();
        assert_eq!(snap_to_beat(&beats, 4.05, 0.1), Some(4.0));
        assert_eq!(snap_to_beat(&beats, 4.4, 0.1), Some(4.5));
        assert_eq!(snap_to_beat(&beats, 4.65, 0.1), None);
    }

    #[test]
    fn cosine_similarity_edge_cases() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn reorder_for_curve_places_highest_energy_track_near_anthem_slot() {
        let tracks: Vec<MixTrackInput> = (0..5).map(|i| input_track(&i.to_string())).collect();
        let mut analyses: Vec<TrackMixAnalysis> =
            (0..5).map(|i| analysis(&i.to_string(), 120.0)).collect();
        // Track "3" is the loudest/highest-energy.
        analyses[3].loudness_lufs = Some(-4.0);
        let deep_features: HashMap<String, DeepTrackFeatures> = HashMap::new();
        let preset = style_preset("long_build");
        let (order, anthem_pos) = reorder_for_curve(&tracks, &analyses, &deep_features, &preset);
        assert_eq!(order.len(), 5);
        assert_eq!(order[anthem_pos], 3);
    }

    #[test]
    fn derive_ordered_ids_follows_chain_and_appends_leftovers() {
        let tracks = vec![input_track("a"), input_track("b"), input_track("c")];
        // Empty transitions -> identity order.
        assert_eq!(
            derive_ordered_ids(&[], &tracks),
            tracks
                .iter()
                .map(|t| t.track_id.clone())
                .collect::<Vec<_>>()
        );

        let t1 = render_transition("a", "b", 12.0, 0.0, false, 0.0);
        let t2 = render_transition("b", "c", 12.0, 0.0, false, 0.0);
        let ordered = derive_ordered_ids(&[t1, t2], &tracks);
        assert_eq!(
            ordered,
            vec![
                EntityId::Str("a".into()),
                EntityId::Str("b".into()),
                EntityId::Str("c".into())
            ]
        );

        // A track not referenced by any transition is appended at the end.
        let tracks4 = vec![
            input_track("a"),
            input_track("b"),
            input_track("c"),
            input_track("d"),
        ];
        let ordered4 = derive_ordered_ids(&[t1_clone(), t2_clone()], &tracks4);
        assert_eq!(ordered4.last(), Some(&EntityId::Str("d".into())));
    }

    fn t1_clone() -> MixTransition {
        render_transition("a", "b", 12.0, 0.0, false, 0.0)
    }
    fn t2_clone() -> MixTransition {
        render_transition("b", "c", 12.0, 0.0, false, 0.0)
    }

    #[test]
    fn select_transition_legacy_path_picks_kind_by_phase_and_compat() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let preset = style_preset("club_blend");
        let warmup = select_transition(
            &from_a,
            &input_track("a"),
            &to_a,
            &input_track("b"),
            "warmup",
            &preset,
            12.0,
        );
        assert_eq!(warmup.kind, "blend");
        assert!(!warmup.deep_used);

        let mut far_to = analysis("c", 160.0);
        far_to.key_estimate = Some("C#".into()); // low harmonic compat vs "Am"
        let low_compat = select_transition(
            &from_a,
            &input_track("a"),
            &far_to,
            &input_track("c"),
            "peak",
            &preset,
            12.0,
        );
        assert_eq!(low_compat.kind, "echo_out");
        assert!(low_compat.echo_tail_sec > 0.0);
    }

    #[test]
    fn build_ai_prompt_includes_style_and_track_ids() {
        let tracks = vec![input_track("a"), input_track("b")];
        let analyses = vec![analysis("a", 120.0), analysis("b", 120.0)];
        let prompt = build_ai_prompt(&tracks, &analyses, 12.0, "club_blend");
        assert!(prompt.contains("club_blend"));
        assert!(prompt.contains("\"a\""));
        assert!(prompt.contains("\"b\""));
    }

    #[test]
    fn parse_ai_plan_response_happy_path_and_rejections() {
        let tracks = vec![input_track("a"), input_track("b")];
        let analyses = vec![analysis("a", 120.0), analysis("b", 120.0)];
        let playlist_id = EntityId::Str("p1".into());

        let good = r#"{"transitions":[{"fromTrackId":"a","toTrackId":"b","startA":200,"endA":230,"startB":8,"type":"blend","confidence":0.8}]}"#;
        let plan =
            parse_ai_plan_response(good, &playlist_id, &tracks, &analyses, 12.0, "club_blend")
                .expect("should parse");
        assert_eq!(plan.transitions.len(), 1);
        assert_eq!(plan.transitions[0].kind, "blend");
        assert_eq!(plan.transitions[0].reason, "ai:blend");

        // Wrapped in prose text (extracts the {...} substring).
        let wrapped = format!("Sure, here you go:\n{good}\nHope that helps!");
        assert!(parse_ai_plan_response(
            &wrapped,
            &playlist_id,
            &tracks,
            &analyses,
            12.0,
            "club_blend"
        )
        .is_some());

        // No transitions array -> None.
        assert!(parse_ai_plan_response(
            r#"{"foo":"bar"}"#,
            &playlist_id,
            &tracks,
            &analyses,
            12.0,
            "club_blend"
        )
        .is_none());

        // Empty transitions array -> None.
        assert!(parse_ai_plan_response(
            r#"{"transitions":[]}"#,
            &playlist_id,
            &tracks,
            &analyses,
            12.0,
            "club_blend"
        )
        .is_none());

        // Unparseable garbage -> None.
        assert!(parse_ai_plan_response(
            "not json at all",
            &playlist_id,
            &tracks,
            &analyses,
            12.0,
            "club_blend"
        )
        .is_none());

        // References an unknown track id -> None (whole response rejected).
        assert!(parse_ai_plan_response(
            r#"{"transitions":[{"fromTrackId":"a","toTrackId":"zzz","type":"blend"}]}"#,
            &playlist_id,
            &tracks,
            &analyses,
            12.0,
            "club_blend"
        )
        .is_none());
    }

    #[test]
    fn parse_ai_plan_response_echo_out_gets_tail_and_unknown_type_falls_back_to_blend() {
        let tracks = vec![input_track("a"), input_track("b")];
        let analyses = vec![analysis("a", 120.0), analysis("b", 120.0)];
        let playlist_id = EntityId::Str("p1".into());

        let echo = r#"{"transitions":[{"fromTrackId":"a","toTrackId":"b","type":"echo_out"}]}"#;
        let plan =
            parse_ai_plan_response(echo, &playlist_id, &tracks, &analyses, 12.0, "club_blend")
                .expect("should parse");
        assert_eq!(plan.transitions[0].kind, "echo_out");
        assert!(plan.transitions[0].echo_tail_sec > 0.0);

        let weird = r#"{"transitions":[{"fromTrackId":"a","toTrackId":"b","type":"teleport"}]}"#;
        let plan2 =
            parse_ai_plan_response(weird, &playlist_id, &tracks, &analyses, 12.0, "club_blend")
                .expect("should parse");
        assert_eq!(plan2.transitions[0].kind, "blend");
    }

    // -- DB/job plumbing ------------------------------------------------------

    struct MixFixture {
        state: PostScanState,
        dir: PathBuf,
    }

    fn mix_fixture(prefix: &str) -> MixFixture {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mix-worker-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = boogiebox_db::init_db(&dir).unwrap().connection;
        let db: DbPool = std::sync::Arc::new(std::sync::Mutex::new(conn));
        MixFixture {
            state: PostScanState {
                db,
                http_client: reqwest::Client::new(),
                db_folder: Some(dir.clone()),
                cancel: tokio_util::sync::CancellationToken::new(),
            },
            dir,
        }
    }

    #[test]
    fn lock_db_returns_a_usable_connection() {
        let fixture = mix_fixture("lock-db");
        let guard = lock_db(&fixture.state.db).expect("lock succeeds");
        let count: i64 = guard
            .query_row("SELECT COUNT(*) FROM libraries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn mix_output_dir_defaults_to_db_folder_subdir_when_unconfigured() {
        let fixture = mix_fixture("output-dir-default");
        let dir = get_mix_output_dir_fn(&fixture.state.db, fixture.state.db_folder.as_ref());
        assert_eq!(dir, fixture.dir.join("mix-outputs"));

        // No db_folder at all -> relative default.
        let dir_no_base = get_mix_output_dir_fn(&fixture.state.db, None);
        assert_eq!(dir_no_base, PathBuf::from("mix-outputs"));
    }

    #[test]
    fn mix_output_dir_honors_absolute_configured_override() {
        let fixture = mix_fixture("output-dir-absolute");
        {
            let conn = fixture.state.db.lock().unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES ('boogiemixOutputFolder', ?)",
                rusqlite::params!["D:\\CustomMixOutputs"],
            )
            .unwrap();
        }
        let dir = get_mix_output_dir_fn(&fixture.state.db, fixture.state.db_folder.as_ref());
        assert_eq!(dir, PathBuf::from("D:\\CustomMixOutputs"));
    }

    #[tokio::test]
    async fn wait_for_deep_analysis_times_out_when_features_never_arrive() {
        let fixture = mix_fixture("wait-deep");
        let tracks = vec![input_track("missing-track")];
        let ready =
            wait_for_deep_analysis(&fixture.state, &tracks, Duration::from_millis(50)).await;
        assert_eq!(ready, Ok(0));
    }

    #[tokio::test]
    async fn try_process_next_is_a_noop_on_an_empty_job_queue() {
        let fixture = mix_fixture("try-process-next-empty");
        // No mix jobs queued at all -> Ok(()) without touching anything else.
        let result = try_process_next(&fixture.state).await;
        assert!(result.is_ok());
    }

    #[test]
    fn deep_with_debug_collects_ranked_candidates_and_marks_the_chosen_one() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.1, 0.8, 0.2);
        let to_deep = deep_for("b", 0.1, 0.8, 0.2);
        let preset = style_preset("club_blend");
        let mut sink: Vec<TransitionCandidateLocal> = Vec::new();

        let trans = select_transition_deep_with_debug(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            8.0,
            Some(&mut sink),
        );

        assert!(trans.deep_used);
        assert!(!sink.is_empty());
        assert!(sink.iter().any(|c| c.chosen));
        assert!(sink.len() <= TRANSITION_DEBUG_TOP_N);
    }

    #[test]
    fn deep_transition_falls_back_to_synthetic_windows_when_none_match_role_filters() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let mut from_deep = deep_for("a", 0.1, 0.8, 0.2);
        let mut to_deep = deep_for("b", 0.1, 0.8, 0.2);
        // Re-role every transition window so neither the outro nor intro
        // filters in select_transition_deep_with_debug match anything,
        // forcing the synthetic single-window fallback for both sides.
        for w in &mut from_deep.transition_windows {
            w.role = "verse".into();
        }
        for w in &mut to_deep.transition_windows {
            w.role = "verse".into();
        }

        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            8.0,
        );

        // Synthetic fallback windows still carry deep=true since from_deep/
        // to_deep are Some and pass the rhythmic-evidence gate.
        assert!(trans.deep_used);
    }

    #[test]
    fn safe_mix_deep_transition_uses_safe_crossfade_kind() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        let from_deep = deep_for("a", 0.1, 0.8, 0.2);
        let to_deep = deep_for("b", 0.1, 0.8, 0.2);
        let preset = style_preset("safe_mix");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "safe_mix",
            &preset,
            8.0,
        );
        assert_eq!(trans.kind, "safe_crossfade");
    }

    #[test]
    fn long_build_deep_transition_gets_an_echo_tail_when_vocal_overlap_is_low() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        // Low vocal_risk on both windows -> vocal_overlap < 0.25 -> echo tail applies.
        let from_deep = deep_for("a", 0.05, 0.8, 0.2);
        let to_deep = deep_for("b", 0.05, 0.8, 0.2);
        let preset = style_preset("long_build");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "long_build",
            &preset,
            30.0,
        );
        assert_eq!(trans.kind, "long_build");
        assert!(trans.echo_tail_sec > 0.0);
    }

    #[test]
    fn deep_transition_bass_duck_prefers_measured_bass_rms_over_overlap_heuristic() {
        let from_a = analysis("a", 120.0);
        let to_a = analysis("b", 120.0);
        // bass_risk (also used as bass_rms in the fixture) > 0.3 -> measured branch.
        let from_deep = deep_for("a", 0.1, 0.8, 0.5);
        let to_deep = deep_for("b", 0.1, 0.8, 0.5);
        let preset = style_preset("club_blend");
        let trans = select_transition_deep(
            &from_a,
            &input_track("a"),
            Some(&from_deep),
            &to_a,
            &input_track("b"),
            Some(&to_deep),
            "groove",
            "club_blend",
            &preset,
            8.0,
        );
        assert!((trans.bass_duck - 0.5).abs() < 1e-9);
    }

    #[test]
    fn reorder_for_curve_handles_small_playlists_and_appends_leftovers_in_score_order() {
        let tracks: Vec<MixTrackInput> = (0..5).map(|i| input_track(&format!("t{i}"))).collect();
        let mut analyses: Vec<TrackMixAnalysis> = (0..5)
            .map(|i| analysis(&format!("t{i}"), 120.0 + i as f64))
            .collect();
        // Vary loudness so energy_of differs across tracks, exercising the
        // best-match scan inside the slot-filling loop rather than ties.
        for (i, a) in analyses.iter_mut().enumerate() {
            a.loudness_lufs = Some(-20.0 + i as f64 * 2.0);
        }
        let deep_features: HashMap<String, DeepTrackFeatures> = HashMap::new();
        let preset = style_preset("club_blend");
        let (ordered, anthem_pos) = reorder_for_curve(&tracks, &analyses, &deep_features, &preset);
        assert_eq!(ordered.len(), 5);
        // Every original index appears exactly once.
        let mut sorted = ordered.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
        assert!(anthem_pos < 5);
    }
}

//! Defines Rust server support logic for Waveform Map.

use crate::{ffmpeg, DbPool};
use serde::Serialize;
use std::{
    path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

static WAVEFORM_MAP_RUNNING: AtomicBool = AtomicBool::new(false);

/// Public Waveform Map Run Result data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveformMapRunResult {
    /// Documents the Started public API surface.
    pub started: bool,
    /// Documents the In Progress public API surface.
    pub in_progress: bool,
    /// Documents the Reason public API surface.
    pub reason: String,
    /// Documents the Started At public API surface.
    pub started_at: String,
    /// Documents the Finished At public API surface.
    pub finished_at: String,
    /// Documents the Batch Size public API surface.
    pub batch_size: i64,
    /// Documents the Total Missing public API surface.
    pub total_missing: i64,
    /// Documents the Processed public API surface.
    pub processed: i64,
    /// Documents the Generated public API surface.
    pub generated: i64,
    /// Documents the Skipped public API surface.
    pub skipped: i64,
    /// Documents the Errors public API surface.
    pub errors: i64,
}

/// Documents the Start Waveform Map Scheduler public API surface.
pub fn start_waveform_map_scheduler(db: DbPool, cancel: CancellationToken) {
    tokio::spawn(async move {
        run_waveform_map_if_due(&db).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => run_waveform_map_if_due(&db).await,
            }
        }
    });
}

async fn run_waveform_map_if_due(db: &DbPool) {
    let db_check = db.clone();
    let due = tokio::task::spawn_blocking(move || {
        let conn = db_check.lock().unwrap_or_else(|p| p.into_inner());
        let settings = boogiebox_db::playback::get_waveform_settings(&conn);
        if !settings.background_enabled {
            return Ok(false);
        }
        let missing = boogiebox_db::playback::get_waveform_map_status(&conn)?.missing_tracks;
        if missing <= 0 {
            return Ok(false);
        }
        let due = match settings
            .next_run
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            None => true,
            Some(next_run) => {
                conn.query_row("SELECT datetime(?) <= datetime('now')", [next_run], |row| {
                    row.get::<_, i64>(0)
                })? != 0
            }
        };
        Ok::<bool, rusqlite::Error>(due)
    })
    .await;

    match due {
        Ok(Ok(true)) => match run_waveform_map_batch(db.clone(), "scheduled").await {
            Ok(result) => tracing::info!(
                processed = result.processed,
                generated = result.generated,
                skipped = result.skipped,
                errors = result.errors,
                "scheduled waveform map run finished"
            ),
            Err(error) => tracing::warn!("scheduled waveform map run failed: {error}"),
        },
        Ok(Ok(false)) => {}
        Ok(Err(error)) => tracing::warn!("scheduled waveform map due check failed: {error}"),
        Err(error) => tracing::warn!("scheduled waveform map due check task failed: {error}"),
    }
}

/// Documents the Run Waveform Map Batch public API surface.
pub async fn run_waveform_map_batch(
    db: DbPool,
    reason: &str,
) -> Result<WaveformMapRunResult, String> {
    if WAVEFORM_MAP_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(WaveformMapRunResult {
            started: false,
            in_progress: true,
            reason: reason.to_string(),
            started_at: timestamp_now(),
            finished_at: timestamp_now(),
            batch_size: 0,
            total_missing: 0,
            processed: 0,
            generated: 0,
            skipped: 0,
            errors: 0,
        });
    }

    let result = run_waveform_map_batch_inner(db, reason).await;
    WAVEFORM_MAP_RUNNING.store(false, Ordering::SeqCst);
    result
}

async fn run_waveform_map_batch_inner(
    db: DbPool,
    reason: &str,
) -> Result<WaveformMapRunResult, String> {
    let started_at = timestamp_now();
    let setup = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let settings = boogiebox_db::playback::get_waveform_settings(&conn);
            let missing = boogiebox_db::playback::get_waveform_map_status(&conn)?.missing_tracks;
            let tracks =
                boogiebox_db::playback::list_tracks_missing_waveforms(&conn, settings.batch_size)?;
            Ok::<_, rusqlite::Error>((settings, missing, tracks))
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let (settings, total_missing, tracks) = setup;
    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    let batch_size = settings.batch_size;
    let mut processed = 0_i64;
    let mut generated = 0_i64;
    let mut skipped = 0_i64;
    let mut errors = 0_i64;

    for track in tracks {
        processed += 1;
        let file_path = path::Path::new(&track.file_path).to_path_buf();
        if std::fs::File::open(&file_path).is_err() {
            skipped += 1;
            tracing::warn!(track_id = %track.id, "waveform map skipped unreadable file");
            continue;
        }

        match ffmpeg::generate_waveform(&ffmpeg_path, &file_path).await {
            Ok(peaks) => {
                let json = serde_json::to_string(&peaks).unwrap_or_else(|_| "[]".into());
                let len = peaks.len() as i64;
                let track_id = track.id.clone();
                let duration = track.duration;
                let db_save = db.clone();
                match tokio::task::spawn_blocking(move || {
                    let conn = db_save.lock().unwrap_or_else(|p| p.into_inner());
                    boogiebox_db::playback::save_track_waveform(
                        &conn, &track_id, len, duration, &json,
                    )
                })
                .await
                {
                    Ok(Ok(())) => generated += 1,
                    _ => errors += 1,
                }
            }
            Err(error) => {
                errors += 1;
                tracing::warn!(track_id = %track.id, error = %error, "waveform map generation failed");
            }
        }
    }

    let frequency_hours = settings.frequency_hours;
    let _ = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::playback::mark_waveform_map_run_complete(&conn, frequency_hours)
        }
    })
    .await;

    Ok(WaveformMapRunResult {
        started: true,
        in_progress: false,
        reason: reason.to_string(),
        started_at,
        finished_at: timestamp_now(),
        batch_size,
        total_missing,
        processed,
        generated,
        skipped,
        errors,
    })
}

fn timestamp_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

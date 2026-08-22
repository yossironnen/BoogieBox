//! Defines Rust server support logic for Bpm Analysis.

use crate::{ffmpeg, DbPool};
use serde::Serialize;
use std::{
    path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const BPM_BATCH_SIZE: i64 = 50;

static BPM_ANALYSIS_RUNNING: AtomicBool = AtomicBool::new(false);

/// Public Bpm Batch Result data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmBatchResult {
    /// Documents the Processed public API surface.
    pub processed: i64,
    /// Documents the Analyzed public API surface.
    pub analyzed: i64,
    /// Documents the Skipped public API surface.
    pub skipped: i64,
    /// Documents the Errors public API surface.
    pub errors: i64,
}

/// Documents the Start Bpm Analysis Scheduler public API surface.
pub fn start_bpm_analysis_scheduler(db: DbPool, cancel: CancellationToken) {
    tokio::spawn(async move {
        run_bpm_analysis_if_due(&db).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => run_bpm_analysis_if_due(&db).await,
            }
        }
    });
}

async fn run_bpm_analysis_if_due(db: &DbPool) {
    let db_check = db.clone();
    let due = tokio::task::spawn_blocking(move || {
        let conn = db_check.lock().unwrap_or_else(|p| p.into_inner());
        let settings = boogiebox_db::playback::get_bpm_settings(&conn);
        if !settings.enabled || !settings.background_enabled {
            return Ok(false);
        }
        let status = boogiebox_db::playback::get_bpm_analysis_status(&conn)?;
        if status.missing_tracks <= 0 {
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
        Ok(Ok(true)) => match run_bpm_analysis_batch(db.clone(), "scheduled").await {
            Ok(result) => tracing::info!(
                processed = result.processed,
                analyzed = result.analyzed,
                skipped = result.skipped,
                errors = result.errors,
                "scheduled BPM analysis run finished"
            ),
            Err(error) => tracing::warn!("scheduled BPM analysis run failed: {error}"),
        },
        Ok(Ok(false)) => {}
        Ok(Err(error)) => tracing::warn!("scheduled BPM analysis due check failed: {error}"),
        Err(error) => tracing::warn!("scheduled BPM analysis due check task failed: {error}"),
    }
}

/// Documents the Run Bpm Analysis Batch public API surface.
pub async fn run_bpm_analysis_batch(db: DbPool, reason: &str) -> Result<BpmBatchResult, String> {
    if BPM_ANALYSIS_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(BpmBatchResult {
            processed: 0,
            analyzed: 0,
            skipped: 0,
            errors: 0,
        });
    }

    tracing::info!(reason, "BPM analysis run started");
    let result = run_bpm_analysis_batch_inner(db).await;
    BPM_ANALYSIS_RUNNING.store(false, Ordering::SeqCst);
    result
}

async fn run_bpm_analysis_batch_inner(db: DbPool) -> Result<BpmBatchResult, String> {
    let setup = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            let settings = boogiebox_db::playback::get_bpm_settings(&conn);
            let tracks = boogiebox_db::playback::list_tracks_missing_bpm(&conn, BPM_BATCH_SIZE)?;
            Ok::<_, rusqlite::Error>((settings, tracks))
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let (settings, tracks) = setup;
    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    let mut processed = 0_i64;
    let mut analyzed = 0_i64;
    let mut skipped = 0_i64;
    let mut errors = 0_i64;

    for track in tracks {
        processed += 1;
        let file_path = path::Path::new(&track.file_path).to_path_buf();
        if !file_path.exists() {
            skipped += 1;
            tracing::warn!(track_id = %track.id, "BPM analysis skipped missing file");
            continue;
        }

        match ffmpeg::detect_bpm(&ffmpeg_path, &file_path).await {
            Ok(Some(bpm)) => {
                let db_save = db.clone();
                let track_id = track.id.clone();
                match tokio::task::spawn_blocking(move || {
                    let conn = db_save.lock().unwrap_or_else(|p| p.into_inner());
                    boogiebox_db::playback::save_track_bpm_detected(
                        &conn,
                        &track_id,
                        bpm,
                        "ffmpeg_onset",
                        0.65,
                    )
                })
                .await
                {
                    Ok(Ok(())) => analyzed += 1,
                    _ => errors += 1,
                }
            }
            Ok(None) => skipped += 1,
            Err(error) => {
                errors += 1;
                tracing::warn!(track_id = %track.id, error = %error, "BPM analysis failed");
            }
        }
    }

    let _ = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            boogiebox_db::playback::mark_bpm_analysis_run_complete(&conn, settings.frequency_hours)
        }
    })
    .await;

    Ok(BpmBatchResult {
        processed,
        analyzed,
        skipped,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::init_db;
    use rusqlite::params;
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;
    use uuid::Uuid;

    /// `BPM_ANALYSIS_RUNNING` is a module-wide static: two tests calling
    /// `run_bpm_analysis_batch` concurrently (the default) could otherwise race
    /// on it, with one seeing "already running" and returning a no-op result.
    /// Every test that drives a batch run holds this (a `tokio::sync::Mutex` so
    /// the guard can be held across `.await`) for its duration.
    static RUN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn temp_db(prefix: &str) -> DbPool {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("bpm-analysis-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Mutex::new(init_db(&dir).unwrap().connection))
    }

    /// Seeds one track missing BPM data, whose file does not exist on disk —
    /// so `run_bpm_analysis_batch_inner` takes the "skipped: missing file"
    /// branch and never spawns a real ffmpeg subprocess (accepted gap; see
    /// wip/server-rust-coverage-gap-plan.md).
    fn seed_track_missing_bpm(db: &DbPool) -> String {
        let conn = db.lock().unwrap();
        let library_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, '/music', 'Lib')",
            params![library_id],
        )
        .unwrap();
        let track_id = Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, title, file_path) \
             VALUES (?, ?, 'Track', '/does/not/exist.mp3')",
            params![track_id, library_id],
        )
        .unwrap();
        track_id
    }

    #[tokio::test]
    async fn run_bpm_analysis_batch_on_empty_library_processes_nothing() {
        let _guard = RUN_LOCK.lock().await;
        let db = temp_db("empty");
        let result = run_bpm_analysis_batch(db, "test").await.unwrap();
        assert_eq!(result.processed, 0);
        assert_eq!(result.analyzed, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.errors, 0);
    }

    #[tokio::test]
    async fn run_bpm_analysis_batch_skips_tracks_missing_from_disk() {
        let _guard = RUN_LOCK.lock().await;
        let db = temp_db("missing-file");
        seed_track_missing_bpm(&db);
        let result = run_bpm_analysis_batch(db.clone(), "test").await.unwrap();
        assert_eq!(result.processed, 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.analyzed, 0);
        assert_eq!(result.errors, 0);

        // The run should have recorded a completion timestamp for the schedule.
        let status = boogiebox_db::playback::get_bpm_analysis_status(&db.lock().unwrap()).unwrap();
        assert!(status.last_run.is_some());
    }

    #[tokio::test]
    async fn run_bpm_analysis_if_due_is_a_no_op_when_disabled_or_nothing_missing() {
        let _guard = RUN_LOCK.lock().await;
        let db = temp_db("if-due-disabled");
        // Fresh DB defaults: bpmAnalysisEnabled=true, bpmBackgroundEnabled=false
        // (see seed_default_settings in boogiebox-db/lib.rs) — background
        // scheduling is off, so this must not run a batch at all.
        run_bpm_analysis_if_due(&db).await;
        let status = boogiebox_db::playback::get_bpm_analysis_status(&db.lock().unwrap()).unwrap();
        assert!(status.last_run.is_none(), "no batch should have run");

        // Enable background scheduling, but with no tracks missing BPM data.
        db.lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value='true' WHERE key='bpmBackgroundEnabled'",
                [],
            )
            .unwrap();
        run_bpm_analysis_if_due(&db).await;
        let status_after =
            boogiebox_db::playback::get_bpm_analysis_status(&db.lock().unwrap()).unwrap();
        assert!(
            status_after.last_run.is_none(),
            "no missing tracks means nothing to run"
        );
    }

    #[tokio::test]
    async fn run_bpm_analysis_if_due_runs_when_enabled_with_missing_tracks_and_no_next_run() {
        let _guard = RUN_LOCK.lock().await;
        let db = temp_db("if-due-runs");
        seed_track_missing_bpm(&db);
        db.lock()
            .unwrap()
            .execute(
                "UPDATE settings SET value='true' WHERE key='bpmBackgroundEnabled'",
                [],
            )
            .unwrap();

        run_bpm_analysis_if_due(&db).await;
        let status = boogiebox_db::playback::get_bpm_analysis_status(&db.lock().unwrap()).unwrap();
        assert!(status.last_run.is_some(), "a due batch should have run");
    }
}

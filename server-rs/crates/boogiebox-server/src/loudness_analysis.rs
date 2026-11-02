//! Background loudness lane (wip/loudness-levelling-plan.md §6): measures tracks without ReplayGain
//! tags, and rendered BoogieMixes, with FFmpeg `ebur128`, one file at a time, then computes album
//! gain for albums whose tracks are all known. The DB lock is held only to pick a batch and to write
//! its results, never during FFmpeg.

use crate::{ffmpeg, lane_failures::LaneFailures, mix_priority_gate, DbPool};
use boogiebox_db::loudness::{self, Measurement};
use std::{
    collections::HashSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Admin switch for the lane (`true` unless set to `false`).
pub const ENABLED_KEY: &str = "loudnessAnalysisEnabled";
const TRACK_BATCH: usize = 50;
const MIX_BATCH: usize = 5;
const ALBUM_BATCH: usize = 200;
const MEASURE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Track ids, or `mix:<id>`, the lane failed on or could not find.
static FAILURES: LaneFailures = LaneFailures::new("loudness");

/// Number of files the lane has given up on since the server started.
pub fn failed_count() -> usize {
    FAILURES.count()
}

/// True while a batch is being measured.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Whether the admin switch is on.
pub fn is_enabled(conn: &rusqlite::Connection) -> bool {
    boogiebox_db::boogiemix::get_setting(conn, ENABLED_KEY).as_deref() != Some("false")
}

/// Starts the lane: one check now, then every minute until `cancel`.
pub fn start_loudness_scheduler(db: DbPool, cancel: CancellationToken) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => run_until_done(&db, &cancel).await,
            }
        }
    });
}

/// Measures batches until nothing is left, the switch is turned off, a mix starts rendering, or
/// the server stops.
pub async fn run_until_done(db: &DbPool, cancel: &CancellationToken) {
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    while !cancel.is_cancelled() && !mix_priority_gate::is_active() {
        match run_batch(db, &ffmpeg_path, cancel).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("loudness measuring stopped: {error}");
                break;
            }
        }
    }
    RUNNING.store(false, Ordering::SeqCst);
}

/// One batch: pick work, measure it without the lock, write it back. Returns how many files it
/// looked at (0 when there was nothing to do).
async fn run_batch(
    db: &DbPool,
    ffmpeg_path: &Path,
    cancel: &CancellationToken,
) -> Result<usize, String> {
    let skip: HashSet<String> = FAILURES.skipped();
    let (tracks, mixes) = blocking(db, move |conn| {
        if !is_enabled(conn) {
            return Ok((Vec::new(), Vec::new()));
        }
        let mix_skip = skip
            .iter()
            .filter_map(|id| id.strip_prefix("mix:").map(str::to_owned))
            .collect();
        Ok((
            loudness::tracks_needing_measurement(conn, TRACK_BATCH, &skip)?,
            loudness::mixes_needing_measurement(conn, MIX_BATCH, &mix_skip)?,
        ))
    })
    .await?;
    if tracks.is_empty() && mixes.is_empty() {
        blocking(db, |conn| loudness::compute_album_gains(conn, ALBUM_BATCH)).await?;
        return Ok(0);
    }

    let mut measured_tracks = Vec::new();
    let mut measured_mixes = Vec::new();
    let work = tracks
        .iter()
        .map(|(id, path)| (id.clone(), path, false))
        .chain(mixes.iter().map(|(id, path)| (id.clone(), path, true)));
    for (id, path, is_mix) in work {
        if cancel.is_cancelled() || mix_priority_gate::is_active() {
            break;
        }
        let key = if is_mix {
            format!("mix:{id}")
        } else {
            id.clone()
        };
        let file = Path::new(path);
        if !file.is_file() {
            FAILURES.note_missing(&key);
            continue;
        }
        match ffmpeg::measure_loudness(ffmpeg_path, file, MEASURE_TIMEOUT).await {
            Ok(m) if is_mix => measured_mixes.push((id, m)),
            Ok(m) => measured_tracks.push((id, m)),
            Err(error) => {
                tracing::debug!(id = %key, %error, "loudness measuring failed");
                FAILURES.note_failure(&key, false);
            }
        }
    }

    let looked_at = tracks.len() + mixes.len();
    let (written, _) = blocking(db, move |conn| {
        let written = loudness::record_measurements(conn, &measured_tracks)?;
        for (
            id,
            Measurement {
                integrated_lufs,
                true_peak_dbtp,
                ..
            },
        ) in &measured_mixes
        {
            // A silent render keeps no value; it is not measured again until the next restart.
            match integrated_lufs {
                Some(lufs) => loudness::record_mix_loudness(conn, id, *lufs, *true_peak_dbtp)?,
                None => FAILURES.note_failure(&format!("mix:{id}"), true),
            }
        }
        Ok((written, loudness::compute_album_gains(conn, ALBUM_BATCH)?))
    })
    .await?;
    tracing::info!(looked_at, written, "loudness measuring batch finished");
    Ok(looked_at)
}

async fn blocking<T: Send + 'static>(
    db: &DbPool,
    work: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
) -> Result<T, String> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        work(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    static LANE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn db_with_tracks(paths: &[&str]) -> DbPool {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL,
               updated_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE tracks (id TEXT PRIMARY KEY, album_id TEXT, file_path TEXT,
               file_size INTEGER, duration REAL, added_at TEXT);
             CREATE TABLE mix_outputs (id TEXT PRIMARY KEY, file_path TEXT, created_at TEXT);",
        )
        .unwrap();
        boogiebox_db::loudness::ensure_loudness_schema(&conn).unwrap();
        for (i, path) in paths.iter().enumerate() {
            conn.execute(
                "INSERT INTO tracks VALUES (?1, 'a', ?2, 1, 10, ?1)",
                rusqlite::params![format!("t{i}"), path],
            )
            .unwrap();
        }
        Arc::new(std::sync::Mutex::new(conn))
    }

    #[tokio::test]
    async fn skips_missing_files_without_counting_them_and_stops_when_switched_off() {
        let _guard = LANE_TEST_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        FAILURES.clear();
        let db = db_with_tracks(&["/definitely/missing/a.flac"]);
        run_until_done(&db, &CancellationToken::new()).await;
        assert_eq!(FAILURES.skipped(), HashSet::from(["t0".to_string()]));
        assert_eq!(
            failed_count(),
            0,
            "a missing file is not a failure (the share may be offline)"
        );
        assert!(!is_running());

        FAILURES.clear();
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value) VALUES (?1, 'false')",
                [ENABLED_KEY],
            )
            .unwrap();
        run_until_done(&db, &CancellationToken::new()).await;
        assert!(
            FAILURES.skipped().is_empty(),
            "switched off: nothing is looked at"
        );
    }

    #[tokio::test]
    async fn stands_down_while_a_mix_renders() {
        let _guard = LANE_TEST_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        FAILURES.clear();
        let db = db_with_tracks(&["/definitely/missing/b.flac"]);
        let priority = crate::mix_priority_gate::PriorityAnalysisGuard::acquire();
        run_until_done(&db, &CancellationToken::new()).await;
        assert!(FAILURES.skipped().is_empty());
        drop(priority);
    }

    #[tokio::test]
    async fn measures_a_real_file_when_ffmpeg_is_available() {
        let _guard = LANE_TEST_LOCK.lock().await;
        let _gate = crate::mix_priority_gate::GATE_TEST_LOCK.lock().await;
        FAILURES.clear();
        let ffmpeg_path = ffmpeg::resolve_ffmpeg();
        let dir = std::env::temp_dir().join(format!("boogiebox-loudness-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sine.wav");
        let made = std::process::Command::new(&ffmpeg_path) // nosemgrep: boogiebox-rust-dynamic-command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("sine=frequency=1000:duration=3")
            .arg(&file)
            .status();
        if !matches!(made, Ok(status) if status.success()) {
            eprintln!("ffmpeg not available; skipping the real measurement");
            return;
        }
        let db = db_with_tracks(&[file.to_str().unwrap()]);
        run_until_done(&db, &CancellationToken::new()).await;
        let found =
            boogiebox_db::loudness::loudness_for_tracks(&db.lock().unwrap(), &["t0".into()])
                .unwrap();
        // A 1 kHz sine at FFmpeg's default 1/8 amplitude, dual mono, sits near −18 LUFS.
        let gain = found["t0"].track_gain_db;
        assert!(gain.abs() < 1.5, "gain {gain}");
        assert!(FAILURES.skipped().is_empty());
    }
}

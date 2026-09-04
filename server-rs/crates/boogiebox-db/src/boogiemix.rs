//! Defines SQLite data access and schema helpers for Boogiemix.

use crate::music::{coerce_entity_id, EntityId};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::{NoContext, Timestamp, Uuid};

fn new_id() -> String {
    Uuid::new_v7(Timestamp::now(NoContext)).to_string()
}

pub use crate::jobs::JobError;

// ── Types ────────────────────────────────────────────────────────────────────

/// Public Mix Track Input data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixTrackInput {
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Duration public API surface.
    pub duration: Option<f64>,
    /// Documents the Bpm public API surface.
    pub bpm: Option<f64>,
    /// Documents the Bpm Detected public API surface.
    pub bpm_detected: Option<f64>,
    /// Documents the File Size public API surface.
    pub file_size: Option<i64>,
    /// Documents the Scanned At public API surface.
    pub scanned_at: Option<String>,
    /// Documents the Position public API surface.
    pub position: i64,
}

/// Public Track Mix Analysis data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMixAnalysis {
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the Duration Sec public API surface.
    pub duration_sec: f64,
    /// Documents the Bpm Estimate public API surface.
    pub bpm_estimate: Option<f64>,
    /// Documents the Loudness Lufs public API surface.
    pub loudness_lufs: Option<f64>,
    /// Documents the Key Estimate public API surface.
    pub key_estimate: Option<String>,
    /// Documents the Beat Grid Sec public API surface.
    pub beat_grid_sec: Option<f64>,
    /// Documents the Phrase Bars public API surface.
    pub phrase_bars: Option<i64>,
    /// Documents the Intro Start Sec public API surface.
    pub intro_start_sec: f64,
    /// Documents the Intro End Sec public API surface.
    pub intro_end_sec: f64,
    /// Documents the Outro Start Sec public API surface.
    pub outro_start_sec: f64,
    /// Documents the Outro End Sec public API surface.
    pub outro_end_sec: f64,
    /// Documents the Low Energy Start Sec public API surface.
    pub low_energy_start_sec: Option<f64>,
    /// Documents the Low Energy End Sec public API surface.
    pub low_energy_end_sec: Option<f64>,
    /// Documents the High Energy Start Sec public API surface.
    pub high_energy_start_sec: Option<f64>,
    /// Documents the High Energy End Sec public API surface.
    pub high_energy_end_sec: Option<f64>,
    /// Documents the Confidence public API surface.
    pub confidence: f64,
}

/// Public Mix Job Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct MixJobRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Playlist Id public API surface.
    pub playlist_id: EntityId,
    /// Documents the User Id public API surface.
    pub user_id: EntityId,
    /// Documents the Status public API surface.
    pub status: String,
    /// Documents the Progress Percent public API surface.
    pub progress_percent: i64,
    /// Documents the Current Step public API surface.
    pub current_step: String,
    /// Documents the Last Message public API surface.
    pub last_message: Option<String>,
    /// Documents the Default Crossfade Sec public API surface.
    pub default_crossfade_sec: i64,
    /// Documents the Mix Style public API surface.
    pub mix_style: String,
    /// Documents the Mix Quality public API surface.
    pub mix_quality: String,
    /// Documents the Mix Strategy public API surface.
    pub mix_strategy: Option<String>,
    /// Documents the Planner Provider public API surface.
    pub planner_provider: Option<String>,
    /// Documents the Used Deep Analysis public API surface.
    pub used_deep_analysis: bool,
    /// Documents the Deep Analysis Status public API surface.
    pub deep_analysis_status: Option<String>,
    /// Documents the Cancel Requested public API surface.
    pub cancel_requested: bool,
    /// Documents the Output Id public API surface.
    pub output_id: Option<EntityId>,
    /// Documents the Started At public API surface.
    pub started_at: Option<String>,
    /// Documents the Finished At public API surface.
    pub finished_at: Option<String>,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
    /// Documents the Updated At public API surface.
    pub updated_at: Option<String>,
}

/// Public Mix Output Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct MixOutputRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Job Id public API surface.
    pub job_id: EntityId,
    /// Documents the Playlist Id public API surface.
    pub playlist_id: EntityId,
    /// Documents the File Name public API surface.
    pub file_name: String,
    /// Documents the Duration Sec public API surface.
    pub duration_sec: f64,
    /// Documents the File Size Bytes public API surface.
    pub file_size_bytes: i64,
    /// Documents the Format public API surface.
    pub format: String,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
}

/// Public Mix Transition Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct MixTransitionRow {
    /// Documents the Step Index public API surface.
    pub step_index: i64,
    /// Documents the From Track Id public API surface.
    pub from_track_id: EntityId,
    /// Documents the To Track Id public API surface.
    pub to_track_id: EntityId,
    /// Documents the Crossfade Sec public API surface.
    pub crossfade_sec: f64,
    /// Documents the From Outro Start Sec public API surface.
    pub from_outro_start_sec: f64,
    /// Documents the To Intro Start Sec public API surface.
    pub to_intro_start_sec: f64,
    /// Documents the Phrase Aware public API surface.
    pub phrase_aware: bool,
    /// Documents the Reason public API surface.
    pub reason: String,
}

/// Public Mix Job Log Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct MixJobLogRow {
    /// Documents the Level public API surface.
    pub level: String,
    /// Documents the Message public API surface.
    pub message: String,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
}

/// Public Deep Analysis Row data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct DeepAnalysisRow {
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the Analysis Version public API surface.
    pub analysis_version: i64,
    /// Documents the Analysis Schema Version public API surface.
    pub analysis_schema_version: i64,
    /// Documents the Demucs Model public API surface.
    pub demucs_model: String,
    /// Documents the Used Gpu public API surface.
    pub used_gpu: bool,
    /// Documents the Stem Feature Json public API surface.
    pub stem_feature_json: String,
    /// Documents the Vocal Windows Json public API surface.
    pub vocal_windows_json: String,
    /// Documents the Drum Windows Json public API surface.
    pub drum_windows_json: String,
    /// Documents the Bass Windows Json public API surface.
    pub bass_windows_json: String,
    /// Documents the Section Json public API surface.
    pub section_json: String,
    /// Documents the Phrase Boundaries Json public API surface.
    pub phrase_boundaries_json: String,
    /// Documents the Intro Outro Refined Json public API surface.
    pub intro_outro_refined_json: String,
    /// Documents the Energy Score Refined public API surface.
    pub energy_score_refined: f64,
    /// Documents the Transition Hints Json public API surface.
    pub transition_hints_json: String,
    /// Documents the Transition Windows Json public API surface.
    pub transition_windows_json: String,
    /// Documents the Confidence public API surface.
    pub confidence: f64,
    /// Documents the Feature Size Bytes public API surface.
    pub feature_size_bytes: i64,
    /// Documents the Source Duration Sec public API surface.
    pub source_duration_sec: Option<f64>,
    /// Documents the Last Used At public API surface.
    pub last_used_at: Option<String>,
    /// Documents the Processing Time Ms public API surface.
    pub processing_time_ms: i64,
}

/// Public Deep Analysis Queue Status data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
pub struct DeepAnalysisQueueStatus {
    /// Documents the Pending public API surface.
    pub pending: i64,
    /// Documents the Running public API surface.
    pub running: i64,
    /// Documents the Failed public API surface.
    pub failed: i64,
    /// Documents the Skipped public API surface.
    pub skipped: i64,
    /// Documents the Done public API surface.
    pub done: i64,
}

/// Per-playlist deep analysis progress counts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDeepAnalysisProgress {
    pub total: i64,
    pub pending: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
    pub skipped: i64,
    pub not_queued: i64,
    /// Tracks with any saved deep-analysis cache row.
    pub analyzed_cached: i64,
    /// Tracks with real (non-synthetic) deep analysis stored.
    pub analyzed_real: i64,
    /// Tracks with saved synthetic fallback rows.
    pub analyzed_fallback: i64,
}

/// Public Deep Analysis Cache Status data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepAnalysisCacheStatus {
    /// Documents the Analyzed Tracks public API surface.
    pub analyzed_tracks: i64,
    /// Documents the Estimated Bytes public API surface.
    pub estimated_bytes: i64,
    /// Documents the Oldest Created At public API surface.
    pub oldest_created_at: Option<String>,
    /// Documents the Newest Created At public API surface.
    pub newest_created_at: Option<String>,
}

/// Public Claimed Deep Analysis Job data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct ClaimedDeepAnalysisJob {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the File Fingerprint public API surface.
    pub file_fingerprint: String,
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the Duration public API surface.
    pub duration: Option<f64>,
    /// Track title for logging.
    pub title: Option<String>,
    /// Track artist for logging.
    pub artist: Option<String>,
    /// Queue priority the job was claimed at; used for preemption decisions.
    pub priority: i64,
}

/// Public Stem Window data shape used by BoogieBox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StemWindow {
    /// Documents the Start public API surface.
    pub start: f64,
    /// Documents the End public API surface.
    pub end: f64,
    /// Documents the Strength public API surface.
    pub strength: f64,
    /// Documents the Average public API surface.
    pub average: f64,
}

/// Public Track Section data shape used by BoogieBox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackSection {
    /// Documents the Kind public API surface.
    pub kind: String,
    /// Documents the Start public API surface.
    pub start: f64,
    /// Documents the End public API surface.
    pub end: f64,
    /// Documents the Confidence public API surface.
    pub confidence: f64,
    /// Documents the Vocal Density public API surface.
    pub vocal_density: f64,
    /// Documents the Drum Density public API surface.
    pub drum_density: f64,
    /// Documents the Energy public API surface.
    pub energy: f64,
}

/// Public Transition Window data shape used by BoogieBox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TransitionWindow {
    /// Documents the Role public API surface.
    pub role: String,
    /// Documents the Start public API surface.
    pub start: f64,
    /// Documents the End public API surface.
    pub end: f64,
    /// Documents the Score public API surface.
    pub score: f64,
    /// Documents the Vocal Risk public API surface.
    pub vocal_risk: f64,
    /// Documents the Drum Continuity public API surface.
    pub drum_continuity: f64,
    /// Documents the Bass Risk public API surface.
    pub bass_risk: f64,
    /// Documents the Energy public API surface.
    pub energy: f64,
    /// Documents the Recommended Min Crossfade public API surface.
    pub recommended_min_crossfade: f64,
    /// Documents the Recommended Max Crossfade public API surface.
    pub recommended_max_crossfade: f64,
    /// Mean vocals RMS over the first 4s of the window (Phase 1).
    pub vocals_rms: Option<f64>,
    /// Mean drums RMS over the first 4s of the window (Phase 1).
    pub drums_rms: Option<f64>,
    /// Mean bass RMS over the first 4s of the window (Phase 1).
    pub bass_rms: Option<f64>,
    /// Mean other-stem RMS over the first 4s of the window (Phase 1).
    pub other_rms: Option<f64>,
}

/// Public Stem Summary data shape used by BoogieBox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StemSummary {
    /// Documents the Vocal Density public API surface.
    pub vocal_density: f64,
    /// Documents the Drum Density public API surface.
    pub drum_density: f64,
    /// Documents the Bass Density public API surface.
    pub bass_density: f64,
    /// Documents the Other Density public API surface.
    pub other_density: f64,
    /// Documents the Instrumental Ratio public API surface.
    pub instrumental_ratio: f64,
    /// Documents the Has Long Intro public API surface.
    pub has_long_intro: bool,
    /// Documents the Has Long Outro public API surface.
    pub has_long_outro: bool,
}

/// Neural key detection result (Krumhansl-Schmuckler chroma method).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeyNeural {
    /// Musical key name, e.g. "F#".
    pub key: String,
    /// "major" or "minor".
    pub mode: String,
    /// Correlation confidence [0..1].
    pub confidence: f64,
    /// Camelot Wheel code, e.g. "11A".
    pub camelot: Option<String>,
}

/// Vocal-activity-derived mix cue points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VocalCuePoints {
    /// Suggested intro end (first vocal-free gap end), seconds.
    #[serde(alias = "intro_end_sec")]
    pub intro_end_sec: Option<f64>,
    /// Suggested outro start (last vocal-free gap start), seconds.
    #[serde(alias = "outro_start_sec")]
    pub outro_start_sec: Option<f64>,
    /// Detection confidence [0..1]; use only when >= 0.6.
    pub confidence: f64,
}

/// Neural mel-spectrogram energy/mood embedding (Phase 4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NeuralEmbedding {
    /// Perceptual energy from mel-spectrogram log-power, normalized to [0,1].
    #[serde(alias = "energy_neural")]
    pub energy_neural: f64,
    /// Onset-regularity danceability proxy, normalized to [0,1].
    pub danceability: f64,
    /// Valence (future model placeholder).
    pub valence: Option<f64>,
    /// 16-dimensional PCA-projected mel embedding.
    #[serde(alias = "embedding_16d")]
    pub embedding_16d: Vec<f64>,
    /// Model identifier for cache invalidation.
    #[serde(alias = "model_version")]
    pub model_version: String,
}

/// Neural beat grid from madmom (Phase 3).
///
/// The worker writes this object with snake_case keys, so every multi-word
/// field needs an explicit `alias`: without them `rename_all = "camelCase"`
/// silently dropped `bpm_neural` and `phrase_boundaries_neural` on every
/// analysed track, which is why phrase-aware transition snapping never fired.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BeatGrid {
    /// Beat timestamps in seconds, capped at 2000 entries.
    pub beats: Vec<f64>,
    /// Median-derived BPM from neural beat tracking.
    #[serde(alias = "bpm_neural")]
    pub bpm_neural: f64,
    /// Bar-one timestamps. Real downbeats when `downbeats_real` is set,
    /// otherwise every 4th beat — an arbitrary bar phase that must not be
    /// trusted for bar-level alignment.
    pub downbeats: Vec<f64>,
    /// 4-bar phrase boundaries, derived from `downbeats`.
    #[serde(alias = "phrase_boundaries_neural")]
    pub phrase_boundaries_neural: Vec<f64>,
    /// True when `downbeats` came from real downbeat tracking rather than
    /// `beats[::4]`. Absent (false) on analyses cached before downbeat
    /// tracking was added.
    #[serde(alias = "downbeats_real")]
    pub downbeats_real: bool,
}

/// Public Deep Track Features data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct DeepTrackFeatures {
    /// Documents the Track Id public API surface.
    pub track_id: EntityId,
    /// Documents the Analysis Schema Version public API surface.
    pub analysis_schema_version: i64,
    /// Documents the Confidence public API surface.
    pub confidence: f64,
    /// Documents the Energy Refined public API surface.
    pub energy_refined: f64,
    /// Documents the Used Gpu public API surface.
    pub used_gpu: bool,
    /// Documents the Demucs Model public API surface.
    pub demucs_model: String,
    /// Documents the Vocal Windows public API surface.
    pub vocal_windows: Vec<StemWindow>,
    /// Documents the Drum Windows public API surface.
    pub drum_windows: Vec<StemWindow>,
    /// Documents the Bass Windows public API surface.
    pub bass_windows: Vec<StemWindow>,
    /// Documents the Sections public API surface.
    pub sections: Vec<TrackSection>,
    /// Documents the Phrase Boundaries public API surface.
    pub phrase_boundaries: Vec<f64>,
    /// Documents the Transition Windows public API surface.
    pub transition_windows: Vec<TransitionWindow>,
    /// Documents the Bpm Refined public API surface.
    pub bpm_refined: Option<f64>,
    /// Documents the Summary public API surface.
    pub summary: StemSummary,
    /// Neural key detection result (Phase 5).
    pub key_neural: Option<KeyNeural>,
    /// Vocal-activity cue points for intro/outro (Phase 2).
    pub cue_points: Option<VocalCuePoints>,
    /// Neural beat grid from madmom (Phase 3).
    pub beat_grid: Option<BeatGrid>,
    /// Neural mel energy/mood embedding (Phase 4).
    pub neural_embedding: Option<NeuralEmbedding>,
}

const DEEP_ANALYSIS_VERSION: i64 = 1;
/// Documents the MAX DEEP ANALYSIS FEATURE BYTES public API surface.
pub const MAX_DEEP_ANALYSIS_FEATURE_BYTES: usize = 64 * 1024;
/// Documents the DEEP ANALYSIS PRIORITY BACKGROUND public API surface.
pub const DEEP_ANALYSIS_PRIORITY_BACKGROUND: i64 = 10;
/// Documents the DEEP ANALYSIS PRIORITY MANUAL public API surface.
pub const DEEP_ANALYSIS_PRIORITY_MANUAL: i64 = 70;
/// Documents the DEEP ANALYSIS PRIORITY PLAYLIST MIX public API surface.
pub const DEEP_ANALYSIS_PRIORITY_PLAYLIST_MIX: i64 = 90;

// ── Mix Job CRUD ──────────────────────────────────────────────────────────────

/// Documents the Enqueue Mix Job public API surface.
pub fn enqueue_mix_job(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &EntityId,
    crossfade_sec: i64,
    mix_style: &str,
    mix_quality: &str,
) -> Result<EntityId, JobError> {
    let playlist = conn
        .query_row(
            "SELECT id FROM playlists WHERE id=?1 AND user_id=?2",
            params![playlist_id, user_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if playlist.is_none() {
        return Err(JobError::LibraryNotFound); // reuse for "playlist not found"
    }
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id=?1",
        params![playlist_id],
        |r| r.get(0),
    )?;
    if count < 2 {
        return Err(JobError::EmptyFolders); // reuse for "needs at least 2 tracks"
    }
    let job_id = new_id();
    // Must match the route-handler and renderer clamps (4-60s) so UI options up
    // to the 45s "long build" blend aren't silently truncated before storage.
    let crossfade = crossfade_sec.clamp(4, 60);
    let style = if ["chill_blend", "club_blend", "long_build", "safe_mix"].contains(&mix_style) {
        mix_style
    } else {
        "club_blend"
    };
    let quality = if mix_quality == "high_quality" {
        "high_quality"
    } else {
        "standard"
    };
    conn.execute(
        "INSERT INTO mix_jobs(id, playlist_id, user_id, status, progress_percent, current_step,
          default_crossfade_sec, mix_style, mix_quality, cancel_requested)
         VALUES(?1, ?2, ?3, 'pending', 0, 'queued', ?4, ?5, ?6, 0)",
        params![job_id, playlist_id, user_id, crossfade, style, quality],
    )?;
    Ok(coerce_entity_id(&job_id))
}

/// Documents the Claim Next Mix Job public API surface.
pub fn claim_next_mix_job(conn: &Connection) -> Result<Option<EntityId>, JobError> {
    let row: Option<String> = conn
        .query_row(
            "SELECT id FROM mix_jobs WHERE status='pending' AND cancel_requested=0
             ORDER BY id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = row else {
        return Ok(None);
    };
    let changed = conn.execute(
        "UPDATE mix_jobs
         SET status='analyzing', started_at=COALESCE(started_at, datetime('now')),
             current_step='analyzing', updated_at=datetime('now')
         WHERE id=?1 AND status='pending' AND cancel_requested=0",
        params![id],
    )?;
    Ok(if changed > 0 {
        Some(coerce_entity_id(&id))
    } else {
        None
    })
}

/// Documents the Cancel Mix Job public API surface.
pub fn cancel_mix_job(
    conn: &Connection,
    job_id: &EntityId,
    user_id: &EntityId,
) -> Result<bool, JobError> {
    let row: Option<String> = conn
        .query_row(
            "SELECT status FROM mix_jobs WHERE id=?1 AND user_id=?2",
            params![job_id, user_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(status) = row else {
        return Ok(false);
    };
    if matches!(
        status.as_str(),
        "rendering" | "done" | "failed" | "canceled"
    ) {
        return Ok(false);
    }
    conn.execute(
        "UPDATE mix_jobs
         SET cancel_requested=1, status='canceled', current_step='canceled',
             finished_at=datetime('now'), updated_at=datetime('now')
         WHERE id=?1 AND user_id=?2",
        params![job_id, user_id],
    )?;
    Ok(true)
}

/// Documents the Get Mix Job public API surface.
pub fn get_mix_job(
    conn: &Connection,
    job_id: &EntityId,
    user_id: &EntityId,
) -> Result<Option<MixJobRow>, JobError> {
    conn.query_row(
        "SELECT id, playlist_id, user_id, status, progress_percent, current_step, last_message,
                default_crossfade_sec, mix_style, mix_quality, mix_strategy, planner_provider,
                used_deep_analysis, deep_analysis_status, cancel_requested, output_id,
                started_at, finished_at, created_at, updated_at
         FROM mix_jobs WHERE id=?1 AND user_id=?2",
        params![job_id, user_id],
        |r| {
            Ok(MixJobRow {
                id: coerce_entity_id(&r.get::<_, String>(0)?),
                playlist_id: coerce_entity_id(&r.get::<_, String>(1)?),
                user_id: coerce_entity_id(&r.get::<_, String>(2)?),
                status: r.get(3)?,
                progress_percent: r.get(4)?,
                current_step: r.get(5)?,
                last_message: r.get(6)?,
                default_crossfade_sec: r.get(7)?,
                mix_style: r.get(8)?,
                mix_quality: r.get(9)?,
                mix_strategy: r.get(10)?,
                planner_provider: r.get(11)?,
                used_deep_analysis: r.get::<_, i64>(12)? != 0,
                deep_analysis_status: r.get(13)?,
                cancel_requested: r.get::<_, i64>(14)? != 0,
                output_id: r
                    .get::<_, Option<String>>(15)?
                    .map(|s| coerce_entity_id(&s)),
                started_at: r.get(16)?,
                finished_at: r.get(17)?,
                created_at: r.get(18)?,
                updated_at: r.get(19)?,
            })
        },
    )
    .optional()
    .map_err(JobError::Db)
}

/// Returns the most recently created job for a playlist (regardless of
/// status), so a client that lost track of a job id — e.g. after remounting
/// its BoogieMix panel — can reattach to it.
pub fn get_latest_mix_job_for_playlist(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &EntityId,
) -> Result<Option<MixJobRow>, JobError> {
    conn.query_row(
        "SELECT id, playlist_id, user_id, status, progress_percent, current_step, last_message,
                default_crossfade_sec, mix_style, mix_quality, mix_strategy, planner_provider,
                used_deep_analysis, deep_analysis_status, cancel_requested, output_id,
                started_at, finished_at, created_at, updated_at
         FROM mix_jobs WHERE playlist_id=?1 AND user_id=?2
         -- rowid, not created_at: two jobs enqueued within the same second (or
         -- even the same UUIDv7 millisecond, whose low bits aren't guaranteed
         -- monotonic) still insert in a strict, gap-free rowid order.
         ORDER BY rowid DESC LIMIT 1",
        params![playlist_id, user_id],
        |r| {
            Ok(MixJobRow {
                id: coerce_entity_id(&r.get::<_, String>(0)?),
                playlist_id: coerce_entity_id(&r.get::<_, String>(1)?),
                user_id: coerce_entity_id(&r.get::<_, String>(2)?),
                status: r.get(3)?,
                progress_percent: r.get(4)?,
                current_step: r.get(5)?,
                last_message: r.get(6)?,
                default_crossfade_sec: r.get(7)?,
                mix_style: r.get(8)?,
                mix_quality: r.get(9)?,
                mix_strategy: r.get(10)?,
                planner_provider: r.get(11)?,
                used_deep_analysis: r.get::<_, i64>(12)? != 0,
                deep_analysis_status: r.get(13)?,
                cancel_requested: r.get::<_, i64>(14)? != 0,
                output_id: r
                    .get::<_, Option<String>>(15)?
                    .map(|s| coerce_entity_id(&s)),
                started_at: r.get(16)?,
                finished_at: r.get(17)?,
                created_at: r.get(18)?,
                updated_at: r.get(19)?,
            })
        },
    )
    .optional()
    .map_err(JobError::Db)
}

/// Documents the Update Mix Job Progress public API surface.
pub fn update_mix_job_progress(
    conn: &Connection,
    job_id: &EntityId,
    step: &str,
    percent: i64,
    message: Option<&str>,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE mix_jobs SET current_step=?1, progress_percent=?2, last_message=?3,
          updated_at=datetime('now') WHERE id=?4",
        params![step, percent.clamp(0, 100), message, job_id],
    )?;
    Ok(())
}

/// Documents the Set Mix Job Status public API surface.
pub fn set_mix_job_status(
    conn: &Connection,
    job_id: &EntityId,
    status: &str,
    step: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE mix_jobs SET status=?1, current_step=?2, updated_at=datetime('now') WHERE id=?3",
        params![status, step, job_id],
    )?;
    Ok(())
}

/// Documents the Fail Mix Job public API surface.
pub fn fail_mix_job(conn: &Connection, job_id: &EntityId, message: &str) -> Result<(), JobError> {
    conn.execute(
        "UPDATE mix_jobs SET status='failed', current_step='failed', last_message=?1,
          finished_at=datetime('now'), updated_at=datetime('now') WHERE id=?2",
        params![message, job_id],
    )?;
    Ok(())
}

/// Documents the Complete Mix Job public API surface.
pub fn complete_mix_job(
    conn: &Connection,
    job_id: &EntityId,
    output_id: &EntityId,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE mix_jobs SET status='done', current_step='done', progress_percent=100,
          output_id=?1, finished_at=datetime('now'), updated_at=datetime('now') WHERE id=?2",
        params![output_id, job_id],
    )?;
    Ok(())
}

/// Documents the Update Mix Job Plan Info public API surface.
pub fn update_mix_job_plan_info(
    conn: &Connection,
    job_id: &EntityId,
    strategy: &str,
    provider: &str,
    used_deep: bool,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE mix_jobs SET mix_strategy=?1, planner_provider=?2, used_deep_analysis=?3,
          deep_analysis_status=?4, updated_at=datetime('now') WHERE id=?5",
        params![
            strategy,
            provider,
            if used_deep { 1i64 } else { 0i64 },
            if used_deep {
                "used"
            } else {
                "fallback_standard"
            },
            job_id
        ],
    )?;
    Ok(())
}

/// Documents the Reset Orphaned Mix Jobs public API surface.
pub fn reset_orphaned_mix_jobs(conn: &Connection) -> Result<usize, JobError> {
    Ok(conn.execute(
        "UPDATE mix_jobs SET status='failed', current_step='failed',
          last_message='Server restarted during mix job', finished_at=datetime('now'),
          updated_at=datetime('now')
         WHERE status IN ('analyzing','planning','rendering')",
        [],
    )?)
}

/// Documents the Is Mix Job Canceled public API surface.
pub fn is_mix_job_canceled(conn: &Connection, job_id: &EntityId) -> Result<bool, JobError> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT cancel_requested FROM mix_jobs WHERE id=?1",
            params![job_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v.unwrap_or(0) != 0)
}

// ── Mix Job Logs ──────────────────────────────────────────────────────────────

/// Documents the Append Mix Job Log public API surface.
pub fn append_mix_job_log(
    conn: &Connection,
    job_id: &EntityId,
    level: &str,
    message: &str,
) -> Result<(), JobError> {
    conn.execute(
        "INSERT INTO mix_job_logs(job_id, level, message) VALUES(?1, ?2, ?3)",
        params![job_id, level, message],
    )?;
    Ok(())
}

/// Documents the Get Mix Job Logs public API surface.
pub fn get_mix_job_logs(
    conn: &Connection,
    job_id: &EntityId,
) -> Result<Vec<MixJobLogRow>, JobError> {
    let mut stmt = conn.prepare(
        "SELECT level, message, created_at FROM mix_job_logs
         WHERE job_id=?1 ORDER BY id ASC LIMIT 200",
    )?;
    let rows = stmt.query_map(params![job_id], |r| {
        Ok(MixJobLogRow {
            level: r.get(0)?,
            message: r.get(1)?,
            created_at: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── Mix Transitions ───────────────────────────────────────────────────────────

/// Documents the Persist Mix Transitions public API surface.
pub fn persist_mix_transitions(
    conn: &Connection,
    job_id: &EntityId,
    transitions: &[MixTransitionRow],
) -> Result<(), JobError> {
    conn.execute(
        "DELETE FROM mix_transitions WHERE job_id=?1",
        params![job_id],
    )?;
    for t in transitions {
        conn.execute(
            "INSERT INTO mix_transitions(
               job_id, step_index, from_track_id, to_track_id, crossfade_sec,
               from_outro_start_sec, to_intro_start_sec, phrase_aware, reason)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                job_id,
                t.step_index,
                t.from_track_id,
                t.to_track_id,
                t.crossfade_sec,
                t.from_outro_start_sec,
                t.to_intro_start_sec,
                if t.phrase_aware { 1i64 } else { 0i64 },
                t.reason,
            ],
        )?;
    }
    Ok(())
}

/// Documents the Get Mix Transitions public API surface.
pub fn get_mix_transitions(
    conn: &Connection,
    job_id: &EntityId,
) -> Result<Vec<MixTransitionRow>, JobError> {
    let mut stmt = conn.prepare(
        "SELECT step_index, from_track_id, to_track_id, crossfade_sec,
                from_outro_start_sec, to_intro_start_sec, phrase_aware, reason
         FROM mix_transitions WHERE job_id=?1 ORDER BY step_index ASC",
    )?;
    let rows = stmt.query_map(params![job_id], |r| {
        Ok(MixTransitionRow {
            step_index: r.get(0)?,
            from_track_id: coerce_entity_id(&r.get::<_, String>(1)?),
            to_track_id: coerce_entity_id(&r.get::<_, String>(2)?),
            crossfade_sec: r.get(3)?,
            from_outro_start_sec: r.get(4)?,
            to_intro_start_sec: r.get(5)?,
            phrase_aware: r.get::<_, i64>(6)? != 0,
            reason: r.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── BoogieMix Plans ───────────────────────────────────────────────────────────

/// Documents the Get Latest Mix Plan public API surface.
pub fn get_latest_mix_plan(
    conn: &Connection,
    playlist_id: &EntityId,
) -> Result<Option<String>, JobError> {
    conn.query_row(
        "SELECT normalized_plan FROM boogiemix_plans
         WHERE playlist_id=?1 AND validation_result='valid'
         ORDER BY id DESC LIMIT 1",
        params![playlist_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(JobError::Db)
}

/// Documents the Persist Mix Plan public API surface.
pub fn persist_mix_plan(
    conn: &Connection,
    playlist_id: &EntityId,
    provider: &str,
    raw_response: Option<&str>,
    normalized_plan: &str,
    validation_result: &str,
) -> Result<(), JobError> {
    conn.execute(
        "INSERT INTO boogiemix_plans(playlist_id, provider, raw_ai_response, normalized_plan, validation_result)
         VALUES(?1,?2,?3,?4,?5)",
        params![playlist_id, provider, raw_response, normalized_plan, validation_result],
    )?;
    Ok(())
}

// ── Mix Outputs ───────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn create_mix_output(
    conn: &Connection,
    job_id: &EntityId,
    playlist_id: &EntityId,
    user_id: &EntityId,
    file_path: &str,
    file_name: &str,
    duration_sec: f64,
    file_size_bytes: u64,
) -> Result<EntityId, JobError> {
    let output_id = new_id();
    conn.execute(
        "INSERT INTO mix_outputs(id, job_id, playlist_id, user_id, file_path, file_name,
          duration_sec, file_size_bytes, format)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'mp3')",
        params![
            output_id,
            job_id,
            playlist_id,
            user_id,
            file_path,
            file_name,
            duration_sec,
            file_size_bytes as i64
        ],
    )?;
    Ok(coerce_entity_id(&output_id))
}

/// Documents the List Mix Outputs public API surface.
pub fn list_mix_outputs(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &EntityId,
) -> Result<Vec<MixOutputRow>, JobError> {
    let mut stmt = conn.prepare(
        "SELECT id, job_id, playlist_id, file_name, duration_sec, file_size_bytes, format, created_at
         FROM mix_outputs WHERE playlist_id=?1 AND user_id=?2 ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(params![playlist_id, user_id], |r| {
        Ok(MixOutputRow {
            id: coerce_entity_id(&r.get::<_, String>(0)?),
            job_id: coerce_entity_id(&r.get::<_, String>(1)?),
            playlist_id: coerce_entity_id(&r.get::<_, String>(2)?),
            file_name: r.get(3)?,
            duration_sec: r.get(4)?,
            file_size_bytes: r.get(5)?,
            format: r.get(6)?,
            created_at: r.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Documents the Get Mix Output File public API surface.
pub fn get_mix_output_file(
    conn: &Connection,
    output_id: &EntityId,
    user_id: &EntityId,
) -> Result<Option<(String, String)>, JobError> {
    conn.query_row(
        "SELECT file_path, file_name FROM mix_outputs WHERE id=?1 AND user_id=?2",
        params![output_id, user_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(JobError::Db)
}

// ── Playlist Tracks for Mix ───────────────────────────────────────────────────

/// Documents the Load Playlist Tracks For Mix public API surface.
pub fn load_playlist_tracks_for_mix(
    conn: &Connection,
    playlist_id: &EntityId,
) -> Result<Vec<MixTrackInput>, JobError> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.file_path, t.title, ar.name, t.duration, t.bpm, t.bpm_detected,
                t.file_size, t.scanned_at, pt.position
         FROM playlist_tracks pt
         JOIN tracks t ON t.id=pt.track_id
         LEFT JOIN artists ar ON ar.id=t.artist_id
         WHERE pt.playlist_id=?1
         ORDER BY pt.position ASC, pt.id ASC",
    )?;
    let rows = stmt.query_map(params![playlist_id], |r| {
        Ok(MixTrackInput {
            track_id: coerce_entity_id(&r.get::<_, String>(0)?),
            file_path: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            duration: r.get(4)?,
            bpm: r.get(5)?,
            bpm_detected: r.get(6)?,
            file_size: r.get(7)?,
            scanned_at: r.get(8)?,
            position: r.get(9)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── Track Mix Analysis ────────────────────────────────────────────────────────

/// Documents the Get Cached Mix Analysis public API surface.
pub fn get_cached_mix_analysis(
    conn: &Connection,
    track_id: &EntityId,
) -> Result<Option<TrackMixAnalysis>, JobError> {
    conn.query_row(
        "SELECT track_id, duration_sec, bpm_estimate, loudness_lufs, key_estimate, beat_grid_sec,
                phrase_bars, intro_start_sec, intro_end_sec, outro_start_sec, outro_end_sec,
                low_energy_start_sec, low_energy_end_sec, high_energy_start_sec, high_energy_end_sec,
                confidence
         FROM track_mix_analysis WHERE track_id=?1",
        params![track_id],
        |r| {
            Ok(TrackMixAnalysis {
                track_id: coerce_entity_id(&r.get::<_, String>(0)?),
                duration_sec: r.get(1)?,
                bpm_estimate: r.get(2)?,
                loudness_lufs: r.get(3)?,
                key_estimate: r.get(4)?,
                beat_grid_sec: r.get(5)?,
                phrase_bars: r.get(6)?,
                intro_start_sec: r.get(7)?,
                intro_end_sec: r.get(8)?,
                outro_start_sec: r.get(9)?,
                outro_end_sec: r.get(10)?,
                low_energy_start_sec: r.get(11)?,
                low_energy_end_sec: r.get(12)?,
                high_energy_start_sec: r.get(13)?,
                high_energy_end_sec: r.get(14)?,
                confidence: r.get(15)?,
            })
        },
    )
    .optional()
    .map_err(JobError::Db)
}

/// Documents the Upsert Mix Analysis public API surface.
pub fn upsert_mix_analysis(conn: &Connection, a: &TrackMixAnalysis) -> Result<(), JobError> {
    conn.execute(
        "INSERT INTO track_mix_analysis(
           track_id, duration_sec, bpm_estimate, loudness_lufs, key_estimate, beat_grid_sec,
           phrase_bars, intro_start_sec, intro_end_sec, outro_start_sec, outro_end_sec,
           low_energy_start_sec, low_energy_end_sec, high_energy_start_sec, high_energy_end_sec,
           confidence, analysis_version, analyzed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,1,datetime('now'))
         ON CONFLICT(track_id) DO UPDATE SET
           duration_sec=excluded.duration_sec, bpm_estimate=excluded.bpm_estimate,
           loudness_lufs=excluded.loudness_lufs, key_estimate=excluded.key_estimate,
           beat_grid_sec=excluded.beat_grid_sec, phrase_bars=excluded.phrase_bars,
           intro_start_sec=excluded.intro_start_sec, intro_end_sec=excluded.intro_end_sec,
           outro_start_sec=excluded.outro_start_sec, outro_end_sec=excluded.outro_end_sec,
           low_energy_start_sec=excluded.low_energy_start_sec,
           low_energy_end_sec=excluded.low_energy_end_sec,
           high_energy_start_sec=excluded.high_energy_start_sec,
           high_energy_end_sec=excluded.high_energy_end_sec,
           confidence=excluded.confidence, analysis_version=1, analyzed_at=datetime('now')",
        params![
            a.track_id,
            a.duration_sec,
            a.bpm_estimate,
            a.loudness_lufs,
            a.key_estimate,
            a.beat_grid_sec,
            a.phrase_bars,
            a.intro_start_sec,
            a.intro_end_sec,
            a.outro_start_sec,
            a.outro_end_sec,
            a.low_energy_start_sec,
            a.low_energy_end_sec,
            a.high_energy_start_sec,
            a.high_energy_end_sec,
            a.confidence,
        ],
    )?;
    Ok(())
}

// ── Deep Analysis ─────────────────────────────────────────────────────────────

/// Documents the Get Deep Analysis Queue Status public API surface.
pub fn get_deep_analysis_queue_status(
    conn: &Connection,
) -> Result<DeepAnalysisQueueStatus, JobError> {
    conn.query_row(
        "SELECT
           SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END),
           SUM(CASE WHEN status='running' THEN 1 ELSE 0 END),
           SUM(CASE WHEN status='failed'  THEN 1 ELSE 0 END),
           SUM(CASE WHEN status='skipped' THEN 1 ELSE 0 END),
           SUM(CASE WHEN status='done'    THEN 1 ELSE 0 END)
         FROM deep_analysis_jobs",
        [],
        |r| {
            Ok(DeepAnalysisQueueStatus {
                pending: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                running: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                failed: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                skipped: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                done: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            })
        },
    )
    .map_err(JobError::Db)
}

/// Documents the Get Deep Analysis Cache Status public API surface.
pub fn get_deep_analysis_cache_status(
    conn: &Connection,
) -> Result<DeepAnalysisCacheStatus, JobError> {
    conn.query_row(
        "SELECT
           COUNT(*),
           COALESCE(SUM(feature_size_bytes), 0),
           MIN(COALESCE(last_used_at, created_at)),
           MAX(COALESCE(last_used_at, created_at))
         FROM track_deep_analysis",
        [],
        |r| {
            Ok(DeepAnalysisCacheStatus {
                analyzed_tracks: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                estimated_bytes: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                oldest_created_at: r.get(2)?,
                newest_created_at: r.get(3)?,
            })
        },
    )
    .map_err(JobError::Db)
}

/// Returns per-playlist deep analysis progress counts.
pub fn get_playlist_deep_analysis_progress(
    conn: &Connection,
    playlist_id: &EntityId,
) -> Result<PlaylistDeepAnalysisProgress, JobError> {
    let row = conn
        .query_row(
            "SELECT
           COUNT(DISTINCT pt.track_id),
           COUNT(DISTINCT CASE WHEN daj.status='pending' THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN daj.status='running' THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN daj.status='done'    THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN daj.status='failed'  THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN daj.status='skipped' THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN daj.track_id IS NULL THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN tda.track_id IS NOT NULL THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN tda.confidence > 0.25 THEN pt.track_id END),
           COUNT(DISTINCT CASE WHEN tda.track_id IS NOT NULL AND tda.confidence <= 0.25 THEN pt.track_id END)
         FROM playlist_tracks pt
         LEFT JOIN deep_analysis_jobs daj ON daj.track_id = pt.track_id
         LEFT JOIN track_deep_analysis tda ON tda.track_id = pt.track_id
         WHERE pt.playlist_id = ?1",
            params![playlist_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            },
        )
        .map_err(JobError::Db)?;
    Ok(PlaylistDeepAnalysisProgress {
        total: row.0,
        pending: row.1,
        running: row.2,
        done: row.3,
        failed: row.4,
        skipped: row.5,
        not_queued: row.6,
        analyzed_cached: row.7,
        analyzed_real: row.8,
        analyzed_fallback: row.9,
    })
}

fn compact_feature_size(json_fields: &[&str]) -> usize {
    json_fields.iter().map(|field| field.len()).sum()
}

fn validate_deep_feature_json(label: &str, raw: &str) -> Result<(), JobError> {
    serde_json::from_str::<serde_json::Value>(raw).map_err(|err| {
        JobError::Db(rusqlite::Error::InvalidParameterName(format!(
            "invalid deep-analysis JSON field {label}: {err}"
        )))
    })?;
    Ok(())
}

/// Resets any jobs left in 'running' state back to 'pending'.
/// Called on server startup to recover from a previous unclean shutdown.
pub fn reset_stale_deep_analysis_jobs(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let changed = conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='pending', started_at=NULL, updated_at=datetime('now')
         WHERE status='running'",
        [],
    )?;
    Ok(changed)
}

/// Documents the Claim Next Deep Analysis Job public API surface.
pub fn claim_next_deep_analysis_job(
    conn: &Connection,
) -> Result<Option<ClaimedDeepAnalysisJob>, JobError> {
    claim_next_deep_analysis_job_with_background(conn, true)
}

/// Documents the Claim Next Deep Analysis Job With Background public API surface.
pub fn claim_next_deep_analysis_job_with_background(
    conn: &Connection,
    include_background: bool,
) -> Result<Option<ClaimedDeepAnalysisJob>, JobError> {
    claim_next_deep_analysis_job_filtered(conn, include_background, i64::MIN)
}

/// Claims the next pending job, ignoring anything below `min_priority`.
/// The floor lets the worker keep the machine dedicated to the highest
/// priority tier currently running (for example a user-requested playlist
/// deep analysis) instead of starting lower-priority work alongside it.
pub fn claim_next_deep_analysis_job_filtered(
    conn: &Connection,
    include_background: bool,
    min_priority: i64,
) -> Result<Option<ClaimedDeepAnalysisJob>, JobError> {
    let row: Option<String> = conn
        .query_row(
            "SELECT id FROM deep_analysis_jobs
             WHERE status='pending'
               AND (?1 OR priority > ?2)
               AND priority >= ?3
             ORDER BY priority DESC, id ASC
             LIMIT 1",
            params![
                include_background,
                DEEP_ANALYSIS_PRIORITY_BACKGROUND,
                min_priority
            ],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = row else {
        return Ok(None);
    };
    let changed = conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='running', started_at=COALESCE(started_at, datetime('now')),
             updated_at=datetime('now')
         WHERE id=?1 AND status='pending'",
        params![id],
    )?;
    if changed == 0 {
        return Ok(None);
    }

    let claimed = conn
        .query_row(
            "SELECT j.id, j.track_id, j.file_fingerprint, t.file_path, t.duration,
                    t.title, ar.name, j.priority
             FROM deep_analysis_jobs j
             JOIN tracks t ON t.id=j.track_id
             LEFT JOIN artists ar ON ar.id=t.artist_id
             WHERE j.id=?1",
            params![id],
            |r| {
                Ok(ClaimedDeepAnalysisJob {
                    id: coerce_entity_id(&r.get::<_, String>(0)?),
                    track_id: coerce_entity_id(&r.get::<_, String>(1)?),
                    file_fingerprint: r.get(2)?,
                    file_path: r.get(3)?,
                    duration: r.get(4)?,
                    title: r.get(5)?,
                    artist: r.get(6)?,
                    priority: r.get(7)?,
                })
            },
        )
        .optional()?;
    if claimed.is_none() {
        conn.execute(
            "UPDATE deep_analysis_jobs
             SET status='failed', error_message='track_missing',
                 finished_at=datetime('now'), updated_at=datetime('now')
             WHERE id=?1",
            params![id],
        )?;
    }
    Ok(claimed)
}

/// Highest priority currently waiting in the deep-analysis queue, if any.
/// Used to decide whether a running lower-priority job should be preempted.
pub fn max_pending_deep_analysis_priority(conn: &Connection) -> Result<Option<i64>, JobError> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT MAX(priority) FROM deep_analysis_jobs WHERE status='pending'",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();
    Ok(value)
}

/// Returns a running job to the pending queue without marking it failed, so a
/// preempted job is retried once the higher-priority work is finished.
pub fn requeue_deep_analysis_job(conn: &Connection, job_id: &EntityId) -> Result<(), JobError> {
    conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='pending', started_at=NULL, error_message=NULL,
             updated_at=datetime('now')
         WHERE id=?1",
        params![job_id],
    )?;
    Ok(())
}

/// Documents the Complete Deep Analysis Job public API surface.
pub fn complete_deep_analysis_job(conn: &Connection, job_id: &EntityId) -> Result<(), JobError> {
    conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='done', error_message=NULL, finished_at=datetime('now'),
             updated_at=datetime('now')
         WHERE id=?1",
        params![job_id],
    )?;
    Ok(())
}

/// Documents the Fail Deep Analysis Job public API surface.
pub fn fail_deep_analysis_job(
    conn: &Connection,
    job_id: &EntityId,
    error: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='failed', error_message=?1, finished_at=datetime('now'),
             updated_at=datetime('now')
         WHERE id=?2",
        params![error, job_id],
    )?;
    Ok(())
}

/// Documents the Skip Deep Analysis Job public API surface.
pub fn skip_deep_analysis_job(
    conn: &Connection,
    job_id: &EntityId,
    reason: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE deep_analysis_jobs
         SET status='skipped', error_message=?1, finished_at=datetime('now'),
             updated_at=datetime('now')
         WHERE id=?2",
        params![reason, job_id],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_deep_analysis(
    conn: &Connection,
    track_id: &EntityId,
    analysis_version: i64,
    analysis_schema_version: i64,
    demucs_model: &str,
    used_gpu: bool,
    file_fingerprint: &str,
    stem_feature_json: &str,
    vocal_windows_json: &str,
    drum_windows_json: &str,
    bass_windows_json: &str,
    section_json: &str,
    phrase_boundaries_json: &str,
    intro_outro_refined_json: &str,
    energy_score_refined: f64,
    transition_hints_json: &str,
    transition_windows_json: &str,
    confidence: f64,
    source_duration_sec: Option<f64>,
    processing_time_ms: i64,
) -> Result<(), JobError> {
    let json_fields = [
        ("stem_feature_json", stem_feature_json),
        ("vocal_windows_json", vocal_windows_json),
        ("drum_windows_json", drum_windows_json),
        ("bass_windows_json", bass_windows_json),
        ("section_json", section_json),
        ("phrase_boundaries_json", phrase_boundaries_json),
        ("intro_outro_refined_json", intro_outro_refined_json),
        ("transition_hints_json", transition_hints_json),
        ("transition_windows_json", transition_windows_json),
    ];
    for (label, raw) in &json_fields {
        validate_deep_feature_json(label, raw)?;
    }
    let feature_size_bytes = compact_feature_size(&json_fields.map(|(_, raw)| raw));
    if feature_size_bytes > MAX_DEEP_ANALYSIS_FEATURE_BYTES {
        return Err(JobError::Db(rusqlite::Error::InvalidParameterName(
            format!("deep-analysis feature payload too large: {feature_size_bytes} bytes"),
        )));
    }

    conn.execute(
        "INSERT INTO track_deep_analysis(
          track_id, analysis_version, analysis_schema_version, demucs_model, used_gpu,
          file_fingerprint, stem_feature_json, vocal_windows_json, drum_windows_json,
          bass_windows_json, section_json, phrase_boundaries_json, intro_outro_refined_json,
          energy_score_refined, transition_hints_json, transition_windows_json, confidence,
          feature_size_bytes, source_duration_sec, processing_time_ms, last_used_at, created_at
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,datetime('now'),datetime('now'))
        ON CONFLICT(track_id) DO UPDATE SET
          analysis_version=excluded.analysis_version,
          analysis_schema_version=excluded.analysis_schema_version,
          demucs_model=excluded.demucs_model,
          used_gpu=excluded.used_gpu,
          file_fingerprint=excluded.file_fingerprint,
          stem_feature_json=excluded.stem_feature_json,
          vocal_windows_json=excluded.vocal_windows_json,
          drum_windows_json=excluded.drum_windows_json,
          bass_windows_json=excluded.bass_windows_json,
          section_json=excluded.section_json,
          phrase_boundaries_json=excluded.phrase_boundaries_json,
          intro_outro_refined_json=excluded.intro_outro_refined_json,
          energy_score_refined=excluded.energy_score_refined,
          transition_hints_json=excluded.transition_hints_json,
          transition_windows_json=excluded.transition_windows_json,
          confidence=excluded.confidence,
          feature_size_bytes=excluded.feature_size_bytes,
          source_duration_sec=excluded.source_duration_sec,
          processing_time_ms=excluded.processing_time_ms,
          last_used_at=excluded.last_used_at,
          created_at=excluded.created_at",
        params![
            track_id,
            analysis_version,
            analysis_schema_version,
            demucs_model,
            if used_gpu { 1i64 } else { 0i64 },
            file_fingerprint,
            stem_feature_json,
            vocal_windows_json,
            drum_windows_json,
            bass_windows_json,
            section_json,
            phrase_boundaries_json,
            intro_outro_refined_json,
            energy_score_refined,
            transition_hints_json,
            transition_windows_json,
            confidence.clamp(0.0, 1.0),
            feature_size_bytes as i64,
            source_duration_sec,
            processing_time_ms,
        ],
    )?;
    Ok(())
}

/// Documents the Build Deep Analysis Fingerprint public API surface.
pub fn build_deep_analysis_fingerprint(track: &MixTrackInput) -> String {
    format!(
        "{}|{}|{}",
        track.file_path,
        track.file_size.unwrap_or(-1),
        track.scanned_at.as_deref().unwrap_or("")
    )
}

/// Documents the Should Skip Deep Analysis public API surface.
pub fn should_skip_deep_analysis(
    track: &MixTrackInput,
    max_duration_secs: Option<f64>,
) -> Option<&'static str> {
    if track.duration.is_some_and(|duration| duration < 45.0) {
        return Some("too_short");
    }
    if let (Some(max), Some(duration)) = (max_duration_secs, track.duration) {
        if max > 0.0 && duration > max {
            return Some("too_long");
        }
    }
    let ext = std::path::Path::new(&track.file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg") => None,
        Some(_) => Some("unsupported_format"),
        None => None,
    }
}

/// Documents the Queue Missing Deep Analysis For Tracks public API surface.
pub fn queue_missing_deep_analysis_for_tracks(
    conn: &Connection,
    tracks: &[MixTrackInput],
    force: bool,
) -> Result<usize, JobError> {
    queue_missing_deep_analysis_for_tracks_with_priority(
        conn,
        tracks,
        force,
        DEEP_ANALYSIS_PRIORITY_PLAYLIST_MIX,
    )
}

/// Documents the Queue Missing Deep Analysis For Tracks With Priority public API surface.
pub fn queue_missing_deep_analysis_for_tracks_with_priority(
    conn: &Connection,
    tracks: &[MixTrackInput],
    force: bool,
    priority: i64,
) -> Result<usize, JobError> {
    let mut queued = 0usize;
    let priority = priority.clamp(DEEP_ANALYSIS_PRIORITY_BACKGROUND, 100);
    for track in tracks {
        if should_skip_deep_analysis(track, None).is_some() {
            continue;
        }
        let fingerprint = build_deep_analysis_fingerprint(track);
        let existing: Option<(i64, String)> = conn
            .query_row(
                "SELECT analysis_version, file_fingerprint
                 FROM track_deep_analysis WHERE track_id=?1",
                params![track.track_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let stale = existing
            .map(|(version, existing_fingerprint)| {
                version != DEEP_ANALYSIS_VERSION || existing_fingerprint != fingerprint
            })
            .unwrap_or(true);
        if force || stale {
            conn.execute(
                "INSERT INTO deep_analysis_jobs(track_id, status, priority, file_fingerprint, created_at, updated_at)
                 VALUES(?1, 'pending', ?2, ?3, datetime('now'), datetime('now'))
                 ON CONFLICT(track_id) DO UPDATE SET
                   status=CASE
                     WHEN deep_analysis_jobs.status='running' THEN deep_analysis_jobs.status
                     ELSE 'pending'
                   END,
                   priority=MAX(deep_analysis_jobs.priority, excluded.priority),
                   file_fingerprint=excluded.file_fingerprint,
                   updated_at=datetime('now')",
                params![track.track_id, priority, fingerprint],
            )?;
            queued += 1;
        }
    }
    Ok(queued)
}

/// Documents the Queue Playlist Deep Analysis public API surface.
pub fn queue_playlist_deep_analysis(
    conn: &Connection,
    playlist_id: &EntityId,
    force: bool,
) -> Result<usize, JobError> {
    let tracks = load_playlist_tracks_for_mix(conn, playlist_id)?;
    // A user-initiated playlist deep analysis outranks library-manual and
    // background work, and preempts anything lower already running.
    queue_missing_deep_analysis_for_tracks_with_priority(
        conn,
        &tracks,
        force,
        DEEP_ANALYSIS_PRIORITY_PLAYLIST_MIX,
    )
}

/// Documents the Queue Library Deep Analysis public API surface.
pub fn queue_library_deep_analysis(
    conn: &Connection,
    library_id: &EntityId,
    force: bool,
) -> Result<usize, JobError> {
    let tracks = load_library_tracks_for_deep_analysis(conn, library_id, None, force)?;
    queue_missing_deep_analysis_for_tracks_with_priority(
        conn,
        &tracks,
        force,
        DEEP_ANALYSIS_PRIORITY_MANUAL,
    )
}

/// Documents the Queue Background Deep Analysis Batch public API surface.
pub fn queue_background_deep_analysis_batch(
    conn: &Connection,
    mode: &str,
    batch_size: usize,
) -> Result<usize, JobError> {
    if mode == "off" || batch_size == 0 {
        return Ok(0);
    }
    let tracks = load_background_tracks_for_deep_analysis(conn, mode, batch_size)?;
    queue_missing_deep_analysis_for_tracks_with_priority(
        conn,
        &tracks,
        false,
        DEEP_ANALYSIS_PRIORITY_BACKGROUND,
    )
}

/// Documents the Clear Deep Analysis Cache public API surface.
pub fn clear_deep_analysis_cache(conn: &Connection) -> Result<(usize, usize), JobError> {
    let deleted_cache = conn.execute("DELETE FROM track_deep_analysis", [])?;
    let deleted_jobs = conn.execute(
        "DELETE FROM deep_analysis_jobs WHERE status IN ('pending','done','failed','skipped')",
        [],
    )?;
    Ok((deleted_cache, deleted_jobs))
}

/// Library tracks eligible for deep analysis.
///
/// `force` decides whether tracks that already hold a current analysis are
/// included. Without it the staleness test lives in this SQL, which means a
/// caller asking `queue_missing_deep_analysis_for_tracks` to force gets an
/// already-empty list on a fully analysed library and silently queues nothing —
/// so the two must be driven from the same flag.
fn load_library_tracks_for_deep_analysis(
    conn: &Connection,
    library_id: &EntityId,
    limit: Option<usize>,
    force: bool,
) -> Result<Vec<MixTrackInput>, JobError> {
    let mut sql = base_deep_track_select(if force {
        "WHERE t.library_id=?1 AND ?2 IS NOT NULL"
    } else {
        "WHERE t.library_id=?1
           AND (tda.track_id IS NULL OR tda.analysis_version != ?2 OR tda.file_fingerprint != (t.file_path || '|' || COALESCE(t.file_size, -1) || '|' || COALESCE(t.scanned_at, '')))"
    });
    append_limit(&mut sql, limit);
    query_deep_track_inputs(conn, &sql, params![library_id, DEEP_ANALYSIS_VERSION])
}

fn load_background_tracks_for_deep_analysis(
    conn: &Connection,
    mode: &str,
    limit: usize,
) -> Result<Vec<MixTrackInput>, JobError> {
    let scope = match mode {
        "playlists_only" => "EXISTS(SELECT 1 FROM playlist_tracks pt WHERE pt.track_id=t.id)",
        "favorites_and_playlists" => {
            "(EXISTS(SELECT 1 FROM playlist_tracks pt WHERE pt.track_id=t.id)
              OR EXISTS(SELECT 1 FROM track_ratings tr WHERE tr.track_id=t.id AND tr.rating >= 4)
              OR EXISTS(SELECT 1 FROM play_history ph WHERE ph.track_id=t.id AND ph.played_at >= datetime('now','-180 days')))"
        }
        "all_music" => "1=1",
        _ => return Ok(Vec::new()),
    };
    let mut sql = base_deep_track_select(&format!(
        "WHERE {scope}
           AND NOT EXISTS(SELECT 1 FROM deep_analysis_jobs j WHERE j.track_id=t.id AND j.status IN ('pending','running'))
           AND (tda.track_id IS NULL OR tda.analysis_version != ?1 OR tda.file_fingerprint != (t.file_path || '|' || COALESCE(t.file_size, -1) || '|' || COALESCE(t.scanned_at, '')))"
    ));
    append_limit(&mut sql, Some(limit));
    query_deep_track_inputs(conn, &sql, params![DEEP_ANALYSIS_VERSION])
}

fn base_deep_track_select(where_clause: &str) -> String {
    format!(
        "SELECT t.id, t.file_path, t.title, ar.name, t.duration, t.bpm, t.bpm_detected,
                t.file_size, t.scanned_at, 0
         FROM tracks t
         LEFT JOIN artists ar ON ar.id=t.artist_id
         LEFT JOIN track_deep_analysis tda ON tda.track_id=t.id
         {where_clause}
         ORDER BY t.scanned_at DESC, t.id ASC"
    )
}

fn append_limit(sql: &mut String, limit: Option<usize>) {
    if let Some(limit) = limit {
        sql.push_str(&format!(" LIMIT {}", limit.min(5000)));
    }
}

fn query_deep_track_inputs<P>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<MixTrackInput>, JobError>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |r| {
        Ok(MixTrackInput {
            track_id: coerce_entity_id(&r.get::<_, String>(0)?),
            file_path: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            duration: r.get(4)?,
            bpm: r.get(5)?,
            bpm_detected: r.get(6)?,
            file_size: r.get(7)?,
            scanned_at: r.get(8)?,
            position: r.get(9)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Documents the Count Deep Analysis Ready public API surface.
pub fn count_deep_analysis_ready(
    conn: &Connection,
    track_ids: &[EntityId],
) -> Result<usize, JobError> {
    if track_ids.is_empty() {
        return Ok(0);
    }
    // IN clause built with parameterized `?` placeholders — no user data in SQL text.
    let placeholders = std::iter::repeat_n("?", track_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql =
        format!("SELECT COUNT(*) FROM track_deep_analysis WHERE track_id IN ({placeholders})");
    let count: i64 = conn.query_row(&sql, rusqlite::params_from_iter(track_ids.iter()), |r| {
        r.get(0)
    })?;
    Ok(count.max(0) as usize)
}

/// Documents the Count Playlist Deep Analysis Ready public API surface.
pub fn count_playlist_deep_analysis_ready(
    conn: &Connection,
    playlist_id: &EntityId,
) -> Result<(i64, i64), JobError> {
    conn.query_row(
        "SELECT
           COUNT(pt.track_id),
           SUM(CASE WHEN tda.track_id IS NOT NULL THEN 1 ELSE 0 END)
         FROM playlist_tracks pt
         JOIN tracks t ON t.id=pt.track_id
         LEFT JOIN track_deep_analysis tda ON tda.track_id=pt.track_id
         WHERE pt.playlist_id=?1",
        params![playlist_id],
        |r| {
            let total = r.get::<_, Option<i64>>(0)?.unwrap_or(0);
            let ready = r.get::<_, Option<i64>>(1)?.unwrap_or(0);
            Ok((ready, total))
        },
    )
    .map_err(JobError::Db)
}

/// Documents the Get Deep Analysis For Track public API surface.
pub fn get_deep_analysis_for_track(
    conn: &Connection,
    track_id: &EntityId,
) -> Result<Option<DeepAnalysisRow>, JobError> {
    conn.query_row(
        "SELECT track_id, analysis_version, analysis_schema_version, demucs_model, used_gpu,
                stem_feature_json, vocal_windows_json, drum_windows_json, bass_windows_json,
                section_json, phrase_boundaries_json, intro_outro_refined_json,
                energy_score_refined, transition_hints_json, transition_windows_json,
                confidence, feature_size_bytes, source_duration_sec, last_used_at, processing_time_ms
         FROM track_deep_analysis WHERE track_id=?1",
        params![track_id],
        |r| {
            Ok(DeepAnalysisRow {
                track_id: coerce_entity_id(&r.get::<_, String>(0)?),
                analysis_version: r.get(1)?,
                analysis_schema_version: r.get(2)?,
                demucs_model: r.get(3)?,
                used_gpu: r.get::<_, i64>(4)? != 0,
                stem_feature_json: r.get(5)?,
                vocal_windows_json: r.get(6)?,
                drum_windows_json: r.get(7)?,
                bass_windows_json: r.get(8)?,
                section_json: r.get(9)?,
                phrase_boundaries_json: r.get(10)?,
                intro_outro_refined_json: r.get(11)?,
                energy_score_refined: r.get(12)?,
                transition_hints_json: r.get(13)?,
                transition_windows_json: r.get(14)?,
                confidence: r.get(15)?,
                feature_size_bytes: r.get(16)?,
                source_duration_sec: r.get(17)?,
                last_used_at: r.get(18)?,
                processing_time_ms: r.get(19)?,
            })
        },
    )
    .optional()
    .map_err(JobError::Db)
}

/// Documents the Load Deep Track Features public API surface.
pub fn load_deep_track_features(
    conn: &Connection,
    track_id: &EntityId,
) -> Result<Option<DeepTrackFeatures>, JobError> {
    let Some(row) = get_deep_analysis_for_track(conn, track_id)? else {
        return Ok(None);
    };
    let vocal_windows: Vec<StemWindow> =
        serde_json::from_str(&row.vocal_windows_json).unwrap_or_default();
    let drum_windows: Vec<StemWindow> =
        serde_json::from_str(&row.drum_windows_json).unwrap_or_default();
    let bass_windows: Vec<StemWindow> =
        serde_json::from_str(&row.bass_windows_json).unwrap_or_default();
    let sections: Vec<TrackSection> = serde_json::from_str(&row.section_json).unwrap_or_default();
    let phrase_boundaries: Vec<f64> =
        serde_json::from_str(&row.phrase_boundaries_json).unwrap_or_default();
    let transition_windows: Vec<TransitionWindow> =
        serde_json::from_str(&row.transition_windows_json).unwrap_or_default();
    let hints: serde_json::Value =
        serde_json::from_str(&row.transition_hints_json).unwrap_or(serde_json::Value::Null);
    let bpm_refined = hints.get("bpmRefined").and_then(|v| v.as_f64());
    let key_neural: Option<KeyNeural> = hints
        .get("keyNeural")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let cue_points: Option<VocalCuePoints> = hints
        .get("cuePoints")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let beat_grid: Option<BeatGrid> = hints
        .get("beatGrid")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let neural_embedding: Option<NeuralEmbedding> = hints
        .get("neuralEmbedding")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let stem: serde_json::Value =
        serde_json::from_str(&row.stem_feature_json).unwrap_or(serde_json::Value::Null);
    let summary: StemSummary = stem
        .get("summary")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    Ok(Some(DeepTrackFeatures {
        track_id: row.track_id,
        analysis_schema_version: row.analysis_schema_version,
        confidence: row.confidence,
        energy_refined: row.energy_score_refined,
        used_gpu: row.used_gpu,
        demucs_model: row.demucs_model,
        vocal_windows,
        drum_windows,
        bass_windows,
        sections,
        phrase_boundaries,
        transition_windows,
        bpm_refined,
        summary,
        key_neural,
        cue_points,
        beat_grid,
        neural_embedding,
    }))
}

/// Documents the Touch Deep Analysis Last Used public API surface.
pub fn touch_deep_analysis_last_used(
    conn: &Connection,
    track_ids: &[EntityId],
) -> Result<(), JobError> {
    if track_ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", track_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    // IN clause built with parameterized `?` placeholders — no user data in SQL text.
    let sql = format!(
        "UPDATE track_deep_analysis SET last_used_at=datetime('now') WHERE track_id IN ({placeholders})"
    );
    conn.execute(&sql, rusqlite::params_from_iter(track_ids.iter()))?;
    Ok(())
}

/// Documents the Get Mix Output Dir From DB public API surface.
pub fn get_mix_output_dir_from_db(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key='boogiemixOutputFolder'",
        [],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|v: String| {
        if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        }
    })
}

/// Public Sonic Fingerprint data shape returned by the per-track fingerprint endpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SonicFingerprintRow {
    /// Documents the Track Id public API surface.
    pub track_id: String,
    /// BPM from the tracks table (detected by FFmpeg/BPM analysis).
    pub bpm_detected: Option<f64>,
    /// Overall energy score (0–1) from deep analysis.
    pub energy_score_refined: f64,
    /// Confidence score (0–1) from deep analysis.
    pub confidence: f64,
    /// Source audio duration in seconds.
    pub source_duration_sec: Option<f64>,
    /// Demucs model used for stem separation.
    pub demucs_model: String,
    /// Whether GPU was used during analysis.
    pub used_gpu: bool,
    /// Schema version of the stored analysis data.
    pub analysis_schema_version: i64,
    /// JSON array of TrackSection objects.
    pub section_json: String,
    /// JSON array of StemWindow objects for vocals.
    pub vocal_windows_json: String,
    /// JSON array of StemWindow objects for drums.
    pub drum_windows_json: String,
    /// JSON array of StemWindow objects for bass.
    pub bass_windows_json: String,
    /// JSON array of TransitionWindow objects.
    pub transition_windows_json: String,
    /// JSON object with introEnd / outroStart timestamps.
    pub intro_outro_refined_json: String,
    /// JSON array of phrase boundary timestamps (seconds).
    pub phrase_boundaries_json: String,
}

/// Returns the sonic fingerprint for the given track, or None if no deep analysis exists.
pub fn get_track_sonic_fingerprint(
    conn: &Connection,
    track_id: &EntityId,
) -> Result<Option<SonicFingerprintRow>, JobError> {
    let id_str = match track_id {
        EntityId::Str(s) => s.clone(),
        EntityId::Int(n) => n.to_string(),
    };
    conn.query_row(
        "SELECT tda.track_id, tda.analysis_schema_version, tda.demucs_model, tda.used_gpu,
                tda.energy_score_refined, tda.confidence, tda.source_duration_sec,
                tda.section_json, tda.vocal_windows_json, tda.drum_windows_json,
                tda.bass_windows_json, tda.transition_windows_json,
                tda.intro_outro_refined_json, tda.phrase_boundaries_json,
                t.bpm_detected
         FROM track_deep_analysis tda
         JOIN tracks t ON t.id = tda.track_id
         WHERE tda.track_id = ?1",
        params![id_str],
        |r| {
            Ok(SonicFingerprintRow {
                track_id: r.get::<_, String>(0)?,
                analysis_schema_version: r.get(1)?,
                demucs_model: r.get(2)?,
                used_gpu: r.get::<_, i64>(3)? != 0,
                energy_score_refined: r.get(4)?,
                confidence: r.get(5)?,
                source_duration_sec: r.get(6)?,
                section_json: r.get(7)?,
                vocal_windows_json: r.get(8)?,
                drum_windows_json: r.get(9)?,
                bass_windows_json: r.get(10)?,
                transition_windows_json: r.get(11)?,
                intro_outro_refined_json: r.get(12)?,
                phrase_boundaries_json: r.get(13)?,
                bpm_detected: r.get(14)?,
            })
        },
    )
    .optional()
    .map_err(JobError::Db)
}

/// Documents the Get Setting public API surface.
pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, path: &str, duration: Option<f64>) -> MixTrackInput {
        MixTrackInput {
            track_id: EntityId::Str(id.to_string()),
            file_path: path.to_string(),
            title: Some("Song".to_string()),
            artist: Some("Artist".to_string()),
            duration,
            bpm: None,
            bpm_detected: None,
            file_size: Some(1234),
            scanned_at: Some("2026-05-24 12:00:00".to_string()),
            position: 0,
        }
    }

    fn setup_deep_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE artists(id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE libraries(id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE tracks(
              id TEXT PRIMARY KEY,
              library_id TEXT DEFAULT 'lib1',
              artist_id TEXT,
              file_path TEXT,
              title TEXT DEFAULT 'Song',
              duration REAL,
              bpm REAL,
              bpm_detected REAL,
              file_size INTEGER DEFAULT 1234,
              scanned_at TEXT DEFAULT '2026-05-24 12:00:00'
            );
            CREATE TABLE playlists(id TEXT PRIMARY KEY);
            CREATE TABLE playlist_tracks(id TEXT PRIMARY KEY, playlist_id TEXT, track_id TEXT, position INTEGER);
            CREATE TABLE track_ratings(user_id TEXT, track_id TEXT, rating REAL);
            CREATE TABLE play_history(id TEXT PRIMARY KEY, user_id TEXT, track_id TEXT, played_at TEXT);
            CREATE TABLE track_deep_analysis (
              track_id TEXT PRIMARY KEY,
              analysis_version INTEGER NOT NULL DEFAULT 1,
              analysis_schema_version INTEGER NOT NULL DEFAULT 1,
              file_fingerprint TEXT NOT NULL DEFAULT '',
              demucs_model TEXT NOT NULL DEFAULT 'htdemucs',
              used_gpu INTEGER NOT NULL DEFAULT 0,
              stem_feature_json TEXT NOT NULL DEFAULT '{}',
              vocal_windows_json TEXT NOT NULL DEFAULT '[]',
              drum_windows_json TEXT NOT NULL DEFAULT '[]',
              bass_windows_json TEXT NOT NULL DEFAULT '[]',
              section_json TEXT NOT NULL DEFAULT '[]',
              phrase_boundaries_json TEXT NOT NULL DEFAULT '[]',
              intro_outro_refined_json TEXT NOT NULL DEFAULT '{}',
              energy_score_refined REAL NOT NULL DEFAULT 0.5,
              transition_hints_json TEXT NOT NULL DEFAULT '{}',
              transition_windows_json TEXT NOT NULL DEFAULT '[]',
              confidence REAL NOT NULL DEFAULT 0.0,
              feature_size_bytes INTEGER NOT NULL DEFAULT 0,
              source_duration_sec REAL,
              processing_time_ms INTEGER NOT NULL DEFAULT 0,
              last_used_at TEXT,
              created_at TEXT
            );
            CREATE TABLE deep_analysis_jobs (
              -- Matches the real schema's random default. A constant here made
              -- the fixture silently unable to hold more than one queued job.
              id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
              track_id TEXT NOT NULL UNIQUE,
              status TEXT NOT NULL DEFAULT 'pending',
              priority INTEGER NOT NULL DEFAULT 50,
              file_fingerprint TEXT NOT NULL DEFAULT '',
              error_message TEXT,
              started_at TEXT,
              finished_at TEXT,
              created_at TEXT,
              updated_at TEXT
            );
            INSERT INTO libraries(id, name) VALUES('lib1', 'Library');
            INSERT INTO playlists(id) VALUES('playlist-1');
            INSERT INTO tracks(id, file_path, duration)
            VALUES('t1', 'D:\Music\one.mp3', 180),('t2', 'D:\Music\two.mp3', 180),('t3', 'D:\Music\three.mp3', 180);
            INSERT INTO playlist_tracks(id, playlist_id, track_id, position)
            VALUES('pt1', 'playlist-1', 't1', 0);
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn queues_missing_deep_analysis_for_supported_stale_tracks() {
        let conn = setup_deep_db();
        let tracks = vec![
            track("t1", "D:\\Music\\one.mp3", Some(180.0)),
            track("t2", "D:\\Music\\short.mp3", Some(30.0)),
            track("t3", "D:\\Music\\clip.txt", Some(180.0)),
        ];

        let queued = queue_missing_deep_analysis_for_tracks(&conn, &tracks, false).unwrap();

        assert_eq!(queued, 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM deep_analysis_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn playlist_mix_priority_outranks_background_priority() {
        let conn = setup_deep_db();
        let tracks = vec![track("t1", "D:\\Music\\one.mp3", Some(180.0))];

        queue_missing_deep_analysis_for_tracks_with_priority(
            &conn,
            &tracks,
            false,
            DEEP_ANALYSIS_PRIORITY_BACKGROUND,
        )
        .unwrap();
        queue_missing_deep_analysis_for_tracks(&conn, &tracks, true).unwrap();

        let priority: i64 = conn
            .query_row(
                "SELECT priority FROM deep_analysis_jobs WHERE track_id='t1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(priority, DEEP_ANALYSIS_PRIORITY_PLAYLIST_MIX);
    }

    #[test]
    fn background_queue_uses_playlist_scope_and_low_priority() {
        let conn = setup_deep_db();

        let queued = queue_background_deep_analysis_batch(&conn, "playlists_only", 10).unwrap();

        assert_eq!(queued, 1);
        let priority: i64 = conn
            .query_row(
                "SELECT priority FROM deep_analysis_jobs WHERE track_id='t1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(priority, DEEP_ANALYSIS_PRIORITY_BACKGROUND);
    }

    #[test]
    fn paused_claim_skips_background_jobs() {
        let conn = setup_deep_db();
        queue_background_deep_analysis_batch(&conn, "playlists_only", 10).unwrap();

        let claimed = claim_next_deep_analysis_job_with_background(&conn, false).unwrap();

        assert!(claimed.is_none());
    }

    /// Marks every track in the fixture library as already analysed with a
    /// current version and a matching fingerprint.
    fn mark_library_analysed(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO track_deep_analysis(track_id, analysis_version, file_fingerprint)
             SELECT t.id, 1,
                    t.file_path || '|' || COALESCE(t.file_size, -1) || '|' || COALESCE(t.scanned_at, '')
             FROM tracks t;",
        )
        .unwrap();
    }

    fn pending_job_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM deep_analysis_jobs", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn library_deep_analysis_queues_nothing_when_everything_is_current() {
        let conn = setup_deep_db();
        mark_library_analysed(&conn);

        let queued = queue_library_deep_analysis(&conn, &coerce_entity_id("lib1"), false).unwrap();

        assert_eq!(queued, 0);
        assert_eq!(pending_job_count(&conn), 0);
    }

    #[test]
    fn forcing_library_deep_analysis_requeues_already_analysed_tracks() {
        // Regression: the force flag was passed to the queueing step but the
        // track query filtered analysed tracks out first, so on a fully
        // analysed library the request reached an empty list and silently
        // queued nothing. Both halves have to be driven from the same flag.
        let conn = setup_deep_db();
        mark_library_analysed(&conn);

        let queued = queue_library_deep_analysis(&conn, &coerce_entity_id("lib1"), true).unwrap();

        assert_eq!(queued, 3, "every track in the library should be re-queued");
        assert_eq!(pending_job_count(&conn), 3);
    }

    #[test]
    fn library_deep_analysis_still_picks_up_stale_tracks_without_forcing() {
        let conn = setup_deep_db();
        mark_library_analysed(&conn);
        conn.execute(
            "UPDATE track_deep_analysis SET file_fingerprint='moved' WHERE track_id='t2'",
            [],
        )
        .unwrap();

        let queued = queue_library_deep_analysis(&conn, &coerce_entity_id("lib1"), false).unwrap();

        assert_eq!(queued, 1);
        let track_id: String = conn
            .query_row("SELECT track_id FROM deep_analysis_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(track_id, "t2");
    }

    #[test]
    fn playlist_deep_analysis_request_outranks_library_manual_work() {
        let conn = setup_deep_db();

        queue_playlist_deep_analysis(&conn, &coerce_entity_id("playlist-1"), true).unwrap();

        let priority: i64 = conn
            .query_row(
                "SELECT priority FROM deep_analysis_jobs WHERE track_id='t1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(priority, DEEP_ANALYSIS_PRIORITY_PLAYLIST_MIX);
        assert!(priority > DEEP_ANALYSIS_PRIORITY_MANUAL);
    }

    #[test]
    fn claim_floor_holds_lower_priority_jobs_until_the_tier_clears() {
        let conn = setup_deep_db();
        conn.execute_batch(
            "INSERT INTO deep_analysis_jobs(id, track_id, status, priority)
             VALUES('job-bg', 't1', 'pending', 10),('job-mix', 't2', 'pending', 90);",
        )
        .unwrap();

        let claimed = claim_next_deep_analysis_job_filtered(&conn, true, 90)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.track_id, coerce_entity_id("t2"));
        assert_eq!(claimed.priority, 90);

        // Nothing else may start while the playlist tier is the running floor.
        assert!(claim_next_deep_analysis_job_filtered(&conn, true, 90)
            .unwrap()
            .is_none());
        // Without the floor the background job is claimable again.
        assert!(claim_next_deep_analysis_job_filtered(&conn, true, i64::MIN)
            .unwrap()
            .is_some());
    }

    #[test]
    fn requeue_returns_a_preempted_job_to_pending_with_its_priority() {
        let conn = setup_deep_db();
        conn.execute_batch(
            "INSERT INTO deep_analysis_jobs(id, track_id, status, priority)
             VALUES('job-bg', 't1', 'pending', 10);",
        )
        .unwrap();
        let claimed = claim_next_deep_analysis_job_filtered(&conn, true, i64::MIN)
            .unwrap()
            .unwrap();

        requeue_deep_analysis_job(&conn, &claimed.id).unwrap();

        let (status, priority, started): (String, i64, Option<String>) = conn
            .query_row(
                "SELECT status, priority, started_at FROM deep_analysis_jobs WHERE id='job-bg'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(priority, 10);
        assert!(started.is_none());
    }

    #[test]
    fn max_pending_priority_reports_the_highest_waiting_tier() {
        let conn = setup_deep_db();
        assert_eq!(max_pending_deep_analysis_priority(&conn).unwrap(), None);
        conn.execute_batch(
            "INSERT INTO deep_analysis_jobs(id, track_id, status, priority)
             VALUES('job-bg', 't1', 'pending', 10),
                   ('job-mix', 't2', 'pending', 90),
                   ('job-run', 't3', 'running', 95);",
        )
        .unwrap();

        assert_eq!(max_pending_deep_analysis_priority(&conn).unwrap(), Some(90));
    }

    #[test]
    fn counts_ready_deep_analysis_rows() {
        let conn = setup_deep_db();
        conn.execute(
            "INSERT INTO track_deep_analysis(track_id, analysis_version, file_fingerprint)
             VALUES('t1', 1, 'x')",
            [],
        )
        .unwrap();

        let ready = count_deep_analysis_ready(
            &conn,
            &[
                EntityId::Str("t1".to_string()),
                EntityId::Str("t2".to_string()),
            ],
        )
        .unwrap();

        assert_eq!(ready, 1);
    }

    #[test]
    fn playlist_progress_counts_cached_real_and_fallback_rows() {
        let conn = setup_deep_db();
        conn.execute_batch(
            r#"
            INSERT INTO playlist_tracks(id, playlist_id, track_id, position)
            VALUES('pt2', 'playlist-1', 't2', 1);
            INSERT INTO track_deep_analysis(track_id, analysis_version, file_fingerprint, confidence)
            VALUES('t1', 1, 'fp-real', 0.82),
                  ('t2', 1, 'fp-fallback', 0.2);
            INSERT INTO deep_analysis_jobs(id, track_id, status, priority, file_fingerprint)
            VALUES('job-real', 't1', 'done', 70, 'fp-real'),
                  ('job-fallback', 't2', 'done', 70, 'fp-fallback');
            "#,
        )
        .unwrap();

        let progress =
            get_playlist_deep_analysis_progress(&conn, &EntityId::Str("playlist-1".into()))
                .unwrap();

        assert_eq!(progress.total, 2);
        assert_eq!(progress.done, 2);
        assert_eq!(progress.analyzed_cached, 2);
        assert_eq!(progress.analyzed_real, 1);
        assert_eq!(progress.analyzed_fallback, 1);
    }

    #[test]
    fn claims_and_completes_deep_analysis_job_with_uuid_track_id() {
        let conn = setup_deep_db();
        let track = track("t1", "D:\\Music\\one.mp3", Some(180.0));
        assert_eq!(
            queue_missing_deep_analysis_for_tracks(&conn, &[track], false).unwrap(),
            1
        );

        let job = claim_next_deep_analysis_job(&conn).unwrap().unwrap();

        assert_eq!(job.track_id, EntityId::Str("t1".to_string()));
        assert_eq!(job.file_path, "D:\\Music\\one.mp3");

        upsert_deep_analysis(
            &conn,
            &job.track_id,
            1,
            2,
            "htdemucs",
            false,
            &job.file_fingerprint,
            "{}",
            "[]",
            "[]",
            "[]",
            "[]",
            "[]",
            "{}",
            0.7,
            "{}",
            "[]",
            0.8,
            Some(180.0),
            123,
        )
        .unwrap();
        complete_deep_analysis_job(&conn, &job.id).unwrap();

        assert_eq!(
            count_deep_analysis_ready(&conn, &[job.track_id]).unwrap(),
            1
        );
        let status = get_deep_analysis_queue_status(&conn).unwrap();
        assert_eq!(status.done, 1);
    }

    #[test]
    fn deep_analysis_queue_status_counts_skipped_jobs() {
        let conn = setup_deep_db();
        conn.execute(
            "INSERT INTO deep_analysis_jobs(track_id, status, priority, file_fingerprint)
             VALUES('track-skipped','skipped',0,'fp')",
            [],
        )
        .unwrap();

        let status = get_deep_analysis_queue_status(&conn).unwrap();

        assert_eq!(status.skipped, 1);
    }

    #[test]
    fn deep_analysis_cache_status_estimates_payload_bytes() {
        let conn = setup_deep_db();
        conn.execute(
            "INSERT INTO track_deep_analysis(
               track_id, analysis_version, file_fingerprint, stem_feature_json,
               vocal_windows_json, drum_windows_json, intro_outro_refined_json,
               transition_hints_json, feature_size_bytes, created_at
             ) VALUES('track-1',1,'fp','{}','[]','[]','{}','{}',10,'2026-05-26 10:00:00')",
            [],
        )
        .unwrap();

        let status = get_deep_analysis_cache_status(&conn).unwrap();

        assert_eq!(status.analyzed_tracks, 1);
        assert!(status.estimated_bytes > 0);
        assert_eq!(
            status.newest_created_at.as_deref(),
            Some("2026-05-26 10:00:00")
        );
    }

    #[test]
    fn upsert_deep_analysis_stores_schema_v2_compact_features() {
        let conn = setup_deep_db();

        upsert_deep_analysis(
            &conn,
            &EntityId::Str("t1".to_string()),
            1,
            2,
            "htdemucs",
            true,
            "fp",
            r#"{"energy":[0.1]}"#,
            r#"[{"start":1,"end":2}]"#,
            "[]",
            r#"[{"start":3,"end":4}]"#,
            r#"[{"kind":"intro","start":0,"end":16}]"#,
            "[0,16,32]",
            "{}",
            0.7,
            "{}",
            r#"[{"role":"intro","start":0,"end":16,"score":0.8}]"#,
            0.9,
            Some(180.0),
            123,
        )
        .unwrap();

        let row = get_deep_analysis_for_track(&conn, &EntityId::Str("t1".to_string()))
            .unwrap()
            .unwrap();

        assert_eq!(row.analysis_schema_version, 2);
        assert_eq!(row.bass_windows_json, r#"[{"start":3,"end":4}]"#);
        assert_eq!(row.section_json, r#"[{"kind":"intro","start":0,"end":16}]"#);
        assert_eq!(row.phrase_boundaries_json, "[0,16,32]");
        assert_eq!(row.confidence, 0.9);
        assert!(row.feature_size_bytes > 0);
        assert_eq!(row.source_duration_sec, Some(180.0));
        assert!(row.last_used_at.is_some());
    }

    #[test]
    fn upsert_deep_analysis_rejects_oversized_feature_payloads() {
        let conn = setup_deep_db();
        let oversized = format!(
            r#"{{"blob":"{}"}}"#,
            "x".repeat(MAX_DEEP_ANALYSIS_FEATURE_BYTES)
        );

        let result = upsert_deep_analysis(
            &conn,
            &EntityId::Str("t1".to_string()),
            1,
            2,
            "htdemucs",
            false,
            "fp",
            &oversized,
            "[]",
            "[]",
            "[]",
            "[]",
            "[]",
            "{}",
            0.5,
            "{}",
            "[]",
            0.5,
            Some(180.0),
            10,
        );

        assert!(result.is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM track_deep_analysis", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn sonic_fingerprint_returns_none_for_unanalyzed_track() {
        let conn = setup_deep_db();
        let track_id = EntityId::Str("t1".to_string());
        let result = get_track_sonic_fingerprint(&conn, &track_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn sonic_fingerprint_returns_row_with_bpm_join() {
        let conn = setup_deep_db();
        // Give t1 a BPM value
        conn.execute("UPDATE tracks SET bpm_detected=128.5 WHERE id='t1'", [])
            .unwrap();
        conn.execute(
            "INSERT INTO track_deep_analysis(
               track_id, analysis_version, analysis_schema_version, file_fingerprint,
               energy_score_refined, confidence, source_duration_sec,
               section_json, vocal_windows_json, drum_windows_json, bass_windows_json,
               transition_windows_json, intro_outro_refined_json, phrase_boundaries_json
             ) VALUES('t1', 1, 2, 'fp1', 0.75, 0.88, 180.0,
               '[{\"kind\":\"intro\"}]', '[{\"start\":0}]', '[]', '[]', '[]', '{}', '[]')",
            [],
        )
        .unwrap();
        let track_id = EntityId::Str("t1".to_string());
        let row = get_track_sonic_fingerprint(&conn, &track_id)
            .unwrap()
            .expect("should have a row");
        assert_eq!(row.track_id, "t1");
        assert!((row.bpm_detected.unwrap() - 128.5).abs() < 0.01);
        assert!((row.energy_score_refined - 0.75).abs() < 0.01);
        assert!((row.confidence - 0.88).abs() < 0.01);
        assert_eq!(row.analysis_schema_version, 2);
        assert!(row.section_json.contains("intro"));
    }

    #[test]
    fn sonic_fingerprint_returns_none_for_unknown_track_id() {
        let conn = setup_deep_db();
        let track_id = EntityId::Str("does-not-exist".to_string());
        let result = get_track_sonic_fingerprint(&conn, &track_id).unwrap();
        assert!(result.is_none());
    }

    fn setup_mix_jobs_db() -> Connection {
        let conn = setup_deep_db();
        conn.execute_batch(
            r#"
            ALTER TABLE playlists ADD COLUMN user_id TEXT;
            UPDATE playlists SET user_id = 'user-1';
            INSERT INTO playlist_tracks(id, playlist_id, track_id, position)
            VALUES('pt2', 'playlist-1', 't2', 1);
            CREATE TABLE mix_jobs(
              id TEXT PRIMARY KEY,
              playlist_id TEXT,
              user_id TEXT,
              status TEXT,
              progress_percent INTEGER,
              current_step TEXT,
              default_crossfade_sec INTEGER,
              mix_style TEXT,
              mix_quality TEXT,
              cancel_requested INTEGER
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn enqueue_mix_job_persists_long_build_45s_crossfade_unclamped() {
        // Regression: enqueue_mix_job used to clamp crossfade_sec to a max of 20,
        // silently truncating the UI's 45s "long build" blend option before it
        // ever reached the renderer (which already supports up to 60s).
        let conn = setup_mix_jobs_db();
        let job_id = enqueue_mix_job(
            &conn,
            &EntityId::Str("playlist-1".into()),
            &EntityId::Str("user-1".into()),
            45,
            "long_build",
            "high_quality",
        )
        .unwrap();
        let stored: i64 = conn
            .query_row(
                "SELECT default_crossfade_sec FROM mix_jobs WHERE id=?1",
                params![job_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 45);
    }

    #[test]
    fn enqueue_mix_job_clamps_crossfade_to_four_to_sixty_range() {
        let conn = setup_mix_jobs_db();
        let job_id = enqueue_mix_job(
            &conn,
            &EntityId::Str("playlist-1".into()),
            &EntityId::Str("user-1".into()),
            9999,
            "club_blend",
            "standard",
        )
        .unwrap();
        let stored: i64 = conn
            .query_row(
                "SELECT default_crossfade_sec FROM mix_jobs WHERE id=?1",
                params![job_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 60);
    }
}

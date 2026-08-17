//! Defines SQLite data access and schema helpers for Jobs.

use crate::music::{coerce_entity_id, EntityId, LibraryRow};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::Serialize;
use uuid::{NoContext, Timestamp, Uuid};

fn new_id() -> String {
    Uuid::new_v7(Timestamp::now(NoContext)).to_string()
}

fn id(raw: &str) -> EntityId {
    coerce_entity_id(raw)
}

fn is_unique_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

/// Public Job Error data shape used by BoogieBox.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("Library name is required")]
    EmptyLibraryName,
    #[error("At least one folder is required")]
    EmptyFolders,
    #[error("Library not found")]
    LibraryNotFound,
    #[error("Folder not found")]
    FolderNotFound,
    #[error("Libraries must keep at least one folder. Remove the library instead.")]
    LastFolder,
    #[error("Library name already exists. Please choose a unique name.")]
    DuplicateLibraryName,
    #[error("This folder is already assigned to another library.")]
    DuplicateFolder,
    #[error("Job not found")]
    JobNotFound,
    #[error("Scan job not found or not active")]
    ScanJobNotActive,
    #[error("Post-scan job not found or not active")]
    PostScanJobNotActive,
    #[error("Post-scan job not found or not pending")]
    PostScanJobNotPending,
    #[error("Post-scan job not found or not retryable")]
    PostScanJobNotRetryable,
    #[error("Unsupported post-scan job type for this library: {0}")]
    UnsupportedPostScanJob(String),
    #[error("Minimum frequency is 0.5 hours (30 minutes)")]
    ScheduleTooFrequent,
    #[error("Scan job was cancelled")]
    ScanCancelled,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// Public Create Library Input data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct CreateLibraryInput {
    /// Documents the Folders public API surface.
    pub folders: Vec<String>,
    /// Documents the Name public API surface.
    pub name: Option<String>,
    /// Documents the Library Type public API surface.
    pub library_type: Option<String>,
    /// Documents the Scanner Profile public API surface.
    pub scanner_profile: Option<String>,
    /// Documents the Metadata Mode public API surface.
    pub metadata_mode: Option<String>,
}

/// Public Scan Job Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ScanJobRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Status public API surface.
    pub status: String,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
    /// Documents the Updated At public API surface.
    pub updated_at: Option<String>,
    /// Documents the Started At public API surface.
    pub started_at: Option<String>,
    /// Documents the Finished At public API surface.
    pub finished_at: Option<String>,
    /// Documents the Files Found public API surface.
    pub files_found: i64,
    /// Documents the Files Scanned public API surface.
    pub files_scanned: i64,
    /// Documents the Errors public API surface.
    pub errors: i64,
    /// Documents the Error Log public API surface.
    pub error_log: Option<String>,
}

/// Public Scan Job Detail data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ScanJobDetail {
    #[serde(flatten)]
    pub job: ScanJobRow,
    /// Documents the Queue Position public API surface.
    pub queue_position: Option<i64>,
    /// Documents the Running Job public API surface.
    pub running_job: Option<RunningJobRow>,
}

/// Public Running Job Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct RunningJobRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Started At public API surface.
    pub started_at: Option<String>,
    /// Documents the Library Name public API surface.
    pub library_name: String,
}

/// Public Schedule Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct ScheduleRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Enabled public API surface.
    pub enabled: i64,
    /// Documents the Frequency Hours public API surface.
    pub frequency_hours: f64,
    /// Documents the Last Run public API surface.
    pub last_run: Option<String>,
    /// Documents the Next Run public API surface.
    pub next_run: Option<String>,
    /// Documents the Created At public API surface.
    pub created_at: Option<String>,
    /// Documents the Updated At public API surface.
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
}

/// Public Queue Snapshot data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct QueueSnapshot {
    /// Documents the Fetched At public API surface.
    pub fetched_at: String,
    /// Documents the Queues public API surface.
    pub queues: QueueGroups,
}

/// Public Queue Groups data shape used by BoogieBox.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueGroups {
    /// Documents the Scan public API surface.
    pub scan: Vec<QueueRow>,
    /// Documents the Post Scan public API surface.
    pub post_scan: Vec<QueueRow>,
    /// Documents the Mix public API surface.
    pub mix: Vec<QueueRow>,
    /// Documents the Deep Analysis public API surface.
    pub deep_analysis: Vec<QueueRow>,
}

/// Public Queue Row data shape used by BoogieBox.
#[derive(Debug, Serialize)]
pub struct QueueRow {
    /// Documents the Id public API surface.
    pub id: EntityId,
    /// Documents the Status public API surface.
    pub status: String,
    /// Documents the Library Id public API surface.
    pub library_id: Option<EntityId>,
    /// Documents the Library Name public API surface.
    pub library_name: Option<String>,
    /// Documents the Job Type public API surface.
    pub job_type: Option<String>,
    /// Documents the Files Scanned public API surface.
    pub files_scanned: Option<i64>,
    /// Documents the Files Found public API surface.
    pub files_found: Option<i64>,
    /// Documents the Errors public API surface.
    pub errors: Option<i64>,
    /// Documents the Started At public API surface.
    pub started_at: Option<String>,
    /// Documents the Finished At public API surface.
    pub finished_at: Option<String>,
    /// Documents the Heartbeat At public API surface.
    pub heartbeat_at: Option<String>,
    /// Documents the Current Step public API surface.
    pub current_step: Option<String>,
    /// Documents the Playlist Name public API surface.
    pub playlist_name: Option<String>,
    /// Documents the Track Title public API surface.
    pub track_title: Option<String>,
    /// Documents the Error Message public API surface.
    pub error_message: Option<String>,
}

/// Public Claimed Scan Job data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct ClaimedScanJob {
    /// Documents the Job Id public API surface.
    pub job_id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Library Type public API surface.
    pub library_type: String,
    /// Documents the Folders public API surface.
    pub folders: Vec<String>,
}

/// Public Post Scan Lane data shape used by BoogieBox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostScanLane {
    Music,
}

const MUSIC_POST_SCAN_JOB_TYPES: &[&str] = &[
    "refresh_library_mappings",
    "cache_album_images",
    "cache_artist_images",
    "warm_lastfm_artist_info",
    "warm_lastfm_album_info",
    "warm_track_lyrics",
    "sync_artist_styles",
    "sync_discogs_album_metadata",
];

/// Public Claimed Post Scan Job data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct ClaimedPostScanJob {
    /// Documents the Job Id public API surface.
    pub job_id: EntityId,
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the Job Type public API surface.
    pub job_type: String,
    /// Documents the Payload public API surface.
    pub payload: Option<String>,
}

/// Public Scanned Track Input data shape used by BoogieBox.
#[derive(Debug, Clone)]
pub struct ScannedTrackInput {
    /// Documents the Library Id public API surface.
    pub library_id: EntityId,
    /// Documents the File Path public API surface.
    pub file_path: String,
    /// Documents the File Name public API surface.
    pub file_name: String,
    /// Documents the File Size public API surface.
    pub file_size: i64,
    /// Documents the Format public API surface.
    pub format: String,
    /// Documents the Title public API surface.
    pub title: String,
    /// Documents the Artist public API surface.
    pub artist: String,
    /// Documents the Album public API surface.
    pub album: String,
    /// Documents the Album Artist public API surface.
    pub album_artist: String,
    /// Documents the Genre public API surface.
    pub genre: String,
    /// Documents the Composer public API surface.
    pub composer: String,
    /// Documents the Track Number public API surface.
    pub track_number: Option<i64>,
    /// Documents the Disc Number public API surface.
    pub disc_number: Option<i64>,
    /// Documents the Year public API surface.
    pub year: Option<i64>,
    /// Documents the Comment public API surface.
    pub comment: Option<String>,
    /// Documents the Bpm public API surface.
    pub bpm: Option<i64>,
    /// Documents the Duration public API surface.
    pub duration: f64,
    /// Documents the Bitrate public API surface.
    pub bitrate: Option<i64>,
    /// Documents the Sample Rate public API surface.
    pub sample_rate: Option<i64>,
    /// Documents the Channels public API surface.
    pub channels: Option<i64>,
}

/// Documents the Create Library public API surface.
pub fn create_library(
    conn: &Connection,
    input: CreateLibraryInput,
) -> Result<LibraryRow, JobError> {
    let folders = normalize_folders(input.folders);
    if folders.is_empty() {
        return Err(JobError::EmptyFolders);
    }
    let primary_path = folders[0].clone();
    let name = normalize_library_name(
        input
            .name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| path_display_name(&primary_path)),
    );
    if name.is_empty() {
        return Err(JobError::EmptyLibraryName);
    }
    if is_library_name_taken(conn, &name, None)? {
        return Err(JobError::DuplicateLibraryName);
    }

    let library_id = new_id();
    let library_type = parse_library_type(input.library_type.as_deref());
    let scanner_profile = defaulted(input.scanner_profile.as_deref(), "default");
    let metadata_mode = defaulted(input.metadata_mode.as_deref(), "path_only");
    conn.execute(
        "INSERT INTO libraries(id, path, name, library_type, scanner_profile, metadata_mode)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            library_id,
            primary_path,
            name,
            library_type,
            scanner_profile,
            metadata_mode
        ],
    )
    .map_err(|err| {
        if is_unique_error(&err) {
            JobError::DuplicateFolder
        } else {
            JobError::Db(err)
        }
    })?;

    for (position, folder) in folders.iter().enumerate() {
        insert_library_folder(conn, &id(&library_id), folder, position as i64)?;
    }
    upsert_schedule(conn, &id(&library_id), true, 24.0)?;
    get_library(conn, &id(&library_id))?.ok_or(JobError::LibraryNotFound)
}

/// Documents the Rename Library public API surface.
pub fn rename_library(
    conn: &Connection,
    library_id: &str,
    name: &str,
) -> Result<LibraryRow, JobError> {
    let library_id = id(library_id);
    let next_name = normalize_library_name(name);
    if next_name.is_empty() {
        return Err(JobError::EmptyLibraryName);
    }
    if is_library_name_taken(conn, &next_name, Some(&library_id))? {
        return Err(JobError::DuplicateLibraryName);
    }
    let changed = conn.execute(
        "UPDATE libraries SET name=?1 WHERE id=?2",
        params![next_name, library_id],
    )?;
    if changed == 0 {
        return Err(JobError::LibraryNotFound);
    }
    get_library(conn, &library_id)?.ok_or(JobError::LibraryNotFound)
}

/// Documents the Delete Library public API surface.
pub fn delete_library(conn: &Connection, library_id: &str) -> Result<(), JobError> {
    conn.execute("DELETE FROM libraries WHERE id=?1", [id(library_id)])?;
    Ok(())
}

/// Documents the Add Library Folder public API surface.
pub fn add_library_folder(
    conn: &Connection,
    library_id: &str,
    path: &str,
) -> Result<LibraryRow, JobError> {
    let library_id = id(library_id);
    let Some(_) = get_library(conn, &library_id)? else {
        return Err(JobError::LibraryNotFound);
    };
    let normalized = normalize_path(path);
    if normalized.is_empty() {
        return Err(JobError::EmptyFolders);
    }
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM library_folders WHERE library_id=?1",
        [&library_id],
        |row| row.get(0),
    )?;
    insert_library_folder(conn, &library_id, &normalized, position)?;
    get_library(conn, &library_id)?.ok_or(JobError::LibraryNotFound)
}

/// Documents the Remove Library Folder public API surface.
pub fn remove_library_folder(
    conn: &Connection,
    library_id: &str,
    folder_id: &str,
) -> Result<LibraryRow, JobError> {
    let library_id = id(library_id);
    let folder_id = id(folder_id);
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM library_folders WHERE library_id=?1",
        [&library_id],
        |row| row.get(0),
    )?;
    if count <= 1 {
        return Err(JobError::LastFolder);
    }
    let changed = conn.execute(
        "DELETE FROM library_folders WHERE id=?1 AND library_id=?2",
        params![folder_id, library_id],
    )?;
    if changed == 0 {
        return Err(JobError::FolderNotFound);
    }
    if let Some(path) = conn
        .query_row(
            "SELECT path FROM library_folders WHERE library_id=?1 ORDER BY position ASC, id ASC LIMIT 1",
            [&library_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        conn.execute(
            "UPDATE libraries SET path=?1 WHERE id=?2",
            params![path, library_id],
        )?;
    }
    get_library(conn, &library_id)?.ok_or(JobError::LibraryNotFound)
}

/// Documents the Enqueue Scan Job public API surface.
pub fn enqueue_scan_job(conn: &Connection, library_id: &str) -> Result<EntityId, JobError> {
    let library_id = id(library_id);
    enqueue_scan_job_for_id(conn, &library_id)
}

fn enqueue_scan_job_for_id(conn: &Connection, library_id: &EntityId) -> Result<EntityId, JobError> {
    let exists: Option<EntityId> = conn
        .query_row(
            "SELECT id FROM scan_jobs WHERE library_id=?1 AND status IN ('pending','running')
             ORDER BY id LIMIT 1",
            [library_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = exists {
        return Ok(existing);
    }
    let job_id = new_id();
    conn.execute(
        "INSERT INTO scan_jobs(id, library_id, status) VALUES(?1, ?2, 'pending')",
        params![job_id, library_id],
    )?;
    Ok(id(&job_id))
}

/// Documents the Enqueue Due Scheduled Scans public API surface.
pub fn enqueue_due_scheduled_scans(conn: &Connection) -> Result<Vec<EntityId>, JobError> {
    let due: Vec<(EntityId, f64)> = {
        let mut stmt = conn.prepare(
            "SELECT library_id, COALESCE(frequency_hours, 24)
             FROM scan_schedules
             WHERE enabled=1
               AND (next_run IS NULL OR TRIM(next_run) = '' OR datetime(next_run) <= datetime('now'))
             ORDER BY COALESCE(next_run, created_at, '') ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };

    let mut queued = Vec::new();
    for (library_id, frequency_hours) in due {
        let job_id = enqueue_scan_job_for_id(conn, &library_id)?;
        conn.execute(
            "UPDATE scan_schedules
             SET last_run=datetime('now'),
                 next_run=datetime('now', '+' || ?1 || ' hours'),
                 updated_at=datetime('now')
             WHERE library_id=?2",
            params![frequency_hours.max(0.5), library_id],
        )?;
        queued.push(job_id);
    }
    Ok(queued)
}

/// Documents the Claim Next Scan Job public API surface.
///
/// Claims whichever pending job is oldest, regardless of library. Used by the
/// background scheduler tick, where "some due library" is the point.
pub fn claim_next_scan_job(conn: &Connection) -> Result<Option<ClaimedScanJob>, JobError> {
    let Some(job_id) = conn
        .query_row(
            "SELECT id FROM scan_jobs WHERE status='pending' ORDER BY created_at ASC, id ASC LIMIT 1",
            [],
            |row| row.get::<_, EntityId>(0),
        )
        .optional()?
    else {
        return Ok(None);
    };
    claim_scan_job(conn, &job_id)
}

/// Claims a specific pending scan job by id, ignoring queue order.
///
/// Used when the caller already knows which job it wants to run — e.g. a
/// user clicking "Scan" on one library. Without this, that request would go
/// through `claim_next_scan_job` instead, which claims whatever job is
/// globally oldest; if another library had an older job still pending (a due
/// schedule, a recovered stale job, etc.), the click would silently start
/// that other library's scan while the clicked library's own job sat queued
/// behind it — the requested library's scan never actually starting despite
/// the UI showing "Scanning...".
pub fn claim_scan_job(
    conn: &Connection,
    job_id: &EntityId,
) -> Result<Option<ClaimedScanJob>, JobError> {
    let changed = conn.execute(
        "UPDATE scan_jobs
         SET status='running', started_at=datetime('now'), updated_at=datetime('now')
         WHERE id=?1 AND status='pending'",
        [job_id],
    )?;
    if changed == 0 {
        return Ok(None);
    }

    let (library_id, library_type): (EntityId, String) = conn.query_row(
        "SELECT l.id, COALESCE(l.library_type, 'music')
         FROM scan_jobs sj JOIN libraries l ON l.id = sj.library_id
         WHERE sj.id=?1",
        [job_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let folders = library_folder_paths(conn, &library_id)?;
    Ok(Some(ClaimedScanJob {
        job_id: job_id.clone(),
        library_id,
        library_type,
        folders,
    }))
}

/// Documents the Recover Stale Scan Jobs public API surface.
pub fn recover_stale_scan_jobs(conn: &Connection, timeout_minutes: i64) -> Result<usize, JobError> {
    Ok(conn.execute(
        "UPDATE scan_jobs
         SET status='pending', started_at=NULL, updated_at=datetime('now'),
             error_log='Recovered stale scan job after missing heartbeat'
         WHERE status='running'
           AND COALESCE(updated_at, started_at, created_at) < datetime('now', ?1)",
        [format!("-{} minutes", timeout_minutes.max(1))],
    )?)
}

/// Reset all running scan jobs to pending on startup (server crash/restart recovery).
/// Documents the Reset Orphaned Scan Jobs public API surface.
pub fn reset_orphaned_scan_jobs(conn: &Connection) -> Result<usize, JobError> {
    Ok(conn.execute(
        "UPDATE scan_jobs
         SET status='pending', started_at=NULL, updated_at=datetime('now'),
             error_log='Recovered: server restarted during scan'
         WHERE status='running'",
        [],
    )?)
}

/// Reset all running post-scan jobs to failed on startup (server crash/restart recovery).
/// Documents the Reset Orphaned Post Scan Jobs public API surface.
pub fn reset_orphaned_post_scan_jobs(conn: &Connection) -> Result<usize, JobError> {
    Ok(conn.execute(
        "UPDATE post_scan_jobs
         SET status='failed', finished_at=datetime('now'),
             error_log=COALESCE(NULLIF(error_log,''),'Server restarted during post-scan job')
         WHERE status='running'",
        [],
    )?)
}

/// Documents the Update Scan Progress public API surface.
pub fn update_scan_progress(
    conn: &Connection,
    job_id: &EntityId,
    files_found: i64,
    files_scanned: i64,
    errors: i64,
) -> Result<(), JobError> {
    if is_scan_cancelled(conn, job_id)? {
        return Err(JobError::ScanCancelled);
    }
    conn.execute(
        "UPDATE scan_jobs
         SET files_found=?1, files_scanned=?2, errors=?3, updated_at=datetime('now')
         WHERE id=?4 AND status='running'",
        params![files_found, files_scanned, errors, job_id],
    )?;
    Ok(())
}

/// Documents the Is Scan Cancelled public API surface.
pub fn is_scan_cancelled(conn: &Connection, job_id: &EntityId) -> Result<bool, rusqlite::Error> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM scan_jobs WHERE id=?1",
            [job_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(matches!(status.as_deref(), Some("cancelled")))
}

/// Documents the Mark Scan Cancelled public API surface.
pub fn mark_scan_cancelled(
    conn: &Connection,
    job_id: &EntityId,
    message: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE scan_jobs
         SET status='cancelled', finished_at=datetime('now'), updated_at=datetime('now'), error_log=?1
         WHERE id=?2",
        params![message, job_id],
    )?;
    Ok(())
}

/// Documents the Mark Scan Failed public API surface.
pub fn mark_scan_failed(
    conn: &Connection,
    job_id: &EntityId,
    message: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE scan_jobs
         SET status='failed', finished_at=datetime('now'), updated_at=datetime('now'), error_log=?1
         WHERE id=?2",
        params![message, job_id],
    )?;
    Ok(())
}

/// Documents the Mark Scan Done public API surface.
pub fn mark_scan_done(
    conn: &Connection,
    job_id: &EntityId,
    library_id: &EntityId,
    files_found: i64,
    files_scanned: i64,
    errors: i64,
    error_log: Option<&str>,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE scan_jobs
         SET status='done', finished_at=datetime('now'), updated_at=datetime('now'),
             files_found=?1, files_scanned=?2, errors=?3, error_log=?4
         WHERE id=?5",
        params![files_found, files_scanned, errors, error_log, job_id],
    )?;
    conn.execute(
        "UPDATE libraries SET last_scan=datetime('now') WHERE id=?1",
        [library_id],
    )?;
    crate::refresh_denormalized_counts(conn)?;
    crate::refresh_stats_cache(conn)?;
    Ok(())
}

/// Documents the Upsert Scanned Track public API surface.
pub fn upsert_scanned_track(
    conn: &Connection,
    input: &ScannedTrackInput,
) -> Result<EntityId, JobError> {
    let artist_id = upsert_artist(conn, &input.artist)?;
    let album_id = upsert_album(conn, &input.album, &input.album_artist)?;
    let track_id = conn
        .query_row(
            "SELECT id FROM tracks WHERE file_path=?1",
            [&input.file_path],
            |row| row.get::<_, EntityId>(0),
        )
        .optional()?
        .unwrap_or_else(|| EntityId::Str(new_id()));

    conn.execute(
        "INSERT INTO tracks(
            id, library_id, artist_id, album_id, title, file_name, album_artist,
            genre, composer, duration, bitrate, sample_rate, channels,
            file_path, file_size, format,
            track_number, disc_number, year, comment, bpm, scanned_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,datetime('now'))
         ON CONFLICT(file_path) DO UPDATE SET
            library_id=excluded.library_id,
            artist_id=excluded.artist_id,
            album_id=excluded.album_id,
            title=excluded.title,
            file_name=excluded.file_name,
            album_artist=excluded.album_artist,
            genre=excluded.genre,
            composer=excluded.composer,
            duration=excluded.duration,
            bitrate=excluded.bitrate,
            sample_rate=excluded.sample_rate,
            channels=excluded.channels,
            file_size=excluded.file_size,
            format=excluded.format,
            track_number=excluded.track_number,
            disc_number=excluded.disc_number,
            year=excluded.year,
            comment=excluded.comment,
            bpm=excluded.bpm,
            scanned_at=datetime('now')",
        params![
            track_id,
            input.library_id,
            artist_id,
            album_id,
            input.title,
            input.file_name,
            input.album_artist,
            input.genre,
            input.composer,
            input.duration,
            input.bitrate,
            input.sample_rate,
            input.channels,
            input.file_path,
            input.file_size,
            input.format,
            input.track_number,
            input.disc_number,
            input.year,
            input.comment,
            input.bpm,
        ],
    )?;
    refresh_track_fts(conn, &track_id, input, &artist_id, &album_id)?;
    Ok(track_id)
}

/// Documents the Prune Missing Tracks public API surface.
pub fn prune_missing_tracks(
    conn: &Connection,
    library_id: &EntityId,
    seen_paths: &std::collections::HashSet<String>,
) -> Result<usize, JobError> {
    let existing: Vec<(EntityId, String)> = {
        let mut stmt = conn.prepare("SELECT id, file_path FROM tracks WHERE library_id=?1")?;
        let rows = stmt
            .query_map([library_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let mut pruned = 0;
    for (track_id, file_path) in existing {
        if !seen_paths.contains(&file_path.to_ascii_lowercase()) {
            conn.execute("DELETE FROM tracks_fts WHERE track_id=?1", [&track_id])?;
            conn.execute("DELETE FROM tracks WHERE id=?1", [&track_id])?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

/// Deletes albums and artists left with no tracks after a scan — e.g. the
/// stale "Singles and EPs"/"Compilations" folder-name entries a mistagging
/// bug used to create, once a rescan re-derives the real tags and moves the
/// tracks to the correct album/artist. Skips anything the user has locked
/// via metadata_locked, and skips artists that still own an album or are
/// directly credited on a track (the compilation-contributor case), even if
/// that album currently has zero tracks of its own.
pub fn prune_orphaned_music_entities(conn: &Connection) -> Result<(usize, usize), JobError> {
    let albums_pruned = conn.execute(
        "DELETE FROM albums
         WHERE metadata_locked = 0
           AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = albums.id)",
        [],
    )?;
    let artists_pruned = conn.execute(
        "DELETE FROM artists
         WHERE metadata_locked = 0
           AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = artists.id)
           AND NOT EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = artists.id)",
        [],
    )?;
    Ok((albums_pruned, artists_pruned))
}

/// Documents the Enqueue Default Music Post Scan Jobs public API surface.
pub fn enqueue_default_music_post_scan_jobs(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<Vec<EntityId>, JobError> {
    let mut ids = Vec::new();
    for job_type in [
        "refresh_library_mappings",
        "cache_album_images",
        "cache_artist_images",
        "warm_lastfm_artist_info",
        "warm_lastfm_album_info",
        "warm_track_lyrics",
        "sync_artist_styles",
        "sync_discogs_album_metadata",
    ] {
        ids.push(enqueue_post_scan_job_for_id(
            conn, library_id, job_type, None,
        )?);
    }
    Ok(ids)
}

/// Documents the Claim Next Post Scan Job public API surface.
pub fn claim_next_post_scan_job(
    conn: &Connection,
    lane: PostScanLane,
) -> Result<Option<ClaimedPostScanJob>, JobError> {
    recover_stale_post_scan_jobs(conn, lane)?;
    if has_running_post_scan_job_in_lane(conn, lane)? {
        return Ok(None);
    }

    let job_types = runnable_post_scan_job_types(lane);
    if job_types.is_empty() {
        return Ok(None);
    }
    let placeholders = vec!["?"; job_types.len()].join(",");
    let select_sql = format!(
        "SELECT id, library_id, job_type, payload
         FROM post_scan_jobs
         WHERE status='pending' AND job_type IN ({placeholders})
         ORDER BY id LIMIT 1"
    );
    let Some(job) = conn
        .query_row(&select_sql, params_from_iter(job_types), |row| {
            Ok(ClaimedPostScanJob {
                job_id: row.get(0)?,
                library_id: row.get(1)?,
                job_type: row.get(2)?,
                payload: row.get(3)?,
            })
        })
        .optional()?
    else {
        return Ok(None);
    };

    let changed = conn.execute(
        "UPDATE post_scan_jobs
         SET status='running', started_at=datetime('now'), heartbeat_at=datetime('now'), finished_at=NULL
         WHERE id=?1 AND status='pending'",
        [&job.job_id],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(job))
}

/// Documents the Touch Post Scan Job public API surface.
pub fn touch_post_scan_job(conn: &Connection, job_id: &EntityId) -> Result<(), JobError> {
    conn.execute(
        "UPDATE post_scan_jobs SET heartbeat_at=datetime('now') WHERE id=?1 AND status='running'",
        [job_id],
    )?;
    Ok(())
}

/// Documents the Mark Post Scan Done public API surface.
pub fn mark_post_scan_done(conn: &Connection, job_id: &EntityId) -> Result<(), JobError> {
    conn.execute(
        "UPDATE post_scan_jobs
         SET status='done', finished_at=datetime('now'), heartbeat_at=NULL, error_log=NULL
         WHERE id=?1",
        [job_id],
    )?;
    Ok(())
}

/// Documents the Mark Post Scan Failed public API surface.
pub fn mark_post_scan_failed(
    conn: &Connection,
    job_id: &EntityId,
    message: &str,
) -> Result<(), JobError> {
    conn.execute(
        "UPDATE post_scan_jobs
         SET status='failed', finished_at=datetime('now'), heartbeat_at=NULL, error_log=?1
         WHERE id=?2",
        params![message, job_id],
    )?;
    Ok(())
}

/// Documents the Refresh Library Entity Mappings public API surface.
pub fn refresh_library_entity_mappings(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<(), JobError> {
    if table_exists(conn, "artist_libraries")? {
        conn.execute(
            "DELETE FROM artist_libraries WHERE library_id=?1",
            [library_id],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO artist_libraries(artist_id, library_id)
             SELECT DISTINCT artist_id, library_id
             FROM tracks
             WHERE library_id=?1 AND artist_id IS NOT NULL",
            [library_id],
        )?;
    }
    if table_exists(conn, "album_libraries")? {
        conn.execute(
            "DELETE FROM album_libraries WHERE library_id=?1",
            [library_id],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO album_libraries(album_id, library_id)
             SELECT DISTINCT album_id, library_id
             FROM tracks
             WHERE library_id=?1 AND album_id IS NOT NULL",
            [library_id],
        )?;
    }
    Ok(())
}

/// Documents the List Active Scan Jobs public API surface.
pub fn list_active_scan_jobs(conn: &Connection) -> Result<Vec<ScanJobRow>, rusqlite::Error> {
    scan_jobs_from_sql(
        conn,
        "SELECT id, library_id, status, created_at, updated_at, started_at, finished_at,
                COALESCE(files_found,0), COALESCE(files_scanned,0), COALESCE(errors,0), error_log
         FROM scan_jobs
         WHERE status IN ('pending', 'running')
         ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, started_at DESC, id DESC",
        [],
    )
}

/// Documents the Get Scan Job Detail public API surface.
pub fn get_scan_job_detail(conn: &Connection, job_id: &str) -> Result<ScanJobDetail, JobError> {
    let job_id = id(job_id);
    let job = scan_job_by_id(conn, &job_id)?.ok_or(JobError::JobNotFound)?;
    let queue_position = if job.status == "pending" {
        Some(conn.query_row(
            "SELECT COUNT(*) FROM scan_jobs
             WHERE status='pending'
               AND (COALESCE(created_at, '') < COALESCE(?1, '')
                    OR (COALESCE(created_at, '') = COALESCE(?2, '') AND id <= ?3))",
            params![job.created_at, job.created_at, job_id],
            |row| row.get(0),
        )?)
    } else {
        None
    };
    let running_job = conn
        .query_row(
            "SELECT sj.id, sj.library_id, sj.started_at, l.name
             FROM scan_jobs sj JOIN libraries l ON l.id = sj.library_id
             WHERE sj.status='running'
             ORDER BY sj.started_at ASC, sj.id ASC LIMIT 1",
            [],
            |row| {
                Ok(RunningJobRow {
                    id: row.get(0)?,
                    library_id: row.get(1)?,
                    started_at: row.get(2)?,
                    library_name: row.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(ScanJobDetail {
        job,
        queue_position,
        running_job,
    })
}

/// Documents the List Library Scan Jobs public API surface.
pub fn list_library_scan_jobs(
    conn: &Connection,
    library_id: &str,
) -> Result<Vec<ScanJobRow>, rusqlite::Error> {
    let library_id = id(library_id);
    scan_jobs_from_sql(
        conn,
        "SELECT id, library_id, status, created_at, updated_at, started_at, finished_at,
                COALESCE(files_found,0), COALESCE(files_scanned,0), COALESCE(errors,0), error_log
         FROM scan_jobs WHERE library_id=?1 ORDER BY started_at DESC LIMIT 20",
        [&library_id],
    )
}

/// Documents the Cancel Scan Job public API surface.
pub fn cancel_scan_job(conn: &Connection, job_id: &str) -> Result<(), JobError> {
    let changed = conn.execute(
        "UPDATE scan_jobs
         SET status='cancelled', finished_at=datetime('now'), error_log='Cancelled by admin'
         WHERE id=?1 AND status IN ('pending','running')",
        [id(job_id)],
    )?;
    if changed == 0 {
        return Err(JobError::ScanJobNotActive);
    }
    Ok(())
}

/// Documents the Upsert Schedule public API surface.
pub fn upsert_schedule(
    conn: &Connection,
    library_id: &EntityId,
    enabled: bool,
    frequency_hours: f64,
) -> Result<ScheduleRow, JobError> {
    if frequency_hours < 0.5 {
        return Err(JobError::ScheduleTooFrequent);
    }
    let schedule_id = existing_schedule_id(conn, library_id)?.unwrap_or_else(new_id);
    let next_expr = if enabled {
        "datetime('now', '+' || ?4 || ' hours')"
    } else {
        "NULL"
    };
    let sql = format!(
        "INSERT INTO scan_schedules(id, library_id, enabled, frequency_hours, next_run)
         VALUES(?1, ?2, ?3, ?4, {next_expr})
         ON CONFLICT(library_id) DO UPDATE SET
           enabled=excluded.enabled,
           frequency_hours=excluded.frequency_hours,
           next_run=excluded.next_run,
           updated_at=datetime('now')"
    );
    conn.execute(
        &sql,
        params![schedule_id, library_id, enabled as i64, frequency_hours],
    )?;
    get_schedule(conn, library_id)?.ok_or(JobError::LibraryNotFound)
}

/// Documents the Get Schedule public API surface.
pub fn get_schedule(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<Option<ScheduleRow>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, library_id, enabled, COALESCE(frequency_hours, 24), last_run, next_run, created_at, updated_at,
                NULL, NULL
         FROM scan_schedules WHERE library_id=?1",
        [library_id],
        map_schedule,
    )
    .optional()
}

/// Documents the List Schedules public API surface.
pub fn list_schedules(conn: &Connection) -> Result<Vec<ScheduleRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT ss.id, ss.library_id, ss.enabled, COALESCE(ss.frequency_hours, 24), ss.last_run,
                ss.next_run, ss.created_at, ss.updated_at, l.name, l.path
         FROM scan_schedules ss JOIN libraries l ON l.id = ss.library_id
         ORDER BY l.name",
    )?;
    let rows = stmt.query_map([], map_schedule)?.collect();
    rows
}

/// Documents the Delete Schedule public API surface.
pub fn delete_schedule(conn: &Connection, library_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM scan_schedules WHERE library_id=?1",
        [id(library_id)],
    )?;
    Ok(())
}

/// Documents the Queue Snapshot public API surface.
pub fn queue_snapshot(conn: &Connection) -> Result<QueueSnapshot, rusqlite::Error> {
    Ok(QueueSnapshot {
        fetched_at: conn.query_row("SELECT datetime('now')", [], |row| row.get(0))?,
        queues: QueueGroups {
            scan: queue_rows(
                conn,
                "SELECT sj.id, sj.status, sj.library_id, l.name, NULL,
                        sj.files_scanned, sj.files_found, sj.errors, sj.started_at, sj.finished_at,
                        NULL, NULL, NULL, NULL, sj.error_log
                 FROM scan_jobs sj JOIN libraries l ON l.id = sj.library_id
                 WHERE sj.status IN ('pending', 'running') ORDER BY sj.id",
            )?,
            post_scan: queue_rows(
                conn,
                "SELECT psj.id, psj.status, psj.library_id, l.name, psj.job_type,
                        NULL, NULL, NULL, psj.started_at, psj.finished_at,
                        psj.heartbeat_at, NULL, NULL, NULL, psj.error_log
                 FROM post_scan_jobs psj JOIN libraries l ON l.id = psj.library_id
                 WHERE psj.status IN ('pending', 'running') ORDER BY psj.id",
            )?,
            mix: Vec::new(),
            deep_analysis: Vec::new(),
        },
    })
}

/// Documents the Fail Post Scan Job public API surface.
pub fn fail_post_scan_job(conn: &Connection, job_id: &str) -> Result<(), JobError> {
    let changed = conn.execute(
        "UPDATE post_scan_jobs
         SET status='failed', finished_at=datetime('now'), error_log='Marked failed by admin'
         WHERE id=?1 AND status IN ('pending','running')",
        [id(job_id)],
    )?;
    if changed == 0 {
        return Err(JobError::PostScanJobNotActive);
    }
    Ok(())
}

/// Documents the Cancel Post Scan Job public API surface.
pub fn cancel_post_scan_job(conn: &Connection, job_id: &str) -> Result<(), JobError> {
    let changed = conn.execute(
        "UPDATE post_scan_jobs
         SET status='cancelled', finished_at=datetime('now'), error_log='Cancelled by admin'
         WHERE id=?1 AND status='pending'",
        [id(job_id)],
    )?;
    if changed == 0 {
        return Err(JobError::PostScanJobNotPending);
    }
    Ok(())
}

/// Documents the Retry Post Scan Job public API surface.
pub fn retry_post_scan_job(conn: &Connection, job_id: &str) -> Result<(), JobError> {
    let changed = conn.execute(
        "UPDATE post_scan_jobs
         SET status='pending', started_at=NULL, heartbeat_at=NULL, finished_at=NULL, error_log=NULL
         WHERE id=?1 AND status IN ('failed','cancelled')",
        [id(job_id)],
    )?;
    if changed == 0 {
        return Err(JobError::PostScanJobNotRetryable);
    }
    Ok(())
}

/// Documents the Enqueue Post Scan Job public API surface.
pub fn enqueue_post_scan_job(
    conn: &Connection,
    library_id: &str,
    job_type: &str,
    payload: Option<String>,
) -> Result<EntityId, JobError> {
    let library_id = id(library_id);
    let library_type: Option<String> = conn
        .query_row(
            "SELECT library_type FROM libraries WHERE id=?1",
            [&library_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(library_type) = library_type else {
        return Err(JobError::LibraryNotFound);
    };
    if !allowed_post_scan_job_types(&library_type).contains(&job_type) {
        return Err(JobError::UnsupportedPostScanJob(job_type.to_owned()));
    }
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM post_scan_jobs
             WHERE library_id=?1 AND job_type=?2 AND status IN ('pending','running')
             ORDER BY id LIMIT 1",
            params![library_id, job_type],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(existing);
    }
    let job_id = new_id();
    conn.execute(
        "INSERT INTO post_scan_jobs(id, library_id, job_type, payload, status)
         VALUES(?1, ?2, ?3, ?4, 'pending')",
        params![job_id, library_id, job_type, payload],
    )?;
    Ok(id(&job_id))
}

fn enqueue_post_scan_job_for_id(
    conn: &Connection,
    library_id: &EntityId,
    job_type: &str,
    payload: Option<String>,
) -> Result<EntityId, JobError> {
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM post_scan_jobs
             WHERE library_id=?1 AND job_type=?2 AND status IN ('pending','running')
             ORDER BY id LIMIT 1",
            params![library_id, job_type],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(existing);
    }
    let job_id = new_id();
    conn.execute(
        "INSERT INTO post_scan_jobs(id, library_id, job_type, payload, status)
         VALUES(?1, ?2, ?3, ?4, 'pending')",
        params![job_id, library_id, job_type, payload],
    )?;
    Ok(id(&job_id))
}

fn get_library(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<Option<LibraryRow>, rusqlite::Error> {
    Ok(crate::music::list_libraries(conn)?
        .into_iter()
        .find(|library| &library.id == library_id))
}

fn library_folder_paths(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut folders: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT path FROM library_folders WHERE library_id=?1 ORDER BY position ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([library_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    if folders.is_empty() {
        if let Some(path) = conn
            .query_row(
                "SELECT path FROM libraries WHERE id=?1",
                [library_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            folders.push(path);
        }
    }
    Ok(folders)
}

/// Recognizes common "various artists" tag spellings so compilations don't get
/// fragmented across near-duplicate artist entries ("VA", "Various", "V/A", ...).
pub(crate) fn canonical_compilation_artist_name(name: &str) -> Option<String> {
    let compact: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    match compact.as_str() {
        "variousartists" | "various" | "va" => Some("Various Artists".to_string()),
        _ => None,
    }
}

/// Finds an existing artist by case-insensitive name match, or creates one.
/// Shared by the scanner and by the track-metadata edit path (Track Info popup)
/// so a manual artist rename resolves the same way an auto-scan would.
pub(crate) fn upsert_artist(conn: &Connection, name: &str) -> Result<EntityId, JobError> {
    let normalized = defaulted(Some(name), "Unknown Artist");
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM artists WHERE LOWER(TRIM(name))=LOWER(TRIM(?1)) LIMIT 1",
            [&normalized],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(existing);
    }
    let artist_id = EntityId::Str(new_id());
    conn.execute(
        "INSERT INTO artists(id, name) VALUES(?1, ?2)",
        params![artist_id, normalized],
    )?;
    Ok(artist_id)
}

/// Finds an existing album by case-insensitive (title, album_artist) match, or
/// creates one. Shared by the scanner and by the track-metadata edit path (Track
/// Info popup) so a manual album rename resolves the same way an auto-scan would.
pub(crate) fn upsert_album(
    conn: &Connection,
    title: &str,
    album_artist: &str,
) -> Result<EntityId, JobError> {
    let title = defaulted(Some(title), "Unknown Album");
    let album_artist = defaulted(Some(album_artist), "Unknown Artist");
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM albums
             WHERE LOWER(TRIM(title))=LOWER(TRIM(?1))
               AND LOWER(TRIM(album_artist))=LOWER(TRIM(?2))
             LIMIT 1",
            params![title, album_artist],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(existing);
    }
    // The album is owned by the artist named in its own album_artist tag, not
    // necessarily by whichever track's artist happens to create the row first
    // (that track's artist may just be one of many contributors on a compilation).
    // Various-artists taggers use inconsistent spellings ("VA", "Various", "V/A"),
    // so collapse them to one canonical artist rather than fragmenting compilations
    // across several near-duplicate artist entries.
    let owning_artist_name =
        canonical_compilation_artist_name(&album_artist).unwrap_or_else(|| album_artist.clone());
    let owning_artist_id = upsert_artist(conn, &owning_artist_name)?;
    let album_id = EntityId::Str(new_id());
    conn.execute(
        "INSERT INTO albums(id, title, album_artist, artist_id) VALUES(?1, ?2, ?3, ?4)",
        params![album_id, title, album_artist, owning_artist_id],
    )?;
    Ok(album_id)
}

fn refresh_track_fts(
    conn: &Connection,
    track_id: &EntityId,
    input: &ScannedTrackInput,
    _artist_id: &EntityId,
    _album_id: &EntityId,
) -> Result<(), JobError> {
    conn.execute("DELETE FROM tracks_fts WHERE track_id=?1", [track_id])?;
    conn.execute(
        "INSERT INTO tracks_fts(track_id, title, artist, album, genre, composer, file_path)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            track_id,
            input.title,
            input.artist,
            input.album,
            input.genre,
            input.composer,
            input.file_path
        ],
    )?;
    Ok(())
}

fn insert_library_folder(
    conn: &Connection,
    library_id: &EntityId,
    path: &str,
    position: i64,
) -> Result<(), JobError> {
    conn.execute(
        "INSERT INTO library_folders(id, library_id, path, position) VALUES(?1, ?2, ?3, ?4)",
        params![new_id(), library_id, path, position],
    )
    .map_err(|err| {
        if is_unique_error(&err) {
            JobError::DuplicateFolder
        } else {
            JobError::Db(err)
        }
    })?;
    Ok(())
}

fn normalize_folders(folders: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for folder in folders {
        let normalized = normalize_path(&folder);
        if !normalized.is_empty()
            && !result
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&normalized))
        {
            result.push(normalized);
        }
    }
    result
}

fn normalize_path(path: &str) -> String {
    path.trim().trim_end_matches(['\\', '/']).to_owned()
}

fn normalize_library_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn path_display_name(path: &str) -> &str {
    path.trim()
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("Library")
}

fn parse_library_type(raw: Option<&str>) -> &'static str {
    let _ = raw;
    "music"
}

fn defaulted(raw: Option<&str>, default_value: &str) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_value)
        .to_owned()
}

fn is_library_name_taken(
    conn: &Connection,
    name: &str,
    exclude_id: Option<&EntityId>,
) -> Result<bool, rusqlite::Error> {
    let count: i64 = if let Some(exclude_id) = exclude_id {
        conn.query_row(
            "SELECT COUNT(*) FROM libraries WHERE LOWER(TRIM(name))=LOWER(TRIM(?1)) AND id<>?2",
            params![name, exclude_id],
            |row| row.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM libraries WHERE LOWER(TRIM(name))=LOWER(TRIM(?1))",
            [name],
            |row| row.get(0),
        )?
    };
    Ok(count > 0)
}

fn scan_job_by_id(
    conn: &Connection,
    job_id: &EntityId,
) -> Result<Option<ScanJobRow>, rusqlite::Error> {
    scan_jobs_from_sql(
        conn,
        "SELECT id, library_id, status, created_at, updated_at, started_at, finished_at,
                COALESCE(files_found,0), COALESCE(files_scanned,0), COALESCE(errors,0), error_log
         FROM scan_jobs WHERE id=?1",
        [job_id],
    )
    .map(|mut rows| rows.pop())
}

fn scan_jobs_from_sql<P>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<ScanJobRow>, rusqlite::Error>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params, |row| {
            Ok(ScanJobRow {
                id: row.get(0)?,
                library_id: row.get(1)?,
                status: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                files_found: row.get(7)?,
                files_scanned: row.get(8)?,
                errors: row.get(9)?,
                error_log: row.get(10)?,
            })
        })?
        .collect();
    rows
}

fn existing_schedule_id(
    conn: &Connection,
    library_id: &EntityId,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT id FROM scan_schedules WHERE library_id=?1",
        [library_id],
        |row| row.get(0),
    )
    .optional()
}

fn map_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduleRow> {
    Ok(ScheduleRow {
        id: row.get(0)?,
        library_id: row.get(1)?,
        enabled: row.get(2)?,
        frequency_hours: row.get(3)?,
        last_run: row.get(4)?,
        next_run: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        library_name: row.get(8)?,
        library_path: row.get(9)?,
    })
}

fn queue_rows(conn: &Connection, sql: &str) -> Result<Vec<QueueRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(QueueRow {
                id: row.get(0)?,
                status: row.get(1)?,
                library_id: row.get(2)?,
                library_name: row.get(3)?,
                job_type: row.get(4)?,
                files_scanned: row.get(5)?,
                files_found: row.get(6)?,
                errors: row.get(7)?,
                started_at: row.get(8)?,
                finished_at: row.get(9)?,
                heartbeat_at: row.get(10)?,
                current_step: row.get(11)?,
                playlist_name: row.get(12)?,
                track_title: row.get(13)?,
                error_message: row.get(14)?,
            })
        })?
        .collect();
    rows
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

fn runnable_post_scan_job_types(lane: PostScanLane) -> &'static [&'static str] {
    match lane {
        PostScanLane::Music => MUSIC_POST_SCAN_JOB_TYPES,
    }
}

fn lane_post_scan_job_types(lane: PostScanLane) -> &'static [&'static str] {
    match lane {
        PostScanLane::Music => MUSIC_POST_SCAN_JOB_TYPES,
    }
}

fn has_running_post_scan_job_in_lane(
    conn: &Connection,
    lane: PostScanLane,
) -> Result<bool, rusqlite::Error> {
    let job_types = lane_post_scan_job_types(lane);
    if job_types.is_empty() {
        return Ok(false);
    }
    let placeholders = vec!["?"; job_types.len()].join(",");
    let sql = format!(
        "SELECT 1 FROM post_scan_jobs
         WHERE status='running' AND job_type IN ({placeholders})
         LIMIT 1"
    );
    Ok(conn
        .query_row(&sql, params_from_iter(job_types), |_| Ok(()))
        .optional()?
        .is_some())
}

fn recover_stale_post_scan_jobs(conn: &Connection, lane: PostScanLane) -> Result<usize, JobError> {
    let mut total = 0;
    for job_type in lane_post_scan_job_types(lane) {
        let timeout = post_scan_stale_timeout_minutes(job_type);
        total += conn.execute(
            "UPDATE post_scan_jobs
             SET status='failed',
                 finished_at=datetime('now'),
                 error_log=COALESCE(NULLIF(error_log, ''), ?1)
             WHERE status='running'
               AND finished_at IS NULL
               AND job_type=?2
               AND COALESCE(heartbeat_at, started_at) < datetime('now', ?3)",
            params![
                format!("Recovered stale post-scan job after {timeout} minutes without heartbeat"),
                job_type,
                format!("-{timeout} minutes")
            ],
        )?;
    }
    Ok(total)
}

fn post_scan_stale_timeout_minutes(job_type: &str) -> i64 {
    match job_type {
        "warm_track_lyrics" => 120,
        "sync_artist_styles" => 120,
        "sync_discogs_album_metadata" => 180,
        "refresh_library_mappings" => 15,
        _ => 30,
    }
}

fn allowed_post_scan_job_types(_library_type: &str) -> &'static [&'static str] {
    MUSIC_POST_SCAN_JOB_TYPES
}

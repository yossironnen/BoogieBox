//! Backups and restores: the schedule, "Back up now", live restore (no restart), first-run restore, a restore
//! left pending at startup, and the offline CLI (`--list-backups`, `--restore`).
//!
//! The database half (snapshot, manifest, validation, retention, path remaps) lives in `boogiebox_db::backup`.
//! This module adds the password key, locked artwork originals, optional extras, and swapping the live database.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use boogiebox_db::backup::{
    self as db_backup, BackupEntry, BackupError, BackupReason, Manifest, PathRemap,
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::artwork_cache::{build_album_art_cache_key, build_artist_art_cache_key, cache_item_dir};
use crate::{DbPool, SharedState};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const SCHEDULE_KEY: &str = "backupSchedule";
const DAY_KEY: &str = "backupDay";
const TIME_KEY: &str = "backupTime";
const KEEP_KEY: &str = "backupKeep";
const INCLUDE_ART_KEY: &str = "backupIncludeAllArtwork";
const INCLUDE_MIXES_KEY: &str = "backupIncludeMixes";
const LAST_FAILURE_KEY: &str = "backupLastFailure";
const LAST_SLOT_KEY: &str = "backupLastScheduledSlot";

/// Marker left in the data folder when a live restore could not close the database; finished at startup.
pub const PENDING_RESTORE_FILE: &str = "restore-pending.json";
/// Database staged next to the live one before a swap.
const STAGED_DB_FILE: &str = "boogiebox.restore.db";
/// Held for the server's lifetime so the CLI can tell a server is using the data folder.
const LOCK_FILE: &str = "boogiebox.lock";
/// How long a live restore waits for background work to let go of the database.
const RELEASE_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(30)
};
const PRE_RESTORE_PREFIX: &str = "pre-restore-";
const MIX_OUTPUT_FALLBACK_DIR: &str = "mix-outputs";

// ── Settings ─────────────────────────────────────────────────────────────────

/// How often scheduled backups run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Schedule {
    Off,
    Daily,
    Weekly,
}

/// Backup settings (admin, global).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    pub schedule: Schedule,
    /// 0 = Sunday … 6 = Saturday (weekly only).
    pub day: u8,
    /// Local `HH:MM`.
    pub time: String,
    pub keep: usize,
    /// Empty = `<data folder>/backups`.
    pub path: String,
    pub include_all_artwork: bool,
    pub include_mixes: bool,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            schedule: Schedule::Weekly,
            day: 0,
            time: "03:30".into(),
            keep: db_backup::DEFAULT_KEEP,
            path: String::new(),
            include_all_artwork: false,
            include_mixes: false,
        }
    }
}

fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
    .ok()
    .flatten()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value],
    )
    .map(|_| ())
}

fn valid_time(time: &str) -> bool {
    let b = time.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && time[..2].parse::<u8>().is_ok_and(|h| h < 24)
        && time[3..].parse::<u8>().is_ok_and(|m| m < 60)
        && b[..2].iter().chain(&b[3..]).all(u8::is_ascii_digit)
}

impl BackupSettings {
    /// Stored settings, falling back to defaults for anything missing or invalid.
    pub fn load(conn: &Connection) -> Self {
        let d = Self::default();
        let schedule = match get_setting(conn, SCHEDULE_KEY).as_deref() {
            Some("off") => Schedule::Off,
            Some("daily") => Schedule::Daily,
            Some("weekly") => Schedule::Weekly,
            _ => d.schedule,
        };
        Self {
            schedule,
            day: get_setting(conn, DAY_KEY)
                .and_then(|v| v.parse().ok())
                .filter(|v| *v <= 6)
                .unwrap_or(d.day),
            time: get_setting(conn, TIME_KEY)
                .filter(|v| valid_time(v))
                .unwrap_or(d.time),
            keep: get_setting(conn, KEEP_KEY)
                .and_then(|v| v.parse().ok())
                .filter(|v| (1..=30).contains(v))
                .unwrap_or(d.keep),
            path: get_setting(conn, db_backup::BACKUP_PATH_SETTING).unwrap_or_default(),
            include_all_artwork: get_setting(conn, INCLUDE_ART_KEY).as_deref() == Some("true"),
            include_mixes: get_setting(conn, INCLUDE_MIXES_KEY).as_deref() == Some("true"),
        }
    }

    /// Checks values coming from the API.
    pub fn validate(&self) -> Result<(), String> {
        if self.day > 6 {
            return Err("day must be 0–6".into());
        }
        if !valid_time(&self.time) {
            return Err("time must be HH:MM".into());
        }
        if !(1..=30).contains(&self.keep) {
            return Err("keep must be 1–30".into());
        }
        let path = self.path.trim();
        if !path.is_empty() && !Path::new(path).is_absolute() && !path.starts_with(r"\\") {
            return Err("The backup folder must be a full path".into());
        }
        Ok(())
    }

    /// Stores every field.
    pub fn save(&self, conn: &Connection) -> rusqlite::Result<()> {
        let schedule = match self.schedule {
            Schedule::Off => "off",
            Schedule::Daily => "daily",
            Schedule::Weekly => "weekly",
        };
        set_setting(conn, SCHEDULE_KEY, schedule)?;
        set_setting(conn, DAY_KEY, &self.day.to_string())?;
        set_setting(conn, TIME_KEY, &self.time)?;
        set_setting(conn, KEEP_KEY, &self.keep.to_string())?;
        set_setting(conn, db_backup::BACKUP_PATH_SETTING, self.path.trim())?;
        set_setting(
            conn,
            INCLUDE_ART_KEY,
            if self.include_all_artwork {
                "true"
            } else {
                "false"
            },
        )?;
        set_setting(
            conn,
            INCLUDE_MIXES_KEY,
            if self.include_mixes { "true" } else { "false" },
        )
    }
}

/// The last failed backup, cleared by the next success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFailure {
    pub at: String,
    pub message: String,
}

/// Reads the stored failure.
pub fn last_failure(conn: &Connection) -> Option<BackupFailure> {
    get_setting(conn, LAST_FAILURE_KEY).and_then(|raw| serde_json::from_str(&raw).ok())
}

fn record_result(conn: &Connection, result: &Result<BackupEntry, String>) {
    let outcome = match result {
        Ok(_) => conn
            .execute("DELETE FROM settings WHERE key = ?1", [LAST_FAILURE_KEY])
            .map(|_| ()),
        Err(message) => {
            let failure = BackupFailure {
                at: db_backup::utc_stamp(SystemTime::now()).1,
                message: message.clone(),
            };
            set_setting(
                conn,
                LAST_FAILURE_KEY,
                &serde_json::to_string(&failure).unwrap_or_default(),
            )
        }
    };
    if let Err(err) = outcome {
        tracing::warn!("Could not record the backup result: {err}");
    }
}

// ── Running state ────────────────────────────────────────────────────────────

/// What is running now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningJob {
    /// `backup` or `restore`.
    pub kind: &'static str,
    pub step: String,
}

#[derive(Debug, Default)]
struct HubInner {
    running: Mutex<Option<RunningJob>>,
    restoring: AtomicBool,
}

/// One backup or restore at a time, plus the maintenance flag the request guard reads.
#[derive(Clone, Debug, Default)]
pub struct BackupHub {
    inner: Arc<HubInner>,
}

/// Clears the running job when dropped.
pub struct JobGuard {
    hub: BackupHub,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        *self
            .hub
            .inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
}

impl BackupHub {
    /// Starts a job, or `None` when one is already running.
    pub fn try_begin(&self, kind: &'static str, step: &str) -> Option<JobGuard> {
        let mut running = self.inner.running.lock().unwrap_or_else(|p| p.into_inner());
        if running.is_some() {
            return None;
        }
        *running = Some(RunningJob {
            kind,
            step: step.into(),
        });
        Some(JobGuard { hub: self.clone() })
    }

    fn step(&self, step: &str) {
        if let Some(job) = self
            .inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
        {
            job.step = step.into();
        }
    }

    /// The running job, if any.
    pub fn running(&self) -> Option<RunningJob> {
        self.inner
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// True while a restore has the database out of service.
    pub fn is_restoring(&self) -> bool {
        self.inner.restoring.load(Ordering::SeqCst)
    }

    fn set_restoring(&self, on: bool) {
        self.inner.restoring.store(on, Ordering::SeqCst);
    }
}

// ── Paths ────────────────────────────────────────────────────────────────────

/// Where a database's files live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// `boogiebox.db`.
    pub db_file: PathBuf,
    /// Folder holding the database and `subsonic.key`.
    pub data_dir: PathBuf,
    /// Base of `art/` (the configured database folder, as the artwork lanes use it).
    pub art_base: PathBuf,
}

impl DataPaths {
    /// Paths for a configured database folder.
    pub fn for_folder(db_folder: &Path) -> Option<Self> {
        let db_file = boogiebox_db::get_db_path(db_folder).ok()?;
        let data_dir = db_file.parent()?.to_path_buf();
        Some(Self {
            db_file,
            data_dir,
            art_base: db_folder.to_path_buf(),
        })
    }

    fn key_file(&self) -> PathBuf {
        self.data_dir.join(db_backup::BUNDLE_KEY_FILE)
    }

    fn art_dir(&self) -> PathBuf {
        self.art_base.join("art")
    }
}

/// The open database and its paths, when one is active.
pub fn active(state: &SharedState) -> Option<(DbPool, DataPaths, BackupHub)> {
    let s = state.read().unwrap_or_else(|p| p.into_inner());
    let db = s.db.clone()?;
    let paths = DataPaths::for_folder(s.db_folder.as_deref()?)?;
    Some((db, paths, s.backup.clone()))
}

/// The backup folder for the active database.
pub fn backup_dir(conn: &Connection, paths: &DataPaths) -> PathBuf {
    let configured = get_setting(conn, db_backup::BACKUP_PATH_SETTING);
    db_backup::resolve_backup_dir(&paths.data_dir, configured.as_deref())
}

// ── Data-folder lock ─────────────────────────────────────────────────────────

static DATA_LOCK: Mutex<Option<fs::File>> = Mutex::new(None);

/// Holds `boogiebox.lock` in `data_dir` until the next call (or exit), so the CLI won't restore under a server.
pub fn hold_data_lock(data_dir: &Path) {
    let path = data_dir.join(LOCK_FILE);
    let mut held = DATA_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    *held = None;
    match fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    {
        Ok(file) => match file.try_lock() {
            Ok(()) => *held = Some(file),
            Err(err) => tracing::warn!("Could not lock {}: {err}", path.display()),
        },
        Err(err) => tracing::warn!("Could not create {}: {err}", path.display()),
    }
}

/// True when another process holds the data folder's lock.
pub fn data_folder_in_use(data_dir: &Path) -> bool {
    let path = data_dir.join(LOCK_FILE);
    let Ok(file) = fs::OpenOptions::new().write(true).open(&path) else {
        return false;
    };
    let busy = file.try_lock().is_err();
    let _ = file.unlock();
    busy
}

// ── Making a backup ──────────────────────────────────────────────────────────

fn copy_tree(src: &Path, dest: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            copy_tree(&path, &dest.join(&name), &child_rel, out)?;
        } else {
            fs::create_dir_all(dest)?;
            fs::copy(&path, dest.join(&name))?;
            out.push(child_rel);
        }
    }
    Ok(())
}

fn locked_ids(snapshot: &Path, table: &str) -> Vec<String> {
    let Ok(conn) =
        Connection::open_with_flags(snapshot, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };
    let sql = format!("SELECT id FROM {table} WHERE metadata_locked = 1");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    stmt.query_map([], |row| row.get::<_, String>(0))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

/// Copies artwork into `bundle/art/`: locked originals always, every original when `all`.
fn copy_artwork(paths: &DataPaths, bundle: &Path, all: bool) -> std::io::Result<Vec<String>> {
    let art = paths.art_dir();
    let dest = bundle.join(db_backup::BUNDLE_ART_DIR);
    let mut files = Vec::new();
    for entity in ["album", "artist"] {
        let original = art.join(entity).join("original");
        let rel = format!("{entity}/original");
        if all {
            if original.is_dir() {
                copy_tree(
                    &original,
                    &dest.join(entity).join("original"),
                    &rel,
                    &mut files,
                )?;
            }
            continue;
        }
        let table = if entity == "album" {
            "albums"
        } else {
            "artists"
        };
        for id in locked_ids(&bundle.join(db_backup::BUNDLE_DB_FILE), table) {
            let key = if entity == "album" {
                build_album_art_cache_key(&id)
            } else {
                build_artist_art_cache_key(&id)
            };
            let item = cache_item_dir(&original, &key);
            if !item.is_dir() {
                continue;
            }
            let item_rel = item
                .strip_prefix(&original)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            copy_tree(
                &item,
                &dest.join(entity).join("original").join(&item_rel),
                &format!("{rel}/{item_rel}"),
                &mut files,
            )?;
        }
    }
    files.sort();
    Ok(files)
}

fn copy_mixes(bundle: &Path) -> std::io::Result<Vec<db_backup::BundledMix>> {
    let db = bundle.join(db_backup::BUNDLE_DB_FILE);
    let Ok(conn) = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Ok(Vec::new());
    };
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, file_path FROM mix_outputs")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<_, _>>()
        })
        .unwrap_or_default();
    let dest = bundle.join(db_backup::BUNDLE_MIXES_DIR);
    let mut out = Vec::new();
    for (id, file_path) in rows {
        let src = Path::new(&file_path);
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !src.is_file() {
            continue;
        }
        let file = format!("{}-{name}", sanitize(&id));
        fs::create_dir_all(&dest)?;
        fs::copy(src, dest.join(&file))?;
        out.push(db_backup::BundledMix { id, file });
    }
    Ok(out)
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Makes a backup of the active database. Blocking; the caller holds the hub's job guard.
pub fn create_backup(
    db: &DbPool,
    paths: &DataPaths,
    hub: &BackupHub,
    reason: BackupReason,
) -> Result<BackupEntry, String> {
    let (settings, dir) = {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        (BackupSettings::load(&conn), backup_dir(&conn, paths))
    };
    let started = std::time::Instant::now();
    let result = make_bundle(paths, hub, reason, &settings, &dir);
    {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        record_result(&conn, &result);
    }
    match &result {
        Ok(entry) => tracing::info!(
            "Backup saved: {} ({} bytes, {} ms)",
            entry.path.display(),
            entry.size_bytes,
            started.elapsed().as_millis()
        ),
        Err(err) => tracing::warn!("Backup failed in {}: {err}", dir.display()),
    }
    result
}

fn make_bundle(
    paths: &DataPaths,
    hub: &BackupHub,
    reason: BackupReason,
    settings: &BackupSettings,
    dir: &Path,
) -> Result<BackupEntry, String> {
    let (stamp, created_at) = db_backup::utc_stamp(SystemTime::now());
    let partial =
        db_backup::begin_bundle(dir, &db_backup::bundle_name(&stamp, VERSION)).map_err(|err| {
            tracing::warn!("Backup folder {} is not writable: {err}", dir.display());
            format!(
                "The backup folder could not be written to: {}. Check that the disk is connected, or choose another folder.",
                dir.display()
            )
        })?;
    let built = (|| -> Result<PathBuf, BackupError> {
        hub.step("Copying the database");
        let mut manifest = db_backup::snapshot_into_bundle(
            &paths.db_file,
            &partial,
            reason,
            VERSION,
            &created_at,
        )?;
        manifest.key_included = db_backup::copy_key(&paths.key_file(), &partial)?;
        hub.step("Copying artwork");
        manifest.artwork = copy_artwork(paths, &partial, settings.include_all_artwork)?;
        if settings.include_mixes {
            hub.step("Copying rendered mixes");
            manifest.mixes = copy_mixes(&partial)?;
        }
        db_backup::finish_bundle(&partial, &manifest)
    })();
    let done = match built {
        Ok(done) => done,
        Err(err) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(err.to_string());
        }
    };
    if matches!(reason, BackupReason::Scheduled | BackupReason::Manual) {
        if let Err(err) = db_backup::prune_backups(
            dir,
            settings.keep,
            &[BackupReason::Scheduled, BackupReason::Manual],
        ) {
            tracing::warn!("Could not delete old backups in {}: {err}", dir.display());
        }
    }
    let name = done
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    db_backup::find_backup(dir, &name)
        .ok_or_else(|| "The backup could not be read back".to_string())
}

/// Runs a backup in the background. `false` when one is already running.
pub fn spawn_backup(db: DbPool, paths: DataPaths, hub: BackupHub, reason: BackupReason) -> bool {
    let Some(guard) = hub.try_begin("backup", "Starting") else {
        return false;
    };
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let _ = create_backup(&db, &paths, &hub, reason);
    });
    true
}

// ── Schedule ─────────────────────────────────────────────────────────────────

/// Local `(date, weekday 0=Sunday, HH:MM)` from SQLite's clock.
fn local_now(conn: &Connection) -> rusqlite::Result<(String, u8, String)> {
    conn.query_row(
        "SELECT date('now','localtime'), CAST(strftime('%w','now','localtime') AS INTEGER), strftime('%H:%M','now','localtime')",
        [],
        |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u8, row.get(2)?)),
    )
}

/// True when the slot for today is due and hasn't run yet.
fn scheduled_due(
    settings: &BackupSettings,
    today: &str,
    weekday: u8,
    now_hm: &str,
    last_slot: Option<&str>,
) -> bool {
    let day_matches = match settings.schedule {
        Schedule::Off => return false,
        Schedule::Daily => true,
        Schedule::Weekly => weekday == settings.day,
    };
    day_matches && now_hm >= settings.time.as_str() && last_slot != Some(today)
}

/// Days until the next slot (0 = later today).
fn days_until_next(
    settings: &BackupSettings,
    weekday: u8,
    now_hm: &str,
    ran_today: bool,
) -> Option<i64> {
    let today_open = now_hm < settings.time.as_str() && !ran_today;
    match settings.schedule {
        Schedule::Off => None,
        Schedule::Daily => Some(if today_open { 0 } else { 1 }),
        Schedule::Weekly => {
            let ahead = (i64::from(settings.day) - i64::from(weekday)).rem_euclid(7);
            Some(if ahead == 0 && !today_open { 7 } else { ahead })
        }
    }
}

/// Local `YYYY-MM-DDTHH:MM` of the next scheduled backup.
pub fn next_run(conn: &Connection, settings: &BackupSettings) -> Option<String> {
    let (today, weekday, now_hm) = local_now(conn).ok()?;
    let ran_today = get_setting(conn, LAST_SLOT_KEY).as_deref() == Some(today.as_str());
    let days = days_until_next(settings, weekday, &now_hm, ran_today)?;
    let date: String = conn
        .query_row(
            "SELECT date('now','localtime', ?1)",
            [format!("+{days} days")],
            |row| row.get(0),
        )
        .ok()?;
    Some(format!("{date}T{}", settings.time))
}

/// True when scheduled backups are on but the newest scheduled/manual backup is older than two intervals.
pub fn is_stale(
    conn: &Connection,
    settings: &BackupSettings,
    newest_created_at: Option<&str>,
) -> bool {
    let days = match settings.schedule {
        Schedule::Off => return false,
        Schedule::Daily => 2,
        Schedule::Weekly => 14,
    };
    let Ok(threshold) = conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)",
        [format!("-{days} days")],
        |row| row.get::<_, String>(0),
    ) else {
        return false;
    };
    match newest_created_at {
        Some(created) => created < threshold.as_str(),
        // Nothing yet: only stale once the schedule has had a full interval to run.
        None => false,
    }
}

/// Polls once a minute and runs the scheduled backup when its slot comes up.
pub fn start_backup_scheduler(
    db: DbPool,
    paths: DataPaths,
    hub: BackupHub,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {}
            }
            let check = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                    let settings = BackupSettings::load(&conn);
                    let (today, weekday, now_hm) = local_now(&conn).ok()?;
                    let last = get_setting(&conn, LAST_SLOT_KEY);
                    if !scheduled_due(&settings, &today, weekday, &now_hm, last.as_deref()) {
                        return None;
                    }
                    // Mark the slot first: a failure shows in the UI instead of retrying every minute.
                    let _ = set_setting(&conn, LAST_SLOT_KEY, &today);
                    Some(())
                })
                .await
            };
            if matches!(check, Ok(Some(()))) && !cancel.is_cancelled() {
                spawn_backup(
                    db.clone(),
                    paths.clone(),
                    hub.clone(),
                    BackupReason::Scheduled,
                );
            }
        }
    });
}

// ── Restore ──────────────────────────────────────────────────────────────────

/// Why a restore did not happen.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("A backup or restore is already running")]
    Busy,
    #[error("{0}")]
    Invalid(String),
    #[error("The restore failed and your current data was put back: {0}")]
    Failed(String),
    #[error("Background work kept the database open. Restart BoogieBox to finish the restore.")]
    Pending,
    #[error("{0}")]
    Unavailable(String),
}

/// A restore left for the next start (`restore-pending.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestore {
    pub bundle: PathBuf,
    #[serde(default)]
    pub remaps: Vec<PathRemap>,
}

/// Validates a bundle for restore on this build.
pub fn inspect(bundle: &Path) -> Result<Manifest, BackupError> {
    db_backup::validate_backup(bundle, &boogiebox_db::known_migration_ids())
}

fn move_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        // Different volume: copy, then remove.
        Err(_) => {
            fs::copy(src, dest)?;
            fs::remove_file(src)
        }
    }
}

/// Files a swap moved aside, for rollback.
#[derive(Default)]
struct Moved {
    pairs: Vec<(PathBuf, PathBuf)>,
    created: Vec<PathBuf>,
}

impl Moved {
    fn aside(&mut self, live: &Path, saved: &Path) -> std::io::Result<()> {
        if live.exists() {
            move_file(live, saved)?;
            self.pairs.push((live.to_path_buf(), saved.to_path_buf()));
        }
        Ok(())
    }

    fn rollback(self) {
        for path in self.created.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for (live, saved) in self.pairs.iter().rev() {
            let _ = fs::remove_file(live);
            if let Err(err) = move_file(saved, live) {
                tracing::error!(
                    "Restore rollback could not move {} back: {err}",
                    saved.display()
                );
            }
        }
    }
}

fn clear_thumbs(paths: &DataPaths, rel: &str) {
    // rel = <entity>/original/<shard>/<hash>/<file>
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() < 5 {
        return;
    }
    let thumb_root = paths.art_dir().join(parts[0]).join("thumb");
    let Ok(sizes) = fs::read_dir(&thumb_root) else {
        return;
    };
    for size in sizes.flatten() {
        let _ = fs::remove_dir_all(size.path().join(parts[2]).join(parts[3]));
    }
}

/// Puts a bundle's database, key, artwork and mixes into `paths`, moving current files under `saved` (when
/// given) so they can be rolled back. Applies remaps and signs everyone out. The database must be closed.
fn install_bundle(
    bundle: &Path,
    manifest: &Manifest,
    paths: &DataPaths,
    saved: Option<&Path>,
    remaps: &[PathRemap],
    moved: &mut Moved,
) -> Result<(), String> {
    let io = |what: &str, err: std::io::Error| format!("{what}: {err}");
    fs::create_dir_all(&paths.data_dir).map_err(|e| io("Could not create the data folder", e))?;

    // Database: stage beside the live file, check it, then swap.
    let staged = paths.data_dir.join(STAGED_DB_FILE);
    fs::copy(bundle.join(db_backup::BUNDLE_DB_FILE), &staged)
        .map_err(|e| io("Could not copy the database", e))?;
    moved.created.push(staged.clone());
    let (size, sha) = db_backup::sha256_file(&staged).map_err(|e| e.to_string())?;
    if size != manifest.db_size || !sha.eq_ignore_ascii_case(&manifest.db_sha256) {
        return Err("The copied database does not match the backup".into());
    }
    if let Some(saved) = saved {
        for suffix in ["", "-wal", "-shm"] {
            let name = format!("{}{suffix}", boogiebox_db::DB_FILE_NAME);
            moved
                .aside(&paths.data_dir.join(&name), &saved.join(&name))
                .map_err(|e| io("Could not save the current database", e))?;
        }
        moved
            .aside(&paths.key_file(), &saved.join(db_backup::BUNDLE_KEY_FILE))
            .map_err(|e| io("Could not save the current password key", e))?;
    }
    move_file(&staged, &paths.db_file)
        .map_err(|e| io("Could not put the restored database in place", e))?;
    moved.created.retain(|p| p != &staged);
    moved.created.push(paths.db_file.clone());

    // Key: the backup's, otherwise keep the current one (passwords made with it still work).
    let bundled_key = bundle.join(db_backup::BUNDLE_KEY_FILE);
    if bundled_key.is_file() {
        fs::copy(&bundled_key, paths.key_file())
            .map_err(|e| io("Could not restore the password key", e))?;
        moved.created.push(paths.key_file());
    } else if let Some(saved) = saved {
        let previous = saved.join(db_backup::BUNDLE_KEY_FILE);
        if previous.is_file() {
            fs::copy(&previous, paths.key_file())
                .map_err(|e| io("Could not keep the password key", e))?;
        }
    }

    // Artwork: replace each bundled file, saving the one it overwrites.
    let art = paths.art_dir();
    for rel in &manifest.artwork {
        if rel.split('/').any(|seg| seg == ".." || seg.is_empty()) {
            continue;
        }
        let src = bundle.join(db_backup::BUNDLE_ART_DIR).join(rel);
        let dest = art.join(rel);
        if !src.is_file() {
            continue;
        }
        if let Some(saved) = saved {
            moved
                .aside(&dest, &saved.join("art").join(rel))
                .map_err(|e| io("Could not save current artwork", e))?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io("Could not create the artwork folder", e))?;
        }
        fs::copy(&src, &dest).map_err(|e| io("Could not restore artwork", e))?;
        moved.created.push(dest);
        clear_thumbs(paths, rel);
    }

    let conn = Connection::open(&paths.db_file).map_err(|e| e.to_string())?;
    db_backup::apply_remaps(&conn, remaps)
        .map_err(|e| format!("Could not update music folders: {e}"))?;
    db_backup::clear_sessions(&conn).map_err(|e| e.to_string())?;
    restore_mixes(&conn, bundle, manifest, paths, moved)?;
    Ok(())
}

fn restore_mixes(
    conn: &Connection,
    bundle: &Path,
    manifest: &Manifest,
    paths: &DataPaths,
    moved: &mut Moved,
) -> Result<(), String> {
    for mix in &manifest.mixes {
        let src = bundle.join(db_backup::BUNDLE_MIXES_DIR).join(&mix.file);
        if !src.is_file() || mix.file.contains(['/', '\\']) {
            continue;
        }
        let recorded: Option<String> = conn
            .query_row(
                "SELECT file_path FROM mix_outputs WHERE id = ?1",
                [&mix.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(recorded) = recorded else { continue };
        let recorded = PathBuf::from(recorded);
        let target = match recorded.parent() {
            Some(parent) if parent.is_dir() => recorded.clone(),
            _ => {
                let name = recorded
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(&mix.file));
                paths.data_dir.join(MIX_OUTPUT_FALLBACK_DIR).join(name)
            }
        };
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(&src, &target).map_err(|e| format!("Could not restore a rendered mix: {e}"))?;
        moved.created.push(target.clone());
        if target != recorded {
            conn.execute(
                "UPDATE mix_outputs SET file_path = ?1 WHERE id = ?2",
                rusqlite::params![target.to_string_lossy(), mix.id],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Writes a manifest for the data a restore saved aside, so it can be restored itself ("Restore from another
/// folder…").
fn describe_saved(saved: &Path) {
    let db = saved.join(boogiebox_db::DB_FILE_NAME);
    if !db.is_file() {
        return;
    }
    let result = (|| -> Result<(), BackupError> {
        let (db_size, db_sha256) = db_backup::sha256_file(&db)?;
        let (migrations, library_roots, counts) = db_backup::database_facts(&db)?;
        let manifest = Manifest {
            format: db_backup::MANIFEST_FORMAT,
            app_version: VERSION.into(),
            created_at: db_backup::utc_stamp(SystemTime::now()).1,
            reason: BackupReason::PreRestore,
            migrations,
            db_size,
            db_sha256,
            key_included: saved.join(db_backup::BUNDLE_KEY_FILE).is_file(),
            library_roots,
            counts,
            artwork: Vec::new(),
            mixes: Vec::new(),
        };
        db_backup::write_manifest(saved, &manifest)
    })();
    if let Err(err) = result {
        tracing::warn!(
            "Could not describe the saved data in {}: {err}",
            saved.display()
        );
    }
}

fn saved_dir(paths: &DataPaths) -> PathBuf {
    let stamp = db_backup::utc_stamp(SystemTime::now()).0;
    let mut dir = paths.data_dir.join(format!("{PRE_RESTORE_PREFIX}{stamp}"));
    let mut n = 2;
    while dir.exists() {
        dir = paths
            .data_dir
            .join(format!("{PRE_RESTORE_PREFIX}{stamp}-{n}"));
        n += 1;
    }
    dir
}

/// Swaps the files of a closed database for a bundle's. Rolls back on failure. Returns the saved folder and
/// what was moved, so a caller can still roll back if the restored database won't open.
fn swap_closed(
    bundle: &Path,
    manifest: &Manifest,
    paths: &DataPaths,
    remaps: &[PathRemap],
) -> Result<(PathBuf, Moved), String> {
    let saved = saved_dir(paths);
    let mut moved = Moved::default();
    match install_bundle(bundle, manifest, paths, Some(&saved), remaps, &mut moved) {
        Ok(()) => {
            describe_saved(&saved);
            Ok((saved, moved))
        }
        Err(err) => {
            moved.rollback();
            Err(err)
        }
    }
}

/// Restores `bundle` over the live database without a restart: requests get 503 `restoring` meanwhile,
/// everyone is signed out, and current data is saved to `pre-restore-<stamp>/` in the data folder.
pub async fn restore_live(
    state: &SharedState,
    bundle: PathBuf,
    remaps: Vec<PathRemap>,
) -> Result<PathBuf, RestoreError> {
    let (db, paths, hub) =
        active(state).ok_or_else(|| RestoreError::Unavailable("No active database".into()))?;
    let _guard = hub
        .try_begin("restore", "Checking the backup")
        .ok_or(RestoreError::Busy)?;
    let manifest = {
        let bundle = bundle.clone();
        tokio::task::spawn_blocking(move || inspect(&bundle))
            .await
            .map_err(|e| RestoreError::Invalid(e.to_string()))?
            .map_err(|e| RestoreError::Invalid(e.to_string()))?
    };

    hub.step("Stopping background work");
    hub.set_restoring(true);
    let db_folder = {
        let mut s = state.write().unwrap_or_else(|p| p.into_inner());
        s.worker_cancel.cancel();
        s.remote.stop();
        s.remote.clear_play_env();
        crate::dlna::stop_dlna(&s.dlna_manager);
        s.subsonic.reset();
        s.plays.reset();
        s.activity.reset();
        s.db = None;
        s.db_folder
            .clone()
            .unwrap_or_else(|| paths.art_base.clone())
    };

    // Windows can't rename an open file: wait for every other handle to the database to go away.
    let deadline = tokio::time::Instant::now() + RELEASE_TIMEOUT;
    while Arc::strong_count(&db) > 1 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if Arc::strong_count(&db) > 1 {
        let pending = PendingRestore { bundle, remaps };
        let marker = paths.data_dir.join(PENDING_RESTORE_FILE);
        let _ = fs::write(
            &marker,
            serde_json::to_vec_pretty(&pending).unwrap_or_default(),
        );
        reactivate(state, db, &db_folder);
        hub.set_restoring(false);
        return Err(RestoreError::Pending);
    }
    let conn = match Arc::try_unwrap(db) {
        Ok(mutex) => mutex.into_inner().unwrap_or_else(|p| p.into_inner()),
        Err(db) => {
            reactivate(state, db, &db_folder);
            hub.set_restoring(false);
            return Err(RestoreError::Pending);
        }
    };
    drop(conn);

    hub.step("Restoring the backup");
    let swap = {
        let (bundle, paths, remaps) = (bundle.clone(), paths.clone(), remaps.clone());
        tokio::task::spawn_blocking(move || swap_closed(&bundle, &manifest, &paths, &remaps))
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
    };

    hub.step("Opening the restored database");
    let folder = db_folder.clone();
    let reopened = tokio::task::spawn_blocking(move || boogiebox_db::init_db(&folder))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    let result = match (swap, reopened) {
        (Ok((saved, _)), Ok(initialized)) => {
            let pool = Arc::new(Mutex::new(initialized.connection));
            let mut s = state.write().unwrap_or_else(|p| p.into_inner());
            crate::activate_db(&mut s, pool, db_folder.clone());
            tracing::info!(
                "Restored the backup {}; previous data saved to {}",
                bundle.display(),
                saved.display()
            );
            Ok(saved)
        }
        (swap, reopened) => {
            let message = match (swap, reopened) {
                (Err(err), _) => err,
                (Ok((_, moved)), Err(err)) => {
                    // The restored database won't open: put the original back.
                    moved.rollback();
                    format!("The restored database could not be opened: {err}")
                }
                (Ok(_), Ok(_)) => unreachable!(),
            };
            tracing::error!("Restore of {} failed: {message}", bundle.display());
            Err(RestoreError::Failed(message))
        }
    };
    if result.is_err() {
        // Whatever is in place now (the rolled-back original, or the restored file that failed to open)
        // must come back up; init_db on the original is the normal path.
        let folder = db_folder.clone();
        if let Ok(Ok(initialized)) =
            tokio::task::spawn_blocking(move || boogiebox_db::init_db(&folder)).await
        {
            let pool = Arc::new(Mutex::new(initialized.connection));
            let mut s = state.write().unwrap_or_else(|p| p.into_inner());
            crate::activate_db(&mut s, pool, db_folder.clone());
        }
    }
    hub.set_restoring(false);
    result
}

fn reactivate(state: &SharedState, db: DbPool, db_folder: &Path) {
    let mut s = state.write().unwrap_or_else(|p| p.into_inner());
    crate::activate_db(&mut s, db, db_folder.to_path_buf());
}

/// First-run restore into an empty data folder. Returns the paths it set up; the caller activates them.
pub fn restore_into_new_folder(
    bundle: &Path,
    db_folder: &Path,
    remaps: &[PathRemap],
) -> Result<DataPaths, RestoreError> {
    let manifest = inspect(bundle).map_err(|e| RestoreError::Invalid(e.to_string()))?;
    let paths = DataPaths::for_folder(db_folder)
        .ok_or_else(|| RestoreError::Invalid("Choose a database folder".into()))?;
    if paths.db_file.exists() {
        return Err(RestoreError::Invalid(
            "This folder already has a BoogieBox database. Choose an empty folder.".into(),
        ));
    }
    for remap in remaps {
        if !Path::new(remap.to.trim()).is_dir() {
            return Err(RestoreError::Invalid(format!(
                "{} does not exist on this machine",
                remap.to
            )));
        }
    }
    let mut moved = Moved::default();
    install_bundle(bundle, &manifest, &paths, None, remaps, &mut moved).map_err(|err| {
        moved.rollback();
        RestoreError::Failed(err)
    })?;
    Ok(paths)
}

/// Finishes a restore a live attempt left pending. Runs before the database is opened at startup.
pub fn complete_pending_restore(db_folder: &Path) {
    let Some(paths) = DataPaths::for_folder(db_folder) else {
        return;
    };
    let marker = paths.data_dir.join(PENDING_RESTORE_FILE);
    let Ok(raw) = fs::read(&marker) else {
        return;
    };
    let _ = fs::remove_file(&marker);
    let pending: PendingRestore = match serde_json::from_slice(&raw) {
        Ok(p) => p,
        Err(err) => {
            tracing::error!("Ignoring an unreadable pending restore: {err}");
            return;
        }
    };
    match inspect(&pending.bundle)
        .map_err(|e| e.to_string())
        .and_then(|manifest| swap_closed(&pending.bundle, &manifest, &paths, &pending.remaps))
    {
        Ok((saved, _)) => tracing::info!(
            "Finished the pending restore of {}; previous data saved to {}",
            pending.bundle.display(),
            saved.display()
        ),
        Err(err) => tracing::error!(
            "The pending restore of {} failed: {err}",
            pending.bundle.display()
        ),
    }
}

// ── Listing for the API ──────────────────────────────────────────────────────

/// A backup as the Settings page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub name: String,
    pub path: String,
    pub created_at: String,
    pub reason: BackupReason,
    pub app_version: String,
    pub size_bytes: u64,
    pub counts: db_backup::BackupCounts,
    pub key_included: bool,
    pub artwork_count: usize,
    pub mix_count: usize,
    pub library_roots: Vec<String>,
    /// Roots that don't exist on this machine (need a new location to restore).
    pub missing_roots: Vec<String>,
}

/// Summary for one bundle.
pub fn summarize(name: &str, path: &Path, manifest: &Manifest, size_bytes: u64) -> BackupSummary {
    BackupSummary {
        name: name.to_string(),
        path: path.to_string_lossy().into_owned(),
        created_at: manifest.created_at.clone(),
        reason: manifest.reason,
        app_version: manifest.app_version.clone(),
        size_bytes,
        counts: manifest.counts.clone(),
        key_included: manifest.key_included,
        artwork_count: manifest.artwork.len(),
        mix_count: manifest.mixes.len(),
        library_roots: manifest.library_roots.clone(),
        missing_roots: manifest
            .library_roots
            .iter()
            .filter(|root| !Path::new(root.as_str()).is_dir())
            .cloned()
            .collect(),
    }
}

impl From<&BackupEntry> for BackupSummary {
    fn from(entry: &BackupEntry) -> Self {
        summarize(&entry.name, &entry.path, &entry.manifest, entry.size_bytes)
    }
}

// ── CLI ──────────────────────────────────────────────────────────────────────

/// Handles `--list-backups` / `--restore`. `None` when the arguments are for the server.
pub fn run_cli(args: &[String]) -> Option<i32> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if !has("--list-backups") && !has("--restore") {
        return None;
    }
    let db_folder = value("--data-folder")
        .map(PathBuf::from)
        .or_else(|| crate::server_config::read_db_config().map(|c| PathBuf::from(c.db_folder)));

    if has("--list-backups") {
        let dir = match (
            value("--path"),
            db_folder.as_deref().and_then(DataPaths::for_folder),
        ) {
            (Some(path), _) => PathBuf::from(path),
            (None, Some(paths)) => {
                let configured = db_backup::configured_backup_path(&paths.db_file);
                db_backup::resolve_backup_dir(&paths.data_dir, configured.as_deref())
            }
            (None, None) => {
                eprintln!("No data folder is configured; pass --path DIR");
                return Some(2);
            }
        };
        let backups = db_backup::list_backups(&dir);
        if backups.is_empty() {
            println!("No backups in {}", dir.display());
        }
        for entry in backups {
            let m = &entry.manifest;
            println!(
                "{}  {}  v{}  {:?}  {} tracks, {} playlists, {} users  {} bytes",
                entry.name,
                m.created_at,
                m.app_version,
                m.reason,
                m.counts.tracks,
                m.counts.playlists,
                m.counts.users,
                entry.size_bytes
            );
        }
        return Some(0);
    }

    let Some(bundle) = value("--restore").map(PathBuf::from) else {
        eprintln!("--restore needs a backup folder");
        return Some(2);
    };
    let Some(paths) = db_folder.as_deref().and_then(DataPaths::for_folder) else {
        eprintln!("No data folder is configured; pass --data-folder DIR");
        return Some(2);
    };
    let mut remaps = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if arg == "--remap" {
            match args.get(i + 1).and_then(|v| v.split_once('=')) {
                Some((from, to)) => remaps.push(PathRemap {
                    from: from.into(),
                    to: to.into(),
                }),
                None => {
                    eprintln!("--remap needs OLD=NEW");
                    return Some(2);
                }
            }
        }
    }
    if data_folder_in_use(&paths.data_dir) {
        eprintln!("BoogieBox is running on {}. Stop it first, or restore from Settings › Advanced › Backup.", paths.data_dir.display());
        return Some(1);
    }
    let manifest = match inspect(&bundle) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Cannot restore {}: {err}", bundle.display());
            return Some(1);
        }
    };
    let result = if paths.db_file.exists() {
        swap_closed(&bundle, &manifest, &paths, &remaps)
            .map(|(saved, _)| format!("Current data saved to {}", saved.display()))
    } else {
        let mut moved = Moved::default();
        install_bundle(&bundle, &manifest, &paths, None, &remaps, &mut moved)
            .map(|()| "Restored into an empty data folder".to_string())
            .inspect_err(|_| moved.rollback())
    };
    match result {
        Ok(message) => {
            println!(
                "Restored {} into {}. {message}. Everyone signs in again.",
                bundle.display(),
                paths.data_dir.display()
            );
            Some(0)
        }
        Err(err) => {
            eprintln!("Restore failed: {err}");
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(schedule: Schedule, day: u8, time: &str) -> BackupSettings {
        BackupSettings {
            schedule,
            day,
            time: time.into(),
            ..BackupSettings::default()
        }
    }

    #[test]
    fn settings_round_trip_and_validate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT)",
        )
        .unwrap();
        assert_eq!(BackupSettings::load(&conn), BackupSettings::default());
        let custom = BackupSettings {
            schedule: Schedule::Daily,
            day: 3,
            time: "22:05".into(),
            keep: 9,
            path: r"E:\Backups".into(),
            include_all_artwork: true,
            include_mixes: true,
        };
        custom.save(&conn).unwrap();
        assert_eq!(BackupSettings::load(&conn), custom);
        for bad in [
            BackupSettings {
                day: 7,
                ..custom.clone()
            },
            BackupSettings {
                time: "24:00".into(),
                ..custom.clone()
            },
            BackupSettings {
                time: "3:30".into(),
                ..custom.clone()
            },
            BackupSettings {
                keep: 0,
                ..custom.clone()
            },
            BackupSettings {
                keep: 31,
                ..custom.clone()
            },
            BackupSettings {
                path: "backups".into(),
                ..custom.clone()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        assert!(custom.validate().is_ok());
        assert!(BackupSettings {
            path: r"\\nas\backups".into(),
            ..custom
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn schedule_due_and_next_run() {
        let weekly = settings(Schedule::Weekly, 0, "03:30");
        // Sunday 03:31, not run yet → due; already ran today → not due; Monday → not due.
        assert!(scheduled_due(&weekly, "2026-11-01", 0, "03:31", None));
        assert!(!scheduled_due(
            &weekly,
            "2026-11-01",
            0,
            "03:31",
            Some("2026-11-01")
        ));
        assert!(!scheduled_due(&weekly, "2026-11-01", 0, "03:29", None));
        assert!(!scheduled_due(&weekly, "2026-11-02", 1, "04:00", None));
        assert!(!scheduled_due(
            &settings(Schedule::Off, 0, "03:30"),
            "d",
            0,
            "23:59",
            None
        ));
        assert!(scheduled_due(
            &settings(Schedule::Daily, 0, "03:30"),
            "2026-11-02",
            1,
            "12:00",
            Some("2026-11-01")
        ));

        assert_eq!(days_until_next(&weekly, 0, "03:00", false), Some(0));
        assert_eq!(days_until_next(&weekly, 0, "04:00", true), Some(7));
        assert_eq!(days_until_next(&weekly, 3, "04:00", false), Some(4));
        assert_eq!(
            days_until_next(&settings(Schedule::Daily, 0, "03:30"), 2, "05:00", true),
            Some(1)
        );
        assert_eq!(
            days_until_next(&settings(Schedule::Daily, 0, "03:30"), 2, "01:00", false),
            Some(0)
        );
        assert_eq!(
            days_until_next(&settings(Schedule::Off, 0, "03:30"), 2, "01:00", false),
            None
        );
    }

    #[test]
    fn staleness_is_twice_the_interval() {
        let conn = Connection::open_in_memory().unwrap();
        let weekly = settings(Schedule::Weekly, 0, "03:30");
        let at = |offset: &str| -> String {
            conn.query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now',?1)",
                [offset],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(!is_stale(&conn, &weekly, Some(&at("-13 days"))));
        assert!(is_stale(&conn, &weekly, Some(&at("-15 days"))));
        assert!(is_stale(
            &conn,
            &settings(Schedule::Daily, 0, "03:30"),
            Some(&at("-3 days"))
        ));
        assert!(!is_stale(
            &conn,
            &settings(Schedule::Off, 0, "03:30"),
            Some(&at("-300 days"))
        ));
        assert!(!is_stale(&conn, &weekly, None));
    }

    #[test]
    fn single_flight_hub() {
        let hub = BackupHub::default();
        let guard = hub.try_begin("backup", "Starting").unwrap();
        assert!(hub.try_begin("restore", "x").is_none());
        hub.step("Copying artwork");
        assert_eq!(hub.running().unwrap().step, "Copying artwork");
        drop(guard);
        assert!(hub.running().is_none());
        assert!(hub.try_begin("restore", "x").is_some());
    }

    // ── Bundles, restore and rollback ───────────────────────────────────────

    use crate::test_support::{build_test_state, temp_db_path};

    struct Fixture {
        db: DbPool,
        paths: DataPaths,
        hub: BackupHub,
        state: SharedState,
    }

    fn fixture(name: &str) -> Fixture {
        let db_path = temp_db_path(name);
        let state: SharedState = Arc::new(std::sync::RwLock::new(build_test_state(&db_path)));
        let (db, paths, hub) = active(&state).unwrap();
        fs::write(paths.key_file(), "aa\n").unwrap();
        {
            let conn = db.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO libraries (id, path, name) VALUES ('lib-1', 'D:\\Music', 'Music');
                 INSERT INTO library_folders (id, library_id, path) VALUES ('f-1', 'lib-1', 'D:\\Music');
                 INSERT INTO artists (id, name) VALUES ('ar-1', 'Artist');
                 INSERT INTO albums (id, title, album_artist, artist_id, metadata_locked) VALUES ('al-1', 'Album', 'Artist', 'ar-1', 1);
                 INSERT INTO albums (id, title, album_artist, artist_id) VALUES ('al-2', 'Other', 'Artist', 'ar-1');
                 INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_path)
                   VALUES ('t-1', 'lib-1', 'ar-1', 'al-1', 'One', 'D:\\Music\\Artist\\one.flac');
                 INSERT INTO playlists (id, name, user_id) SELECT 'p-1', 'Road trip', id FROM users LIMIT 1;",
            )
            .unwrap();
        }
        Fixture {
            db,
            paths,
            hub,
            state,
        }
    }

    fn album_art_dir(paths: &DataPaths, id: &str) -> PathBuf {
        cache_item_dir(
            &paths.art_dir().join("album").join("original"),
            &build_album_art_cache_key(id),
        )
    }

    fn backup_now(f: &Fixture) -> BackupEntry {
        let _guard = f.hub.try_begin("backup", "Starting").unwrap();
        create_backup(&f.db, &f.paths, &f.hub, BackupReason::Manual).unwrap()
    }

    #[test]
    fn backup_includes_the_key_and_only_locked_artwork() {
        let f = fixture("bk-art");
        let locked = album_art_dir(&f.paths, "al-1");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("cover.jpg"), b"locked").unwrap();
        let fetched = album_art_dir(&f.paths, "al-2");
        fs::create_dir_all(&fetched).unwrap();
        fs::write(fetched.join("cover.jpg"), b"fetched").unwrap();

        let entry = backup_now(&f);
        assert!(entry.manifest.key_included);
        assert_eq!(entry.manifest.artwork.len(), 1);
        assert!(entry.manifest.artwork[0].starts_with("album/original/"));
        assert!(entry.manifest.artwork[0].ends_with("/cover.jpg"));
        assert!(entry
            .path
            .join("art")
            .join(&entry.manifest.artwork[0])
            .is_file());

        // "All original artwork" adds the fetched one too.
        {
            let conn = f.db.lock().unwrap();
            BackupSettings {
                include_all_artwork: true,
                ..BackupSettings::default()
            }
            .save(&conn)
            .unwrap();
        }
        let all = backup_now(&f);
        assert_eq!(all.manifest.artwork.len(), 2);
        assert!(last_failure(&f.db.lock().unwrap()).is_none());
    }

    #[test]
    fn failed_backup_is_recorded_and_cleared_by_the_next_success() {
        let f = fixture("bk-fail");
        let blocker = f.paths.data_dir.join("not-a-folder");
        fs::write(&blocker, b"x").unwrap();
        {
            let conn = f.db.lock().unwrap();
            BackupSettings {
                path: blocker.join("inner").to_string_lossy().into_owned(),
                ..BackupSettings::default()
            }
            .save(&conn)
            .unwrap();
        }
        let _guard = f.hub.try_begin("backup", "Starting").unwrap();
        assert!(create_backup(&f.db, &f.paths, &f.hub, BackupReason::Scheduled).is_err());
        let failure = last_failure(&f.db.lock().unwrap()).expect("failure recorded");
        assert!(
            failure.message.contains("could not be written"),
            "{}",
            failure.message
        );
        {
            let conn = f.db.lock().unwrap();
            BackupSettings::default().save(&conn).unwrap();
        }
        create_backup(&f.db, &f.paths, &f.hub, BackupReason::Manual).unwrap();
        assert!(last_failure(&f.db.lock().unwrap()).is_none());
    }

    #[test]
    fn swap_rolls_back_when_the_bundle_is_bad() {
        let f = fixture("bk-rollback");
        let entry = backup_now(&f);
        let before = fs::read(f.paths.key_file()).unwrap();
        // The database changes after validation: the staged copy no longer matches its checksum.
        let mut manifest = entry.manifest.clone();
        manifest.db_sha256 = "0".repeat(64);
        let conn = std::mem::replace(
            &mut *f.db.lock().unwrap(),
            Connection::open_in_memory().unwrap(),
        );
        drop(conn);
        assert!(swap_closed(&entry.path, &manifest, &f.paths, &[]).is_err());
        assert!(
            f.paths.db_file.is_file(),
            "the original database is still in place"
        );
        assert_eq!(fs::read(f.paths.key_file()).unwrap(), before);
        let conn = Connection::open(&f.paths.db_file).unwrap();
        let playlists: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(playlists, 1);
        assert!(!f.paths.data_dir.join(STAGED_DB_FILE).exists());
    }

    #[test]
    fn restore_brings_back_locked_artwork_and_clears_its_thumbnails() {
        let f = fixture("bk-art-restore");
        let locked = album_art_dir(&f.paths, "al-1");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("cover.jpg"), b"original upload").unwrap();
        let entry = backup_now(&f);
        // Later the upload is replaced and a thumbnail made from the replacement.
        fs::write(locked.join("cover.jpg"), b"replacement").unwrap();
        let rel = entry.manifest.artwork[0].clone();
        let parts: Vec<&str> = rel.split('/').collect();
        let thumb = f
            .paths
            .art_dir()
            .join("album")
            .join("thumb")
            .join("300")
            .join(parts[2])
            .join(parts[3]);
        fs::create_dir_all(&thumb).unwrap();
        fs::write(thumb.join("thumb.jpg"), b"thumb").unwrap();

        let conn = std::mem::replace(
            &mut *f.db.lock().unwrap(),
            Connection::open_in_memory().unwrap(),
        );
        drop(conn);
        let (saved, _) = swap_closed(&entry.path, &entry.manifest, &f.paths, &[]).unwrap();
        assert_eq!(
            fs::read(locked.join("cover.jpg")).unwrap(),
            b"original upload"
        );
        assert_eq!(
            fs::read(saved.join("art").join(&rel)).unwrap(),
            b"replacement",
            "the replaced file is saved"
        );
        assert!(!thumb.exists(), "stale thumbnail removed");
    }

    #[tokio::test]
    async fn requests_get_503_while_restoring() {
        let f = fixture("bk-503");
        let app = crate::build_app(f.state.read().unwrap().clone(), None);
        f.hub.set_restoring(true);
        let call = |uri: &'static str| {
            let app = app.clone();
            async move {
                use tower::ServiceExt;
                let req = axum::http::Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap();
                app.oneshot(req).await.unwrap().status()
            }
        };
        assert_eq!(
            call("/api/libraries").await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            call("/rest/ping").await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(call("/api/system/status").await, axum::http::StatusCode::OK);
        f.hub.set_restoring(false);
        assert_ne!(
            call("/api/libraries").await,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn a_held_database_leaves_the_restore_pending_for_startup() {
        let f = fixture("bk-pending");
        let entry = backup_now(&f);
        {
            let conn = f.db.lock().unwrap();
            conn.execute("DELETE FROM playlists", []).unwrap();
        }
        let holder = f.db.clone();
        let Fixture {
            db,
            paths,
            state,
            hub,
        } = f;
        drop(db);
        let result = restore_live(&state, entry.path.clone(), Vec::new()).await;
        assert!(matches!(result, Err(RestoreError::Pending)), "{result:?}");
        assert!(paths.data_dir.join(PENDING_RESTORE_FILE).is_file());
        assert!(!hub.is_restoring());
        assert!(
            state.read().unwrap().db.is_some(),
            "the current database is back in service"
        );

        // Next start: everything is closed, the restore finishes before the database opens.
        let pool = {
            let mut s = state.write().unwrap();
            s.worker_cancel.cancel();
            s.remote.stop();
            s.remote.clear_play_env();
            crate::dlna::stop_dlna(&s.dlna_manager);
            s.db.take().unwrap()
        };
        drop(holder);
        drop(state);
        for _ in 0..100 {
            if Arc::strong_count(&pool) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        drop(pool);
        complete_pending_restore(&paths.art_base);
        assert!(!paths.data_dir.join(PENDING_RESTORE_FILE).exists());
        let conn = Connection::open(&paths.db_file).unwrap();
        let playlists: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(playlists, 1);
    }

    #[test]
    fn first_run_restore_into_an_empty_folder_with_remaps() {
        let f = fixture("bk-first-run");
        let entry = backup_now(&f);
        let target_music = std::env::temp_dir().join(format!("bb-music-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&target_music).unwrap();
        let new_folder = std::env::temp_dir().join(format!("bb-new-data-{}", uuid::Uuid::now_v7()));
        let remaps = vec![PathRemap {
            from: r"D:\Music".into(),
            to: target_music.to_string_lossy().into_owned(),
        }];

        // A folder that doesn't exist on this machine is refused.
        let bad = vec![PathRemap {
            from: r"D:\Music".into(),
            to: "/no/such/folder".into(),
        }];
        assert!(matches!(
            restore_into_new_folder(&entry.path, &new_folder, &bad),
            Err(RestoreError::Invalid(_))
        ));

        let paths = restore_into_new_folder(&entry.path, &new_folder, &remaps).unwrap();
        assert_eq!(fs::read(paths.key_file()).unwrap(), b"aa\n");
        let conn = Connection::open(&paths.db_file).unwrap();
        let track: String = conn
            .query_row("SELECT file_path FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert!(
            track.starts_with(target_music.to_string_lossy().as_ref()),
            "{track}"
        );
        assert!(track.ends_with("one.flac"));
        drop(conn);
        // A folder that already has a database is refused.
        assert!(matches!(
            restore_into_new_folder(&entry.path, &new_folder, &remaps),
            Err(RestoreError::Invalid(_))
        ));
    }

    #[test]
    fn the_cli_refuses_while_the_server_holds_the_data_folder() {
        let dir = std::env::temp_dir().join(format!("bb-lock-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        assert!(!data_folder_in_use(&dir));
        // What `hold_data_lock` does for a running server (the global slot is shared with other tests).
        let server = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .unwrap();
        server.try_lock().unwrap();
        assert!(data_folder_in_use(&dir));
        let args: Vec<String> = ["--restore", "x", "--data-folder", dir.to_str().unwrap()]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_cli(&args), Some(1));
        drop(server);
        assert!(!data_folder_in_use(&dir));
        assert_eq!(run_cli(&["--port".to_string()]), None);
    }
}

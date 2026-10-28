//! Database backups: `VACUUM INTO` snapshots, the bundle manifest, validation, retention and path remapping.
//!
//! A bundle is a folder `boogiebox-<YYYYMMDD-HHMMSS>-v<version>/` holding `boogiebox.db`, `subsonic.key` (when
//! present), `manifest.json` and optional `art/` and `mixes/` copies. It is written as `<name>.partial/` and renamed
//! once complete, so a listed bundle is always whole. Copying artwork and mixes is the server's job; this module
//! owns everything about the database file itself.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bundle folder prefix; retention only ever touches folders that start with it and hold a manifest.
pub const BACKUP_PREFIX: &str = "boogiebox-";
/// Suffix of a bundle that is still being written.
pub const PARTIAL_SUFFIX: &str = ".partial";
/// Manifest file inside a bundle.
pub const MANIFEST_FILE: &str = "manifest.json";
/// Database file inside a bundle.
pub const BUNDLE_DB_FILE: &str = "boogiebox.db";
/// Subsonic/scrobbling password key inside a bundle (same name as in the data folder).
pub const BUNDLE_KEY_FILE: &str = "subsonic.key";
/// Artwork copies inside a bundle, mirroring the data folder's `art/` layout.
pub const BUNDLE_ART_DIR: &str = "art";
/// Rendered-mix copies inside a bundle.
pub const BUNDLE_MIXES_DIR: &str = "mixes";
/// Manifest format written by this build.
pub const MANIFEST_FORMAT: u32 = 1;
/// Before-upgrade snapshots kept, separately from the scheduled/manual limit.
pub const PRE_MIGRATION_KEEP: usize = 2;
/// Default `backupKeep`.
pub const DEFAULT_KEEP: usize = 4;
/// Folder under the data folder used when `backupPath` is empty.
pub const DEFAULT_BACKUP_DIR: &str = "backups";
/// `settings` key holding the backup folder.
pub const BACKUP_PATH_SETTING: &str = "backupPath";

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupReason {
    /// The weekly/daily schedule.
    Scheduled,
    /// "Back up now".
    Manual,
    /// Startup, before pending migrations ran.
    PreMigration,
    /// Saved data before a restore replaced it.
    PreRestore,
}

/// Row counts shown with a backup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCounts {
    pub tracks: i64,
    pub albums: i64,
    pub playlists: i64,
    pub users: i64,
}

/// A rendered mix copied into the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundledMix {
    /// `mix_outputs.id`.
    pub id: String,
    /// File name under `mixes/`.
    pub file: String,
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub app_version: String,
    /// UTC, RFC 3339.
    pub created_at: String,
    pub reason: BackupReason,
    /// Tracked migration ids applied to the snapshot.
    pub migrations: Vec<String>,
    pub db_size: u64,
    pub db_sha256: String,
    pub key_included: bool,
    /// Library folder paths as stored (absolute paths on the source machine).
    pub library_roots: Vec<String>,
    pub counts: BackupCounts,
    /// Artwork files, relative to the bundle's `art/` folder with `/` separators.
    #[serde(default)]
    pub artwork: Vec<String>,
    #[serde(default)]
    pub mixes: Vec<BundledMix>,
}

/// A finished bundle found in a backup folder.
#[derive(Debug, Clone, PartialEq)]
pub struct BackupEntry {
    pub name: String,
    pub path: PathBuf,
    pub manifest: Manifest,
    /// Bytes on disk for the whole bundle.
    pub size_bytes: u64,
}

/// Why a backup can't be used or made.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("the backup has no readable manifest: {0}")]
    Manifest(String),
    #[error("the backup's database is damaged")]
    Integrity,
    #[error("the backup's database does not match its checksum")]
    Checksum,
    #[error("the backup was made by a newer version of BoogieBox ({0})")]
    Newer(String),
    #[error("not a BoogieBox backup folder")]
    NotABackup,
}

// ── Names and times ──────────────────────────────────────────────────────────

/// UTC `(YYYYMMDD-HHMMSS, RFC 3339)` for `time`.
pub fn utc_stamp(time: SystemTime) -> (String, String) {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    (
        format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}"),
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z"),
    )
}

/// `boogiebox-<stamp>-v<version>`.
pub fn bundle_name(stamp: &str, version: &str) -> String {
    format!("{BACKUP_PREFIX}{stamp}-v{version}")
}

/// True for a finished bundle name this build would write: no path segments, no `.partial`.
pub fn is_bundle_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(BACKUP_PREFIX) else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.len() < 17 || name.ends_with(PARTIAL_SUFFIX) {
        return false;
    }
    let stamp_ok = bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'-';
    stamp_ok
        && rest[16..].contains('v')
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'+')
        && !rest.contains("..")
}

/// The backup folder for a data folder: `backupPath` when set, otherwise `<data folder>/backups`.
pub fn resolve_backup_dir(data_dir: &Path, configured: Option<&str>) -> PathBuf {
    match configured.map(str::trim).filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => data_dir.join(DEFAULT_BACKUP_DIR),
    }
}

// ── Snapshot ─────────────────────────────────────────────────────────────────

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "busy_timeout", 30_000)?;
    Ok(conn)
}

/// Copies the live database at `source` into `dest` with `VACUUM INTO`, on a connection of its own, so the
/// server's connection keeps working (in WAL mode it is never blocked).
pub fn snapshot_database(source: &Path, dest: &Path) -> Result<(), BackupError> {
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    let conn = open_read_only(source)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    drop(conn);
    if !integrity_ok(dest)? {
        return Err(BackupError::Integrity);
    }
    // Windows flushes only through a handle opened for writing.
    fs::OpenOptions::new().write(true).open(dest)?.sync_all()?;
    Ok(())
}

/// `PRAGMA integrity_check` on a database file.
pub fn integrity_ok(path: &Path) -> Result<bool, BackupError> {
    let conn = open_read_only(path)?;
    let result: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    Ok(result == "ok")
}

/// `(size, lowercase hex SHA-256)` of a file.
pub fn sha256_file(path: &Path) -> Result<(u64, String), BackupError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

fn count(conn: &Connection, table: &str) -> i64 {
    if !table_exists(conn, table) {
        return 0;
    }
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap_or(0)
}

/// Applied migrations, library roots and counts of a database file (tolerates old schemas).
pub fn database_facts(
    path: &Path,
) -> Result<(Vec<String>, Vec<String>, BackupCounts), BackupError> {
    let conn = open_read_only(path)?;
    let migrations = if table_exists(&conn, "schema_migrations") {
        let mut stmt = conn.prepare("SELECT id FROM schema_migrations ORDER BY id")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    } else {
        Vec::new()
    };
    let roots = if table_exists(&conn, "library_folders") {
        let mut stmt = conn.prepare("SELECT path FROM library_folders ORDER BY path")?;
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        paths
    } else if table_exists(&conn, "libraries") {
        let mut stmt = conn.prepare("SELECT path FROM libraries ORDER BY path")?;
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        paths
    } else {
        Vec::new()
    };
    let counts = BackupCounts {
        tracks: count(&conn, "tracks"),
        albums: count(&conn, "albums"),
        playlists: count(&conn, "playlists"),
        users: count(&conn, "users"),
    };
    Ok((migrations, roots, counts))
}

// ── Bundles ──────────────────────────────────────────────────────────────────

/// Writes `manifest.json` and syncs it.
pub fn write_manifest(bundle: &Path, manifest: &Manifest) -> Result<(), BackupError> {
    let json = serde_json::to_vec_pretty(manifest)
        .map_err(|err| BackupError::Manifest(err.to_string()))?;
    let path = bundle.join(MANIFEST_FILE);
    let mut file = fs::File::create(&path)?;
    file.write_all(&json)?;
    file.sync_all()?;
    Ok(())
}

/// Reads a bundle's manifest.
pub fn read_manifest(bundle: &Path) -> Result<Manifest, BackupError> {
    let raw = fs::read(bundle.join(MANIFEST_FILE))
        .map_err(|err| BackupError::Manifest(err.to_string()))?;
    serde_json::from_slice(&raw).map_err(|err| BackupError::Manifest(err.to_string()))
}

/// Copies the password key into the bundle with owner-only permissions where the OS supports it.
pub fn copy_key(src: &Path, bundle: &Path) -> Result<bool, BackupError> {
    if !src.is_file() {
        return Ok(false);
    }
    let dest = bundle.join(BUNDLE_KEY_FILE);
    fs::copy(src, &dest)?;
    restrict_permissions(&dest);
    Ok(true)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

/// Creates `<dir>/<name>.partial/`, removing a stale one, and returns it.
pub fn begin_bundle(dir: &Path, name: &str) -> Result<PathBuf, BackupError> {
    fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{name}{PARTIAL_SUFFIX}"));
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    fs::create_dir_all(&partial)?;
    Ok(partial)
}

/// Snapshots `source_db` into a partial bundle and fills the database half of its manifest.
pub fn snapshot_into_bundle(
    source_db: &Path,
    partial: &Path,
    reason: BackupReason,
    app_version: &str,
    created_at: &str,
) -> Result<Manifest, BackupError> {
    let db = partial.join(BUNDLE_DB_FILE);
    snapshot_database(source_db, &db)?;
    let (db_size, db_sha256) = sha256_file(&db)?;
    let (migrations, library_roots, counts) = database_facts(&db)?;
    Ok(Manifest {
        format: MANIFEST_FORMAT,
        app_version: app_version.to_string(),
        created_at: created_at.to_string(),
        reason,
        migrations,
        db_size,
        db_sha256,
        key_included: false,
        library_roots,
        counts,
        artwork: Vec::new(),
        mixes: Vec::new(),
    })
}

/// Writes the manifest and renames `<name>.partial` to its final name (adding `-2`, `-3`… if taken).
pub fn finish_bundle(partial: &Path, manifest: &Manifest) -> Result<PathBuf, BackupError> {
    write_manifest(partial, manifest)?;
    let name = partial
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(PARTIAL_SUFFIX))
        .ok_or(BackupError::NotABackup)?
        .to_string();
    let dir = partial.parent().ok_or(BackupError::NotABackup)?;
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        // Same second: keep the stamp sortable and the version last.
        let (stamp, version) = name.rsplit_once("-v").unwrap_or((&name, ""));
        dest = dir.join(format!("{stamp}-{n}-v{version}"));
        n += 1;
    }
    fs::rename(partial, &dest)?;
    Ok(dest)
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Finished bundles in `dir`, newest first. Partial, foreign and unreadable folders are skipped.
pub fn list_backups(dir: &Path) -> Vec<BackupEntry> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<BackupEntry> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            if !is_bundle_name(&name) {
                return None;
            }
            let path = entry.path();
            let manifest = read_manifest(&path).ok()?;
            Some(BackupEntry {
                size_bytes: dir_size(&path),
                name,
                path,
                manifest,
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.manifest
            .created_at
            .cmp(&a.manifest.created_at)
            .then(b.name.cmp(&a.name))
    });
    found
}

/// Finds a bundle by name in `dir`.
pub fn find_backup(dir: &Path, name: &str) -> Option<BackupEntry> {
    if !is_bundle_name(name) {
        return None;
    }
    list_backups(dir)
        .into_iter()
        .find(|entry| entry.name == name)
}

/// Deletes the oldest bundles with one of `reasons` beyond `keep`. Returns the deleted names.
pub fn prune_backups(
    dir: &Path,
    keep: usize,
    reasons: &[BackupReason],
) -> Result<Vec<String>, BackupError> {
    let mut deleted = Vec::new();
    let matching: Vec<BackupEntry> = list_backups(dir)
        .into_iter()
        .filter(|entry| reasons.contains(&entry.manifest.reason))
        .collect();
    for entry in matching.into_iter().skip(keep) {
        fs::remove_dir_all(&entry.path)?;
        deleted.push(entry.name);
    }
    Ok(deleted)
}

/// Checks a bundle before a restore: manifest, checksum, integrity, and that every migration is known here.
pub fn validate_backup(bundle: &Path, known_migrations: &[&str]) -> Result<Manifest, BackupError> {
    let manifest = read_manifest(bundle)?;
    let db = bundle.join(BUNDLE_DB_FILE);
    if !db.is_file() {
        return Err(BackupError::NotABackup);
    }
    if manifest
        .migrations
        .iter()
        .any(|id| !known_migrations.contains(&id.as_str()))
    {
        return Err(BackupError::Newer(manifest.app_version.clone()));
    }
    let (size, sha) = sha256_file(&db)?;
    if size != manifest.db_size || !sha.eq_ignore_ascii_case(&manifest.db_sha256) {
        return Err(BackupError::Checksum);
    }
    if !integrity_ok(&db)? {
        return Err(BackupError::Integrity);
    }
    Ok(manifest)
}

// ── Path remapping ───────────────────────────────────────────────────────────

/// A library root moved from `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRemap {
    pub from: String,
    pub to: String,
}

fn looks_windows(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with(r"\\") || (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
}

fn trim_separators(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
}

/// Rewrites `path` when it is `from` or under it. Windows sources match case-insensitively with either
/// separator; the rest of the path takes the separator style of `to`.
pub fn remap_path(path: &str, from: &str, to: &str) -> Option<String> {
    let from = trim_separators(from);
    let to = trim_separators(to);
    if from.is_empty() {
        return None;
    }
    let windows = looks_windows(from);
    let norm = |s: &str| -> String {
        if windows {
            s.replace('\\', "/").to_lowercase()
        } else {
            s.to_string()
        }
    };
    let (np, nf) = (norm(path), norm(from));
    let rest = if np == nf {
        ""
    } else if np.len() > nf.len() && np.starts_with(&nf) && np.as_bytes()[nf.len()] == b'/' {
        path.get(from.len()..)?
    } else {
        return None;
    };
    // `rest` keeps its own separators; switch them to the target's style.
    let to_sep = if looks_windows(to) { '\\' } else { '/' };
    let rest: String = rest
        .chars()
        .map(|c| if c == '/' || c == '\\' { to_sep } else { c })
        .collect();
    Some(format!("{to}{rest}"))
}

fn remap_any(path: &str, remaps: &[PathRemap]) -> Option<String> {
    remaps
        .iter()
        .find_map(|remap| remap_path(path, &remap.from, &remap.to))
}

/// Rows rewritten by [`apply_remaps`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemapCounts {
    pub folders: usize,
    pub tracks: usize,
    pub mixes: usize,
}

fn remap_column(
    conn: &Connection,
    table: &str,
    key: &str,
    column: &str,
    remaps: &[PathRemap],
) -> rusqlite::Result<usize> {
    if !table_exists(conn, table) {
        return Ok(0);
    }
    let rows: Vec<(rusqlite::types::Value, String)> = {
        let mut stmt = conn.prepare(&format!("SELECT {key}, {column} FROM {table}"))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut update = conn.prepare(&format!(
        "UPDATE {table} SET {column} = ?1 WHERE {key} = ?2"
    ))?;
    let mut changed = 0;
    for (id, path) in rows {
        if let Some(new_path) = remap_any(&path, remaps) {
            if new_path != path {
                update.execute(rusqlite::params![new_path, id])?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// Rewrites library roots, track paths (and their search index) and rendered-mix paths, in one transaction.
pub fn apply_remaps(conn: &Connection, remaps: &[PathRemap]) -> rusqlite::Result<RemapCounts> {
    if remaps.is_empty() {
        return Ok(RemapCounts::default());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let mut counts = RemapCounts {
            folders: remap_column(conn, "library_folders", "id", "path", remaps)?,
            ..RemapCounts::default()
        };
        remap_column(conn, "libraries", "id", "path", remaps)?;
        counts.tracks = remap_column(conn, "tracks", "id", "file_path", remaps)?;
        remap_column(conn, "tracks_fts", "rowid", "file_path", remaps)?;
        counts.mixes = remap_column(conn, "mix_outputs", "id", "file_path", remaps)?;
        Ok(counts)
    })();
    match result {
        Ok(counts) => {
            conn.execute_batch("COMMIT")?;
            Ok(counts)
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// Signs everybody out: restored sign-ins must not be reused.
pub fn clear_sessions(conn: &Connection) -> rusqlite::Result<usize> {
    if !table_exists(conn, "sessions") {
        return Ok(0);
    }
    conn.execute("DELETE FROM sessions", [])
}

/// The `backupPath` setting of an existing database file, read without migrating it.
pub fn configured_backup_path(db_file: &Path) -> Option<String> {
    let conn = open_read_only(db_file).ok()?;
    if !table_exists(&conn, "settings") {
        return None;
    }
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [BACKUP_PATH_SETTING],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-backup-{name}-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_db(dir: &Path) -> PathBuf {
        let initialized = crate::init_db(dir).expect("init");
        initialized
            .connection
            .execute_batch(
                "INSERT INTO libraries (id, path, name) VALUES ('lib-1', 'D:\\Music', 'Music');
                 INSERT INTO library_folders (id, library_id, path, position) VALUES ('f-1', 'lib-1', 'D:\\Music', 0);
                 INSERT INTO artists (id, name) VALUES ('ar-1', 'Artist');
                 INSERT INTO albums (id, title, album_artist, artist_id) VALUES ('al-1', 'Album', 'Artist', 'ar-1');
                 INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_path)
                   VALUES ('t-1', 'lib-1', 'ar-1', 'al-1', 'One', 'D:\\Music\\Artist\\one.flac');
                 INSERT INTO tracks_fts (track_id, title, artist, album, genre, composer, file_path)
                   VALUES ('t-1', 'One', 'Artist', 'Album', '', '', 'D:\\Music\\Artist\\one.flac');",
            )
            .unwrap();
        initialized.db_path
    }

    #[test]
    fn stamps_are_utc_and_sortable() {
        let t = UNIX_EPOCH + Duration::from_secs(1_793_188_326);
        let (stamp, rfc) = utc_stamp(t);
        assert_eq!(stamp, "20261028-115206");
        assert_eq!(rfc, "2026-10-28T11:52:06Z");
        assert_eq!(utc_stamp(UNIX_EPOCH).0, "19700101-000000");
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(951_782_400)).1,
            "2000-02-29T00:00:00Z"
        );
    }

    #[test]
    fn bundle_names_are_validated() {
        let name = bundle_name("20261028-033000", "0.9.43");
        assert_eq!(name, "boogiebox-20261028-033000-v0.9.43");
        assert!(is_bundle_name(&name));
        assert!(is_bundle_name("boogiebox-20261028-033000-2-v0.9.43"));
        for bad in [
            "boogiebox-20261028-033000-v0.9.43.partial",
            "boogiebox-2026102-033000-v1",
            "boogiebox-20261028-033000-v../x",
            "boogiebox-20261028-033000-v1/..",
            "other-20261028-033000-v1",
            "boogiebox-20261028-033000",
            "pre-restore-20261028-101500",
        ] {
            assert!(!is_bundle_name(bad), "{bad}");
        }
    }

    #[test]
    fn snapshot_bundle_round_trip_lists_and_validates() {
        let data = temp_dir("roundtrip");
        let db = sample_db(&data);
        let dir = data.join("backups");
        let partial = begin_bundle(&dir, &bundle_name("20261028-033000", "0.9.43")).unwrap();
        // A partial bundle is never listed.
        assert!(list_backups(&dir).is_empty());
        let manifest = snapshot_into_bundle(
            &db,
            &partial,
            BackupReason::Manual,
            "0.9.43",
            "2026-10-28T03:30:00Z",
        )
        .unwrap();
        assert_eq!(manifest.counts.tracks, 1);
        assert_eq!(manifest.library_roots, vec!["D:\\Music".to_string()]);
        assert!(manifest
            .migrations
            .contains(&"2026-10-27-library-missing-kept".to_string()));
        let done = finish_bundle(&partial, &manifest).unwrap();
        assert!(!partial.exists());

        let listed = list_backups(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].manifest, manifest);
        assert!(listed[0].size_bytes >= manifest.db_size);

        let known = crate::known_migration_ids();
        assert_eq!(validate_backup(&done, &known).unwrap(), manifest);

        // Unknown (newer) migration ids are refused.
        let mut newer = manifest.clone();
        newer.migrations.push("2099-01-01-future".into());
        write_manifest(&done, &newer).unwrap();
        assert!(matches!(
            validate_backup(&done, &known),
            Err(BackupError::Newer(_))
        ));

        // A changed database fails the checksum.
        write_manifest(&done, &manifest).unwrap();
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(done.join(BUNDLE_DB_FILE))
            .unwrap();
        file.write_all(b"x").unwrap();
        drop(file);
        assert!(matches!(
            validate_backup(&done, &known),
            Err(BackupError::Checksum)
        ));
    }

    #[test]
    fn snapshot_is_consistent_while_the_source_is_written() {
        let data = temp_dir("concurrent");
        let db = sample_db(&data);
        let writer = Connection::open(&db).unwrap();
        writer.execute_batch("BEGIN; INSERT INTO playlists (id, name, user_id) SELECT 'p-1', 'Open', id FROM users LIMIT 1;").unwrap();
        let dest = data.join("snap.db");
        // The uncommitted write is not in the snapshot, and the writer is not blocked.
        snapshot_database(&db, &dest).unwrap();
        writer.execute_batch("COMMIT").unwrap();
        let (_, _, counts) = database_facts(&dest).unwrap();
        assert_eq!(counts.playlists, 0);
        assert!(integrity_ok(&dest).unwrap());
    }

    #[test]
    fn same_second_bundles_get_distinct_names() {
        let data = temp_dir("collide");
        let db = sample_db(&data);
        let dir = data.join("backups");
        let name = bundle_name("20261028-033000", "0.9.43");
        let mut names = Vec::new();
        for _ in 0..2 {
            let partial = begin_bundle(&dir, &name).unwrap();
            let m = snapshot_into_bundle(
                &db,
                &partial,
                BackupReason::Manual,
                "0.9.43",
                "2026-10-28T03:30:00Z",
            )
            .unwrap();
            names.push(finish_bundle(&partial, &m).unwrap());
        }
        assert_ne!(names[0], names[1]);
        assert!(names
            .iter()
            .all(|p| is_bundle_name(p.file_name().unwrap().to_str().unwrap())));
        assert_eq!(list_backups(&dir).len(), 2);
    }

    #[test]
    fn retention_keeps_the_newest_per_reason_and_ignores_foreign_folders() {
        let data = temp_dir("retention");
        let db = sample_db(&data);
        let dir = data.join("backups");
        let make = |stamp: &str, reason: BackupReason| {
            let partial = begin_bundle(&dir, &bundle_name(stamp, "1")).unwrap();
            let created = format!("2026-10-{}T03:30:00Z", &stamp[6..8]);
            let m = snapshot_into_bundle(&db, &partial, reason, "1", &created).unwrap();
            finish_bundle(&partial, &m).unwrap();
        };
        for day in 20..26 {
            make(&format!("202610{day}-033000"), BackupReason::Scheduled);
        }
        make("20261019-033000", BackupReason::PreMigration);
        fs::create_dir_all(dir.join("my-notes")).unwrap();
        fs::create_dir_all(dir.join("boogiebox-20261001-000000-v1")).unwrap(); // no manifest

        let deleted =
            prune_backups(&dir, 4, &[BackupReason::Scheduled, BackupReason::Manual]).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(deleted
            .iter()
            .all(|n| n.contains("2026102") && (n.contains("1020") || n.contains("1021"))));
        let left: Vec<BackupReason> = list_backups(&dir)
            .iter()
            .map(|e| e.manifest.reason)
            .collect();
        assert_eq!(
            left.iter()
                .filter(|r| **r == BackupReason::Scheduled)
                .count(),
            4
        );
        assert!(left.contains(&BackupReason::PreMigration));
        assert!(dir.join("my-notes").exists());
        assert!(dir.join("boogiebox-20261001-000000-v1").exists());
    }

    #[test]
    fn remap_handles_windows_unc_and_separators() {
        assert_eq!(
            remap_path(r"D:\Music\Artist\one.flac", r"D:\Music", "/mnt/music").as_deref(),
            Some("/mnt/music/Artist/one.flac")
        );
        assert_eq!(
            remap_path(r"d:\music\Artist\one.flac", r"D:\Music\", "/mnt/music/").as_deref(),
            Some("/mnt/music/Artist/one.flac")
        );
        assert_eq!(
            remap_path(
                r"\\nas\music\Library\a\b.mp3",
                r"\\NAS\Music\Library",
                "/mnt/nas/music/Library"
            )
            .as_deref(),
            Some("/mnt/nas/music/Library/a/b.mp3")
        );
        assert_eq!(
            remap_path(r"D:\Music", r"D:\Music", "/mnt/music").as_deref(),
            Some("/mnt/music")
        );
        // Linux → Windows.
        assert_eq!(
            remap_path("/srv/music/x/y.flac", "/srv/music", r"E:\Music").as_deref(),
            Some(r"E:\Music\x\y.flac")
        );
        // Linux paths are case-sensitive and must match whole segments.
        assert_eq!(remap_path("/srv/Music/x.flac", "/srv/music", "/m"), None);
        assert_eq!(remap_path(r"D:\Musical\x.flac", r"D:\Music", "/m"), None);
        assert_eq!(remap_path("/srv/musical/x.flac", "/srv/music", "/m"), None);
    }

    #[test]
    fn apply_remaps_rewrites_roots_tracks_search_and_mixes() {
        let data = temp_dir("remap");
        let db = sample_db(&data);
        let conn = Connection::open(&db).unwrap();
        let counts = apply_remaps(
            &conn,
            &[PathRemap {
                from: r"D:\Music".into(),
                to: "/mnt/music".into(),
            }],
        )
        .unwrap();
        assert_eq!(counts.folders, 1);
        assert_eq!(counts.tracks, 1);
        let track: String = conn
            .query_row("SELECT file_path FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(track, "/mnt/music/Artist/one.flac");
        let fts: String = conn
            .query_row(
                "SELECT file_path FROM tracks_fts WHERE track_id = 't-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts, "/mnt/music/Artist/one.flac");
        let root: String = conn
            .query_row("SELECT path FROM libraries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(root, "/mnt/music");
        assert_eq!(clear_sessions(&conn).unwrap(), 0);
    }

    #[test]
    fn upgrade_snapshot_is_taken_once_per_upgrade() {
        let data = temp_dir("pre-migration");
        let db = sample_db(&data);
        fs::write(data.join(BUNDLE_KEY_FILE), "00ff\n").unwrap();
        let dir = data.join(DEFAULT_BACKUP_DIR);
        // A fresh database and an up-to-date reopen take no snapshot.
        drop(crate::init_db(&data).unwrap());
        assert!(list_backups(&dir).is_empty());

        // Pretend this build added a migration the database hasn't run.
        Connection::open(&db)
            .unwrap()
            .execute(
                "DELETE FROM schema_migrations WHERE id = '2026-10-27-library-missing-kept'",
                [],
            )
            .unwrap();
        drop(crate::init_db(&data).unwrap());
        let listed = list_backups(&dir);
        assert_eq!(listed.len(), 1);
        let manifest = &listed[0].manifest;
        assert_eq!(manifest.reason, BackupReason::PreMigration);
        assert!(manifest.key_included);
        assert!(!manifest
            .migrations
            .contains(&"2026-10-27-library-missing-kept".to_string()));
        assert_eq!(manifest.counts.tracks, 1);

        drop(crate::init_db(&data).unwrap());
        assert_eq!(
            list_backups(&dir).len(),
            1,
            "migrations applied, nothing pending"
        );
    }

    #[test]
    fn configured_backup_path_reads_settings_without_migrating() {
        let data = temp_dir("setting");
        let db = sample_db(&data);
        assert_eq!(configured_backup_path(&db), None);
        Connection::open(&db)
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value) VALUES ('backupPath', 'E:\\Backups')",
                [],
            )
            .unwrap();
        assert_eq!(configured_backup_path(&db).as_deref(), Some("E:\\Backups"));
        assert_eq!(resolve_backup_dir(&data, Some("  ")), data.join("backups"));
        assert_eq!(
            resolve_backup_dir(&data, Some("E:\\Backups")),
            PathBuf::from("E:\\Backups")
        );
    }
}

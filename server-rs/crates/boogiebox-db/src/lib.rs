//! SQLite initialization, migration, and shared database helpers for BoogieBox.

pub mod artwork;
pub mod jobs;
pub mod music;
pub mod playback;
pub mod playlists;

use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use uuid::Uuid;

const DB_FILE_NAME: &str = "boogiebox.db";
const DEFAULT_BUSY_TIMEOUT_MS: u32 = 15_000;
const LOCAL_MMAP_SIZE: i64 = 268_435_456;
const CACHE_SIZE_KIB: i64 = -32_000;
const WAL_AUTO_CHECKPOINT_PAGES: u32 = 4_000;

/// Public Journal Mode data shape used by BoogieBox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalMode {
    Wal,
    Delete,
}

impl JournalMode {
    fn as_sql(self) -> &'static str {
        match self {
            Self::Wal => "WAL",
            Self::Delete => "DELETE",
        }
    }
}

/// Public Init DB Error data shape used by BoogieBox.
#[derive(Debug, Error)]
pub enum InitDbError {
    #[error("dbFolder is required")]
    EmptyDbFolder,
    #[error("failed to create database directory {path}: {source}")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("failed to open database {path}: {source}")]
    Open {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("failed to configure database {path}: {source}")]
    Configure {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("failed to initialize schema in {path}: {source}")]
    Schema {
        path: PathBuf,
        source: rusqlite::Error,
    },
}

/// Public Initialized Database data shape used by BoogieBox.
#[derive(Debug)]
pub struct InitializedDatabase {
    /// Open rusqlite connection after schema initialization has completed.
    pub connection: Connection,
    /// Resolved path to the `boogiebox.db` file used for this connection.
    pub db_path: PathBuf,
    /// Journal mode selected for the database path, with DELETE used for network paths.
    pub journal_mode: JournalMode,
}

/// Resolves a configured database folder or direct DB path to `boogiebox.db`.
pub fn get_db_path(db_folder: &Path) -> Result<PathBuf, InitDbError> {
    let trimmed = db_folder.to_string_lossy().trim().to_string();
    if trimmed.is_empty() {
        return Err(InitDbError::EmptyDbFolder);
    }
    let path = PathBuf::from(trimmed);
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(DB_FILE_NAME))
    {
        return Ok(path);
    }
    Ok(path.join(DB_FILE_NAME))
}

/// Returns whether the configured database location currently contains a DB file.
pub fn database_exists(db_folder: &Path) -> bool {
    get_db_path(db_folder).is_ok_and(|path| path.is_file())
}

/// Detects UNC or slash-prefixed network database paths that should avoid WAL mode.
pub fn is_network_db_path(db_path: &Path) -> bool {
    let path = db_path.to_string_lossy();
    path.starts_with(r"\\") || path.starts_with("//")
}

/// Selects the safest SQLite journal mode for local or network-backed databases.
pub fn get_preferred_journal_mode(db_path: &Path) -> JournalMode {
    if is_network_db_path(db_path) {
        JournalMode::Delete
    } else {
        JournalMode::Wal
    }
}

/// Opens, configures, migrates, and returns a ready-to-use BoogieBox database.
pub fn init_db(db_folder: &Path) -> Result<InitializedDatabase, InitDbError> {
    let db_path = get_db_path(db_folder)?;
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).map_err(|source| InitDbError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let connection = Connection::open(&db_path).map_err(|source| InitDbError::Open {
        path: db_path.clone(),
        source,
    })?;
    let journal_mode = configure_database_connection(&connection, &db_path).map_err(|source| {
        InitDbError::Configure {
            path: db_path.clone(),
            source,
        }
    })?;
    initialize_schema(&connection).map_err(|source| InitDbError::Schema {
        path: db_path.clone(),
        source,
    })?;

    Ok(InitializedDatabase {
        connection,
        db_path,
        journal_mode,
    })
}

/// Documents the Configure Database Connection public API surface.
pub fn configure_database_connection(
    connection: &Connection,
    db_path: &Path,
) -> Result<JournalMode, rusqlite::Error> {
    let journal_mode = get_preferred_journal_mode(db_path);
    connection.pragma_update(None, "busy_timeout", DEFAULT_BUSY_TIMEOUT_MS)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "journal_mode", journal_mode.as_sql())?;
    connection.pragma_update(None, "cache_size", CACHE_SIZE_KIB)?;
    if !is_network_db_path(db_path) {
        connection.pragma_update(None, "mmap_size", LOCAL_MMAP_SIZE)?;
    }
    if journal_mode == JournalMode::Wal {
        connection.pragma_update(None, "wal_autocheckpoint", WAL_AUTO_CHECKPOINT_PAGES)?;
    }
    Ok(journal_mode)
}

/// Documents the Initialize Schema public API surface.
pub fn initialize_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
          id TEXT PRIMARY KEY,
          applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS settings (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS user_settings (
          user_id TEXT NOT NULL,
          key TEXT NOT NULL,
          value TEXT NOT NULL,
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY (user_id, key)
        );

        CREATE TABLE IF NOT EXISTS users (
          id TEXT PRIMARY KEY,
          username TEXT NOT NULL UNIQUE COLLATE NOCASE,
          pin_hash TEXT,
          role TEXT NOT NULL DEFAULT 'user' CHECK(role IN ('admin','user')),
          can_scan INTEGER NOT NULL DEFAULT 0,
          can_edit_metadata INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);

        CREATE TABLE IF NOT EXISTS sessions (
          token TEXT PRIMARY KEY,
          user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          created_at TEXT NOT NULL DEFAULT (datetime('now')),
          expires_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS libraries (
          id TEXT PRIMARY KEY,
          path TEXT NOT NULL UNIQUE,
          name TEXT NOT NULL,
          library_type TEXT NOT NULL DEFAULT 'music' CHECK(library_type = 'music'),
          scanner_profile TEXT NOT NULL DEFAULT 'default',
          metadata_mode TEXT NOT NULL DEFAULT 'path_only',
          added_at TEXT NOT NULL DEFAULT (datetime('now')),
          last_scan TEXT
        );

        CREATE TABLE IF NOT EXISTS library_folders (
          id TEXT PRIMARY KEY,
          library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
          path TEXT NOT NULL,
          position INTEGER NOT NULL DEFAULT 0,
          added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_library_folders_library_id
          ON library_folders(library_id, position, id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_library_folders_path_unique
          ON library_folders(LOWER(TRIM(path)));

        CREATE TABLE IF NOT EXISTS artists (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT,
          metadata_locked INTEGER NOT NULL DEFAULT 0,
          track_count INTEGER NOT NULL DEFAULT 0,
          album_count INTEGER NOT NULL DEFAULT 0,
          play_count INTEGER NOT NULL DEFAULT 0,
          lastfm_mbid TEXT,
          lastfm_canonical_name TEXT,
          lastfm_identity_checked_at TEXT,
          deezer_artist_id TEXT,
          deezer_identity_checked_at TEXT,
          spotify_artist_id TEXT,
          spotify_identity_checked_at TEXT,
          discogs_artist_id TEXT,
          discogs_identity_checked_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_artists_lastfm_mbid
          ON artists(lastfm_mbid) WHERE lastfm_mbid IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_deezer_artist_id
          ON artists(deezer_artist_id) WHERE deezer_artist_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_spotify_artist_id
          ON artists(spotify_artist_id) WHERE spotify_artist_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_discogs_artist_id
          ON artists(discogs_artist_id) WHERE discogs_artist_id IS NOT NULL;

        CREATE TABLE IF NOT EXISTS albums (
          id TEXT PRIMARY KEY,
          title TEXT NOT NULL,
          year INTEGER,
          genre TEXT,
          description TEXT,
          release_type TEXT NOT NULL DEFAULT 'album',
          metadata_locked INTEGER NOT NULL DEFAULT 0,
          album_artist TEXT NOT NULL DEFAULT '',
          artist_id TEXT REFERENCES artists(id) ON DELETE SET NULL,
          track_count INTEGER NOT NULL DEFAULT 0,
          total_duration_sec REAL NOT NULL DEFAULT 0,
          added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_albums_added_at ON albums(added_at DESC);
        CREATE INDEX IF NOT EXISTS idx_albums_artist_id ON albums(artist_id);

        CREATE TABLE IF NOT EXISTS tracks (
          id TEXT PRIMARY KEY,
          library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
          artist_id TEXT REFERENCES artists(id) ON DELETE SET NULL,
          album_id TEXT REFERENCES albums(id) ON DELETE SET NULL,
          title TEXT NOT NULL,
          file_name TEXT NOT NULL DEFAULT '',
          album_artist TEXT NOT NULL DEFAULT '',
          genre TEXT NOT NULL DEFAULT '',
          composer TEXT NOT NULL DEFAULT '',
          duration REAL NOT NULL DEFAULT 0,
          format TEXT,
          bitrate INTEGER,
          sample_rate INTEGER,
          channels INTEGER,
          file_path TEXT NOT NULL UNIQUE,
          file_size INTEGER NOT NULL DEFAULT 0,
          track_number INTEGER,
          disc_number INTEGER,
          year INTEGER,
          comment TEXT,
          bpm INTEGER,
          bpm_detected REAL,
          bpm_source TEXT,
          bpm_confidence REAL,
          bpm_analyzed_at TEXT,
          scanned_at TEXT NOT NULL DEFAULT (datetime('now')),
          last_played_at TEXT,
          play_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_tracks_library ON tracks(library_id);
        CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist_id);
        CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id);
        CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
        CREATE INDEX IF NOT EXISTS idx_tracks_genre ON tracks(genre);
        CREATE INDEX IF NOT EXISTS idx_tracks_year ON tracks(year);
        CREATE INDEX IF NOT EXISTS idx_tracks_last_played ON tracks(last_played_at);
        CREATE INDEX IF NOT EXISTS idx_tracks_play_count ON tracks(play_count);
        CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
          track_id UNINDEXED,
          title, artist, album, genre, composer, file_path,
          content='',
          tokenize='unicode61'
        );

        CREATE TABLE IF NOT EXISTS playlists (
          id TEXT PRIMARY KEY,
          user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
          name TEXT NOT NULL,
          description TEXT,
          created_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          remember_progress INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_playlists_name_lower
          ON playlists(user_id, LOWER(TRIM(name)));

        CREATE TABLE IF NOT EXISTS playlist_tracks (
          id TEXT PRIMARY KEY,
          playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
          track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
          position INTEGER NOT NULL DEFAULT 0,
          added_at TEXT NOT NULL DEFAULT (datetime('now')),
          progress_seconds REAL NOT NULL DEFAULT 0,
          UNIQUE(playlist_id, track_id)
        );
        CREATE INDEX IF NOT EXISTS idx_pl_tracks_playlist ON playlist_tracks(playlist_id, position);

        CREATE TABLE IF NOT EXISTS crossfade_overrides (
          id TEXT PRIMARY KEY,
          entity_type TEXT NOT NULL CHECK(entity_type IN ('album','playlist','autodj')),
          entity_id TEXT NOT NULL,
          mode TEXT NOT NULL CHECK(mode IN ('off','zerogap','crossfade')),
          duration INTEGER NOT NULL DEFAULT 2
        );

        CREATE TABLE IF NOT EXISTS track_waveforms (
          track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          sample_count INTEGER NOT NULL DEFAULT 0,
          duration_seconds REAL,
          waveform_json TEXT NOT NULL,
          version INTEGER NOT NULL DEFAULT 1,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_track_waveforms_updated_at ON track_waveforms(updated_at);

        CREATE TABLE IF NOT EXISTS lyrics_cache (
          track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          artist TEXT NOT NULL,
          title TEXT NOT NULL,
          lyrics TEXT NOT NULL,
          synced_lyrics TEXT,
          source TEXT NOT NULL DEFAULT 'lrclib',
          fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_lyrics_cache_artist_title
          ON lyrics_cache(LOWER(TRIM(artist)), LOWER(TRIM(title)));

        CREATE TABLE IF NOT EXISTS play_history (
          id TEXT PRIMARY KEY,
          user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
          played_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_play_history_user ON play_history(user_id, played_at DESC);

        CREATE TABLE IF NOT EXISTS scan_jobs (
          id TEXT PRIMARY KEY,
          library_id TEXT REFERENCES libraries(id) ON DELETE CASCADE,
          status TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          started_at TEXT,
          finished_at TEXT,
          files_found INTEGER DEFAULT 0,
          files_scanned INTEGER DEFAULT 0,
          errors INTEGER DEFAULT 0,
          error_log TEXT
        );

        CREATE TABLE IF NOT EXISTS scan_schedules (
          id TEXT PRIMARY KEY,
          library_id TEXT NOT NULL UNIQUE REFERENCES libraries(id) ON DELETE CASCADE,
          enabled INTEGER NOT NULL DEFAULT 1,
          frequency_hours REAL NOT NULL DEFAULT 24,
          last_run TEXT,
          next_run TEXT,
          created_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS post_scan_jobs (
          id TEXT PRIMARY KEY,
          library_id TEXT REFERENCES libraries(id) ON DELETE CASCADE,
          job_type TEXT NOT NULL,
          payload TEXT,
          status TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          started_at TEXT,
          heartbeat_at TEXT,
          finished_at TEXT,
          error_log TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_post_scan_jobs_pending
          ON post_scan_jobs(id ASC) WHERE status = 'pending';

        CREATE TABLE IF NOT EXISTS provider_usage_stats (
          provider TEXT NOT NULL,
          entity_type TEXT NOT NULL,
          usage_type TEXT NOT NULL,
          count INTEGER NOT NULL DEFAULT 1,
          last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY(provider, entity_type, usage_type)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_usage_last_used
          ON provider_usage_stats(last_used_at DESC);

        CREATE TABLE IF NOT EXISTS stats_cache (
          id TEXT PRIMARY KEY,
          total_tracks INTEGER NOT NULL DEFAULT 0,
          total_artists INTEGER NOT NULL DEFAULT 0,
          total_albums INTEGER NOT NULL DEFAULT 0,
          total_libraries INTEGER NOT NULL DEFAULT 0,
          total_hours REAL NOT NULL DEFAULT 0,
          total_gb REAL NOT NULL DEFAULT 0,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS track_mix_analysis (
          track_id          TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          duration_sec      REAL NOT NULL,
          bpm_estimate      REAL,
          loudness_lufs     REAL,
          key_estimate      TEXT,
          beat_grid_sec     REAL,
          phrase_bars       INTEGER,
          intro_start_sec   REAL NOT NULL DEFAULT 0,
          intro_end_sec     REAL NOT NULL DEFAULT 12,
          outro_start_sec   REAL NOT NULL DEFAULT 0,
          outro_end_sec     REAL NOT NULL DEFAULT 0,
          low_energy_start_sec  REAL,
          low_energy_end_sec    REAL,
          high_energy_start_sec REAL,
          high_energy_end_sec   REAL,
          confidence        REAL NOT NULL DEFAULT 0.5,
          analysis_version  INTEGER NOT NULL DEFAULT 1,
          analyzed_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_track_mix_analysis_analyzed_at
          ON track_mix_analysis(analyzed_at);

        CREATE TABLE IF NOT EXISTS mix_jobs (
          id                   TEXT PRIMARY KEY,
          playlist_id          TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
          user_id              TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          status               TEXT NOT NULL DEFAULT 'pending',
          progress_percent     INTEGER NOT NULL DEFAULT 0,
          current_step         TEXT NOT NULL DEFAULT 'queued',
          last_message         TEXT,
          default_crossfade_sec INTEGER NOT NULL DEFAULT 8,
          mix_style            TEXT NOT NULL DEFAULT 'club_blend',
          mix_quality          TEXT NOT NULL DEFAULT 'standard',
          mix_strategy         TEXT,
          planner_provider     TEXT,
          used_deep_analysis   INTEGER NOT NULL DEFAULT 0,
          deep_analysis_status TEXT,
          cancel_requested     INTEGER NOT NULL DEFAULT 0,
          output_id            TEXT,
          started_at           TEXT,
          finished_at          TEXT,
          created_at           TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at           TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_mix_jobs_playlist
          ON mix_jobs(playlist_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_mix_jobs_user
          ON mix_jobs(user_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_mix_jobs_status
          ON mix_jobs(status, created_at);

        CREATE TABLE IF NOT EXISTS mix_outputs (
          id               TEXT PRIMARY KEY,
          job_id           TEXT NOT NULL UNIQUE REFERENCES mix_jobs(id) ON DELETE CASCADE,
          playlist_id      TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
          user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          file_path        TEXT NOT NULL,
          file_name        TEXT NOT NULL,
          duration_sec     REAL NOT NULL DEFAULT 0,
          file_size_bytes  INTEGER NOT NULL DEFAULT 0,
          format           TEXT NOT NULL DEFAULT 'mp3',
          created_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_mix_outputs_playlist
          ON mix_outputs(playlist_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS mix_transitions (
          id                   TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
          job_id               TEXT NOT NULL REFERENCES mix_jobs(id) ON DELETE CASCADE,
          step_index           INTEGER NOT NULL,
          from_track_id        TEXT NOT NULL,
          to_track_id          TEXT NOT NULL,
          crossfade_sec        REAL NOT NULL DEFAULT 8,
          from_outro_start_sec REAL NOT NULL DEFAULT 0,
          to_intro_start_sec   REAL NOT NULL DEFAULT 0,
          phrase_aware         INTEGER NOT NULL DEFAULT 0,
          reason               TEXT NOT NULL DEFAULT '',
          created_at           TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_mix_transitions_job
          ON mix_transitions(job_id, step_index);

        CREATE TABLE IF NOT EXISTS mix_job_logs (
          id         TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
          job_id     TEXT NOT NULL REFERENCES mix_jobs(id) ON DELETE CASCADE,
          level      TEXT NOT NULL DEFAULT 'info',
          message    TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_mix_job_logs_job ON mix_job_logs(job_id, id);

        CREATE TABLE IF NOT EXISTS boogiemix_plans (
          id                TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
          playlist_id       TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
          provider          TEXT NOT NULL DEFAULT 'deterministic',
          raw_ai_response   TEXT,
          normalized_plan   TEXT NOT NULL,
          validation_result TEXT NOT NULL DEFAULT 'fallback',
          created_at        TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_boogiemix_plans_playlist
          ON boogiemix_plans(playlist_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS track_deep_analysis (
          track_id                 TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          analysis_version         INTEGER NOT NULL DEFAULT 1,
          analysis_schema_version  INTEGER NOT NULL DEFAULT 1,
          demucs_model             TEXT NOT NULL DEFAULT 'htdemucs',
          used_gpu                 INTEGER NOT NULL DEFAULT 0,
          file_fingerprint         TEXT NOT NULL DEFAULT '',
          stem_feature_json        TEXT NOT NULL DEFAULT '{}',
          vocal_windows_json       TEXT NOT NULL DEFAULT '[]',
          drum_windows_json        TEXT NOT NULL DEFAULT '[]',
          bass_windows_json        TEXT NOT NULL DEFAULT '[]',
          section_json             TEXT NOT NULL DEFAULT '[]',
          phrase_boundaries_json   TEXT NOT NULL DEFAULT '[]',
          intro_outro_refined_json TEXT NOT NULL DEFAULT '{}',
          energy_score_refined     REAL NOT NULL DEFAULT 0.5,
          transition_hints_json    TEXT NOT NULL DEFAULT '{}',
          transition_windows_json  TEXT NOT NULL DEFAULT '[]',
          confidence               REAL NOT NULL DEFAULT 0.0,
          feature_size_bytes       INTEGER NOT NULL DEFAULT 0,
          source_duration_sec      REAL,
          processing_time_ms       INTEGER NOT NULL DEFAULT 0,
          last_used_at             TEXT,
          created_at               TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_track_deep_analysis_track_id
          ON track_deep_analysis(track_id);
        CREATE INDEX IF NOT EXISTS idx_track_deep_analysis_created_at
          ON track_deep_analysis(created_at);
        -- idx_track_deep_analysis_last_used_at is created by the schema repair
        -- migration so upgrade DBs whose track_deep_analysis predates the
        -- last_used_at column do not fail this bootstrap step.

        CREATE TABLE IF NOT EXISTS deep_analysis_jobs (
          id               TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
          track_id         TEXT NOT NULL UNIQUE REFERENCES tracks(id) ON DELETE CASCADE,
          status           TEXT NOT NULL DEFAULT 'pending',
          priority         INTEGER NOT NULL DEFAULT 50,
          file_fingerprint TEXT NOT NULL DEFAULT '',
          error_message    TEXT,
          started_at       TEXT,
          finished_at      TEXT,
          created_at       TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_deep_analysis_jobs_status
          ON deep_analysis_jobs(status, priority DESC, id ASC);
        CREATE INDEX IF NOT EXISTS idx_deep_analysis_jobs_priority_status
          ON deep_analysis_jobs(status, priority DESC, updated_at ASC);
        "#,
    )?;

    seed_default_settings(connection)?;
    seed_admin_user(connection)?;
    run_tracked_migrations(connection)?;
    refresh_denormalized_counts(connection)?;
    refresh_stats_cache(connection)?;
    Ok(())
}

#[derive(Clone, Copy)]
struct Migration {
    id: &'static str,
    apply: fn(&Connection) -> Result<(), rusqlite::Error>,
}

fn run_tracked_migrations(connection: &Connection) -> Result<(), rusqlite::Error> {
    let migrations = [
        Migration {
            id: "2026-05-23-users-schema-fix",
            apply: ensure_correct_users_schema,
        },
        Migration {
            id: "2026-05-23-users-permissions-columns",
            apply: ensure_user_permissions_columns,
        },
        Migration {
            id: "2026-05-23-library-name-uniqueness",
            apply: ensure_library_name_uniqueness,
        },
        Migration {
            id: "2026-05-23-playlist-name-uniqueness",
            apply: ensure_playlist_name_uniqueness,
        },
        Migration {
            id: "2026-05-23-playlist-mutation-schema",
            apply: ensure_playlist_mutation_schema,
        },
        Migration {
            id: "2026-05-23-music-metadata-edit-schema",
            apply: ensure_music_metadata_edit_schema,
        },
        Migration {
            id: "2026-05-23-add-ratings-and-album-added-at",
            apply: ensure_ratings_and_album_added_at,
        },
        Migration {
            id: "2026-05-23-typed-library-folders",
            apply: ensure_typed_library_and_folder_schema,
        },
        Migration {
            id: "2026-05-23-playback-and-music-ratings",
            apply: ensure_playback_and_music_rating_schema,
        },
        Migration {
            id: "2026-05-23-music-feature-tables",
            apply: ensure_music_feature_tables_schema,
        },
        Migration {
            id: "2026-05-24-provider-usage-and-art-tables",
            apply: ensure_provider_usage_and_art_tables,
        },
        Migration {
            id: "2026-05-24-job-schedule-schema",
            apply: ensure_job_schedule_schema,
        },
        Migration {
            id: "2026-05-24-track-scan-schema",
            apply: ensure_track_scan_schema,
        },
        Migration {
            id: "2026-05-24-boogiemix-schema",
            apply: ensure_boogiemix_schema,
        },
        Migration {
            id: "2026-05-25-remove-video-support",
            apply: remove_video_support_schema,
        },
        Migration {
            id: "2026-05-25-library-last-scan",
            apply: ensure_library_last_scan_column,
        },
        Migration {
            id: "2026-05-26-deep-analysis-feature-schema",
            apply: ensure_deep_analysis_feature_schema,
        },
        Migration {
            id: "2026-07-29-album-label-column",
            apply: ensure_album_label_column,
        },
        Migration {
            id: "2026-08-02-fix-compilation-album-ownership",
            apply: fix_compilation_album_ownership,
        },
        Migration {
            id: "2026-08-05-rename-can-scan-to-can-manage-libraries",
            apply: ensure_can_manage_libraries_column,
        },
        Migration {
            id: "2026-08-07-browse-performance-indexes",
            apply: ensure_browse_performance_indexes,
        },
        Migration {
            id: "2026-08-17-artist-external-identities",
            apply: ensure_artist_external_identity_schema,
        },
    ];

    for migration in migrations {
        apply_tracked_migration(connection, migration)?;
    }

    Ok(())
}

fn apply_tracked_migration(
    connection: &Connection,
    migration: Migration,
) -> Result<(), rusqlite::Error> {
    let already_applied = connection
        .query_row(
            "SELECT id FROM schema_migrations WHERE id = ?1",
            [migration.id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .is_some();
    if already_applied {
        return Ok(());
    }

    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (migration.apply)(connection).and_then(|_| {
        connection.execute(
            "INSERT INTO schema_migrations(id, applied_at) VALUES(?1, datetime('now'))",
            [migration.id],
        )?;
        connection.execute_batch("COMMIT")
    });

    if result.is_err() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    result
}

fn ensure_correct_users_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "users") {
        return Ok(());
    }
    // Only migrate if the old schema exists (uses 'name' instead of 'username').
    if !column_exists(connection, "users", "name")? {
        return Ok(());
    }
    // Build SELECT dynamically — older schemas may not have is_admin / can_scan / created_at.
    let role_expr = if column_exists(connection, "users", "is_admin")? {
        "CASE WHEN COALESCE(is_admin, 0) = 1 THEN 'admin' ELSE 'user' END"
    } else {
        "'user'"
    };
    let can_scan_expr = if column_exists(connection, "users", "can_scan")? {
        "COALESCE(can_scan, 0)"
    } else {
        "0"
    };
    let can_edit_expr = if column_exists(connection, "users", "can_edit_metadata")? {
        "COALESCE(can_edit_metadata, 0)"
    } else {
        "0"
    };
    let created_at_expr = if column_exists(connection, "users", "created_at")? {
        "created_at"
    } else {
        "datetime('now')"
    };
    let sql = format!(
        r#"
        PRAGMA foreign_keys = OFF;
        CREATE TABLE users_migrated (
          id TEXT PRIMARY KEY,
          username TEXT NOT NULL UNIQUE COLLATE NOCASE,
          pin_hash TEXT,
          role TEXT NOT NULL DEFAULT 'user' CHECK(role IN ('admin','user')),
          can_scan INTEGER NOT NULL DEFAULT 0,
          can_edit_metadata INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO users_migrated(id, username, pin_hash, role, can_scan, can_edit_metadata, created_at)
        SELECT
          id,
          name,
          CASE WHEN pin_hash = '' THEN NULL ELSE pin_hash END,
          {role_expr},
          {can_scan_expr},
          {can_edit_expr},
          {created_at_expr}
        FROM users;
        DROP TABLE users;
        ALTER TABLE users_migrated RENAME TO users;
        CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
        PRAGMA foreign_keys = ON;
        "#
    );
    connection.execute_batch(&sql)
}

fn ensure_user_permissions_columns(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "users") {
        return Ok(());
    }
    if !column_exists(connection, "users", "can_scan")? {
        connection
            .execute_batch("ALTER TABLE users ADD COLUMN can_scan INTEGER NOT NULL DEFAULT 0")?;
    }
    if !column_exists(connection, "users", "can_edit_metadata")? {
        connection.execute_batch(
            "ALTER TABLE users ADD COLUMN can_edit_metadata INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    Ok(())
}

/// Renames the `can_scan` permission column to `can_manage_libraries`: the permission now
/// gates full library management (create/rename/delete libraries and folders, schedules),
/// not just enqueuing scans.
fn ensure_can_manage_libraries_column(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "users") {
        return Ok(());
    }
    if column_exists(connection, "users", "can_manage_libraries")? {
        return Ok(());
    }
    if column_exists(connection, "users", "can_scan")? {
        connection
            .execute_batch("ALTER TABLE users RENAME COLUMN can_scan TO can_manage_libraries")?;
    } else {
        connection.execute_batch(
            "ALTER TABLE users ADD COLUMN can_manage_libraries INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    Ok(())
}

fn ensure_library_name_uniqueness(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "libraries") {
        return Ok(());
    }

    let has_library_folders = table_exists(connection, "library_folders");
    let libraries = if has_library_folders {
        let mut statement = connection.prepare(
            r#"
            SELECT
              l.id,
              l.name,
              l.path,
              (
                SELECT lf.path
                FROM library_folders lf
                WHERE lf.library_id = l.id
                ORDER BY lf.position ASC, lf.id ASC
                LIMIT 1
              ) AS primary_path
            FROM libraries l
            ORDER BY datetime(added_at) ASC, id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(LibraryNameRow {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                primary_path: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    } else {
        let mut statement = connection.prepare(
            r#"
            SELECT id, name, path, path AS primary_path
            FROM libraries
            ORDER BY datetime(added_at) ASC, id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(LibraryNameRow {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                primary_path: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut used_names = std::collections::HashSet::new();
    let mut canonical_names = std::collections::HashMap::new();
    let mut update = connection.prepare("UPDATE libraries SET name = ?1 WHERE id = ?2")?;

    for library in libraries {
        let fallback_source = library
            .primary_path
            .as_deref()
            .unwrap_or(&library.path)
            .trim_end_matches(['\\', '/']);
        let fallback_name = Path::new(fallback_source)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Library");
        let normalized_current = normalize_library_name(&library.name)
            .unwrap_or_else(|| normalize_library_name(fallback_name).unwrap_or("Library".into()));
        let normalized_key = normalized_current.to_lowercase();
        let canonical_base = canonical_names
            .entry(normalized_key)
            .or_insert_with(|| normalized_current.clone())
            .clone();
        let unique_name = build_unique_library_name(&canonical_base, &mut used_names);
        if unique_name != library.name {
            update.execute(params![unique_name, library.id])?;
        }
    }

    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_libraries_name_unique ON libraries(LOWER(TRIM(name)))",
    )?;
    Ok(())
}

fn ensure_playlist_name_uniqueness(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "playlists") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS trg_playlists_name_unique_insert;
        DROP TRIGGER IF EXISTS trg_playlists_name_unique_update;

        CREATE TRIGGER IF NOT EXISTS trg_playlists_name_unique_insert
        BEFORE INSERT ON playlists
        FOR EACH ROW
        BEGIN
          SELECT CASE
            WHEN EXISTS (
              SELECT 1 FROM playlists p
              WHERE LOWER(TRIM(p.name)) = LOWER(TRIM(NEW.name))
                AND p.user_id IS NEW.user_id
            )
            THEN RAISE(ABORT, 'duplicate playlist name')
          END;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_playlists_name_unique_update
        BEFORE UPDATE OF name ON playlists
        FOR EACH ROW
        BEGIN
          SELECT CASE
            WHEN EXISTS (
              SELECT 1 FROM playlists p
              WHERE p.id != NEW.id
                AND LOWER(TRIM(p.name)) = LOWER(TRIM(NEW.name))
                AND p.user_id IS NEW.user_id
            )
            THEN RAISE(ABORT, 'duplicate playlist name')
          END;
        END;
        "#,
    )?;
    Ok(())
}

fn ensure_playlist_mutation_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "playlists") {
        if !column_exists(connection, "playlists", "description")? {
            connection.execute_batch("ALTER TABLE playlists ADD COLUMN description TEXT")?;
        }
        if !column_exists(connection, "playlists", "remember_progress")? {
            connection.execute_batch(
                "ALTER TABLE playlists ADD COLUMN remember_progress INTEGER NOT NULL DEFAULT 0",
            )?;
        }
    }

    if table_exists(connection, "playlist_tracks") {
        if !column_exists(connection, "playlist_tracks", "progress_seconds")? {
            connection.execute_batch(
                "ALTER TABLE playlist_tracks ADD COLUMN progress_seconds REAL NOT NULL DEFAULT 0",
            )?;
        }
        connection.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_pl_tracks_playlist
              ON playlist_tracks(playlist_id, position);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_playlist_tracks_unique_track
              ON playlist_tracks(playlist_id, track_id);
            "#,
        )?;
    }

    Ok(())
}

fn ensure_music_metadata_edit_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "artists") && !column_exists(connection, "artists", "description")?
    {
        connection.execute_batch("ALTER TABLE artists ADD COLUMN description TEXT")?;
    }
    if table_exists(connection, "artists")
        && !column_exists(connection, "artists", "metadata_locked")?
    {
        connection.execute_batch(
            "ALTER TABLE artists ADD COLUMN metadata_locked INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    if table_exists(connection, "albums") && !column_exists(connection, "albums", "year")? {
        connection.execute_batch("ALTER TABLE albums ADD COLUMN year INTEGER")?;
    }
    if table_exists(connection, "albums") && !column_exists(connection, "albums", "genre")? {
        connection.execute_batch("ALTER TABLE albums ADD COLUMN genre TEXT")?;
    }
    if table_exists(connection, "albums") && !column_exists(connection, "albums", "description")? {
        connection.execute_batch("ALTER TABLE albums ADD COLUMN description TEXT")?;
    }
    if table_exists(connection, "albums") && !column_exists(connection, "albums", "release_type")? {
        connection.execute_batch(
            "ALTER TABLE albums ADD COLUMN release_type TEXT NOT NULL DEFAULT 'album'",
        )?;
    }
    if table_exists(connection, "albums")
        && !column_exists(connection, "albums", "metadata_locked")?
    {
        connection.execute_batch(
            "ALTER TABLE albums ADD COLUMN metadata_locked INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    if table_exists(connection, "albums") && column_exists(connection, "albums", "release_type")? {
        connection.execute_batch(
            "
            UPDATE albums
            SET release_type = 'album'
            WHERE release_type IS NULL OR TRIM(release_type) = '';
            ",
        )?;
    }
    Ok(())
}

fn ensure_artist_external_identity_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "artists") {
        return Ok(());
    }
    for (column, definition) in [
        ("lastfm_mbid", "TEXT"),
        ("lastfm_canonical_name", "TEXT"),
        ("lastfm_identity_checked_at", "TEXT"),
        ("deezer_artist_id", "TEXT"),
        ("deezer_identity_checked_at", "TEXT"),
        ("spotify_artist_id", "TEXT"),
        ("spotify_identity_checked_at", "TEXT"),
        ("discogs_artist_id", "TEXT"),
        ("discogs_identity_checked_at", "TEXT"),
    ] {
        if !column_exists(connection, "artists", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE artists ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_artists_lastfm_mbid
          ON artists(lastfm_mbid) WHERE lastfm_mbid IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_deezer_artist_id
          ON artists(deezer_artist_id) WHERE deezer_artist_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_spotify_artist_id
          ON artists(spotify_artist_id) WHERE spotify_artist_id IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_artists_discogs_artist_id
          ON artists(discogs_artist_id) WHERE discogs_artist_id IS NOT NULL;
        "#,
    )?;
    Ok(())
}

fn ensure_album_label_column(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "albums") && !column_exists(connection, "albums", "label")? {
        connection.execute_batch("ALTER TABLE albums ADD COLUMN label TEXT")?;
    }
    Ok(())
}

/// Older scans stamped `albums.artist_id` from whichever track happened to be
/// scanned first, so a compilation could end up "owned" by one of its many
/// contributing artists instead of the artist named in its `album_artist` tag.
/// That falsely made the compilation look like a real release by that artist
/// (and made it appear twice: once as an "owned" album, once under "appears
/// on"). Realign every album's `artist_id` to the artist matching its
/// `album_artist` field, creating that artist if it doesn't exist yet.
fn fix_compilation_album_ownership(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "albums") || !table_exists(connection, "artists") {
        return Ok(());
    }
    if !column_exists(connection, "albums", "artist_id")?
        || !column_exists(connection, "albums", "album_artist")?
    {
        return Ok(());
    }
    let mismatched: Vec<(String, String)> = {
        let mut stmt = connection.prepare(
            "SELECT al.id, al.album_artist
             FROM albums al
             LEFT JOIN artists ar ON ar.id = al.artist_id
             WHERE TRIM(COALESCE(al.album_artist, '')) != ''
               AND (ar.id IS NULL OR LOWER(TRIM(ar.name)) != LOWER(TRIM(al.album_artist)))",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, rusqlite::Error>>()?;
        rows
    };

    for (album_id, album_artist) in mismatched {
        let trimmed = album_artist.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        let owning_name =
            crate::jobs::canonical_compilation_artist_name(&trimmed).unwrap_or(trimmed);
        let artist_id: String = match connection
            .query_row(
                "SELECT id FROM artists WHERE LOWER(TRIM(name)) = LOWER(TRIM(?1)) LIMIT 1",
                [&owning_name],
                |row| row.get(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => {
                let id = Uuid::now_v7().to_string();
                connection.execute(
                    "INSERT INTO artists(id, name) VALUES(?1, ?2)",
                    params![id, owning_name],
                )?;
                id
            }
        };
        connection.execute(
            "UPDATE albums SET artist_id = ?1 WHERE id = ?2",
            params![artist_id, album_id],
        )?;
    }

    refresh_denormalized_counts(connection)?;
    Ok(())
}

fn ensure_ratings_and_album_added_at(connection: &Connection) -> Result<(), rusqlite::Error> {
    ensure_albums_added_at_schema(connection)?;
    ensure_artist_ratings_schema(connection)?;
    Ok(())
}

fn ensure_typed_library_and_folder_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    ensure_typed_library_schema(connection)?;
    ensure_library_folders_schema(connection)?;
    Ok(())
}

fn ensure_playback_and_music_rating_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    ensure_tracks_last_played_schema(connection)?;
    ensure_playback_count_schema(connection)?;
    ensure_music_ratings_schema(connection)?;
    Ok(())
}

fn ensure_music_feature_tables_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    ensure_crossfade_overrides_schema(connection)?;
    ensure_track_waveforms_schema(connection)?;
    ensure_lyrics_cache_schema(connection)?;
    ensure_play_history_schema(connection)?;
    Ok(())
}

fn ensure_deep_analysis_feature_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "track_deep_analysis") {
        return Ok(());
    }

    let columns = [
        (
            "analysis_schema_version",
            "ALTER TABLE track_deep_analysis ADD COLUMN analysis_schema_version INTEGER NOT NULL DEFAULT 1",
        ),
        (
            "confidence",
            "ALTER TABLE track_deep_analysis ADD COLUMN confidence REAL NOT NULL DEFAULT 0.0",
        ),
        (
            "feature_size_bytes",
            "ALTER TABLE track_deep_analysis ADD COLUMN feature_size_bytes INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "source_duration_sec",
            "ALTER TABLE track_deep_analysis ADD COLUMN source_duration_sec REAL",
        ),
        (
            "last_used_at",
            "ALTER TABLE track_deep_analysis ADD COLUMN last_used_at TEXT",
        ),
        (
            "section_json",
            "ALTER TABLE track_deep_analysis ADD COLUMN section_json TEXT NOT NULL DEFAULT '[]'",
        ),
        (
            "bass_windows_json",
            "ALTER TABLE track_deep_analysis ADD COLUMN bass_windows_json TEXT NOT NULL DEFAULT '[]'",
        ),
        (
            "phrase_boundaries_json",
            "ALTER TABLE track_deep_analysis ADD COLUMN phrase_boundaries_json TEXT NOT NULL DEFAULT '[]'",
        ),
        (
            "transition_windows_json",
            "ALTER TABLE track_deep_analysis ADD COLUMN transition_windows_json TEXT NOT NULL DEFAULT '[]'",
        ),
    ];

    for (column, sql) in columns {
        if !column_exists(connection, "track_deep_analysis", column)? {
            connection.execute_batch(sql)?;
        }
    }

    connection.execute_batch(
        r#"
        UPDATE track_deep_analysis
        SET feature_size_bytes =
          LENGTH(stem_feature_json) +
          LENGTH(vocal_windows_json) +
          LENGTH(drum_windows_json) +
          LENGTH(bass_windows_json) +
          LENGTH(section_json) +
          LENGTH(phrase_boundaries_json) +
          LENGTH(intro_outro_refined_json) +
          LENGTH(transition_hints_json) +
          LENGTH(transition_windows_json)
        WHERE feature_size_bytes = 0;

        CREATE INDEX IF NOT EXISTS idx_track_deep_analysis_track_id
          ON track_deep_analysis(track_id);
        CREATE INDEX IF NOT EXISTS idx_track_deep_analysis_last_used_at
          ON track_deep_analysis(last_used_at);
        "#,
    )?;
    if table_exists(connection, "deep_analysis_jobs") {
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_deep_analysis_jobs_priority_status
             ON deep_analysis_jobs(status, priority DESC, updated_at ASC)",
        )?;
    }
    Ok(())
}

fn ensure_albums_added_at_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "albums") {
        return Ok(());
    }

    if !column_exists(connection, "albums", "added_at")? {
        connection.execute_batch("ALTER TABLE albums ADD COLUMN added_at TEXT")?;
    }

    if table_exists(connection, "tracks") && column_exists(connection, "tracks", "scanned_at")? {
        connection.execute_batch(
            r#"
            UPDATE albums
            SET added_at = COALESCE(
              (
                SELECT MIN(t.scanned_at)
                FROM tracks t
                WHERE t.album_id = albums.id
                  AND t.scanned_at IS NOT NULL
                  AND TRIM(t.scanned_at) <> ''
              ),
              added_at,
              datetime('now')
            )
            WHERE added_at IS NULL OR TRIM(added_at) = ''
            "#,
        )?;
    }

    connection
        .execute_batch("CREATE INDEX IF NOT EXISTS idx_albums_added_at ON albums(added_at DESC)")?;
    Ok(())
}

fn ensure_artist_ratings_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !(table_exists(connection, "users") && table_exists(connection, "artists")) {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS artist_ratings (
          user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          artist_id TEXT NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
          rating REAL NOT NULL CHECK(rating >= 0.5 AND rating <= 5 AND rating * 2 = CAST(rating * 2 AS INTEGER)),
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY (user_id, artist_id)
        )
        "#,
    )?;
    Ok(())
}

fn ensure_typed_library_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "libraries") {
        return Ok(());
    }

    if !column_exists(connection, "libraries", "library_type")? {
        connection.execute_batch(
            "ALTER TABLE libraries ADD COLUMN library_type TEXT NOT NULL DEFAULT 'music' CHECK(library_type = 'music')",
        )?;
    }
    if !column_exists(connection, "libraries", "scanner_profile")? {
        connection.execute_batch(
            "ALTER TABLE libraries ADD COLUMN scanner_profile TEXT NOT NULL DEFAULT 'default'",
        )?;
    }
    if !column_exists(connection, "libraries", "metadata_mode")? {
        connection.execute_batch(
            "ALTER TABLE libraries ADD COLUMN metadata_mode TEXT NOT NULL DEFAULT 'path_only'",
        )?;
    }

    connection.execute_batch(
        r#"
        UPDATE libraries
        SET library_type = 'music'
        WHERE library_type IS NULL OR TRIM(library_type) = '';

        UPDATE libraries
        SET scanner_profile = 'default'
        WHERE scanner_profile IS NULL OR TRIM(scanner_profile) = '';

        UPDATE libraries
        SET metadata_mode = 'path_only'
        WHERE metadata_mode IS NULL OR TRIM(metadata_mode) = '';

        CREATE INDEX IF NOT EXISTS idx_libraries_type ON libraries(library_type);
        "#,
    )?;

    Ok(())
}

fn ensure_library_folders_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "libraries") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS library_folders (
          id TEXT PRIMARY KEY,
          library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
          path TEXT NOT NULL,
          position INTEGER NOT NULL DEFAULT 0,
          added_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_library_folders_library_id
          ON library_folders(library_id, position, id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_library_folders_path_unique
          ON library_folders(LOWER(TRIM(path)));
        CREATE UNIQUE INDEX IF NOT EXISTS idx_library_folders_library_position_unique
          ON library_folders(library_id, position);
        "#,
    )?;

    let mut select_libraries = connection
        .prepare("SELECT id, path FROM libraries ORDER BY datetime(added_at) ASC, id ASC")?;
    let libraries = select_libraries
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut has_folder =
        connection.prepare("SELECT 1 FROM library_folders WHERE library_id = ?1 LIMIT 1")?;
    let mut insert_folder = connection.prepare(
        "INSERT INTO library_folders(id, library_id, path, position) VALUES(?1, ?2, ?3, 0)",
    )?;
    for (library_id, library_path) in libraries {
        let normalized_path = library_path.trim();
        if normalized_path.is_empty() {
            continue;
        }
        let existing = has_folder
            .query_row([library_id.as_str()], |_| Ok(()))
            .optional()?
            .is_some();
        if existing {
            continue;
        }
        let folder_id = format!("folder-{library_id}");
        insert_folder.execute(params![folder_id, library_id, normalized_path])?;
    }

    let mut select_primary_folders = connection.prepare(
        r#"
        SELECT l.id, lf.path
        FROM libraries l
        LEFT JOIN library_folders lf
          ON lf.id = (
            SELECT inner_lf.id
            FROM library_folders inner_lf
            WHERE inner_lf.library_id = l.id
            ORDER BY inner_lf.position ASC, inner_lf.id ASC
            LIMIT 1
          )
        "#,
    )?;
    let primary_folders = select_primary_folders
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut update_library_path =
        connection.prepare("UPDATE libraries SET path = ?1 WHERE id = ?2")?;
    for (library_id, primary_path) in primary_folders {
        if let Some(primary_path) = primary_path.filter(|path| !path.trim().is_empty()) {
            update_library_path.execute(params![primary_path, library_id])?;
        }
    }

    Ok(())
}

fn ensure_tracks_last_played_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "tracks") {
        return Ok(());
    }

    if !column_exists(connection, "tracks", "last_played_at")? {
        connection.execute_batch("ALTER TABLE tracks ADD COLUMN last_played_at TEXT")?;
    }
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tracks_last_played ON tracks(last_played_at)",
    )?;
    Ok(())
}

fn ensure_playback_count_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "tracks") {
        if !column_exists(connection, "tracks", "play_count")? {
            connection.execute_batch(
                "ALTER TABLE tracks ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        connection.execute_batch(
            r#"
            UPDATE tracks
            SET play_count = 0
            WHERE play_count IS NULL
            "#,
        )?;
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tracks_play_count ON tracks(play_count)",
        )?;
    }

    if table_exists(connection, "artists") {
        if !column_exists(connection, "artists", "play_count")? {
            connection.execute_batch(
                "ALTER TABLE artists ADD COLUMN play_count INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        connection.execute_batch(
            r#"
            UPDATE artists
            SET play_count = 0
            WHERE play_count IS NULL
            "#,
        )?;
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_artists_play_count ON artists(play_count)",
        )?;
    }

    Ok(())
}

/// Adds the browse/lookup indexes that back the correlated-subquery hot paths.
///
/// `albums.artist_id` is the important one: `list_artists` (and the
/// artists-with-albums counts used by home genre stats and the compilation
/// filter) correlate on it, so without an index every artist row triggers a
/// full scan of `albums`. The remaining four cover FK lookups that are cheap
/// today only because their tables are still small.
fn ensure_browse_performance_indexes(connection: &Connection) -> Result<(), rusqlite::Error> {
    // Guarded by column_exists: some legacy migration-chain fixtures predate
    // albums.artist_id. DBs created via initialize_schema always have it.
    if table_exists(connection, "albums") && column_exists(connection, "albums", "artist_id")? {
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_albums_artist_id ON albums(artist_id)",
        )?;
    }

    if table_exists(connection, "playlist_tracks")
        && column_exists(connection, "playlist_tracks", "track_id")?
    {
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_playlist_tracks_track ON playlist_tracks(track_id)",
        )?;
    }

    if table_exists(connection, "track_ratings")
        && column_exists(connection, "track_ratings", "track_id")?
    {
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_track_ratings_track ON track_ratings(track_id)",
        )?;
    }

    if table_exists(connection, "play_history")
        && column_exists(connection, "play_history", "track_id")?
    {
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_play_history_track ON play_history(track_id)",
        )?;
    }

    if table_exists(connection, "crossfade_overrides")
        && column_exists(connection, "crossfade_overrides", "entity_type")?
        && column_exists(connection, "crossfade_overrides", "entity_id")?
    {
        // upsert_crossfade_override deletes before inserting, so one row per
        // (entity_type, entity_id) is the intended invariant. Legacy DBs could
        // still carry duplicates, so collapse them before enforcing it.
        connection.execute_batch(
            r#"
            DELETE FROM crossfade_overrides
            WHERE rowid NOT IN (
              SELECT MIN(rowid) FROM crossfade_overrides GROUP BY entity_type, entity_id
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_crossfade_overrides_entity
              ON crossfade_overrides(entity_type, entity_id);
            "#,
        )?;
    }

    Ok(())
}

fn ensure_music_ratings_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "users") {
        return Ok(());
    }

    if table_exists(connection, "albums") {
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS album_ratings (
              user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
              rating REAL NOT NULL CHECK(rating >= 0.5 AND rating <= 5 AND rating * 2 = CAST(rating * 2 AS INTEGER)),
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              PRIMARY KEY (user_id, album_id)
            )
            "#,
        )?;
    }

    if table_exists(connection, "tracks") {
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS track_ratings (
              user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
              track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
              rating REAL NOT NULL CHECK(rating >= 0.5 AND rating <= 5 AND rating * 2 = CAST(rating * 2 AS INTEGER)),
              updated_at TEXT NOT NULL DEFAULT (datetime('now')),
              PRIMARY KEY (user_id, track_id)
            )
            "#,
        )?;
    }

    Ok(())
}

fn ensure_crossfade_overrides_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS crossfade_overrides (
          id TEXT PRIMARY KEY,
          entity_type TEXT NOT NULL CHECK(entity_type IN ('album','playlist','autodj')),
          entity_id TEXT NOT NULL,
          mode TEXT NOT NULL CHECK(mode IN ('off','zerogap','crossfade')),
          duration INTEGER NOT NULL DEFAULT 2
        )
        "#,
    )?;
    Ok(())
}

fn ensure_track_waveforms_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "tracks") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS track_waveforms (
          track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          sample_count INTEGER NOT NULL DEFAULT 0,
          duration_seconds REAL,
          waveform_json TEXT NOT NULL,
          version INTEGER NOT NULL DEFAULT 1,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_track_waveforms_updated_at ON track_waveforms(updated_at);
        "#,
    )?;
    Ok(())
}

fn ensure_lyrics_cache_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "tracks") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS lyrics_cache (
          track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
          artist TEXT NOT NULL,
          title TEXT NOT NULL,
          lyrics TEXT NOT NULL,
          synced_lyrics TEXT,
          source TEXT NOT NULL DEFAULT 'lrclib',
          fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_lyrics_cache_artist_title
          ON lyrics_cache(LOWER(TRIM(artist)), LOWER(TRIM(title)));
        "#,
    )?;

    if !column_exists(connection, "lyrics_cache", "synced_lyrics")? {
        connection.execute_batch("ALTER TABLE lyrics_cache ADD COLUMN synced_lyrics TEXT")?;
    }

    Ok(())
}

fn ensure_play_history_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !(table_exists(connection, "users") && table_exists(connection, "tracks")) {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS play_history (
          id TEXT PRIMARY KEY,
          user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
          played_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_play_history_user ON play_history(user_id, played_at DESC);
        "#,
    )?;
    Ok(())
}

fn remove_video_support_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        r#"
        DROP TABLE IF EXISTS movie_ratings;
        DROP TABLE IF EXISTS show_ratings;
        DROP TABLE IF EXISTS video_artwork_assets;
        DROP TABLE IF EXISTS video_reviews;
        DROP TABLE IF EXISTS video_people;
        DROP TABLE IF EXISTS metadata_matches;
        DROP TABLE IF EXISTS video_playback_progress;
        DROP TABLE IF EXISTS audio_streams;
        DROP TABLE IF EXISTS subtitle_tracks;
        DROP TABLE IF EXISTS media_files;
        DROP TABLE IF EXISTS episodes;
        DROP TABLE IF EXISTS seasons;
        DROP TABLE IF EXISTS shows_fts;
        DROP TABLE IF EXISTS shows;
        DROP TABLE IF EXISTS movies_fts;
        DROP TABLE IF EXISTS movies;
        "#,
    )?;

    if table_exists(connection, "post_scan_jobs") {
        connection.execute_batch(
            r#"
            DELETE FROM post_scan_jobs
            WHERE job_type IN (
              'sync_video_metadata',
              'sync_video_title_metadata',
              'warm_video_filmography',
              'cache_video_filmography_artwork'
            );
            "#,
        )?;
    }
    if table_exists(connection, "libraries")
        && column_exists(connection, "libraries", "library_type")?
    {
        connection.execute(
            "DELETE FROM libraries WHERE library_type IN ('movies', 'tv')",
            [],
        )?;
    }
    if table_exists(connection, "scan_jobs") && table_exists(connection, "libraries") {
        connection.execute_batch(
            "DELETE FROM scan_jobs WHERE library_id NOT IN (SELECT id FROM libraries)",
        )?;
    }
    if table_exists(connection, "scan_schedules") && table_exists(connection, "libraries") {
        connection.execute_batch(
            "DELETE FROM scan_schedules WHERE library_id NOT IN (SELECT id FROM libraries)",
        )?;
    }
    if table_exists(connection, "post_scan_jobs") && table_exists(connection, "libraries") {
        connection.execute_batch(
            "DELETE FROM post_scan_jobs WHERE library_id NOT IN (SELECT id FROM libraries)",
        )?;
    }
    if table_exists(connection, "settings") {
        connection.execute_batch(
            r#"
            DELETE FROM settings
            WHERE key IN (
              'tmdbApiKey',
              'dlnaMediaTypes',
              'dlnaVideoTranscoding',
              'dlnaForceVideoTranscode'
            );
            "#,
        )?;
    }

    rebuild_music_only_stats_cache_table(connection)?;
    Ok(())
}

fn ensure_library_last_scan_column(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "libraries") {
        return Ok(());
    }
    if !column_exists(connection, "libraries", "last_scan")? {
        connection.execute_batch("ALTER TABLE libraries ADD COLUMN last_scan TEXT")?;
    }
    Ok(())
}

fn rebuild_music_only_stats_cache_table(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "stats_cache") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
        CREATE TABLE stats_cache_music_only (
          id TEXT PRIMARY KEY,
          total_tracks INTEGER NOT NULL DEFAULT 0,
          total_artists INTEGER NOT NULL DEFAULT 0,
          total_albums INTEGER NOT NULL DEFAULT 0,
          total_libraries INTEGER NOT NULL DEFAULT 0,
          total_hours REAL NOT NULL DEFAULT 0,
          total_gb REAL NOT NULL DEFAULT 0,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        INSERT INTO stats_cache_music_only(
          id, total_tracks, total_artists, total_albums,
          total_libraries, total_hours, total_gb, updated_at
        )
        SELECT
          id,
          COALESCE(total_tracks, 0),
          COALESCE(total_artists, 0),
          COALESCE(total_albums, 0),
          COALESCE(total_libraries, 0),
          COALESCE(total_hours, 0),
          COALESCE(total_gb, 0),
          COALESCE(NULLIF(updated_at, ''), datetime('now'))
        FROM stats_cache;
        DROP TABLE stats_cache;
        ALTER TABLE stats_cache_music_only RENAME TO stats_cache;
        "#,
    )?;
    Ok(())
}

/// Documents the Refresh Denormalized Counts public API surface.
pub fn refresh_denormalized_counts(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "artists") && table_exists(connection, "tracks") {
        connection.execute_batch(
            r#"
            UPDATE artists SET
              track_count = (SELECT COUNT(*) FROM tracks WHERE artist_id = artists.id),
              album_count = (
                SELECT COUNT(DISTINCT album_id)
                FROM tracks
                WHERE artist_id = artists.id
                  AND album_id IS NOT NULL
              )
            "#,
        )?;
    }

    if table_exists(connection, "albums") && table_exists(connection, "tracks") {
        connection.execute_batch(
            r#"
            UPDATE albums SET
              track_count = (SELECT COUNT(*) FROM tracks WHERE album_id = albums.id),
              total_duration_sec = (
                SELECT COALESCE(SUM(duration), 0)
                FROM tracks
                WHERE album_id = albums.id
              )
            "#,
        )?;

        // The scanner only ever wrote year/genre onto tracks, so every album row
        // kept a NULL year and browse could not sort by it. Derive both from the
        // album's own tracks: the earliest tagged year (remasters and reissues
        // often carry a later year on individual tracks, and the original release
        // year is what browse should sort by) and the most common non-empty genre.
        // Albums whose metadata was edited by hand are locked and left alone.
        if column_exists(connection, "albums", "metadata_locked")? {
            connection.execute_batch(
                r#"
                UPDATE albums SET
                  year = COALESCE(
                    (
                      SELECT MIN(t.year) FROM tracks t
                      WHERE t.album_id = albums.id AND t.year IS NOT NULL AND t.year > 0
                    ),
                    year
                  ),
                  genre = COALESCE(
                    (
                      SELECT TRIM(t.genre) FROM tracks t
                      WHERE t.album_id = albums.id AND TRIM(COALESCE(t.genre, '')) != ''
                      GROUP BY LOWER(TRIM(t.genre))
                      ORDER BY COUNT(*) DESC, LOWER(TRIM(t.genre)) ASC
                      LIMIT 1
                    ),
                    genre
                  )
                WHERE COALESCE(metadata_locked, 0) = 0
                "#,
            )?;
        }
    }

    Ok(())
}

/// Documents the Refresh Stats Cache public API surface.
pub fn refresh_stats_cache(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "stats_cache") {
        return Ok(());
    }

    let total_tracks = count_rows(connection, "tracks")?;
    // Artists that only appear via tracks on someone else's album (e.g. a track or
    // two on a Various Artists compilation) don't own any release of their own, so
    // they're excluded here to match what Browse shows by default.
    let total_artists = if column_exists(connection, "albums", "artist_id")? {
        count_where(
            connection,
            "artists",
            "EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = artists.id)",
        )?
    } else {
        count_where(connection, "artists", "track_count > 0")?
    };
    let total_albums = count_where(connection, "albums", "track_count > 0")?;
    let total_libraries = count_rows(connection, "libraries")?;
    let total_hours = sum_real(connection, "tracks", "duration")? / 3600.0;
    let total_gb = sum_real(connection, "tracks", "file_size")? / 1_073_741_824.0;

    let stats_cache_id = connection
        .query_row("SELECT id FROM stats_cache LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
        .unwrap_or_else(|| "stats-cache".to_string());

    connection.execute(
        r#"
        INSERT INTO stats_cache(
          id, total_tracks, total_artists, total_albums,
          total_libraries, total_hours, total_gb, updated_at
        )
        VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
        ON CONFLICT(id) DO UPDATE SET
          total_tracks = excluded.total_tracks,
          total_artists = excluded.total_artists,
          total_albums = excluded.total_albums,
          total_libraries = excluded.total_libraries,
          total_hours = excluded.total_hours,
          total_gb = excluded.total_gb,
          updated_at = excluded.updated_at
        "#,
        params![
            stats_cache_id,
            total_tracks,
            total_artists,
            total_albums,
            total_libraries,
            total_hours,
            total_gb
        ],
    )?;

    Ok(())
}

#[derive(Debug)]
struct LibraryNameRow {
    id: String,
    name: String,
    path: String,
    primary_path: Option<String>,
}

fn normalize_library_name(name: &str) -> Option<String> {
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

fn build_unique_library_name(
    base_name: &str,
    used_names: &mut std::collections::HashSet<String>,
) -> String {
    let mut candidate = base_name.to_string();
    let mut suffix = 2_u32;
    while !used_names.insert(candidate.to_lowercase()) {
        candidate = format!("{base_name} {suffix}");
        suffix += 1;
    }
    candidate
}

fn table_exists(connection: &Connection, table_name: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            [table_name],
            |_| Ok(()),
        )
        .is_ok()
}

fn column_exists(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, rusqlite::Error> {
    let pragma = format!("PRAGMA table_info({table_name})");
    let mut statement = connection.prepare(&pragma)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row?.eq_ignore_ascii_case(column_name) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn count_rows(connection: &Connection, table_name: &str) -> Result<i64, rusqlite::Error> {
    if !table_exists(connection, table_name) {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {table_name}");
    connection.query_row(&sql, [], |row| row.get::<_, i64>(0))
}

fn count_where(
    connection: &Connection,
    table_name: &str,
    where_clause: &str,
) -> Result<i64, rusqlite::Error> {
    if !table_exists(connection, table_name) {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {table_name} WHERE {where_clause}");
    connection.query_row(&sql, [], |row| row.get::<_, i64>(0))
}

fn sum_real(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<f64, rusqlite::Error> {
    if !(table_exists(connection, table_name)
        && column_exists(connection, table_name, column_name)?)
    {
        return Ok(0.0);
    }
    let sql = format!("SELECT COALESCE(SUM({column_name}), 0) FROM {table_name}");
    connection.query_row(&sql, [], |row| row.get::<_, f64>(0))
}

fn seed_admin_user(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "users") {
        return Ok(());
    }
    let admin_exists: bool = connection
        .query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'admin'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !admin_exists {
        let admin_id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT OR IGNORE INTO users(id, username, role) VALUES(?1, 'admin', 'admin')",
            [admin_id],
        )?;
    }
    // Assign existing playlists with no owner to the admin user
    if table_exists(connection, "playlists") && column_exists(connection, "playlists", "user_id")? {
        connection.execute_batch(
            r#"UPDATE playlists
               SET user_id = (SELECT id FROM users WHERE role='admin' LIMIT 1)
               WHERE user_id IS NULL"#,
        )?;
    }
    Ok(())
}

fn seed_default_settings(connection: &Connection) -> Result<(), rusqlite::Error> {
    let defaults = [
        ("discogsToken", ""),
        ("lastfmKey", ""),
        ("spotifyClientId", ""),
        ("spotifyClientSecret", ""),
        ("geniusClientId", ""),
        ("geniusClientSecret", ""),
        ("dlnaEnabled", "false"),
        ("dlnaFriendlyName", "BoogieBox"),
        ("dlnaPort", "8200"),
        ("dlnaMediaMode", "audio"),
        ("waveformGenerateOnMissing", "true"),
        ("waveformBackgroundEnabled", "false"),
        ("waveformBackgroundFrequencyHours", "24"),
        ("waveformBackgroundBatchSize", "100"),
        ("bpmAnalysisEnabled", "true"),
        ("bpmBackgroundEnabled", "false"),
        ("bpmBackgroundFrequencyHours", "24"),
        ("scanDebugLoggingEnabled", "false"),
        ("deepmixDebugLoggingEnabled", "false"),
        ("boogiemixOutputFolder", ""),
        ("boogiemixDeepAnalysisEnabled", "true"),
        ("boogiemixDeepAnalysisBackgroundMode", "off"),
        ("boogiemixDeepAnalysisPauseBackground", "false"),
        ("boogiemixDeepAnalysisModel", "mdx_extra_q"),
        ("boogiemixDebugCandidates", "false"),
    ];

    let mut statement = connection.prepare(
        "INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES(?1, ?2, datetime('now'))",
    )?;
    for (key, value) in defaults {
        statement.execute((key, value))?;
    }
    Ok(())
}

// ── Public auth / user query types ──────────────────────────────────────────

/// Public Login User data shape used by BoogieBox.
#[derive(Debug)]
pub struct LoginUser {
    /// Documents the Id public API surface.
    pub id: String,
    /// Documents the Username public API surface.
    pub username: String,
}

/// Public Auth User Row data shape used by BoogieBox.
#[derive(Debug)]
pub struct AuthUserRow {
    /// Documents the Id public API surface.
    pub id: String,
    /// Documents the Username public API surface.
    pub username: String,
    /// Documents the Role public API surface.
    pub role: String,
    /// Documents the Pin Hash public API surface.
    pub pin_hash: Option<String>,
    /// Documents the Can Manage Libraries public API surface.
    pub can_manage_libraries: bool,
    /// Documents the Can Edit Metadata public API surface.
    pub can_edit_metadata: bool,
}

/// Public Session User data shape used by BoogieBox.
#[derive(Debug)]
pub struct SessionUser {
    /// Documents the User Id public API surface.
    pub user_id: String,
    /// Documents the Username public API surface.
    pub username: String,
    /// Documents the Role public API surface.
    pub role: String,
    /// Documents the Can Manage Libraries public API surface.
    pub can_manage_libraries: bool,
    /// Documents the Can Edit Metadata public API surface.
    pub can_edit_metadata: bool,
}

/// Public Admin User Row data shape used by BoogieBox.
#[derive(Debug)]
pub struct AdminUserRow {
    /// Documents the Id public API surface.
    pub id: String,
    /// Documents the Username public API surface.
    pub username: String,
    /// Documents the Role public API surface.
    pub role: String,
    /// Documents the Has Pin public API surface.
    pub has_pin: bool,
    /// Documents the Can Manage Libraries public API surface.
    pub can_manage_libraries: bool,
    /// Documents the Can Edit Metadata public API surface.
    pub can_edit_metadata: bool,
    /// Documents the Created At public API surface.
    pub created_at: String,
}

// ── Public auth / user query functions ──────────────────────────────────────

/// Documents the List Login Users public API surface.
pub fn list_login_users(conn: &Connection) -> Result<Vec<LoginUser>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT id, username FROM users ORDER BY username ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(LoginUser {
            id: row.get(0)?,
            username: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Documents the Get User For Auth public API surface.
pub fn get_user_for_auth(
    conn: &Connection,
    user_id: &str,
) -> Result<Option<AuthUserRow>, rusqlite::Error> {
    conn.query_row(
        r#"SELECT id, username, role, pin_hash,
                  COALESCE(can_manage_libraries, 0), COALESCE(can_edit_metadata, 0)
           FROM users WHERE id = ?1"#,
        [user_id],
        |row| {
            Ok(AuthUserRow {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                pin_hash: row.get(3)?,
                can_manage_libraries: row.get::<_, i64>(4)? != 0,
                can_edit_metadata: row.get::<_, i64>(5)? != 0,
            })
        },
    )
    .optional()
}

/// Documents the Create Session public API surface.
pub fn create_session(
    conn: &Connection,
    token: &str,
    user_id: &str,
    expires_at: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sessions(token, user_id, expires_at) VALUES(?1, ?2, ?3)",
        params![token, user_id, expires_at],
    )?;
    Ok(())
}

/// Documents the Get Session User public API surface.
pub fn get_session_user(
    conn: &Connection,
    token: &str,
) -> Result<Option<SessionUser>, rusqlite::Error> {
    conn.query_row(
        r#"SELECT s.user_id, u.username, u.role,
                  COALESCE(u.can_manage_libraries, 0), COALESCE(u.can_edit_metadata, 0)
           FROM sessions s
           JOIN users u ON u.id = s.user_id
           WHERE s.token = ?1 AND s.expires_at > datetime('now')"#,
        [token],
        |row| {
            Ok(SessionUser {
                user_id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                can_manage_libraries: row.get::<_, i64>(3)? != 0,
                can_edit_metadata: row.get::<_, i64>(4)? != 0,
            })
        },
    )
    .optional()
}

/// Documents the Delete Session public API surface.
pub fn delete_session(conn: &Connection, token: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", [token])?;
    Ok(())
}

/// Documents the Delete Expired Sessions For User public API surface.
pub fn delete_expired_sessions_for_user(
    conn: &Connection,
    user_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sessions WHERE user_id = ?1 AND expires_at <= datetime('now')",
        [user_id],
    )?;
    Ok(())
}

/// Documents the List Admin Users public API surface.
pub fn list_admin_users(conn: &Connection) -> Result<Vec<AdminUserRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"SELECT id, username, role,
                  CASE WHEN pin_hash IS NOT NULL THEN 1 ELSE 0 END AS has_pin,
                  COALESCE(can_manage_libraries, 0), COALESCE(can_edit_metadata, 0), created_at
           FROM users
           ORDER BY role DESC, username ASC"#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AdminUserRow {
            id: row.get(0)?,
            username: row.get(1)?,
            role: row.get(2)?,
            has_pin: row.get::<_, i64>(3)? != 0,
            can_manage_libraries: row.get::<_, i64>(4)? != 0,
            can_edit_metadata: row.get::<_, i64>(5)? != 0,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// Documents the Create User public API surface.
pub fn create_user(
    conn: &Connection,
    id: &str,
    username: &str,
    role: &str,
    pin_hash: Option<&str>,
    can_manage_libraries: bool,
    can_edit_metadata: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO users(id, username, role, pin_hash, can_manage_libraries, can_edit_metadata) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, username, role, pin_hash, can_manage_libraries as i64, can_edit_metadata as i64],
    )?;
    Ok(())
}

/// Documents the Update User Permissions public API surface.
pub fn update_user_permissions(
    conn: &Connection,
    user_id: &str,
    can_manage_libraries: bool,
    can_edit_metadata: bool,
) -> Result<bool, rusqlite::Error> {
    let changed = conn.execute(
        "UPDATE users SET can_manage_libraries = ?1, can_edit_metadata = ?2 WHERE id = ?3",
        params![
            can_manage_libraries as i64,
            can_edit_metadata as i64,
            user_id
        ],
    )?;
    Ok(changed > 0)
}

/// Documents the Update User Pin public API surface.
pub fn update_user_pin(
    conn: &Connection,
    user_id: &str,
    pin_hash: Option<&str>,
) -> Result<bool, rusqlite::Error> {
    let changed = conn.execute(
        "UPDATE users SET pin_hash = ?1 WHERE id = ?2",
        params![pin_hash, user_id],
    )?;
    Ok(changed > 0)
}

/// Documents the Delete User By Id public API surface.
pub fn delete_user_by_id(conn: &Connection, user_id: &str) -> Result<bool, rusqlite::Error> {
    let changed = conn.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
    Ok(changed > 0)
}

/// Documents the Count Admin Users public API surface.
pub fn count_admin_users(conn: &Connection) -> Result<u64, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM users WHERE role = 'admin'",
        [],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

/// Documents the Get All Settings public API surface.
pub fn get_all_settings(conn: &Connection) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, v);
    }
    Ok(map)
}

/// Documents the Get Settings By Keys public API surface.
pub fn get_settings_by_keys(
    conn: &Connection,
    keys: &[&str],
) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut map = HashMap::new();
    for &key in keys {
        let val: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(v) = val {
            map.insert(key.to_string(), v);
        }
    }
    Ok(map)
}

/// Documents the Upsert Setting public API surface.
pub fn upsert_setting(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"INSERT INTO settings(key, value, updated_at) VALUES(?1, ?2, datetime('now'))
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        params![key, value],
    )?;
    Ok(())
}

/// Documents the Get User Settings public API surface.
pub fn get_user_settings(
    conn: &Connection,
    user_id: &str,
) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM user_settings WHERE user_id = ?1")?;
    let rows = stmt.query_map([user_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, v);
    }
    Ok(map)
}

/// Documents the Upsert User Setting public API surface.
pub fn upsert_user_setting(
    conn: &Connection,
    user_id: &str,
    key: &str,
    value: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"INSERT INTO user_settings(user_id, key, value, updated_at) VALUES(?1, ?2, ?3, datetime('now'))
           ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"#,
        params![user_id, key, value],
    )?;
    Ok(())
}

fn ensure_provider_usage_and_art_tables(connection: &Connection) -> Result<(), rusqlite::Error> {
    // Fix provider_usage_stats if it has the old bootstrap schema (has column 'action').
    // The correct schema uses a composite PK of (provider, entity_type, usage_type) with a count.
    let has_old_schema = column_exists(connection, "provider_usage_stats", "action")?;
    if has_old_schema {
        connection.execute_batch("DROP TABLE IF EXISTS provider_usage_stats;")?;
    }

    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS provider_usage_stats (
          provider TEXT NOT NULL,
          entity_type TEXT NOT NULL,
          usage_type TEXT NOT NULL,
          count INTEGER NOT NULL DEFAULT 1,
          last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY(provider, entity_type, usage_type)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_usage_last_used
          ON provider_usage_stats(last_used_at DESC);

        CREATE TABLE IF NOT EXISTS lastfm_cache (
          cache_key TEXT PRIMARY KEY,
          data TEXT NOT NULL,
          fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
          expires_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_lastfm_cache_expires
          ON lastfm_cache(expires_at);

        CREATE TABLE IF NOT EXISTS artist_styles (
          artist_id TEXT NOT NULL,
          style TEXT NOT NULL COLLATE NOCASE,
          updated_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY (artist_id, style)
        );
        CREATE INDEX IF NOT EXISTS idx_artist_styles_style
          ON artist_styles(style);
        CREATE INDEX IF NOT EXISTS idx_artist_styles_updated
          ON artist_styles(updated_at);
        "#,
    )?;
    Ok(())
}

fn ensure_job_schedule_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(connection, "scan_jobs") {
        for (column, definition) in [
            ("started_at", "TEXT"),
            ("finished_at", "TEXT"),
            ("files_found", "INTEGER DEFAULT 0"),
            ("files_scanned", "INTEGER DEFAULT 0"),
            ("errors", "INTEGER DEFAULT 0"),
            ("error_log", "TEXT"),
        ] {
            if !column_exists(connection, "scan_jobs", column)? {
                connection.execute_batch(&format!(
                    "ALTER TABLE scan_jobs ADD COLUMN {column} {definition}"
                ))?;
            }
        }
    }

    if table_exists(connection, "post_scan_jobs") {
        for (column, definition) in [
            ("payload", "TEXT"),
            ("started_at", "TEXT"),
            ("heartbeat_at", "TEXT"),
            ("finished_at", "TEXT"),
            ("error_log", "TEXT"),
        ] {
            if !column_exists(connection, "post_scan_jobs", column)? {
                connection.execute_batch(&format!(
                    "ALTER TABLE post_scan_jobs ADD COLUMN {column} {definition}"
                ))?;
            }
        }
    }

    if table_exists(connection, "scan_schedules") {
        if !column_exists(connection, "scan_schedules", "frequency_hours")? {
            connection
                .execute_batch("ALTER TABLE scan_schedules ADD COLUMN frequency_hours REAL")?;
            if column_exists(connection, "scan_schedules", "interval_hours")? {
                connection.execute_batch(
                    "UPDATE scan_schedules
                     SET frequency_hours = COALESCE(interval_hours, 24)
                     WHERE frequency_hours IS NULL",
                )?;
            }
            connection.execute_batch(
                "UPDATE scan_schedules
                 SET frequency_hours = 24
                 WHERE frequency_hours IS NULL OR frequency_hours < 0.5",
            )?;
        }
        if !column_exists(connection, "scan_schedules", "last_run")? {
            connection.execute_batch("ALTER TABLE scan_schedules ADD COLUMN last_run TEXT")?;
        }
        if !column_exists(connection, "scan_schedules", "next_run")? {
            connection.execute_batch("ALTER TABLE scan_schedules ADD COLUMN next_run TEXT")?;
        }
        connection.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_scan_schedules_library_unique
               ON scan_schedules(library_id);
             CREATE INDEX IF NOT EXISTS idx_schedules_next ON scan_schedules(next_run);
             CREATE INDEX IF NOT EXISTS idx_scan_jobs_lib ON scan_jobs(library_id);
             CREATE INDEX IF NOT EXISTS idx_post_scan_jobs_status ON post_scan_jobs(status, id);
             CREATE INDEX IF NOT EXISTS idx_post_scan_jobs_library ON post_scan_jobs(library_id, job_type);",
        )?;
    }

    Ok(())
}

fn ensure_track_scan_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    if !table_exists(connection, "tracks") {
        return Ok(());
    }
    for (column, definition) in [
        ("library_id", "TEXT"),
        ("artist_id", "TEXT"),
        ("album_id", "TEXT"),
        ("title", "TEXT NOT NULL DEFAULT ''"),
        ("file_name", "TEXT NOT NULL DEFAULT ''"),
        ("file_path", "TEXT"),
        ("file_size", "INTEGER NOT NULL DEFAULT 0"),
        ("album_artist", "TEXT NOT NULL DEFAULT ''"),
        ("genre", "TEXT NOT NULL DEFAULT ''"),
        ("composer", "TEXT NOT NULL DEFAULT ''"),
        ("duration", "REAL NOT NULL DEFAULT 0"),
        ("format", "TEXT"),
        ("bitrate", "INTEGER"),
        ("sample_rate", "INTEGER"),
        ("channels", "INTEGER"),
        ("track_number", "INTEGER"),
        ("disc_number", "INTEGER"),
        ("year", "INTEGER"),
        ("comment", "TEXT"),
        ("bpm", "INTEGER"),
        ("bpm_detected", "REAL"),
        ("bpm_source", "TEXT"),
        ("bpm_confidence", "REAL"),
        ("bpm_analyzed_at", "TEXT"),
    ] {
        if !column_exists(connection, "tracks", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE tracks ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    if column_exists(connection, "tracks", "file_path")? {
        connection.execute_batch(
            r#"
            UPDATE tracks
            SET file_path = 'legacy:' || CAST(id AS TEXT)
            WHERE file_path IS NULL OR TRIM(file_path) = '';
            "#,
        )?;
    }
    connection.execute_batch(
        r#"
        UPDATE tracks
        SET file_name = file_path
        WHERE file_name IS NULL OR TRIM(file_name) = '';
        CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
        "#,
    )?;
    if column_exists(connection, "tracks", "file_path")? {
        connection.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tracks_file_path_unique ON tracks(file_path)",
        )?;
    }
    if column_exists(connection, "tracks", "library_id")? {
        connection
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_tracks_library ON tracks(library_id)")?;
    }
    if column_exists(connection, "tracks", "artist_id")? {
        connection
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist_id)")?;
    }
    if column_exists(connection, "tracks", "album_id")? {
        connection
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id)")?;
    }
    if column_exists(connection, "tracks", "genre")? {
        connection.execute_batch("CREATE INDEX IF NOT EXISTS idx_tracks_genre ON tracks(genre)")?;
    }
    if column_exists(connection, "tracks", "year")? {
        connection.execute_batch("CREATE INDEX IF NOT EXISTS idx_tracks_year ON tracks(year)")?;
    }
    Ok(())
}

fn ensure_boogiemix_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    // Repair mix_jobs columns that older Node DBs may be missing
    if table_exists(connection, "mix_jobs") {
        for (col, def) in [
            ("mix_style", "TEXT NOT NULL DEFAULT 'club_blend'"),
            ("mix_quality", "TEXT NOT NULL DEFAULT 'standard'"),
            ("mix_strategy", "TEXT"),
            ("planner_provider", "TEXT"),
            ("used_deep_analysis", "INTEGER NOT NULL DEFAULT 0"),
            ("deep_analysis_status", "TEXT"),
            ("cancel_requested", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !column_exists(connection, "mix_jobs", col)? {
                connection
                    .execute_batch(&format!("ALTER TABLE mix_jobs ADD COLUMN {col} {def}"))?;
            }
        }
        connection.execute_batch(
            "UPDATE mix_jobs SET mix_style='club_blend'
             WHERE mix_style IS NULL OR TRIM(mix_style)='';
             UPDATE mix_jobs SET mix_quality='standard'
             WHERE mix_quality IS NULL OR TRIM(mix_quality)='';",
        )?;
    }
    // Repair track_mix_analysis columns
    if table_exists(connection, "track_mix_analysis") {
        for (col, def) in [
            ("key_estimate", "TEXT"),
            ("beat_grid_sec", "REAL"),
            ("phrase_bars", "INTEGER"),
        ] {
            if !column_exists(connection, "track_mix_analysis", col)? {
                connection.execute_batch(&format!(
                    "ALTER TABLE track_mix_analysis ADD COLUMN {col} {def}"
                ))?;
            }
        }
    }
    Ok(())
}

pub mod boogiemix;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, time::SystemTime};

    #[test]
    fn detects_network_paths_and_picks_delete_journal_for_unc() {
        let unc = PathBuf::from(r"\\server\share\boogiebox.db");
        let local = PathBuf::from(r"D:\BoogieBox\data\boogiebox.db");

        assert!(is_network_db_path(&unc));
        assert!(!is_network_db_path(&local));
        assert_eq!(get_preferred_journal_mode(&unc), JournalMode::Delete);
        assert_eq!(get_preferred_journal_mode(&local), JournalMode::Wal);
    }

    #[test]
    fn get_db_path_appends_database_file_name() {
        let db_path = get_db_path(Path::new(r"D:\BoogieBox\data")).expect("db path");

        assert_eq!(db_path, PathBuf::from(r"D:\BoogieBox\data\boogiebox.db"));
    }

    #[test]
    fn get_db_path_accepts_database_file_path() {
        let db_path = get_db_path(Path::new(r"\\server\share\data\boogiebox.db")).expect("db path");

        assert_eq!(db_path, PathBuf::from(r"\\server\share\data\boogiebox.db"));
    }

    #[test]
    fn database_exists_checks_for_real_database_file() {
        let root = temp_dir("db-exists");
        fs::create_dir_all(&root).expect("create dir");

        assert!(!database_exists(&root));

        let db_path = get_db_path(&root).expect("db path");
        fs::write(&db_path, []).expect("write db file");

        assert!(database_exists(&root));
    }

    #[test]
    fn init_db_creates_database_file_core_tables_and_seed_settings() {
        let root = temp_dir("init-db");
        let InitializedDatabase {
            connection,
            db_path,
            journal_mode,
        } = init_db(&root).expect("db init");

        assert!(db_path.is_file());
        assert_eq!(journal_mode, JournalMode::Wal);
        assert_eq!(query_single_i64(&connection, "PRAGMA foreign_keys"), 1);
        assert_eq!(query_single_text(&connection, "PRAGMA journal_mode"), "wal");
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT value FROM settings WHERE key = 'dlnaFriendlyName'"
            ),
            "BoogieBox"
        );
        assert!(table_exists(&connection, "schema_migrations"));
        assert!(table_exists(&connection, "libraries"));
        assert!(table_exists(&connection, "tracks"));
        assert!(table_exists(&connection, "stats_cache"));
        assert!(table_exists(&connection, "artist_ratings"));
        assert!(table_exists(&connection, "crossfade_overrides"));
        assert!(table_exists(&connection, "track_waveforms"));
        assert!(table_exists(&connection, "lyrics_cache"));
        assert!(table_exists(&connection, "play_history"));
        assert!(!table_exists(&connection, "movies"));
        assert!(!table_exists(&connection, "shows"));
        assert!(!table_exists(&connection, "media_files"));
        assert!(!table_exists(&connection, "video_people"));
        assert!(!table_exists(&connection, "video_reviews"));
        assert!(!table_exists(&connection, "video_artwork_assets"));
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM schema_migrations WHERE id = '2026-05-23-library-name-uniqueness'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM schema_migrations WHERE id = '2026-05-23-playlist-name-uniqueness'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM schema_migrations WHERE id = '2026-05-23-music-feature-tables'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM schema_migrations WHERE id = '2026-05-25-remove-video-support'"
            ),
            1
        );
    }

    #[test]
    fn tracked_migration_backfills_albums_added_at_and_artist_ratings() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE settings (
                  key TEXT PRIMARY KEY,
                  value TEXT NOT NULL
                );
                CREATE TABLE users (
                  id TEXT PRIMARY KEY,
                  pin_hash TEXT NOT NULL,
                  name TEXT NOT NULL
                );
                CREATE TABLE artists (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  track_count INTEGER NOT NULL DEFAULT 0,
                  album_count INTEGER NOT NULL DEFAULT 0,
                  play_count INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE albums (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  album_artist TEXT NOT NULL DEFAULT '',
                  track_count INTEGER NOT NULL DEFAULT 0,
                  total_duration_sec REAL NOT NULL DEFAULT 0
                );
                CREATE TABLE tracks (
                  id TEXT PRIMARY KEY,
                  artist_id TEXT,
                  album_id TEXT,
                  duration REAL NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0,
                  scanned_at TEXT
                );
                INSERT INTO albums(id, title, album_artist) VALUES ('album-1', 'Alpha', 'Artist');
                INSERT INTO tracks(id, album_id, scanned_at) VALUES
                  ('track-1', 'album-1', '2026-05-20 10:00:00'),
                  ('track-2', 'album-1', '2026-05-21 10:00:00');
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(table_exists(&connection, "artist_ratings"));
        assert!(column_exists(&connection, "albums", "added_at").expect("column exists"));
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT added_at FROM albums WHERE id = 'album-1'"
            ),
            "2026-05-20 10:00:00"
        );
    }

    #[test]
    fn tracked_migration_adds_typed_library_columns_and_backfills_library_folders() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE libraries (
                  id TEXT PRIMARY KEY,
                  path TEXT NOT NULL UNIQUE,
                  name TEXT NOT NULL,
                  added_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO libraries(id, path, name, added_at) VALUES
                  ('lib-1', 'D:\Music', 'Music', '2026-05-20 10:00:00'),
                  ('lib-2', 'D:\Movies', 'Movies', '2026-05-20 11:00:00');
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(column_exists(&connection, "libraries", "library_type").expect("library_type"));
        assert!(
            column_exists(&connection, "libraries", "scanner_profile").expect("scanner_profile")
        );
        assert!(column_exists(&connection, "libraries", "metadata_mode").expect("metadata_mode"));
        assert!(table_exists(&connection, "library_folders"));
        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM library_folders"),
            2
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT library_type FROM libraries WHERE id = 'lib-1'"
            ),
            "music"
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT scanner_profile FROM libraries WHERE id = 'lib-1'"
            ),
            "default"
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT metadata_mode FROM libraries WHERE id = 'lib-1'"
            ),
            "path_only"
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT path FROM library_folders WHERE library_id = 'lib-2'"
            ),
            "D:\\Movies"
        );
    }

    #[test]
    fn tracked_migration_repairs_playback_columns_and_adds_music_rating_tables() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE settings (
                  key TEXT PRIMARY KEY,
                  value TEXT NOT NULL
                );
                CREATE TABLE users (
                  id TEXT PRIMARY KEY,
                  pin_hash TEXT NOT NULL,
                  name TEXT NOT NULL
                );
                CREATE TABLE artists (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  track_count INTEGER NOT NULL DEFAULT 0,
                  album_count INTEGER NOT NULL DEFAULT 0,
                  play_count INTEGER
                );
                CREATE TABLE albums (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  album_artist TEXT NOT NULL DEFAULT '',
                  track_count INTEGER NOT NULL DEFAULT 0,
                  total_duration_sec REAL NOT NULL DEFAULT 0
                );
                CREATE TABLE tracks (
                  id TEXT PRIMARY KEY,
                  library_id TEXT,
                  artist_id TEXT,
                  album_id TEXT,
                  title TEXT NOT NULL,
                  duration REAL NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0,
                  play_count INTEGER
                );
                INSERT INTO artists(id, name, play_count) VALUES ('artist-1', 'Artist', NULL);
                INSERT INTO albums(id, title, album_artist) VALUES ('album-1', 'Album', 'Artist');
                INSERT INTO tracks(id, artist_id, album_id, title, play_count) VALUES
                  ('track-1', 'artist-1', 'album-1', 'Song', NULL);
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(column_exists(&connection, "tracks", "last_played_at").expect("last_played_at"));
        assert!(table_exists(&connection, "album_ratings"));
        assert!(table_exists(&connection, "track_ratings"));
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT play_count FROM tracks WHERE id = 'track-1'"
            ),
            0
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT play_count FROM artists WHERE id = 'artist-1'"
            ),
            0
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_artists_play_count'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_tracks_last_played'"
            ),
            1
        );
    }

    #[test]
    fn tracked_migration_adds_music_feature_tables_and_lyrics_synced_column() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE users (
                  id TEXT PRIMARY KEY,
                  pin_hash TEXT NOT NULL,
                  name TEXT NOT NULL
                );
                CREATE TABLE tracks (
                  id TEXT PRIMARY KEY,
                  library_id TEXT,
                  title TEXT NOT NULL,
                  duration REAL NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE lyrics_cache (
                  track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                  artist TEXT NOT NULL,
                  title TEXT NOT NULL,
                  lyrics TEXT NOT NULL,
                  source TEXT NOT NULL DEFAULT 'lrclib',
                  fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
                  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO users(id, pin_hash, name) VALUES ('user-1', 'hash', 'User');
                INSERT INTO tracks(id, title) VALUES ('track-1', 'Song');
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(table_exists(&connection, "crossfade_overrides"));
        assert!(table_exists(&connection, "track_waveforms"));
        assert!(table_exists(&connection, "play_history"));
        assert!(column_exists(&connection, "lyrics_cache", "synced_lyrics").expect("synced_lyrics"));
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_play_history_user'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_lyrics_cache_artist_title'"
            ),
            1
        );
    }

    #[test]
    fn tracked_migration_removes_beta_video_support_schema() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE settings (
                  key TEXT PRIMARY KEY,
                  value TEXT NOT NULL
                );
                CREATE TABLE users (
                  id TEXT PRIMARY KEY,
                  pin_hash TEXT NOT NULL,
                  name TEXT NOT NULL
                );
                CREATE TABLE libraries (
                  id TEXT PRIMARY KEY,
                  path TEXT NOT NULL UNIQUE,
                  name TEXT NOT NULL,
                  library_type TEXT NOT NULL DEFAULT 'music' CHECK(library_type IN ('music','movies','tv')),
                  scanner_profile TEXT NOT NULL DEFAULT 'default',
                  metadata_mode TEXT NOT NULL DEFAULT 'path_only',
                  added_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE scan_jobs (
                  id TEXT PRIMARY KEY,
                  library_id TEXT,
                  status TEXT NOT NULL
                );
                CREATE TABLE scan_schedules (
                  id TEXT PRIMARY KEY,
                  library_id TEXT NOT NULL UNIQUE,
                  enabled INTEGER NOT NULL DEFAULT 1,
                  frequency_hours REAL NOT NULL DEFAULT 24
                );
                CREATE TABLE post_scan_jobs (
                  id TEXT PRIMARY KEY,
                  library_id TEXT,
                  job_type TEXT NOT NULL,
                  status TEXT NOT NULL
                );
                CREATE TABLE movies(id TEXT PRIMARY KEY);
                CREATE TABLE shows(id TEXT PRIMARY KEY);
                CREATE TABLE seasons(id TEXT PRIMARY KEY);
                CREATE TABLE episodes(id TEXT PRIMARY KEY);
                CREATE TABLE media_files(id TEXT PRIMARY KEY);
                CREATE TABLE subtitle_tracks(id TEXT PRIMARY KEY);
                CREATE TABLE audio_streams(id TEXT PRIMARY KEY);
                CREATE TABLE video_playback_progress(user_id TEXT, media_file_id TEXT);
                CREATE TABLE metadata_matches(id TEXT PRIMARY KEY);
                CREATE TABLE stats_cache (
                  id TEXT PRIMARY KEY,
                  total_tracks INTEGER NOT NULL DEFAULT 0,
                  total_artists INTEGER NOT NULL DEFAULT 0,
                  total_albums INTEGER NOT NULL DEFAULT 0,
                  total_movies INTEGER NOT NULL DEFAULT 0,
                  total_tv_shows INTEGER NOT NULL DEFAULT 0,
                  total_libraries INTEGER NOT NULL DEFAULT 0,
                  total_hours REAL NOT NULL DEFAULT 0,
                  total_gb REAL NOT NULL DEFAULT 0,
                  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE video_people (
                  id TEXT PRIMARY KEY,
                  owner_type TEXT NOT NULL,
                  owner_id TEXT NOT NULL,
                  person_name TEXT NOT NULL,
                  role_type TEXT NOT NULL,
                  created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE video_reviews (
                  id TEXT PRIMARY KEY,
                  owner_type TEXT NOT NULL,
                  owner_id TEXT NOT NULL,
                  provider TEXT NOT NULL,
                  author_name TEXT NOT NULL,
                  content TEXT NOT NULL,
                  created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO users(id, pin_hash, name) VALUES ('user-1', 'hash', 'User');
                INSERT INTO libraries(id, path, name, library_type) VALUES
                  ('lib-music', 'D:\Music', 'Music', 'music'),
                  ('lib-video', 'D:\Video', 'Video', 'movies');
                INSERT INTO scan_jobs(id, library_id, status) VALUES('scan-video', 'lib-video', 'pending');
                INSERT INTO scan_schedules(id, library_id) VALUES('schedule-video', 'lib-video');
                INSERT INTO post_scan_jobs(id, library_id, job_type, status) VALUES
                  ('video-job', 'lib-video', 'sync_video_metadata', 'pending'),
                  ('music-job', 'lib-music', 'cache_album_images', 'pending');
                INSERT INTO settings(key, value) VALUES
                  ('tmdbApiKey', 'secret'),
                  ('dlnaMediaTypes', 'both'),
                  ('lastfmKey', 'keep');
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(!table_exists(&connection, "movies"));
        assert!(!table_exists(&connection, "shows"));
        assert!(!table_exists(&connection, "media_files"));
        assert!(!table_exists(&connection, "video_people"));
        assert!(!table_exists(&connection, "video_reviews"));
        assert!(!table_exists(&connection, "video_artwork_assets"));
        assert!(!column_exists(&connection, "stats_cache", "total_movies").expect("total_movies"));
        assert!(
            !column_exists(&connection, "stats_cache", "total_tv_shows").expect("total_tv_shows")
        );
        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM libraries"),
            1
        );
        assert_eq!(
            query_single_text(&connection, "SELECT id FROM libraries"),
            "lib-music"
        );
        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM post_scan_jobs"),
            1
        );
        assert_eq!(
            query_single_text(&connection, "SELECT key FROM settings"),
            "lastfmKey"
        );
    }

    #[test]
    fn refresh_helpers_update_denormalized_counts_and_stats_cache() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE artists (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL,
                  track_count INTEGER NOT NULL DEFAULT 0,
                  album_count INTEGER NOT NULL DEFAULT 0,
                  play_count INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE albums (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  album_artist TEXT NOT NULL DEFAULT '',
                  track_count INTEGER NOT NULL DEFAULT 0,
                  total_duration_sec REAL NOT NULL DEFAULT 0,
                  added_at TEXT
                );
                CREATE TABLE tracks (
                  id TEXT PRIMARY KEY,
                  artist_id TEXT,
                  album_id TEXT,
                  duration REAL NOT NULL DEFAULT 0,
                  file_size INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE libraries (
                  id TEXT PRIMARY KEY,
                  path TEXT NOT NULL UNIQUE,
                  name TEXT NOT NULL
                );
                CREATE TABLE stats_cache (
                  id TEXT PRIMARY KEY,
                  total_tracks INTEGER NOT NULL DEFAULT 0,
                  total_artists INTEGER NOT NULL DEFAULT 0,
                  total_albums INTEGER NOT NULL DEFAULT 0,
                  total_libraries INTEGER NOT NULL DEFAULT 0,
                  total_hours REAL NOT NULL DEFAULT 0,
                  total_gb REAL NOT NULL DEFAULT 0,
                  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO artists(id, name) VALUES ('artist-1', 'Artist');
                INSERT INTO albums(id, title, album_artist) VALUES ('album-1', 'Album', 'Artist');
                INSERT INTO tracks(id, artist_id, album_id, duration, file_size) VALUES
                  ('track-1', 'artist-1', 'album-1', 180, 1073741824),
                  ('track-2', 'artist-1', 'album-1', 120, 536870912);
                INSERT INTO libraries(id, path, name) VALUES ('lib-1', 'D:\Music', 'Music');
                "#,
            )
            .expect("seed");

        refresh_denormalized_counts(&connection).expect("refresh counts");
        refresh_stats_cache(&connection).expect("refresh stats");

        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT track_count FROM artists WHERE id = 'artist-1'"
            ),
            2
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT album_count FROM artists WHERE id = 'artist-1'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT track_count FROM albums WHERE id = 'album-1'"
            ),
            2
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT total_tracks FROM stats_cache WHERE id = 'stats-cache'"
            ),
            2
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT total_libraries FROM stats_cache WHERE id = 'stats-cache'"
            ),
            1
        );
    }

    #[test]
    fn refresh_denormalized_counts_derives_album_year_and_genre_from_tracks() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE albums (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  album_artist TEXT NOT NULL DEFAULT '',
                  year INTEGER,
                  genre TEXT,
                  metadata_locked INTEGER NOT NULL DEFAULT 0,
                  track_count INTEGER NOT NULL DEFAULT 0,
                  total_duration_sec REAL NOT NULL DEFAULT 0
                );
                CREATE TABLE tracks (
                  id TEXT PRIMARY KEY,
                  album_id TEXT,
                  year INTEGER,
                  genre TEXT,
                  duration REAL NOT NULL DEFAULT 0
                );
                INSERT INTO albums(id, title, year, genre, metadata_locked) VALUES
                  ('album-1', 'Reissued', NULL, NULL, 0),
                  ('album-2', 'Hand Edited', 1999, 'Jazz', 1),
                  ('album-3', 'Untagged', NULL, NULL, 0),
                  ('album-4', 'Provider Filled', 1984, 'Synthpop', 0);
                INSERT INTO tracks(id, album_id, year, genre, duration) VALUES
                  ('t1', 'album-1', 2011, 'Rock', 10),
                  ('t2', 'album-1', 1976, 'Rock', 10),
                  ('t3', 'album-1', NULL, 'Pop', 10),
                  ('t4', 'album-2', 2020, 'Metal', 10),
                  ('t5', 'album-3', 0, '  ', 10),
                  ('t6', 'album-4', NULL, NULL, 10);
                "#,
            )
            .expect("seed");

        refresh_denormalized_counts(&connection).expect("refresh counts");

        // Earliest tagged year wins, so a remaster's later year does not hide the
        // original release year, and the dominant genre is carried up.
        assert_eq!(
            query_single_i64(&connection, "SELECT year FROM albums WHERE id = 'album-1'"),
            1976
        );
        let genre: String = connection
            .query_row("SELECT genre FROM albums WHERE id = 'album-1'", [], |row| {
                row.get(0)
            })
            .expect("genre");
        assert_eq!(genre, "Rock");

        // A hand-edited album is locked and must keep its curated values.
        assert_eq!(
            query_single_i64(&connection, "SELECT year FROM albums WHERE id = 'album-2'"),
            1999
        );

        // Placeholder years and blank genres are not treated as real metadata.
        let untagged: Option<i64> = connection
            .query_row("SELECT year FROM albums WHERE id = 'album-3'", [], |row| {
                row.get(0)
            })
            .expect("year");
        assert_eq!(untagged, None);

        // A year the provider lane resolved for an album whose tracks carry no
        // year tag must survive later refreshes rather than being nulled out.
        assert_eq!(
            query_single_i64(&connection, "SELECT year FROM albums WHERE id = 'album-4'"),
            1984
        );
        let kept_genre: String = connection
            .query_row("SELECT genre FROM albums WHERE id = 'album-4'", [], |row| {
                row.get(0)
            })
            .expect("genre");
        assert_eq!(kept_genre, "Synthpop");
    }

    #[test]
    fn tracked_migration_normalizes_duplicate_library_names_before_unique_index() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE libraries (
                  id TEXT PRIMARY KEY,
                  path TEXT NOT NULL UNIQUE,
                  name TEXT NOT NULL,
                  added_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO libraries(id, path, name, added_at) VALUES
                  ('lib-1', 'D:\Music', 'Main Library', '2026-05-23 10:00:00'),
                  ('lib-2', 'D:\Movies', 'main library', '2026-05-23 11:00:00'),
                  ('lib-3', 'D:\TV', 'Main   Library', '2026-05-23 12:00:00');
                "#,
            )
            .expect("seed");

        run_tracked_migrations(&connection).expect("migrate");

        let mut statement = connection
            .prepare("SELECT name FROM libraries ORDER BY id ASC")
            .expect("query names");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("map rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect names");

        assert_eq!(
            rows,
            vec![
                "Main Library".to_string(),
                "Main Library 2".to_string(),
                "Main Library 3".to_string()
            ]
        );
    }

    #[test]
    fn tracked_migration_adds_playlist_duplicate_name_triggers() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE playlists (
                  id TEXT PRIMARY KEY,
                  user_id TEXT,
                  name TEXT NOT NULL
                );
                "#,
            )
            .expect("seed schema");

        run_tracked_migrations(&connection).expect("migrate");
        connection
            .execute(
                "INSERT INTO playlists(id, user_id, name) VALUES(?1, ?2, ?3)",
                params!["playlist-1", "user-1", "Road Trip"],
            )
            .expect("insert first playlist");

        let duplicate = connection.execute(
            "INSERT INTO playlists(id, user_id, name) VALUES(?1, ?2, ?3)",
            params!["playlist-2", "user-1", "road trip"],
        );

        assert!(duplicate.is_err());
    }

    #[test]
    fn tracked_migration_adds_playlist_mutation_columns() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE playlists (
                  id TEXT PRIMARY KEY,
                  user_id TEXT,
                  name TEXT NOT NULL
                );
                CREATE TABLE playlist_tracks (
                  id TEXT,
                  playlist_id TEXT NOT NULL,
                  track_id TEXT NOT NULL,
                  position INTEGER NOT NULL DEFAULT 0,
                  added_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                "#,
            )
            .expect("seed schema");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(column_exists(&connection, "playlists", "description").expect("description"));
        assert!(column_exists(&connection, "playlists", "remember_progress")
            .expect("remember_progress"));
        assert!(
            column_exists(&connection, "playlist_tracks", "progress_seconds")
                .expect("progress_seconds")
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_playlist_tracks_unique_track'"
            ),
            1
        );
    }

    #[test]
    fn tracked_migration_adds_deep_analysis_feature_columns() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE track_deep_analysis (
                  track_id TEXT PRIMARY KEY,
                  analysis_version INTEGER NOT NULL DEFAULT 1,
                  file_fingerprint TEXT NOT NULL DEFAULT '',
                  demucs_model TEXT NOT NULL DEFAULT 'htdemucs',
                  used_gpu INTEGER NOT NULL DEFAULT 0,
                  stem_feature_json TEXT NOT NULL DEFAULT '{}',
                  vocal_windows_json TEXT NOT NULL DEFAULT '[]',
                  drum_windows_json TEXT NOT NULL DEFAULT '[]',
                  intro_outro_refined_json TEXT NOT NULL DEFAULT '{}',
                  energy_score_refined REAL NOT NULL DEFAULT 0.5,
                  transition_hints_json TEXT NOT NULL DEFAULT '{}',
                  processing_time_ms INTEGER NOT NULL DEFAULT 0,
                  created_at TEXT
                );
                CREATE TABLE deep_analysis_jobs (
                  id TEXT PRIMARY KEY,
                  track_id TEXT NOT NULL UNIQUE,
                  status TEXT NOT NULL DEFAULT 'pending',
                  priority INTEGER NOT NULL DEFAULT 50,
                  file_fingerprint TEXT NOT NULL DEFAULT '',
                  updated_at TEXT
                );
                INSERT INTO track_deep_analysis(
                  track_id, stem_feature_json, vocal_windows_json, drum_windows_json,
                  intro_outro_refined_json, transition_hints_json
                ) VALUES('track-1', '{"a":1}', '[]', '[]', '{}', '{}');
                "#,
            )
            .expect("seed schema");

        run_tracked_migrations(&connection).expect("migrate");

        for column in [
            "analysis_schema_version",
            "confidence",
            "feature_size_bytes",
            "source_duration_sec",
            "last_used_at",
            "section_json",
            "bass_windows_json",
            "phrase_boundaries_json",
            "transition_windows_json",
        ] {
            assert!(column_exists(&connection, "track_deep_analysis", column).unwrap());
        }
        assert!(
            query_single_i64(
                &connection,
                "SELECT feature_size_bytes FROM track_deep_analysis WHERE track_id='track-1'"
            ) > 0
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_deep_analysis_jobs_priority_status'"
            ),
            1
        );
    }

    #[test]
    fn scheduler_enqueue_due_scans_advances_schedule_and_reuses_active_job() {
        let root = temp_dir("scheduler-due");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        let library_id = "lib-schedule";
        connection
            .execute(
                "INSERT INTO libraries(id, path, name, library_type) VALUES(?1, ?2, ?3, 'music')",
                params![library_id, r"D:\Music", "Scheduled Music"],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO scan_schedules(id, library_id, enabled, frequency_hours, next_run)
                 VALUES(?1, ?2, 1, 0.5, datetime('now', '-1 minute'))",
                params!["sched-1", library_id],
            )
            .expect("schedule");

        let first = jobs::enqueue_due_scheduled_scans(&connection).expect("first enqueue");
        let second = jobs::enqueue_due_scheduled_scans(&connection).expect("second enqueue");

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM scan_jobs"),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM scan_schedules WHERE last_run IS NOT NULL AND datetime(next_run) > datetime('now')"
            ),
            1
        );
    }

    #[test]
    fn post_scan_claim_runs_only_runnable_music_mapping_jobs() {
        let root = temp_dir("post-scan-claim");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        let library_id = "lib-post-scan";
        connection
            .execute(
                "INSERT INTO libraries(id, path, name, library_type) VALUES(?1, ?2, ?3, 'music')",
                params![library_id, r"D:\Music", "Post Scan Music"],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO post_scan_jobs(id, library_id, job_type, status)
                 VALUES('post-refresh', ?1, 'refresh_library_mappings', 'pending')",
                params![library_id],
            )
            .expect("refresh job");
        connection
            .execute(
                "INSERT INTO post_scan_jobs(id, library_id, job_type, status)
                 VALUES('post-cache', ?1, 'cache_album_images', 'pending')",
                params![library_id],
            )
            .expect("cache job");
        // Non-runnable job (unknown type) should never be claimed.
        connection
            .execute(
                "INSERT INTO post_scan_jobs(id, library_id, job_type, status)
                 VALUES('post-unknown', ?1, 'not_a_real_job', 'pending')",
                params![library_id],
            )
            .expect("unknown job");

        // Both cache and refresh are now runnable; ORDER BY id picks 'post-cache' first.
        let claimed = jobs::claim_next_post_scan_job(&connection, jobs::PostScanLane::Music)
            .expect("claim")
            .expect("claimed job");
        let blocked = jobs::claim_next_post_scan_job(&connection, jobs::PostScanLane::Music)
            .expect("blocked claim");

        assert_eq!(claimed.job_id, music::coerce_entity_id("post-cache"));
        assert_eq!(claimed.job_type, "cache_album_images");
        assert!(claimed.payload.is_none());
        // Second claim is None because a job is already running in the lane.
        assert!(blocked.is_none());
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM post_scan_jobs WHERE id='post-cache' AND status='running' AND heartbeat_at IS NOT NULL"
            ),
            1
        );
        // The other runnable job stays pending.
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM post_scan_jobs WHERE id='post-refresh'"
            ),
            "pending"
        );
        // The unknown type is never claimed.
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM post_scan_jobs WHERE id='post-unknown'"
            ),
            "pending"
        );
    }

    #[test]
    fn artist_identity_jobs_are_default_and_startup_backfill_is_idempotent() {
        let connection = Connection::open_in_memory().expect("memory db");
        initialize_schema(&connection).expect("schema");
        connection
            .execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES('library-1', 'D:/Music', 'Music');
                 INSERT INTO artists(id, name) VALUES('artist-1', 'Artist');
                 INSERT INTO albums(id, title, album_artist, artist_id)
                   VALUES('album-1', 'Release', 'Artist', 'artist-1');
                 INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path)
                   VALUES('track-1', 'library-1', 'artist-1', 'album-1', 'Track', 'D:/Music/track.flac');",
            )
            .expect("fixtures");

        let first = jobs::enqueue_missing_artist_identity_backfill_jobs(&connection)
            .expect("first backfill");
        let second = jobs::enqueue_missing_artist_identity_backfill_jobs(&connection)
            .expect("second backfill");
        assert_eq!(first, 1);
        assert_eq!(second, 1);
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM post_scan_jobs WHERE job_type='enrich_artist_external_ids' AND status='pending'"
            ),
            1
        );

        connection
            .execute(
                "UPDATE artists SET lastfm_mbid='m', deezer_artist_id='d', spotify_artist_id='s', discogs_artist_id='c' WHERE id='artist-1'",
                [],
            )
            .unwrap();
        assert_eq!(
            jobs::enqueue_missing_artist_identity_backfill_jobs(&connection).unwrap(),
            0
        );

        connection
            .execute("DELETE FROM post_scan_jobs", [])
            .unwrap();
        jobs::enqueue_default_music_post_scan_jobs(
            &connection,
            &music::coerce_entity_id("library-1"),
        )
        .unwrap();
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM post_scan_jobs WHERE job_type='enrich_artist_external_ids'"
            ),
            1
        );
    }

    #[test]
    fn refresh_library_entity_mappings_rebuilds_optional_mapping_tables() {
        let root = temp_dir("post-scan-mappings");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        let library_id = music::coerce_entity_id("lib-mappings");
        connection
            .execute_batch(
                r#"
                CREATE TABLE artist_libraries (
                  artist_id TEXT NOT NULL,
                  library_id TEXT NOT NULL,
                  PRIMARY KEY (artist_id, library_id)
                );
                CREATE TABLE album_libraries (
                  album_id TEXT NOT NULL,
                  library_id TEXT NOT NULL,
                  PRIMARY KEY (album_id, library_id)
                );
                "#,
            )
            .expect("mapping tables");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name, library_type) VALUES(?1, ?2, ?3, 'music')",
                params![library_id, r"D:\Music", "Mapped Music"],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO artists(id, name) VALUES('artist-1', 'Artist')",
                [],
            )
            .expect("artist");
        connection
            .execute(
                "INSERT INTO albums(id, title, artist_id, album_artist) VALUES('album-1', 'Album', 'artist-1', 'Artist')",
                [],
            )
            .expect("album");
        connection
            .execute(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_name, album_artist, genre, composer, duration, file_path, file_size)
                 VALUES('track-1', ?1, 'artist-1', 'album-1', 'Track', 'track.mp3', 'Artist', '', '', 0, ?2, 123)",
                params![library_id, r"D:\Music\Artist\Album\track.mp3"],
            )
            .expect("track");

        jobs::refresh_library_entity_mappings(&connection, &library_id).expect("refresh mappings");

        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM artist_libraries"),
            1
        );
        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM album_libraries"),
            1
        );
    }

    #[test]
    fn tracked_migration_adds_music_metadata_edit_columns() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE artists (
                  id TEXT PRIMARY KEY,
                  name TEXT NOT NULL
                );
                CREATE TABLE albums (
                  id TEXT PRIMARY KEY,
                  title TEXT NOT NULL,
                  album_artist TEXT NOT NULL DEFAULT ''
                );
                "#,
            )
            .expect("seed schema");

        run_tracked_migrations(&connection).expect("migrate");

        assert!(column_exists(&connection, "artists", "description").expect("artist description"));
        assert!(column_exists(&connection, "artists", "metadata_locked").expect("artist locked"));
        assert!(column_exists(&connection, "albums", "year").expect("album year"));
        assert!(column_exists(&connection, "albums", "genre").expect("album genre"));
        assert!(column_exists(&connection, "albums", "description").expect("album description"));
        assert!(column_exists(&connection, "albums", "release_type").expect("album release type"));
        assert!(column_exists(&connection, "albums", "metadata_locked").expect("album locked"));
    }

    #[test]
    fn artist_external_identity_schema_is_fresh_upgrade_safe_and_idempotent() {
        let fresh = Connection::open_in_memory().expect("fresh db");
        initialize_schema(&fresh).expect("fresh schema");
        for column in [
            "lastfm_mbid",
            "lastfm_canonical_name",
            "lastfm_identity_checked_at",
            "deezer_artist_id",
            "deezer_identity_checked_at",
            "spotify_artist_id",
            "spotify_identity_checked_at",
            "discogs_artist_id",
            "discogs_identity_checked_at",
        ] {
            assert!(column_exists(&fresh, "artists", column).expect("identity column"));
        }
        for index in [
            "idx_artists_lastfm_mbid",
            "idx_artists_deezer_artist_id",
            "idx_artists_spotify_artist_id",
            "idx_artists_discogs_artist_id",
        ] {
            assert_eq!(
                query_single_i64(
                    &fresh,
                    &format!(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='{index}'"
                    ),
                ),
                1
            );
        }

        let upgrade = Connection::open_in_memory().expect("upgrade db");
        upgrade
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   id TEXT PRIMARY KEY,
                   applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 CREATE TABLE artists (id TEXT PRIMARY KEY, name TEXT NOT NULL);
                 INSERT INTO artists(id, name) VALUES('artist-1', 'Existing Artist');",
            )
            .expect("old schema");
        ensure_artist_external_identity_schema(&upgrade).expect("first migration");
        ensure_artist_external_identity_schema(&upgrade).expect("idempotent migration");
        assert_eq!(
            query_single_text(&upgrade, "SELECT name FROM artists WHERE id='artist-1'"),
            "Existing Artist"
        );
        assert!(column_exists(&upgrade, "artists", "lastfm_mbid").expect("upgraded column"));
        assert_eq!(
            query_single_i64(
                &upgrade,
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_artists_lastfm_mbid'"
            ),
            1
        );
    }

    #[test]
    fn reset_orphaned_scan_jobs_moves_running_to_pending() {
        let root = temp_dir("orphan-scan");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-orphan', 'D:\\Music', 'Music')",
                [],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO scan_jobs(id, library_id, status) VALUES('sj-running', 'lib-orphan', 'running')",
                [],
            )
            .expect("running scan job");
        connection
            .execute(
                "INSERT INTO scan_jobs(id, library_id, status) VALUES('sj-pending', 'lib-orphan', 'pending')",
                [],
            )
            .expect("pending scan job");

        let reset = jobs::reset_orphaned_scan_jobs(&connection).expect("reset");
        assert_eq!(reset, 1);
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM scan_jobs WHERE id='sj-running'"
            ),
            "pending"
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM scan_jobs WHERE id='sj-pending'"
            ),
            "pending"
        );
    }

    #[test]
    fn reset_orphaned_post_scan_jobs_marks_running_as_failed() {
        let root = temp_dir("orphan-post-scan");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-ops', 'D:\\Music', 'Music')",
                [],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO post_scan_jobs(id, library_id, job_type, status)
                 VALUES('psj-running', 'lib-ops', 'cache_artist_images', 'running')",
                [],
            )
            .expect("running post-scan job");
        connection
            .execute(
                "INSERT INTO post_scan_jobs(id, library_id, job_type, status)
                 VALUES('psj-pending', 'lib-ops', 'cache_album_images', 'pending')",
                [],
            )
            .expect("pending post-scan job");

        let reset = jobs::reset_orphaned_post_scan_jobs(&connection).expect("reset");
        assert_eq!(reset, 1);
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM post_scan_jobs WHERE id='psj-running'"
            ),
            "failed"
        );
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM post_scan_jobs WHERE id='psj-pending'"
            ),
            "pending"
        );
    }

    #[test]
    fn claim_scan_job_runs_the_requested_library_even_behind_an_older_pending_job() {
        let root = temp_dir("claim-scan-job");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-a', 'D:\\A', 'A')",
                [],
            )
            .expect("library a");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-b', 'D:\\B', 'B')",
                [],
            )
            .expect("library b");
        // Library B's job was queued first (e.g. a due schedule), so a naive
        // FIFO claim would pick it over whatever the user just clicked.
        connection
            .execute(
                "INSERT INTO scan_jobs(id, library_id, status, created_at) VALUES('sj-b', 'lib-b', 'pending', '2026-01-01 00:00:00')",
                [],
            )
            .expect("library b job");
        connection
            .execute(
                "INSERT INTO scan_jobs(id, library_id, status, created_at) VALUES('sj-a', 'lib-a', 'pending', '2026-01-02 00:00:00')",
                [],
            )
            .expect("library a job");

        let claimed = jobs::claim_scan_job(&connection, &music::coerce_entity_id("sj-a"))
            .expect("claim")
            .expect("job claimed");
        assert_eq!(claimed.job_id, music::coerce_entity_id("sj-a"));
        assert_eq!(claimed.library_id, music::coerce_entity_id("lib-a"));
        assert_eq!(
            query_single_text(&connection, "SELECT status FROM scan_jobs WHERE id='sj-a'"),
            "running"
        );
        assert_eq!(
            query_single_text(&connection, "SELECT status FROM scan_jobs WHERE id='sj-b'"),
            "pending"
        );
    }

    #[test]
    fn manual_scan_reuses_and_claims_the_library_scheduled_pending_job() {
        let root = temp_dir("manual-scheduled-scan");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-scheduled', 'D:\\Music', 'Scheduled')",
                [],
            )
            .expect("library");
        connection
            .execute(
                "INSERT INTO scan_jobs(id, library_id, status) VALUES('scheduled-job', 'lib-scheduled', 'pending')",
                [],
            )
            .expect("scheduled job");

        let job_id = jobs::enqueue_scan_job(&connection, "lib-scheduled").expect("manual enqueue");
        assert_eq!(job_id, music::coerce_entity_id("scheduled-job"));

        let claimed = jobs::claim_scan_job(&connection, &job_id)
            .expect("claim")
            .expect("job claimed");
        assert_eq!(claimed.library_id, music::coerce_entity_id("lib-scheduled"));
        assert_eq!(
            query_single_text(
                &connection,
                "SELECT status FROM scan_jobs WHERE id='scheduled-job'"
            ),
            "running"
        );
    }

    #[test]
    fn prune_orphaned_music_entities_removes_trackless_albums_and_artists() {
        let root = temp_dir("orphan-music");
        let InitializedDatabase { connection, .. } = init_db(&root).expect("db init");
        connection
            .execute(
                "INSERT INTO libraries(id, path, name) VALUES('lib-orphan-music', 'D:\\Music', 'Music')",
                [],
            )
            .expect("library");
        // Real artist with a real album and a track: must survive.
        connection
            .execute(
                "INSERT INTO artists(id, name) VALUES('artist-real', 'Solarstone')",
                [],
            )
            .expect("real artist");
        connection
            .execute(
                "INSERT INTO albums(id, title, artist_id, album_artist) VALUES('album-real', 'The Impressions Ep', 'artist-real', 'Solarstone')",
                [],
            )
            .expect("real album");
        connection
            .execute(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_name, album_artist, genre, composer, duration, file_path, file_size)
                 VALUES('track-real', 'lib-orphan-music', 'artist-real', 'album-real', 'Track', 'track.flac', 'Solarstone', '', '', 0, 'D:\\Music\\track.flac', 1)",
                [],
            )
            .expect("real track");
        // Stale folder-name artist/album left behind after a rescan reassigned the track: must be pruned.
        connection
            .execute(
                "INSERT INTO artists(id, name) VALUES('artist-stale', 'Singles and EPs')",
                [],
            )
            .expect("stale artist");
        connection
            .execute(
                "INSERT INTO albums(id, title, artist_id, album_artist) VALUES('album-stale', '(1998) Solarstone - The Impressions Ep', 'artist-stale', 'Singles and EPs')",
                [],
            )
            .expect("stale album");
        // Locked artist with no tracks/albums: must survive despite being orphaned.
        connection
            .execute(
                "INSERT INTO artists(id, name, metadata_locked) VALUES('artist-locked', 'Kept On Purpose', 1)",
                [],
            )
            .expect("locked artist");

        let (albums_pruned, artists_pruned) =
            jobs::prune_orphaned_music_entities(&connection).expect("prune");
        assert_eq!(albums_pruned, 1);
        assert_eq!(artists_pruned, 1);
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM albums WHERE id='album-real'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM artists WHERE id='artist-real'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM artists WHERE id='artist-locked'"
            ),
            1
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM albums WHERE id='album-stale'"
            ),
            0
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM artists WHERE id='artist-stale'"
            ),
            0
        );
    }

    #[test]
    fn browse_performance_indexes_exist_and_are_used_by_list_artists() {
        let connection = Connection::open_in_memory().expect("memory db");
        initialize_schema(&connection).expect("schema");

        for index in [
            "idx_albums_artist_id",
            "idx_playlist_tracks_track",
            "idx_track_ratings_track",
            "idx_play_history_track",
            "idx_crossfade_overrides_entity",
        ] {
            assert_eq!(
                query_single_i64(
                    &connection,
                    &format!(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
                    ),
                ),
                1,
                "missing index {index}"
            );
        }

        // The albums.artist_id correlated subquery that list_artists runs must
        // plan as an indexed SEARCH, not a full SCAN of albums per artist row.
        let mut stmt = connection
            .prepare(
                "EXPLAIN QUERY PLAN \
                 SELECT ar.id, \
                   (SELECT COUNT(*) FROM albums oal2 WHERE oal2.artist_id = ar.id) AS album_count \
                 FROM artists ar",
            )
            .expect("prepare plan");
        let details: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(3))
            .expect("plan rows")
            .collect::<Result<_, _>>()
            .expect("plan detail");

        let albums_step = details
            .iter()
            .find(|detail| detail.contains("albums"))
            .expect("plan mentions albums");
        assert!(
            albums_step.contains("SEARCH") && albums_step.contains("idx_albums_artist_id"),
            "expected indexed SEARCH on albums, got: {albums_step}"
        );
    }

    #[test]
    fn browse_performance_migration_dedupes_crossfade_overrides() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                  id TEXT PRIMARY KEY,
                  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE crossfade_overrides (
                  id TEXT PRIMARY KEY,
                  entity_type TEXT NOT NULL,
                  entity_id TEXT NOT NULL,
                  mode TEXT NOT NULL,
                  duration INTEGER NOT NULL DEFAULT 2
                );
                INSERT INTO crossfade_overrides(id, entity_type, entity_id, mode, duration)
                VALUES ('a', 'album', 'x', 'crossfade', 2),
                       ('b', 'album', 'x', 'off', 3),
                       ('c', 'album', 'y', 'off', 4);
                "#,
            )
            .expect("legacy fixture");

        ensure_browse_performance_indexes(&connection).expect("migrate");

        assert_eq!(
            query_single_i64(&connection, "SELECT COUNT(*) FROM crossfade_overrides"),
            2
        );
        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_crossfade_overrides_entity'"
            ),
            1
        );
    }

    #[test]
    fn browse_performance_migration_skips_missing_tables_and_columns() {
        let connection = Connection::open_in_memory().expect("memory db");
        connection
            .execute_batch("CREATE TABLE albums (id TEXT PRIMARY KEY, title TEXT NOT NULL);")
            .expect("legacy fixture");

        // albums predates artist_id here, and none of the other tables exist.
        ensure_browse_performance_indexes(&connection).expect("migrate");

        assert_eq!(
            query_single_i64(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_albums_artist_id'"
            ),
            0
        );
    }

    fn query_single_text(connection: &Connection, sql: &str) -> String {
        connection
            .query_row(sql, [], |row| row.get::<_, String>(0))
            .expect("query row")
    }

    fn query_single_i64(connection: &Connection, sql: &str) -> i64 {
        connection
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .expect("query row")
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("boogiebox-rs-db-{prefix}-{nanos}"));
        path
    }
}

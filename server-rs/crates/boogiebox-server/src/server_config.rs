//! Defines Rust server support logic for Server Config.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env, fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

const DB_LOCATOR_FILE: &str = "boogiebox-config.json";

/// Public DB Config data shape used by BoogieBox.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbConfig {
    /// Documents the DB Folder public API surface.
    pub db_folder: String,
}

/// Public DB Config Write Error data shape used by BoogieBox.
#[derive(Debug, Error)]
pub enum DbConfigWriteError {
    #[error("dbFolder is required")]
    EmptyDbFolder,
    #[error("failed to create directory {path}: {source}")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("failed to write locator {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("failed to serialize locator: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LocatorEnv {
    explicit_path: Option<String>,
    explicit_dir: Option<String>,
    program_data: Option<String>,
}

impl LocatorEnv {
    fn from_process() -> Self {
        Self {
            explicit_path: env::var("BOOGIEBOX_CONFIG_PATH").ok(),
            explicit_dir: env::var("BOOGIEBOX_CONFIG_DIR").ok(),
            program_data: env::var("PROGRAMDATA").ok(),
        }
    }
}

/// Documents the Get DB Locator Path Candidates public API surface.
pub fn get_db_locator_path_candidates() -> Vec<PathBuf> {
    let exec_path = env::current_exe().unwrap_or_else(|_| PathBuf::from("boogiebox-server.exe"));
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    db_locator_path_candidates_for(&exec_path, &cwd, app_root(), LocatorEnv::from_process())
}

/// Documents the Get Writable DB Locator Path public API surface.
pub fn get_writable_db_locator_path() -> PathBuf {
    let exec_path = env::current_exe().unwrap_or_else(|_| PathBuf::from("boogiebox-server.exe"));
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let paths =
        db_locator_path_candidates_for(&exec_path, &cwd, app_root(), LocatorEnv::from_process());
    writable_db_locator_path_for(&exec_path, app_root(), LocatorEnv::from_process(), &paths)
}

/// Documents the Read DB Config public API surface.
pub fn read_db_config() -> Option<DbConfig> {
    read_db_config_from_candidates(&get_db_locator_path_candidates())
}

/// Documents the Write DB Config public API surface.
pub fn write_db_config(db_folder: &Path) -> Result<PathBuf, DbConfigWriteError> {
    let locator_path = get_writable_db_locator_path();
    write_db_config_to(&locator_path, db_folder)
}

/// Documents the Write DB Config To public API surface.
pub fn write_db_config_to(
    locator_path: &Path,
    db_folder: &Path,
) -> Result<PathBuf, DbConfigWriteError> {
    write_db_config_at(locator_path, db_folder)
}

fn read_db_config_from_candidates(candidates: &[PathBuf]) -> Option<DbConfig> {
    let locator_path = candidates.iter().find(|candidate| candidate.exists())?;
    let raw = fs::read_to_string(locator_path).ok()?;
    let parsed: DbConfig = serde_json::from_str(&raw).ok()?;
    let db_folder = parsed.db_folder.trim();
    if db_folder.is_empty() {
        return None;
    }
    Some(DbConfig {
        db_folder: db_folder.to_string(),
    })
}

fn write_db_config_at(
    locator_path: &Path,
    db_folder: &Path,
) -> Result<PathBuf, DbConfigWriteError> {
    let db_folder_text = db_folder.to_string_lossy().trim().to_string();
    if db_folder_text.is_empty() {
        return Err(DbConfigWriteError::EmptyDbFolder);
    }

    create_dir_all(db_folder)?;
    if let Some(parent) = locator_path.parent() {
        create_dir_all(parent)?;
    }

    let payload = serde_json::to_string_pretty(&DbConfig {
        db_folder: db_folder_text,
    })?;
    fs::write(locator_path, payload).map_err(|source| DbConfigWriteError::Write {
        path: locator_path.to_path_buf(),
        source,
    })?;
    Ok(locator_path.to_path_buf())
}

fn create_dir_all(path: &Path) -> Result<(), DbConfigWriteError> {
    fs::create_dir_all(path).map_err(|source| DbConfigWriteError::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}

fn db_locator_path_candidates_for(
    exec_path: &Path,
    cwd: &Path,
    app_root: PathBuf,
    env: LocatorEnv,
) -> Vec<PathBuf> {
    let configured = configured_locator_path(&env);
    let exe_dir = exec_path.parent().unwrap_or_else(|| Path::new("."));

    if is_packaged_server_executable(exec_path) {
        return unique_paths(vec![
            configured,
            Some(packaged_config_path(exec_path, &env)),
            Some(exe_dir.join(DB_LOCATOR_FILE)),
            Some(cwd.join(DB_LOCATOR_FILE)),
        ]);
    }

    unique_paths(vec![
        configured,
        Some(cwd.join(DB_LOCATOR_FILE)),
        Some(app_root.join(DB_LOCATOR_FILE)),
        Some(exe_dir.join(DB_LOCATOR_FILE)),
    ])
}

fn writable_db_locator_path_for(
    exec_path: &Path,
    app_root: PathBuf,
    env: LocatorEnv,
    candidates: &[PathBuf],
) -> PathBuf {
    if let Some(configured) = configured_locator_path(&env) {
        return configured;
    }

    if let Some(existing) = candidates.iter().find(|candidate| candidate.exists()) {
        return existing.clone();
    }

    if is_packaged_server_executable(exec_path) {
        return packaged_config_path(exec_path, &env);
    }

    app_root.join(DB_LOCATOR_FILE)
}

fn configured_locator_path(env: &LocatorEnv) -> Option<PathBuf> {
    if let Some(explicit_path) = env
        .explicit_path
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(PathBuf::from(explicit_path));
    }

    env.explicit_dir
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|dir| PathBuf::from(dir).join(DB_LOCATOR_FILE))
}

fn is_packaged_server_executable(exec_path: &Path) -> bool {
    exec_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("boogiebox-server.exe"))
}

fn packaged_config_path(exec_path: &Path, env: &LocatorEnv) -> PathBuf {
    let base_dir = env
        .program_data
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            exec_path
                .components()
                .next()
                .map(|component| PathBuf::from(component.as_os_str()))
                .unwrap_or_else(|| PathBuf::from("C:\\"))
                .join("ProgramData")
        });

    base_dir.join("BoogieBox").join(DB_LOCATOR_FILE)
}

fn app_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn unique_paths(paths: Vec<Option<PathBuf>>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in paths.into_iter().flatten() {
        let key = path.to_string_lossy().to_lowercase();
        if seen.insert(key) {
            result.push(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn explicit_config_path_is_first_candidate() {
        let path = PathBuf::from("D:\\config\\custom.json");
        let candidates = db_locator_path_candidates_for(
            Path::new("C:\\Program Files\\BoogieBox\\boogiebox-server.exe"),
            Path::new("C:\\work"),
            PathBuf::from("C:\\repo"),
            LocatorEnv {
                explicit_path: Some(path.to_string_lossy().into_owned()),
                explicit_dir: Some("D:\\ignored".to_string()),
                program_data: Some("C:\\ProgramData".to_string()),
            },
        );

        assert_eq!(candidates.first(), Some(&path));
    }

    #[test]
    fn packaged_server_prefers_program_data_before_install_adjacent_config() {
        let candidates = db_locator_path_candidates_for(
            Path::new("C:\\Program Files\\BoogieBox\\boogiebox-server.exe"),
            Path::new("C:\\Program Files\\BoogieBox"),
            PathBuf::from("C:\\repo"),
            LocatorEnv {
                program_data: Some("D:\\ProgramData".to_string()),
                ..LocatorEnv::default()
            },
        );

        assert_eq!(
            candidates[0],
            PathBuf::from("D:\\ProgramData\\BoogieBox\\boogiebox-config.json")
        );
        assert_eq!(
            candidates[1],
            PathBuf::from("C:\\Program Files\\BoogieBox\\boogiebox-config.json")
        );
    }

    #[test]
    fn source_server_prefers_cwd_before_repo_root_and_exe_dir() {
        let candidates = db_locator_path_candidates_for(
            Path::new("C:\\node\\node.exe"),
            Path::new("C:\\repo"),
            PathBuf::from("D:\\repo-root"),
            LocatorEnv::default(),
        );

        assert_eq!(
            candidates[0],
            PathBuf::from("C:\\repo\\boogiebox-config.json")
        );
        assert_eq!(
            candidates[1],
            PathBuf::from("D:\\repo-root\\boogiebox-config.json")
        );
    }

    #[test]
    fn read_db_config_ignores_missing_invalid_and_empty_locators() {
        let missing = temp_dir("missing").join("boogiebox-config.json");
        assert_eq!(read_db_config_from_candidates(&[missing]), None);

        let invalid = temp_dir("invalid").join("boogiebox-config.json");
        fs::create_dir_all(invalid.parent().unwrap()).expect("invalid parent");
        fs::write(&invalid, "{").expect("invalid locator");
        assert_eq!(read_db_config_from_candidates(&[invalid]), None);

        let empty = temp_dir("empty").join("boogiebox-config.json");
        fs::create_dir_all(empty.parent().unwrap()).expect("empty parent");
        fs::write(&empty, r#"{ "dbFolder": "   " }"#).expect("empty locator");
        assert_eq!(read_db_config_from_candidates(&[empty]), None);
    }

    #[test]
    fn write_db_config_creates_locator_and_database_folder() {
        let root = temp_dir("write");
        let locator = root.join("config").join("boogiebox-config.json");
        let db_folder = root.join("db");

        let written = write_db_config_at(&locator, &db_folder).expect("write config");
        let parsed =
            read_db_config_from_candidates(std::slice::from_ref(&locator)).expect("read config");

        assert_eq!(written, locator);
        assert!(db_folder.is_dir());
        assert_eq!(PathBuf::from(parsed.db_folder), db_folder);
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("boogiebox-rs-config-{prefix}-{nanos}"));
        path
    }
}

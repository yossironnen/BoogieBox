//! Defines Tauri desktop shell logic for Server Process.

use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Mutex,
};
use tauri::AppHandle;

use crate::config;

/// Public Server Process Store data shape used by BoogieBox.
#[derive(Default)]
pub struct ServerProcessStore {
    child: Mutex<Option<Child>>,
}

/// Public Packaged Server Status data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagedServerStatus {
    /// Documents the Running public API surface.
    pub running: bool,
    /// Documents the Pid public API surface.
    pub pid: Option<u32>,
    /// Documents the Path public API surface.
    pub path: Option<String>,
    /// Documents the Error public API surface.
    pub error: Option<String>,
}

/// Documents the Server Exe Candidates public API surface.
pub fn server_exe_candidates(app: &AppHandle, explicit_path: Option<String>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = explicit_path.filter(|p| !p.trim().is_empty()) {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(path) = env::var("BOOGIEBOX_SERVER_EXE") {
        if !path.trim().is_empty() {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Some(path) = config::load(app)
        .server_exe_path
        .filter(|p| !p.trim().is_empty())
    {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("boogiebox-server.exe"));
            candidates.push(dir.join("resources").join("boogiebox-server.exe"));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("boogiebox-server.exe"));
    }
    dedupe(candidates)
}

/// Documents the Resolve Server Exe public API surface.
pub fn resolve_server_exe(app: &AppHandle, explicit_path: Option<String>) -> Option<PathBuf> {
    server_exe_candidates(app, explicit_path)
        .into_iter()
        .find(|path| path.exists())
}

/// Documents the Status public API surface.
pub fn status(store: &ServerProcessStore) -> PackagedServerStatus {
    let mut guard = store.child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(Some(exit)) => {
                *guard = None;
                PackagedServerStatus {
                    running: false,
                    pid: None,
                    path: None,
                    error: Some(format!("server exited with {exit}")),
                }
            }
            Ok(None) => PackagedServerStatus {
                running: true,
                pid: Some(child.id()),
                path: None,
                error: None,
            },
            Err(error) => PackagedServerStatus {
                running: false,
                pid: Some(child.id()),
                path: None,
                error: Some(error.to_string()),
            },
        }
    } else {
        PackagedServerStatus {
            running: false,
            pid: None,
            path: None,
            error: None,
        }
    }
}

/// Documents the Start public API surface.
pub fn start(
    app: &AppHandle,
    store: &ServerProcessStore,
    explicit_path: Option<String>,
) -> PackagedServerStatus {
    {
        let current = status(store);
        if current.running {
            return current;
        }
    }

    let Some(exe_path) = resolve_server_exe(app, explicit_path) else {
        return PackagedServerStatus {
            running: false,
            pid: None,
            path: None,
            error: Some("boogiebox-server.exe not found".to_string()),
        };
    };

    let mut command = Command::new(&exe_path);
    if let Some(parent) = exe_path.parent() {
        command.current_dir(parent);
    }
    match command.spawn() {
        Ok(child) => {
            let pid = child.id();
            *store.child.lock().unwrap() = Some(child);
            PackagedServerStatus {
                running: true,
                pid: Some(pid),
                path: Some(exe_path.to_string_lossy().into_owned()),
                error: None,
            }
        }
        Err(error) => PackagedServerStatus {
            running: false,
            pid: None,
            path: Some(exe_path.to_string_lossy().into_owned()),
            error: Some(error.to_string()),
        },
    }
}

/// Documents the Stop public API surface.
pub fn stop(store: &ServerProcessStore) -> PackagedServerStatus {
    let mut guard = store.child.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let pid = child.id();
        let result = child.kill().and_then(|_| child.wait().map(|_| ()));
        PackagedServerStatus {
            running: false,
            pid: Some(pid),
            path: None,
            error: result.err().map(|error| error.to_string()),
        }
    } else {
        PackagedServerStatus {
            running: false,
            pid: None,
            path: None,
            error: None,
        }
    }
}

fn dedupe(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if !out
            .iter()
            .any(|existing: &PathBuf| same_path(existing, &path))
        {
            out.push(path);
        }
    }
    out
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::dedupe;
    use std::path::PathBuf;

    #[test]
    fn dedupes_case_insensitive_windows_style_paths() {
        let paths = dedupe(vec![
            PathBuf::from("C:/BoogieBox/boogiebox-server.exe"),
            PathBuf::from("c:/boogiebox/BOOGIEBOX-SERVER.EXE"),
            PathBuf::from("D:/BoogieBox/boogiebox-server.exe"),
        ]);
        assert_eq!(paths.len(), 2);
    }
}

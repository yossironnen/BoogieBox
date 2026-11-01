//! The server log must receive INFO+ events, and the debug toggles must route debug detail to their own files
//! without switching the server log off. Its own test binary, because it installs the global `tracing`
//! subscriber. (From 2026-09 to 0.9.59 the composed layers made the global level OFF and every event was lost.)

use boogiebox_server::logging;
use std::fs;
use std::path::Path;
use tracing::level_filters::LevelFilter;

fn read_prefixed(dir: &Path, prefix: &str) -> String {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| fs::read_to_string(e.path()).unwrap_or_default())
        .collect()
}

#[test]
fn server_log_gets_events_and_debug_toggles_route_only_their_modules() {
    let dir = std::env::temp_dir().join(format!("bb-log-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let paths = logging::resolve_log_paths(&dir.join("boogiebox-server.log")).unwrap();
    logging::init(&paths, false, false);
    assert_eq!(LevelFilter::current(), LevelFilter::INFO);

    tracing::info!("startup info marker");
    // Settings are applied again once the database opens.
    logging::sync_debug_toggles(false, false);
    tracing::warn!("after sync warn marker");
    tracing::debug!(target: "boogiebox_server::scanner", "hidden scan detail");

    logging::set_scan_debug_enabled(true);
    assert_eq!(LevelFilter::current(), LevelFilter::DEBUG);
    tracing::debug!(target: "boogiebox_server::scanner", "scan debug marker");
    tracing::debug!(target: "boogiebox_server::dlna", "unrelated debug detail");
    logging::set_scan_debug_enabled(false);
    assert_eq!(LevelFilter::current(), LevelFilter::INFO);
    tracing::info!("after toggle info marker");

    let server = fs::read_to_string(&paths.server_log).unwrap();
    let scan = read_prefixed(&dir, "boogiebox-scan-debug");
    let _ = fs::remove_dir_all(&dir);
    for marker in [
        "startup info marker",
        "after sync warn marker",
        "after toggle info marker",
    ] {
        assert!(server.contains(marker), "missing {marker:?} in:\n{server}");
    }
    assert!(
        !server.contains("scan debug marker"),
        "debug detail stays out of server.log"
    );
    assert!(
        scan.contains("scan debug marker"),
        "scan debug log:\n{scan}"
    );
    assert!(
        !scan.contains("hidden scan detail"),
        "nothing while the toggle is off"
    );
    assert!(!scan.contains("unrelated debug detail"));
}

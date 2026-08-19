//! Three-way log routing: a terse `server.log` (key events only, always on),
//! plus two opt-in debug logs (`scan-debug.log`, `deep-analysis-debug.log`)
//! that are toggled live via `scanDebugLoggingEnabled` / `deepmixDebugLoggingEnabled`
//! without a server restart.
//!
//! Routing is by module path, not by manually tagging every call site: the
//! `boogiebox_server::scanner` / `::post_scan` / `::providers` modules feed the
//! scan-debug layer, `::deep_analysis` / `::mix_worker` feed the deep-analysis
//! layer, and everything at INFO-or-above feeds the server log + stdout.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Registry;

/// Server log gets truncated (rotated to `.1`) once it exceeds this size.
const SERVER_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
/// Debug logs rotate daily via `tracing_appender` and keep this many files
/// (≈14 days of history), which also bounds their total size on disk.
const DEBUG_LOG_MAX_FILES: usize = 14;

type ScanFilteredLayer = tracing_subscriber::filter::Filtered<
    Box<dyn Layer<Registry> + Send + Sync + 'static>,
    Targets,
    Registry,
>;
type DeepFilteredLayer = tracing_subscriber::filter::Filtered<
    Box<dyn Layer<Registry> + Send + Sync + 'static>,
    Targets,
    Registry,
>;

static SCAN_RELOAD: OnceLock<reload::Handle<ScanFilteredLayer, Registry>> = OnceLock::new();
static DEEP_RELOAD: OnceLock<reload::Handle<DeepFilteredLayer, Registry>> = OnceLock::new();

/// Module-path prefixes routed to `scan-debug.log`.
const SCAN_MODULES: &[&str] = &[
    "boogiebox_server::scanner",
    "boogiebox_server::post_scan",
    "boogiebox_server::providers",
];
/// Module-path prefixes routed to `deep-analysis-debug.log`.
const DEEP_MODULES: &[&str] = &[
    "boogiebox_server::deep_analysis",
    "boogiebox_server::mix_worker",
];

fn scan_targets(enabled: bool) -> Targets {
    let mut targets = Targets::new().with_default(LevelFilter::OFF);
    if enabled {
        for module in SCAN_MODULES {
            targets = targets.with_target(*module, LevelFilter::DEBUG);
        }
    }
    targets
}

fn deep_targets(enabled: bool) -> Targets {
    let mut targets = Targets::new().with_default(LevelFilter::OFF);
    if enabled {
        for module in DEEP_MODULES {
            targets = targets.with_target(*module, LevelFilter::DEBUG);
        }
    }
    targets
}

// -- Writers ------------------------------------------------------------------

/// Mirrors everything written to it to stdout and appends to a size-capped
/// file. When the file would exceed `max_bytes`, it is rotated to `<name>.1`
/// (overwriting any previous `.1`) before the next write lands in a fresh file.
#[derive(Clone)]
struct CappedFileWriter {
    path: PathBuf,
    max_bytes: u64,
}

struct CappedFile {
    file: Option<fs::File>,
}

impl Write for CappedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stdout().write_all(buf);
        if let Some(file) = self.file.as_mut() {
            let _ = file.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stdout().flush();
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CappedFileWriter {
    type Writer = CappedFile;

    fn make_writer(&'a self) -> Self::Writer {
        rotate_if_oversized(&self.path, self.max_bytes);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        CappedFile { file }
    }
}

fn rotate_if_oversized(path: &Path, max_bytes: u64) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < max_bytes {
        return;
    }
    let rotated = PathBuf::from(format!("{}.1", path.display()));
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, &rotated);
}

pub fn append_log_marker(path: &Path, message: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

// -- Init -----------------------------------------------------------------

/// Resolved paths for the three log files, derived from the existing
/// `resolve_log_file()` locator's parent directory.
pub struct LogPaths {
    pub dir: PathBuf,
    pub server_log: PathBuf,
    pub scan_debug_log: PathBuf,
    pub deep_debug_log: PathBuf,
}

pub fn resolve_log_paths(server_log: &Path) -> Option<LogPaths> {
    let dir = server_log.parent()?.to_path_buf();
    Some(LogPaths {
        server_log: server_log.to_path_buf(),
        scan_debug_log: dir.join("boogiebox-scan-debug.log"),
        deep_debug_log: dir.join("boogiebox-deepmix-debug.log"),
        dir,
    })
}

/// Initializes the three-layer subscriber. `scan_enabled`/`deep_enabled` are
/// the best-known state at process start (before settings can be read from
/// the DB); call [`sync_debug_toggles`] once the DB is open to reconcile.
pub fn init(paths: &LogPaths, scan_enabled: bool, deep_enabled: bool) {
    let env_level = std::env::var("BOOGIEBOX_LOG_LEVEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "info".to_owned());
    let server_level: LevelFilter = env_level.trim().parse().unwrap_or(LevelFilter::INFO);

    let _ = fs::create_dir_all(&paths.dir);
    append_log_marker(&paths.server_log, "BoogieBox server log initialized");

    let server_writer = CappedFileWriter {
        path: paths.server_log.clone(),
        max_bytes: SERVER_LOG_MAX_BYTES,
    };
    let server_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(server_writer)
        .with_filter(Targets::new().with_default(server_level));

    let scan_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("boogiebox-scan-debug")
        .filename_suffix("log")
        .max_log_files(DEBUG_LOG_MAX_FILES)
        .build(&paths.dir)
        .ok();
    let scan_fmt_layer: Box<dyn Layer<Registry> + Send + Sync> = match scan_appender {
        Some(appender) => Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(appender),
        ),
        None => Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(io::sink),
        ),
    };
    let (scan_layer, scan_handle) =
        reload::Layer::new(scan_fmt_layer.with_filter(scan_targets(scan_enabled)));
    let _ = SCAN_RELOAD.set(scan_handle);

    let deep_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("boogiebox-deepmix-debug")
        .filename_suffix("log")
        .max_log_files(DEBUG_LOG_MAX_FILES)
        .build(&paths.dir)
        .ok();
    let deep_fmt_layer: Box<dyn Layer<Registry> + Send + Sync> = match deep_appender {
        Some(appender) => Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(appender),
        ),
        None => Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(io::sink),
        ),
    };
    let (deep_layer, deep_handle) =
        reload::Layer::new(deep_fmt_layer.with_filter(deep_targets(deep_enabled)));
    let _ = DEEP_RELOAD.set(deep_handle);

    // `.with()`-chaining requires each layer to implement `Layer<S>` for the
    // progressively-accumulated subscriber type. Since these three layers are
    // independent (none needs to see the others' spans), combine them into a
    // single `Layer<Registry>` via `and_then` first, then apply that combined
    // layer to `Registry` in one `.with()` call.
    let combined = server_layer.and_then(scan_layer).and_then(deep_layer);
    let _ = tracing_subscriber::registry().with(combined).try_init();
}

/// Reconciles the live debug-log filters with the DB-backed settings once the
/// database is available (settings aren't readable at the point [`init`] runs).
pub fn sync_debug_toggles(scan_enabled: bool, deep_enabled: bool) {
    set_scan_debug_enabled(scan_enabled);
    set_deep_debug_enabled(deep_enabled);
}

/// Flips the scan-debug log on/off immediately, no restart required. Called
/// from the settings PUT route right after `scanDebugLoggingEnabled` is saved.
pub fn set_scan_debug_enabled(enabled: bool) {
    if let Some(handle) = SCAN_RELOAD.get() {
        let _ = handle.modify(|filtered| *filtered.filter_mut() = scan_targets(enabled));
    }
}

/// Flips the deep-analysis-debug log on/off immediately, no restart required.
/// Called from the settings PUT route right after `deepmixDebugLoggingEnabled`
/// is saved.
pub fn set_deep_debug_enabled(enabled: bool) {
    if let Some(handle) = DEEP_RELOAD.get() {
        let _ = handle.modify(|filtered| *filtered.filter_mut() = deep_targets(enabled));
    }
}

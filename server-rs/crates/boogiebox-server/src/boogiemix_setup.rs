//! In-app BoogieMix deep-analysis setup (Windows; `wip/microsoft-store-distribution-plan.md` phase 1).
//!
//! Runs the bundled `bootstrap_env.ps1` into a managed environment under `%PROGRAMDATA%\BoogieBox\boogiemix`
//! (writable by the tray user and the service account, unlike the install folder), reports its progress from
//! the `##BB-STEP n/m key` lines the script prints, keeps the last output lines and writes every line to the
//! server log. One setup runs at a time. Linux keeps `install.sh --with-boogiemix`, so there the setup is
//! reported as not supported.
//!
//! Every path and argument is chosen by the server; nothing from a request reaches the command line.

use serde::Serialize;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

/// Steps `bootstrap_env.ps1` announces (python, venv, tools, torch, packages, extras, models, check).
pub const SETUP_TOTAL_STEPS: u32 = 8;
/// Output lines kept for the Settings card.
pub const LOG_TAIL_LINES: usize = 200;
/// Longer output lines are cut.
const MAX_LINE_CHARS: usize = 1000;
/// Free space the setup asks for before it starts (Python, PyTorch, Demucs models, pip cache).
pub const REQUIRED_FREE_BYTES: u64 = 6 * 1024 * 1024 * 1024;
/// The import check after the script finishes.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(180);
const STEP_PREFIX: &str = "##BB-STEP ";

/// Error keys returned to the client (it owns the wording).
pub mod errors {
    pub const NOT_SUPPORTED: &str = "not_supported";
    pub const ALREADY_RUNNING: &str = "already_running";
    pub const NOT_RUNNING: &str = "not_running";
    pub const JOBS_RUNNING: &str = "jobs_running";
    pub const SCRIPT_MISSING: &str = "script_missing";
    pub const LOW_DISK: &str = "low_disk";
    pub const NOT_INSTALLED: &str = "not_installed";
    pub const SPAWN_FAILED: &str = "spawn_failed";
    pub const EXIT_CODE: &str = "exit_code";
    pub const WAIT_FAILED: &str = "wait_failed";
    pub const VERIFY_FAILED: &str = "verify_failed";
    pub const REMOVE_FAILED: &str = "remove_failed";
}

/// Where a setup run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupState {
    Idle,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// The step the script last announced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub index: u32,
    pub total: u32,
    pub key: String,
}

/// What `GET /api/boogiemix/deep-analysis/setup` returns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupSnapshot {
    pub supported: bool,
    pub state: SetupState,
    pub step: Option<SetupStep>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub lines: Vec<String>,
    /// Whether the managed environment's Python is there.
    pub installed: bool,
    pub free_bytes: Option<u64>,
    pub required_bytes: u64,
}

/// Paths one setup run uses.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub script: PathBuf,
    pub venv_dir: PathBuf,
    pub torch_home: PathBuf,
}

/// Builds the commands a run executes; production uses PowerShell, tests use small shell commands.
pub trait SetupRunner: Send + Sync + 'static {
    /// The bootstrap command.
    fn command(&self, plan: &LaunchPlan) -> Command;
    /// The check that the environment imports torch and demucs.
    fn verify_command(&self, plan: &LaunchPlan) -> Command;
}

/// Runs `bootstrap_env.ps1` with Windows PowerShell.
pub struct PowerShellRunner;

impl SetupRunner for PowerShellRunner {
    fn command(&self, plan: &LaunchPlan) -> Command {
        let mut cmd = Command::new(powershell_exe());
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&plan.script)
            .arg("-Auto")
            .arg("-PrimeDemucsModel")
            .arg("-Force")
            .arg("-VenvPath")
            .arg(&plan.venv_dir)
            .arg("-TorchHome")
            .arg(&plan.torch_home);
        cmd
    }

    fn verify_command(&self, plan: &LaunchPlan) -> Command {
        let mut cmd = Command::new(venv_python(&plan.venv_dir));
        cmd.arg("-c").arg("import torch, demucs");
        cmd
    }
}

fn powershell_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[derive(Default)]
struct Inner {
    state: Option<SetupState>,
    step: Option<SetupStep>,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    error: Option<String>,
    exit_code: Option<i32>,
    lines: VecDeque<String>,
    cancel: Option<CancellationToken>,
}

/// One machine-wide setup slot.
pub struct SetupHub {
    inner: Arc<Mutex<Inner>>,
    runner: Arc<dyn SetupRunner>,
}

impl SetupHub {
    pub fn new(runner: Arc<dyn SetupRunner>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            runner,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self) -> SetupState {
        self.lock().state.unwrap_or(SetupState::Idle)
    }

    pub fn is_running(&self) -> bool {
        self.state() == SetupState::Running
    }

    /// The run's state; `installed` and disk figures are filled in by [`snapshot`].
    fn core_snapshot(&self) -> SetupSnapshot {
        let inner = self.lock();
        SetupSnapshot {
            supported: supported(),
            state: inner.state.unwrap_or(SetupState::Idle),
            step: inner.step.clone(),
            started_at_ms: inner.started_at_ms,
            finished_at_ms: inner.finished_at_ms,
            error: inner.error.clone(),
            exit_code: inner.exit_code,
            lines: inner.lines.iter().cloned().collect(),
            installed: false,
            free_bytes: None,
            required_bytes: REQUIRED_FREE_BYTES,
        }
    }

    /// Starts a run unless one is going. Must be called inside a Tokio runtime.
    pub fn start(&self, plan: LaunchPlan) -> Result<(), &'static str> {
        let cancel = CancellationToken::new();
        {
            let mut inner = self.lock();
            if inner.state == Some(SetupState::Running) {
                return Err(errors::ALREADY_RUNNING);
            }
            *inner = Inner {
                state: Some(SetupState::Running),
                started_at_ms: Some(now_ms()),
                cancel: Some(cancel.clone()),
                ..Inner::default()
            };
        }
        tracing::info!("[boogiemix:setup] started");
        let inner = self.inner.clone();
        let runner = self.runner.clone();
        tokio::spawn(async move { run(inner, runner, plan, cancel).await });
        Ok(())
    }

    /// Asks the running setup to stop.
    pub fn cancel(&self) -> Result<(), &'static str> {
        let inner = self.lock();
        match (&inner.state, &inner.cancel) {
            (Some(SetupState::Running), Some(token)) => {
                token.cancel();
                Ok(())
            }
            _ => Err(errors::NOT_RUNNING),
        }
    }

    /// Back to idle after a remove.
    fn reset(&self) {
        *self.lock() = Inner::default();
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn finish(inner: &Mutex<Inner>, state: SetupState, error: Option<&str>, exit_code: Option<i32>) {
    let mut guard = inner.lock().unwrap_or_else(|p| p.into_inner());
    guard.state = Some(state);
    guard.error = error.map(str::to_string);
    guard.exit_code = exit_code;
    guard.finished_at_ms = Some(now_ms());
    guard.cancel = None;
    drop(guard);
    match error {
        Some(e) => {
            tracing::warn!("[boogiemix:setup] finished: {state:?} ({e}, exit code {exit_code:?})")
        }
        None => tracing::info!("[boogiemix:setup] finished: {state:?}"),
    }
}

/// A `##BB-STEP n/m key` line, or `None` for any other line (or a malformed one).
pub fn parse_step_line(line: &str) -> Option<SetupStep> {
    let rest = line.trim().strip_prefix(STEP_PREFIX)?;
    let (counts, key) = rest.split_once(' ')?;
    let (index, total) = counts.split_once('/')?;
    let index: u32 = index.parse().ok()?;
    let total: u32 = total.parse().ok()?;
    let key = key.trim();
    let key_ok = !key.is_empty()
        && key.len() <= 32
        && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    (index >= 1 && index <= total && key_ok).then(|| SetupStep {
        index,
        total,
        key: key.to_string(),
    })
}

/// Adds one output line: a step marker updates the step, anything else goes to the tail and the server log.
fn record_line(inner: &Mutex<Inner>, raw: &str) {
    let line = raw.trim_end_matches(['\r', '\n']);
    if line.trim().is_empty() {
        return;
    }
    let mut guard = inner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(step) = parse_step_line(line) {
        tracing::info!(
            "[boogiemix:setup] step {}/{} {}",
            step.index,
            step.total,
            step.key
        );
        guard.step = Some(step);
        return;
    }
    let line: String = if line.chars().count() > MAX_LINE_CHARS {
        let mut cut: String = line.chars().take(MAX_LINE_CHARS).collect();
        cut.push('…');
        cut
    } else {
        line.to_string()
    };
    tracing::info!("[boogiemix:setup] {line}");
    guard.lines.push_back(line);
    while guard.lines.len() > LOG_TAIL_LINES {
        guard.lines.pop_front();
    }
}

async fn read_lines<R: AsyncRead + Unpin>(reader: R, inner: Arc<Mutex<Inner>>) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => record_line(&inner, &String::from_utf8_lossy(&buf)),
        }
    }
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Stops the script and everything it started (Python, pip, the Python installer).
async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut taskkill = Command::new(PathBuf::from(root).join("System32").join("taskkill.exe"));
        taskkill
            .arg("/T")
            .arg("/F")
            .arg("/PID")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_window(&mut taskkill);
        let _ = taskkill.status().await;
    }
    let _ = child.kill().await;
}

async fn run(
    inner: Arc<Mutex<Inner>>,
    runner: Arc<dyn SetupRunner>,
    plan: LaunchPlan,
    cancel: CancellationToken,
) {
    let mut cmd = runner.command(&plan);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_window(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!("[boogiemix:setup] could not start the setup script: {e}");
            finish(&inner, SetupState::Failed, Some(errors::SPAWN_FAILED), None);
            return;
        }
    };
    let out = child
        .stdout
        .take()
        .map(|s| tokio::spawn(read_lines(s, inner.clone())));
    let err = child
        .stderr
        .take()
        .map(|s| tokio::spawn(read_lines(s, inner.clone())));

    let outcome = tokio::select! {
        status = child.wait() => Some(status),
        _ = cancel.cancelled() => None,
    };
    if outcome.is_none() {
        kill_tree(&mut child).await;
    }
    for reader in [out, err].into_iter().flatten() {
        let _ = reader.await;
    }

    match outcome {
        None => {
            // A half-built environment is no use; the next install starts clean anyway.
            let venv = plan.venv_dir.clone();
            let _ = tokio::task::spawn_blocking(move || remove_dir_if_present(&venv)).await;
            finish(&inner, SetupState::Cancelled, None, None);
        }
        Some(Err(e)) => {
            tracing::warn!("[boogiemix:setup] waiting for the setup script failed: {e}");
            finish(&inner, SetupState::Failed, Some(errors::WAIT_FAILED), None);
        }
        Some(Ok(status)) if !status.success() => {
            finish(
                &inner,
                SetupState::Failed,
                Some(errors::EXIT_CODE),
                status.code(),
            );
        }
        Some(Ok(_)) => {
            if verify(runner.as_ref(), &plan).await {
                finish(&inner, SetupState::Succeeded, None, Some(0));
            } else {
                finish(
                    &inner,
                    SetupState::Failed,
                    Some(errors::VERIFY_FAILED),
                    Some(0),
                );
            }
        }
    }
    crate::routes::boogiemix_routes::invalidate_deep_runtime_cache().await;
}

async fn verify(runner: &dyn SetupRunner, plan: &LaunchPlan) -> bool {
    let mut cmd = runner.verify_command(plan);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    hide_window(&mut cmd);
    matches!(
        tokio::time::timeout(VERIFY_TIMEOUT, cmd.status()).await,
        Ok(Ok(status)) if status.success()
    )
}

fn remove_dir_if_present(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path).map(|_| true),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

// ── Paths ──────────────────────────────────────────────────────────────────────

/// Whether in-app setup runs on this OS (Windows only; Linux uses `install.sh --with-boogiemix`).
pub const fn supported() -> bool {
    cfg!(windows)
}

/// `%PROGRAMDATA%\BoogieBox\boogiemix` on Windows; `None` elsewhere.
pub fn managed_root() -> Option<PathBuf> {
    if !supported() {
        return None;
    }
    std::env::var_os("PROGRAMDATA")
        .filter(|v| !v.is_empty())
        .map(|pd| PathBuf::from(pd).join("BoogieBox").join("boogiemix"))
}

/// The managed environment folder.
pub fn managed_venv_dir() -> Option<PathBuf> {
    managed_root().map(|root| root.join(".venv"))
}

/// The Python inside a venv folder.
pub fn venv_python(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

/// The managed environment's Python, when it exists. Deep-analysis detection tries it first.
pub fn installed_managed_python() -> Option<PathBuf> {
    managed_venv_dir()
        .map(|dir| venv_python(&dir))
        .filter(|p| p.is_file())
}

/// The bundled `bootstrap_env.ps1` (installed layout first, then the source tree).
pub fn bootstrap_script() -> Option<PathBuf> {
    let rels: &[&[&str]] = &[
        &["resources", "Services", "boogiemix", "python"],
        &["Services", "boogiemix", "python"],
    ];
    crate::deep_analysis::candidate_roots()
        .into_iter()
        .flat_map(|root| {
            rels.iter()
                .map(move |rel| rel.iter().fold(root.clone(), |p, s| p.join(s)))
        })
        .map(|dir| dir.join("bootstrap_env.ps1"))
        .find(|p| p.is_file())
}

/// Free bytes on the drive that holds `path` (or its nearest existing parent); `None` when unknown.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    free_bytes_at(existing)
}

#[cfg(windows)]
fn free_bytes_at(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointer is a valid u64.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(not(windows))]
fn free_bytes_at(_path: &Path) -> Option<u64> {
    None
}

// ── Checks and the shared hub ──────────────────────────────────────────────────

/// Why an install can't start, checked in this order.
pub fn preflight(
    is_supported: bool,
    running: bool,
    jobs_running: bool,
    script: Option<&Path>,
    free: Option<u64>,
) -> Result<(), &'static str> {
    if !is_supported {
        return Err(errors::NOT_SUPPORTED);
    }
    if running {
        return Err(errors::ALREADY_RUNNING);
    }
    if jobs_running {
        return Err(errors::JOBS_RUNNING);
    }
    if script.is_none() {
        return Err(errors::SCRIPT_MISSING);
    }
    if free.is_some_and(|f| f < REQUIRED_FREE_BYTES) {
        return Err(errors::LOW_DISK);
    }
    Ok(())
}

/// The server's setup hub.
pub fn hub() -> &'static SetupHub {
    static HUB: OnceLock<SetupHub> = OnceLock::new();
    HUB.get_or_init(|| SetupHub::new(Arc::new(PowerShellRunner)))
}

/// The full snapshot: run state plus whether the environment exists and the free space.
pub async fn snapshot(hub: &SetupHub) -> SetupSnapshot {
    let mut snap = hub.core_snapshot();
    let root = managed_root();
    let (installed, free) = tokio::task::spawn_blocking(move || {
        let installed = installed_managed_python().is_some();
        let free = root.as_deref().and_then(free_bytes);
        (installed, free)
    })
    .await
    .unwrap_or((false, None));
    snap.installed = installed;
    snap.free_bytes = free;
    snap
}

/// Starts an install after the checks pass.
pub async fn install(hub: &SetupHub, jobs_running: bool) -> Result<(), &'static str> {
    let (script, venv_dir, free) = tokio::task::spawn_blocking(|| {
        let venv = managed_venv_dir();
        let free = managed_root().as_deref().and_then(free_bytes);
        (bootstrap_script(), venv, free)
    })
    .await
    .unwrap_or((None, None, None));
    preflight(
        supported(),
        hub.is_running(),
        jobs_running,
        script.as_deref(),
        free,
    )?;
    let (Some(script), Some(venv_dir)) = (script, venv_dir) else {
        return Err(errors::NOT_SUPPORTED);
    };
    hub.start(LaunchPlan {
        script,
        venv_dir,
        torch_home: crate::deep_analysis::model_cache_path(),
    })
}

/// Deletes the managed environment and the model cache.
pub async fn remove(hub: &SetupHub, jobs_running: bool) -> Result<(), &'static str> {
    if !supported() {
        return Err(errors::NOT_SUPPORTED);
    }
    if hub.is_running() {
        return Err(errors::ALREADY_RUNNING);
    }
    if jobs_running {
        return Err(errors::JOBS_RUNNING);
    }
    let Some(root) = managed_root() else {
        return Err(errors::NOT_SUPPORTED);
    };
    let models = crate::deep_analysis::model_cache_path();
    let result = tokio::task::spawn_blocking(move || {
        let venv = remove_dir_if_present(&root)?;
        let cache = remove_dir_if_present(&models)?;
        Ok::<bool, std::io::Error>(venv || cache)
    })
    .await;
    let outcome = match result {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => Err(errors::NOT_INSTALLED),
        Ok(Err(e)) => {
            tracing::warn!("[boogiemix:setup] remove failed: {e}");
            Err(errors::REMOVE_FAILED)
        }
        Err(_) => Err(errors::REMOVE_FAILED),
    };
    if outcome.is_ok() {
        tracing::info!("[boogiemix:setup] removed the managed environment and model cache");
        hub.reset();
        crate::routes::boogiemix_routes::invalidate_deep_runtime_cache().await;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a shell snippet in place of the bootstrap script; the verify step exits with `verify_code`.
    struct ShellRunner {
        script: &'static str,
        verify_code: i32,
    }

    fn shell(snippet: &str) -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(snippet);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(snippet);
            cmd
        }
    }

    impl SetupRunner for ShellRunner {
        fn command(&self, _plan: &LaunchPlan) -> Command {
            shell(self.script)
        }
        fn verify_command(&self, _plan: &LaunchPlan) -> Command {
            shell(&format!("exit {}", self.verify_code))
        }
    }

    fn plan(dir: &Path) -> LaunchPlan {
        LaunchPlan {
            script: dir.join("bootstrap_env.ps1"),
            venv_dir: dir.join(".venv"),
            torch_home: dir.join("models"),
        }
    }

    async fn wait_until_done(hub: &SetupHub) -> SetupSnapshot {
        for _ in 0..400 {
            if !hub.is_running() {
                return hub.core_snapshot();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("setup did not finish");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_step_line_reads_markers_and_rejects_everything_else() {
        assert_eq!(
            parse_step_line("##BB-STEP 4/8 torch\r"),
            Some(SetupStep {
                index: 4,
                total: 8,
                key: "torch".into()
            })
        );
        assert_eq!(
            parse_step_line("  ##BB-STEP 1/8 python  ").unwrap().index,
            1
        );
        for bad in [
            "Collecting torch",
            "##BB-STEP",
            "##BB-STEP 0/8 python",
            "##BB-STEP 9/8 python",
            "##BB-STEP x/8 python",
            "##BB-STEP 1/8",
            "##BB-STEP 1/8 Python",
            "##BB-STEP 1/8 py-thon",
            "##BB-STEP 1-8 python",
        ] {
            assert_eq!(parse_step_line(bad), None, "{bad}");
        }
    }

    #[test]
    fn record_line_keeps_a_bounded_tail_and_skips_markers_and_blanks() {
        let inner = Mutex::new(Inner::default());
        record_line(&inner, "##BB-STEP 2/8 venv\n");
        record_line(&inner, "   \r\n");
        for i in 0..(LOG_TAIL_LINES + 5) {
            record_line(&inner, &format!("line {i}\r\n"));
        }
        let guard = inner.lock().unwrap();
        assert_eq!(guard.step.as_ref().unwrap().key, "venv");
        assert_eq!(guard.lines.len(), LOG_TAIL_LINES);
        assert_eq!(guard.lines.front().unwrap(), "line 5");
        assert_eq!(
            guard.lines.back().unwrap(),
            &format!("line {}", LOG_TAIL_LINES + 4)
        );
    }

    #[test]
    fn record_line_cuts_long_lines() {
        let inner = Mutex::new(Inner::default());
        record_line(&inner, &"é".repeat(MAX_LINE_CHARS + 10));
        let guard = inner.lock().unwrap();
        let line = guard.lines.back().unwrap();
        assert_eq!(line.chars().count(), MAX_LINE_CHARS + 1);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn preflight_checks_in_order() {
        let script = Path::new("bootstrap_env.ps1");
        let enough = Some(REQUIRED_FREE_BYTES);
        assert_eq!(
            preflight(false, true, true, None, Some(0)),
            Err(errors::NOT_SUPPORTED)
        );
        assert_eq!(
            preflight(true, true, true, None, Some(0)),
            Err(errors::ALREADY_RUNNING)
        );
        assert_eq!(
            preflight(true, false, true, None, Some(0)),
            Err(errors::JOBS_RUNNING)
        );
        assert_eq!(
            preflight(true, false, false, None, Some(0)),
            Err(errors::SCRIPT_MISSING)
        );
        assert_eq!(
            preflight(
                true,
                false,
                false,
                Some(script),
                Some(REQUIRED_FREE_BYTES - 1)
            ),
            Err(errors::LOW_DISK)
        );
        assert_eq!(preflight(true, false, false, Some(script), enough), Ok(()));
        // Unknown free space doesn't block.
        assert_eq!(preflight(true, false, false, Some(script), None), Ok(()));
    }

    #[test]
    fn snapshot_serializes_camel_case_with_lowercase_state() {
        let hub = SetupHub::new(Arc::new(ShellRunner {
            script: "exit 0",
            verify_code: 0,
        }));
        let json = serde_json::to_value(hub.core_snapshot()).unwrap();
        assert_eq!(json["state"], "idle");
        assert_eq!(json["supported"], supported());
        assert_eq!(json["requiredBytes"], REQUIRED_FREE_BYTES);
        assert!(json["lines"].as_array().unwrap().is_empty());
        assert!(json.get("startedAtMs").is_some());
    }

    #[tokio::test]
    async fn successful_run_records_steps_lines_and_succeeds() {
        let dir = temp_dir("ok");
        let script = if cfg!(windows) {
            "echo ##BB-STEP 3/8 tools&& echo hello"
        } else {
            "echo '##BB-STEP 3/8 tools'; echo hello"
        };
        let hub = SetupHub::new(Arc::new(ShellRunner {
            script,
            verify_code: 0,
        }));
        hub.start(plan(&dir)).unwrap();
        assert_eq!(hub.start(plan(&dir)), Err(errors::ALREADY_RUNNING));
        let snap = wait_until_done(&hub).await;
        assert_eq!(snap.state, SetupState::Succeeded);
        assert_eq!(snap.step.unwrap().key, "tools");
        assert!(snap.lines.iter().any(|l| l.trim() == "hello"));
        assert_eq!(snap.exit_code, Some(0));
        assert!(snap.finished_at_ms.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failing_script_reports_its_exit_code() {
        let dir = temp_dir("exit");
        let hub = SetupHub::new(Arc::new(ShellRunner {
            script: "exit 3",
            verify_code: 0,
        }));
        hub.start(plan(&dir)).unwrap();
        let snap = wait_until_done(&hub).await;
        assert_eq!(snap.state, SetupState::Failed);
        assert_eq!(snap.error.as_deref(), Some(errors::EXIT_CODE));
        assert_eq!(snap.exit_code, Some(3));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_import_check_fails_the_run() {
        let dir = temp_dir("verify");
        let hub = SetupHub::new(Arc::new(ShellRunner {
            script: "exit 0",
            verify_code: 1,
        }));
        hub.start(plan(&dir)).unwrap();
        let snap = wait_until_done(&hub).await;
        assert_eq!(snap.state, SetupState::Failed);
        assert_eq!(snap.error.as_deref(), Some(errors::VERIFY_FAILED));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cancel_stops_the_run_and_removes_the_partial_environment() {
        let dir = temp_dir("cancel");
        std::fs::create_dir_all(dir.join(".venv").join("Lib")).unwrap();
        let long = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >nul"
        } else {
            "sleep 30"
        };
        let hub = SetupHub::new(Arc::new(ShellRunner {
            script: long,
            verify_code: 0,
        }));
        assert_eq!(hub.cancel(), Err(errors::NOT_RUNNING));
        hub.start(plan(&dir)).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        hub.cancel().unwrap();
        let snap = wait_until_done(&hub).await;
        assert_eq!(snap.state, SetupState::Cancelled);
        assert!(!dir.join(".venv").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn missing_program_fails_to_spawn() {
        struct Missing;
        impl SetupRunner for Missing {
            fn command(&self, _plan: &LaunchPlan) -> Command {
                Command::new("boogiebox-no-such-program-xyz")
            }
            fn verify_command(&self, _plan: &LaunchPlan) -> Command {
                Command::new("boogiebox-no-such-program-xyz")
            }
        }
        let dir = temp_dir("spawn");
        let hub = SetupHub::new(Arc::new(Missing));
        hub.start(plan(&dir)).unwrap();
        let snap = wait_until_done(&hub).await;
        assert_eq!(snap.state, SetupState::Failed);
        assert_eq!(snap.error.as_deref(), Some(errors::SPAWN_FAILED));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_dir_if_present_handles_missing_and_files() {
        let dir = temp_dir("remove");
        assert!(!remove_dir_if_present(&dir.join("nope")).unwrap());
        let file = dir.join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(!remove_dir_if_present(&file).unwrap());
        assert!(file.exists());
        let sub = dir.join("sub");
        std::fs::create_dir_all(sub.join("a")).unwrap();
        assert!(remove_dir_if_present(&sub).unwrap());
        assert!(!sub.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn paths_follow_the_platform() {
        let venv = Path::new("v");
        if cfg!(windows) {
            assert_eq!(
                venv_python(venv),
                Path::new("v").join("Scripts").join("python.exe")
            );
            if std::env::var_os("PROGRAMDATA").is_some() {
                let root = managed_root().unwrap();
                assert!(root.ends_with(Path::new("BoogieBox").join("boogiemix")));
                assert_eq!(managed_venv_dir().unwrap(), root.join(".venv"));
            }
        } else {
            assert_eq!(venv_python(venv), Path::new("v").join("bin").join("python"));
            assert!(managed_root().is_none());
            assert!(installed_managed_python().is_none());
        }
    }

    #[test]
    fn bootstrap_script_is_found_in_the_source_tree() {
        let script = bootstrap_script().expect("repo copy of bootstrap_env.ps1");
        assert!(script.ends_with("bootstrap_env.ps1"));
    }

    #[test]
    fn free_bytes_walks_up_to_an_existing_folder() {
        let missing = std::env::temp_dir().join("bb-setup-free").join("deeper");
        let free = free_bytes(&missing);
        if cfg!(windows) {
            assert!(free.is_some_and(|f| f > 0));
        } else {
            assert!(free.is_none());
        }
    }
}

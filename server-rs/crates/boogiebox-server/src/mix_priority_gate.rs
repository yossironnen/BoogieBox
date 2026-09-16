//! Shared coordination gate for BoogieMix build prerequisites
//! (wip/boogiemix-story-timeline-plan.md §4.7).
//!
//! While a mix's pre-render analysis wait is in flight, the CPU/ffmpeg-bound
//! background sweep paths of `bpm_analysis`, `waveform_map`, and
//! `deep_analysis` stand down from claiming *new* library-wide work, so the
//! mix's own targeted requests get the machine. Nothing already running is
//! force-cancelled by this gate (deep analysis has its own separate
//! priority-preemption for that); `scanner`, `post_scan`, and
//! `db_maintenance` deliberately do not check this gate — they're network-
//! or I/O-bound, not competing for the same resource.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Reference count, not a bool: concurrent mix builds must not clear the
/// gate for one another when the first one finishes.
static ACTIVE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// True while at least one BoogieMix build's pre-render analysis wait is in
/// progress.
pub fn is_active() -> bool {
    ACTIVE_COUNT.load(Ordering::SeqCst) > 0
}

/// RAII guard: acquiring raises the gate, dropping (including on an early
/// `?` return or panic unwind) lowers it — a mix build can never leave the
/// gate stuck up.
#[must_use]
pub struct PriorityAnalysisGuard {
    _private: (),
}

impl PriorityAnalysisGuard {
    pub fn acquire() -> Self {
        ACTIVE_COUNT.fetch_add(1, Ordering::SeqCst);
        Self { _private: () }
    }
}

impl Drop for PriorityAnalysisGuard {
    fn drop(&mut self) {
        ACTIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `ACTIVE_COUNT` is process-wide, so any test anywhere in this crate that
/// acquires a [`PriorityAnalysisGuard`] (directly, or indirectly via
/// `bpm_analysis`/`waveform_map`/`deep_analysis` gate checks) must hold this
/// lock for the duration — otherwise unrelated tests racing under
/// `cargo test`'s default parallelism could observe a gate state some other
/// test put it in. Mirrors the per-file `RUN_LOCK` pattern already used in
/// `bpm_analysis.rs`/`waveform_map.rs`, just crate-wide instead of per-file
/// since this particular static is shared, not module-private. `tokio::sync`
/// (not `std::sync`) because several consumers hold this across an
/// `.await`, which clippy's `await_holding_lock` correctly refuses for a
/// std `Mutex`.
#[cfg(test)]
pub(crate) static GATE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gate_is_inactive_by_default_and_active_while_a_guard_is_held() {
        let _guard = GATE_TEST_LOCK.lock().await;
        assert!(!is_active());
        let guard = PriorityAnalysisGuard::acquire();
        assert!(is_active());
        drop(guard);
        assert!(!is_active());
    }

    #[tokio::test]
    async fn nested_guards_only_clear_once_all_are_dropped() {
        let _guard = GATE_TEST_LOCK.lock().await;
        let a = PriorityAnalysisGuard::acquire();
        let b = PriorityAnalysisGuard::acquire();
        assert!(is_active());
        drop(a);
        assert!(
            is_active(),
            "one build finishing must not clear the gate for another still running"
        );
        drop(b);
        assert!(!is_active());
    }
}

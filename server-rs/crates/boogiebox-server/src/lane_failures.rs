//! Remembers files a background lane keeps failing on (a damaged file FFmpeg cannot decode), so the lane stops
//! retrying them until the server restarts instead of failing and logging on every run. A missing file is not a
//! failure: a library on a network share can be briefly offline, so a missing file is only left alone for an
//! hour and never counted as given up.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Failures a lane allows per file before it leaves the file alone.
pub const MAX_ATTEMPTS: u8 = 3;
/// How long a missing file is left alone before the lane looks for it again.
pub const MISSING_RETRY: Duration = Duration::from_secs(60 * 60);

/// Per-lane failure counts, kept in memory for the life of the process.
pub struct LaneFailures {
    lane: &'static str,
    attempts: Mutex<BTreeMap<String, u8>>,
    missing: Mutex<BTreeMap<String, Instant>>,
}

impl LaneFailures {
    /// A tracker for the lane named `lane` (used in the log line when it gives up on a file).
    pub const fn new(lane: &'static str) -> Self {
        Self {
            lane,
            attempts: Mutex::new(BTreeMap::new()),
            missing: Mutex::new(BTreeMap::new()),
        }
    }

    fn given_up_ids(&self) -> HashSet<String> {
        self.attempts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(_, n)| **n >= MAX_ATTEMPTS)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Ids the lane should not pick now: given up on, or missing within the last [`MISSING_RETRY`].
    pub fn skipped(&self) -> HashSet<String> {
        let mut ids = self.given_up_ids();
        let mut missing = self.missing.lock().unwrap_or_else(|p| p.into_inner());
        missing.retain(|_, at| at.elapsed() < MISSING_RETRY);
        ids.extend(missing.keys().cloned());
        ids
    }

    /// Number of ids this lane has given up on (missing files are not counted).
    pub fn count(&self) -> usize {
        self.given_up_ids().len()
    }

    /// Counts one failure for `id`; `give_up` skips straight to the limit (a failure retrying cannot fix).
    pub fn note_failure(&self, id: &str, give_up: bool) {
        let mut attempts = self.attempts.lock().unwrap_or_else(|p| p.into_inner());
        let n = attempts.entry(id.to_owned()).or_default();
        if *n >= MAX_ATTEMPTS {
            return;
        }
        *n = if give_up {
            MAX_ATTEMPTS
        } else {
            n.saturating_add(1)
        };
        if *n >= MAX_ATTEMPTS {
            tracing::warn!(
                lane = self.lane,
                id,
                "background lane stopped retrying this file until the next restart"
            );
        }
    }

    /// Leaves `id` alone for [`MISSING_RETRY`] because its file was not found.
    pub fn note_missing(&self, id: &str) {
        self.missing
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_owned(), Instant::now());
    }

    /// `rows` without the ids in [`Self::skipped`], at most `limit` of them.
    pub fn without_skipped<T>(
        &self,
        rows: Vec<T>,
        id: impl Fn(&T) -> &str,
        limit: usize,
    ) -> Vec<T> {
        let skip = self.skipped();
        rows.into_iter()
            .filter(|row| !skip.contains(id(row)))
            .take(limit)
            .collect()
    }

    /// Forgets every count (tests share the process-wide trackers).
    #[cfg(test)]
    pub fn clear(&self) {
        self.attempts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.missing
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gives_up_after_the_limit_or_at_once_and_filters_rows() {
        let failures = LaneFailures::new("test");
        failures.note_failure("a", false);
        failures.note_failure("a", false);
        assert_eq!(failures.count(), 0);
        failures.note_failure("a", false);
        failures.note_failure("b", true);
        assert_eq!(
            failures.skipped(),
            HashSet::from(["a".to_string(), "b".to_string()])
        );
        // Further failures neither count again nor log again.
        failures.note_failure("a", false);
        assert_eq!(failures.count(), 2);

        let rows = vec!["a", "c", "b", "d", "e"];
        assert_eq!(failures.without_skipped(rows, |r| r, 2), vec!["c", "d"]);
        failures.clear();
        assert_eq!(failures.count(), 0);
    }

    #[test]
    fn a_missing_file_is_skipped_for_a_while_but_never_counted() {
        let failures = LaneFailures::new("test");
        failures.note_missing("gone");
        assert_eq!(failures.count(), 0);
        assert!(failures.skipped().contains("gone"));
        // Once the retry time has passed it is picked again.
        let Some(long_ago) = Instant::now().checked_sub(MISSING_RETRY + Duration::from_secs(1))
        else {
            return; // the clock started less than an hour ago
        };
        failures
            .missing
            .lock()
            .unwrap()
            .insert("gone".into(), long_ago);
        assert!(failures.skipped().is_empty());
    }
}

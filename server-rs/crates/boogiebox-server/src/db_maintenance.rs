//! Weekly, late-night `PRAGMA incremental_vacuum` sweep.
//!
//! See `boogiebox_db::maintenance` for the due-check and the pragma call itself; this
//! module only owns the polling loop, mirroring `bpm_analysis`/`waveform_map`.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::DbPool;

/// Documents the Start Db Maintenance Scheduler public API surface.
pub fn start_db_maintenance_scheduler(db: DbPool, cancel: CancellationToken) {
    tokio::spawn(async move {
        run_incremental_vacuum_if_due(&db).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => run_incremental_vacuum_if_due(&db).await,
            }
        }
    });
}

async fn run_incremental_vacuum_if_due(db: &DbPool) {
    let db_check = db.clone();
    let due = tokio::task::spawn_blocking(move || {
        let conn = db_check.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::maintenance::incremental_vacuum_due(&conn)
    })
    .await;
    let due = match due {
        Ok(Ok(due)) => due,
        Ok(Err(err)) => {
            tracing::warn!("db-maintenance due-check failed: {err}");
            return;
        }
        Err(err) => {
            tracing::error!("db-maintenance due-check spawn failed: {err}");
            return;
        }
    };
    if !due {
        return;
    }

    let db_run = db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db_run.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::maintenance::run_incremental_vacuum(&conn)
    })
    .await;
    match result {
        Ok(Ok((before, after))) => {
            tracing::info!(
                "Weekly database maintenance: incremental_vacuum reclaimed {} page(s) ({} -> {})",
                before.saturating_sub(after),
                before,
                after
            );
        }
        Ok(Err(err)) => tracing::warn!("db-maintenance incremental_vacuum failed: {err}"),
        Err(err) => tracing::error!("db-maintenance spawn failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_db_path;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn run_incremental_vacuum_if_due_leaves_the_db_in_incremental_autovacuum_mode() {
        let db_path = temp_db_path("db-maintenance-scheduler");
        let folder = db_path.parent().expect("db path has a parent dir");
        let initialized = boogiebox_db::init_db(folder).expect("init test db");
        let db: DbPool = Arc::new(Mutex::new(initialized.connection));

        // Freshly initialized DB just ran the one-time autovacuum-enabling VACUUM, so
        // `incremental_vacuum_due` depends purely on the current local hour; either branch
        // must complete without panicking or leaving the db in a broken state.
        run_incremental_vacuum_if_due(&db).await;

        let conn = db.lock().unwrap();
        // Regardless of whether it ran, `auto_vacuum` must remain INCREMENTAL.
        let mode: i64 = conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, 2);
    }
}

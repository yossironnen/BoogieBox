//! `getScanStatus` (any user) and `startScan` (admins and library managers): the same library scans the
//! BoogieBox UI starts.

use boogiebox_db::subsonic::{self as sdb, ScanStatus};

use super::{
    handlers::{db_err, Ctx},
    model,
    response::{Node, SubsonicError},
};

fn status_payload(status: &ScanStatus) -> Node {
    Node::new("payload").object(
        Node::new("scanStatus")
            .attr("scanning", status.scanning)
            .attr("count", status.count)
            .opt("lastScan", status.last_scan.as_deref().map(model::iso)),
    )
}

/// `getScanStatus`.
pub async fn scan_status(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let status = ctx
        .db(|conn| sdb::scan_status(conn).map_err(db_err))
        .await?;
    Ok(status_payload(&status))
}

/// `startScan`: queues a scan of every library (an existing pending/running scan is reused) and runs them in
/// turn. `fullScan` is accepted and ignored: BoogieBox scans are incremental.
pub async fn start_scan(ctx: &Ctx) -> Result<Node, SubsonicError> {
    if !ctx.user.is_admin() && !ctx.user.can_manage_libraries {
        return Err(SubsonicError::not_authorized());
    }
    let (jobs, status) = ctx
        .db(|conn| {
            let mut jobs = Vec::new();
            for (library_id, _) in sdb::music_folders(conn).map_err(db_err)? {
                let job =
                    boogiebox_db::jobs::enqueue_scan_job(conn, &library_id).map_err(|err| {
                        tracing::error!(error = %err, "Subsonic startScan could not queue a scan");
                        SubsonicError::generic("Could not start the scan")
                    })?;
                jobs.push(job);
            }
            Ok((jobs, sdb::scan_status(conn).map_err(db_err)?))
        })
        .await?;
    if !jobs.is_empty() {
        if let Some(worker) = crate::scanner::post_scan_state(&ctx.state) {
            crate::scanner::spawn_scan_jobs(worker, jobs);
        }
    }
    Ok(status_payload(&status))
}

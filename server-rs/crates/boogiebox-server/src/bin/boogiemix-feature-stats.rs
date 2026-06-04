//! Defines the Boogiemix Feature Stats command-line utility for BoogieBox server maintenance.

// BoogieMix deep-analysis feature-size validation harness (Phase 7).
//
// Usage:
//   cargo run --release -p boogiebox-server --bin boogiemix-feature-stats -- \
//     --db "C:\\path\\to\\boogiebox.db"
//
// Reports BoogieMix deep-analysis cache health for the configured library:
//   - total tracks
//   - deep-analyzed track count
//   - per-track feature-size min / p50 / p90 / max
//   - cache total size
//   - playlist/mix-job summary
//   - SQLite page-derived total DB size
//
// This binary opens the SQLite file directly (read-only) and does NOT modify it.

use rusqlite::{Connection, OpenFlags};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const DB_PATH_ENV: &str = "BOOGIEBOX_DB_PATH";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let mut db_path = env::var_os(DB_PATH_ENV).map(PathBuf::from);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                if i + 1 >= args.len() {
                    eprintln!("--db requires a path argument");
                    return ExitCode::from(2);
                }
                db_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--help" | "-h" => {
                println!(
                    "boogiemix-feature-stats --db <sqlite-path>\n\
                     \n\
                     Reports BoogieMix deep-analysis feature-size and cache statistics.\n\
                     Pass --db <sqlite-path> or set BOOGIEBOX_DB_PATH."
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let Some(db_path) = db_path else {
        eprintln!("Missing DB path. Pass --db <sqlite-path> or set {DB_PATH_ENV}.");
        return ExitCode::from(2);
    };

    if !db_path.exists() {
        eprintln!("DB not found: {}", db_path.display());
        return ExitCode::from(1);
    }

    let conn = match Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Failed to open DB read-only: {err}");
            return ExitCode::from(1);
        }
    };

    println!("BoogieMix deep-analysis feature-size report");
    println!("DB: {}", db_path.display());
    println!();

    if let Err(err) = print_overall_size(&conn) {
        eprintln!("warn: overall size query failed: {err}");
    }
    if let Err(err) = print_track_counts(&conn) {
        eprintln!("warn: track count query failed: {err}");
    }
    if let Err(err) = print_deep_analysis_stats(&conn) {
        eprintln!("warn: deep-analysis stats query failed: {err}");
    }
    if let Err(err) = print_mix_job_breakdown(&conn) {
        eprintln!("warn: mix-job query failed: {err}");
    }

    ExitCode::SUCCESS
}

fn print_overall_size(conn: &Connection) -> rusqlite::Result<()> {
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let bytes = page_size * page_count;
    println!("--- SQLite ---");
    println!("page_size  : {page_size}");
    println!("page_count : {page_count}");
    println!("total      : {} bytes ({:.2} MB)", bytes, mb(bytes));
    println!();
    Ok(())
}

fn print_track_counts(conn: &Connection) -> rusqlite::Result<()> {
    let tracks: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0);
    let playlists: i64 = conn
        .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
        .unwrap_or(0);
    let libraries: i64 = conn
        .query_row("SELECT COUNT(*) FROM libraries", [], |r| r.get(0))
        .unwrap_or(0);
    println!("--- Library ---");
    println!("libraries  : {libraries}");
    println!("tracks     : {tracks}");
    println!("playlists  : {playlists}");
    println!();
    Ok(())
}

fn print_deep_analysis_stats(conn: &Connection) -> rusqlite::Result<()> {
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='track_deep_analysis'",
        [],
        |r| r.get(0),
    )?;
    if has_table == 0 {
        println!("--- Deep analysis ---");
        println!("track_deep_analysis table not present");
        println!();
        return Ok(());
    }

    let analyzed: i64 = conn
        .query_row("SELECT COUNT(*) FROM track_deep_analysis", [], |r| r.get(0))
        .unwrap_or(0);
    let total_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(feature_size_bytes), 0) FROM track_deep_analysis",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut sizes: Vec<i64> = Vec::with_capacity(analyzed.max(0) as usize);
    if analyzed > 0 {
        let mut stmt = conn.prepare(
            "SELECT feature_size_bytes FROM track_deep_analysis ORDER BY feature_size_bytes ASC",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        for v in rows.flatten() {
            sizes.push(v);
        }
    }

    println!("--- Deep analysis ---");
    println!("analyzed tracks   : {analyzed}");
    println!(
        "cache total bytes : {} ({:.2} MB)",
        total_bytes,
        mb(total_bytes)
    );

    if !sizes.is_empty() {
        let min = *sizes.first().unwrap();
        let max = *sizes.last().unwrap();
        let p50 = sizes[sizes.len() / 2];
        let p90 = sizes[(sizes.len() * 9) / 10];
        let avg: f64 = sizes.iter().map(|&v| v as f64).sum::<f64>() / sizes.len() as f64;
        println!("feature_size_bytes:");
        println!("  min  : {min}");
        println!("  p50  : {p50}");
        println!("  p90  : {p90}");
        println!("  max  : {max}");
        println!("  avg  : {avg:.0}");
        let over_target = sizes.iter().filter(|&&v| v > 20_000).count();
        let cap_violations = sizes.iter().filter(|&&v| v > 64_000).count();
        println!("  >20KB: {over_target}");
        println!("  >64KB: {cap_violations} (hard-cap violations)");
    } else {
        println!("(no deep-analysis rows yet)");
    }

    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let running: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status='running'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let failed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status='failed'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let skipped: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status='skipped'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deep_analysis_jobs WHERE status='done'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!(
        "queue: pending={pending} running={running} failed={failed} skipped={skipped} done={done}"
    );
    println!();
    Ok(())
}

fn print_mix_job_breakdown(conn: &Connection) -> rusqlite::Result<()> {
    let has_table: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='mix_jobs'",
        [],
        |r| r.get(0),
    )?;
    if has_table == 0 {
        return Ok(());
    }
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM mix_jobs", [], |r| r.get(0))
        .unwrap_or(0);
    let high_quality: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mix_jobs WHERE mix_quality='high_quality'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let used_deep: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mix_jobs WHERE used_deep_analysis=1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mix_jobs WHERE status='done'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!("--- Mix jobs ---");
    println!("total              : {total}");
    println!("done               : {done}");
    println!("requested HQ       : {high_quality}");
    println!("consumed deep rows : {used_deep}");
    println!();
    Ok(())
}

fn mb(bytes: i64) -> f64 {
    bytes as f64 / 1_048_576.0
}

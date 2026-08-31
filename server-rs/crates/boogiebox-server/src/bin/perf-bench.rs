//! Read-only performance benchmark harness for the large-dataset album/artwork
//! performance plan (see `wip/large-dataset-album-artwork-performance-plan.md`).
//!
//! Opens the supplied database read-only (never writes to it) and reports:
//! - `list_albums_latest` / `list_albums` SQL timing and row counts;
//! - artwork cache coverage (original/300/800) for grouped albums;
//! - cached-art path resolution/read latency;
//! - missing-art discovery latency without contacting providers;
//! - simulated `Arc<Mutex<Connection>>` wait time while a scanner-style batch
//!   transaction holds the lock, approximating the production contention
//!   pattern described in the plan's Phase 0.
//!
//! Usage: `perf-bench <db-path> [--iterations N] [--limit N] [--art-root PATH]`

use boogiebox_server::artwork_cache::{
    build_album_art_cache_key, cache_item_dir, find_existing_cached_image, find_folder_cover_image,
    get_assigned_cache_file_path,
};
use rusqlite::{Connection, OpenFlags};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ALBUM_THUMB_SIZES: &[u32] = &[300, 800];

struct Args {
    db_path: PathBuf,
    iterations: u32,
    limit: i64,
    art_root: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = env::args().skip(1).collect();
    let db_path = raw.first().cloned().map(PathBuf::from).ok_or_else(|| {
        "usage: perf-bench <db-path> [--iterations N] [--limit N] [--art-root PATH]".to_string()
    })?;

    let mut iterations: u32 = 20;
    let mut limit: i64 = 60;
    let mut art_root: Option<PathBuf> = None;

    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--iterations" => {
                i += 1;
                iterations = raw
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(iterations);
            }
            "--limit" => {
                i += 1;
                limit = raw.get(i).and_then(|s| s.parse().ok()).unwrap_or(limit);
            }
            "--art-root" => {
                i += 1;
                art_root = raw.get(i).map(PathBuf::from);
            }
            _ => {}
        }
        i += 1;
    }

    let art_root = art_root.unwrap_or_else(|| {
        db_path
            .parent()
            .map(|p| p.join("art").join("album"))
            .unwrap_or_else(|| PathBuf::from("art/album"))
    });

    Ok(Args {
        db_path,
        iterations,
        limit,
        art_root,
    })
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
}

fn avg_min_max(samples: &[Duration]) -> (f64, f64, f64) {
    let ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    let sum: f64 = ms.iter().sum();
    let avg = sum / ms.len() as f64;
    let min = ms.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = ms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (avg, min, max)
}

fn bench_albums_latest(conn: &Connection, iterations: u32, limit: i64) {
    let mut samples = Vec::with_capacity(iterations as usize);
    let mut row_count = 0usize;
    for _ in 0..iterations {
        let start = Instant::now();
        let rows = boogiebox_db::music::list_albums_latest(conn, "bench", limit)
            .expect("list_albums_latest failed");
        samples.push(start.elapsed());
        row_count = rows.len();
    }
    let (avg, min, max) = avg_min_max(&samples);
    println!(
        "list_albums_latest(limit={limit}): rows={row_count} avg={avg:.2}ms min={min:.2}ms max={max:.2}ms over {iterations} iterations"
    );
}

fn bench_albums_full(conn: &Connection, iterations: u32) {
    let mut samples = Vec::with_capacity(iterations as usize);
    let mut row_count = 0usize;
    for _ in 0..iterations {
        let start = Instant::now();
        let rows = boogiebox_db::music::list_albums(
            conn,
            boogiebox_db::music::ListAlbumsParams {
                user_id: "bench",
                library_ids: &[],
                genres: &[],
                by_album_artist: true,
                sonic_fingerprint_only: false,
                after_album_rowid: None,
                through_album_rowid: None,
            },
        )
        .expect("list_albums failed");
        samples.push(start.elapsed());
        row_count = rows.len();
    }
    let (avg, min, max) = avg_min_max(&samples);
    println!(
        "list_albums (grouped, unfiltered): rows={row_count} avg={avg:.2}ms min={min:.2}ms max={max:.2}ms over {iterations} iterations"
    );
}

/// Representative (album_id, file_path) pairs for every grouped album, mirroring
/// the `(title, album_artist)` grouping used by the browse/latest queries.
fn grouped_album_representatives(conn: &Connection) -> Vec<(String, String)> {
    let sql = "SELECT MIN(al.id), MIN(t.file_path)
               FROM albums al JOIN tracks t ON t.album_id = al.id
               GROUP BY al.title, COALESCE(al.album_artist,'')";
    let mut stmt = conn.prepare(sql).expect("prepare grouped albums");
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .expect("query grouped albums")
    .filter_map(Result::ok)
    .collect()
}

fn bench_artwork_coverage(art_root: &Path, albums: &[(String, String)]) {
    let original_root = art_root.join("original");
    let mut original_hits = 0usize;
    let mut thumb_hits = [0usize; 2];

    let start = Instant::now();
    for (album_id, _file_path) in albums {
        let cache_key = build_album_art_cache_key(album_id);
        let item_dir = cache_item_dir(&original_root, &cache_key);
        if find_existing_cached_image(&item_dir).is_some() {
            original_hits += 1;
        }
        for (idx, size) in ALBUM_THUMB_SIZES.iter().enumerate() {
            let thumb_root = art_root.join("thumb").join(size.to_string());
            let slot = format!("thumb-{size}");
            if let Some(p) =
                get_assigned_cache_file_path(&thumb_root, &cache_key, &slot, ".jpg", false)
            {
                if p.is_file() {
                    thumb_hits[idx] += 1;
                }
            }
        }
    }
    let elapsed = start.elapsed();

    println!(
        "artwork coverage: original {}/{}  300px {}/{}  800px {}/{}  (path resolution/read: {:.2}ms total, {:.3}ms/album)",
        original_hits,
        albums.len(),
        thumb_hits[0],
        albums.len(),
        thumb_hits[1],
        albums.len(),
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / albums.len().max(1) as f64
    );
}

/// Measures missing-art discovery latency (folder.jpg probing only — no provider
/// network calls) for the albums missing a cached original among the latest N.
fn bench_missing_art_discovery(art_root: &Path, latest_albums: &[(String, String)]) {
    let original_root = art_root.join("original");
    let mut missing = Vec::new();
    for (album_id, file_path) in latest_albums {
        let cache_key = build_album_art_cache_key(album_id);
        let item_dir = cache_item_dir(&original_root, &cache_key);
        if find_existing_cached_image(&item_dir).is_none() {
            missing.push(file_path.clone());
        }
    }

    let start = Instant::now();
    let mut found_via_folder = 0usize;
    for file_path in &missing {
        if find_folder_cover_image(Path::new(file_path)).is_some() {
            found_via_folder += 1;
        }
    }
    let elapsed = start.elapsed();

    println!(
        "missing-art discovery (no providers): {}/{} latest albums missing cached original; {found_via_folder} resolvable via folder.jpg; probing took {:.2}ms total",
        missing.len(),
        latest_albums.len(),
        elapsed.as_secs_f64() * 1000.0
    );
}

/// Simulates the production `Arc<Mutex<Connection>>` contention pattern: a
/// background thread holds the lock for `hold_for` (approximating a scanner
/// batch-insert transaction, see scanner.rs's `BEGIN`..commit span), while the
/// main thread measures how long it waits to acquire the same lock and run a
/// representative query.
fn bench_mutex_contention(db_path: &Path, hold_for: Duration, limit: i64) {
    let conn = open_read_only(db_path).expect("open read-only connection for contention sim");
    let shared = Arc::new(Mutex::new(conn));

    let holder = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let _guard = shared.lock().unwrap_or_else(|p| p.into_inner());
            std::thread::sleep(hold_for);
        })
    };

    // Give the holder thread a head start so it acquires the lock first.
    std::thread::sleep(Duration::from_millis(20));

    let wait_start = Instant::now();
    let conn = shared.lock().unwrap_or_else(|p| p.into_inner());
    let mutex_wait = wait_start.elapsed();

    let query_start = Instant::now();
    let rows =
        boogiebox_db::music::list_albums_latest(&conn, "bench", limit).expect("query under lock");
    let query_time = query_start.elapsed();

    drop(conn);
    holder.join().ok();

    println!(
        "mutex contention (simulated {:.0}ms scanner-batch hold): wait={:.2}ms query={:.2}ms rows={} total={:.2}ms",
        hold_for.as_secs_f64() * 1000.0,
        mutex_wait.as_secs_f64() * 1000.0,
        query_time.as_secs_f64() * 1000.0,
        rows.len(),
        (mutex_wait + query_time).as_secs_f64() * 1000.0
    );
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    println!("BoogieBox perf-bench (read-only; no writes performed)");
    println!("db: {}", args.db_path.display());
    println!("art root: {}", args.art_root.display());
    println!();

    let conn = match open_read_only(&args.db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to open {} read-only: {e}", args.db_path.display());
            std::process::exit(1);
        }
    };

    bench_albums_latest(&conn, args.iterations, args.limit);
    bench_albums_full(&conn, args.iterations);
    println!();

    let grouped = grouped_album_representatives(&conn);
    bench_artwork_coverage(&args.art_root, &grouped);

    let latest_ids: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT MIN(al.id) FROM albums al JOIN tracks t ON t.album_id = al.id
                 GROUP BY al.title, COALESCE(al.album_artist,'')
                 ORDER BY MIN(al.added_at) DESC LIMIT ?",
            )
            .expect("prepare latest ids");
        stmt.query_map([args.limit], |row| row.get::<_, String>(0))
            .expect("query latest ids")
            .filter_map(Result::ok)
            .collect()
    };
    let latest_set: std::collections::HashSet<&str> =
        latest_ids.iter().map(String::as_str).collect();
    let latest_pairs: Vec<(String, String)> = grouped
        .iter()
        .filter(|(id, _)| latest_set.contains(id.as_str()))
        .cloned()
        .collect();
    bench_missing_art_discovery(&args.art_root, &latest_pairs);
    println!();

    bench_mutex_contention(&args.db_path, Duration::from_millis(250), args.limit);
}

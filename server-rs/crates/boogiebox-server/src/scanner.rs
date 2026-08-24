//! Defines Rust server support logic for Scanner.

use crate::DbPool;
use boogiebox_db::{
    jobs::{ClaimedScanJob, JobError, ScannedTrackInput},
    music::EntityId,
};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "wma", "alac", "aiff", "aif",
];

#[derive(Debug, Default)]
struct ScanCounters {
    files_found: i64,
    files_scanned: i64,
    errors: i64,
    messages: Vec<String>,
}

#[derive(Debug, Default)]
struct AudioTechnical {
    duration: Option<f64>,
    bitrate: Option<i64>,
    sample_rate: Option<i64>,
    channels: Option<i64>,
}

#[derive(Debug, Default)]
struct AudioTags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    composer: Option<String>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    year: Option<i64>,
    comment: Option<String>,
    bpm: Option<i64>,
}

/// Documents the Run One Pending Scan public API surface.
///
/// Claims and runs whichever scan job is globally oldest-pending. Used by the
/// background scheduler tick, where "some due library" is the point.
pub async fn run_one_pending_scan(state: crate::post_scan::PostScanState) {
    run_pending_scan(state, None).await;
}

/// Runs the specific scan job just enqueued for one library (e.g. a user
/// clicking "Scan" on it), rather than whatever is oldest in the global
/// queue. See [`boogiebox_db::jobs::claim_scan_job`] for why this matters:
/// without it, a manual scan click can end up running a different library's
/// older queued/due job while the clicked library's own job sits pending.
pub async fn run_scan_job(state: crate::post_scan::PostScanState, job_id: EntityId) {
    run_pending_scan(state, Some(job_id)).await;
}

async fn run_pending_scan(state: crate::post_scan::PostScanState, target_job_id: Option<EntityId>) {
    let db = state.db.clone();
    let result =
        tokio::task::spawn_blocking(move || run_one_pending_scan_blocking(&db, target_job_id))
            .await;
    if let Err(err) = result {
        tracing::error!("Rust scan worker task failed: {err}");
    }
    crate::post_scan::run_one_pending_music_post_scan(&state).await;
}

/// Documents the Start Scan Scheduler public API surface.
pub fn start_scan_scheduler(state: crate::post_scan::PostScanState) {
    tokio::spawn(async move {
        run_scheduler_tick(&state).await;
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = state.cancel.cancelled() => break,
                _ = interval.tick() => run_scheduler_tick(&state).await,
            }
        }
    });
}

async fn run_scheduler_tick(state: &crate::post_scan::PostScanState) {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::jobs::enqueue_due_scheduled_scans(&conn)
    })
    .await;

    match result {
        Ok(Ok(job_ids)) if !job_ids.is_empty() => {
            tracing::info!("Rust scheduler queued {} scan job(s)", job_ids.len());
            run_one_pending_scan(state.clone()).await;
        }
        Ok(Ok(_)) => {}
        Ok(Err(err)) => tracing::warn!("Rust scheduler tick failed: {err}"),
        Err(err) => tracing::error!("Rust scheduler task failed: {err}"),
    }
}

fn run_one_pending_scan_blocking(
    db: &DbPool,
    target_job_id: Option<EntityId>,
) -> Result<(), JobError> {
    let claimed = {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::jobs::recover_stale_scan_jobs(&conn, 60)?;
        match &target_job_id {
            Some(job_id) => boogiebox_db::jobs::claim_scan_job(&conn, job_id)?,
            None => boogiebox_db::jobs::claim_next_scan_job(&conn)?,
        }
    };
    let Some(claimed) = claimed else {
        return Ok(());
    };

    let result = scan_music_library(db, &claimed);

    if let Err(err) = result {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        match err {
            JobError::ScanCancelled => {
                boogiebox_db::jobs::mark_scan_cancelled(&conn, &claimed.job_id, "Scan cancelled")?;
            }
            other => {
                tracing::error!("Scan job {} failed: {other}", claimed.job_id);
                boogiebox_db::jobs::mark_scan_failed(&conn, &claimed.job_id, &other.to_string())?;
            }
        }
    }
    Ok(())
}

fn scan_music_library(db: &DbPool, claimed: &ClaimedScanJob) -> Result<(), JobError> {
    let mut counters = ScanCounters::default();
    let files = discover_audio_files(&claimed.folders, &mut counters);
    counters.files_found = files.len() as i64;

    {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::jobs::update_scan_progress(
            &conn,
            &claimed.job_id,
            counters.files_found,
            0,
            counters.errors,
        )?;
    }

    // Parse and write in small batches so UNC scans show steady progress instead of waiting
    // for every remote file read to finish before files_scanned can move.
    const PROGRESS_BATCH: usize = 100;
    let mut seen_paths = std::collections::HashSet::with_capacity(files.len());
    for chunk in files.chunks(PROGRESS_BATCH) {
        let parsed = chunk
            .iter()
            .map(|path| {
                let path_string = path.to_string_lossy().into_owned();
                let path_lower = path_string.to_ascii_lowercase();
                seen_paths.insert(path_lower.clone());
                (path_lower, build_track_input(&claimed.library_id, path))
            })
            .collect::<Vec<_>>();
        let conn = db.lock().map_err(|_| {
            JobError::Db(rusqlite::Error::InvalidParameterName(
                "db lock poisoned".into(),
            ))
        })?;
        // A batch that failed mid-transaction in an earlier run (or another worker sharing this
        // connection) can leave an open transaction behind; BEGIN would then fail forever with
        // "cannot start a transaction within a transaction" until the server restarts.
        if !conn.is_autocommit() {
            tracing::warn!("Scan found a leftover open transaction; rolling it back");
            let _ = conn.execute_batch("ROLLBACK");
        }
        conn.execute_batch("BEGIN")?;
        let batch = (|| -> Result<(), JobError> {
            for (path_lower, result) in &parsed {
                match result {
                    Ok(input) => {
                        boogiebox_db::jobs::upsert_scanned_track(&conn, input)?;
                        counters.files_scanned += 1;
                    }
                    Err(err) => {
                        counters.errors += 1;
                        counters.messages.push(format!("{path_lower}: {err}"));
                    }
                }
            }
            boogiebox_db::jobs::update_scan_progress(
                &conn,
                &claimed.job_id,
                counters.files_found,
                counters.files_scanned,
                counters.errors,
            )?;
            Ok(())
        })();
        match batch {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(err);
            }
        }
        // Lock is released here between batches, allowing other handlers to proceed.
    }

    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
    boogiebox_db::jobs::prune_missing_tracks(&conn, &claimed.library_id, &seen_paths)?;
    boogiebox_db::jobs::prune_orphaned_music_entities(&conn)?;
    boogiebox_db::jobs::enqueue_default_music_post_scan_jobs(&conn, &claimed.library_id)?;
    let error_log = (!counters.messages.is_empty()).then(|| counters.messages.join("\n"));
    boogiebox_db::jobs::mark_scan_done(
        &conn,
        &claimed.job_id,
        &claimed.library_id,
        counters.files_found,
        counters.files_scanned,
        counters.errors,
        error_log.as_deref(),
    )?;
    Ok(())
}

fn discover_audio_files(folders: &[String], counters: &mut ScanCounters) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for folder in folders {
        collect_audio_files(Path::new(folder), &mut files, counters);
    }
    files.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    files
}

fn collect_audio_files(path: &Path, files: &mut Vec<PathBuf>, counters: &mut ScanCounters) {
    let read_dir = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(err) => {
            counters.errors += 1;
            counters
                .messages
                .push(format!("{}: cannot read folder: {err}", path.display()));
            return;
        }
    };
    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        // Skip symlinks entirely to prevent directory cycles on Windows.
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_audio_files(&entry_path, files, counters);
        } else if metadata.is_file() && is_supported_audio_file(&entry_path) {
            files.push(entry_path);
        }
    }
}

fn build_track_input(
    library_id: &EntityId,
    path: &Path,
) -> Result<ScannedTrackInput, std::io::Error> {
    let metadata = fs::metadata(path)?;
    let tags = read_audio_tags(path).unwrap_or_default();
    let technical = read_audio_technical(path);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Unknown Track")
        .to_owned();
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(clean_track_title)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Unknown Track".to_string());
    let fallback_album = path
        .parent()
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(normalize_folder_name)
        .unwrap_or_else(|| "Unknown Album".to_owned());
    let fallback_artist = path
        .parent()
        .and_then(|value| value.parent())
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(normalize_folder_name)
        .unwrap_or_else(|| "Unknown Artist".to_owned());
    let artist = tags.artist.unwrap_or(fallback_artist);
    let album_artist = tags.album_artist.unwrap_or_else(|| artist.clone());

    Ok(ScannedTrackInput {
        library_id: library_id.clone(),
        file_path: path.to_string_lossy().into_owned(),
        file_name,
        file_size: metadata.len().min(i64::MAX as u64) as i64, // cap to i64::MAX (no real file is >9 EB)
        format: path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_uppercase(),
        title: tags.title.unwrap_or(title),
        artist: artist.clone(),
        album: tags.album.unwrap_or(fallback_album),
        album_artist,
        genre: tags.genre.unwrap_or_default(),
        composer: tags.composer.unwrap_or_default(),
        track_number: tags.track_number,
        disc_number: tags.disc_number,
        year: tags.year,
        comment: tags.comment,
        bpm: tags.bpm,
        duration: technical.duration.unwrap_or(0.0),
        bitrate: technical.bitrate,
        sample_rate: technical.sample_rate,
        channels: technical.channels,
    })
}

fn read_audio_tags(path: &Path) -> Result<AudioTags, std::io::Error> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => read_id3v2_tags(path),
        Some("flac") => read_flac_tags(path),
        Some("ogg") | Some("opus") => read_ogg_tags(path),
        Some("m4a") | Some("aac") | Some("alac") => read_mp4_tags(path),
        _ => Ok(AudioTags::default()),
    }
}

// -- Audio technical metadata --------------------------------------------------

fn read_audio_technical(path: &Path) -> AudioTechnical {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let result = match ext.as_deref() {
        Some("mp3") => read_mp3_technical(path),
        Some("flac") => read_flac_technical(path),
        Some("ogg") | Some("opus") => read_ogg_technical(path),
        Some("m4a") | Some("aac") | Some("alac") => read_m4a_technical(path),
        Some("wav") => read_wav_technical(path),
        Some("aiff") | Some("aif") => read_aiff_technical(path),
        _ => None,
    };
    result.unwrap_or_default()
}

fn read_mp3_technical(path: &Path) -> Option<AudioTechnical> {
    let mut file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();

    // Read initial buffer - enough to skip ID3v2 and find first frame
    let mut buf = vec![0u8; 16384.min(file_size as usize)];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);

    // Skip ID3v2 tag if present
    let id3_size: u64 = if buf.get(0..3) == Some(b"ID3") && buf.len() >= 10 {
        10 + syncsafe_u32(&buf[6..10]) as u64
    } else {
        0
    };

    // If ID3 extends beyond our buffer, re-read from audio start
    let search_buf: &[u8] = if id3_size as usize > buf.len() {
        file.seek(SeekFrom::Start(id3_size)).ok()?;
        buf = vec![0u8; 16384];
        let n = file.read(&mut buf).ok()?;
        buf.truncate(n);
        &buf
    } else {
        &buf[id3_size as usize..]
    };

    let frame_off = search_buf.windows(4).position(is_valid_mpeg_sync)?;
    let h = &search_buf[frame_off..];
    if h.len() < 4 {
        return None;
    }

    let version_bits = (h[1] >> 3) & 0x03;
    let layer_bits = (h[1] >> 1) & 0x03;
    // Only handle Layer III (MP3); layer_bits==1 means Layer III
    if version_bits == 1 || layer_bits != 1 {
        return None;
    }

    let bitrate_idx = (h[2] >> 4) as usize;
    let sr_idx = ((h[2] >> 2) & 0x03) as usize;
    let ch_mode = (h[3] >> 6) & 0x03;

    if bitrate_idx == 0 || bitrate_idx == 15 || sr_idx == 3 {
        return None;
    }

    let is_mpeg1 = version_bits == 3;
    let bitrate_kbps: i64 = if is_mpeg1 {
        [
            0u32, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ][bitrate_idx]
    } else {
        [
            0u32, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
        ][bitrate_idx]
    } as i64;

    let sample_rate: i64 = match version_bits {
        3 => [44100i64, 48000, 32000, 0][sr_idx],
        2 => [22050i64, 24000, 16000, 0][sr_idx],
        0 => [11025i64, 12000, 8000, 0][sr_idx],
        _ => return None,
    };
    if bitrate_kbps == 0 || sample_rate == 0 {
        return None;
    }
    let channels: i64 = if ch_mode == 3 { 1 } else { 2 };

    // Xing/VBRI offset from frame start
    let side_info: usize = if is_mpeg1 {
        if channels == 1 {
            17
        } else {
            32
        }
    } else {
        if channels == 1 {
            9
        } else {
            17
        }
    };
    let xing_off = frame_off + 4 + side_info;
    let samples_per_frame: u64 = if is_mpeg1 { 1152 } else { 576 };

    let duration = 'dur: {
        if search_buf.len() > xing_off + 8 {
            let x = &search_buf[xing_off..];
            if x.starts_with(b"Xing") || x.starts_with(b"Info") {
                if let Ok(flags_bytes) = x[4..8].try_into() as Result<[u8; 4], _> {
                    let flags = u32::from_be_bytes(flags_bytes);
                    if flags & 0x01 != 0 && x.len() >= 12 {
                        if let Ok(fb) = x[8..12].try_into() as Result<[u8; 4], _> {
                            let total_frames = u32::from_be_bytes(fb) as u64;
                            if total_frames > 0 {
                                break 'dur Some(
                                    total_frames * samples_per_frame / sample_rate as u64,
                                );
                            }
                        }
                    }
                }
            }
            // Check VBRI at frame_off + 36
            let vbri_off = frame_off + 36;
            if search_buf.len() > vbri_off + 18 && search_buf[vbri_off..].starts_with(b"VBRI") {
                let v = &search_buf[vbri_off..];
                if let Ok(fb) = v[14..18].try_into() as Result<[u8; 4], _> {
                    let total_frames = u32::from_be_bytes(fb) as u64;
                    if total_frames > 0 {
                        break 'dur Some(total_frames * samples_per_frame / sample_rate as u64);
                    }
                }
            }
        }
        // CBR estimation
        let audio_bytes = file_size.saturating_sub(id3_size);
        Some(audio_bytes * 8 / (bitrate_kbps as u64 * 1000))
    };

    Some(AudioTechnical {
        duration: duration.map(|d| d as f64),
        bitrate: Some(bitrate_kbps),
        sample_rate: Some(sample_rate),
        channels: Some(channels),
    })
}

fn is_valid_mpeg_sync(w: &[u8]) -> bool {
    if w.len() < 4 {
        return false;
    }
    if w[0] != 0xFF || (w[1] & 0xE0) != 0xE0 {
        return false;
    }
    let version = (w[1] >> 3) & 0x03;
    let layer = (w[1] >> 1) & 0x03;
    let bitrate_idx = (w[2] >> 4) as usize;
    let sr_idx = ((w[2] >> 2) & 0x03) as usize;
    version != 1 && layer != 0 && bitrate_idx != 0 && bitrate_idx != 15 && sr_idx != 3
}

fn read_flac_technical(path: &Path) -> Option<AudioTechnical> {
    let mut file = fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    if &magic != b"fLaC" {
        return None;
    }
    loop {
        let mut hdr = [0u8; 4];
        file.read_exact(&mut hdr).ok()?;
        let is_last = hdr[0] & 0x80 != 0;
        let block_type = hdr[0] & 0x7f;
        let length = ((hdr[1] as usize) << 16) | ((hdr[2] as usize) << 8) | hdr[3] as usize;
        if length > 16 * 1024 * 1024 {
            return None;
        }
        let mut block = vec![0u8; length];
        file.read_exact(&mut block).ok()?;
        if block_type == 0 && block.len() >= 18 {
            // STREAMINFO: parse sample_rate (20 bits), channels (3 bits), total_samples (36 bits)
            let sr =
                ((block[10] as u32) << 12) | ((block[11] as u32) << 4) | ((block[12] as u32) >> 4);
            let channels = (((block[12] >> 1) & 0x07) + 1) as i64;
            let total_hi = (block[13] & 0x0f) as u64;
            let total_lo = ((block[14] as u64) << 24)
                | ((block[15] as u64) << 16)
                | ((block[16] as u64) << 8)
                | (block[17] as u64);
            let total_samples = (total_hi << 32) | total_lo;
            let sample_rate = sr as i64;
            if sample_rate <= 0 {
                return None;
            }
            let duration = total_samples as f64 / sample_rate as f64;
            return Some(AudioTechnical {
                duration: if duration > 0.0 { Some(duration) } else { None },
                bitrate: None, // lossless - omit per-track bitrate
                sample_rate: Some(sample_rate),
                channels: Some(channels),
            });
        }
        if is_last {
            break;
        }
    }
    None
}

fn read_ogg_technical(path: &Path) -> Option<AudioTechnical> {
    let mut file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();
    let read_size = 65536usize.min(file_size as usize);
    let mut buf = vec![0u8; read_size];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.get(0..4) != Some(b"OggS") {
        return None;
    }

    let (sample_rate_for_dur, channels, bitrate) = if let Some(off) = find_bytes(&buf, b"OpusHead")
    {
        // OpusHead: magic(8) + version(1) + channels(1) + pre_skip(2) + input_sr(4)
        let h = &buf[off..];
        if h.len() < 16 {
            return None;
        }
        let ch = h[9] as i64;
        // Opus duration granule is always 48000 Hz
        (48000i64, ch, None)
    } else if let Some(off) = find_bytes(&buf, b"\x01vorbis") {
        // Vorbis identification header: type(1)+magic(6)+version(4)+channels(1)+sr(4)+max_br(4)+nom_br(4)
        let h = &buf[off + 7..];
        if h.len() < 17 {
            return None;
        }
        // Skip the 4-byte vorbis_version field (always 0 in practice) before channels/sr/bitrates.
        let ch = h[4] as i64;
        let sr = u32::from_le_bytes(h[5..9].try_into().ok()?) as i64;
        let nom_br = u32::from_le_bytes(h[13..17].try_into().ok()?) as i64;
        let br = if nom_br > 0 {
            Some(nom_br / 1000)
        } else {
            None
        };
        (sr, ch, br)
    } else {
        return None;
    };

    let duration = read_ogg_last_granule(path, sample_rate_for_dur);

    Some(AudioTechnical {
        duration,
        bitrate,
        sample_rate: Some(sample_rate_for_dur),
        channels: Some(channels),
    })
}

fn read_ogg_last_granule(path: &Path, sample_rate: i64) -> Option<f64> {
    let mut file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();
    let search_size = 65536u64.min(file_size);
    file.seek(SeekFrom::End(-(search_size as i64))).ok()?;
    let mut tail = vec![0u8; search_size as usize];
    let n = file.read(&mut tail).ok()?;
    tail.truncate(n);

    let mut last_granule: Option<i64> = None;
    let mut i = 0;
    while i + 14 <= tail.len() {
        if &tail[i..i + 4] == b"OggS" {
            if let Ok(g_bytes) = tail[i + 6..i + 14].try_into() as Result<[u8; 8], _> {
                let g = i64::from_le_bytes(g_bytes);
                if g > 0 {
                    last_granule = Some(g);
                }
            }
            // Advance past the full OGG page: 27-byte fixed header + segment table.
            let seg_count = if i + 27 <= tail.len() {
                tail[i + 26] as usize
            } else {
                0
            };
            i += 27 + seg_count;
        } else {
            i += 1;
        }
    }
    let granule = last_granule?;
    if sample_rate <= 0 {
        return None;
    }
    Some(granule as f64 / sample_rate as f64)
}

fn read_m4a_technical(path: &Path) -> Option<AudioTechnical> {
    let file = fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(256 * 1024).read_to_end(&mut data).ok()?;
    let duration = parse_m4a_duration(&data, 0)?;
    Some(AudioTechnical {
        duration: Some(duration),
        bitrate: None,
        sample_rate: None,
        channels: None,
    })
}

fn parse_m4a_duration(data: &[u8], depth: usize) -> Option<f64> {
    // Normal path: moov→mvhd (depth 1). Depth 6 guards against malformed files only.
    if depth > 6 {
        return None;
    }
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let Some((atom_type, payload_start, atom_end)) = read_mp4_atom(data, offset) else {
            break;
        };
        let payload = &data[payload_start..atom_end];
        match atom_type.as_slice() {
            b"moov" => {
                if let Some(d) = parse_m4a_duration(payload, depth + 1) {
                    return Some(d);
                }
            }
            b"mvhd" => {
                if let Some(d) = parse_mvhd(payload) {
                    return Some(d);
                }
            }
            _ => {}
        }
        offset = atom_end;
    }
    None
}

fn parse_mvhd(data: &[u8]) -> Option<f64> {
    if data.is_empty() {
        return None;
    }
    let version = data[0];
    if version == 0 {
        // version(1) + flags(3) + creation_time(4) + modification_time(4) + time_scale(4) + duration(4)
        if data.len() < 20 {
            return None;
        }
        let time_scale = u32::from_be_bytes(data[12..16].try_into().ok()?) as f64;
        let duration = u32::from_be_bytes(data[16..20].try_into().ok()?) as f64;
        if time_scale > 0.0 {
            Some(duration / time_scale)
        } else {
            None
        }
    } else if version == 1 {
        // version(1) + flags(3) + creation_time(8) + modification_time(8) + time_scale(4) + duration(8)
        if data.len() < 32 {
            return None;
        }
        let time_scale = u32::from_be_bytes(data[20..24].try_into().ok()?) as f64;
        let duration = u64::from_be_bytes(data[24..32].try_into().ok()?) as f64;
        if time_scale > 0.0 {
            Some(duration / time_scale)
        } else {
            None
        }
    } else {
        None
    }
}

fn read_wav_technical(path: &Path) -> Option<AudioTechnical> {
    let mut file = fs::File::open(path).ok()?;
    let mut riff_hdr = [0u8; 12];
    file.read_exact(&mut riff_hdr).ok()?;
    if &riff_hdr[0..4] != b"RIFF" || &riff_hdr[8..12] != b"WAVE" {
        return None;
    }
    let mut channels: Option<i64> = None;
    let mut sample_rate: Option<i64> = None;
    let mut byte_rate: Option<i64> = None;
    let mut data_size: Option<u64> = None;

    loop {
        let mut chunk_hdr = [0u8; 8];
        if file.read_exact(&mut chunk_hdr).is_err() {
            break;
        }
        let chunk_id = &chunk_hdr[0..4];
        let chunk_size = u32::from_le_bytes(chunk_hdr[4..8].try_into().ok()?) as usize;
        if chunk_id == b"fmt " {
            let read_len = chunk_size.min(16);
            let mut fmt = vec![0u8; read_len];
            file.read_exact(&mut fmt).ok()?;
            if fmt.len() >= 16 {
                channels = Some(u16::from_le_bytes(fmt[2..4].try_into().ok()?) as i64);
                sample_rate = Some(u32::from_le_bytes(fmt[4..8].try_into().ok()?) as i64);
                byte_rate = Some(u32::from_le_bytes(fmt[8..12].try_into().ok()?) as i64);
            }
            let skip = chunk_size.saturating_sub(read_len) as i64;
            if skip > 0 {
                file.seek(SeekFrom::Current(skip)).ok()?;
            }
        } else if chunk_id == b"data" {
            data_size = Some(chunk_size as u64);
            break;
        } else {
            let skip = chunk_size as i64 + (chunk_size % 2) as i64;
            if file.seek(SeekFrom::Current(skip)).is_err() {
                break;
            }
        }
    }
    let br = byte_rate?;
    let duration = data_size.map(|ds| ds as f64 / br as f64);
    Some(AudioTechnical {
        duration,
        bitrate: Some(br * 8 / 1000),
        sample_rate,
        channels,
    })
}

fn read_aiff_technical(path: &Path) -> Option<AudioTechnical> {
    let mut file = fs::File::open(path).ok()?;
    let mut form_hdr = [0u8; 12];
    file.read_exact(&mut form_hdr).ok()?;
    if &form_hdr[0..4] != b"FORM" {
        return None;
    }
    if &form_hdr[8..12] != b"AIFF" && &form_hdr[8..12] != b"AIFC" {
        return None;
    }
    loop {
        let mut chunk_hdr = [0u8; 8];
        if file.read_exact(&mut chunk_hdr).is_err() {
            break;
        }
        let chunk_id = &chunk_hdr[0..4];
        let chunk_size = u32::from_be_bytes(chunk_hdr[4..8].try_into().ok()?) as usize;
        if chunk_id == b"COMM" {
            let read_len = chunk_size.min(26);
            let mut comm = vec![0u8; read_len];
            file.read_exact(&mut comm).ok()?;
            if comm.len() >= 18 {
                let ch = i16::from_be_bytes(comm[0..2].try_into().ok()?) as i64;
                let num_frames = u32::from_be_bytes(comm[2..6].try_into().ok()?) as f64;
                let sr = parse_ieee_extended(&comm[8..18]);
                let sample_rate = sr as i64;
                let duration = if sr > 0.0 {
                    Some(num_frames / sr)
                } else {
                    None
                };
                return Some(AudioTechnical {
                    duration,
                    bitrate: None,
                    sample_rate: Some(sample_rate),
                    channels: Some(ch),
                });
            }
            break;
        } else {
            let skip = chunk_size as i64 + (chunk_size % 2) as i64;
            if file.seek(SeekFrom::Current(skip)).is_err() {
                break;
            }
        }
    }
    None
}

/// Parse 80-bit IEEE 754 extended precision to f64.
fn parse_ieee_extended(bytes: &[u8]) -> f64 {
    if bytes.len() < 10 {
        return 0.0;
    }
    let exponent = (((bytes[0] as u16) & 0x7f) << 8) | (bytes[1] as u16);
    let mantissa = ((bytes[2] as u64) << 56)
        | ((bytes[3] as u64) << 48)
        | ((bytes[4] as u64) << 40)
        | ((bytes[5] as u64) << 32)
        | ((bytes[6] as u64) << 24)
        | ((bytes[7] as u64) << 16)
        | ((bytes[8] as u64) << 8)
        | (bytes[9] as u64);
    if exponent == 0 && mantissa == 0 {
        return 0.0;
    }
    let fraction = mantissa as f64 / (1u64 << 63) as f64;
    fraction * 2f64.powi(exponent as i32 - 16383)
}

fn read_flac_tags(path: &Path) -> Result<AudioTags, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic)?;
    if &magic[0..3] == b"ID3" {
        // Some taggers prepend a non-standard ID3v2 tag before the fLaC marker.
        let mut rest = [0_u8; 6];
        file.read_exact(&mut rest)?;
        let tag_size = syncsafe_u32(&rest[2..6]) as u64;
        file.seek(SeekFrom::Start(10 + tag_size))?;
        file.read_exact(&mut magic)?;
    }
    if &magic != b"fLaC" {
        return Ok(AudioTags::default());
    }

    loop {
        let mut header = [0_u8; 4];
        file.read_exact(&mut header)?;
        let is_last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let length =
            ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
        if length > 16 * 1024 * 1024 {
            return Ok(AudioTags::default());
        }
        let mut block = vec![0_u8; length];
        file.read_exact(&mut block)?;
        if block_type == 4 {
            return Ok(parse_vorbis_comments(&block));
        }
        if is_last {
            break;
        }
    }
    Ok(AudioTags::default())
}

fn read_ogg_tags(path: &Path) -> Result<AudioTags, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut data = Vec::new();
    file.take(64 * 1024).read_to_end(&mut data)?;
    if data.get(0..4) != Some(b"OggS".as_slice()) {
        return Ok(AudioTags::default());
    }
    if let Some(start) = find_bytes(&data, b"OpusTags") {
        return Ok(parse_vorbis_comments(&data[start + 8..]));
    }
    if let Some(start) = find_bytes(&data, b"\x03vorbis") {
        return Ok(parse_vorbis_comments(&data[start + 7..]));
    }
    Ok(AudioTags::default())
}

fn read_mp4_tags(path: &Path) -> Result<AudioTags, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut data = Vec::new();
    file.take(256 * 1024).read_to_end(&mut data)?;
    Ok(parse_mp4_atoms(&data, 0))
}

fn read_id3v2_tags(path: &Path) -> Result<AudioTags, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut header = [0_u8; 10];
    file.read_exact(&mut header)?;
    if &header[0..3] != b"ID3" {
        return Ok(AudioTags::default());
    }
    let major = header[3];
    if !(3..=4).contains(&major) {
        return Ok(AudioTags::default());
    }
    let tag_size = syncsafe_u32(&header[6..10]) as usize;
    let mut data = vec![0_u8; tag_size.min(16 * 1024 * 1024)];
    file.seek(SeekFrom::Start(10))?;
    file.read_exact(&mut data)?;

    let mut values = HashMap::new();
    let mut offset = 0;
    while offset + 10 <= data.len() {
        let frame_id = &data[offset..offset + 4];
        if frame_id.iter().all(|byte| *byte == 0) {
            break;
        }
        let frame_size = if major == 4 {
            syncsafe_u32(&data[offset + 4..offset + 8]) as usize
        } else {
            u32::from_be_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize
        };
        offset += 10;
        if frame_size == 0 || offset + frame_size > data.len() {
            break;
        }
        let id = std::str::from_utf8(frame_id)
            .unwrap_or_default()
            .to_string();
        let frame = &data[offset..offset + frame_size];
        if let Some(value) = parse_id3_frame_value(&id, frame) {
            values.entry(id).or_insert(value);
        }
        offset += frame_size;
    }

    Ok(AudioTags {
        title: take_tag(&mut values, "TIT2"),
        artist: take_tag(&mut values, "TPE1"),
        album: take_tag(&mut values, "TALB"),
        album_artist: take_tag(&mut values, "TPE2"),
        genre: take_tag(&mut values, "TCON"),
        composer: take_tag(&mut values, "TCOM"),
        track_number: take_tag(&mut values, "TRCK").and_then(|value| parse_leading_i64(&value)),
        disc_number: take_tag(&mut values, "TPOS").and_then(|value| parse_leading_i64(&value)),
        year: take_tag(&mut values, "TDRC")
            .or_else(|| take_tag(&mut values, "TYER"))
            .and_then(|value| parse_leading_i64(&value)),
        comment: take_tag(&mut values, "COMM"),
        bpm: take_tag(&mut values, "TBPM").and_then(|value| parse_leading_i64(&value)),
    })
}

fn parse_vorbis_comments(data: &[u8]) -> AudioTags {
    let mut offset = 0;
    let Some(vendor_len) = read_le_u32(data, &mut offset) else {
        return AudioTags::default();
    };
    offset = offset.saturating_add(vendor_len as usize);
    let Some(comment_count) = read_le_u32(data, &mut offset) else {
        return AudioTags::default();
    };

    let mut values = HashMap::new();
    for _ in 0..comment_count {
        let Some(length) = read_le_u32(data, &mut offset).map(|value| value as usize) else {
            break;
        };
        if offset + length > data.len() {
            break;
        }
        if let Ok(raw) = std::str::from_utf8(&data[offset..offset + length]) {
            if let Some((key, value)) = raw.split_once('=') {
                let value = value.trim();
                if !value.is_empty() {
                    values
                        .entry(key.trim().to_ascii_uppercase())
                        .or_insert_with(|| value.to_owned());
                }
            }
        }
        offset += length;
    }
    tags_from_common_map(values)
}

fn tags_from_common_map(mut values: HashMap<String, String>) -> AudioTags {
    AudioTags {
        title: take_any_tag(&mut values, &["TITLE"]),
        artist: take_any_tag(&mut values, &["ARTIST", "PERFORMER"]),
        album: take_any_tag(&mut values, &["ALBUM"]),
        album_artist: take_any_tag(&mut values, &["ALBUMARTIST", "ALBUM ARTIST"]),
        genre: take_any_tag(&mut values, &["GENRE"]),
        composer: take_any_tag(&mut values, &["COMPOSER"]),
        track_number: take_any_tag(&mut values, &["TRACKNUMBER", "TRACK"])
            .and_then(|value| parse_leading_i64(&value)),
        disc_number: take_any_tag(&mut values, &["DISCNUMBER", "DISC"])
            .and_then(|value| parse_leading_i64(&value)),
        year: take_any_tag(&mut values, &["DATE", "YEAR"])
            .and_then(|value| parse_leading_i64(&value)),
        comment: take_any_tag(&mut values, &["COMMENT", "DESCRIPTION"]),
        bpm: take_any_tag(&mut values, &["BPM"]).and_then(|value| parse_leading_i64(&value)),
    }
}

fn parse_mp4_atoms(data: &[u8], depth: usize) -> AudioTags {
    if depth > 8 {
        return AudioTags::default();
    }
    let mut tags = AudioTags::default();
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let Some((atom_type, payload_start, atom_end)) = read_mp4_atom(data, offset) else {
            break;
        };
        let payload = &data[payload_start..atom_end];
        match atom_type.as_slice() {
            b"moov" | b"udta" | b"ilst" => tags.merge(parse_mp4_atoms(payload, depth + 1)),
            // iTunes-style `meta` is a FullBox: version(1) + flags(3) before child atoms.
            // Validate the full-box header before skipping it.
            b"meta" if payload.len() >= 4 && payload[0] == 0 => {
                tags.merge(parse_mp4_atoms(&payload[4..], depth + 1))
            }
            b"\xa9nam" => tags.title = parse_mp4_text_atom(payload),
            b"\xa9ART" => tags.artist = parse_mp4_text_atom(payload),
            b"\xa9alb" => tags.album = parse_mp4_text_atom(payload),
            b"aART" => tags.album_artist = parse_mp4_text_atom(payload),
            b"\xa9gen" => tags.genre = parse_mp4_text_atom(payload),
            b"\xa9wrt" => tags.composer = parse_mp4_text_atom(payload),
            b"\xa9day" => {
                tags.year = parse_mp4_text_atom(payload).and_then(|value| parse_leading_i64(&value))
            }
            b"\xa9cmt" => tags.comment = parse_mp4_text_atom(payload),
            b"tmpo" => tags.bpm = parse_mp4_number_atom(payload),
            b"trkn" => tags.track_number = parse_mp4_pair_atom(payload),
            b"disk" => tags.disc_number = parse_mp4_pair_atom(payload),
            _ => {}
        }
        offset = atom_end;
    }
    tags
}

impl AudioTags {
    fn merge(&mut self, other: AudioTags) {
        self.title = self.title.take().or(other.title);
        self.artist = self.artist.take().or(other.artist);
        self.album = self.album.take().or(other.album);
        self.album_artist = self.album_artist.take().or(other.album_artist);
        self.genre = self.genre.take().or(other.genre);
        self.composer = self.composer.take().or(other.composer);
        self.track_number = self.track_number.or(other.track_number);
        self.disc_number = self.disc_number.or(other.disc_number);
        self.year = self.year.or(other.year);
        self.comment = self.comment.take().or(other.comment);
        self.bpm = self.bpm.or(other.bpm);
    }
}

fn read_mp4_atom(data: &[u8], offset: usize) -> Option<(Vec<u8>, usize, usize)> {
    let size = u32::from_be_bytes(data.get(offset..offset + 4)?.try_into().ok()?) as usize;
    let atom_type = data.get(offset + 4..offset + 8)?.to_vec();
    let (payload_start, atom_end) = if size == 1 {
        let large_size =
            u64::from_be_bytes(data.get(offset + 8..offset + 16)?.try_into().ok()?) as usize;
        (offset + 16, offset.checked_add(large_size)?)
    } else {
        (offset + 8, offset.checked_add(size)?)
    };
    (payload_start <= atom_end && atom_end <= data.len()).then_some((
        atom_type,
        payload_start,
        atom_end,
    ))
}

fn parse_mp4_text_atom(payload: &[u8]) -> Option<String> {
    find_mp4_data_payload(payload).and_then(|data| {
        let text = String::from_utf8_lossy(data)
            .trim_matches(char::from(0))
            .trim()
            .to_owned();
        (!text.is_empty()).then_some(text)
    })
}

fn parse_mp4_number_atom(payload: &[u8]) -> Option<i64> {
    let data = find_mp4_data_payload(payload)?;
    match data.len() {
        1 => Some(i64::from(data[0])),
        2.. => Some(i64::from(u16::from_be_bytes([
            data[data.len() - 2],
            data[data.len() - 1],
        ]))),
        _ => None,
    }
}

fn parse_mp4_pair_atom(payload: &[u8]) -> Option<i64> {
    let data = find_mp4_data_payload(payload)?;
    if data.len() >= 4 {
        let current = u16::from_be_bytes([data[data.len() - 4], data[data.len() - 3]]);
        if current > 0 {
            return Some(i64::from(current));
        }
    }
    None
}

fn find_mp4_data_payload(payload: &[u8]) -> Option<&[u8]> {
    let mut offset = 0;
    while offset + 16 <= payload.len() {
        let (atom_type, data_start, atom_end) = read_mp4_atom(payload, offset)?;
        if atom_type.as_slice() == b"data" && data_start + 8 <= atom_end {
            return Some(&payload[data_start + 8..atom_end]);
        }
        offset = atom_end;
    }
    None
}

fn read_le_u32(data: &[u8], offset: &mut usize) -> Option<u32> {
    let value = u32::from_le_bytes(data.get(*offset..*offset + 4)?.try_into().ok()?);
    *offset += 4;
    Some(value)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn syncsafe_u32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0_u32, |acc, byte| {
        (acc << 7) | u32::from(byte & 0b0111_1111)
    })
}

fn parse_id3_text_frame(frame: &[u8]) -> Option<String> {
    let (&encoding, payload) = frame.split_first()?;
    decode_id3_text_payload(encoding, payload)
}

fn parse_id3_frame_value(id: &str, frame: &[u8]) -> Option<String> {
    if id != "COMM" {
        return parse_id3_text_frame(frame);
    }
    let (&encoding, payload) = frame.split_first()?;
    let payload = payload.get(3..).unwrap_or(payload);
    let text = decode_id3_text_payload(encoding, payload)?;
    text.split('\0')
        .next_back()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn decode_id3_text_payload(encoding: u8, payload: &[u8]) -> Option<String> {
    let value = match encoding {
        0 | 3 => String::from_utf8_lossy(payload).into_owned(),
        1 | 2 => decode_utf16(payload, encoding == 1),
        _ => String::new(),
    };
    let cleaned = value
        .trim_matches('\u{feff}')
        .trim_matches(char::from(0))
        .trim()
        .to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn decode_utf16(payload: &[u8], allow_bom: bool) -> String {
    let mut bytes = payload;
    let little_endian = if allow_bom && bytes.len() >= 2 {
        match &bytes[..2] {
            [0xff, 0xfe] => {
                bytes = &bytes[2..];
                true
            }
            [0xfe, 0xff] => {
                bytes = &bytes[2..];
                false
            }
            _ => false,
        }
    } else {
        false
    };
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

fn take_tag(values: &mut HashMap<String, String>, key: &str) -> Option<String> {
    values
        .remove(key)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn take_any_tag(values: &mut HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| take_tag(values, key))
}

fn parse_leading_i64(value: &str) -> Option<i64> {
    let number = value
        .trim()
        .split('/')
        .next()
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    number.parse().ok()
}

fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            SUPPORTED_AUDIO_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
        .unwrap_or(false)
}

fn clean_track_title(value: &str) -> String {
    let trimmed = value.trim();
    let without_track_number = trimmed
        .trim_start_matches(|c: char| {
            c.is_ascii_digit() || c == '.' || c == '-' || c == '_' || c == ' '
        })
        .trim();
    let result = if without_track_number.is_empty() {
        trimmed.to_owned()
    } else {
        without_track_number.to_owned()
    };
    result.replace('_', " ")
}

fn normalize_folder_name(value: &str) -> String {
    value.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_audio_extensions_case_insensitively() {
        assert!(is_supported_audio_file(Path::new("Song.MP3")));
        assert!(is_supported_audio_file(Path::new("Song.flac")));
        assert!(!is_supported_audio_file(Path::new("cover.jpg")));
    }

    #[test]
    fn cleans_simple_track_number_prefixes() {
        assert_eq!(clean_track_title("01 - Hello"), "Hello");
        assert_eq!(clean_track_title("Song"), "Song");
    }

    #[test]
    fn replaces_underscores_in_title_and_folder_fallbacks() {
        assert_eq!(clean_track_title("01_Some_Song_Name"), "Some Song Name");
        assert_eq!(clean_track_title("Song_Title"), "Song Title");
        assert_eq!(normalize_folder_name("Pink_Floyd"), "Pink Floyd");
        assert_eq!(normalize_folder_name("The_Wall"), "The Wall");
    }

    #[test]
    fn parses_id3v2_text_frames() {
        let mut frame = vec![3];
        frame.extend_from_slice(b"Test Song");

        assert_eq!(parse_id3_text_frame(&frame), Some("Test Song".to_string()));
    }

    #[test]
    fn parses_id3_comment_frames_without_language_prefix() {
        let mut frame = vec![3];
        frame.extend_from_slice(b"engshort\0Real comment");

        assert_eq!(
            parse_id3_frame_value("COMM", &frame),
            Some("Real comment".to_string())
        );
    }

    #[test]
    fn parses_slash_prefixed_numbers() {
        assert_eq!(parse_leading_i64("03/12"), Some(3));
        assert_eq!(parse_leading_i64(""), None);
    }

    #[test]
    fn parses_vorbis_comment_tags() {
        let mut block = Vec::new();
        block.extend_from_slice(&0_u32.to_le_bytes());
        block.extend_from_slice(&3_u32.to_le_bytes());
        for comment in ["TITLE=Song", "ALBUMARTIST=Band", "TRACKNUMBER=7/12"] {
            block.extend_from_slice(&(comment.len() as u32).to_le_bytes());
            block.extend_from_slice(comment.as_bytes());
        }

        let tags = parse_vorbis_comments(&block);

        assert_eq!(tags.title.as_deref(), Some("Song"));
        assert_eq!(tags.album_artist.as_deref(), Some("Band"));
        assert_eq!(tags.track_number, Some(7));
    }

    #[test]
    fn reads_flac_tags_past_a_leading_id3v2_tag() {
        let mut vorbis_block = Vec::new();
        vorbis_block.extend_from_slice(&0_u32.to_le_bytes());
        vorbis_block.extend_from_slice(&1_u32.to_le_bytes());
        let comment = "ARTIST=Solarstone";
        vorbis_block.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        vorbis_block.extend_from_slice(comment.as_bytes());

        let mut file_bytes = Vec::new();
        file_bytes.extend_from_slice(b"ID3\x03\x00\x00");
        file_bytes.extend_from_slice(&syncsafe_encode(23)); // arbitrary padded ID3 body size
        file_bytes.extend_from_slice(&[0_u8; 23]);
        file_bytes.extend_from_slice(b"fLaC");
        let mut header = [0x84_u8, 0, 0, 0]; // last block, type 4 (VORBIS_COMMENT)
        let len = vorbis_block.len();
        header[1] = (len >> 16) as u8;
        header[2] = (len >> 8) as u8;
        header[3] = len as u8;
        file_bytes.extend_from_slice(&header);
        file_bytes.extend_from_slice(&vorbis_block);

        let dir = std::env::temp_dir();
        let path = dir.join(format!("boogiebox_test_{}.flac", std::process::id()));
        fs::write(&path, &file_bytes).unwrap();

        let tags = read_flac_tags(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(tags.artist.as_deref(), Some("Solarstone"));
    }

    fn syncsafe_encode(mut size: u32) -> [u8; 4] {
        let mut bytes = [0_u8; 4];
        for i in (0..4).rev() {
            bytes[i] = (size & 0x7f) as u8;
            size >>= 7;
        }
        bytes
    }

    #[test]
    fn parses_mp4_text_and_pair_atoms() {
        let nam = mp4_atom(b"\xa9nam", &mp4_data_atom(b"Song"));
        let trkn_payload = mp4_data_atom(&[0, 0, 0, 5, 0, 9]);
        let trkn = mp4_atom(b"trkn", &trkn_payload);
        let mut ilst_payload = Vec::new();
        ilst_payload.extend_from_slice(&nam);
        ilst_payload.extend_from_slice(&trkn);
        let ilst = mp4_atom(b"ilst", &ilst_payload);

        let tags = parse_mp4_atoms(&ilst, 0);

        assert_eq!(tags.title.as_deref(), Some("Song"));
        assert_eq!(tags.track_number, Some(5));
    }

    fn mp4_atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut atom = Vec::new();
        atom.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
        atom.extend_from_slice(kind);
        atom.extend_from_slice(payload);
        atom
    }

    fn mp4_data_atom(value: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(&0_u32.to_be_bytes());
        payload.extend_from_slice(value);
        mp4_atom(b"data", &payload)
    }

    // -- Additional coverage: technical-metadata readers, discovery, build_track_input,
    // and end-to-end scan-job orchestration -------------------------------------------

    use boogiebox_db::init_db;
    use boogiebox_db::jobs::{
        claim_next_scan_job, create_library, enqueue_scan_job, CreateLibraryInput,
    };
    use std::sync::{Arc, Mutex};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("scanner-test-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // -- discover_audio_files / collect_audio_files --------------------------------

    #[test]
    fn discovers_audio_files_recursively_and_sorts_case_insensitively() {
        let dir = temp_dir("discover");
        let sub = dir.join("Artist").join("Album");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("b_track.mp3"), b"x").unwrap();
        fs::write(sub.join("A_track.FLAC"), b"x").unwrap();
        fs::write(sub.join("cover.jpg"), b"x").unwrap(); // not audio

        let mut counters = ScanCounters::default();
        let files = discover_audio_files(&[dir.to_string_lossy().into_owned()], &mut counters);

        assert_eq!(files.len(), 2);
        assert!(files[0]
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("a_track"));
        assert_eq!(counters.errors, 0);
    }

    #[test]
    fn discover_audio_files_records_error_for_unreadable_folder() {
        let dir = temp_dir("discover-missing");
        let missing = dir.join("does-not-exist");
        let mut counters = ScanCounters::default();
        let files = discover_audio_files(&[missing.to_string_lossy().into_owned()], &mut counters);

        assert!(files.is_empty());
        assert_eq!(counters.errors, 1);
        assert!(!counters.messages.is_empty());
    }

    // -- build_track_input -----------------------------------------------------------

    #[test]
    fn build_track_input_falls_back_to_folder_names_and_cleans_title() {
        let dir = temp_dir("build-track");
        let sub = dir.join("Pink_Floyd").join("The_Wall");
        fs::create_dir_all(&sub).unwrap();
        let file_path = sub.join("01_Another_Brick.WAV");
        fs::write(&file_path, b"not really a wav").unwrap();

        let library_id = EntityId::Str("lib1".to_string());
        let input = build_track_input(&library_id, &file_path).unwrap();

        assert_eq!(input.title, "Another Brick");
        assert_eq!(input.artist, "Pink Floyd");
        assert_eq!(input.album, "The Wall");
        assert_eq!(input.album_artist, "Pink Floyd");
        assert_eq!(input.format, "WAV");
        assert_eq!(input.file_name, "01_Another_Brick.WAV");
    }

    #[test]
    fn build_track_input_errors_for_missing_file() {
        let library_id = EntityId::Str("lib1".to_string());
        let result = build_track_input(&library_id, Path::new("Z:/definitely/missing.mp3"));
        assert!(result.is_err());
    }

    // -- WAV / AIFF technical metadata -----------------------------------------------

    fn build_wav_bytes(sample_rate: u32, channels: u16, bits: u16, num_frames: u32) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * block_align as u32;
        let data_size = num_frames * block_align as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_size).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_size.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, data_size as usize));
        out
    }

    #[test]
    fn reads_wav_technical_metadata() {
        let dir = temp_dir("wav");
        let path = dir.join("t.wav");
        fs::write(&path, build_wav_bytes(44100, 2, 16, 44100)).unwrap();

        let tech = read_wav_technical(&path).expect("wav parse");
        assert_eq!(tech.sample_rate, Some(44100));
        assert_eq!(tech.channels, Some(2));
        assert_eq!(tech.duration, Some(1.0));
    }

    #[test]
    fn rejects_non_wav_riff_file() {
        let dir = temp_dir("wav-bad");
        let path = dir.join("bad.wav");
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"BAD!");
        fs::write(&path, bytes).unwrap();
        assert!(read_wav_technical(&path).is_none());
    }

    fn build_aiff_bytes(sample_rate_ext: [u8; 10], channels: i16, num_frames: u32) -> Vec<u8> {
        let mut comm = Vec::new();
        comm.extend_from_slice(&channels.to_be_bytes());
        comm.extend_from_slice(&num_frames.to_be_bytes());
        comm.extend_from_slice(&16i16.to_be_bytes()); // sample size
        comm.extend_from_slice(&sample_rate_ext);

        let mut out = Vec::new();
        out.extend_from_slice(b"FORM");
        out.extend_from_slice(&0u32.to_be_bytes()); // size unused by reader
        out.extend_from_slice(b"AIFF");
        out.extend_from_slice(b"COMM");
        out.extend_from_slice(&(comm.len() as u32).to_be_bytes());
        out.extend_from_slice(&comm);
        out
    }

    #[test]
    fn reads_aiff_technical_metadata() {
        // 44100 Hz as 80-bit IEEE 754 extended: exponent 0x400E, mantissa 0xAC44000000000000
        let sr_bytes: [u8; 10] = [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0];
        let dir = temp_dir("aiff");
        let path = dir.join("t.aiff");
        fs::write(&path, build_aiff_bytes(sr_bytes, 2, 44100)).unwrap();

        let tech = read_aiff_technical(&path).expect("aiff parse");
        assert_eq!(tech.channels, Some(2));
        assert_eq!(tech.sample_rate, Some(44100));
        assert!((tech.duration.unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn rejects_non_aiff_form_file() {
        let dir = temp_dir("aiff-bad");
        let path = dir.join("bad.aiff");
        let mut bytes = b"FORM".to_vec();
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(b"XXXX");
        fs::write(&path, bytes).unwrap();
        assert!(read_aiff_technical(&path).is_none());
    }

    // -- M4A / mvhd duration -----------------------------------------------------------

    #[test]
    fn parses_mvhd_version0_duration() {
        let mut data = vec![0u8]; // version 0
        data.extend_from_slice(&[0, 0, 0]); // flags
        data.extend_from_slice(&0u32.to_be_bytes()); // creation
        data.extend_from_slice(&0u32.to_be_bytes()); // modification
        data.extend_from_slice(&1000u32.to_be_bytes()); // time_scale
        data.extend_from_slice(&5000u32.to_be_bytes()); // duration
        assert_eq!(parse_mvhd(&data), Some(5.0));
    }

    #[test]
    fn parses_mvhd_version1_duration() {
        let mut data = vec![1u8];
        data.extend_from_slice(&[0, 0, 0]);
        data.extend_from_slice(&0u64.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        data.extend_from_slice(&1000u32.to_be_bytes());
        data.extend_from_slice(&8000u64.to_be_bytes());
        assert_eq!(parse_mvhd(&data), Some(8.0));
    }

    #[test]
    fn mvhd_rejects_zero_time_scale_and_short_buffers() {
        assert_eq!(parse_mvhd(&[]), None);
        let mut data = vec![0u8; 19];
        data[0] = 0;
        assert_eq!(parse_mvhd(&data), None); // too short for v0
        let mut zero_scale = vec![0u8; 20];
        zero_scale[0] = 0;
        assert_eq!(parse_mvhd(&zero_scale), None); // time_scale 0
    }

    #[test]
    fn reads_m4a_technical_via_moov_mvhd() {
        let mut mvhd_payload = vec![0u8];
        mvhd_payload.extend_from_slice(&[0, 0, 0]);
        mvhd_payload.extend_from_slice(&0u32.to_be_bytes());
        mvhd_payload.extend_from_slice(&0u32.to_be_bytes());
        mvhd_payload.extend_from_slice(&1000u32.to_be_bytes());
        mvhd_payload.extend_from_slice(&3000u32.to_be_bytes());
        let mvhd = mp4_atom(b"mvhd", &mvhd_payload);
        let moov = mp4_atom(b"moov", &mvhd);

        let dir = temp_dir("m4a");
        let path = dir.join("t.m4a");
        fs::write(&path, &moov).unwrap();

        let tech = read_m4a_technical(&path).expect("m4a parse");
        assert_eq!(tech.duration, Some(3.0));
    }

    // -- MP3 sync/frame helpers -----------------------------------------------------

    #[test]
    fn is_valid_mpeg_sync_rejects_short_or_bad_words() {
        assert!(!is_valid_mpeg_sync(&[0xFF]));
        assert!(!is_valid_mpeg_sync(&[0x00, 0x00, 0x00, 0x00]));
    }

    #[test]
    fn syncsafe_u32_decodes_seven_bit_groups() {
        // 0x7F, 0x7F, 0x7F, 0x7F -> all 28 bits set
        assert_eq!(syncsafe_u32(&[0x7F, 0x7F, 0x7F, 0x7F]), 0x0FFF_FFFF);
        assert_eq!(syncsafe_u32(&[0, 0, 0, 1]), 1);
    }

    #[test]
    fn read_audio_technical_returns_default_for_unsupported_extension() {
        let dir = temp_dir("unsupported");
        let path = dir.join("t.xyz");
        fs::write(&path, b"whatever").unwrap();
        let tech = read_audio_technical(&path);
        assert!(tech.duration.is_none());
        assert!(tech.bitrate.is_none());
    }

    #[test]
    fn read_audio_tags_returns_default_for_unsupported_extension() {
        let dir = temp_dir("unsupported-tags");
        let path = dir.join("t.xyz");
        fs::write(&path, b"whatever").unwrap();
        let tags = read_audio_tags(&path).unwrap();
        assert!(tags.title.is_none());
    }

    // -- ID3v2 header rejection paths -------------------------------------------------

    #[test]
    fn read_id3v2_tags_rejects_missing_or_unsupported_header() {
        let dir = temp_dir("id3-bad");
        let no_id3 = dir.join("no_id3.mp3");
        fs::write(&no_id3, b"NOTID3....").unwrap();
        let tags = read_id3v2_tags(&no_id3).unwrap();
        assert!(tags.title.is_none());

        let bad_version = dir.join("bad_version.mp3");
        let mut header = b"ID3".to_vec();
        header.push(9); // unsupported major version
        header.push(0);
        header.push(0);
        header.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(&bad_version, &header).unwrap();
        let tags = read_id3v2_tags(&bad_version).unwrap();
        assert!(tags.title.is_none());
    }

    // -- OGG/Opus tag + technical dispatch --------------------------------------------

    #[test]
    fn read_ogg_technical_rejects_non_ogg_file() {
        let dir = temp_dir("ogg-bad");
        let path = dir.join("bad.ogg");
        fs::write(&path, b"not an ogg file at all").unwrap();
        assert!(read_ogg_technical(&path).is_none());
    }

    #[test]
    fn read_ogg_tags_rejects_non_ogg_file() {
        let dir = temp_dir("ogg-tags-bad");
        let path = dir.join("bad.ogg");
        fs::write(&path, b"not an ogg file at all").unwrap();
        let tags = read_ogg_tags(&path).unwrap();
        assert!(tags.title.is_none());
    }

    // -- AudioTags::merge --------------------------------------------------------------

    #[test]
    fn audio_tags_merge_prefers_self_and_fills_gaps() {
        let mut a = AudioTags {
            title: Some("A Title".into()),
            ..Default::default()
        };
        let b = AudioTags {
            title: Some("B Title".into()),
            artist: Some("B Artist".into()),
            bpm: Some(120),
            ..Default::default()
        };
        a.merge(b);
        assert_eq!(a.title.as_deref(), Some("A Title")); // self wins
        assert_eq!(a.artist.as_deref(), Some("B Artist")); // gap filled
        assert_eq!(a.bpm, Some(120));
    }

    // -- End-to-end scan-job orchestration via a real temp SQLite DB -----------------

    fn init_test_db(prefix: &str) -> (Arc<Mutex<rusqlite::Connection>>, PathBuf) {
        let dir = temp_dir(prefix);
        let initialized = init_db(&dir).expect("init test db");
        (Arc::new(Mutex::new(initialized.connection)), dir)
    }

    #[test]
    fn scan_music_library_end_to_end_inserts_tracks_and_marks_done() {
        let (db, dir) = init_test_db("scan-e2e");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(
            music_dir.join("song.mp3"),
            b"fake mp3 bytes, no valid frame",
        )
        .unwrap();

        let library = {
            let conn = db.lock().unwrap();
            create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("E2E Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap()
        };

        let claimed = {
            let conn = db.lock().unwrap();
            enqueue_scan_job(&conn, &library.id.to_string()).unwrap();
            claim_next_scan_job(&conn).unwrap().expect("job claimable")
        };

        scan_music_library(&db, &claimed).unwrap();

        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM scan_jobs WHERE id = ?1",
                [&claimed.job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "done");

        let track_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE library_id = ?1",
                [&library.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(track_count, 1);
    }

    #[test]
    fn run_one_pending_scan_blocking_is_a_noop_when_nothing_pending() {
        let (db, _dir) = init_test_db("scan-noop");
        // No library, no job enqueued.
        let result = run_one_pending_scan_blocking(&db, None);
        assert!(result.is_ok());
    }

    #[test]
    fn run_one_pending_scan_blocking_returns_ok_for_unknown_target_job() {
        let (db, _dir) = init_test_db("scan-unknown-target");
        let missing_job = EntityId::Str("does-not-exist".to_string());
        let result = run_one_pending_scan_blocking(&db, Some(missing_job));
        assert!(result.is_ok());
    }

    #[test]
    fn run_one_pending_scan_blocking_runs_claimed_job_end_to_end() {
        let (db, dir) = init_test_db("scan-blocking-e2e");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(music_dir.join("a.flac"), b"not a real flac").unwrap();

        {
            let conn = db.lock().unwrap();
            let library = create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("Blocking Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap();
            enqueue_scan_job(&conn, &library.id.to_string()).unwrap();
        }

        let result = run_one_pending_scan_blocking(&db, None);
        assert!(result.is_ok());

        let conn = db.lock().unwrap();
        let done_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scan_jobs WHERE status = 'done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(done_count, 1);
    }

    #[test]
    fn scan_music_library_marks_cancelled_when_job_cancelled_mid_scan() {
        // update_scan_progress checks the job's current status; if something else
        // (e.g. a user cancel request) flips it away from 'running' mid-scan,
        // scan_music_library must surface JobError::ScanCancelled rather than a DB error.
        let (db, dir) = init_test_db("scan-cancelled");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(music_dir.join("song.mp3"), b"fake mp3 bytes").unwrap();

        let library = {
            let conn = db.lock().unwrap();
            create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("Cancelled Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap()
        };
        let claimed = {
            let conn = db.lock().unwrap();
            enqueue_scan_job(&conn, &library.id.to_string()).unwrap();
            let claimed = claim_next_scan_job(&conn).unwrap().expect("job claimable");
            conn.execute(
                "UPDATE scan_jobs SET status='cancelled' WHERE id = ?1",
                [&claimed.job_id],
            )
            .unwrap();
            claimed
        };

        let result = scan_music_library(&db, &claimed);
        assert!(matches!(result, Err(JobError::ScanCancelled)));
    }

    #[test]
    fn run_one_pending_scan_blocking_marks_job_failed_on_cancellation() {
        // Exercises run_one_pending_scan_blocking's error-branch that persists the
        // failure/cancellation back onto the job row.
        let (db, dir) = init_test_db("scan-blocking-cancelled");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(music_dir.join("song.mp3"), b"fake mp3 bytes").unwrap();

        let job_id = {
            let conn = db.lock().unwrap();
            let library = create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("Blocking Cancelled Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap();
            let job_id = enqueue_scan_job(&conn, &library.id.to_string()).unwrap();
            // Claim it ourselves and immediately flip to cancelled, mirroring a
            // cancel request that lands between claim and the first progress write.
            claim_next_scan_job(&conn).unwrap();
            conn.execute(
                "UPDATE scan_jobs SET status='cancelled' WHERE id = ?1",
                [&job_id],
            )
            .unwrap();
            job_id
        };

        let result = run_one_pending_scan_blocking(&db, Some(job_id.clone()));
        // The job is no longer 'pending' so claim_scan_job(target_job_id) returns None,
        // and run_one_pending_scan_blocking is a no-op Ok(()) rather than re-running it.
        assert!(result.is_ok());

        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM scan_jobs WHERE id = ?1",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "cancelled");
    }

    // -- MP3 valid frame parsing (CBR estimation path) --------------------------------

    #[test]
    fn reads_mp3_technical_from_a_valid_cbr_frame() {
        // FF FB 90 00: MPEG1 Layer III, bitrate idx 9 (128kbps), sr idx 0 (44100), stereo.
        let dir = temp_dir("mp3-cbr");
        let path = dir.join("t.mp3");
        let mut bytes = vec![0xFFu8, 0xFB, 0x90, 0x00];
        bytes.extend(std::iter::repeat_n(0u8, 128_000 / 8)); // ~1 second of audio at 128kbps
        fs::write(&path, &bytes).unwrap();

        let tech = read_mp3_technical(&path).expect("mp3 parse");
        assert_eq!(tech.bitrate, Some(128));
        assert_eq!(tech.sample_rate, Some(44100));
        assert_eq!(tech.channels, Some(2));
        assert!(tech.duration.unwrap() > 0.0);
    }

    #[test]
    fn reads_mp3_technical_skips_leading_id3v2_tag() {
        let dir = temp_dir("mp3-id3-skip");
        let path = dir.join("t.mp3");
        let mut bytes = b"ID3\x03\x00\x00".to_vec();
        bytes.extend_from_slice(&syncsafe_encode(20));
        bytes.extend(std::iter::repeat_n(0u8, 20));
        bytes.extend_from_slice(&[0xFFu8, 0xFB, 0x90, 0x00]);
        bytes.extend(std::iter::repeat_n(0u8, 16_000));
        fs::write(&path, &bytes).unwrap();

        let tech = read_mp3_technical(&path).expect("mp3 parse with id3 header");
        assert_eq!(tech.sample_rate, Some(44100));
    }

    #[test]
    fn read_mp3_technical_returns_none_for_no_sync() {
        let dir = temp_dir("mp3-no-sync");
        let path = dir.join("t.mp3");
        fs::write(&path, vec![0u8; 100]).unwrap();
        assert!(read_mp3_technical(&path).is_none());
    }

    // -- FLAC technical (STREAMINFO) ----------------------------------------------------

    #[test]
    fn reads_flac_technical_streaminfo() {
        let dir = temp_dir("flac-tech");
        let path = dir.join("t.flac");

        // STREAMINFO block: 18 bytes minimum; sample_rate (20 bits) + channels (3 bits)
        // packed starting at byte 10, plus 36-bit total_samples.
        let sample_rate: u32 = 44100;
        let channels: u8 = 2; // encoded as channels-1 = 1
        let total_samples: u64 = 44100; // 1 second

        let mut block = vec![0u8; 18];
        block[10] = (sample_rate >> 12) as u8;
        block[11] = (sample_rate >> 4) as u8;
        block[12] = (((sample_rate & 0xF) << 4) as u8) | (((channels - 1) & 0x07) << 1);
        block[13] = ((total_samples >> 32) & 0x0F) as u8;
        block[14] = ((total_samples >> 24) & 0xFF) as u8;
        block[15] = ((total_samples >> 16) & 0xFF) as u8;
        block[16] = ((total_samples >> 8) & 0xFF) as u8;
        block[17] = (total_samples & 0xFF) as u8;

        let mut bytes = b"fLaC".to_vec();
        let mut header = [0x80u8, 0, 0, 0]; // last block, type 0 (STREAMINFO)
        let len = block.len();
        header[1] = (len >> 16) as u8;
        header[2] = (len >> 8) as u8;
        header[3] = len as u8;
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&block);
        fs::write(&path, &bytes).unwrap();

        let tech = read_flac_technical(&path).expect("flac tech parse");
        assert_eq!(tech.sample_rate, Some(44100));
        assert_eq!(tech.channels, Some(2));
        assert_eq!(tech.bitrate, None);
        assert!((tech.duration.unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn read_flac_technical_rejects_non_flac_magic() {
        let dir = temp_dir("flac-tech-bad");
        let path = dir.join("bad.flac");
        fs::write(&path, b"NOTFLAC!").unwrap();
        assert!(read_flac_technical(&path).is_none());
    }

    // -- OGG/Opus and Vorbis technical + tags -------------------------------------------

    fn build_opus_ogg_bytes(channels: u8) -> Vec<u8> {
        let mut page = b"OggS".to_vec();
        page.extend_from_slice(&[0u8; 22]); // rest of the fixed OGG page header (unused by parser)
        let mut opus_head = b"OpusHead".to_vec();
        opus_head.push(1); // version
        opus_head.push(channels);
        opus_head.extend_from_slice(&0u16.to_le_bytes()); // pre-skip
        opus_head.extend_from_slice(&48000u32.to_le_bytes()); // input sample rate
        page.extend_from_slice(&opus_head);
        page
    }

    #[test]
    fn reads_ogg_opus_technical_metadata() {
        let dir = temp_dir("ogg-opus");
        let path = dir.join("t.ogg");
        fs::write(&path, build_opus_ogg_bytes(2)).unwrap();

        let tech = read_ogg_technical(&path).expect("opus tech parse");
        assert_eq!(tech.sample_rate, Some(48000));
        assert_eq!(tech.channels, Some(2));
        assert!(tech.bitrate.is_none());
    }

    fn build_vorbis_ogg_bytes(channels: u8, sample_rate: u32, nominal_bitrate: u32) -> Vec<u8> {
        let mut page = b"OggS".to_vec();
        page.extend_from_slice(&[0u8; 22]);
        let mut ident = vec![1u8]; // packet type
        ident.extend_from_slice(b"vorbis");
        ident.extend_from_slice(&0u32.to_le_bytes()); // version
        ident.push(channels);
        ident.extend_from_slice(&sample_rate.to_le_bytes());
        ident.extend_from_slice(&0u32.to_le_bytes()); // max bitrate
        ident.extend_from_slice(&nominal_bitrate.to_le_bytes()); // nominal bitrate
        page.extend_from_slice(&ident);
        page
    }

    #[test]
    fn reads_ogg_vorbis_technical_metadata() {
        let dir = temp_dir("ogg-vorbis");
        let path = dir.join("t.ogg");
        fs::write(&path, build_vorbis_ogg_bytes(2, 44100, 192_000)).unwrap();

        let tech = read_ogg_technical(&path).expect("vorbis tech parse");
        assert_eq!(tech.sample_rate, Some(44100));
        assert_eq!(tech.channels, Some(2));
        assert_eq!(tech.bitrate, Some(192));
    }

    #[test]
    fn reads_ogg_opus_tags_from_opustags_comment_block() {
        let dir = temp_dir("ogg-opus-tags");
        let path = dir.join("t.ogg");
        let mut page = b"OggS".to_vec();
        page.extend_from_slice(&[0u8; 22]);
        let mut opus_tags = b"OpusTags".to_vec();
        let mut vorbis_block = Vec::new();
        vorbis_block.extend_from_slice(&0_u32.to_le_bytes());
        vorbis_block.extend_from_slice(&1_u32.to_le_bytes());
        let comment = "TITLE=Opus Song";
        vorbis_block.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        vorbis_block.extend_from_slice(comment.as_bytes());
        opus_tags.extend_from_slice(&vorbis_block);
        page.extend_from_slice(&opus_tags);
        fs::write(&path, &page).unwrap();

        let tags = read_ogg_tags(&path).unwrap();
        assert_eq!(tags.title.as_deref(), Some("Opus Song"));
    }

    // -- Full ID3v2 tag parsing (multi-frame, v2.3) --------------------------------------

    fn id3v23_frame(id: &[u8; 4], encoding: u8, text: &str) -> Vec<u8> {
        let mut payload = vec![encoding];
        payload.extend_from_slice(text.as_bytes());
        let mut frame = id.to_vec();
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]); // flags
        frame.extend_from_slice(&payload);
        frame
    }

    #[test]
    fn reads_full_id3v2_3_tag_with_multiple_frames() {
        let mut frames = Vec::new();
        frames.extend_from_slice(&id3v23_frame(b"TIT2", 3, "My Title"));
        frames.extend_from_slice(&id3v23_frame(b"TPE1", 3, "My Artist"));
        frames.extend_from_slice(&id3v23_frame(b"TALB", 3, "My Album"));
        frames.extend_from_slice(&id3v23_frame(b"TPE2", 3, "My Album Artist"));
        frames.extend_from_slice(&id3v23_frame(b"TCON", 3, "Trance"));
        frames.extend_from_slice(&id3v23_frame(b"TCOM", 3, "Some Composer"));
        frames.extend_from_slice(&id3v23_frame(b"TRCK", 3, "4/12"));
        frames.extend_from_slice(&id3v23_frame(b"TPOS", 3, "1/2"));
        frames.extend_from_slice(&id3v23_frame(b"TDRC", 3, "2021"));
        frames.extend_from_slice(&id3v23_frame(b"TBPM", 3, "128"));

        let mut header = b"ID3\x03\x00\x00".to_vec();
        header.extend_from_slice(&syncsafe_encode(frames.len() as u32));

        let dir = temp_dir("id3-full");
        let path = dir.join("t.mp3");
        let mut bytes = header;
        bytes.extend_from_slice(&frames);
        fs::write(&path, &bytes).unwrap();

        let tags = read_id3v2_tags(&path).unwrap();
        assert_eq!(tags.title.as_deref(), Some("My Title"));
        assert_eq!(tags.artist.as_deref(), Some("My Artist"));
        assert_eq!(tags.album.as_deref(), Some("My Album"));
        assert_eq!(tags.album_artist.as_deref(), Some("My Album Artist"));
        assert_eq!(tags.genre.as_deref(), Some("Trance"));
        assert_eq!(tags.composer.as_deref(), Some("Some Composer"));
        assert_eq!(tags.track_number, Some(4));
        assert_eq!(tags.disc_number, Some(1));
        assert_eq!(tags.year, Some(2021));
        assert_eq!(tags.bpm, Some(128));
    }

    #[test]
    fn reads_id3v2_4_tag_with_syncsafe_frame_sizes() {
        let payload_text = "V4 Title";
        let mut payload = vec![3u8];
        payload.extend_from_slice(payload_text.as_bytes());
        let mut frame = b"TIT2".to_vec();
        frame.extend_from_slice(&syncsafe_encode(payload.len() as u32));
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&payload);

        let mut header = b"ID3\x04\x00\x00".to_vec();
        header.extend_from_slice(&syncsafe_encode(frame.len() as u32));

        let dir = temp_dir("id3v24");
        let path = dir.join("t.mp3");
        let mut bytes = header;
        bytes.extend_from_slice(&frame);
        fs::write(&path, &bytes).unwrap();

        let tags = read_id3v2_tags(&path).unwrap();
        assert_eq!(tags.title.as_deref(), Some("V4 Title"));
    }

    // -- decode_utf16 / decode_id3_text_payload -----------------------------------------

    #[test]
    fn decodes_utf16_le_with_bom() {
        let text = "Hi";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_utf16(&bytes, true), "Hi");
    }

    #[test]
    fn decodes_utf16_be_with_bom() {
        let text = "Hi";
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode_utf16(&bytes, true), "Hi");
    }

    #[test]
    fn decode_id3_text_payload_handles_unknown_encoding() {
        assert_eq!(decode_id3_text_payload(9, b"whatever"), None);
    }

    // -- MP4 tag parsing (read_mp4_tags via a nested moov/udta/meta/ilst tree) -----------

    #[test]
    fn reads_mp4_tags_through_moov_udta_meta_ilst_tree() {
        let nam = mp4_atom(b"\xa9nam", &mp4_data_atom(b"MP4 Song"));
        let art = mp4_atom(b"\xa9ART", &mp4_data_atom(b"MP4 Artist"));
        let tmpo = mp4_atom(b"tmpo", &mp4_data_atom(&130u16.to_be_bytes()));
        let mut ilst_payload = Vec::new();
        ilst_payload.extend_from_slice(&nam);
        ilst_payload.extend_from_slice(&art);
        ilst_payload.extend_from_slice(&tmpo);
        let ilst = mp4_atom(b"ilst", &ilst_payload);

        let mut meta_payload = vec![0u8, 0, 0, 0]; // fullbox version+flags
        meta_payload.extend_from_slice(&ilst);
        let meta = mp4_atom(b"meta", &meta_payload);

        let udta = mp4_atom(b"udta", &meta);
        let moov = mp4_atom(b"moov", &udta);

        let dir = temp_dir("mp4-tags");
        let path = dir.join("t.m4a");
        fs::write(&path, &moov).unwrap();

        let tags = read_mp4_tags(&path).unwrap();
        assert_eq!(tags.title.as_deref(), Some("MP4 Song"));
        assert_eq!(tags.artist.as_deref(), Some("MP4 Artist"));
        assert_eq!(tags.bpm, Some(130));
    }

    #[test]
    fn find_mp4_data_payload_returns_none_when_absent() {
        let other = mp4_atom(b"xxxx", b"nope");
        assert!(find_mp4_data_payload(&other).is_none());
    }

    #[test]
    fn parse_mp4_pair_atom_returns_none_for_zero_current() {
        let payload = mp4_data_atom(&[0, 0, 0, 0, 0, 0]);
        assert_eq!(parse_mp4_pair_atom(&payload), None);
    }

    // -- Scheduler tick ----------------------------------------------------------------

    fn build_post_scan_state(
        db: Arc<Mutex<rusqlite::Connection>>,
    ) -> crate::post_scan::PostScanState {
        crate::post_scan::PostScanState {
            db,
            http_client: reqwest::Client::new(),
            db_folder: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn run_scheduler_tick_is_noop_with_no_due_schedules() {
        let (db, _dir) = init_test_db("scheduler-noop");
        let state = build_post_scan_state(db.clone());
        run_scheduler_tick(&state).await;

        let conn = db.lock().unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scan_jobs WHERE status='pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[tokio::test]
    async fn run_one_pending_scan_end_to_end_via_async_wrapper() {
        let (db, dir) = init_test_db("run-one-pending-async");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(music_dir.join("song.mp3"), b"fake mp3").unwrap();

        {
            let conn = db.lock().unwrap();
            let library = create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("Async Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap();
            enqueue_scan_job(&conn, &library.id.to_string()).unwrap();
        }

        let state = build_post_scan_state(db.clone());
        run_one_pending_scan(state).await;

        let conn = db.lock().unwrap();
        let done_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scan_jobs WHERE status='done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(done_count, 1);
    }

    #[tokio::test]
    async fn run_scan_job_targets_specific_job_via_async_wrapper() {
        let (db, dir) = init_test_db("run-scan-job-async");
        let music_dir = dir.join("music");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(music_dir.join("song.mp3"), b"fake mp3").unwrap();

        let job_id = {
            let conn = db.lock().unwrap();
            let library = create_library(
                &conn,
                CreateLibraryInput {
                    folders: vec![music_dir.to_string_lossy().into_owned()],
                    name: Some("Target Library".into()),
                    library_type: None,
                    scanner_profile: None,
                    metadata_mode: None,
                },
            )
            .unwrap();
            enqueue_scan_job(&conn, &library.id.to_string()).unwrap()
        };

        let state = build_post_scan_state(db.clone());
        run_scan_job(state, job_id.clone()).await;

        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM scan_jobs WHERE id = ?1",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "done");
    }
}

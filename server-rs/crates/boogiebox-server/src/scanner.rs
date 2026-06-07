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
pub async fn run_one_pending_scan(state: crate::post_scan::PostScanState) {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || run_one_pending_scan_blocking(&db)).await;
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
            interval.tick().await;
            run_scheduler_tick(&state).await;
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

fn run_one_pending_scan_blocking(db: &DbPool) -> Result<(), JobError> {
    let claimed = {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        boogiebox_db::jobs::recover_stale_scan_jobs(&conn, 60)?;
        boogiebox_db::jobs::claim_next_scan_job(&conn)?
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
        conn.execute_batch("BEGIN")?;
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
        conn.execute_batch("COMMIT")?;
        // Lock is released here between batches, allowing other handlers to proceed.
    }

    let conn = db.lock().unwrap_or_else(|p| p.into_inner());
    boogiebox_db::jobs::prune_missing_tracks(&conn, &claimed.library_id, &seen_paths)?;
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
        if h.len() < 13 {
            return None;
        }
        let ch = h[0] as i64;
        let sr = u32::from_le_bytes(h[1..5].try_into().ok()?) as i64;
        let nom_br = u32::from_le_bytes(h[9..13].try_into().ok()?) as i64;
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
}

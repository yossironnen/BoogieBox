//! Shared HTTP media responses used by browser playback (`/api/tracks/{id}/stream`), the DLNA MediaServer and
//! Play On renderers (`/media/r/{token}`): ranged file serving with ETag/304/HEAD, the constant-bitrate MP3
//! transcode, and a byte-exact seekable WAV (LPCM) transcode.
//!
//! Both transcodes are constant bitrate, so a renderer's `Range: bytes=N-` maps to a time offset and seeking
//! works without DLNA time-seek support.

use std::path::Path;

use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, DuplexStream};
use tokio_util::io::ReaderStream;

use crate::{artwork_cache::http_date_secs, ffmpeg, ErrorResponse};

/// Extra response headers (e.g. DLNA `transferMode.dlna.org`).
pub type ExtraHeaders<'a> = &'a [(&'static str, String)];

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Internal server error".into(),
            setup_required: None,
        }),
    )
        .into_response()
}

fn with_extra(
    mut builder: axum::http::response::Builder,
    extra: ExtraHeaders<'_>,
) -> axum::http::response::Builder {
    for (name, value) in extra {
        builder = builder.header(*name, value);
    }
    builder
}

fn range_not_satisfiable(total: u64) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{total}"))
        .header(header::ACCEPT_RANGES, "bytes")
        .body(Body::empty())
        .unwrap_or_else(|_| internal_error())
}

/// Serves a file with `Range`, `ETag`/`If-None-Match` (304) and `HEAD` support. The file is streamed, never
/// loaded into memory.
pub async fn serve_file(
    path: &Path,
    mime: &str,
    req_headers: &HeaderMap,
    head_only: bool,
    extra: ExtraHeaders<'_>,
) -> Response {
    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(_) => return internal_error(),
    };
    let file_size = meta.len();
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = format!("\"{file_size}-{modified_secs}\"");
    let last_modified = http_date_secs(modified_secs);

    // Conditional requests so browsers/clients get 304 on unchanged files.
    if let Some(inm) = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if inm == etag || inm == "*" {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .header("Last-Modified", &last_modified)
                .body(Body::empty())
                .unwrap_or_else(|_| internal_error());
        }
    }

    let range = req_headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, start, end) = match range {
        Some(rh) => match ffmpeg::parse_byte_range(rh, file_size) {
            None => return range_not_satisfiable(file_size),
            Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e),
        },
        None => (StatusCode::OK, 0, file_size.saturating_sub(1)),
    };
    let len = if file_size == 0 { 0 } else { end - start + 1 };

    let mut builder = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, len.to_string())
        .header(header::CONTENT_TYPE, mime)
        .header(header::ETAG, &etag)
        .header("Last-Modified", &last_modified);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{file_size}"),
        );
    }
    builder = with_extra(builder, extra);
    if head_only {
        return builder
            .body(Body::empty())
            .unwrap_or_else(|_| internal_error());
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return internal_error(),
    };
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return internal_error();
    }
    builder
        .body(Body::from_stream(ReaderStream::new(file.take(len))))
        .unwrap_or_else(|_| internal_error())
}

/// How to shape a child's stdout into the response body.
struct BridgeShape {
    /// Bytes emitted before the child's output (e.g. part of a WAV header).
    prefix: Vec<u8>,
    /// Child output bytes to drop first (sub-frame alignment).
    skip: u64,
    /// Exact body length: truncate extra output, pad a short one with zero bytes (silence for PCM).
    exact_len: Option<u64>,
}

/// Bridges a sync `std::process::Child` stdout into an async stream.
///
/// `std::process` avoids the IOCP named-pipe incompatibility with FFmpeg 8.x async muxer tasks (EINVAL after
/// ~1 KB on Tokio async pipes).
fn bridge_child(
    mut child: std::process::Child,
    shape: BridgeShape,
    log_id: String,
    log_ext: String,
) -> DuplexStream {
    let (read_side, write_side) = tokio::io::duplex(64 * 1024);
    let mut child_stdout = child.stdout.take().expect("stdout piped");
    let mut child_stderr = child.stderr.take();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let mut sync_writer = tokio_util::io::SyncIoBridge::new(write_side);
        let mut remaining = shape.exact_len.unwrap_or(u64::MAX);
        let mut write = |data: &[u8], remaining: &mut u64| -> bool {
            let n = (data.len() as u64).min(*remaining) as usize;
            if n == 0 {
                return false;
            }
            if sync_writer.write_all(&data[..n]).is_err() {
                return false;
            }
            *remaining -= n as u64;
            *remaining > 0
        };
        let mut open = shape.prefix.is_empty() || write(&shape.prefix, &mut remaining);
        let mut to_skip = shape.skip;
        let mut buf = [0u8; 8192];
        let mut bytes_written: u64 = 0;
        while open {
            match child_stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let skip_now = (to_skip as usize).min(n);
                    to_skip -= skip_now as u64;
                    let chunk = &buf[skip_now..n];
                    if !chunk.is_empty() {
                        bytes_written += chunk.len() as u64;
                        open = write(chunk, &mut remaining);
                    }
                }
                Err(_) => break,
            }
        }
        if open && shape.exact_len.is_some() {
            let zeros = [0u8; 8192];
            while remaining > 0 && write(&zeros, &mut remaining) {}
            // `write` returns false once remaining hits 0; flush the final partial chunk.
        }
        drop(sync_writer);
        if !open {
            // Client went away or the range is complete: stop the encoder instead of waiting for it.
            let _ = child.kill();
        }
        let mut stderr_text = String::new();
        if let Some(ref mut e) = child_stderr {
            let _ = e.read_to_string(&mut stderr_text);
        }
        match child.wait() {
            Ok(s) if !s.success() && open => tracing::warn!(
                track_id = %log_id,
                ext = %log_ext,
                status = ?s,
                bytes_written,
                stderr = %stderr_text.trim(),
                "ffmpeg audio transcode exited unsuccessfully"
            ),
            Ok(s) => tracing::info!(
                track_id = %log_id,
                ext = %log_ext,
                status = ?s,
                bytes_written,
                "ffmpeg audio transcode completed"
            ),
            Err(error) => tracing::warn!(
                track_id = %log_id,
                ext = %log_ext,
                bytes_written,
                error = %error,
                stderr = %stderr_text.trim(),
                "failed waiting for ffmpeg audio transcode"
            ),
        }
    });
    read_side
}

/// Parameters for the constant-bitrate MP3 transcode.
pub struct Mp3Transcode<'a> {
    /// Source file.
    pub file_path: &'a Path,
    /// Track duration in seconds (enables `Content-Length` and byte-range seeking).
    pub duration: Option<f64>,
    /// Raw `Range` header.
    pub range: Option<&'a str>,
    /// `transcodeQuality` (`high` = 320 kbps, otherwise 192 kbps).
    pub quality: &'a str,
    /// Apply loudness normalization.
    pub replay_gain: bool,
    /// Answer a `HEAD` request (headers only, no encoder spawned).
    pub head_only: bool,
    /// Track id for logs.
    pub log_id: &'a str,
    /// Source extension for logs.
    pub log_ext: &'a str,
    /// Extra response headers.
    pub extra: ExtraHeaders<'a>,
}

/// Streams a CBR MP3 transcode, mapping `Range` offsets to seek times.
pub async fn serve_mp3_transcode(req: Mp3Transcode<'_>) -> Response {
    let id = req.log_id;
    let track_ext = req.log_ext;
    let quality = req.quality;
    let total_bytes = req.duration.and_then(|duration| {
        if duration > 0.0 {
            let total = ffmpeg::transcoded_total_bytes(duration, quality);
            (total > 0).then_some(total)
        } else {
            None
        }
    });
    let parsed_transcode_range = req
        .range
        .and_then(|rh| total_bytes.and_then(|total| ffmpeg::parse_byte_range(rh, total)));
    let seek_seconds = parsed_transcode_range
        .filter(|(start, _)| *start > 0)
        .map(|(start, _)| ffmpeg::byte_offset_to_seconds(start, quality))
        .unwrap_or(0.0);
    if req.range.is_some() && total_bytes.is_some() && parsed_transcode_range.is_none() {
        tracing::warn!(
            track_id = %id,
            ext = %track_ext,
            range = req.range.unwrap_or(""),
            duration = req.duration.unwrap_or(0.0),
            estimated_total_bytes = total_bytes.unwrap_or(0),
            "invalid audio transcode range request"
        );
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::empty())
            .unwrap_or_else(|_| internal_error());
    }

    let start_byte = ffmpeg::transcoded_byte_offset(seek_seconds, quality);
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::ACCEPT_RANGES, "bytes")
        .header("Cache-Control", "no-cache");
    if req.range.is_some() {
        if let (Some(total), Some((range_start, range_end))) = (total_bytes, parsed_transcode_range)
        {
            let response_start = start_byte.max(range_start);
            let response_end = range_end.max(response_start).min(total.saturating_sub(1));
            let remaining = response_end.saturating_sub(response_start) + 1;
            tracing::info!(
                track_id = %id,
                ext = %track_ext,
                status = 206,
                content_range = %format!("bytes {response_start}-{response_end}/{total}"),
                content_length = remaining,
                "audio transcode range response"
            );
            builder = builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {response_start}-{response_end}/{total}"),
                )
                .header(header::CONTENT_LENGTH, remaining.to_string());
        } else {
            tracing::info!(
                track_id = %id,
                ext = %track_ext,
                status = 200,
                duration = req.duration.unwrap_or(0.0),
                "audio transcode range requested without known duration"
            );
            builder = builder.status(StatusCode::OK);
        }
    } else {
        tracing::info!(
            track_id = %id,
            ext = %track_ext,
            status = 200,
            content_length = total_bytes.unwrap_or(0),
            "audio transcode full response"
        );
        builder = builder.status(StatusCode::OK);
        if let Some(total) = total_bytes {
            builder = builder.header(header::CONTENT_LENGTH, total.to_string());
        }
    }
    builder = with_extra(builder, req.extra);
    if req.head_only {
        return builder
            .body(Body::empty())
            .unwrap_or_else(|_| internal_error());
    }

    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    tracing::info!(
        track_id = %id,
        ext = %track_ext,
        quality = %quality,
        replay_gain = req.replay_gain,
        seek_seconds,
        duration = req.duration.unwrap_or(0.0),
        estimated_total_bytes = total_bytes.unwrap_or(0),
        ffmpeg = %ffmpeg_path.display(),
        "audio stream using ffmpeg transcode"
    );
    let child = match ffmpeg::spawn_transcode(
        &ffmpeg_path,
        req.file_path,
        seek_seconds,
        quality,
        req.replay_gain,
    ) {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(
                track_id = %id,
                ext = %track_ext,
                ffmpeg = %ffmpeg_path.display(),
                error = %error,
                "failed to spawn ffmpeg audio transcode"
            );
            return internal_error();
        }
    };
    tracing::info!(track_id = %id, ext = %track_ext, pid = child.id(), "spawned ffmpeg audio transcode");
    let read_side = bridge_child(
        child,
        BridgeShape {
            prefix: Vec::new(),
            skip: 0,
            exact_len: None,
        },
        id.to_string(),
        track_ext.to_string(),
    );
    builder
        .body(Body::from_stream(ReaderStream::new(read_side)))
        .unwrap_or_else(|_| internal_error())
}

/// Size of the canonical RIFF/WAVE header written before PCM data.
pub const WAV_HEADER_LEN: u64 = 44;

/// Byte layout of a synthesized PCM WAV stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavLayout {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample (16 or 24).
    pub bits: u16,
    /// Bytes per sample frame.
    pub block_align: u64,
    /// Bytes per second.
    pub byte_rate: u64,
    /// PCM data size in bytes (whole frames).
    pub data_len: u64,
}

impl WavLayout {
    /// Layout for `duration` seconds of PCM. Rates/channels/depth are clamped to sane WAV values.
    pub fn new(duration: f64, sample_rate: u32, channels: u16, bits: u16) -> Self {
        let sample_rate = sample_rate.clamp(8_000, 192_000);
        let channels = channels.clamp(1, 2);
        let bits = if bits >= 24 { 24 } else { 16 };
        let block_align = channels as u64 * (bits as u64 / 8);
        let byte_rate = sample_rate as u64 * block_align;
        let frames = (duration.max(0.0) * sample_rate as f64).ceil() as u64;
        Self {
            sample_rate,
            channels,
            bits,
            block_align,
            byte_rate,
            data_len: frames * block_align,
        }
    }

    /// Total stream size (header + data).
    pub fn total_len(&self) -> u64 {
        WAV_HEADER_LEN + self.data_len
    }

    /// The 44-byte PCM WAV header.
    pub fn header(&self) -> [u8; 44] {
        let mut h = [0u8; 44];
        let data_len = self.data_len.min(u32::MAX as u64 - 36) as u32;
        h[0..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
        h[8..12].copy_from_slice(b"WAVE");
        h[12..16].copy_from_slice(b"fmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
        h[22..24].copy_from_slice(&self.channels.to_le_bytes());
        h[24..28].copy_from_slice(&self.sample_rate.to_le_bytes());
        h[28..32].copy_from_slice(&(self.byte_rate as u32).to_le_bytes());
        h[32..34].copy_from_slice(&(self.block_align as u16).to_le_bytes());
        h[34..36].copy_from_slice(&self.bits.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&data_len.to_le_bytes());
        h
    }

    /// Where a byte offset lands: `(header bytes to emit, seek seconds, PCM bytes to skip)`.
    pub fn locate(&self, offset: u64) -> (Vec<u8>, f64, u64) {
        if offset < WAV_HEADER_LEN {
            return (self.header()[offset as usize..].to_vec(), 0.0, 0);
        }
        let pcm = offset - WAV_HEADER_LEN;
        let aligned = pcm - pcm % self.block_align;
        (
            Vec::new(),
            aligned as f64 / self.byte_rate as f64,
            pcm - aligned,
        )
    }
}

/// Parameters for the seekable PCM WAV transcode.
pub struct WavTranscode<'a> {
    /// Source file.
    pub file_path: &'a Path,
    /// Stream layout (duration-derived size).
    pub layout: WavLayout,
    /// Raw `Range` header.
    pub range: Option<&'a str>,
    /// Answer a `HEAD` request.
    pub head_only: bool,
    /// Id for logs.
    pub log_id: &'a str,
    /// Extra response headers.
    pub extra: ExtraHeaders<'a>,
}

/// Streams lossless PCM WAV with exact byte-range support (renderers seek by bytes).
pub async fn serve_wav_transcode(req: WavTranscode<'_>) -> Response {
    let layout = req.layout;
    let total = layout.total_len();
    let (status, start, end) = match req.range {
        Some(rh) => match ffmpeg::parse_byte_range(rh, total) {
            None => return range_not_satisfiable(total),
            Some((s, e)) => (StatusCode::PARTIAL_CONTENT, s, e),
        },
        None => (StatusCode::OK, 0, total - 1),
    };
    let len = end - start + 1;
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, len.to_string())
        .header("Cache-Control", "no-cache");
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    builder = with_extra(builder, req.extra);
    if req.head_only {
        return builder
            .body(Body::empty())
            .unwrap_or_else(|_| internal_error());
    }

    let (prefix, seek_seconds, skip) = layout.locate(start);
    let ffmpeg_path = ffmpeg::resolve_ffmpeg();
    let child = match ffmpeg::spawn_pcm(
        &ffmpeg_path,
        req.file_path,
        seek_seconds,
        layout.sample_rate,
        layout.channels,
        layout.bits,
    ) {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(track_id = %req.log_id, error = %error, "failed to spawn ffmpeg PCM transcode");
            return internal_error();
        }
    };
    tracing::info!(
        track_id = %req.log_id,
        seek_seconds,
        sample_rate = layout.sample_rate,
        bits = layout.bits,
        range_start = start,
        content_length = len,
        "audio stream using ffmpeg PCM WAV transcode"
    );
    let read_side = bridge_child(
        child,
        BridgeShape {
            prefix,
            skip,
            exact_len: Some(len),
        },
        req.log_id.to_string(),
        "wav".to_string(),
    );
    builder
        .body(Body::from_stream(ReaderStream::new(read_side)))
        .unwrap_or_else(|_| internal_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-media-serve-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    async fn body(resp: Response) -> Vec<u8> {
        to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    #[tokio::test]
    async fn serve_file_full_range_head_and_304() {
        let p = temp_file("a.flac", b"0123456789");
        let extra = [("transferMode.dlna.org", "Streaming".to_string())];

        let full = serve_file(&p, "audio/flac", &HeaderMap::new(), false, &extra).await;
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(full.headers()["content-length"], "10");
        assert_eq!(full.headers()["transferMode.dlna.org"], "Streaming");
        let etag = full.headers()["etag"].to_str().unwrap().to_string();
        assert_eq!(body(full).await, b"0123456789");

        let part = serve_file(
            &p,
            "audio/flac",
            &headers(&[("range", "bytes=2-4")]),
            false,
            &[],
        )
        .await;
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(body(part).await, b"234");

        let open = serve_file(
            &p,
            "audio/flac",
            &headers(&[("range", "bytes=7-")]),
            false,
            &[],
        )
        .await;
        assert_eq!(body(open).await, b"789");

        let head = serve_file(
            &p,
            "audio/flac",
            &headers(&[("range", "bytes=0-")]),
            true,
            &[],
        )
        .await;
        assert_eq!(head.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(head.headers()["content-length"], "10");
        assert!(body(head).await.is_empty());

        let bad = serve_file(
            &p,
            "audio/flac",
            &headers(&[("range", "bytes=50-")]),
            false,
            &[],
        )
        .await;
        assert_eq!(bad.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(bad.headers()["content-range"], "bytes */10");

        let not_modified = serve_file(
            &p,
            "audio/flac",
            &headers(&[("if-none-match", etag.as_str())]),
            false,
            &[],
        )
        .await;
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);

        let missing = serve_file(
            &p.with_file_name("nope"),
            "audio/flac",
            &HeaderMap::new(),
            false,
            &[],
        )
        .await;
        assert_eq!(missing.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn serve_file_handles_empty_files() {
        let p = temp_file("empty.mp3", b"");
        let r = serve_file(&p, "audio/mpeg", &HeaderMap::new(), false, &[]).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["content-length"], "0");
        assert!(body(r).await.is_empty());
    }

    #[test]
    fn wav_layout_header_and_locate() {
        let l = WavLayout::new(2.0, 44_100, 2, 16);
        assert_eq!(l.block_align, 4);
        assert_eq!(l.byte_rate, 176_400);
        assert_eq!(l.data_len, 352_800);
        assert_eq!(l.total_len(), 352_844);
        let h = l.header();
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(h[4..8].try_into().unwrap()),
            36 + 352_800
        );
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 44_100);
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 352_800);

        let (prefix, t, skip) = l.locate(10);
        assert_eq!(prefix.len(), 34);
        assert_eq!((t, skip), (0.0, 0));
        let (prefix, t, skip) = l.locate(44 + 176_400 + 3);
        assert!(prefix.is_empty());
        assert_eq!(t, 1.0);
        assert_eq!(skip, 3);

        let hi = WavLayout::new(1.0, 500_000, 6, 32);
        assert_eq!((hi.sample_rate, hi.channels, hi.bits), (192_000, 2, 24));
        assert_eq!(hi.block_align, 6);
        assert_eq!(WavLayout::new(-1.0, 1, 0, 8).data_len, 0);
    }

    #[tokio::test]
    async fn transcode_heads_and_bad_ranges_never_spawn_ffmpeg() {
        let p = temp_file("a.aiff", b"not really audio");
        let mp3 = serve_mp3_transcode(Mp3Transcode {
            file_path: &p,
            duration: Some(10.0),
            range: Some("bytes=0-"),
            quality: "high",
            replay_gain: false,
            head_only: true,
            log_id: "t1",
            log_ext: "aiff",
            extra: &[],
        })
        .await;
        assert_eq!(mp3.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(mp3.headers()["content-type"], "audio/mpeg");
        assert_eq!(mp3.headers()["content-range"], "bytes 0-399999/400000");

        let mp3_full = serve_mp3_transcode(Mp3Transcode {
            file_path: &p,
            duration: None,
            range: None,
            quality: "low",
            replay_gain: false,
            head_only: true,
            log_id: "t1",
            log_ext: "aiff",
            extra: &[],
        })
        .await;
        assert_eq!(mp3_full.status(), StatusCode::OK);
        assert!(mp3_full.headers().get("content-length").is_none());

        let mp3_bad = serve_mp3_transcode(Mp3Transcode {
            file_path: &p,
            duration: Some(1.0),
            range: Some("bytes=999999-"),
            quality: "high",
            replay_gain: false,
            head_only: false,
            log_id: "t1",
            log_ext: "aiff",
            extra: &[],
        })
        .await;
        assert_eq!(mp3_bad.status(), StatusCode::RANGE_NOT_SATISFIABLE);

        let layout = WavLayout::new(1.0, 44_100, 2, 16);
        let wav = serve_wav_transcode(WavTranscode {
            file_path: &p,
            layout,
            range: Some("bytes=100-199"),
            head_only: true,
            log_id: "t1",
            extra: &[("transferMode.dlna.org", "Streaming".into())],
        })
        .await;
        assert_eq!(wav.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(wav.headers()["content-length"], "100");
        assert_eq!(
            wav.headers()["content-range"],
            format!("bytes 100-199/{}", layout.total_len())
        );
        let wav_bad = serve_wav_transcode(WavTranscode {
            file_path: &p,
            layout,
            range: Some("bytes=999999999-"),
            head_only: false,
            log_id: "t1",
            extra: &[],
        })
        .await;
        assert_eq!(wav_bad.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    /// Real ffmpeg round trip (skipped when ffmpeg is not resolvable).
    #[tokio::test]
    async fn wav_transcode_streams_exact_length_with_header() {
        if !ffmpeg::ffmpeg_available() {
            eprintln!("skipped: ffmpeg not available");
            return;
        }
        let ffmpeg_path = ffmpeg::resolve_ffmpeg();
        let dir = std::env::temp_dir().join(format!("bb-wav-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("tone.flac");
        // Test-only: the path comes from resolve_ffmpeg's fixed candidate list, never from input.
        // nosemgrep: boogiebox-rust-dynamic-command
        let ok = std::process::Command::new(&ffmpeg_path)
            .args([
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "flac",
            ])
            .arg(&src)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipped: could not generate a test tone");
            return;
        }
        let layout = WavLayout::new(1.0, 44_100, 2, 16);
        let full = serve_wav_transcode(WavTranscode {
            file_path: &src,
            layout,
            range: None,
            head_only: false,
            log_id: "tone",
            extra: &[],
        })
        .await;
        let bytes = body(full).await;
        assert_eq!(bytes.len() as u64, layout.total_len());
        assert_eq!(&bytes[..4], b"RIFF");

        let tail = serve_wav_transcode(WavTranscode {
            file_path: &src,
            layout,
            range: Some("bytes=10-"),
            head_only: false,
            log_id: "tone",
            extra: &[],
        })
        .await;
        let tail = body(tail).await;
        assert_eq!(tail.len() as u64, layout.total_len() - 10);
        assert_eq!(&tail[..34], &layout.header()[10..]);
    }
}

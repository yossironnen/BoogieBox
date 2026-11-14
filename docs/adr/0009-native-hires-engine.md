# ADR-0009: Native Hi-Res audio engine in the Windows desktop app

## Status
Proposed (decisions approved by the user 2026-11-14; plan `wip/native-hires-engine-plan.md` §3)

## Context
The desktop app (Tauri, Windows only) plays through WebView2 `<audio>`, which goes through the Windows mixer:
everything is resampled to the device rate and is never bit-perfect. Users with DACs want the file's own sample
rate and bit depth sent unchanged. The web UI (queue, smart playlists, radio, BoogieMix, scrobbling, Activity)
must keep working unchanged, and the Rust side of the app must not get broad powers over the server page.

## Decision Drivers
- Bit-perfect output at the track's own rate and bit depth (44.1–384 kHz, 16/24/32-bit)
- No second queue to keep in sync with the web UI
- The session cookie stays in the WebView; the remote origin gets only audio commands
- Hi-Res bandwidth (24/192 FLAC ≈ 9 Mbit/s) without IPC copies
- No 80 MB FFmpeg bundle in the desktop app

## Options Considered

### Queue ownership (D1)
- **Web UI keeps the queue; engine holds current + next** (like a Play On renderer with SetNext). Pros: no server or
  queue changes, all queue features keep working. Cons: playback needs the window open (D3).
- Engine-owned queue. Pros: playback with the window closed. Cons: a second queue to sync; large client rework.

### How the engine gets the bytes (D2)
- **Short-lived native stream tokens** (`POST /api/native/stream-tokens`) serving the untouched file with Range.
  Pros: Rust never handles the session cookie; Range seeking. Cons: one new server route.
- Read the WebView2 cookie store from Rust. Cons: fragile, widens what the shell touches.
- Proxy bytes over Tauri IPC. Cons: too slow for Hi-Res.

### Output API (D4)
- **WASAPI exclusive + shared via the `wasapi` crate** (event-driven). Pros: bypasses the mixer, per-track rate.
- `cpal`. Cons: shared mode only on Windows (to verify). ASIO: later, phase 8.

### Decoder (D5)
- **`symphonia`** (FLAC, ALAC, WAV, AIFF, MP3, AAC, Vorbis); the server sends FLAC at the source rate for formats
  it can't decode (Opus, WMA), labelled "Converted by server". Pros: pure Rust, no bundle growth.
- Bundle FFmpeg in the desktop app. Cons: +80 MB.

## Decision
We will add a native playback engine to the Windows desktop app as a new playback target beside the browser and
Play On: the web UI keeps the queue and sends current + next to the engine; the engine fetches the original file
through short-lived server-issued stream tokens, decodes with `symphonia` and writes to WASAPI (exclusive or
shared). The engine lives in a new crate `desktop/src-tauri/crates/boogiebox-audio` with no Tauri dependency, and
the server page gets a separate Tauri capability that allows only the audio commands.

## Rationale
It is the smallest change that reaches bit-perfect output: no queue rewrite, no cookie handling in Rust, no
FFmpeg bundle, and the remote origin's new powers are limited to audio.

## Consequences
- Bit-perfect playback on the desktop app, with a badge showing the path taken.
- Playback stops when the window is closed (D3); minimised keeps playing.
- Vinyl FX not available on the native output in v1 (D12); crossfade off in Pure mode (D11).
- A rate change between tracks reopens the stream (short gap, D8).
- New server route for native stream tokens; new capability file for the server origin.
- Follow-up: engine-owned queue if close-to-tray playback is wanted; ASIO/DSD (phase 8, separate decision).

## Related
- `wip/native-hires-engine-plan.md`
- ADR-0005 (bundled FFmpeg on the server), ADR-0006 (UI copy approval)

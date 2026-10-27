# ADR-0005: Bundle FFmpeg and prefer it over PATH

## Status
Accepted (2026-05-15; build choice 2026-08-14)

## Context
Transcoding, waveforms and BPM analysis need FFmpeg and FFprobe. Standalone installs should not require a global FFmpeg, but developer machines may rely on one. The Windows release first bundled gyan.dev's "full" build, which is much larger than BoogieBox needs.

## Decision Drivers
- Self-contained release, no manual FFmpeg install
- Small installer
- Developer setups keep working

## Options Considered

### Require FFmpeg on PATH
Pros:
- Nothing to bundle

Cons:
- An extra install step; versions differ between machines

### Bundle the "full" build
Pros:
- Every codec and filter available

Cons:
- Much larger installer for features BoogieBox never uses

### Bundle the "essentials" build, fall back to PATH
Pros:
- Self-contained and smaller
- Covers the audio codecs and muxing BoogieBox uses

Cons:
- Some rare codecs and filters are missing

## Decision
We will ship FFmpeg with the release (gyan.dev "essentials" on Windows, a static build on Linux, under `resources/ffmpeg`) and resolve it in this order: `BOOGIEBOX_FFMPEG_DIR`; `resources/ffmpeg` and `ffmpeg` beside the executable; `resources/ffmpeg` and `tools/ffmpeg` under the working directory; then the bare name on PATH.

## Rationale
Bundling removes a setup step, and the essentials build covers every audio feature at a fraction of the size.

## Consequences
- Server code uses `ffmpeg::resolve_ffmpeg()` / `resolve_ffprobe()`, never hardcoded command names
- Binaries live in gitignored `tools/` caches; changing builds is a re-download, not a tracked diff
- If a codec or filter is reported missing, first check whether the essentials build excludes it

## Related
- `server-rs/crates/boogiebox-server/src/ffmpeg.rs`
- `tools/ffmpeg/README.md`
- `docs/memory/DECISIONS.md` (2026-05-15, 2026-08-14)

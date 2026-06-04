# BoogieMix Deep Analysis Worker (Windows)

## Requirements
- Python 3.10+ (`python --version`)
- FFmpeg on PATH (`ffmpeg -version`)
- PowerShell

## Automatic setup
Tries CUDA PyTorch when `nvidia-smi` is available, falls back to CPU PyTorch, and downloads the default Demucs model into the app-local cache.

```powershell
cd Services\boogiemix\python
.\bootstrap_env.ps1 -Auto -PrimeDemucsModel
```

## CPU setup
```powershell
cd Services\boogiemix\python
.\bootstrap_env.ps1 -CpuOnly
```

## GPU setup (CUDA)
```powershell
cd Services\boogiemix\python
.\bootstrap_env.ps1 -Cuda -PrimeDemucsModel
```

## Runtime checks
Server startup probes:
- python availability
- ffmpeg availability
- demucs callable
- torch import
- CUDA GPU availability
The installer uses automatic setup when the optional BoogieMix feature is selected.

If unavailable, BoogieMix falls back to standard analysis automatically.

## Notes
- This worker is analysis-only (no playback path use).
- Deep analysis runs in background jobs and cached DB rows.
- Demucs model weights are cached under `model-cache\torch` beside this script so service accounts can reuse the installer download.
- Temporary demucs outputs are cleaned unless `boogiemixDeepAnalysisCleanupTemp=false`.

## Worker output (schema v2)
The worker prints a single JSON object on stdout for the Rust deep-analysis worker. Fields:

- `analysis_schema_version` — always `2`.
- `stem_feature_json` — 1 Hz envelopes for `vocals`, `drums`, `bass`, `other`, plus a `summary` block (densities, instrumentalRatio, hasLongIntro/Outro).
- `vocal_windows_json`, `drum_windows_json`, `bass_windows_json` — windows where each stem is meaningfully present (`start`, `end`, `strength`, `average`).
- `section_json` — heuristic section guesses (`intro`, `verse`, `chorus`, `breakdown`, `build`, `drop`, `outro`, `unknown`) with `vocalDensity`, `drumDensity`, `energy`, `confidence`.
- `phrase_boundaries_json` — list of 16-beat phrase starts in seconds, derived from drum-onset autocorrelation.
- `transition_windows_json` — `intro`, `outro`, and instrumental safe-windows with `vocalRisk`, `drumContinuity`, `bassRisk`, `recommendedMinCrossfade`, `recommendedMaxCrossfade`.
- `intro_outro_refined_json`, `transition_hints_json` — convenience summaries.
- `confidence` — overall analysis confidence (low when Demucs fell back to synthetic).
- `energy_score_refined`, `bpm_refined`.

The worker enforces hard caps: max 128 windows per category, 64 sections, 256 phrase boundaries, ~60 KB total JSON. The Rust side rejects rows larger than 64 KB.

#!/usr/bin/env python
"""BoogieMix deep-analysis worker.

Runs Demucs (htdemucs by default) against a single audio file and emits a
schema v2 compact feature JSON object on stdout for the Rust deep-analysis
worker. The features are designed to drive transition planning without
storing audio: low-resolution stem envelopes, vocal/drum/bass windows,
section guesses, phrase boundary hints, and transition-safe candidate
windows.

Hard rules:
- never write more than ~60 KB of JSON
- never keep separated audio or temp WAVs after processing
- always emit schema v2 fields, even when Demucs fails (low confidence)
"""

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ANALYSIS_VERSION = 1
ANALYSIS_SCHEMA_VERSION = 3
DEFAULT_MODEL = "htdemucs"

ENVELOPE_HZ = 1.0
ONSET_HZ = 10.0
ENV_TARGET_SR = 8000

MIN_WINDOW_SEC = 4.0
MIN_SAFE_WINDOW_SEC = 8.0
MAX_TRANSITION_SEC = 32.0

MAX_WINDOWS = 128
MAX_SECTIONS = 64
MAX_PHRASES = 256
MAX_TIMELINE_POINTS = 600

TARGET_JSON_BYTES = 60 * 1024

# Camelot Wheel lookup tables (for harmonic mixing)
_CAMELOT_MAJOR = {
    'C': '8B', 'G': '9B', 'D': '10B', 'A': '11B', 'E': '12B', 'B': '1B',
    'F#': '2B', 'C#': '3B', 'G#': '4B', 'D#': '5B', 'A#': '6B', 'F': '7B',
}
_CAMELOT_MINOR = {
    'A': '8A', 'E': '9A', 'B': '10A', 'F#': '11A', 'C#': '12A', 'G#': '1A',
    'D#': '2A', 'A#': '3A', 'F': '4A', 'C': '5A', 'G': '6A', 'D': '7A',
}
_MAJOR_PROFILE = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88]
_MINOR_PROFILE = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17]
_NOTE_NAMES    = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']

try:
    import madmom  # noqa: F401
    MADMOM_AVAILABLE = True
except ImportError:
    MADMOM_AVAILABLE = False

VOCAL_PRESENT_THRESHOLD = 0.32
VOCAL_FREE_THRESHOLD = 0.18
DRUM_PRESENT_THRESHOLD = 0.30
BASS_PRESENT_THRESHOLD = 0.30


def run_cmd(cmd):
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return proc.returncode, proc.stdout, proc.stderr


def configure_model_cache():
    root = Path(__file__).resolve().parent
    torch_home = root / "model-cache" / "torch"
    torch_home.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("TORCH_HOME", str(torch_home))


def ffprobe_duration(file_path):
    code, out, _ = run_cmd([
        "ffprobe", "-v", "error", "-show_entries", "format=duration",
        "-of", "default=noprint_wrappers=1:nokey=1", file_path,
    ])
    if code != 0:
        return 0.0
    try:
        return float((out or "0").strip())
    except Exception:
        return 0.0


def run_demucs(file_path, model, use_gpu, temp_root):
    out_dir = tempfile.mkdtemp(prefix="demucs-", dir=temp_root)
    runner = Path(__file__).resolve().parent / "demucs_runner.py"
    args = [
        sys.executable, str(runner), "-n", model,
        "--device", "cuda" if use_gpu else "cpu",
        "--out", out_dir, file_path,
    ]
    code, _, err = run_cmd(args)
    return code == 0, out_dir, err


def extract_segment_clip(file_path, start_sec, duration_sec, out_path):
    """Extract a time segment from file_path to out_path using ffmpeg."""
    args = [
        "ffmpeg", "-y", "-ss", str(start_sec), "-t", str(duration_sec),
        "-i", file_path, "-ar", "44100", "-ac", "2", "-f", "wav", out_path,
    ]
    code, _, err = run_cmd(args)
    return code == 0, err


def run_demucs_segment(file_path, model, use_gpu, temp_root, duration_sec, segment_seconds):
    """Run Demucs only on the intro and outro segments, returning stitched stem arrays
    padded with zeros for the middle of the track.

    Returns (success, stems_dict_of_arrays, error_str) where each array is the full-length
    envelope at ENVELOPE_HZ, with zeros for the unprocessed middle region.
    """
    import numpy as np

    seg_dur = min(float(segment_seconds), duration_sec / 2.0)
    outro_start = max(seg_dur, duration_sec - seg_dur)
    full_len = max(1, int(round(duration_sec * ENVELOPE_HZ)))

    clip_dir = tempfile.mkdtemp(prefix="seg-clips-", dir=temp_root)
    intro_clip = os.path.join(clip_dir, "intro.wav")
    outro_clip = os.path.join(clip_dir, "outro.wav")

    ok_i, err_i = extract_segment_clip(file_path, 0.0, seg_dur, intro_clip)
    ok_o, err_o = extract_segment_clip(file_path, outro_start, seg_dur, outro_clip)
    if not ok_i or not ok_o:
        shutil.rmtree(clip_dir, ignore_errors=True)
        return False, {}, (err_i or err_o or "segment extraction failed")

    stem_arrays = {name: np.zeros(full_len, dtype=np.float32)
                   for name in ("vocals", "drums", "bass", "other")}
    any_ok = False

    for clip_path, seg_start in [(intro_clip, 0.0), (outro_clip, outro_start)]:
        ok, out_dir, err = run_demucs(clip_path, model, use_gpu, temp_root)
        if not ok:
            shutil.rmtree(clip_dir, ignore_errors=True)
            return False, {}, err
        stems = locate_stems(out_dir, model, clip_path)
        if stems:
            any_ok = True
            seg_offset_frames = int(round(seg_start * ENVELOPE_HZ))
            for name, path in stems.items():
                try:
                    data, sr = load_audio_mono(path)
                    env = normalize_envelope(rms_envelope(data, sr, ENVELOPE_HZ))
                    end_frame = min(seg_offset_frames + len(env), full_len)
                    copy_len = end_frame - seg_offset_frames
                    if copy_len > 0:
                        stem_arrays[name][seg_offset_frames:end_frame] = env[:copy_len]
                except Exception:
                    pass
        shutil.rmtree(out_dir, ignore_errors=True)

    shutil.rmtree(clip_dir, ignore_errors=True)
    if not any_ok:
        return False, {}, "no stems found in segment clips"

    # Return as pseudo-file paths dict but with pre-loaded arrays via a wrapper.
    # analyze_with_stems expects file paths; we use a different path here.
    return True, stem_arrays, None


def locate_stems(out_dir, model, source_path):
    base = Path(source_path).stem
    candidate = Path(out_dir) / model / base
    if not candidate.is_dir():
        for child in Path(out_dir).iterdir():
            if child.is_dir():
                for sub in child.iterdir():
                    if sub.is_dir() and sub.name == base:
                        candidate = sub
                        break
    stems = {}
    for name in ("vocals", "drums", "bass", "other"):
        path = candidate / f"{name}.wav"
        if path.is_file():
            stems[name] = path
    return stems if len(stems) >= 3 else {}


def load_audio_mono(path, target_sr=ENV_TARGET_SR):
    import numpy as np
    try:
        import soundfile as sf
        data, sr = sf.read(str(path), always_2d=True)
        data = data.mean(axis=1).astype(np.float32)
    except Exception:
        data, sr = _load_wav_stdlib(path)
    if sr > target_sr:
        factor = max(1, int(round(sr / target_sr)))
        if factor > 1:
            trim = (len(data) // factor) * factor
            if trim > 0:
                data = data[:trim].reshape(-1, factor).mean(axis=1).astype(np.float32)
            sr = sr // factor
    return data, sr


def _load_wav_stdlib(path):
    import wave
    import numpy as np
    with wave.open(str(path), "rb") as wf:
        sr = wf.getframerate()
        ch = wf.getnchannels()
        n = wf.getnframes()
        sw = wf.getsampwidth()
        raw = wf.readframes(n)
    if sw == 2:
        arr = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    elif sw == 4:
        arr = np.frombuffer(raw, dtype=np.int32).astype(np.float32) / 2147483648.0
    elif sw == 3:
        a = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
        ints = (a[:, 0].astype(np.int32)
                | (a[:, 1].astype(np.int32) << 8)
                | (a[:, 2].astype(np.int32) << 16))
        ints = np.where(ints & 0x800000, ints - 0x1000000, ints)
        arr = ints.astype(np.float32) / 8388608.0
    else:
        raise ValueError(f"unsupported sample width: {sw}")
    if ch > 1:
        arr = arr.reshape(-1, ch).mean(axis=1)
    return arr, sr


def rms_envelope(samples, sr, hz):
    import numpy as np
    if len(samples) == 0:
        return np.zeros(0, dtype=np.float32)
    frame_len = max(1, int(round(sr / hz)))
    n_frames = max(1, len(samples) // frame_len)
    trim = n_frames * frame_len
    block = samples[:trim].reshape(n_frames, frame_len)
    rms = np.sqrt(np.mean(block * block, axis=1) + 1e-12)
    return rms.astype(np.float32)


def normalize_envelope(env):
    import numpy as np
    if env.size == 0:
        return env
    ref = np.percentile(env, 95)
    if ref <= 1e-6:
        return np.zeros_like(env)
    out = env / ref
    return np.clip(out, 0.0, 1.0).astype(np.float32)


def round_list(values, decimals=3):
    return [round(float(v), decimals) for v in values]


def downsample_for_output(env, max_points=MAX_TIMELINE_POINTS):
    import numpy as np
    if env.size <= max_points:
        return env
    factor = max(1, int(math.ceil(env.size / max_points)))
    trim = (env.size // factor) * factor
    if trim == 0:
        return env
    return env[:trim].reshape(-1, factor).mean(axis=1).astype(np.float32)


def windows_from_threshold(env, hz, threshold, min_sec=MIN_WINDOW_SEC,
                           merge_gap_sec=1.5, mode="above"):
    if env.size == 0:
        return []
    step = 1.0 / hz
    windows = []
    start = None
    for i in range(env.size):
        v = float(env[i])
        active = v >= threshold if mode == "above" else v <= threshold
        if active and start is None:
            start = i
        if (not active or i == env.size - 1) and start is not None:
            end = i if not active else i + 1
            windows.append((start, end))
            start = None
    merged = []
    gap_steps = max(1, int(round(merge_gap_sec / step)))
    for a, b in windows:
        if merged and a - merged[-1][1] <= gap_steps:
            merged[-1] = (merged[-1][0], b)
        else:
            merged.append((a, b))
    out = []
    for a, b in merged:
        start_sec = a * step
        end_sec = b * step
        if end_sec - start_sec < min_sec:
            continue
        seg = env[a:b] if b > a else env[a:a + 1]
        avg = float(seg.mean()) if seg.size else 0.0
        peak = float(seg.max()) if seg.size else 0.0
        out.append({
            "start": round(start_sec, 2),
            "end": round(end_sec, 2),
            "strength": round(min(1.0, peak), 3),
            "average": round(min(1.0, avg), 3),
        })
    return out


def trim_to_max(items, limit):
    if len(items) <= limit:
        return items
    items_sorted = sorted(items, key=lambda w: w.get("strength", 0.0), reverse=True)
    return items_sorted[:limit]


def detect_sections(vocal, drums, bass, other, hz):
    import numpy as np
    n = min(vocal.size, drums.size, bass.size, other.size)
    if n < 4:
        return []
    step = 1.0 / hz
    feats = np.stack([vocal[:n], drums[:n], bass[:n], other[:n]], axis=1)
    win = max(3, int(round(4.0 * hz)))
    kernel = np.ones(win, dtype=np.float32) / win
    smoothed = np.stack(
        [np.convolve(feats[:, c], kernel, mode="same") for c in range(feats.shape[1])],
        axis=1,
    )
    deltas = np.linalg.norm(np.diff(smoothed, axis=0), axis=1)
    if deltas.size == 0:
        return []
    cooldown = max(3, int(round(6.0 * hz)))
    threshold = float(np.percentile(deltas, 80)) if deltas.size else 0.0
    boundaries = [0]
    last = 0
    for i, d in enumerate(deltas):
        if d >= threshold and i - last >= cooldown:
            boundaries.append(i + 1)
            last = i + 1
    if boundaries[-1] != n:
        boundaries.append(n)
    sections = []
    for idx in range(len(boundaries) - 1):
        a = boundaries[idx]
        b = boundaries[idx + 1]
        if b <= a:
            continue
        v = float(smoothed[a:b, 0].mean())
        d = float(smoothed[a:b, 1].mean())
        bb = float(smoothed[a:b, 2].mean())
        o = float(smoothed[a:b, 3].mean())
        energy = float(np.mean([d, bb, o]))
        kind = "unknown"
        is_first = idx == 0
        is_last = idx == len(boundaries) - 2
        if is_first and v < 0.2:
            kind = "intro"
        elif is_last and v < 0.2:
            kind = "outro"
        elif v >= 0.4 and d >= 0.3:
            kind = "chorus"
        elif v >= 0.25:
            kind = "verse"
        elif d < 0.2 and bb < 0.2:
            kind = "breakdown"
        elif energy >= 0.55 and v < 0.2:
            kind = "drop"
        elif d >= 0.3 and v < 0.25:
            kind = "build"
        sections.append({
            "kind": kind,
            "start": round(a * step, 2),
            "end": round(b * step, 2),
            "confidence": round(min(1.0, 0.4 + 0.4 * float(deltas[max(0, a - 1)]) / (threshold + 1e-6)), 3),
            "vocalDensity": round(v, 3),
            "drumDensity": round(d, 3),
            "energy": round(energy, 3),
        })
    return sections[:MAX_SECTIONS]


def detect_phrase_boundaries(drum_samples, sr, duration_sec, bpm_hint=None):
    import numpy as np
    if drum_samples.size == 0 or duration_sec <= 0:
        return [], None
    onset_env = rms_envelope(drum_samples, sr, ONSET_HZ)
    if onset_env.size < 8:
        return [], bpm_hint
    diff = np.diff(onset_env, prepend=onset_env[0])
    diff = np.clip(diff, 0.0, None)
    if diff.max() > 0:
        diff = diff / diff.max()
    bpm = bpm_hint if bpm_hint and 40.0 <= bpm_hint <= 220.0 else None
    if bpm is None:
        sig = diff - diff.mean()
        n = sig.size
        ac = np.correlate(sig, sig, mode="full")[n - 1:]
        ac = ac.astype(np.float32)
        ac[0] = 0
        lag_min = max(1, int(round(ONSET_HZ * 60.0 / 200.0)))
        lag_max = min(ac.size - 1, int(round(ONSET_HZ * 60.0 / 60.0)))
        if lag_max > lag_min:
            best = lag_min + int(np.argmax(ac[lag_min:lag_max]))
            if ac[best] > 0:
                bpm = 60.0 / (best / ONSET_HZ)
                bpm = float(np.clip(bpm, 60.0, 200.0))
    # Fallback: use librosa beat tracking on the drum stem (more robust than
    # autocorrelation when no bpm_hint is available).
    if not bpm:
        try:
            import librosa
            y = drum_samples.astype(np.float32)
            tempo, _ = librosa.beat.beat_track(y=y, sr=int(sr))
            tempo = float(np.atleast_1d(tempo)[0])
            # Print for diagnostics (captured by Rust on success too, appended after JSON)
            import sys
            print(f"[librosa_bpm] tempo={tempo:.2f}", file=sys.stderr)
            if 60.0 <= tempo <= 200.0:
                bpm = tempo
        except Exception as e:
            import sys
            print(f"[boogiemix] librosa beat_track failed: {e}", file=sys.stderr)
    if not bpm:
        return [], None
    beat_sec = 60.0 / bpm
    phrase_sec = beat_sec * 16.0
    if phrase_sec <= 1.0 or phrase_sec > duration_sec:
        return [], bpm
    onset_peaks = []
    threshold = float(np.percentile(diff, 92))
    for i in range(2, diff.size - 2):
        if diff[i] >= threshold and diff[i] >= diff[i - 1] and diff[i] >= diff[i + 1]:
            onset_peaks.append(i / ONSET_HZ)
    anchor = onset_peaks[0] if onset_peaks else 0.0
    anchor = max(0.0, min(anchor, phrase_sec))
    boundaries = []
    t = anchor
    while t < duration_sec and len(boundaries) < MAX_PHRASES:
        boundaries.append(round(t, 2))
        t += phrase_sec
    return boundaries, bpm


def transition_safe_windows(duration_sec, vocal_env, drum_env, bass_env, sections, hz, other_env=None):
    import numpy as np
    out = []

    def env_avg(env, a, b):
        ia = max(0, int(round(a * hz)))
        ib = min(env.size, int(round(b * hz)))
        if ib <= ia:
            return 0.0
        return float(env[ia:ib].mean())

    def stem_rms(a, b):
        """Return per-stem mean RMS over [a, b] seconds, capped at 4s window."""
        w_end = min(b, a + 4.0)
        return {
            "vocalsRms": round(env_avg(vocal_env, a, w_end), 3),
            "drumsRms":  round(env_avg(drum_env,  a, w_end), 3),
            "bassRms":   round(env_avg(bass_env,   a, w_end), 3),
            "otherRms":  round(env_avg(other_env,  a, w_end), 3) if other_env is not None else None,
        }

    intro_end = 0.0
    for i in range(vocal_env.size):
        if vocal_env[i] >= VOCAL_PRESENT_THRESHOLD:
            intro_end = i / hz
            break
    if intro_end <= 4.0:
        for s in sections:
            if s["kind"] == "intro":
                intro_end = max(intro_end, s["end"])
                break
    intro_end = min(intro_end, MAX_TRANSITION_SEC)
    if intro_end >= MIN_SAFE_WINDOW_SEC:
        vocal_risk = env_avg(vocal_env, 0.0, intro_end)
        drum_avg = env_avg(drum_env, 0.0, intro_end)
        bass_avg = env_avg(bass_env, 0.0, intro_end)
        out.append({
            "role": "intro",
            "start": 0.0,
            "end": round(intro_end, 2),
            "score": round(min(1.0, 0.7 + (1.0 - vocal_risk) * 0.3), 3),
            "vocalRisk": round(vocal_risk, 3),
            "drumContinuity": round(min(1.0, drum_avg), 3),
            "bassRisk": round(bass_avg, 3),
            "energy": round(env_avg(np.maximum(drum_env, bass_env), 0.0, intro_end), 3),
            "recommendedMinCrossfade": 4,
            "recommendedMaxCrossfade": int(min(intro_end, MAX_TRANSITION_SEC)),
            **stem_rms(0.0, intro_end),
        })

    outro_start = duration_sec
    for i in range(vocal_env.size - 1, -1, -1):
        if vocal_env[i] >= VOCAL_PRESENT_THRESHOLD:
            outro_start = (i + 1) / hz
            break
    if duration_sec - outro_start < 4.0:
        for s in reversed(sections):
            if s["kind"] == "outro":
                outro_start = min(outro_start, s["start"])
                break
    outro_len = max(0.0, duration_sec - outro_start)
    if outro_len >= MIN_SAFE_WINDOW_SEC:
        outro_len = min(outro_len, MAX_TRANSITION_SEC)
        outro_start = duration_sec - outro_len
        vocal_risk = env_avg(vocal_env, outro_start, duration_sec)
        drum_avg = env_avg(drum_env, outro_start, duration_sec)
        bass_avg = env_avg(bass_env, outro_start, duration_sec)
        out.append({
            "role": "outro",
            "start": round(outro_start, 2),
            "end": round(duration_sec, 2),
            "score": round(min(1.0, 0.7 + (1.0 - vocal_risk) * 0.3), 3),
            "vocalRisk": round(vocal_risk, 3),
            "drumContinuity": round(min(1.0, drum_avg), 3),
            "bassRisk": round(bass_avg, 3),
            "energy": round(env_avg(np.maximum(drum_env, bass_env), outro_start, duration_sec), 3),
            "recommendedMinCrossfade": 4,
            "recommendedMaxCrossfade": int(min(outro_len, MAX_TRANSITION_SEC)),
            **stem_rms(outro_start, duration_sec),
        })

    vocal_free = windows_from_threshold(vocal_env, hz, VOCAL_FREE_THRESHOLD,
                                        min_sec=MIN_SAFE_WINDOW_SEC, mode="below")
    for w in vocal_free:
        a, b = w["start"], w["end"]
        if a < intro_end + 2.0 or b > outro_start - 2.0:
            continue
        drum_avg = env_avg(drum_env, a, b)
        bass_avg = env_avg(bass_env, a, b)
        out.append({
            "role": "instrumental",
            "start": round(a, 2),
            "end": round(b, 2),
            "score": round(min(1.0, 0.55 + (1.0 - drum_avg) * 0.2 + (1.0 - bass_avg) * 0.15), 3),
            "vocalRisk": round(w["average"], 3),
            "drumContinuity": round(min(1.0, drum_avg), 3),
            "bassRisk": round(bass_avg, 3),
            "energy": round(env_avg(np.maximum(drum_env, bass_env), a, b), 3),
            "recommendedMinCrossfade": 4,
            "recommendedMaxCrossfade": int(min(b - a, MAX_TRANSITION_SEC)),
            **stem_rms(a, b),
        })

    return trim_to_max(out, MAX_WINDOWS)


def extract_neural_embedding(file_path, duration_sec):
    """Compute a mel-spectrogram energy/danceability embedding using librosa.

    Returns a dict with energy_neural, danceability, embedding_16d (PCA-projected),
    and model_version, or None on failure.
    """
    try:
        import numpy as np

        load_dur = min(120.0, duration_sec) if duration_sec > 0 else 120.0
        y, sr = _load_audio_safe(file_path, 11025, load_dur)

        # 128-band mel-spectrogram via pure-numpy STFT (no scipy FFT)
        _N, _H = 1024, 256
        _y_pad = np.pad(y, _N // 2, mode='reflect')
        _nf = 1 + (len(_y_pad) - _N) // _H
        _frames = np.lib.stride_tricks.as_strided(
            _y_pad, shape=(_nf, _N),
            strides=(_y_pad.strides[0] * _H, _y_pad.strides[0]),
        )
        _win = np.hanning(_N).astype(np.float32)
        power_spec = np.abs(np.fft.rfft(np.ascontiguousarray(_frames) * _win, axis=1)) ** 2  # (nf, N//2+1)
        # Build mel filterbank manually
        n_mels = 128
        f_min, f_max = 0.0, sr / 2.0
        mel_min = 2595 * np.log10(1 + f_min / 700)
        mel_max = 2595 * np.log10(1 + f_max / 700)
        mel_points = np.linspace(mel_min, mel_max, n_mels + 2)
        hz_points = 700 * (10 ** (mel_points / 2595) - 1)
        bin_points = np.floor((_N + 1) * hz_points / sr).astype(int)
        mel_fb = np.zeros((n_mels, _N // 2 + 1), dtype=np.float32)
        for m in range(1, n_mels + 1):
            lo, ctr, hi = bin_points[m-1], bin_points[m], bin_points[m+1]
            if ctr > lo:
                mel_fb[m-1, lo:ctr] = (np.arange(lo, ctr) - lo) / (ctr - lo)
            if hi > ctr:
                mel_fb[m-1, ctr:hi] = (hi - np.arange(ctr, hi)) / (hi - ctr)
        mel_power = mel_fb.dot(power_spec.T)  # (128, nf)
        mel_db = 10 * np.log10(np.maximum(mel_power, 1e-10))
        mel_db -= mel_db.max()

        mean_v = np.mean(mel_db, axis=1).astype(np.float32)
        std_v  = np.std(mel_db, axis=1).astype(np.float32)
        feature_v = np.concatenate([mean_v, std_v])

        high_energy = float(np.mean(mel_db[40:, :]))
        energy_neural = round(float(np.clip((high_energy + 80.0) / 80.0, 0.0, 1.0)), 3)

        # Danceability: onset strength from frame-to-frame energy delta (pure numpy)
        frame_energy = power_spec.sum(axis=1)
        onset_env = np.maximum(np.diff(frame_energy, prepend=frame_energy[0]), 0)
        onset_env /= (onset_env.max() + 1e-8)
        threshold = onset_env.mean() + 0.3 * onset_env.std()
        peaks = np.where((onset_env[1:-1] > onset_env[:-2]) &
                         (onset_env[1:-1] > onset_env[2:]) &
                         (onset_env[1:-1] > threshold))[0] + 1
        if len(peaks) > 2:
            ioi = np.diff(peaks.astype(np.float32))
            cv = float(np.std(ioi) / (np.mean(ioi) + 1e-6))
            danceability = round(float(np.clip(1.0 - cv * 0.5, 0.0, 1.0)), 3)
        else:
            danceability = 0.5

        # PCA projection to 16 dimensions using a shipped or identity PCA matrix
        pca_path = Path(__file__).resolve().parent / 'mel_pca_v1.npy'
        if pca_path.exists():
            pca_matrix = np.load(str(pca_path))  # shape (16, 256)
            if pca_matrix.shape == (16, 256):
                embedding_16d = pca_matrix.dot(feature_v).tolist()
            else:
                embedding_16d = feature_v[:16].tolist()
        else:
            # No PCA matrix: use first 16 mel mean values (centroid proxy)
            embedding_16d = [round(float(x), 4) for x in mean_v[:16]]

        return {
            'energy_neural': energy_neural,
            'danceability': danceability,
            'valence': None,  # placeholder for future model
            'embedding_16d': [round(float(x), 4) for x in embedding_16d],
            'model_version': 'librosa-mel-pca-v1',
        }
    except Exception as exc:
        print(f"[boogiemix] neural embedding failed: {exc}", file=sys.stderr)
        return None


def detect_beat_grid(file_path, use_madmom=True):
    """Neural beat tracking via madmom DBNBeatTrackingProcessor.

    Returns a dict with beats, bpm_neural, downbeats, phrase_boundaries_neural,
    or None if madmom is unavailable or tracking fails.
    """
    if not use_madmom or not MADMOM_AVAILABLE:
        return None
    try:
        import numpy as np
        import madmom.features.beats as mb
        act = mb.RNNBeatProcessor()(file_path)
        proc = mb.DBNBeatTrackingProcessor(fps=100)
        beats = proc(act)  # numpy array of beat timestamps in seconds
        if beats is None or len(beats) < 4:
            return None
        diffs = np.diff(beats)
        bpm_neural = round(float(60.0 / np.median(diffs)), 2)
        # Cap beats at 2000 entries (~26 min at 128 BPM)
        beats_list = [round(float(b), 3) for b in beats[:2000]]
        downbeats = [beats_list[i] for i in range(0, len(beats_list), 4)]
        phrase_boundaries = [beats_list[i] for i in range(0, len(beats_list), 16)]
        return {
            'beats': beats_list,
            'bpm_neural': bpm_neural,
            'downbeats': downbeats,
            'phrase_boundaries_neural': phrase_boundaries,
        }
    except Exception as exc:
        print(f"[boogiemix] beat grid detection failed: {exc}", file=sys.stderr)
        return None


def detect_key_neural(file_path, duration_sec):
    """Chroma-based key detection using Krumhansl-Schmuckler profiles."""
    try:
        import numpy as np
        load_dur = min(120.0, duration_sec) if duration_sec > 0 else 120.0
        y, sr = _load_audio_safe(file_path, 11025, load_dur)
        # Chroma via STFT (pure numpy) — 12 pitch classes from harmonic content
        _N = 2048
        _H = 512
        _y_pad = np.pad(y, _N // 2, mode='reflect')
        _nf = 1 + (len(_y_pad) - _N) // _H
        _frames = np.lib.stride_tricks.as_strided(
            _y_pad, shape=(_nf, _N),
            strides=(_y_pad.strides[0] * _H, _y_pad.strides[0]),
        )
        _win = np.hanning(_N).astype(np.float32)
        _D = np.abs(np.fft.rfft(np.ascontiguousarray(_frames) * _win, axis=1))  # (nf, N//2+1)
        freqs = np.fft.rfftfreq(_N, d=1.0 / sr)
        # Map FFT bins to chroma (pitch class) by closest equal-tempered bin
        A4 = 440.0
        with np.errstate(divide='ignore', invalid='ignore'):
            semitones = np.where(freqs > 0, 12 * np.log2(freqs / A4), -999)
        pitch_class = (np.round(semitones).astype(int) % 12 + 12) % 12
        chroma = np.zeros((12, _nf), dtype=np.float32)
        for pc in range(12):
            mask = pitch_class == pc
            chroma[pc] = _D[:, mask].sum(axis=1)
        chroma_mean = np.mean(chroma, axis=1)
        best_key, best_mode, best_corr = None, None, -2.0
        for i in range(12):
            for profile, mode in [(_MAJOR_PROFILE, 'major'), (_MINOR_PROFILE, 'minor')]:
                rotated = np.roll(np.array(profile, dtype=np.float32), i)
                c = np.corrcoef(chroma_mean.astype(np.float32), rotated)[0, 1]
                if c > best_corr:
                    best_corr, best_key, best_mode = c, _NOTE_NAMES[i], mode
        camelot = (_CAMELOT_MAJOR if best_mode == 'major' else _CAMELOT_MINOR).get(best_key)
        return {
            'key': best_key,
            'mode': best_mode,
            'confidence': round(float(best_corr), 3),
            'camelot': camelot,
        }
    except Exception as exc:
        print(f"[boogiemix] key detection failed: {exc}", file=sys.stderr)
        return None


def detect_vocal_cue_points(vocal_env, duration_sec, hz=ENVELOPE_HZ):
    """Find intro_end_sec / outro_start_sec from low-vocal regions in the envelope."""
    try:
        import numpy as np
        if vocal_env is None or len(vocal_env) == 0 or duration_sec <= 0:
            return None
        arr = np.asarray(vocal_env, dtype=np.float32)
        # 4-sample running mean to smooth noise
        kernel = np.ones(4, dtype=np.float32) / 4
        smoothed = np.convolve(arr, kernel, mode='same')
        threshold = 0.10
        min_gap_frames = max(1, int(round(4.0 * hz)))
        below = smoothed < threshold
        gap_runs = []
        start = None
        for i in range(len(below)):
            if below[i] and start is None:
                start = i
            elif not below[i] and start is not None:
                if i - start >= min_gap_frames:
                    gap_runs.append((start, i))
                start = None
        if start is not None and len(below) - start >= min_gap_frames:
            gap_runs.append((start, len(below)))
        if not gap_runs:
            return {'intro_end_sec': None, 'outro_start_sec': None, 'confidence': 0.0}
        first = gap_runs[0]
        last  = gap_runs[-1]
        intro_end   = round(first[1] / hz, 2)
        outro_start = round(last[0] / hz, 2)
        intro_conf  = min(1.0, (first[1] - first[0]) / hz / 16.0)
        outro_conf  = min(1.0, (last[1]  - last[0])  / hz / 16.0)
        confidence  = round((intro_conf + outro_conf) / 2.0, 3)
        # Sanity-check positions
        intro_end_out   = intro_end   if intro_end   < duration_sec * 0.5 else None
        outro_start_out = outro_start if outro_start > duration_sec * 0.25 else None
        return {
            'intro_end_sec':   intro_end_out,
            'outro_start_sec': outro_start_out,
            'confidence':      confidence,
        }
    except Exception as exc:
        print(f"[boogiemix] vocal cue detection failed: {exc}", file=sys.stderr)
        return None


def _build_analysis_from_envs(envs, duration_sec, bpm_hint, stem_samples=None):
    """Build analysis dict from pre-computed stem envelope arrays.

    stem_samples: optional dict of name -> (data, sr) for BPM detection from raw audio.
    When None (segment mode), BPM falls back to detect_beat_grid on the full file.
    """
    import numpy as np

    vocal = envs["vocals"]
    drums = envs["drums"]
    bass = envs["bass"]
    other = envs["other"]

    vocal_windows = trim_to_max(
        windows_from_threshold(vocal, ENVELOPE_HZ, VOCAL_PRESENT_THRESHOLD),
        MAX_WINDOWS,
    )
    drum_windows = trim_to_max(
        windows_from_threshold(drums, ENVELOPE_HZ, DRUM_PRESENT_THRESHOLD),
        MAX_WINDOWS,
    )
    bass_windows = trim_to_max(
        windows_from_threshold(bass, ENVELOPE_HZ, BASS_PRESENT_THRESHOLD),
        MAX_WINDOWS,
    )

    sections = detect_sections(vocal, drums, bass, other, ENVELOPE_HZ)
    transitions = transition_safe_windows(duration_sec, vocal, drums, bass,
                                          sections, ENVELOPE_HZ, other_env=other)
    cue_points = detect_vocal_cue_points(vocal, duration_sec)

    bpm_refined = None
    phrase_boundaries = []
    if stem_samples and "drums" in stem_samples:
        drum_samples, sr = stem_samples["drums"]
        phrase_boundaries, bpm_refined = detect_phrase_boundaries(
            drum_samples, sr, duration_sec, bpm_hint=bpm_hint
        )
        if not bpm_refined:
            try:
                import librosa, numpy as _np
                tempo, _ = librosa.beat.beat_track(y=drum_samples.astype(_np.float32), sr=int(sr))
                tempo = float(_np.atleast_1d(tempo)[0])
                print(f"[librosa_bpm] drum_sr={sr} tempo={tempo:.2f}", file=sys.stderr)
                if 60.0 <= tempo <= 200.0:
                    bpm_refined = tempo
            except Exception as e:
                print(f"[librosa_bpm] failed: {e}", file=sys.stderr)

    drum_mean = float(drums.mean()) if drums.size else 0.0
    bass_mean = float(bass.mean()) if bass.size else 0.0
    vocal_mean = float(vocal.mean()) if vocal.size else 0.0
    other_mean = float(other.mean()) if other.size else 0.0

    energy_refined = float(np.clip(
        drum_mean * 0.4 + bass_mean * 0.25 + other_mean * 0.2 + (1.0 - vocal_mean) * 0.15,
        0.0, 1.0,
    ))
    instrumental_ratio = float(np.clip(
        1.0 - vocal_mean / max(1e-6, drum_mean + bass_mean + other_mean + 1e-3), 0.0, 1.0
    ))

    stem_feature_json = {
        "schemaVersion": ANALYSIS_SCHEMA_VERSION,
        "envelopeHz": ENVELOPE_HZ,
        "vocals": round_list(downsample_for_output(vocal), 3),
        "drums": round_list(downsample_for_output(drums), 3),
        "bass": round_list(downsample_for_output(bass), 3),
        "other": round_list(downsample_for_output(other), 3),
        "summary": {
            "vocalDensity": round(vocal_mean, 3),
            "drumDensity": round(drum_mean, 3),
            "bassDensity": round(bass_mean, 3),
            "otherDensity": round(other_mean, 3),
            "instrumentalRatio": round(instrumental_ratio, 3),
            "hasLongIntro": any(s["kind"] == "intro" and (s["end"] - s["start"]) >= 12.0 for s in sections),
            "hasLongOutro": any(s["kind"] == "outro" and (s["end"] - s["start"]) >= 12.0 for s in sections),
        },
    }

    intro_outro = build_intro_outro_refined(transitions, duration_sec)
    confidence = float(np.clip(
        0.55 + 0.25 * min(1.0, drum_mean * 2.0) + 0.20 * min(1.0, (vocal_mean + drum_mean)),
        0.0, 1.0,
    ))
    transition_hints = {
        "preferLongBlend": instrumental_ratio > 0.35,
        "avoidVocalTransitions": vocal_mean > 0.2,
        "rhythmWeak": drum_mean < 0.18,
        "bassHeavy": bass_mean > 0.45,
        "safeTransitionTypes": _pick_safe_types(vocal_mean, drum_mean, bass_mean),
        "bpmRefined": round(bpm_refined, 2) if bpm_refined else None,
    }

    return {
        "stem_feature_json": stem_feature_json,
        "vocal_windows_json": vocal_windows,
        "drum_windows_json": drum_windows,
        "bass_windows_json": bass_windows,
        "section_json": sections,
        "phrase_boundaries_json": phrase_boundaries[:MAX_PHRASES],
        "intro_outro_refined_json": intro_outro,
        "transition_hints_json": transition_hints,
        "transition_windows_json": transitions,
        "energy_score_refined": round(energy_refined, 4),
        "confidence": round(confidence, 3),
        "bpm_refined": bpm_refined,
        "cue_points": cue_points,
    }


def analyze_with_stem_arrays(stem_envs, duration_sec, bpm_hint):
    """Segment mode: accepts pre-stitched envelope arrays (no raw audio samples)."""
    import numpy as np
    envs = {}
    for name in ("vocals", "drums", "bass", "other"):
        arr = stem_envs.get(name)
        if arr is not None and len(arr) > 0:
            envs[name] = arr.astype(np.float32)
        else:
            envs[name] = np.zeros(max(1, int(round(duration_sec * ENVELOPE_HZ))), dtype=np.float32)
    return _build_analysis_from_envs(envs, duration_sec, bpm_hint, stem_samples=None)


def analyze_with_stems(stems, duration_sec, bpm_hint):
    import numpy as np

    stem_arrays = {}
    sr_ref = None
    for name, path in stems.items():
        try:
            data, sr = load_audio_mono(path)
            stem_arrays[name] = (data, sr)
            sr_ref = sr
        except Exception:
            continue

    if not stem_arrays or sr_ref is None:
        return None

    if duration_sec <= 0.0:
        longest = max(len(d) / s for d, s in stem_arrays.values())
        duration_sec = float(longest)

    envs = {}
    for name in ("vocals", "drums", "bass", "other"):
        if name in stem_arrays:
            data, sr = stem_arrays[name]
            envs[name] = normalize_envelope(rms_envelope(data, sr, ENVELOPE_HZ))
        else:
            envs[name] = np.zeros(max(1, int(round(duration_sec * ENVELOPE_HZ))), dtype=np.float32)

    return _build_analysis_from_envs(envs, duration_sec, bpm_hint, stem_samples=stem_arrays)


def _load_audio_safe(file_path, target_sr, max_duration_s):
    """Load audio without librosa.load or soxr — both hang on some Windows machines.

    Handles NAS/UNC paths by copying to local temp first.
    Returns (y_float32_mono, sr) or raises.
    """
    import numpy as np
    import soundfile as _sf
    import scipy.signal as _sig

    load_path = file_path
    tmp_copy = None
    if file_path.startswith('\\\\') or file_path.startswith('//'):
        import shutil, tempfile, concurrent.futures
        ext = Path(file_path).suffix or '.audio'
        _fd, tmp_copy = tempfile.mkstemp(suffix=ext, prefix='bbmix_')
        os.close(_fd)
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as _ex:
            _fut = _ex.submit(shutil.copy2, file_path, tmp_copy)
            _fut.result(timeout=300)
        load_path = tmp_copy

    try:
        info = _sf.info(load_path)
        native_sr = info.samplerate
        max_frames = int(native_sr * max_duration_s)
        data, _ = _sf.read(load_path, frames=max_frames, dtype='float32', always_2d=True)
        # mono mix
        y = data.mean(axis=1) if data.shape[1] > 1 else data[:, 0]
        # resample if needed
        if native_sr != target_sr:
            y = _sig.resample_poly(y, target_sr, native_sr).astype(np.float32)
        return y, target_sr
    finally:
        if tmp_copy and os.path.exists(tmp_copy):
            os.unlink(tmp_copy)


def analyze_with_hpss(file_path, duration_sec, bpm_hint):
    """Fast CPU alternative to Demucs using librosa HPSS.

    Harmonic component → vocal proxy; percussive → drum proxy;
    low-frequency harmonic bins → bass proxy.  Runs in seconds on CPU.
    """
    import numpy as np
    print("[hpss] importing librosa...", file=sys.stderr, flush=True)
    import librosa
    print("[hpss] librosa imported", file=sys.stderr, flush=True)

    # Load at a low sample rate — enough for envelope analysis.
    SR = 11025
    MAX_DURATION_S = 300  # cap: beyond 300 s the envelope adds no extra info
    # For UNC/network paths, copy to local temp first — reading 50-100 MB sequentially
    # over SMB from a VM can hang mid-transfer even when small reads succeed fine.
    # libsndfile's seeking behaviour on FLAC also interacts poorly with SMB.
    load_path = file_path
    _tmp_copy = None
    if file_path.startswith('\\\\') or file_path.startswith('//'):
        import shutil, tempfile, concurrent.futures
        ext = Path(file_path).suffix or '.audio'
        _tmp_fd, _tmp_copy = tempfile.mkstemp(suffix=ext, prefix='bbmix_')
        os.close(_tmp_fd)
        print(f"[hpss] copying from NAS to local temp: {_tmp_copy!r}", file=sys.stderr, flush=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as _ex:
            _fut = _ex.submit(shutil.copy2, file_path, _tmp_copy)
            try:
                _fut.result(timeout=300)  # 5 min max for copy
            except concurrent.futures.TimeoutError:
                raise RuntimeError(f"File copy timed out after 300s (slow NAS?): {file_path!r}")
        print(f"[hpss] copy done", file=sys.stderr, flush=True)
        load_path = _tmp_copy

    import time as _time
    _t0 = _time.time()
    print(f"[hpss] loading audio: {load_path!r}", file=sys.stderr, flush=True)
    try:
        # Use soundfile directly to bypass librosa's resampling (which uses soxr and can be slow).
        # We read at native SR and downsample manually with scipy — much faster on CPU.
        import soundfile as _sf
        import scipy.signal as _sig
        _info = _sf.info(load_path)
        _native_sr = _info.samplerate
        _max_native_frames = int(_native_sr * MAX_DURATION_S)
        _data, _native_sr = _sf.read(load_path, frames=_max_native_frames, dtype='float32', always_2d=True)
        print(f"[hpss] soundfile read done in {_time.time()-_t0:.1f}s native_sr={_native_sr} shape={_data.shape}", file=sys.stderr, flush=True)
        # Mix to mono
        if _data.ndim > 1 and _data.shape[1] > 1:
            _data = _data.mean(axis=1)
        else:
            _data = _data[:, 0] if _data.ndim > 1 else _data
        # Resample to target SR if needed
        if _native_sr != SR:
            _t1 = _time.time()
            print(f"[hpss] resampling {_native_sr}->{SR}...", file=sys.stderr, flush=True)
            _n_out = int(round(len(_data) * SR / _native_sr))
            _data = _sig.resample_poly(_data, SR, _native_sr).astype(np.float32)
            print(f"[hpss] resample done in {_time.time()-_t1:.1f}s", file=sys.stderr, flush=True)
        y, sr = _data, SR
    finally:
        if _tmp_copy and os.path.exists(_tmp_copy):
            os.unlink(_tmp_copy)
    print(f"[hpss] audio loaded: {len(y)/sr:.1f}s total={_time.time()-_t0:.1f}s", file=sys.stderr, flush=True)

    if duration_sec <= 0.0:
        duration_sec = float(len(y)) / sr

    # STFT → HPSS
    # Use manual numpy STFT — librosa.stft hangs on scipy.fft dispatch init on some machines.
    _N_FFT = 1024
    _HOP = 256
    _t2 = _time.time()
    print("[hpss] computing STFT...", file=sys.stderr, flush=True)
    _y_pad = np.pad(y, _N_FFT // 2, mode='reflect')
    _n_frames = 1 + (len(_y_pad) - _N_FFT) // _HOP
    _frames = np.lib.stride_tricks.as_strided(
        _y_pad,
        shape=(_n_frames, _N_FFT),
        strides=(_y_pad.strides[0] * _HOP, _y_pad.strides[0]),
    )
    _win = np.hanning(_N_FFT).astype(np.float32)
    D = np.fft.rfft(np.ascontiguousarray(_frames) * _win, axis=1).T  # (n_fft//2+1, n_frames)
    print(f"[hpss] STFT done in {_time.time()-_t2:.1f}s, running HPSS...", file=sys.stderr, flush=True)
    mag = np.abs(D).astype(np.float32)
    # Pure-numpy HPSS via rolling mean (avoids scipy.ndimage which hangs on this machine).
    # Rolling mean along time axis → harmonic smoothing
    _W = 31
    _mp_t = np.pad(mag, ((0, 0), (_W - 1, 0)), mode='edge')
    _cs_t = np.cumsum(np.pad(_mp_t, ((0, 0), (1, 0))), axis=1)
    harm_smooth = (_cs_t[:, _W:] - _cs_t[:, :-_W]) / _W
    # Rolling mean along freq axis → percussive smoothing
    _mp_f = np.pad(mag, ((_W - 1, 0), (0, 0)), mode='edge')
    _cs_f = np.cumsum(np.pad(_mp_f, ((1, 0), (0, 0))), axis=0)
    perc_smooth = (_cs_f[_W:] - _cs_f[:-_W]) / _W
    # Soft masks with margin=3.0
    _tiny = np.finfo(np.float32).tiny
    _hp = harm_smooth ** 2
    _pp = perc_smooth ** 2
    H_mag = mag * _hp / (_hp + _pp * 3.0 + _tiny)
    P_mag = mag * _pp / (_pp + _hp * 3.0 + _tiny)
    print(f"[hpss] HPSS done in {_time.time()-_t2:.1f}s total", file=sys.stderr, flush=True)

    # Bass: low-frequency bins of harmonic (< ~300 Hz)
    freqs = np.fft.rfftfreq(_N_FFT, d=1.0 / sr)
    bass_cutoff_bin = int(np.searchsorted(freqs, 300.0))
    B_mag = np.zeros_like(H_mag)
    B_mag[:bass_cutoff_bin] = H_mag[:bass_cutoff_bin]

    # Convert magnitude spectrograms → time-domain power envelopes
    def mag_to_env(m):
        power = (m ** 2).mean(axis=0)  # mean power per frame
        ref = np.percentile(power, 95)
        if ref <= 1e-8:
            return np.zeros(len(power), dtype=np.float32)
        return np.clip(power / ref, 0.0, 1.0).astype(np.float32)

    # HPSS frames are at hop_length resolution; resample to ENVELOPE_HZ (1 Hz)
    hpss_fps = sr / _HOP
    h_env_hpss = mag_to_env(H_mag)
    p_env_hpss = mag_to_env(P_mag)
    b_env_hpss = mag_to_env(B_mag)

    n_frames_out = max(1, int(round(duration_sec * ENVELOPE_HZ)))

    def resample_env(env, n_out):
        if len(env) == 0:
            return np.zeros(n_out, dtype=np.float32)
        indices = np.linspace(0, len(env) - 1, n_out)
        return np.interp(indices, np.arange(len(env)), env).astype(np.float32)

    vocal_env = resample_env(h_env_hpss, n_frames_out)
    drum_env  = resample_env(p_env_hpss, n_frames_out)
    bass_env  = resample_env(b_env_hpss, n_frames_out)
    other_env = np.zeros(n_frames_out, dtype=np.float32)

    envs = {"vocals": vocal_env, "drums": drum_env, "bass": bass_env, "other": other_env}

    # Skip stem_samples — avoids triggering numba JIT (librosa.beat.beat_track) which
    # hangs on first invocation when the numba cache dir is not writable.
    # BPM is already handled by detect_beat_grid() in main().
    return _build_analysis_from_envs(envs, duration_sec, bpm_hint, stem_samples=None)


def _pick_safe_types(vocal_mean, drum_mean, bass_mean):
    types = ["safe_crossfade"]
    if drum_mean >= 0.25:
        types.append("beat_blend")
    if drum_mean < 0.22 or vocal_mean > 0.35:
        types.append("chill_fade")
    if drum_mean >= 0.25 and bass_mean >= 0.25:
        types.append("long_build")
    if vocal_mean < 0.2:
        types.append("filter_mix")
    return types


def build_intro_outro_refined(transitions, duration_sec):
    intro = None
    outro = None
    for w in transitions:
        if w["role"] == "intro" and intro is None:
            intro = w
        if w["role"] == "outro":
            outro = w
    return {
        "likelyInstrumentalIntro": {
            "start": intro["start"] if intro else 0.0,
            "end": intro["end"] if intro else 0.0,
            "score": intro.get("score", 0.0) if intro else 0.0,
        },
        "likelyInstrumentalOutro": {
            "start": outro["start"] if outro else duration_sec,
            "end": outro["end"] if outro else duration_sec,
            "score": outro.get("score", 0.0) if outro else 0.0,
        },
    }


def synthetic_fallback(duration_sec, demucs_error, bpm_hint):
    if duration_sec <= 0:
        duration_sec = 180.0
    n = max(8, int(round(duration_sec * ENVELOPE_HZ)))
    try:
        import numpy as np
        idx = np.arange(n, dtype=np.float32)
        x = idx / max(1, n - 1)
        vocal = np.clip(0.32 + 0.30 * np.sin(x * math.pi), 0.0, 1.0)
        drums = np.clip(0.45 + 0.30 * np.sin(x * math.pi * 1.7), 0.0, 1.0)
        bass = np.clip(0.40 + 0.25 * np.sin(x * math.pi * 1.3 + 1.1), 0.0, 1.0)
        other = np.clip(0.40 + 0.20 * np.sin(x * math.pi * 0.9 + 0.4), 0.0, 1.0)
    except Exception:
        vocal = [0.4] * n
        drums = [0.5] * n
        bass = [0.4] * n
        other = [0.4] * n
        vocal = _to_array(vocal)
        drums = _to_array(drums)
        bass = _to_array(bass)
        other = _to_array(other)

    sections = [
        {"kind": "intro", "start": 0.0, "end": round(min(24.0, duration_sec * 0.18), 2),
         "confidence": 0.4, "vocalDensity": 0.2, "drumDensity": 0.3, "energy": 0.3},
        {"kind": "outro", "start": round(max(0.0, duration_sec - min(24.0, duration_sec * 0.18)), 2),
         "end": round(duration_sec, 2),
         "confidence": 0.4, "vocalDensity": 0.2, "drumDensity": 0.3, "energy": 0.3},
    ]
    transitions = [
        {"role": "intro", "start": 0.0, "end": sections[0]["end"], "score": 0.55,
         "vocalRisk": 0.25, "drumContinuity": 0.35, "bassRisk": 0.3, "energy": 0.3,
         "recommendedMinCrossfade": 4, "recommendedMaxCrossfade": int(sections[0]["end"])},
        {"role": "outro", "start": sections[1]["start"], "end": sections[1]["end"], "score": 0.55,
         "vocalRisk": 0.25, "drumContinuity": 0.35, "bassRisk": 0.3, "energy": 0.3,
         "recommendedMinCrossfade": 4,
         "recommendedMaxCrossfade": int(min(MAX_TRANSITION_SEC, sections[1]["end"] - sections[1]["start"]))},
    ]
    return {
        "stem_feature_json": {
            "schemaVersion": ANALYSIS_SCHEMA_VERSION,
            "envelopeHz": ENVELOPE_HZ,
            "vocals": round_list(downsample_for_output(_to_array(vocal)), 3),
            "drums": round_list(downsample_for_output(_to_array(drums)), 3),
            "bass": round_list(downsample_for_output(_to_array(bass)), 3),
            "other": round_list(downsample_for_output(_to_array(other)), 3),
            "summary": {
                "vocalDensity": 0.32,
                "drumDensity": 0.45,
                "bassDensity": 0.4,
                "otherDensity": 0.4,
                "instrumentalRatio": 0.4,
                "hasLongIntro": True,
                "hasLongOutro": True,
            },
            "fallback": True,
        },
        "vocal_windows_json": [],
        "drum_windows_json": [],
        "bass_windows_json": [],
        "section_json": sections,
        "phrase_boundaries_json": [],
        "intro_outro_refined_json": build_intro_outro_refined(transitions, duration_sec),
        "transition_hints_json": {
            "preferLongBlend": False,
            "avoidVocalTransitions": False,
            "rhythmWeak": True,
            "bassHeavy": False,
            "safeTransitionTypes": ["safe_crossfade", "chill_fade"],
            "bpmRefined": bpm_hint if bpm_hint else None,
            "demucsError": demucs_error,
        },
        "transition_windows_json": transitions,
        "energy_score_refined": 0.4,
        "confidence": 0.2,
        "bpm_refined": bpm_hint,
        "cue_points": None,
    }


def _to_array(values):
    try:
        import numpy as np
        if hasattr(values, "shape"):
            return values.astype(np.float32)
        return np.asarray(values, dtype=np.float32)
    except Exception:
        return list(values)


def enforce_size(output):
    raw = json.dumps(output, separators=(",", ":"))
    if len(raw.encode("utf-8")) <= TARGET_JSON_BYTES:
        return output
    stem = output.get("stem_feature_json", {})
    for key in ("vocals", "drums", "bass", "other"):
        if isinstance(stem.get(key), list) and len(stem[key]) > 120:
            arr = stem[key]
            factor = max(2, int(math.ceil(len(arr) / 120)))
            stem[key] = [round(sum(arr[i:i + factor]) / max(1, len(arr[i:i + factor])), 3)
                         for i in range(0, len(arr), factor)]
    output["phrase_boundaries_json"] = output.get("phrase_boundaries_json", [])[:128]
    for k in ("vocal_windows_json", "drum_windows_json", "bass_windows_json",
              "transition_windows_json"):
        output[k] = output.get(k, [])[:64]
    output["section_json"] = output.get("section_json", [])[:32]
    return output


def main():
    print("[worker] startup", file=sys.stderr, flush=True)
    configure_model_cache()

    parser = argparse.ArgumentParser()
    parser.add_argument("--payload", required=False)
    args = parser.parse_args()

    if args.payload:
        payload = json.loads(args.payload)
    else:
        payload = json.loads(sys.stdin.read())
    file_path = payload.get("file_path", "")
    track_id = payload.get("track_id", 0)
    model = payload.get("demucs_model", DEFAULT_MODEL)
    use_gpu = bool(payload.get("use_gpu", False))
    cleanup_temp = bool(payload.get("cleanup_temp", True))
    temp_root = payload.get("temp_root") or tempfile.gettempdir()
    bpm_hint = payload.get("bpm_hint")
    use_madmom = bool(payload.get("use_madmom", True))
    try:
        bpm_hint = float(bpm_hint) if bpm_hint is not None else None
    except Exception:
        bpm_hint = None

    Path(temp_root).mkdir(parents=True, exist_ok=True)

    start = time.time()
    duration = float(payload.get("duration_sec") or 0.0)
    if duration <= 0:
        duration = ffprobe_duration(file_path)

    demucs_used = False
    demucs_error = None
    temp_dir = None

    analysis = None
    if model == "hpss":
        # Fast CPU path — librosa HPSS, no neural model needed.
        try:
            analysis = analyze_with_hpss(file_path, duration, bpm_hint)
            demucs_used = True  # signals Rust that real analysis ran (not synthetic fallback)
        except Exception as exc:
            demucs_error = f"hpss_failed: {exc}"[-2000:]
    else:
        stems = {}
        try:
            ok, out_dir, err = run_demucs(file_path, model, use_gpu, temp_root)
            if ok:
                stems = locate_stems(out_dir, model, file_path)
                if stems:
                    demucs_used = True
                    temp_dir = out_dir
                else:
                    demucs_error = "stems_not_found"
                    shutil.rmtree(out_dir, ignore_errors=True)
            else:
                demucs_error = (err or "").strip()[-2000:]
                if out_dir and os.path.isdir(out_dir):
                    shutil.rmtree(out_dir, ignore_errors=True)
        except Exception as exc:
            demucs_error = str(exc)[-2000:]

        if demucs_used:
            try:
                analysis = analyze_with_stems(stems, duration, bpm_hint)
            except Exception as exc:
                demucs_error = f"analysis_failed: {exc}"[-2000:]
                analysis = None

    if analysis is None:
        analysis = synthetic_fallback(duration, demucs_error, bpm_hint)

    # Run lightweight features that don't require Demucs stems.
    print("[worker] detect_key_neural...", file=sys.stderr, flush=True)
    key_neural = detect_key_neural(file_path, duration)
    print("[worker] detect_beat_grid...", file=sys.stderr, flush=True)
    beat_grid = detect_beat_grid(file_path, use_madmom=use_madmom)
    print("[worker] extract_neural_embedding...", file=sys.stderr, flush=True)
    neural_embedding = extract_neural_embedding(file_path, duration)
    print("[worker] features done", file=sys.stderr, flush=True)

    output = {
        "track_id": track_id,
        "analysis_version": ANALYSIS_VERSION,
        "analysis_schema_version": ANALYSIS_SCHEMA_VERSION,
        "demucs_model": model,
        "used_gpu": use_gpu and demucs_used,
        "used_demucs": demucs_used,
        "stem_feature_json": analysis["stem_feature_json"],
        "vocal_windows_json": analysis["vocal_windows_json"],
        "drum_windows_json": analysis["drum_windows_json"],
        "bass_windows_json": analysis["bass_windows_json"],
        "section_json": analysis["section_json"],
        "phrase_boundaries_json": analysis["phrase_boundaries_json"],
        "intro_outro_refined_json": analysis["intro_outro_refined_json"],
        "transition_hints_json": {
            **analysis["transition_hints_json"],
            "demucsError": demucs_error,
        },
        "transition_windows_json": analysis["transition_windows_json"],
        "energy_score_refined": analysis["energy_score_refined"],
        "confidence": analysis["confidence"],
        "bpm_refined": analysis.get("bpm_refined"),
        "key_neural": key_neural,
        "beat_grid": beat_grid,
        "neural_embedding": neural_embedding,
        "cue_points": analysis.get("cue_points"),
        "madmom_available": MADMOM_AVAILABLE,
        "processing_time_ms": int((time.time() - start) * 1000),
    }

    output = enforce_size(output)

    if cleanup_temp and temp_dir:
        shutil.rmtree(temp_dir, ignore_errors=True)

    sys.stdout.write(json.dumps(output, separators=(",", ":")))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()

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
        import librosa

        load_dur = min(120.0, duration_sec) if duration_sec > 0 else 120.0
        y, sr = librosa.load(file_path, sr=22050, mono=True, duration=load_dur)

        # 128-band mel-spectrogram
        mel = librosa.feature.melspectrogram(y=y, sr=sr, n_mels=128, hop_length=512)
        mel_db = librosa.power_to_db(mel, ref=np.max)

        # Segment statistics: mean + std per mel band → 256-dim vector
        mean_v = np.mean(mel_db, axis=1).astype(np.float32)
        std_v  = np.std(mel_db, axis=1).astype(np.float32)
        feature_v = np.concatenate([mean_v, std_v])  # 256-dim

        # energy_neural: mean log-power of mid-high mels (bands 40-128), normalized
        high_energy = float(np.mean(mel_db[40:, :]))
        energy_neural = round(float(np.clip((high_energy + 80.0) / 80.0, 0.0, 1.0)), 3)

        # danceability: onset strength regularity (coefficient of variation of inter-onset intervals)
        onset_env = librosa.onset.onset_strength(y=y, sr=sr)
        if onset_env.size > 4:
            peaks = librosa.util.peak_pick(onset_env, pre_max=3, post_max=3, pre_avg=3, post_avg=5, delta=0.5, wait=10)
            if len(peaks) > 2:
                ioi = np.diff(peaks.astype(np.float32))
                cv = float(np.std(ioi) / (np.mean(ioi) + 1e-6))
                danceability = round(float(np.clip(1.0 - cv * 0.5, 0.0, 1.0)), 3)
            else:
                danceability = 0.5
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
    """Chroma-based key detection using Krumhansl-Schmuckler profiles (librosa)."""
    try:
        import librosa
        import numpy as np
        load_dur = min(120.0, duration_sec) if duration_sec > 0 else 120.0
        y, sr = librosa.load(file_path, sr=22050, mono=True, duration=load_dur)
        chroma = librosa.feature.chroma_cqt(y=y, sr=sr)
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
    if "drums" in stem_arrays:
        drum_samples, sr = stem_arrays["drums"]
        phrase_boundaries, bpm_refined = detect_phrase_boundaries(
            drum_samples, sr, duration_sec, bpm_hint=bpm_hint
        )
    # If drum autocorrelation didn't yield a BPM, try librosa on the drum stem.
    if not bpm_refined and "drums" in stem_arrays:
        try:
            import librosa, numpy as _np
            drum_data, drum_sr = stem_arrays["drums"]
            tempo, _ = librosa.beat.beat_track(y=drum_data.astype(_np.float32), sr=int(drum_sr))
            tempo = float(_np.atleast_1d(tempo)[0])
            import sys
            print(f"[librosa_bpm] drum_sr={drum_sr} tempo={tempo:.2f}", file=sys.stderr)
            if 60.0 <= tempo <= 200.0:
                bpm_refined = tempo
        except Exception as e:
            import sys
            print(f"[librosa_bpm] failed: {e}", file=sys.stderr)

    drum_mean = float(drums.mean()) if drums.size else 0.0
    bass_mean = float(bass.mean()) if bass.size else 0.0
    vocal_mean = float(vocal.mean()) if vocal.size else 0.0
    other_mean = float(other.mean()) if other.size else 0.0

    energy_refined = float(np.clip(
        drum_mean * 0.4 + bass_mean * 0.25 + other_mean * 0.2 + (1.0 - vocal_mean) * 0.15,
        0.0, 1.0,
    ))
    instrumental_ratio = float(np.clip(1.0 - vocal_mean / max(1e-6, drum_mean + bass_mean + other_mean + 1e-3), 0.0, 1.0))

    vocal_out = downsample_for_output(vocal)
    drums_out = downsample_for_output(drums)
    bass_out = downsample_for_output(bass)
    other_out = downsample_for_output(other)

    stem_feature_json = {
        "schemaVersion": ANALYSIS_SCHEMA_VERSION,
        "envelopeHz": ENVELOPE_HZ,
        "vocals": round_list(vocal_out, 3),
        "drums": round_list(drums_out, 3),
        "bass": round_list(bass_out, 3),
        "other": round_list(other_out, 3),
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
    configure_model_cache()

    parser = argparse.ArgumentParser()
    parser.add_argument("--payload", required=False)
    args = parser.parse_args()

    if args.payload:
        payload = json.loads(args.payload)
    else:
        import sys
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
    stems = {}
    temp_dir = None
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

    analysis = None
    if demucs_used:
        try:
            analysis = analyze_with_stems(stems, duration, bpm_hint)
        except Exception as exc:
            demucs_error = f"analysis_failed: {exc}"[-2000:]
            analysis = None

    if analysis is None:
        analysis = synthetic_fallback(duration, demucs_error, bpm_hint)

    # Run lightweight features that don't require Demucs stems.
    key_neural = detect_key_neural(file_path, duration)
    beat_grid = detect_beat_grid(file_path, use_madmom=use_madmom)
    neural_embedding = extract_neural_embedding(file_path, duration)

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

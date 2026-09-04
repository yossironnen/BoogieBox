"""Neural beat and downbeat tracking, isolated in its own process.

This exists because madmom cannot safely share a process with librosa. Both
pull in an OpenMP/BLAS runtime, and two of those in one address space hard-crash
Python on Windows with 0xC0000005 — measured at roughly 1 run in 4 when the
HPSS (librosa) analysis path and madmom ran together in `detect_beat_grid`.
`KMP_DUPLICATE_LIB_OK=TRUE` silences the usual "OMP: Error #15" but does not
make the conflict safe. Giving madmom its own address space does, and it
mirrors what `demucs_runner.py` already does for Demucs.

Reads an audio path from argv, writes the beat grid to stdout as JSON, or
`null` when tracking is unavailable or fails. All diagnostics go to stderr so
stdout stays parseable.
"""
import json
import os
import sys

# Single-threaded BLAS: madmom's per-timestep RNN loops do not benefit from it
# (measured 37.0s / 39.2s / 38.0s at 1 / 4 / unclamped threads on a 9-minute
# track), and keeping one runtime is what makes this process safe.
os.environ.setdefault("KMP_DUPLICATE_LIB_OK", "TRUE")
os.environ.setdefault("OMP_NUM_THREADS", "1")
os.environ.setdefault("MKL_NUM_THREADS", "1")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")

MAX_BEATS = 2000


def _apply_compat_shims():
    """madmom 0.16.1 predates two Python/numpy removals it still relies on.

    `from collections import MutableSequence` (moved to collections.abc in
    Python 3.10) and the `np.float`/`np.int`/`np.bool` aliases (removed in
    numpy>=1.24, used across madmom's beat, tempo, and downbeat modules).
    """
    import collections
    import collections.abc
    if not hasattr(collections, "MutableSequence"):
        collections.MutableSequence = collections.abc.MutableSequence
    import numpy as np
    for name, value in (("float", float), ("int", int), ("bool", bool)):
        if not hasattr(np, name):
            setattr(np, name, value)


def _single_hmm_map(func, iterable):
    """Works around a third madmom-vs-modern-numpy incompatibility.

    `DBNDownBeatTrackingProcessor.process` picks its best HMM with
    `np.argmax(np.asarray(results)[:, 1])`, where each result is a
    `(viterbi_path, log_probability)` pair. That array is ragged — an N-element
    path beside a scalar — and numpy >= 1.24 refuses to build it, so downbeat
    tracking fails outright with "inhomogeneous shape after 2 dimensions".

    With a single bar length there is only ever one HMM to choose between, so
    padding each log probability out to the path's length makes the array
    rectangular without changing the outcome: every value in the row is equal
    and `argmax` returns the only candidate, index 0. Valid only while the
    processor is built with one `beats_per_bar` entry, which the caller asserts.
    """
    import numpy as np
    return [
        (path, np.full(len(path), log_prob, dtype=float))
        for path, log_prob in map(func, iterable)
    ]


def _pack(beats_list, downbeats, real):
    if len(beats_list) < 4:
        return None
    import numpy as np
    diffs = np.diff(np.asarray(beats_list))
    if len(diffs) == 0:
        return None
    return {
        "beats": beats_list,
        "bpm_neural": round(float(60.0 / np.median(diffs)), 2),
        "downbeats": downbeats,
        # Phrases are 4 bars; with real downbeats that is a real phrase edge.
        "phrase_boundaries_neural": downbeats[::4],
        "downbeats_real": real,
    }


def detect(file_path):
    """Real downbeats when the downbeat tracker works, beats-only otherwise.

    Downbeat tracking reports which beat of the bar each beat is, which is what
    a transition needs: the every-4th-beat approximation it replaces put bar one
    at an arbitrary offset, so two tracks could have their kicks lined up while
    the incoming clap landed on the outgoing track's beat one. The beats-only
    fallback is flagged `downbeats_real: false` so the renderer knows not to
    claim the bars line up.

    Deliberately sequential: madmom's `ParallelProcessor` splits the ensemble
    with `multiprocessing.Pool`, which was measured 2.7x faster at 4 workers but
    intermittently deadlocked on Windows — one track sat for 10+ minutes having
    burned only 23s of CPU. Wrong answers late beat right answers never.
    """
    try:
        import madmom.features.downbeats as md
        proc = md.DBNDownBeatTrackingProcessor(beats_per_bar=[4], fps=100)
        if len(proc.hmms) != 1:
            raise RuntimeError(f"_single_hmm_map assumes one HMM, got {len(proc.hmms)}")
        proc.map = _single_hmm_map
        act = md.RNNDownBeatProcessor()(file_path)
        tracked = proc(act)  # rows of [timestamp_sec, beat_number]
        if tracked is not None and len(tracked) >= 4:
            rows = tracked[:MAX_BEATS]  # ~26 min at 128 BPM
            beats_list = [round(float(r[0]), 3) for r in rows]
            downbeats = [round(float(r[0]), 3) for r in rows if int(r[1]) == 1]
            if len(downbeats) >= 2:
                packed = _pack(beats_list, downbeats, True)
                if packed is not None:
                    return packed
    except Exception as exc:
        print(f"[beatgrid] downbeat tracking failed: {exc}", file=sys.stderr)

    try:
        import madmom.features.beats as mb
        beats = mb.DBNBeatTrackingProcessor(fps=100)(mb.RNNBeatProcessor()(file_path))
        if beats is None or len(beats) < 4:
            return None
        beats_list = [round(float(b), 3) for b in beats[:MAX_BEATS]]
        # No bar phase available: every 4th beat is a placeholder only, and
        # `downbeats_real` stays false so nothing aligns bars against it.
        return _pack(beats_list, beats_list[::4], False)
    except Exception as exc:
        print(f"[beatgrid] beat grid detection failed: {exc}", file=sys.stderr)
        return None


def main():
    if len(sys.argv) < 2:
        print("usage: beatgrid_runner.py <audio-file>", file=sys.stderr)
        return 2
    try:
        _apply_compat_shims()
    except Exception as exc:
        print(f"[beatgrid] compat shims failed: {exc}", file=sys.stderr)
        json.dump(None, sys.stdout)
        return 0
    json.dump(detect(sys.argv[1]), sys.stdout)
    sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())

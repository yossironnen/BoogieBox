"""
Thin wrapper around 'python -m demucs' that monkey-patches torchaudio.save
to use soundfile instead of TorchCodec (which requires FFmpeg shared DLLs).
Accepts the same CLI arguments as 'python -m demucs'.
"""
import sys

# Patch torchaudio.save before Demucs imports it
try:
    import torchaudio as _ta
    import soundfile as _sf
    import torch as _torch

    def _save_via_soundfile(uri, src, sample_rate, channels_first=True, **kwargs):
        import numpy as np
        data = src.numpy() if isinstance(src, _torch.Tensor) else src
        if data.ndim == 2:
            data = data.T if channels_first else data
        elif data.ndim == 1:
            pass
        _sf.write(str(uri), data, sample_rate, format="WAV", subtype="PCM_16")

    _ta.save = _save_via_soundfile
except Exception:
    pass  # If patching fails, let Demucs fail naturally

from demucs.__main__ import main  # noqa: E402
main()

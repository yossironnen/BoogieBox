#!/usr/bin/env bash
# Bootstrap the BoogieMix Python deep-analysis environment on Linux/macOS.
# Linux equivalent of bootstrap_env.ps1.
#
# Usage:
#   bootstrap_env.sh [--python <exe>] [--venv <path>] [--cpu-only|--cuda|--auto]
#                    [--prime-model] [--force] [--log <file>]
#
# Options:
#   --python <exe>   Python executable to use (default: auto-detect 3.10+)
#   --venv <path>    Venv dir relative to this script (default: .venv)
#   --cpu-only       Force CPU PyTorch
#   --cuda           Force CUDA PyTorch
#   --auto           Auto-detect GPU (default)
#   --prime-model    Prime htdemucs model cache after install
#   --force          Delete and recreate an existing venv
#   --log <file>     Append all output to a log file in addition to stdout

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# ── Defaults ──────────────────────────────────────────────────────────────────
PYTHON_EXE=""
VENV_PATH=".venv"
CPU_ONLY=0
CUDA=0
AUTO=0
PRIME_MODEL=0
FORCE=0
LOG_FILE=""
DEFAULT_DEMUCS_MODEL="htdemucs"
CUDA_INDEX_URL="https://download.pytorch.org/whl/cu128"
CPU_INDEX_URL="https://download.pytorch.org/whl/cpu"

# ── Arg parsing ───────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --python)     PYTHON_EXE="$2";  shift 2 ;;
    --venv)       VENV_PATH="$2";   shift 2 ;;
    --cpu-only)   CPU_ONLY=1;       shift   ;;
    --cuda)       CUDA=1;           shift   ;;
    --auto)       AUTO=1;           shift   ;;
    --prime-model) PRIME_MODEL=1;   shift   ;;
    --force)      FORCE=1;          shift   ;;
    --log)        LOG_FILE="$2";    shift 2 ;;
    *) echo "[ERROR] Unknown argument: $1" >&2; exit 1 ;;
  esac
done

if [[ "$CPU_ONLY" -eq 1 && "$CUDA" -eq 1 ]]; then
  echo "[ERROR] Use only one of --cpu-only or --cuda." >&2
  exit 1
fi

# Default to auto-detect when no mode flag given
if [[ "$CPU_ONLY" -eq 0 && "$CUDA" -eq 0 ]]; then
  AUTO=1
fi

# ── Logging ───────────────────────────────────────────────────────────────────
if [[ -n "$LOG_FILE" ]]; then
  mkdir -p "$(dirname "$LOG_FILE")"
  # Tee all stdout+stderr to the log file for the rest of the script
  exec > >(tee -a "$LOG_FILE") 2>&1
fi

trap 'echo "[ERROR] bootstrap_env.sh failed at line $LINENO${LOG_FILE:+ — see $LOG_FILE}" >&2' ERR

echo "[bootstrap_env.sh] BoogieMix setup started."

# ── Python detection ──────────────────────────────────────────────────────────
find_python() {
  # Prefer versions with stable PyTorch/Demucs wheel availability.
  # 3.12 is the primary target; 3.11/3.10/3.13 are acceptable.
  # 3.14+ and generic python3 are last resorts — 3.14 may lack apt venv packages.
  # Also require ensurepip so venv creation doesn't fail on Debian/Ubuntu systems
  # where python3.X-venv is not installed.
  local candidates=("python3.12" "python3.11" "python3.10" "python3.13" "python3.14" "python3")
  for exe in "${candidates[@]}"; do
    if command -v "$exe" &>/dev/null; then
      if "$exe" -c "import sys; assert sys.version_info >= (3,10)" 2>/dev/null; then
        if "$exe" -c "import ensurepip" 2>/dev/null; then
          echo "$exe"
          return 0
        fi
      fi
    fi
  done
  return 1
}

if [[ -z "$PYTHON_EXE" ]]; then
  PYTHON_EXE="$(find_python || true)"
  if [[ -z "$PYTHON_EXE" ]]; then
    echo "[ERROR] No Python 3.10+ with working venv/ensurepip found." >&2
    echo "[ERROR] On Debian/Ubuntu, install a supported version and its venv package, e.g.:" >&2
    echo "[ERROR]   sudo apt-get install -y python3.12 python3.12-venv" >&2
    echo "[ERROR] Then retry this script." >&2
    exit 1
  fi
fi

echo "[bootstrap_env.sh] Using Python: $PYTHON_EXE ($("$PYTHON_EXE" --version 2>&1))"

# ── Venv ──────────────────────────────────────────────────────────────────────
VENV_FULL="${SCRIPT_DIR}/${VENV_PATH}"

if [[ -d "$VENV_FULL" && "$FORCE" -eq 1 ]]; then
  echo "[bootstrap_env.sh] --force: removing existing venv at $VENV_FULL"
  rm -rf "$VENV_FULL"
fi

if [[ ! -d "$VENV_FULL" ]]; then
  echo "[bootstrap_env.sh] Creating venv at $VENV_FULL..."
  if ! "$PYTHON_EXE" -m venv "$VENV_FULL" 2>/tmp/bb_venv_err; then
    _PY_MINOR_TMP="$("$PYTHON_EXE" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"
    VENV_PKG="python${_PY_MINOR_TMP}-venv"
    echo "[ERROR] venv creation failed. The '${VENV_PKG}' package is likely missing." >&2
    echo "[ERROR] Fix as root then retry:" >&2
    echo "[ERROR]   sudo apt-get install -y ${VENV_PKG}" >&2
    cat /tmp/bb_venv_err >&2
    exit 1
  fi
fi

VENV_PY="${VENV_FULL}/bin/python"
VENV_PIP="${VENV_FULL}/bin/pip"
PY_MINOR="$("$PYTHON_EXE" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"

# ── Base packages ─────────────────────────────────────────────────────────────
echo "[bootstrap_env.sh] Upgrading pip/setuptools/wheel..."
"$VENV_PY" -m pip install --upgrade pip setuptools wheel

echo "[bootstrap_env.sh] Installing Cython + numpy..."
# Cython 3.x generates Python 3.13/3.14-compatible C code; the old <3 cap breaks on newer Python.
"$VENV_PIP" install --upgrade "Cython>=3" "numpy>=1.24"

# ── Torch model cache ─────────────────────────────────────────────────────────
TORCH_HOME_DIR="${SCRIPT_DIR}/model-cache/torch"
mkdir -p "$TORCH_HOME_DIR"
export TORCH_HOME="$TORCH_HOME_DIR"
echo "[bootstrap_env.sh] TORCH_HOME=$TORCH_HOME"

# ── GPU detection ─────────────────────────────────────────────────────────────
has_nvidia_gpu() {
  nvidia-smi &>/dev/null && return 0 || return 1
}

test_torch_cuda() {
  "$VENV_PY" -c "import torch; raise SystemExit(0 if torch.cuda.is_available() else 1)" 2>/dev/null
}

reset_torch_packages() {
  "$VENV_PIP" uninstall -y torch torchvision torchaudio 2>/dev/null || true
}

install_torch_cpu() {
  echo "[bootstrap_env.sh] Installing CPU PyTorch..."
  reset_torch_packages
  "$VENV_PIP" install --upgrade --force-reinstall torch torchvision torchaudio \
    --index-url "$CPU_INDEX_URL"
}

install_torch_cuda() {
  echo "[bootstrap_env.sh] Installing CUDA PyTorch from $CUDA_INDEX_URL..."
  reset_torch_packages
  "$VENV_PIP" install --upgrade --force-reinstall torch torchvision torchaudio \
    --index-url "$CUDA_INDEX_URL"
  if ! test_torch_cuda; then
    echo "[ERROR] CUDA PyTorch installed but torch.cuda.is_available() is false." >&2
    return 1
  fi
}

# ── PyTorch install ───────────────────────────────────────────────────────────
TORCH_MODE=""

if [[ "$CPU_ONLY" -eq 1 ]]; then
  install_torch_cpu
  TORCH_MODE="cpu"
elif [[ "$CUDA" -eq 1 ]]; then
  install_torch_cuda
  TORCH_MODE="cuda"
elif [[ "$AUTO" -eq 1 ]]; then
  if has_nvidia_gpu; then
    echo "[bootstrap_env.sh] NVIDIA GPU detected — trying CUDA PyTorch..."
    if install_torch_cuda; then
      TORCH_MODE="cuda"
    else
      echo "[bootstrap_env.sh] CUDA install failed — falling back to CPU PyTorch."
      install_torch_cpu
      TORCH_MODE="cpu"
    fi
  else
    echo "[bootstrap_env.sh] No NVIDIA GPU detected — installing CPU PyTorch."
    install_torch_cpu
    TORCH_MODE="cpu"
  fi
fi

# ── requirements.txt ──────────────────────────────────────────────────────────
echo "[bootstrap_env.sh] Installing requirements.txt..."
"$VENV_PIP" install --no-build-isolation -r "${SCRIPT_DIR}/requirements.txt"

# ── madmom ────────────────────────────────────────────────────────────────────
# Requires gcc and python3.X-dev (pre-installed by install.sh on Debian/Ubuntu).
# --no-build-isolation lets madmom's build find the Cython already installed in the venv.
#
# The PyPI sdist ships pre-generated C files built against old Cython / old Python C API.
# On Python 3.13+ those C files fail to compile (_PyDict_SetItem_KnownHash removed,
# _PyLong_AsByteArray signature changed). We detect this and fall back to the GitHub
# source tree which has .pyx files; Cython 3.x then regenerates compatible C code.
echo "[bootstrap_env.sh] Installing madmom (neural beat tracking)..."
_PY_MINOR_INT="$("$PYTHON_EXE" -c 'import sys; print(sys.version_info.minor)')"
_MADMOM_OK=0
if [[ "$_PY_MINOR_INT" -lt 13 ]]; then
  # Python ≤3.12: PyPI sdist C files are compatible.
  if "$VENV_PIP" install --no-build-isolation "madmom>=0.16.1" 2>&1; then
    _MADMOM_OK=1
  fi
fi
if [[ "$_MADMOM_OK" -eq 0 ]]; then
  # Python 3.13+ (or PyPI build failed): install from GitHub source so Cython 3.x
  # regenerates the C files with the modern Python C API.
  echo "[bootstrap_env.sh] Python 3.${_PY_MINOR_INT} detected — installing madmom from source (GitHub)..."
  if "$VENV_PIP" install --no-build-isolation \
      "git+https://github.com/CPJKU/madmom.git" 2>&1; then
    _MADMOM_OK=1
  fi
fi
if [[ "$_MADMOM_OK" -eq 0 ]]; then
  echo "[ERROR] madmom install failed." >&2
  echo "[ERROR] Ensure system packages are installed and retry:" >&2
  echo "[ERROR]   sudo apt-get install -y build-essential python${PY_MINOR}-dev git" >&2
  exit 1
fi
echo "[bootstrap_env.sh] madmom installed."

# ── Verify core imports ───────────────────────────────────────────────────────
echo "[bootstrap_env.sh] Verifying torch + demucs imports..."
"$VENV_PY" -c "import torch, demucs"
echo "[bootstrap_env.sh] Verification passed."

# ── Prime Demucs model (optional) ────────────────────────────────────────────
if [[ "$PRIME_MODEL" -eq 1 ]]; then
  echo "[bootstrap_env.sh] Priming Demucs model cache for ${DEFAULT_DEMUCS_MODEL}..."
  if TORCH_HOME="$TORCH_HOME_DIR" "$VENV_PY" -c \
      "from demucs.pretrained import get_model; get_model('${DEFAULT_DEMUCS_MODEL}'); print('Demucs model ready: ${DEFAULT_DEMUCS_MODEL}')"; then
    echo "[bootstrap_env.sh] Model primed successfully."
  else
    echo "[bootstrap_env.sh] WARNING: Model priming failed — model will download on first analysis run."
  fi
fi

echo ""
echo "[bootstrap_env.sh] BoogieMix deep-analysis worker environment ready."
echo "  Venv:             $VENV_FULL"
echo "  PyTorch mode:     $TORCH_MODE"
echo "  Torch model cache: $TORCH_HOME_DIR"
exit 0

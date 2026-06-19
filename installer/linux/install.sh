#!/usr/bin/env bash
# Installs BoogieBox as a systemd service on Linux.
# Must be run as root (sudo ./install.sh) from the release folder.
#
# Usage:
#   sudo ./install.sh [--with-boogiemix]
#
# Options:
#   --with-boogiemix   Also install the BoogieMix deep-analysis Python environment
#                      (AI stem separation — requires internet, ~2-5 GB, 10-30 min).
#                      When omitted and running interactively, the script will prompt.
set -euo pipefail

INSTALL_DIR="/opt/boogiebox"
DATA_DIR="/var/lib/boogiebox"
SERVICE_NAME="boogiebox"
SERVICE_USER="boogiebox"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASE_DIR="$(dirname "$SCRIPT_DIR")"

WITH_BOOGIEMIX=0
for arg in "$@"; do
  case "$arg" in
    --with-boogiemix) WITH_BOOGIEMIX=1 ;;
  esac
done

if [ "$(id -u)" -ne 0 ]; then
  echo "[ERROR] This script must be run as root."
  echo "        Run: sudo $0"
  exit 1
fi

if ! command -v systemctl &>/dev/null; then
  echo "[ERROR] systemd is required. This installer does not support non-systemd Linux distributions."
  exit 1
fi

# ── BoogieMix prompt (interactive only) ──────────────────────────────────────
if [ "$WITH_BOOGIEMIX" -eq 0 ] && [ -t 0 ]; then
  echo ""
  echo " BoogieMix deep analysis uses AI stem separation (Demucs/PyTorch) to improve"
  echo " mix transitions. It requires Python 3.10+, an internet connection, ~2-5 GB of"
  echo " disk space, and 10-30 minutes to install."
  echo ""
  read -r -p " Install BoogieMix deep analysis? [y/N] " _bm_resp
  case "$_bm_resp" in
    [Yy]*) WITH_BOOGIEMIX=1 ;;
  esac
fi

echo ""
echo " Installing BoogieBox to ${INSTALL_DIR}..."
echo ""

# ── System user ───────────────────────────────────────────────────────────────
if ! id -u "$SERVICE_USER" &>/dev/null; then
  echo " Creating system user: ${SERVICE_USER}"
  useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# ── Stop existing service ─────────────────────────────────────────────────────
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
  echo " Stopping existing service..."
  systemctl stop "$SERVICE_NAME"
fi

# ── Copy files ────────────────────────────────────────────────────────────────
echo " Copying files to ${INSTALL_DIR}..."
mkdir -p "$INSTALL_DIR"
cp "${RELEASE_DIR}/boogiebox-server"   "${INSTALL_DIR}/boogiebox-server"
chmod +x "${INSTALL_DIR}/boogiebox-server"

if [ -d "${RELEASE_DIR}/client" ]; then
  rm -rf "${INSTALL_DIR}/client"
  cp -r "${RELEASE_DIR}/client" "${INSTALL_DIR}/client"
fi

if [ -d "${RELEASE_DIR}/resources" ]; then
  rm -rf "${INSTALL_DIR}/resources"
  cp -r "${RELEASE_DIR}/resources" "${INSTALL_DIR}/resources"
fi

# ── Data directory ────────────────────────────────────────────────────────────
echo " Creating data directory: ${DATA_DIR}..."
mkdir -p "$DATA_DIR"
chown "${SERVICE_USER}:${SERVICE_USER}" "$DATA_DIR"

# ── BoogieMix bootstrap ───────────────────────────────────────────────────────
BOOGIEMIX_STATUS="not installed"
if [ "$WITH_BOOGIEMIX" -eq 1 ]; then
  PYTHON_ASSETS="${INSTALL_DIR}/resources/Services/boogiemix/python"
  BSTRAP="${PYTHON_ASSETS}/bootstrap_env.sh"
  BM_LOG="${DATA_DIR}/installer-boogiemix.log"

  if [ ! -d "$PYTHON_ASSETS" ]; then
    echo " [WARN] BoogieMix python assets not found in release folder — skipping."
    BOOGIEMIX_STATUS="skipped (assets missing)"
  elif [ ! -f "$BSTRAP" ]; then
    echo " [WARN] bootstrap_env.sh not found at ${BSTRAP} — skipping."
    BOOGIEMIX_STATUS="skipped (bootstrap missing)"
  else
    chmod +x "$BSTRAP"
    chown -R "${SERVICE_USER}:${SERVICE_USER}" "${INSTALL_DIR}/resources/Services/boogiemix"
    mkdir -p "${DATA_DIR}/model-cache/torch"
    chown -R "${SERVICE_USER}:${SERVICE_USER}" "${DATA_DIR}/model-cache"

    # ── Pre-install system build deps as root (bootstrap runs as non-root service user) ──
    if command -v apt-get &>/dev/null; then
      # gcc + make are required to compile madmom's C extensions.
      # git is required to install madmom from source on Python 3.13+ (PyPI sdist is incompatible).
      echo " Pre-installing build-essential and git (required for madmom)..."
      apt-get install -y build-essential git 2>/dev/null || \
        echo " [WARN] Could not install build-essential/git — madmom may fail to build."

      # Find a Python 3.10+ where we can successfully install python3.X-venv.
      # Mirror the same candidate order as bootstrap_env.sh so both agree on the interpreter.
      _PY_EXE=""
      _PY_MINOR=""
      for _candidate in python3.12 python3.11 python3.10 python3.13 python3.14 python3; do
        if ! command -v "$_candidate" &>/dev/null; then continue; fi
        if ! "$_candidate" -c "import sys; assert sys.version_info >= (3,10)" 2>/dev/null; then continue; fi
        _minor="$("$_candidate" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")' 2>/dev/null || true)"
        [ -z "$_minor" ] && continue
        # python3.X-venv: needed for venv creation on Debian/Ubuntu.
        # python3.X-dev:  C headers needed by madmom's Cython extensions.
        echo " Pre-installing python${_minor}-venv and python${_minor}-dev..."
        apt-get install -y "python${_minor}-venv" "python${_minor}-dev" 2>/dev/null || true
        # Only accept this interpreter if ensurepip is now available (venv will work).
        if "$_candidate" -c "import ensurepip" 2>/dev/null; then
          _PY_EXE="$_candidate"
          _PY_MINOR="$_minor"
          echo " Selected Python: $_PY_EXE (${_PY_MINOR})"
          break
        fi
        echo " [WARN] python${_minor}-venv not available for $_candidate — trying next candidate..."
      done

      if [ -z "$_PY_EXE" ]; then
        echo " [WARN] No Python 3.10+ with working venv found — BoogieMix bootstrap may fail."
        echo "        Install a supported Python and its venv package, e.g.:"
        echo "          sudo apt-get install -y python3.12 python3.12-venv"
      fi
    fi

    echo ""
    echo " Running BoogieMix bootstrap (may take 10-30 minutes)..."
    echo " Log: ${BM_LOG}"
    echo ""
    if sudo -u "$SERVICE_USER" TORCH_HOME="${DATA_DIR}/model-cache/torch" \
        bash "$BSTRAP" --prime-model --auto --log "$BM_LOG"; then
      BOOGIEMIX_STATUS="installed"
    else
      echo ""
      echo " [WARN] BoogieMix bootstrap failed. See ${BM_LOG} for details."
      echo "        To retry after fixing Python/GPU dependencies:"
      echo "          sudo -u ${SERVICE_USER} TORCH_HOME=${DATA_DIR}/model-cache/torch \\"
      echo "            bash ${BSTRAP} --prime-model"
      BOOGIEMIX_STATUS="failed (see ${BM_LOG})"
    fi
  fi
fi

# ── systemd unit ──────────────────────────────────────────────────────────────
echo " Installing systemd unit..."
cp "${SCRIPT_DIR}/boogiebox.service" "/etc/systemd/system/${SERVICE_NAME}.service"
systemctl daemon-reload
systemctl enable "$SERVICE_NAME"
systemctl start "$SERVICE_NAME"

# ── Status ────────────────────────────────────────────────────────────────────
sleep 2
if systemctl is-active --quiet "$SERVICE_NAME"; then
  PORT=${PORT:-3001}
  echo ""
  echo " ================================================================="
  echo "  BoogieBox installed and running."
  echo "  Open http://localhost:${PORT} in your browser."
  echo "  On first launch, complete the setup wizard."
  echo ""
  echo "  BoogieMix deep analysis: ${BOOGIEMIX_STATUS}"
  echo ""
  echo "  Logs:    journalctl -u ${SERVICE_NAME} -f"
  echo "  Stop:    sudo systemctl stop ${SERVICE_NAME}"
  echo "  Start:   sudo systemctl start ${SERVICE_NAME}"
  echo "  Disable: sudo systemctl disable ${SERVICE_NAME}"
  echo " ================================================================="
  echo ""
else
  echo "[ERROR] Service failed to start. Check logs:"
  echo "        journalctl -u ${SERVICE_NAME} -n 50"
  exit 1
fi

#!/usr/bin/env bash
set -euo pipefail

APP_DIR="/opt/boogiebox"
DATA_DIR="${BOOGIEBOX_DATA_DIR:-/var/lib/boogiebox/data}"
CONFIG_DIR="${BOOGIEBOX_CONFIG_DIR:-/var/lib/boogiebox}"
LOG_DIR="${BOOGIEBOX_LOG_DIR:-/var/lib/boogiebox/logs}"
TORCH_CACHE="${TORCH_HOME:-/var/lib/boogiebox/model-cache/torch}"
BOOGIEMIX_RUNTIME_DIR="/var/lib/boogiebox/boogiemix"
IMAGE_TORCH_CACHE="${APP_DIR}/resources/Services/boogiemix/python/model-cache/torch"
BOOGIEMIX_PYTHON="${APP_DIR}/resources/Services/boogiemix/python/.venv/bin/python"

mkdir -p "$DATA_DIR" "$CONFIG_DIR" "$LOG_DIR" "$TORCH_CACHE" "$BOOGIEMIX_RUNTIME_DIR"

if [ "$(id -u)" = "0" ]; then
  chown -R boogiebox:boogiebox /var/lib/boogiebox
fi

if [ -d "$IMAGE_TORCH_CACHE" ] && [ -z "$(find "$TORCH_CACHE" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
  echo "[boogiebox] Seeding Torch model cache at ${TORCH_CACHE}"
  cp -a "${IMAGE_TORCH_CACHE}/." "$TORCH_CACHE/"
  if [ "$(id -u)" = "0" ]; then
    chown -R boogiebox:boogiebox "$TORCH_CACHE"
  fi
fi

if [ "${BOOGIEBOX_STARTUP_DIAGNOSTICS:-0}" = "1" ]; then
  echo "[boogiebox] Startup diagnostics"
  echo "  app:        ${APP_DIR}"
  echo "  config:     ${CONFIG_DIR}"
  echo "  data:       ${DATA_DIR}"
  echo "  logs:       ${LOG_DIR}"
  echo "  torch home: ${TORCH_CACHE}"
  "${APP_DIR}/resources/ffmpeg/ffmpeg" -version | head -n 1 || true
  "$BOOGIEMIX_PYTHON" --version || true
  "$BOOGIEMIX_PYTHON" -c "import torch; print('torch', torch.__version__, 'cuda', torch.cuda.is_available())" || true
  "$BOOGIEMIX_PYTHON" -c "import demucs; print('demucs ready')" || true
fi

if [ "$(id -u)" = "0" ]; then
  exec gosu boogiebox "$APP_DIR/boogiebox-server" "$@"
fi

exec "$APP_DIR/boogiebox-server" "$@"

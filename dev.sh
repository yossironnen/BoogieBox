#!/usr/bin/env bash
# Starts BoogieBox in development mode on Linux.
# Mirrors dev.bat: runs the latest Linux release build, optionally starts Vite.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
SERVER_PORT=3001

APP_VERSION=$(cat "$ROOT_DIR/VERSION" | tr -d '[:space:]')
if [ -z "$APP_VERSION" ]; then
  echo "[ERROR] Could not read version from VERSION file."
  exit 1
fi

SERVER_RELEASE_DIR="$ROOT_DIR/Releases/boogiebox-${APP_VERSION}-linux-rs"
SERVER_BIN="$SERVER_RELEASE_DIR/boogiebox-server"

# Fall back to the latest available linux-rs release if exact version not found
if [ ! -f "$SERVER_BIN" ]; then
  FALLBACK=$(ls -dt "$ROOT_DIR"/Releases/boogiebox-*-linux-rs 2>/dev/null | head -1 || true)
  if [ -n "$FALLBACK" ] && [ -f "$FALLBACK/boogiebox-server" ]; then
    echo ""
    echo "[WARN] Matching ${APP_VERSION} server binary not found."
    echo "       Using latest available release instead:"
    echo "       $FALLBACK/boogiebox-server"
    SERVER_RELEASE_DIR="$FALLBACK"
    SERVER_BIN="$FALLBACK/boogiebox-server"
  fi
fi

if [ ! -f "$SERVER_BIN" ]; then
  echo ""
  echo "[ERROR] Missing Linux server binary. Expected:"
  echo "        $SERVER_BIN"
  echo ""
  echo "        Run: bash build-server-rust.sh --no-test --no-installer"
  echo "        Then rerun dev.sh."
  echo ""
  exit 1
fi

echo ""
echo " Starting BoogieBox in development mode..."
echo " Server binary: $SERVER_BIN"
echo " Server:        http://localhost:${SERVER_PORT}"
echo ""

# Start the server in the background
PORT=$SERVER_PORT \
NODE_ENV=development \
BOOGIEBOX_LOAD_DEV_CONFIG=1 \
BOOGIEBOX_DEV_CONFIG_PATH="$ROOT_DIR/dev.config" \
BOOGIEBOX_DEBUG_LOG_PATH="$ROOT_DIR/logs/debug.log" \
  "$SERVER_BIN" &
SERVER_PID=$!

echo " Server PID: $SERVER_PID (kill with: kill $SERVER_PID)"
echo ""

# Give the server a moment to start
sleep 2

# Start Vite client if node/npx are available
if ! command -v node &>/dev/null; then
  echo " Node.js not found — Vite client will not start."
  echo " Open the packaged client at http://localhost:${SERVER_PORT}"
  echo ""
  wait "$SERVER_PID"
  exit 0
fi

if ! command -v npx &>/dev/null; then
  echo " npx not found — Vite client will not start."
  echo " Open the packaged client at http://localhost:${SERVER_PORT}"
  echo ""
  wait "$SERVER_PID"
  exit 0
fi

echo " Client:        http://localhost:3000  (Vite dev server)"
echo ""
echo " Press Ctrl+C to stop both server and client."
echo ""

# Ensure server is killed when this script exits
trap "kill $SERVER_PID 2>/dev/null || true" EXIT

cd "$ROOT_DIR/client"
if [ ! -d node_modules/@rollup/rollup-linux-x64-gnu ] && [ ! -d node_modules/@rollup/rollup-linux-arm64-gnu ]; then
  echo "  Native rollup binary missing — reinstalling client dependencies for Linux..."
  npm install
fi
npx vite --port 3000

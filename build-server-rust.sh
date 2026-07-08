#!/usr/bin/env bash
# Builds the BoogieBox standalone server for Linux (Rust).
set -euo pipefail

RUN_SMOKE=0
SKIP_INSTALLER=0
SKIP_TESTS=0
SKIP_MODEL_CACHE=0

for arg in "$@"; do
  case "$arg" in
    --smoke)           RUN_SMOKE=1 ;;
    --no-installer)    SKIP_INSTALLER=1 ;;
    --no-test)         SKIP_TESTS=1 ;;
    --no-model-cache)  SKIP_MODEL_CACHE=1 ;;
  esac
done

cd "$(dirname "$0")"

echo ""
echo " Building BoogieBox standalone server (Rust / Linux)..."
echo ""

# Always use a Linux-native Cargo target directory so Linux and Windows builds
# never share the same target/ tree (avoids lock conflicts and fingerprint stomping
# when both platforms build from the same repo simultaneously).
# Honour an explicit override if the caller already set CARGO_TARGET_DIR.
if [ -z "${CARGO_TARGET_DIR:-}" ]; then
  export CARGO_TARGET_DIR="$HOME/.cargo-targets/boogiebox-server"
fi
echo "[INFO] Cargo target dir: $CARGO_TARGET_DIR"

if ! command -v cargo &>/dev/null; then
  echo "[ERROR] cargo (Rust toolchain) is required."
  echo "        Install from https://rustup.rs/"
  exit 1
fi

if ! command -v node &>/dev/null; then
  echo "[ERROR] Node.js is required to build the React client."
  echo "        Install Node.js LTS from https://nodejs.org/"
  exit 1
fi

if ! command -v pkg-config &>/dev/null; then
  echo "[ERROR] pkg-config is required to locate OpenSSL."
  echo "        Install with: sudo apt install pkg-config libssl-dev"
  exit 1
fi

# ------------------------------------------------------------------
# [1/8] Quality checks
# ------------------------------------------------------------------
if [ "$SKIP_TESTS" -eq 1 ]; then
  echo " [1/8] Skipping quality checks (--no-test)."
else
  echo " [1/8] Running quality checks..."
  npm run lint
  npm run security:semgrep

  echo ""
  echo " Running cargo audit..."
  if command -v cargo-audit &>/dev/null; then
    pushd server-rs >/dev/null
    cargo audit
    popd >/dev/null
  else
    echo "[INFO] cargo-audit not installed. Install with: cargo install cargo-audit"
    echo "       Continuing build without audit check..."
  fi
fi

# ------------------------------------------------------------------
# [2/8] React client build
# ------------------------------------------------------------------
echo ""
echo " [2/8] Building React client..."
pushd client >/dev/null
# Reinstall if the Linux native rollup binary is missing (node_modules was
# installed on Windows and lacks linux-x64-gnu optional dependencies).
if [ ! -d node_modules/@rollup/rollup-linux-x64-gnu ] && [ ! -d node_modules/@rollup/rollup-linux-arm64-gnu ]; then
  echo "  Native rollup binary missing — reinstalling client dependencies for Linux..."
  npm install
fi
npx vite build
popd >/dev/null

# ------------------------------------------------------------------
# [3/8] Read version
# ------------------------------------------------------------------
echo ""
echo " [3/8] Reading version..."
APP_VERSION=$(cat VERSION | tr -d '[:space:]')
if [ -z "$APP_VERSION" ]; then
  echo "[ERROR] Could not read version from VERSION file"
  exit 1
fi
echo " Version: $APP_VERSION"

# ------------------------------------------------------------------
# [4/8] Rust server release build
# ------------------------------------------------------------------
echo ""
echo " [4/8] Building Rust server release binary..."
cargo build --release --manifest-path server-rs/Cargo.toml

RELEASE_NAME="boogiebox-${APP_VERSION}-linux-rs"
DIST_DIR="Releases/${RELEASE_NAME}"
FFMPEG_CACHE_DIR="tools/ffmpeg-linux"
FFMPEG_RELEASE_DIR="${DIST_DIR}/resources/ffmpeg"
TARGET_DIR="${CARGO_TARGET_DIR:-server-rs/target}"
RUST_BIN="${TARGET_DIR}/release/boogiebox-server"

if [ ! -f "$RUST_BIN" ]; then
  echo "[ERROR] Rust build did not produce $RUST_BIN"
  exit 1
fi

# ------------------------------------------------------------------
# FFmpeg cache: download static Linux build if not present
# ------------------------------------------------------------------
FFMPEG_EXE="${FFMPEG_CACHE_DIR}/ffmpeg"
FFPROBE_EXE="${FFMPEG_CACHE_DIR}/ffprobe"

if [ ! -f "$FFMPEG_EXE" ] || [ ! -f "$FFPROBE_EXE" ]; then
  echo ""
  echo " Downloading static FFmpeg for Linux (amd64)..."
  mkdir -p "$FFMPEG_CACHE_DIR"

  FFMPEG_URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
  ARCHIVE="${FFMPEG_CACHE_DIR}/ffmpeg-release-amd64-static.tar.xz"

  if command -v curl &>/dev/null; then
    curl -L --progress-bar -o "$ARCHIVE" "$FFMPEG_URL"
  elif command -v wget &>/dev/null; then
    wget -q --show-progress -O "$ARCHIVE" "$FFMPEG_URL"
  else
    echo "[ERROR] curl or wget is required to download FFmpeg."
    exit 1
  fi

  # Extract only ffmpeg and ffprobe from the archive (they live in the top dir)
  tar -xJf "$ARCHIVE" --strip-components=1 \
    --wildcards \
    --no-anchored \
    -C "$FFMPEG_CACHE_DIR" \
    "*/ffmpeg" "*/ffprobe"

  rm -f "$ARCHIVE"

  # Strip debug symbols to reduce binary size
  if command -v strip &>/dev/null; then
    strip "$FFMPEG_EXE" "$FFPROBE_EXE" 2>/dev/null || true
  fi

  chmod +x "$FFMPEG_EXE" "$FFPROBE_EXE"
  echo " FFmpeg cached at $FFMPEG_CACHE_DIR/"
fi

if [ ! -f "$FFMPEG_EXE" ]; then
  echo "[ERROR] Missing ${FFMPEG_CACHE_DIR}/ffmpeg"
  echo "        Place ffmpeg and ffprobe in tools/ffmpeg-linux/ before building."
  exit 1
fi
if [ ! -f "$FFPROBE_EXE" ]; then
  echo "[ERROR] Missing ${FFMPEG_CACHE_DIR}/ffprobe"
  exit 1
fi

# ------------------------------------------------------------------
# [5/8] Create release folder
# ------------------------------------------------------------------
echo ""
echo " [5/8] Creating release folder: ${DIST_DIR}/"
mkdir -p Releases
rm -rf "$DIST_DIR"
mkdir -p "${DIST_DIR}/client"
mkdir -p "${DIST_DIR}/resources"
mkdir -p "${DIST_DIR}/install"

# ------------------------------------------------------------------
# [6/8] Copy Rust server binary
# ------------------------------------------------------------------
echo ""
echo " [6/8] Copying Rust server binary..."
cp "$RUST_BIN" "${DIST_DIR}/boogiebox-server"
chmod +x "${DIST_DIR}/boogiebox-server"

# ------------------------------------------------------------------
# [7/8] Copy sidecars and assets
# ------------------------------------------------------------------
echo " [7/8] Copying sidecars and assets..."

cp -r client/build/ "${DIST_DIR}/client/build/"

mkdir -p "$FFMPEG_RELEASE_DIR"
cp "$FFMPEG_EXE" "$FFPROBE_EXE" "$FFMPEG_RELEASE_DIR/"
# Copy any license/notice files that accompany the FFmpeg cache
find "$FFMPEG_CACHE_DIR" -maxdepth 1 -type f ! -name 'ffmpeg' ! -name 'ffprobe' \
  -exec cp {} "$FFMPEG_RELEASE_DIR/" \;

if [ -d "Services/boogiemix/python" ]; then
  mkdir -p "${DIST_DIR}/resources/Services/boogiemix"
  cp -r Services/boogiemix/python "${DIST_DIR}/resources/Services/boogiemix/python"
  rm -rf "${DIST_DIR}/resources/Services/boogiemix/python/.venv"
  if [ "$SKIP_MODEL_CACHE" -eq 1 ] && [ -d "${DIST_DIR}/resources/Services/boogiemix/python/model-cache" ]; then
    echo "  Excluding Demucs model cache from release (--no-model-cache) - BoogieMix setup will download models on first use instead."
    rm -rf "${DIST_DIR}/resources/Services/boogiemix/python/model-cache"
  fi
  if [ -f "${DIST_DIR}/resources/Services/boogiemix/python/bootstrap_env.sh" ]; then
    chmod +x "${DIST_DIR}/resources/Services/boogiemix/python/bootstrap_env.sh"
  fi
fi

if [ -d "Services/boogiemix/ai" ]; then
  mkdir -p "${DIST_DIR}/resources/Services/boogiemix"
  cp -r Services/boogiemix/ai "${DIST_DIR}/resources/Services/boogiemix/ai"
fi

# Installer helpers
cp installer/linux/boogiebox.service "${DIST_DIR}/install/boogiebox.service"
cp installer/linux/install.sh        "${DIST_DIR}/install/install.sh"
chmod +x "${DIST_DIR}/install/install.sh"

# ------------------------------------------------------------------
# [8/8] Release metadata
# ------------------------------------------------------------------
echo ""
echo " [8/8] Writing release metadata files..."

cat > "${DIST_DIR}/start.sh" <<EOF
#!/usr/bin/env bash
cd "\$(dirname "\$0")"
exec ./boogiebox-server "\$@"
EOF
chmod +x "${DIST_DIR}/start.sh"

cat > "${DIST_DIR}/README.md" <<EOF
# BoogieBox ${APP_VERSION} — Linux

BoogieBox standalone server package for Linux.

## Requirements

- Linux x86_64
- No Node.js or npm required at runtime
- Bundled ffmpeg and ffprobe are included under resources/ffmpeg

## Quick start (manual)

1. Run \`./start.sh\` or \`./boogiebox-server\`
2. Open **http://localhost:3001** in your browser
3. On first launch, follow the setup wizard to choose the database folder and libraries

## Systemd service install

Run \`sudo ./install/install.sh\` to install as a systemd service.

The service runs as the \`boogiebox\` system user.
Database and config are stored under \`/var/lib/boogiebox/\`.
Logs are available via \`journalctl -u boogiebox\`.

## Configuration overrides

- \`BOOGIEBOX_CONFIG_PATH\` / \`BOOGIEBOX_CONFIG_DIR\` — custom config location
- \`BOOGIEBOX_FFMPEG_DIR\` — custom FFmpeg binary directory
- \`PORT\` — HTTP listen port (default 3001)

## Notes

The React client is served from client/build beside the binary.
FFmpeg is resolved from resources/ffmpeg before PATH.
EOF

cat > "${DIST_DIR}/THIRD_PARTY_NOTICES.md" <<'EOF'
# Third-Party Notices

## Rust Runtime Crates

This BoogieBox release is built with Rust and links these third-party crates:
  axum, tokio, rusqlite, serde, serde_json, reqwest, tower-http, tracing,
  uuid, image, pbkdf2, sha2, hex, base64, rand, socket2, thiserror, time,
  tokio-util, axum-extra, sha1, tower.

SQLite is bundled via rusqlite with the "bundled" feature. SQLite is public domain.
See server-rs/Cargo.lock and each crate's upstream repository LICENSE file for terms.

## FFmpeg

This BoogieBox release bundles a static FFmpeg and FFprobe build under resources/ffmpeg.
The included build is sourced from https://johnvansickle.com/ffmpeg/ (LGPL static build).
FFmpeg is licensed under the GNU Lesser General Public License (LGPL) version 2.1 or later.
See https://ffmpeg.org/legal.html for terms.
EOF

# ------------------------------------------------------------------
# Installer / tarball (optional)
# ------------------------------------------------------------------
if [ "$SKIP_INSTALLER" -eq 1 ]; then
  echo ""
  echo " Skipping tarball (--no-installer)."
else
  echo ""
  echo " Creating release tarball..."
  TARBALL_SUFFIX=""
  if [ "$SKIP_MODEL_CACHE" -eq 1 ]; then
    TARBALL_SUFFIX="-nomodels"
  fi
  TARBALL="Releases/boogiebox-${APP_VERSION}-linux${TARBALL_SUFFIX}-rs.tar.gz"
  tar -czf "$TARBALL" -C Releases "$RELEASE_NAME"
  echo " Tarball: $TARBALL"
fi

echo ""
echo " ================================================================="
echo "  Linux build complete: ${DIST_DIR}/"
echo "  Run ./start.sh or ./boogiebox-server"
echo " ================================================================="
echo ""

# ------------------------------------------------------------------
# Smoke test
# ------------------------------------------------------------------
if [ "$RUN_SMOKE" -eq 1 ]; then
  echo " Running smoke test..."
  SMOKE_PORT=3199
  SMOKE_CONFIG_DIR="${DIST_DIR}/.smoke-config"
  rm -rf "$SMOKE_CONFIG_DIR"
  mkdir -p "$SMOKE_CONFIG_DIR"

  SMOKE_LOG="${DIST_DIR}/smoke.log"
  PORT=$SMOKE_PORT BOOGIEBOX_CONFIG_DIR="$(realpath "$SMOKE_CONFIG_DIR")" \
    "${DIST_DIR}/boogiebox-server" > "$SMOKE_LOG" 2>&1 &
  SERVER_PID=$!

  SMOKE_PASSED=0
  for i in $(seq 1 45); do
    sleep 1
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      echo "[ERROR] boogiebox-server exited before health check."
      cat "$SMOKE_LOG"
      exit 1
    fi
    STATUS=$(curl -sf "http://127.0.0.1:${SMOKE_PORT}/api/system/status" 2>/dev/null || true)
    if echo "$STATUS" | grep -q '"server":"boogiebox"'; then
      SMOKE_PASSED=1
      break
    fi
  done

  kill "$SERVER_PID" 2>/dev/null || true

  if [ "$SMOKE_PASSED" -eq 1 ]; then
    echo " Smoke test passed."
  else
    echo "[ERROR] Smoke test failed — server did not respond in time."
    cat "$SMOKE_LOG"
    exit 1
  fi
fi

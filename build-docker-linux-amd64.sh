#!/usr/bin/env bash
# Builds the BoogieBox Docker image for Linux amd64 with CPU-only BoogieMix.
set -euo pipefail

cd "$(dirname "$0")"

IMAGE_TAG="boogiebox:linux-amd64-boogiemix-cpu"
PRIME_MODEL=1
NO_CACHE=()
OUTPUT_FLAG=(--load)
PROGRESS_FLAG=()
RUN_AFTER_BUILD=0
CONTAINER_NAME="boogiebox-local"
HOST_PORT="3001"
DATA_VOLUME="boogiebox-data"
MUSIC_MOUNT=""
STARTUP_DIAGNOSTICS=0

usage() {
  cat <<'EOF'
Usage: ./build-docker-linux-amd64.sh [options]

Options:
  --tag <tag>           Image tag. Default: boogiebox:linux-amd64-boogiemix-cpu
  --no-prime-model      Do not prime the htdemucs model during build
  --no-cache            Build without Docker layer cache
  --push                Push build result instead of loading it locally
  --no-load             Do not pass --load or --push
  --progress <mode>     Docker progress mode: auto, plain, or tty
  --run                 Start/recreate a container after a successful build
  --name <name>         Container name for --run. Default: boogiebox-local
  --host-port <port>    Host port for --run. Default: 3001
  --data-volume <name>  Docker volume for /var/lib/boogiebox. Default: boogiebox-data
  --music <path>        Optional host music path mounted read-only at /music
  --diagnostics         Enable startup diagnostics when using --run
  --help                Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --tag requires a value." >&2
        exit 1
      fi
      IMAGE_TAG="$2"
      shift 2
      ;;
    --no-prime-model)
      PRIME_MODEL=0
      shift
      ;;
    --no-cache)
      NO_CACHE=(--no-cache)
      shift
      ;;
    --push)
      OUTPUT_FLAG=(--push)
      shift
      ;;
    --no-load)
      OUTPUT_FLAG=()
      shift
      ;;
    --progress)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --progress requires a value such as plain, auto, or tty." >&2
        exit 1
      fi
      PROGRESS_FLAG=(--progress="$2")
      shift 2
      ;;
    --run)
      RUN_AFTER_BUILD=1
      shift
      ;;
    --name)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --name requires a value." >&2
        exit 1
      fi
      CONTAINER_NAME="$2"
      shift 2
      ;;
    --host-port)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --host-port requires a value." >&2
        exit 1
      fi
      HOST_PORT="$2"
      shift 2
      ;;
    --data-volume)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --data-volume requires a value." >&2
        exit 1
      fi
      DATA_VOLUME="$2"
      shift 2
      ;;
    --music)
      if [[ $# -lt 2 ]]; then
        echo "[ERROR] --music requires a host path." >&2
        exit 1
      fi
      MUSIC_MOUNT="$2"
      shift 2
      ;;
    --diagnostics)
      STARTUP_DIAGNOSTICS=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "[ERROR] Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if ! command -v docker >/dev/null 2>&1; then
  echo "[ERROR] Docker is required. Install Docker Desktop or Docker Engine first." >&2
  exit 1
fi
if [[ "$RUN_AFTER_BUILD" -eq 1 && "${OUTPUT_FLAG[*]:-}" == "--push" ]]; then
  echo "[ERROR] --run cannot be used with --push because the image may not be loaded locally." >&2
  exit 1
fi
if [[ "$RUN_AFTER_BUILD" -eq 1 && "${#OUTPUT_FLAG[@]}" -eq 0 ]]; then
  echo "[ERROR] --run cannot be used with --no-load because the image may not be loaded locally." >&2
  exit 1
fi

echo
echo "Building BoogieBox Docker image for linux/amd64..."
echo "  Tag:         ${IMAGE_TAG}"
echo "  Prime model: ${PRIME_MODEL}"
echo "  Output:      ${OUTPUT_FLAG[*]:-(none)}"
if [[ "$RUN_AFTER_BUILD" -eq 1 ]]; then
  echo "  Run:         yes"
  echo "  Container:   ${CONTAINER_NAME}"
  echo "  Port:        ${HOST_PORT}:3001"
  echo "  Data volume: ${DATA_VOLUME}:/var/lib/boogiebox"
  [[ -n "$MUSIC_MOUNT" ]] && echo "  Music mount: ${MUSIC_MOUNT}:/music:ro"
fi
echo

docker buildx build \
  --platform linux/amd64 \
  -f Dockerfile.linux-amd64 \
  -t "$IMAGE_TAG" \
  --build-arg "BOOGIEMIX_PRIME_MODEL=${PRIME_MODEL}" \
  "${NO_CACHE[@]}" \
  "${OUTPUT_FLAG[@]}" \
  "${PROGRESS_FLAG[@]}" \
  .

if [[ "$RUN_AFTER_BUILD" -eq 1 ]]; then
  echo
  echo "Starting container with host port mapping..."
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  run_args=(
    -d
    --name "$CONTAINER_NAME"
    -p "${HOST_PORT}:3001"
    -v "${DATA_VOLUME}:/var/lib/boogiebox"
    -e "BOOGIEBOX_STARTUP_DIAGNOSTICS=${STARTUP_DIAGNOSTICS}"
  )
  if [[ -n "$MUSIC_MOUNT" ]]; then
    run_args+=(-v "${MUSIC_MOUNT}:/music:ro")
  fi
  docker run "${run_args[@]}" "$IMAGE_TAG"
  echo
  echo "BoogieBox is starting at http://localhost:${HOST_PORT}"
fi

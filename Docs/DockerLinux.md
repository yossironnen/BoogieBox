# Docker Linux Deployment

BoogieBox has an additive Docker deployment path for Linux `amd64`. It does not replace the existing Linux release folder, `build-server-rust.sh`, or systemd installer.

The image includes:

- Rust Axum server
- Built React client
- Static FFmpeg and FFprobe under `/opt/boogiebox/resources/ffmpeg`
- BoogieMix Python assets and a CPU-only Python venv with PyTorch, Demucs, madmom, and related requirements
- Optional build-time `htdemucs` model priming

## Requirements

- Docker Desktop with WSL2 integration, or Docker Engine on Linux
- `buildx` support
- `linux/amd64` host or emulation
- Enough disk space for PyTorch, Demucs, and the optional model cache

## Build

From the repo root:

Windows:

```bat
.\build-docker-linux-amd64.bat
```

Linux or WSL:

```bash
bash ./build-docker-linux-amd64.sh
```

Both scripts default to:

```bash
docker buildx build --platform linux/amd64 -f Dockerfile.linux-amd64 -t boogiebox:linux-amd64-boogiemix-cpu --load .
```

The default build primes the Demucs `htdemucs` model. To make a smaller or faster local image build that downloads the model on first analysis instead:

```bash
bash ./build-docker-linux-amd64.sh --no-prime-model
```

or:

```bat
.\build-docker-linux-amd64.bat --no-prime-model
```

Equivalent raw Docker command:

```bash
docker buildx build --platform linux/amd64 --build-arg BOOGIEMIX_PRIME_MODEL=0 -f Dockerfile.linux-amd64 -t boogiebox:linux-amd64-boogiemix-cpu --load .
```

Useful script options:

```text
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
```

To build and start the app with the browser port mapped in one step:

```bat
.\build-docker-linux-amd64.bat --run
```

or:

```bash
bash ./build-docker-linux-amd64.sh --run
```

Then open `http://localhost:3001`.

## Run

```bash
docker run --rm \
  --platform linux/amd64 \
  -p 3001:3001 \
  -v boogiebox-data:/var/lib/boogiebox \
  -v /srv/music:/music:ro \
  boogiebox:linux-amd64-boogiemix-cpu
```

Open `http://localhost:3001` and complete first-run setup. Use `/music` or another bind-mounted path when selecting music libraries.

## Compose

Use `docker-compose.linux-amd64.yml` as the reference:

```bash
docker compose -f docker-compose.linux-amd64.yml up --build
```

Edit the music bind mount before running:

```yaml
- /srv/music:/music:ro
```

For NAS libraries, mount the NAS on the Docker host first, then bind the mounted host path into the container. Do not use Windows UNC paths directly inside the Linux container.

## Publish From GitHub Actions

The repo includes a manual workflow at `.github/workflows/docker-linux-amd64.yml`.

In GitHub:

1. Open **Actions**.
2. Select **Docker Linux amd64**.
3. Click **Run workflow**.
4. Leave `tag` blank to publish `VERSION-linux-amd64-boogiemix-cpu`, or enter a custom tag.
5. Keep `publish_latest` enabled to also publish `latest-linux-amd64-boogiemix-cpu`.

The workflow publishes to:

```text
ghcr.io/<owner>/boogiebox:<tag>
```

To run a published image on a Linux host:

```bash
docker pull ghcr.io/<owner>/boogiebox:0.8.82-linux-amd64-boogiemix-cpu
docker run -d \
  --name boogiebox \
  -p 3001:3001 \
  -v boogiebox-data:/var/lib/boogiebox \
  -v /srv/music:/music:ro \
  ghcr.io/<owner>/boogiebox:0.8.82-linux-amd64-boogiemix-cpu
```

## Persistent Data

Persist `/var/lib/boogiebox`.

This stores:

- `boogiebox-config.json`
- SQLite database and app data
- logs
- Torch model cache
- BoogieMix runtime temp/cache folders

The application under `/opt/boogiebox` is image-owned and should not be mounted over.

## Environment

The image sets these defaults:

```text
BOOGIEBOX_CONFIG_DIR=/var/lib/boogiebox
BOOGIEBOX_DATA_DIR=/var/lib/boogiebox/data
BOOGIEBOX_FFMPEG_DIR=/opt/boogiebox/resources/ffmpeg
BOOGIEBOX_LOG_DIR=/var/lib/boogiebox/logs
BOOGIEBOX_LOG_LEVEL=info
TORCH_HOME=/var/lib/boogiebox/model-cache/torch
PORT=3001
```

Optional diagnostics:

```bash
docker run --rm -e BOOGIEBOX_STARTUP_DIAGNOSTICS=1 ...
```

## BoogieMix CPU Notes

The Docker image forces CPU-only PyTorch. It does not use CUDA or GPU detection.

Expected runtime:

- Python: `/opt/boogiebox/resources/Services/boogiemix/python/.venv/bin/python`
- Torch CUDA availability: `false`
- Demucs model cache: `/var/lib/boogiebox/model-cache/torch`

CPU Demucs analysis is slow and memory-heavy. Keep BoogieMix deep-analysis concurrency at `1` unless the host has enough CPU and RAM.

## Verification

After the container starts:

```bash
curl http://localhost:3001/api/system/status
```

Then verify BoogieMix from the app Settings or:

```bash
curl http://localhost:3001/api/boogiemix/deep-analysis/status
```

Expected:

- FFmpeg available
- Python available
- Torch available
- Demucs callable
- GPU unavailable or false

Inside the container:

```bash
docker exec -it <container> bash
/opt/boogiebox/resources/Services/boogiemix/python/.venv/bin/python -c "import torch; print(torch.cuda.is_available())"
/opt/boogiebox/resources/Services/boogiemix/python/.venv/bin/python -c "import demucs"
```

The first command should print `False`; the second should exit successfully.

## Upgrade

1. Stop the old container.
2. Build or pull the new image.
3. Start the container with the same `/var/lib/boogiebox` volume.

The database and config remain in the persistent volume.

## WSL2 Testing

This image can be tested from WSL2 when Docker Desktop WSL integration is enabled.

From WSL:

```bash
docker version
docker buildx version
docker compose -f docker-compose.linux-amd64.yml up --build
```

Open `http://localhost:3001` from Windows or WSL.

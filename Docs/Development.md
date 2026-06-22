# Development

This guide covers local development for BoogieBox.

## Requirements

### Windows (full stack)

- Windows
- Git
- Node.js
- Rust and the MSVC toolchain
- WebView2 runtime
- FFmpeg and FFprobe
- Semgrep
- Optional: Python for BoogieMix deep analysis
- Optional: Inno Setup for installer builds

Run `setup-dev.bat` from the repo root to install or verify the common development prerequisites.

### Linux (server only)

- Rust toolchain (`rustup`)
- Node.js and npm (for the React client build)
- FFmpeg and FFprobe (bundled automatically by `build-server-rust.sh`)
- Optional: Python 3.10+ for BoogieMix deep analysis (`python3.12` / `python3.11` etc. from your distro)

## Install Dependencies

```bat
npm run install:all
```

The root `npm install` installs repo-level tooling. The client and Tauri desktop shell have their own package manifests under `client/` and `desktop/`.

## Run In Development

```bat
dev.bat
```

`dev.bat` prefers a matching Rust release server under `Releases\boogiebox-VERSION-win-rs\`, starts it with development configuration, then starts the Vite client when Node tooling is available.

## Build

### Windows

Build the Rust standalone server release:

```bat
build-server-rust.bat
```

Build only the Rust server release folder without an installer:

```bat
build-server-rust.bat --no-installer
```

Build the desktop shell:

```bat
build-desktop.bat
```

Build the desktop shell release folder without an installer:

```bat
build-desktop.bat --no-installer
```

### Linux

Build the Rust standalone server release folder (auto-downloads static FFmpeg on first run):

```bash
./build-server-rust.sh
```

Build without smoke test:

```bash
./build-server-rust.sh --no-test
```

Build and run a smoke probe:

```bash
./build-server-rust.sh --smoke
```

Output is placed in `Releases/boogiebox-VERSION-linux-rs/`.

## Verification

After code changes, run:

```bat
npm test
npm run lint
npm run security:semgrep
cargo clippy --manifest-path server-rs/Cargo.toml --all-targets --all-features -- -D warnings
```

Useful focused Rust commands:

```bat
cargo test --manifest-path server-rs/Cargo.toml
cargo fmt --manifest-path server-rs/Cargo.toml --all --check
cargo check --manifest-path server-rs/Cargo.toml
```

## Versioning

If a code file changes, run:

```bat
npm run version:bump
```

Then keep these version fields aligned:

- `client/src/version.ts`
- `desktop/package.json`
- `desktop/src-tauri/tauri.conf.json`
- `desktop/src-tauri/Cargo.toml`
- Rust workspace metadata in `server-rs/Cargo.toml`

Docs-only changes do not require a version bump.

## Database

BoogieBox creates and migrates SQLite during first-run setup. The database lives in the user-selected data folder. Packaged server installs store the database-folder locator at `%PROGRAMDATA%\BoogieBox\boogiebox-config.json`; development runs use the repo-root `boogiebox-config.json`.

Do not commit local database files, runtime config, logs, release output, temporary files, or dependency folders.

## Logs

**Windows:** Packaged server logs default to `%PROGRAMDATA%\BoogieBox\logs\boogiebox-server.log`.

**Linux:** Logs default to the working directory or systemd journal when running as a service. Use `BOOGIEBOX_LOG_DIR` to override.

The server supports:

- `BOOGIEBOX_LOG_PATH`
- `BOOGIEBOX_LOG_DIR`
- `BOOGIEBOX_LOG_LEVEL`

## Linux Install

After building with `build-server-rust.sh`, use the bundled installer:

```bash
sudo ./Releases/boogiebox-VERSION-linux-rs/install/install.sh
```

This copies the binary and resources to `/opt/boogiebox/`, creates a `boogiebox` system user, installs the systemd unit, and prints the first-run setup URL.

When run interactively the installer will prompt whether to also install BoogieMix deep analysis. Pass `--with-boogiemix` to enable it non-interactively:

```bash
sudo ./Releases/boogiebox-VERSION-linux-rs/install/install.sh --with-boogiemix
```

BoogieMix setup requires Python 3.10+, an internet connection, ~2–5 GB of disk space, and 10–30 minutes. It auto-detects an NVIDIA GPU and installs CUDA PyTorch when available, otherwise installs the CPU build. Installer output is logged to `/var/lib/boogiebox/installer-boogiemix.log`.

To install or re-run BoogieMix setup manually after the initial install:

```bash
sudo -u boogiebox TORCH_HOME=/var/lib/boogiebox/model-cache/torch \
  bash /opt/boogiebox/resources/Services/boogiemix/python/bootstrap_env.sh \
  --prime-model
```

Additional bootstrap flags:

| Flag | Description |
|------|-------------|
| `--cpu-only` | Force CPU PyTorch even if an NVIDIA GPU is present |
| `--cuda` | Force CUDA PyTorch |
| `--force` | Delete and recreate the existing `.venv` |
| `--log <file>` | Append output to a log file |

The systemd service sets `TORCH_HOME=/var/lib/boogiebox/model-cache/torch` so the model cache persists in the data directory across updates.

Override the config path for non-system installs:

```bash
BOOGIEBOX_CONFIG_DIR=~/.config/boogiebox ./boogiebox-server
```

## Public Repo Hygiene

Keep public-facing docs free of private machine paths, personal credentials, local database paths, provider tokens, and machine-specific assistant state. Local runtime artifacts should stay ignored.

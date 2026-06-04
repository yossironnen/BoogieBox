# Development

This guide covers local development for BoogieBox.

## Requirements

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

## Install Dependencies

```bat
npm install
cd client
npm install
cd ..
```

The root `npm install` installs repo-level tooling. The client has its own package manifest under `client/`.

## Run In Development

```bat
dev.bat
```

`dev.bat` prefers a matching Rust release server under `Releases\boogiebox-VERSION-win-rs\`, starts it with development configuration, then starts the Vite client when Node tooling is available.

## Build

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

Packaged server logs default to `%PROGRAMDATA%\BoogieBox\logs\boogiebox-server.log`. Development logging depends on the active server configuration.

The server supports:

- `BOOGIEBOX_LOG_PATH`
- `BOOGIEBOX_LOG_DIR`
- `BOOGIEBOX_LOG_LEVEL`

## Public Repo Hygiene

Keep public-facing docs free of private machine paths, personal credentials, local database paths, provider tokens, and machine-specific assistant state. Local runtime artifacts should stay ignored.

# Project Structure

BoogieBox is organized around a Rust backend, React client, Tauri desktop shell, optional BoogieMix Python worker assets, and Windows/Linux build scripts.

## Top Level

| Path | Purpose |
| --- | --- |
| `README.md` | Public project overview. |
| `CLAUDE.md` / `agents.md` | Agent instructions and high-level architecture summary. |
| `CONTRIBUTING.md` | Contribution guidelines. |
| `VERSION` | Canonical app version (synced by `npm run version:bump`). |
| `package.json` | Root developer scripts for tests, linting, Semgrep, and Rust checks. |
| `setup-dev.bat` | Interactive developer environment bootstrap. |
| `setup.bat` | Compatibility wrapper for `setup-dev.bat`. |
| `dev.bat` | Development launcher (Windows). |
| `dev.sh` | Development launcher (Linux); mirrors `dev.bat`. |
| `build-server-rust.bat` | Rust standalone server release and installer build flow (Windows). |
| `build-server-rust.sh` | Rust standalone server release build flow (Linux). |
| `build-desktop.bat` | Tauri desktop release build flow. |
| `build-docker-linux-amd64.bat` / `.sh` | Builds the Linux amd64 Docker image (CPU-only BoogieMix). |
| `Dockerfile.linux-amd64` / `docker-compose.linux-amd64.yml` | Linux amd64 Docker image and compose definitions. |
| `docker/linux-amd64/` | Container entrypoint for the Linux image. |
| `Docs/` | Public and project documentation. |
| `client/` | React and TypeScript client. |
| `e2e/` | Playwright UI release suite run against the packaged server build (see `e2e/README.md`). |
| `server-rs/` | Rust server and database crates. |
| `desktop/` | Tauri desktop shell. |
| `Services/boogiemix/python/` | Optional Python BoogieMix deep-analysis worker assets. |
| `installer/` | Installer definitions (Inno Setup for Windows, systemd unit and install script for Linux). |
| `scripts/` | Repo-level helper scripts (version sync, Semgrep runner, seed data, UPnP probe, stale-deps check). |
| `seed-random-playback-history.bat` / `seed-random-ratings.bat` | Dev wrappers for the seed scripts. |
| `website/` | Static public landing page (published by `.github/workflows/pages.yml`). |
| `_site/` | Committed copy of the landing page build. |
| `.github/` | GitHub Pages and Docker image workflows. |
| `tools/` | Local binary caches for packaged dependencies such as FFmpeg and WinSW. |

## Docs

| Path | Purpose |
| --- | --- |
| `Docs/Architecture.md` | Runtime architecture and component responsibilities. |
| `Docs/API.md` | REST API overview. |
| `Docs/Development.md` | Local setup, build, and verification workflow. |
| `Docs/ProjectStructure.md` | Repository layout reference. |
| `Docs/backend-safety.md` | Backend safety rules. |
| `Docs/testing.md` | Testing expectations. |
| `Docs/git.md` | Git workflow notes. |
| `Docs/DockerLinux.md` | Linux Docker image usage. |
| `Docs/Screenshots/` | Screenshots used by the README. |
| `Docs/Agents/` | Agent workflow documentation. |
| `Docs/memory/` | Repo-tracked project memory for development sessions. |

## Client

| Path | Purpose |
| --- | --- |
| `client/src/App.tsx` | App shell, routing, and global state. |
| `client/src/api.ts` | API client and response normalization. |
| `client/src/components/` | Desktop views and shared React components. |
| `client/src/mobile/` | Mobile shell and mobile-specific views. |
| `client/src/platform/` | Browser and desktop platform abstraction. |
| `client/src/remote/` | Play On hooks, SSE stream, PlayerState bridge, output picker, and takeover prompt. |
| `client/src/audio/` | EQ model and Vinyl Mode effects engine. |
| `client/src/hooks/` | Shared data hooks. |
| `client/src/__tests__/` | Client test suites. |
| `client/src/types/` | Shared TypeScript data shapes. |
| `client/src/version.ts` | Client-visible app version. |
| `client/public/` | Static assets copied into the Vite build. |
| `client/build/` | Generated client build output. |

## Rust Server

| Path | Purpose |
| --- | --- |
| `server-rs/Cargo.toml` | Rust workspace manifest. |
| `server-rs/crates/boogiebox-server/` | Axum server crate. |
| `server-rs/crates/boogiebox-server/src/main.rs` | Process entry and graceful shutdown. |
| `server-rs/crates/boogiebox-server/src/lib.rs` | `AppState`, route mounting, setup and database-switch flow. |
| `server-rs/crates/boogiebox-server/src/routes/` | API route groups. |
| `server-rs/crates/boogiebox-server/src/scanner.rs` | Music scanning. |
| `server-rs/crates/boogiebox-server/src/post_scan.rs` | Post-scan job handling. |
| `server-rs/crates/boogiebox-server/src/mix_worker.rs` | BoogieMix planning and rendering. |
| `server-rs/crates/boogiebox-server/src/deep_analysis.rs` | BoogieMix deep-analysis orchestration. |
| `server-rs/crates/boogiebox-server/src/media_serve.rs` | Shared ranged serving and transcodes (browser, DLNA, Play On). |
| `server-rs/crates/boogiebox-server/src/dlna.rs` | Optional DLNA MediaServer. |
| `server-rs/crates/boogiebox-server/src/upnp/` | UPnP AV control point: XML, device descriptions, SOAP, SSDP discovery, DIDL helpers. |
| `server-rs/crates/boogiebox-server/src/remote/` | Play On manager, renderer registry, media tokens, queue, and per-renderer session actors. |
| `server-rs/crates/boogiebox-server/src/bin/` | Maintenance utilities (`probe-init`, `perf-bench`, `boogiemix-feature-stats`). |
| `server-rs/crates/boogiebox-db/` | SQLite schema, migrations, and query helpers. |

## Route Groups

| Path | Purpose |
| --- | --- |
| `routes/auth_routes.rs` | Login, logout, and current-user endpoints. |
| `routes/library_routes.rs` | Libraries, scan jobs, and scan schedules. |
| `routes/music_routes.rs` | Artists, albums, tracks, search, genres, and stats. |
| `routes/playback_routes.rs` | Audio streaming, lyrics, history, and playback helpers. |
| `routes/playlist_routes.rs` | Playlists and playlist tracks. |
| `routes/artwork_routes.rs` | Artwork serving and caching. |
| `routes/settings_routes.rs` | Global and per-user settings. |
| `routes/boogiemix_routes.rs` | BoogieMix jobs, outputs, and deep-analysis status. |
| `routes/provider_routes.rs` | Metadata and provider integrations. |
| `routes/admin_routes.rs` | Admin users, libraries, folder browsing, queues, and diagnostics. |
| `routes/dlna_routes.rs` | Optional DLNA server controls. |
| `routes/crossfade_routes.rs` | Crossfade settings and overrides. |
| `routes/remote_routes.rs` | Play On renderer list/refresh/admin edits, icons, diagnostics, and `/media/r/{token}`. |
| `routes/remote_session_routes.rs` | Play On session load/command/queue/transfer/end and the `/api/remote/events` SSE stream. |

## Desktop

| Path | Purpose |
| --- | --- |
| `desktop/package.json` | Desktop package metadata and scripts. |
| `desktop/src-tauri/tauri.conf.json` | Tauri app configuration. |
| `desktop/src-tauri/src/lib.rs` | Tauri app setup. |
| `desktop/src-tauri/src/commands.rs` | Client-callable desktop commands. |
| `desktop/src-tauri/src/config.rs` | Desktop config persistence. |
| `desktop/src-tauri/src/server.rs` | Server probing and discovery. |
| `desktop/src-tauri/src/server_process.rs` | Packaged server process control. |
| `desktop/src-tauri/src/windows/` | Windows-specific helpers. |
| `desktop/src-tauri/icons/` | Desktop application icons. |

## BoogieMix Python Assets

| Path | Purpose |
| --- | --- |
| `Services/boogiemix/python/README.md` | Python worker notes. |
| `Services/boogiemix/python/boogiemix_demucs_worker.py` | Demucs-based deep-analysis worker (main entry point). |
| `Services/boogiemix/python/demucs_runner.py` | Thin Demucs wrapper that patches torchaudio to use soundfile as the WAV backend. |
| `Services/boogiemix/python/beatgrid_runner.py` | madmom beat/downbeat tracking in its own process (avoids the OpenMP clash with librosa). |
| `Services/boogiemix/python/build-wheels.bat` | Builds pre-compiled wheels bundled with the Windows installer. |
| `Services/boogiemix/python/bootstrap_env.ps1` | Python environment bootstrap for Windows. |
| `Services/boogiemix/python/bootstrap_env.sh` | Python environment bootstrap for Linux. |
| `Services/boogiemix/python/requirements.txt` | Python dependency list. |
| `Services/boogiemix/python/constraints.txt` | Python dependency version constraints. |

## Generated Or Local-Only Paths

These paths are generated, machine-specific, or release artifacts and should not be treated as source:

- `node_modules/`
- `client/node_modules/`
- `desktop/node_modules/`
- `client/build/`
- `desktop/dist/`
- `server-rs/target/`
- `Releases/`
- `tmp/`
- `wip/` (local plans, gitignored)
- local SQLite databases and runtime config files

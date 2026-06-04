# Project Structure

BoogieBox is organized around a Rust backend, React client, Tauri desktop shell, optional BoogieMix Python worker assets, and Windows build scripts.

## Top Level

| Path | Purpose |
| --- | --- |
| `README.md` | Public project overview. |
| `package.json` | Root developer scripts for tests, linting, Semgrep, and Rust checks. |
| `setup-dev.bat` | Interactive developer environment bootstrap. |
| `dev.bat` | Development launcher. |
| `build-server-rust.bat` | Rust standalone server release and installer build flow. |
| `build-desktop.bat` | Tauri desktop release build flow. |
| `Docs/` | Public and project documentation. |
| `client/` | React and TypeScript client. |
| `server-rs/` | Rust server and database crates. |
| `desktop/` | Tauri desktop shell. |
| `server/Services/boogiemix/` | Optional Python BoogieMix deep-analysis worker assets. |
| `installer/` | Inno Setup installer definition. |
| `scripts/` | Repo-level helper scripts. |
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
| `client/src/types/` | Shared TypeScript data shapes. |
| `client/src/version.ts` | Client-visible app version. |
| `client/public/` | Static assets copied into the Vite build. |
| `client/build/` | Generated client build output. |

## Rust Server

| Path | Purpose |
| --- | --- |
| `server-rs/Cargo.toml` | Rust workspace manifest. |
| `server-rs/crates/boogiebox-server/` | Axum server crate. |
| `server-rs/crates/boogiebox-server/src/lib.rs` | Server assembly and setup flow. |
| `server-rs/crates/boogiebox-server/src/routes/` | API route groups. |
| `server-rs/crates/boogiebox-server/src/scanner.rs` | Music scanning. |
| `server-rs/crates/boogiebox-server/src/post_scan.rs` | Post-scan job handling. |
| `server-rs/crates/boogiebox-server/src/mix_worker.rs` | BoogieMix planning and rendering. |
| `server-rs/crates/boogiebox-server/src/deep_analysis.rs` | BoogieMix deep-analysis orchestration. |
| `server-rs/crates/boogiebox-db/` | SQLite schema, migrations, and query helpers. |

## Route Groups

| Path | Purpose |
| --- | --- |
| `routes/auth_routes.rs` | Login, logout, and current-user endpoints. |
| `routes/library_routes.rs` | Libraries and scanning. |
| `routes/music_routes.rs` | Artists, albums, tracks, search, genres, and stats. |
| `routes/playback_routes.rs` | Audio streaming, lyrics, history, and playback helpers. |
| `routes/playlist_routes.rs` | Playlists and playlist tracks. |
| `routes/artwork_routes.rs` | Artwork serving and caching. |
| `routes/settings_routes.rs` | Global and per-user settings. |
| `routes/boogiemix_routes.rs` | BoogieMix jobs, outputs, and deep-analysis status. |
| `routes/provider_routes.rs` | Metadata and provider integrations. |
| `routes/admin_routes.rs` | Admin queue and diagnostic surfaces. |
| `routes/dlna_routes.rs` | Optional DLNA server controls. |
| `routes/crossfade_routes.rs` | Crossfade settings and overrides. |

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
| `desktop/src-tauri/icons/` | Desktop application icons. |

## BoogieMix Python Assets

| Path | Purpose |
| --- | --- |
| `server/Services/boogiemix/README.md` | Python worker notes. |
| `server/Services/boogiemix/boogiemix_demucs_worker.py` | Optional Demucs-based deep-analysis worker. |
| `server/Services/boogiemix/bootstrap_env.ps1` | Python environment bootstrap. |
| `server/Services/boogiemix/requirements.txt` | Python dependency list. |

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
- local SQLite databases and runtime config files

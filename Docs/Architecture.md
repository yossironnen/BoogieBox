# Architecture

BoogieBox is a self-hosted music library application for Windows and Linux. The packaged runtime is a Rust Axum server with a React client, SQLite storage, optional Tauri desktop shell (Windows only), FFmpeg-powered audio processing, and optional Python assets for BoogieMix deep analysis.

## Runtime Shape

- `server-rs/` contains the primary backend runtime.
- `client/` contains the browser/mobile React application.
- `desktop/` contains the Tauri 2 desktop wrapper.
- `Services/boogiemix/python/` contains optional Python worker assets for deep music analysis.
- `build-server-rust.bat` builds the standalone Windows server release and installer flow.
- `build-server-rust.sh` builds the standalone Linux server release and tarball flow.
- `build-desktop.bat` builds the desktop shell release flow.

The server owns authentication, setup, the SQLite database connection, scanning, post-scan jobs, transcoding, metadata/artwork providers, BoogieMix planning/rendering, and static delivery of the built client.

## Backend

The Rust backend is split into two crates:

- `server-rs/crates/boogiebox-server`: Axum HTTP server, route handlers, workers, streaming, provider calls, and runtime configuration.
- `server-rs/crates/boogiebox-db`: SQLite schema, migrations, and query helpers.

Important server modules:

- `src/lib.rs`: app assembly, setup flow, logging, and server startup.
- `src/routes/`: grouped API routes for auth, libraries, music, playlists, settings, artwork, playback, DLNA, providers, admin, and BoogieMix.
- `src/scanner.rs`: music library scanning.
- `src/post_scan.rs`: follow-up jobs after scans.
- `src/artwork_cache.rs`: artwork caching.
- `src/waveform_map.rs`: waveform generation.
- `src/bpm_analysis.rs`: BPM analysis.
- `src/mix_worker.rs`: BoogieMix planning and rendering.
- `src/beat_grid.rs`: beat-grid detection used for BoogieMix transitions.
- `src/mix_priority_gate.rs`: BoogieMix job priority/concurrency gating.
- `src/story_image.rs`: BoogieMix story image generation.
- `src/deep_analysis.rs`: optional BoogieMix deep-analysis queueing and status.
- `src/artist_radio.rs`: Artist Radio queue building.
- `src/similar_artists.rs`: locally-owned similar-artist resolution.
- `src/radio_metadata.rs`: background Last.fm/MusicBrainz/ListenBrainz tag and mood-tag collection.
- `src/tag_taxonomy.rs`: style/mood tag normalization for Artist Radio.
- `src/db_warmup.rs`: background cache warm-up on startup and library switch.
- `src/ffmpeg.rs`: FFmpeg and FFprobe resolution.
- `src/settings.rs`: global and per-user settings normalization.
- `src/server_config.rs`: database locator and packaged runtime config.

## Frontend

The React client is the main user interface for desktop browser, mobile browser, and the desktop shell.

Important client modules:

- `client/src/App.tsx`: top-level routing and global app state.
- `client/src/api.ts`: typed API client and URL helpers.
- `client/src/components/`: desktop views and shared UI components.
- `client/src/mobile/`: mobile shell and mobile-specific views.
- `client/src/platform/`: browser and Tauri desktop platform abstractions.
- `client/src/types/`: shared TypeScript payload types.
- `client/src/version.ts`: app version displayed by the client.

### UI Design Language

The client uses an **icon-first design**: stat/metric boxes, tabs, and pill toggles all carry a small leading icon rather than bare text or numbers (see `App.tsx`'s `StatsBar`, `HomeView.tsx`'s `StatsWidget`/"Let's Boogie!" metrics, and the Browse page's Artists/Albums tabs). Icons are inline SVG (`viewBox="0 0 24 24"`, stroke-based, `currentColor`), sized ~14-20px inline or ~28-40px on cards/tiles; stat icons are tinted `var(--accent)`. Unbounded lists (e.g. genres) reuse one generic icon rather than a unique icon per item. New UI work should follow this convention — see `CLAUDE.md` for the full rule set.

## Desktop Shell

The `desktop/` project is a Tauri 2 wrapper around the web client. It provides Windows desktop packaging, local server discovery, and optional control of a packaged BoogieBox server executable.

Important desktop modules:

- `desktop/src-tauri/src/lib.rs`: Tauri application setup.
- `desktop/src-tauri/src/commands.rs`: commands exposed to the client.
- `desktop/src-tauri/src/config.rs`: desktop configuration handling.
- `desktop/src-tauri/src/server.rs`: server probing and discovery helpers.
- `desktop/src-tauri/src/server_process.rs`: optional packaged server process control.

## Data Model

BoogieBox stores data in SQLite inside the folder selected during first-run setup. The packaged server stores the database-folder locator in `%PROGRAMDATA%\BoogieBox\boogiebox-config.json`; dev runs use the repo-root locator.

Core tables include:

- `libraries`, `library_folders`
- `tracks`, `artists`, `albums`, `tracks_fts`
- `artist_ratings`, `album_ratings`, `track_ratings`
- `scan_jobs`, `post_scan_jobs`, `scan_schedules`
- `playlists`, `playlist_tracks`
- `track_waveforms`, `track_deep_analysis`
- `mix_jobs`
- `settings`, `user_settings`, `provider_usage_stats`

## Background Work

Scanning discovers local music, writes normalized library rows, and queues post-scan work. Post-scan jobs handle artwork caching, waveform generation, BPM analysis, and optional deep-analysis work. BoogieMix rendering runs as a separate planning/rendering flow and writes downloadable mix outputs.

## Security Boundary

BoogieBox is intended for trusted local networks. It uses authenticated API sessions and protects local file access through server-side path validation, but it is not designed for direct public internet exposure.

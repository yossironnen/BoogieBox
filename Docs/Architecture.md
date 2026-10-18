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
- `installer/` holds the Inno Setup script (Windows) and the systemd unit plus `install.sh` (Linux).
- `website/` is the static public landing page (not served by the app).

The server owns authentication, setup, the SQLite database connection, scanning, post-scan jobs, transcoding, metadata/artwork providers, BoogieMix planning/rendering, the optional DLNA MediaServer, Play On (playback on LAN UPnP/DLNA renderers), and static delivery of the built client.

## Backend

The Rust backend is split into two crates:

- `server-rs/crates/boogiebox-server`: Axum HTTP server, route handlers, workers, streaming, provider calls, and runtime configuration.
- `server-rs/crates/boogiebox-db`: SQLite schema, migrations, and query helpers.

Important server modules:

- `src/main.rs`: process entry, Tokio runtime, and graceful shutdown.
- `src/lib.rs`: `AppState`, route mounting, setup and database-switch flow, middleware, and static client delivery.
- `src/routes/`: grouped API routes for auth, libraries/scans/schedules, music, playlists, settings, artwork, playback, crossfade, DLNA, Play On, providers, admin, and BoogieMix.
- `src/auth.rs`: PIN hashing, sessions, brute-force tracking, and Axum auth extractors.
- `src/cors.rs`: local/LAN origin allowlist.
- `src/media_serve.rs`: shared ranged file serving and MP3/WAV transcodes for browser playback, DLNA, and Play On.
- `src/dlna.rs`: optional DLNA MediaServer (SSDP + ContentDirectory).
- `src/upnp/`: UPnP AV control point (XML parsing, device descriptions, SOAP, SSDP discovery, shared DIDL helpers).
- `src/remote/`: Play On: `RemoteManager` (in `AppState`), renderer registry, short-lived media tokens (`/media/r/{token}`), pure queue logic, and one session actor per renderer.
- `src/subsonic/`: OpenSubsonic-compatible `/rest/{method}` API for third-party apps: parameter parsing (query + form), Subsonic-password auth (AES-GCM at rest, `subsonic.key` beside the database, cached; never the PIN), one node tree rendered to XML/JSON/JSONP, browse/list/search/playlist handlers, library-confined `stream`/`download` with a transcode limiter, and `getCoverArt` via the artwork cache. `routes/subsonic_routes.rs` manages the passwords for the UI.
- `src/providers.rs`: Discogs, Deezer, Spotify, LRCLIB, and lyrics.ovh clients.
- `src/image_thumb.rs`: JPEG thumbnail generation.
- `src/home_cache.rs`: bounded, change-counter-invalidated Home query cache.
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
- `src/db_maintenance.rs`: weekly late-night `PRAGMA incremental_vacuum` sweep.
- `src/logging.rs`: `server.log` plus live-toggled scan and deep-analysis debug logs.
- `src/event_log.rs`: Windows Event Log startup banner.
- `src/ffmpeg.rs`: FFmpeg and FFprobe resolution.
- `src/settings.rs`: global and per-user settings normalization.
- `src/server_config.rs`: database locator and packaged runtime config.
- `src/bin/`: maintenance utilities (`probe-init`, `perf-bench`, `boogiemix-feature-stats`).

`boogiebox-db` modules are split by domain: `music`, `playback`, `playlists`, `artwork`, `boogiemix`, `jobs`, `radio`, `remote`, and `maintenance`.

## Frontend

The React client is the main user interface for desktop browser, mobile browser, and the desktop shell.

Important client modules:

- `client/src/App.tsx`: top-level routing and global app state.
- `client/src/api.ts`: typed API client and URL helpers.
- `client/src/components/`: desktop views and shared UI components.
- `client/src/mobile/`: mobile shell and mobile-specific views.
- `client/src/platform/`: browser and Tauri desktop platform abstractions.
- `client/src/remote/`: Play On: `usePlayOn` orchestration, per-browser playback target, shared SSE stream (`/api/remote/events`), `remoteBridge` mapping `PlayerState` changes to server actions, output picker, and takeover prompt.
- `client/src/audio/`: Web Audio EQ model and Vinyl Mode effects engine.
- `client/src/hooks/`: shared data hooks (Home data, deep-analysis status, scan activity, adaptive accent).
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
- `desktop/src-tauri/src/windows/`: Windows-specific helpers.

## Data Model

BoogieBox stores data in SQLite inside the folder selected during first-run setup. The packaged server stores the database-folder locator in `%PROGRAMDATA%\BoogieBox\boogiebox-config.json` on Windows or `/etc/boogiebox/boogiebox-config.json` on Linux; dev runs use the repo-root locator. Schema and migrations live in `boogiebox-db`; tracked migrations run once per id, so schema changes always add a new migration.

Core tables include:

- `users`, `sessions`
- `libraries`, `library_folders`, `artist_libraries`, `album_libraries`
- `tracks`, `artists`, `albums`, `tracks_fts`
- `artist_merges`, `artist_merge_members`, `artist_merge_moves`, `artist_name_aliases`
- `artist_ratings`, `album_ratings`, `track_ratings`, `play_history`
- `artist_styles`, `album_styles`, `track_tags`, `track_tag_sync`, `lastfm_cache`, `lyrics_cache`
- `scan_jobs`, `post_scan_jobs`, `scan_schedules`
- `playlists`, `playlist_tracks`, `crossfade_overrides`
- `track_waveforms`, `track_deep_analysis`, `track_mix_analysis`, `deep_analysis_jobs`
- `mix_jobs`, `mix_job_logs`, `mix_outputs`, `mix_output_tracks`, `mix_transitions`, `boogiemix_plans`
- `upnp_renderers`, `remote_sessions`
- `settings`, `user_settings`, `provider_usage_stats`, `stats_cache`

## Background Work

Scanning discovers local music, writes normalized library rows, and queues post-scan work. Post-scan jobs handle artwork caching, waveform generation, BPM analysis, and optional deep-analysis work. Background loops also collect radio tags/moods, warm caches, and run weekly database maintenance. BoogieMix rendering runs as a separate planning/rendering flow and writes downloadable mix outputs. Deep analysis shells out to the optional Python worker; Demucs (`demucs_runner.py`) and madmom beat tracking (`beatgrid_runner.py`) each run in their own process.

## Network Playback

- **DLNA MediaServer** (`dlna.rs`): optional; exposes the library to DLNA clients over SSDP + ContentDirectory.
- **Play On** (`upnp/` + `remote/`): BoogieBox acts as a UPnP AV control point. `RemoteManager` starts after setup (and after a database switch), discovers MediaRenderers, and runs one session actor per renderer that owns the server-side queue, drives the device over SOAP, polls state, keeps gapless `SetNext` prefetch in sync, and records plays. Renderers fetch audio and artwork from unauthenticated, short-lived token URLs (`/media/r/{token}`). Sessions persist in `remote_sessions` and are restored when the renderer is rediscovered. Another user's active session requires an explicit takeover. Play On does not depend on the DLNA server being enabled.

## Security Boundary

BoogieBox is intended for trusted local networks. It uses authenticated API sessions and protects local file access through server-side path validation, but it is not designed for direct public internet exposure. Play On renderer media is served without a session, behind random, short-lived tokens scoped to one track, mix, or cover.

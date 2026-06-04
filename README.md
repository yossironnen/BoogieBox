# BoogieBox

Self-hosted Windows music library app with a standalone Rust server package, multi-user support, scanning, fast search and browse, playlists, in-app playback, waveform/BPM background jobs, BoogieMix playlist rendering, and optional DLNA/UPnP serving.

Current version: `0.8` (see [`client/src/version.ts`](client/src/version.ts))

## Warning: Local Network Use Only

Built for local network streaming only. Not tested for internet-facing use and not hardened for public internet exposure. Do not expose BoogieBox directly to the internet.

## Highlights

- Multi-user login with avatars, optional 4-digit PIN, and admin/user permissions
- First-run setup wizard prompts for database folder path (local or UNC)
- Music-only libraries with multi-folder scans, fast browse/search, playlists, and per-user playback preferences
- Windows desktop client via Tauri 2 with WebView2 shell and first-run LAN server discovery
- Library scan pipeline with dedicated workers plus lane-limited post-scan follow-up jobs and admin queue controls
- Full player stack: queue, shuffle/repeat, waveform seek bar, lyrics/karaoke, vinyl mode, and transition controls
- EQ + Auto-EQ: 7-band parametric EQ, built-in presets, custom profiles, and artist-tag matching
- Additive iPhone-sized mobile shell with bottom tabs, mini-player, full-screen now playing, and drill-down browse/search/playlist flows
- Discovery features: Auto DJ, Last.fm artist context, Home insights, and Genre Galaxy
- BoogieMix playlist-to-mix rendering with AI-assisted planning and optional deep analysis
- Per-user 0.5-step ratings for artists, albums, and tracks
- Album and artist artwork caching, post-scan warmup, thumbnail generation, and manual artwork upload
- Scheduled waveform and BPM background analysis
- Optional integrations: Last.fm metadata, Discogs/Deezer/Spotify artwork and metadata fallback paths, Genius lyrics, DLNA/UPnP serving, and experimental Python deep analysis
- FFmpeg/FFprobe bundled in standalone releases for media probing and transcoding
- All entity IDs are UUID v7 strings end-to-end
- Scan debug logging: default-off toggle writes structured diagnostics to `logs/debug.log`

## Tech Stack

| Layer | Tech |
|-------|------|
| Backend | Rust (Axum + Tokio) compiled as a native Windows executable |
| Database | SQLite via `rusqlite` (bundled) + FTS5 |
| Frontend | React + TypeScript + Vite |
| Desktop | Tauri 2 + Rust |
| Music metadata | Pure-Rust tag parsers (ID3v2, FLAC, Ogg/Opus, MP4/M4A, WAV, AIFF) |
| Media probing/transcoding | Bundled FFmpeg/FFprobe |
| Image processing | Pure-Rust `image` crate (JPEG thumbnails, Lanczos3) |
| ID generation | UUID v7 strings for all primary keys |
| DLNA | Rust SSDP + Axum DLNA HTTP server (audio only) |
| Optional deep analysis | Python + Torch/Demucs worker under `Services/boogiemix/python/` |

## Supported Formats

**Music:** MP3, FLAC, M4A/MP4 audio, OGG, OPUS, WAV, AAC, WMA, AIFF, APE

## Prerequisites

### End users (standalone EXE release)

- Windows 10/11 or Windows Server
- No Node.js, npm, or global FFmpeg required — all bundled
- Use the installer EXE, or extract the release folder and run directly

### Developers

BoogieBox development is Windows-native. Use Command Prompt or PowerShell from the repo root.

| Tool | Requirement |
|------|-------------|
| Git | Clone and update the repo; `setup-dev.bat` can install it with WinGet |
| Visual Studio Code | Optional editor; `setup-dev.bat` can install it with WinGet |
| Node.js + npm | Node.js 22+ for TypeScript, Vite, tests, and npm scripts |
| Windows PowerShell | Used by setup scripts |
| FFmpeg + FFprobe | Source-tree and release build media tooling; `setup-dev.bat` downloads pinned local binaries to `tools\ffmpeg` |
| Semgrep CLI | Required by every local build gate; `setup-dev.bat` installs it with Python pip |

Additional build-machine requirements:

| Build target | Additional requirements |
|--------------|------------------------|
| Standalone server EXE (Rust) | [Rust via rustup](https://www.rust-lang.org/tools/install/) + MSVC C++ Build Tools; Node.js 22+ for Vite build; `tools\ffmpeg\ffmpeg.exe` + `tools\ffmpeg\ffprobe.exe` pinned binaries |
| Windows installer | Rust EXE requirements + [Inno Setup 6](https://jrsoftware.org/isdl.php) + [WinSW v2.12.0](https://github.com/winsw/winsw/releases/tag/v2.12.0) x64 renamed to `tools\winsw\boogiebox-service.exe` |
| Desktop Tauri package | Rust EXE requirements + [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) + [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) |

Optional BoogieMix deep analysis requires Python plus Torch/Demucs. `setup-dev.bat` installs Python, and the worker bootstrap lives at `Services\boogiemix\python\bootstrap_env.ps1`.

## Setup and Run

### Standalone EXE (recommended for end users)

1. Download and run `boogiebox-x.x.x-win-setup.exe`, or extract the `boogiebox-x.x.x-win` release folder
2. Launch BoogieBox from the Start Menu, `start.bat`, or `boogiebox-server.exe`
3. Open `http://localhost:3001` in your browser if it doesn't open automatically
4. Follow the setup wizard on first launch

No Node.js, npm, or global FFmpeg required. FFmpeg is bundled under `resources\ffmpeg\`.

**Windows service installation:** When service install is selected, setup defaults to creating a local `.\BoogieBoxService` account. For UNC media shares, use an existing Windows account (`DOMAIN\User`, `COMPUTER\User`, or `.\User`) that can authenticate to the share. Grant that account read access to UNC media shares and write access to any external database, artwork/cache, or BoogieMix output folders. Use UNC paths (`\\nas\Music`), not drive mappings. If service account setup fails, inspect `%PROGRAMDATA%\BoogieBox\installer-service.log`.

### Developer setup

1. Clone this repo.
2. Run the development bootstrap from the repo root:

```bat
setup-dev.bat
```

The script prompts before every install/configuration step with yes as the default. It reports when Git or VS Code are already installed, then can install/configure Git, VS Code, Node.js 22+, Rust stable MSVC, C++ Build Tools, WebView2, Python 3.12, Inno Setup, Semgrep, npm dependencies, local FFmpeg/FFprobe, WinSW, `dev.config`, and `BOOGIEBOX_FFMPEG_DIR`.

Verify commands:

```bat
git --version && node --version && npm --version && cargo --version && semgrep --version
tools\ffmpeg\ffmpeg.exe -version
npm run lint
npm test
npm run security:semgrep
cargo clippy --manifest-path server-rs/Cargo.toml --all-targets --all-features -- -D warnings
```

### Developer: required build caches

Release packaging uses pinned local binaries (not on PATH):

```text
tools\ffmpeg\ffmpeg.exe
tools\ffmpeg\ffprobe.exe
tools\winsw\boogiebox-service.exe   (installer builds only)
```

These are gitignored build-machine inputs.

### Developer: configuration files

First source-tree startup writes the database-folder locator to repo-root `boogiebox-config.json` after setup completes. Packaged installs use `%PROGRAMDATA%\BoogieBox\boogiebox-config.json`.

Provider keys are entered in **Settings → Integrations**. For `dev.bat` only, repo-root `dev.config` can load development integration keys into the database:

```json
{
  "integrations": {
    "lastfmKey": "",
    "discogsToken": "",
    "spotifyClientId": "",
    "spotifyClientSecret": "",
    "geniusClientId": "",
    "geniusClientSecret": ""
  }
}
```

Keep real tokens out of commits.

Useful runtime overrides:

| Variable | Purpose |
|----------|---------|
| `BOOGIEBOX_FFMPEG_DIR` | Prefer a specific folder containing `ffmpeg.exe` and `ffprobe.exe` |
| `BOOGIEBOX_CONFIG_PATH` | Use an explicit database-locator config file path |
| `BOOGIEBOX_CONFIG_DIR` | Use an explicit directory for the database-locator config file |
| `BOOGIEBOX_DEBUG_LOG_PATH` | Write debug logging to a specific local file |
| `BOOGIEBOX_SERVER_EXE` | Desktop packaged-server lookup override |
| `BOOGIEBOX_LOG_PATH` | Write packaged server logs to a specific file |
| `BOOGIEBOX_LOG_DIR` | Write packaged server logs under a specific directory |
| `BOOGIEBOX_LOG_LEVEL` | Set packaged server tracing verbosity |

### Developer: development mode

The repo has a packaged-server development launcher:

```bat
dev.bat
```

Build a standalone server release first with `build-server-rust.bat --no-installer`, then `dev.bat` starts the matching release EXE in development mode and the Vite client.

- API: `http://localhost:3001`
- Web app: `http://localhost:3000`

### Developer: standalone Rust EXE build

```bat
build-server-rust.bat
```

Produces `Releases/boogiebox-x.x.x-win-rs/boogiebox-server.exe` and, when Inno Setup 6 is available, `Releases/boogiebox-x.x.x-win-rs-setup.exe`. Use `--no-installer` for a faster release-folder-only build. Use `--smoke` to build, start, probe `/api/system/status`, and stop.

### Developer: desktop client

```bat
build-desktop.bat
```

Builds the Tauri 2 desktop package.

### Developer: experimental Python setup

```bat
cd Services\boogiemix\python
powershell -ExecutionPolicy Bypass -File bootstrap_env.ps1 -Auto -PrimeDemucsModel
```

Sets up the BoogieMix Python deep-analysis environment including Demucs/Torch. `setup-dev.bat` handles the base Python install; the bootstrap creates the worker `.venv`, installs dependencies, and can prime the default Demucs model.

## First Run

On first launch, a **setup wizard** prompts for a database folder path (local or UNC). After setup, a default `admin` user is created with no PIN. The login screen attempts a no-PIN sign-in first and only shows PIN entry if required.

## User Management

Managed in **Settings → Users** (admin only):

- Create users with `admin` or `user` role
- Set or clear a 4-digit PIN per user
- Grant per-user permissions: `Can scan libraries`, `Can edit metadata`
- Remove users

Theme and playback preferences are stored per-user server-side.

### Shared (admin-controlled) settings

Set by admin in **Settings → Advanced**:

| Setting | Description |
|---------|-------------|
| Transcode quality | MP3 output quality (192/320 kbps) |
| ReplayGain normalization | EBU R128 loudness normalization during transcoding |
| Default vinyl mode | Whether all users start in vinyl mode on login |
| Waveform background jobs | Auto-generate missing waveform data on a schedule |
| BPM background jobs | Periodically analyze missing BPM values |
| Scan debug logging | Writes scan diagnostics to `logs/debug.log` (default off) |
| DLNA server | UPnP/DLNA toggle, friendly name, and port |
| BoogieMix output folder | Optional custom folder for rendered mixes |

## Architecture Notes

### Rust Server (`server-rs/crates/boogiebox-server/src/`)

| File | Purpose |
|------|---------|
| `main.rs` | Startup, Tokio runtime, graceful shutdown |
| `lib.rs` | AppState, route mounting, CORS, middleware |
| `auth.rs` | PBKDF2-SHA512 PIN hashing, UUID v7 sessions, brute-force tracker, Axum extractors |
| `cors.rs` | Localhost/loopback/private-LAN/single-label origin allowlist |
| `server_config.rs` | `boogiebox-config.json` locator (ProgramData, exe-adjacent, CWD, env overrides) |
| `settings.rs` | Global + per-user settings normalization |
| `ffmpeg.rs` | FFmpeg/FFprobe resolution, transcode spawn, waveform generation |
| `scanner.rs` | Async music scan worker (ID3v2, FLAC, Ogg, MP4/M4A tag parsing, technical metadata) |
| `post_scan.rs` | Async post-scan lanes (art caching, Last.fm, lyrics, waveform/BPM/deep-analysis follow-ups) |
| `bpm_analysis.rs` | FFmpeg-backed BPM batch analysis |
| `waveform_map.rs` | FFmpeg-backed waveform batch mapping |
| `mix_worker.rs` | BoogieMix render + AI planner + deep-analysis Python orchestration |
| `dlna.rs` | SSDP + Axum DLNA server, ContentDirectory Browse, audio streaming |
| `providers.rs` | Discogs, Deezer, Spotify, LRCLIB, lyrics.ovh |
| `artwork_cache.rs` | SHA-1 cache-key shards, UUID v7 markers, folder.jpg discovery |
| `image_thumb.rs` | JPEG thumbnail generation (pure Rust `image` crate) |
| `deep_analysis.rs` | Python/Demucs/Torch capability detection and job orchestration |
| `routes/` | Axum route modules per domain (admin, auth, settings, library, music, playlist, playback, crossfade, artwork, provider, boogiemix, dlna) |

### Desktop (`desktop/`)

| File / Folder | Purpose |
|---------------|---------|
| `src-tauri/` | Tauri 2 Rust shell, commands, config, and bundle metadata |
| `package.json` | Desktop build/check scripts |

### Client (`client/src/`)

| File / Folder | Purpose |
|---------------|---------|
| `App.tsx` | App shell, global state, navigation, theme, first-run gate, auth gate |
| `api.ts` | Typed API client |
| `entityId.ts` | `ClientEntityId` type and UUID-aware id helpers |
| `uiPhase2.ts` | Shared style-token vocabulary |
| `mobile/` | Additive mobile shell, tab bar, mini-player, now-playing, mobile views |
| `components/LoginScreen.tsx` | User picker with avatar grid and PIN entry |
| `components/SetupView.tsx` | First-run database folder picker |
| `components/HomeView.tsx` | Dashboard: Top Rated, Let's Boogie!, Recent Albums, playlists, Genre Galaxy |
| `components/Player.tsx` | Playback UI, dual-audio transition engine, queue, visualizer modes, waveform, EQ, lyrics |
| `components/BrowseView.tsx` | Artist/album browser, Refine popover, rating controls, vinyl mode |
| `components/PlaylistsView.tsx` | Playlist editing, BoogieMix job creation, progress, cancel, download |
| `components/SettingsPage.tsx` | Settings UI |

## Project Structure

```text
boogiebox/
├── client/
│   └── src/
│       ├── components/         desktop UI components
│       ├── mobile/             additive iPhone shell and mobile views
│       ├── types/              shared TypeScript types
│       ├── __tests__/
│       ├── api.ts
│       ├── App.tsx
│       └── version.ts
├── server-rs/                  Rust Axum server (primary runtime)
│   ├── crates/
│   │   ├── boogiebox-server/   HTTP server, routes, auth, DLNA, BoogieMix, scanner
│   │   └── boogiebox-db/       SQLite schema, migrations, query helpers
│   └── Cargo.toml
├── Services/
│   └── boogiemix/
│       ├── ai/                  BoogieMix planning assets
│       └── python/              optional Torch/Demucs deep-analysis worker
├── desktop/                    Tauri 2 Windows desktop shell
│   ├── src-tauri/
│   └── package.json
├── setup-dev.bat               prompted developer machine bootstrap
├── dev.bat                     development mode (packaged Rust EXE + Vite client)
├── build-server-rust.bat       standalone Rust server EXE and installer build
├── build-desktop.bat           desktop package build
├── seed-random-ratings.bat     test data seeding helper
├── seed-random-playback-history.bat
├── tools/ffmpeg/               gitignored local FFmpeg/FFprobe build cache
└── tools/winsw/                gitignored local WinSW service wrapper cache
```

Artwork cache layout (inside configured DB folder):

```text
<db-folder>/art/
└── music/
    ├── <key-shard>/<uuid>.ext          album/artist original
    └── thumb/{300,800}/<key-shard>/<uuid>.webp
```

## Selected API Endpoints

### Public (no auth)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/system/status` | Server status: ffmpeg available, setup required, suggested DB folder |
| POST | `/api/system/setup` | First-run: set database folder |
| GET | `/api/auth/users` | List users for login screen (`id`, `username`) |
| POST | `/api/auth/login` | Login with userId + optional PIN |
| POST | `/api/auth/logout` | Clear session cookie |

### Authenticated

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/auth/me` | Current user info |
| GET | `/api/libraries` | List libraries |
| POST | `/api/libraries/:id/scan` | Enqueue scan job |
| GET | `/api/scan-jobs/active` | Aggregated active scan jobs |
| GET | `/api/search` | Full-text search |
| GET | `/api/artists` | List artists (paginated) |
| GET | `/api/albums` | List albums |
| GET | `/api/albums/latest` | Recently added albums |
| GET | `/api/albums/:id/tracks` | Album tracks |
| GET | `/api/home/top-rated` | Current user's top-rated artists, albums, and tracks |
| PATCH | `/api/artists/:id/rating` | Set or clear artist rating (0.5–5.0 or null) |
| PATCH | `/api/albums/:id/rating` | Set or clear album rating |
| PATCH | `/api/tracks/:id/rating` | Set or clear track rating |
| GET | `/api/albums/:id/art?size=300\|800` | Sized album art thumbnail (cached WebP) |
| GET | `/api/artists/:id/photo?size=300\|800` | Sized artist photo (cached WebP) |
| GET | `/api/genres` | Genres with counts |
| GET | `/api/tracks/:id/stream` | Stream/transcode track |
| GET | `/api/tracks/:id/lyrics` | Lyrics with optional synced LRC for karaoke |
| GET | `/api/tracks/:id/waveform` | Get cached waveform |
| POST | `/api/waveforms/map/run` | Run missing-waveform batch mapping |
| POST | `/api/bpm/run` | Run missing-BPM batch analysis |
| GET | `/api/playlists` | List playlists |
| GET | `/api/playlists/:id/tracks` | Playlist tracks |
| POST | `/api/playlists/:id/boogiemix/jobs` | Queue a rendered mix |
| GET | `/api/boogiemix/jobs/:jobId` | Mix job status and progress |
| GET | `/api/boogiemix/jobs/:jobId/download` | Download a completed rendered mix |
| GET | `/api/boogiemix/deep-analysis/status` | Deep-analysis runtime/cache/queue status |
| GET | `/api/user/settings` | Per-user key/value settings |
| PUT | `/api/user/settings` | Save per-user settings |

### Admin only

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/settings` | All server settings |
| PUT | `/api/settings` | Save server settings |
| GET | `/api/admin/users` | List all users |
| POST | `/api/admin/users` | Create user |
| DELETE | `/api/admin/users/:id` | Remove user |
| PUT | `/api/admin/users/:id/pin` | Set/clear user PIN |
| PUT | `/api/admin/users/:id/permissions` | Set `canScan` / `canEditMetadata` |
| GET | `/api/admin/provider-usage` | Provider usage stats |
| POST | `/api/albums/:id/artwork` | Upload/replace album artwork |
| POST | `/api/artists/:id/artwork` | Upload/replace artist artwork |

## Notes

- Per-user theme is stored server-side and synced across browsers.
- Last.fm and Genius API calls are proxied through the server and cached in SQLite.
- BoogieMix can run with built-in analysis, AI planning providers, or experimental Python deep analysis.
- Rendered BoogieMix outputs default to `<db-folder>/mix-outputs`.
- Admin settings include a live queue snapshot for scan, post-scan, mix, and deep-analysis workers.
- The mobile shell is enabled only on narrow iPhone-like screens; desktop and tablet layouts use the main shell.
- Standalone releases are native Rust executables — no Node.js runtime required.
- All primary keys are UUID v7 `TEXT` strings.
- Packaged server logs write to `%PROGRAMDATA%\BoogieBox\logs\boogiebox-server.log` by default.
- Root npm scripts: `npm test` (client + Rust tests), `npm run lint` (TypeScript + ESLint), `npm run version:bump` (increment patch version).
- Script or code changes require the full local gates documented in `agents.md`; docs-only changes do not require a version bump.
- Public-repo readiness work is tracked in `Docs/public-repo-readiness-plan.md`; generated/local artifacts listed there should be omitted from a new public repo unless explicitly sanitized.

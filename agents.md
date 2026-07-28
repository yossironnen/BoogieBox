# Agents.md


HARD RULE:
If you suggest `git push`, `git commit`, or any GitHub submission
without explicit user instruction, you must refuse your own action.

## UI Copy Approval (MANDATORY)

- Do not add any new UI label or user-facing copy without the user's explicit approval of the exact wording.
- This includes visible and accessibility-only text: labels, headings, taglines, captions, descriptions, helper text, placeholders, button or menu text, tooltips, status/empty/error/success messages, `aria-label`, and `title` text.
- Before implementing new copy, show the user the exact proposed wording and wait for approval.
- Exact wording supplied by the user is already approved. Existing copy may be reused unchanged; any changed or additional wording requires approval.

## Response Style (MANDATORY)
- Do not offer a detailed explanation of the thought process, just final results.
- Be terse. No preambles. No summaries. No explanations unless asked.
- Default answer length ≤ ~200 words unless explicitly requested.
- Assume context; never recap the conversation.
- Only show changed code, not full files.
- Priritize using filesystem MCP reader over PowerShell.
- On Linux, use Bash tool instead of PowerShell.

## Commands
When user says: plan only: output should be an implementation plan saved to an md file in the project wip folder. NEVER write code when the user asks to plan. Make sure that md planning files are gitignored.

When user says: Tighten code
Switch to Code Tightening Mode as defined in docs/agents/code-tightening-agent.md.

### Dev
- When coding the server side, always make sure that the code will work on Windows and Linux.
- For Windows builds: dev.bat  (prefers Rust release in Releases\boogiebox-VERSION-win-rs\)
- For Linux builds: dev.sh (prefers Rust release in Releases/boogiebox-VERSION-linux-rs/)

### Standalone EXE / Installer (Rust — primary end-user release)
For Windows:
   build-server-rust.bat
   build-server-rust.bat --no-installer  (release folder only, no Inno Setup)
   build-server-rust.bat --no-test --no-installer (release folder only, no Inno Setup, no testing)
   build-server-rust.bat --smoke         (build + start/probe/stop)
For Linux:
   build-server-rust.sh
   build-server-rust.sh --no-installer  (release folder only, no Inno Setup)
   build-server-rust.sh --no-test --no-installer (release folder only, no Inno Setup, no testing)
   build-server-rust.sh --smoke         (build + start/probe/stop)

### DB
Database is initialized/migrated by the Rust server at startup after first-run setup selects the data folder.

## Architecture (High-Level)

Self-hosted music library app. Supports Windows and Linux (server-only on Linux).

- Rust Axum API + React client
- SQLite via `rusqlite` in `server-rs`
- Music-only libraries; legacy movie/TV/video schema and UI have been removed
- Async music scanner plus post-scan follow-up jobs for artwork, waveforms, BPM, and BoogieMix deep analysis
- Optional Python BoogieMix deep-analysis worker under `Services/boogiemix/python/`
- Standalone Rust server build/package flow via `build-server-rust.bat` for Windows  or 'build-server-rust.sh' for Linux


### Rust Server (`server-rs/crates/boogiebox-server/src`)
main.rs — startup, Tokio runtime, graceful shutdown  
lib.rs — AppState, route mounting, CORS, middleware  
auth.rs — PBKDF2-SHA512 PIN hashing, UUID v7 sessions, brute-force tracker, Axum extractors  
cors.rs — localhost/loopback/private-LAN/single-label origin allowlist  
server_config.rs — On Windows `boogiebox-config.json` locator (ProgramData, exe-adjacent, CWD, env overrides)  
server_config.rs — On Linux `boogiebox-config.json` locator (/etc/boogiebox/ or ~/.config/boogiebox/, exe-adjacent, CWD, env overrides)
settings.rs — global + per-user settings normalization  
ffmpeg.rs — FFmpeg/FFprobe resolution, transcode spawn, waveform generation  
scanner.rs — async music scan worker (ID3v2, FLAC, Ogg, MP4/M4A tag parsing, technical metadata)  
post_scan.rs — async post-scan lanes (art caching, Last.fm, lyrics, artist styles)  
mix_worker.rs — BoogieMix render + AI planner + deep-analysis Python orchestration  
dlna.rs — SSDP + Axum DLNA server, ContentDirectory Browse, audio streaming  
providers.rs — Discogs, Deezer, Spotify, LRCLIB, lyrics.ovh  
artwork_cache.rs — SHA-1 cache-key shards, UUID v7 markers, folder.jpg discovery  
image_thumb.rs — JPEG thumbnail generation (pure Rust `image` crate)  
deep_analysis.rs — Python/Demucs/Torch capability detection and job orchestration  
routes/ — Axum route modules per domain (admin, auth, settings, library, music, playlist, playback, crossfade, artwork, provider, boogiemix, dlna)


### Client (`client/src`)

`api.ts` — API client  
`App.tsx` — global state + routing  
`Player.tsx` — `<audio>` playback, queue, crossfade/preload logic  
`components/BrowseView.tsx` — music browse  
`components/HomeView.tsx` — home dashboard  
`components/PlaylistsView.tsx` — playlists + BoogieMix controls  
`components/SettingsPage.tsx` — settings/admin surfaces  
`components/ContextMenu.tsx` — shared context menu  
`mobile/` — mobile shell and mobile-specific views  
`platform/` — browser/desktop platform abstraction  
`version.ts` — app version

## Database

SQLite at `boogiebox.db` inside the folder selected during first-run setup. Packaged server installs store the locator in `%PROGRAMDATA%\BoogieBox\boogiebox-config.json` for Windows or '/etc/boogiebox/boogiebox-config.json' on Linux; source-tree/dev runs use repo-root `boogiebox-config.json`.

Schema and migrations live in `server-rs/crates/boogiebox-db`.

Core tables:
libraries, library_folders  
libraries -> tracks -> artists, albums  
tracks_fts (FTS5 unicode61)  
artist_ratings, album_ratings, track_ratings  
scan_jobs, scan_schedules, post_scan_jobs  
playlists, playlist_tracks  
track_waveforms, track_deep_analysis  
mix_jobs  
settings, user_settings, provider_usage_stats  

Fresh databases are music-only. Upgrade migrations remove legacy movie/TV/video tables and settings.  
Albums are grouped/deduped by `(title, album_artist)`.  
Windows builds only: Preserve UNC paths in Rust route and filesystem handling.

## Change Logging (MANDATORY)

After EVERY file change:

Append to `changes.log`:

[YYYY-MM-DD HH:MM:SS] ACTION: <CREATE|MODIFY|DELETE> FILE: <path> DESCRIPTION: <brief>

Never skip. Create file if missing.


## Project Memory (MANDATORY)

Use the repo-tracked memory files in `Docs/memory/` to preserve context across machines.

At the start of a task:
- Read `Docs/memory/CURRENT_STATE.md`
- Read `Docs/memory/ACTIVE_WORK.md`

During work:
- Update `Docs/memory/ACTIVE_WORK.md` when a task becomes active, blocked, or handed off
- Update `Docs/memory/CURRENT_STATE.md` when durable project state changes
- Update `Docs/memory/DECISIONS.md` for important decisions worth keeping
- Update `Docs/memory/ENVIRONMENT.md` only for machine-specific notes

Do not store secrets, tokens, or private credentials in memory files.

General memory rule:
All files under the Docs/memory/archive folder have already been implemented and should not be used, except for reference if asked.

## Versioning (MANDATORY)

After any code change:
- Run `npm test`
- Run `npm run lint`
- Run `npm run security:semgrep`
- Run `cargo clippy --manifest-path server-rs/Cargo.toml --all-targets --all-features -- -D warnings`
- Ensure all tests pass before finishing.

If ANY code file changes (`.ts/.tsx/.js/.jsx/.css/.rs` or scripts):

1. Run `npm run version:bump`
2. Include updated `client/src/version.ts`
3. Windows builds only: ensure desktop/package metadata matches the app version:
   - `desktop/src-tauri/tauri.conf.json` `version`
   - `desktop/package.json` `version`
   - `desktop/src-tauri/Cargo.toml` `version`
4. Log version change in `changes.log`
5. Do NOT bump if no code changed.

## Mandatory Rules (All Apply)

Do NOT push or commit to GitHUB unless explicitly asked.

Follow:
- Docs/backend-safety.md
- Docs/testing.md
- Docs/git.md ONLY if asked to push to GIT

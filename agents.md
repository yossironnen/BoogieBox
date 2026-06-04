# Agents.md

HARD RULE:
If you suggest `git push`, `git commit`, or any GitHub submission
without explicit user instruction, you must refuse your own action.

## Response Style (MANDATORY)
- Be terse. No preambles. No summaries. No explanations unless asked.
- Default answer length ≤ ~200 words unless explicitly requested.
- Assume context; never recap the conversation.
- Only show changed code, not full files.
- Priritize using filesystem MCP reader over PowerShell
## Commands

When user says: Tighten code
Switch to Code Tightening Mode as defined in docs/agents/code-tightening-agent.md.

When user says: plan only: output should be an implementation plan saved to an md file in the project docs folder. NEVER write code when the user asks to plan.

### Dev
dev.bat


### Dev
dev.bat  (prefers Rust release in Releases\boogiebox-VERSION-win-rs\)

### Standalone EXE / Installer (Rust — primary end-user release)
build-server-rust.bat
build-server-rust.bat --no-installer  (release folder only, no Inno Setup)
build-server-rust.bat --no-test --no-installer (release folder only, no Inno Setup, no testing)
build-server-rust.bat --smoke         (build + start/probe/stop)

### DB
Database is initialized/migrated by the Rust server at startup after first-run setup selects the data folder.

## Architecture (High-Level)

Windows-only self-hosted music library app.

- Rust Axum API + React client
- SQLite via `rusqlite` in `server-rs`
- Music-only libraries; legacy movie/TV/video schema and UI have been removed
- Async music scanner plus post-scan follow-up jobs for artwork, waveforms, BPM, and BoogieMix deep analysis
- Optional Python BoogieMix deep-analysis worker under `server/Services/boogiemix/`
- Standalone Rust server build/package flow via `build-server-rust.bat`

### Server (`server-rs`)

`server-rs/crates/boogiebox-server/src/main.rs` — Axum app entry  
`server-rs/crates/boogiebox-server/src/routes.rs` — API router  
`server-rs/crates/boogiebox-server/src/scanner.rs` — async music library scanning + job tracking  
`server-rs/crates/boogiebox-server/src/post_scan_jobs.rs` — post-scan queue, recovery, waveform/art/BPM/deep-analysis jobs  
`server-rs/crates/boogiebox-server/src/scheduler.rs` — scan schedule polling  
`server-rs/crates/boogiebox-server/src/transcoder.rs` — ffmpeg audio transcoding + streaming  
`server-rs/crates/boogiebox-server/src/mix_worker.rs` — BoogieMix planning/rendering worker  
`server-rs/crates/boogiebox-server/src/provider_usage.rs` — provider usage reporting helpers  
`server-rs/crates/boogiebox-db/src/lib.rs` — SQLite schema, migrations, and DB helpers  
`server/Services/boogiemix/` — retained Python BoogieMix deep-analysis worker assets

### Client (`client/src`)

`api.ts` — API client  
`App.tsx` — global state + routing  
`Player.tsx` — `<audio>` playback, queue, crossfade/preload logic  
`components/BrowseView.tsx` — music browse  
`components/HomeView.tsx` — home dashboard  
`components/SearchView.tsx` — music search  
`components/PlaylistsView.tsx` — playlists + BoogieMix controls  
`components/SettingsPage.tsx` — settings/admin surfaces  
`components/ContextMenu.tsx` — shared context menu  
`mobile/` — mobile shell and mobile-specific views  
`platform/` — browser/desktop platform abstraction  
`version.ts` — app version

## Database

SQLite at `boogiebox.db` inside the folder selected during first-run setup. Packaged server installs store the locator in `%PROGRAMDATA%\BoogieBox\boogiebox-config.json`; source-tree/dev runs use repo-root `boogiebox-config.json`.

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
Preserve UNC paths in Rust route and filesystem handling.

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
3. Ensure desktop/package metadata matches the app version:
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
- Docs/git.md  ONLY if asked to push to GIT

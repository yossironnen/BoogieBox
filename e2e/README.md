# UI release suite (Playwright)

End-to-end browser tests to run before publishing a release. They drive the **packaged server**
from `Releases/boogiebox-<VERSION>-win-rs` (or `-linux-rs`) against a generated music library in a
throwaway data folder, so they test what ships.

## One-time setup

```
npm install
npm run e2e:install        # downloads Playwright's Chromium
```

## Before a release

One command builds the release and runs the suite against it:

```
build-server-rust.bat --e2e                         # or ./build-server-rust.sh --e2e on Linux
```

`--e2e` combines with the other flags (e.g. `--no-test --no-installer --e2e`), installs Chromium if needed and
makes the build exit non-zero when a test fails. To run the suite on its own against an existing build:

```
npm run e2e
npm run e2e:report                                  # HTML report, traces and videos of failures
```

The suite itself takes about 2 minutes. Exit code is non-zero if anything fails. `npm run e2e` first checks that
Playwright and its Chromium browser are installed and prints the install command if not.

| Command | What it does |
| --- | --- |
| `npm run e2e` | Full suite, headless |
| `npm run e2e -- tests/desktop/playback.spec.ts` | One file |
| `npm run e2e -- -g "playlist"` | Tests whose name matches |
| `npm run e2e:headed` | Watch it run in a browser window |
| `npm run e2e:ui` | Playwright UI mode (step through, time-travel) |
| `npm run e2e:serve` | Start the sandbox server + library and leave it running (http://127.0.0.1:3917) |
| `npm run e2e:typecheck` | Type-check the suite |

## What a run does

1. Picks the server: `Releases/boogiebox-<VERSION>-*-rs`. If that folder is missing it falls back to the newest
   release folder and prints a warning. Set `BOOGIEBOX_E2E_STRICT_VERSION=1` to fail instead (use this for real
   release checks).
2. Wipes `tmp/e2e-run/` and copies in a fixture library of 10 short sine-tone tracks: MP3, FLAC and M4A, 3 artists,
   4 albums, `folder.jpg` covers and one album with only embedded covers. The audio is generated with FFmpeg once
   and cached in `tmp/e2e-fixtures/`.
3. Starts the server on port 3917. Every place it could read a config locator or data folder from
   (`BOOGIEBOX_CONFIG_PATH`, `PROGRAMDATA`, `LOCALAPPDATA`, `HOME`, the working directory) points into `tmp/e2e-run/`,
   so a run never touches a real database. It refuses to start if a `boogiebox-config.json` sits next to the exe.
4. **setup** project: first-run setup through the UI, admin sign-in, add the library, scan.
5. **desktop** (1440×900) and **mobile** (Pixel 7) projects reuse that signed-in session.
6. Stops the server.

Every test also fails on uncaught page errors, app `console.error` output and any HTTP 5xx response.

## Coverage

| Area | Spec |
| --- | --- |
| First-run setup, library add + scan | `tests/first-run.setup.ts` |
| Every view and Settings tab renders, version, sidebar | `tests/desktop/shell.spec.ts` |
| Browse artists/albums, drill-down, per-format tags, artwork, genres, ratings, scan safety (offline folder, mass-removal brake, Remove missing tracks) | `tests/desktop/library.spec.ts` |
| Real audio playback per format, transport, queue, seek, auto-advance, OS media controls (MediaSession), sleep timer (minutes fade/pause with `page.clock`, end of track, cancel) | `tests/desktop/playback.spec.ts` |
| Keyboard shortcuts (transport, volume, mute, `/` search, `?` help) and when they stay quiet | `tests/desktop/shortcuts.spec.ts` |
| Playlist create / add / rename / play / delete, M3U import (preview tiles, not-found download, file order) | `tests/desktop/playlists.spec.ts` |
| Search by title/artist/album, genre filter | `tests/desktop/search.spec.ts` |
| PIN users, wrong PIN, non-admin UI, sign-out, delete | `tests/desktop/users.spec.ts` |
| Favourites, radio stations, themes, Subsonic switch, Activity, Home, Auto DJ | `tests/desktop/features.spec.ts` |
| Mobile shell, browse, search, Now Playing, OS media controls, Sleep tool, M3U import sheet | `tests/mobile/mobile.spec.ts` |
| Backup: back up now (with an uploaded cover), schedule/keep, delete | `tests/desktop/backup.spec.ts` |
| Restore (project `restore`, runs last because it signs everyone out): live restore, first-run restore on a second fresh server with a remapped music folder | `tests/restore/restore.spec.ts` |

Not covered (needs hardware or network): Play On renderers, DLNA, BoogieMix rendering/deep analysis, Last.fm,
ListenBrainz and provider lookups, the desktop (Tauri) shell.

## Options

| Variable | Default | Purpose |
| --- | --- | --- |
| `BOOGIEBOX_E2E_SERVER_EXE` | release for `VERSION` | Test a specific `boogiebox-server(.exe)` |
| `BOOGIEBOX_E2E_STRICT_VERSION` | off | `1` = fail when the `VERSION` build is missing |
| `BOOGIEBOX_E2E_PORT` | `3917` | Server port |
| `BOOGIEBOX_E2E_PORT2` | `3918` | Port of the second, fresh server the first-run restore test starts |
| `BOOGIEBOX_PRUNE_BRAKE_MIN` | `2` | Passed to the server: lowers the scan's mass-removal brake floor (normally 200 tracks) so the fixture library can trip it |

## Writing tests

- Import `test`, `expect` and helpers from `support/test.ts`, not from `@playwright/test`.
- Prefer roles and accessible names (`getByRole('button', { name: 'Play All' })`); the app labels most controls.
- Tests share one server and run serially. Give anything you create a `uniqueName()` and delete it at the end.
- Never log the stored admin session out; sign other users in with `browser.newContext()` (see `users.spec.ts`).

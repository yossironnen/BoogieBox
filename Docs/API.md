# API

BoogieBox exposes a JSON REST API under `/api`. The React client uses `client/src/api.ts` as the main typed wrapper around these endpoints.

## Conventions

- Authenticated requests use the browser session cookie and `credentials: include`.
- Most successful JSON responses use camelCase fields.
- Error responses generally include an `error` field.
- Streaming and artwork endpoints may return binary responses instead of JSON.
- API routes are implemented in `server-rs/crates/boogiebox-server/src/routes/`.

## Setup And System

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/system/status` | Report setup state, FFmpeg status, log path, and discovery metadata. |
| `POST` | `/api/system/setup` | Complete first-run setup by selecting the database folder. |
| `POST` | `/api/system/select-folder` | Open the native folder picker during local first-run setup. |

## Authentication

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/auth/me` | Return the current authenticated user. |
| `POST` | `/api/auth/login` | Log in as a user. |
| `POST` | `/api/auth/logout` | End the current session. |

## Libraries And Scanning

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/libraries` | List configured music libraries. |
| `POST` | `/api/libraries` | Create a music library. |
| `PUT` | `/api/libraries/{id}` | Update library settings. |
| `DELETE` | `/api/libraries/{id}` | Remove a library. |
| `POST` | `/api/libraries/{id}/scan` | Start a scan for one library. |
| `GET` | `/api/scan-jobs/active` | List active scan jobs and status. |

## Music Browse And Search

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/search` | Search tracks, artists, albums, and playlists. |
| `GET` | `/api/artists` | Browse artists. |
| `GET` | `/api/artists/{id}` | Fetch artist detail. |
| `GET` | `/api/albums` | Browse albums. |
| `GET` | `/api/albums/{id}` | Fetch album detail. |
| `GET` | `/api/albums/latest` | Fetch recently added albums. |
| `GET` | `/api/tracks/{id}` | Fetch track detail. |
| `GET` | `/api/genres` | List library genres. |
| `GET` | `/api/stats` | Fetch library statistics. |

## Playback

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/tracks/{id}/stream` | Stream or transcode a track. |
| `GET` | `/api/tracks/{id}/waveform` | Fetch cached waveform data. |
| `GET` | `/api/tracks/{id}/lyrics` | Fetch cached or provider lyrics. |
| `GET` | `/api/tracks/{id}/sonic-fingerprint` | Fetch Sonic Fingerprint (stem analysis, sections, transition windows) for a track. |
| `POST` | `/api/tracks/{id}/played` | Record a track play event. |
| `GET` | `/api/user/history` | Fetch listening history for the current user. |

## Playlists

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/playlists` | List playlists. |
| `POST` | `/api/playlists` | Create a playlist. |
| `GET` | `/api/playlists/{id}` | Fetch playlist detail and tracks. |
| `PUT` | `/api/playlists/{id}` | Update playlist metadata. |
| `DELETE` | `/api/playlists/{id}` | Delete a playlist. |
| `POST` | `/api/playlists/{id}/tracks` | Add tracks to a playlist. |
| `PUT` | `/api/playlists/{id}/tracks/order` | Reorder playlist tracks. |

## Ratings And Metadata

| Method | Path | Purpose |
| --- | --- | --- |
| `PATCH` | `/api/artists/{id}/rating` | Set an artist rating. |
| `PATCH` | `/api/albums/{id}/rating` | Set an album rating. |
| `PATCH` | `/api/tracks/{id}/rating` | Set a track rating. |
| `GET` | `/api/integrations/metadata-search` | Search metadata providers. |
| `PUT` | `/api/albums/{id}/metadata` | Apply metadata to an album. |
| `PUT` | `/api/artists/{id}/metadata` | Apply metadata to an artist. |

## Artwork

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/albums/{id}/cover` | Serve cached album cover thumbnail. |
| `GET` | `/api/albums/{id}/art` | Serve full-size album artwork. |
| `POST` | `/api/albums/{id}/artwork` | Upload or assign album artwork. |
| `GET` | `/api/artists/{id}/photo` | Serve artist photo. |
| `POST` | `/api/artists/{id}/artwork` | Upload or assign artist artwork. |

## Analysis Jobs

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/waveforms/map/run` | Generate missing waveform rows. |
| `GET` | `/api/waveforms/map/status` | Report waveform mapping status. |
| `POST` | `/api/bpm/run` | Run BPM analysis for missing BPM values. |
| `GET` | `/api/bpm/status` | Report BPM analysis status. |

## BoogieMix

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/boogiemix/create` | Create a BoogieMix job (generic). |
| `POST` | `/api/playlists/{id}/boogiemix/jobs` | Create a BoogieMix job for a specific playlist. |
| `GET` | `/api/boogiemix/jobs/{jobId}` | Fetch BoogieMix job status, transitions, and logs. |
| `POST` | `/api/boogiemix/jobs/{jobId}/cancel` | Cancel a running or pending BoogieMix job. |
| `GET` | `/api/playlists/{id}/boogiemix/outputs` | List rendered mix outputs for a playlist. |
| `GET` | `/api/boogiemix/outputs/{outputId}/file` | Download a rendered mix file. |
| `GET` | `/api/boogiemix/deep-analysis/status` | Report deep-analysis runtime health, queue, and cache state. |
| `POST` | `/api/boogiemix/deep-analysis/playlists/{playlistId}/queue` | Queue deep analysis for all tracks in a playlist. |
| `GET` | `/api/boogiemix/deep-analysis/playlists/{playlistId}/progress` | Fetch deep-analysis progress for a playlist. |
| `POST` | `/api/boogiemix/deep-analysis/libraries/{libraryId}/queue` | Queue deep analysis for all tracks in a library. |
| `POST` | `/api/boogiemix/deep-analysis/pause` | Pause background deep-analysis jobs. |
| `POST` | `/api/boogiemix/deep-analysis/resume` | Resume paused background deep-analysis jobs. |
| `POST` | `/api/boogiemix/deep-analysis/cache/clear` | Clear the deep-analysis cache. |

## Admin, Settings, And Providers

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/settings` | Read global settings. |
| `PUT` | `/api/settings` | Update global settings. |
| `GET` | `/api/user/settings` | Read current user settings. |
| `PUT` | `/api/user/settings` | Update current user settings. |
| `GET` | `/api/admin/users` | List all users (admin only). |
| `POST` | `/api/admin/users` | Create a user (admin only). |
| `PUT` | `/api/admin/users/{id}/permissions` | Update user scan/edit permissions (admin only). |
| `PUT` | `/api/admin/users/{id}/pin` | Set or clear a user PIN (admin only). |
| `DELETE` | `/api/admin/users/{id}` | Delete a user (admin only). |
| `GET` | `/api/admin/queues` | Fetch scan, post-scan, mix, and analysis queue snapshots. |
| `POST` | `/api/admin/browse-folder` | Browse the server filesystem for folder selection. |
| `GET` | `/api/admin/fs/browse` | List directory contents on the server filesystem. |
| `GET` | `/api/admin/provider-usage` | Fetch provider usage statistics. |
| `GET` | `/api/lastfm/info` | Fetch Last.fm artist information. |
| `GET` | `/api/lastfm/top-tracks` | Fetch Last.fm top-track suggestions for an artist. |

## Implementation Notes

Use the route modules as the source of truth when adding or changing endpoints. Keep `client/src/api.ts` and `client/src/types/` aligned with response payload changes.

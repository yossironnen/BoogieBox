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
| `POST` | `/api/setup` | Complete first-run setup by selecting the database folder. |
| `POST` | `/api/setup/select-folder` | Open the Windows folder picker during local first-run setup. |

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
| `GET` | `/api/scan-jobs` | List scan jobs and status. |

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
| `POST` | `/api/playback/history` | Record playback history. |
| `GET` | `/api/history` | Fetch listening history. |

## Playlists

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/playlists` | List playlists. |
| `POST` | `/api/playlists` | Create a playlist. |
| `GET` | `/api/playlists/{id}` | Fetch playlist detail and tracks. |
| `PUT` | `/api/playlists/{id}` | Update playlist metadata. |
| `DELETE` | `/api/playlists/{id}` | Delete a playlist. |
| `POST` | `/api/playlists/{id}/tracks` | Add tracks to a playlist. |
| `PUT` | `/api/playlists/{id}/tracks/reorder` | Reorder playlist tracks. |

## Ratings And Metadata

| Method | Path | Purpose |
| --- | --- | --- |
| `PUT` | `/api/artists/{id}/rating` | Set an artist rating. |
| `PUT` | `/api/albums/{id}/rating` | Set an album rating. |
| `PUT` | `/api/tracks/{id}/rating` | Set a track rating. |
| `POST` | `/api/metadata/search` | Search metadata providers. |
| `POST` | `/api/metadata/apply` | Apply selected metadata. |

## Artwork

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/artwork/album/{id}` | Serve album artwork. |
| `GET` | `/api/artwork/artist/{id}` | Serve artist artwork. |
| `POST` | `/api/artwork/cache` | Queue or refresh artwork cache work. |

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
| `POST` | `/api/boogiemix/jobs` | Create a BoogieMix job. |
| `GET` | `/api/boogiemix/jobs/{id}` | Fetch BoogieMix job status. |
| `GET` | `/api/boogiemix/outputs/{id}/download` | Download a rendered mix. |
| `GET` | `/api/boogiemix/deep-analysis/status` | Report optional deep-analysis health and queue state. |

## Admin, Settings, And Providers

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/settings` | Read global settings. |
| `PUT` | `/api/settings` | Update global settings. |
| `GET` | `/api/user-settings` | Read current user settings. |
| `PUT` | `/api/user-settings` | Update current user settings. |
| `GET` | `/api/admin/queues` | Fetch scan, post-scan, mix, and analysis queue snapshots. |
| `GET` | `/api/provider-usage` | Fetch provider usage statistics. |
| `GET` | `/api/lastfm/info` | Fetch Last.fm artist information. |
| `GET` | `/api/lastfm/top-tracks` | Fetch Last.fm top-track suggestions for an artist. |

## Implementation Notes

Use the route modules as the source of truth when adding or changing endpoints. Keep `client/src/api.ts` and `client/src/types/` aligned with response payload changes.

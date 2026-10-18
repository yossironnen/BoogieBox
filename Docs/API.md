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
| `GET` | `/api/artists/{id}/similar` | Similar artists owned locally (Last.fm, Deezer, ListenBrainz). |
| `GET` | `/api/artists/{id}/radio` | Build an Artist Radio queue. Query: `limit` (10-300, default 100), `focus` (`similar`\|`mood`), `moods` (comma-separated: `chill,melancholic,uplifting,energetic,dark,dreamy,romantic,aggressive`), `variety` (0-1). Returns `artist`, `tags`, `moods`, `mix`, `coverage`, `degraded` (non-error notice when data is thin) and `tracks`, each with a `radio_reason` (`seed`\|`similar`\|`mood`\|`style`). Never 404s for a known artist. |
| `GET` | `/api/artists/{id}/radio/options` | What the radio options popover offers: seed `tags`, `autoMoods`, per-mood availability and library-wide tagging progress. |
| `GET` | `/api/tracks/{id}` | Fetch track detail. |
| `GET` | `/api/genres` | List library genres. |
| `GET` | `/api/stats` | Fetch library statistics. |

## Playback

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/tracks/{id}/stream` | Stream or transcode a track (requires a session). |
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

## Play On (UPnP/DLNA Renderers)

All routes require a session and return `404` while the `remotePlaybackEnabled` setting is off. State-changing routes are rate-limited per user (`429`). A command on a renderer whose session another user is actively holding returns `409` with `code: "session_in_use"`, `ownerName`, `deviceName`, `status` and `nowPlaying`. Resend it with `takeover: true` (query `takeover=true` for `DELETE`) after the user confirms. Sessions are saved (`remote_sessions`) and come back after a server restart once the renderer is found again: still playing if the speaker kept our stream, otherwise paused at the saved position.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/remote/renderers` | List known renderers, with online state, capabilities and a live-session summary (`heldByOther`). Hidden renderers are returned to admins only. |
| `POST` | `/api/remote/renderers/refresh` | Trigger an immediate network search. |
| `PATCH` | `/api/remote/renderers/{udn}` | Admin: set `alias` (`null` clears), `formatPolicy` (`auto`/`direct`/`lossless`/`mp3`) and `hidden`. |
| `GET` | `/api/remote/renderers/{udn}/icon` | Renderer icon proxied from the device. |
| `GET` | `/api/remote/diagnostics` | Admin: discovery state (`running`, `detectedInterfaces`, `searchInterfaces`, `lastSearchMs`, `lastError`, `httpPort`) and renderer/online/session counts. Works while Play On is off. |
| `GET` | `/api/remote/sessions/{udn}` | Current session snapshot (`404` when idle). |
| `POST` | `/api/remote/sessions/{udn}/load` | Replace the queue and play: `items: [{kind: "track"|"mix", id, radioReason?}]`, `startIndex`, `startAtSec`, `queueSource`, `shuffle`, `repeat`, `resumed` (handoff: the start item's play was already recorded, so it is not counted again). |
| `POST` | `/api/remote/sessions/{udn}/command` | `cmd`: `play`, `pause`, `toggle`, `stop`, `next`, `prev`, `seek` (value s), `jump` (value qid), `shuffle`/`mute` (bool), `repeat` (`off`/`one`/`all`), `volume` (0–100). |
| `POST` | `/api/remote/sessions/{udn}/queue` | `op`: `enqueue` (`items`, `mode` `end`/`next`), `remove` (`qid`), `move` (`qid`, `toIndex`), `clear`. |
| `POST` | `/api/remote/sessions/{udn}/transfer` | Move queue and position to `toUdn` (target loads first; the source is then stopped). |
| `DELETE` | `/api/remote/sessions/{udn}` | End the session; `stop` defaults to the `remoteReleaseStopsRenderer` setting. |
| `GET` | `/api/remote/events` | Server-Sent Events: `renderers`, `session` (full snapshot), `position`, `takeover`, `notice`, `sessionEnded`. |
| `GET`/`HEAD` | `/media/r/{token}` | Unauthenticated media for renderers. The token is random, short-lived and scoped to one track, mix or cover. |

## Subsonic Apps (OpenSubsonic `/rest`)

Third-party Subsonic apps use `GET`/`POST` `/rest/{method}` (`.view` suffix optional; POST takes a form body).
Off until an admin sets `subsonicApiEnabled=true`. Apps sign in with `u` plus `p` (plain or `enc:<hex>`) or
`t`+`s` (`t = md5(password + salt)`), using the user's **Subsonic password**, never the PIN. API keys
(`apiKey`) are not supported yet (error 42). Responses are XML by default, `f=json`, or `f=jsonp&callback=`;
failures are HTTP 200 with a Subsonic `error` (0 generic/unsupported, 10 missing parameter, 40 wrong
credentials or locked out after 5 failures for a minute, 43 conflicting mechanisms, 50 not allowed, 70 not
found). `/rest` CORS allows any origin without credentials. Responses never include filesystem paths: `path`
is a virtual `Artist/Album/NN - Title.ext`.

Supported: `ping`, `getLicense`, `getOpenSubsonicExtensions` (no sign-in needed; lists `formPost` and
`songLyrics`), `getUser`, `getMusicFolders` (folder ids are 1-based positions of the libraries), `getIndexes`,
`getArtists`, `getArtist`, `getAlbum`, `getSong`, `getGenres`, `getMusicDirectory`, `getAlbumList`/`getAlbumList2`
(`random`, `newest`, `highest`, `frequent`, `recent`, `alphabeticalByName`, `alphabeticalByArtist`, `starred`,
`byYear`, `byGenre`), `getRandomSongs`, `getSongsByGenre`, `getStarred`/`getStarred2`, `star`/`unstar`
(`id` resolves song → album → artist; `albumId`, `artistId`), `setRating` (whole stars 1–5, 0 clears),
`search2`/`search3` (an empty query pages through everything, which is how apps sync), `getPlaylists`,
`getPlaylist`, `createPlaylist` (a taken name gets " (2)"; with `playlistId` it replaces the entries; returns the
playlist), `updatePlaylist` (`name`, `comment`, `songIndexToRemove…` then `songIdToAdd…`; `public` ignored),
`deletePlaylist`, `getLyrics` (cache only), `getLyricsBySongId` (cache, then LRCLIB/lyrics.ovh within 5 s;
synced lines in ms), `getArtistInfo`/`getArtistInfo2` (local bio, else the cached Last.fm summary; similar
artists in the library), `getAlbumInfo`/`getAlbumInfo2`, `getSimilarSongs`/`getSimilarSongs2` (an Artist Radio
queue seeded by the song's, album's or artist's artist; provider lookups capped at 2 s), `getTopSongs` (cached
Last.fm top tracks first, then local plays), `getNowPlaying` (what apps reported with `scrobble
submission=false` in the last 10 minutes, all users), `scrobble` (`submission=true` counts a play at `time`,
ignoring repeats within 30 s), `stream`, `download` (original file) and `getCoverArt` (`al-`/`ar-`/`tr-`/`pl-`/`mx-`
ids; 300 or 800 px).

Playlists: a track appears at most once per BoogieBox playlist, so repeated song ids are dropped. Each user also
sees a read-only **BoogieMix** playlist (`bm-mixes`, `readonly=true`) listing their rendered mixes newest first,
each as one long song `mx-{outputId}` (album and artist "BoogieMix"). Mixes stream and download as the original
file from the BoogieMix output folder only, are never counted as plays, can't be starred or rated, and the
playlist can't be edited or deleted (error 50). Transitions and Mix Story chapters are not shown.

`stream` serves the original file unless the app asks for `format=mp3`, the file needs conversion to play, or
`maxBitRate` is below the file's bitrate; MP3 bitrates are 128/192/256/320 and at most `subsonicMaxTranscodes`
conversions run at once (HTTP 503 after a 10 s wait). Files outside their library folders are never served.
Other spec methods return error 0 "Not supported by BoogieBox". Half-star ratings are shown rounded up to whole
stars.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/subsonic/credentials` | Current user: `enabled`, `hasPassword`, `updatedAt`, `lastUsedAt`, `lastClient`, `serverUrls` (never the password). |
| `PUT` | `/api/subsonic/credentials/password` | Current user: set or change the Subsonic password (`{ password }`). Rules: 8–128 characters, at least one symbol, no control characters, not the PIN. A violation returns `400` with `error` and `rule` (`too_short`, `too_long`, `needs_symbol`, `invalid_characters`, `same_as_pin`). |
| `DELETE` | `/api/subsonic/credentials/password` | Current user: remove the Subsonic password. |
| `PUT` | `/api/admin/users/{id}/subsonic-password` | Admin: set another user's Subsonic password (same rules). |
| `DELETE` | `/api/admin/users/{id}/subsonic-password` | Admin: remove another user's Subsonic password. |

Settings (global, admin): `subsonicApiEnabled` (`true`/`false`, default `false`), `subsonicMaxTranscodes`
(1–8, default 3), `subsonicIgnoredArticles` (up to 20 words, default `The El La Los Las Le Les`). Subsonic
passwords are stored AES-256-GCM encrypted with the key file `subsonic.key` next to `boogiebox.db`; when moving
the database by hand, move `subsonic.key` with it or users must set their Subsonic passwords again. Apps send
the password (or its hash) in the URL, so use HTTPS (reverse proxy or VPN) beyond the home network.

## Admin, Settings, And Providers

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/settings` | Read global settings. |
| `PUT` | `/api/settings` | Update global settings. |
| `GET` | `/api/user/settings` | Read current user settings. |
| `PUT` | `/api/user/settings` | Update current user settings. |
| `GET` | `/api/admin/users` | List all users (admin only), including `hasPin` and `hasSubsonicPassword`. |
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
| `GET` | `/api/radio/metadata/status` | Artist Radio metadata collection mode, provider availability and progress (`tracksChecked`/`tracksTotal`, `artistsTagged`/`artistsTotal`). |

Artist Radio settings (global, admin): `radioTrackTagSync` (`off` | `lazy` | `full`, default `full`) controls
Last.fm track-tag collection; `radioKeylessProviders` (`true`/`false`, default `true`) enables MusicBrainz and
ListenBrainz (no key needed).

## Implementation Notes

Use the route modules as the source of truth when adding or changing endpoints. Keep `client/src/api.ts` and `client/src/types/` aligned with response payload changes.

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
| `POST` | `/api/system/backups/inspect` | First-run only: validate a backup folder (`{path}`) and summarize it. |
| `POST` | `/api/system/setup-restore` | First-run only: restore a backup into an empty database folder (`{backupPath, dbFolder, remaps}`). |

## Backups (Admin)

While a restore swaps the database, every `/api` and `/rest` call except `/api/system/status` answers
`503 {code: "restoring"}`; the status reports `restoring` and `restoreStep`.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/admin/backups` | Settings, running job, last failure, next run, staleness and the list of backups. |
| `PUT` | `/api/admin/backups/settings` | Save `{schedule, day, time, keep, path, includeAllArtwork, includeMixes}`. |
| `POST` | `/api/admin/backups` | Back up now (202; one backup or restore at a time, 409 otherwise). |
| `GET` | `/api/admin/backups/health` | `ok`, `failed`, `stale` or `off`, for the sidebar warning. |
| `POST` | `/api/admin/backups/inspect` | Validate a backup folder (`{path}`) and summarize it. |
| `POST` | `/api/admin/backups/{name}/restore` | Restore a listed backup (`{remaps}`); signs everyone out. 202 `pending` if it must finish at the next start. |
| `POST` | `/api/admin/backups/restore-from` | Restore a backup from another folder (`{path, remaps}`). |
| `DELETE` | `/api/admin/backups/{name}` | Delete a backup. |

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
| `POST` | `/api/tracks/{id}/played` | Record a track play event (also lists it in Subsonic `getNowPlaying` as "BoogieBox" and tells the user's scrobbling services it started). |
| `POST` | `/api/tracks/{id}/scrobble` | The web player says a play counts (played half its length or 4 minutes; tracks of 30 s or less never count). Body `{ startedAt }` (Unix seconds, optional). Queued for the user's Last.fm / ListenBrainz accounts; does not change play counts. |
| `GET` | `/api/user/history` | Fetch listening history for the current user. |

Per-user play counts: every row in `play_history` also updates `user_track_stats` (database triggers), so each
user's own counts are available without scanning the history. Subsonic `playCount`/`played` use them; the web UI's
counts stay global.

## Favourites

Hearts in the web UI. They are the same per-user data as stars in Subsonic apps (`subsonic_stars`).

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/favourites/ids` | `{ tracks, albums, artists }` id lists, newest first. |
| `GET` | `/api/favourites?limit=` | Full rows `{ artists, albums, tracks }` in the usual web shapes, newest first (at most 1000 each). |
| `PUT` | `/api/favourites/{track\|album\|artist}/{id}` | Add a favourite (`404` when the item does not exist). |
| `DELETE` | `/api/favourites/{track\|album\|artist}/{id}` | Remove a favourite. |

## Internet Radio

Shared by every user; only admins edit. The same stations appear in Subsonic apps (`getInternetRadioStations`).

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/radio-stations` | List stations `{ id, name, streamUrl, homepageUrl }` by name. |
| `POST` | `/api/radio-stations` | Admin: add a station `{ name, streamUrl, homepageUrl }` (`201`). Name 1–200 characters; URLs must be http(s), at most 2048 characters. |
| `PUT` | `/api/radio-stations/{id}` | Admin: replace a station's fields. |
| `DELETE` | `/api/radio-stations/{id}` | Admin: delete a station. |
| `GET` | `/api/radio-stations/{id}/stream` | Relay the station for the web player (any signed-in user). Follows one `.pls`/`.m3u` level, refuses HLS (`415`) and non-audio responses (`502`), sends no `Content-Length` (`Accept-Ranges: none`), at most 8 relays at once (`503`). |

## Scrobbling

Each user connects their own accounts; the server sends plays from the web player, Play On speakers and Subsonic
apps. Tokens and session keys are stored AES-256-GCM encrypted with `subsonic.key` and are never returned.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/scrobble/accounts` | `{ lastfmAvailable, accounts: [{ service, accountName, needsReconnect, connectedAt, lastScrobbleAt, pending }] }`. |
| `PUT` | `/api/scrobble/listenbrainz` | Connect ListenBrainz with `{ token }` (validated with ListenBrainz first; `400` when refused, `502` when unreachable). |
| `POST` | `/api/scrobble/lastfm/connect` | Start the Last.fm sign-in: body `{ origin }` (the web UI's own origin); returns `{ url }` to open. `409` until an admin sets `lastfmKey` and `lastfmSecret`. |
| `GET` | `/api/scrobble/lastfm/callback?state&token` | Where Last.fm sends the user back. No session cookie needed: the one-time `state` (10 minutes) names the user. Returns a small HTML page. |
| `DELETE` | `/api/scrobble/{lastfm\|listenbrainz}` | Disconnect and drop queued plays. |

Delivery: plays are queued and sent in batches (Last.fm 50, ListenBrainz 100) by a background worker. Temporary
failures retry with backoff (1 minute doubling to 6 hours); plays are dropped after 14 days or 20 attempts; at most
1000 plays wait per account. A rejected token or session marks the account `needsReconnect` and keeps its plays.
"Now playing" is sent when a track starts (best effort). Play On sessions scrobble once the speaker's position
passes the same threshold as the web player.

## Playlists

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/playlists` | List playlists. |
| `POST` | `/api/playlists` | Create a playlist. With `kind: "smart"` and a `definition` (rules, sort, limit), creates and evaluates a smart playlist in one step; invalid rules answer `400` `code: "invalid_rules"` with `errors[]` (`path`, `code`). Playlist rows carry `kind` (`manual` / `smart`). |
| `GET` | `/api/playlists/{id}` | Fetch playlist detail and tracks. |
| `PUT` | `/api/playlists/{id}` | Update playlist metadata. |
| `DELETE` | `/api/playlists/{id}` | Delete a playlist. |
| `POST` | `/api/playlists/{id}/tracks` | Add tracks to a playlist. |
| `PUT` | `/api/playlists/{id}/tracks/order` | Reorder playlist tracks. |
| | | Adding, removing or reordering tracks of a smart playlist answers `409` `code: "smart_playlist"`. |
| `GET` | `/api/playlists/{id}/export.m3u` | Download a playlist as M3U (smart playlists are brought up to date first; `503` `timed_out` if that fails). |
| `POST` | `/api/playlists/import/preview` | Match an M3U/M3U8 file (`{ contentBase64, remaps? }`, ≤ 2 MB, ≤ 10,000 entries) against library tracks. Returns each entry's status (`matched` / `ambiguous` / `unmatched` / `remote` / `duplicate`), candidates for ambiguous lines, totals, and up to 3 suggested folder swaps. File problems answer `413`/`422` with `code` `too_big` / `too_long` / `empty`. Nothing is written. |
| `POST` | `/api/playlists/import/commit` | Create a playlist from `{ name, description?, trackIds }` in that order, in one transaction (unknown ids and repeats skipped). Returns `{ playlist, added }`; `409` when the name is taken. |
| `POST` | `/api/playlists/smart-preview` | Evaluate unsaved rules `{ definition }` for the signed-in user: `{ trackCount, totalDuration, tracks (first 20), missingLibraryIds }`. Nothing is written. `429` `busy` when too many previews are waiting. |
| `GET` | `/api/playlists/{id}/smart-definition` | The owner's rules with `schemaVersion`, `editRevision`, `membershipRevision`, `evaluatedAt`, `supported` (false when a newer version stored them) and `missingLibraryIds`. |
| `PUT` | `/api/playlists/{id}/smart-definition` | Replace the rules `{ definition, expectedEditRevision }` and re-evaluate, all or nothing. `409` `edit_conflict` (with the current `editRevision`) when they changed meanwhile; `409` `unsupported` for rules from a newer version. |
| `GET` | `/api/playlists/{id}/snapshot` | Playlist and tracks from one read, smart playlists refreshed first: `{ playlist, tracks, membershipRevision, refreshError }`. `refreshError` (`timed_out` / `unsupported` / `failed`) means the last result is shown. |
| `POST` | `/api/playlists/{id}/refresh` | Re-evaluate a smart playlist now; returns the snapshot. |
| `POST` | `/api/playlists/{id}/static-copy` | Save a smart playlist's current tracks as a new ordinary playlist `{ name, description? }`; returns `{ playlist, added }`. |

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

## Activity (Admin)

The desktop Activity page. Everything except the heartbeat is admin-only (`403` otherwise). Live transfers are kept in memory and counted as response bodies are sent (web streams, Subsonic `stream`/`download`, Play On media, DLNA, the radio relay and BoogieMix downloads). History (plays, finished downloads, DLNA/radio streams over 30 s, Play On stops) is stored in `activity_events` for 7 days and merged with finished BoogieMix jobs and scans.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/activity` | Snapshot: `totals` (`playing`, `downloading`, `mixJobs`, `transcodes`, `subsonicTranscodeLimit`, `activeUsers`, `serverJobs`), `playing` (source `web`/`subsonic`/`playOn`/`dlna`/`radio`, user, client, item, `positionSec`, `durationSec`, `estimated`, `sampledAtMs`, `status`, `delivery`), `downloads` (`offlineSync` sessions per user and app, or single `file` downloads), `mixJobs` (with `canCancel`), `serverJobs` (scans, post-scan queue by type, deep-analysis counts) and `recent` (last 24 h, newest first). DLNA rows name the device when it is a known renderer and never show its address. |
| `GET` | `/api/activity/events` | Server-Sent Events: `snapshot` with the same body, sent when something changed (at most once a second; job tables are re-read every 2 s while connected). |
| `POST` | `/api/activity/heartbeat` | Any signed-in user: the web player's `{playerId, trackId, positionSec, durationSec, paused, stopped}` (`trackId` is a track id, `mx-{outputId}` or `st-{stationId}`). `204`. A player that stops reporting for 45 s is dropped. |
| `POST` | `/api/activity/mix-jobs/{jobId}/cancel` | Cancel any user's queued or analysing/planning BoogieMix job (`409` once rendering or finished). |

## Subsonic Apps (OpenSubsonic `/rest`)

Third-party Subsonic apps use `GET`/`POST` `/rest/{method}` (`.view` suffix optional; POST takes a form body).
Off until an admin sets `subsonicApiEnabled=true`. Apps sign in with `u` plus `p` (plain or `enc:<hex>`) or
`t`+`s` (`t = md5(password + salt)`), using the user's **Subsonic password**, never the PIN, or with an
OpenSubsonic **API key** alone (`apiKey`, no `u`). API keys are stored as SHA-256 only, so a replaced
`subsonic.key` does not affect them. Responses are XML by default, `f=json`, or `f=jsonp&callback=`;
failures are HTTP 200 with a Subsonic `error` (0 generic/unsupported, 10 missing parameter, 40 wrong
credentials or locked out after 5 failures for a minute — also for a user who has only an API key, 43 conflicting
mechanisms including `apiKey` with `u`, 44 invalid API key, 50 not allowed, 70 not found; 41 is not used). `/rest` CORS allows any origin without credentials. Responses never include filesystem paths: `path`
is a virtual `Artist/Album/NN - Title.ext`.

Supported: `ping`, `getLicense`, `getOpenSubsonicExtensions` (no sign-in needed; lists `formPost`,
`songLyrics`, `transcodeOffset`, `indexBasedQueue` and `apiKeyAuthentication`), `tokenInfo` (the API key's
username; error 0 when signed in another way), `getUser` (`jukeboxRole` is true when a jukebox speaker is set and Play On is on), `getMusicFolders` (folder ids are 1-based positions of the libraries), `getIndexes`,
`getArtists`, `getArtist`, `getAlbum`, `getSong`, `getGenres`, `getMusicDirectory`, `getAlbumList`/`getAlbumList2`
(`random`, `newest`, `highest`, `frequent`, `recent`, `alphabeticalByName`, `alphabeticalByArtist`, `starred`,
`byYear`, `byGenre`; `frequent` and `recent` use the user's own plays), `getRandomSongs`, `getSongsByGenre`, `getStarred`/`getStarred2`, `star`/`unstar`
(`id` resolves song → album → artist; `albumId`, `artistId`), `setRating` (whole stars 1–5, 0 clears),
`search2`/`search3` (an empty query pages through everything, which is how apps sync), `getPlaylists`,
`getPlaylist`, `createPlaylist` (a taken name gets " (2)"; with `playlistId` it replaces the entries; returns the
playlist), `updatePlaylist` (`name`, `comment`, `songIndexToRemove…` then `songIdToAdd…`; `public` ignored),
`deletePlaylist`, `getLyrics` (cache only), `getLyricsBySongId` (cache, then LRCLIB/lyrics.ovh within 5 s;
synced lines in ms), `getArtistInfo`/`getArtistInfo2` (local bio, else the cached Last.fm summary; similar
artists in the library), `getAlbumInfo`/`getAlbumInfo2`, `getSimilarSongs`/`getSimilarSongs2` (an Artist Radio
queue seeded by the song's, album's or artist's artist; provider lookups capped at 2 s), `getTopSongs` (cached
Last.fm top tracks first, then local plays), `getNowPlaying` (what apps, the BoogieBox web player and Play On
speakers reported in the last 10 minutes, all users; `playerName` is the app, "BoogieBox" or the speaker),
`scrobble` (`submission=true` counts a play at `time`, ignoring repeats within 30 s, and queues it for the user's
Last.fm/ListenBrainz accounts; `submission=false` marks now playing), `stream`, `download` (original file),
`getCoverArt` (`al-`/`ar-`/`tr-`/`pl-`/`mx-` ids; 300 or 800 px), `getInternetRadioStations` (all users; the
station's own URL), `create`/`update`/`deleteInternetRadioStation` (admins, else error 50), `jukeboxControl`,
`getScanStatus` (`scanning`; `count` = files scanned so far while scanning, else the track count; `lastScan`) and
`startScan` (admins and library managers, else error 50; scans every library in turn, reusing a scan already
queued; `fullScan` is ignored because scans are incremental), `getPlayQueue`/`savePlayQueue` and
`getPlayQueueByIndex`/`savePlayQueueByIndex` (one saved queue per user, shared by all their apps; up to 1000
entries and a song may repeat; `current` picks the first occurrence of that song, `currentIndex` a position;
`changedBy` is the saving app's `c`; saving no `id` clears the queue; songs that no longer exist, and other users'
mixes, are left out when the queue is read, and the saved position is dropped if the current entry is gone. The web
player does not share this queue).

`playCount` and `played` are the signed-in user's own (album `playCount` sums the user's plays of its tracks).

Jukebox: `jukeboxControl` drives the Play On speaker chosen in `subsonicJukeboxRenderer` as the requesting user.
Actions `get` (status plus `entry` songs), `status`, `set` (replace the queue; keeps playing if it was),
`start`, `stop` (pause), `skip` (`index`, `offset` seconds), `add`, `clear`, `remove` (`index`), `shuffle`,
`setGain` (`gain` 0–1 → speaker volume). Status: `currentIndex`, `playing`, `gain`, `position`. A speaker held by
another user returns error 50 ("in use by …"); no speaker, Play On off or the speaker offline returns error 0.

Playlists: a track appears at most once per BoogieBox playlist, so repeated song ids are dropped. Each user also
sees a read-only **BoogieMix** playlist (`bm-mixes`, `readonly=true`) listing their rendered mixes newest first,
each as one long song `mx-{outputId}` (album and artist "BoogieMix"). Mixes stream and download as the original
file from the BoogieMix output folder only, are never counted as plays, can't be starred or rated, and the
playlist can't be edited or deleted (error 50). Transitions and Mix Story chapters are not shown.

`stream` serves the original file unless the app asks for `format=mp3`, the file needs conversion to play, or
`maxBitRate` is below the file's bitrate; MP3 bitrates are 128/192/256/320 and at most `subsonicMaxTranscodes`
conversions run at once (HTTP 503 after a 10 s wait). `format=opus` streams Ogg Opus (64/96/128/160/192 kbps,
default 128; MP3 at the same cap when FFmpeg has no `libopus`) and `format=aac` (or `m4a`) streams AAC in ADTS
(96/128/192/256 kbps, default 192). These have no `Content-Length` and no byte ranges (`Accept-Ranges: none`).
`timeOffset` (seconds, OpenSubsonic `transcodeOffset`) starts an MP3, Opus or AAC conversion that far into the track;
an MP3 started this way is sized for the rest of the track and offers no byte ranges, and an offset at or past the
end returns an empty stream. The original file ignores `timeOffset` (apps seek it with byte ranges). Files outside their library folders are never served.
Other spec methods return error 0 "Not supported by BoogieBox". Half-star ratings are shown rounded up to whole
stars.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/subsonic/credentials` | Current user: `enabled`, `hasPassword`, `updatedAt`, `lastUsedAt`, `lastClient`, `serverUrls` (never the password). |
| `PUT` | `/api/subsonic/credentials/password` | Current user: set or change the Subsonic password (`{ password }`). Rules: 8–128 characters, at least one symbol, no control characters, not the PIN. A violation returns `400` with `error` and `rule` (`too_short`, `too_long`, `needs_symbol`, `invalid_characters`, `same_as_pin`). |
| `DELETE` | `/api/subsonic/credentials/password` | Current user: remove the Subsonic password. |
| `PUT` | `/api/admin/users/{id}/subsonic-password` | Admin: set another user's Subsonic password (same rules). |
| `DELETE` | `/api/admin/users/{id}/subsonic-password` | Admin: remove another user's Subsonic password. |
| `POST` | `/api/subsonic/credentials/api-key` | Current user: create an API key, replacing any existing one (apps using the old key are signed out). Returns `{ apiKey }` once (`Cache-Control: no-store`); only its hash is kept. `GET /api/subsonic/credentials` reports `hasApiKey` and `apiKeyCreatedAt`. |
| `DELETE` | `/api/subsonic/credentials/api-key` | Current user: revoke the API key (`404` when there is none). |
| `DELETE` | `/api/admin/users/{id}/subsonic-api-key` | Admin: revoke another user's API key (admins cannot create keys for others). `GET /api/admin/users` reports `hasSubsonicApiKey`. |

Settings (global, admin): `subsonicApiEnabled` (`true`/`false`, default `false`), `subsonicMaxTranscodes`
(1–8, default 3), `subsonicIgnoredArticles` (up to 20 words, default `The El La Los Las Le Les`),
`subsonicJukeboxRenderer` (a Play On renderer UDN, empty = no jukebox). `lastfmSecret` (the Last.fm API shared
secret, next to `lastfmKey`) enables Last.fm scrobbling. Subsonic
passwords are stored AES-256-GCM encrypted with the key file `subsonic.key` next to `boogiebox.db`; when moving
the database by hand, move `subsonic.key` with it or users must set their Subsonic passwords again. Apps send
the password (or its hash, or the API key) in the URL over plain HTTP. BoogieBox is for your home network only:
do not expose it to the internet. To listen away from home, download music to the app while on the home network.

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

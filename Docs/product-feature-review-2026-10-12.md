# BoogieBox functionality and market comparison — 2026-10-12

Reviewed version: **0.9.4**. Constraint: self-hosted music on the local network.

**The three highest-value gaps are LAN playback zones and remote control, saved smart playlists, and OpenSubsonic client compatibility.** Ranking reflects the value to a household using its own music collection, the size of the gap in the current implementation, and how well each feature fits local hosting. It is product judgment, not a market-share ranking.

**Review coverage and limits**

The review covered the repository's application structure and functionality across the React desktop/mobile client, Rust API and database, authentication/settings, scanning/enrichment, audio playback, Artist Radio, BoogieMix and Python analysis, DLNA, desktop shell, and Windows/Linux packaging. It traced the primary user flows through client calls, mounted routes and persisted data, and searched the source tree for the proposed missing capabilities. Current implementation took precedence over README claims and historical memory.

This is a repository-wide functionality review with targeted inspection of implementation paths, not a line-by-line defect/security audit. Physical playback devices, mobile background playback, installers and a running production library were not exercised. Competitor capabilities were checked against current primary documentation on October 12, 2026.

**What is already implemented**

| Area | Current functionality found in code | Main evidence |
| --- | --- | --- |
| Library and search | Multiple libraries/folders, scheduled scans, format/tag parsing, FTS search, artist/album browsing, filters, compilations and artist merge/unmerge | [scanner.rs](../server-rs/crates/boogiebox-server/src/scanner.rs), [music.rs](../server-rs/crates/boogiebox-db/src/music.rs), [BrowseView.tsx](../client/src/components/BrowseView.tsx) |
| Users | Local profiles, optional PINs, sessions, administrative permissions, per-user settings/ratings/playlists/history | [auth.rs](../server-rs/crates/boogiebox-server/src/auth.rs), [database schema](../server-rs/crates/boogiebox-db/src/lib.rs), [admin routes](../server-rs/crates/boogiebox-server/src/routes/admin_routes.rs) |
| Playback | Browser audio, editable queues, shuffle/repeat, two audio slots for crossfade/zero-gap transitions, waveform seeking, parametric EQ, vinyl effects, playlist progress | [Player.tsx](../client/src/components/Player.tsx), [playback routes](../server-rs/crates/boogiebox-server/src/routes/playback_routes.rs) |
| Audio delivery | Direct/range streaming and FFmpeg transcoding; optional EBU R128 loudness normalization during transcoding | [ffmpeg.rs](../server-rs/crates/boogiebox-server/src/ffmpeg.rs), [playback routes](../server-rs/crates/boogiebox-server/src/routes/playback_routes.rs) |
| Discovery | Genre-based Auto DJ, Artist Radio with seed/similar/mood pools, rating/recency signals, audio proximity and reason chips, listening dashboard | [artist_radio.rs](../server-rs/crates/boogiebox-server/src/artist_radio.rs), [music routes](../server-rs/crates/boogiebox-server/src/routes/music_routes.rs), [HomeView.tsx](../client/src/components/HomeView.tsx) |
| Playlists and mixes | Fixed per-user playlists, reordering, M3U export, rendered BoogieMix outputs, local analysis, deterministic planning, optional external AI planning, desktop mix timelines/images and mobile mix playback | [playlist data](../server-rs/crates/boogiebox-db/src/playlists.rs), [mix_worker.rs](../server-rs/crates/boogiebox-server/src/mix_worker.rs), [PlaylistsView.tsx](../client/src/components/PlaylistsView.tsx), [Python worker](../Services/boogiemix/python/boogiemix_demucs_worker.py) |
| Enrichment | Artwork caching/manual artwork, metadata editing and locks, lyrics, artist identity/styles, external provider lookup and background queues | [post_scan.rs](../server-rs/crates/boogiebox-server/src/post_scan.rs), [providers.rs](../server-rs/crates/boogiebox-server/src/providers.rs), [artwork routes](../server-rs/crates/boogiebox-server/src/routes/artwork_routes.rs) |
| Access and deployment | Shared browser/mobile player, Tauri shell with server discovery/control, Windows/Linux server packages and Linux Docker build, optional DLNA music serving | [MobileApp.tsx](../client/src/mobile/MobileApp.tsx), [desktop commands](../desktop/src-tauri/src/commands.rs), [dlna.rs](../server-rs/crates/boogiebox-server/src/dlna.rs), [Windows build](../build-server-rust.bat), [Linux build](../build-server-rust.sh), [Dockerfile](../Dockerfile.linux-amd64) |

Two implementation distinctions matter when interpreting existing capabilities. “ReplayGain” currently invokes FFmpeg loudness normalization only on the transcode path; it is not universal track/album gain handling for direct playback. Last.fm lookup and display of global scrobble counts are implemented, but the reviewed source did not contain personal scrobble submission. BoogieMix analysis/rendering and deterministic planning can run locally; configured Gemini/OpenRouter planning and metadata providers make external requests.

**Market benchmarks**

| Product | Relevant benchmark | Implication for BoogieBox |
| --- | --- | --- |
| Roon | Separates server, controller and audio output; centrally manages zone queues and compatible grouped outputs. [Architecture](https://help.roonlabs.com/portal/en/kb/articles/architecture), [supported outputs](https://help.roonlabs.com/portal/en/kb/articles/faq-what-audio-outputs-or-devices-are-supported-by-roon) | A phone can control the stereo without being the device producing the audio. |
| Lyrion Music Server | Supports a player/controller ecosystem and server commands for synchronization groups. [Players/controllers](https://lyrion.org/players-and-controllers/), [sync commands](https://lyrion.org/reference/cli/players/#sync) | Local hosting can support a coordinated household audio system. |
| Navidrome | Offers dynamic playlists and a broad Subsonic/OpenSubsonic client ecosystem; jukebox mode can play through server audio hardware. [Smart playlists](https://www.navidrome.org/docs/usage/features/smart-playlists/), [overview](https://www.navidrome.org/docs/overview/), [jukebox](https://www.navidrome.org/docs/usage/features/jukebox/) | Automation and standard client access add value without replacing the local server. |
| Plexamp | Offers smart playlists/stations, native mobile integration, casting and loudness leveling; additional sonic features are available. [Official feature list](https://www.plex.tv/plexamp/) | BoogieBox already addresses several discovery/audio ideas; daily playback convenience and automation remain strong comparison points. |

These are capability benchmarks. The recommendations do not depend on adopting the competitors' account, subscription or hosting models.

**1. LAN playback zones, remote control and eventual synchronized rooms**

**Current gap:** Playback queue/state is owned by React in [App.tsx](../client/src/App.tsx), around line 1092. [Player.tsx](../client/src/components/Player.tsx) plays through two HTML audio elements in the current client. The desktop bridge exposes server/configuration/folder operations, rather than an independent audio-output engine. DLNA advertises a **MediaServer** with ContentDirectory and ConnectionManager services and serves music for other devices to retrieve. It does not supply a BoogieBox control point that discovers renderers and sends playback commands. The reviewed routes/schema contain no registered playback zones or shared live queue/control model.

**Value:** Start an album on the living-room system from a phone, adjust it from a laptop, and keep the same queue when changing controllers. This is the strongest gap specifically created by the local-network use case.

**First useful scope:** Register one supported playback endpoint, keep its queue/current position on the server, and let another BoogieBox client control play/pause/seek/volume. A desktop connected to a stereo or a small dedicated local receiver would be a concrete first target. Persist the queue and support reconnecting controllers.

**Later scope:** Add selected network-output protocols and synchronized groups. Renderer control, queue transfer and synchronized audio are separate engineering milestones. Ordinary DLNA serving does not provide synchronization, and support for different protocols should be validated independently.

**Local fit:** Library, commands and audio remain on the LAN. A controlled local receiver can avoid dependence on a third-party device cloud.

**Effort:** High overall; a single endpoint plus remote control is a smaller first release than a complete multi-room system. Benchmark: Roon's centralized controller/output model and Lyrion's explicit sync groups, documented above.

**2. Saved rule-based smart playlists**

**Current gap:** The playlist schema at [lib.rs](../server-rs/crates/boogiebox-db/src/lib.rs), around line 314, stores name, description, owner and progress preference; playlist_tracks stores explicit track membership/order. [PlaylistPayload](../server-rs/crates/boogiebox-server/src/routes/playlist_routes.rs), around line 67, has no rule definition. The API offers CRUD, track mutations, ordering and export. Browse filters, Artist Radio and Auto DJ already select music, but none persists an editable rule set that automatically maintains playlist membership.

**Value:** A collector could save “my highly rated electronic tracks not played in 90 days” or “new additions at 110–125 BPM,” then have the selection update after scanning, rating and listening. These are example selection rules, not approved application UI copy.

**First useful scope:** A visual rule editor for genre, year, library, personal rating, date added, personal last-played and BPM; AND/OR groups; ordering and a track/duration limit. Evaluate locally and refresh with a bounded cache.

**Distinct benefit:** Radio creates a listening queue from a seed and scoring preferences. A smart playlist retains the user's explicit criteria and identity. It could also feed BoogieMix: snapshot the matched tracks at render time so later rule changes do not alter a mix already built.

**Existing assets:** Ratings, per-user play_history, tags, detected BPM and analysis data provide much of the underlying information. Personal history rules must use the playlist owner's history, rather than the shared tracks.play_count aggregate.

**Local fit:** Selection runs against SQLite and cached analysis. No external recommendation service is necessary; rules using enrichment should tolerate missing tags.

**Effort:** Medium for basic rules; more for sonic criteria. This is the strongest first development step by value relative to implementation effort. Navidrome documents automatic criterion-based membership, including ratings, dates and BPM; Plexamp lists smart playlists/stations.

**3. OpenSubsonic compatibility for existing native music clients**

**Current gap:** BoogieBox mounts its own /api routes in [build_app](../server-rs/crates/boogiebox-server/src/lib.rs), around line 495. The React [API client](../client/src/api.ts) consumes these custom routes. Authentication uses local PIN/session cookies. No Subsonic/OpenSubsonic REST adapter was found. The dedicated mobile layout reuses the web audio player, and the Tauri application wraps the same client; these do not make standard third-party music apps compatible.

**Value:** Users could choose an established native iOS, Android, desktop or TV music client while keeping BoogieBox as their local server. That can provide native playback/background/lock-screen integrations and broader device access through a supported client. Navidrome's [client directory](https://www.navidrome.org/apps/) shows the breadth of this ecosystem.

**First useful scope:** Select two clients, one for each mobile platform, and implement/test the endpoint subset they actually require: authentication/capability discovery, libraries, artists/albums/tracks, search, cover art, streaming, playlists and playback reporting. Extend ratings/lyrics/queue support as needed. Native functionality varies by client; compatibility should be demonstrated rather than assumed.

**Important boundary:** This complements feature 1. A standard API lets another application browse and play BoogieBox music; it does not automatically create synchronized BoogieBox playback zones. BoogieMix's detailed timeline and stem controls can continue using the existing application API.

**Local fit:** Clients connect directly to the server's LAN address. Public hosting, port forwarding, remote access and a cloud relay are unnecessary for this use case.

**Effort:** Medium-to-high. Existing database/streaming code is reusable, but protocol payloads, credentials, codec handling and client expectations need real compatibility tests. The [OpenSubsonic specification](https://opensubsonic.netlify.app/docs/) permits optional extensions; its [API-key authentication extension](https://opensubsonic.netlify.app/docs/extensions/apikeyauth/) offers one authentication option whose support must be checked in the selected clients.

**Validation**

- Source inspection and negative-capability searches covered the relevant client, Rust server/database, desktop and analysis-worker trees.
- npm run security:semgrep passed: 323 rules, 223 files, zero findings.
- Application code and version were unchanged. No test suite, installer, live production data or physical audio-device integration was exercised during this review.

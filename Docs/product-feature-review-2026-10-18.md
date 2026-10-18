# Competitive analysis — 2026-10-18

Reviewed BoogieBox: **0.9.16**. Product constraint: locally hosted, with no required software fees, subscriptions, cloud relay or paid API.

The recommended order is **smart playlists → offline mobile listening through free clients → synchronized multi-room playback**. This is product judgment based on daily usefulness, fit with an owned music collection, existing implementation and relative effort. No user interviews or usage analytics were available.

## Competitive benchmarks

| Free self-hosted alternative | Verified advantage relevant to BoogieBox | Product implication |
| --- | --- | --- |
| Navidrome | Rule-based playlists and a Subsonic/OpenSubsonic client ecosystem. Its own smart-playlist creation currently uses JSON files; Feishin supplies a graphical editor. | Deliver an integrated visual editor, then enable existing native clients. [Smart playlists](https://www.navidrome.org/docs/usage/features/smart-playlists/), [free server and API](https://www.navidrome.org/) |
| Jellyfin + Finamp | Finamp is free and provides music downloads, offline playback and native mobile/car integration. | Reliable listening after leaving home is a competitive expectation that can coexist with a server confined to the LAN. [Finamp project](https://github.com/finamp-app/finamp) |
| Lyrion Music Server | Local playback ecosystem with explicit synchronization groups and software players. | Extend household playback from controlling one speaker to coordinated rooms. [Server](https://lyrion.org/), [sync commands](https://lyrion.org/reference/cli/players/#sync), [players](https://lyrion.org/players-and-controllers/) |

These are selected capability benchmarks, not claims that each competitor supports every feature. Software can be free while playback hardware and maintenance still have costs.

## 1. Saved rule-based smart playlists

**Gap verified:** Playlists persist explicit track membership; their API payload has no saved rules. Browse filters, Auto DJ and Artist Radio already exist, but do not maintain a named selection against user-defined criteria. The October 12 smart-playlist implementation plan remains unimplemented.

**User value:** Automatically keep a selection of highly rated electronic tracks not played in 90 days, or recent additions within a chosen BPM range. This turns the user's existing ratings, history and analysis into recurring value.

**First release:** Visual rules for genre, year, rating, added date, personal last-played and BPM; all/any matching; sort and size limits; automatic refresh. Allow a selection to feed BoogieMix using a fixed snapshot for each rendered mix.

**Why first:** Broad collector value, reusable data and medium relative effort. An integrated editor would also improve on Navidrome's file-based setup. [Navidrome documentation](https://www.navidrome.org/docs/usage/features/smart-playlists/)

**Success measure:** Repeat playback from smart playlists and time needed to create a useful first selection.

## 2. Offline mobile listening through free native clients

**Gap verified:** The mobile UI uses BoogieBox's browser player. Source inspection found no offline library/cache system or Subsonic/OpenSubsonic adapter. Downloadable BoogieMix files already exist; they do not provide managed offline album/playlist listening.

**User value:** Download music while connected at home, then listen with the screen locked or without connectivity. The server can remain local throughout.

**Recommended delivery:** Add a tested OpenSubsonic compatibility layer rather than maintaining two new native applications. Initial candidates are Amperfy on iOS and Ultrasonic on Android: both support the relevant server family and offline use; Amperfy is listed as free and Ultrasonic describes itself as free software. [Amperfy features](https://github.com/BLeeEZ/amperfy), [Amperfy listing](https://apps.apple.com/us/app/amperfy-music/id1530145038), [Ultrasonic](https://ultrasonic.gitlab.io/), [client directory](https://www.navidrome.org/apps/)

**First release:** Test authentication, browsing, search, artwork, streaming and album/playlist downloads in those clients. Verify offline playback after disconnecting and reconnect behavior. Client capability and API compatibility must be demonstrated; implementing an API does not automatically deliver every native integration.

**Why second:** High everyday reach, with medium-to-high adapter/testing effort and ongoing client dependencies. Finamp demonstrates the free offline experience to match. [Finamp](https://github.com/finamp-app/finamp)

**Success measure:** Successful download-to-offline playback and reliable reconnection in one free client per mobile platform.

## 3. Synchronized multi-room playback

**Gap verified:** Play On already discovers and controls UPnP/DLNA renderers, shares server-owned sessions between controllers, transfers playback and persists sessions. These are implemented strengths. It lacks a synchronized group transport; independent renderer sessions do not provide aligned audio across rooms.

**User value:** Play one album or BoogieMix in the kitchen and living room with shared transport and individual room volume.

**First release:** Support one validated synchronization-capable endpoint family or a free local software receiver. Establish measured timing behavior on two outputs before expanding supported hardware. Protocol/integration choice needs a feasibility check; sending simultaneous play commands to arbitrary DLNA speakers is insufficient.

**Why third:** Strong fit for a home music product, but high effort and hardware-dependent reach. Lyrion's documented groups establish a free local benchmark. [Synchronization](https://lyrion.org/reference/cli/players/#sync), [software endpoints](https://lyrion.org/players-and-controllers/)

**Success measure:** Measured alignment and stable grouped playback through pause, seek, track changes and reconnects on the declared supported setup.

## Evidence and limits

Local evidence: [playlist API](../server-rs/crates/boogiebox-server/src/routes/playlist_routes.rs), [playlist persistence](../server-rs/crates/boogiebox-db/src/playlists.rs), [route mounting](../server-rs/crates/boogiebox-server/src/lib.rs), [mobile shell](../client/src/mobile/MobileApp.tsx), [Play On manager](../server-rs/crates/boogiebox-server/src/remote/mod.rs), [renderer sessions](../server-rs/crates/boogiebox-server/src/remote/session.rs), [active work](memory/ACTIVE_WORK.md).

Current source and project memory take precedence over marketing copy and the October 12 review, whose largest playback-control gap has since been implemented. Targeted searches found no implementations of the three remaining capabilities. Competitor claims were checked against primary sources on October 18, 2026; no live device/client compatibility tests or market-share claims are included.

All three recommendations preserve local operation and can avoid required service/client fees. Product wording, implementation and architecture choices are proposals only. No application code, version or production data changed.

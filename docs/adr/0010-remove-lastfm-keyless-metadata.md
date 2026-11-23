# ADR-0010: Remove Last.fm; metadata from keyless providers

## Status
Accepted 2026-11-23 (decisions made by the user 2026-11-22; plan `wip/lastfm-replacement-plan.md` §5; built in 0.9.123)

## Context
From 2026-12-01 a Last.fm API key only works if its owner has a Last.fm Pro subscription. BoogieBox asks each admin
for their own key, so every admin without Pro loses all Last.fm features that day: artist bios and stats, album
descriptions, artist identity, artist styles, track tags (Artist Radio, steering pills, the smart playlist `style`
rule), track popularity ("Popular" pill), artist top songs, part of the similar-artists list, Refresh Metadata search,
and Last.fm scrobbling. Non-commercial and open-source use is not exempt.

## Decision Drivers
- No feature should depend on a paid third-party subscription.
- No new setup for the admin: sources that need no key or account.
- Keep what the library already has (tags and popularity feed steering pills and Artist Radio).
- LAN-only server; outbound metadata calls are fine, nothing is exposed.

## Options Considered

### Option 1: keep Last.fm as a fallback behind keyless sources
Pros:
- Admins with Pro keep Last.fm's richer crowd tags and scrobbling.

Cons:
- Two code paths per feature; a feature most admins can't use; key-health handling stays forever.

### Option 2: remove Last.fm, keyless providers only
Pros:
- One path per feature, no key settings, nothing that breaks when a subscription lapses.

Cons:
- MusicBrainz tags are thinner than Last.fm's, especially per track; ListenBrainz counts are far smaller.
- Last.fm scrobblers must move (ListenBrainz, Libre.fm, a self-hosted server).

## Decision
We will remove Last.fm support (metadata, scrobbling, key settings) and take metadata from keyless providers:
identity from file tags and MusicBrainz; bios from Wikipedia through MusicBrainz → Wikidata links, then TheAudioDB with
its free shared key; styles and tags from MusicBrainz plus Discogs; popularity and top songs from ListenBrainz, then
Deezer; similar artists from ListenBrainz and Deezer. Stats are shown as ListenBrainz numbers. Scrobbling targets are
ListenBrainz (with a custom server address for compatible servers) and Libre.fm. Track tags, track popularity and
artist ids already stored from Last.fm are kept until a keyless source replaces them; saved Last.fm answers (bios,
album descriptions, top tracks, similar artists), Last.fm scrobble accounts with their waiting plays, the key settings
and queued Last.fm jobs are deleted by the removal migration. The Last.fm-only "Sync artist styles" admin job goes
too. A dead-key guard ships first so a release before the removal degrades quietly.

## Rationale
Most admins lose Last.fm on 1 December anyway; keeping it as a fallback would cost two code paths for few users.
Every replacement needs no key or user setting.

## Consequences
- Positive: no API keys to manage for metadata; nothing stops working when a subscription ends.
- Trade-off: thinner tags and smaller counts; coverage is measured on the dev library before and after.
- Trade-off: credit lines for Wikipedia (CC BY-SA) and MusicBrainz/ListenBrainz data.
- Follow-up: phases 0–7 and the removal are done (0.9.123); phase 8 (docs, credits, the keyless switch as the main
  switch) remains. UI wording needs approval first.

## Related
- `wip/lastfm-replacement-plan.md`
- https://support.last.fm/t/api-keys-will-require-pro-from-1st-december/121030
- ADR-0002 (LAN-only)

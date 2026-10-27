# ADR-0001: BoogieBox is a music-only product

## Status
Accepted (2026-05-25)

## Context
Video features (movies, TV, media files, subtitles, TMDb metadata) were built but never shipped. When the Rust server became the active runtime, the hidden video code still added schema, routes, settings, jobs, client views and desktop native-video plumbing.

## Decision Drivers
- Music is the only feature set in use and validated
- Hidden code still adds schema risk and maintenance work
- Smaller surface for the Rust port and packaging

## Options Considered

### Keep video hidden behind flags
Pros:
- Video could return without rewriting

Cons:
- Schema and build complexity with no user benefit
- Every migration has to keep dead tables consistent

### Remove video completely
Pros:
- Music-only schema and code paths
- Less to port, test and package

Cons:
- Any future video work starts from scratch
- Beta databases lose video data (no export)

## Decision
We will remove all video schema, routes, settings, jobs, client views and desktop native-video commands. Upgrade migrations drop legacy video tables and settings destructively.

## Rationale
Keeping unused video code costs more than rebuilding it later if video is ever wanted.

## Consequences
- Fresh databases are music-only; `libraries.library_type` accepts only `music`
- DLNA serves audio only
- Future video work needs its own design and ADR
- Completed in 0.7.159

## Related
- `docs/memory/DECISIONS.md` (2026-05-25)

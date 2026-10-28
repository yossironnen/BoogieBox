# ADR-0008: Backup bundles and before-upgrade snapshots

## Status
Accepted (2026-10-28)

## Context
BoogieBox had no backup. Tracked migrations run once per database and can't be undone, so a bad migration or a
failed disk loses users, playlists, ratings, play history and password secrets. Users also need to move a server to
a new machine (often Windows → Linux) where every stored music path changes.

## Decision Drivers
- Backups must never block the server (single SQLite connection behind a mutex).
- A backup must be restorable on a later version and on another OS.
- Secrets (the Subsonic/scrobbling key) must come along, or restored passwords are useless.
- Restore must be safe to undo.

## Options Considered

### Copy the database file
Pros:
- Trivial.

Cons:
- Not consistent while the server writes (WAL, `-shm`); needs the server stopped.

### `VACUUM INTO` on a separate connection, as a folder bundle with a manifest
Pros:
- Consistent snapshot without blocking the server's connection in WAL mode.
- The manifest (version, applied migration ids, checksum, counts, library roots, artwork list) lets a build refuse
  backups from newer versions and remap paths on a new machine.

Cons:
- The bundle layout and manifest become a compatibility contract.

## Decision
A backup is a folder `boogiebox-<YYYYMMDD-HHMMSS>-v<version>/` with `boogiebox.db` (`VACUUM INTO`), `subsonic.key`,
`manifest.json` (format 1) and optional `art/` (locked originals always; all originals optionally) and `mixes/`. It is
written as `<name>.partial` and renamed when complete. `init_db` takes a `pre-migration` bundle before running
pending migrations on an existing database (last 2 kept). A restore saves current data to `pre-restore-<stamp>/` with
its own manifest, clears sessions, and applies library-root remaps to `libraries`, `library_folders`, `tracks`,
`tracks_fts` and `mix_outputs`. A backup whose manifest names a migration this build doesn't know is refused.

## Rationale
`VACUUM INTO` is SQLite's supported online copy; folder bundles stay inspectable and copyable by hand; refusing
unknown migrations is the only safe rule given append-only migrations (ADR-0004).

## Consequences
- Future builds must keep reading manifest format 1 (add fields with defaults; bump `format` only with a reader for
  the old one).
- The backup folder is as sensitive as the data folder (it holds the key); password-encrypted bundles are deferred.
- In DELETE journal mode (UNC data folders) writers wait for the snapshot.

## Related
- ADR-0004 (append-only tracked migrations), ADR-0002 (LAN-only)
- `wip/p1-features-implementation-plan.md` §3 (F03)
- `server-rs/crates/boogiebox-db/src/backup.rs`, `server-rs/crates/boogiebox-server/src/backup.rs`

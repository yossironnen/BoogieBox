# ADR-0004: Tracked migrations are append-only

## Status
Accepted (2026-09-15)

## Context
`boogiebox-db` runs each tracked migration (`Migration { id, apply }`) once per database and records its id in `schema_migrations`. Editing the body of an existing migration therefore changes nothing on databases that already ran it. In 2026-09 a column added inside an existing migration passed the full test suite, because tests use fresh in-memory databases, and was missing on every real database.

## Decision Drivers
- Existing user databases must reach the same schema as fresh ones
- Tests cannot detect edits to already-applied migrations

## Options Considered

### Edit existing migrations in place
Pros:
- Fewer migration ids

Cons:
- Silently skipped on every existing database
- Fresh and upgraded databases diverge

### Add a new migration id for every schema change
Pros:
- Upgraded databases always receive the change
- The history of schema changes stays readable

Cons:
- More migration entries over time

## Decision
We will never change the body of a released migration. Every schema change gets a new, dated migration id (for example `2026-10-27-library-missing-kept`).

## Rationale
The runner's once-per-id design is correct; this rule keeps code and real databases in step.

## Consequences
- New migrations must tolerate databases that lack optional tables (legacy-migration tests check this)
- Schema changes need a check against a real, pre-existing database, not only the test suite

## Related
- ADR-0003
- `server-rs/crates/boogiebox-db/src/lib.rs` (`run_tracked_migrations`)

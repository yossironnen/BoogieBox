# ADR-0003: Rust Axum server with SQLite owned by `boogiebox-db`

## Status
Accepted (2026-05-23)

## Context
The original Node server was replaced by a standalone Rust server (Axum, Tokio, `rusqlite`) to ship one self-contained executable on Windows and Linux. As routes were ported, SQLite opening, schema bootstrap, migrations and startup repair risked spreading across the HTTP crate.

## Decision Drivers
- One self-contained server binary per platform
- One place for SQLite policy (journal mode, UNC handling, migrations)
- Predictable schema state for every route and job

## Options Considered

### Open SQLite directly from route and job code
Pros:
- Fewer crates

Cons:
- Schema and pragma policy duplicated and drifting
- Startup repair order unclear

### Dedicated `boogiebox-db` crate
Pros:
- Setup, startup, migrations and repair in one crate
- Routes and jobs depend on one consistent schema state

Cons:
- An extra crate boundary to maintain

## Decision
We will run the server as the Rust `boogiebox-server` crate and keep all database opening, schema creation, tracked migrations, startup repair and immediate stat refreshes in `server-rs/crates/boogiebox-db`.

## Rationale
Centralizing database behavior early kept the port consistent and made database readiness (`boogiebox.db` exists) a single check.

## Consequences
- New server code goes through `boogiebox-db`, not ad hoc `rusqlite` connections
- Schema changes follow ADR-0004
- Windows builds must keep UNC paths intact in database and filesystem handling

## Related
- ADR-0004
- `docs/memory/DECISIONS.md` (2026-05-14 and 2026-05-23 entries)

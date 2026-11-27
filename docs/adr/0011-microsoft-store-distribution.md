# ADR-0011: Microsoft Store distribution from the GitHub release installers

## Status
Accepted 2026-11-26 (plan `wip/microsoft-store-distribution-plan.md`; decisions D1–D11 accepted as recommended by the user)

## Context
BoogieBox Server and the desktop app are only on GitHub Releases today (unsigned Inno Setup and Tauri NSIS installers).
Listing them in the Microsoft Store makes them easier to find. Tauri can't produce MSIX, so the Store route is a Win32
EXE listing: the installer must be code signed, fully offline, silent-installable and hosted at a stable versioned HTTPS
URL. The server installer's optional BoogieMix task downloads 2–5 GB during install, which breaks the offline rule.
GitHub downloads must keep working, and releases must stay cheap to ship.

## Decision Drivers
- One build per release, no Store-only variants to keep in sync
- GitHub stays a first-class download source
- Store offline-installer, signing and silent-install rules
- LAN-only product stance (no public demo server)

## Options Considered

### Option 1: Win32 EXE listings pointing at the GitHub release assets
Pros:
- Same signed installer for both channels; users can switch freely
- No new packaging toolchain
Cons:
- Needs an own code-signing certificate (Azure Artifact Signing, individual, Canada)
- Updates need a new Store submission per release (scriptable)

### Option 2: MSIX packages
Pros:
- Microsoft signs them; the Store handles updates
Cons:
- Hand-made packaging outside Tauri/Inno; per-user, virtualised install differs from the GitHub install
- Two different artifacts per release

## Decision
We will list both Windows apps in the Microsoft Store as Win32 EXE listings that point at the signed, versioned GitHub
release installers (Option 1). BoogieMix deep-analysis setup moves from the Windows installer into the server UI. Linux
keeps BoogieMix setup in `install.sh`.

## Rationale
One artifact per app keeps releases simple and keeps GitHub and Store installs identical. Moving BoogieMix setup in-app
satisfies the offline rule for both channels.

## Consequences
- Code signing added to both build flows
- Daily GitHub release check with an admin update notice (can be turned off)
- `publish-release.bat` creates the GitHub release and updates both Store submissions
- Release assets a Store listing points to must never be deleted
- Desktop installer grows by ~127 MB (offline WebView2)

## Related
- `wip/microsoft-store-distribution-plan.md`
- ADR-0009 (desktop native Hi-Res engine)

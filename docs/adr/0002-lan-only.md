# ADR-0002: BoogieBox stays LAN-only

## Status
Accepted (2026-10-23)

## Context
The OpenSubsonic API made it tempting to document remote access (reverse proxy, HTTPS front end, port forwarding, VPN). The server was designed for the home network: PIN sign-in, a LAN-focused CORS allowlist, no TLS, no internet-grade rate limiting.

## Decision Drivers
- The security model assumes a trusted home network
- Avoid guidance that exposes an unhardened server
- Keep Subsonic, DLNA and Play On simple

## Options Considered

### Document internet exposure now
Pros:
- Listening away from home without downloads

Cons:
- Exposes PIN auth, CORS and Subsonic credentials to the internet
- Support burden for proxies and certificates

### LAN-only, internet exposure as a future phase
Pros:
- Security model matches how the server is built
- Clear message in docs and UI

Cons:
- Away from home, users must download music to their app first

## Decision
We will keep BoogieBox (web UI, Subsonic `/rest`, DLNA, Play On) for the home network only. Docs and UI do not suggest reverse proxies, HTTPS front ends, port forwarding, tunnels or VPN remote access.

## Rationale
Internet exposure needs TLS, credential handling, CORS and rate-limit work first; until then any such guidance would be unsafe.

## Consequences
- Subsonic users listen away from home by downloading on the LAN
- The planned phase-4 reverse-proxy guide is replaced by a LAN-only statement
- Opening to the web is a future development phase and needs a new ADR that supersedes this one

## Related
- `docs/memory/DECISIONS.md` (2026-10-23)
- `wip/opensubsonic-api-implementation-plan.md` (earlier reverse-proxy notes are superseded)

# ADR-0007: 80% client coverage on every metric

## Status
Accepted (2026-07-24)

## Context
Client test coverage was below the required minimum, and nothing stopped it from dropping further.

## Decision Drivers
- Prevent coverage regressions automatically
- A single aggregate number can hide untested branches

## Options Considered

### One aggregate or line-only threshold
Pros:
- Easier to meet

Cons:
- Untested branches and functions can hide behind good line coverage

### 80% on statements, branches, functions and lines separately
Pros:
- Every metric is protected

Cons:
- Branch coverage is the hardest to keep above the bar

## Decision
We will enforce Vitest/V8 global thresholds of 80% for statements, branches, functions and lines, each on its own.

## Rationale
The four metrics measure different gaps; each must stay at or above the requirement.

## Consequences
- `npm --prefix client test` fails when any global metric drops below 80%
- Client changes include meaningful regression tests and keep a margin above the branch threshold

## Related
- `client/vite.config.ts` (`coverage.thresholds`)
- `docs/memory/DECISIONS.md` (2026-07-24)

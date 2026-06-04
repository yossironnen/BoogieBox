# Project Memory

Use these files to carry project context across machines and sessions.

## Files

- `CURRENT_STATE.md`: what is true right now
- `ACTIVE_WORK.md`: in-flight tasks, blockers, and next actions
- `DECISIONS.md`: durable architectural or product decisions
- `ENVIRONMENT.md`: machine-specific setup notes and local caveats
- `HANDOFF_TEMPLATE.md`: copy/paste template for session handoffs

## Suggested Workflow

1. Update `CURRENT_STATE.md` when project behavior or scope changes.
2. Update `ACTIVE_WORK.md` at the start and end of each work session.
3. Add a short entry to `DECISIONS.md` when a decision should be remembered later.
4. Use `ENVIRONMENT.md` for machine-specific notes that affect development.
5. Copy `HANDOFF_TEMPLATE.md` when handing work to another machine or session.

## Ground Rules

- Keep entries short and factual.
- Prefer dates in `YYYY-MM-DD`.
- Separate durable facts from temporary notes.
- Do not store secrets, tokens, or private credentials here.

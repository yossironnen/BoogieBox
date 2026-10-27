# ADR-0006: New UI copy requires approval

## Status
Accepted (2026-07-28)

## Context
Labels and text added without being asked for cluttered the interface and changed the product's voice.

## Decision Drivers
- The product owner keeps control of every user-facing word
- Consistent tone across desktop and mobile

## Options Considered

### Contributors choose wording during implementation
Pros:
- Faster implementation

Cons:
- Inconsistent voice; unwanted text reaches releases

### Approve exact wording before implementation
Pros:
- Every label is intentional

Cons:
- An approval step in each UI task

## Decision
We will not add or change visible or accessibility-only UI copy until the product owner approves the exact wording. Wording the owner supplied, and unchanged reuse of existing copy, count as approved.

## Rationale
Wording is a product decision, not an implementation detail.

## Consequences
- UI plans list proposed labels, headings, descriptions, controls, messages, tooltips, placeholders and accessibility text for approval
- This covers `title` and `aria-label` text on icon-only controls
- No taglines under view titles unless asked

## Related
- `CLAUDE.md` "UI Design Conventions"
- `docs/memory/DECISIONS.md` (2026-07-28)

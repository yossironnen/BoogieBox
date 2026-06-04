---
name: ui-designer
description: Switch to UI Designer mode for BoogieBox. Use when the user asks to design, redesign, improve, or implement UI for desktop or mobile views.
---

You are now in **UI Designer mode** for BoogieBox.

## What you know about this UI

BoogieBox is a self-hosted music app. It has two UI surfaces that must stay visually consistent:

**Desktop** — full React SPA served from `client/src/`. Inline styles via JS objects. No CSS modules or styled-components.

**Mobile** — a separate shell (`client/src/mobile/`) that auto-activates on iPhone-sized viewports. Same React patterns, same inline style approach.

## Design System

All colors come from CSS custom properties — never hardcode hex values for themeable colors:

| Variable | Purpose |
|---|---|
| `var(--bg)` | Page/app background |
| `var(--surface)` | Cards, panels, elevated elements |
| `var(--border)` | Dividers, outlines |
| `var(--accent)` | Primary interactive color (buttons, highlights, active states) |
| `var(--text)` | Primary text |
| `var(--text-muted)` | Secondary/disabled text |
| `var(--font)` | Font family |

`var(--surface-hover)` and `var(--accent-primary)` exist but are sparsely used — prefer `--surface` and `--accent`.

The theme is user-configurable (colors + background texture). Never assume dark or light — design for both.

## Desktop UI Conventions

- **Layout**: Inline JS style objects. No external CSS files for component styles.
- **Spacing**: 8px base grid. Common values: 4, 8, 12, 16, 20, 24, 32px.
- **Typography**: `var(--font)` for family. Sizes: 11px (labels/meta), 12px (secondary), 13px (body), 14-16px (headings).
- **Borders**: `1px solid var(--border)` standard. `borderRadius: 4` small elements, `6` medium, `8` large cards.
- **Interactive states**: Use `opacity` or color shift on hover/active. No box-shadow for hover — keep it flat.
- **Lists**: Typically flex column. Row items use `display: flex, alignItems: center, gap: 8-12`.
- **Scrollable areas**: `overflowY: 'auto'`, `flex: 1, minHeight: 0` on the scroll container inside a flex parent.
- **Desktop key files**: `App.tsx`, `BrowseView.tsx`, `PlaylistsView.tsx`, `HomeView.tsx`, `SettingsPage.tsx`, `Player.tsx`

## Mobile UI Conventions

- **Shell**: `client/src/mobile/MobileApp.tsx` — top brand bar + tab content area + docked mini-player + bottom tab bar.
- **Viewport**: Treat as ~390px wide max. Full-height with `height: 100dvh` or `100%`.
- **Touch targets**: Minimum 44×44px for all interactive elements.
- **Navigation**: Tab-based (`browse`, `search`, `playlists`, `now-playing`). Drill-down within tabs — no separate router.
- **Hero screens**: Artist and album detail screens use a large image hero at top, content scrolls beneath.
- **Mini-player**: Always docked above the tab bar when something is playing. Do not obscure it.
- **Now Playing**: Full-screen takeover. Album art tap cycles: cover → karaoke lyrics → plain lyrics.
- **Lists**: Track rows show album art thumbnail (40×40) + title/artist + duration. Swipe-to-delete and drag-reorder on playlist screens.
- **Mobile key files**: `MobileApp.tsx`, `MobileBrowseView.tsx`, `MobileNowPlayingView.tsx`, `MobilePlaylistsView.tsx`, `MobileSearchView.tsx`, `MobileMiniPlayer.tsx`, `MobileTabBar.tsx`

## Shared props interface (`MobileSharedProps`)

Mobile views receive: `currentUser`, `playerState`, `playbackSnapshot`, `onPlayTrack`, `onAddToQueue`, `onPlaybackStateChange`. Pass these through — never fetch auth or player state independently inside a mobile view.

## Design process

When asked to design or change UI:

1. **Clarify scope** — desktop only, mobile only, or both (must stay consistent)?
2. **Read the relevant file(s)** before proposing anything.
3. **Propose visually** — describe the layout clearly (what's at top, what scrolls, what's fixed) before writing code.
4. **Design for both surfaces** if the feature exists on both. Mobile gets a simplified version, not a scaled-down copy.
5. **Use existing patterns** — match the style of surrounding components. Introduce new patterns only when necessary.
6. **Accessibility**: meaningful `aria-label` on icon-only buttons. Sufficient color contrast. Keyboard navigability on desktop.

## Output rules

- Show only changed code — not full files.
- After changes: run `npm test`, `npm run lint`, `npm run version:bump`, update `changes.log`.
- Never commit or push.
- If a design requires a new CSS variable, propose it — but confirm with the user before adding to the theme system.

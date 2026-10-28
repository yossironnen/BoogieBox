/**
 * Desktop transport shortcuts: one window listener, ignored while typing, inside widgets that own the key,
 * or while a modal dialog is open.
 */

import { useEffect, useRef } from 'react';

export type PlayerShortcutAction =
  | 'togglePlay'
  | 'seekBack'
  | 'seekForward'
  | 'previous'
  | 'next'
  | 'volumeUp'
  | 'volumeDown'
  | 'mute'
  | 'search'
  | 'help';

export type PlayerShortcutHandlers = Record<PlayerShortcutAction, () => void>;

export const SHORTCUT_SEEK_SECONDS = 10;
export const SHORTCUT_VOLUME_STEP = 0.05;

/** Help dialog rows, in display order. */
export const PLAYER_SHORTCUTS: ReadonlyArray<{ keys: string; label: string }> = [
  { keys: 'Space', label: 'Play / pause' },
  { keys: '← / →', label: 'Seek back / forward 10 s' },
  { keys: 'Shift+← / →', label: 'Previous / next track' },
  { keys: 'Shift+↑ / ↓', label: 'Volume up / down' },
  { keys: 'M', label: 'Mute' },
  { keys: '/', label: 'Search' },
  { keys: '?', label: 'Show shortcuts' },
];

type KeyInfo = Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey'>;

/** The action a key press maps to, ignoring where it happened. */
export function shortcutAction(event: KeyInfo): PlayerShortcutAction | null {
  if (event.ctrlKey || event.altKey || event.metaKey) return null;
  switch (event.key) {
    case ' ':
    case 'Spacebar':
      return event.shiftKey ? null : 'togglePlay';
    case 'ArrowLeft':
      return event.shiftKey ? 'previous' : 'seekBack';
    case 'ArrowRight':
      return event.shiftKey ? 'next' : 'seekForward';
    // Plain ↑/↓ stay as page scroll.
    case 'ArrowUp':
      return event.shiftKey ? 'volumeUp' : null;
    case 'ArrowDown':
      return event.shiftKey ? 'volumeDown' : null;
    case 'm':
    case 'M':
      return 'mute';
    case '/':
      return 'search';
    case '?':
      return 'help';
    default:
      return null;
  }
}

const EDITABLE_SELECTOR = 'input, textarea, select, [contenteditable=""], [contenteditable="true"]';
/** Widgets that use Space themselves. */
const SPACE_OWNER_SELECTOR = 'button, a[href], summary, [role="button"], [role="checkbox"], [role="switch"], [role="menuitem"], [role="option"], [role="tab"], [role="radio"]';
/** Widgets that use the arrow keys themselves. */
const ARROW_OWNER_SELECTOR = '[role="slider"], [role="tab"], [role="tablist"], [role="radio"], [role="radiogroup"], [role="menu"], [role="menuitem"], [role="listbox"], [role="option"]';
const MODAL_SELECTOR = '[aria-modal="true"], [role="alertdialog"]';

/** True when the key belongs to whatever has focus (or a modal), not to the player. */
export function shortcutBlocked(event: KeyboardEvent, action: PlayerShortcutAction, doc: Document = document): boolean {
  if (event.defaultPrevented || event.isComposing) return true;
  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest(EDITABLE_SELECTOR)) return true;
  if (doc.querySelector(MODAL_SELECTOR)) return true;
  if (action === 'togglePlay' && target?.closest(SPACE_OWNER_SELECTOR)) return true;
  const arrow = event.key.startsWith('Arrow');
  if (arrow && target?.closest(ARROW_OWNER_SELECTOR)) return true;
  return false;
}

/** Held keys repeat only the seek actions. */
function repeats(action: PlayerShortcutAction): boolean {
  return action === 'seekBack' || action === 'seekForward';
}

export function usePlayerShortcuts(handlers: PlayerShortcutHandlers | null): void {
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  const enabled = handlers != null;

  useEffect(() => {
    if (!enabled) return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      const current = handlersRef.current;
      const action = shortcutAction(event);
      if (!current || !action || shortcutBlocked(event, action)) return;
      event.preventDefault();
      if (event.repeat && !repeats(action)) return;
      current[action]();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [enabled]);
}

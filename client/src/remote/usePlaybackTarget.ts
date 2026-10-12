/**
 * Where playback happens. Stored per browser (not per user): it describes the device in your hand.
 */

import { useCallback, useState } from 'react';
import type { PlaybackTarget } from './remoteTypes';

export const PLAYBACK_TARGET_STORAGE_KEY = 'bb.playbackTarget';
const LOCAL: PlaybackTarget = { kind: 'local' };

export function readStoredPlaybackTarget(): PlaybackTarget {
  try {
    const raw = localStorage.getItem(PLAYBACK_TARGET_STORAGE_KEY);
    if (!raw) return LOCAL;
    const parsed = JSON.parse(raw) as Partial<{ kind: string; udn: unknown }>;
    return parsed?.kind === 'renderer' && typeof parsed.udn === 'string' && parsed.udn
      ? { kind: 'renderer', udn: parsed.udn }
      : LOCAL;
  } catch {
    return LOCAL;
  }
}

export function storePlaybackTarget(target: PlaybackTarget): void {
  try {
    if (target.kind === 'local') localStorage.removeItem(PLAYBACK_TARGET_STORAGE_KEY);
    else localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify(target));
  } catch {
    // Storage unavailable (private mode, blocked): the target just isn't remembered.
  }
}

/** The persisted playback target. */
export function usePlaybackTarget(): [PlaybackTarget, (target: PlaybackTarget) => void] {
  const [target, setTargetState] = useState<PlaybackTarget>(readStoredPlaybackTarget);
  const setTarget = useCallback((next: PlaybackTarget) => {
    storePlaybackTarget(next);
    setTargetState(next);
  }, []);
  return [target, setTarget];
}

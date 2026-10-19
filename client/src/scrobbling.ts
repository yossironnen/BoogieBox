/**
 * Scrobbling to the signed-in user's own Last.fm and ListenBrainz accounts. The server keeps the secrets and
 * sends the plays; the web player only says when a play counts (`scrobbleThreshold`).
 */

export type ScrobbleService = 'lastfm' | 'listenbrainz';

/** One connected account (never includes the token or session key). */
export interface ScrobbleAccount {
  service: ScrobbleService;
  accountName: string | null;
  needsReconnect: boolean;
  connectedAt: string;
  lastScrobbleAt: string | null;
  /** Plays waiting to be sent. */
  pending: number;
}

/** `GET /api/scrobble/accounts`. */
export interface ScrobbleAccountsResponse {
  /** The admin entered the Last.fm API key and shared secret. */
  lastfmAvailable: boolean;
  accounts: ScrobbleAccount[];
}

/**
 * Seconds a track must play before it counts: half its length or 4 minutes, whichever is less. Tracks of
 * 30 seconds or less never count (Last.fm rule); so does anything without a known length.
 */
export function scrobbleThreshold(duration: number | null | undefined): number | null {
  if (duration == null || !Number.isFinite(duration) || duration <= 30) return null;
  return Math.min(duration / 2, 240);
}

/** ListenBrainz tokens are UUID-like; mirrors the server's shape check. */
export function plausibleListenBrainzToken(token: string): boolean {
  const t = token.trim();
  return t.length >= 8 && t.length <= 128 && /^[A-Za-z0-9-]+$/.test(t);
}

/** A play that just passed its threshold. */
export interface ScrobbleDue {
  trackId: string;
  /** When the play started (Unix seconds). */
  startedAt: number;
}

/**
 * Watches one player's position and says once per play when it counts. A play only counts after it was seen
 * below its threshold, so a track continued past the threshold (a Play On handoff) is not scrobbled twice.
 */
export function createScrobbleTracker() {
  let key: string | null = null;
  let startedAt = 0;
  let armed = false;
  let done = false;
  return {
    /** `playKey` changes with every new play of a track; `nowSec` is the wall clock in seconds. */
    observe(playKey: string | null, trackId: string, duration: number | null | undefined, position: number, nowSec: number): ScrobbleDue | null {
      if (playKey !== key) {
        key = playKey;
        startedAt = Math.floor(nowSec - Math.max(0, position));
        armed = false;
        done = false;
      }
      if (playKey == null || done || !Number.isFinite(position)) return null;
      const threshold = scrobbleThreshold(duration);
      if (threshold == null) return null;
      if (position < threshold) {
        armed = true;
        return null;
      }
      if (!armed) return null;
      done = true;
      return { trackId, startedAt };
    },
  };
}

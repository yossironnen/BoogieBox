/**
 * Reports this tab's web player to the server (`POST /api/activity/heartbeat`) so admins see real positions and
 * pauses on the Activity page: on every track or play/pause change, every 15 s while something is loaded, and a
 * final "stopped" when playback ends, moves to a Play On speaker or the tab closes.
 */

import { useEffect, useRef } from 'react';
import { api } from '../api';
import type { PlaybackSnapshot } from '../components/Player';
import { RADIO_TRACK_PREFIX } from '../radioStations';

export const HEARTBEAT_INTERVAL_MS = 15_000;

/** Server item id: library track id, `mx-{outputId}` for a BoogieMix render, `st-{stationId}` for radio. */
export function heartbeatItemId(id: string | number | null | undefined): string | null {
  if (id == null) return null;
  const text = String(id);
  if (!text) return null;
  if (text.startsWith('boogiemix:')) return `mx-${text.slice('boogiemix:'.length)}`;
  if (text.startsWith(RADIO_TRACK_PREFIX)) return `st-${text.slice(RADIO_TRACK_PREFIX.length)}`;
  return text;
}

function newPlayerId(): string {
  try {
    const existing = sessionStorage.getItem('boogiebox.playerId.v1');
    if (existing) return existing;
  } catch {
    // Storage blocked: a fresh id per page load is fine.
  }
  const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `p-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  try { sessionStorage.setItem('boogiebox.playerId.v1', id); } catch { /* ignore */ }
  return id;
}

/**
 * `snapshot` is the local player's state; `active` is false while signed out or playing on a Play On speaker.
 */
export function useWebPlayerHeartbeat(snapshot: PlaybackSnapshot | null, active: boolean): void {
  const playerIdRef = useRef<string | null>(null);
  const latestRef = useRef<PlaybackSnapshot | null>(snapshot);
  const sentRef = useRef(false);
  latestRef.current = snapshot;

  const itemId = active ? heartbeatItemId(snapshot?.currentTrack?.id) : null;
  const playing = Boolean(snapshot?.isPlaying);

  useEffect(() => {
    if (!playerIdRef.current) playerIdRef.current = newPlayerId();
    const playerId = playerIdRef.current;
    const send = (closing = false) => {
      const latest = latestRef.current;
      if (!itemId || closing) {
        if (sentRef.current) {
          api.activity.heartbeat(
            { playerId, trackId: null, positionSec: 0, durationSec: null, paused: true, stopped: true },
            { closing },
          );
          sentRef.current = false;
        }
        return;
      }
      const duration = latest?.duration && Number.isFinite(latest.duration) && latest.duration > 0
        ? latest.duration
        : null;
      api.activity.heartbeat({
        playerId,
        trackId: itemId,
        positionSec: Math.max(0, latest?.currentTime ?? 0),
        durationSec: duration,
        paused: !latest?.isPlaying,
      });
      sentRef.current = true;
    };
    send();
    if (!itemId) return undefined;
    const timer = setInterval(() => send(), HEARTBEAT_INTERVAL_MS);
    const onPageHide = () => send(true);
    window.addEventListener('pagehide', onPageHide);
    return () => {
      clearInterval(timer);
      window.removeEventListener('pagehide', onPageHide);
    };
  }, [itemId, playing]);

  // Final "stopped" when the hook goes away (signed out, shell switch).
  useEffect(() => () => {
    if (sentRef.current && playerIdRef.current) {
      api.activity.heartbeat({
        playerId: playerIdRef.current, trackId: null, positionSec: 0, durationSec: null, paused: true, stopped: true,
      });
      sentRef.current = false;
    }
  }, []);
}

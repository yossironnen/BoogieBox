/**
 * Live Activity snapshot for admins: one `EventSource` on `/api/activity/events` (App calls this once and shares
 * the result with the nav badge, the sidebar indicator and the Activity page). When the stream fails it polls
 * `GET /api/activity` every 5 s and retries the stream every 30 s. Hidden tabs disconnect and catch up on return.
 */

import { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { ActivitySnapshot } from './activityTypes';

export type ActivityConnection = 'connecting' | 'live' | 'polling';

export const ACTIVITY_POLL_MS = 5_000;
export const ACTIVITY_SSE_RETRY_MS = 30_000;

export interface ActivityFeed {
  snapshot: ActivitySnapshot | null;
  connection: ActivityConnection;
  /** When the last snapshot arrived (ms since epoch). */
  updatedAt: number | null;
  /** Fetches a snapshot now (after an admin action). */
  refresh: () => void;
}

export function useActivityFeed(enabled: boolean): ActivityFeed {
  const [snapshot, setSnapshot] = useState<ActivitySnapshot | null>(null);
  const [connection, setConnection] = useState<ActivityConnection>('connecting');
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const refreshRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!enabled) {
      setSnapshot(null);
      setUpdatedAt(null);
      setConnection('connecting');
      refreshRef.current = () => {};
      return undefined;
    }
    let disposed = false;
    let source: EventSource | null = null;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;

    const accept = (next: ActivitySnapshot) => {
      if (disposed) return;
      setSnapshot(next);
      setUpdatedAt(Date.now());
    };
    const fetchOnce = () => {
      api.activity.snapshot().then(accept).catch(() => {});
    };
    refreshRef.current = fetchOnce;

    const stopPolling = () => {
      if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
      if (retryTimer) { clearTimeout(retryTimer); retryTimer = null; }
    };
    const closeSource = () => {
      source?.close();
      source = null;
    };
    const startPolling = () => {
      if (disposed || pollTimer) return;
      setConnection('polling');
      fetchOnce();
      pollTimer = setInterval(fetchOnce, ACTIVITY_POLL_MS);
      retryTimer = setTimeout(() => {
        retryTimer = null;
        openSource();
      }, ACTIVITY_SSE_RETRY_MS);
    };
    const openSource = () => {
      if (disposed || source) return;
      if (typeof EventSource === 'undefined') {
        startPolling();
        return;
      }
      const next = new EventSource(api.activity.eventsUrl(), { withCredentials: true });
      source = next;
      next.addEventListener('snapshot', (message) => {
        try {
          accept(JSON.parse((message as MessageEvent<string>).data) as ActivitySnapshot);
          stopPolling();
          setConnection('live');
        } catch {
          // Ignore malformed frames.
        }
      });
      next.onerror = () => {
        if (source !== next) return;
        // The browser retries on its own while CONNECTING; a refused stream (CLOSED) falls back to polling.
        if (next.readyState === EventSource.CLOSED) {
          source = null;
          startPolling();
        } else {
          setConnection('connecting');
        }
      };
    };

    const onVisibility = () => {
      if (document.hidden) {
        closeSource();
        stopPolling();
        return;
      }
      fetchOnce();
      openSource();
    };

    setConnection('connecting');
    fetchOnce();
    openSource();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      disposed = true;
      document.removeEventListener('visibilitychange', onVisibility);
      closeSource();
      stopPolling();
      refreshRef.current = () => {};
    };
  }, [enabled]);

  return {
    snapshot,
    connection,
    updatedAt,
    refresh: () => refreshRef.current(),
  };
}

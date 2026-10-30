/**
 * Keeps smart playlist cards current: cards on screen ask the server for their summary one at a time, each at
 * most once a minute while the page is visible. The playlist list itself only returns stored summaries.
 */

import { useCallback, useEffect, useRef } from 'react';
import { api } from '../api';
import type { Playlist } from '../types';
import { SMART_RECHECK_MS } from './useSmartRecheck';

/** Returns a ref for each smart card; the card also needs `data-smart-card={playlist.id}`. */
export function useSmartCardSummaries(onSummary: (playlist: Playlist) => void): (el: HTMLElement | null) => (() => void) | void {
  const onSummaryRef = useRef(onSummary);
  onSummaryRef.current = onSummary;
  const stateRef = useRef({
    visible: new Set<string>(),
    checkedAt: new Map<string, number>(),
    running: false,
    mounted: true,
    observer: null as IntersectionObserver | null,
  });

  const pump = useCallback(() => {
    const s = stateRef.current;
    if (s.running || !s.mounted || document.visibilityState !== 'visible') return;
    const now = Date.now();
    // A second of slack so timer jitter doesn't skip a whole interval.
    const next = [...s.visible].find(id => now - (s.checkedAt.get(id) ?? -Infinity) >= SMART_RECHECK_MS - 1_000);
    if (!next) return;
    s.running = true;
    s.checkedAt.set(next, now);
    api.playlists.summary(next)
      .then(row => { if (s.mounted) onSummaryRef.current(row); })
      .catch(() => {})
      .finally(() => { s.running = false; pump(); });
  }, []);

  const observer = useCallback((): IntersectionObserver | null => {
    const s = stateRef.current;
    if (!s.observer && typeof IntersectionObserver !== 'undefined') {
      s.observer = new IntersectionObserver(entries => {
        for (const entry of entries) {
          const id = (entry.target as HTMLElement).dataset.smartCard;
          if (!id) continue;
          if (entry.isIntersecting) s.visible.add(id); else s.visible.delete(id);
        }
        pump();
      });
    }
    return s.observer;
  }, [pump]);

  useEffect(() => {
    const s = stateRef.current;
    s.mounted = true;
    const id = window.setInterval(pump, SMART_RECHECK_MS);
    document.addEventListener('visibilitychange', pump);
    return () => {
      s.mounted = false;
      window.clearInterval(id);
      document.removeEventListener('visibilitychange', pump);
      // Each card's ref cleanup unobserves it; the observer stays for a StrictMode re-mount.
    };
  }, [pump]);

  return useCallback((el: HTMLElement | null) => {
    const id = el?.dataset.smartCard;
    if (!el || !id) return undefined;
    const s = stateRef.current;
    const io = observer();
    if (io) io.observe(el);
    else { s.visible.add(id); pump(); }
    return () => {
      io?.unobserve(el);
      s.visible.delete(id);
    };
  }, [observer, pump]);
}

/** Replaces a playlist in a list only when its card would change. */
export function withSummary(list: Playlist[], row: Playlist): Playlist[] {
  const i = list.findIndex(p => p.id === row.id);
  if (i < 0 || JSON.stringify(list[i]) === JSON.stringify(row)) return list;
  const next = list.slice();
  next[i] = row;
  return next;
}

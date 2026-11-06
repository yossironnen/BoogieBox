/**
 * A smart playlist's steering pills: loads them, applies taps (the pill shows at once, and a failure turns it
 * back), and remembers whether a tap has left a playing queue out of date. Shared by desktop and mobile.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { steerOnly, togglePill, type SmartSteer, type SteerView } from '../smartSteer';
import type { EntityId } from '../entityId';
import type { PlaylistSnapshot } from '../types';

export interface SteerState {
  /** Null until loaded, and when pills can't be read. */
  view: SteerView | null;
  /** Pills on right now, including one still being saved. */
  active: SmartSteer[];
  /** Tracks in the playlist only because of the pills. */
  addedIds: Set<string>;
  busy: boolean;
  /** The last tap failed and was undone. */
  failed: boolean;
  /** A tap changed the tracks since the queue prompt was last answered. */
  queueStale: boolean;
  toggle: (pill: SmartSteer) => void;
  reset: () => void;
  retry: () => void;
  answerQueue: () => void;
  /** Reads the pills again, quietly. */
  reload: () => void;
}

const NONE: SmartSteer[] = [];

export function useSteer(playlistId: EntityId | null, enabled: boolean, onSnapshot: (snapshot: PlaylistSnapshot) => void): SteerState {
  const [view, setView] = useState<SteerView | null>(null);
  const [optimistic, setOptimistic] = useState<SmartSteer[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [queueStale, setQueueStale] = useState(false);
  const seqRef = useRef(0);
  const lastTryRef = useRef<SmartSteer[] | null>(null);
  const onSnapshotRef = useRef(onSnapshot);
  useEffect(() => { onSnapshotRef.current = onSnapshot; }, [onSnapshot]);

  const id = enabled ? playlistId : null;

  const read = useCallback(async () => {
    if (id === null) return;
    const seq = ++seqRef.current;
    try {
      const next = await api.playlists.steer(id);
      if (seq === seqRef.current) setView(next);
    } catch {
      // Pills are an extra: without them the playlist still works, and the next read may succeed.
    }
  }, [id]);

  useEffect(() => {
    seqRef.current += 1;
    setView(null);
    setOptimistic(null);
    setFailed(false);
    setQueueStale(false);
    lastTryRef.current = null;
    void read();
  }, [read]);

  const send = useCallback(async (next: SmartSteer[]) => {
    if (id === null) return;
    const seq = ++seqRef.current;
    lastTryRef.current = next;
    setOptimistic(next);
    setFailed(false);
    try {
      const update = await api.playlists.setSteer(id, next);
      if (seq !== seqRef.current) return;
      lastTryRef.current = null;
      setView(update.steer);
      setOptimistic(null);
      setQueueStale(true);
      onSnapshotRef.current(update.snapshot);
    } catch {
      if (seq !== seqRef.current) return;
      setOptimistic(null);
      setFailed(true);
    }
  }, [id]);

  const busy = optimistic !== null;
  // A read landing during a tap would drop the tap's answer, so reads wait.
  const busyRef = useRef(busy);
  busyRef.current = busy;
  const reload = useCallback(() => { if (!busyRef.current) void read(); }, [read]);
  const active = optimistic ?? view?.active ?? NONE;

  const toggle = useCallback((pill: SmartSteer) => {
    if (busy) return;
    const next = togglePill(active, pill);
    if (next !== active) void send(next.map(steerOnly));
  }, [active, busy, send]);

  const reset = useCallback(() => {
    if (!busy && active.length > 0) void send([]);
  }, [active.length, busy, send]);

  const retry = useCallback(() => {
    if (!busy && lastTryRef.current) void send(lastTryRef.current);
  }, [busy, send]);

  const answerQueue = useCallback(() => setQueueStale(false), []);

  // While saving, tracks from before the tap are still on screen, so their marks stay until the answer comes.
  const addedIds = new Set((view?.addedTrackIds ?? []).map(String));

  return { view, active, addedIds, busy, failed, queueStale, toggle, reset, retry, answerQueue, reload };
}

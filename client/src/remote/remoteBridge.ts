/**
 * Pure Play On glue: mirrors a server session as a `PlayerState`, and turns a `PlayerState` change made by any
 * existing surface (desktop Player, mobile Now Playing, mini player, App play actions) into server actions.
 */

import { api } from '../api';
import type { PlayerState } from '../components/Player';
import type { ClientEntityId, QueueSource, Track } from '../types';
import type {
  RemoteCommand,
  RemoteItemRef,
  RemoteLoadBody,
  RemoteQueueEntry,
  RemoteQueueOp,
  RemoteSessionState,
  RemoteSessionStatus,
} from './remoteTypes';

const MIX_PREFIX = 'boogiemix:';
/** Position ticks are ignored this long after a local seek, so the bar doesn't bounce back (plan 8.7). */
export const SEEK_SETTLE_MS = 1500;

export type RemoteAction =
  | { kind: 'load'; body: RemoteLoadBody }
  | { kind: 'queue'; op: RemoteQueueOp }
  | { kind: 'command'; command: RemoteCommand };

/** A session snapshot plus the client-clock position sample used for interpolation. */
export interface RemoteSessionView {
  state: RemoteSessionState;
  positionSec: number;
  /** `Date.now()` when `positionSec` was sampled. */
  positionAt: number;
  ignoreTicksUntil: number;
}

export function isRemotePlayingStatus(status: RemoteSessionStatus | null | undefined): boolean {
  return status === 'playing' || status === 'loading';
}

/** The server item for a client track; synthetic `boogiemix:` tracks become mix items. */
export function trackToItemRef(track: Track): RemoteItemRef {
  const id = String(track.id);
  if (id.startsWith(MIX_PREFIX)) return { kind: 'mix', id: id.slice(MIX_PREFIX.length) };
  return track.radio_reason ? { kind: 'track', id, radioReason: track.radio_reason } : { kind: 'track', id };
}

function entryTrackId(entry: RemoteQueueEntry): string {
  return entry.kind === 'mix' ? `${MIX_PREFIX}${entry.id}` : entry.id;
}

function minimalTrack(entry: RemoteQueueEntry): Track {
  const isMix = entry.kind === 'mix';
  return {
    id: entryTrackId(entry),
    file_name: entry.title,
    file_size: null,
    format: null,
    duration: entry.duration,
    bitrate: null,
    sample_rate: null,
    channels: null,
    title: entry.title,
    artist: isMix ? 'BoogieBox BoogieMix' : entry.artist,
    album: isMix ? null : entry.album,
    library_name: null,
    track_number: null,
    disc_number: null,
    year: null,
    genre: null,
    composer: null,
    comment: null,
    bpm: null,
    album_id: isMix ? null : entry.albumId,
    scanned_at: '',
    ...(isMix ? { stream_url_override: api.boogiemix.playUrl(entry.id) } : {}),
  };
}

/**
 * Track objects for a session queue. Known tracks (the ones this client sent) keep their full metadata; objects are
 * reused per `qid` so `currentTrack` stays referentially stable across snapshots (no waveform/lyrics refetch).
 */
export function sessionQueueTracks(
  queue: RemoteQueueEntry[],
  known: ReadonlyMap<string, Track>,
  previous: ReadonlyMap<number, Track>,
): Track[] {
  return queue.map((entry) => {
    const id = entryTrackId(entry);
    const reused = previous.get(entry.qid);
    if (reused && String(reused.id) === id) return reused;
    const base = known.get(id) ?? minimalTrack(entry);
    const reason = entry.kind === 'track' ? entry.radioReason ?? base.radio_reason ?? null : null;
    const { startAtSec: _startAtSec, ...rest } = base;
    return { ...rest, radio_reason: reason, remoteQid: entry.qid };
  });
}

function asQueueSource(value: unknown): QueueSource | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const source = value as Partial<QueueSource>;
  return typeof source.type === 'string' && source.id != null ? source as QueueSource : undefined;
}

/** The `PlayerState` every surface renders while a renderer is the target. */
export function sessionToPlayerState(session: RemoteSessionState | null, tracks: Track[], playToken: number): PlayerState {
  if (!session) return { queue: [], currentIndex: 0, isPlaying: false, playToken };
  const index = session.queue.findIndex((entry) => entry.qid === session.currentQid);
  return {
    queue: tracks,
    currentIndex: Math.max(0, index),
    isPlaying: isRemotePlayingStatus(session.status),
    playToken,
    queueSource: asQueueSource(session.queueSource),
  };
}

/**
 * Moves that turn `current` into `desired` (same qids) with the server's `move_to` semantics. Entries on the longest
 * increasing run stay put; each other entry is moved right after its predecessor in `desired`.
 */
export function planMoves(current: number[], desired: number[]): Array<{ qid: number; toIndex: number }> {
  const position = new Map(current.map((qid, i) => [qid, i]));
  const ranks = desired.map((qid) => position.get(qid) ?? -1);
  // Longest increasing subsequence over `ranks` (patience sorting with back-links).
  const tails: number[] = [];
  const back: number[] = new Array(ranks.length).fill(-1);
  for (let i = 0; i < ranks.length; i++) {
    let lo = 0;
    let hi = tails.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (ranks[tails[mid]] < ranks[i]) lo = mid + 1; else hi = mid;
    }
    if (lo > 0) back[i] = tails[lo - 1];
    tails[lo] = i;
  }
  const stay = new Set<number>();
  for (let i = tails.length ? tails[tails.length - 1] : -1; i >= 0; i = back[i]) stay.add(desired[i]);

  const order = [...current];
  const moves: Array<{ qid: number; toIndex: number }> = [];
  desired.forEach((qid, k) => {
    if (stay.has(qid)) return;
    order.splice(order.indexOf(qid), 1);
    const toIndex = k === 0 ? 0 : order.indexOf(desired[k - 1]) + 1;
    order.splice(toIndex, 0, qid);
    moves.push({ qid, toIndex });
  });
  return moves;
}

function startSeconds(state: PlayerState, track: Track | undefined): number | undefined {
  if (!track) return undefined;
  const resume = state.resumeAt;
  if (resume && resume.playToken === state.playToken && String(resume.trackId) === String(track.id)) return resume.sec;
  return track.startAtSec != null && track.startAtSec > 0 ? track.startAtSec : undefined;
}

/** Server actions equivalent to a `prev → next` PlayerState change, in the order they must run. */
export function translatePlayerStateChange(prev: PlayerState, next: PlayerState): RemoteAction[] {
  if (next === prev) return [];
  if (!next.queue.length) return prev.queue.length ? [{ kind: 'queue', op: { op: 'clear' } }] : [];

  const prevQids = prev.queue.map((t) => t.remoteQid).filter((q): q is number => q != null);
  const prevSet = new Set(prevQids);
  const seen = new Set<number>();
  const knownQid = next.queue.map((t) => {
    const qid = t.remoteQid;
    if (qid == null || !prevSet.has(qid) || seen.has(qid)) return null;
    seen.add(qid);
    return qid;
  });
  const nextIndex = Math.min(Math.max(0, next.currentIndex), next.queue.length - 1);

  // Nothing carried over: a brand-new queue.
  if (seen.size === 0) {
    const start = next.queue[nextIndex];
    const actions: RemoteAction[] = [{
      kind: 'load',
      body: {
        items: next.queue.map(trackToItemRef),
        startIndex: nextIndex,
        startAtSec: startSeconds(next, start),
        queueSource: next.queueSource,
      },
    }];
    if (!next.isPlaying) actions.push({ kind: 'command', command: { cmd: 'pause' } });
    return actions;
  }

  const actions: RemoteAction[] = [];
  const removed = prevQids.filter((qid) => !seen.has(qid));
  for (const qid of removed) actions.push({ kind: 'queue', op: { op: 'remove', qid } });

  const desired = knownQid.filter((q): q is number => q != null);
  const kept = prevQids.filter((qid) => seen.has(qid));
  for (const move of planMoves(kept, desired)) actions.push({ kind: 'queue', op: { op: 'move', ...move } });

  const freshIndexes = knownQid.flatMap((q, i) => (q == null ? [i] : []));
  if (freshIndexes.length) {
    const first = freshIndexes[0];
    const contiguous = freshIndexes.every((i, k) => i === first + k);
    const lastKnown = knownQid.reduce<number>((last, q, i) => (q != null ? i : last), -1);
    const afterCurrent = contiguous && first === nextIndex + 1 && knownQid[nextIndex] != null;
    const mode = !afterCurrent || first > lastKnown ? 'end' : 'next';
    actions.push({ kind: 'queue', op: { op: 'enqueue', items: freshIndexes.map((i) => trackToItemRef(next.queue[i])), mode } });
  }

  const prevCurrent = prev.queue[prev.currentIndex]?.remoteQid ?? null;
  const nextCurrent = knownQid[nextIndex];
  let expectPlaying = prev.isPlaying;
  if (prevCurrent != null && removed.includes(prevCurrent)) {
    // The server moves on to the following entry by itself and keeps playing.
    expectPlaying = true;
    if (nextCurrent != null && next.playToken > prev.playToken) {
      actions.push({ kind: 'command', command: { cmd: 'jump', value: nextCurrent } });
    }
  } else if (nextCurrent != null && (nextCurrent !== prevCurrent || next.playToken > prev.playToken)) {
    actions.push({ kind: 'command', command: { cmd: 'jump', value: nextCurrent } });
    expectPlaying = true;
  }
  if (next.isPlaying !== expectPlaying) {
    actions.push({ kind: 'command', command: { cmd: next.isPlaying ? 'play' : 'pause' } });
  }
  return actions;
}

/** Applies a full snapshot, ignoring stale revisions and keeping a fresh local seek. */
export function applySessionSnapshot(view: RemoteSessionView | null, state: RemoteSessionState, now: number): RemoteSessionView {
  if (view && view.state.udn === state.udn && state.revision < view.state.revision) return view;
  const settling = view != null && view.state.udn === state.udn && now < view.ignoreTicksUntil;
  return {
    state,
    positionSec: settling ? view.positionSec : state.positionSec,
    positionAt: settling ? view.positionAt : now,
    ignoreTicksUntil: settling ? view.ignoreTicksUntil : 0,
  };
}

/** Applies a light position tick. */
export function applyPositionTick(
  view: RemoteSessionView | null,
  tick: { udn: string; status: RemoteSessionStatus; positionSec: number; durationSec: number | null; sampledAtMs: number },
  now: number,
): RemoteSessionView | null {
  if (!view || view.state.udn !== tick.udn || tick.sampledAtMs < view.state.sampledAtMs) return view;
  const state = { ...view.state, status: tick.status, durationSec: tick.durationSec, sampledAtMs: tick.sampledAtMs };
  if (now < view.ignoreTicksUntil) return { ...view, state };
  return { ...view, state: { ...state, positionSec: tick.positionSec }, positionSec: tick.positionSec, positionAt: now };
}

/** Records a local seek so ticks don't pull the bar back while the device settles. */
export function applyLocalSeek(view: RemoteSessionView | null, sec: number, now: number): RemoteSessionView | null {
  if (!view) return view;
  return { ...view, positionSec: sec, positionAt: now, ignoreTicksUntil: now + SEEK_SETTLE_MS };
}

/** Current position: extrapolated while playing, clamped to the duration. */
export function interpolatePosition(view: RemoteSessionView | null, now: number, fallbackDuration = 0): number {
  if (!view) return 0;
  const duration = view.state.durationSec ?? fallbackDuration;
  const elapsed = view.state.status === 'playing' ? Math.max(0, now - view.positionAt) / 1000 : 0;
  const position = view.positionSec + elapsed;
  return duration > 0 ? Math.min(position, duration) : position;
}

/** A local `PlayerState` that continues the session here (renderer → this browser handoff). */
export function sessionToLocalState(
  session: RemoteSessionState,
  tracks: Track[],
  positionSec: number,
  previousToken: number,
): PlayerState {
  const queue = tracks.map(({ remoteQid: _remoteQid, ...track }) => track as Track);
  const index = Math.max(0, session.queue.findIndex((entry) => entry.qid === session.currentQid));
  const playToken = previousToken + 1;
  const current = queue[index];
  return {
    queue,
    currentIndex: index,
    isPlaying: isRemotePlayingStatus(session.status),
    playToken,
    queueSource: asQueueSource(session.queueSource),
    ...(current && positionSec > 1 ? { resumeAt: { playToken, trackId: current.id as ClientEntityId, sec: positionSec } } : {}),
  };
}

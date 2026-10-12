import { describe, expect, it } from 'vitest';
import type { PlayerState } from '../components/Player';
import type { Track } from '../types';
import {
  applyLocalSeek,
  applyPositionTick,
  applySessionSnapshot,
  interpolatePosition,
  planMoves,
  sessionQueueTracks,
  sessionToLocalState,
  sessionToPlayerState,
  trackToItemRef,
  translatePlayerStateChange,
} from './remoteBridge';
import type { RemoteQueueEntry, RemoteSessionState } from './remoteTypes';

function track(id: string, extra: Partial<Track> = {}): Track {
  return {
    id, file_name: `${id}.flac`, file_size: 1, format: 'flac', duration: 200, bitrate: null, sample_rate: null,
    channels: null, title: `Title ${id}`, artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1,
    disc_number: 1, year: 2020, genre: null, composer: null, comment: null, bpm: null, album_id: 'al1',
    scanned_at: '2026-01-01', ...extra,
  };
}

function entry(qid: number, id: string): RemoteQueueEntry {
  return { qid, kind: 'track', id, title: `Title ${id}`, artist: 'Artist', album: 'Album', albumId: 'al1', duration: 200 };
}

function session(over: Partial<RemoteSessionState> = {}): RemoteSessionState {
  return {
    udn: 'uuid:wiim', rendererName: 'WiiM', status: 'playing', queue: [entry(1, 'a'), entry(2, 'b'), entry(3, 'c')],
    currentQid: 1, positionSec: 10, durationSec: 200, sampledAtMs: 1000, volume: 40, muted: false, shuffle: false,
    repeat: 'off', queueSource: { type: 'album', id: 'al1' }, ownerUserId: 'u1', ownerName: 'Ann',
    lastControllerUserId: 'u1', canSeek: true, canSetVolume: true, revision: 5, error: null, ...over,
  };
}

/** Remote-mode state with qids 1..n on tracks a, b, c… */
function mirrored(ids: string[], currentIndex = 0, isPlaying = true): PlayerState {
  return { queue: ids.map((id, i) => track(id, { remoteQid: i + 1 })), currentIndex, isPlaying, playToken: 0 };
}

describe('trackToItemRef', () => {
  it('maps library tracks, radio reasons and synthetic BoogieMix tracks', () => {
    expect(trackToItemRef(track('7'))).toEqual({ kind: 'track', id: '7' });
    const reason = { kind: 'similar' as const, label: 'Similar' } as unknown as Track['radio_reason'];
    expect(trackToItemRef(track('7', { radio_reason: reason }))).toEqual({ kind: 'track', id: '7', radioReason: reason });
    expect(trackToItemRef(track('boogiemix:42'))).toEqual({ kind: 'mix', id: '42' });
  });
});

describe('sessionQueueTracks / sessionToPlayerState', () => {
  it('keeps known metadata, tags qids, strips startAtSec and reuses objects per qid', () => {
    const known = new Map([['a', track('a', { genre: 'House', startAtSec: 30 })]]);
    const first = sessionQueueTracks(session().queue, known, new Map());
    expect(first[0]).toMatchObject({ id: 'a', genre: 'House', remoteQid: 1 });
    expect(first[0].startAtSec).toBeUndefined();
    expect(first[1]).toMatchObject({ id: 'b', title: 'Title b', remoteQid: 2, album_id: 'al1' });
    const again = sessionQueueTracks(session().queue, known, new Map(first.map((t) => [t.remoteQid!, t])));
    expect(again[0]).toBe(first[0]);
  });

  it('builds playable minimal tracks for mixes', () => {
    const [mix] = sessionQueueTracks([{ qid: 9, kind: 'mix', id: '42', title: 'Mix', duration: 3600 }], new Map(), new Map());
    expect(mix).toMatchObject({ id: 'boogiemix:42', title: 'Mix', artist: 'BoogieBox BoogieMix', remoteQid: 9 });
    expect(mix.stream_url_override).toBe('/api/boogiemix/outputs/42/play');
  });

  it('derives index, play state and queue source', () => {
    const s = session({ currentQid: 2, status: 'loading' });
    const state = sessionToPlayerState(s, sessionQueueTracks(s.queue, new Map(), new Map()), 3);
    expect(state).toMatchObject({ currentIndex: 1, isPlaying: true, playToken: 3, queueSource: { type: 'album', id: 'al1' } });
    expect(sessionToPlayerState(null, [], 3)).toEqual({ queue: [], currentIndex: 0, isPlaying: false, playToken: 3 });
    expect(sessionToPlayerState(session({ status: 'paused', queueSource: 'junk' }), [], 0)).toMatchObject({ isPlaying: false, queueSource: undefined });
  });
});

describe('planMoves', () => {
  /** The server's `move_to`: remove, then insert at the clamped index. */
  function apply(order: number[], moves: Array<{ qid: number; toIndex: number }>) {
    const out = [...order];
    for (const { qid, toIndex } of moves) {
      out.splice(out.indexOf(qid), 1);
      out.splice(Math.min(toIndex, out.length), 0, qid);
    }
    return out;
  }

  it('moves a single dragged item with one move', () => {
    expect(planMoves([1, 2, 3, 4], [2, 3, 4, 1])).toEqual([{ qid: 1, toIndex: 3 }]);
    expect(planMoves([1, 2, 3, 4], [4, 1, 2, 3])).toEqual([{ qid: 4, toIndex: 0 }]);
    expect(planMoves([1, 2, 3], [1, 2, 3])).toEqual([]);
  });

  it('reaches any permutation', () => {
    let seed = 7;
    const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
    for (let round = 0; round < 300; round++) {
      const n = 1 + Math.floor(rand() * 9);
      const current = Array.from({ length: n }, (_, i) => i + 1);
      const desired = [...current];
      for (let i = n - 1; i > 0; i--) { const j = Math.floor(rand() * (i + 1)); [desired[i], desired[j]] = [desired[j], desired[i]]; }
      expect(apply(current, planMoves(current, desired))).toEqual(desired);
    }
  });
});

describe('translatePlayerStateChange', () => {
  it('loads a brand-new queue at the chosen index with the queue source', () => {
    const prev = mirrored(['a', 'b']);
    const next: PlayerState = { queue: [track('x'), track('y', { startAtSec: 42 })], currentIndex: 1, isPlaying: true, playToken: 1, queueSource: { type: 'album', id: 'al9' } };
    expect(translatePlayerStateChange(prev, next)).toEqual([{
      kind: 'load',
      body: { items: [{ kind: 'track', id: 'x' }, { kind: 'track', id: 'y' }], startIndex: 1, startAtSec: 42, queueSource: { type: 'album', id: 'al9' } },
    }]);
  });

  it('loads then pauses when adding to an empty queue', () => {
    const empty: PlayerState = { queue: [], currentIndex: 0, isPlaying: false, playToken: 0 };
    const actions = translatePlayerStateChange(empty, { ...empty, queue: [track('x')] });
    expect(actions.map((a) => a.kind)).toEqual(['load', 'command']);
    expect(actions[1]).toEqual({ kind: 'command', command: { cmd: 'pause' } });
  });

  it('enqueues appended tracks at the end and inserted-after-current tracks next', () => {
    const prev = mirrored(['a', 'b', 'c']);
    expect(translatePlayerStateChange(prev, { ...prev, queue: [...prev.queue, track('x')] })).toEqual([
      { kind: 'queue', op: { op: 'enqueue', items: [{ kind: 'track', id: 'x' }], mode: 'end' } },
    ]);
    const inserted = [prev.queue[0], track('x'), track('y'), ...prev.queue.slice(1)];
    expect(translatePlayerStateChange(prev, { ...prev, queue: inserted })).toEqual([
      { kind: 'queue', op: { op: 'enqueue', items: [{ kind: 'track', id: 'x' }, { kind: 'track', id: 'y' }], mode: 'next' } },
    ]);
  });

  it('removes an upcoming entry without touching playback', () => {
    const prev = mirrored(['a', 'b', 'c']);
    expect(translatePlayerStateChange(prev, { ...prev, queue: [prev.queue[0], prev.queue[2]] })).toEqual([
      { kind: 'queue', op: { op: 'remove', qid: 2 } },
    ]);
  });

  it('removing the current entry lets the server advance, pausing when the surface asked to stop', () => {
    const prev = mirrored(['a', 'b', 'c']);
    const queue = prev.queue.slice(1);
    expect(translatePlayerStateChange(prev, { ...prev, queue, isPlaying: true })).toEqual([
      { kind: 'queue', op: { op: 'remove', qid: 1 } },
    ]);
    expect(translatePlayerStateChange(prev, { ...prev, queue, isPlaying: false })).toEqual([
      { kind: 'queue', op: { op: 'remove', qid: 1 } },
      { kind: 'command', command: { cmd: 'pause' } },
    ]);
  });

  it('turns a drag reorder into one move and keeps the current entry', () => {
    const prev = mirrored(['a', 'b', 'c', 'd']);
    const [a, b, c, d] = prev.queue;
    expect(translatePlayerStateChange(prev, { ...prev, queue: [a, d, b, c] })).toEqual([
      { kind: 'queue', op: { op: 'move', qid: 4, toIndex: 1 } },
    ]);
  });

  it('jumps on queue selection and restarts on a replay of the same entry', () => {
    const prev = mirrored(['a', 'b', 'c']);
    expect(translatePlayerStateChange(prev, { ...prev, currentIndex: 2, playToken: 1 })).toEqual([
      { kind: 'command', command: { cmd: 'jump', value: 3 } },
    ]);
    expect(translatePlayerStateChange(prev, { ...prev, playToken: 1 })).toEqual([
      { kind: 'command', command: { cmd: 'jump', value: 1 } },
    ]);
    expect(translatePlayerStateChange(prev, { ...prev, currentIndex: 1, playToken: 1, isPlaying: false })).toEqual([
      { kind: 'command', command: { cmd: 'jump', value: 2 } },
      { kind: 'command', command: { cmd: 'pause' } },
    ]);
  });

  it('maps play/pause toggles and clearing', () => {
    const prev = mirrored(['a', 'b']);
    expect(translatePlayerStateChange(prev, { ...prev, isPlaying: false })).toEqual([{ kind: 'command', command: { cmd: 'pause' } }]);
    expect(translatePlayerStateChange({ ...prev, isPlaying: false }, prev)).toEqual([{ kind: 'command', command: { cmd: 'play' } }]);
    expect(translatePlayerStateChange(prev, { ...prev, queue: [] })).toEqual([{ kind: 'queue', op: { op: 'clear' } }]);
    expect(translatePlayerStateChange(prev, prev)).toEqual([]);
  });
});

describe('session view reducers', () => {
  it('drops stale snapshots and keeps a fresh local seek', () => {
    const view = applySessionSnapshot(null, session(), 0);
    expect(applySessionSnapshot(view, session({ revision: 4, positionSec: 99 }), 10)).toBe(view);
    const seeked = applyLocalSeek(view, 120, 100)!;
    const during = applySessionSnapshot(seeked, session({ revision: 6, positionSec: 11 }), 500);
    expect(during.positionSec).toBe(120);
    const after = applySessionSnapshot(during, session({ revision: 7, positionSec: 121 }), 2000);
    expect(after.positionSec).toBe(121);
  });

  it('applies ticks, ignoring old samples and positions while settling', () => {
    const view = applySessionSnapshot(null, session(), 0);
    const tick = { udn: 'uuid:wiim', status: 'paused' as const, positionSec: 30, durationSec: 200, sampledAtMs: 2000 };
    expect(applyPositionTick(view, { ...tick, sampledAtMs: 500 }, 10)).toBe(view);
    expect(applyPositionTick(view, { ...tick, udn: 'other' }, 10)).toBe(view);
    expect(applyPositionTick(view, tick, 10)).toMatchObject({ positionSec: 30, positionAt: 10, state: { status: 'paused' } });
    const settling = applyLocalSeek(view, 150, 10)!;
    expect(applyPositionTick(settling, tick, 20)).toMatchObject({ positionSec: 150, state: { status: 'paused' } });
  });

  it('interpolates only while playing and clamps to the duration', () => {
    const view = applySessionSnapshot(null, session({ positionSec: 10, durationSec: 12 }), 1000);
    expect(interpolatePosition(view, 2000)).toBe(11);
    expect(interpolatePosition(view, 9000)).toBe(12);
    const paused = applySessionSnapshot(null, session({ status: 'paused', positionSec: 10 }), 1000);
    expect(interpolatePosition(paused, 9000)).toBe(10);
    expect(interpolatePosition(null, 0)).toBe(0);
  });
});

describe('sessionToLocalState', () => {
  it('continues the session locally from the current position', () => {
    const s = session({ currentQid: 2 });
    const tracks = sessionQueueTracks(s.queue, new Map(), new Map());
    const local = sessionToLocalState(s, tracks, 64.5, 7);
    expect(local.queue.every((t) => t.remoteQid === undefined)).toBe(true);
    expect(local).toMatchObject({ currentIndex: 1, isPlaying: true, playToken: 8, resumeAt: { playToken: 8, trackId: 'b', sec: 64.5 } });
    expect(sessionToLocalState(session({ status: 'paused' }), tracks, 0, 0).resumeAt).toBeUndefined();
  });
});

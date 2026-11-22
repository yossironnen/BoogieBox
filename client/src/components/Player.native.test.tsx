import { act, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { NativePlayerOptions, NativePlayerView } from '../native/useNativePlayer';
import type { Track } from '../types';
import Player, { type PlayerState } from './Player';

// The desktop engine is the output ("This PC"); the hook that drives it is replaced by a stand-in the test moves.
const engine = vi.hoisted(() => ({
  options: null as NativePlayerOptions | null,
  view: {
    status: 'idle',
    positionSec: 0,
    durationSec: null,
    badge: null,
    error: null,
    trackId: null,
    seek: () => {},
  } as NativePlayerView,
  rerender: null as (() => void) | null,
}));

vi.mock('../native/useNativePlayer', async (importOriginal) => {
  const real = await importOriginal<typeof import('../native/useNativePlayer')>();
  const React = await import('react');
  return {
    ...real,
    useNativePlayer: (options: NativePlayerOptions) => {
      const [, force] = React.useState(0);
      engine.options = options;
      engine.rerender = () => force((n) => n + 1);
      return engine.view;
    },
  };
});

vi.mock('../remote/usePlayOn', async (importOriginal) => {
  const real = await importOriginal<typeof import('../remote/usePlayOn')>();
  const playOn = { target: { kind: 'native' }, notify: vi.fn(), available: false, remoteActions: null, notice: null };
  return { ...real, usePlayOnContext: () => playOn };
});

vi.mock('../audio/VinylFxEngine', () => ({ playNeedleDrop: vi.fn(), preloadVinylFx: vi.fn() }));

function track(id: string, overrides: Record<string, unknown> = {}): Track {
  return {
    id,
    file_path: `D:\\Music\\${id}.flac`,
    file_name: `${id}.flac`,
    format: 'flac',
    duration: 300,
    title: `Track ${id}`,
    artist: 'Artist',
    album: 'Album',
    album_id: 'album-1',
    ...overrides,
  } as unknown as Track;
}

/** The engine reports a new state (as its `state` events do). */
function engineAt(view: Partial<NativePlayerView>) {
  act(() => {
    engine.view = { ...engine.view, ...view };
    engine.rerender?.();
  });
}

const playlist = { type: 'playlist', id: 'pl-1', rememberProgress: true } as PlayerState['queueSource'];

describe('Player on This PC with a remember-progress playlist', () => {
  beforeEach(() => {
    engine.options = null;
    engine.view = { ...engine.view, status: 'idle', positionSec: 0, trackId: null };
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
    vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({});
    vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.crossfade, 'config').mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    vi.spyOn(api, 'trackWaveform').mockResolvedValue({ status: 'missing', waveform: null } as any);
    vi.spyOn(api, 'trackSonicFingerprint').mockResolvedValue(null);
    vi.spyOn(api.playlists, 'saveTrackProgress').mockResolvedValue({ ok: true });
    vi.spyOn(api, 'trackLyrics').mockResolvedValue({ lyrics: '', source: 'cache', synced: [] });
    vi.spyOn(api, 'trackEqProfile').mockResolvedValue({ eqProfile: 'Rock', source: 'genre' });
    vi.spyOn(api.savedEq, 'get').mockResolvedValue({ scope: null, bands: null, hasTrack: false, hasAlbum: false, albumId: 'album-1' });
    vi.spyOn(api.auth, 'sessionValid').mockResolvedValue(true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function renderPlayer(state: PlayerState) {
    const onStateChange = vi.fn();
    const utils = render(<Player state={state} onStateChange={onStateChange} ffmpegAvailable />);
    return { ...utils, onStateChange };
  }

  it('starts a song where the playlist left it', () => {
    renderPlayer({
      queue: [track('a', { progress_seconds: 95 }), track('b', { progress_seconds: 2 }), track('c')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
      queueSource: playlist,
    });
    const startAt = engine.options!.startAt!;
    expect(startAt(track('a', { progress_seconds: 95 }))).toBe(95);
    expect(startAt(track('b', { progress_seconds: 2 }))).toBeNull();
    expect(startAt(track('c'))).toBeNull();
  });

  it('does not resume outside a remember-progress playlist', () => {
    renderPlayer({ queue: [track('a', { progress_seconds: 95 })], currentIndex: 0, isPlaying: true, playToken: 1 });
    expect(engine.options!.startAt!(track('a', { progress_seconds: 95 }))).toBeNull();
  });

  it('saves the spot while playing, on pause, and clears it when a song ends', async () => {
    const state: PlayerState = {
      queue: [track('a'), track('b')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
      queueSource: playlist,
    };
    const { rerender, onStateChange } = renderPlayer(state);
    const saves = vi.mocked(api.playlists.saveTrackProgress);

    // Not yet playing this song: nothing saved.
    engineAt({ status: 'loading', positionSec: 40, trackId: null });
    expect(saves).not.toHaveBeenCalled();

    engineAt({ status: 'playing', positionSec: 40, trackId: 'a' });
    await waitFor(() => expect(saves).toHaveBeenCalledWith('pl-1', 'a', 40));
    engineAt({ positionSec: 42 });
    expect(saves).toHaveBeenCalledTimes(1);
    engineAt({ positionSec: 46 });
    expect(saves).toHaveBeenLastCalledWith('pl-1', 'a', 46);

    // Paused: the spot is saved again.
    saves.mockClear();
    engineAt({ positionSec: 47 });
    rerender(<Player state={{ ...state, isPlaying: false }} onStateChange={onStateChange} ffmpegAvailable />);
    expect(saves).toHaveBeenCalledWith('pl-1', 'a', 47);

    // The engine moved on to the next song by itself: "a" finished, so its spot is cleared.
    saves.mockClear();
    act(() => engine.options!.onAdvance(1));
    expect(saves).toHaveBeenCalledWith('pl-1', 'a', 0);
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));
  });

  it('never files the last song\'s spot under the next one', () => {
    const state: PlayerState = {
      queue: [track('a'), track('b')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
      queueSource: playlist,
    };
    const { rerender, onStateChange } = renderPlayer(state);
    engineAt({ status: 'playing', positionSec: 120, trackId: 'a' });
    const saves = vi.mocked(api.playlists.saveTrackProgress);
    saves.mockClear();
    // The Player moves to "b" while the engine still reports "a" at 130 s.
    rerender(<Player state={{ ...state, currentIndex: 1, playToken: 2 }} onStateChange={onStateChange} ffmpegAvailable />);
    engineAt({ positionSec: 130 });
    expect(saves.mock.calls.filter(([, id]) => id === 'b')).toEqual([]);
  });
});

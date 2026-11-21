import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Track } from '../types';
import type { NativeMediaAction, NativeNowPlaying } from './nativeTypes';

const fake = vi.hoisted(() => ({
  shown: [] as Array<NativeNowPlaying | null>,
  handler: null as ((a: NativeMediaAction) => void) | null,
}));

vi.mock('../platform', () => ({
  platform: {
    isDesktop: true,
    audio: {
      nowPlaying: vi.fn(async (now: NativeNowPlaying) => { fake.shown.push(now); }),
      clearNowPlaying: vi.fn(async () => { fake.shown.push(null); }),
      onMediaControl: vi.fn(async (h: (a: NativeMediaAction) => void) => { fake.handler = h; return () => { fake.handler = null; }; }),
    },
  },
}));

vi.mock('../api', () => ({ api: { albumArtUrl: (id: string, size: number) => `/api/albums/${id}/art?size=${size}` } }));

import { nowPlayingFor, runMediaAction, useNativeMediaControls, type NativeMediaActions, type NativeMediaOptions } from './useNativeMediaControls';

const track = (id: string, extra: Partial<Track> = {}): Track =>
  ({ id, title: `Song ${id}`, artist: 'Enigma', album: 'MCMXC a.D.', album_id: 'al1', duration: 269, ...extra } as unknown as Track);

function actions(): NativeMediaActions {
  return { play: vi.fn(), pause: vi.fn(), toggle: vi.fn(), stop: vi.fn(), previous: vi.fn(), next: vi.fn(), seekTo: vi.fn() };
}

describe('Windows media controls for This PC', () => {
  beforeEach(() => {
    fake.shown = [];
    fake.handler = null;
  });

  it('describes the song for Windows', () => {
    expect(nowPlayingFor(track('a'), false)).toEqual({
      title: 'Song a',
      artist: 'Enigma',
      album: 'MCMXC a.D.',
      artUrl: `${window.location.origin}/api/albums/al1/art?size=300`,
      durationSec: 269,
      live: false,
    });
    const radio = nowPlayingFor(track('r', { album_id: null, album: null } as Partial<Track>), true, null);
    expect(radio).toMatchObject({ artUrl: null, durationSec: null, live: true, album: null });
    expect(nowPlayingFor(track('m'), false, 'BoogieMix').album).toBe('BoogieMix');
  });

  it('runs each button, and seeks only when it can', () => {
    const a = actions();
    for (const action of ['play', 'pause', 'toggle', 'stop', 'next', 'previous'] as const) runMediaAction({ action }, a, true);
    expect([a.play, a.pause, a.toggle, a.stop, a.next, a.previous].every((f) => (f as ReturnType<typeof vi.fn>).mock.calls.length === 1)).toBe(true);
    runMediaAction({ action: 'seek', sec: 42.5 }, a, true);
    runMediaAction({ action: 'seek', sec: -3 }, a, true);
    runMediaAction({ action: 'seek', sec: 10 }, a, false);
    runMediaAction({ action: 'seek' }, a, true);
    expect(a.seekTo).toHaveBeenCalledTimes(2);
    expect(a.seekTo).toHaveBeenNthCalledWith(1, 42.5);
    expect(a.seekTo).toHaveBeenNthCalledWith(2, 0);
  });

  it('shows the song while This PC plays and clears it when the output changes', async () => {
    const a = actions();
    const initial: NativeMediaOptions = { enabled: true, track: track('a'), live: false };
    const hook = renderHook((o: NativeMediaOptions) => useNativeMediaControls(o, a), { initialProps: initial });
    await act(async () => {});
    expect(fake.shown).toHaveLength(1);
    expect(fake.shown[0]?.title).toBe('Song a');

    hook.rerender({ ...initial, track: track('b') });
    expect(fake.shown[1]?.title).toBe('Song b');
    // The same song again: nothing new.
    hook.rerender({ ...initial, track: track('b') });
    expect(fake.shown).toHaveLength(2);

    await act(async () => { fake.handler?.({ action: 'next' }); });
    expect(a.next).toHaveBeenCalledTimes(1);

    hook.rerender({ ...initial, enabled: false, track: track('b') });
    await act(async () => {});
    expect(fake.shown[2]).toBeNull();
    expect(fake.handler).toBeNull();
    hook.unmount();
    expect(fake.shown).toHaveLength(3);
  });
});

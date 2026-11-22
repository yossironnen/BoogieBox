import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Track } from '../types';
import type { NativeCommand, NativeEvent } from './nativeTypes';

const engine = vi.hoisted(() => ({
  commands: [] as NativeCommand[],
  handler: null as ((e: NativeEvent) => void) | null,
}));

vi.mock('../platform', () => ({
  platform: {
    isDesktop: true,
    audio: {
      command: vi.fn(async (c: NativeCommand) => { engine.commands.push(c); }),
      devices: vi.fn(async () => []),
      settings: vi.fn(),
      saveSettings: vi.fn(),
      machineName: vi.fn(async () => 'STUDIO-PC'),
      onEvent: vi.fn(async (h: (e: NativeEvent) => void) => { engine.handler = h; return () => { engine.handler = null; }; }),
    },
  },
  nativeAudioAvailable: async () => true,
}));

const tokens = vi.hoisted(() => ({ delayMs: 0 }));

vi.mock('../api', () => ({
  api: {
    nativeStreamTokens: vi.fn(async (ids: string[]) => {
      if (tokens.delayMs) await new Promise((r) => setTimeout(r, tokens.delayMs));
      return {
      tokens: ids.map((id) => (id === 'missing'
        ? { id, error: 'file_missing' }
        : { id, url: `/media/n/${id}`, expiresAt: Date.now() + 6 * 3_600_000, format: 'FLAC', decode: 'native', duration: 180 })),
      };
    }),
    absoluteUrl: (p: string) => `http://srv${p}`,
  },
}));

import { nextIndexAfter, useNativePlayer, type NativePlayerOptions } from './useNativePlayer';

const track = (id: string): Track => ({ id, title: `Song ${id}`, duration: 180 } as unknown as Track);
const flac = { sampleRate: 96_000, bits: 24, channels: 2, codec: 'FLAC' };
const dac = { sampleRate: 96_000, bits: 24, channels: 2, codec: 's24in32' };

function setup(overrides: Partial<NativePlayerOptions> = {}) {
  const onAdvance = vi.fn();
  const onFinished = vi.fn();
  const notify = vi.fn();
  const initial: NativePlayerOptions = {
    enabled: true,
    queue: [track('a'), track('b'), track('c')],
    currentIndex: 0,
    isPlaying: true,
    playToken: 1,
    volume: 1,
    repeatMode: 'off',
    deviceName: 'FiiO Q series',
    onAdvance,
    onFinished,
    notify,
    ...overrides,
  };
  const hook = renderHook((props: NativePlayerOptions) => useNativePlayer(props), { initialProps: initial });
  return { hook, onAdvance, onFinished, notify, initial };
}

const emit = (e: NativeEvent) => act(() => { engine.handler?.(e); });
const of = (type: NativeCommand['type']) => engine.commands.filter((c) => c.type === type);

describe('useNativePlayer', () => {
  beforeEach(() => {
    engine.commands = [];
    engine.handler = null;
  });

  it('works out the next index with repeat', () => {
    expect(nextIndexAfter(0, 3, 'off')).toBe(1);
    expect(nextIndexAfter(2, 3, 'off')).toBeNull();
    expect(nextIndexAfter(2, 3, 'all')).toBe(0);
    expect(nextIndexAfter(1, 3, 'one')).toBe(1);
    expect(nextIndexAfter(0, 0, 'all')).toBeNull();
  });

  it('loads the current song, queues the next and follows the engine onto it', async () => {
    const { hook, onAdvance, initial } = setup();
    await waitFor(() => expect(of('setNext')).toHaveLength(1));
    const load = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    expect(load.item.url).toBe('http://srv/media/n/a');
    expect(load.item.trackId).toMatch(/^a#\d+$/);
    expect(load.autoplay).toBe(true);
    const next = of('setNext')[0] as Extract<NativeCommand, { type: 'setNext' }>;
    expect(next.item?.url).toBe('http://srv/media/n/b');

    await emit({ type: 'trackStarted', trackId: load.item.trackId, source: flac, output: dac, path: [{ kind: 'bitPerfect' }] });
    expect(hook.result.current.badge?.label).toBe('24/96 · Bit-perfect');
    expect(onAdvance).not.toHaveBeenCalled();

    // The engine moves on by itself (gapless): the Player follows, without a reload.
    await emit({ type: 'trackEnded', trackId: load.item.trackId });
    await emit({ type: 'trackStarted', trackId: next.item!.trackId, source: flac, output: dac, path: [{ kind: 'bitPerfect' }] });
    expect(onAdvance).toHaveBeenCalledWith(1);
    hook.rerender({ ...initial, currentIndex: 1 });
    await waitFor(() => expect(of('setNext')).toHaveLength(2));
    expect(of('load')).toHaveLength(1);
    expect((of('setNext')[1] as Extract<NativeCommand, { type: 'setNext' }>).item?.url).toBe('http://srv/media/n/c');

    await emit({ type: 'state', status: 'playing', trackId: next.item!.trackId, positionSec: 12.5, durationSec: 180 });
    expect(hook.result.current.positionSec).toBe(12.5);
    act(() => hook.result.current.seek(30));
    expect(of('seek')).toEqual([{ type: 'seek', sec: 30 }]);
  });

  it('reports the end, pauses, volume and leaving the output', async () => {
    const { hook, onFinished, initial } = setup({ queue: [track('a')] });
    await waitFor(() => expect(of('load')).toHaveLength(1));
    const load = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    await emit({ type: 'trackStarted', trackId: load.item.trackId, source: flac, output: dac, path: [] });
    await emit({ type: 'trackEnded', trackId: load.item.trackId });
    expect(onFinished).toHaveBeenCalledTimes(1);

    hook.rerender({ ...initial, queue: [track('a')], isPlaying: false, volume: 0.4 });
    expect(of('pause').length).toBeGreaterThan(0);
    expect(of('setVolume')[of('setVolume').length - 1]).toEqual({ type: 'setVolume', linear: 0.4 });
    hook.rerender({ ...initial, queue: [track('a')], enabled: false });
    expect(of('stop')).toHaveLength(1);
  });

  it('starts where a hand-off left off, refreshes expired links and words errors', async () => {
    const { hook, notify } = setup({ resumeAt: { playToken: 1, trackId: 'a', sec: 42 } });
    await waitFor(() => expect(of('load')).toHaveLength(1));
    const load = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    expect(load.startSec).toBe(42);

    await emit({ type: 'tokenExpired', trackId: load.item.trackId });
    await waitFor(() => expect(of('refreshUrl')).toHaveLength(1));
    expect(of('refreshUrl')[0]).toEqual({ type: 'refreshUrl', trackId: load.item.trackId, url: 'http://srv/media/n/a' });

    await emit({ type: 'error', trackId: null, code: 'device_busy', message: 'in use' });
    expect(notify).toHaveBeenCalledWith('Another app is using the FiiO Q series. Playing in Shared mode until it lets go.', 'warning');
    await emit({ type: 'error', trackId: load.item.trackId, code: 'decode', message: 'broken' });
    expect(hook.result.current.error).toBe("This song can't be played.");

    await emit({ type: 'error', trackId: load.item.trackId, code: 'device_lost', message: 'unplugged' });
    await emit({ type: 'state', status: 'stopped', trackId: null, positionSec: 0, durationSec: null });
    expect(notify).toHaveBeenLastCalledWith(
      "The FiiO Q series didn't come back. Playback stopped. Press Play to carry on with another device.",
      'danger',
    );
    await emit({ type: 'trackStarted', trackId: load.item.trackId, source: flac, output: { ...dac, sampleRate: 88_200 }, path: [{ kind: 'resampled', toHz: 88_200 }] });
    expect(notify).toHaveBeenLastCalledWith("This DAC can't play 96 kHz. Playing at 88.2 kHz instead.", 'muted');
  });

  it('sends the sound settings first and each song\'s gain and curve with it', async () => {
    const bell = { type: 'peaking' as const, frequencyHz: 1_000, gainDb: 4, q: 1.1, enabled: true };
    const settings = { pure: false, eq: [bell], itemEq: true, levelling: true, crossfadeSec: 4 };
    const levels = vi.fn();
    const { hook, initial } = setup({
      sound: { settings, items: { a: { gainDb: -3.5, eq: null }, b: { gainDb: 1, eq: [bell] } } },
      onLevels: levels,
    });
    await waitFor(() => expect(of('setNext')).toHaveLength(1));
    expect(engine.commands[0]).toEqual({ type: 'setSound', sound: settings });
    const load = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    expect(load.item).toMatchObject({ gainDb: -3.5, eq: null });
    const next = of('setNext')[0] as Extract<NativeCommand, { type: 'setNext' }>;
    expect(next.item).toMatchObject({ gainDb: 1, eq: [bell] });
    expect(of('setItemSound')).toHaveLength(0);

    // The loudness values for "a" change: only that song is told again.
    hook.rerender({
      ...initial,
      sound: { settings, items: { a: { gainDb: -6, eq: null }, b: { gainDb: 1, eq: [bell] } } },
      onLevels: levels,
    });
    expect(of('setItemSound')).toEqual([{ type: 'setItemSound', trackId: load.item.trackId, gainDb: -6, eq: null }]);
    expect(of('setSound')).toHaveLength(1);
    // Pure mode on: new settings go out.
    hook.rerender({
      ...initial,
      sound: { settings: { ...settings, pure: true }, items: { a: { gainDb: -6, eq: null }, b: { gainDb: 1, eq: [bell] } } },
      onLevels: levels,
    });
    expect(of('setSound')).toHaveLength(2);

    await emit({ type: 'levels', peak: 0.5, rms: 0.2, wave: [1, -1] });
    expect(levels).toHaveBeenCalledWith([1, -1]);
  });

  it('loads the next song when Next is pressed during playback', async () => {
    // A long queue: Next moves the link window on, so the load waits for the server while the next-item refresh
    // is answered from the cache.
    const queue = Array.from({ length: 14 }, (_, i) => track(`t${i}`));
    const { hook, initial } = setup({ queue });
    await waitFor(() => expect(of('setNext')).toHaveLength(1));
    const first = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    await emit({ type: 'trackStarted', trackId: first.item.trackId, source: flac, output: dac, path: [] });
    tokens.delayMs = 30;
    hook.rerender({ ...initial, queue, currentIndex: 1, playToken: 2 });
    await waitFor(() => expect(of('load')).toHaveLength(2));
    expect((of('load')[1] as Extract<NativeCommand, { type: 'load' }>).item.url).toBe('http://srv/media/n/t1');
    await waitFor(() => expect((of('setNext')[of('setNext').length - 1] as Extract<NativeCommand, { type: 'setNext' }>).item?.url).toBe('http://srv/media/n/t2'));
    tokens.delayMs = 0;
  });

  it('starts a song where the Player says (remember progress) and names the song it plays', async () => {
    const startAt = vi.fn((t: Track) => (t.id === 'a' ? 95 : null));
    const { hook } = setup({ startAt });
    await waitFor(() => expect(of('load')).toHaveLength(1));
    const load = of('load')[0] as Extract<NativeCommand, { type: 'load' }>;
    expect(load.startSec).toBe(95);
    expect(startAt).toHaveBeenCalledWith(expect.objectContaining({ id: 'a' }));
    expect(hook.result.current.trackId).toBeNull();
    await emit({ type: 'trackStarted', trackId: load.item.trackId, source: flac, output: dac, path: [] });
    expect(hook.result.current.trackId).toBe('a');
  });

  it('a hand-off position wins over the saved one', async () => {
    setup({ startAt: () => 95, resumeAt: { playToken: 1, trackId: 'a', sec: 42 } });
    await waitFor(() => expect(of('load')).toHaveLength(1));
    expect((of('load')[0] as Extract<NativeCommand, { type: 'load' }>).startSec).toBe(42);
  });

  it('says when a song has no link', async () => {
    const { hook } = setup({ queue: [track('missing')] });
    await waitFor(() => expect(hook.result.current.error).toBe("This song can't be played."));
    expect(of('load')).toHaveLength(0);
  });
});

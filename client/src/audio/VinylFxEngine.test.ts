import { beforeEach, describe, expect, it, vi } from 'vitest';

describe('VinylFxEngine', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.unstubAllGlobals();
    delete (window as any).AudioContext;
    delete (window as any).webkitAudioContext;
  });

  it('preloads, decodes, resumes, and plays the Web Audio needle drop', async () => {
    const start = vi.fn();
    const connectGain = vi.fn();
    const gainNode = {
      gain: { value: 0 },
      connect: vi.fn(() => ({ connect: connectGain })),
    };
    const source = {
      buffer: null,
      connect: vi.fn(() => gainNode),
      start,
    };
    const decoded = {} as AudioBuffer;
    const context = {
      state: 'suspended',
      destination: {},
      resume: vi.fn().mockResolvedValue(undefined),
      decodeAudioData: vi.fn().mockResolvedValue(decoded),
      createBufferSource: vi.fn(() => source),
      createGain: vi.fn(() => gainNode),
    };
    (window as any).AudioContext = class {
      constructor() { return context; }
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      arrayBuffer: vi.fn().mockResolvedValue(new ArrayBuffer(8)),
    }));
    const { playNeedleDrop, preloadVinylFx } = await import('./VinylFxEngine');

    await Promise.all([preloadVinylFx(), preloadVinylFx()]);
    await playNeedleDrop(2);

    expect(context.decodeAudioData).toHaveBeenCalled();
    expect(context.resume).toHaveBeenCalled();
    expect(source.buffer).toBe(decoded);
    expect(gainNode.gain.value).toBe(1);
    expect(source.connect).toHaveBeenCalledWith(gainNode);
    expect(start).toHaveBeenCalled();
  });

  it('uses the HTML audio fallback when Web Audio is unavailable', async () => {
    const load = vi.fn().mockImplementation(() => { throw new Error('load unsupported'); });
    const play = vi.fn().mockResolvedValue(undefined);
    const audio = { preload: '', currentTime: 10, volume: 0, load, play };
    vi.stubGlobal('Audio', class {
      constructor() { return audio; }
    });
    const { playNeedleDrop, preloadVinylFx } = await import('./VinylFxEngine');

    await preloadVinylFx();
    await preloadVinylFx();
    await playNeedleDrop(-1);
    await playNeedleDrop(0.4);

    expect(load).toHaveBeenCalled();
    expect(play).toHaveBeenCalled();
    expect(audio.currentTime).toBe(0);
    expect(audio.volume).toBe(0.4);
  });

  it('falls back after resume and source failures without rejecting', async () => {
    const fallbackPlay = vi.fn().mockRejectedValue(new Error('autoplay denied'));
    const fallback = {
      preload: '', currentTime: 0, volume: 0, load: vi.fn(), play: fallbackPlay,
    };
    vi.stubGlobal('Audio', class {
      constructor() { return fallback; }
    });
    const context = {
      state: 'suspended',
      destination: {},
      resume: vi.fn().mockRejectedValue(new Error('resume denied')),
      decodeAudioData: vi.fn().mockResolvedValue({}),
      createBufferSource: vi.fn(() => { throw new Error('source failed'); }),
      createGain: vi.fn(),
    };
    (window as any).webkitAudioContext = class {
      constructor() { return context; }
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      arrayBuffer: vi.fn().mockResolvedValue(new ArrayBuffer(1)),
    }));
    const { playNeedleDrop, preloadVinylFx } = await import('./VinylFxEngine');

    await preloadVinylFx();
    await playNeedleDrop(0.8);

    expect(context.resume).toHaveBeenCalled();
    expect(fallbackPlay).toHaveBeenCalled();
  });

  it('falls back when the needle-drop asset cannot be fetched', async () => {
    const play = vi.fn().mockResolvedValue(undefined);
    const fallback = {
      preload: '', currentTime: 0, volume: 0, load: vi.fn(), play,
    };
    vi.stubGlobal('Audio', class {
      constructor() { return fallback; }
    });
    const context = {
      state: 'running',
      destination: {},
      decodeAudioData: vi.fn(),
      createBufferSource: vi.fn(),
      createGain: vi.fn(),
    };
    (window as any).AudioContext = class {
      constructor() { return context; }
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 404 }));
    const { playNeedleDrop } = await import('./VinylFxEngine');

    await playNeedleDrop(0.5);
    expect(play).toHaveBeenCalled();
    expect(context.decodeAudioData).not.toHaveBeenCalled();
  });
});

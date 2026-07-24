import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAdaptiveAccentEnabled } from './useAdaptiveAccent';

const { getColorAsync } = vi.hoisted(() => ({ getColorAsync: vi.fn() }));

vi.mock('fast-average-color', () => ({
  FastAverageColor: class {
    getColorAsync = getColorAsync;
  },
}));

describe('useAdaptiveAccentEnabled', () => {
  beforeEach(() => {
    getColorAsync.mockReset();
    document.documentElement.removeAttribute('style');
  });

  it('samples a loaded image, adjusts dark colors, caches them, and resets on cleanup', async () => {
    getColorAsync.mockResolvedValue({ value: [2, 4, 8, 255] });
    const image = document.createElement('img');
    Object.defineProperties(image, {
      currentSrc: { configurable: true, value: new URL('/cover-dark.jpg', window.location.href).toString() },
      complete: { configurable: true, value: true },
      naturalWidth: { configurable: true, value: 300 },
    });
    document.documentElement.style.setProperty('--accent-base', '#123456');

    const first = renderHook(() => useAdaptiveAccentEnabled('/cover-dark.jpg', true, image));
    await waitFor(() => expect(getColorAsync).toHaveBeenCalledWith(
      image,
      { algorithm: 'dominant', mode: 'speed' },
    ));
    await waitFor(() => expect(document.documentElement.style.getPropertyValue('--accent')).toMatch(/^#/));
    const sampled = document.documentElement.style.getPropertyValue('--accent');
    first.unmount();
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#123456');

    const cached = renderHook(() => useAdaptiveAccentEnabled('/cover-dark.jpg', true));
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe(sampled);
    expect(getColorAsync).toHaveBeenCalledTimes(1);
    cached.unmount();
  });

  it('waits for an existing image load and adjusts overly bright colors', async () => {
    getColorAsync.mockResolvedValue({ value: [250, 245, 240, 255] });
    const image = document.createElement('img');
    image.src = '/cover-bright.jpg';
    Object.defineProperties(image, {
      currentSrc: { configurable: true, value: '' },
      complete: { configurable: true, value: false },
      naturalWidth: { configurable: true, value: 0 },
    });
    const removeSpy = vi.spyOn(image, 'removeEventListener');
    const hook = renderHook(() => useAdaptiveAccentEnabled('/cover-bright.jpg', true, image));
    expect(getColorAsync).not.toHaveBeenCalled();

    act(() => image.dispatchEvent(new Event('load')));
    await waitFor(() => expect(getColorAsync).toHaveBeenCalled());
    hook.unmount();
    expect(removeSpy).toHaveBeenCalledWith('load', expect.any(Function));
  });

  it('creates a cross-origin image when the provided element does not match', async () => {
    getColorAsync.mockResolvedValue({ value: [20, 120, 220, 255] });
    const realImage = globalThis.Image;
    const created: Array<{ crossOrigin: string; src: string; onload?: () => Promise<void> }> = [];
    class FakeImage {
      crossOrigin = '';
      src = '';
      onload?: () => Promise<void>;
      constructor() {
        created.push(this);
      }
    }
    vi.stubGlobal('Image', FakeImage);
    const mismatched = document.createElement('img');
    mismatched.src = '/other.jpg';

    const hook = renderHook(() => useAdaptiveAccentEnabled('/generated.jpg', true, mismatched));
    expect(created[0].crossOrigin).toBe('anonymous');
    expect(created[0].src).toBe('/generated.jpg');
    await act(async () => created[0].onload?.());
    await waitFor(() => expect(getColorAsync).toHaveBeenCalled());
    hook.unmount();
    vi.stubGlobal('Image', realImage);
  });

  it('keeps the default accent for disabled, empty, cancelled, invalid, and failed samples', async () => {
    document.documentElement.style.setProperty('--accent', '#abcdef');
    const disabled = renderHook(() => useAdaptiveAccentEnabled('/cover.jpg', false));
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
    disabled.unmount();

    const empty = renderHook(() => useAdaptiveAccentEnabled(null, true));
    expect(getColorAsync).not.toHaveBeenCalled();
    empty.unmount();

    const invalid = document.createElement('img');
    Object.defineProperty(invalid, 'src', { configurable: true, get: () => 'http://[invalid' });
    getColorAsync.mockRejectedValueOnce(new Error('CORS'));
    const hook = renderHook(() => useAdaptiveAccentEnabled('http://[invalid', true, invalid));
    hook.unmount();
  });
});

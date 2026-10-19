import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { favouritesStore, useFavourites } from './favourites';

const { favouritesApi } = vi.hoisted(() => ({
  favouritesApi: { ids: vi.fn(), add: vi.fn(), remove: vi.fn(), list: vi.fn() },
}));

vi.mock('./api', () => ({ api: { favourites: favouritesApi } }));

describe('favouritesStore', () => {
  beforeEach(() => {
    Object.values(favouritesApi).forEach((m) => m.mockReset());
    favouritesStore.reset();
  });

  it('loads ids once and answers lookups and counts', async () => {
    favouritesApi.ids.mockResolvedValue({ tracks: ['t1', 't2'], albums: ['al1'], artists: [] });
    await favouritesStore.load();
    await favouritesStore.load();
    expect(favouritesApi.ids).toHaveBeenCalledTimes(1);
    expect(favouritesStore.has('track', 't1')).toBe(true);
    expect(favouritesStore.has('album', 'al1')).toBe(true);
    expect(favouritesStore.has('artist', 'ar1')).toBe(false);
    expect(favouritesStore.has('track', null)).toBe(false);
    expect(favouritesStore.count('track')).toBe(2);
    await favouritesStore.load(true);
    expect(favouritesApi.ids).toHaveBeenCalledTimes(2);
  });

  it('survives a failed or missing API', async () => {
    favouritesApi.ids.mockRejectedValue(new Error('offline'));
    await favouritesStore.load();
    expect(favouritesStore.count('track')).toBe(0);
  });

  it('sets optimistically and reverts when the server refuses', async () => {
    favouritesApi.add.mockResolvedValue({ ok: true });
    favouritesApi.remove.mockRejectedValue(new Error('nope'));
    const before = favouritesStore.getVersion();
    expect(await favouritesStore.toggle('track', 't9')).toBe(true);
    expect(favouritesApi.add).toHaveBeenCalledWith('track', 't9');
    expect(favouritesStore.has('track', 't9')).toBe(true);
    expect(favouritesStore.getVersion()).toBeGreaterThan(before);
    // No change → no request.
    expect(await favouritesStore.set('track', 't9', true)).toBe(true);
    expect(favouritesApi.add).toHaveBeenCalledTimes(1);
    await expect(favouritesStore.toggle('track', 't9')).rejects.toThrow('nope');
    expect(favouritesStore.has('track', 't9')).toBe(true);

    favouritesApi.add.mockRejectedValueOnce(new Error('missing'));
    await expect(favouritesStore.set('album', 'al5', true)).rejects.toThrow('missing');
    expect(favouritesStore.has('album', 'al5')).toBe(false);
  });

  it('re-renders hook users when favourites change', async () => {
    favouritesApi.add.mockResolvedValue({ ok: true });
    const { result } = renderHook(() => useFavourites());
    expect(result.current.has('artist', 'ar1')).toBe(false);
    await act(async () => { await result.current.toggle('artist', 'ar1'); });
    expect(result.current.has('artist', 'ar1')).toBe(true);
    expect(result.current.count('artist')).toBe(1);
  });
});

import { describe, expect, it, vi } from 'vitest';
import { MAX_BATCH, NativeLinks, REFRESH_BEFORE_MS } from './nativeLinks';
import type { NativeStreamLink } from '../api';

const HOUR = 3_600_000;

function fetcher(now: () => number) {
  return vi.fn(async (ids: string[]): Promise<NativeStreamLink[]> => ids.map((id) => (
    id === 'gone'
      ? { id, error: 'file_missing' as const }
      : {
        id,
        url: `/media/n/${id}-${now()}`,
        expiresAt: now() + 6 * HOUR,
        format: id.startsWith('radio:') ? undefined : 'FLAC',
        decode: id.startsWith('radio:') ? 'live' as const : id === 'opus' ? 'server_wav' as const : 'native' as const,
        duration: 200,
      }
  )));
}

describe('NativeLinks', () => {
  it('fetches in batches, makes urls absolute and keeps fresh links', async () => {
    let t = 1_000_000;
    const fetch = fetcher(() => t);
    const links = new NativeLinks(fetch, (p) => `http://srv:3001${p}`, () => t);
    const ids = Array.from({ length: MAX_BATCH + 5 }, (_, i) => `t${i}`);
    const got = await links.get(ids);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[0][0]).toHaveLength(MAX_BATCH);
    expect(got.get('t0')?.url).toBe(`http://srv:3001/media/n/t0-${t}`);
    expect(got.get('t0')?.format).toBe('FLAC');

    await links.get(['t0', 't1']);
    expect(fetch).toHaveBeenCalledTimes(2);

    // Close to expiry: fetched again.
    t += 6 * HOUR - REFRESH_BEFORE_MS + 1;
    await links.get(['t0']);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(fetch.mock.calls[2][0]).toEqual(['t0']);
  });

  it('remembers failures, flags live and server-decoded items, refreshes on demand', async () => {
    const t = 5;
    const fetch = fetcher(() => t);
    const links = new NativeLinks(fetch, (p) => p, () => t);
    const got = await links.get(['gone', 'radio:abc', 'opus']);
    expect(got.has('gone')).toBe(false);
    expect(links.failure('gone')).toBe('file_missing');
    expect(got.get('radio:abc')).toMatchObject({ live: true, serverDecoded: false, format: null });
    expect(got.get('opus')).toMatchObject({ live: false, serverDecoded: true });
    const fresh = await links.refresh('opus');
    expect(fresh?.url).toBe('/media/n/opus-5');
    expect(fetch).toHaveBeenCalledTimes(2);
    links.clear();
    expect(links.failure('gone')).toBeUndefined();
  });
});

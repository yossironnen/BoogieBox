/**
 * Tests Mobile Artist Browse.Test behavior for BoogieBox regressions.
 */

import { describe, expect, it } from 'vitest';
import {
  buildArtistBrowseCacheKey,
  computeVirtualWindow,
  mergeArtistBrowseRows,
  MOBILE_ARTIST_OVERSCAN,
  MOBILE_ARTIST_ROW_HEIGHT,
} from './mobileArtistBrowse';

describe('mobileArtistBrowse helpers', () => {
  it('builds a stable cache key from normalized filters', () => {
    expect(buildArtistBrowseCacheKey({
      query: '  Neon  ',
      startsWith: 'n',
      order: 'desc',
    })).toBe('desc::N::neon');
  });

  it('deduplicates appended artist rows by id', () => {
    const merged = mergeArtistBrowseRows(
      [
        { id: '1', name: 'Alpha', album_count: 1, track_count: 1 },
        { id: '2', name: 'Beta', album_count: 2, track_count: 2 },
      ],
      [
        { id: '2', name: 'Beta', album_count: 2, track_count: 2 },
        { id: '3', name: 'Gamma', album_count: 3, track_count: 3 },
      ],
    );

    expect(merged.map((artist) => artist.id)).toEqual(['1', '2', '3']);
  });

  it('computes a bounded virtual window with overscan', () => {
    const window = computeVirtualWindow({
      itemCount: 100,
      rowHeight: MOBILE_ARTIST_ROW_HEIGHT,
      viewportHeight: MOBILE_ARTIST_ROW_HEIGHT * 4,
      scrollTop: MOBILE_ARTIST_ROW_HEIGHT * 10,
      overscan: MOBILE_ARTIST_OVERSCAN,
    });

    expect(window.start).toBe(4);
    expect(window.end).toBe(20);
    expect(window.paddingTop).toBe(MOBILE_ARTIST_ROW_HEIGHT * 4);
    expect(window.paddingBottom).toBe(MOBILE_ARTIST_ROW_HEIGHT * 80);
  });
});


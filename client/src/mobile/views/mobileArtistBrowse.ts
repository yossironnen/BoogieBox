/**
 * Defines mobile Mobile Artist Browse behavior for the BoogieBox React client.
 */

import type { Artist, ClientEntityId } from '../../types';

/** MOBILE ARTIST PAGE SIZE is part of this module's public API. */
export const MOBILE_ARTIST_PAGE_SIZE = 120;
/** MOBILE ARTIST ROW HEIGHT is part of this module's public API. */
export const MOBILE_ARTIST_ROW_HEIGHT = 84;
/** MOBILE ARTIST GRID ROW HEIGHT is part of this module's public API. */
export const MOBILE_ARTIST_GRID_ROW_HEIGHT = 210; // 2-col grid: ~160px art + ~38px text + 12px inter-row gap
/** MOBILE ARTIST OVERSCAN is part of this module's public API. */
export const MOBILE_ARTIST_OVERSCAN = 6;

/** Artist Browse Cache State is part of this module's public API. */
export interface ArtistBrowseCacheState {
  items: Artist[];
  total: number;
  hasMore: boolean;
  scrollTop: number;
}

/** Virtual Window Metrics is part of this module's public API. */
export interface VirtualWindowMetrics {
  start: number;
  end: number;
  paddingTop: number;
  paddingBottom: number;
}

/** Build Artist Browse Cache Key is part of this module's public API. */
export function buildArtistBrowseCacheKey(params: {
  query?: string;
  startsWith?: string | null;
  order?: 'asc' | 'desc';
}): string {
  const query = params.query?.trim().toLowerCase() ?? '';
  const startsWith = params.startsWith?.trim().toUpperCase() ?? '';
  const order = params.order ?? 'asc';
  return `${order}::${startsWith}::${query}`;
}

/** Merge Artist Browse Rows is part of this module's public API. */
export function mergeArtistBrowseRows(current: Artist[], incoming: Artist[], reset = false): Artist[] {
  if (reset) return [...incoming];
  const seen = new Set<ClientEntityId>();
  const merged: Artist[] = [];
  for (const artist of [...current, ...incoming]) {
    if (seen.has(artist.id)) continue;
    seen.add(artist.id);
    merged.push(artist);
  }
  return merged;
}

/** Compute Virtual Window is part of this module's public API. */
export function computeVirtualWindow(params: {
  itemCount: number;
  rowHeight: number;
  viewportHeight: number;
  scrollTop: number;
  overscan?: number;
}): VirtualWindowMetrics {
  const { itemCount, rowHeight, viewportHeight, scrollTop } = params;
  const overscan = params.overscan ?? 0;
  if (itemCount <= 0 || rowHeight <= 0) {
    return { start: 0, end: 0, paddingTop: 0, paddingBottom: 0 };
  }

  const visibleCount = Math.max(1, Math.ceil(Math.max(viewportHeight, rowHeight) / rowHeight));
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(itemCount, start + visibleCount + overscan * 2);
  const paddingTop = start * rowHeight;
  const paddingBottom = Math.max(0, (itemCount - end) * rowHeight);
  return { start, end, paddingTop, paddingBottom };
}

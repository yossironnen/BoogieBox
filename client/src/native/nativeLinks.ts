/**
 * Stream links for the desktop engine (`POST /api/native/stream-tokens`): fetched in batches for the current item
 * and the ones after it, kept until shortly before they expire, and refreshed when the engine reports an expired
 * link.
 */

import type { NativeStreamLink } from '../api';

/** Links are fetched again this long before they expire. */
export const REFRESH_BEFORE_MS = 30 * 60 * 1000;
/** Most ids per request (server limit). */
export const MAX_BATCH = 50;

export interface ResolvedLink {
  /** Absolute URL for the engine. */
  url: string;
  expiresAt: number;
  serverDecoded: boolean;
  live: boolean;
  format: string | null;
  durationSec: number | null;
}

export type LinkFetcher = (ids: string[]) => Promise<NativeStreamLink[]>;

export class NativeLinks {
  private cache = new Map<string, ResolvedLink>();
  private failed = new Map<string, NativeStreamLink['error']>();

  constructor(
    private readonly fetchLinks: LinkFetcher,
    private readonly absolute: (path: string) => string,
    private readonly now: () => number = Date.now,
  ) {}

  private fresh(id: string): ResolvedLink | null {
    const link = this.cache.get(id);
    return link && link.expiresAt - this.now() > REFRESH_BEFORE_MS ? link : null;
  }

  /** Links for `ids` (missing or stale ones fetched, in batches); unknown ids are left out. */
  async get(ids: string[]): Promise<Map<string, ResolvedLink>> {
    const wanted = [...new Set(ids)];
    const missing = wanted.filter((id) => !this.fresh(id));
    for (let i = 0; i < missing.length; i += MAX_BATCH) {
      await this.fetch(missing.slice(i, i + MAX_BATCH));
    }
    const out = new Map<string, ResolvedLink>();
    for (const id of wanted) {
      const link = this.cache.get(id);
      if (link) out.set(id, link);
    }
    return out;
  }

  /** A new link for `id`, whatever the cache holds (the old one expired). */
  async refresh(id: string): Promise<ResolvedLink | null> {
    this.cache.delete(id);
    await this.fetch([id]);
    return this.cache.get(id) ?? null;
  }

  /** Why the server gave no link for `id`, if it said. */
  failure(id: string): NativeStreamLink['error'] | undefined {
    return this.failed.get(id);
  }

  clear(): void {
    this.cache.clear();
    this.failed.clear();
  }

  private async fetch(ids: string[]): Promise<void> {
    if (!ids.length) return;
    const links = await this.fetchLinks(ids);
    for (const link of links) {
      if (link.url && link.expiresAt) {
        this.failed.delete(link.id);
        this.cache.set(link.id, {
          url: this.absolute(link.url),
          expiresAt: link.expiresAt,
          serverDecoded: link.decode === 'server_wav',
          live: link.decode === 'live',
          format: link.format ?? null,
          durationSec: link.duration ?? null,
        });
      } else if (link.error) {
        this.failed.set(link.id, link.error);
      }
    }
  }
}

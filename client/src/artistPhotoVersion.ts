/**
 * Artist photo cache-busting version, persisted per artist id.
 *
 * `GET /api/artists/:id/photo` is served with `Cache-Control: no-cache`
 * (forces revalidation, doesn't forbid caching) — but a component-local
 * counter reset to 0 on every mount isn't enough to guarantee a freshly
 * edited photo shows up immediately: `ArtistHeader` remounts every time
 * Browse navigates away from and back to the artist screen, and any browser
 * cache entry created before a fix to the response headers keeps whatever
 * freshness rules it was fetched under until it naturally expires. Storing
 * the version here survives both the remount and a full reload, so a picked
 * photo's URL is provably different from whatever was cached before it.
 */

function safeLocalStorageGet(key: string): string | null {
  try {
    if (typeof localStorage === 'undefined' || typeof localStorage.getItem !== 'function') return null;
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeLocalStorageSet(key: string, value: string): void {
  try {
    if (typeof localStorage === 'undefined' || typeof localStorage.setItem !== 'function') return;
    localStorage.setItem(key, value);
  } catch {
    // Best effort only.
  }
}

function artistPhotoVersionKey(artistId: string | number): string {
  return `boogiebox.artistPhotoVersion.${artistId}`;
}

export function getArtistPhotoVersion(artistId: string | number): number {
  const raw = safeLocalStorageGet(artistPhotoVersionKey(artistId));
  const parsed = raw ? Number(raw) : 0;
  return Number.isFinite(parsed) ? parsed : 0;
}

export function bumpArtistPhotoVersion(artistId: string | number): number {
  const next = Date.now();
  safeLocalStorageSet(artistPhotoVersionKey(artistId), String(next));
  return next;
}

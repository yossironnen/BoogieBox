/**
 * Defines Release Types behavior for BoogieBox.
 */

import type { Album } from './types';

/** Artist Release Type is part of this module's public API. */
export type ArtistReleaseType = 'album' | 'single' | 'compilation';

/** Normalize Release Type is part of this module's public API. */
export function normalizeReleaseType(value: unknown): ArtistReleaseType {
  if (value === 'single' || value === 'compilation') return value;
  return 'album';
}

/** Group Artist Discography By Release Type is part of this module's public API. */
export function groupArtistDiscographyByReleaseType(albums: Album[]): Record<ArtistReleaseType, Album[]> {
  const partitions: Record<ArtistReleaseType, Album[]> = {
    album: [],
    single: [],
    compilation: [],
  };
  albums.forEach((album) => {
    partitions[normalizeReleaseType(album.releaseType)].push(album);
  });
  return partitions;
}

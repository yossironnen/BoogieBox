/**
 * Defines Artist Track Matching behavior for BoogieBox.
 */

import type { Track } from './types';

function normalizeTrackText(value: string | null | undefined): string {
  return (value || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

/** Find Top Track Match is part of this module's public API. */
export function findTopTrackMatch(candidates: Track[], songName: string): Track | null {
  const targetSong = normalizeTrackText(songName);

  const exact = candidates.find((track) => normalizeTrackText(track.title || track.file_name) === targetSong);
  if (exact) return exact;

  const contains = candidates.find((track) => normalizeTrackText(track.title || track.file_name).includes(targetSong));
  return contains || null;
}

/** Matches Track Artist is part of this module's public API. */
export function matchesTrackArtist(artistName: string, trackArtist?: string | null): boolean {
  const target = normalizeTrackText(artistName);
  const artistTag = normalizeTrackText(trackArtist);
  return artistTag === target;
}

/** Resolve Top Track From Library Search is part of this module's public API. */
export function resolveTopTrackFromLibrarySearch(
  artistName: string,
  songName: string,
  tracks: Track[],
): Track | null {
  const artistMatches = tracks.filter((track) => matchesTrackArtist(artistName, track.artist));
  return findTopTrackMatch(artistMatches, songName);
}

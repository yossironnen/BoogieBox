/**
 * Playlist import (M3U/M3U8): file reading, approved copy, and the choices that turn a preview into the
 * ordered track list the commit sends.
 */

import type { PlaylistImportCandidate, PlaylistImportPreview } from './types';

export const MAX_IMPORT_BYTES = 2 * 1024 * 1024;
export const PLAYLIST_FILE_ACCEPT = '.m3u,.m3u8,audio/x-mpegurl,audio/mpegurl,application/vnd.apple.mpegurl';
export const NAME_TAKEN_MESSAGE = 'A playlist with this name already exists';

export type ImportProblem = 'too_big' | 'too_long' | 'empty';

export const IMPORT_PROBLEM_COPY: Record<ImportProblem, { title: string; body: string }> = {
  too_big: { title: 'This file is too big', body: 'Playlist files up to 2 MB can be imported.' },
  too_long: {
    title: 'This playlist is too long',
    body: 'Up to 10,000 tracks can be imported at once. Split the file and import each part.',
  },
  empty: { title: 'No tracks in this file', body: 'Choose an .m3u or .m3u8 playlist file.' },
};

export function isImportProblem(code: string | null | undefined): code is ImportProblem {
  return code === 'too_big' || code === 'too_long' || code === 'empty';
}

/** The file name without its extension: the suggested playlist name. */
export function playlistNameFromFile(fileName: string): string {
  const base = fileName.split(/[\\/]/).pop() ?? fileName;
  const dot = base.lastIndexOf('.');
  return (dot > 0 ? base.slice(0, dot) : base).trim();
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

export async function readFileBase64(file: File): Promise<string> {
  return bytesToBase64(new Uint8Array(await file.arrayBuffer()));
}

/** Per ambiguous line: the chosen track id, or null to skip it. */
export type ImportChoices = Record<number, string | null>;

/** Pre-selects the closest-duration candidate; keeps earlier choices for lines still ambiguous. */
export function initialChoices(preview: PlaylistImportPreview, previous: ImportChoices = {}): ImportChoices {
  const choices: ImportChoices = {};
  for (const entry of preview.entries) {
    if (entry.status !== 'ambiguous') continue;
    const kept = previous[entry.line];
    const valid = kept === null || entry.candidates?.some((c) => c.trackId === kept);
    choices[entry.line] = kept !== undefined && valid ? kept : entry.suggestedTrackId ?? null;
  }
  return choices;
}

/** Track ids in file order, each once. */
export function importTrackIds(preview: PlaylistImportPreview, choices: ImportChoices): string[] {
  const seen = new Set<string>();
  const ids: string[] = [];
  for (const entry of preview.entries) {
    const id = entry.status === 'matched' ? entry.trackId : entry.status === 'ambiguous' ? choices[entry.line] : null;
    if (id && !seen.has(id)) {
      seen.add(id);
      ids.push(id);
    }
  }
  return ids;
}

export function formatTrackDuration(seconds: number | null | undefined): string {
  if (!seconds || seconds <= 0) return '';
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/** "Mezzanine (1998) · 5:29". */
export function candidateDetail(candidate: PlaylistImportCandidate): string {
  const album = candidate.album ? `${candidate.album}${candidate.year ? ` (${candidate.year})` : ''}` : candidate.year ? String(candidate.year) : '';
  return [album, formatTrackDuration(candidate.duration)].filter(Boolean).join(' · ');
}

export function trackCount(count: number): string {
  return `${count.toLocaleString('en-GB')} ${count === 1 ? 'track' : 'tracks'}`;
}

/** Toast body after the import: "197 tracks added. 33 lines were left out." */
export function importSummary(added: number, leftOut: number): string {
  const head = `${trackCount(added)} added.`;
  if (leftOut <= 0) return head;
  return `${head} ${leftOut.toLocaleString('en-GB')} ${leftOut === 1 ? 'line was' : 'lines were'} left out.`;
}

/** The "Not found" list as text, one line per entry. */
export function notFoundText(preview: PlaylistImportPreview): string {
  return preview.entries
    .filter((entry) => entry.status === 'unmatched')
    .map((entry) => `Line ${entry.line}: ${entry.raw}`)
    .join('\n') + '\n';
}

export function downloadTextFile(fileName: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

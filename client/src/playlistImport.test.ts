import { describe, expect, it } from 'vitest';
import type { PlaylistImportPreview } from './types';
import {
  bytesToBase64,
  candidateDetail,
  importSummary,
  importTrackIds,
  initialChoices,
  notFoundText,
  playlistNameFromFile,
} from './playlistImport';

const preview: PlaylistImportPreview = {
  entries: [
    { line: 2, raw: 'a.mp3', status: 'matched', trackId: 'a', label: 'a' },
    {
      line: 4, raw: 'Teardrop.mp3', status: 'ambiguous', label: 'Massive Attack – Teardrop', suggestedTrackId: 't2',
      candidates: [
        { trackId: 't1', title: 'Teardrop', artist: 'Massive Attack', album: 'Mezzanine', year: 1998, duration: 329 },
        { trackId: 't2', title: 'Teardrop', artist: 'Massive Attack', album: 'Collected', year: 2006, duration: 331 },
      ],
    },
    {
      line: 5, raw: 'Intro.mp3', status: 'ambiguous', label: 'Intro',
      candidates: [
        { trackId: 'a', title: 'Intro', artist: null, album: null, year: null, duration: null },
        { trackId: 'i2', title: 'Intro', artist: null, album: 'Two', year: null, duration: 90 },
      ],
    },
    { line: 6, raw: 'gone.mp3', status: 'unmatched', label: 'gone' },
    { line: 7, raw: 'a.mp3', status: 'duplicate', trackId: 'a', label: 'a' },
    { line: 8, raw: 'http://x', status: 'remote', label: 'x' },
  ],
  totals: { matched: 1, ambiguous: 2, unmatched: 1, remote: 1, duplicate: 1 },
  suggestedRemaps: [],
  appliedRemaps: [],
};

describe('playlistImport helpers', () => {
  it('suggests the name from the file name', () => {
    expect(playlistNameFromFile('Road Trip 2026.m3u8')).toBe('Road Trip 2026');
    expect(playlistNameFromFile('C:\\Lists\\mix.m3u')).toBe('mix');
    expect(playlistNameFromFile('noext')).toBe('noext');
  });

  it('pre-selects the suggested candidate, keeps valid earlier choices, and skips the rest', () => {
    expect(initialChoices(preview)).toEqual({ 4: 't2', 5: null });
    expect(initialChoices(preview, { 4: 't1', 5: 'gone' })).toEqual({ 4: 't1', 5: null });
    expect(initialChoices(preview, { 4: null })).toEqual({ 4: null, 5: null });
  });

  it('builds the track list in file order, each track once', () => {
    expect(importTrackIds(preview, { 4: 't2', 5: null })).toEqual(['a', 't2']);
    expect(importTrackIds(preview, { 4: 't1', 5: 'a' })).toEqual(['a', 't1']);
  });

  it('formats the candidate detail, summary and not-found list', () => {
    expect(candidateDetail(preview.entries[1].candidates![0])).toBe('Mezzanine (1998) · 5:29');
    expect(candidateDetail(preview.entries[2].candidates![0])).toBe('');
    expect(importSummary(197, 33)).toBe('197 tracks added. 33 lines were left out.');
    expect(importSummary(1, 1)).toBe('1 track added. 1 line was left out.');
    expect(importSummary(5, 0)).toBe('5 tracks added.');
    expect(notFoundText(preview)).toBe('Line 6: gone.mp3\n');
  });

  it('encodes bytes as base64 in chunks', () => {
    const bytes = new Uint8Array(70_000).map((_, i) => i % 256);
    expect(atob(bytesToBase64(bytes)).length).toBe(70_000);
    expect(bytesToBase64(new TextEncoder().encode('Björk'))).toBe('QmrDtnJr');
  });
});

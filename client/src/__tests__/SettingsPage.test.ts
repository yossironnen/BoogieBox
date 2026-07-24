/**
 * Tests Settings Page.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect } from 'vitest';
import {
  fmtLastRun,
  fmtNextRun,
  formatBytes,
  formatProviderLabel,
  formatQueueSnapshot,
  formatQueueStateLabel,
  hasSpotifyCredentials,
  isValidDlnaPort,
  THEME_PRESETS,
} from '../components/SettingsPage';
import type { AdminQueueEntry, AdminQueueSnapshot } from '../types';

describe('hasSpotifyCredentials', () => {
  it('returns true when both client ID and secret are provided', () => {
    expect(hasSpotifyCredentials('abc123', 'secret456')).toBe(true);
  });

  it('returns false when either field is empty/whitespace', () => {
    expect(hasSpotifyCredentials('', 'secret456')).toBe(false);
    expect(hasSpotifyCredentials('abc123', '')).toBe(false);
    expect(hasSpotifyCredentials('   ', 'secret456')).toBe(false);
  });
});

describe('THEME_PRESETS', () => {
  it('includes existing built-in presets', () => {
    const labels = THEME_PRESETS.map(p => p.label);
    expect(labels).toContain('Dark (default)');
    expect(labels).toContain('Midnight Blue');
    expect(labels).toContain('Forest');
    expect(labels).toContain('Warm Dark');
    expect(labels).toContain('Vintage Radio');
    expect(labels).toContain('Light');
    expect(labels).toContain('Solarized');
    expect(labels).not.toContain('Original');
  });

  it('includes the requested groovy, classic, and modern themes with fonts', () => {
    const labels = THEME_PRESETS.map(p => p.label);
    expect(labels).toContain('Neon Groove');
    expect(labels).toContain('Disco Citrus');
    expect(labels).toContain('Ivory Ledger');
    expect(labels).toContain('Oxford Brass');
    expect(labels).toContain('Graphite Mint');

    const modern = THEME_PRESETS.find(p => p.label === 'Graphite Mint');
    expect(modern?.settings.fontFamily).toBeTruthy();
  });

  it('includes Vintage Radio with wood background texture', () => {
    const vintage = THEME_PRESETS.find(p => p.label === 'Vintage Radio');
    expect(vintage).toBeTruthy();
    expect(vintage?.settings.bgTexture).toBe('wood');
  });
});

describe('isValidDlnaPort', () => {
  it('returns true for valid integer ports in the allowed range', () => {
    expect(isValidDlnaPort('1024')).toBe(true);
    expect(isValidDlnaPort('8200')).toBe(true);
    expect(isValidDlnaPort('65535')).toBe(true);
  });

  it('returns false for non-integer or out-of-range values', () => {
    expect(isValidDlnaPort('1023')).toBe(false);
    expect(isValidDlnaPort('65536')).toBe(false);
    expect(isValidDlnaPort('not-a-number')).toBe(false);
    expect(isValidDlnaPort('80.5')).toBe(false);
  });
});

describe('formatQueueSnapshot', () => {
  const emptyEntry: AdminQueueEntry = {
    id: 'job-1',
    status: 'pending',
    library_id: null,
    library_name: null,
    job_type: null,
    files_scanned: null,
    files_found: null,
    errors: null,
    started_at: null,
    finished_at: null,
    current_step: null,
    playlist_name: null,
    track_title: null,
    error_message: null,
  };

  it('handles a missing snapshot and labels empty queues as idle', () => {
    expect(formatQueueSnapshot(null)).toBe('No queue snapshot loaded yet.');
    const snapshot: AdminQueueSnapshot = {
      fetched_at: '2026-07-24T12:00:00Z',
      queues: { scan: [], postScan: [], mix: [], deepAnalysis: [] },
    };
    const output = formatQueueSnapshot(snapshot);
    expect(output).toContain('Snapshot:');
    expect(output.match(/idle/g)).toHaveLength(4);
  });

  it('formats every optional queue-entry detail and missing file counters', () => {
    const detailed: AdminQueueEntry = {
      ...emptyEntry,
      status: 'running',
      library_name: 'Main Library',
      job_type: 'cache_artist_images',
      files_scanned: null,
      files_found: 12,
      errors: 2,
      started_at: '2026-07-24T12:00:00Z',
      current_step: 'reading tags',
      playlist_name: 'Morning',
      track_title: 'Blue Train',
      error_message: 'retrying',
    };
    const snapshot: AdminQueueSnapshot = {
      fetched_at: '2026-07-24T12:00:00Z',
      queues: {
        scan: [detailed],
        postScan: [{ ...emptyEntry, id: 'job-2', files_scanned: 7 }],
        mix: [emptyEntry],
        deepAnalysis: [],
      },
    };
    const output = formatQueueSnapshot(snapshot);
    expect(output).toContain('scan #job-1 | status=running');
    expect(output).toContain('library=Main Library');
    expect(output).toContain('type=cache_artist_images');
    expect(output).toContain('playlist=Morning');
    expect(output).toContain('track=Blue Train');
    expect(output).toContain('step=reading tags');
    expect(output).toContain('files=0/12');
    expect(output).toContain('errors=2');
    expect(output).toContain('started=');
    expect(output).toContain('note=retrying');
    expect(output).toContain('files=7/0');
  });
});

describe('settings display formatters', () => {
  it('formats absent, invalid, overdue, minute, hour, and day schedule times', () => {
    const now = Date.now();
    expect(fmtNextRun(null)).toBe('Not scheduled');
    expect(fmtNextRun('not-a-date')).toBe('Not scheduled');
    expect(fmtNextRun(new Date(now - 60_000).toISOString())).toBe('Overdue');
    expect(fmtNextRun(new Date(now + 30 * 60_000).toISOString())).toMatch(/^in (29|30)m$/);
    expect(fmtNextRun(new Date(now + 2 * 3_600_000 + 15 * 60_000).toISOString())).toMatch(/^in 2h (14|15)m$/);
    expect(fmtNextRun(new Date(now + 49 * 3_600_000).toISOString())).toMatch(/^in 2d (0|1)h$/);
  });

  it('formats last-run and byte-size boundary cases', () => {
    expect(fmtLastRun(null)).toBe('Never');
    expect(fmtLastRun('2026-07-24T12:00:00Z')).not.toBe('Never');
    expect(formatBytes(undefined)).toBe('0 B');
    expect(formatBytes(null)).toBe('0 B');
    expect(formatBytes(-1)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(12 * 1024)).toBe('12 KB');
    expect(formatBytes(2.5 * 1024 * 1024)).toBe('2.5 MB');
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe('3.0 GB');
  });

  it('labels providers and every queue state, including unknown values', () => {
    expect(formatProviderLabel('lastfm')).toBe('Last.fm');
    expect(formatProviderLabel('spotify')).toBe('Spotify');
    expect(formatProviderLabel('')).toBe('');
    expect(['running', 'pending', 'failed', 'done', 'cancelled', 'custom'].map(formatQueueStateLabel))
      .toEqual(['Running', 'Queued', 'Failed', 'Done', 'Cancelled', 'custom']);
  });
});

import { describe, expect, it } from 'vitest';
import {
  deliveryLabel, downloadDetail, formatClock, formatRate, formatRelative, mixStepLabel, postScanLabel,
  progressLabel, recentMeta, recentText, sourceLabel,
} from './format';
import { activeCount, type ActiveMixJob, type DownloadRow, type PlayingRow, type RecentRow } from './activityTypes';

const playing = (over: Partial<PlayingRow> = {}): PlayingRow => ({
  id: 'r', source: 'subsonic', user: 'alice', client: 'Amperfy', itemId: 't1', title: 'Song', artist: 'Band',
  albumId: null, positionSec: 134, durationSec: 245, estimated: false, sampledAtMs: 0, status: 'playing',
  delivery: null, ...over,
});

const recent = (over: Partial<RecentRow>): RecentRow => ({
  atMs: 0, kind: 'play', user: 'alice', source: 'subsonic', client: 'Amperfy', itemId: 't1', title: 'Song',
  count: null, bytes: null, ...over,
});

describe('activity formatting', () => {
  it('formats clocks, rates and relative times', () => {
    expect(formatClock(134)).toBe('2:14');
    expect(formatClock(3725)).toBe('1:02:05');
    expect(formatClock(null)).toBe('0:00');
    expect(formatRate(9.4 * 1024 * 1024)).toBe('9.4 MB/s');
    expect(formatRelative(1_000, 20_000)).toBe('just now');
    expect(formatRelative(0, 4 * 60_000)).toBe('4 min ago');
    expect(formatRelative(0, 3 * 3_600_000)).toBe('3 h ago');
  });

  it('labels sources with the approved chip wording', () => {
    expect(sourceLabel('web', null)).toBe('Web player');
    expect(sourceLabel('playOn', 'Living room')).toBe('Play On · Living room');
    expect(sourceLabel('subsonic', 'Amperfy')).toBe('Subsonic · Amperfy');
    expect(sourceLabel('dlna', 'Bedroom TV')).toBe('DLNA · Bedroom TV');
    expect(sourceLabel('dlna', '')).toBe('DLNA device');
    expect(sourceLabel('radio', null)).toBe('Radio');
  });

  it('describes delivery', () => {
    expect(deliveryLabel(null, 'web', null)).toBeNull();
    expect(deliveryLabel({ transcoded: false, sourceFormat: 'FLAC', targetFormat: null, kbps: null }, 'web', null))
      .toBe('Original file');
    expect(deliveryLabel({ transcoded: true, sourceFormat: 'FLAC', targetFormat: 'MP3', kbps: 192 }, 'subsonic', 'Amperfy'))
      .toBe('FLAC → MP3 192 kbps');
    expect(deliveryLabel({ transcoded: true, sourceFormat: 'FLAC', targetFormat: 'WAV', kbps: null }, 'playOn', 'Living room'))
      .toBe('WAV for Living room');
  });

  it('marks estimated positions', () => {
    expect(progressLabel(playing(), 134)).toBe('2:14 / 4:05');
    expect(progressLabel(playing({ estimated: true }), 134)).toBe('~2:14 / 4:05');
    expect(progressLabel(playing({ durationSec: null }), 5)).toBe('0:05');
    expect(progressLabel(playing(), null)).toBeNull();
  });

  it('describes downloads, mix steps and the post-scan queue', () => {
    const sync: DownloadRow = {
      id: 's', source: 'subsonic', user: 'bob', client: 'Amperfy', label: 'offlineSync', itemId: null, title: null,
      itemsCompleted: 36, bytes: 412 * 1024 * 1024, inProgress: 1, rateBps: 0, totalBytes: null, startedAtMs: 0,
    };
    expect(downloadDetail(sync)).toBe('37 songs · 412 MB in the last 5 min');
    expect(downloadDetail({ ...sync, label: 'file', bytes: 1024 * 1024, totalBytes: 4 * 1024 * 1024 })).toBe('1.0 MB / 4.0 MB');
    const job = { status: 'rendering', step: 'rendering transitions', progressPercent: 64 } as ActiveMixJob;
    expect(mixStepLabel(job)).toBe('Rendering transitions · 64%');
    expect(mixStepLabel({ ...job, status: 'pending', step: 'queued', progressPercent: 0 })).toBe('Queued · 0%');
    expect(postScanLabel({ scans: [], postScanQueued: [['artwork', 3], ['waveforms', 2], ['bpm', 1]], deepAnalysisQueued: 0, deepAnalysisRunning: 0 }))
      .toBe('Queued after scan: artwork, waveforms, BPM');
    expect(postScanLabel({ scans: [], postScanQueued: [], deepAnalysisQueued: 0, deepAnalysisRunning: 0 })).toBeNull();
  });

  it('words recent events', () => {
    expect(recentText(recent({}))).toBe('alice played Song');
    expect(recentText(recent({ kind: 'download', count: 120 }))).toBe('alice downloaded 120 songs');
    expect(recentText(recent({ kind: 'download', source: 'mixFile', title: 'Friday' }))).toBe('alice downloaded Friday');
    expect(recentText(recent({ kind: 'mixFinished', title: 'Friday' }))).toBe("alice's mix Friday finished");
    expect(recentText(recent({ kind: 'mixFailed', title: 'Friday' }))).toBe("alice's mix Friday failed");
    expect(recentText(recent({ kind: 'scanFinished', user: null, title: 'Music' }))).toBe('Scan of Music finished');
    expect(recentText(recent({ kind: 'playOnStopped', client: 'Kitchen' }))).toBe('Play On · Kitchen stopped');
    expect(recentText(recent({ kind: 'stream', source: 'dlna', user: null, client: 'Bedroom TV' }))).toBe('Bedroom TV played Song');
    expect(recentMeta(recent({ atMs: 0 }), 60 * 60_000)).toBe('Amperfy · 1 h ago');
    expect(recentMeta(recent({ source: 'web', client: 'BoogieBox', atMs: 0 }), 10_000)).toBe('just now');
  });

  it('counts the active badge', () => {
    expect(activeCount(null)).toBe(0);
    expect(activeCount({ playing: 2, downloading: 1, mixJobs: 3, transcodes: 9, subsonicTranscodeLimit: 3, activeUsers: 2, serverJobs: 4 }))
      .toBe(6);
  });
});

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ActivityView, { ACTIVITY_FILTER_STORAGE_KEY } from './ActivityView';
import type { ActivityFeed } from '../activity/useActivityFeed';
import type { ActivitySnapshot } from '../activity/activityTypes';

const cancelMixJob = vi.fn();
vi.mock('../api', () => ({
  api: {
    albumArtUrl: (id: string) => `/api/albums/${id}/art?size=300`,
    activity: { cancelMixJob: (id: string) => cancelMixJob(id) },
  },
}));

const NOW = 1_800_000_000_000;

function snapshot(over: Partial<ActivitySnapshot> = {}): ActivitySnapshot {
  return {
    generatedAtMs: NOW,
    totals: { playing: 2, downloading: 1, mixJobs: 2, transcodes: 1, subsonicTranscodeLimit: 3, activeUsers: 3, serverJobs: 1 },
    playing: [
      {
        id: 'p1', source: 'subsonic', user: 'alice', client: 'Amperfy', itemId: 't1', title: 'Opening', artist: 'The Band',
        albumId: 'al1', positionSec: 60, durationSec: 240, estimated: true, sampledAtMs: NOW, status: 'playing',
        delivery: { transcoded: true, sourceFormat: 'FLAC', targetFormat: 'MP3', kbps: 192 },
      },
      {
        id: 'p2', source: 'playOn', user: 'bob', client: 'Living room', itemId: 't2', title: 'Second', artist: null,
        albumId: null, positionSec: 30, durationSec: 200, estimated: false, sampledAtMs: NOW, status: 'paused',
        delivery: { transcoded: false, sourceFormat: 'FLAC', targetFormat: null, kbps: null },
      },
    ],
    downloads: [{
      id: 'd1', source: 'subsonic', user: 'bob', client: 'Amperfy', label: 'offlineSync', itemId: null, title: null,
      itemsCompleted: 36, bytes: 412 * 1024 * 1024, inProgress: 1, rateBps: 2 * 1024 * 1024, totalBytes: null, startedAtMs: NOW,
    }],
    mixJobs: [
      { id: 'j1', user: 'alice', playlistName: 'Party', style: 'club_blend', trackCount: 24, status: 'rendering',
        progressPercent: 64, step: 'rendering', message: null, startedAt: null, canCancel: false },
      { id: 'j2', user: 'carol', playlistName: 'Chill', style: 'chill_blend', trackCount: 10, status: 'pending',
        progressPercent: 0, step: 'queued', message: null, startedAt: null, canCancel: true },
    ],
    serverJobs: {
      scans: [{ id: 's1', library: 'Music', status: 'running', filesScanned: 410, filesFound: 1000, startedAt: null }],
      postScanQueued: [['artwork', 4]],
      deepAnalysisQueued: 0,
      deepAnalysisRunning: 0,
    },
    recent: [
      { atMs: NOW - 120_000, kind: 'play', user: 'alice', source: 'subsonic', client: 'Amperfy', itemId: 't1', title: 'Opening', count: null, bytes: null },
      { atMs: NOW - 3_600_000, kind: 'scanFinished', user: null, source: null, client: null, itemId: 'lib1', title: 'Music', count: 12, bytes: null },
    ],
    ...over,
  };
}

function feed(snap: ActivitySnapshot | null, over: Partial<ActivityFeed> = {}): ActivityFeed {
  return { snapshot: snap, connection: 'live', updatedAt: NOW, refresh: vi.fn(), ...over };
}

describe('ActivityView', () => {
  beforeEach(() => {
    cancelMixJob.mockReset().mockResolvedValue({ ok: true });
    try { window.localStorage.removeItem(ACTIVITY_FILTER_STORAGE_KEY); } catch { /* ignore */ }
    vi.spyOn(Date, 'now').mockReturnValue(NOW);
  });

  it('shows stat tiles, every section and the recent feed', () => {
    render(<ActivityView feed={feed(snapshot())} />);
    expect(screen.getByText('Playing now')).toBeInTheDocument();
    expect(screen.getByText('1 / 3')).toBeInTheDocument();
    expect(screen.getByTitle('1 transcodes running · Subsonic limit 3')).toBeInTheDocument();

    const rows = screen.getAllByTestId('activity-playing-row');
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText('Subsonic · Amperfy')).toBeInTheDocument();
    expect(within(rows[0]).getByText(/~1:00 \/ 4:00 · FLAC → MP3 192 kbps/)).toBeInTheDocument();
    expect(within(rows[1]).getByText('Play On · Living room')).toBeInTheDocument();
    expect(within(rows[1]).getByText(/Paused · 0:30 \/ 3:20 · Original file/)).toBeInTheDocument();

    const download = screen.getByTestId('activity-download-row');
    expect(within(download).getByText('Offline sync')).toBeInTheDocument();
    expect(within(download).getByText('37 songs · 412 MB in the last 5 min')).toBeInTheDocument();
    expect(within(download).getByText('2.0 MB/s')).toBeInTheDocument();

    const mixes = screen.getAllByTestId('activity-mix-row');
    expect(within(mixes[0]).getByText('Rendering · 64%')).toBeInTheDocument();
    expect(within(mixes[0]).queryByRole('button', { name: 'Cancel mix' })).toBeNull();
    expect(within(mixes[1]).getByRole('button', { name: 'Cancel mix' })).toBeInTheDocument();

    expect(screen.getByText('Library scan · Music')).toBeInTheDocument();
    expect(screen.getByText('Queued after scan: artwork')).toBeInTheDocument();
    expect(screen.getByText('alice played Opening')).toBeInTheDocument();
    expect(screen.getByText('Amperfy · 2 min ago')).toBeInTheDocument();
    expect(screen.getByText('Scan of Music finished')).toBeInTheDocument();
    expect(screen.getByText('Last 24 hours')).toBeInTheDocument();
    expect(screen.getByTestId('activity-connection')).toHaveTextContent('Live');
  });

  it('spins a ring around the icon of busy sections only', () => {
    const { container } = render(<ActivityView feed={feed(snapshot({
      serverJobs: { scans: [], postScanQueued: [], deepAnalysisQueued: 0, deepAnalysisRunning: 0 },
      totals: { ...snapshot().totals, serverJobs: 0 },
    }))} />);
    const frames = [...container.querySelectorAll('[data-busy]')];
    // Now playing, Downloads, BoogieMix, Server jobs, Recent.
    expect(frames.map((f) => f.getAttribute('data-busy'))).toEqual(['true', 'true', 'true', 'false', 'false']);
    expect(frames[0].querySelector('.sidebar-scan-spin')).not.toBeNull();
    expect(frames[3].querySelector('.sidebar-scan-spin')).toBeNull();
    expect(screen.getAllByRole('img', { name: 'Active' })).toHaveLength(3);
  });

  it('filters sections with icon pills and remembers the choice', () => {
    render(<ActivityView feed={feed(snapshot())} />);
    const tab = screen.getByRole('tab', { name: 'Downloads' });
    fireEvent.click(tab);
    expect(tab).toHaveAttribute('aria-selected', 'true');
    expect(screen.queryByTestId('activity-playing-row')).toBeNull();
    expect(screen.getByTestId('activity-download-row')).toBeInTheDocument();
    expect(screen.queryByTestId('activity-mix-row')).toBeNull();
    expect(window.localStorage.getItem(ACTIVITY_FILTER_STORAGE_KEY)).toBe('dl');
  });

  it('shows empty states and "All quiet."', () => {
    const quiet = snapshot({
      playing: [], downloads: [], mixJobs: [], recent: [],
      serverJobs: { scans: [], postScanQueued: [], deepAnalysisQueued: 0, deepAnalysisRunning: 0 },
    });
    const { rerender } = render(<ActivityView feed={feed(quiet)} />);
    expect(screen.getByText('All quiet.')).toBeInTheDocument();
    expect(screen.getByText('Nothing yet')).toBeInTheDocument();

    rerender(<ActivityView feed={feed({ ...quiet, downloads: snapshot().downloads })} />);
    expect(screen.getByText('Nothing playing')).toBeInTheDocument();
    expect(screen.getByText('No BoogieMix jobs running')).toBeInTheDocument();
    expect(screen.getByText('No server jobs running')).toBeInTheDocument();
  });

  it('confirms before cancelling another user\'s mix and refreshes', async () => {
    const refresh = vi.fn();
    render(<ActivityView feed={feed(snapshot(), { refresh })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel mix' }));
    expect(screen.getByText(/carol's mix from Chill will be cancelled/)).toBeInTheDocument();
    const dialog = screen.getByRole('alertdialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel mix' }));
    await waitFor(() => expect(cancelMixJob).toHaveBeenCalledWith('j2'));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });

  it('says when it is polling instead of live', () => {
    render(<ActivityView feed={feed(snapshot(), { connection: 'polling', updatedAt: NOW - 5 * 60_000 })} />);
    expect(screen.getByTestId('activity-connection')).toHaveTextContent('Updated 5 min ago');
  });
});

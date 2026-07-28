/**
 * Tests the mobile playlist BoogieMix workflow.
 */

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  BoogieMixDeepAnalysisStatus,
  BoogieMixJob,
  BoogieMixOutput,
  PlaylistDeepAnalysisProgress,
} from '../../types';
import MobileBoogieMixPanel, { formatMobileDeepAnalysisProgress } from './MobileBoogieMixPanel';

const { boogieMixMock } = vi.hoisted(() => ({
  boogieMixMock: {
    deepAnalysisStatus: vi.fn(),
    playlistDeepAnalysisProgress: vi.fn(),
    listOutputs: vi.fn(),
    queuePlaylistDeepAnalysis: vi.fn(),
    createJob: vi.fn(),
    getJob: vi.fn(),
    cancelJob: vi.fn(),
    outputDownloadUrl: vi.fn((outputId: string) => `/mixes/${outputId}`),
  },
}));

vi.mock('../../api', () => ({
  api: {
    boogiemix: boogieMixMock,
  },
}));

const progress: PlaylistDeepAnalysisProgress = {
  total: 3,
  pending: 0,
  running: 0,
  done: 3,
  failed: 0,
  skipped: 0,
  notQueued: 0,
  analyzedCached: 3,
  analyzedReal: 3,
  analyzedFallback: 0,
};

const status: BoogieMixDeepAnalysisStatus = {
  enabled: true,
  runtime: {
    pythonAvailable: true,
    ffmpegAvailable: true,
    demucsCallable: true,
    torchAvailable: true,
    gpuAvailable: true,
    enabled: true,
    details: [],
    missingCapabilities: [],
    summary: 'GPU deep analysis ready',
    python: { available: true, version: '3.11', detail: null },
    ffmpeg: { available: true, version: '7', detail: null },
    demucs: { available: true, version: '4', detail: null },
    torch: { available: true, version: '2', detail: null },
    gpu: { available: true, version: null, detail: null },
  },
  queue: { pending: 0, running: 0, failed: 0, skipped: 0, done: 3 },
  cache: {
    analyzedTracks: 3,
    estimatedBytes: 1024,
    oldestCreatedAt: null,
    newestCreatedAt: null,
  },
};

const output: BoogieMixOutput = {
  id: 'output-1',
  job_id: 'job-1',
  playlist_id: '7',
  file_name: 'some-electro-mix.m4a',
  duration_sec: 720,
  file_size_bytes: 4096,
  format: 'm4a',
  created_at: '2026-07-28T00:00:00Z',
};

function job(overrides: Partial<BoogieMixJob> = {}): BoogieMixJob {
  return {
    id: 'job-1',
    playlist_id: '7',
    user_id: '1',
    status: 'done',
    progress_percent: 100,
    current_step: 'Complete',
    last_message: 'Mix ready',
    cancel_requested: 0,
    default_crossfade_sec: 24,
    output_id: 'output-1',
    mix_style: 'long_build',
    mix_quality: 'high_quality',
    used_deep_analysis: true,
    deep_analysis_status: 'ready',
    deep_analysis_ready_count: 3,
    deep_analysis_total_count: 3,
    deep_analysis_missing_reason: null,
    started_at: '2026-07-28T00:00:00Z',
    finished_at: '2026-07-28T00:10:00Z',
    created_at: '2026-07-28T00:00:00Z',
    updated_at: '2026-07-28T00:10:00Z',
    transitions: [],
    logs: [],
    ...overrides,
  };
}

describe('MobileBoogieMixPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    boogieMixMock.deepAnalysisStatus.mockResolvedValue(status);
    boogieMixMock.playlistDeepAnalysisProgress.mockResolvedValue(progress);
    boogieMixMock.listOutputs.mockResolvedValue([output]);
    boogieMixMock.queuePlaylistDeepAnalysis.mockResolvedValue({ queued: 2 });
    boogieMixMock.createJob.mockResolvedValue({ jobId: 'job-1' });
    boogieMixMock.getJob.mockResolvedValue(job());
    boogieMixMock.cancelJob.mockResolvedValue({ ok: true });
  });

  it('loads readiness and exposes the latest completed output only after expansion', async () => {
    render(<MobileBoogieMixPanel playlistId="7" playlistName="Some Electro" trackCount={3} />);

    const toggle = screen.getByRole('button', { name: /Build a BoogieMix/i });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle).toHaveStyle({ minHeight: '72px' });
    expect(boogieMixMock.deepAnalysisStatus).not.toHaveBeenCalled();

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(await screen.findByText('GPU deep analysis ready')).toBeInTheDocument();
    expect(screen.getByText('3/3 tracks have real deep analysis')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Download' })).toHaveAttribute('href', '/mixes/output-1');
  });

  it('queues deep analysis and creates a mix with the chosen mobile controls', async () => {
    render(<MobileBoogieMixPanel playlistId="7" playlistName="Some Electro" trackCount={3} />);
    fireEvent.click(screen.getByRole('button', { name: /Build a BoogieMix/i }));
    await screen.findByText('GPU deep analysis ready');

    fireEvent.click(screen.getByRole('button', { name: 'Analyze playlist' }));
    await waitFor(() => expect(boogieMixMock.queuePlaylistDeepAnalysis).toHaveBeenCalledWith('7'));

    fireEvent.change(screen.getByRole('combobox', { name: 'BoogieMix style' }), {
      target: { value: 'long_build' },
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'BoogieMix quality' }), {
      target: { value: 'high_quality' },
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'BoogieMix transition length' }), {
      target: { value: '24' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Build Some Electro' }));

    await waitFor(() => expect(boogieMixMock.createJob).toHaveBeenCalledWith(
      '7',
      'long_build',
      'high_quality',
      24,
    ));
    expect(await screen.findByText('Mix done')).toBeInTheDocument();
    expect(screen.getByText('Deep analysis used', { exact: false })).toBeInTheDocument();
  });

  it('cancels an active planning job and formats queued readiness states', async () => {
    boogieMixMock.listOutputs.mockResolvedValue([]);
    boogieMixMock.getJob
      .mockResolvedValueOnce(job({
        status: 'planning',
        progress_percent: 42,
        current_step: 'Planning transitions',
        finished_at: null,
        output_id: null,
      }))
      .mockResolvedValueOnce(job({
        status: 'canceled',
        progress_percent: 42,
        current_step: 'Canceled',
        finished_at: '2026-07-28T00:02:00Z',
        output_id: null,
      }));

    const { unmount } = render(
      <MobileBoogieMixPanel playlistId="7" playlistName="Some Electro" trackCount={3} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Build a BoogieMix/i }));
    await screen.findByText('GPU deep analysis ready');
    fireEvent.click(screen.getByRole('button', { name: 'Build Some Electro' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(boogieMixMock.cancelJob).toHaveBeenCalledWith('job-1'));
    expect(formatMobileDeepAnalysisProgress({
      ...progress,
      analyzedReal: 1,
      running: 1,
      pending: 1,
    })).toBe('1/3 ready · 1 running · 1 queued');
    unmount();
  });
});

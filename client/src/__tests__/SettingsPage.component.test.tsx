/**
 * Tests Settings Page.Component.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsPage from '../components/SettingsPage';
import { DEFAULT_SETTINGS } from '../types';

const { apiMock, getStreamDirectMock, setStreamDirectMock } = vi.hoisted(() => ({
  apiMock: {
    libraries: { list: vi.fn(), add: vi.fn(), rename: vi.fn(), remove: vi.fn(), addFolder: vi.fn(), removeFolder: vi.fn(), scan: vi.fn() },
    debugTestPath: vi.fn(),
    scanJobs: { active: vi.fn(), get: vi.fn() },
    schedules: { list: vi.fn(), upsert: vi.fn(), remove: vi.fn() },
    admin: { queues: vi.fn(), providerUsage: vi.fn(), cancelScanJob: vi.fn(), cancelPostScanJob: vi.fn(), failPostScanJob: vi.fn(), retryPostScanJob: vi.fn(), enqueuePostScanJob: vi.fn() },
    settings: { get: vi.fn(), update: vi.fn() },
    waveforms: { status: vi.fn(), runMap: vi.fn() },
    integrations: { spotifyTest: vi.fn(), geniusTest: vi.fn() },
    dlna: { status: vi.fn() },
    boogiemix: {
      deepAnalysisStatus: vi.fn(),
      queueLibraryDeepAnalysis: vi.fn(),
      pauseDeepAnalysisBackground: vi.fn(),
      resumeDeepAnalysisBackground: vi.fn(),
      clearDeepAnalysisCache: vi.fn(),
    },
  },
  getStreamDirectMock: vi.fn(),
  setStreamDirectMock: vi.fn(),
}));

vi.mock('../api', () => ({
  api: apiMock,
  getStreamDirect: getStreamDirectMock,
  setStreamDirect: setStreamDirectMock,
}));

function okJson(data: unknown, contentType = 'application/json') {
  return Promise.resolve({
    ok: true,
    headers: { get: (key: string) => (key.toLowerCase() === 'content-type' ? contentType : null) },
    json: () => Promise.resolve(data),
  } as unknown as Response);
}

describe('SettingsPage component flows', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());
    vi.stubGlobal('confirm', vi.fn(() => true));

    getStreamDirectMock.mockReturnValue(false);
    apiMock.libraries.list.mockResolvedValue([{ id: '1', name: 'Main Library', path: 'D:\\Music', primary_path: 'D:\\Music', folder_count: 2, folders: [{ id: 'f1', library_id: '1', path: 'D:\\Music', position: 0 }, { id: 'f2', library_id: '1', path: 'D:\\More Music', position: 1 }], library_type: 'music', added_at: '2026-01-01', last_scan: null, track_count: 12 }]);
    apiMock.libraries.add.mockResolvedValue({ ok: true });
    apiMock.libraries.rename.mockResolvedValue({ id: '1', name: 'Renamed Library', path: 'D:\\Music', primary_path: 'D:\\Music', folder_count: 2, folders: [{ id: 'f1', library_id: '1', path: 'D:\\Music', position: 0 }, { id: 'f2', library_id: '1', path: 'D:\\More Music', position: 1 }], library_type: 'music', added_at: '2026-01-01', last_scan: null, track_count: 12 });
    apiMock.libraries.remove.mockResolvedValue({ ok: true });
    apiMock.libraries.addFolder.mockResolvedValue({ ok: true });
    apiMock.libraries.removeFolder.mockResolvedValue({ ok: true });
    apiMock.libraries.scan.mockResolvedValue({ jobId: 55 });
    apiMock.debugTestPath.mockResolvedValue({ exists: true, isDirectory: true, displayName: 'More' });
    apiMock.scanJobs.active.mockResolvedValue([]);
    apiMock.scanJobs.get.mockResolvedValue({
      id: '55',
      library_id: '1',
      status: 'done',
      files_found: 12,
      files_scanned: 12,
      errors: 0,
      started_at: '2026-03-14T10:10:00.000Z',
      finished_at: '2026-03-14T10:15:00.000Z',
    });
    apiMock.schedules.list.mockResolvedValue([
      {
        id: '1',
        library_id: '1',
        enabled: 1,
        frequency_hours: 24,
        last_run: null,
        next_run: null,
      },
    ]);
    apiMock.schedules.upsert.mockResolvedValue({ ok: true });
    apiMock.schedules.remove.mockResolvedValue({ ok: true });
    apiMock.admin.queues.mockResolvedValue({
      fetched_at: '2026-03-14T10:15:00.000Z',
      queues: {
        scan: [
          {
            id: '11',
            status: 'running',
            library_id: '1',
            library_name: 'Main Library',
            job_type: null,
            files_scanned: 42,
            files_found: 100,
            errors: 0,
            started_at: '2026-03-14T10:10:00.000Z',
            finished_at: null,
            current_step: null,
            playlist_name: null,
            track_title: null,
            error_message: null,
          },
        ],
        postScan: [
          {
            id: '21',
            status: 'pending',
            library_id: '1',
            library_name: 'Main Library',
            job_type: 'sync_artist_styles',
            files_scanned: null,
            files_found: null,
            errors: null,
            started_at: null,
            finished_at: null,
            current_step: null,
            playlist_name: null,
            track_title: null,
            error_message: null,
          },
        ],
        mix: [],
        deepAnalysis: [],
      },
    });
    apiMock.admin.cancelScanJob.mockResolvedValue({ ok: true, id: '11', status: 'cancelled' });
    apiMock.admin.cancelPostScanJob.mockResolvedValue({ ok: true, id: '21', status: 'cancelled' });
    apiMock.admin.failPostScanJob.mockResolvedValue({ ok: true, id: '21', status: 'failed' });
    apiMock.admin.retryPostScanJob.mockResolvedValue({ ok: true, id: '21', status: 'pending' });
    apiMock.admin.enqueuePostScanJob.mockResolvedValue({ ok: true, id: '31', status: 'pending', job_type: 'warm_track_lyrics', library_id: '1' });
    apiMock.admin.providerUsage.mockResolvedValue({
      fetched_at: '2026-03-14T10:15:00.000Z',
      providers: [
        {
          provider: 'lastfm',
          total_count: 130,
          last_used_at: '2026-03-14T10:14:00.000Z',
          usage_breakdown: {
            cache_write: 100,
            cache_hit_served: 30,
          },
        },
        {
          provider: 'spotify',
          total_count: 34,
          last_used_at: '2026-03-14T09:00:00.000Z',
          usage_breakdown: {
            cache_write: 34,
          },
        },
      ],
      rows: [
        {
          provider: 'lastfm',
          entity_type: 'artist_info',
          usage_type: 'cache_write',
          count: 100,
          last_used_at: '2026-03-14T10:14:00.000Z',
        },
        {
          provider: 'spotify',
          entity_type: 'artist_artwork',
          usage_type: 'cache_write',
          count: 34,
          last_used_at: '2026-03-14T09:00:00.000Z',
        },
      ],
    });
    apiMock.settings.get.mockResolvedValue({
      discogsToken: 'old-token',
      spotifyClientId: 'client-id',
      spotifyClientSecret: 'client-secret',
      geniusClientId: 'genius-id',
      geniusClientSecret: 'genius-secret',
      lastfmKey: 'lastfm-key',
      dlnaEnabled: 'true',
      dlnaFriendlyName: 'BoogieBox',
      dlnaPort: '8200',
      dlnaMediaMode: 'audio',
      dlnaForceVideoTranscode: 'false',
      transcodeQuality: 'low',
      waveformGenerateOnMissing: 'true',
      waveformBackgroundEnabled: 'false',
      waveformBackgroundFrequencyHours: '24',
      waveformBackgroundBatchSize: '100',
      scanDebugLoggingEnabled: 'false',
      boogiemixOutputFolder: 'D:\\Mixes',
    });
    apiMock.settings.update.mockResolvedValue({ ok: true });
    apiMock.waveforms.status.mockResolvedValue({
      enabled: false,
      generateOnMissing: true,
      frequencyHours: 24,
      batchSize: 100,
      lastRun: null,
      nextRun: null,
      inProgress: false,
      totalTracks: 10,
      mappedTracks: 6,
      missingTracks: 4,
      activeRun: null,
    });
    apiMock.waveforms.runMap.mockResolvedValue({
      started: true,
      inProgress: false,
      reason: 'manual',
      startedAt: '2026-02-28T00:00:00.000Z',
      finishedAt: '2026-02-28T00:00:01.000Z',
      batchSize: 100,
      totalMissing: 4,
      processed: 4,
      generated: 2,
      skipped: 2,
      errors: 0,
    });
    apiMock.integrations.spotifyTest.mockResolvedValue({ ok: true });
    apiMock.integrations.geniusTest.mockResolvedValue({ ok: true });
    apiMock.dlna.status.mockResolvedValue({ running: true, port: 8200, friendlyName: 'BoogieBox' });
    apiMock.boogiemix.deepAnalysisStatus.mockResolvedValue({
      enabled: false,
      runtime: {
        pythonAvailable: true,
        ffmpegAvailable: true,
        demucsCallable: false,
        torchAvailable: false,
        gpuAvailable: false,
        enabled: false,
        details: [],
        missingCapabilities: ['torch'],
        summary: 'Missing: torch.',
        python: { available: true, version: 'Python 3.12', detail: 'app-local Python runtime' },
        ffmpeg: { available: true, version: null, detail: null },
        demucs: { available: false, version: null, detail: null },
        torch: { available: false, version: null, detail: null },
        gpu: { available: false, version: null, detail: 'No CUDA GPU reported' },
      },
      queue: { pending: 2, running: 1, failed: 3, skipped: 4, done: 5 },
      cache: {
        analyzedTracks: 12,
        estimatedBytes: 2048,
        oldestCreatedAt: '2026-02-01T00:00:00.000Z',
        newestCreatedAt: '2026-02-02T00:00:00.000Z',
      },
      controls: {
        backgroundMode: 'off',
        pauseBackground: false,
      },
    });
    apiMock.boogiemix.queueLibraryDeepAnalysis.mockResolvedValue({ queued: 12 });
    apiMock.boogiemix.pauseDeepAnalysisBackground.mockResolvedValue({ ok: true });
    apiMock.boogiemix.resumeDeepAnalysisBackground.mockResolvedValue({ ok: true });
    apiMock.boogiemix.clearDeepAnalysisCache.mockResolvedValue({ ok: true, deletedCacheRows: 12, deletedJobRows: 5 });
  });

  it('shows the About tab with the Ko-fi support link', () => {
    const { rerender } = render(
      <SettingsPage
        currentUser={{ id: '2', username: 'listener', role: 'user', canScan: false, canEditMetadata: false }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    const userTabs = screen.getByRole('button', { name: 'About' }).parentElement;
    expect(within(userTabs!).getAllByRole('button').map((button) => button.textContent)).toEqual([
      '🎨 Appearance',
      '📚 Libraries',
      'About',
    ]);

    rerender(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    const adminTabs = screen.getByRole('button', { name: 'About' }).parentElement;
    expect(within(adminTabs!).getAllByRole('button').map((button) => button.textContent)).toEqual([
      '🎨 Appearance',
      '📚 Libraries',
      '🕐 Auto-Scan',
      '🔌 Integrations',
      '⚙ Advanced',
      '👥 Users',
      'About',
    ]);

    fireEvent.click(screen.getByRole('button', { name: 'About' }));

    expect(screen.getByText(/self-hosted music library/i)).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'Support BoogieBox on Ko-fi' });
    expect(link).toHaveAttribute('href', 'https://ko-fi.com/yronnen');
    expect(link).toHaveAttribute('target', '_blank');
  });

  it('manages libraries from the dedicated settings tab', async () => {
    render(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Libraries/i }));
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());
    expect(screen.getByText('Main Library')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText(/Folder path/i), { target: { value: 'D:\\More' } });
    expect(screen.queryByRole('option', { name: 'Movies' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Test' }));
    await waitFor(() => expect(apiMock.debugTestPath).toHaveBeenCalledWith('D:\\More'));
    expect(await screen.findByText(/Path OK/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Queue Folder/i }));
    fireEvent.change(screen.getByPlaceholderText(/Name \(optional\)/i), { target: { value: 'More' } });
    fireEvent.click(screen.getByRole('button', { name: /^Add$/i }));
    await waitFor(() => expect(apiMock.libraries.add).toHaveBeenCalledWith(['D:\\More'], 'More'));

    fireEvent.click(screen.getByRole('button', { name: /^Rename$/i }));
    fireEvent.change(screen.getByLabelText(/Library name for D:\\Music/i), { target: { value: 'Renamed Library' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(apiMock.libraries.rename).toHaveBeenCalledWith('1', 'Renamed Library'));

    expect(screen.getByText(/Primary: D:\\Music/i)).toBeInTheDocument();
    expect(screen.getByText(/D:\\More Music/i)).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/Add another folder/i), { target: { value: 'D:\\Third' } });
    fireEvent.click(screen.getByRole('button', { name: /^Add Folder$/i }));
    await waitFor(() => expect(apiMock.libraries.addFolder).toHaveBeenCalledWith('1', 'D:\\Third'));

    fireEvent.click(screen.getAllByTitle('Remove folder')[0]);
    await waitFor(() => expect(apiMock.libraries.removeFolder).toHaveBeenCalledWith('1', 'f1'));

    fireEvent.click(screen.getByRole('button', { name: /^Scan$/i }));
    await waitFor(() => expect(apiMock.libraries.scan).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByTitle('Remove library'));
    await waitFor(() => expect(apiMock.libraries.remove).toHaveBeenCalledWith('1'));
  }, 15000);

  it('shows the unique-name error when renaming a library to an existing name', async () => {
    apiMock.libraries.rename.mockRejectedValueOnce(new Error('Library name already exists. Please choose a unique name.'));
    render(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Libraries/i }));
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: /^Rename$/i }));
    fireEvent.change(screen.getByLabelText(/Library name for D:\\Music/i), { target: { value: 'Main Library' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));

    expect(await screen.findByText(/Library name already exists\. Please choose a unique name\./i)).toBeInTheDocument();
  });

  it('loads schedules and saves updated frequency', async () => {
    render(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Auto-Scan/i }));
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(apiMock.admin.queues).toHaveBeenCalledTimes(1));
    expect(screen.getByText('Queue & Maintenance')).toBeInTheDocument();
    expect(screen.getByText('Live queue')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Show raw queue snapshot' }));
    const queueSnapshot = screen.getByLabelText('Queue Snapshot') as HTMLTextAreaElement;
    expect(queueSnapshot.value).toContain('scan #11');
    expect(queueSnapshot.value).toContain('post-scan #21');

    const combo = screen.getByDisplayValue('Daily') as HTMLSelectElement;
    fireEvent.change(combo, { target: { value: '12' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(apiMock.schedules.upsert).toHaveBeenCalledWith('1', true, 12));
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(apiMock.admin.queues).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole('button', { name: 'Stop scan' }));
    await waitFor(() => expect(apiMock.admin.cancelScanJob).toHaveBeenCalledWith('11'));

    fireEvent.click(screen.getByRole('button', { name: 'Cancel task' }));
    await waitFor(() => expect(apiMock.admin.cancelPostScanJob).toHaveBeenCalledWith('21'));

    fireEvent.click(screen.getByRole('button', { name: 'Warm lyrics' }));
    await waitFor(() => expect(apiMock.admin.enqueuePostScanJob).toHaveBeenCalledWith('1', 'warm_track_lyrics'));
  }, 15000);

  it('validates and saves DLNA settings and toggles browser-local transcoding preference', async () => {
    const onStreamDirectChange = vi.fn();
    render(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
        onStreamDirectChange={onStreamDirectChange}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Advanced/i }));
    await waitFor(() => expect(apiMock.dlna.status).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(apiMock.settings.get).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.waveforms.status).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalledTimes(1));
    expect(screen.getByText('BoogieMix deep analysis')).toBeInTheDocument();
    expect(screen.getByText('Missing: torch.')).toBeInTheDocument();
    expect(screen.getByText(/Queue: 2 pending \/ 1 running \/ 3 failed \/ 4 skipped \/ 5 done/i)).toBeInTheDocument();
    expect(screen.getByText(/Cache:/i)).toHaveTextContent('12 tracks analyzed, about 2.0 KB stored in SQLite');
    expect(screen.getByDisplayValue('Off')).toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue('Off'), { target: { value: 'all_music' } });
    expect(screen.getByText(/Full-library deep analysis can take many hours/i)).toBeInTheDocument();
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      boogiemixDeepAnalysisBackgroundMode: 'all_music',
    }));

    fireEvent.change(screen.getByLabelText('BoogieMix deep-analysis library'), { target: { value: '1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Analyze Library' }));
    await waitFor(() => expect(apiMock.boogiemix.queueLibraryDeepAnalysis).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('Queued 12 tracks')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Pause Background' }));
    await waitFor(() => expect(apiMock.boogiemix.pauseDeepAnalysisBackground).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole('button', { name: 'Clear Cache' }));
    await waitFor(() => expect(apiMock.boogiemix.clearDeepAnalysisCache).toHaveBeenCalledTimes(1));

    const streamToggle = screen.getByTitle(/Transcoding enabled/i);
    fireEvent.click(streamToggle);
    expect(setStreamDirectMock).toHaveBeenCalledWith(true);
    expect(onStreamDirectChange).toHaveBeenCalledWith(true);

    const batchSelect = screen.getByDisplayValue('100 tracks') as HTMLSelectElement;
    fireEvent.change(batchSelect, { target: { value: '250' } });
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      waveformBackgroundBatchSize: '250',
    }));

    const debugCard = screen.getByText('Scan debug logging').closest('div')?.parentElement?.parentElement;
    if (!debugCard) throw new Error('Scan debug logging card not found');
    fireEvent.click(within(debugCard).getByTitle('Off'));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      scanDebugLoggingEnabled: 'true',
    }));
    expect(await screen.findByText('Debug logging enabled')).toBeInTheDocument();

    const mixFolderInput = screen.getByDisplayValue('D:\\Mixes') as HTMLInputElement;
    fireEvent.change(mixFolderInput, { target: { value: 'D:\\DJ\\Sets' } });
    fireEvent.click(screen.getByRole('button', { name: /Save Folder/i }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      boogiemixOutputFolder: 'D:\\DJ\\Sets',
    }));

    fireEvent.click(screen.getByRole('button', { name: /Run Mapping Now/i }));
    await waitFor(() => expect(apiMock.waveforms.runMap).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByDisplayValue('Low (192 kbps CBR)'), { target: { value: 'high' } });
    fireEvent.click(screen.getByRole('button', { name: /Save Quality/i }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ transcodeQuality: 'high' }));

    const portInput = screen.getByDisplayValue('8200') as HTMLInputElement;
    fireEvent.change(portInput, { target: { value: '80' } });
    fireEvent.click(screen.getByRole('button', { name: /Save DLNA Settings/i }));
    expect(await screen.findByText(/Port must be an integer between 1024 and 65535/i)).toBeInTheDocument();

    expect(screen.queryByLabelText('DLNA library types')).not.toBeInTheDocument();
    expect(screen.queryByTitle(/Forced DLNA video transcoding disabled/i)).not.toBeInTheDocument();
    fireEvent.change(portInput, { target: { value: '8300' } });
    fireEvent.click(screen.getByRole('button', { name: /Save DLNA Settings/i }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      dlnaEnabled: 'true',
      dlnaFriendlyName: 'BoogieBox',
      dlnaPort: '8300',
    }));
  });

  it('executes integrations test/save actions for Discogs, Spotify, and Last.fm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = String(input);
      if (url.includes('api.discogs.com')) return okJson({});
      if (url.includes('ws.audioscrobbler.com')) return okJson({ artists: { artist: [{ name: 'A' }] } });
      return okJson({});
    });

    render(
      <SettingsPage
        currentUser={{ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }}
        onLogout={() => {}}
        settings={DEFAULT_SETTINGS}
        onSettingsChange={() => {}}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: /Integrations/i }));
    await waitFor(() => expect(apiMock.settings.get).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.admin.providerUsage).toHaveBeenCalledTimes(1));

    const discogsInput = screen.getByPlaceholderText(/Discogs personal access token/i);
    fireEvent.change(discogsInput, { target: { value: 'new-discogs-token' } });
    const discogsCard = discogsInput.closest('div');
    if (!discogsCard) throw new Error('Discogs section not found');
    fireEvent.click(within(discogsCard.parentElement ?? discogsCard).getAllByRole('button', { name: 'Test' })[0]);
    expect(await screen.findByText(/Discogs connection OK/i)).toBeInTheDocument();
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('api.discogs.com/database/search'),
      expect.objectContaining({
        headers: expect.objectContaining({ 'User-Agent': 'BoogieBox/1.0' }),
      })
    ));
    expect(screen.getByText(/When you open an album, BoogieBox checks the album's folder/i)).toBeInTheDocument();
    fireEvent.click(within(discogsCard.parentElement ?? discogsCard).getAllByRole('button', { name: 'Save' })[0]);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ discogsToken: 'new-discogs-token' }));

    expect(screen.queryByPlaceholderText(/Genius Client ID/i)).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/Genius Client Secret/i)).not.toBeInTheDocument();
    expect(apiMock.integrations.geniusTest).not.toHaveBeenCalled();

    fireEvent.change(screen.getByPlaceholderText(/Last\.fm API key/i), { target: { value: 'lfm-2' } });
    fireEvent.click(screen.getAllByRole('button', { name: 'Test' })[1]);
    expect(await screen.findByText(/Last\.fm connection OK/i)).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole('button', { name: 'Save' })[1]);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ lastfmKey: 'lfm-2' }));

    const lastfmHeading = screen.getAllByText('Last.fm', { selector: 'div' })[0];
    const spotifyHeading = screen.getByText(/Artist Images: Deezer \+ Spotify Fallback/i);
    expect(lastfmHeading.compareDocumentPosition(spotifyHeading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.change(screen.getByPlaceholderText(/Spotify Client ID/i), { target: { value: 'id-2' } });
    fireEvent.change(screen.getByPlaceholderText(/Spotify Client Secret/i), { target: { value: 'secret-2' } });
    fireEvent.click(screen.getAllByRole('button', { name: 'Test' })[2]);
    await waitFor(() => expect(apiMock.integrations.spotifyTest).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getAllByRole('button', { name: 'Save' })[2]);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({
      spotifyClientId: 'id-2',
      spotifyClientSecret: 'secret-2',
    }));

    expect(screen.queryByPlaceholderText(/TMDb API key/i)).not.toBeInTheDocument();

    expect(screen.getByText('Provider Usage')).toBeInTheDocument();
    expect(screen.getByText('Metadata provider usage')).toBeInTheDocument();
    expect(screen.getAllByText('Last.fm').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Spotify').length).toBeGreaterThan(0);
    expect(screen.getByText(/cache write: 100/i)).toBeInTheDocument();
    expect(screen.getByText(/cache hit served: 30/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Show usage rows' }));
    expect(screen.getByText('artist_info')).toBeInTheDocument();
    expect(screen.getByText('artist_artwork')).toBeInTheDocument();
  });
});

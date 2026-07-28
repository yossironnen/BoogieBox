import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../api';
import MobileAdminSettingsPanel from './MobileAdminSettingsPanel';

const library = {
  id: 'library-1',
  name: 'Studio Library',
  path: 'D:\\Music',
  primary_path: 'D:\\Music',
  added_at: '2026-07-01T00:00:00Z',
  last_scan: null,
  track_count: 1240,
  folder_count: 2,
};

const queues = {
  fetched_at: '2026-07-28T15:00:00Z',
  queues: {
    scan: [{ id: 'scan-1' }],
    postScan: [],
    mix: [{ id: 'mix-1' }],
    deepAnalysis: [{ id: 'deep-1' }, { id: 'deep-2' }],
  },
};

const deepStatus = {
  enabled: true,
  runtime: {
    enabled: true,
    summary: 'Deep analysis is ready.',
  },
  queue: { pending: 2, running: 1, failed: 0, skipped: 3, done: 40 },
  cache: {
    analyzedTracks: 438,
    estimatedBytes: 2048,
    oldestCreatedAt: null,
    newestCreatedAt: null,
  },
  controls: {
    backgroundMode: 'playlists_only',
    pauseBackground: false,
  },
};

describe('MobileAdminSettingsPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(api.libraries, 'list').mockResolvedValue([library] as any);
    vi.spyOn(api.admin, 'queues').mockResolvedValue(queues as any);
    vi.spyOn(api.boogiemix, 'deepAnalysisStatus').mockResolvedValue(deepStatus as any);
    vi.spyOn(api.libraries, 'scan').mockResolvedValue({ jobId: 'scan-new' });
    vi.spyOn(api.boogiemix, 'queueLibraryDeepAnalysis').mockResolvedValue({ queued: 9 });
    vi.spyOn(api.boogiemix, 'pauseDeepAnalysisBackground').mockResolvedValue({ ok: true });
    vi.spyOn(api.boogiemix, 'resumeDeepAnalysisBackground').mockResolvedValue({ ok: true });
    vi.spyOn(api.settings, 'update').mockResolvedValue({ ok: true });
  });

  it('loads admin status only after disclosure and renders compact operations', async () => {
    render(<MobileAdminSettingsPanel />);

    expect(api.libraries.list).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));

    expect((await screen.findAllByText('Studio Library')).length).toBeGreaterThan(0);
    expect(screen.getByText('1,240 tracks · 2 folders')).toBeInTheDocument();
    expect(screen.getByText('Deep analysis is ready.')).toBeInTheDocument();
    expect(screen.getByText('438')).toBeInTheDocument();
    expect(screen.getByText('Queued jobs').previousSibling).toHaveTextContent('4');
    expect(screen.getByRole('button', { name: 'Scan' })).toHaveStyle({ minHeight: '44px' });
  });

  it('queues scans and analysis, changes the background mode, and pauses work', async () => {
    render(<MobileAdminSettingsPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));
    await screen.findAllByText('Studio Library');

    fireEvent.click(screen.getByRole('button', { name: 'Scan' }));
    await waitFor(() => expect(api.libraries.scan).toHaveBeenCalledWith('library-1'));
    expect(await screen.findByText('Scan queued for Studio Library.')).toBeInTheDocument();

    fireEvent.change(screen.getByRole('combobox', { name: 'Background analysis' }), {
      target: { value: 'all_music' },
    });
    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({
        boogiemixDeepAnalysisBackgroundMode: 'all_music',
      });
    });

    fireEvent.change(screen.getByRole('combobox', { name: 'Deep-analysis library' }), {
      target: { value: 'library-1' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Analyze' }));
    await waitFor(() => {
      expect(api.boogiemix.queueLibraryDeepAnalysis).toHaveBeenCalledWith('library-1');
    });
    expect(await screen.findByText('Queued 9 tracks from Studio Library.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Pause background analysis' }));
    await waitFor(() => expect(api.boogiemix.pauseDeepAnalysisBackground).toHaveBeenCalled());
  });

  it('keeps partial status available when one source fails', async () => {
    vi.mocked(api.admin.queues).mockRejectedValue(new Error('queue offline'));
    render(<MobileAdminSettingsPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));

    expect(await screen.findByText('Could not load 1 admin status source.')).toBeInTheDocument();
    expect(screen.getAllByText('Studio Library').length).toBeGreaterThan(0);
    expect(screen.getByText('Deep analysis is ready.')).toBeInTheDocument();
  });

  it('shows limited all-music status and resumes paused background work', async () => {
    vi.mocked(api.libraries.list).mockResolvedValue([]);
    vi.mocked(api.boogiemix.deepAnalysisStatus).mockResolvedValue({
      ...deepStatus,
      runtime: null,
      controls: {
        backgroundMode: 'all_music',
        pauseBackground: true,
      },
    } as any);
    render(<MobileAdminSettingsPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));

    expect(await screen.findByText('No libraries configured.')).toBeInTheDocument();
    expect(screen.getByText('Runtime status unavailable.')).toBeInTheDocument();
    expect(screen.getByText('Limited')).toBeInTheDocument();
    expect(screen.getByText('All music can create sustained CPU and disk activity.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Resume background analysis' }));
    await waitFor(() => expect(api.boogiemix.resumeDeepAnalysisBackground).toHaveBeenCalled());
  });

  it('reports action failures and refreshes without reloading on disclosure alone', async () => {
    vi.mocked(api.libraries.scan).mockRejectedValue(new Error('scan unavailable'));
    render(<MobileAdminSettingsPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));
    await screen.findAllByText('Studio Library');

    fireEvent.click(screen.getByRole('button', { name: 'Scan' }));
    expect(await screen.findByText('scan unavailable')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Hide admin tools' }));
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));
    expect(api.libraries.list).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(api.libraries.list).toHaveBeenCalledTimes(2));
  });

  it('keeps the empty operations frame usable when every source fails', async () => {
    vi.mocked(api.libraries.list).mockRejectedValue(new Error('library offline'));
    vi.mocked(api.admin.queues).mockRejectedValue(new Error('queue offline'));
    vi.mocked(api.boogiemix.deepAnalysisStatus).mockRejectedValue(new Error('analysis offline'));
    render(<MobileAdminSettingsPanel />);
    fireEvent.click(screen.getByRole('button', { name: 'Open admin tools' }));

    expect(await screen.findByText('Could not load 3 admin status sources.')).toBeInTheDocument();
    expect(screen.getByText('No libraries configured.')).toBeInTheDocument();
    expect(screen.getByText('Runtime status unavailable.')).toBeInTheDocument();
  });
});

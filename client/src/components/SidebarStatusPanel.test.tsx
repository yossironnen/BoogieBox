/** Tests the unified desktop sidebar status panel. */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import SidebarStatusPanel, {
  describeDeepAnalysis,
  describeScan,
  describeTranscoding,
} from './SidebarStatusPanel';
import type { BoogieMixDeepAnalysisStatus, Library, ScanJob } from '../types';

const user = {
  id: 'user-1',
  username: 'admin',
  role: 'admin',
  canManageLibraries: true,
  canEditMetadata: true,
} as const;

const libraries: Library[] = [{
  id: 'library-1',
  path: 'D:\\Music',
  name: 'Main Music',
  added_at: '2026-01-01',
  last_scan: null,
  track_count: 3912,
}];

const activeScanJobs: ScanJob[] = [{
  id: 'scan-1',
  library_id: 'library-1',
  status: 'running',
  started_at: '2026-01-01',
  finished_at: null,
  files_found: 3912,
  files_scanned: 1284,
  errors: 0,
}];

const deepStatus: BoogieMixDeepAnalysisStatus = {
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
  queue: { pending: 1, running: 1, failed: 0, skipped: 0, done: 12 },
  cache: { analyzedTracks: 12, estimatedBytes: 1024, oldestCreatedAt: null, newestCreatedAt: null },
};

function renderPanel(
  collapsed = false,
  onLogout = vi.fn(),
  streamDirect = false,
  scanJobs = activeScanJobs,
) {
  return render(
    <SidebarStatusPanel
      currentUser={user}
      collapsed={collapsed}
      streamDirect={streamDirect}
      ffmpegAvailable
      transcodeQuality="high"
      activeScanJobs={scanJobs}
      libraries={libraries}
      deepAnalysisStatus={deepStatus}
      onLogout={onLogout}
    />,
  );
}

describe('SidebarStatusPanel', () => {
  it('builds approved status descriptions from live state', () => {
    expect(describeTranscoding(false, true, 'high')).toBe('Transcoding on (320 kbps)');
    expect(describeTranscoding(true, true, 'high')).toBe('Transcoding off');
    expect(describeScan(activeScanJobs, libraries)).toBe('Library scan: Main Music • 1,284 / 3,912 files');
    expect(describeScan([], libraries)).toBe('Library scan: Idle');
    expect(describeDeepAnalysis(deepStatus)).toContain('BoogieMix deep analysis • GPU deep analysis ready • Queue: 1 pending / 1 running');
    expect(describeDeepAnalysis(null)).toBe('BoogieMix deep analysis: Status unavailable');
  });

  it('renders status icons above the bottom identity row and reveals details on hover', () => {
    renderPanel();
    const panel = screen.getByTestId('sidebar-status-panel');
    expect(panel.children[0]).toContainElement(screen.getByTestId('sidebar-status-scan'));
    expect(panel.children[1]).toHaveTextContent('admin');
    expect(screen.getByTestId('sidebar-status-deep-analysis')).toHaveTextContent('2');

    fireEvent.mouseEnter(screen.getByTestId('sidebar-status-scan'));
    expect(screen.getByRole('tooltip')).toHaveTextContent('Library scan: Main Music • 1,284 / 3,912 files');
    fireEvent.focus(screen.getByTestId('sidebar-status-scan'));
    expect(screen.getByTestId('sidebar-status-scan')).toHaveStyle({ boxShadow: 'var(--focus-ring)' });

    fireEvent.blur(screen.getByTestId('sidebar-status-scan'));
    fireEvent.mouseLeave(screen.getByTestId('sidebar-status-scan'));
    fireEvent.mouseEnter(screen.getByTestId('sidebar-status-transcoding'));
    expect(screen.getByRole('tooltip')).toHaveStyle({ left: '0px', transform: 'none' });
  });

  it('visually distinguishes enabled transcoding from direct streaming', () => {
    const { unmount } = renderPanel();
    expect(screen.getByTestId('sidebar-status-transcoding')).toHaveAttribute('data-active', 'true');
    expect(screen.getByTestId('transcode-conversion-mark')).toBeInTheDocument();

    unmount();
    renderPanel(false, vi.fn(), true);
    expect(screen.getByTestId('sidebar-status-transcoding')).toHaveAttribute('data-active', 'false');
    expect(screen.queryByTestId('transcode-conversion-mark')).not.toBeInTheDocument();
  });

  it('spins only for running scans, not pending scheduled jobs', () => {
    const running = renderPanel();
    const runningIndicator = screen.getByTestId('sidebar-status-scan');
    expect(runningIndicator).toHaveAttribute('data-active', 'true');
    expect(runningIndicator.querySelector('svg')).toHaveClass('sidebar-scan-spin');

    running.unmount();
    renderPanel(false, vi.fn(), false, [{ ...activeScanJobs[0], status: 'pending' }]);
    const pendingIndicator = screen.getByTestId('sidebar-status-scan');
    expect(pendingIndicator).toHaveAttribute('data-active', 'false');
    expect(pendingIndicator.querySelector('svg')).not.toHaveClass('sidebar-scan-spin');
    expect(pendingIndicator).toHaveAttribute('aria-label', 'Library scan: Idle');
  });

  it('keeps logout usable and hides identity text in the collapsed layout', () => {
    const onLogout = vi.fn();
    renderPanel(true, onLogout);

    expect(screen.queryByText('admin')).not.toBeInTheDocument();
    const logout = screen.getByRole('button', { name: 'Log out admin' });
    expect(logout).toHaveStyle({ width: '38px', height: '38px' });
    fireEvent.click(logout);
    expect(onLogout).toHaveBeenCalledTimes(1);
  });
});

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import type { BoogieMixDeepAnalysisStatus, BoogieMixSetupSnapshot } from '../types';
import BoogieMixSetupCard, { setupReason, setupStatusText, setupStepLabel, setupView } from './BoogieMixSetupCard';

const GB = 1024 * 1024 * 1024;

function snap(over: Partial<BoogieMixSetupSnapshot> = {}): BoogieMixSetupSnapshot {
  return {
    supported: true,
    state: 'idle',
    step: null,
    startedAtMs: null,
    finishedAtMs: null,
    error: null,
    exitCode: null,
    lines: [],
    installed: false,
    freeBytes: 40 * GB,
    requiredBytes: 6 * GB,
    ...over,
  };
}

function runtime(enabled: boolean, gpu = false): BoogieMixDeepAnalysisStatus['runtime'] {
  const c = { available: enabled, version: null, detail: null };
  return {
    pythonAvailable: enabled, ffmpegAvailable: true, demucsCallable: enabled, torchAvailable: enabled,
    gpuAvailable: gpu, enabled, details: [], missingCapabilities: [], summary: '',
    python: c, ffmpeg: c, demucs: c, torch: c, gpu: c,
  };
}

describe('setup helpers', () => {
  it('labels steps and falls back to the key', () => {
    expect(setupStepLabel('torch')).toBe('Installing PyTorch (the largest download)');
    expect(setupStepLabel('mystery')).toBe('mystery');
  });

  it('words every reason key', () => {
    const s = { freeBytes: 2 * GB, exitCode: 3 };
    expect(setupReason('low_disk', s)).toBe('Not enough free space: setup needs 6 GB and 2.0 GB is free.');
    expect(setupReason('exit_code', s)).toContain('(code 3)');
    expect(setupReason('exit_code', null)).toContain('(code ?)');
    for (const key of ['jobs_running', 'script_missing', 'spawn_failed', 'wait_failed', 'verify_failed', 'remove_failed', 'already_running', 'not_installed']) {
      expect(setupReason(key, s)).not.toBe(key);
    }
    expect(setupReason('Server error 500', s)).toBe('Server error 500');
  });

  it('picks the view from the run state and the runtime', () => {
    expect(setupView(snap({ state: 'running' }), runtime(true))).toBe('running');
    expect(setupView(snap({ state: 'failed' }), runtime(true))).toBe('failed');
    expect(setupView(snap({ installed: true }), runtime(true))).toBe('ready');
    expect(setupView(snap({ installed: false }), runtime(true))).toBe('ready_legacy');
    expect(setupView(snap({ supported: false }), runtime(true))).toBe('ready');
    expect(setupView(snap({ state: 'cancelled' }), runtime(false))).toBe('cancelled');
    expect(setupView(snap(), null)).toBe('not_installed');
  });

  it('writes the status row', () => {
    const s = snap({ state: 'running', step: { index: 4, total: 8, key: 'torch' } });
    expect(setupStatusText('running', s, null)).toBe('Installing · step 4 of 8: Installing PyTorch (the largest download)');
    expect(setupStatusText('running', snap({ state: 'running' }), null)).toBe('Installing · step 1 of 8: Finding or installing Python 3.12');
    expect(setupStatusText('ready', s, runtime(true, true))).toBe('Ready · GPU (CUDA)');
    expect(setupStatusText('ready', s, runtime(true))).toBe('Ready · CPU only');
    expect(setupStatusText('ready_legacy', s, null)).toBe('Ready · set up by an earlier installer');
    expect(setupStatusText('failed', s, null)).toBe('Setup failed');
    expect(setupStatusText('cancelled', s, null)).toBe('Setup cancelled');
    expect(setupStatusText('not_installed', s, null)).toBe('Not installed');
  });
});

describe('BoogieMixSetupCard', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows Install with the free space when not installed, and starts setup', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap());
    const install = vi.spyOn(api.boogiemix, 'installDeepAnalysis').mockResolvedValue(snap({ state: 'running' }));
    render(<BoogieMixSetupCard runtime={runtime(false)} onChanged={vi.fn()} />);

    expect(await screen.findByText('Not installed')).toBeInTheDocument();
    expect(screen.getByText('Needs 6 GB free · 40 GB free now')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Install' }));
    await waitFor(() => expect(install).toHaveBeenCalled());
    expect(await screen.findByText('Installing · step 1 of 8: Finding or installing Python 3.12')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
  });

  it('shows the reason when the server refuses to start', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap());
    vi.spyOn(api.boogiemix, 'installDeepAnalysis').mockRejectedValue(new Error('jobs_running'));
    render(<BoogieMixSetupCard runtime={runtime(false)} onChanged={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Install' }));
    expect(await screen.findByText(/Pause it and wait for the current track/)).toBeInTheDocument();
  });

  it('polls while running, then calls onChanged when setup ends', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const status = vi.spyOn(api.boogiemix, 'deepAnalysisSetup')
      .mockResolvedValueOnce(snap({ state: 'running', step: { index: 2, total: 8, key: 'venv' } }))
      .mockResolvedValue(snap({ state: 'succeeded', installed: true }));
    const onChanged = vi.fn();
    render(<BoogieMixSetupCard runtime={runtime(true)} onChanged={onChanged} />);
    expect(await screen.findByText('Installing · step 2 of 8: Creating the Python environment')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Setup progress' })).toBeInTheDocument();

    await act(async () => { await vi.advanceTimersByTimeAsync(2100); });
    expect(status).toHaveBeenCalledTimes(2);
    expect(await screen.findByText('Ready · CPU only')).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('asks before cancelling and sends the cancel', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap({ state: 'running' }));
    const cancel = vi.spyOn(api.boogiemix, 'cancelDeepAnalysisSetup').mockResolvedValue(snap({ state: 'running' }));
    render(<BoogieMixSetupCard runtime={null} onChanged={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(screen.getByText('What was installed so far is deleted.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Keep installing' }));
    expect(cancel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel setup' }));
    await waitFor(() => expect(cancel).toHaveBeenCalled());
  });

  it('removes a ready environment after confirming and reloads the runtime', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap({ installed: true, state: 'succeeded' }));
    const remove = vi.spyOn(api.boogiemix, 'removeDeepAnalysis').mockResolvedValue(snap());
    const onChanged = vi.fn();
    render(<BoogieMixSetupCard runtime={runtime(true, true)} onChanged={onChanged} />);
    expect(await screen.findByText('Ready · GPU (CUDA)')).toBeInTheDocument();
    expect(screen.queryByText(/free now/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(screen.getByText(/Analysis already stored for your tracks is kept/)).toBeInTheDocument();
    const removeButtons = screen.getAllByRole('button', { name: 'Remove' });
    fireEvent.click(removeButtons[removeButtons.length - 1]);
    await waitFor(() => expect(remove).toHaveBeenCalled());
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('shows the failure reason, Try again, and the output toggle', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(
      snap({ state: 'failed', error: 'exit_code', exitCode: 1, lines: ['Collecting torch', 'ERROR: no space'] }),
    );
    render(<BoogieMixSetupCard runtime={runtime(false)} onChanged={vi.fn()} />);
    expect(await screen.findByText('Setup failed')).toBeInTheDocument();
    expect(screen.getByText(/Setup stopped with an error \(code 1\)/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Show setup output' }));
    expect(screen.getByTestId('boogiemix-setup-output')).toHaveTextContent('ERROR: no space');
    fireEvent.click(screen.getByRole('button', { name: 'Hide setup output' }));
    expect(screen.queryByTestId('boogiemix-setup-output')).not.toBeInTheDocument();
  });

  it('says there is no output yet for a run without lines', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap({ state: 'cancelled' }));
    render(<BoogieMixSetupCard runtime={runtime(false)} onChanged={vi.fn()} />);
    expect(await screen.findByText('Setup cancelled')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Show setup output' }));
    expect(screen.getByTestId('boogiemix-setup-output')).toHaveTextContent('No output yet.');
  });

  it('marks an older installer environment as ready without Remove', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap({ installed: false }));
    render(<BoogieMixSetupCard runtime={runtime(true)} onChanged={vi.fn()} />);
    expect(await screen.findByText('Ready · set up by an earlier installer')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Install' })).not.toBeInTheDocument();
  });

  it('shows the install.sh hint and no buttons on Linux', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockResolvedValue(snap({ supported: false, freeBytes: null }));
    render(<BoogieMixSetupCard runtime={runtime(false)} onChanged={vi.fn()} />);
    expect(await screen.findByText(/On Linux, deep analysis is set up by the installer/)).toBeInTheDocument();
    expect(screen.getByText('sudo ./install.sh --with-boogiemix')).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('keeps showing the intro when the status call fails', async () => {
    vi.spyOn(api.boogiemix, 'deepAnalysisSetup').mockRejectedValue(new Error('Server error 500'));
    render(<BoogieMixSetupCard runtime={null} onChanged={vi.fn()} />);
    expect(screen.getByText('Deep analysis setup')).toBeInTheDocument();
    await waitFor(() => expect(api.boogiemix.deepAnalysisSetup).toHaveBeenCalled());
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
});

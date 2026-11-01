/**
 * Tests the admin Logs view: sources and files, level/search filters, older pages, follow, copy and states.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    admin: {
      logs: {
        list: vi.fn(),
        lines: vi.fn(),
        followUrl: vi.fn((key: string, from: number) => `/api/admin/logs/${key}/follow?from=${from}`),
        downloadUrl: vi.fn((key: string) => `/api/admin/logs/${key}/download`),
      },
    },
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

import LogsView from './LogsView';

const LIST = {
  folder: 'C:\\ProgramData\\BoogieBox\\logs',
  sources: [
    { id: 'server', enabled: true, files: [
      { key: 'server', name: 'boogiebox-server.log', size: 1153434, modifiedMs: 1, current: true },
      { key: 'server.1', name: 'boogiebox-server.log.1', size: 20971520, modifiedMs: 1, current: false },
    ] },
    { id: 'scan', enabled: false, files: [] },
    { id: 'deep', enabled: false, files: [
      { key: 'deep.2026-10-31', name: 'boogiebox-deepmix-debug.2026-10-31.log', size: 2048, modifiedMs: 1, current: true },
    ] },
  ],
};

const line = (offset: number, text: string, level?: string, extra: Record<string, string> = {}) => ({
  offset, text, ...(level ? { level, ts: '2026-10-31T13:12:03.047064Z', target: 'boogiebox_server::scanner' } : {}), ...extra,
});

const PAGE = {
  lines: [
    line(0, 'BoogieBox server log initialized'),
    line(40, 'listening on http://localhost:8899', 'info', { target: 'boogiebox_server' }),
    line(120, 'claim failed', 'warn'),
    line(200, 'worker exited with status 1', 'error'),
    line(260, '    stderr: boom'),
  ],
  start: 0, end: 300, size: 300, more: true, truncated: false,
};

class FakeEventSource {
  static last: FakeEventSource | null = null;
  static CLOSED = 2;
  listeners: Record<string, (e: { data: string }) => void> = {};
  onerror: (() => void) | null = null;
  closed = false;
  constructor(readonly url: string) { FakeEventSource.last = this; }
  addEventListener(type: string, fn: (e: { data: string }) => void) { this.listeners[type] = fn; }
  close() { this.closed = true; }
  emit(type: string, data: unknown) { act(() => this.listeners[type]?.({ data: JSON.stringify(data) })); }
}

describe('LogsView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    FakeEventSource.last = null;
    vi.stubGlobal('EventSource', FakeEventSource);
    apiMock.admin.logs.list.mockResolvedValue(LIST);
    apiMock.admin.logs.lines.mockResolvedValue(PAGE);
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it('shows the newest server log lines with levels, modules and the folder', async () => {
    render(<LogsView onOpenDiagnostics={vi.fn()} />);
    const lines = await screen.findByTestId('log-lines');
    await waitFor(() => expect(within(lines).getByText('claim failed')).toBeInTheDocument());
    expect(apiMock.admin.logs.lines).toHaveBeenCalledWith('server', { level: undefined, q: undefined, limit: 500 });
    expect(within(lines).getByText('WARN')).toBeInTheDocument();
    expect(within(lines).getAllByText('scanner').length).toBeGreaterThan(0);
    expect(within(lines).getByText('server')).toBeInTheDocument();
    expect(within(lines).getByText('BoogieBox server log initialized')).toBeInTheDocument();
    expect(screen.getByText('Showing 5 lines · 1.1 MB file')).toBeInTheDocument();
    expect(screen.getByText('C:\\ProgramData\\BoogieBox\\logs')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'File' })).toHaveDisplayValue('Current · 1.1 MB');
    expect(screen.getByRole('link', { name: 'Download' })).toHaveAttribute('href', '/api/admin/logs/server/download');

    fireEvent.click(screen.getByRole('button', { name: 'Copy lines' }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(expect.stringContaining('WARN boogiebox_server::scanner: claim failed'));
    fireEvent.click(screen.getByRole('button', { name: 'Copy path' }));
    expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith('C:\\ProgramData\\BoogieBox\\logs');
    const wrap = screen.getByRole('button', { name: 'Wrap lines' });
    fireEvent.click(wrap);
    expect(wrap).toHaveAttribute('aria-pressed', 'true');
  });

  it('filters by level and search, loads older lines and switches files', async () => {
    render(<LogsView onOpenDiagnostics={vi.fn()} />);
    await screen.findByText('claim failed');

    fireEvent.click(screen.getByRole('button', { name: 'Warnings' }));
    await waitFor(() => expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('server', { level: 'warn', q: undefined, limit: 500 }));
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search logs' }), { target: { value: ' Claim ' } });
    await waitFor(() => expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('server', { level: 'warn', q: 'claim', limit: 500 }));

    apiMock.admin.logs.lines.mockResolvedValueOnce({ ...PAGE, lines: [line(0, 'older line', 'info')], start: 0, more: false, truncated: true });
    await screen.findByRole('button', { name: 'Load older lines' });
    fireEvent.click(screen.getByRole('button', { name: 'Load older lines' }));
    expect(await screen.findByText('older line')).toBeInTheDocument();
    expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('server', { level: 'warn', q: 'claim', before: 0, limit: 500 });
    expect(screen.getByText('Older lines are in the downloaded file.')).toBeInTheDocument();

    apiMock.admin.logs.lines.mockResolvedValue({ ...PAGE, lines: [], more: false });
    fireEvent.change(screen.getByRole('combobox', { name: 'File' }), { target: { value: 'server.1' } });
    expect(await screen.findByText('No lines match.')).toBeInTheDocument();
    expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('server.1', expect.anything());
    expect(screen.getByRole('switch', { name: 'Follow' })).toBeDisabled();
  });

  it('follows new lines, keeps the filter and picks up a replaced file', async () => {
    render(<LogsView onOpenDiagnostics={vi.fn()} />);
    await screen.findByText('claim failed');
    fireEvent.click(screen.getByRole('switch', { name: 'Follow' }));
    await waitFor(() => expect(FakeEventSource.last?.url).toBe('/api/admin/logs/server/follow?from=300'));
    const source = FakeEventSource.last!;

    source.emit('lines', { lines: [line(300, 'fresh event', 'info'), line(340, '    continued')], end: 380 });
    expect(screen.getByText('fresh event')).toBeInTheDocument();
    expect(screen.getByText('continued')).toBeInTheDocument();

    source.emit('rotated', { key: 'server' });
    expect(screen.getByText('The log file was replaced; following the new one.')).toBeInTheDocument();

    // A dropped connection reconnects from where it got to.
    vi.useFakeTimers();
    try {
      act(() => source.onerror?.());
      expect(source.closed).toBe(true);
      act(() => { vi.advanceTimersByTime(3000); });
      expect(FakeEventSource.last).not.toBe(source);
      expect(FakeEventSource.last?.url).toBe('/api/admin/logs/server/follow?from=0');
    } finally {
      vi.useRealTimers();
    }

    fireEvent.click(screen.getByRole('switch', { name: 'Follow' }));
    expect(FakeEventSource.last?.closed).toBe(true);
  });

  it('drops filtered-out lines while following', async () => {
    render(<LogsView onOpenDiagnostics={vi.fn()} />);
    await screen.findByText('claim failed');
    fireEvent.click(screen.getByRole('button', { name: 'Errors' }));
    await waitFor(() => expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('server', { level: 'error', q: undefined, limit: 500 }));
    await screen.findByText('claim failed');
    fireEvent.click(screen.getByRole('switch', { name: 'Follow' }));
    await waitFor(() => expect(FakeEventSource.last).not.toBeNull());
    FakeEventSource.last!.emit('lines', { lines: [line(300, 'quiet info', 'info'), line(320, 'loud error', 'error'), line(360, '    trace line')], end: 400 });
    expect(screen.queryByText('quiet info')).toBeNull();
    expect(screen.getByText('loud error')).toBeInTheDocument();
    expect(screen.getByText('trace line')).toBeInTheDocument();
  });

  it('explains a debug log that is off and has no files, and links to Diagnostics', async () => {
    const onOpenDiagnostics = vi.fn();
    render(<LogsView onOpenDiagnostics={onOpenDiagnostics} />);
    await screen.findByText('claim failed');
    const scan = screen.getByRole('tab', { name: /Scan debug/ });
    expect(within(scan).getByText('Off')).toBeInTheDocument();
    fireEvent.click(scan);
    const status = await screen.findByRole('status');
    expect(status).toHaveTextContent('Scan debug logging is off. Turn it on in Advanced → Diagnostics to record scans.');
    fireEvent.click(within(status).getByRole('link', { name: 'Advanced → Diagnostics' }));
    expect(onOpenDiagnostics).toHaveBeenCalled();

    // Off, but older files exist: they can still be read.
    fireEvent.click(screen.getByRole('tab', { name: /Deep analysis debug/ }));
    await waitFor(() => expect(apiMock.admin.logs.lines).toHaveBeenLastCalledWith('deep.2026-10-31', expect.anything()));
    expect(screen.getByRole('combobox', { name: 'File' })).toHaveDisplayValue('2026-10-31 · 2.0 KB');
  });

  it('says when a log is empty or cannot be read', async () => {
    apiMock.admin.logs.lines.mockResolvedValueOnce({ ...PAGE, lines: [], more: false });
    render(<LogsView onOpenDiagnostics={vi.fn()} />);
    expect(await screen.findByText('This log is empty.')).toBeInTheDocument();
    apiMock.admin.logs.lines.mockRejectedValueOnce(new Error('boom'));
    expect(apiMock.admin.logs.lines).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Info' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Couldn’t read this log.');
  });
});

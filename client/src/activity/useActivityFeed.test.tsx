import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ACTIVITY_POLL_MS, ACTIVITY_SSE_RETRY_MS, useActivityFeed } from './useActivityFeed';
import type { ActivitySnapshot } from './activityTypes';

const snapshotMock = vi.fn();
vi.mock('../api', () => ({
  api: {
    activity: {
      snapshot: () => snapshotMock(),
      eventsUrl: () => '/api/activity/events',
    },
  },
}));

function snap(playing: number): ActivitySnapshot {
  return {
    generatedAtMs: 1,
    totals: { playing, downloading: 0, mixJobs: 0, transcodes: 0, subsonicTranscodeLimit: 3, activeUsers: 0, serverJobs: 0 },
    playing: [],
    downloads: [],
    mixJobs: [],
    serverJobs: { scans: [], postScanQueued: [], deepAnalysisQueued: 0, deepAnalysisRunning: 0 },
    recent: [],
  };
}

class FakeEventSource {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 2;
  static instances: FakeEventSource[] = [];
  readyState = FakeEventSource.CONNECTING;
  onerror: (() => void) | null = null;
  listeners: Record<string, Array<(event: MessageEvent<string>) => void>> = {};
  closed = false;
  constructor(public url: string, public init?: EventSourceInit) {
    FakeEventSource.instances.push(this);
  }
  addEventListener(name: string, fn: (event: MessageEvent<string>) => void) {
    (this.listeners[name] ??= []).push(fn);
  }
  close() { this.closed = true; this.readyState = FakeEventSource.CLOSED; }
  emit(name: string, data: unknown) {
    for (const fn of this.listeners[name] ?? []) fn({ data: JSON.stringify(data) } as MessageEvent<string>);
  }
  fail() { this.readyState = FakeEventSource.CLOSED; this.onerror?.(); }
}

describe('useActivityFeed', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeEventSource.instances = [];
    vi.stubGlobal('EventSource', FakeEventSource);
    snapshotMock.mockReset().mockResolvedValue(snap(1));
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('does nothing for non-admins', () => {
    const { result } = renderHook(() => useActivityFeed(false));
    expect(result.current.snapshot).toBeNull();
    expect(FakeEventSource.instances).toHaveLength(0);
    expect(snapshotMock).not.toHaveBeenCalled();
  });

  it('takes SSE snapshots and goes live', async () => {
    const { result } = renderHook(() => useActivityFeed(true));
    await act(async () => { await Promise.resolve(); });
    expect(result.current.snapshot?.totals.playing).toBe(1);
    const source = FakeEventSource.instances[0];
    expect(source.url).toBe('/api/activity/events');
    expect(source.init).toEqual({ withCredentials: true });
    act(() => source.emit('snapshot', snap(3)));
    expect(result.current.snapshot?.totals.playing).toBe(3);
    expect(result.current.connection).toBe('live');
  });

  it('falls back to polling when the stream is refused, then retries the stream', async () => {
    const { result, unmount } = renderHook(() => useActivityFeed(true));
    await act(async () => { await Promise.resolve(); });
    snapshotMock.mockClear();
    act(() => FakeEventSource.instances[0].fail());
    expect(result.current.connection).toBe('polling');
    expect(snapshotMock).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(ACTIVITY_POLL_MS * 2); });
    expect(snapshotMock).toHaveBeenCalledTimes(3);
    await act(async () => { vi.advanceTimersByTime(ACTIVITY_SSE_RETRY_MS); });
    expect(FakeEventSource.instances).toHaveLength(2);
    unmount();
    expect(FakeEventSource.instances[1].closed).toBe(true);
  });

  it('disconnects while hidden and catches up on return', async () => {
    renderHook(() => useActivityFeed(true));
    await act(async () => { await Promise.resolve(); });
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
    act(() => { document.dispatchEvent(new Event('visibilitychange')); });
    expect(FakeEventSource.instances[0].closed).toBe(true);
    hidden.mockReturnValue(false);
    snapshotMock.mockClear();
    act(() => { document.dispatchEvent(new Event('visibilitychange')); });
    expect(snapshotMock).toHaveBeenCalledTimes(1);
    expect(FakeEventSource.instances).toHaveLength(2);
    hidden.mockRestore();
  });
});

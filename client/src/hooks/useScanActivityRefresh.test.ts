/**
 * Tests Use Scan Activity Refresh behavior for BoogieBox regressions.
 */

import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useScanActivityRefresh } from './useScanActivityRefresh';

const { apiMock } = vi.hoisted(() => ({
  apiMock: { scanJobs: { active: vi.fn() } },
}));

vi.mock('../api', () => ({ api: apiMock }));

describe('useScanActivityRefresh', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    apiMock.scanJobs.active.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('refreshes on every poll while a scan is active', async () => {
    apiMock.scanJobs.active.mockResolvedValue([{ id: '1', status: 'running' }]);
    const onRefresh = vi.fn();
    renderHook(() => useScanActivityRefresh(onRefresh));

    await vi.advanceTimersByTimeAsync(15000);

    expect(onRefresh).toHaveBeenCalledTimes(3);
  });

  it('refreshes once after the last scan finishes, then goes quiet', async () => {
    apiMock.scanJobs.active.mockResolvedValueOnce([{ id: '1', status: 'running' }]);
    const onRefresh = vi.fn();
    renderHook(() => useScanActivityRefresh(onRefresh));

    await vi.advanceTimersByTimeAsync(10000);
    expect(onRefresh).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(20000);
    expect(onRefresh).toHaveBeenCalledTimes(2);
  });

  it('never refreshes when no scan runs and survives polling failures', async () => {
    const onRefresh = vi.fn();
    const { unmount } = renderHook(() => useScanActivityRefresh(onRefresh));

    await vi.advanceTimersByTimeAsync(10000);
    apiMock.scanJobs.active.mockRejectedValueOnce(new Error('offline'));
    await vi.advanceTimersByTimeAsync(10000);

    expect(onRefresh).not.toHaveBeenCalled();
    expect(apiMock.scanJobs.active).toHaveBeenCalled();

    unmount();
    const callsAtUnmount = apiMock.scanJobs.active.mock.calls.length;
    await vi.advanceTimersByTimeAsync(10000);
    expect(apiMock.scanJobs.active.mock.calls.length).toBe(callsAtUnmount);
  });

  it('always calls the latest callback without restarting the poll', async () => {
    apiMock.scanJobs.active.mockResolvedValue([{ id: '1', status: 'running' }]);
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ cb }) => useScanActivityRefresh(cb), {
      initialProps: { cb: first },
    });

    await vi.advanceTimersByTimeAsync(5000);
    rerender({ cb: second });
    await vi.advanceTimersByTimeAsync(5000);

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });
});

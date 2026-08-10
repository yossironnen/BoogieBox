/** Tests resilient BoogieMix deep-analysis status polling. */

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useDeepAnalysisStatus } from './useDeepAnalysisStatus';

const { apiMock } = vi.hoisted(() => ({
  apiMock: { boogiemix: { deepAnalysisStatus: vi.fn() } },
}));

vi.mock('../api', () => ({ api: apiMock }));

const status = {
  enabled: true,
  runtime: null,
  queue: { pending: 2, running: 0, failed: 0, skipped: 0, done: 4 },
  cache: { analyzedTracks: 4, estimatedBytes: 100, oldestCreatedAt: null, newestCreatedAt: null },
};

describe('useDeepAnalysisStatus', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    apiMock.boogiemix.deepAnalysisStatus.mockResolvedValue(status);
  });

  afterEach(() => vi.useRealTimers());

  it('loads immediately and polls at the requested interval', async () => {
    const { result } = renderHook(() => useDeepAnalysisStatus(true, 5000));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(result.current.status).toEqual(status);
    expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalledTimes(1);

    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalledTimes(2);
  });

  it('keeps the last successful status after a transient failure', async () => {
    const { result } = renderHook(() => useDeepAnalysisStatus(true, 5000));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    apiMock.boogiemix.deepAnalysisStatus.mockRejectedValueOnce(new Error('offline'));
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(result.current.status).toEqual(status);
  });

  it('does not fetch while disabled and stops polling on unmount', async () => {
    const { unmount } = renderHook(() => useDeepAnalysisStatus(false, 5000));
    await vi.advanceTimersByTimeAsync(10000);
    expect(apiMock.boogiemix.deepAnalysisStatus).not.toHaveBeenCalled();

    unmount();
    await vi.advanceTimersByTimeAsync(5000);
    expect(apiMock.boogiemix.deepAnalysisStatus).not.toHaveBeenCalled();
  });

  it('supports an immediate manual refresh', async () => {
    const { result } = renderHook(() => useDeepAnalysisStatus(true, 60000));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    await act(async () => { await result.current.refresh(); });
    expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalledTimes(2);
  });
});

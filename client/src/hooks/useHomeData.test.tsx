import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useHomeData } from './useHomeData';
import { homeSnapshot, invalidateHomeData, refreshHomeData } from '../homeCache';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe('Home snapshots', () => {
  beforeEach(() => invalidateHomeData(true));

  it('renders cached data immediately on remount, then replaces it without a loading flash', async () => {
    const load = vi.fn().mockResolvedValue(['old']);
    const first = renderHook(() => useHomeData<string[]>('latest:24', load, []));
    await waitFor(() => expect(first.result.current.data).toEqual(['old']));
    first.unmount();
    const next = deferred<string[]>();
    load.mockReturnValue(next.promise);
    const second = renderHook(() => useHomeData<string[]>('latest:24', load, []));
    expect(second.result.current.data).toEqual(['old']);
    expect(second.result.current.loading).toBe(false);
    await act(async () => next.resolve(['new']));
    expect(second.result.current.data).toEqual(['new']);
  });

  it('coalesces simultaneous consumers and preserves cached data on refresh failure', async () => {
    const pending = deferred<string[]>();
    const load = vi.fn(() => pending.promise);
    const a = renderHook(() => useHomeData('covers:rock', load, []));
    const b = renderHook(() => useHomeData('covers:rock', load, []));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    await act(async () => pending.resolve(['a']));
    expect(a.result.current.data).toEqual(['a']);
    expect(b.result.current.data).toEqual(['a']);
    load.mockRejectedValue(new Error('offline'));
    await act(async () => a.result.current.refresh());
    expect(a.result.current.data).toEqual(['a']);
    expect(a.result.current.loading).toBe(false);
  });

  it('discards responses from a previous user/database, including failed requests', async () => {
    const old = deferred<string[]>();
    const first = refreshHomeData('rated', () => old.promise);
    invalidateHomeData(true);
    await refreshHomeData('rated', async () => ['new-user']);
    old.resolve(['old-user']);
    await first;
    expect(homeSnapshot('rated').data).toEqual(['new-user']);
    const failing = deferred<string[]>();
    const failed = refreshHomeData('rated', () => failing.promise);
    invalidateHomeData(true);
    failing.reject(new Error('old failure'));
    await failed;
    expect(homeSnapshot('rated').data).toBeUndefined();
  });

  it('refreshes mounted data on mutations and focus; does not fetch disabled tabs', async () => {
    const load = vi.fn().mockResolvedValue(['first']);
    const hook = renderHook(({ enabled, revision }) => useHomeData<string[]>('recent', load, [], revision, enabled),
      { initialProps: { enabled: false, revision: 0 } });
    expect(load).not.toHaveBeenCalled();
    hook.rerender({ enabled: true, revision: 0 });
    await waitFor(() => expect(hook.result.current.data).toEqual(['first']));
    load.mockResolvedValue(['played']);
    act(() => invalidateHomeData());
    await waitFor(() => expect(hook.result.current.data).toEqual(['played']));
    load.mockResolvedValue(['focused']);
    act(() => window.dispatchEvent(new Event('focus')));
    await waitFor(() => expect(hook.result.current.data).toEqual(['focused']));
    load.mockResolvedValue(['explicit']);
    hook.rerender({ enabled: true, revision: 1 });
    await waitFor(() => expect(hook.result.current.data).toEqual(['explicit']));
    hook.unmount();
    const calls = load.mock.calls.length;
    act(() => invalidateHomeData());
    expect(load).toHaveBeenCalledTimes(calls);
  });

  it('defers visibility refresh while hidden and reloads when visible again', async () => {
    const load = vi.fn().mockResolvedValue(['ready']);
    const hook = renderHook(() => useHomeData('visible', load, []));
    await waitFor(() => expect(hook.result.current.loading).toBe(false));
    const visibility = vi.spyOn(document, 'visibilityState', 'get');
    visibility.mockReturnValue('hidden');
    act(() => document.dispatchEvent(new Event('visibilitychange')));
    expect(load).toHaveBeenCalledTimes(1);
    visibility.mockReturnValue('visible');
    act(() => document.dispatchEvent(new Event('visibilitychange')));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    visibility.mockRestore();
  });

  it('settles initial failures and bounds dynamic snapshots', async () => {
    const failed = renderHook(() => useHomeData('failure', async () => { throw new Error('offline'); }, []));
    await waitFor(() => expect(failed.result.current.loading).toBe(false));
    expect(failed.result.current.data).toEqual([]);
    failed.unmount();
    for (let n = 0; n < 70; n += 1) await refreshHomeData(`key:${n}`, async () => n);
    expect(homeSnapshot('key:0').data).toBeUndefined();
    expect(homeSnapshot('key:69').data).toBe(69);
  });
});

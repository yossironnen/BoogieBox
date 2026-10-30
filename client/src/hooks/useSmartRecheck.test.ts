/**
 * Tests the open smart playlist re-check: once a minute while visible, and on return after a minute away.
 */

// @vitest-environment jsdom

import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SMART_RECHECK_MS, useSmartRecheck } from './useSmartRecheck';

let visibility: DocumentVisibilityState = 'visible';

function setVisibility(next: DocumentVisibilityState) {
  visibility = next;
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('useSmartRecheck', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    visibility = 'visible';
    vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => visibility);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('re-checks once a minute while visible', () => {
    const recheck = vi.fn();
    renderHook(() => useSmartRecheck(true, recheck));
    vi.advanceTimersByTime(SMART_RECHECK_MS - 1_500);
    expect(recheck).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1_500);
    expect(recheck).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(SMART_RECHECK_MS);
    expect(recheck).toHaveBeenCalledTimes(2);
  });

  it('waits while hidden and checks on return only after a minute', () => {
    const recheck = vi.fn();
    renderHook(() => useSmartRecheck(true, recheck));
    setVisibility('hidden');
    vi.advanceTimersByTime(SMART_RECHECK_MS * 3);
    expect(recheck).not.toHaveBeenCalled();
    setVisibility('visible');
    expect(recheck).toHaveBeenCalledTimes(1);
    setVisibility('hidden');
    setVisibility('visible');
    expect(recheck).toHaveBeenCalledTimes(1);
  });

  it('does nothing when off', () => {
    const recheck = vi.fn();
    const { rerender, unmount } = renderHook(({ on }) => useSmartRecheck(on, recheck), { initialProps: { on: false } });
    vi.advanceTimersByTime(SMART_RECHECK_MS * 2);
    expect(recheck).not.toHaveBeenCalled();
    rerender({ on: true });
    unmount();
    vi.advanceTimersByTime(SMART_RECHECK_MS * 2);
    expect(recheck).not.toHaveBeenCalled();
  });
});

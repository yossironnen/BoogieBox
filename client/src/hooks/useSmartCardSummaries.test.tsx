/**
 * Tests smart card summaries: only cards on screen ask, one request at a time, each at most once a minute.
 */

// @vitest-environment jsdom

import React from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { summary } = vi.hoisted(() => ({ summary: vi.fn() }));
vi.mock('../api', () => ({ api: { playlists: { summary } } }));

import { useSmartCardSummaries, withSummary } from './useSmartCardSummaries';
import { SMART_RECHECK_MS } from './useSmartRecheck';
import type { Playlist } from '../types';

const row = (id: string, track_count = 1): Playlist => ({
  id, name: id, description: '', created_at: null, updated_at: null, track_count, total_duration: 60, kind: 'smart',
});

class FakeObserver {
  static last: FakeObserver;
  targets = new Set<Element>();
  constructor(readonly callback: IntersectionObserverCallback) { FakeObserver.last = this; }
  observe(el: Element) { this.targets.add(el); }
  unobserve(el: Element) { this.targets.delete(el); }
  disconnect() { this.targets.clear(); }
  show(ids: string[], visible = true) {
    const entries = [...this.targets]
      .filter(el => ids.includes((el as HTMLElement).dataset.smartCard!))
      .map(target => ({ target, isIntersecting: visible }) as unknown as IntersectionObserverEntry);
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}

function Cards({ ids, onSummary }: { ids: string[]; onSummary: (p: Playlist) => void }) {
  const observe = useSmartCardSummaries(onSummary);
  return <>{ids.map(id => <div key={id} data-smart-card={id} ref={observe} />)}</>;
}

describe('useSmartCardSummaries', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    summary.mockReset();
    summary.mockImplementation((id: string) => Promise.resolve(row(id, 5)));
    vi.stubGlobal('IntersectionObserver', FakeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('asks for cards on screen one at a time, then again after a minute', async () => {
    const onSummary = vi.fn();
    render(<Cards ids={['a', 'b', 'c']} onSummary={onSummary} />);
    expect(summary).not.toHaveBeenCalled();

    await act(async () => { FakeObserver.last.show(['a', 'b']); });
    expect(summary.mock.calls.map(c => c[0])).toEqual(['a', 'b']);
    expect(onSummary).toHaveBeenCalledWith(row('b', 5));

    await act(async () => { FakeObserver.last.show(['a'], false); });
    await act(async () => { await vi.advanceTimersByTimeAsync(SMART_RECHECK_MS); });
    expect(summary.mock.calls.map(c => c[0])).toEqual(['a', 'b', 'b']);
  });

  it('keeps going after a failed request and stops when unmounted', async () => {
    summary.mockRejectedValueOnce(new Error('offline'));
    const onSummary = vi.fn();
    const { unmount } = render(<Cards ids={['a', 'b']} onSummary={onSummary} />);
    await act(async () => { FakeObserver.last.show(['a', 'b']); });
    expect(summary).toHaveBeenCalledTimes(2);
    expect(onSummary).toHaveBeenCalledTimes(1);
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(SMART_RECHECK_MS * 2); });
    expect(summary).toHaveBeenCalledTimes(2);
  });

  it('treats every card as on screen without IntersectionObserver', async () => {
    vi.stubGlobal('IntersectionObserver', undefined);
    render(<Cards ids={['a', 'b']} onSummary={vi.fn()} />);
    await act(async () => { await Promise.resolve(); });
    expect(summary.mock.calls.map(c => c[0])).toEqual(['a', 'b']);
  });

  it('replaces a playlist only when its card would change', () => {
    const list = [row('a'), row('b')];
    expect(withSummary(list, row('a'))).toBe(list);
    expect(withSummary(list, row('z', 9))).toBe(list);
    expect(withSummary(list, row('b', 9))).toEqual([row('a'), row('b', 9)]);
  });
});

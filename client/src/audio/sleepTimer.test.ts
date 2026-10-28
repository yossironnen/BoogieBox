import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  SLEEP_FADE_MS,
  createSleepTimer,
  formatSleepRemaining,
  sleepFadeLevel,
  sleepStopsAfterCurrent,
  sleepTimerBadge,
  sleepTimerDue,
  sleepTimerLabel,
  useLocalSleepTimer,
} from './sleepTimer';

const NOW = 1_700_000_000_000;

describe('sleep timer state', () => {
  it('creates minutes timers with a wall-clock deadline and rejects bad minutes', () => {
    expect(createSleepTimer('minutes', 15, NOW)).toEqual({ mode: 'minutes', deadlineMs: NOW + 15 * 60_000 });
    expect(createSleepTimer('minutes', 600, NOW)?.deadlineMs).toBe(NOW + 600 * 60_000);
    for (const bad of [undefined, 0, 601, 2.5, Number.NaN]) {
      expect(createSleepTimer('minutes', bad, NOW)).toBeNull();
    }
    expect(createSleepTimer('endOfTrack', undefined, NOW)).toEqual({ mode: 'endOfTrack', deadlineMs: null });
  });

  it('fades over the last 20 s and is due at the deadline', () => {
    const timer = createSleepTimer('minutes', 1, NOW)!;
    const deadline = NOW + 60_000;
    expect(sleepFadeLevel(timer, NOW)).toBe(1);
    expect(sleepFadeLevel(timer, deadline - SLEEP_FADE_MS)).toBe(1);
    expect(sleepFadeLevel(timer, deadline - SLEEP_FADE_MS / 2)).toBeCloseTo(0.5);
    expect(sleepFadeLevel(timer, deadline)).toBe(0);
    expect(sleepTimerDue(timer, deadline - 1)).toBe(false);
    expect(sleepTimerDue(timer, deadline)).toBe(true);
    expect(sleepTimerDue(timer, deadline + 5_000)).toBe(true);
    expect(sleepFadeLevel({ mode: 'endOfTrack', deadlineMs: null }, deadline)).toBe(1);
    expect(sleepTimerDue(null, deadline)).toBe(false);
  });

  it('stops after the current item for end of track, and when the album changes for end of album', () => {
    const a1 = { album: 'A', album_id: 'a' };
    const a2 = { album: 'A', album_id: 'a' };
    const b1 = { album: 'B', album_id: 'b' };
    const endOfTrack = { mode: 'endOfTrack' as const, deadlineMs: null };
    const endOfAlbum = { mode: 'endOfAlbum' as const, deadlineMs: null };
    expect(sleepStopsAfterCurrent(endOfTrack, a1, a2)).toBe(true);
    expect(sleepStopsAfterCurrent(endOfAlbum, a1, a2)).toBe(false);
    expect(sleepStopsAfterCurrent(endOfAlbum, a1, b1)).toBe(true);
    expect(sleepStopsAfterCurrent(endOfAlbum, a1, null)).toBe(true);
    // Falls back to the album title without an id; an unknown album stops.
    expect(sleepStopsAfterCurrent(endOfAlbum, { album: 'A' }, { album: 'A' })).toBe(false);
    expect(sleepStopsAfterCurrent(endOfAlbum, { album: null }, { album: null })).toBe(true);
    expect(sleepStopsAfterCurrent(createSleepTimer('minutes', 5, NOW), a1, b1)).toBe(false);
    expect(sleepStopsAfterCurrent(null, a1, b1)).toBe(false);
  });

  it('labels and badges every mode', () => {
    const timer = createSleepTimer('minutes', 15, NOW)!;
    expect(sleepTimerLabel(null, NOW)).toBe('Sleep timer');
    expect(sleepTimerLabel(timer, NOW)).toBe('Sleep timer: 15:00 left');
    expect(sleepTimerLabel(timer, NOW + 60_500)).toBe('Sleep timer: 14:00 left');
    expect(sleepTimerLabel({ mode: 'endOfTrack', deadlineMs: null }, NOW)).toBe('Sleep timer: end of track');
    expect(sleepTimerLabel({ mode: 'endOfAlbum', deadlineMs: null }, NOW)).toBe('Sleep timer: end of album');
    expect(sleepTimerBadge(null, NOW)).toBeNull();
    expect(sleepTimerBadge(timer, NOW)).toBe('15m');
    expect(sleepTimerBadge(timer, NOW + 15 * 60_000 - 5_000)).toBe('1m');
    expect(sleepTimerBadge({ mode: 'endOfAlbum', deadlineMs: null }, NOW)).toBe('End');
    expect(formatSleepRemaining(65_000)).toBe('1:05');
    expect(formatSleepRemaining(0)).toBe('0:00');
  });
});

describe('useLocalSleepTimer', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });
  afterEach(() => vi.useRealTimers());

  it('fades, fires once at the deadline and resets', () => {
    const onDue = vi.fn();
    const { result } = renderHook(() => useLocalSleepTimer(onDue));
    act(() => result.current.start('minutes', 1));
    expect(result.current.timer?.mode).toBe('minutes');
    act(() => { vi.advanceTimersByTime(50_000); });
    expect(result.current.fade).toBeCloseTo(0.5, 1);
    expect(onDue).not.toHaveBeenCalled();
    act(() => { vi.advanceTimersByTime(11_000); });
    expect(onDue).toHaveBeenCalledTimes(1);
    expect(result.current.timer).toBeNull();
    expect(result.current.fade).toBe(1);
    act(() => { vi.advanceTimersByTime(5_000); });
    expect(onDue).toHaveBeenCalledTimes(1);
  });

  it('checks on demand (timeupdate) when interval timers are throttled', () => {
    const onDue = vi.fn();
    const { result } = renderHook(() => useLocalSleepTimer(onDue));
    act(() => result.current.start('minutes', 1));
    vi.setSystemTime(NOW + 61_000);
    act(() => result.current.check());
    expect(onDue).toHaveBeenCalledTimes(1);
  });

  it('cancels and ignores invalid starts', () => {
    const onDue = vi.fn();
    const { result } = renderHook(() => useLocalSleepTimer(onDue));
    act(() => result.current.start('minutes', 0));
    expect(result.current.timer).toBeNull();
    act(() => result.current.start('minutes', 1));
    act(() => result.current.cancel());
    act(() => { vi.advanceTimersByTime(120_000); });
    expect(onDue).not.toHaveBeenCalled();
    act(() => result.current.start('endOfTrack'));
    expect(result.current.timer).toEqual({ mode: 'endOfTrack', deadlineMs: null });
  });
});

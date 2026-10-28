/**
 * Sleep timer: pause after N minutes, or stop at the end of the track/album instead of advancing.
 * The browser timer lives in memory (a reload clears it); Play On timers are owned by the server session.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

export type SleepMode = 'minutes' | 'endOfTrack' | 'endOfAlbum';

/** Same shape as the Play On session snapshot's `sleep`. */
export interface SleepTimer {
  mode: SleepMode;
  /** Wall-clock deadline (ms since epoch) for `minutes`. */
  deadlineMs: number | null;
}

/** What the timer UI drives: this browser's timer or the Play On speaker's. */
export interface SleepTimerControls {
  timer: SleepTimer | null;
  /** The timer belongs to this browser, so a reload clears it. */
  local: boolean;
  start: (mode: SleepMode, minutes?: number) => void;
  cancel: () => void;
}

export const SLEEP_PRESET_MINUTES = [15, 30, 45, 60, 90] as const;
export const SLEEP_MIN_MINUTES = 1;
export const SLEEP_MAX_MINUTES = 600;
/** Minutes timers fade the volume out over the last stretch. */
export const SLEEP_FADE_MS = 20_000;
/** Background tabs throttle timers, so `timeupdate` checks as well. */
export const SLEEP_TICK_MS = 1_000;

export function isValidSleepMinutes(minutes: number): boolean {
  return Number.isInteger(minutes) && minutes >= SLEEP_MIN_MINUTES && minutes <= SLEEP_MAX_MINUTES;
}

export function createSleepTimer(mode: SleepMode, minutes: number | undefined, nowMs: number): SleepTimer | null {
  if (mode !== 'minutes') return { mode, deadlineMs: null };
  if (minutes == null || !isValidSleepMinutes(minutes)) return null;
  return { mode, deadlineMs: nowMs + minutes * 60_000 };
}

export function sleepRemainingMs(timer: SleepTimer | null, nowMs: number): number | null {
  if (timer?.mode !== 'minutes' || timer.deadlineMs == null) return null;
  return Math.max(0, timer.deadlineMs - nowMs);
}

export function sleepTimerDue(timer: SleepTimer | null, nowMs: number): boolean {
  return sleepRemainingMs(timer, nowMs) === 0;
}

/** Volume multiplier: 1 until the fade window, then down to 0 at the deadline. */
export function sleepFadeLevel(timer: SleepTimer | null, nowMs: number): number {
  const remaining = sleepRemainingMs(timer, nowMs);
  if (remaining == null || remaining >= SLEEP_FADE_MS) return 1;
  return remaining / SLEEP_FADE_MS;
}

interface AlbumEntry {
  album?: string | null;
  album_id?: string | number | null;
}

function albumKey(entry: AlbumEntry | null | undefined): string | null {
  if (!entry) return null;
  const key = entry.album_id ?? entry.album;
  return key == null || key === '' ? null : String(key);
}

/** An end-of-track/album timer stops playback when the current item finishes. */
export function sleepStopsAfterCurrent(
  timer: SleepTimer | null,
  current: AlbumEntry | null | undefined,
  next: AlbumEntry | null | undefined,
): boolean {
  if (timer?.mode === 'endOfTrack') return true;
  if (timer?.mode !== 'endOfAlbum') return false;
  const nextKey = albumKey(next);
  return nextKey == null || nextKey !== albumKey(current);
}

export function formatSleepRemaining(ms: number): string {
  const total = Math.ceil(ms / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

/** Button name/title. */
export function sleepTimerLabel(timer: SleepTimer | null, nowMs: number): string {
  if (!timer) return 'Sleep timer';
  if (timer.mode === 'endOfTrack') return 'Sleep timer: end of track';
  if (timer.mode === 'endOfAlbum') return 'Sleep timer: end of album';
  return `Sleep timer: ${formatSleepRemaining(sleepRemainingMs(timer, nowMs) ?? 0)} left`;
}

/** Badge on the moon icon. */
export function sleepTimerBadge(timer: SleepTimer | null, nowMs: number): string | null {
  if (!timer) return null;
  if (timer.mode !== 'minutes') return 'End';
  return `${Math.max(1, Math.ceil((sleepRemainingMs(timer, nowMs) ?? 0) / 60_000))}m`;
}

/**
 * This browser's timer. `onDue` runs once when a minutes timer reaches its deadline; `fade` is the volume
 * multiplier for the last {@link SLEEP_FADE_MS}. Call `check()` from `timeupdate` too.
 */
export function useLocalSleepTimer(onDue: () => void) {
  const [timer, setTimer] = useState<SleepTimer | null>(null);
  const [fade, setFade] = useState(1);
  const timerRef = useRef(timer);
  timerRef.current = timer;
  const onDueRef = useRef(onDue);
  onDueRef.current = onDue;

  const cancel = useCallback(() => {
    timerRef.current = null;
    setTimer(null);
    setFade(1);
  }, []);

  const check = useCallback((nowMs = Date.now()) => {
    const current = timerRef.current;
    if (current?.mode !== 'minutes') return;
    if (sleepTimerDue(current, nowMs)) {
      onDueRef.current();
      timerRef.current = null;
      setTimer(null);
      setFade(1);
      return;
    }
    setFade(sleepFadeLevel(current, nowMs));
  }, []);

  const start = useCallback((mode: SleepMode, minutes?: number) => {
    const next = createSleepTimer(mode, minutes, Date.now());
    if (!next) return;
    timerRef.current = next;
    setTimer(next);
    setFade(1);
  }, []);

  useEffect(() => {
    if (timer?.mode !== 'minutes') return undefined;
    const id = window.setInterval(() => check(), SLEEP_TICK_MS);
    return () => window.clearInterval(id);
  }, [check, timer]);

  return { timer, fade, start, cancel, check };
}

/** `Date.now()`, refreshed every second while `active`. */
export function useSleepClock(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return undefined;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), SLEEP_TICK_MS);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}

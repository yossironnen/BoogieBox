import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HEARTBEAT_INTERVAL_MS, heartbeatItemId, useWebPlayerHeartbeat } from './useWebPlayerHeartbeat';
import type { PlaybackSnapshot } from '../components/Player';

const heartbeat = vi.fn();
vi.mock('../api', () => ({ api: { activity: { heartbeat: (...args: unknown[]) => heartbeat(...args) } } }));

function playback(id: string | null, isPlaying = true, currentTime = 10): PlaybackSnapshot {
  return {
    currentTrack: id ? ({ id, duration: 200 } as PlaybackSnapshot['currentTrack']) : null,
    currentTime,
    duration: 200,
    isPlaying,
    volume: 1,
    muted: false,
    loading: false,
    audioError: null,
  };
}

describe('useWebPlayerHeartbeat', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    heartbeat.mockReset();
  });
  afterEach(() => vi.useRealTimers());

  it('maps synthetic ids to server item ids', () => {
    expect(heartbeatItemId('t1')).toBe('t1');
    expect(heartbeatItemId('boogiemix:abc')).toBe('mx-abc');
    expect(heartbeatItemId('radio:42')).toBe('st-42');
    expect(heartbeatItemId(null)).toBeNull();
  });

  it('reports on start, on pause, every interval, and stops when playback moves away', () => {
    const { rerender, unmount } = renderHook(
      ({ snap, active }) => useWebPlayerHeartbeat(snap, active),
      { initialProps: { snap: playback('t1'), active: true } },
    );
    expect(heartbeat).toHaveBeenCalledTimes(1);
    expect(heartbeat.mock.calls[0][0]).toMatchObject({ trackId: 't1', positionSec: 10, durationSec: 200, paused: false });

    rerender({ snap: playback('t1', false, 12), active: true });
    expect(heartbeat).toHaveBeenCalledTimes(2);
    expect(heartbeat.mock.calls[1][0]).toMatchObject({ paused: true, positionSec: 12 });

    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS);
    expect(heartbeat).toHaveBeenCalledTimes(3);

    // Moving to a Play On speaker sends one "stopped".
    rerender({ snap: playback('t1'), active: false });
    expect(heartbeat).toHaveBeenCalledTimes(4);
    expect(heartbeat.mock.calls[3][0]).toMatchObject({ trackId: null, stopped: true });
    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS * 3);
    expect(heartbeat).toHaveBeenCalledTimes(4);
    unmount();
    expect(heartbeat).toHaveBeenCalledTimes(4);
  });

  it('sends a closing beacon when the page hides', () => {
    renderHook(() => useWebPlayerHeartbeat(playback('t1'), true));
    window.dispatchEvent(new Event('pagehide'));
    const last = heartbeat.mock.calls[heartbeat.mock.calls.length - 1];
    expect(last[0]).toMatchObject({ stopped: true });
    expect(last[1]).toEqual({ closing: true });
  });

  it('stays quiet with nothing loaded', () => {
    renderHook(() => useWebPlayerHeartbeat(playback(null), true));
    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS * 2);
    expect(heartbeat).not.toHaveBeenCalled();
  });
});

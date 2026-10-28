import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, setStreamDirect } from '../api';
import type { Track } from '../types';
import Player, { type PlayerState, type PlayerTransportControls } from './Player';

vi.mock('../audio/VinylFxEngine', () => ({ playNeedleDrop: vi.fn(), preloadVinylFx: vi.fn() }));

function track(id: string, albumId = 'album-1'): Track {
  return {
    id,
    file_path: `D:\\Music\\track-${id}.mp3`,
    file_name: `track-${id}.mp3`,
    file_size: 1000,
    format: 'mp3',
    duration: 100,
    title: `Track ${id}`,
    artist: 'Artist',
    album: `Album ${albumId}`,
    album_id: albumId,
    library_name: 'Music',
    scanned_at: '2026-07-24T00:00:00Z',
  } as Track;
}

function setMediaState(media: HTMLMediaElement, values: { currentTime?: number; duration?: number }) {
  if (values.currentTime !== undefined) {
    Object.defineProperty(media, 'currentTime', { configurable: true, writable: true, value: values.currentTime });
  }
  if (values.duration !== undefined) {
    Object.defineProperty(media, 'duration', { configurable: true, value: values.duration });
  }
}

function playing(queue: Track[], currentIndex = 0): PlayerState {
  return { queue, currentIndex, isPlaying: true, playToken: 1 };
}

function openSleep() {
  fireEvent.click(screen.getByRole('button', { name: 'Sleep timer' }));
  return screen.getByRole('dialog', { name: 'Sleep timer' });
}

describe('Player sleep timer', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    setStreamDirect(false);
    localStorage.clear();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({});
    vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.crossfade, 'config').mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    vi.spyOn(api, 'trackWaveform').mockResolvedValue({ status: 'missing', waveform: null } as any);
    vi.spyOn(api, 'trackSonicFingerprint').mockResolvedValue(null);
    vi.spyOn(api.playlists, 'saveTrackProgress').mockResolvedValue({ ok: true });
    vi.spyOn(api, 'trackLyrics').mockResolvedValue({ lyrics: '', source: 'cache', synced: [] });
    vi.spyOn(api, 'trackEqProfile').mockResolvedValue({ eq_profile: 'Rock', source: 'genre' });
    vi.spyOn(api.auth, 'sessionValid').mockResolvedValue(true);
  });

  afterEach(() => {
    setStreamDirect(false);
  });

  it('stops at the end of the track instead of advancing', () => {
    const onStateChange = vi.fn();
    const { container } = render(<Player state={playing([track('1'), track('2')])} onStateChange={onStateChange} ffmpegAvailable />);
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    openSleep();
    fireEvent.click(screen.getByRole('button', { name: 'End of track' }));
    expect(screen.queryByRole('dialog', { name: 'Sleep timer' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sleep timer: end of track' })).toHaveTextContent('End');
    fireEvent.ended(audioA);
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ currentIndex: 0, isPlaying: false }));
    expect(screen.getByRole('button', { name: 'Sleep timer' })).toBeInTheDocument();
  });

  it('finishes the album, then stops before the next one', () => {
    const onStateChange = vi.fn();
    const queue = [track('1', 'a'), track('2', 'a'), track('3', 'b')];
    const { container, rerender } = render(<Player state={playing(queue)} onStateChange={onStateChange} ffmpegAvailable />);
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    openSleep();
    fireEvent.click(screen.getByRole('button', { name: 'End of album' }));
    fireEvent.ended(audioA);
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));
    rerender(<Player state={{ ...playing(queue, 1), playToken: 2 }} onStateChange={onStateChange} ffmpegAvailable />);
    fireEvent.ended(audioA);
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: false }));
  });

  it('fades out and pauses at the deadline, then restores the volume', async () => {
    const onStateChange = vi.fn();
    const { container } = render(<Player state={playing([track('1'), track('2')])} onStateChange={onStateChange} ffmpegAvailable />);
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    await waitFor(() => expect(api.userSettings.get).toHaveBeenCalled());
    const start = Date.now();
    const now = vi.spyOn(Date, 'now').mockReturnValue(start);
    openSleep();
    fireEvent.click(screen.getByRole('button', { name: '15 min' }));
    expect(screen.getByRole('button', { name: 'Sleep timer: 15:00 left' })).toHaveTextContent('15m');
    const fullVolume = audioA.volume;
    expect(fullVolume).toBeGreaterThan(0);

    now.mockReturnValue(start + 15 * 60_000 - 10_000);
    setMediaState(audioA, { currentTime: 50, duration: 100 });
    act(() => { fireEvent.timeUpdate(audioA); });
    expect(audioA.volume).toBeCloseTo(fullVolume / 2, 2);
    expect(onStateChange).not.toHaveBeenCalledWith(expect.objectContaining({ isPlaying: false }));

    now.mockReturnValue(start + 15 * 60_000);
    act(() => { fireEvent.timeUpdate(audioA); });
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ isPlaying: false }));
    expect(audioA.volume).toBeCloseTo(fullVolume, 5);
    now.mockRestore();
  });

  it('applies the saved volume to audio that mounts with the first queued track', async () => {
    vi.mocked(api.userSettings.get).mockResolvedValue({ volume: '0.3' });
    const onStateChange = vi.fn();
    const empty: PlayerState = { queue: [], currentIndex: 0, isPlaying: false, playToken: 0 };
    const { container, rerender } = render(<Player state={empty} onStateChange={onStateChange} ffmpegAvailable />);
    await waitFor(() => expect(api.userSettings.get).toHaveBeenCalled());
    await act(async () => {});
    rerender(<Player state={playing([track('1')])} onStateChange={onStateChange} ffmpegAvailable />);
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    expect(audioA.volume).toBeCloseTo(0.3, 5);
  });

  it('exposes relative seek, volume steps and track changes for shortcuts', () => {
    let controls: PlayerTransportControls | null = null;
    const onStateChange = vi.fn();
    const { container } = render(
      <Player
        state={playing([track('1'), track('2')])}
        onStateChange={onStateChange}
        ffmpegAvailable
        onTransportControlsChange={(c) => { controls = c; }}
      />,
    );
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    setMediaState(audioA, { currentTime: 30, duration: 100 });
    fireEvent.timeUpdate(audioA);
    act(() => controls!.seekBy(10));
    expect(audioA.currentTime).toBe(40);
    act(() => controls!.seekBy(-60));
    expect(audioA.currentTime).toBe(0);
    const before = audioA.volume;
    act(() => controls!.nudgeVolume(-0.05));
    expect(audioA.volume).toBeCloseTo(Math.max(0, before - 0.05), 5);
    act(() => controls!.next());
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));
    act(() => controls!.togglePlay());
    expect(onStateChange).toHaveBeenLastCalledWith(expect.objectContaining({ isPlaying: false }));
  });
});

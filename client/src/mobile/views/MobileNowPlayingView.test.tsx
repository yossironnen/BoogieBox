/**
 * Tests Mobile Now Playing View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_PARAMETRIC_BANDS } from '../../audio/eq';
import MobileNowPlayingView, { buildMobileStemBins } from './MobileNowPlayingView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    albumArtUrl: vi.fn(() => '/cover.jpg'),
    trackLyrics: vi.fn(),
    trackSonicFingerprint: vi.fn(),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

describe('MobileNowPlayingView', () => {
  beforeEach(() => {
    apiMock.trackLyrics.mockReset();
    apiMock.trackSonicFingerprint.mockReset().mockResolvedValue(null);
  });

  it('cycles album art to karaoke lyrics to plain lyrics', async () => {
    apiMock.trackLyrics.mockResolvedValue({
      lyrics: 'plain lyrics',
      source: 'cache',
      synced: [
        { time: 0, text: 'first line' },
        { time: 10, text: 'second line' },
      ],
    });

    render(
      <MobileNowPlayingView
        snapshot={{
          currentTrack: { id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song', artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1, disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', album_id: '9' },
          currentTime: 0,
          duration: 120,
          isPlaying: true,
          volume: 0.5,
          muted: false,
          loading: false,
          audioError: null,
        }}
        playerState={{
          queue: [{ id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song', artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1, disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', album_id: '9' }],
          currentIndex: 0,
          isPlaying: true,
          playToken: 1,
        }}
        onStateChange={() => {}}
      />,
    );

    const mediaStage = screen.getByRole('button', { name: /show lyrics/i });
    expect(mediaStage).toHaveStyle({ borderRadius: '18px' });
    expect(screen.getByRole('button', { name: 'Open settings' })).toHaveStyle({ minHeight: '44px' });
    expect(screen.getByRole('progressbar', { name: 'Playback progress' })).toHaveAttribute('aria-valuemax', '120');
    fireEvent.click(mediaStage);
    await waitFor(() => expect(apiMock.trackLyrics).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('first line')).toBeInTheDocument();
    expect(screen.getByText('Karaoke')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /show plain lyrics/i }));
    expect(await screen.findByText('plain lyrics')).toBeInTheDocument();
    expect(screen.getByText('Text')).toBeInTheDocument();
  });

  it('handles transport and every queue editing gesture', () => {
    const queue = ['1', '2', '3'].map((id) => ({
      id, file_path: 'x', file_name: `song-${id}.mp3`, file_size: 1, format: 'MP3',
      duration: 120, bitrate: 320, sample_rate: 44100, channels: 2,
      title: `Song ${id}`, artist: id === '3' ? '' : 'Artist', album: id === '3' ? '' : 'Album',
      library_name: 'Main', track_number: 1, disc_number: 1, year: 2025,
      genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01',
    })) as any[];
    const onStateChange = vi.fn();
    render(
      <MobileNowPlayingView
        snapshot={{
          currentTrack: queue[1], currentTime: 65, duration: 120, isPlaying: false,
          volume: 0.5, muted: false, loading: false, audioError: null,
        }}
        playerState={{ queue, currentIndex: 1, isPlaying: false, playToken: 4 }}
        onStateChange={onStateChange}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Previous track' }));
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    fireEvent.click(screen.getByRole('button', { name: 'Next track' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 0, playToken: 5 }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ isPlaying: true }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 2, playToken: 5 }));

    fireEvent.click(screen.getByRole('button', { name: 'Remove Song 1 from queue' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 0 }));
    fireEvent.click(screen.getByRole('button', { name: 'Clear queue' }));
    const clearDialog = screen.getByRole('alertdialog', { name: 'Clear the queue?' });
    expect(onStateChange).not.toHaveBeenCalledWith(expect.objectContaining({ queue: [] }));
    fireEvent.click(within(clearDialog).getByRole('button', { name: 'Clear queue' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ queue: [], isPlaying: false }));

    const song3 = screen.getByRole('button', { name: 'Song 3Unknown artist' });
    fireEvent.click(song3);
    fireEvent.doubleClick(song3.closest('div[style*="grid-template-columns"]')!);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ queue: expect.any(Array) }));

    fireEvent.pointerDown(song3, { button: 0, pointerId: 7, clientX: 100, clientY: 20 });
    fireEvent.pointerMove(song3, { pointerId: 7, clientX: 10, clientY: 20 });
    fireEvent.pointerUp(song3, { pointerId: 7, clientX: 10, clientY: 20 });
    fireEvent.click(song3);

    const handle = screen.getByRole('button', { name: 'Reorder Song 3' });
    fireEvent.pointerDown(handle, { button: 0, pointerId: 8, clientX: 10, clientY: 20 });
    fireEvent.pointerMove(handle, { pointerId: 8, clientX: 10, clientY: -60 });
    fireEvent.pointerUp(handle, { pointerId: 8, clientX: 10, clientY: -60 });
    fireEvent.pointerCancel(handle, { pointerId: 99 });
    fireEvent.click(handle);
  });

  it('renders empty, lyric error, missing-sync, and unknown metadata states', async () => {
    const emptyState = { queue: [], currentIndex: 0, isPlaying: false, playToken: 1 };
    const { rerender } = render(
      <MobileNowPlayingView snapshot={null} playerState={emptyState} onStateChange={vi.fn()} />,
    );
    expect(screen.getByText('Nothing playing yet.')).toBeInTheDocument();

    const unknown = {
      id: '9', file_path: 'x', file_name: 'unknown.mp3', file_size: 1, format: 'MP3',
      duration: 0, bitrate: null, sample_rate: null, channels: null, title: '',
      artist: '', album: '', library_name: 'Main', track_number: null, disc_number: null,
      year: null, genre: null, composer: null, comment: null, bpm: null, scanned_at: '2026-01-01',
    } as any;
    apiMock.trackLyrics.mockRejectedValue(new Error('No lyrics'));
    rerender(
      <MobileNowPlayingView
        snapshot={null}
        playerState={{ queue: [unknown], currentIndex: 0, isPlaying: false, playToken: 1 }}
        onStateChange={vi.fn()}
      />,
    );
    expect(screen.getAllByText('unknown.mp3')).toHaveLength(2);
    expect(screen.getByText('Unknown artist • Unknown album')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /show lyrics/i }));
    expect(await screen.findByText('No lyrics')).toBeInTheDocument();
  });

  it('loads, renders, and exits the sonic fingerprint panel', async () => {
    apiMock.trackLyrics.mockResolvedValue({ lyrics: '', synced: null });
    apiMock.trackSonicFingerprint.mockResolvedValue({
      trackId: '1',
      bpmDetected: 123.6,
      energyScoreRefined: 0.72,
      confidence: 0.88,
      sourceDurationSec: 120,
      demucsModel: 'htdemucs',
      usedGpu: true,
      analysisSchemaVersion: 1,
      sectionJson: [],
      vocalWindowsJson: [{ start: 0, end: 30, strength: 0.8 }],
      drumWindowsJson: [{ start: 30, end: 60, strength: 1.2 }],
      bassWindowsJson: [{ start: 60, end: 90, strength: -0.2 }],
      transitionWindowsJson: [],
      introOutroRefinedJson: { introEnd: null, outroStart: null },
      phraseBoundariesJson: [],
    });
    const track = {
      id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3',
      duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song',
      artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1,
      disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null,
      bpm: null, scanned_at: '2026-01-01',
    } as any;
    render(
      <MobileNowPlayingView
        snapshot={{ currentTrack: track, currentTime: 150, duration: 0, isPlaying: true, volume: 1, muted: false, loading: false, audioError: null }}
        playerState={{ queue: [track], currentIndex: 0, isPlaying: true, playToken: 1 }}
        onStateChange={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Show lyrics' }));
    await screen.findByText('Lyrics not available.');
    fireEvent.click(screen.getByRole('button', { name: 'Show plain lyrics' }));
    fireEvent.click(screen.getByRole('button', { name: 'Show sonic fingerprint' }));
    expect(await screen.findByText('Sonic Fingerprint ✦')).toBeInTheDocument();
    await waitFor(() => expect(apiMock.trackSonicFingerprint).toHaveBeenCalledWith('1'));
    expect(screen.getByText('♩ 124 BPM')).toBeInTheDocument();
    expect(screen.getByText('⚡ 72% energy')).toBeInTheDocument();
    expect(screen.getByText('◎ 88% conf')).toBeInTheDocument();
    expect(screen.getByText('htdemucs')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Show album art' }));
    expect(screen.getByText('Tap for lyrics')).toBeInTheDocument();
  });

  it('builds bounded mobile stem bins for empty, invalid-duration, and overlapping windows', () => {
    expect(buildMobileStemBins([], 120)).toEqual(new Array(80).fill(0));
    expect(buildMobileStemBins([{ start: 0, end: 1, strength: 1, average: 1 }], 0))
      .toEqual(new Array(80).fill(0));
    const bins = buildMobileStemBins([
      { start: 0, end: 60, strength: -1, average: -1 },
      { start: 30, end: 90, strength: 0.5, average: 0.5 },
      { start: 80, end: 120, strength: 2, average: 2 },
    ], 120);
    expect(bins).toHaveLength(80);
    expect(Math.min(...bins)).toBe(0);
    expect(Math.max(...bins)).toBe(1);
    expect(bins.some((value) => value === 0.5)).toBe(true);
  });

  it('opens the live Equalizer and persisted Vinyl tools from the Now Playing header', () => {
    const track = {
      id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3',
      duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song',
      artist: 'Artist', album: 'Album', album_id: '9', library_name: 'Main', track_number: 1,
      disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null,
      bpm: null, scanned_at: '2026-01-01',
    } as any;
    const onPlaybackModeChange = vi.fn();
    render(
      <MobileNowPlayingView
        snapshot={{
          currentTrack: track, currentTime: 30, duration: 120, isPlaying: true,
          volume: 0.5, muted: false, loading: false, audioError: null,
        }}
        playerState={{ queue: [track], currentIndex: 0, isPlaying: true, playToken: 1 }}
        onStateChange={vi.fn()}
        eqControls={{
          bands: DEFAULT_PARAMETRIC_BANDS,
          profile: 'Manual',
          customProfiles: [],
          autoEqEnabled: false,
          autoEqCurrentPreset: 'Rock',
          onAutoEqEnabledChange: vi.fn(),
          onBandsChange: vi.fn(),
          onProfileChange: vi.fn(),
          onSaveProfile: vi.fn().mockResolvedValue(null),
          onDeleteProfile: vi.fn().mockResolvedValue(undefined),
        }}
        playbackMode="standard"
        onPlaybackModeChange={onPlaybackModeChange}
      />,
    );

    expect(screen.getByRole('button', { name: 'Open equalizer' })).toHaveStyle({ minHeight: '44px' });
    fireEvent.click(screen.getByRole('button', { name: 'Open equalizer' }));
    expect(screen.getByRole('dialog', { name: 'Equalizer' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close equalizer' }));

    fireEvent.click(screen.getByRole('button', { name: 'Open Vinyl controls' }));
    expect(screen.getByRole('dialog', { name: 'Vinyl' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Vinyl' }));
    expect(onPlaybackModeChange).toHaveBeenCalledWith('vinyl');
  });
});


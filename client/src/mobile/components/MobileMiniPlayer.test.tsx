/**
 * Tests Mobile Mini Player.Test behavior for BoogieBox regressions.
 */

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import MobileMiniPlayer from './MobileMiniPlayer';
import type { ClientEntityId } from '../../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

describe('MobileMiniPlayer', () => {
  it('renders cached album art and transport controls in the docked playback bar', () => {
    const onStateChange = vi.fn();
    const onOpenNowPlaying = vi.fn();

    const { container } = render(
      <MobileMiniPlayer
        snapshot={{
          currentTrack: {
            id: '1',
            file_path: 'x',
            file_name: 'song.mp3',
            file_size: 1,
            format: 'MP3',
            duration: 240,
            bitrate: 320,
            sample_rate: 44100,
            channels: 2,
            title: 'Velvet Night',
            artist: 'Aural Static',
            album: 'City Lights',
            library_name: 'Main',
            track_number: 1,
            disc_number: 1,
            year: 2026,
            genre: 'Electronic',
            composer: null,
            comment: null,
            bpm: null,
            scanned_at: '2026-01-01',
            album_id: '41',
          },
          currentTime: 60,
          duration: 240,
          isPlaying: true,
          volume: 0.8,
          muted: false,
          loading: false,
          audioError: null,
        }}
        playerState={{
          queue: [
            {
              id: '1',
              file_path: 'x',
              file_name: 'song.mp3',
              file_size: 1,
              format: 'MP3',
              duration: 240,
              bitrate: 320,
              sample_rate: 44100,
              channels: 2,
              title: 'Velvet Night',
              artist: 'Aural Static',
              album: 'City Lights',
              library_name: 'Main',
              track_number: 1,
              disc_number: 1,
              year: 2026,
              genre: 'Electronic',
              composer: null,
              comment: null,
              bpm: null,
              scanned_at: '2026-01-01',
              album_id: '41',
            },
            {
              id: '2',
              file_path: 'y',
              file_name: 'next.mp3',
              file_size: 1,
              format: 'MP3',
              duration: 200,
              bitrate: 320,
              sample_rate: 44100,
              channels: 2,
              title: 'Afterglow',
              artist: 'Aural Static',
              album: 'City Lights',
              library_name: 'Main',
              track_number: 2,
              disc_number: 1,
              year: 2026,
              genre: 'Electronic',
              composer: null,
              comment: null,
              bpm: null,
              scanned_at: '2026-01-01',
              album_id: '41',
            },
          ],
          currentIndex: 0,
          isPlaying: true,
          playToken: 5,
        }}
        onStateChange={onStateChange}
        onOpenNowPlaying={onOpenNowPlaying}
      />,
    );

    expect(screen.getByText('Aural Static')).toBeInTheDocument();
    expect(screen.getByText('Velvet Night')).toBeInTheDocument();
    expect(screen.getByRole('presentation')).toHaveAttribute('src', expect.stringContaining('/api/albums/41/art?size=300'));
    expect(container.firstElementChild).toHaveStyle({
      height: '66px',
      left: '8px',
      bottom: 'calc(env(safe-area-inset-bottom, 0px) + 78px)',
    });
    expect(screen.getByRole('button', { name: 'Pause' })).toHaveStyle({
      width: '44px',
      height: '44px',
    });
    expect(screen.getByRole('button', { name: /Quick rate/i })).toHaveAttribute('aria-pressed', 'false');

    fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ isPlaying: false }));

    fireEvent.click(screen.getByRole('button', { name: 'Next track' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));

    fireEvent.click(screen.getByRole('button', { name: /Aural Static/i }));
    expect(onOpenNowPlaying).toHaveBeenCalledTimes(1);
  });

  it('handles fallback metadata, quick rating, gestures, disabled navigation, and empty queues', () => {
    const track = {
      id: '1',
      file_path: 'x',
      file_name: 'fallback.mp3',
      file_size: 1,
      format: 'MP3',
      duration: 0,
      bitrate: null,
      sample_rate: null,
      channels: null,
      title: '',
      artist: '',
      album: '',
      library_name: 'Main',
      track_number: null,
      disc_number: null,
      year: null,
      genre: null,
      composer: null,
      comment: null,
      bpm: null,
      scanned_at: '2026-01-01',
      album_id: null,
      rating: 4,
    } as any;
    const onStateChange = vi.fn();
    const onOpenNowPlaying = vi.fn();
    const onQuickRate = vi.fn();
    const state = { queue: [track], currentIndex: 0, isPlaying: false, playToken: 1 };
    const { container, rerender } = render(
      <MobileMiniPlayer
        snapshot={{
          currentTrack: null,
          currentTime: -10,
          duration: 0,
          isPlaying: false,
          volume: 1,
          muted: false,
          loading: false,
          audioError: null,
        }}
        playerState={state}
        onStateChange={onStateChange}
        onOpenNowPlaying={onOpenNowPlaying}
        onQuickRate={onQuickRate}
      />,
    );

    expect(screen.getByText('Unknown artist')).toBeInTheDocument();
    expect(screen.getByText('fallback.mp3')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next track' })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Quick rate/i })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ isPlaying: true }));
    fireEvent.click(screen.getByRole('button', { name: /Quick rate/i }));
    expect(onQuickRate).toHaveBeenCalledWith(null);

    const wrap = container.firstElementChild!;
    fireEvent.touchEnd(wrap, { changedTouches: [{ clientX: 10, clientY: 0 }] });
    fireEvent.touchStart(wrap, { touches: [{ clientX: 10, clientY: 100 }] });
    fireEvent.touchEnd(wrap, { changedTouches: [{ clientX: 20, clientY: 40 }] });
    expect(onOpenNowPlaying).toHaveBeenCalled();
    fireEvent.touchStart(wrap, { touches: [{ clientX: 10, clientY: 100 }] });
    fireEvent.touchEnd(wrap, { changedTouches: [{ clientX: 200, clientY: 40 }] });
    expect(onOpenNowPlaying).toHaveBeenCalledTimes(1);

    rerender(
      <MobileMiniPlayer
        snapshot={null}
        playerState={{ ...state, queue: [], currentIndex: 4 }}
        onStateChange={onStateChange}
        onOpenNowPlaying={onOpenNowPlaying}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});


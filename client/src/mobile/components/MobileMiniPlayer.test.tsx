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

    render(
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

    fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ isPlaying: false }));

    fireEvent.click(screen.getByRole('button', { name: 'Next track' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));

    fireEvent.click(screen.getByRole('button', { name: /Aural Static/i }));
    expect(onOpenNowPlaying).toHaveBeenCalledTimes(1);
  });
});


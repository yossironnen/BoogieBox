/**
 * Tests Mobile Now Playing View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import MobileNowPlayingView from './MobileNowPlayingView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    albumArtUrl: vi.fn(() => '/cover.jpg'),
    trackLyrics: vi.fn(),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

describe('MobileNowPlayingView', () => {
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

    fireEvent.click(screen.getByRole('button', { name: /show lyrics/i }));
    await waitFor(() => expect(apiMock.trackLyrics).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('first line')).toBeInTheDocument();
    expect(screen.getByText('Karaoke')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /show plain lyrics/i }));
    expect(await screen.findByText('plain lyrics')).toBeInTheDocument();
    expect(screen.getByText('Text')).toBeInTheDocument();
  });
});


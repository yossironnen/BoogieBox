/**
 * Tests Mobile Browse View.Test behavior for BoogieBox regressions.
 */

import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MobileBrowseView, { clearMobileArtistBrowseCache } from './MobileBrowseView';
import type { ClientEntityId } from '../../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    artists: vi.fn(),
    artistAlbums: vi.fn(),
    albumTracks: vi.fn(),
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`),
    playlists: {
      list: vi.fn(),
      addTrack: vi.fn(),
      create: vi.fn(),
    },
    setTrackRating: vi.fn(),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

describe('MobileBrowseView', () => {
  beforeEach(() => {
    clearMobileArtistBrowseCache();
    vi.clearAllMocks();
    apiMock.artists.mockResolvedValue([]);
    apiMock.artistAlbums.mockResolvedValue([]);
    apiMock.albumTracks.mockResolvedValue([]);
    apiMock.playlists.list.mockResolvedValue([{ id: '77', name: 'Pocket Mix', track_count: 2, total_duration: 500, created_at: '2026-01-01', updated_at: '2026-01-01', description: null }]);
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.create.mockResolvedValue({ id: '88', name: 'Fresh Playlist', track_count: 0, total_duration: 0, created_at: '2026-01-01', updated_at: '2026-01-01', description: null });
    apiMock.setTrackRating.mockResolvedValue({ ok: true });
  });

  it('renders artists and opens an artist album grid', async () => {
    const onSelectionChange = vi.fn();
    apiMock.artists.mockResolvedValue([{ id: '7', name: 'Neon Skyline', track_count: 14, album_count: 2 }]);

    render(
      <MobileBrowseView
        onPlayTrack={() => {}}
        onAddToQueue={() => {}}
        selection={{ artist: null, album: null, tracks: [] }}
        onSelectionChange={onSelectionChange}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /Neon Skyline/i }));
    expect(onSelectionChange).toHaveBeenCalledWith({
      artist: { id: '7', name: 'Neon Skyline', track_count: 14, album_count: 2 },
      album: null,
      tracks: [],
    });
  });

  it('renders album art for a selected artist', async () => {
    apiMock.artistAlbums.mockResolvedValue([
      { id: '11', title: 'Big Album', artist: 'Artist', album_artist: 'Artist', year: 2020, genre: null, track_count: 12 },
    ]);

    render(
      <MobileBrowseView
        onPlayTrack={() => {}}
        onAddToQueue={() => {}}
        selection={{ artist: { id: '2', name: 'Artist', track_count: 10, album_count: 1 }, album: null, tracks: [] }}
        onSelectionChange={() => {}}
      />,
    );

    expect(await screen.findByRole('button', { name: /Big Album/i })).toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('11', 300);
  });

  it('loads album tracks and opens mobile track actions', async () => {
    const onSelectionChange = vi.fn();
    apiMock.albumTracks.mockResolvedValue([
      { id: '99', file_path: 'x', file_name: 'anthem.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Anthem', artist: 'Artist', album: 'Big Album', library_name: 'Main', track_number: 1, disc_number: 1, year: 2020, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', album_id: '11' },
    ]);

    render(
      <MobileBrowseView
        onPlayTrack={() => {}}
        onAddToQueue={() => {}}
        selection={{
          artist: { id: '2', name: 'Artist', track_count: 10, album_count: 1 },
          album: { id: '11', title: 'Big Album', artist: 'Artist', album_artist: 'Artist', year: 2020, genre: null, track_count: 12 },
          tracks: [],
        }}
        onSelectionChange={onSelectionChange}
      />,
    );

    expect(await screen.findByText('Anthem')).toBeInTheDocument();
    await waitFor(() => expect(onSelectionChange).toHaveBeenCalledWith(expect.objectContaining({ tracks: expect.any(Array) })));

    fireEvent.click(screen.getByRole('button', { name: 'More actions for Anthem' }));
    fireEvent.click(await screen.findByRole('button', { name: /Add to Playlist/i }));
    fireEvent.click(await screen.findByRole('button', { name: 'Pocket Mix' }));
    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenCalledWith('77', '99'));
  });
});

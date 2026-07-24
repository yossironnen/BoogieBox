/**
 * Tests Playlists View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PlaylistsView, {
  buildPlaylistCollageAlbumIds,
  createPlaylistFallbackTiles,
  fmtDur,
  fmtTrackDur,
  normalizePlaylistName,
} from '../components/PlaylistsView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      list: vi.fn(),
      create: vi.fn(),
      tracks: vi.fn(),
      update: vi.fn(),
      reorder: vi.fn(),
      remove: vi.fn(),
      removeTrack: vi.fn(),
    },
    crossfade: {
      config: vi.fn(),
      overrides: vi.fn(),
      upsertOverride: vi.fn(),
      removeOverride: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

describe('PlaylistsView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.list.mockResolvedValue([
      {
        id: '1',
        name: 'Road   Trip',
        description: null,
        created_at: '2026-02-25T00:00:00Z',
        updated_at: '2026-02-25T00:00:00Z',
        track_count: 3,
        total_duration: 500,
      },
    ]);
    apiMock.playlists.create.mockResolvedValue({ id: '2', name: 'Focus' });
    apiMock.playlists.tracks.mockResolvedValue([]);
    apiMock.playlists.update.mockResolvedValue({});
    apiMock.playlists.reorder.mockResolvedValue({ ok: true });
    apiMock.playlists.remove.mockResolvedValue({ ok: true });
    apiMock.playlists.removeTrack.mockResolvedValue({ ok: true });
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
  });

  it('prevents creating duplicate playlist names in the Playlists view', async () => {
    render(
      <PlaylistsView
        playTrack={() => {}}
        addToQueue={() => {}}
      />
    );

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByTitle('New playlist'));
    fireEvent.change(screen.getByPlaceholderText('Playlist name'), { target: { value: 'road trip' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(apiMock.playlists.create).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText('A playlist with this name already exists')).toBeInTheDocument());
  });

  it('shows BoogieBox delete confirmation dialog before removing a playlist', async () => {
    render(
      <PlaylistsView
        playTrack={() => {}}
        addToQueue={() => {}}
      />
    );

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole('button', { name: /Road\s+Trip/i }));
    fireEvent.click(screen.getByTitle('Delete playlist'));

    expect(screen.getByText('BoogieBox')).toBeInTheDocument();
    expect(screen.getByText('Delete Playlist')).toBeInTheDocument();
    expect(screen.getByText(/Delete playlist \"Road\s+Trip\"\?/i)).toBeInTheDocument();
    expect(apiMock.playlists.remove).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(apiMock.playlists.remove).toHaveBeenCalledWith('1'));
  });

  it('formats playlist names, durations, collage ids, and fallback tiles at boundaries', () => {
    expect(normalizePlaylistName('  Road   TRIP ')).toBe('road trip');
    expect([fmtDur(null), fmtDur(59), fmtDur(60), fmtDur(3660)])
      .toEqual(['', '0m', '1m', '1h 1m']);
    expect([fmtTrackDur(null), fmtTrackDur(5), fmtTrackDur(65)])
      .toEqual(['–', '0:05', '1:05']);
    expect(createPlaylistFallbackTiles(0)).toEqual([0, 1, 2, 3]);
    expect(createPlaylistFallbackTiles(3)).toEqual([0]);
    expect(createPlaylistFallbackTiles(5)).toEqual([]);

    const rows = [
      { id: '1', album_id: null },
      { id: '2', album_id: 'a' },
      { id: '3', album_id: 'a' },
      { id: '4', album_id: 'b' },
      { id: '5', album_id: 'c' },
      { id: '6', album_id: 'd' },
      { id: '7', album_id: 'e' },
    ] as any;
    expect(buildPlaylistCollageAlbumIds(rows)).toEqual(['a', 'b', 'c', 'd']);
  });
});


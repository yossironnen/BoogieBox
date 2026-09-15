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
  mixOutputToTrack,
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
    boogiemix: {
      listOutputs: vi.fn(),
      playUrl: vi.fn((outputId: string) => `/api/boogiemix/outputs/${outputId}/play`),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

describe('PlaylistsView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
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
    apiMock.boogiemix.listOutputs.mockResolvedValue([]);
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
    const optionButtons = screen.getAllByRole('button', { name: 'More actions' });
    fireEvent.click(optionButtons[optionButtons.length - 1]);
    fireEvent.click(screen.getByTitle('Delete playlist'));

    expect(screen.getByRole('dialog', { name: 'Delete Playlist' })).toBeInTheDocument();
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

  it('synthesizes a playable Track from a finished BoogieMix output', () => {
    const track = mixOutputToTrack(
      {
        id: 'out1',
        job_id: 'job1',
        playlist_id: '1',
        file_name: 'road-trip-mix.mp3',
        name: 'Road Trip — BoogieMix',
        playlist_name: 'Road Trip',
        cover_album_ids: null,
        duration_sec: 245,
        file_size_bytes: 4_200_000,
        format: 'mp3',
        created_at: '2026-03-01T00:00:00Z',
      },
      'Road Trip',
    );

    expect(track).toEqual(expect.objectContaining({
      id: 'boogiemix:out1',
      title: 'Road Trip — BoogieMix',
      artist: 'BoogieBox BoogieMix',
      album: 'Road Trip',
      duration: 245,
      file_name: 'road-trip-mix.mp3',
      file_size: 4_200_000,
      format: 'mp3',
      scanned_at: '2026-03-01T00:00:00Z',
      stream_url_override: '/api/boogiemix/outputs/out1/play',
    }));
    expect(apiMock.boogiemix.playUrl).toHaveBeenCalledWith('out1');
    // Library-only fields are absent so Player's guarded fetches (ratings/lyrics/
    // waveform/EQ) have nothing to key off for a synthetic track.
    expect(track.bpm).toBeNull();
    expect(track.year).toBeNull();
  });

  it('falls back to null when a BoogieMix output has no known size/duration', () => {
    const track = mixOutputToTrack(
      {
        id: 'out2',
        job_id: 'job2',
        playlist_id: '1',
        file_name: 'mystery-mix.mp3',
        name: 'Mix — Mar 2',
        playlist_name: 'Unnamed',
        cover_album_ids: null,
        duration_sec: null,
        file_size_bytes: null,
        format: 'mp3',
        created_at: '2026-03-02T00:00:00Z',
      },
      'Unnamed',
    );

    expect(track.id).toBe('boogiemix:out2');
    expect(track.duration).toBeNull();
    expect(track.file_size).toBeNull();
    expect(track.album_id).toBeNull();
  });

  it('carries the full collage plus a single-image fallback so both the playbar collage and single-image surfaces (vinyl turntable, queue rows) work', () => {
    const track = mixOutputToTrack(
      {
        id: 'out3',
        job_id: 'job3',
        playlist_id: '1',
        file_name: 'club-mix.mp3',
        name: 'Club Night',
        playlist_name: 'Electronic',
        cover_album_ids: '["album-a","album-b","album-c","album-d"]',
        duration_sec: 300,
        file_size_bytes: 5_000_000,
        format: 'mp3',
        created_at: '2026-03-03T00:00:00Z',
      },
      'Electronic',
    );

    expect(track.album_id).toBe('album-a');
    expect(track.cover_album_ids).toEqual(['album-a', 'album-b', 'album-c', 'album-d']);
  });

  it('leaves album_id null when cover_album_ids is malformed or empty', () => {
    const malformed = mixOutputToTrack(
      {
        id: 'out4',
        job_id: 'job4',
        playlist_id: '1',
        file_name: 'broken-mix.mp3',
        name: 'Broken',
        playlist_name: 'Electronic',
        cover_album_ids: 'not-json',
        duration_sec: 300,
        file_size_bytes: 5_000_000,
        format: 'mp3',
        created_at: '2026-03-04T00:00:00Z',
      },
      'Electronic',
    );
    expect(malformed.album_id).toBeNull();
    expect(malformed.cover_album_ids).toBeNull();

    const empty = mixOutputToTrack(
      {
        id: 'out5',
        job_id: 'job5',
        playlist_id: '1',
        file_name: 'empty-mix.mp3',
        name: 'Empty',
        playlist_name: 'Electronic',
        cover_album_ids: '[]',
        duration_sec: 300,
        file_size_bytes: 5_000_000,
        format: 'mp3',
        created_at: '2026-03-05T00:00:00Z',
      },
      'Electronic',
    );
    expect(empty.album_id).toBeNull();
    expect(empty.cover_album_ids).toBeNull();
  });

  it('does not render the playbar collage for a single-album mix, only the single-image fallback', () => {
    const track = mixOutputToTrack(
      {
        id: 'out6',
        job_id: 'job6',
        playlist_id: '1',
        file_name: 'one-album-mix.mp3',
        name: 'One Album',
        playlist_name: 'Electronic',
        cover_album_ids: '["album-a"]',
        duration_sec: 300,
        file_size_bytes: 5_000_000,
        format: 'mp3',
        created_at: '2026-03-06T00:00:00Z',
      },
      'Electronic',
    );
    // Player.tsx only switches to the collage renderer at >= 2 album ids —
    // a single-entry array should still populate album_id for the
    // single-image fallback, which is what this asserts indirectly.
    expect(track.album_id).toBe('album-a');
    expect(track.cover_album_ids).toEqual(['album-a']);
  });
});

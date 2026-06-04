/**
 * Tests Playlists View.Component.Test behavior for BoogieBox regressions.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PlaylistsView from '../components/PlaylistsView';

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
      addTrack: vi.fn(),
    },
    search: vi.fn(),
    albumArtUrl: vi.fn(),
    crossfade: {
      config: vi.fn(),
      overrides: vi.fn(),
      upsertOverride: vi.fn(),
      removeOverride: vi.fn(),
    },
    boogiemix: {
      deepAnalysisStatus: vi.fn(),
      listOutputs: vi.fn(),
      createJob: vi.fn(),
      getJob: vi.fn(),
      cancelJob: vi.fn(),
      outputDownloadUrl: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

const playlist = {
  id: '1',
  name: 'Road Trip',
  description: 'Travel songs',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-02T00:00:00Z',
  track_count: 2,
  total_duration: 420,
  art_album_ids: ['401', '402'],
};

const trackA = {
  playlist_track_id: '1001',
  id: '101',
  album_id: '301',
  title: 'Alpha One',
  artist: 'Artist A',
  album: 'Album A',
  duration: 120,
  file_name: 'alpha-one.mp3',
};

const trackB = {
  playlist_track_id: '1002',
  id: '102',
  album_id: '302',
  title: 'Alpha Two',
  artist: 'Artist A',
  album: 'Album A',
  duration: 180,
  file_name: 'alpha-two.mp3',
};

describe('PlaylistsView integration flows', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.list.mockResolvedValue([playlist]);
    apiMock.playlists.tracks.mockResolvedValue([trackA, trackB]);
    apiMock.playlists.create.mockResolvedValue({ id: '2', name: 'Focus' });
    apiMock.playlists.update.mockResolvedValue({ ...playlist, name: 'Road Trip Updated' });
    apiMock.playlists.reorder.mockResolvedValue({ ok: true });
    apiMock.playlists.remove.mockResolvedValue({ ok: true });
    apiMock.playlists.removeTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.albumArtUrl.mockImplementation((albumId: string, size: number) => `/api/albums/${albumId}/art?size=${size}`);
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    apiMock.boogiemix.deepAnalysisStatus.mockResolvedValue({
      enabled: false,
      runtime: {
        pythonAvailable: true,
        ffmpegAvailable: true,
        demucsCallable: false,
        torchAvailable: false,
        gpuAvailable: false,
        enabled: false,
        details: [],
        missingCapabilities: ['torch', 'demucs'],
        summary: 'Missing: torch, demucs.',
        python: { available: true, version: 'Python 3.12', detail: null },
        ffmpeg: { available: true, version: null, detail: null },
        demucs: { available: false, version: null, detail: null },
        torch: { available: false, version: null, detail: null },
        gpu: { available: false, version: null, detail: 'CPU fallback' },
      },
      queue: { pending: 0, running: 0, failed: 0, skipped: 0, done: 0 },
      cache: { analyzedTracks: 0, estimatedBytes: 0, oldestCreatedAt: null, newestCreatedAt: null },
    });
    apiMock.boogiemix.listOutputs.mockResolvedValue([]);
    apiMock.boogiemix.outputDownloadUrl.mockImplementation((id: string) => `/api/boogiemix/outputs/${id}/file`);
    apiMock.search.mockResolvedValue({
      tracks: [
        {
          id: '201',
          title: 'Gamma Song',
          artist: 'Artist G',
          album: 'Album G',
          duration: 240,
          file_name: 'gamma.mp3',
        },
      ],
      total: 1,
      page: 1,
      limit: 50,
      artists: [],
      albums: [],
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('loads a playlist, plays/queues tracks, reorders, and removes a track', async () => {
    const playTrack = vi.fn();
    const addToQueue = vi.fn();

    render(<PlaylistsView playTrack={playTrack} addToQueue={addToQueue} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));
    expect(screen.getAllByText('Road Trip').length).toBeGreaterThan(0);
    expect(screen.getByText('Alpha One')).toBeInTheDocument();
    expect(screen.getByText('Alpha Two')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Play All/i }));
    expect(playTrack).toHaveBeenCalledWith(expect.objectContaining({ id: '101' }), expect.arrayContaining([expect.objectContaining({ id: '101' }), expect.objectContaining({ id: '102' })]), expect.objectContaining({ type: 'playlist', id: '1' }));

    fireEvent.click(screen.getByRole('button', { name: /Queue All/i }));
    expect(addToQueue).toHaveBeenCalledTimes(2);
    expect(addToQueue).toHaveBeenCalledWith(expect.objectContaining({ id: '101' }));
    expect(addToQueue).toHaveBeenCalledWith(expect.objectContaining({ id: '102' }));

    const rowA = screen.getByText('Alpha One').closest('[draggable="true"]')!;
    const rowB = screen.getByText('Alpha Two').closest('[draggable="true"]')!;
    fireEvent.dragStart(rowA);
    fireEvent.dragEnter(rowB);
    fireEvent.dragEnd(rowA);
    await waitFor(() => expect(apiMock.playlists.reorder).toHaveBeenCalledWith('1', ['102', '101']));

    fireEvent.click(screen.getAllByTitle('Remove from playlist')[0]);
    await waitFor(() => expect(apiMock.playlists.removeTrack).toHaveBeenCalledWith('1', '102'));
  });

  it('renders collage artwork beside the playlist title from album art', async () => {
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));
    expect(screen.getByLabelText('Road Trip artwork')).toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('301', 300);
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('302', 300);
    expect(
      screen.getAllByRole('presentation').some((image) =>
        image.getAttribute('src')?.includes('/api/albums/301/art?size=300'),
      ),
    ).toBe(true);
  });

  it('uses playlist-level artwork ids while tracks are still loading', async () => {
    apiMock.playlists.tracks.mockImplementation(() => new Promise(() => {}));

    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));
    expect(screen.getByLabelText('Road Trip artwork')).toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('401', 300);
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('402', 300);
  });

  it('stops loading and shows an error when playlist tracks fail to load', async () => {
    apiMock.playlists.tracks.mockRejectedValue(new Error('Internal server error'));

    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(screen.getByText('Could not load tracks')).toBeInTheDocument());
    expect(screen.getByText('Internal server error')).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('401', 300);
  });

  it('warns before high-quality BoogieMix when deep analysis dependencies are missing', async () => {
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalled());
    fireEvent.change(screen.getByTitle('BoogieMix quality'), { target: { value: 'high_quality' } });

    expect(screen.getByText(/High Quality needs deep analysis\. Missing: torch, demucs/i)).toBeInTheDocument();
    expect(screen.getByText(/Deep analysis runtime:/i)).toHaveTextContent('Missing: torch, demucs.');
  });

  it('searches and adds tracks from the Add Tracks panel', async () => {
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));
    fireEvent.click(screen.getByRole('button', { name: /Add Tracks/i }));

    fireEvent.change(screen.getByPlaceholderText(/Search for tracks to add/i), { target: { value: 'gamma' } });

    await waitFor(() => expect(apiMock.search).toHaveBeenCalledWith({ q: 'gamma', limit: 50, page: 1 }));
    expect(screen.getByText('Gamma Song')).toBeInTheDocument();

    fireEvent.click(screen.getByTitle('Add to playlist'));
    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenCalledWith('1', '201'));
    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledTimes(2));
  });

  it('renames and deletes the selected playlist', async () => {
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByTitle('Rename'));
    const nameInput = screen.getByDisplayValue('Road Trip');
    fireEvent.change(nameInput, { target: { value: 'Road Trip Updated' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(apiMock.playlists.update).toHaveBeenCalledWith('1', 'Road Trip Updated', 'Travel songs'));

    fireEvent.click(screen.getByTitle('Delete playlist'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(apiMock.playlists.remove).toHaveBeenCalledWith('1'));
  });
});


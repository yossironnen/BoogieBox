/**
 * Tests Mobile Playlists View.Test behavior for BoogieBox regressions.
 */

import React, { useCallback, useState } from 'react';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import type { BoogieMixOutput, ClientEntityId, Playlist, PlaylistTrack, Track } from '../../types';
import type { MobilePlaylistSelection } from '../mobileShell';
import MobilePlaylistsView, {
  buildCollageAlbumIds,
  createFallbackTiles,
  fmtDuration,
  fmtTrackDuration,
  resolvePlaylistDuration,
  sortPlaylistTracks,
  updatePlaylistSummary,
} from './MobilePlaylistsView';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      list: vi.fn(),
      get: vi.fn(),
      tracks: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      addTrack: vi.fn(),
      reorder: vi.fn(),
      removeTrack: vi.fn(),
      exportM3uUrl: vi.fn(),
    },
    boogiemix: {
      listAllOutputs: vi.fn(),
      renameOutput: vi.fn(),
      deleteOutput: vi.fn(),
      playUrl: vi.fn((outputId: ClientEntityId) => `/api/boogiemix/outputs/${outputId}/play`),
    },
    search: vi.fn(),
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

const playlist: Playlist = {
  id: '7',
  name: 'Some Electro',
  description: 'Late-night mobile mix.',
  created_at: '2026-03-01T00:00:00.000Z',
  updated_at: '2026-03-17T00:00:00.000Z',
  track_count: 3,
  total_duration: 10800,
  art_album_ids: ['31'],
  remember_progress: 0,
};

const tracks: PlaylistTrack[] = [
  {
    id: '11',
    playlist_track_id: '501',
    position: 1,
    file_path: 'x',
    file_name: 'one.mp3',
    file_size: 1,
    format: 'MP3',
    duration: 220,
    bitrate: 320,
    sample_rate: 44100,
    channels: 2,
    title: 'Neon One',
    artist: 'Artist A',
    album: 'Album A',
    album_id: '31',
    library_name: 'Main',
    track_number: 1,
    disc_number: 1,
    year: 2024,
    genre: 'Electronic',
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-03-01T00:00:00.000Z',
  },
  {
    id: '22',
    playlist_track_id: '502',
    position: 2,
    file_path: 'y',
    file_name: 'two.mp3',
    file_size: 1,
    format: 'MP3',
    duration: 240,
    bitrate: 320,
    sample_rate: 44100,
    channels: 2,
    title: 'Neon Two',
    artist: 'Artist B',
    album: 'Album B',
    album_id: '32',
    library_name: 'Main',
    track_number: 2,
    disc_number: 1,
    year: 2024,
    genre: 'Electronic',
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-03-01T00:00:00.000Z',
  },
  {
    id: '33',
    playlist_track_id: '503',
    position: 3,
    file_path: 'z',
    file_name: 'three.mp3',
    file_size: 1,
    format: 'MP3',
    duration: 260,
    bitrate: 320,
    sample_rate: 44100,
    channels: 2,
    title: 'Neon Three',
    artist: 'Artist C',
    album: 'Album C',
    album_id: '33',
    library_name: 'Main',
    track_number: 3,
    disc_number: 1,
    year: 2024,
    genre: 'Electronic',
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-03-01T00:00:00.000Z',
  },
];

const mixOutput: BoogieMixOutput = {
  id: 'mix-1',
  job_id: 'job-1',
  playlist_id: '7',
  file_name: 'late-night-mix.mp3',
  name: 'Late Night Mix',
  playlist_name: 'Some Electro',
  cover_album_ids: '["31","32"]',
  duration_sec: 725,
  file_size_bytes: 123456,
  format: 'mp3',
  created_at: '2026-03-10T00:00:00.000Z',
};

function TestHarness({
  initialSelection,
  onSelectionChangeSpy,
  onPlayMixTrackSpy,
}: {
  initialSelection: MobilePlaylistSelection;
  onSelectionChangeSpy?: (selection: MobilePlaylistSelection) => void;
  onPlayMixTrackSpy?: (track: Track, allTracks?: Track[]) => void;
}) {
  const [selection, setSelection] = useState(initialSelection);
  const handleSelectionChange = useCallback((next: MobilePlaylistSelection) => {
    onSelectionChangeSpy?.(next);
    setSelection(next);
  }, [onSelectionChangeSpy]);

  return (
    <MobilePlaylistsView
      initialPlaylistId={initialSelection.playlist?.id ?? null}
      selection={selection}
      onSelectionChange={handleSelectionChange}
      onPlayTrack={() => {}}
      onPlayMixTrack={onPlayMixTrackSpy ?? (() => {})}
      onAddToQueue={() => {}}
    />
  );
}

describe('MobilePlaylistsView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.list.mockResolvedValue([playlist]);
    apiMock.playlists.get.mockResolvedValue(playlist);
    apiMock.playlists.tracks.mockResolvedValue(tracks);
    apiMock.playlists.create.mockResolvedValue({ ...playlist, id: '99', name: 'Fresh Queue', track_count: 0, total_duration: 0 });
    apiMock.playlists.update.mockResolvedValue({ ...playlist, name: 'Updated Electro', description: 'Edited mobile copy.' });
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.reorder.mockResolvedValue({ ok: true });
    apiMock.playlists.removeTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.exportM3uUrl.mockReturnValue('#playlist-export');
    apiMock.search.mockResolvedValue({ tracks: [] });
    apiMock.albumArtUrl.mockImplementation((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`);
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([mixOutput]);
    apiMock.boogiemix.renameOutput.mockResolvedValue({ ok: true });
    apiMock.boogiemix.deleteOutput.mockResolvedValue({ ok: true });
  });

  it('renders playlist art thumbnails on mobile track rows', async () => {
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    expect(await screen.findByText('Some Electro')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Build a BoogieMix/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Play Neon One' })).toBeInTheDocument();
    expect(
      screen
        .getAllByRole('presentation')
        .some((image) => image.getAttribute('src')?.includes('/api/albums/31/art?size=300')),
    ).toBe(true);
  });

  it('falls back to loaded track durations when the playlist summary duration is zero', async () => {
    apiMock.playlists.get.mockResolvedValue({ ...playlist, total_duration: 0 });
    render(<TestHarness initialSelection={{ playlist: { ...playlist, total_duration: 0 }, tracks }} />);

    expect(await screen.findByText('Some Electro')).toBeInTheDocument();
    expect(screen.getByText('3 tracks - 12m')).toBeInTheDocument();
  });

  it('reorders tracks by dragging the mobile handle', async () => {
    const onSelectionChangeSpy = vi.fn();
    render(<TestHarness initialSelection={{ playlist, tracks }} onSelectionChangeSpy={onSelectionChangeSpy} />);

    await screen.findByText('Neon One');
    onSelectionChangeSpy.mockClear();
    apiMock.playlists.reorder.mockClear();

    const reorderHandle = screen.getByRole('button', { name: 'Reorder Neon One' });
    fireEvent.pointerDown(reorderHandle, { button: 0, pointerId: 1, clientX: 20, clientY: 20 });
    fireEvent.pointerMove(reorderHandle, { pointerId: 1, clientX: 20, clientY: 120 });
    fireEvent.pointerUp(reorderHandle, { pointerId: 1, clientX: 20, clientY: 120 });

    await waitFor(() => expect(apiMock.playlists.reorder).toHaveBeenCalledWith('7', ['22', '11', '33']));
    await waitFor(() => expect(onSelectionChangeSpy).toHaveBeenCalledWith(expect.objectContaining({ tracks: expect.arrayContaining([expect.objectContaining({ id: '22' })]) })));
    const titles = screen.getAllByText(/Neon /i).map((node) => node.textContent);
    expect(titles.slice(0, 3)).toEqual(['Neon Two', 'Neon One', 'Neon Three']);
  });

  it('deletes a track after swiping left on mobile', async () => {
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    const row = await screen.findByRole('button', { name: 'Play Neon One' });
    fireEvent.pointerDown(row, { button: 0, pointerId: 2, clientX: 180, clientY: 20 });
    fireEvent.pointerMove(row, { pointerId: 2, clientX: 40, clientY: 20 });
    fireEvent.pointerUp(row, { pointerId: 2, clientX: 40, clientY: 20 });

    const removeDialog = await screen.findByRole('alertdialog', { name: 'Remove this track?' });
    expect(apiMock.playlists.removeTrack).not.toHaveBeenCalled();
    fireEvent.click(within(removeDialog).getByRole('button', { name: 'Remove track' }));
    await waitFor(() => expect(apiMock.playlists.removeTrack).toHaveBeenCalledWith('7', '501'));
    await waitFor(() => expect(screen.queryByText('Neon One')).not.toBeInTheDocument());
  });

  it('creates a playlist from the mobile root and opens it', async () => {
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} />);

    fireEvent.click(await screen.findByRole('button', { name: 'New Playlist' }));
    fireEvent.change(screen.getByPlaceholderText('Late-night mix'), { target: { value: 'Fresh Queue' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));

    await waitFor(() => expect(apiMock.playlists.create).toHaveBeenCalledWith('Fresh Queue', ''));
    await waitFor(() => expect(apiMock.playlists.get).toHaveBeenCalledWith('99'));
  });

  it('renders artwork-backed Hybrid playlist rows with mobile-sized actions', async () => {
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} />);

    const playlistRow = await screen.findByRole('button', { name: /Some Electro/i });
    expect(playlistRow).toHaveStyle({ minHeight: '66px' });
    expect(screen.getByRole('button', { name: 'New Playlist' })).toHaveStyle({ minHeight: '48px' });
    expect(
      screen
        .getAllByRole('presentation')
        .some((image) => image.getAttribute('src')?.includes('/api/albums/31/art?size=300')),
    ).toBe(true);
  });

  it('edits a playlist from the mobile detail hero', async () => {
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    fireEvent.change(screen.getByDisplayValue('Some Electro'), { target: { value: 'Updated Electro' } });
    fireEvent.change(screen.getByDisplayValue('Late-night mobile mix.'), { target: { value: 'Edited mobile copy.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(apiMock.playlists.update).toHaveBeenCalledWith('7', 'Updated Electro', 'Edited mobile copy.'));
    expect(await screen.findByText('Updated Electro')).toBeInTheDocument();
  });

  it('opens track actions from the playlist kebab and adds to queue', async () => {
    const onAddToQueue = vi.fn();
    const onPlayTrack = vi.fn();
    render(
      <MobilePlaylistsView
        initialPlaylistId={playlist.id}
        selection={{ playlist, tracks }}
        onSelectionChange={() => {}}
        onPlayTrack={onPlayTrack}
        onPlayMixTrack={() => {}}
        onAddToQueue={onAddToQueue}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'More actions for Neon One' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add To Queue' }));

    expect(onAddToQueue).toHaveBeenCalledWith(tracks[0]);
    expect(onPlayTrack).not.toHaveBeenCalled();
  });

  it('adds a playlist track to another playlist from the picker flow', async () => {
    apiMock.playlists.list.mockResolvedValue([playlist, { ...playlist, id: '8', name: 'Road Trip' }]);
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    fireEvent.click(await screen.findByRole('button', { name: 'More actions for Neon One' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add To Playlist' }));
    fireEvent.click(screen.getByRole('button', { name: 'Road Trip' }));

    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenCalledWith('8', '11'));
  });

  it('loads root failures, retries, opens a playlist, refreshes by pull, and returns', async () => {
    apiMock.playlists.list.mockRejectedValueOnce(new Error('List failed')).mockResolvedValue([playlist]);
    const onSelectionChange = vi.fn();
    const { container, rerender } = render(
      <MobilePlaylistsView
        initialPlaylistId={null}
        selection={{ playlist: null, tracks: [] }}
        onSelectionChange={onSelectionChange}
        onPlayTrack={vi.fn()}
        onPlayMixTrack={vi.fn()}
        onAddToQueue={vi.fn()}
      />,
    );
    expect(await screen.findByText('List failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByRole('button', { name: /Some Electro/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Some Electro/i }));
    expect(onSelectionChange).toHaveBeenCalledWith({ playlist, tracks: [] });

    const root = container.firstElementChild!;
    fireEvent.touchStart(root, { touches: [{ clientY: 10 }] });
    fireEvent.touchMove(root, { touches: [{ clientY: 100 }] });
    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(3));
    fireEvent.touchEnd(root);

    rerender(
      <MobilePlaylistsView
        initialPlaylistId="7"
        selection={{ playlist, tracks }}
        onSelectionChange={onSelectionChange}
        onPlayTrack={vi.fn()}
        onPlayMixTrack={vi.fn()}
        onAddToQueue={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Back' }));
    expect(onSelectionChange).toHaveBeenLastCalledWith({ playlist: null, tracks: [] });
  });

  it('renders empty root and supports both create entry points and validation failures', async () => {
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockRejectedValueOnce(new Error('Create failed'));
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} />);
    expect(await screen.findByText('No playlists yet.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Create Playlist' }));
    fireEvent.change(screen.getByPlaceholderText('Late-night mix'), { target: { value: 'Broken' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));
    expect(await screen.findByText('Create failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Close New Playlist/i }));
    fireEvent.click(screen.getByRole('button', { name: 'New Playlist' }));
    expect(screen.getByPlaceholderText('Late-night mix')).toBeInTheDocument();
  });

  it('plays, filters, sorts, exports, and shows empty search results in playlist detail', async () => {
    const onPlayTrack = vi.fn();
    render(
      <MobilePlaylistsView
        initialPlaylistId="7"
        selection={{ playlist, tracks }}
        onSelectionChange={vi.fn()}
        onPlayTrack={onPlayTrack}
        onPlayMixTrack={vi.fn()}
        onAddToQueue={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Play' }));
    expect(onPlayTrack).toHaveBeenCalledWith(tracks[0], tracks);
    fireEvent.click(screen.getByRole('button', { name: /Export/i }));
    expect(apiMock.playlists.exportM3uUrl).toHaveBeenCalledWith('7');

    fireEvent.click(screen.getByRole('button', { name: 'Manual order' }));
    fireEvent.click(screen.getByRole('button', { name: 'By artist' }));
    fireEvent.click(screen.getByRole('button', { name: 'By artist' }));
    fireEvent.click(screen.getByRole('button', { name: 'By album' }));
    fireEvent.click(screen.getByRole('button', { name: 'By album' }));
    fireEvent.click(screen.getByRole('button', { name: 'By rating' }));
    fireEvent.change(screen.getByPlaceholderText('Search this playlist'), { target: { value: 'not-found' } });
    expect(screen.getByText('No tracks match that search.')).toBeInTheDocument();
  });

  it('searches for tracks, handles add failures, and refreshes after a successful add', async () => {
    const candidate = { ...tracks[0], id: '88', title: '', file_name: 'candidate.mp3', artist: '', album: '' };
    apiMock.search.mockResolvedValue({ tracks: [tracks[0], candidate] });
    apiMock.playlists.addTrack.mockRejectedValueOnce(new Error('Add failed')).mockResolvedValue({ ok: true });
    render(<TestHarness initialSelection={{ playlist, tracks }} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Add' }));
    expect(screen.getByText('Search your library to add tracks.')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/Search tracks for/i), { target: { value: 'candidate' } });
    await waitFor(() => expect(apiMock.search).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Added' })).toBeDisabled();
    const addButtons = screen.getAllByRole('button', { name: 'Add' });
    fireEvent.click(addButtons[addButtons.length - 1]);
    expect(await screen.findByText('Add failed')).toBeInTheDocument();
    const retryButtons = screen.getAllByRole('button', { name: 'Add' });
    fireEvent.click(retryButtons[retryButtons.length - 1]);
    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenLastCalledWith('7', '88'));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
  });

  it('restores server state after reorder, removal, and edit failures', async () => {
    apiMock.playlists.reorder.mockRejectedValueOnce(new Error('reorder'));
    apiMock.playlists.removeTrack.mockRejectedValueOnce(new Error('remove'));
    apiMock.playlists.update.mockRejectedValueOnce(new Error('Update failed'));
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    const handle = await screen.findByRole('button', { name: 'Reorder Neon One' });
    fireEvent.pointerDown(handle, { button: 0, pointerId: 3, clientX: 20, clientY: 20 });
    fireEvent.pointerMove(handle, { pointerId: 3, clientX: 20, clientY: 120 });
    fireEvent.pointerUp(handle, { pointerId: 3, clientX: 20, clientY: 120 });
    expect(await screen.findByText('Could not reorder tracks.')).toBeInTheDocument();

    const row = screen.getByRole('button', { name: 'Play Neon One' });
    fireEvent.pointerDown(row, { button: 0, pointerId: 4, clientX: 180, clientY: 20 });
    fireEvent.pointerMove(row, { pointerId: 4, clientX: 40, clientY: 20 });
    fireEvent.pointerUp(row, { pointerId: 4, clientX: 40, clientY: 20 });
    const removeDialog = await screen.findByRole('alertdialog', { name: 'Remove this track?' });
    fireEvent.click(within(removeDialog).getByRole('button', { name: 'Remove track' }));
    expect(await screen.findByText('Could not remove track.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    fireEvent.change(screen.getByDisplayValue('Some Electro'), { target: { value: 'Failed Name' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(await screen.findByText('Update failed')).toBeInTheDocument();
  });

  it('formats durations and derives bounded playlist summaries and collage tiles', () => {
    expect([fmtDuration(null), fmtDuration(30), fmtDuration(60), fmtDuration(3660)])
      .toEqual(['0m', '1m', '1m', '1h 1m']);
    expect([fmtTrackDuration(undefined), fmtTrackDuration(5), fmtTrackDuration(65)])
      .toEqual(['--', '0:05', '1:05']);
    expect(createFallbackTiles(0)).toEqual([0, 1, 2, 3]);
    expect(createFallbackTiles(3)).toEqual([0]);
    expect(createFallbackTiles(5)).toEqual([]);

    const collageTracks = [
      { ...tracks[0], album_id: null },
      tracks[0],
      { ...tracks[0], id: '2a' },
      { ...tracks[1], album_id: '32' },
      { ...tracks[2], album_id: '33' },
      { ...tracks[2], id: '44', album_id: '44' },
      { ...tracks[2], id: '55', album_id: '55' },
    ];
    expect(buildCollageAlbumIds(collageTracks)).toEqual(['31', '32', '33', '44']);

    expect(updatePlaylistSummary({ ...playlist, track_count: 0, total_duration: 0 }, -1, -20))
      .toEqual(expect.objectContaining({ track_count: 0, total_duration: 0 }));
    expect(resolvePlaylistDuration(playlist, tracks)).toBe(10800);
    expect(resolvePlaylistDuration({ ...playlist, total_duration: 0 }, [])).toBe(0);
    expect(resolvePlaylistDuration({ ...playlist, total_duration: 0 }, tracks)).toBe(720);
  });

  it('sorts playlist tracks through manual, artist, album, rating, null, and tie branches', () => {
    const sortable = [
      { ...tracks[0], id: 'a', artist: 'Zulu', album: null, title: '', file_name: 'z.mp3', rating: null },
      { ...tracks[1], id: 'b', artist: null, album: 'Beta', title: 'Bravo', rating: 5 },
      { ...tracks[2], id: 'c', artist: 'Alpha', album: 'Able', title: 'Charlie', rating: 5 },
      { ...tracks[2], id: 'd', artist: 'Alpha', album: 'Able', title: 'Delta', rating: 2 },
    ];
    expect(sortPlaylistTracks(sortable, 'manual')).toBe(sortable);
    expect(sortPlaylistTracks(sortable, 'artist').map((track) => track.id)).toEqual(['c', 'd', 'b', 'a']);
    expect(sortPlaylistTracks(sortable, 'album').map((track) => track.id)).toEqual(['c', 'd', 'b', 'a']);
    expect(sortPlaylistTracks(sortable, 'rating').map((track) => track.id)).toEqual(['c', 'b', 'd', 'a']);
  });

  it('switches to the Mixes segment, plays a mix, and shows its source playlist', async () => {
    const onPlayMixTrackSpy = vi.fn();
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} onPlayMixTrackSpy={onPlayMixTrackSpy} />);

    await screen.findByText('Some Electro');
    fireEvent.click(screen.getByRole('button', { name: /Mixes/ }));

    expect(await screen.findByText('Late Night Mix')).toBeInTheDocument();
    expect(screen.getByText('From Some Electro')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Play Late Night Mix' }));
    expect(onPlayMixTrackSpy).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'boogiemix:mix-1', title: 'Late Night Mix' }),
      expect.any(Array),
    );

    fireEvent.click(screen.getByRole('button', { name: 'More actions for Late Night Mix' }));
    fireEvent.click(screen.getByRole('button', { name: /Open source playlist/i }));
    await waitFor(() => expect(apiMock.playlists.get).toHaveBeenCalledWith('7'));
    expect(screen.getByText('3 tracks - 3h 0m')).toBeInTheDocument();
  });

  it('renames and deletes a mix from its actions sheet', async () => {
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} />);
    fireEvent.click(screen.getByRole('button', { name: /Mixes/ }));
    await screen.findByText('Late Night Mix');

    fireEvent.click(screen.getByRole('button', { name: 'More actions for Late Night Mix' }));
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const renameInput = screen.getByDisplayValue('Late Night Mix');
    fireEvent.change(renameInput, { target: { value: 'Renamed Mix' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Renamed Mix')).toBeInTheDocument();
    expect(apiMock.boogiemix.renameOutput).toHaveBeenCalledWith('mix-1', 'Renamed Mix');

    fireEvent.click(screen.getByRole('button', { name: 'More actions for Renamed Mix' }));
    fireEvent.click(screen.getByRole('button', { name: /Delete mix/i }));
    const deleteDialog = await screen.findByRole('alertdialog', { name: 'Delete this mix?' });
    fireEvent.click(within(deleteDialog).getByRole('button', { name: 'Delete mix' }));
    await waitFor(() => expect(apiMock.boogiemix.deleteOutput).toHaveBeenCalledWith('mix-1'));
    expect(screen.queryByText('Renamed Mix')).not.toBeInTheDocument();
  });

  it('shows an empty state and a retry action for the Mixes segment', async () => {
    apiMock.boogiemix.listAllOutputs.mockResolvedValueOnce([]);
    render(<TestHarness initialSelection={{ playlist: null, tracks: [] }} />);
    fireEvent.click(screen.getByRole('button', { name: /Mixes/ }));
    expect(await screen.findByText('No mixes yet.')).toBeInTheDocument();

    apiMock.boogiemix.listAllOutputs.mockRejectedValueOnce(new Error('offline'));
    fireEvent.click(screen.getByRole('button', { name: /Playlists/ }));
    fireEvent.click(screen.getByRole('button', { name: /Mixes/ }));
    expect(await screen.findByText('offline')).toBeInTheDocument();

    apiMock.boogiemix.listAllOutputs.mockResolvedValueOnce([mixOutput]);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('Late Night Mix')).toBeInTheDocument();
  });
});


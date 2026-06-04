/**
 * Tests Mobile Playlists View.Test behavior for BoogieBox regressions.
 */

import React, { useCallback, useState } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import type { ClientEntityId, Playlist, PlaylistTrack } from '../../types';
import type { MobilePlaylistSelection } from '../mobileShell';
import MobilePlaylistsView from './MobilePlaylistsView';

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
    },
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

function TestHarness({
  initialSelection,
  onSelectionChangeSpy,
}: {
  initialSelection: MobilePlaylistSelection;
  onSelectionChangeSpy?: (selection: MobilePlaylistSelection) => void;
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
      onAddToQueue={() => {}}
    />
  );
}

describe('MobilePlaylistsView', () => {
  beforeEach(() => {
    apiMock.playlists.list.mockResolvedValue([playlist]);
    apiMock.playlists.get.mockResolvedValue(playlist);
    apiMock.playlists.tracks.mockResolvedValue(tracks);
    apiMock.playlists.create.mockResolvedValue({ ...playlist, id: '99', name: 'Fresh Queue', track_count: 0, total_duration: 0 });
    apiMock.playlists.update.mockResolvedValue({ ...playlist, name: 'Updated Electro', description: 'Edited mobile copy.' });
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.reorder.mockResolvedValue({ ok: true });
    apiMock.playlists.removeTrack.mockResolvedValue({ ok: true });
    apiMock.albumArtUrl.mockImplementation((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`);
  });

  it('renders playlist art thumbnails on mobile track rows', async () => {
    render(<TestHarness initialSelection={{ playlist, tracks }} />);

    expect(await screen.findByText('Some Electro')).toBeInTheDocument();
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
});


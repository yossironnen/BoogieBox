/**
 * Tests Mobile Action Sheets.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../api';
import type { Playlist } from '../../types';
import { MobilePlaylistEditorSheet, useMobileTrackActions } from './MobileActionSheets';

vi.mock('../../api', () => ({
  api: {
    albumArtUrl: vi.fn((albumId: string, size: number) => `/api/albums/${albumId}/art?size=${size}`),
    playlists: {
      create: vi.fn(),
      addTrack: vi.fn(),
    },
  },
}));

const playlists: Playlist[] = [
  {
    id: '1',
    name: 'Night Drive',
    description: null,
    created_at: '2026-01-01',
    updated_at: '2026-01-01',
    track_count: 2,
    total_duration: 500,
    art_album_ids: ['3'],
  },
];

function Harness() {
  const actions = useMobileTrackActions({
    playlists,
    onPlayTrack: vi.fn(),
    onAddToQueue: vi.fn(),
  });

  return (
    <div>
      <button
        type="button"
        onClick={() => actions.openForTrack({
          id: '22',
          file_path: 'x',
          file_name: 'neon.mp3',
          file_size: 1,
          format: 'MP3',
          duration: 200,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Neon',
          artist: 'Artist',
          album: 'Album',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2025,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-01-01',
        })}
      >
        Open
      </button>
      {actions.actionsSheet}
      {actions.pickerSheet}
      {actions.createSheet}
    </div>
  );
}

describe('MobileActionSheets', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('blocks duplicate playlist names in the editor sheet', async () => {
    const onSubmit = vi.fn();
    render(
      <MobilePlaylistEditorSheet
        open={true}
        mode="create"
        existingPlaylists={playlists}
        onClose={() => {}}
        onSubmit={onSubmit}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('Late-night mix'), { target: { value: ' night   drive ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));

    expect(await screen.findByText('A playlist with this name already exists.')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('creates a playlist from the picker flow and adds the active track', async () => {
    vi.mocked(api.playlists.create).mockResolvedValue({
      id: '7',
      name: 'Fresh Picks',
      description: null,
      created_at: '2026-01-01',
      updated_at: '2026-01-01',
      track_count: 0,
      total_duration: 0,
    });
    vi.mocked(api.playlists.addTrack).mockResolvedValue({ ok: true });

    render(<Harness />);

    fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add To Playlist' }));
    expect(document.querySelector('img[src*="/api/albums/3/art?size=300"]')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'New Playlist' }));
    fireEvent.change(screen.getByPlaceholderText('Late-night mix'), { target: { value: 'Fresh Picks' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist' }));

    await waitFor(() => expect(api.playlists.create).toHaveBeenCalledWith('Fresh Picks', ''));
    await waitFor(() => expect(api.playlists.addTrack).toHaveBeenCalledWith('7', '22'));
  });
});

/**
 * Tests Playlists View.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
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
});


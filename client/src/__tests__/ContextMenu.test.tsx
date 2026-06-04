/**
 * Tests Context Menu.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ContextMenuRoot } from '../components/ContextMenu';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      list: vi.fn(),
      create: vi.fn(),
      addTrack: vi.fn(),
      addTracks: vi.fn(),
    },
    albumTracks: vi.fn(),
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

describe('ContextMenuRoot', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.list.mockResolvedValue([{ id: '1', name: 'Road   Trip' }]);
    apiMock.playlists.create.mockResolvedValue({ id: '2', name: 'Focus' });
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.addTracks.mockResolvedValue({ ok: true });
    apiMock.albumTracks.mockResolvedValue([]);
  });

  it('prevents duplicate playlist creation from the context menu', async () => {
    render(<ContextMenuRoot />);

    await act(async () => {
      window.dispatchEvent(new CustomEvent('boogiebox:contextmenu', {
        detail: {
          x: 20,
          y: 20,
          target: { kind: 'track', trackId: 9, title: 'Chromakey Dreamcoat' },
          callbacks: { onPlay: vi.fn(), onQueue: vi.fn() },
        },
      }));
    });

    fireEvent.click(await screen.findByRole('button', { name: /Add to playlist/i }));
    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole('button', { name: /\+ New playlist/i }));
    fireEvent.change(screen.getByPlaceholderText(/Playlist name/i), { target: { value: 'road trip' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(apiMock.playlists.create).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText('A playlist with this name already exists')).toBeInTheDocument());
  });

  it('opens playlist submenu on hover', async () => {
    render(<ContextMenuRoot />);

    await act(async () => {
      window.dispatchEvent(new CustomEvent('boogiebox:contextmenu', {
        detail: {
          x: 20,
          y: 20,
          target: { kind: 'track', trackId: 9, title: 'Chromakey Dreamcoat' },
          callbacks: { onPlay: vi.fn(), onQueue: vi.fn() },
        },
      }));
    });

    fireEvent.mouseEnter(screen.getByRole('button', { name: /Add to playlist/i }));

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(1));
    expect(screen.getByRole('button', { name: /Road Trip/i })).toBeInTheDocument();
  });
});


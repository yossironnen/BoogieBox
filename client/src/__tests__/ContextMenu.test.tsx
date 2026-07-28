/**
 * Tests Context Menu.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ContextMenuRoot,
  KebabButton,
  openContextMenu,
  openKebabMenu,
  type ContextCallbacks,
  type ContextTarget,
} from '../components/ContextMenu';

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
  const open = async (target: ContextTarget, callbacks: ContextCallbacks = {}) => {
    await act(async () => {
      window.dispatchEvent(new CustomEvent('boogiebox:contextmenu', {
        detail: { x: window.innerWidth, y: window.innerHeight, target, callbacks },
      }));
    });
  };

  beforeEach(() => {
    vi.resetAllMocks();
    apiMock.playlists.list.mockResolvedValue([{ id: '1', name: 'Road   Trip' }]);
    apiMock.playlists.create.mockResolvedValue({ id: '2', name: 'Focus' });
    apiMock.playlists.addTrack.mockResolvedValue({ ok: true });
    apiMock.playlists.addTracks.mockResolvedValue({ ok: true });
    apiMock.albumTracks.mockResolvedValue([]);
    apiMock.crossfade.config.mockResolvedValue({ mode: 'crossfade', duration: 4, source: 'override' });
    apiMock.crossfade.upsertOverride.mockResolvedValue({ ok: true });
    apiMock.crossfade.removeOverride.mockResolvedValue({ ok: true });
  });

  it('prevents duplicate playlist creation from the context menu', async () => {
    render(<ContextMenuRoot />);

    await act(async () => {
      window.dispatchEvent(new CustomEvent('boogiebox:contextmenu', {
        detail: {
          x: 20,
          y: 20,
          target: { kind: 'track', trackId: '9', title: 'Chromakey Dreamcoat' },
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
          target: { kind: 'track', trackId: '9', title: 'Chromakey Dreamcoat' },
          callbacks: { onPlay: vi.fn(), onQueue: vi.fn() },
        },
      }));
    });

    fireEvent.mouseEnter(screen.getByRole('button', { name: /Add to playlist/i }));

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole('button', { name: /Road Trip/i })).toBeInTheDocument();
  });

  it('runs track, artist, and playlist callbacks and dismisses the menu', async () => {
    render(<ContextMenuRoot />);
    const callbacks = {
      onPlay: vi.fn(),
      onQueue: vi.fn(),
      onOpen: vi.fn(),
      onRename: vi.fn(),
      onDelete: vi.fn(),
      onRemove: vi.fn(),
    };

    await open({ kind: 'track', trackId: '9', title: 'Track' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(callbacks.onPlay).toHaveBeenCalled();

    await open({ kind: 'track', trackId: '9', title: 'Track' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: /Add to queue/i }));
    expect(callbacks.onQueue).toHaveBeenCalled();

    await open({ kind: 'track', trackId: '9', title: 'Track' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: /Remove from playlist/i }));
    expect(callbacks.onRemove).toHaveBeenCalled();

    await open({ kind: 'artist', artistId: '5', name: 'Artist' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: /Play artist radio/i }));
    expect(callbacks.onPlay).toHaveBeenCalledTimes(2);

    await open({ kind: 'artist', artistId: '5', name: 'Artist' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: /Open artist/i }));
    expect(callbacks.onOpen).toHaveBeenCalled();

    await open({ kind: 'playlist', playlistId: 'p1', name: 'Mix' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    expect(callbacks.onRename).toHaveBeenCalled();

    await open({ kind: 'playlist', playlistId: 'p1', name: 'Mix' }, callbacks);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(callbacks.onDelete).toHaveBeenCalled();
    expect(screen.queryByText('Mix')).not.toBeInTheDocument();
  });

  it('adds tracks and albums to existing and newly created playlists', async () => {
    render(<ContextMenuRoot />);

    await open({ kind: 'track', trackId: '9', title: 'Track' });
    fireEvent.mouseEnter(screen.getByRole('button', { name: /Add to playlist/i }));
    fireEvent.click(await screen.findByRole('button', { name: /Road Trip/i }));
    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenCalledWith('1', '9'));

    apiMock.albumTracks.mockResolvedValue([{ id: 11 }, { id: 12 }]);
    await open({ kind: 'album', albumId: '8', title: 'Album' });
    fireEvent.click(screen.getByRole('button', { name: /Add to playlist/i }));
    fireEvent.click(await screen.findByRole('button', { name: /Road Trip/i }));
    await waitFor(() => expect(apiMock.playlists.addTracks).toHaveBeenCalledWith('1', [11, 12]));

    await open({ kind: 'track', trackId: '10', title: 'Track 2' });
    fireEvent.click(screen.getByRole('button', { name: /Add to playlist/i }));
    fireEvent.click(await screen.findByRole('button', { name: /\+ New playlist/i }));
    fireEvent.change(screen.getByPlaceholderText(/Playlist name/i), { target: { value: ' Focus ' } });
    fireEvent.keyDown(screen.getByPlaceholderText(/Playlist name/i), { key: 'Enter' });
    await waitFor(() => expect(apiMock.playlists.create).toHaveBeenCalledWith('Focus'));
    await waitFor(() => expect(apiMock.playlists.addTrack).toHaveBeenCalledWith('2', '10'));
  });

  it('handles playlist loading, creation, and add failures', async () => {
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockRejectedValueOnce(new Error('Create failed'));
    apiMock.playlists.addTrack.mockRejectedValueOnce(new Error('Add failed'));
    render(<ContextMenuRoot />);
    await open({ kind: 'track', trackId: '9', title: 'Track' });
    fireEvent.click(screen.getByRole('button', { name: /Add to playlist/i }));
    expect(await screen.findByText('No playlists yet')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /\+ New playlist/i }));
    const input = screen.getByPlaceholderText(/Playlist name/i);
    fireEvent.change(input, { target: { value: 'Broken' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(await screen.findByText('Create failed')).toBeInTheDocument();
    fireEvent.change(input, { target: { value: '' } });
    expect(screen.queryByText('Create failed')).not.toBeInTheDocument();
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByPlaceholderText(/Playlist name/i)).not.toBeInTheDocument();

    apiMock.playlists.list.mockResolvedValue([{ id: '1', name: 'Road Trip' }]);
    await open({ kind: 'track', trackId: '9', title: 'Track' });
    fireEvent.click(screen.getByRole('button', { name: /Add to playlist/i }));
    const playlist = await screen.findByRole('button', { name: /Road Trip/i });
    fireEvent.click(playlist);
    await waitFor(() => expect(playlist).not.toHaveTextContent('…'));
  });

  it('configures and resets album crossfade overrides, including API failures', async () => {
    render(<ContextMenuRoot />);
    await open({ kind: 'album', albumId: '8', title: 'Album' });
    fireEvent.mouseEnter(screen.getByRole('button', { name: /Set crossfade/i }));
    expect(await screen.findByText('Crossfade override')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Off' }));
    fireEvent.click(screen.getByRole('button', { name: 'Zero-gap' }));
    fireEvent.click(screen.getByRole('button', { name: 'Crossfade' }));
    fireEvent.change(screen.getByRole('slider'), { target: { value: '7' } });
    await waitFor(() => expect(apiMock.crossfade.upsertOverride).toHaveBeenLastCalledWith({
      entity_type: 'album', entity_id: '8', mode: 'crossfade', duration: 7,
    }));

    fireEvent.click(screen.getByRole('button', { name: /Reset to default/i }));
    await waitFor(() => expect(apiMock.crossfade.removeOverride).toHaveBeenCalledWith('album', '8'));

    apiMock.crossfade.config.mockRejectedValueOnce(new Error('load failed'));
    await open({ kind: 'album', albumId: '9', title: 'Other Album' });
    fireEvent.click(screen.getByRole('button', { name: /Set crossfade/i }));
    expect(await screen.findByRole('button', { name: 'Off' })).toBeInTheDocument();
    expect(screen.getByText('Using global default')).toBeInTheDocument();
  });

  it('opens via public helpers and kebab button, then dismisses outside or with Escape', async () => {
    const onPlay = vi.fn();
    render(
      <>
        <KebabButton
          target={{ kind: 'track', trackId: '1', title: 'Kebab Track' }}
          callbacks={{ onPlay }}
          visible={false}
          style={{ color: 'red' }}
        />
        <ContextMenuRoot />
      </>,
    );
    const kebab = screen.getByRole('button', { name: 'More actions' });
    fireEvent.mouseDown(kebab);
    fireEvent.click(kebab);
    expect(await screen.findByText('Kebab Track')).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByText('Kebab Track')).not.toBeInTheDocument();

    openKebabMenu(new DOMRect(10, 10, 20, 20), { kind: 'artist', artistId: '2', name: 'Helper Artist' }, {});
    expect(await screen.findByText('Helper Artist')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByText('Helper Artist')).not.toBeInTheDocument());

    const event = {
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
      clientX: 2,
      clientY: 3,
    } as unknown as React.MouseEvent;
    openContextMenu(event, { kind: 'playlist', playlistId: 'p', name: 'Legacy' }, {});
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
    expect(await screen.findByText('Legacy')).toBeInTheDocument();
  });
});


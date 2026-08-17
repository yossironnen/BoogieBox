/**
 * Tests Context Menu.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ContextMenuRoot,
  KebabButton,
  openContextMenu,
  openKebabMenu,
  TRACK_INFO_EVENT,
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

  it('fires the Info entry as a global event and closes the menu', async () => {
    const onTrackInfo = vi.fn();
    window.addEventListener(TRACK_INFO_EVENT, onTrackInfo);
    render(<ContextMenuRoot />);

    await open({ kind: 'track', trackId: '9', title: 'Track' });
    fireEvent.click(screen.getByRole('button', { name: 'Info' }));

    expect(onTrackInfo).toHaveBeenCalledTimes(1);
    expect((onTrackInfo.mock.calls[0][0] as CustomEvent).detail).toEqual({ trackId: '9' });
    expect(screen.queryByText('Track')).not.toBeInTheDocument();

    window.removeEventListener(TRACK_INFO_EVENT, onTrackInfo);
  });

  it('renders extensible library actions and prevents disabled actions', async () => {
    const onRadio = vi.fn();
    const onScan = vi.fn();
    const onDeepAnalysis = vi.fn();
    render(<ContextMenuRoot />);

    await open(
      { kind: 'library', libraryId: 'library-1', name: 'Main Music' },
      {
        actions: [
          { id: 'radio', label: 'Play library radio', icon: 'play', onSelect: onRadio },
          { id: 'scan', label: 'Scan library', icon: 'scan', disabled: true, dividerBefore: true, onSelect: onScan },
          { id: 'deep', label: 'Run deep analysis', icon: 'deep-analysis', disabled: true, onSelect: onDeepAnalysis },
        ],
      },
    );

    expect(screen.getByText('Library')).toBeInTheDocument();
    expect(screen.getByText('Main Music')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Play library radio' }));
    expect(onRadio).toHaveBeenCalledTimes(1);

    await open(
      { kind: 'library', libraryId: 'library-1', name: 'Main Music' },
      {
        actions: [
          { id: 'scan', label: 'Scan library', icon: 'scan', disabled: true, onSelect: onScan },
          { id: 'deep', label: 'Run deep analysis', icon: 'deep-analysis', disabled: true, onSelect: onDeepAnalysis },
        ],
      },
    );
    expect(screen.getByRole('button', { name: 'Scan library' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Run deep analysis' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Scan library' }));
    expect(onScan).not.toHaveBeenCalled();
    expect(onDeepAnalysis).not.toHaveBeenCalled();
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

  it('opens via public helpers and toggles the same kebab closed', async () => {
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

    fireEvent.mouseDown(kebab);
    fireEvent.click(kebab);
    expect(screen.queryByText('Kebab Track')).not.toBeInTheDocument();

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

  describe('viewport clamping', () => {
    const MENU_HEIGHT = 300;
    const VIEWPORT_HEIGHT = 600;
    const MARGIN = 8;
    let originalHeight: number;

    beforeEach(() => {
      originalHeight = window.innerHeight;
      Object.defineProperty(window, 'innerHeight', { value: VIEWPORT_HEIGHT, configurable: true });
      // jsdom has no layout, so the menu reports zero size without this.
      vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
        width: 220, height: MENU_HEIGHT, top: 0, left: 0,
        right: 220, bottom: MENU_HEIGHT, x: 0, y: 0, toJSON: () => ({}),
      } as DOMRect);
    });

    afterEach(() => {
      Object.defineProperty(window, 'innerHeight', { value: originalHeight, configurable: true });
    });

    /** The rendered menu element (the portal root holding the target label). */
    const menuEl = () => screen.getByText('Track').parentElement!.parentElement!;

    it('flips a bottom-edge menu above its anchor instead of letting it overflow', async () => {
      render(<ContextMenuRoot />);
      // Anchor near the bottom: a downward menu would end at 880, well past 600.
      await open({ kind: 'track', trackId: 't1', title: 'Bottom Track' }, {});

      const menu = menuEl();
      const top = parseFloat(menu.style.top);
      expect(top + MENU_HEIGHT).toBeLessThanOrEqual(VIEWPORT_HEIGHT - MARGIN);
      expect(top).toBeGreaterThanOrEqual(MARGIN);
    });

    it('pins to the bottom edge when the menu cannot fit above the anchor either', async () => {
      render(<ContextMenuRoot />);
      await act(async () => {
        window.dispatchEvent(new CustomEvent('boogiebox:contextmenu', {
          // flipY too small to fit a 300px menu above it, so flipping is invalid.
          detail: { x: 10, y: 580, flipY: 120, target: { kind: 'track', trackId: 't2', title: 'Tight' }, callbacks: {} },
        }));
      });

      const top = parseFloat(menuEl().style.top);
      expect(top).toBe(VIEWPORT_HEIGHT - MARGIN - MENU_HEIGHT);
      expect(top + MENU_HEIGHT).toBeLessThanOrEqual(VIEWPORT_HEIGHT - MARGIN);
    });

    it('stays put when a submenu makes the menu taller', async () => {
      // Growth must be absorbed by the height cap, never by moving the menu:
      // sliding it out from under the pointer mid-click is worse than scrolling.
      let currentHeight = MENU_HEIGHT;
      vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(() => ({
        width: 220, height: currentHeight, top: 0, left: 0,
        right: 220, bottom: currentHeight, x: 0, y: 0, toJSON: () => ({}),
      }) as DOMRect);

      render(<ContextMenuRoot />);
      await open({ kind: 'track', trackId: 't4', title: 'Grower' }, {});
      const topWhenOpened = menuEl().style.top;

      currentHeight = MENU_HEIGHT + 200;   // submenu expands the menu
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: /Add to playlist/i }));
      });

      expect(menuEl().style.top).toBe(topWhenOpened);
    });

    it('caps menu height to the viewport so an oversized menu scrolls', async () => {
      Object.defineProperty(window, 'innerHeight', { value: 200, configurable: true });
      render(<ContextMenuRoot />);
      await open({ kind: 'track', trackId: 't3', title: 'Short viewport' }, {});

      expect(menuEl().style.maxHeight).toBe(`${200 - MARGIN * 2}px`);
    });
  });
});


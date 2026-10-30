/**
 * Tests smart playlists in the mobile Playlists view: list cards, the rule-picked detail, refresh failures and
 * answers that arrive after leaving the detail.
 */

// @vitest-environment jsdom

import React, { useCallback, useState } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Playlist, PlaylistSnapshot, PlaylistTrack } from '../../types';
import type { MobilePlaylistSelection } from '../mobileShell';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      list: vi.fn(),
      get: vi.fn(),
      tracks: vi.fn(),
      snapshot: vi.fn(),
      summary: vi.fn(),
      refresh: vi.fn(),
      exportM3uUrl: vi.fn(() => '#export'),
    },
    boogiemix: { listAllOutputs: vi.fn() },
    search: vi.fn(),
    albumArtUrl: vi.fn(() => '/art'),
  },
}));

vi.mock('../../api', () => ({ api: apiMock }));

// The editor has its own tests; here it only has to open with the right playlist and report a save.
vi.mock('../../components/SmartPlaylistEditor', () => ({
  SmartIcon: () => <svg aria-hidden="true" />,
  default: ({ playlist, variant, onSaved, onClose }: {
    playlist?: Playlist | null; variant?: string; onSaved: (p: Playlist) => void; onClose: () => void;
  }) => (
    <div role="dialog" aria-label={playlist ? 'Edit rules' : 'New smart playlist'} data-variant={variant}>
      <button type="button" onClick={() => onSaved(playlist ?? { ...smart, id: 's2', name: 'Fresh smart' })}>Save stub</button>
      <button type="button" onClick={onClose}>Close stub</button>
    </div>
  ),
}));

import MobilePlaylistsView from './MobilePlaylistsView';

const smart: Playlist = {
  id: 's1', name: 'Forgotten favourites', description: null, created_at: null, updated_at: null,
  track_count: 1, total_duration: 300, kind: 'smart',
};
const manual: Playlist = {
  id: 'm1', name: 'Road Trip', description: null, created_at: null, updated_at: null,
  track_count: 2, total_duration: 600, kind: 'manual',
};
const track = {
  id: 't1', playlist_track_id: 'pt1', position: 1, title: 'Teardrop', artist: 'Massive Attack', album: 'Mezzanine',
  album_id: null, file_name: 'a.mp3', duration: 300,
} as unknown as PlaylistTrack;

function snapshot(over: Partial<PlaylistSnapshot> = {}): PlaylistSnapshot {
  return { playlist: smart, tracks: [track], membershipRevision: 1, refreshError: null, ...over };
}

function Harness({ initial, spy }: { initial: MobilePlaylistSelection; spy?: (s: MobilePlaylistSelection) => void }) {
  const [selection, setSelection] = useState(initial);
  const onChange = useCallback((next: MobilePlaylistSelection) => { spy?.(next); setSelection(next); }, [spy]);
  return (
    <MobilePlaylistsView initialPlaylistId={null} selection={selection} onSelectionChange={onChange}
      onPlayTrack={() => {}} onPlayMixTrack={() => {}} onAddToQueue={() => {}} />
  );
}

describe('MobilePlaylistsView smart playlists', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.list.mockResolvedValue([smart, manual]);
    apiMock.playlists.snapshot.mockResolvedValue(snapshot());
    apiMock.playlists.summary.mockResolvedValue(smart);
    apiMock.boogiemix.listAllOutputs.mockResolvedValue([]);
    apiMock.search.mockResolvedValue({ tracks: [] });
  });

  afterEach(() => cleanup());

  it('marks smart playlists in the list and offers a new one beside Import', async () => {
    render(<Harness initial={{ playlist: null, tracks: [] }} />);
    const card = await screen.findByRole('button', { name: 'Forgotten favourites — Smart playlist · 1 tracks · 5m' });
    expect(within(card).getByTestId('smart-badge')).toBeInTheDocument();
    expect(within(card).getByText('Smart · 1 tracks · 5m')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Road Trip/ })).not.toContainElement(screen.getByTestId('smart-badge'));

    fireEvent.click(screen.getByRole('button', { name: 'New smart playlist' }));
    const editor = screen.getByRole('dialog', { name: 'New smart playlist' });
    expect(editor).toHaveAttribute('data-variant', 'mobile');

    apiMock.playlists.snapshot.mockResolvedValue(snapshot({ playlist: { ...smart, id: 's2', name: 'Fresh smart' } }));
    fireEvent.click(within(editor).getByRole('button', { name: 'Save stub' }));
    expect(await screen.findByRole('heading', { name: 'Fresh smart' })).toBeInTheDocument();
    expect(apiMock.playlists.snapshot).toHaveBeenCalledWith('s2');
  });

  it('updates smart cards on screen from their summaries', async () => {
    apiMock.playlists.summary.mockResolvedValue({ ...smart, track_count: 4, total_duration: 1200 });
    render(<Harness initial={{ playlist: null, tracks: [] }} />);
    const card = await screen.findByRole('button', { name: 'Forgotten favourites — Smart playlist · 4 tracks · 20m' });
    expect(within(card).getByText('Smart · 4 tracks · 20m')).toBeInTheDocument();
    expect(apiMock.playlists.summary).toHaveBeenCalledWith('s1');
  });

  it('shows rule-picked tracks without add, delete or reorder', async () => {
    render(<Harness initial={{ playlist: smart, tracks: [] }} />);
    expect(await screen.findByRole('button', { name: 'Play Teardrop' })).toBeInTheDocument();
    expect(apiMock.playlists.snapshot).toHaveBeenCalledWith('s1');
    expect(apiMock.playlists.tracks).not.toHaveBeenCalled();

    expect(screen.getByText('Smart playlist')).toBeInTheDocument();
    expect(screen.getByText('Picked by rules. Edit the rules to change the tracks.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reorder Teardrop' })).toBeNull();
    expect(screen.queryByText('Delete')).toBeNull();

    const row = screen.getByRole('button', { name: 'Play Teardrop' });
    fireEvent.pointerDown(row, { button: 0, pointerId: 1, clientX: 180, clientY: 20 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 20, clientY: 20 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 20, clientY: 20 });
    expect(screen.queryByRole('alertdialog')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Rules' }));
    fireEvent.click(within(screen.getByRole('dialog', { name: 'Edit rules' })).getByRole('button', { name: 'Save stub' }));
    await waitFor(() => expect(apiMock.playlists.snapshot).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole('dialog', { name: 'Edit rules' })).toBeNull();
  });

  it('offers to edit the rules when nothing matches', async () => {
    apiMock.playlists.snapshot.mockResolvedValue(snapshot({ tracks: [], playlist: { ...smart, track_count: 0, total_duration: 0 } }));
    render(<Harness initial={{ playlist: smart, tracks: [] }} />);
    expect(await screen.findByText('No tracks match these rules.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Edit rules' }));
    expect(screen.getByRole('dialog', { name: 'Edit rules' })).toBeInTheDocument();
  });

  it('keeps the last result when a refresh fails and retries on request', async () => {
    apiMock.playlists.snapshot.mockResolvedValue(snapshot({ refreshError: 'timed_out' }));
    apiMock.playlists.refresh.mockResolvedValue(snapshot());
    render(<Harness initial={{ playlist: smart, tracks: [] }} />);
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Couldn’t update this playlist. Showing the last result.');
    expect(screen.getByRole('button', { name: 'Play Teardrop' })).toBeInTheDocument();

    fireEvent.click(within(alert).getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(apiMock.playlists.refresh).toHaveBeenCalledWith('s1'));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  });

  it('shows Updating… while loading and ignores an answer that arrives after Back', async () => {
    let resolve: (value: PlaylistSnapshot) => void = () => {};
    apiMock.playlists.snapshot.mockReturnValue(new Promise<PlaylistSnapshot>(r => { resolve = r; }));
    const spy = vi.fn();
    render(<Harness initial={{ playlist: smart, tracks: [] }} spy={spy} />);
    expect(await screen.findByText('Updating…')).toBeInTheDocument();
    expect(screen.queryByText('No tracks match these rules.')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(await screen.findByRole('heading', { name: 'Playlists' })).toBeInTheDocument();
    await act(async () => { resolve(snapshot()); });
    expect(spy).toHaveBeenLastCalledWith({ playlist: null, tracks: [] });
    expect(screen.getByRole('heading', { name: 'Playlists' })).toBeInTheDocument();
  });

  it('opens a smart playlist of unknown kind as a snapshot', async () => {
    apiMock.playlists.get.mockResolvedValue(smart);
    apiMock.playlists.tracks.mockResolvedValue([]);
    render(<Harness initial={{ playlist: { ...smart, kind: undefined }, tracks: [] }} />);
    expect(await screen.findByRole('button', { name: 'Play Teardrop' })).toBeInTheDocument();
    expect(apiMock.playlists.snapshot).toHaveBeenCalledWith('s1');
    expect(screen.getByRole('button', { name: 'Rules' })).toBeInTheDocument();
  });
});

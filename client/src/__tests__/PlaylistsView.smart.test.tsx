/**
 * Tests smart playlists in the desktop Playlists view: cards, detail, refresh failure, options and copies.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    playlists: {
      list: vi.fn(),
      create: vi.fn(),
      tracks: vi.fn(),
      snapshot: vi.fn(),
      summary: vi.fn(),
      refresh: vi.fn(),
      smartDefinition: vi.fn(),
      smartPreview: vi.fn(),
      saveAsOrdinary: vi.fn(),
      update: vi.fn(),
      remove: vi.fn(),
    },
    libraries: { list: vi.fn() },
    genres: vi.fn(),
    search: vi.fn(),
    albumArtUrl: vi.fn(),
    album: vi.fn(),
    artist: vi.fn(),
    crossfade: { config: vi.fn(), upsertOverride: vi.fn(), removeOverride: vi.fn() },
    boogiemix: {
      deepAnalysisStatus: vi.fn(),
      listOutputs: vi.fn(),
      latestJobForPlaylist: vi.fn(),
      getJob: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({ api: apiMock, SmartPlaylistApiError: class extends Error {} }));

import PlaylistsView from '../components/PlaylistsView';

const smart = {
  id: 's1', name: 'Late night', description: '', created_at: null, updated_at: null,
  track_count: 2, total_duration: 600, art_album_ids: [], kind: 'smart' as const,
};
const manual = {
  id: 'm1', name: 'Road Trip', description: '', created_at: null, updated_at: null,
  track_count: 1, total_duration: 120, art_album_ids: [], kind: 'manual' as const,
};
const tracks = [
  { id: 't1', playlist_track_id: 'pt1', title: 'Teardrop', artist: 'Massive Attack', album: 'Mezzanine', album_id: null, duration: 329, file_name: 'a.mp3', position: 0 },
  { id: 't2', playlist_track_id: 'pt2', title: 'Glory Box', artist: 'Portishead', album: 'Dummy', album_id: null, duration: 301, file_name: 'b.mp3', position: 1 },
];

function renderView(playTrack = vi.fn()) {
  return render(<PlaylistsView playTrack={playTrack} addToQueue={vi.fn()} />);
}

async function openSmart(playTrack = vi.fn()) {
  renderView(playTrack);
  fireEvent.click(await screen.findByRole('button', { name: 'Late night — Smart playlist · 2 tracks · 10m' }));
  return document.querySelector('[data-ui-region="playlist-detail"]') as HTMLElement;
}

describe('PlaylistsView smart playlists', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
    apiMock.playlists.list.mockResolvedValue([smart, manual]);
    apiMock.playlists.snapshot.mockResolvedValue({ playlist: smart, tracks, membershipRevision: 1, refreshError: null });
    apiMock.playlists.summary.mockResolvedValue(smart);
    apiMock.playlists.smartDefinition.mockResolvedValue({
      definition: { schemaVersion: 1, root: { mode: 'all', children: [{ field: 'artist', operator: 'is', value: 'Air' }] } },
      schemaVersion: 1, editRevision: 1, membershipRevision: 1, evaluatedAt: null, supported: true, missingLibraryIds: [],
    });
    apiMock.playlists.smartPreview.mockResolvedValue({ trackCount: 0, totalDuration: 0, tracks: [], missingLibraryIds: [] });
    apiMock.libraries.list.mockResolvedValue([]);
    apiMock.genres.mockResolvedValue([]);
    apiMock.search.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.albumArtUrl.mockReturnValue('/art');
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    apiMock.boogiemix.deepAnalysisStatus.mockResolvedValue(null);
    apiMock.boogiemix.listOutputs.mockResolvedValue([]);
    apiMock.boogiemix.latestJobForPlaylist.mockResolvedValue(null);
  });

  afterEach(() => cleanup());

  it('marks smart cards and opens the editor from the header', async () => {
    renderView();
    const card = await screen.findByRole('button', { name: 'Late night — Smart playlist · 2 tracks · 10m' });
    expect(card).toHaveTextContent('Smart · 2 tracks · 10m');
    expect(within(card).getByTestId('smart-badge')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Road Trip — 1 track · 2m' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'New smart playlist' }));
    expect(screen.getByRole('dialog', { name: 'New smart playlist' })).toBeInTheDocument();
  });

  it('updates smart cards on screen from their summaries', async () => {
    apiMock.playlists.summary.mockResolvedValue({ ...smart, track_count: 5, total_duration: 1200 });
    renderView();
    expect(await screen.findByRole('button', { name: 'Late night — Smart playlist · 5 tracks · 20m' })).toBeInTheDocument();
    expect(apiMock.playlists.summary).toHaveBeenCalledTimes(1);
    expect(apiMock.playlists.summary).toHaveBeenCalledWith('s1');
  });

  it('shows rule-picked tracks without manual editing', async () => {
    const detail = await openSmart();
    expect(await within(detail).findByText('Teardrop')).toBeVisible();
    expect(apiMock.playlists.snapshot).toHaveBeenCalledWith('s1');
    expect(apiMock.playlists.tracks).not.toHaveBeenCalled();
    expect(within(detail).getByText('Smart playlist')).toHaveAttribute(
      'title', 'Smart playlist\nIts tracks are picked by rules and update on their own',
    );
    expect(within(detail).queryByRole('button', { name: 'Add tracks' })).toBeNull();
    expect(within(detail).queryByRole('button', { name: /from playlist$/ })).toBeNull();
    expect(within(detail).queryByTitle('Drag to reorder')).toBeNull();

    fireEvent.click(within(detail).getByRole('button', { name: 'Edit rules' }));
    expect(await screen.findByRole('dialog', { name: 'Edit rules' })).toBeInTheDocument();
  });

  it('keeps the last result when a refresh fails and retries', async () => {
    apiMock.playlists.snapshot.mockResolvedValue({ playlist: smart, tracks, membershipRevision: 1, refreshError: 'timed_out' });
    apiMock.playlists.refresh.mockResolvedValue({ playlist: smart, tracks: tracks.slice(0, 1), membershipRevision: 2, refreshError: null });
    const detail = await openSmart();
    const alert = await within(detail).findByRole('alert');
    expect(alert).toHaveTextContent('Couldn’t update this playlist. Showing the last result.');
    expect(within(detail).getByText('Glory Box')).toBeVisible();
    fireEvent.click(within(alert).getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(within(detail).queryByText('Glory Box')).toBeNull());
    expect(within(detail).queryByRole('alert')).toBeNull();
  });

  it('leaves the playing queue alone when a refresh changes the tracks', async () => {
    const playTrack = vi.fn();
    apiMock.playlists.snapshot.mockResolvedValue({ playlist: smart, tracks, membershipRevision: 1, refreshError: 'timed_out' });
    apiMock.playlists.refresh.mockResolvedValue({ playlist: smart, tracks: [tracks[1]], membershipRevision: 2, refreshError: null });
    const detail = await openSmart(playTrack);
    await within(detail).findByText('Teardrop');
    fireEvent.click(within(detail).getByRole('button', { name: 'Play All' }));
    expect(playTrack).toHaveBeenCalledTimes(1);
    const queued = playTrack.mock.calls[0][1];
    const before = queued.map((t: { id: string }) => t.id);
    expect(before).toEqual(['t1', 't2']);

    fireEvent.click(within(await within(detail).findByRole('alert')).getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(within(detail).queryByText('Teardrop')).toBeNull());
    expect(playTrack).toHaveBeenCalledTimes(1);
    expect(queued.map((t: { id: string }) => t.id)).toEqual(before);
  });

  it('re-checks an open smart playlist each minute without "Updating…"', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const detail = await openSmart();
      await within(detail).findByText('Teardrop');
      apiMock.playlists.snapshot.mockResolvedValue({ playlist: smart, tracks: [tracks[1]], membershipRevision: 2, refreshError: null });
      await vi.advanceTimersByTimeAsync(60_000);
      expect(apiMock.playlists.snapshot).toHaveBeenCalledTimes(2);
      await waitFor(() => expect(within(detail).queryByText('Teardrop')).toBeNull());
      expect(within(detail).getByText('Glory Box')).toBeInTheDocument();
      expect(within(detail).queryByText('Updating…')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('offers the rules when nothing matches', async () => {
    apiMock.playlists.snapshot.mockResolvedValue({ playlist: smart, tracks: [], membershipRevision: 1, refreshError: null });
    const detail = await openSmart();
    const empty = await within(detail).findByText('No tracks match these rules.');
    fireEvent.click(within(empty.parentElement as HTMLElement).getByRole('button', { name: /Edit rules/ }));
    expect(await screen.findByRole('dialog', { name: 'Edit rules' })).toBeInTheDocument();
  });

  it('saves a copy as an ordinary playlist', async () => {
    const copy = { ...manual, id: 'c1', name: 'Late night (copy)', track_count: 2 };
    apiMock.playlists.saveAsOrdinary.mockResolvedValue({ playlist: copy, added: 2 });
    apiMock.playlists.tracks.mockResolvedValue(tracks);
    const detail = await openSmart();
    await within(detail).findByText('Teardrop');
    fireEvent.click(within(detail).getByRole('button', { name: 'More actions' }));
    const options = screen.getByRole('dialog', { name: 'Late night' });
    expect(within(options).getByRole('button', { name: /Edit rules/ })).toBeInTheDocument();
    fireEvent.click(within(options).getByRole('button', { name: /Save as ordinary playlist/ }));

    const dialog = screen.getByRole('dialog', { name: 'Save as ordinary playlist' });
    expect(dialog).toHaveTextContent('Saves these 2 tracks as a playlist you can edit by hand. It won’t change when the rules match other tracks.');
    expect(within(dialog).getByLabelText('Playlist name')).toHaveValue('Late night (copy)');
    apiMock.playlists.list.mockResolvedValue([smart, manual, copy]);
    fireEvent.click(within(dialog).getByRole('button', { name: /Save copy/ }));
    await waitFor(() => expect(apiMock.playlists.saveAsOrdinary).toHaveBeenCalledWith('s1', 'Late night (copy)'));
    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('c1'));
  });
});

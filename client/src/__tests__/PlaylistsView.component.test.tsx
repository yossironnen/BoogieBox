/**
 * Tests Playlists View.Component.Test behavior for BoogieBox regressions.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
    album: vi.fn(),
    artist: vi.fn(),
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
      latestJobForPlaylist: vi.fn(),
      cancelJob: vi.fn(),
      outputDownloadUrl: vi.fn(),
      playUrl: vi.fn(),
      queuePlaylistDeepAnalysis: vi.fn(),
      playlistDeepAnalysisProgress: vi.fn(),
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


function openOptions() {
  const detail = document.querySelector('[data-ui-region="playlist-detail"]') as HTMLElement;
  fireEvent.click(within(detail).getByRole('button', { name: 'More actions' }));
}

function openMix() {
  fireEvent.click(screen.getByRole('button', { name: 'BoogieMix (Experimental)' }));
}

describe('PlaylistsView integration flows', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // jsdom cannot provide the browser top layer; browser QA covers native focus containment.
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', ''); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute('open'); };
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
    apiMock.boogiemix.latestJobForPlaylist.mockResolvedValue(null);
    apiMock.boogiemix.outputDownloadUrl.mockImplementation((id: string) => `/api/boogiemix/outputs/${id}/file`);
    apiMock.boogiemix.playUrl.mockImplementation((id: string) => `/api/boogiemix/outputs/${id}/play`);
    apiMock.boogiemix.createJob.mockResolvedValue({ jobId: 'mix-1' });
    apiMock.boogiemix.getJob.mockResolvedValue({
      id: 'mix-1', status: 'pending', progress_percent: 10, current_step: 'Planning',
      mix_quality: 'standard', used_deep_analysis: false, last_message: 'Working',
    });
    apiMock.boogiemix.cancelJob.mockResolvedValue({ ok: true });
    apiMock.boogiemix.queuePlaylistDeepAnalysis.mockResolvedValue({ queued: 2 });
    apiMock.boogiemix.playlistDeepAnalysisProgress.mockResolvedValue({
      pending: 0, running: 0, done: 2, failed: 0, skipped: 0, total: 2,
    });
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

  it('shares header artwork with sidebar rows without extra track fetches', async () => {
    apiMock.playlists.list.mockResolvedValue([playlist, { ...playlist, id: '2', name: 'Focus', art_album_ids: ['501'] }]);
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    const sources = (node: HTMLElement) => Array.from(node.querySelectorAll('img')).map(img => img.getAttribute('src'));
    const row = screen.getByRole('button', { name: /Road Trip.*2 tracks/ });
    expect(sources(row)).toEqual(sources(screen.getByLabelText('Road Trip artwork')));
    expect(sources(row)).toEqual(['/api/albums/401/art?size=300', '/api/albums/402/art?size=300']);
    expect(sources(screen.getByRole('button', { name: /Focus.*2 tracks/ }))).toEqual(['/api/albums/501/art?size=300']);
    expect(apiMock.playlists.tracks).toHaveBeenCalledTimes(1);
    expect(apiMock.playlists.list).toHaveBeenCalledTimes(1);
  });

  it('opens another playlist options without switching selection and deletes only that playlist', async () => {
    apiMock.playlists.list.mockResolvedValue([playlist, { ...playlist, id: '2', name: 'Focus' }]);
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    const row = screen.getByRole('button', { name: /Focus.*2 tracks/ }).parentElement!;
    fireEvent.click(within(row).getByRole('button', { name: 'More actions' }));
    expect(screen.getByRole('dialog', { name: 'Focus' })).toBeInTheDocument();
    await waitFor(() => expect(apiMock.crossfade.config).toHaveBeenCalledWith('playlist', '2'));
    expect(screen.getByRole('button', { name: /Road Trip.*2 tracks/ })).toHaveAttribute('aria-current', 'true');
    expect(apiMock.playlists.tracks).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTitle('Delete playlist'));
    expect(apiMock.playlists.remove).not.toHaveBeenCalled();
    apiMock.playlists.list.mockResolvedValue([playlist]);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(apiMock.playlists.remove).toHaveBeenCalledWith('2'));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByText('Alpha One')).toBeInTheDocument();
  });

  it('supports keyboard navigation and escape or outside dismissal in the playback menu', async () => {
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    const arrow = screen.getByRole('button', { name: 'Queue All' });
    fireEvent.click(arrow);
    expect(screen.getByRole('menuitem', { name: 'Play All' })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowDown' });
    expect(screen.getByRole('menuitem', { name: 'Queue All' })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(arrow).toHaveFocus();
    fireEvent.click(arrow);
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('disables empty playlist actions and renders shared fallback artwork', async () => {
    apiMock.playlists.list.mockResolvedValue([{ ...playlist, track_count: 0, art_album_ids: [] }]);
    apiMock.playlists.tracks.mockResolvedValue([]);
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByLabelText('Road Trip artwork');
    expect(screen.getByRole('button', { name: 'Play All' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Queue All' })).toBeDisabled();
    expect(screen.getByLabelText('Road Trip artwork').firstElementChild?.children).toHaveLength(4);
    openMix();
    expect(screen.getByTitle('BoogieMix is experimental')).toBeDisabled();
    expect(screen.getByTitle(/Run Demucs/)).toBeDisabled();
  });

  it('restores focus on popup dismissal and preserves mix configuration', async () => {
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    const trigger = screen.getByRole('button', { name: 'BoogieMix (Experimental)' });
    trigger.focus();
    openMix();
    const dismiss = screen.getByRole('button', { name: 'Dismiss' });
    dismiss.focus();
    fireEvent.keyDown(dismiss, { key: 'Tab', shiftKey: true });
    expect(screen.getByTitle(/Run Demucs/)).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: 'Tab' });
    expect(dismiss).toHaveFocus();
    fireEvent.change(screen.getByTitle('BoogieMix style'), { target: { value: 'safe_mix' } });
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    openMix();
    expect(screen.getByTitle('BoogieMix style')).toHaveValue('safe_mix');
    fireEvent.click(screen.getByRole('dialog'));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(apiMock.boogiemix.createJob).not.toHaveBeenCalled();
  });

  it('leaves remember position unchanged when saving fails', async () => {
    apiMock.playlists.update.mockRejectedValueOnce(new Error('Could not update playlist'));
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    openOptions();
    fireEvent.click(screen.getByRole('switch', { name: 'Remember track position' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not update playlist');
    expect(screen.getByRole('switch', { name: 'Remember track position' })).toHaveAttribute('aria-checked', 'false');
  });

  it('loads a playlist, plays/queues tracks, reorders, and removes a track', async () => {
    const playTrack = vi.fn();
    const addToQueue = vi.fn();

    const { container } = render(<PlaylistsView playTrack={playTrack} addToQueue={addToQueue} initialPlaylistId={'1'} />);

    await waitFor(() => expect(apiMock.playlists.list).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.playlists.tracks).toHaveBeenCalledWith('1'));
    expect(container.firstElementChild).toHaveAttribute('data-ui-design', 'hybrid');
    expect(container.querySelector('[data-ui-region="playlist-sidebar"]')).toBeInTheDocument();
    expect(container.querySelector('[data-ui-region="playlist-detail"]')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Road Trip.*2 tracks/i })).toHaveAttribute('aria-current', 'true');
    expect(screen.getAllByText('Road Trip').length).toBeGreaterThan(0);
    expect(screen.getByText('Alpha One')).toBeInTheDocument();
    expect(screen.getByText('Alpha Two')).toBeInTheDocument();
    // Each track row shows its album artwork thumbnail.
    const rowAArt = screen.getByText('Alpha One').closest('[draggable="true"]')!.querySelector('img')!;
    expect(rowAArt).toHaveAttribute('src', '/api/albums/301/art?size=300');
    const rowBArt = screen.getByText('Alpha Two').closest('[draggable="true"]')!.querySelector('img')!;
    expect(rowBArt).toHaveAttribute('src', '/api/albums/302/art?size=300');

    fireEvent.click(screen.getByRole('button', { name: /Play All/i }));
    expect(playTrack).toHaveBeenCalledWith(expect.objectContaining({ id: '101' }), expect.arrayContaining([expect.objectContaining({ id: '101' }), expect.objectContaining({ id: '102' })]), expect.objectContaining({ type: 'playlist', id: '1' }));

    fireEvent.click(screen.getByRole('button', { name: /Queue All/i }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Queue All' }));
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

  it('falls back to a note icon in a track row when the track has no album_id', async () => {
    apiMock.playlists.tracks.mockResolvedValue([{ ...trackA, album_id: null }]);
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" />);

    await screen.findByText('Alpha One');
    const row = screen.getByText('Alpha One').closest('[draggable="true"]')!;
    expect(row.querySelector('img')).not.toBeInTheDocument();
    expect(row.querySelector('svg')).toBeInTheDocument();
  });

  it('links the album name in a track row to that album via a resolved lookup', async () => {
    const onOpenAlbum = vi.fn();
    const album = { id: '301', title: 'Album A', artist: 'Artist A', album_artist: 'Artist A', year: 2020, genre: null, track_count: 5 };
    apiMock.album.mockResolvedValue(album);
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" onOpenAlbum={onOpenAlbum} />);

    await screen.findByText('Alpha One');
    const row = screen.getByText('Alpha One').closest('[draggable="true"]') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: 'Album A' }));

    await waitFor(() => expect(apiMock.album).toHaveBeenCalledWith('301'));
    await waitFor(() => expect(onOpenAlbum).toHaveBeenCalledWith(album));
  });

  it('makes the artist name itself the clickable link when it matches the album artist, instead of a redundant second link', async () => {
    const onOpenArtist = vi.fn();
    apiMock.playlists.tracks.mockResolvedValue([
      { ...trackA, album_artist_id: '901', album_artist_name: 'Artist A' },
    ]);
    const artist = { id: '901', name: 'Artist A' };
    apiMock.artist.mockResolvedValue(artist);
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" onOpenArtist={onOpenArtist} />);

    await screen.findByText('Alpha One');
    const row = screen.getByText('Alpha One').closest('[draggable="true"]') as HTMLElement;
    // Exactly one "Artist A" control — the artist name itself, not a separate redundant link.
    const artistLinks = within(row).getAllByRole('button', { name: 'Artist A' });
    expect(artistLinks).toHaveLength(1);
    fireEvent.click(artistLinks[0]);

    await waitFor(() => expect(apiMock.artist).toHaveBeenCalledWith('901'));
    await waitFor(() => expect(onOpenArtist).toHaveBeenCalledWith(artist));
  });

  it('links a compilation track\'s distinct album artist to that artist via a resolved lookup', async () => {
    const onOpenArtist = vi.fn();
    apiMock.playlists.tracks.mockResolvedValue([
      { ...trackA, artist: 'Featured Performer', album_artist_id: '901', album_artist_name: 'Various Artists' },
    ]);
    const artist = { id: '901', name: 'Various Artists' };
    apiMock.artist.mockResolvedValue(artist);
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" onOpenArtist={onOpenArtist} />);

    await screen.findByText('Alpha One');
    const row = screen.getByText('Alpha One').closest('[draggable="true"]') as HTMLElement;
    expect(row).toHaveTextContent('Featured Performer');
    fireEvent.click(within(row).getByRole('button', { name: 'Various Artists' }));

    await waitFor(() => expect(apiMock.artist).toHaveBeenCalledWith('901'));
    await waitFor(() => expect(onOpenArtist).toHaveBeenCalledWith(artist));
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
    openMix();
    fireEvent.change(screen.getByTitle('BoogieMix quality'), { target: { value: 'high_quality' } });

    expect(within(screen.getByRole('dialog')).getByText(/High Quality needs deep analysis\. Missing: torch, demucs/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    fireEvent.mouseEnter(screen.getByTestId('boogiemix-status'));
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

    openOptions();
    fireEvent.click(screen.getByTitle('Rename'));
    expect(screen.getByRole('dialog', { name: 'Rename Playlist' })).toBeInTheDocument();
    const nameInput = screen.getByDisplayValue('Road Trip');
    fireEvent.change(nameInput, { target: { value: 'Road Trip Updated' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(apiMock.playlists.update).toHaveBeenCalledWith('1', 'Road Trip Updated', 'Travel songs'));

    fireEvent.click(screen.getByTitle('Delete playlist'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(apiMock.playlists.remove).toHaveBeenCalledWith('1'));
  });

  it('toggles remember-progress and configures every crossfade mode and reset', async () => {
    apiMock.crossfade.config.mockResolvedValue({ mode: 'crossfade', duration: 4, source: 'override' });
    apiMock.crossfade.upsertOverride.mockResolvedValue({ ok: true });
    apiMock.crossfade.removeOverride.mockResolvedValue({ ok: true });
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');

    openOptions();
    fireEvent.click(screen.getByTitle('Remember track position: Off'));
    await waitFor(() => expect(apiMock.playlists.update).toHaveBeenCalledWith('1', 'Road Trip', 'Travel songs', 1));
    expect(await screen.findByText('Crossfade settings')).toBeInTheDocument();
    for (const mode of ['Off', 'Zero-gap', 'Crossfade']) {
      fireEvent.click(screen.getByRole('button', { name: mode }));
    }
    fireEvent.change(screen.getByRole('slider'), { target: { value: '8' } });
    await waitFor(() => expect(apiMock.crossfade.upsertOverride).toHaveBeenLastCalledWith({
      entity_type: 'playlist', entity_id: '1', mode: 'crossfade', duration: 8,
    }));
    fireEvent.click(screen.getByRole('button', { name: 'Reset to default' }));
    await waitFor(() => expect(apiMock.crossfade.removeOverride).toHaveBeenCalledWith('playlist', '1'));
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByText('Crossfade settings')).not.toBeInTheDocument();
  });

  it('creates playlists from empty state and reports create, edit, and delete failures', async () => {
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockRejectedValueOnce(new Error('Create failed'));
    const { unmount } = render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} />);
    expect(await screen.findByText(/lonely here/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'New Playlist' }));
    fireEvent.change(screen.getByPlaceholderText('Playlist name'), { target: { value: 'Broken' } });
    fireEvent.keyDown(screen.getByPlaceholderText('Description (optional)'), { key: 'Enter' });
    expect(await screen.findByText('Create failed')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByPlaceholderText('Playlist name'), { key: 'Escape' });
    unmount();

    apiMock.playlists.list.mockResolvedValue([playlist]);
    apiMock.playlists.update.mockRejectedValueOnce(new Error('Edit failed'));
    apiMock.playlists.remove.mockRejectedValueOnce(new Error('Delete failed'));
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    openOptions();
    fireEvent.click(screen.getByTitle('Rename'));
    fireEvent.change(screen.getByDisplayValue('Road Trip'), { target: { value: 'Failed' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Edit failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByTitle('Delete playlist'));
    expect(screen.getByRole('dialog', { name: 'Delete Playlist' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(await screen.findByText('Delete failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  });

  it('starts and cancels BoogieMix, runs deep analysis, and renders output and plan details', async () => {
    const playTrack = vi.fn();
    apiMock.boogiemix.listOutputs.mockResolvedValue([{ id: 'out1', file_name: 'mix.flac', duration_sec: 200, file_size_bytes: 1000, format: 'mp3' }]);
    apiMock.boogiemix.getJob.mockResolvedValueOnce({
      id: 'mix-1', status: 'planning', progress_percent: 50, current_step: 'AI plan',
      mix_quality: 'high_quality', used_deep_analysis: true,
      mix_strategy: 'Build slowly', last_message: 'Almost there',
      deep_analysis_total_count: 2, deep_analysis_ready_count: 1,
      plan_summary: { energyCurvePhases: ['warmup', 'peak'], anthemTrackId: '101' },
    });
    render(<PlaylistsView playTrack={playTrack} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    openMix();
    fireEvent.change(screen.getByTitle('BoogieMix style'), { target: { value: 'chill_blend' } });
    fireEvent.change(screen.getByTitle('BoogieMix quality'), { target: { value: 'high_quality' } });
    fireEvent.change(screen.getByTitle('Transition length'), { target: { value: '45' } });
    fireEvent.click(screen.getByTitle('BoogieMix is experimental'));
    await waitFor(() => expect(apiMock.boogiemix.createJob).toHaveBeenCalledWith('1', 'chill_blend', 'high_quality', 45));
    // The compact status line shows just the step; strategy/energy-curve/anthem detail is hover-only.
    expect(await screen.findByText('BoogieMix — AI plan 50%')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByTestId('boogiemix-status'));
    expect(screen.getByText(/AI Mix Strategy: Build slowly/)).toBeInTheDocument();
    expect(screen.getByText('warmup → peak')).toBeInTheDocument();
    expect(screen.getByText(/Anthem Track ID: 101/)).toBeInTheDocument();
    // While the new mix is still rendering, Play/Download for the old output sit on their
    // own labeled "Previous mix" line so they can't be mistaken for the in-progress job's output.
    expect(screen.getByText('Previous mix — mix.flac')).toBeInTheDocument();
    // Play button plays the rendered output in-app; Download stays untouched alongside it.
    expect(screen.getByRole('link', { name: 'Download' })).toHaveAttribute('href', '/api/boogiemix/outputs/out1/file');
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(apiMock.boogiemix.playUrl).toHaveBeenCalledWith('out1');
    expect(playTrack).toHaveBeenCalledTimes(1);
    const [playedTrack, playedQueue] = playTrack.mock.calls[0];
    expect(playedTrack).toEqual(expect.objectContaining({
      id: 'boogiemix:out1',
      title: 'Road Trip — BoogieMix',
      stream_url_override: '/api/boogiemix/outputs/out1/play',
    }));
    expect(playedQueue).toEqual([playedTrack]);
    // Persistent (not "once") so it also covers the concurrent progress-poll
    // tick racing the cancel handler's own getJob refresh.
    apiMock.boogiemix.getJob.mockResolvedValue({
      id: 'mix-1', status: 'canceled', progress_percent: 50,
      mix_quality: 'high_quality', used_deep_analysis: true,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(apiMock.boogiemix.cancelJob).toHaveBeenCalledWith('mix-1'));
    await waitFor(() => expect(screen.getByTestId('boogiemix-status')).toHaveTextContent('BoogieMix canceled'));

    openMix();
    fireEvent.click(screen.getByTitle(/Run Demucs deep analysis/i));
    await waitFor(() => expect(apiMock.boogiemix.queuePlaylistDeepAnalysis).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('Deep analysis — 2/2 tracks')).toBeInTheDocument();
  });

  it('keeps Play/Download on the main status line (no separate "Previous mix" line) once the mix is ready', async () => {
    apiMock.boogiemix.listOutputs.mockResolvedValue([{ id: 'out1', file_name: 'mix.flac', duration_sec: 200, file_size_bytes: 1000, format: 'mp3' }]);
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" />);

    await waitFor(() => expect(apiMock.boogiemix.listOutputs).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('BoogieMix ready — mix.flac')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Play' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Download' })).toBeInTheDocument();
    expect(screen.queryByText(/Previous mix —/)).not.toBeInTheDocument();
  });

  it('reattaches to a still-running BoogieMix job on mount', async () => {
    apiMock.boogiemix.latestJobForPlaylist.mockResolvedValue({
      id: 'mix-9', status: 'rendering', progress_percent: 70, current_step: 'Rendering',
      mix_quality: 'standard', used_deep_analysis: false,
    });
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" />);

    await waitFor(() => expect(apiMock.boogiemix.latestJobForPlaylist).toHaveBeenCalledWith('1'));
    expect(await screen.findByText('BoogieMix — Rendering 70%')).toBeInTheDocument();
    // Configuration stays accessible; starting a second render is disabled.
    openMix();
    expect(screen.getByTitle('BoogieMix is experimental')).toBeDisabled();
  });

  it('ignores a finished BoogieMix job on mount so stale failures/cancellations do not resurface', async () => {
    apiMock.boogiemix.latestJobForPlaylist.mockResolvedValue({
      id: 'mix-8', status: 'failed', progress_percent: 40, mix_quality: 'standard', used_deep_analysis: false,
    });
    render(<PlaylistsView playTrack={() => {}} addToQueue={() => {}} initialPlaylistId="1" />);

    await waitFor(() => expect(apiMock.boogiemix.latestJobForPlaylist).toHaveBeenCalledWith('1'));
    expect(screen.queryByTestId('boogiemix-status')).not.toBeInTheDocument();
  });

  it('reports BoogieMix and deep-analysis startup failures', async () => {
    apiMock.boogiemix.createJob.mockRejectedValueOnce(new Error('Mix failed'));
    apiMock.boogiemix.queuePlaylistDeepAnalysis.mockRejectedValueOnce(new Error('Deep failed'));
    render(<PlaylistsView playTrack={vi.fn()} addToQueue={vi.fn()} initialPlaylistId="1" />);
    await screen.findByText('Alpha One');
    openMix();
    fireEvent.click(screen.getByTitle('BoogieMix is experimental'));
    expect(await screen.findByText('Mix failed')).toBeInTheDocument();
    openMix();
    fireEvent.click(screen.getByTitle(/Run Demucs deep analysis/i));
    expect(await screen.findByText('Deep failed')).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByTestId('boogiemix-status'));
    fireEvent.click(screen.getByTitle('Dismiss'));
    expect(screen.queryByText('Deep failed')).not.toBeInTheDocument();
  });
});

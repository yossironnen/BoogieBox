/**
 * Tests Home View.Playback Tabs.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import HomeView from '../components/HomeView';
import type { Artist, ClientEntityId, Stats } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    latestAlbums: vi.fn(),
    albumTracks: vi.fn(),
    homeTopRated: vi.fn(),
    homeGenres: vi.fn(),
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number) => `/api/albums/${albumId}/art?size=${size}`),
    artistPhotoUrl: vi.fn((artistId: ClientEntityId, size: number) => `/api/artists/${artistId}/photo?size=${size}`) ,
    genres: vi.fn(),
    recentlyPlayed: vi.fn(),
    topPlayedTracks: vi.fn(),
    mostPlayedArtists: vi.fn(),
    artists: vi.fn(),
    playlists: {
      list: vi.fn(),
      create: vi.fn(),
    },
    crossfade: {
      config: vi.fn(),
      upsertOverride: vi.fn(),
      removeOverride: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

const STATS: Stats = {
  total_tracks: 1,
  total_artists: 1,
  total_albums: 1,
  total_libraries: 1,
  total_hours: 1,
  total_gb: 1,
};

function mockIntersectionObserver(): void {
  class FakeIntersectionObserver {
    observe(): void {}
    disconnect(): void {}
    unobserve(): void {}
  }
  Object.defineProperty(window, 'IntersectionObserver', {
    writable: true,
    configurable: true,
    value: FakeIntersectionObserver,
  });
}

function renderHome(
  onOpenPlaylist?: (playlistId: string) => void,
  onStartAutoDj?: (genres: string[]) => Promise<number>,
  refreshKey = 0,
  onBrowseMusic?: () => void,
  onOpenArtist?: (artist: Artist) => void,
  stats: Stats | null = STATS,
){
  return render(
    <HomeView
      stats={stats}
      refreshKey={refreshKey}
      onOpenAlbum={() => {}}
      onOpenArtist={onOpenArtist ?? (() => {})}
      onOpenGenre={() => {}}
      onBrowseMusic={onBrowseMusic ?? (() => {})}
      onOpenPlaylist={onOpenPlaylist ?? (() => {})}
      onPlayTrack={() => {}}
      onStartAutoDj={onStartAutoDj ?? (async () => 0)}
    />
  );
}

describe('HomeView playback activity tabs', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    mockIntersectionObserver();
    apiMock.latestAlbums.mockResolvedValue([]);
    apiMock.albumTracks.mockResolvedValue([]);
    apiMock.homeTopRated.mockResolvedValue({ artists: [], albums: [], tracks: [] });
    apiMock.homeGenres.mockResolvedValue([]);
    apiMock.genres.mockResolvedValue([]);
    apiMock.recentlyPlayed.mockResolvedValue([]);
    apiMock.topPlayedTracks.mockResolvedValue([]);
    apiMock.mostPlayedArtists.mockResolvedValue([]);
    apiMock.artists.mockResolvedValue([]);
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockResolvedValue({ id: '1', name: 'New Playlist' });
    apiMock.crossfade.config.mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    apiMock.crossfade.upsertOverride.mockResolvedValue({ ok: true });
    apiMock.crossfade.removeOverride.mockResolvedValue({ ok: true });
  });

  it('refetches top played tracks each time the tab is reselected', async () => {
    renderHome(undefined, undefined, 0);

    expect(screen.getByText("Let's Boogie!")).toBeInTheDocument();

    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Top Played Tracks' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Top Played Tracks' }));
    await waitFor(() => expect(apiMock.topPlayedTracks).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Recently Played' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Recently Played' }));
    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Top Played Tracks' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Top Played Tracks' }));
    await waitFor(() => expect(apiMock.topPlayedTracks).toHaveBeenCalledTimes(2));
  });

  it('refetches most played artists each time the tab is reselected', async () => {
    renderHome(undefined, undefined, 0);

    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Most Played Artists' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Most Played Artists' }));
    await waitFor(() => expect(apiMock.mostPlayedArtists).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Recently Played' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Recently Played' }));
    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Most Played Artists' })).toBeInTheDocument());

    fireEvent.click(screen.getByRole('tab', { name: 'Most Played Artists' }));
    await waitFor(() => expect(apiMock.mostPlayedArtists).toHaveBeenCalledTimes(2));
  });

  it('shows artist total play counts instead of track totals in Most Played Artists view', async () => {
    apiMock.mostPlayedArtists.mockResolvedValue([
      { id: '7', name: 'Tycho', play_count: 12, track_count: 3, album_count: 2 },
    ]);

    renderHome(undefined, undefined, 0);

    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole('tab', { name: 'Most Played Artists' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('tab', { name: 'Most Played Artists' }));
    await waitFor(() => expect(apiMock.mostPlayedArtists).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByText('12 plays')).toBeInTheDocument());
    expect(screen.getAllByText('12 plays')).toHaveLength(1);
    expect(screen.queryByText('3 tracks')).not.toBeInTheDocument();
  });

  it('opens the top artist metric from the matching artist page', async () => {
    const onOpenArtist = vi.fn();
    apiMock.recentlyPlayed.mockResolvedValue([
      {
        id: 'track-1',
        file_name: 'alpha.mp3',
        file_size: null,
        format: 'mp3',
        duration: 180,
        bitrate: null,
        sample_rate: null,
        channels: null,
        title: 'Alpha',
        artist: 'Boards of Canada',
        album: 'Geogaddi',
        library_name: null,
        track_number: null,
        disc_number: null,
        year: null,
        genre: null,
        composer: null,
        comment: null,
        bpm: null,
        scanned_at: '2026-06-01 09:00:00',
        last_played_at: new Date().toISOString(),
      },
    ]);
    apiMock.artists.mockResolvedValue([
      { id: 'artist-1', name: 'Boards of Canada', track_count: 12, album_count: 3 },
    ]);

    renderHome(undefined, undefined, 0, undefined, onOpenArtist);

    const topArtistButton = await screen.findByRole('button', { name: 'Open artist Boards of Canada' });
    fireEvent.click(topArtistButton);

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onOpenArtist).toHaveBeenCalledWith(expect.objectContaining({
      id: 'artist-1',
      name: 'Boards of Canada',
    })));
  });

  it('uses distinct active/inactive styling for playback view tabs', async () => {
    renderHome();

    await waitFor(() => expect(apiMock.recentlyPlayed).toHaveBeenCalledTimes(1));
    const tablist = screen.getByRole('tablist', { name: 'Playback activity views' });
    const recentTab = screen.getByRole('tab', { name: 'Recently Played' });
    const topTracksTab = screen.getByRole('tab', { name: 'Top Played Tracks' });

    expect(tablist.getAttribute('style')).toContain('background-color: color-mix(in srgb, var(--surface) 74%, var(--bg))');
    expect(recentTab.getAttribute('style')).toContain('background-color: color-mix(in srgb, var(--surface) 72%, var(--bg))');
    expect(topTracksTab.getAttribute('style')).toContain('background-color: transparent');

    fireEvent.click(topTracksTab);
    await waitFor(() => expect(apiMock.topPlayedTracks).toHaveBeenCalledTimes(1));
    expect(topTracksTab.getAttribute('style')).toContain('background-color: color-mix(in srgb, var(--surface) 72%, var(--bg))');
    expect(recentTab.getAttribute('style')).toContain('background-color: transparent');
  });

  it('creates a playlist from home pane and opens the created playlist id', async () => {
    const onOpenPlaylist = vi.fn();
    const promptSpy = vi.spyOn(window, 'prompt').mockReturnValue('Road Trip');
    apiMock.playlists.create.mockResolvedValue({ id: '42', name: 'Road Trip' });

    renderHome(onOpenPlaylist);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Create playlist from home' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist from home' }));

    await waitFor(() => expect(apiMock.playlists.create).toHaveBeenCalledWith('Road Trip'));
    await waitFor(() => expect(onOpenPlaylist).toHaveBeenCalledWith('42'));

    promptSpy.mockRestore();
  });

  it('opens an existing playlist from the Home playlists pane', async () => {
    const onOpenPlaylist = vi.fn();
    apiMock.playlists.list.mockResolvedValue([{ id: '42', name: 'Road Trip', track_count: 5, art_album_ids: ['301', '302'] }]);

    renderHome(onOpenPlaylist);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Open playlist Road Trip' })).toBeInTheDocument());
    expect(screen.getByLabelText('Road Trip artwork')).toBeInTheDocument();
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('301', 300);
    expect(apiMock.albumArtUrl).toHaveBeenCalledWith('302', 300);
    fireEvent.click(screen.getByRole('button', { name: 'Open playlist Road Trip' }));

    expect(onOpenPlaylist).toHaveBeenCalledWith('42');
  });

  it('updates the Home playlists pane with the newly created playlist immediately', async () => {
    const promptSpy = vi.spyOn(window, 'prompt').mockReturnValue('New Mix');
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create.mockResolvedValue({ id: '77', name: 'New Mix' });

    renderHome();

    await waitFor(() => expect(screen.getByRole('button', { name: 'Create playlist from home' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist from home' }));

    await waitFor(() => expect(apiMock.playlists.create).toHaveBeenCalledWith('New Mix'));
    await waitFor(() => expect(screen.getByText('New Mix')).toBeInTheDocument());

    promptSpy.mockRestore();
  });

  it('prevents creating a duplicate playlist name from the Home pane', async () => {
    const promptSpy = vi.spyOn(window, 'prompt').mockReturnValue('road trip');
    apiMock.playlists.list.mockResolvedValue([{ id: '1', name: 'Road   Trip', track_count: 5 }]);

    renderHome();

    await waitFor(() => expect(screen.getByRole('button', { name: 'Create playlist from home' })).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Create playlist from home' }));

    expect(apiMock.playlists.create).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText('A playlist with this name already exists')).toBeInTheDocument());

    promptSpy.mockRestore();
  });

  it('starts Auto DJ from the Let\'s Boogie quick genre chips', async () => {
    const onStartAutoDj = vi.fn(async () => 12);
    apiMock.homeGenres.mockResolvedValue([{ label: 'Rock', canonical_key: 'rock', track_count: 3, artist_count: 2, album_count: 1, raw_labels: ['Rock'] }]);

    renderHome(undefined, onStartAutoDj);

    fireEvent.click(await screen.findByRole('button', { name: 'Start Home Auto DJ with Rock' }));

    await waitFor(() => expect(onStartAutoDj).toHaveBeenCalledWith(['Rock']));
    await waitFor(() => expect(screen.getByText(/Auto DJ started/i)).toBeInTheDocument());
  });

  it('opens the extra Auto DJ picker and starts selected genres', async () => {
    const onStartAutoDj = vi.fn(async () => 8);
    apiMock.genres.mockResolvedValue([{ genre: 'Rock', track_count: 3 }]);

    renderHome(undefined, onStartAutoDj);

    fireEvent.click(await screen.findByRole('button', { name: 'Toggle more Home Auto DJ genres' }));
    const genreSelect = await screen.findByLabelText('Home Auto DJ genre picker') as HTMLSelectElement;
    const rockOption = Array.from(genreSelect.options).find((option) => option.value === 'Rock');
    expect(rockOption).toBeTruthy();
    if (!rockOption) throw new Error('Rock option missing for Home Auto DJ picker');
    rockOption.selected = true;
    fireEvent.change(genreSelect);
    fireEvent.click(screen.getByRole('button', { name: 'Start Home Auto DJ from picker' }));

    await waitFor(() => expect(onStartAutoDj).toHaveBeenCalledWith(['Rock']));
  });

  it('opens music browse from the new Genres footer CTA', async () => {
    const onBrowseMusic = vi.fn();
    apiMock.homeGenres.mockResolvedValue([{ label: 'Rock', canonical_key: 'rock', track_count: 3, artist_count: 2, album_count: 1, raw_labels: ['Rock'] }]);

    renderHome(undefined, undefined, 0, onBrowseMusic);

    fireEvent.click(await screen.findByRole('button', { name: 'Browse all genres in music' }));
    expect(onBrowseMusic).toHaveBeenCalledTimes(1);
  });

  it('opens ranked artists and albums, and starts ranked track playback from Home Top Rated', async () => {
    const onOpenArtist = vi.fn();
    const onOpenAlbum = vi.fn();
    const onPlayTrack = vi.fn();
    const rankedTrack = {
      id: '71',
      file_path: 'ranked.mp3',
      file_name: 'ranked.mp3',
      file_size: null,
      format: 'mp3',
      duration: 201,
      bitrate: null,
      sample_rate: null,
      channels: null,
      title: 'Ranked Track',
      artist: 'Ranked Artist',
      album: 'Ranked Album',
      library_name: null,
      track_number: null,
      disc_number: null,
      year: null,
      genre: null,
      composer: null,
      comment: null,
      bpm: null,
      scanned_at: '2026-03-22 12:00:00',
      rating: 4.5,
    };
    apiMock.homeTopRated.mockResolvedValue({
      artists: [{ id: '5', name: 'Boards of Canada', track_count: 12, album_count: 3, rating: 5 }],
      albums: [{ id: '6', title: 'Music Has the Right to Children', artist: 'Boards of Canada', album_artist: 'Boards of Canada', track_count: 18, rating: 4.5 }],
      tracks: [rankedTrack],
    });

    render(
      <HomeView
        stats={STATS}
        refreshKey={0}
        onOpenAlbum={onOpenAlbum}
        onOpenArtist={onOpenArtist}
        onOpenGenre={() => {}}
        onBrowseMusic={() => {}}
        onOpenPlaylist={() => {}}
        onPlayTrack={onPlayTrack}
        onStartAutoDj={async () => 0}
      />,
    );

    await waitFor(() => expect(apiMock.homeTopRated).toHaveBeenCalledWith(3));

    fireEvent.click(screen.getByRole('button', { name: 'Open artist Boards of Canada' }));
    expect(onOpenArtist).toHaveBeenCalledWith(expect.objectContaining({ id: '5', name: 'Boards of Canada' }));

    fireEvent.click(screen.getByRole('button', { name: 'Open album Music Has the Right to Children' }));
    expect(onOpenAlbum).toHaveBeenCalledWith(expect.objectContaining({ id: '6', title: 'Music Has the Right to Children' }));

    fireEvent.click(screen.getByRole('button', { name: 'Play ranked track Ranked Track' }));
    expect(onPlayTrack).toHaveBeenCalledWith(rankedTrack, [rankedTrack]);
  });

  it('shows the Top Rated empty state when the user has no ratings yet', async () => {
    apiMock.homeTopRated.mockResolvedValue({ artists: [], albums: [], tracks: [] });

    renderHome();

    await waitFor(() => expect(apiMock.homeTopRated).toHaveBeenCalledWith(3));
    await waitFor(() => expect(screen.getByText('Rate some artists, albums, or tracks to build your rankings')).toBeInTheDocument());
  });

  it('refetches recent albums when the home refresh key changes', async () => {
    apiMock.latestAlbums
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        {
          id: '11',
          title: 'Fresh Arrival',
          artist: 'New Artist',
          album_artist: 'New Artist',
          year: 2026,
          genre: 'Rock',
          track_count: 8,
          total_duration: 1800,
          added_at: '2026-03-19 10:00:00',
          latest_scanned_at: '2026-03-19 10:00:00',
        },
      ]);

    const view = renderHome(undefined, undefined, 0);

    await waitFor(() => expect(screen.getByText('No albums yet')).toBeInTheDocument());
    expect(apiMock.latestAlbums).toHaveBeenCalledTimes(1);

    view.rerender(
      <HomeView
        stats={STATS}
        refreshKey={1}
        onOpenAlbum={() => {}}
        onOpenArtist={() => {}}
        onOpenGenre={() => {}}
        onBrowseMusic={() => {}}
        onOpenPlaylist={() => {}}
        onPlayTrack={() => {}}
        onStartAutoDj={async () => 0}
      />,
    );

    await waitFor(() => expect(apiMock.latestAlbums).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByText('Fresh Arrival')).toBeInTheDocument());
  });

  it('saves Home Auto DJ transition override mode and duration changes', async () => {
    apiMock.homeGenres.mockResolvedValue([{ label: 'Rock', canonical_key: 'rock', track_count: 3, artist_count: 2, album_count: 1, raw_labels: ['Rock'] }]);

    renderHome();

    await waitFor(() => expect(apiMock.crossfade.config).toHaveBeenCalledWith('autodj', '0'));
    fireEvent.click(await screen.findByRole('button', { name: 'Toggle Home Auto DJ options' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Set Home Auto DJ transition mode Crossfade' }));

    await waitFor(() => expect(apiMock.crossfade.upsertOverride).toHaveBeenCalledWith({
      entity_type: 'autodj',
      entity_id: '0',
      mode: 'crossfade',
      duration: 2,
    }));

    const durationSlider = screen.getByLabelText('Home Auto DJ crossfade duration');
    fireEvent.change(durationSlider, { target: { value: '6' } });

    await waitFor(() => expect(apiMock.crossfade.upsertOverride).toHaveBeenLastCalledWith({
      entity_type: 'autodj',
      entity_id: '0',
      mode: 'crossfade',
      duration: 6,
    }));
  });

  it('resets Home Auto DJ transition override to global default', async () => {
    apiMock.homeGenres.mockResolvedValue([{ label: 'Rock', canonical_key: 'rock', track_count: 3, artist_count: 2, album_count: 1, raw_labels: ['Rock'] }]);
    apiMock.crossfade.config
      .mockResolvedValueOnce({ mode: 'crossfade', duration: 4, source: 'override' })
      .mockResolvedValueOnce({ mode: 'off', duration: 2, source: 'global' });

    renderHome();

    fireEvent.click(await screen.findByRole('button', { name: 'Toggle Home Auto DJ options' }));
    const resetButton = await screen.findByRole('button', { name: 'Reset Home Auto DJ transition override' });
    fireEvent.click(resetButton);

    await waitFor(() => expect(apiMock.crossfade.removeOverride).toHaveBeenCalledWith('autodj', '0'));
    await waitFor(() => expect(apiMock.crossfade.config).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByText('Using global default')).toBeInTheDocument());
  });

  it('persists collapsed dashboard panes and expands a previously collapsed pane', async () => {
    localStorage.setItem('boogiebox-pane-collapsed-Library', 'true');
    renderHome();

    expect(screen.getByTitle('Expand')).toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Expand'));
    expect(screen.getByText('Tracks')).toBeInTheDocument();

    const collapseButtons = screen.getAllByTitle('Collapse');
    expect(collapseButtons).toHaveLength(6);
    for (const button of collapseButtons) fireEvent.click(button);

    expect(screen.getAllByTitle('Expand')).toHaveLength(6);
    expect(localStorage.getItem('boogiebox-pane-collapsed-Genres')).toBe('true');
  });

  it('shows null-stat and rejected-service fallbacks without breaking the dashboard', async () => {
    apiMock.latestAlbums.mockRejectedValue(new Error('albums unavailable'));
    apiMock.homeTopRated.mockRejectedValue(new Error('ratings unavailable'));
    apiMock.homeGenres.mockRejectedValue(new Error('genres unavailable'));
    apiMock.genres.mockRejectedValue(new Error('genres unavailable'));
    apiMock.recentlyPlayed.mockRejectedValue(new Error('history unavailable'));
    apiMock.crossfade.config.mockRejectedValue(new Error('crossfade unavailable'));

    renderHome(undefined, undefined, 0, undefined, undefined, null);

    expect(screen.getAllByText('--')).toHaveLength(3);
    await waitFor(() => expect(screen.getByText('No albums yet')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText('Rate some artists, albums, or tracks to build your rankings')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText('No genre data yet')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText(/No playlists yet/i)).toBeInTheDocument());
  });

  it('handles recent-album hover, unknown artists, empty playback, and successful playback', async () => {
    const onOpenAlbum = vi.fn();
    const onPlayTrack = vi.fn();
    const track = {
      id: 'track-1',
      title: 'Playable',
      file_name: 'playable.mp3',
    };
    apiMock.latestAlbums.mockResolvedValue([
      { id: '11', title: 'Mystery Album', artist: null, album_artist: null },
      { id: '12', title: 'Playable Album', artist: 'Known Artist', album_artist: null },
    ]);
    apiMock.albumTracks
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([track]);

    render(
      <HomeView
        stats={STATS}
        onOpenAlbum={onOpenAlbum}
        onOpenArtist={() => {}}
        onOpenGenre={() => {}}
        onBrowseMusic={() => {}}
        onOpenPlaylist={() => {}}
        onPlayTrack={onPlayTrack}
        onStartAutoDj={async () => 0}
      />,
    );

    const mystery = await screen.findByTitle('Mystery Album — Unknown Artist');
    fireEvent.mouseEnter(mystery);
    fireEvent.click(screen.getByRole('button', { name: 'Play album Mystery Album' }));
    await waitFor(() => expect(apiMock.albumTracks).toHaveBeenCalledWith('11'));
    expect(onPlayTrack).not.toHaveBeenCalled();
    fireEvent.mouseLeave(mystery);
    fireEvent.click(mystery);
    expect(onOpenAlbum).toHaveBeenCalledWith(expect.objectContaining({ id: '11' }));

    const playable = screen.getByTitle('Playable Album — Known Artist');
    fireEvent.mouseEnter(playable);
    fireEvent.click(screen.getByRole('button', { name: 'Play album Playable Album' }));
    await waitFor(() => expect(onPlayTrack).toHaveBeenCalledWith(track, [track]));
  });

  it('handles Auto DJ validation, launch failure, and transition persistence failures', async () => {
    apiMock.homeGenres.mockResolvedValue([{ label: 'Rock', canonical_key: 'rock', track_count: 3, artist_count: 2, album_count: 1, raw_labels: ['Rock'] }]);
    apiMock.genres.mockResolvedValue([{ genre: 'Rock', track_count: 3 }]);
    apiMock.crossfade.config.mockResolvedValue({ mode: 'crossfade', duration: 4, source: 'override' });
    apiMock.crossfade.upsertOverride.mockRejectedValue(new Error('save failed'));
    apiMock.crossfade.removeOverride.mockRejectedValue(new Error('reset failed'));
    const onStartAutoDj = vi.fn().mockRejectedValue({});
    renderHome(undefined, onStartAutoDj);

    fireEvent.click(await screen.findByRole('button', { name: 'Start Home Auto DJ with Rock' }));
    expect(await screen.findByText('Failed to start Auto DJ.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Toggle more Home Auto DJ genres' }));
    fireEvent.click(screen.getByRole('button', { name: 'Start Home Auto DJ from picker' }));
    expect(screen.getByText('Select at least one genre for Auto DJ.')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Toggle Home Auto DJ options' }));
    fireEvent.click(screen.getByRole('button', { name: 'Set Home Auto DJ transition mode Zero-gap' }));
    await waitFor(() => expect(apiMock.crossfade.upsertOverride).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Reset Home Auto DJ transition override' }));
    await waitFor(() => expect(apiMock.crossfade.removeOverride).toHaveBeenCalled());
  });

  it('handles cancelled, blank, id-less, and rejected Home playlist creation', async () => {
    apiMock.playlists.list.mockResolvedValue([]);
    apiMock.playlists.create
      .mockResolvedValueOnce({ name: 'Missing ID' })
      .mockRejectedValueOnce({});
    const promptSpy = vi.spyOn(window, 'prompt')
      .mockReturnValueOnce(null)
      .mockReturnValueOnce('   ')
      .mockReturnValueOnce('Missing ID')
      .mockReturnValueOnce('Rejected');
    renderHome();
    const create = await screen.findByRole('button', { name: 'Create playlist from home' });

    fireEvent.click(create);
    fireEvent.click(create);
    expect(apiMock.playlists.create).not.toHaveBeenCalled();
    fireEvent.click(create);
    expect(await screen.findByText('Could not create playlist')).toBeInTheDocument();
    fireEvent.click(create);
    expect(await screen.findByText('Could not create playlist')).toBeInTheDocument();
    promptSpy.mockRestore();
  });
});


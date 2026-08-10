/**
 * Tests App.Component.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App, { THEME_STORAGE_KEY, sortSearchTracks } from '../App';
import { DEFAULT_SETTINGS } from '../types';

const { apiMock, getStreamDirectMock, openContextMenuMock, kebabPropsMock } = vi.hoisted(() => ({
  apiMock: {
    libraries: {
      list: vi.fn(),
      add: vi.fn(),
      remove: vi.fn(),
      scan: vi.fn(),
    },
    scanJobs: { get: vi.fn(), active: vi.fn() },
    boogiemix: { deepAnalysisStatus: vi.fn(), queueLibraryDeepAnalysis: vi.fn() },
    admin: { cancelScanJob: vi.fn() },
    stats: vi.fn(),
    playbackSettings: vi.fn(),
    systemStatus: vi.fn(),
    settings: { get: vi.fn() },
    userSettings: { get: vi.fn(), update: vi.fn() },
    genres: vi.fn(),
    search: vi.fn(),
    autoDjTracks: vi.fn(),
    markTrackPlayed: vi.fn(),
    setArtistRating: vi.fn(),
    setAlbumRating: vi.fn(),
    setTrackRating: vi.fn(),
    artists: vi.fn(),
    albums: vi.fn(),
    debugTestPath: vi.fn(),
    auth: {
      me: vi.fn(),
      getLoginUsers: vi.fn(),
      login: vi.fn(),
      logout: vi.fn(),
    },
  },
  getStreamDirectMock: vi.fn(),
  openContextMenuMock: vi.fn(),
  kebabPropsMock: vi.fn(),
}));

vi.mock('../api', () => ({
  api: apiMock,
  getStreamDirect: getStreamDirectMock,
}));

vi.mock('../components/HomeView', () => ({
  default: (props: any) => (
    <div data-testid="home-view">
      <button onClick={() => props.onOpenArtist({ id: '1', name: 'Artist One', track_count: 2, album_count: 1 })}>home-open-artist</button>
      <button onClick={() => props.onOpenAlbum({ id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2 })}>home-open-album</button>
      <button onClick={() => props.onOpenGenre('Rock')}>home-open-genre</button>
      <button onClick={() => props.onOpenPlaylist('42')}>home-open-playlist</button>
      <button onClick={() => props.onPlayTrack({ id: '1', file_path: 'x', file_name: 'x.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Track One', artist: 'Artist One', album: 'Album One', library_name: 'Main', track_number: 1, disc_number: 1, year: 2020, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01' })}>home-play-track</button>
      <button onClick={() => props.onStartAutoDj(['Rock'])}>home-start-auto-dj</button>
      <button onClick={() => props.onBrowseMusic()}>home-browse-music</button>
      <button onClick={() => props.onOpenGenre('   ')}>home-empty-genre</button>
    </div>
  ),
}));


vi.mock('../components/BrowseView', () => ({
  default: (props: any) => (
    <div data-testid="browse-view">
      browse:{props.openAlbumRequest ? 'album' : props.openArtistRequest ? 'artist' : props.openGenreRequest ? 'genre' : 'none'}:libs:{(props.forcedLibraryIds ?? []).join(',') || 'all'}
      <button onClick={() => props.playTrack({ id: 'b1', title: 'Browse Track' }, [{ id: 'b1' }])}>browse-play</button>
      <button onClick={() => props.addToQueue({ id: 'b1', title: 'Browse Track' })}>browse-queue</button>
      <button onClick={() => props.playAlbumInVinylMode([{ id: 'v1', title: 'Vinyl One' }, { id: 'v2', title: 'Vinyl Two' }])}>browse-vinyl</button>
    </div>
  ),
}));

vi.mock('../components/PlaylistsView', () => ({
  default: (props: any) => <div data-testid="playlists-view">playlist:{props.initialPlaylistId ?? 'none'}</div>,
}));

vi.mock('../components/SettingsPage', () => ({
  default: (props: any) => (
    <div data-testid="settings-view">
      <button onClick={() => props.onSettingsChange({ ...DEFAULT_SETTINGS, lastfmKey: 'changed' })}>change-settings</button>
      <button onClick={() => props.onStreamDirectChange?.(true)}>set-stream-direct</button>
      <button onClick={() => props.onAdaptiveAccentEnabledChange(false)}>set-adaptive</button>
      <button onClick={() => props.onVinylHardcoreChange(true)}>set-hardcore</button>
      <button onClick={() => props.onVinylNeedleDropChange(true)}>set-needle</button>
      <button onClick={() => props.onVinylAnalogFxDisabledChange(true)}>set-analog-off</button>
      <button onClick={() => props.onVinylNeedleDropIntensityChange(0.9)}>set-intensity</button>
      <button onClick={() => props.onLibrariesRefresh()}>refresh-libraries</button>
      <button onClick={() => props.onLogout()}>settings-logout</button>
    </div>
  ),
}));

vi.mock('../components/Player', () => ({
  default: (props: any) => (
    <div data-testid="player-view">
      player
      <button onClick={() => props.onStateChange({ queue: [{ id: 'p1' }], currentIndex: 0, isPlaying: true, playToken: 1 })}>player-state</button>
      <button onClick={() => props.onOpenArtist('Artist One')}>player-artist</button>
      <button onClick={() => props.onOpenAlbum('Album One', 'Artist One')}>player-album</button>
      <button onClick={() => props.onPlaybackSnapshotChange({ currentTrack: { id: 'p1' }, currentTime: 1, duration: 2 })}>player-snapshot</button>
    </div>
  ),
}));

vi.mock('../components/ContextMenu', async () => {
  const actual = await vi.importActual('../components/ContextMenu');
  return {
    ...actual,
    ContextMenuRoot: () => <div data-testid="context-menu-root">context</div>,
    KebabButton: (props: any) => {
      kebabPropsMock(props);
      return <button type="button" aria-label="More actions" />;
    },
    openContextMenu: openContextMenuMock,
  };
});

describe('App component flows', () => {
  it('uses the BoogieBox theme storage key', () => {
    expect(THEME_STORAGE_KEY).toBe('boogiebox.theme.v1');
  });

  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('confirm', vi.fn(() => true));

    getStreamDirectMock.mockReturnValue(false);
    apiMock.libraries.list.mockResolvedValue([
      { id: '1', path: 'D:\\Music', name: 'Main Library', added_at: '2026-01-01', last_scan: null, track_count: 3 },
    ]);
    apiMock.stats.mockResolvedValue({
      total_tracks: 3,
      total_artists: 2,
      total_albums: 2,
      total_libraries: 1,
      total_hours: 1,
      total_gb: 0.1,
    });
    apiMock.systemStatus.mockResolvedValue({ ffmpegAvailable: true });
    apiMock.playbackSettings.mockResolvedValue({ transcodeQuality: 'high', replayGainEnabled: '0', vinylMode: '0', lastfmConfigured: false });
    apiMock.auth.me.mockResolvedValue({ id: '1', username: 'admin', role: 'admin', canManageLibraries: true, canEditMetadata: true });
    apiMock.settings.get.mockResolvedValue({ lastfmKey: 'lastfm', transcodeQuality: 'high' });
    apiMock.userSettings.get.mockResolvedValue({});
    apiMock.userSettings.update.mockResolvedValue({ ok: true });
    apiMock.genres.mockResolvedValue([{ genre: 'Rock', track_count: 3 }]);
    apiMock.search.mockResolvedValue({
      tracks: [
        {
          id: '1', file_path: 'D:\\Music\\track-one.mp3', file_name: 'track-one.mp3', file_size: 1,
          format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2,
          title: 'Track One', artist: 'Artist One', album: 'Album One', library_name: 'Main',
          track_number: 1, disc_number: 1, year: 2020, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01',
        },
      ],
      total: 1,
      page: 1,
      limit: 100,
      artists: [{ id: '1', name: 'Artist One', track_count: 2, album_count: 1 }],
      albums: [{ id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2 }],
      top_results: [],
    });
    apiMock.autoDjTracks.mockResolvedValue({
      tracks: [
        {
          id: '11', file_path: 'D:\\Music\\dj-one.mp3', file_name: 'dj-one.mp3', file_size: 1,
          format: 'MP3', duration: 180, bitrate: 320, sample_rate: 44100, channels: 2,
          title: 'DJ One', artist: 'Artist One', album: 'Album One', library_name: 'Main',
          track_number: 1, disc_number: 1, year: 2020, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01',
        },
        {
          id: '12', file_path: 'D:\\Music\\dj-two.mp3', file_name: 'dj-two.mp3', file_size: 1,
          format: 'MP3', duration: 181, bitrate: 320, sample_rate: 44100, channels: 2,
          title: 'DJ Two', artist: 'Artist Two', album: 'Album Two', library_name: 'Main',
          track_number: 1, disc_number: 1, year: 2021, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01',
        },
      ],
    });
    apiMock.markTrackPlayed.mockResolvedValue({ ok: true });
    apiMock.setArtistRating.mockResolvedValue({ ok: true });
    apiMock.setAlbumRating.mockResolvedValue({ ok: true });
    apiMock.setTrackRating.mockResolvedValue({ ok: true });
    apiMock.artists.mockResolvedValue([{ id: '1', name: 'Artist One', track_count: 2, album_count: 1 }]);
    apiMock.albums.mockResolvedValue([{ id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2 }]);
    apiMock.debugTestPath.mockResolvedValue({ exists: true, isDirectory: true, displayName: 'Music' });
    apiMock.libraries.add.mockResolvedValue({ id: '2', path: 'D:\\More', name: 'More', added_at: '2026-01-01', last_scan: null, track_count: 0 });
    apiMock.libraries.remove.mockResolvedValue({ ok: true });
    apiMock.libraries.scan.mockResolvedValue({ jobId: '55' });
    apiMock.admin.cancelScanJob.mockResolvedValue({ ok: true, id: '55', status: 'cancelled' });
    apiMock.scanJobs.active.mockResolvedValue([]);
    apiMock.boogiemix.queueLibraryDeepAnalysis.mockResolvedValue({ queued: 3 });
    apiMock.boogiemix.deepAnalysisStatus.mockResolvedValue({
      enabled: true,
      runtime: null,
      queue: { pending: 0, running: 0, failed: 0, skipped: 0, done: 0 },
      cache: { analyzedTracks: 0, estimatedBytes: 0, oldestCreatedAt: null, newestCreatedAt: null },
    });
    apiMock.scanJobs.get.mockResolvedValue({
      id: '55',
      library_id: '1',
      status: 'done',
      files_found: 3,
      files_scanned: 3,
      errors: 0,
      started_at: '2026-01-01',
      finished_at: '2026-01-01',
    });
  });

  it('navigates home/search/browse/playlists/settings and keeps library management inside settings', async () => {
    render(<App />);

    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());
    expect(screen.getByText('BoogieBox')).toBeInTheDocument();
    expect(screen.getByTestId('home-view')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText(/Transcoding on \(320 kbps\)/i)).toBeInTheDocument());

    fireEvent.click(screen.getByText('home-open-playlist'));
    expect(await screen.findByTestId('playlists-view')).toHaveTextContent('playlist:42');

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    expect(screen.getByText('Find artists, albums, and tracks across your music library.')).toBeInTheDocument();
    const searchInput = screen.getByPlaceholderText(/Search titles, artists, albums/i);
    expect(searchInput).toBeInTheDocument();
    fireEvent.focus(searchInput);
    expect(searchInput.parentElement).toHaveStyle({ boxShadow: 'var(--focus-ring)' });
    fireEvent.change(searchInput, { target: { value: 'Track' } });
    await waitFor(() => expect(apiMock.search).toHaveBeenCalled(), { timeout: 2000 });
    expect(screen.getByText(/1 artist/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Album One.*Artist One/i }));
    expect(await screen.findByTestId('browse-view')).toHaveTextContent('browse:album');

    expect(screen.getByText('Libraries')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByTestId('settings-view')).toBeInTheDocument();
    fireEvent.click(screen.getByText('set-stream-direct'));
    expect(screen.getByLabelText(/Transcoding off/i)).toBeInTheDocument();
  }, 10000);

  it('does not render a standalone Libraries sidebar item', async () => {
    render(<App />);
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());
    expect(screen.queryByRole('button', { name: 'Libraries' })).not.toBeInTheDocument();
  });

  it('shows active scan progress and refreshes stats while a background scan is running', async () => {
    apiMock.scanJobs.active.mockResolvedValue([{
      id: '55', library_id: '1', status: 'running', started_at: '2026-01-01', finished_at: null,
      files_found: 40, files_scanned: 3, errors: 0,
    }]);
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<App />);
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.getByTestId('sidebar-status-scan')).toHaveAttribute('aria-label', 'Library scan: Main Library • 3 / 40 files');

      apiMock.stats.mockResolvedValue({
        total_tracks: 41, total_artists: 9, total_albums: 7, total_libraries: 1, total_hours: 4, total_gb: 1.2,
      });
      const callsBeforePoll = apiMock.stats.mock.calls.length;
      await vi.advanceTimersByTimeAsync(5000);

      expect(apiMock.stats.mock.calls.length).toBeGreaterThan(callsBeforePoll);
      expect(screen.queryByText('Tracks')).not.toBeInTheDocument();
      expect(screen.queryByText('Artists')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('opens Browse scoped to the clicked sidebar library and clears back to all libraries from Browse nav', async () => {
    render(<App />);

    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());

    expect(screen.queryByRole('button', { name: 'Movies' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Browse Music' }));
    expect(await screen.findByTestId('browse-view')).toHaveTextContent('browse:none:libs:all');
  });

  it('runs library radio, scan, and deep analysis from the library action model', async () => {
    render(<App />);
    await waitFor(() => expect(kebabPropsMock).toHaveBeenCalled());

    const props = kebabPropsMock.mock.calls
      .map(([value]) => value)
      .find((value) => value.target.kind === 'library');
    expect(props.callbacks.actions.map((action: any) => action.label)).toEqual([
      'Play library radio', 'Scan library', 'Run deep analysis',
    ]);

    await act(async () => props.callbacks.actions[0].onSelect());
    expect(apiMock.autoDjTracks).toHaveBeenCalledWith({ genres: [], library_id: '1', limit: 200 });
    await waitFor(() => expect(apiMock.markTrackPlayed).toHaveBeenCalledWith('11'));

    await act(async () => props.callbacks.actions[1].onSelect());
    expect(apiMock.libraries.scan).toHaveBeenCalledWith('1');
    expect(apiMock.scanJobs.active).toHaveBeenCalledTimes(2);

    await act(async () => props.callbacks.actions[2].onSelect());
    expect(apiMock.boogiemix.queueLibraryDeepAnalysis).toHaveBeenCalledWith('1');
    expect(apiMock.boogiemix.deepAnalysisStatus).toHaveBeenCalledTimes(2);
  });

  it('shows disabled administrative library actions when the user lacks permission', async () => {
    apiMock.auth.me.mockResolvedValue({
      id: '2', username: 'listener', role: 'user', canManageLibraries: false, canEditMetadata: false,
    });
    apiMock.scanJobs.active.mockResolvedValue([{
      id: '55', library_id: '1', status: 'running', started_at: '2026-01-01', finished_at: null,
      files_found: 40, files_scanned: 3, errors: 0,
    }]);
    render(<App />);

    await waitFor(() => {
      const props = kebabPropsMock.mock.calls
        .map(([value]) => value)
        .find((value) => value.target.kind === 'library'
          && value.callbacks.actions.some((action: any) => action.label === 'Cancel scan'));
      expect(props).toBeDefined();
      const actions = props.callbacks.actions;
      expect(actions.find((action: any) => action.label === 'Play library radio').disabled).toBe(false);
      expect(actions.find((action: any) => action.label === 'Cancel scan').disabled).toBe(true);
      expect(actions.find((action: any) => action.label === 'Run deep analysis').disabled).toBe(true);
    });
  });

  it('removes Search view options and keeps the results grid sortable, including rating', async () => {
    const tracks = [
      {
        id: '1', file_path: 'D:\\Music\\track-one.mp3', file_name: 'track-one.mp3', file_size: 1,
        format: 'MP3', duration: 180, bitrate: 192, sample_rate: 44100, channels: 2,
        title: 'Gamma Track', artist: 'Zulu Artist', album: 'Beta Album', library_name: 'Main',
        track_number: 1, disc_number: 1, year: 2022, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', rating: 2.5,
      },
      {
        id: '2', file_path: 'D:\\Music\\track-two.mp3', file_name: 'track-two.mp3', file_size: 1,
        format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2,
        title: 'Alpha Track', artist: 'Echo Artist', album: 'Zulu Album', library_name: 'Main',
        track_number: 2, disc_number: 1, year: 2019, genre: 'Pop', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', rating: 5,
      },
      {
        id: '3', file_path: 'D:\\Music\\track-three.mp3', file_name: 'track-three.mp3', file_size: 1,
        format: 'MP3', duration: 240, bitrate: 256, sample_rate: 44100, channels: 2,
        title: 'Beta Track', artist: 'Mike Artist', album: 'Alpha Album', library_name: 'Main',
        track_number: 3, disc_number: 1, year: 2021, genre: 'Jazz', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', rating: null,
      },
    ];
    apiMock.search.mockImplementation(async ({ sort = 'title', order = 'asc' }: { sort?: any; order?: any }) => ({
      tracks: sortSearchTracks(tracks, sort, order),
      total: tracks.length,
      page: 1,
      limit: 100,
      artists: [],
      albums: [],
      top_results: [],
    }));

    render(<App />);
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.change(screen.getByPlaceholderText(/Search titles, artists, albums/i), { target: { value: 'Track' } });
    await waitFor(() => expect(apiMock.search).toHaveBeenCalled(), { timeout: 2000 });
    expect(screen.queryByRole('button', { name: 'View options' })).not.toBeInTheDocument();

    const getTrackTitles = () =>
      screen.getAllByText(/^(Alpha Track|Beta Track|Gamma Track)$/).map((node) => node.textContent);

    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Alpha Track', 'Beta Track', 'Gamma Track']));

    fireEvent.click(screen.getByText('Year'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'year', order: 'asc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Alpha Track', 'Beta Track', 'Gamma Track']));

    fireEvent.click(screen.getByText('Year'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'year', order: 'desc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Gamma Track', 'Beta Track', 'Alpha Track']));

    fireEvent.click(screen.getByText('Dur'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'duration', order: 'asc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Alpha Track', 'Gamma Track', 'Beta Track']));

    fireEvent.click(screen.getByText('Kbps'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'bitrate', order: 'asc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Gamma Track', 'Beta Track', 'Alpha Track']));

    fireEvent.click(screen.getByText('Rating'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'rating', order: 'desc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Alpha Track', 'Gamma Track', 'Beta Track']));

    fireEvent.click(screen.getByText('Rating'));
    await waitFor(() => expect(apiMock.search).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'Track', sort: 'rating', order: 'asc' })));
    await waitFor(() => expect(getTrackTitles().slice(0, 3)).toEqual(['Beta Track', 'Gamma Track', 'Alpha Track']));
  }, 10000);


  it('keeps exactly one sidebar menu item active when switching views', async () => {
    render(<App />);

    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());

    const navLabels = ['Home', 'Search', 'Browse Music', 'Playlists', 'Settings'] as const;
    const homeButton = screen.getByRole('button', { name: 'Home' });
    const searchButton = screen.getByRole('button', { name: 'Search' });
    const settingsButton = screen.getByRole('button', { name: 'Settings' });

    expect(homeButton).toHaveAttribute('aria-current', 'page');
    expect(searchButton).not.toHaveAttribute('aria-current');

    fireEvent.click(searchButton);
    expect(searchButton).toHaveAttribute('aria-current', 'page');
    expect(homeButton).not.toHaveAttribute('aria-current');

    fireEvent.click(settingsButton);
    expect(settingsButton).toHaveAttribute('aria-current', 'page');
    expect(searchButton).not.toHaveAttribute('aria-current');

    const activeNavButtons = navLabels
      .map((label) => screen.getByRole('button', { name: label }))
      .filter((button) => button.getAttribute('aria-current') === 'page');
    expect(activeNavButtons).toHaveLength(1);
    expect(activeNavButtons[0]).toBe(settingsButton);
  });

  it('starts Auto DJ from the Home Genres pane action', async () => {
    render(<App />);

    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalled());
    fireEvent.click(screen.getByText('home-start-auto-dj'));

    await waitFor(() => expect(apiMock.autoDjTracks).toHaveBeenCalledWith({
      genres: ['Rock'],
      library_id: undefined,
      limit: 200,
    }));
    await waitFor(() => expect(apiMock.markTrackPlayed).toHaveBeenCalledWith('11'));
  });

  it('executes Browse, Settings, and Player callback state flows', async () => {
    render(<App />);
    await screen.findByTestId('home-view');
    fireEvent.click(screen.getByText('home-empty-genre'));
    expect(screen.getByTestId('home-view')).toBeInTheDocument();
    fireEvent.click(screen.getByText('home-browse-music'));
    expect(await screen.findByTestId('browse-view')).toBeInTheDocument();
    fireEvent.click(screen.getByText('browse-play'));
    fireEvent.click(screen.getByText('browse-queue'));
    fireEvent.click(screen.getByText('browse-vinyl'));
    await waitFor(() => expect(apiMock.markTrackPlayed).toHaveBeenCalledWith('v1'));

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    for (const name of [
      'change-settings',
      'set-stream-direct',
      'set-adaptive',
      'set-hardcore',
      'set-needle',
      'set-analog-off',
      'set-intensity',
      'refresh-libraries',
    ]) {
      fireEvent.click(screen.getByText(name));
    }
    await waitFor(() => expect(apiMock.libraries.list).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByText('player-state'));
    fireEvent.click(screen.getByText('player-snapshot'));
    fireEvent.click(screen.getByText('player-artist'));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:artist'));
    fireEvent.click(screen.getByText('player-album'));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:album'));
  });

  it('exercises Search filters, sorting, ratings, rows, pagination, and quick results', async () => {
    const secondTrack = {
      id: '2', file_path: 'D:\\Music\\untitled.flac', file_name: 'untitled.flac', file_size: 2,
      format: 'FLAC', duration: null, bitrate: null, sample_rate: 48000, channels: 2,
      title: '', artist: '', album: '', library_name: 'Main',
      track_number: null, disc_number: null, year: null, genre: '', composer: null, comment: null,
      bpm: null, scanned_at: '2026-01-01', rating: null,
    };
    apiMock.search.mockResolvedValue({
      tracks: [
        {
          id: '1', file_path: 'D:\\Music\\track-one.mp3', file_name: 'track-one.mp3', file_size: 1,
          format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2,
          title: 'Track One', artist: 'Artist One', album: 'Album One', library_name: 'Main',
          track_number: 1, disc_number: 1, year: 2020, genre: 'Rock', composer: null, comment: null,
          bpm: null, scanned_at: '2026-01-01', rating: 3,
        },
        secondTrack,
      ],
      total: 201,
      page: 1,
      limit: 100,
      artists: [
        { id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: 2 },
        { id: '2', name: 'Artist Two', track_count: 1, album_count: 2, rating: null },
      ],
      albums: [
        { id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2, rating: 4 },
        { id: '20', title: 'Album Two', artist: null, album_artist: null, year: null, genre: null, track_count: 0, rating: null },
      ],
      top_results: [
        { id: '1', type: 'artist', title: 'Top Artist', subtitle: 'Artist One' },
        { id: '10', type: 'album', title: 'Top Album', subtitle: '' },
        { id: '1', type: 'track', title: 'Top Track', subtitle: 'Artist One' },
        { id: 'missing', type: 'track', title: 'Missing Track', subtitle: null },
      ],
    });

    render(<App />);
    await screen.findByTestId('home-view');
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const query = screen.getByPlaceholderText(/Search titles, artists, albums/i);
    fireEvent.change(query, { target: { value: 'music' } });
    await screen.findByText('Top Results');

    const [librarySelect, genreSelect] = screen.getAllByRole('combobox');
    fireEvent.change(librarySelect, { target: { value: '1' } });
    fireEvent.change(genreSelect, { target: { value: 'Rock' } });
    fireEvent.change(screen.getByPlaceholderText('Year'), { target: { value: '2020' } });
    fireEvent.click(screen.getByTitle(/Sonic Fingerprint/));

    for (const heading of ['Title', 'Title', 'Artist', 'Album', 'Genre', 'Year', 'Dur', 'Kbps', 'Rating', 'Rating']) {
      fireEvent.click(screen.getByText(heading));
    }
    fireEvent.keyDown(screen.getByRole('slider', { name: 'Artist One artist rating' }), { key: 'End' });
    fireEvent.keyDown(screen.getByRole('slider', { name: 'Album One album rating' }), { key: 'Home' });
    fireEvent.keyDown(screen.getByRole('slider', { name: 'Track One search rating' }), { key: 'ArrowRight' });
    await waitFor(() => expect(apiMock.setArtistRating).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.setAlbumRating).toHaveBeenCalled());
    await waitFor(() => expect(apiMock.setTrackRating).toHaveBeenCalled());

    fireEvent.mouseEnter(screen.getByText('Track One').closest('[style*="cursor: pointer"]')!);
    fireEvent.click(screen.getAllByTitle('Play')[0]);
    fireEvent.mouseLeave(screen.getByText('Track One').closest('[style*="cursor: pointer"]')!);
    fireEvent.click(screen.getByRole('button', { name: /Top Track.*Artist One/ }));
    fireEvent.click(screen.getByRole('button', { name: /Missing Track.*track/ }));
    fireEvent.click(screen.getByText('→'));
    await waitFor(() => expect(apiMock.search).toHaveBeenCalledWith(expect.objectContaining({ page: 2 })));
    fireEvent.click(screen.getByText('←'));
    await waitFor(() => expect(apiMock.search).toHaveBeenCalledWith(expect.objectContaining({ page: 1 })));

    fireEvent.click(screen.getByRole('button', { name: /Artist Two.*2 albums/i }));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:artist'));
  });

  it('uses Home album lookup exact, fallback, and failure paths', async () => {
    const { unmount } = render(<App />);
    await screen.findByTestId('home-view');
    fireEvent.click(screen.getByText('home-open-album'));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:album'));
    unmount();

    apiMock.albums.mockResolvedValueOnce([{ id: 'different', title: 'Album One', album_artist: 'Artist One' }]);
    const second = render(<App />);
    await screen.findByTestId('home-view');
    fireEvent.click(screen.getByText('home-open-album'));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:album'));
    second.unmount();

    apiMock.albums.mockRejectedValueOnce(new Error('offline'));
    render(<App />);
    await screen.findByTestId('home-view');
    fireEvent.click(screen.getByText('home-open-album'));
    await waitFor(() => expect(screen.getByTestId('browse-view')).toHaveTextContent('browse:album'));
  });

});

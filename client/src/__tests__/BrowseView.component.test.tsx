/**
 * Tests Browse View.Component.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import BrowseView, { AlbumGrid, AlbumList, mergeAlbumChanges } from '../components/BrowseView';
import { ContextMenuRoot } from '../components/ContextMenu';
import type { Album, ClientEntityId, Library, Track } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    genres: vi.fn(),
    artists: vi.fn(),
    albums: vi.fn(),
    albumChangeCursor: vi.fn(),
    artistAlbums: vi.fn(),
    artistAppearsOn: vi.fn(),
    artistSimilar: vi.fn(),
    resolveArtistReleaseTypes: vi.fn(),
    albumTracks: vi.fn(),
    albumTracksByGroup: vi.fn(),
    artistRadio: vi.fn(),
    search: vi.fn(),
    setArtistRating: vi.fn(),
    setAlbumRating: vi.fn(),
    setTrackRating: vi.fn(),
    artist: vi.fn(),
    mergeArtists: vi.fn(),
    getArtistMergeInfo: vi.fn(),
    unmergeArtist: vi.fn(),
    lockArtistIdentity: vi.fn(),
    scanJobs: { active: vi.fn() },
    albumArtUrl: vi.fn((albumId: ClientEntityId, size: number, version?: string | number) => `/api/albums/${albumId}/art?size=${size}${version ? `&v=${version}` : ''}`),
    artistPhotoUrl: vi.fn((artistId: ClientEntityId, size: number, version?: string | number) => `/api/artists/${artistId}/photo?size=${size}${version ? `&v=${version}` : ''}`),
    lastfm: {
      info: vi.fn(),
      topTracks: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

vi.mock('../components/ContextMenu', async () => await vi.importActual('../components/ContextMenu'));

function makeTrack(id: ClientEntityId, title: string, rating?: number | null): Track {
  return {
    id,
    file_path: `D:\\Music\\${title}.mp3`,
    file_name: `${title}.mp3`,
    file_size: 1000,
    format: 'mp3',
    duration: 180,
    bitrate: 320,
    sample_rate: 44100,
    channels: 2,
    title,
    artist: 'Artist One',
    album: 'Album One',
    library_name: 'Main',
    track_number: 1,
    disc_number: 1,
    year: 2020,
    genre: 'Rock',
    composer: null,
    comment: null,
    bpm: null,
    scanned_at: '2026-02-26T00:00:00Z',
    rating: rating ?? null,
  };
}

describe('BrowseView component flows', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('fetch', vi.fn());
    vi.stubGlobal('alert', vi.fn());

    const album: Album = {
      id: '10',
      title: 'Album One',
      artist: 'Artist One',
      album_artist: 'Artist One',
      year: 2020,
      genre: 'Rock',
      track_count: 2,
      total_duration: 360,
      rating: 4.5,
    };
    const tracks = [makeTrack('1', 'Top Song'), makeTrack('2', 'Second Song')];

    apiMock.genres.mockResolvedValue([{ genre: 'Rock', track_count: 2 }]);
    apiMock.artists.mockResolvedValue([{ id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: 3.5 }]);
    apiMock.albums.mockResolvedValue([album]);
    apiMock.albumChangeCursor.mockResolvedValue({ cursor: 1 });
    apiMock.scanJobs.active.mockResolvedValue([]);
    apiMock.artistAlbums.mockResolvedValue([album]);
    apiMock.artistAppearsOn.mockResolvedValue([]);
    apiMock.artistSimilar.mockResolvedValue({ sourceArtistId: '1', artists: [] });
    apiMock.resolveArtistReleaseTypes.mockResolvedValue({ updated: false });
    apiMock.albumTracks.mockResolvedValue(tracks);
    apiMock.albumTracksByGroup.mockResolvedValue(tracks);
    apiMock.artistRadio.mockResolvedValue({ artist: 'Artist One', tags: ['rock'], tracks: [tracks[0]] });
    apiMock.setArtistRating.mockResolvedValue({ ok: true, rating: 4.5 });
    apiMock.setAlbumRating.mockResolvedValue({ ok: true, rating: 5, updated: 1 });
    apiMock.setTrackRating.mockResolvedValue({ ok: true, rating: 0.5 });
    apiMock.getArtistMergeInfo.mockResolvedValue({ merged: false, members: [] });
    apiMock.artist.mockResolvedValue({ id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: 3.5 });
    apiMock.search.mockImplementation(async ({ q }: { q?: string }) => ({
      tracks: [makeTrack('100', q || 'Top Song')],
      total: 1,
      page: 1,
      limit: 50,
      artists: [],
      albums: [],
    }));
    apiMock.lastfm.info.mockResolvedValue({
      summary: 'Artist summary',
      content: 'Artist full',
      listeners: 100,
      playcount: 200,
      tags: ['rock'],
      url: 'https://last.fm/artist',
    });
    apiMock.lastfm.topTracks.mockResolvedValue([
      { name: 'Top Song', playcount: 500, listeners: 50, url: 'https://last.fm/top-song' },
    ]);

    vi.mocked(fetch).mockImplementation(async (input) => {
      const url = String(input);
      if (url.includes('/api/artists/1/photo')) {
        return {
          ok: true,
          headers: { get: () => 'application/json' },
          json: async () => ({ url: 'https://img.example.com/a.jpg', source: 'deezer' }),
        } as unknown as Response;
      }
      if (url.includes('method=artist.getInfo')) {
        return {
          ok: true,
          json: async () => ({
            artist: {
              bio: { summary: 'Artist summary', content: 'Artist full' },
              stats: { listeners: '100', playcount: '200' },
              tags: { tag: [{ name: 'rock' }] },
              url: 'https://last.fm/artist',
            },
          }),
        } as unknown as Response;
      }
      if (url.includes('method=artist.getTopTracks')) {
        return {
          ok: true,
          json: async () => ({
            toptracks: {
              track: [
                { name: 'Top Song', playcount: '500', listeners: '50', url: 'https://last.fm/top-song' },
              ],
            },
          }),
        } as unknown as Response;
      }
      if (url.includes('method=album.getInfo')) {
        return {
          ok: true,
          json: async () => ({
            album: {
              wiki: { summary: 'Album summary', content: 'Album full' },
              tags: { tag: [{ name: 'rock' }] },
            },
          }),
        } as unknown as Response;
      }
      return {
        ok: true,
        json: async () => ({}),
      } as unknown as Response;
    });
  });

  it('merges grouped album scan deltas without replacing unchanged albums', () => {
    const unchanged = { id: '1', title: 'Existing', artist: 'One', album_artist: 'One', year: 2020, genre: null, track_count: 1 } satisfies Album;
    const grouped = { id: '2', title: 'Shared', artist: 'Two', album_artist: 'Two', year: 2021, genre: null, track_count: 1 } satisfies Album;
    const updatedGroup = { ...grouped, id: '3', track_count: 2 };
    const added = { id: '4', title: 'New', artist: 'Four', album_artist: 'Four', year: 2026, genre: null, track_count: 1 } satisfies Album;

    const merged = mergeAlbumChanges([unchanged, grouped], [updatedGroup, added], 'album_artist');

    expect(merged).toEqual([unchanged, updatedGroup, added]);
    expect(merged[0]).toBe(unchanged);
    expect(mergeAlbumChanges(merged, [], 'album_artist')).toBe(merged);
  });

  it('keeps same-title albums distinct in artist grouping mode', () => {
    const first = { id: '1', title: 'Shared', artist: 'One', album_artist: 'One', year: 2020, genre: null, track_count: 1 } satisfies Album;
    const second = { id: '2', title: 'Shared', artist: 'Two', album_artist: 'Two', year: 2021, genre: null, track_count: 1 } satisfies Album;

    expect(mergeAlbumChanges([first], [second], 'artist')).toEqual([first, second]);
  });

  it('merges bounded album deltas during a scan and fully reconciles after completion', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const initial = { id: '10', title: 'Initial Album', artist: 'One', album_artist: 'One', year: 2020, genre: null, track_count: 1 } satisfies Album;
      const discovered = { id: '11', title: 'Discovered Album', artist: 'Two', album_artist: 'Two', year: 2026, genre: null, track_count: 1 } satisfies Album;
      let cursor = 1;
      let activeJobs: Array<{ id: string; status: string }> = [];
      apiMock.albumChangeCursor.mockImplementation(async () => ({ cursor }));
      apiMock.scanJobs.active.mockImplementation(async () => activeJobs);
      apiMock.albums.mockImplementation(async (params?: { after_album_rowid?: number }) => (
        params?.after_album_rowid ? [discovered] : cursor === 1 ? [initial] : [initial, discovered]
      ));

      render(
        <BrowseView
          libraries={[]}
          playTrack={vi.fn()}
          playAlbumInVinylMode={vi.fn()}
          addToQueue={vi.fn()}
          lastfmKey=""
        />,
      );
      fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
      await waitFor(() => expect(screen.getByText('Initial Album')).toBeInTheDocument());

      cursor = 2;
      activeJobs = [{ id: 'scan-1', status: 'running' }];
      await vi.advanceTimersByTimeAsync(5000);
      await waitFor(() => expect(screen.getByText('Discovered Album')).toBeInTheDocument());
      expect(apiMock.albums).toHaveBeenLastCalledWith(expect.objectContaining({
        after_album_rowid: 1,
        through_album_rowid: 2,
      }));

      activeJobs = [];
      await vi.advanceTimersByTimeAsync(5000);
      await waitFor(() => expect(apiMock.albums).toHaveBeenLastCalledWith(expect.not.objectContaining({
        after_album_rowid: expect.anything(),
      })));
    } finally {
      vi.useRealTimers();
    }
  });

  it('drills artist -> album and plays radio, top tracks, album tracks, and queue-all actions', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByText('Artist One'));

    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1'));
    expect(await screen.findByText(/Top 5 Songs/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Play Artist Radio/i }));
    await waitFor(() => expect(apiMock.artistRadio).toHaveBeenCalledWith('1', 120));
    expect(playTrack).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ id: '1' }),
      expect.arrayContaining([expect.objectContaining({ id: '1' })]),
    );

    fireEvent.click(screen.getByRole('button', { name: /Play Top 5/i }));
    await waitFor(() => expect(apiMock.search).toHaveBeenCalled());

    fireEvent.click(screen.getByText('Album One'));
    await waitFor(() => expect(apiMock.albumTracks).toHaveBeenCalledWith('10'));
    expect(await screen.findByRole('button', { name: /Play All/i })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Play All/i }));
    await waitFor(() => expect(playTrack).toHaveBeenCalledTimes(3));
    const lastPlayCall = playTrack.mock.calls[playTrack.mock.calls.length - 1];
    expect(lastPlayCall?.[0]).toEqual(expect.objectContaining({ id: expect.stringMatching(/^(1|2)$/) }));
    expect(lastPlayCall?.[1]).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: '1' }),
      expect.objectContaining({ id: '2' }),
    ]));
    expect(lastPlayCall?.[2]).toEqual({ type: 'album', id: '10' });

    fireEvent.click(screen.getByRole('button', { name: /\+ Queue All/i }));
    expect(addToQueue).toHaveBeenCalledTimes(2);
  }, 15000);

  it('renders owned similar artists and navigates to the selected artist', async () => {
    apiMock.artistSimilar.mockImplementation(async (artistId: ClientEntityId) => ({
      sourceArtistId: artistId,
      artists: artistId === '1'
        ? [{ id: '2', name: 'Related Artist', track_count: 5, album_count: 1, score: 1, providers: ['lastfm'] }]
        : [],
    }));

    render(
      <BrowseView
        libraries={[]}
        playTrack={vi.fn()}
        playAlbumInVinylMode={vi.fn()}
        addToQueue={vi.fn()}
        lastfmKey="test-lastfm-key"
      />
    );

    fireEvent.click(await screen.findByText('Artist One'));
    expect(await screen.findByText('similar artists')).toBeInTheDocument();
    expect(apiMock.artistSimilar).toHaveBeenCalledWith('1', 12);

    const relatedArtistButton = screen.getByText('Related Artist').closest('button');
    if (!relatedArtistButton) throw new Error('Related artist button not found');
    fireEvent.click(relatedArtistButton);

    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('2'));
    expect(apiMock.artistSimilar).toHaveBeenCalledWith('2', 12);
  });

  it('plays album rows with album queue source metadata', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());

    fireEvent.click(screen.getByTitle('Play album'));
    await waitFor(() => expect(playTrack).toHaveBeenCalledWith(
      expect.objectContaining({ id: '1' }),
      expect.arrayContaining([expect.objectContaining({ id: '1' }), expect.objectContaining({ id: '2' })]),
      { type: 'album', id: '10' },
    ));
  });

  it('allows Artist Radio playback even when local lastfmKey prop is empty', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey=""
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByText('Artist One'));
    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByRole('button', { name: /Play Artist Radio/i }));
    await waitFor(() => expect(apiMock.artistRadio).toHaveBeenCalledWith('1', 120));
  });

  it('renders every artist release section and handles empty and failed playback builders', async () => {
    const releases: Album[] = [
      { id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2, total_duration: 360, releaseType: 'album' },
      { id: '11', title: 'Single One', artist: 'Artist One', album_artist: 'Artist One', year: null, genre: null, track_count: 1, total_duration: 30, releaseType: 'single' },
      { id: '12', title: 'Compilation One', artist: 'Artist One', album_artist: 'Artist One', year: 2021, genre: 'Rock', track_count: 1, total_duration: 3600, releaseType: 'compilation' },
    ];
    apiMock.artistAlbums.mockResolvedValue(releases);
    apiMock.artistAppearsOn.mockResolvedValue([
      { id: '13', title: 'Guest Album', artist: 'Artist One', album_artist: 'Various', year: 2022, genre: 'Pop', track_count: 1, total_duration: null },
    ]);
    apiMock.resolveArtistReleaseTypes.mockResolvedValueOnce({ updated: true });
    apiMock.artistRadio
      .mockResolvedValueOnce({ artist: 'Artist One', tags: [], tracks: [] })
      .mockRejectedValueOnce(new Error('radio offline'));
    apiMock.search.mockRejectedValue(new Error('search offline'));
    apiMock.albumTracks.mockResolvedValue([]);
    const playTrack = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={vi.fn()}
        addToQueue={vi.fn()}
        lastfmKey="test-lastfm-key"
      />,
    );
    await screen.findByTitle('Artist One');
    fireEvent.click(screen.getByText('Artist One'));
    expect(await screen.findByText('Singles & EPs')).toBeInTheDocument();
    expect(screen.getByText('Compilations')).toBeInTheDocument();
    expect(screen.getByText('Appears On')).toBeInTheDocument();
    expect(screen.getByText('Guest Album')).toBeInTheDocument();
    await waitFor(() => expect(apiMock.resolveArtistReleaseTypes).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByRole('button', { name: /Play Artist Radio/i }));
    await waitFor(() => expect(alert).toHaveBeenCalledWith(expect.stringContaining('No radio tracks')));
    fireEvent.click(screen.getByRole('button', { name: /Play Artist Radio/i }));
    await waitFor(() => expect(alert).toHaveBeenCalledWith('radio offline'));
    fireEvent.click(screen.getByRole('button', { name: /Play Top 5/i }));
    await waitFor(() => expect(alert).toHaveBeenCalledWith('search offline'));

    fireEvent.click(screen.getByText('Single One'));
    await waitFor(() => expect(apiMock.albumTracks).toHaveBeenCalledWith('11'));
    expect(await screen.findByText('No tracks found.')).toBeInTheDocument();
    expect(playTrack).not.toHaveBeenCalled();
  });

  it('refetches album root rows when breadcrumb Browse is clicked from an externally opened album', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();
    const album: Album = {
      id: '10',
      title: 'Album One',
      artist: 'Artist One',
      album_artist: 'Artist One',
      year: 2020,
      genre: 'Rock',
      track_count: 2,
      total_duration: 360,
    };

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
        openAlbumRequest={{ album, token: 1 }}
      />
    );

    await waitFor(() => expect(apiMock.albumTracksByGroup).toHaveBeenCalledWith('Album One', 'Artist One'));

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());
    expect(await screen.findByText('Album One')).toBeInTheDocument();
  });

  it('renders the album detail cover after opening an album from the desktop album grid', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    fireEvent.click(within(screen.getByRole('dialog', { name: 'Browse refine options' })).getByRole('button', { name: 'Grid' }));

    fireEvent.click(screen.getByTitle('Album One'));
    await waitFor(() => expect(apiMock.albumTracksByGroup).toHaveBeenCalledWith('Album One', 'Artist One'));

    await waitFor(() => {
      const detailCover = screen.getAllByAltText('Album One').find((node) =>
        node instanceof HTMLImageElement && node.getAttribute('src')?.includes('size=800'),
      );
      expect(detailCover).toBeTruthy();
    });
  });

  it('restores the selected album anchor when returning to the desktop album grid root', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    fireEvent.click(within(screen.getByRole('dialog', { name: 'Browse refine options' })).getByRole('button', { name: 'Grid' }));

    const scrollIntoViewMock = vi.fn();
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: scrollIntoViewMock,
    });

    fireEvent.click(screen.getByTitle('Album One'));
    await waitFor(() => expect(apiMock.albumTracksByGroup).toHaveBeenCalledWith('Album One', 'Artist One'));

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));

    await waitFor(() => expect(scrollIntoViewMock).toHaveBeenCalled());
  });

  it('shows album ratings in the desktop album grid and saves album + track rating edits in album detail', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    fireEvent.click(within(screen.getByRole('dialog', { name: 'Browse refine options' })).getByRole('button', { name: 'Grid' }));

    expect(screen.getByRole('img', { name: 'Album One album rating' })).toBeInTheDocument();
    expect(screen.getByText('4.5')).toBeInTheDocument();

    fireEvent.click(screen.getByTitle('Album One'));
    await waitFor(() => expect(apiMock.albumTracksByGroup).toHaveBeenCalledWith('Album One', 'Artist One'));

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Album One album rating' }), { key: 'End' });
    await waitFor(() => expect(apiMock.setAlbumRating).toHaveBeenCalledWith('10', 5));
    await waitFor(() => expect(screen.getByRole('slider', { name: 'Album One album rating' })).toHaveAttribute('aria-valuenow', '5'));
    expect(screen.getByText('5')).toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Top Song track rating' }), { key: 'Home' });
    await waitFor(() => expect(apiMock.setTrackRating).toHaveBeenCalledWith('1', 0.5));
  });

  it('saves artist ratings from the artist detail header', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        libraries={[{ id: '1', name: 'Main', path: 'D:\\Music', added_at: '2026-01-01', last_scan: null, track_count: 2 }]}
        lastfmKey="abc123"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByText('Artist One'));

    await waitFor(() => expect(screen.getByRole('slider', { name: 'Artist One artist rating' })).toBeInTheDocument());
    fireEvent.keyDown(screen.getByRole('slider', { name: 'Artist One artist rating' }), { key: 'End' });

    await waitFor(() => expect(apiMock.setArtistRating).toHaveBeenCalledWith('1', 5));
    await waitFor(() => expect(screen.getByRole('slider', { name: 'Artist One artist rating' })).toHaveAttribute('aria-valuenow', '4.5'));
  });

  it('defaults root artist and album browse to grid and resets back to grid', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    expect(screen.getByTitle('Artist One')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());
    expect(screen.getByTitle('Album One')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    const refineDialog = screen.getByRole('dialog', { name: 'Browse refine options' });
    fireEvent.click(within(refineDialog).getByRole('button', { name: 'Table' }));

    await waitFor(() => expect(screen.queryByTitle('Album One')).not.toBeInTheDocument());
    expect(screen.getByText('View: Table')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Clear refine filters' }));

    await waitFor(() => expect(screen.getByTitle('Album One')).toBeInTheDocument());
    expect(screen.queryByText('View: Table')).not.toBeInTheDocument();
  });

  it('exercises root refinement chips, multi-select filters, layout, grouping, and sort directions', async () => {
    apiMock.genres.mockResolvedValue([
      { genre: 'Rock', track_count: 2 },
      { genre: 'Jazz', track_count: 1 },
    ]);
    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: 4.5 },
      { id: '2', name: '# Noise', track_count: 1, album_count: 2, rating: null },
    ]);
    apiMock.albums.mockResolvedValue([
      { id: '10', title: 'Album One', artist: 'Artist One', album_artist: 'Artist One', year: 2020, genre: 'Rock', track_count: 2, total_duration: 3661, rating: 4.5 },
      { id: '20', title: '# Untitled', artist: null, album_artist: null, year: null, genre: null, track_count: 0, total_duration: 45, rating: null },
    ]);
    const libraries: Library[] = [
      { id: '1', path: 'D:\\Music', name: 'Main', added_at: '2026-01-01', last_scan: null, track_count: 2 },
      { id: '2', path: 'D:\\Jazz', name: 'Jazz', added_at: '2026-01-01', last_scan: null, track_count: 1 },
    ];

    render(
      <BrowseView
        libraries={libraries}
        playTrack={vi.fn()}
        playAlbumInVinylMode={vi.fn()}
        addToQueue={vi.fn()}
        lastfmKey="test-lastfm-key"
      />,
    );
    await screen.findByTitle('Artist One');

    fireEvent.click(screen.getByTitle(/Sonic Fingerprint/));
    fireEvent.click(screen.getByRole('button', { name: 'Library filter menu' }));
    const libraryMenu = screen.getByRole('menu', { name: 'Library filter options' });
    for (const checkbox of within(libraryMenu).getAllByRole('checkbox')) fireEvent.click(checkbox);
    expect(screen.getByText('2 selected')).toBeInTheDocument();
    fireEvent.click(within(libraryMenu).getByRole('button', { name: 'Clear' }));

    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    let dialog = screen.getByRole('dialog', { name: 'Browse refine options' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Table' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Name ↑' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Unrated' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Genre filter menu' }));
    const genreMenu = screen.getByRole('menu', { name: 'Genre filter options' });
    fireEvent.click(within(genreMenu).getAllByRole('checkbox')[0]);
    fireEvent.click(within(genreMenu).getAllByRole('checkbox')[1]);
    fireEvent.click(within(genreMenu).getByRole('button', { name: 'Clear' }));
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Browse refine options' })).not.toBeInTheDocument();
    expect(screen.getAllByText('✦ Sonic Fingerprint')).toHaveLength(2);
    expect(screen.getByText('Rating: Unrated')).toBeInTheDocument();
    expect(screen.getByText('Sort: Name ↓')).toBeInTheDocument();
    expect(screen.getByText('View: Table')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Rating: Unrated'));
    fireEvent.click(screen.getByText('Sort: Name ↓'));

    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    dialog = screen.getByRole('dialog', { name: 'Browse refine options' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Artist' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Year' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Year ↑' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Rating' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Rating ↓' }));
    fireEvent.click(within(dialog).getByRole('button', { name: '3+' }));
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('dialog', { name: 'Browse refine options' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Clear refine filters' }));
    await waitFor(() => expect(screen.getByTitle('Album One')).toBeInTheDocument());
  });

  it('shows root browse kebab actions for artist and album grid cards', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <>
        <BrowseView
          libraries={[]}
          playTrack={playTrack}
          playAlbumInVinylMode={playAlbumInVinylMode}
          addToQueue={addToQueue}
          lastfmKey="test-lastfm-key"
        />
        <ContextMenuRoot />
      </>
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());

    const artistCard = screen.getByTitle('Artist One');
    fireEvent.mouseEnter(artistCard);
    fireEvent.click(within(artistCard).getByRole('button', { name: 'More actions' }));

    expect(await screen.findByText('Play artist radio')).toBeInTheDocument();
    expect(screen.getByText('Open artist')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Play artist radio'));
    await waitFor(() => expect(apiMock.artistRadio).toHaveBeenCalledWith('1', 120));

    fireEvent.mouseEnter(artistCard);
    fireEvent.click(within(artistCard).getByRole('button', { name: 'More actions' }));
    fireEvent.click(screen.getByText('Open artist'));
    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));
    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());

    const albumCard = screen.getByTitle('Album One');
    fireEvent.mouseEnter(albumCard);
    fireEvent.click(within(albumCard).getByRole('button', { name: 'More actions' }));

    expect(await screen.findByText('Play album')).toBeInTheDocument();
    expect(screen.getByText('Add to queue')).toBeInTheDocument();
    expect(screen.getByText(/Set crossfade/)).toBeInTheDocument();

    fireEvent.click(screen.getByText('Add to queue'));
    await waitFor(() => expect(addToQueue).toHaveBeenCalledTimes(2));
  });

  it('shows a clickable artist name under album titles in the root album grid', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());

    const artistLink = screen.getByRole('button', { name: 'Open artist Artist One' });
    expect(artistLink).toBeInTheDocument();
    expect(screen.getByTitle('Album One')).toContainElement(artistLink);

    fireEvent.click(artistLink);

    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1'));
  });

  it('filters artists by rating on the desktop artist browse root', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Top Artist', track_count: 3, album_count: 2, rating: 4.5 },
      { id: '2', name: 'Mid Artist', track_count: 2, album_count: 1, rating: 3.5 },
      { id: '3', name: 'Unrated Artist', track_count: 1, album_count: 1, rating: null },
    ]);

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    const artistRefineDialog = screen.getByRole('dialog', { name: 'Browse refine options' });
    fireEvent.click(within(artistRefineDialog).getByRole('button', { name: 'Rated' }));
    await waitFor(() => expect(screen.queryByText('Unrated Artist')).not.toBeInTheDocument());
    expect(screen.getByText('Top Artist')).toBeInTheDocument();
    expect(screen.getByText('Mid Artist')).toBeInTheDocument();

    fireEvent.click(within(artistRefineDialog).getByRole('button', { name: '4+' }));
    await waitFor(() => expect(screen.queryByText('Mid Artist')).not.toBeInTheDocument());
    expect(screen.getByText('Top Artist')).toBeInTheDocument();
  });

  it('filters and sorts albums and album tracks by rating on desktop', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    apiMock.albums.mockResolvedValue([
      {
        id: '11',
        title: 'Low Rated',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2019,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: 2.5,
      },
      {
        id: '12',
        title: 'Top Rated',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2021,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: 4.5,
      },
      {
        id: '13',
        title: 'Unrated Album',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2020,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: null,
      },
    ]);
    apiMock.albumTracksByGroup.mockResolvedValue([
      makeTrack('1', 'Unrated Track', null),
      makeTrack('2', 'Mid Track', 3.5),
      makeTrack('3', 'Top Track', 5),
    ]);

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByRole('button', { name: 'Albums' }));
    await waitFor(() => expect(apiMock.albums).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: 'Browse refine options' }));
    const albumRefineDialog = screen.getByRole('dialog', { name: 'Browse refine options' });
    fireEvent.click(within(albumRefineDialog).getByRole('button', { name: 'Rated' }));
    await waitFor(() => {
      expect(screen.queryByText('Unrated Album')).not.toBeInTheDocument();
    });

    fireEvent.click(within(albumRefineDialog).getByRole('button', { name: /^Rating/ }));
    if (!screen.queryByRole('button', { name: /^Rating ↓$/ })) {
      fireEvent.click(screen.getByRole('button', { name: /^Rating ↑$/ }));
    }
    await waitFor(() => {
      const titles = screen.getAllByText(/^(Top Rated|Low Rated)$/).map((node) => node.textContent);
      expect(titles.slice(0, 2)).toEqual(['Top Rated', 'Low Rated']);
    });

    fireEvent.click(screen.getByText('Top Rated'));
    await waitFor(() => expect(apiMock.albumTracksByGroup).toHaveBeenCalledWith('Top Rated', 'Artist One'));

    fireEvent.click(screen.getByRole('button', { name: 'Rated' }));
    await waitFor(() => {
      expect(screen.queryByText('Unrated Track')).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^Rating$/ }));
    await waitFor(() => {
      const trackTitles = screen.getAllByText(/^(Top Track|Mid Track)$/).map((node) => node.textContent);
      expect(trackTitles.slice(0, 2)).toEqual(['Top Track', 'Mid Track']);
    });
  }, 15000);

  it('filters and sorts artist release lists by rating on desktop', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();

    apiMock.artistAlbums.mockResolvedValue([
      {
        id: '21',
        title: 'Artist Low',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2019,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: 2.5,
        releaseType: 'album',
      },
      {
        id: '22',
        title: 'Artist High',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2022,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: 4.5,
        releaseType: 'album',
      },
      {
        id: '23',
        title: 'Artist Unrated',
        artist: 'Artist One',
        album_artist: 'Artist One',
        year: 2020,
        genre: 'Rock',
        track_count: 2,
        total_duration: 360,
        rating: null,
        releaseType: 'album',
      },
    ]);
    apiMock.artistAppearsOn.mockResolvedValue([
      {
        id: '24',
        title: 'Guest Spot',
        artist: 'Various Artists',
        album_artist: 'Various Artists',
        year: 2021,
        genre: 'Rock',
        track_count: 1,
        total_duration: 180,
        rating: 4,
      },
    ]);

    render(
      <BrowseView
        libraries={[]}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalled());
    fireEvent.click(screen.getByText('Artist One'));
    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1'));

    fireEvent.click(screen.getByRole('button', { name: 'Rated' }));
    await waitFor(() => expect(screen.queryByText('Artist Unrated')).not.toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /^Rating/ }));
    if (!screen.queryByRole('button', { name: /^Rating ↓$/ })) {
      fireEvent.click(screen.getByRole('button', { name: /^Rating ↑$/ }));
    }
    await waitFor(() => {
      const titles = screen.getAllByText(/^(Artist High|Artist Low)$/).map((node) => node.textContent);
      expect(titles.slice(0, 2)).toEqual(['Artist High', 'Artist Low']);
    });
    expect(screen.getByText('Guest Spot')).toBeInTheDocument();
  });

  it('uses forced sidebar library scope for root and drill-in fetches and locks the local library filter', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();
    const libraries = [
      { id: '1', name: 'Main', path: 'D:\\Music', added_at: '2026-01-01', last_scan: null, track_count: 2 },
      { id: '2', name: 'Movies', path: 'D:\\Movies', added_at: '2026-01-02', last_scan: null, track_count: 4 },
    ];

    const { rerender } = render(
      <BrowseView
        libraries={libraries}
        forcedLibraryIds={['2']}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    await waitFor(() => expect(apiMock.artists).toHaveBeenCalledWith(expect.objectContaining({ library_ids: ['2'] })));
    expect(screen.getByRole('button', { name: 'Library filter locked to sidebar selection' })).toBeDisabled();
    expect(screen.getByText('Sidebar scoped')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Artist One'));
    await waitFor(() => expect(apiMock.artistAlbums).toHaveBeenCalledWith('1', ['2']));

    rerender(
      <BrowseView
        libraries={libraries}
        forcedLibraryIds={null}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Browse' }));
    await waitFor(() => expect(apiMock.artists).toHaveBeenLastCalledWith(expect.objectContaining({ library_ids: undefined })));
    expect(screen.getByRole('button', { name: 'Library filter menu' })).not.toBeDisabled();
  });

  it('shows available libraries in the music browse library filter and labels the hero for music browsing', async () => {
    const playTrack = vi.fn();
    const playAlbumInVinylMode = vi.fn();
    const addToQueue = vi.fn();
    const libraries: Library[] = [
      { id: '1', name: 'Main Music', path: 'D:\\Music', library_type: 'music', added_at: '2026-01-01', last_scan: null, track_count: 2 },
      { id: '2', name: 'Legacy Library', path: 'D:\\Legacy', added_at: '2026-01-02', last_scan: null, track_count: 3 },
    ];

    render(
      <BrowseView
        libraries={libraries}
        playTrack={playTrack}
        playAlbumInVinylMode={playAlbumInVinylMode}
        addToQueue={addToQueue}
        lastfmKey="test-lastfm-key"
      />
    );

    expect(screen.getByText('Browse Music')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Library filter menu' }));
    const menu = screen.getByRole('menu', { name: 'Library filter options' });
    expect(within(menu).getByText('Main Music')).toBeInTheDocument();
    expect(within(menu).getByText('Legacy Library')).toBeInTheDocument();
  });

  it('applies Hybrid only when requested and limits tile hover to the artwork', async () => {
    const props = {
      libraries: [] as Library[],
      playTrack: vi.fn(),
      playAlbumInVinylMode: vi.fn(),
      addToQueue: vi.fn(),
      lastfmKey: '',
    };
    const view = render(<BrowseView {...props} />);

    await screen.findByText('Artist One');
    expect(view.container.querySelector('[data-hybrid-preview-surface="browse"]')).toBeNull();

    view.rerender(<BrowseView {...props} hybridPreview />);

    const surface = view.container.querySelector('[data-hybrid-preview-surface="browse"]');
    expect(surface).toBeInTheDocument();
    expect(surface).toHaveStyle({ background: 'var(--bg)' });

    const artistLabel = screen.getByText('Artist One');
    const artistTile = artistLabel.closest('[role="button"]') as HTMLElement;
    const artistArt = screen.getByAltText('Artist One').parentElement?.parentElement as HTMLElement;
    fireEvent.mouseEnter(artistTile);

    expect(artistTile.style.backgroundColor).toBe('transparent');
    expect(artistTile.style.borderColor).toBe('transparent');
    expect(artistTile.style.boxShadow).toBe('none');
    expect(artistArt.style.outline).toContain('--browse-art-hover-outline');
    const artistOverlay = artistArt.querySelector('[data-hybrid-art-hover-overlay="artist"]') as HTMLElement;
    expect(artistOverlay.style.opacity).toBe('1');
    expect(artistOverlay.style.background).toContain('var(--accent)');
    expect(artistLabel).toHaveStyle({ color: 'var(--text)' });

    fireEvent.click(screen.getByRole('button', { name: /^Albums/ }));
    const albumLabel = await screen.findByText('Album One');
    const albumTile = albumLabel.closest('[role="button"]') as HTMLElement;
    const albumArt = screen.getByAltText('Album One').parentElement?.parentElement as HTMLElement;
    fireEvent.mouseEnter(albumTile);

    expect(albumTile.style.backgroundColor).toBe('transparent');
    expect(albumTile.style.borderColor).toBe('transparent');
    expect(albumTile.style.boxShadow).toBe('none');
    expect(albumArt.style.outline).toContain('--browse-art-hover-outline');
    const albumOverlay = albumArt.querySelector('[data-hybrid-art-hover-overlay="album"]') as HTMLElement;
    expect(albumOverlay.style.opacity).toBe('1');
    expect(albumOverlay.style.background).toContain('var(--accent)');
    expect(albumLabel).toHaveStyle({ color: 'var(--text)' });
  });

  it('recovers from root fetch failures and applies only non-empty external genre requests', async () => {
    apiMock.genres.mockRejectedValue(new Error('genres unavailable'));
    apiMock.artists.mockRejectedValue(new Error('artists unavailable'));
    apiMock.albums.mockRejectedValue(new Error('albums unavailable'));
    const props = {
      libraries: [] as Library[],
      playTrack: vi.fn(),
      playAlbumInVinylMode: vi.fn(),
      addToQueue: vi.fn(),
      lastfmKey: '',
    };

    const view = render(<BrowseView {...props} openGenreRequest={{ genre: '   ', token: 1 }} />);
    await waitFor(() => expect(screen.getByText('No artists found.')).toBeInTheDocument());

    fireEvent.click(screen.getByRole('button', { name: /^Albums/ }));
    await waitFor(() => expect(screen.getByText('No albums found.')).toBeInTheDocument());

    apiMock.artists.mockResolvedValue([]);
    view.rerender(<BrowseView {...props} openGenreRequest={{ genre: ' Jazz ', token: 2 }} />);
    await waitFor(() => expect(apiMock.artists).toHaveBeenLastCalledWith(expect.objectContaining({
      genres: ['Jazz'],
    })));
    expect(screen.getByText('No artists found.')).toBeInTheDocument();
  });

  // -- Artist consolidation (merge/unmerge) — see
  // wip/artist-consolidation-implementation-plan.md.

  it('hides the Select entry point without canEditMetadata', async () => {
    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" />,
    );
    await screen.findByText('Artist One');
    expect(screen.queryByRole('button', { name: 'Select' })).not.toBeInTheDocument();
  });

  it('lets a permitted user select 2 artists and merge them', async () => {
    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Madonna', track_count: 2, album_count: 1, rating: null },
      { id: '2', name: 'Madonna Ciccone', track_count: 1, album_count: 1, rating: null },
    ]);
    apiMock.mergeArtists.mockResolvedValue({ id: '1', name: 'Madonna', track_count: 3, album_count: 2 });

    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" canEditMetadata />,
    );
    await screen.findByText('Madonna');

    fireEvent.click(screen.getByRole('button', { name: 'Select' }));
    fireEvent.click(screen.getByText('Madonna'));
    fireEvent.click(screen.getByText('Madonna Ciccone'));
    expect(screen.getByText('2 artists selected')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Merge 2 Artists' }));
    expect(screen.getByText('Merge 2 artists into one')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Merge Artists' }));
    await waitFor(() => expect(apiMock.mergeArtists).toHaveBeenCalledWith(['1', '2'], 'Madonna', '1'));
    // Exits select mode and refetches the artist list.
    await waitFor(() => expect(apiMock.artists).toHaveBeenCalledTimes(2));
    expect(screen.queryByText('Merge 2 artists into one')).not.toBeInTheDocument();
  });

  it('shows "Merged from" on a merged artist and unmerges via the modal', async () => {
    apiMock.getArtistMergeInfo.mockResolvedValue({
      merged: true,
      members: [{ id: 'm1', original_name: 'Madonna Ciccone', album_count: 1, track_count: 1 }],
    });
    apiMock.unmergeArtist.mockResolvedValue({ master: { id: '1', name: 'Artist One' }, new_artist_ids: ['n1'] });

    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" canEditMetadata />,
    );
    fireEvent.click(await screen.findByText('Artist One'));

    expect(await screen.findByText('Madonna Ciccone')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Unmerge' }));
    expect(screen.getByText('Unmerge Artist One')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Unmerge Selected/ }));
    await waitFor(() => expect(apiMock.unmergeArtist).toHaveBeenCalledWith('1', ['m1']));
  });

  it('hides Select on the Albums tab even when canEditMetadata is true', async () => {
    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" canEditMetadata />,
    );
    await screen.findByText('Artist One');
    expect(screen.getByRole('button', { name: 'Select' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /^Albums/ }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Select' })).not.toBeInTheDocument());
  });

  it('disables selecting the Various Artists compilation entry', async () => {
    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Madonna', track_count: 2, album_count: 1, rating: null },
      { id: 'va', name: 'Various Artists', track_count: 40, album_count: 5, rating: null },
    ]);

    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" canEditMetadata />,
    );
    await screen.findByText('Various Artists');
    fireEvent.click(screen.getByRole('button', { name: 'Select' }));

    fireEvent.click(screen.getByText('Various Artists'));
    expect(screen.queryByText('1 artist selected')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('Madonna'));
    expect(screen.getByText('1 artist selected')).toBeInTheDocument();
  });

  it('shows the lock badge on a metadata-locked artist tile', async () => {
    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: null, metadata_locked: 1 },
    ]);
    const { container } = render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" />,
    );
    await screen.findByText('Artist One');
    expect(container.querySelector('[title="Custom metadata — protected from auto-scan"]')).toBeInTheDocument();
  });

  it('shows the pending online-match state and locks it on demand', async () => {
    apiMock.artists.mockResolvedValue([
      { id: '1', name: 'Artist One', track_count: 2, album_count: 1, rating: null, identity_lock_pending: 1 },
    ]);
    apiMock.lockArtistIdentity.mockResolvedValue({
      id: '1', name: 'Artist One', track_count: 2, album_count: 1, metadata_locked: 1, identity_lock_pending: 0,
    });

    render(
      <BrowseView libraries={[]} playTrack={vi.fn()} playAlbumInVinylMode={vi.fn()} addToQueue={vi.fn()} lastfmKey="" canEditMetadata />,
    );
    fireEvent.click(await screen.findByText('Artist One'));

    expect(await screen.findByText('Matching artist metadata online…')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Lock now' }));
    await waitFor(() => expect(apiMock.lockArtistIdentity).toHaveBeenCalledWith('1'));
    await waitFor(() => expect(apiMock.artist).toHaveBeenCalledWith('1'));
  });
});

// ─── Root album windowing (Phase 2) ────────────────────────────────────────────
// AlbumGrid/AlbumList render their tile/row content identically whether windowed
// or not (same renderTile/renderRow), so sorting, filtering, rating, keyboard
// activation, hover actions, and context menus are already covered by the
// unwindowed-mode tests above (jsdom never reports a real container size, so
// every test above exercises the unwindowed fallback path). These tests target
// only the new windowing-specific mechanics: row/column math, the scrolled
// mount window, alpha-jump/anchor math, focus preservation, and resize
// handling — see wip/large-dataset-album-artwork-performance-plan.md §7.

function makeAlbumFixtures(count: number): Album[] {
  return Array.from({ length: count }, (_, i) => {
    const letter = String.fromCharCode(65 + (i % 26)); // A..Z cycling
    return {
      id: String(i),
      title: `${letter} Album ${String(i).padStart(4, '0')}`,
      artist: 'Fixture Artist',
      album_artist: 'Fixture Artist',
      year: 2020,
      genre: 'Rock',
      track_count: 10,
      total_duration: 2400,
      rating: null,
    } as Album;
  }).sort((a, b) => a.title.localeCompare(b.title));
}

class MockResizeObserver {
  static instances: MockResizeObserver[] = [];
  callback: ResizeObserverCallback;
  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    MockResizeObserver.instances.push(this);
  }
  observe() {}
  unobserve() {}
  disconnect() {}
  trigger() {
    this.callback([] as unknown as ResizeObserverEntry[], this as unknown as ResizeObserver);
  }
}

describe('Root album windowing (Phase 2)', () => {
  let widthValue = 928; // 4 columns at the 160px/12px-gap/16px-padding grid math
  let heightValue = 600;

  beforeEach(() => {
    MockResizeObserver.instances = [];
    vi.stubGlobal('ResizeObserver', MockResizeObserver);
    Element.prototype.scrollTo = vi.fn();
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get() { return widthValue; },
    });
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get() { return heightValue; },
    });
  });

  afterEach(() => {
    widthValue = 928;
    heightValue = 600;
    vi.unstubAllGlobals();
    // @ts-expect-error restoring the real prototype descriptors after patching them above
    delete HTMLElement.prototype.clientWidth;
    // @ts-expect-error restoring the real prototype descriptors after patching them above
    delete HTMLElement.prototype.clientHeight;
  });

  it('AlbumGrid mounts only the calculated window plus overscan for a 9,000-album fixture', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist alphabeticalJump />,
    );
    const tiles = container.querySelectorAll('[role="button"]');
    expect(tiles.length).toBeGreaterThan(0);
    expect(tiles.length).toBeLessThan(50);
  });

  it('AlbumList mounts only the calculated window plus overscan for a 9,000-album fixture', () => {
    const albums = makeAlbumFixtures(9000);
    render(
      <AlbumList albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} alphabeticalJump />,
    );
    // Fixed 46px rows over a 600px viewport with overscan: comfortably under 50.
    expect(screen.getAllByText(/Album \d{4}/).length).toBeLessThan(50);
  });

  it('scroll changes replace the rendered window without mounting all albums', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );
    const lastAlbumTitle = albums[albums.length - 1].title;
    expect(screen.queryByTitle(lastAlbumTitle)).not.toBeInTheDocument();

    const scrollContainer = container.querySelector('[role="button"]')!.parentElement!.parentElement as HTMLElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 999999 });
    fireEvent.scroll(scrollContainer);

    // Still bounded to a small window after scrolling to the bottom.
    const tiles = container.querySelectorAll('[role="button"]');
    expect(tiles.length).toBeGreaterThan(0);
    expect(tiles.length).toBeLessThan(50);
  });

  it('alphabetical jump reaches the correct calculated row', () => {
    const albums = makeAlbumFixtures(9000);
    render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist alphabeticalJump />,
    );
    const scrollToMock = Element.prototype.scrollTo as ReturnType<typeof vi.fn>;
    scrollToMock.mockClear();

    fireEvent.click(screen.getByRole('button', { name: 'Jump to M' }));

    expect(scrollToMock).toHaveBeenCalled();
    const call = scrollToMock.mock.calls[0][0] as { top: number };
    expect(call.top).toBeGreaterThan(0);
  });

  it('root anchor/scroll restoration positions the container without any DOM measurement', () => {
    const albums = makeAlbumFixtures(9000);
    const targetId = albums[500].id;
    const { container } = render(
      <AlbumGrid
        albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()}
        showArtist initialAnchorId={targetId}
      />,
    );
    const scrollContainer = container.querySelector('[role="button"]')!.parentElement!.parentElement as HTMLElement;
    expect(scrollContainer.scrollTop).toBeGreaterThan(0);
  });

  it('keyboard focus is not left on an unmounted card after scrolling', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );
    const firstTile = container.querySelector('[role="button"]') as HTMLElement;
    firstTile.focus();
    expect(document.activeElement).toBe(firstTile);

    const scrollContainer = firstTile.parentElement!.parentElement as HTMLElement;
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 999999 });
    fireEvent.scroll(scrollContainer);

    // The focused tile stays mounted (and focused) even though it scrolled out
    // of the rendered window.
    expect(document.activeElement).toBe(firstTile);
    expect(document.body.contains(firstTile)).toBe(true);
  });

  it('responsive column changes retain the visible top album', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );
    const scrollContainer = container.querySelector('[role="button"]')!.parentElement!.parentElement as HTMLElement;
    // Scroll partway down at the initial (4-column) width.
    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 2000 });
    fireEvent.scroll(scrollContainer);
    const topTitleBefore = container.querySelector('[title]')?.getAttribute('title');
    expect(topTitleBefore).toBeTruthy();

    // Narrow to 2 columns; the previously-visible album should still be present.
    widthValue = 480;
    MockResizeObserver.instances.forEach((instance) => instance.trigger());

    expect(screen.getByTitle(topTitleBefore!)).toBeInTheDocument();
  });

  it('AlbumGrid renders correctly with showArtist off and hybrid-preview styling on', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} hybridPreview />,
    );
    const firstTile = container.querySelector('[role="button"]') as HTMLElement;
    expect(firstTile).toBeInTheDocument();
    // No artist link rendered when showArtist is off.
    expect(container.querySelector('[aria-label^="Open artist"]')).not.toBeInTheDocument();

    fireEvent.mouseEnter(firstTile);
    fireEvent.mouseLeave(firstTile);
    firstTile.focus();
    firstTile.blur();
  });

  it('AlbumList alphabetical jump reaches the correct calculated row', () => {
    const albums = makeAlbumFixtures(9000);
    render(<AlbumList albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} alphabeticalJump />);
    const scrollToMock = Element.prototype.scrollTo as ReturnType<typeof vi.fn>;
    scrollToMock.mockClear();

    fireEvent.click(screen.getByRole('button', { name: 'Jump to M' }));

    expect(scrollToMock).toHaveBeenCalled();
    const call = scrollToMock.mock.calls[0][0] as { top: number };
    expect(call.top).toBeGreaterThan(0);
  });

  it('AlbumList anchor restoration positions the container without any DOM measurement', () => {
    const albums = makeAlbumFixtures(9000);
    const targetId = albums[500].id;
    const { container } = render(
      <AlbumList albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} initialAnchorId={targetId} />,
    );
    const scrollContainer = container.firstElementChild!.firstElementChild as HTMLElement;
    expect(scrollContainer.scrollTop).toBeGreaterThan(0);
  });

  it('AlbumList keyboard focus is not left on an unmounted row after scrolling', () => {
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumList albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} />,
    );
    const scrollContainer = container.firstElementChild!.firstElementChild as HTMLElement;
    const firstRow = scrollContainer.querySelector('div > div') as HTMLElement;
    fireEvent.mouseEnter(firstRow);
    fireEvent.mouseLeave(firstRow);

    Object.defineProperty(scrollContainer, 'scrollTop', { configurable: true, value: 999999 });
    fireEvent.scroll(scrollContainer);

    // Bounded window after scrolling to the bottom (list stays responsive).
    expect(screen.getAllByText(/Album \d{4}/).length).toBeLessThan(50);
  });

  it('AlbumGrid self-corrects when the first synchronous measurement reads a too-small width', () => {
    // Belt-and-suspenders case: even once the container is attached and
    // measured, the very first synchronous read could in principle land
    // before an ancestor's layout has fully settled. A one-time
    // requestAnimationFrame re-check after the browser has committed layout
    // self-heals that. (The actual reported "garbled near-zero-width column"
    // bug had a different, more direct cause — see the "never attaches to a
    // real container on the render that has data" test below.)
    let rafCallback: FrameRequestCallback | null = null;
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      rafCallback = cb;
      return 1;
    });
    vi.stubGlobal('cancelAnimationFrame', () => {});

    widthValue = 0; // ancestor layout not yet settled at the moment of mount
    const albums = makeAlbumFixtures(9000);
    const { container } = render(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );

    const firstTile = container.querySelector('[role="button"]') as HTMLElement;
    expect(firstTile.style.width).toBe('0px');

    // Layout settles a frame later — the queued rAF re-check must catch it.
    widthValue = 928;
    expect(rafCallback).not.toBeNull();
    act(() => {
      rafCallback!(0);
    });

    const correctedTile = container.querySelector('[role="button"]') as HTMLElement;
    expect(parseFloat(correctedTile.style.width)).toBeGreaterThan(100);
  });

  it('AlbumGrid measures correctly once real data arrives after an initial empty render', () => {
    // Regression test for the actual reported bug: root Browse fetches album
    // data asynchronously, so AlbumGrid's very first mount often has
    // `albums={[]}` — which hits the early return (`if (!albums.length)
    // return <div>No albums found</div>`) *after* the component's hooks have
    // already run. `useContainerSize`'s measurement effect therefore ran
    // once against a `ref.current` that was never attached to a real DOM
    // node (that branch never rendered the scroll container at all), and —
    // because a plain `useRef` object's identity never changes — a
    // `useLayoutEffect` keyed on it does not run again even once real data
    // arrives on a later render and the container finally mounts. The fix
    // routes attachment through `useState` (`attachRef`) so "the element
    // showed up" is a real dependency change the effect reacts to.
    const { container, rerender } = render(
      <AlbumGrid albums={[]} loading={true} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );
    // Nothing to measure yet — the "No albums found"/"Loading..." branch,
    // no scroll container in the DOM at all.
    expect(container.querySelector('[role="button"]')).toBeNull();

    const albums = makeAlbumFixtures(9000);
    rerender(
      <AlbumGrid albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} showArtist />,
    );

    const tile = container.querySelector('[role="button"]') as HTMLElement;
    expect(tile).not.toBeNull();
    expect(parseFloat(tile.style.width)).toBeGreaterThan(100);
  });

  it('AlbumList measures correctly once real data arrives after an initial empty render', () => {
    const { container, rerender } = render(
      <AlbumList albums={[]} loading={true} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} />,
    );
    expect(screen.queryByText(/Album \d{4}/)).toBeNull();

    const albums = makeAlbumFixtures(9000);
    rerender(
      <AlbumList albums={albums} loading={false} onSelect={vi.fn()} onPlay={vi.fn()} onQueue={vi.fn()} />,
    );

    // A real measured height (600px fixture) yields ~20 visible rows at
    // 46px/row; a still-stuck-at-0 measurement (the bug) would clamp to only
    // the overscan (~7) regardless of the actual viewport. Bounded above by
    // 50 either way, so the lower bound is what actually distinguishes them.
    const rendered = screen.getAllByText(/Album \d{4}/).length;
    expect(rendered).toBeGreaterThan(10);
    expect(rendered).toBeLessThan(50);
  });
});

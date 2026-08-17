import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';

function jsonResponse(body: unknown = {}) {
  return {
    ok: true,
    status: 200,
    headers: { get: () => 'application/json' },
    json: vi.fn().mockResolvedValue(body),
    text: vi.fn().mockResolvedValue(''),
  } as unknown as Response;
}

describe('API public endpoint contract', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(jsonResponse())));
  });

  it('issues valid requests for every public REST wrapper', async () => {
    const calls: Array<() => unknown> = [
      () => api.libraries.list(),
      () => api.libraries.add('D:\\Music', 'Music'),
      () => api.libraries.add(['D:\\One', 'D:\\Two']),
      () => api.libraries.rename('lib-1', 'Renamed'),
      () => api.libraries.remove('lib-1'),
      () => api.libraries.addFolder('lib-1', 'D:\\More'),
      () => api.libraries.removeFolder('lib-1', 'folder-1'),
      () => api.libraries.scan('lib-1'),
      () => api.libraries.scanJobs('lib-1'),
      () => api.scanJobs.get('job-1'),
      () => api.scanJobs.active(),
      () => api.search({ q: 'song', page: 2, include_total: true }),
      () => api.autoDjTracks({ genres: [' Rock ', '', 'Pop'], library_id: 'lib-1', limit: 5 }),
      () => api.artists(),
      () => api.artists('lib-1'),
      () => api.artists({ library_ids: ['lib-1', ' '], genres: ['Rock'], sonic_fingerprint_only: true }),
      () => api.artistBrowsePage({ library_id: 'lib-1', q: 'a', paged: undefined } as any),
      () => api.albums({ artist_id: 'artist-1', library_ids: ['lib-1'], genres: ['Rock'], sonic_fingerprint_only: true }),
      () => api.latestAlbums(),
      () => api.homeTopRated(),
      () => api.homeGenres(),
      () => api.albumTracks('album-1', ['lib-1']),
      () => api.albumTracksByGroup('Title', null, ['lib-1']),
      () => api.albumCover('album-1'),
      () => api.refreshAlbumCover('album-1'),
      () => api.artistAlbums('artist-1', ['lib-1']),
      () => api.artistAppearsOn('artist-1', ['lib-1']),
      () => api.artistSimilar('artist-1'),
      () => api.resolveArtistReleaseTypes('artist-1'),
      () => api.artistRadio('artist-1'),
      () => api.genres(),
      () => api.stats(),
      () => api.recentlyPlayed(),
      () => api.topPlayedTracks(),
      () => api.mostPlayedArtists(),
      () => api.artist('artist-1'),
      () => api.setArtistRating('artist-1', 4),
      () => api.refreshArtistPhoto('artist-1'),
      () => api.album('album-1'),
      () => api.setAlbumRating('album-1', null),
      () => api.track('track-1'),
      () => api.setTrackRating('track-1', 5),
      () => api.trackEqProfile('track-1'),
      () => api.trackLyrics('track-1'),
      () => api.markTrackPlayed('track-1'),
      () => api.trackWaveform('track-1'),
      () => api.generateTrackWaveform('track-1'),
      () => api.fsBrowse(),
      () => api.fsBrowse('D:\\Music'),
      () => api.fsMkdir('D:\\Music', 'New'),
      () => api.debugTestPath('D:\\Music'),
      () => api.systemStatus(),
      () => api.systemSelectFolder('D:\\'),
      () => api.systemSetup('D:\\Data'),
      () => api.systemSwitchDb('D:\\Other'),
      () => api.playbackSettings(),
      () => api.settings.get(),
      () => api.settings.update({ theme: 'dark' }),
      () => api.schedules.list(),
      () => api.schedules.get('lib-1'),
      () => api.schedules.upsert('lib-1', true, 12),
      () => api.schedules.remove('lib-1'),
      () => api.waveforms.status(),
      () => api.waveforms.runMap(),
      () => api.bpm.status(),
      () => api.bpm.run(),
      () => api.integrations.spotifyTest(),
      () => api.integrations.geniusTest('client', 'secret'),
      () => api.integrations.lyrics({ artist: 'Artist', title: 'Song' }),
      () => api.integrations.metadataSearch({ artist: 'Artist', album: 'Album' }),
      () => api.updateAlbumMetadata('album-1', { title: 'Title' }),
      () => api.updateAlbumMetadata('album-1', { title: 'Title' }, true),
      () => api.updateArtistMetadata('artist-1', { name: 'Artist' }),
      () => api.updateArtistMetadata('artist-1', { name: 'Artist' }, true),
      () => api.uploadAlbumArtwork('album-1', 'base64', 'image/jpeg'),
      () => api.uploadArtistArtwork('artist-1', 'base64', 'image/jpeg'),
      () => api.playlists.list(),
      () => api.playlists.get('playlist-1'),
      () => api.playlists.create('Mix', 'Description'),
      () => api.playlists.update('playlist-1', 'Mix', 'Description', 1),
      () => api.playlists.remove('playlist-1'),
      () => api.playlists.tracks('playlist-1'),
      () => api.playlists.addTrack('playlist-1', 'track-1'),
      () => api.playlists.addTracks('playlist-1', ['track-1', 'track-2']),
      () => api.playlists.removeTrack('playlist-1', 'track-1'),
      () => api.playlists.reorder('playlist-1', ['track-2', 'track-1']),
      () => api.playlists.saveTrackProgress('playlist-1', 'track-1', 42),
      () => api.boogiemix.createJob('playlist-1', 'long_build', 'high_quality', 45),
      () => api.boogiemix.getJob('job-1'),
      () => api.boogiemix.cancelJob('job-1'),
      () => api.boogiemix.deepAnalysisStatus(),
      () => api.boogiemix.queuePlaylistDeepAnalysis('playlist-1'),
      () => api.boogiemix.playlistDeepAnalysisProgress('playlist-1'),
      () => api.boogiemix.queueLibraryDeepAnalysis('lib-1'),
      () => api.boogiemix.pauseDeepAnalysisBackground(),
      () => api.boogiemix.resumeDeepAnalysisBackground(),
      () => api.boogiemix.clearDeepAnalysisCache(),
      () => api.boogiemix.listOutputs('playlist-1'),
      () => api.dlna.status(),
      () => api.dlna.restart(),
      () => api.crossfade.config('artist', 'artist-1'),
      () => api.crossfade.overrides('album'),
      () => api.crossfade.upsertOverride({ entity_type: 'artist', entity_id: 'artist-1', mode: 'fixed', duration: 8 }),
      () => api.crossfade.removeOverride('artist', 'artist-1'),
      () => api.auth.getLoginUsers(),
      () => api.auth.login('user-1', '1234', true),
      () => api.auth.logout(),
      () => api.auth.me(),
      () => api.userSettings.get(),
      () => api.userSettings.update({ theme: 'dark' }),
      () => api.userHistory(),
      () => api.lastfm.info('Artist', 'Album'),
      () => api.lastfm.info('Artist'),
      () => api.lastfm.topTracks('Artist'),
      () => api.admin.queues(),
      () => api.admin.providerUsage(),
      () => api.admin.cancelScanJob('job-1'),
      () => api.admin.cancelPostScanJob('job-1'),
      () => api.admin.failPostScanJob('job-1'),
      () => api.admin.retryPostScanJob('job-1'),
      () => api.admin.enqueuePostScanJob('lib-1', 'cache_album_images'),
      () => api.admin.users.list(),
      () => api.admin.users.create({ username: 'new', role: 'user' }),
      () => api.admin.users.remove('user-1'),
      () => api.admin.users.setPin('user-1', '1234'),
      () => api.admin.users.setPermissions('user-1', { canManageLibraries: true, canEditMetadata: false }),
    ];

    for (const invoke of calls) await invoke();

    expect(fetch).toHaveBeenCalledTimes(calls.length);
    const requests = vi.mocked(fetch).mock.calls.map(([url, options]) => ({
      url: String(url),
      method: options?.method ?? 'GET',
    }));
    expect(requests).toContainEqual(expect.objectContaining({ url: expect.stringContaining('/api/libraries') }));
    expect(requests).toContainEqual(expect.objectContaining({ method: 'POST' }));
    expect(requests).toContainEqual(expect.objectContaining({ method: 'PUT' }));
    expect(requests).toContainEqual(expect.objectContaining({ method: 'PATCH' }));
    expect(requests).toContainEqual(expect.objectContaining({ method: 'DELETE' }));
    expect(requests.every(({ url }) => new URL(url, window.location.href).pathname.startsWith('/api/'))).toBe(true);
  });

  it('builds versioned artwork, stream, playlist export, and mix-output URLs', () => {
    document.cookie = 'bb_stream_direct=1; path=/';
    expect(api.albumArtUrl('album-1', 800, 3.9)).toContain('/api/albums/album-1/art?size=800&v=3');
    expect(api.artistPhotoUrl('artist-1')).toBe('/api/artists/artist-1/photo?size=300');
    expect(api.trackStreamUrl('track-1')).toContain('noTranscode=1');
    expect(api.playlists.exportM3uUrl('playlist-1')).toBe('/api/playlists/playlist-1/export.m3u');
    expect(api.boogiemix.outputDownloadUrl('output-1')).toBe('/api/boogiemix/outputs/output-1/file');
  });
});

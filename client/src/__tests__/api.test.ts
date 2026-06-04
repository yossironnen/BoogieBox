/**
 * Tests Api.Test behavior for BoogieBox regressions.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { api, encodeGenresParam, getStreamDirect, setStreamDirect } from '../api';

// ── Cookie helpers ───────────────────────────────────────────────────────────

const clearStreamDirectCookie = () => {
  document.cookie = 'bb_stream_direct=; max-age=0; path=/';
};

describe('getStreamDirect / setStreamDirect', () => {
  beforeEach(clearStreamDirectCookie);
  afterEach(clearStreamDirectCookie);

  it('returns false when cookie is absent', () => {
    expect(getStreamDirect()).toBe(false);
  });

  it('returns true after setStreamDirect(true)', () => {
    setStreamDirect(true);
    expect(getStreamDirect()).toBe(true);
  });

  it('returns false after setStreamDirect(false) clears the cookie', () => {
    setStreamDirect(true);
    setStreamDirect(false);
    expect(getStreamDirect()).toBe(false);
  });

  it('setStreamDirect(true) twice is idempotent', () => {
    setStreamDirect(true);
    setStreamDirect(true);
    expect(getStreamDirect()).toBe(true);
  });
});

// ── Pure URL helpers ────────────────────────────────────────────────────────

describe('api.trackStreamUrl', () => {
  beforeEach(clearStreamDirectCookie);
  afterEach(clearStreamDirectCookie);

  it('returns the standard stream URL when transcoding is enabled (default)', () => {
    expect(api.trackStreamUrl('42')).toBe('/api/tracks/42/stream');
  });

  it('appends noTranscode=1 when streamDirect is enabled', () => {
    setStreamDirect(true);
    expect(api.trackStreamUrl('42')).toBe('/api/tracks/42/stream?noTranscode=1');
  });

  it('handles string UUID-compatible ids', () => {
    expect(api.trackStreamUrl('1')).toBe('/api/tracks/1/stream');
    expect(api.trackStreamUrl('0195f3e5-2222-7222-8222-222222222222')).toBe('/api/tracks/0195f3e5-2222-7222-8222-222222222222/stream');
  });

  it('reverts to plain URL after disabling streamDirect', () => {
    setStreamDirect(true);
    setStreamDirect(false);
    expect(api.trackStreamUrl('7')).toBe('/api/tracks/7/stream');
  });
});

describe('api.trackLyrics', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('normalizes syncedLyrics from the server to synced lines', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({
      lyrics: 'Line one\nLine two',
      source: 'cache',
      syncedLyrics: [{ time: 0, text: 'Line one' }],
    }));

    const result = await api.trackLyrics('77');
    const url = vi.mocked(fetch).mock.calls[0][0] as string;

    expect(url).toContain('/api/tracks/77/lyrics');
    expect(result.synced).toEqual([{ time: 0, text: 'Line one' }]);
  });
});

// ── Fetch-based helpers (mocked) ─────────────────────────────────────────────

function okJson(data: unknown) {
  return Promise.resolve({
    ok: true,
    json: () => Promise.resolve(data),
  } as Response);
}

function errJson(status: number, error: string) {
  return Promise.resolve({
    ok: false,
    status,
    json: () => Promise.resolve({ error }),
  } as Response);
}

describe('api.libraries', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('list() calls GET /api/libraries', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    const result = await api.libraries.list();
    const url = vi.mocked(fetch).mock.calls[0][0] as string;
    expect(url).toContain('/api/libraries');
    expect(result).toEqual([]);
  });

  it('add() calls POST /api/libraries with folders and name', async () => {
    const lib = { id: '1', path: '/music', name: 'Music' };
    vi.mocked(fetch).mockReturnValue(okJson(lib));
    const result = await api.libraries.add(['/music', '/more-music'], 'Music');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries');
    expect(opts.method).toBe('POST');
    expect(JSON.parse(opts.body as string)).toMatchObject({ path: '/music', folders: ['/music', '/more-music'], name: 'Music' });
    expect(result).toEqual(lib);
  });

  it('addFolder() calls POST /api/libraries/:id/folders', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ id: '1', folders: [] }));
    await api.libraries.addFolder('7', '/movies/extra');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries/7/folders');
    expect(opts.method).toBe('POST');
    expect(JSON.parse(opts.body as string)).toMatchObject({ path: '/movies/extra' });
  });

  it('removeFolder() calls DELETE /api/libraries/:id/folders/:folderId', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ id: '7', folders: [] }));
    await api.libraries.removeFolder('7', '11');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries/7/folders/11');
    expect(opts.method).toBe('DELETE');
  });

  it('rename() calls PUT /api/libraries/:id with the new name', async () => {
    const lib = { id: '7', path: '/movies', name: 'Movies 2' };
    vi.mocked(fetch).mockReturnValue(okJson(lib));
    const result = await api.libraries.rename('7', 'Movies 2');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries/7');
    expect(opts.method).toBe('PUT');
    expect(JSON.parse(opts.body as string)).toMatchObject({ name: 'Movies 2' });
    expect(result).toEqual(lib);
  });

  it('remove() calls DELETE /api/libraries/:id', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ ok: true }));
    await api.libraries.remove('7');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries/7');
    expect(opts.method).toBe('DELETE');
  });

  it('throws when response is not ok', async () => {
    vi.mocked(fetch).mockReturnValue(errJson(500, 'Internal Server Error'));
    await expect(api.libraries.list()).rejects.toThrow('Internal Server Error');
  });

  it('scan() calls POST /api/libraries/:id/scan', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ jobId: '3' }));
    const result = await api.libraries.scan('2');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/libraries/2/scan');
    expect(opts.method).toBe('POST');
    expect(result).toEqual({ jobId: '3' });
  });

  it('scan() preserves UUID library and job ids', async () => {
    const libraryId = '0195f3e5-1111-7111-8111-111111111111';
    const jobId = '0195f3e5-4444-7444-8444-444444444444';
    vi.mocked(fetch).mockReturnValue(okJson({ jobId }));
    const result = await api.libraries.scan(libraryId);
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain(`/api/libraries/${libraryId}/scan`);
    expect(result).toEqual({ jobId });
  });

  it('scanJobs.active() calls GET /api/scan-jobs/active', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    const result = await api.scanJobs.active();
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain('/api/scan-jobs/active');
    expect(result).toEqual([]);
  });
});

describe('api.search', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('passes query params in the URL', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ tracks: [], total: 0, page: 1, limit: 50 }));
    await api.search({ q: 'beethoven', page: 2, limit: 20 });
    const url = vi.mocked(fetch).mock.calls[0][0] as string;
    expect(url).toContain('q=beethoven');
    expect(url).toContain('page=2');
    expect(url).toContain('limit=20');
  });

  it('omits undefined params', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ tracks: [], total: 0, page: 1, limit: 50 }));
    await api.search({ q: 'jazz' });
    const url = vi.mocked(fetch).mock.calls[0][0] as string;
    expect(url).toContain('q=jazz');
    expect(url).not.toContain('undefined');
  });

  it('passes mobile search tuning params in the URL', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ tracks: [], total: 0, page: 1, limit: 20, artists: [], albums: [], top_results: [], hasMore: false }));
    await api.search({
      q: 'beatles',
      limit: 20,
      page: 1,
      search_mode: 'mobile_omni',
      mode: 'music',
      include_artists: false,
      include_albums: false,
      include_total: false,
      sort: 'relevance',
    });
    const url = new URL(vi.mocked(fetch).mock.calls[0][0] as string);
    expect(url.searchParams.get('search_mode')).toBe('mobile_omni');
    expect(url.searchParams.get('mode')).toBe('music');
    expect(url.searchParams.get('include_artists')).toBe('false');
    expect(url.searchParams.get('include_albums')).toBe('false');
    expect(url.searchParams.get('include_total')).toBe('false');
    expect(url.searchParams.get('sort')).toBe('relevance');
  });
});

describe('encodeGenresParam', () => {
  it('returns undefined for empty input', () => {
    expect(encodeGenresParam()).toBeUndefined();
    expect(encodeGenresParam([])).toBeUndefined();
    expect(encodeGenresParam(['', '  '])).toBeUndefined();
  });

  it('returns comma-separated values with whitespace trimmed', () => {
    expect(encodeGenresParam([' Rock ', 'Jazz'])).toBe('Rock,Jazz');
  });
});

describe('api browse genre filters', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('passes genres for artists list requests', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    await api.artists({ genres: ['Rock', 'Jazz'] });
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/artists');
    expect(url.searchParams.get('genres')).toBe('Rock,Jazz');
  });

  it('passes genres for albums list requests', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    await api.albums({ group_by: 'album_artist', genres: ['Electronic'] });
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/albums');
    expect(url.searchParams.get('group_by')).toBe('album_artist');
    expect(url.searchParams.get('genres')).toBe('Electronic');
  });

  it('passes UUID library filters for paged artist browse requests', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ items: [], total: 0, limit: 50, offset: 0, has_more: false }));
    await api.artistBrowsePage({
      library_id: '0195f3e5-1111-7111-8111-111111111111',
      library_ids: [
        '0195f3e5-1111-7111-8111-111111111111',
        '0195f3e5-2222-7222-8222-222222222222',
      ],
      view: 'summary',
    });
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/artists');
    expect(url.searchParams.get('library_id')).toBe('0195f3e5-1111-7111-8111-111111111111');
    expect(url.searchParams.get('library_ids')).toBe('0195f3e5-1111-7111-8111-111111111111,0195f3e5-2222-7222-8222-222222222222');
    expect(url.searchParams.get('view')).toBe('summary');
    expect(url.searchParams.get('paged')).toBe('1');
  });
});

describe('api.autoDjTracks', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('calls auto-dj endpoint with genres, library, and limit params', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ tracks: [] }));
    await api.autoDjTracks({ genres: ['Rock', 'Jazz'], library_id: '7', limit: 120 });
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/auto-dj/tracks');
    expect(url.searchParams.get('genres')).toBe('Rock,Jazz');
    expect(url.searchParams.get('library_id')).toBe('7');
    expect(url.searchParams.get('limit')).toBe('120');
  });
});

describe('api.artistAppearsOn', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('calls GET /api/artists/:id/appears-on', async () => {
    const albums = [{ id: '1', title: 'Now 90s', album_artist: 'Various Artists' }];
    vi.mocked(fetch).mockReturnValue(okJson(albums));
    const result = await api.artistAppearsOn('5');
    const url = vi.mocked(fetch).mock.calls[0][0] as string;
    expect(url).toContain('/api/artists/5/appears-on');
    expect(result).toEqual(albums);
  });

  it('returns empty array when artist has no compilation appearances', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    const result = await api.artistAppearsOn('99');
    expect(result).toEqual([]);
  });
});

describe('api recently played', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('calls GET /api/tracks/recently-played with limit', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    await api.recentlyPlayed(10);
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/tracks/recently-played');
    expect(url.searchParams.get('limit')).toBe('10');
  });

  it('calls POST /api/tracks/:id/played', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ ok: true }));
    const result = await api.markTrackPlayed('7');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/tracks/7/played');
    expect(opts.method).toBe('POST');
    expect(result).toEqual({ ok: true });
  });

  it('calls GET /api/tracks/top-played with limit', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    await api.topPlayedTracks(5);
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/tracks/top-played');
    expect(url.searchParams.get('limit')).toBe('5');
  });

  it('calls GET /api/artists/most-played with limit', async () => {
    vi.mocked(fetch).mockReturnValue(okJson([]));
    await api.mostPlayedArtists(8);
    const rawUrl = vi.mocked(fetch).mock.calls[0][0] as string;
    const url = new URL(rawUrl, window.location.href);
    expect(url.pathname).toContain('/api/artists/most-played');
    expect(url.searchParams.get('limit')).toBe('8');
  });
});

describe('api waveform endpoints', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('calls GET /api/tracks/:id/waveform', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ status: 'ready', waveform: { trackId: '1', points: [1, 2] } }));
    const result = await api.trackWaveform('1');
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain('/api/tracks/1/waveform');
    expect(result.status).toBe('ready');
  });

  it('calls POST /api/tracks/:id/waveform/generate', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ status: 'ready', waveform: { trackId: '1', points: [1, 2] } }));
    const result = await api.generateTrackWaveform('1');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/tracks/1/waveform/generate');
    expect(opts.method).toBe('POST');
    expect(result.status).toBe('ready');
  });

  it('calls GET /api/waveforms/map/status', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ enabled: false, totalTracks: 10, mappedTracks: 6, missingTracks: 4 }));
    const result = await api.waveforms.status();
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain('/api/waveforms/map/status');
    expect(result.mappedTracks).toBe(6);
  });

  it('calls POST /api/waveforms/map/run', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ started: true, reason: 'manual', processed: 0, generated: 0, skipped: 0, errors: 0 }));
    const result = await api.waveforms.runMap();
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/waveforms/map/run');
    expect(opts.method).toBe('POST');
    expect(result.started).toBe(true);
  });
});

describe('api.playlists', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('update() calls PUT /api/playlists/:id', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ id: '7', name: 'Focus' }));
    const result = await api.playlists.update('7', 'Focus');
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/playlists/7');
    expect(opts.method).toBe('PUT');
    expect(result).toEqual({ id: '7', name: 'Focus' });
  });

  it('update() throws when server returns duplicate-name conflict', async () => {
    vi.mocked(fetch).mockReturnValue(errJson(409, 'A playlist with this name already exists'));
    await expect(api.playlists.update('7', 'Road Trip')).rejects.toThrow('A playlist with this name already exists');
  });
});

describe('api.integrations.spotifyTest', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('calls GET /api/integrations/spotify/test', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ ok: true }));
    const result = await api.integrations.spotifyTest();
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain('/api/integrations/spotify/test');
    expect(result).toEqual({ ok: true });
  });
});

describe('api.dlna', () => {
  beforeEach(() => { vi.stubGlobal('fetch', vi.fn()); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('status() calls GET /api/dlna/status', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ running: false, port: null, friendlyName: null }));
    const result = await api.dlna.status();
    const [url] = vi.mocked(fetch).mock.calls[0] as [string];
    expect(url).toContain('/api/dlna/status');
    expect(result).toEqual({ running: false, port: null, friendlyName: null });
  });

  it('restart() calls POST /api/dlna/restart', async () => {
    vi.mocked(fetch).mockReturnValue(okJson({ ok: true }));
    const result = await api.dlna.restart();
    const [url, opts] = vi.mocked(fetch).mock.calls[0] as [string, RequestInit];
    expect(url).toContain('/api/dlna/restart');
    expect(opts.method).toBe('POST');
    expect(result).toEqual({ ok: true });
  });
});


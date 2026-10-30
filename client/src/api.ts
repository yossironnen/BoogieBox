/**
 * Central browser API client for BoogieBox REST endpoints, auth-aware fetches,
 * artwork URL helpers, streaming preferences, and response normalization.
 */

import type {
  BackupHealth,
  BackupSettings,
  BackupSummary,
  BackupsOverview,
  PathRemap,
  PlaylistImportPreview,
  RestoreResult,
  Library,
  Track,
  Artist,
  ArtistBrowsePage,
  ArtistMergeInfo,
  UnmergeResult,
  SimilarArtistsResponse,
  ArtistRadioOptions,
  ArtistRadioOptionsSnapshot,
  ArtistRadioResponse,
  RadioMetadataStatus,
  Album,
  LatestAlbum,
  HomeTopRated,
  HomeGenreSummary,
  Genre,
  Stats,
  ScanJob,
  SearchResult,
  SortField,
  SortOrder,
  CrossfadeConfig,
  CrossfadeOverride,
  TrackWaveformLookupResponse,
  WaveformMapRunResult,
  WaveformMappingStatus,
  BpmAnalysisStatus,
  BpmBatchResult,
  MetadataSearchResult,
  ProviderSearchWarning,
  AuthUser,
  LoginUser,
  AdminUser,
  AdminQueueSnapshot,
  ProviderUsageSnapshot,
  HistoryEntry,
  LastFmInfo,
  Playlist,
  PlaylistSnapshot,
  PlaylistTrack,
  SmartPlaylistPreview,
  BoogieMixJob,
  BoogieMixOutput,
  BoogieMixDeepAnalysisStatus,
  ClientEntityId,
  AdminPostScanJobType,
  SonicFingerprint,
  TrackSection,
  StemWindow,
  TransitionWindow,
  IntroOutroRefined,
} from './types';
import type { EntityId } from './entityId';
import type { SubsonicCredentials } from './subsonic';
import type { FavouriteIds, FavouriteKind, Favourites } from './favourites';
import type { RadioStation, RadioStationInput } from './radioStations';
import type { ScrobbleAccountsResponse } from './scrobbling';
import type { SmartDefinition, SmartDefinitionView } from './smartPlaylistRules';
import { invalidateHomeData } from './homeCache';
import { notifyServerRestoring } from './restoreState';
import type {
  RemoteCommand,
  RemoteDiagnostics,
  RemoteFormatPolicy,
  RemoteLoadBody,
  RemoteQueueOp,
  RemoteSessionState,
  RenderersResponse,
  SessionInUseInfo,
} from './remote/remoteTypes';
import type { ActivitySnapshot, PlayerHeartbeat } from './activity/activityTypes';

type ApiEntityId = ClientEntityId;

const BASE = import.meta.env.VITE_API_URL || '';
/** Supported server-generated album artwork thumbnail sizes. */
export type AlbumArtSize = 300 | 800;

function withVersion(url: string, version?: number): string {
  if (!version || !Number.isFinite(version) || version <= 0) return url;
  const next = new URL(url, window.location.href);
  next.searchParams.set('v', String(Math.floor(version)));
  const isAbsolute = /^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(url) || url.startsWith('//');
  return isAbsolute ? next.toString() : `${next.pathname}${next.search}${next.hash}`;
}

// ─── Browser-local stream preference (cookie, per-device) ─────────────────────
// bb_stream_direct=1  →  client requests raw file bytes (no server transcoding)
// absent / 0          →  server transcodes FLAC/M4A/WMA/etc. to MP3 (default)

const STREAM_DIRECT_COOKIE = 'bb_stream_direct';

/** Returns the per-browser preference for direct audio streaming instead of transcoding. */
export function getStreamDirect(): boolean {
  try {
    return document.cookie.split(';').some(c => c.trim().startsWith(`${STREAM_DIRECT_COOKIE}=1`));
  } catch { return false; }
}

/** Persists the per-browser direct-streaming preference in a SameSite cookie. */
export function setStreamDirect(enabled: boolean): void {
  try {
    if (enabled) {
      document.cookie = `${STREAM_DIRECT_COOKIE}=1; max-age=31536000; path=/; SameSite=Lax`;
    } else {
      document.cookie = `${STREAM_DIRECT_COOKIE}=; max-age=0; path=/; SameSite=Lax`;
    }
  } catch {}
}

const FETCH_OPTS: RequestInit = { credentials: 'include' };

function describeNetworkError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error ?? '');
  if (/fetch failed|failed to fetch/i.test(message)) {
    return 'Network request failed. Check server logs/combined.log or logs/error.log for TMDb refresh details.';
  }
  return message || 'Network request failed';
}

async function readErrorMessage(res: Response): Promise<string> {
  const contentType = res.headers?.get?.('content-type') || '';
  if (contentType.includes('application/json')) {
    const body = await res.json().catch(() => ({}));
    if (res.status === 503 && body?.code === 'restoring') notifyServerRestoring();
    return body?.error || `Server error ${res.status}`;
  }
  if (!contentType && typeof res.json === 'function') {
    const body = await res.json().catch(() => ({}));
    return body?.error || `Server error ${res.status}`;
  }
  const text = await res.text().catch(() => '');
  const trimmed = text.trim();
  if (!trimmed) return `Server error ${res.status}`;
  if (trimmed.startsWith('<')) return `Server error ${res.status}`;
  return trimmed;
}

async function readJson<T>(res: Response): Promise<T> {
  const contentType = res.headers?.get?.('content-type') || '';
  if (!contentType && typeof res.json === 'function') {
    return res.json() as Promise<T>;
  }
  if (!contentType.includes('application/json')) {
    const text = await res.text().catch(() => '');
    throw new Error(text.trim().startsWith('<') || !text.trim() ? `Server error ${res.status}` : text.trim());
  }
  return res.json() as Promise<T>;
}

async function get<T>(path: string, params?: Record<string, string | number | boolean | undefined>): Promise<T> {
  const url = new URL(`${BASE}/api${path}`, window.location.href);
  if (params) {
    Object.entries(params).forEach(([k, v]) => {
      if (v !== undefined && v !== '') url.searchParams.set(k, String(v));
    });
  }
  let res: Response;
  try {
    res = await fetch(url.toString(), FETCH_OPTS);
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    throw new Error(await readErrorMessage(res));
  }
  return readJson<T>(res);
}

function homeMutation(path: string): void {
  if (/^\/(auth\/|system\/(?:setup|switch-db))/.test(path)) {
    invalidateHomeData(true);
  } else if (/^\/(?:libraries|playlists|artists|albums|tracks)(?:\/|$)/.test(path)) {
    invalidateHomeData();
  }
}

async function post<T>(path: string, body?: unknown): Promise<T> {
  if (/^\/(auth\/|system\/(?:setup|switch-db))/.test(path)) invalidateHomeData(true);
  let res: Response;
  try {
    res = await fetch(`${BASE}/api${path}`, {
      ...FETCH_OPTS,
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    throw new Error(await readErrorMessage(res));
  }
  const result = await readJson<T>(res);
  homeMutation(path);
  return result;
}

async function put<T>(path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api${path}`, {
      ...FETCH_OPTS,
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    throw new Error(await readErrorMessage(res));
  }
  const result = await readJson<T>(res);
  homeMutation(path);
  return result;
}

async function del<T>(path: string): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api${path}`, { ...FETCH_OPTS, method: 'DELETE' });
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    throw new Error(await readErrorMessage(res));
  }
  const result = await readJson<T>(res);
  homeMutation(path);
  return result;
}

async function patch<T>(path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api${path}`, {
      ...FETCH_OPTS,
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    throw new Error(await readErrorMessage(res));
  }
  const result = await readJson<T>(res);
  homeMutation(path);
  return result;
}

/** Playlist import failure; `code` is `too_big`, `too_long` or `empty` when the file is the problem. */
export class PlaylistImportError extends Error {
  constructor(message: string, readonly code: string | null) {
    super(message);
    this.name = 'PlaylistImportError';
  }
}

async function postPlaylistImportPreview(contentBase64: string, remaps: PathRemap[]): Promise<PlaylistImportPreview> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api/playlists/import/preview`, {
      ...FETCH_OPTS,
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ contentBase64, remaps }),
    });
  } catch (error) {
    throw new Error(describeNetworkError(error));
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    const body = await res.clone().json().catch(() => ({}));
    const code = typeof body?.code === 'string' ? body.code : res.status === 413 ? 'too_big' : null;
    throw new PlaylistImportError(await readErrorMessage(res), code);
  }
  return readJson<PlaylistImportPreview>(res);
}

/** Play On request failure that keeps the HTTP status and JSON body (409 carries the takeover details). */
export class RemoteApiError extends Error {
  constructor(message: string, readonly status: number, readonly code: string | null, readonly body: unknown) {
    super(message);
    this.name = 'RemoteApiError';
  }

  /** The 409 `session_in_use` details, when this is one. */
  get inUse(): SessionInUseInfo | null {
    return this.status === 409 && this.code === 'session_in_use' ? this.body as SessionInUseInfo : null;
  }
}

async function remoteRequest<T>(method: 'GET' | 'POST' | 'PATCH' | 'DELETE', path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api/remote${path}`, {
      ...FETCH_OPTS,
      method,
      ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
    });
  } catch (error) {
    throw new RemoteApiError(describeNetworkError(error), 0, null, null);
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    const data = await res.json().catch(() => null) as { error?: string; code?: string } | null;
    throw new RemoteApiError(data?.error || `Server error ${res.status}`, res.status, data?.code ?? null, data);
  }
  return readJson<T>(res);
}

const remoteUdn = (udn: string) => encodeURIComponent(udn);

/** Smart playlist request failure with the HTTP status and the server's `code` (`edit_conflict`, `busy`, …). */
export class SmartPlaylistApiError extends Error {
  constructor(message: string, readonly status: number, readonly code: string | null, readonly body: unknown) {
    super(message);
    this.name = 'SmartPlaylistApiError';
  }
}

async function smartRequest<T>(method: 'GET' | 'POST' | 'PUT', path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${BASE}/api${path}`, {
      ...FETCH_OPTS,
      method,
      signal,
      ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
    });
  } catch (error) {
    if (signal?.aborted) throw error;
    throw new SmartPlaylistApiError(describeNetworkError(error), 0, null, null);
  }
  if (!res.ok) {
    if (res.status === 401) invalidateHomeData(true);
    const data = await res.json().catch(() => null) as { error?: string; code?: string } | null;
    throw new SmartPlaylistApiError(data?.error || `Server error ${res.status}`, res.status, data?.code ?? null, data);
  }
  const result = await readJson<T>(res);
  if (method !== 'GET' && !path.endsWith('/smart-preview')) homeMutation(path);
  return result;
}

/** Encode Genres Param is part of this module's public API. */
export function encodeGenresParam(genres?: string[]): string | undefined {
  if (!genres?.length) return undefined;
  const cleaned = genres.map((genre) => genre.trim()).filter(Boolean);
  return cleaned.length ? cleaned.join(',') : undefined;
}

/** Encode Library Ids Param is part of this module's public API. */
export function encodeLibraryIdsParam(libraryIds?: ApiEntityId[]): string | undefined {
  if (!libraryIds?.length) return undefined;
  const cleaned = libraryIds
    .map((id) => String(id).trim())
    .filter(Boolean);
  return cleaned.length ? cleaned.join(',') : undefined;
}

export const api = {
  libraries: {
    list: () => get<Library[]>('/libraries'),
    add: (paths: string | string[], name?: string) => {
      const folders = Array.isArray(paths) ? paths : [paths];
      return post<Library>('/libraries', { path: folders[0], folders, name, libraryType: 'music' });
    },
    rename: (id: ApiEntityId, name: string) => put<Library>(`/libraries/${id}`, { name }),
    remove: (id: ApiEntityId) => del<{ ok: boolean }>(`/libraries/${id}`),
    addFolder: (id: ApiEntityId, path: string) => post<Library>(`/libraries/${id}/folders`, { path }),
    removeFolder: (id: ApiEntityId, folderId: ApiEntityId) => del<Library>(`/libraries/${id}/folders/${folderId}`),
    scan: (id: ApiEntityId) => post<{ jobId: ApiEntityId }>(`/libraries/${id}/scan`),
    scanJobs: (id: ApiEntityId) => get<ScanJob[]>(`/libraries/${id}/scan-jobs`),
    removeMissingTracks: (id: ApiEntityId) =>
      post<{ removed: number; unchecked: number }>(`/libraries/${id}/remove-missing-tracks`),
  },
  scanJobs: {
    get: (id: ApiEntityId) => get<ScanJob>(`/scan-jobs/${id}`),
    active: () => get<ScanJob[]>('/scan-jobs/active'),
  },
  search: (params: {
    q?: string; library_id?: ApiEntityId; genre?: string; year?: number;
    format?: string; sort?: SortField; order?: SortOrder; page?: number; limit?: number;
    search_mode?: 'default' | 'omni' | 'mobile_omni' | 'mobile_tracks';
    mode?: 'all' | 'music';
    include_artists?: boolean;
    include_albums?: boolean;
    include_total?: boolean;
    artist_rating_filter?: 'all' | 'rated' | 'unrated' | 'gte4' | 'gte3';
    artist_sort_field?: 'name' | 'rating';
    artist_sort_dir?: SortOrder;
    album_rating_filter?: 'all' | 'rated' | 'unrated' | 'gte4' | 'gte3';
    album_sort_field?: 'title' | 'year' | 'rating';
    album_sort_dir?: SortOrder;
    track_rating_filter?: 'all' | 'rated' | 'unrated' | 'gte4' | 'gte3';
    track_sort_mode?: 'default' | 'rating';
    track_rating_sort_dir?: SortOrder;
    sonic_fingerprint_only?: boolean;
  }) => get<SearchResult>('/search', params as any),
  /** A shuffled, artist-interleaved queue from one library ("Play library radio"). */
  libraryRadioTracks: (params: { library_id: ApiEntityId; limit?: number }) =>
    get<{ tracks: Track[] }>('/library-radio/tracks', {
      library_id: params.library_id,
      limit: params.limit,
    }),
  artists: (params?: ApiEntityId | { library_id?: ApiEntityId; library_ids?: ApiEntityId[]; genres?: string[]; sonic_fingerprint_only?: boolean; hide_compilation_only?: boolean }) => {
    const query: Record<string, string | number | boolean | undefined> = {};
    if (typeof params === 'string' || typeof params === 'number') {
      query.library_id = params;
    } else if (params) {
      query.library_id = params.library_id;
      query.library_ids = encodeLibraryIdsParam(params.library_ids);
      query.genres = encodeGenresParam(params.genres);
      if (params.sonic_fingerprint_only) query.sonic_fingerprint_only = true;
      if (params.hide_compilation_only) query.hide_compilation_only = true;
    }
    return get<Artist[]>('/artists', Object.keys(query).length ? query : undefined);
  },
  artistBrowsePage: (params?: {
    library_id?: ApiEntityId;
    library_ids?: ApiEntityId[];
    genres?: string[];
    q?: string;
    starts_with?: string;
    limit?: number;
    offset?: number;
    order?: 'asc' | 'desc';
    view?: 'summary' | 'full';
    sonic_fingerprint_only?: boolean;
    hide_compilation_only?: boolean;
  }) =>
    get<ArtistBrowsePage>('/artists', {
      library_id: params?.library_id,
      library_ids: encodeLibraryIdsParam(params?.library_ids),
      genres: encodeGenresParam(params?.genres),
      q: params?.q,
      starts_with: params?.starts_with,
      limit: params?.limit,
      offset: params?.offset,
      order: params?.order,
      view: params?.view,
      paged: 1,
      sonic_fingerprint_only: params?.sonic_fingerprint_only || undefined,
      hide_compilation_only: params?.hide_compilation_only || undefined,
    } as any),
  albums: (params?: { artist_id?: ApiEntityId; library_id?: ApiEntityId; library_ids?: ApiEntityId[]; group_by?: 'artist' | 'album_artist'; genres?: string[]; sonic_fingerprint_only?: boolean; after_album_rowid?: number; through_album_rowid?: number }) =>
    get<Album[]>('/albums', {
      ...params,
      library_ids: encodeLibraryIdsParam(params?.library_ids),
      genres: encodeGenresParam(params?.genres),
      sonic_fingerprint_only: params?.sonic_fingerprint_only || undefined,
    } as any),
  albumChangeCursor: () => get<{ cursor: number }>('/albums/change-cursor'),
  latestAlbums: (limit = 60) => get<LatestAlbum[]>('/albums/latest', { limit }),
  homeTopRated: (limit = 5) => get<HomeTopRated>('/home/top-rated', { limit }),
  homeGenreCovers: (genre: string) => get<ClientEntityId[]>('/home/genre-covers', { genre }),
  homeGenres: (limit = 6) => get<HomeGenreSummary[]>('/home/genres', { limit }),
  albumTracks: (albumId: ApiEntityId, libraryIds?: ApiEntityId[]) =>
    get<Track[]>(`/albums/${albumId}/tracks`, { library_ids: encodeLibraryIdsParam(libraryIds) }),
  albumTracksByGroup: (title: string, albumArtist: string | null, libraryIds?: ApiEntityId[]) =>
    get<Track[]>('/albums/by-group/tracks', { title, album_artist: albumArtist ?? undefined, library_ids: encodeLibraryIdsParam(libraryIds) }),
  albumArtUrl: (albumId: ApiEntityId, size: AlbumArtSize, version?: number) => withVersion(`${BASE}/api/albums/${albumId}/art?size=${size}`, version),
  albumCover: (albumId: ApiEntityId) => fetch(`${BASE}/api/albums/${albumId}/cover`),
  refreshAlbumCover: (albumId: ApiEntityId) => fetch(`${BASE}/api/albums/${albumId}/cover?refresh=1`),
  artistPhotoUrl: (artistId: ApiEntityId, size: AlbumArtSize = 300, version?: number) => withVersion(`${BASE}/api/artists/${artistId}/photo?size=${size}`, version),
  artistAlbums: (artistId: ApiEntityId, libraryIds?: ApiEntityId[]) =>
    get<any[]>(`/artists/${artistId}/albums`, { library_ids: encodeLibraryIdsParam(libraryIds) }),
  artistAppearsOn: (artistId: ApiEntityId, libraryIds?: ApiEntityId[]) =>
    get<any[]>(`/artists/${artistId}/appears-on`, { library_ids: encodeLibraryIdsParam(libraryIds) }),
  artistSimilar: (artistId: ApiEntityId, limit = 12) =>
    get<SimilarArtistsResponse>(`/artists/${artistId}/similar`, { limit }),
  resolveArtistReleaseTypes: (artistId: ApiEntityId) =>
    post<{ ok: boolean; updated: number }>(`/artists/${artistId}/release-types/resolve`),
  artistRadio: (artistId: ApiEntityId, options: Partial<ArtistRadioOptions> & { limit?: number } = {}) =>
    get<ArtistRadioResponse>(`/artists/${artistId}/radio`, {
      limit: options.limit ?? 100,
      focus: options.focus,
      moods: options.moods && options.moods.length ? options.moods.join(',') : undefined,
      variety: options.variety,
    }),
  artistRadioOptions: (artistId: ApiEntityId) =>
    get<ArtistRadioOptionsSnapshot>(`/artists/${artistId}/radio/options`),
  radioMetadataStatus: () => get<RadioMetadataStatus>('/radio/metadata/status'),
  genres: () => get<Genre[]>('/genres'),
  stats: () => get<Stats>('/stats'),
  recentlyPlayed: (limit = 10) => get<Track[]>('/tracks/recently-played', { limit }),
  topPlayedTracks: (limit = 10) => get<Track[]>('/tracks/top-played', { limit }),
  mostPlayedArtists: (limit = 10) => get<Artist[]>('/artists/most-played', { limit }),
  artist: (id: ApiEntityId) => get<Artist>(`/artists/${id}`),
  setArtistRating: (id: ApiEntityId, rating: number | null) =>
    patch<{ ok: boolean; rating: number | null }>(`/artists/${id}/rating`, { rating }),
  refreshArtistPhoto: (id: ApiEntityId) => fetch(`${BASE}/api/artists/${id}/photo?refresh=1`),
  artistPhotoCandidates: (id: ApiEntityId) =>
    get<{ candidates: { provider: string; url: string }[] }>(`/artists/${id}/photo/candidates`),
  selectArtistPhoto: (id: ApiEntityId, url: string, provider?: string) =>
    post<{ ok: boolean }>(`/artists/${id}/photo/select`, { url, provider }),
  // Artist consolidation (merge/unmerge duplicate artists) — see
  // wip/artist-consolidation-implementation-plan.md.
  mergeArtists: (artistIds: ApiEntityId[], masterName: string, masterArtistId?: ApiEntityId) =>
    post<Artist>('/artists/merge', {
      artist_ids: artistIds,
      master_name: masterName,
      master_artist_id: masterArtistId,
    }),
  getArtistMergeInfo: (id: ApiEntityId) => get<ArtistMergeInfo>(`/artists/${id}/merge-info`),
  unmergeArtist: (id: ApiEntityId, memberIds: ApiEntityId[]) =>
    post<UnmergeResult>(`/artists/${id}/unmerge`, { member_ids: memberIds }),
  lockArtistIdentity: (id: ApiEntityId) => post<Artist>(`/artists/${id}/lock-identity`),
  album: (id: ApiEntityId) => get<Album>(`/albums/${id}`),
  setAlbumRating: (id: ApiEntityId, rating: number | null) =>
    patch<{ ok: boolean; rating: number | null; updated: number }>(`/albums/${id}/rating`, { rating }),
  track: (id: ApiEntityId) => get<Track>(`/tracks/${id}`),
  setTrackRating: (id: ApiEntityId, rating: number | null) =>
    patch<{ ok: boolean; rating: number | null }>(`/tracks/${id}/rating`, { rating }),
  trackEqProfile: (id: ApiEntityId) => get<{ eq_profile: 'Rock' | 'Metal' | 'Pop' | 'Punk' | 'Electronic' | 'Club' | 'Hip-Hop' | 'Soul' | 'Acoustic' | 'Atmosphere' | 'Classical' | 'Vintage'; source: string }>(`/tracks/${id}/eq-profile`),
  trackLyrics: (id: ApiEntityId) => get<{
    lyrics: string;
    source: 'cache' | 'lrclib' | 'lyrics.ovh';
    synced?: Array<{ time: number; text: string }>;
    syncedLyrics?: Array<{ time: number; text: string }>;
  }>(`/tracks/${id}/lyrics`).then((result) => ({
    lyrics: result.lyrics,
    source: result.source,
    synced: Array.isArray(result.synced) ? result.synced : result.syncedLyrics,
  })),
  markTrackPlayed: (id: ApiEntityId) => post<{ ok: boolean }>(`/tracks/${id}/played`),
  /** The track played long enough to count; queued for the user's Last.fm / ListenBrainz accounts. */
  scrobbleTrack: (id: ApiEntityId, startedAt: number) =>
    post<{ ok: boolean }>(`/tracks/${id}/scrobble`, { startedAt: Math.floor(startedAt) }),
  trackWaveform: (id: ApiEntityId) => get<TrackWaveformLookupResponse>(`/tracks/${id}/waveform`),
  trackSonicFingerprint: async (id: ApiEntityId): Promise<SonicFingerprint | null> => {
    type RawFingerprint = {
      trackId: string;
      bpmDetected: number | null;
      energyScoreRefined: number;
      confidence: number;
      sourceDurationSec: number | null;
      demucsModel: string;
      usedGpu: boolean;
      analysisSchemaVersion: number;
      sectionJson: string;
      vocalWindowsJson: string;
      drumWindowsJson: string;
      bassWindowsJson: string;
      transitionWindowsJson: string;
      introOutroRefinedJson: string;
      phraseBoundariesJson: string;
    };
    try {
      const raw = await get<RawFingerprint>(`/tracks/${id}/sonic-fingerprint`);
      const parseJson = <T>(s: string, fallback: T): T => {
        try { return JSON.parse(s) as T; } catch { return fallback; }
      };
      return {
        trackId: raw.trackId,
        bpmDetected: raw.bpmDetected,
        energyScoreRefined: raw.energyScoreRefined,
        confidence: raw.confidence,
        sourceDurationSec: raw.sourceDurationSec,
        demucsModel: raw.demucsModel,
        usedGpu: raw.usedGpu,
        analysisSchemaVersion: raw.analysisSchemaVersion,
        sectionJson: parseJson<TrackSection[]>(raw.sectionJson, []),
        vocalWindowsJson: parseJson<StemWindow[]>(raw.vocalWindowsJson, []),
        drumWindowsJson: parseJson<StemWindow[]>(raw.drumWindowsJson, []),
        bassWindowsJson: parseJson<StemWindow[]>(raw.bassWindowsJson, []),
        transitionWindowsJson: parseJson<TransitionWindow[]>(raw.transitionWindowsJson, []),
        introOutroRefinedJson: parseJson<IntroOutroRefined>(raw.introOutroRefinedJson, { introEnd: null, outroStart: null }),
        phraseBoundariesJson: parseJson<number[]>(raw.phraseBoundariesJson, []),
      };
    } catch {
      return null;
    }
  },
  generateTrackWaveform: (id: ApiEntityId) => post<TrackWaveformLookupResponse>(`/tracks/${id}/waveform/generate`),
  trackStreamUrl: (id: ApiEntityId) => {
    const base = `${BASE}/api/tracks/${id}/stream`;
    return getStreamDirect() ? `${base}?noTranscode=1` : base;
  },
  fsBrowse: (path?: string) =>
    get<{ path: string; parent: string | null; entries: { name: string; path: string }[] }>(
      `/admin/fs/browse${path !== undefined ? `?path=${encodeURIComponent(path)}` : ''}`,
    ),
  fsMkdir: (parent: string, name: string) =>
    post<{ path: string }>('/admin/fs/mkdir', { parent, name }),
  debugTestPath: (path: string) => post<any>('/debug/test-path', { path }),
  systemStatus: () => get<{ ffmpegAvailable: boolean; setupRequired: boolean; suggestedDbFolder?: string; dbFolder?: string; version?: string; logFile?: string; scanDebugLogFile?: string; deepDebugLogFile?: string; restoring?: boolean; restoreStep?: string }>('/system/status'),
  backups: {
    overview: () => get<BackupsOverview>('/admin/backups'),
    saveSettings: (settings: BackupSettings) => put<BackupsOverview>('/admin/backups/settings', settings),
    create: () => post<{ ok: boolean }>('/admin/backups'),
    remove: (name: string) => del<{ ok: boolean }>(`/admin/backups/${encodeURIComponent(name)}`),
    health: () => get<BackupHealth>('/admin/backups/health'),
    inspect: (path: string) => post<BackupSummary>('/admin/backups/inspect', { path }),
    restore: (name: string, remaps: PathRemap[]) =>
      post<RestoreResult>(`/admin/backups/${encodeURIComponent(name)}/restore`, { remaps }),
    restoreFrom: (path: string, remaps: PathRemap[]) =>
      post<RestoreResult>('/admin/backups/restore-from', { path, remaps }),
    setupInspect: (path: string) => post<BackupSummary>('/system/backups/inspect', { path }),
    setupRestore: (backupPath: string, dbFolder: string, remaps: PathRemap[]) =>
      post<{ ok: boolean }>('/system/setup-restore', { backupPath, dbFolder, remaps }),
  },
  systemSelectFolder: (initialDir?: string) => post<{ folder: string | null }>('/system/select-folder', { initialDir }),
  systemSetup: (dbFolder: string) => post<{ ok: boolean }>('/system/setup', { dbFolder }),
  systemSwitchDb: (dbFolder: string) => post<{ ok: boolean }>('/system/switch-db', { dbFolder }),
  playbackSettings: () => get<{ transcodeQuality: string; replayGainEnabled: string; vinylMode: string; lastfmConfigured: string }>('/playback-settings'),
  settings: {
    get: () => get<Record<string, string>>('/settings'),
    update: (updates: Record<string, string>) => put<{ ok: boolean }>('/settings', updates),
  },
  schedules: {
    list: () => get<any[]>('/schedules'),
    get: (libraryId: ApiEntityId) => get<any>(`/schedules/${libraryId}`),
    upsert: (libraryId: ApiEntityId, enabled: boolean, frequency_hours: number) =>
      put<any>(`/schedules/${libraryId}`, { enabled, frequency_hours }),
    remove: (libraryId: ApiEntityId) =>
      fetch(`${BASE}/api/schedules/${libraryId}`, { method: 'DELETE' }).then(r => r.json()),
  },
  waveforms: {
    status: () => get<WaveformMappingStatus>('/waveforms/map/status'),
    runMap: () => post<WaveformMapRunResult>('/waveforms/map/run'),
  },
  bpm: {
    status: () => get<BpmAnalysisStatus>('/bpm/status'),
    run: () => post<BpmBatchResult>('/bpm/run'),
  },
  integrations: {
    spotifyTest: () => get<{ ok: boolean }>('/integrations/spotify/test'),
    geniusTest: (clientId?: string, clientSecret?: string) =>
      post<{ ok: boolean }>('/integrations/genius/test', { clientId, clientSecret }),
    lyrics: (params: { artist: string; title: string }) =>
      get<{ lyrics: string; sourceUrl: string }>('/integrations/lyrics', params as any),
    metadataSearch: (params: { artist: string; album?: string }) =>
      get<{ results: MetadataSearchResult[]; provider_warnings: ProviderSearchWarning[] }>(
        '/integrations/metadata-search', params as any,
      ),
  },
  updateAlbumMetadata: (
    id: ApiEntityId,
    data: {
      title?: string;
      album_artist?: string;
      year?: number;
      genre?: string;
      description?: string;
      label?: string;
      releaseType?: 'album' | 'single' | 'compilation';
      discogsReleaseType?: 'album' | 'single' | 'compilation';
      spotifyReleaseType?: 'album' | 'single' | 'compilation';
    },
    resetLock?: boolean,
  ) =>
    put<{ ok: boolean; merged_into?: ApiEntityId }>(`/albums/${id}/metadata`, resetLock ? { ...data, reset_lock: true } : data),
  updateArtistMetadata: (id: ApiEntityId, data: { name: string; description?: string }, resetLock?: boolean) =>
    put<{ ok: boolean }>(`/artists/${id}/metadata`, resetLock ? { ...data, reset_lock: true } : data),
  updateTrackMetadata: (
    id: ApiEntityId,
    data: {
      title?: string;
      artist?: string;
      album?: string;
      genre?: string;
      composer?: string;
      comment?: string;
      trackNumber?: number;
      discNumber?: number;
      year?: number;
    },
  ) => put<{ ok: boolean }>(`/tracks/${id}/metadata`, data),
  uploadAlbumArtwork: (id: ApiEntityId, imageBase64: string, mimeType: string) =>
    post<{ ok: boolean }>(`/albums/${id}/artwork`, { imageBase64, mimeType }),
  uploadArtistArtwork: (id: ApiEntityId, imageBase64: string, mimeType: string) =>
    post<{ ok: boolean }>(`/artists/${id}/artwork`, { imageBase64, mimeType }),
  playlists: {
    list:        ()                          => get<Playlist[]>('/playlists'),
    get:         (id: EntityId)             => get<Playlist>(`/playlists/${id}`),
    create:      (name: string, description?: string) => post<Playlist>('/playlists', { name, description }),
    update:      (id: EntityId, name: string, description?: string, remember_progress?: number) =>
                   put<Playlist>(`/playlists/${id}`, { name, description, remember_progress }),
    remove:      (id: EntityId)             => del<{ ok: boolean }>(`/playlists/${id}`),
    tracks:      (id: EntityId)             => get<PlaylistTrack[]>(`/playlists/${id}/tracks`),
    addTrack:    (id: EntityId, trackId: ApiEntityId) => post<any>(`/playlists/${id}/tracks`, { track_id: trackId }),
    addTracks:   (id: EntityId, trackIds: ApiEntityId[]) => post<any>(`/playlists/${id}/tracks/batch`, { track_ids: trackIds }),
    removeTrack: (id: EntityId, trackId: ApiEntityId) => del<{ ok: boolean }>(`/playlists/${id}/tracks/${trackId}`),
    reorder:     (id: EntityId, trackIds: ApiEntityId[]) =>
                   put<{ ok: boolean }>(`/playlists/${id}/tracks/order`, { track_ids: trackIds }),
    saveTrackProgress: (playlistId: EntityId, trackId: ApiEntityId, seconds: number) =>
                   patch<{ ok: boolean }>(`/playlists/${playlistId}/tracks/${trackId}/progress`, { seconds }),
    exportM3uUrl: (id: EntityId) => `${BASE}/api/playlists/${id}/export.m3u`,
    /** A playlist and its tracks from one read (smart playlists refresh first). */
    snapshot:    (id: EntityId) => smartRequest<PlaylistSnapshot>('GET', `/playlists/${id}/snapshot`),
    /** One playlist's card row; a smart playlist refreshes first when its inputs changed. */
    summary:     (id: EntityId) => get<Playlist>(`/playlists/${id}/summary`),
    /** Re-evaluates a smart playlist now. */
    refresh:     (id: EntityId) => smartRequest<PlaylistSnapshot>('POST', `/playlists/${id}/refresh`),
    createSmart: (name: string, definition: SmartDefinition) =>
                   smartRequest<Playlist>('POST', '/playlists', { name, kind: 'smart', definition }),
    smartPreview: (definition: SmartDefinition, signal?: AbortSignal) =>
                   smartRequest<SmartPlaylistPreview>('POST', '/playlists/smart-preview', { definition }, signal),
    smartDefinition: (id: EntityId) => smartRequest<SmartDefinitionView>('GET', `/playlists/${id}/smart-definition`),
    updateSmartDefinition: (id: EntityId, definition: SmartDefinition, expectedEditRevision: number) =>
                   smartRequest<SmartDefinitionView>('PUT', `/playlists/${id}/smart-definition`, { definition, expectedEditRevision }),
    /** Saves a smart playlist's current tracks as a new ordinary playlist. */
    saveAsOrdinary: (id: EntityId, name: string) =>
                   smartRequest<{ playlist: Playlist; added: number }>('POST', `/playlists/${id}/static-copy`, { name }),
    importPreview: (contentBase64: string, remaps: PathRemap[] = []) => postPlaylistImportPreview(contentBase64, remaps),
    importCommit: (name: string, trackIds: string[], description?: string) =>
                   post<{ playlist: Playlist; added: number }>('/playlists/import/commit', { name, description, trackIds }),
  },
  boogiemix: {
    createJob: (
      playlistId: EntityId,
      style?: 'chill_blend' | 'club_blend' | 'long_build' | 'safe_mix',
      quality: 'standard' | 'high_quality' = 'high_quality',
      crossfadeSec?: number,
      orderMode: 'style' | 'playlist' = 'style',
      name?: string,
    ) =>
      post<{ jobId: ApiEntityId }>(`/playlists/${playlistId}/boogiemix/jobs`, {
        style,
        quality,
        default_crossfade_sec: crossfadeSec,
        order_mode: orderMode,
        name,
      }),
    getJob: (jobId: ApiEntityId) =>
      get<BoogieMixJob>(`/boogiemix/jobs/${jobId}`),
    latestJobForPlaylist: (playlistId: EntityId) =>
      get<BoogieMixJob | null>(`/playlists/${playlistId}/boogiemix/latest-job`),
    cancelJob: (jobId: ApiEntityId) =>
      post<{ ok: boolean }>(`/boogiemix/jobs/${jobId}/cancel`),
    deepAnalysisStatus: () =>
      get<BoogieMixDeepAnalysisStatus>('/boogiemix/deep-analysis/status'),
    queuePlaylistDeepAnalysis: (playlistId: EntityId) =>
      post<{ queued: number }>(`/boogiemix/deep-analysis/playlists/${playlistId}/queue`),
    playlistDeepAnalysisProgress: (playlistId: EntityId) =>
      get<import('./types').PlaylistDeepAnalysisProgress>(`/boogiemix/deep-analysis/playlists/${playlistId}/progress`),
    // `force` re-analyses tracks that already hold a current analysis; without
    // it only missing/stale tracks are queued, which on a fully analysed
    // library means nothing at all.
    queueLibraryDeepAnalysis: (libraryId: EntityId, force = false) =>
      post<{ queued: number }>(
        `/boogiemix/deep-analysis/libraries/${libraryId}/queue${force ? '?force=true' : ''}`,
      ),
    // Collection-wide sibling of queueLibraryDeepAnalysis. Queued at
    // background priority server-side, so Pause/Resume Background governs it.
    queueAllDeepAnalysis: (force = false) =>
      post<{ queued: number }>(
        `/boogiemix/deep-analysis/all/queue${force ? '?force=true' : ''}`,
      ),
    pauseDeepAnalysisBackground: () =>
      post<{ ok: boolean }>('/boogiemix/deep-analysis/pause'),
    resumeDeepAnalysisBackground: () =>
      post<{ ok: boolean }>('/boogiemix/deep-analysis/resume'),
    clearDeepAnalysisCache: () =>
      post<{ ok: boolean; deletedCacheRows: number; deletedJobRows: number }>('/boogiemix/deep-analysis/cache/clear'),
    listOutputs: (playlistId: EntityId) =>
      get<BoogieMixOutput[]>(`/playlists/${playlistId}/boogiemix/outputs`),
    listAllOutputs: () =>
      get<BoogieMixOutput[]>('/boogiemix/outputs'),
    renameOutput: (outputId: ApiEntityId, name: string) =>
      patch<{ ok: boolean }>(`/boogiemix/outputs/${outputId}`, { name }),
    deleteOutput: (outputId: ApiEntityId) =>
      del<{ ok: boolean }>(`/boogiemix/outputs/${outputId}`),
    outputDownloadUrl: (outputId: ApiEntityId) =>
      `${BASE}/api/boogiemix/outputs/${outputId}/file`,
    playUrl: (outputId: ApiEntityId) =>
      `${BASE}/api/boogiemix/outputs/${outputId}/play`,
    timeline: (outputId: ApiEntityId) =>
      get<import('./types').MixTimelineResponse>(`/boogiemix/outputs/${outputId}/timeline`),
    storyImageUrl: (outputId: ApiEntityId) =>
      `${BASE}/api/boogiemix/outputs/${outputId}/story-image`,
  },
  dlna: {
    status:  () => get<{ running: boolean; port: number | null; friendlyName: string | null }>('/dlna/status'),
    restart: () => post<{ ok: boolean }>('/dlna/restart'),
  },
  /** Play On: UPnP/DLNA renderers and server-owned sessions. Errors are `RemoteApiError`. */
  remote: {
    renderers: () => remoteRequest<RenderersResponse>('GET', '/renderers'),
    refresh: () => remoteRequest<{ ok: boolean }>('POST', '/renderers/refresh'),
    updateRenderer: (udn: string, patch: { alias?: string | null; formatPolicy?: RemoteFormatPolicy; hidden?: boolean }) =>
      remoteRequest<{ ok: boolean }>('PATCH', `/renderers/${remoteUdn(udn)}`, patch),
    rendererIconUrl: (udn: string) => `${BASE}/api/remote/renderers/${remoteUdn(udn)}/icon`,
    diagnostics: () => remoteRequest<RemoteDiagnostics>('GET', '/diagnostics'),
    /** Current session, or null when the renderer is idle. */
    session: (udn: string) =>
      remoteRequest<RemoteSessionState>('GET', `/sessions/${remoteUdn(udn)}`).catch((error: unknown) => {
        if (error instanceof RemoteApiError && error.status === 404) return null;
        throw error;
      }),
    load: (udn: string, body: RemoteLoadBody) =>
      remoteRequest<RemoteSessionState>('POST', `/sessions/${remoteUdn(udn)}/load`, body),
    command: (udn: string, command: RemoteCommand, takeover = false) =>
      remoteRequest<RemoteSessionState>('POST', `/sessions/${remoteUdn(udn)}/command`, { ...command, takeover }),
    queue: (udn: string, op: RemoteQueueOp, takeover = false) =>
      remoteRequest<RemoteSessionState>('POST', `/sessions/${remoteUdn(udn)}/queue`, { ...op, takeover }),
    transfer: (udn: string, toUdn: string, takeover = false) =>
      remoteRequest<RemoteSessionState>('POST', `/sessions/${remoteUdn(udn)}/transfer`, { toUdn, takeover }),
    release: (udn: string, options: { stop?: boolean; takeover?: boolean } = {}) => {
      const query = new URLSearchParams();
      if (options.stop !== undefined) query.set('stop', String(options.stop));
      if (options.takeover) query.set('takeover', 'true');
      const qs = query.toString();
      return remoteRequest<RemoteSessionState>('DELETE', `/sessions/${remoteUdn(udn)}${qs ? `?${qs}` : ''}`);
    },
    eventsUrl: () => `${BASE}/api/remote/events`,
  },
  activity: {
    /** Admin only. */
    snapshot:     () => get<ActivitySnapshot>('/activity'),
    /** Admin only: SSE `snapshot` events. */
    eventsUrl:    () => `${BASE}/api/activity/events`,
    /** Admin only: cancels any user's queued or analysing/planning BoogieMix job. */
    cancelMixJob: (jobId: string) => post<{ ok: boolean }>(`/activity/mix-jobs/${encodeURIComponent(jobId)}/cancel`, {}),
    /** The web player's position for the admin Activity page. Fire and forget (204, no body). */
    heartbeat:    (beat: PlayerHeartbeat, options: { closing?: boolean } = {}): void => {
      const url = `${BASE}/api/activity/heartbeat`;
      const body = JSON.stringify(beat);
      try {
        if (options.closing && typeof navigator !== 'undefined' && typeof navigator.sendBeacon === 'function') {
          navigator.sendBeacon(url, new Blob([body], { type: 'application/json' }));
          return;
        }
        fetch(url, {
          ...FETCH_OPTS,
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body,
          keepalive: true,
        }).catch(() => {});
      } catch {
        // Best effort.
      }
    },
  },
  crossfade: {
    config:          (entityType?: string, entityId?: ApiEntityId) =>
                       get<CrossfadeConfig>('/crossfade/config', { entity_type: entityType, entity_id: entityId }),
    overrides:       (entityType?: string) =>
                       get<CrossfadeOverride[]>('/crossfade/overrides', { entity_type: entityType }),
    upsertOverride:  (override: { entity_type: string; entity_id: ApiEntityId; mode: string; duration: number }) =>
                       put<{ ok: boolean }>('/crossfade/overrides', override),
    removeOverride:  (entityType: string, entityId: ApiEntityId) =>
                       del<{ ok: boolean }>(`/crossfade/overrides/${entityType}/${entityId}`),
  },
  auth: {
    // Public — no auth required
    getLoginUsers: () => get<LoginUser[]>('/auth/users'),
    login: (userId: EntityId, pin?: string, stayLoggedIn?: boolean) =>
      post<{ user: AuthUser }>('/auth/login', { userId, pin, stayLoggedIn }),
    logout: () => post<{ ok: boolean }>('/auth/logout'),
    // Requires auth
    me: () => get<AuthUser>('/auth/me'),
    // False only on an explicit 401; network failures reject so callers can tell them apart.
    sessionValid: async () => (await fetch(`${BASE}/api/auth/me`, FETCH_OPTS)).status !== 401,
  },
  userSettings: {
    get: () => get<Record<string, string>>('/user/settings'),
    update: (updates: Record<string, string>) => put<{ ok: boolean }>('/user/settings', updates),
  },
  userHistory: (limit = 50) => get<HistoryEntry[]>('/user/history', { limit }),
  lastfm: {
    info: (artist: string, album?: string) => {
      const params = new URLSearchParams({ artist });
      if (album) params.set('album', album);
      return get<LastFmInfo>(`/lastfm/info?${params}`);
    },
    topTracks: (artist: string) =>
      get<{ name: string; playcount: number; listeners?: number; url?: string }[]>(
        `/lastfm/top-tracks?${new URLSearchParams({ artist })}`
      ),
  },
  admin: {
    queues:             ()                                                                                   => get<AdminQueueSnapshot>('/admin/queues'),
    providerUsage:      ()                                                                                   => get<ProviderUsageSnapshot>('/admin/provider-usage'),
    cancelScanJob:      (id: ApiEntityId)                                                                    => post<{ ok: boolean; id: ApiEntityId; status: string }>(`/admin/queues/scan/${id}/cancel`),
    cancelPostScanJob:  (id: ApiEntityId)                                                                    => post<{ ok: boolean; id: ApiEntityId; status: string }>(`/admin/queues/post-scan/${id}/cancel`),
    failPostScanJob:    (id: ApiEntityId)                                                                    => post<{ ok: boolean; id: ApiEntityId; status: string }>(`/admin/queues/post-scan/${id}/fail`),
    retryPostScanJob:   (id: ApiEntityId)                                                                    => post<{ ok: boolean; id: ApiEntityId; status: string }>(`/admin/queues/post-scan/${id}/retry`),
    enqueuePostScanJob: (libraryId: ApiEntityId, jobType: AdminPostScanJobType)                              => post<{ ok: boolean; id: ApiEntityId; status: string; job_type: AdminPostScanJobType; library_id: ApiEntityId }>(`/admin/libraries/${libraryId}/post-scan`, { jobType }),
    users: {
      list:               ()                                                                                   => get<AdminUser[]>('/admin/users'),
      create:             (data: { username: string; role: string; pin?: string; canManageLibraries?: boolean; canEditMetadata?: boolean }) => post<AdminUser>('/admin/users', data),
      remove:             (id: EntityId)                                                                      => del<{ ok: boolean }>(`/admin/users/${id}`),
      setPin:             (id: EntityId, pin: string | null)                                                  => put<{ ok: boolean }>(`/admin/users/${id}/pin`, { pin }),
      setPermissions:     (id: EntityId, perms: { canManageLibraries: boolean; canEditMetadata: boolean })   => put<{ ok: boolean }>(`/admin/users/${id}/permissions`, perms),
      setSubsonicPassword:   (id: EntityId, password: string)                                                => put<{ ok: boolean }>(`/admin/users/${id}/subsonic-password`, { password }),
      clearSubsonicPassword: (id: EntityId)                                                                  => del<{ ok: boolean }>(`/admin/users/${id}/subsonic-password`),
      revokeSubsonicApiKey:  (id: EntityId)                                                                  => del<{ ok: boolean }>(`/admin/users/${id}/subsonic-api-key`),
    },
  },
  /** Favourites (hearts), shared with Subsonic apps' stars. */
  favourites: {
    ids:    () => get<FavouriteIds>('/favourites/ids'),
    list:   (limit?: number) => get<Favourites>('/favourites', { limit }),
    add:    (kind: FavouriteKind, id: ApiEntityId) => put<{ ok: boolean }>(`/favourites/${kind}/${encodeURIComponent(String(id))}`),
    remove: (kind: FavouriteKind, id: ApiEntityId) => del<{ ok: boolean }>(`/favourites/${kind}/${encodeURIComponent(String(id))}`),
  },
  /** Internet radio stations (admins edit). */
  radioStations: {
    list:      () => get<RadioStation[]>('/radio-stations'),
    create:    (input: RadioStationInput) => post<RadioStation>('/radio-stations', input),
    update:    (id: string, input: RadioStationInput) => put<RadioStation>(`/radio-stations/${encodeURIComponent(id)}`, input),
    remove:    (id: string) => del<{ ok: boolean }>(`/radio-stations/${encodeURIComponent(id)}`),
    streamUrl: (id: string) => `${BASE}/api/radio-stations/${encodeURIComponent(id)}/stream`,
  },
  /** The signed-in user's scrobbling accounts. */
  scrobble: {
    accounts:            () => get<ScrobbleAccountsResponse>('/scrobble/accounts'),
    connectListenBrainz: (token: string) => put<{ ok: boolean; accountName: string }>('/scrobble/listenbrainz', { token }),
    /** The Last.fm sign-in page to open; Last.fm returns to this origin's callback. */
    connectLastFm:       (origin: string) => post<{ url: string }>('/scrobble/lastfm/connect', { origin }),
    disconnect:          (service: 'lastfm' | 'listenbrainz') => del<{ ok: boolean }>(`/scrobble/${service}`),
  },
  /** The signed-in user's Subsonic password and API key for third-party apps (`/rest`). */
  subsonic: {
    credentials:   () => get<SubsonicCredentials>('/subsonic/credentials'),
    setPassword:   (password: string) => put<{ ok: boolean }>('/subsonic/credentials/password', { password }),
    clearPassword: () => del<{ ok: boolean }>('/subsonic/credentials/password'),
    /** Creates or replaces the API key; the key is returned only here. */
    createApiKey:  () => post<{ apiKey: string }>('/subsonic/credentials/api-key', {}),
    revokeApiKey:  () => del<{ ok: boolean }>('/subsonic/credentials/api-key'),
  },
};

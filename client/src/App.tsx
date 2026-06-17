/**
 * Defines App behavior for BoogieBox.
 */

import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { api, getStreamDirect } from './api';
import type {
  Library, Track, Album, Artist, Stats, SearchResult,
  SortField, SortOrder, Genre, AppSettings, QueueSource, AuthUser, ClientEntityId,
} from './types';
import type { EntityId } from './entityId';
import { DEFAULT_SETTINGS } from './types';
import Player, { type PlaybackSnapshot, type PlayerState } from './components/Player';
import SettingsPage from './components/SettingsPage';
import BrowseView from './components/BrowseView';
import PlaylistsView from './components/PlaylistsView';
import HomeView from './components/HomeView';
import SetupView from './components/SetupView';
import LoginScreen from './components/LoginScreen';
import { ContextMenuRoot, KebabButton } from './components/ContextMenu';
import StarRating from './components/StarRating';
import { APP_VERSION } from './version';
import { parseServerDate } from './utils';
import MobileApp from './mobile/MobileApp';
import { phase2 } from './uiPhase2';
import { useMobileShell } from './mobile/useMobileShell';

// ─── CSS Variable injection ───────────────────────────────────────────────────
// Injects theme as CSS custom properties on :root so ALL components pick them up.

function applyTheme(s: AppSettings) {
  const style = document.documentElement.style;
  const texture = getThemeTextureVars(s.bgTexture);
  style.setProperty('--bg',         s.colorBg);
  style.setProperty('--surface',    s.colorSurface);
  style.setProperty('--border',     s.colorBorder);
  style.setProperty('--accent',     s.colorAccent);
  style.setProperty('--accent-base', s.colorAccent);
  style.setProperty('--text',       s.colorText);
  style.setProperty('--text-muted', s.colorTextMuted);
  style.setProperty('--bg-texture', s.bgTexture);
  style.setProperty('--bg-texture-image', texture.image);
  style.setProperty('--bg-texture-size', texture.size);
  style.setProperty('--font',       s.fontFamily);
  // Also update body background immediately
  document.body.style.backgroundColor = s.colorBg;
  document.body.style.backgroundImage = texture.image;
  document.body.style.backgroundSize = texture.size;
  document.body.style.backgroundRepeat = s.bgTexture === 'wood' ? 'repeat' : 'no-repeat';
  document.body.style.color = s.colorText;
}

type ThemeSettings = Pick<AppSettings,
  'colorBg' | 'colorSurface' | 'colorBorder' | 'colorAccent' | 'colorText' | 'colorTextMuted' | 'bgTexture' | 'fontFamily'
>;

const THEME_COLOR_KEYS: Array<keyof ThemeSettings> = [
  'colorBg',
  'colorSurface',
  'colorBorder',
  'colorAccent',
  'colorText',
  'colorTextMuted',
];

const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/;
/** THEME STORAGE KEY is part of this module's public API. */
export const THEME_STORAGE_KEY = 'boogiebox.theme.v1';
/** ADAPTIVE ACCENT STORAGE KEY is part of this module's public API. */
export const ADAPTIVE_ACCENT_STORAGE_KEY = 'boogiebox.theme.adaptiveAccent.v1';
const THEME_TEXTURE_VALUES = new Set(['none', 'wood']);
const WOOD_BG_TEXTURE = [
  'linear-gradient(90deg, rgba(255,255,255,0.05) 0%, rgba(255,255,255,0.01) 8%, rgba(0,0,0,0.07) 16%, rgba(255,255,255,0.02) 24%, rgba(0,0,0,0.06) 32%, rgba(255,255,255,0.02) 40%, rgba(0,0,0,0.08) 48%, rgba(255,255,255,0.03) 56%, rgba(0,0,0,0.07) 64%, rgba(255,255,255,0.02) 72%, rgba(0,0,0,0.06) 80%, rgba(255,255,255,0.02) 88%, rgba(0,0,0,0.08) 100%)',
  'linear-gradient(8deg, rgba(255,255,255,0.07) 0%, rgba(255,255,255,0.00) 38%, rgba(0,0,0,0.14) 100%)',
  'radial-gradient(circle at 14% 24%, rgba(255,255,255,0.08) 0%, rgba(255,255,255,0.00) 44%)',
].join(',');
const WOOD_BG_TEXTURE_SIZE = '300px 300px, 100% 100%, 100% 100%';

/** Get Theme Texture Vars is part of this module's public API. */
export function getThemeTextureVars(bgTexture: string): { image: string; size: string } {
  if (bgTexture === 'wood') return { image: WOOD_BG_TEXTURE, size: WOOD_BG_TEXTURE_SIZE };
  return { image: 'none', size: 'auto' };
}

/** Extract Theme Settings is part of this module's public API. */
export function extractThemeSettings(settings: AppSettings): ThemeSettings {
  return {
    colorBg: settings.colorBg,
    colorSurface: settings.colorSurface,
    colorBorder: settings.colorBorder,
    colorAccent: settings.colorAccent,
    colorText: settings.colorText,
    colorTextMuted: settings.colorTextMuted,
    bgTexture: settings.bgTexture,
    fontFamily: settings.fontFamily,
  };
}

/** Parse Theme Settings is part of this module's public API. */
export function parseThemeSettings(raw: string | null): Partial<ThemeSettings> | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const src = parsed as Record<string, unknown>;
    const out: Partial<ThemeSettings> = {};
    for (const key of THEME_COLOR_KEYS) {
      const value = src[key];
      if (typeof value === 'string' && HEX_COLOR_RE.test(value)) {
        (out as Record<string, string>)[key] = value;
      }
    }
    if (typeof src.bgTexture === 'string' && THEME_TEXTURE_VALUES.has(src.bgTexture)) {
      out.bgTexture = src.bgTexture;
    }
    if (typeof src.fontFamily === 'string' && src.fontFamily.trim()) {
      out.fontFamily = src.fontFamily;
    }
    return Object.keys(out).length ? out : null;
  } catch {
    return null;
  }
}

function themeKey(userId: EntityId) { return `${THEME_STORAGE_KEY}.u${userId}`; }
function accentKey(userId: EntityId) { return `${ADAPTIVE_ACCENT_STORAGE_KEY}.u${userId}`; }

function safeLocalStorageGet(key: string): string | null {
  if (typeof window === 'undefined') return null;
  try {
    const storage = window.localStorage;
    if (!storage || typeof storage.getItem !== 'function') return null;
    return storage.getItem(key);
  } catch {
    return null;
  }
}

function getStoredTheme(userId: EntityId): Partial<ThemeSettings> | null {
  return parseThemeSettings(safeLocalStorageGet(themeKey(userId)));
}

function getStoredAdaptiveAccentEnabled(userId: EntityId): boolean {
  const raw = safeLocalStorageGet(accentKey(userId));
  return raw !== 'false';
}

function saveThemeToStorage(settings: AppSettings, userId: EntityId): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(themeKey(userId), JSON.stringify(extractThemeSettings(settings)));
  } catch {
    // Best effort only (private mode/quota issues).
  }
}

function saveAdaptiveAccentToStorage(enabled: boolean, userId: EntityId): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(accentKey(userId), enabled ? 'true' : 'false');
  } catch {
    // Best effort only.
  }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function fmt(seconds: number | null): string {
  if (seconds == null) return '–';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function fmtSize(bytes: number | null): string {
  if (bytes == null) return '–';
  if (bytes > 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

/** Fulfilled Value is part of this module's public API. */
export function fulfilledValue<T>(result: PromiseSettledResult<T>): T | null {
  return result.status === 'fulfilled' ? result.value : null;
}

/** Create Browse Genre Request is part of this module's public API. */
export function createBrowseGenreRequest(
  genre: string,
  token = Date.now(),
): { genre: string; token: number } | null {
  const trimmed = genre.trim();
  if (!trimmed) return null;
  return { genre: trimmed, token };
}

/** Should Record Track Play is part of this module's public API. */
export function shouldRecordTrackPlay(track: Pick<Track, 'id'> | null | undefined): track is Pick<Track, 'id'> {
  if (!track) return false;
  return track.id.trim().length > 0 && track.id !== '0';
}

/** Normalize Artist Lookup Name is part of this module's public API. */
export function normalizeArtistLookupName(name: string): string {
  return name.trim().replace(/\s+/g, ' ').toLowerCase();
}

/** Find Artist By Name is part of this module's public API. */
export function findArtistByName(artists: Artist[], artistName: string): Artist | null {
  const needle = normalizeArtistLookupName(artistName);
  if (!needle) return null;
  return artists.find((artist) => normalizeArtistLookupName(artist.name) === needle) ?? null;
}

/** Normalize Album Lookup Title is part of this module's public API. */
export function normalizeAlbumLookupTitle(title: string): string {
  return title.trim().replace(/\s+/g, ' ').toLowerCase();
}

/** Find Album By Name is part of this module's public API. */
export function findAlbumByName(albums: Album[], albumTitle: string, artistName?: string | null): Album | null {
  const titleNeedle = normalizeAlbumLookupTitle(albumTitle);
  if (!titleNeedle) return null;

  const titleMatches = albums.filter((album) => normalizeAlbumLookupTitle(album.title) === titleNeedle);
  if (!titleMatches.length) return null;
  if (!artistName) return titleMatches[0];

  const artistNeedle = normalizeArtistLookupName(artistName);
  if (!artistNeedle) return titleMatches[0];

  const exactArtistMatch = titleMatches.find((album) => {
    const candidates = [album.album_artist, album.artist]
      .filter((value): value is string => Boolean(value && value.trim()))
      .map((value) => normalizeArtistLookupName(value));
    return candidates.includes(artistNeedle);
  });
  return exactArtistMatch ?? titleMatches[0];
}

// ─── Icons ────────────────────────────────────────────────────────────────────

const Icon = {
  Music: () => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>
    </svg>
  ),
  Library: () => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>
    </svg>
  ),
  Search: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>
    </svg>
  ),
  Browse: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/>
      <rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/>
    </svg>
  ),
  Film: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <rect x="2" y="4" width="20" height="16" rx="2"/>
      <polygon points="10,9 16,12 10,15" fill="currentColor" stroke="none"/>
    </svg>
  ),
  Home: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M3 10.5L12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/>
    </svg>
  ),
  Playlist: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/>
      <line x1="8" y1="18" x2="21" y2="18"/>
      <path d="M3 6h.01M3 12h.01M3 18h.01"/>
    </svg>
  ),
  Galaxy: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="8" />
      <path d="M4 12h16M12 4c2 2 3 5 3 8s-1 6-3 8c-2-2-3-5-3-8s1-6 3-8z" />
    </svg>
  ),
  Settings: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="3"/>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
    </svg>
  ),
  Scan: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/>
      <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
    </svg>
  ),
  Plus: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
    </svg>
  ),
  Trash: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14H6L5 6"/>
      <path d="M10 11v6"/><path d="M14 11v6"/><path d="M9 6V4h6v2"/>
    </svg>
  ),
  X: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
    </svg>
  ),
  Play: () => (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
      <polygon points="5,3 19,12 5,21"/>
    </svg>
  ),
  ChevronUp: () => (
    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="18 15 12 9 6 15"/>
    </svg>
  ),
  ChevronDown: () => (
    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="6 9 12 15 18 9"/>
    </svg>
  ),
  ChevronLeft: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="15 18 9 12 15 6"/>
    </svg>
  ),
  ChevronRight: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="9 18 15 12 9 6"/>
    </svg>
  ),
};

// ─── Stats Bar ────────────────────────────────────────────────────────────────

function StatsBar({ stats }: { stats: Stats | null }) {
  const items = [
    { label: 'Tracks',  value: stats?.total_tracks?.toLocaleString()  ?? '–' },
    { label: 'Artists', value: stats?.total_artists?.toLocaleString() ?? '–' },
    { label: 'Albums',  value: stats?.total_albums?.toLocaleString()  ?? '–' },
    { label: 'Hours',   value: stats?.total_hours != null ? stats.total_hours.toLocaleString() : '–' },
    { label: 'GB',      value: stats?.total_gb    != null ? String(stats.total_gb) : '–' },
  ];
  return (
    <div style={S.statsBar}>
      {items.map(({ label, value }) => (
        <div key={label} style={S.statItem}>
          <span style={S.statValue}>{value}</span>
          <span style={S.statLabel}>{label}</span>
        </div>
      ))}
    </div>
  );
}

// ─── Library Manager ──────────────────────────────────────────────────────────

function ArtistRowIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M12 12c2.76 0 5-2.24 5-5s-2.24-5-5-5-5 2.24-5 5 2.24 5 5 5z" />
      <path d="M4 22c0-4.42 3.58-8 8-8s8 3.58 8 8" />
    </svg>
  );
}

function AlbumRowIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
      <path d="M12 3v4" />
    </svg>
  );
}

function OpenIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M7 17L17 7" />
      <path d="M8 7h9v9" />
    </svg>
  );
}

/** Sort Search Tracks is part of this module's public API. */
export function sortSearchTracks(tracks: Track[], field: SortField, order: SortOrder): Track[] {
  const direction = order === 'asc' ? 1 : -1;
  const sorted = [...tracks];
  sorted.sort((a, b) => {
    switch (field) {
      case 'year':
        return ((a.year ?? 0) - (b.year ?? 0)) * direction;
      case 'duration':
        return ((a.duration ?? 0) - (b.duration ?? 0)) * direction;
      case 'bitrate':
        return ((a.bitrate ?? 0) - (b.bitrate ?? 0)) * direction;
      case 'rating':
        return (((a.rating ?? 0) as number) - ((b.rating ?? 0) as number)) * direction;
      case 'artist':
        return (a.artist ?? '').localeCompare(b.artist ?? '') * direction;
      case 'album':
        return (a.album ?? '').localeCompare(b.album ?? '') * direction;
      case 'title':
      default:
        return (a.title ?? '').localeCompare(b.title ?? '') * direction;
    }
  });
  return sorted;
}

type SearchTrackField = 'title' | 'artist' | 'album' | 'genre' | 'year' | 'duration' | 'bitrate' | 'rating';

function TrackModal({
  track,
  onClose,
  onPlay,
  onQueue,
}: {
  track: Track;
  onClose: () => void;
  onPlay: () => void;
  onQueue: () => void;
}) {
  return (
    <div style={S.modalOverlay} onClick={onClose}>
      <div style={S.modal} onClick={(e) => e.stopPropagation()}>
        <div style={S.modalHeader}>
          <div>
            <div style={S.modalTitle}>{track.title || track.file_name}</div>
            <div style={S.modalSub}>{[track.artist, track.album, track.year].filter(Boolean).join(' · ') || 'Track details'}</div>
          </div>
          <button style={S.closeBtn} onClick={onClose} type="button">
            <Icon.X />
          </button>
        </div>
        <table style={S.metaTable}>
          <tbody>
            <tr><td style={S.metaLabel}>Artist</td><td>{track.artist || '–'}</td></tr>
            <tr><td style={S.metaLabel}>Album</td><td>{track.album || '–'}</td></tr>
            <tr><td style={S.metaLabel}>Genre</td><td>{track.genre || '–'}</td></tr>
            <tr><td style={S.metaLabel}>Year</td><td>{track.year || '–'}</td></tr>
            <tr><td style={S.metaLabel}>Duration</td><td>{fmt(track.duration)}</td></tr>
            <tr><td style={S.metaLabel}>Bitrate</td><td>{track.bitrate != null ? `${track.bitrate} kbps` : '–'}</td></tr>
            <tr><td style={S.metaLabel}>File</td><td style={{ wordBreak: 'break-all' }}>{track.file_name}</td></tr>
          </tbody>
        </table>
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 16 }}>
          <button style={S.btnSecondary} onClick={onQueue} type="button">Queue</button>
          <button style={S.btnPrimary} onClick={onPlay} type="button">Play</button>
        </div>
      </div>
    </div>
  );
}

function SearchView({ libraries, playTrack, addToQueue, onOpenArtist, onOpenAlbum }: {
  libraries: Library[];
  playTrack: (track: Track, allTracks?: Track[]) => void;
  addToQueue: (track: Track) => void;
  onOpenArtist: (artist: Artist) => void;
  onOpenAlbum: (album: Album) => void;
}) {
  const [query, setQuery]         = useState('');
  const [libraryId, setLibraryId] = useState<ClientEntityId | undefined>();
  const [genre, setGenre]         = useState('');
  const [year, setYear]           = useState('');
  const [sonicFingerprintOnly, setSonicFingerprintOnly] = useState(false);
  const [sort, setSort]           = useState<SearchTrackField>('title');
  const [order, setOrder]         = useState<SortOrder>('asc');
  const [trackSortField, setTrackSortField] = useState<SearchTrackField>('title');
  const [trackSortDir, setTrackSortDir] = useState<SortOrder>('asc');
  const [page, setPage]           = useState(1);
  const [result, setResult]       = useState<SearchResult | null>(null);
  const [loading, setLoading]     = useState(false);
  const [genres, setGenres]       = useState<Genre[]>([]);
  const [selectedTrack, setSelectedTrack] = useState<Track | null>(null);
  const [hoveredTrackId, setHoveredTrackId] = useState<ClientEntityId | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>();
  const searchRequestSeqRef = useRef(0);

  useEffect(() => { api.genres().then(setGenres).catch(() => {}); }, []);

  const doSearch = useCallback(async (p = 1) => {
    const requestSeq = ++searchRequestSeqRef.current;
    setLoading(true);
    try {
      const res = await api.search({
        q: query, library_id: libraryId, genre: genre || undefined,
        year: year ? Number(year) : undefined, sort, order, page: p, limit: 100,
        search_mode: 'omni',
        mode: 'music',
        sonic_fingerprint_only: sonicFingerprintOnly || undefined,
      });
      if (requestSeq !== searchRequestSeqRef.current) return;
      setResult(res); setPage(p);
    } finally {
      if (requestSeq === searchRequestSeqRef.current) {
        setLoading(false);
      }
    }
  }, [
    query,
    libraryId,
    genre,
    year,
    sort,
    order,
    sonicFingerprintOnly,
  ]);

  useEffect(() => {
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => doSearch(1), 300);
    return () => clearTimeout(debounceRef.current);
  }, [doSearch]);

  const toggleSort = (field: SearchTrackField) => {
    if (sort === field) {
      const nextDir = order === 'asc' ? 'desc' : 'asc';
      setOrder(nextDir);
      setTrackSortDir(nextDir);
    } else {
      setSort(field);
      const nextDir = field === 'rating' ? 'desc' : 'asc';
      setOrder(nextDir);
      setTrackSortDir(nextDir);
    }
    setTrackSortField(field);
  };

  const SortIcon = ({ field }: { field: SearchTrackField }) => {
    if (trackSortField !== field) return null;
    return trackSortDir === 'asc' ? <Icon.ChevronUp /> : <Icon.ChevronDown />;
  };

  const cols: { label: string; field: SearchTrackField; style?: React.CSSProperties }[] = [
    { label: 'Title',  field: 'title',    style: { flex: 3 } },
    { label: 'Artist', field: 'artist',   style: { flex: 2 } },
    { label: 'Album',  field: 'album',    style: { flex: 2 } },
    { label: 'Genre',  field: 'genre',    style: { flex: 1 } },
    { label: 'Year',   field: 'year',     style: { width: 60 } },
    { label: 'Dur',    field: 'duration', style: { width: 75, textAlign: 'right' } },
    { label: 'Kbps',   field: 'bitrate',  style: { width: 55, textAlign: 'right' } },
  ];

  const onRateArtist = useCallback(async (artistId: ClientEntityId, rating: number | null) => {
    await api.setArtistRating(artistId, rating);
    setResult(prev => prev ? {
      ...prev,
      artists: prev.artists.map(a => a.id === artistId ? { ...a, rating } : a),
    } : prev);
  }, []);

  const onRateAlbum = useCallback(async (albumId: ClientEntityId, rating: number | null) => {
    await api.setAlbumRating(albumId, rating);
    setResult(prev => prev ? {
      ...prev,
      albums: prev.albums.map(al => al.id === albumId ? { ...al, rating } : al),
    } : prev);
  }, []);

  const onRateTrack = useCallback(async (trackId: ClientEntityId, rating: number | null) => {
    await api.setTrackRating(trackId, rating);
    setResult(prev => prev ? {
      ...prev,
      tracks: prev.tracks.map(t => t.id === trackId ? { ...t, rating } : t),
    } : prev);
  }, []);

  const totalPages = result ? Math.ceil(result.total / 100) : 1;
  const displayedArtists = useMemo(() => result?.artists ?? [], [result?.artists]);
  const displayedAlbums = useMemo(() => result?.albums ?? [], [result?.albums]);
  const displayedTopResults = result?.top_results ?? [];
  const displayedTracks = useMemo(() => {
    if (!result?.tracks) return [];
    return trackSortField === 'rating'
      ? sortSearchTracks(result.tracks, trackSortField, trackSortDir)
      : result.tracks;
  }, [result?.tracks, trackSortField, trackSortDir]);
  const hasArtists = displayedArtists.length > 0;
  const hasAlbums  = displayedAlbums.length > 0;
  const hasTopResults = displayedTopResults.length > 0;
  const showQuick  = query.trim().length > 0 && (hasTopResults || hasArtists || hasAlbums);

  const openTopResult = useCallback((item: NonNullable<SearchResult['top_results']>[number]) => {
    if (item.type === 'artist') {
      const artist = displayedArtists.find((candidate) => candidate.id === item.id);
      if (artist) onOpenArtist(artist);
      return;
    }
    if (item.type === 'album') {
      const album = displayedAlbums.find((candidate) => candidate.id === item.id);
      if (album) onOpenAlbum(album);
      return;
    }
    const track = displayedTracks.find((candidate) => candidate.id === item.id);
    if (track) playTrack(track, displayedTracks);
  }, [displayedAlbums, displayedArtists, displayedTracks, onOpenAlbum, onOpenArtist, playTrack]);

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>

      {/* ── Filter bar ── */}
      <div style={S.filterBar}>
        <div style={S.searchWrap}>
          <Icon.Search />
          <input style={S.searchInput} placeholder="Search titles, artists, albums..."
            value={query} onChange={e => setQuery(e.target.value)} autoFocus />
          {query && <button style={S.clearBtn} onClick={() => setQuery('')}><Icon.X /></button>}
        </div>
        <select style={S.select} value={libraryId ?? ''} onChange={e => setLibraryId(e.target.value || undefined)}>
          <option value="">All Libraries</option>
          {libraries.map(l => <option key={l.id} value={l.id}>{l.name}</option>)}
        </select>
        <select style={S.select} value={genre} onChange={e => setGenre(e.target.value)}>
          <option value="">All Genres</option>
          {genres.map(g => <option key={g.genre} value={g.genre}>{g.genre} ({g.track_count})</option>)}
        </select>
        <input style={{ ...S.select, width: 90 }} placeholder="Year" value={year}
          onChange={e => setYear(e.target.value)} type="number" min="1900" max="2099" />
        <button
          style={{
            ...S.select,
            cursor: 'pointer',
            fontFamily: 'inherit',
            fontWeight: 700,
            whiteSpace: 'nowrap',
            border: sonicFingerprintOnly
              ? '1px solid color-mix(in srgb, var(--accent) 60%, var(--border))'
              : undefined,
            background: sonicFingerprintOnly
              ? 'color-mix(in srgb, var(--accent) 14%, var(--surface))'
              : undefined,
            color: sonicFingerprintOnly ? 'var(--accent)' : undefined,
          }}
          onClick={() => setSonicFingerprintOnly(v => !v)}
          title="Show only tracks with Sonic Fingerprint (AI stem analysis)"
          aria-pressed={sonicFingerprintOnly}
        >
          ✦ Sonic Fingerprint
        </button>
      </div>

      {/* ── Artists + Albums quick results ── */}
      {showQuick && (
        <div style={S.quickPanel}>
          {hasTopResults && (
            <div style={S.quickSection}>
              <div style={S.quickSectionLabel}>Top Results</div>
              {displayedTopResults.map((item) => (
                <button key={`${item.type}-${item.id}`} style={S.quickRow} onClick={() => openTopResult(item)}>
                  <span style={S.quickIcon}>
                    {item.type === 'artist' ? <ArtistRowIcon /> : item.type === 'album' ? <AlbumRowIcon /> : <Icon.Music />}
                  </span>
                  <span style={S.quickName}>{item.title}</span>
                  <span style={S.quickMeta}>{item.subtitle || item.type}</span>
                  <span style={S.quickArrow}><OpenIcon /></span>
                </button>
              ))}
            </div>
          )}
          {hasArtists && (
            <div style={S.quickSection}>
              <div style={S.quickSectionLabel}>Artists</div>
              {displayedArtists.map(artist => (
                <button key={artist.id} style={S.quickRow} onClick={() => onOpenArtist(artist)}>
                  <span style={S.quickIcon}><ArtistRowIcon /></span>
                  <span style={S.quickName}>{artist.name}</span>
                  <span style={S.quickMeta}>
                    {artist.album_count} {artist.album_count === 1 ? 'album' : 'albums'} · {artist.track_count} tracks
                  </span>
                  <span onClick={e => e.stopPropagation()} onMouseDown={e => e.stopPropagation()}>
                    <StarRating value={artist.rating ?? null} onChange={rating => onRateArtist(artist.id, rating)} ariaLabel={`${artist.name} artist rating`} size="compact" subdued={!artist.rating} />
                  </span>
                  <span style={S.quickArrow}><OpenIcon /></span>
                </button>
              ))}
            </div>
          )}
          {hasAlbums && (
            <div style={S.quickSection}>
              <div style={S.quickSectionLabel}>Albums</div>
              {displayedAlbums.map((album: Album) => (
                <button key={album.id} style={S.quickRow} onClick={() => onOpenAlbum(album)}>
                  <span style={S.quickIcon}><AlbumRowIcon /></span>
                  <span style={S.quickName}>{album.title}</span>
                  <span style={S.quickMeta}>
                    {[album.album_artist || album.artist, album.year].filter(Boolean).join(' · ')}
                    {album.track_count ? ` · ${album.track_count} tracks` : ''}
                  </span>
                  <span onClick={e => e.stopPropagation()} onMouseDown={e => e.stopPropagation()}>
                    <StarRating value={album.rating ?? null} onChange={rating => onRateAlbum(album.id, rating)} ariaLabel={`${album.title} album rating`} size="compact" subdued={!album.rating} />
                  </span>
                  <span style={S.quickArrow}><OpenIcon /></span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── Tracks section ── */}
      <div style={S.resultsMeta}>
        <div style={S.resultsMetaRow}>
          {result != null && (
            <span style={S.muted}>
              {loading ? 'Loading…' : [
                displayedTopResults.length ? `${displayedTopResults.length} top result${displayedTopResults.length !== 1 ? 's' : ''}` : '',
                displayedArtists.length  ? `${displayedArtists.length} artist${displayedArtists.length !== 1 ? 's' : ''}` : '',
                displayedAlbums.length  ? `${displayedAlbums.length} album${displayedAlbums.length !== 1 ? 's' : ''}` : '',
                `${displayedTracks.length.toLocaleString()} track${displayedTracks.length !== 1 ? 's' : ''}`,
              ].filter(Boolean).join(' · ')}
              {result.total > 100 && ` · Page ${page} of ${totalPages}`}
            </span>
          )}
        </div>
      </div>

      {showQuick && (
        <div style={S.sectionDivider}>
          <span style={S.sectionDividerLabel}>Tracks</span>
        </div>
      )}

      <>
      <div style={S.tableHeader}>
        <div style={{ ...S.thCell, width: 32, flexShrink: 0 }} />
        {cols.map(col => (
          <div key={col.field} style={{ ...S.thCell, ...col.style }} onClick={() => toggleSort(col.field)}>
            {col.label} <SortIcon field={col.field} />
          </div>
        ))}
        <div style={{ ...S.thCell, width: 120, justifyContent: 'center' }} onClick={() => toggleSort('rating')}>
          Rating <SortIcon field="rating" />
        </div>
      </div>

      <div style={S.tableBody}>
        {!displayedTracks.length && !loading && (
          <div style={{ padding: '40px 20px', textAlign: 'center', color: 'var(--text-muted)' }}>
            {result != null ? 'No tracks found.' : 'Search your library above.'}
          </div>
        )}
        {displayedTracks.map((track: Track, i: number) => (
          <div key={track.id}
            style={{
              ...S.tableRow,
              backgroundColor: hoveredTrackId === track.id
                ? 'color-mix(in srgb, var(--accent) 14%, transparent)'
                : i % 2 === 0 ? 'transparent' : 'rgba(255,255,255,0.02)',
            }}
            onClick={() => playTrack(track, displayedTracks)}
            onMouseEnter={() => setHoveredTrackId(track.id)}
            onMouseLeave={() => setHoveredTrackId((prev) => (prev === track.id ? null : prev))}
          >
            <div style={{ ...S.tdCell, width: 32, flexShrink: 0 }}>
              <button style={S.playRowBtn} title="Play"
                onClick={e => { e.stopPropagation(); playTrack(track, displayedTracks); }}>
                <Icon.Play />
              </button>
            </div>
            <div style={{ ...S.tdCell, flex: 3 }}><div style={S.trackTitle}>{track.title || track.file_name}</div></div>
            <div style={{ ...S.tdCell, flex: 2, color: 'var(--text-muted)' }}>{track.artist || '–'}</div>
            <div style={{ ...S.tdCell, flex: 2, color: 'var(--text-muted)' }}>{track.album || '–'}</div>
            <div style={{ ...S.tdCell, flex: 1, color: 'var(--text-muted)' }}>{track.genre || '–'}</div>
            <div style={{ ...S.tdCell, width: 60, color: 'var(--text-muted)' }}>{track.year || '–'}</div>
            <div style={{ ...S.tdCell, width: 75, textAlign: 'right', color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums' }}>{fmt(track.duration)}</div>
            <div style={{ ...S.tdCell, width: 55, textAlign: 'right', color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums' }}>
              {track.bitrate != null ? track.bitrate : '–'}
            </div>
            <div style={{ ...S.tdCell, width: 120, display: 'flex', justifyContent: 'center' }}
              onClick={e => e.stopPropagation()} onMouseDown={e => e.stopPropagation()}>
              <StarRating value={track.rating ?? null} onChange={rating => onRateTrack(track.id, rating)} ariaLabel={`${track.title || track.file_name} search rating`} size="compact" subdued={!track.rating} />
            </div>
            <div style={{ ...S.tdCell, width: 32, flexShrink: 0, display: 'flex', justifyContent: 'center' }}>
              <KebabButton
                target={{ kind: 'track', trackId: track.id, title: track.title || track.file_name }}
                callbacks={{ onPlay: () => playTrack(track, displayedTracks), onQueue: () => addToQueue(track) }}
                visible={hoveredTrackId === track.id}
              />
            </div>
          </div>
        ))}
      </div>
      </>

      {totalPages > 1 && (
        <div style={S.pagination}>
          <button style={S.pageBtn} disabled={page === 1} onClick={() => doSearch(page - 1)}>←</button>
          <span style={S.muted}>Page {page} / {totalPages}</span>
          <button style={S.pageBtn} disabled={page === totalPages} onClick={() => doSearch(page + 1)}>→</button>
        </div>
      )}

      {selectedTrack && (
        <TrackModal
          track={selectedTrack}
          onClose={() => setSelectedTrack(null)}
          onPlay={() => playTrack(selectedTrack, displayedTracks)}
          onQueue={() => addToQueue(selectedTrack)}
        />
      )}
    </div>
  );
}

// BrowseView moved to components/BrowseView.tsx

// ─── App Shell ────────────────────────────────────────────────────────────────

type View = 'home' | 'search' | 'browse' | 'settings' | 'playlists';
type PlaybackMode = 'standard' | 'vinyl';
const VINYL_SETTINGS_STORAGE_KEY = 'boogiebox.playback.vinyl.v1';
const SIDEBAR_COLLAPSED_STORAGE_KEY = 'boogiebox.sidebar.collapsed.v1';

function getStoredVinylSettings(): {
  playbackMode: PlaybackMode;
  hardcore: boolean;
  needleDrop: boolean;
  analogFxDisabled: boolean;
  needleDropIntensity: number;
} {
  if (typeof window === 'undefined') {
    return { playbackMode: 'standard', hardcore: false, needleDrop: false, analogFxDisabled: false, needleDropIntensity: 0.65 };
  }
  try {
    const raw = window.localStorage.getItem(VINYL_SETTINGS_STORAGE_KEY);
    if (!raw) return { playbackMode: 'standard', hardcore: false, needleDrop: false, analogFxDisabled: false, needleDropIntensity: 0.65 };
    const parsed = JSON.parse(raw) as Partial<{
      playbackMode: PlaybackMode;
      hardcore: boolean;
      needleDrop: boolean;
      analogFxDisabled: boolean;
      needleDropIntensity: number;
    }>;
    return {
      playbackMode: parsed.playbackMode === 'vinyl' ? 'vinyl' : 'standard',
      hardcore: parsed.hardcore === true,
      needleDrop: parsed.needleDrop === true,
      analogFxDisabled: parsed.analogFxDisabled === true,
      needleDropIntensity: Math.max(0, Math.min(1, Number.isFinite(parsed.needleDropIntensity) ? Number(parsed.needleDropIntensity) : 0.65)),
    };
  } catch {
    return { playbackMode: 'standard', hardcore: false, needleDrop: false, analogFxDisabled: false, needleDropIntensity: 0.65 };
  }
}

function getStoredSidebarCollapsed(): boolean {
  if (typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

/** App is part of this module's public API. */
export default function App() {
  const isMobileShell = useMobileShell();
  const [currentUser, setCurrentUser] = useState<AuthUser | null | 'loading'>('loading');
  const [view, setView]         = useState<View>('home');
  const [sidebarCollapsed, setSidebarCollapsed] = useState<boolean>(() => getStoredSidebarCollapsed());
  const [activeSidebarLibraryId, setActiveSidebarLibraryId] = useState<ClientEntityId | null>(null);
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [stats, setStats]       = useState<Stats | null>(null);
  const [homeRefreshKey, setHomeRefreshKey] = useState(0);
  const [ffmpegAvailable, setFfmpegAvailable] = useState<boolean | null>(null);
  const [setupRequired, setSetupRequired] = useState<boolean>(false);
  const [serverVersion, setServerVersion] = useState<string | null>(null);
  const [streamDirect, setStreamDirect] = useState(() => getStreamDirect());
  const [adaptiveAccentEnabled, setAdaptiveAccentEnabled] = useState<boolean>(true);
  const [playbackMode, setPlaybackMode] = useState<PlaybackMode>(() => getStoredVinylSettings().playbackMode);
  const [vinylHardcore, setVinylHardcore] = useState<boolean>(() => getStoredVinylSettings().hardcore);
  const [vinylNeedleDrop, setVinylNeedleDrop] = useState<boolean>(() => getStoredVinylSettings().needleDrop);
  const [vinylAnalogFxDisabled, setVinylAnalogFxDisabled] = useState<boolean>(() => getStoredVinylSettings().analogFxDisabled);
  const [vinylNeedleDropIntensity, setVinylNeedleDropIntensity] = useState<number>(() => getStoredVinylSettings().needleDropIntensity);
  const [playerState, setPlayerState] = useState<PlayerState>({ queue: [], currentIndex: 0, isPlaying: false, playToken: 0 });
  const [settings, setSettings] = useState<AppSettings>(() => ({ ...DEFAULT_SETTINGS }));
  const [browseOpenAlbumRequest, setBrowseOpenAlbumRequest] =
    useState<{ album: Album; token: number } | null>(null);
  const [browseOpenArtistRequest, setBrowseOpenArtistRequest] =
    useState<{ artist: Artist; token: number } | null>(null);
  const [browseOpenGenreRequest, setBrowseOpenGenreRequest] =
    useState<{ genre: string; token: number } | null>(null);
  const [openPlaylistRequest, setOpenPlaylistRequest] =
    useState<{ playlistId: EntityId; token: number } | null>(null);
  const [playbackSnapshot, setPlaybackSnapshot] = useState<PlaybackSnapshot | null>(null);
  const lastRecordedPlayKeyRef = useRef<string>('');
  const themeSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Apply theme on mount and whenever settings change
  useEffect(() => { applyTheme(settings); }, [settings]);

  // Persist appearance per-user: localStorage for instant load, server for cross-browser sync.
  useEffect(() => {
    if (!currentUser || currentUser === 'loading') return;
    const userId = (currentUser as AuthUser).id;
    saveThemeToStorage(settings, userId);
    if (themeSaveTimerRef.current) clearTimeout(themeSaveTimerRef.current);
    themeSaveTimerRef.current = setTimeout(() => {
      api.userSettings.update({ theme: JSON.stringify(extractThemeSettings(settings)) }).catch(() => {});
    }, 1000);
  }, [currentUser, settings]);

  useEffect(() => {
    if (!currentUser || currentUser === 'loading') return;
    saveAdaptiveAccentToStorage(adaptiveAccentEnabled, (currentUser as AuthUser).id);
    api.userSettings.update({ adaptiveAccent: String(adaptiveAccentEnabled) }).catch(() => {});
  }, [adaptiveAccentEnabled, currentUser]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(
        VINYL_SETTINGS_STORAGE_KEY,
        JSON.stringify({
          playbackMode,
          hardcore: vinylHardcore,
          needleDrop: vinylNeedleDrop,
          analogFxDisabled: vinylAnalogFxDisabled,
          needleDropIntensity: vinylNeedleDropIntensity,
        }),
      );
    } catch {
      // Best effort only.
    }
  }, [playbackMode, vinylHardcore, vinylNeedleDrop, vinylAnalogFxDisabled, vinylNeedleDropIntensity]);

  useEffect(() => {
    if (typeof document === 'undefined' || typeof window === 'undefined') return;
    const serverName = window.location.hostname?.trim() || 'localhost';
    document.title = `BoogieBox - ${serverName}`;
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, sidebarCollapsed ? 'true' : 'false');
    } catch {
      // Best effort only.
    }
  }, [sidebarCollapsed]);

  // Check auth and setup state on startup
  useEffect(() => {
    api.auth.me().then(user => setCurrentUser(user)).catch(() => setCurrentUser(null));
    api.systemStatus().then(s => { setSetupRequired(s.setupRequired ?? false); if (s.version) setServerVersion(s.version); }).catch(() => {});
  }, []);

  // Load user-specific theme and shared server settings when user changes.
  useEffect(() => {
    if (!currentUser || currentUser === 'loading') return;
    const userId = (currentUser as AuthUser).id;
    // Apply localStorage immediately to avoid flash, then override with server (cross-browser source of truth).
    setSettings(prev => ({ ...DEFAULT_SETTINGS, ...prev, ...(getStoredTheme(userId) ?? {}) }));
    setAdaptiveAccentEnabled(getStoredAdaptiveAccentEnabled(userId));
    api.userSettings.get().then(userSettings => {
      if (userSettings.theme) {
        const serverTheme = parseThemeSettings(userSettings.theme);
        if (serverTheme) {
          setSettings(prev => ({ ...DEFAULT_SETTINGS, ...prev, ...serverTheme }));
          saveThemeToStorage({ ...DEFAULT_SETTINGS, ...serverTheme } as AppSettings, userId);
        }
      }
      if (userSettings.adaptiveAccent !== undefined) {
        const val = userSettings.adaptiveAccent !== 'false';
        setAdaptiveAccentEnabled(val);
        saveAdaptiveAccentToStorage(val, userId);
      }
    }).catch(() => {});
    api.playbackSettings().then(shared => {
      setSettings(prev => ({
        ...prev,
        ...(shared.transcodeQuality != null ? { transcodeQuality: shared.transcodeQuality } : {}),
        ...(shared.replayGainEnabled != null ? { replayGainEnabled: shared.replayGainEnabled } : {}),
        ...(shared.vinylMode != null ? { vinylMode: shared.vinylMode } : {}),
        ...(shared.lastfmConfigured != null ? { lastfmConfigured: shared.lastfmConfigured } : {}),
      }));
      if (shared.vinylMode === 'vinyl' || shared.vinylMode === 'standard') {
        setPlaybackMode(shared.vinylMode);
      }
    }).catch(() => {});
  }, [currentUser]);

  const playTrack = (track: Track, allTracks?: Track[], source?: QueueSource) => {
    const queue = allTracks ?? [track];
    const idx = Math.max(0, queue.findIndex(t => t.id === track.id));
    if (playbackMode === 'vinyl') {
      setPlaybackMode('standard');
    }
    setPlayerState(prev => ({
      queue, currentIndex: idx, isPlaying: true, playToken: prev.playToken + 1,
      queueSource: source ?? { type: 'single', id: track.id },
    }));
  };

  const playAlbumInVinylMode = useCallback((tracks: Track[], albumId: ClientEntityId) => {
    if (!tracks.length) return;
    setPlaybackMode('vinyl');
    setPlayerState(prev => ({
      queue: tracks,
      currentIndex: 0,
      isPlaying: true,
      playToken: prev.playToken + 1,
      queueSource: { type: 'album', id: albumId },
    }));
  }, []);

  const startAutoDj = useCallback(async (selectedGenres: string[], libraryId?: ClientEntityId): Promise<number> => {
    const cleanedGenres = selectedGenres.map((genre) => genre.trim()).filter(Boolean);
    if (!cleanedGenres.length) {
      throw new Error('Select at least one genre for Auto DJ.');
    }

    const response = await api.autoDjTracks({
      genres: cleanedGenres,
      library_id: libraryId,
      limit: 200,
    });
    const queue = response.tracks ?? [];
    if (!queue.length) {
      throw new Error('No tracks found for the selected genres.');
    }

    if (playbackMode === 'vinyl') {
      setPlaybackMode('standard');
    }
    setPlayerState((prev) => ({
      queue,
      currentIndex: 0,
      isPlaying: true,
      playToken: prev.playToken + 1,
      queueSource: { type: 'autodj', id: '0' },
    }));
    return queue.length;
  }, [playbackMode]);

  useEffect(() => {
    const current = playerState.queue[playerState.currentIndex];
    if (!shouldRecordTrackPlay(current)) return;
    const key = `${playerState.playToken}:${playerState.currentIndex}:${current.id}`;
    if (lastRecordedPlayKeyRef.current === key) return;
    lastRecordedPlayKeyRef.current = key;
    api.markTrackPlayed(current.id).catch(() => {});
  }, [playerState.playToken, playerState.currentIndex, playerState.queue]);

  const addToQueue = (track: Track) =>
    setPlayerState(prev => {
      if (playbackMode === 'vinyl') return prev;
      return { ...prev, queue: [...prev.queue, track] };
    });
  const nowPlayingTrack = playerState.queue[playerState.currentIndex] ?? null;

  const openArtistFromPlayer = useCallback(async (artistName: string) => {
    const normalized = normalizeArtistLookupName(artistName);
    if (!normalized) return;
    try {
      const artists = await api.artists();
      const matchedArtist = findArtistByName(artists, artistName);
      if (!matchedArtist) return;
      setBrowseOpenArtistRequest({ artist: matchedArtist, token: Date.now() });
      setView('browse');
    } catch {
      // Ignore lookup failures from player shortcut navigation.
    }
  }, []);

  const openAlbumFromPlayer = useCallback(async (albumTitle: string, artistName?: string | null) => {
    const normalizedTitle = normalizeAlbumLookupTitle(albumTitle);
    if (!normalizedTitle) return;
    try {
      const albums = await api.albums({ group_by: 'album_artist' });
      const matchedAlbum = findAlbumByName(albums, albumTitle, artistName);
      if (!matchedAlbum) return;
      setBrowseOpenAlbumRequest({ album: matchedAlbum, token: Date.now() });
      setView('browse');
    } catch {
      // Ignore lookup failures from player shortcut navigation.
    }
  }, []);

  const refresh = useCallback(async () => {
    const [libsRes, statsRes, sysRes] = await Promise.allSettled([
      api.libraries.list(), api.stats(), api.systemStatus(),
    ]);
    const libs = fulfilledValue(libsRes);
    const st = fulfilledValue(statsRes);
    const sys = fulfilledValue(sysRes);
    if (libs) setLibraries(libs);
    if (st) setStats(st);
    setHomeRefreshKey((prev) => prev + 1);
    if (sys) {
      setFfmpegAvailable(sys.ffmpegAvailable);
      setSetupRequired(sys.setupRequired ?? false);
      if (sys.version) setServerVersion(sys.version);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const navItems: { id: View; label: string; icon: React.ReactNode }[] = [
    { id: 'home',         label: 'Home',         icon: <Icon.Home /> },
    { id: 'search',       label: 'Search',       icon: <Icon.Search /> },
    { id: 'browse',       label: 'Browse Music', icon: <Icon.Browse /> },
    { id: 'playlists',    label: 'Playlists',    icon: <Icon.Playlist /> },
    { id: 'settings',     label: 'Settings',     icon: <Icon.Settings /> },
  ];

  const handleLogout = async () => {
    await api.auth.logout().catch(() => {});
    setCurrentUser(null);
  };

  const clearSidebarLibrarySelection = useCallback(() => {
    setActiveSidebarLibraryId(null);
  }, []);
  const activeSidebarLibrary = activeSidebarLibraryId != null
    ? libraries.find((library) => library.id === activeSidebarLibraryId) ?? null
    : null;
  const activeSidebarLibraryType = activeSidebarLibrary?.library_type ?? 'music';

  const openView = useCallback((nextView: View) => {
    if (nextView === 'browse') {
      setActiveSidebarLibraryId(null);
    } else {
      clearSidebarLibrarySelection();
    }
    setView(nextView);
  }, [clearSidebarLibrarySelection]);

  const openLibraryBrowse = useCallback((libraryId: ClientEntityId) => {
    setActiveSidebarLibraryId(libraryId);
    setView('browse');
  }, []);

  // Auth gate
  if (currentUser === 'loading') return null;
  if (setupRequired) {
    return <SetupView onComplete={() => { setSetupRequired(false); refresh(); }} />;
  }
  if (currentUser === null) {
    return <LoginScreen onLogin={user => { setCurrentUser(user); refresh(); }} />;
  }

  if (isMobileShell) {
    return (
      <MobileApp
        currentUser={currentUser as AuthUser}
        libraries={libraries}
        settings={settings}
        ffmpegAvailable={ffmpegAvailable}
        playerState={playerState}
        playbackSnapshot={playbackSnapshot}
        openPlaylistId={openPlaylistRequest?.playlistId ?? null}
        onPlaybackStateChange={setPlayerState}
        onPlayTrack={playTrack}
        onAddToQueue={addToQueue}
        onConsumeOpenPlaylist={() => setOpenPlaylistRequest(null)}
      />
    );
  }

  return (
    <div style={S.root}>
      {/* Body row: sidebar + main */}
      <div style={S.body}>
        {/* Sidebar */}
        <aside style={{ ...S.sidebar, ...(sidebarCollapsed ? S.sidebarCollapsed : {}) }}>
          <div
            style={{ ...S.logo, ...(sidebarCollapsed ? S.logoCollapsed : {}) }}
            title={`BoogieBox v${serverVersion ?? APP_VERSION}`}
          >
            <img src="/boogiebox.png" alt="BoogieBox logo" style={S.logoImage} />
            {!sidebarCollapsed && (
              <div>
                <div>BoogieBox</div>
                <div style={S.logoMetaRow}>
                  <span style={S.logoVersion}>v{serverVersion ?? APP_VERSION}</span>
                  <a
                    href="https://ko-fi.com/yronnen"
                    target="_blank"
                    rel="noreferrer"
                    aria-label="Support BoogieBox on Ko-fi"
                    title="Support BoogieBox on Ko-fi"
                    style={S.logoKofiLink}
                  >
                    <img src="/kofi_symbol.png" alt="" aria-hidden="true" style={S.logoKofiImage} />
                  </a>
                </div>
              </div>
            )}
          </div>
          <button
            type="button"
            aria-label={sidebarCollapsed ? 'Expand left menu' : 'Collapse left menu'}
            title={sidebarCollapsed ? 'Expand menu' : 'Collapse menu'}
            style={{ ...S.sidebarToggle, ...(sidebarCollapsed ? S.sidebarToggleCollapsed : {}) }}
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
          >
            {sidebarCollapsed ? <Icon.ChevronRight /> : <Icon.ChevronLeft />}
          </button>
          <nav style={{ ...S.nav, ...(sidebarCollapsed ? S.navCollapsed : {}) }}>
            {navItems.map(({ id, label, icon }) => {
              const isActive = view === id;
              return (
                <button
                  key={id}
                  type="button"
                  aria-current={isActive ? 'page' : undefined}
                  aria-label={label}
                  title={label}
                  style={{ ...S.navItem, ...(sidebarCollapsed ? S.navItemCollapsed : {}), ...(isActive ? S.navItemActive : {}) }}
                  onClick={() => openView(id)}
                >
                  {icon}
                  {!sidebarCollapsed && label}
                </button>
              );
            })}
            <div style={S.sidebarSection}>
              {!sidebarCollapsed && <div style={S.sidebarSectionLabel}>Libraries</div>}
              <div style={S.libraryList}>
                {libraries.length === 0 ? (
                  !sidebarCollapsed && <div style={S.libraryEmpty}>No libraries yet</div>
                ) : (
                  libraries.map((library) => {
                    const isActive = view === 'browse' && activeSidebarLibraryId === library.id;
                    return (
                      <button
                        key={library.id}
                        type="button"
                        aria-pressed={isActive}
                        aria-label={library.name}
                        style={{ ...S.libraryNavItem, ...(sidebarCollapsed ? S.libraryNavItemCollapsed : {}), ...(isActive ? S.libraryNavItemActive : {}) }}
                        onClick={() => openLibraryBrowse(library.id)}
                        title={library.name}
                      >
                        <span style={S.libraryNavIcon}><Icon.Browse /></span>
                        {!sidebarCollapsed && <span style={S.libraryNavText}>{library.name}</span>}
                      </button>
                    );
                  })
                )}
              </div>
            </div>
          </nav>
          {!sidebarCollapsed && <div style={S.sidebarBottom}>
            <div style={S.sidebarStats}>
              <div style={{ color: 'var(--accent)', fontWeight: 700, fontSize: 20 }}>{stats?.total_tracks?.toLocaleString() ?? '–'}</div>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Tracks</div>
            </div>
            <div style={S.sidebarStats}>
              <div style={{ color: 'var(--accent)', fontWeight: 700, fontSize: 20 }}>{stats?.total_artists?.toLocaleString() ?? '–'}</div>
              <div style={{ color: 'var(--text-muted)', fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Artists</div>
            </div>
          </div>}
          {(streamDirect || ffmpegAvailable !== null) && (
            <div style={{ padding: sidebarCollapsed ? '8px 0' : '8px 16px', borderTop: '1px solid var(--border)' }}>
              {streamDirect ? (
                <div
                  style={{ display: 'flex', alignItems: 'center', justifyContent: sidebarCollapsed ? 'center' : 'flex-start', gap: 6, fontSize: 10, color: '#71717a' }}
                  title="Transcoding off"
                >
                  <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: '#71717a', flexShrink: 0 }} />
                  {!sidebarCollapsed && 'Transcoding off'}
                </div>
              ) : (
                <div
                  style={{ display: 'flex', alignItems: 'center', justifyContent: sidebarCollapsed ? 'center' : 'flex-start', gap: 6, fontSize: 10, color: ffmpegAvailable ? '#22c55e' : '#f59e0b' }}
                  title={ffmpegAvailable
                    ? `Transcoding on (${settings.transcodeQuality === 'high' ? '320 kbps' : '192 kbps'})`
                    : 'No ffmpeg'}
                >
                  <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: ffmpegAvailable ? '#22c55e' : '#f59e0b', flexShrink: 0 }} />
                  {!sidebarCollapsed && (ffmpegAvailable
                    ? `Transcoding on (${settings.transcodeQuality === 'high' ? '320 kbps' : '192 kbps'})`
                    : 'No ffmpeg')}
                </div>
              )}
            </div>
          )}
          {/* User info + logout */}
          <div style={{ ...S.userFooter, ...(sidebarCollapsed ? S.userFooterCollapsed : {}) }}>
            {!sidebarCollapsed && (
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {(currentUser as AuthUser).username}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-muted)' }}>{(currentUser as AuthUser).role}</div>
              </div>
            )}
            <button
              onClick={handleLogout}
              title={`Log out ${(currentUser as AuthUser).username}`}
              aria-label={`Log out ${(currentUser as AuthUser).username}`}
              style={{ ...S.logoutButton, ...(sidebarCollapsed ? S.logoutButtonCollapsed : {}) }}
            >
              ⏻
            </button>
          </div>
        </aside>

        {/* Main */}
        <main style={S.main}>
          {view !== 'settings' && view !== 'playlists' && view !== 'home' && <StatsBar stats={stats} />}
          {view === 'home'      && (
            <HomeView
              stats={stats}
              libraries={libraries}
              refreshKey={homeRefreshKey}
              onOpenArtist={(artist) => {
                setBrowseOpenArtistRequest({ artist, token: Date.now() });
                clearSidebarLibrarySelection();
                setView('browse');
              }}
              onOpenAlbum={async (album) => {
                let targetAlbum = album;
                try {
                  const browseAlbums = await api.albums({ group_by: 'album_artist' });
                  const exactById = browseAlbums.find((candidate) => candidate.id === album.id);
                  if (exactById) {
                    targetAlbum = exactById;
                  } else {
                    const resolved = findAlbumByName(
                      browseAlbums,
                      album.title,
                      album.album_artist || album.artist,
                    );
                    if (resolved) targetAlbum = resolved;
                  }
                } catch {
                  // Fallback to the Home payload when browse album lookup fails.
                }
                setBrowseOpenAlbumRequest({ album: targetAlbum, token: Date.now() });
                clearSidebarLibrarySelection();
                setView('browse');
              }}
              onOpenGenre={(genre) => {
                const request = createBrowseGenreRequest(genre);
                if (!request) return;
                setBrowseOpenGenreRequest(request);
                clearSidebarLibrarySelection();
                setView('browse');
              }}
              onBrowseMusic={() => {
                clearSidebarLibrarySelection();
                setView('browse');
              }}
              onOpenPlaylist={(playlistId) => {
                setOpenPlaylistRequest({ playlistId, token: Date.now() });
                setView('playlists');
              }}
              onPlayTrack={(track, allTracks) => {
                playTrack(track, allTracks ?? [track]);
              }}
              onStartAutoDj={(selectedGenres) => startAutoDj(selectedGenres)}
            />
          )}
          {view === 'search'    && (
            <SearchView
              libraries={libraries}
              playTrack={playTrack}
              addToQueue={addToQueue}
              onOpenArtist={artist => { setBrowseOpenArtistRequest({ artist, token: Date.now() }); clearSidebarLibrarySelection(); setView('browse'); }}
              onOpenAlbum={album   => { setBrowseOpenAlbumRequest({ album, token: Date.now() }); clearSidebarLibrarySelection(); setView('browse'); }}
            />
          )}
          {view === 'browse'    && (
              <BrowseView
                libraries={libraries}
                forcedLibraryIds={activeSidebarLibraryId ? [activeSidebarLibraryId] : null}
                playTrack={playTrack}
                playAlbumInVinylMode={playAlbumInVinylMode}
                addToQueue={addToQueue}
                lastfmKey={settings.lastfmKey}
                openAlbumRequest={browseOpenAlbumRequest}
                openArtistRequest={browseOpenArtistRequest}
                openGenreRequest={browseOpenGenreRequest}
                adaptiveAccentEnabled={adaptiveAccentEnabled}
              />
          )}
          {view === 'playlists' && (
            <PlaylistsView
              playTrack={playTrack}
              addToQueue={addToQueue}
              initialPlaylistId={openPlaylistRequest?.playlistId ?? null}
            />
          )}
          {view === 'settings'  && (
            <SettingsPage
              currentUser={currentUser as AuthUser}
              onLogout={handleLogout}
              settings={settings}
              libraries={libraries}
              onLibrariesRefresh={refresh}
              onSettingsChange={setSettings}
              onStreamDirectChange={setStreamDirect}
              adaptiveAccentEnabled={adaptiveAccentEnabled}
              onAdaptiveAccentEnabledChange={setAdaptiveAccentEnabled}
              playbackMode={playbackMode}
              onPlaybackModeChange={setPlaybackMode}
              vinylHardcore={vinylHardcore}
              onVinylHardcoreChange={setVinylHardcore}
              vinylNeedleDrop={vinylNeedleDrop}
              onVinylNeedleDropChange={setVinylNeedleDrop}
              vinylAnalogFxDisabled={vinylAnalogFxDisabled}
              onVinylAnalogFxDisabledChange={setVinylAnalogFxDisabled}
              vinylNeedleDropIntensity={vinylNeedleDropIntensity}
              onVinylNeedleDropIntensityChange={setVinylNeedleDropIntensity}
            />
          )}
        </main>
      </div>

      <Player
        state={playerState}
        onStateChange={setPlayerState}
        ffmpegAvailable={ffmpegAvailable}
        onOpenArtist={openArtistFromPlayer}
        onOpenAlbum={openAlbumFromPlayer}
        playbackMode={playbackMode}
        vinylHardcore={vinylHardcore}
        vinylNeedleDrop={vinylNeedleDrop}
        vinylAnalogFxDisabled={vinylAnalogFxDisabled}
        vinylNeedleDropIntensity={vinylNeedleDropIntensity}
        onPlaybackSnapshotChange={setPlaybackSnapshot}
      />
      <ContextMenuRoot />
    </div>
  );
}

// ─── Global styles (uses CSS vars set by applyTheme) ─────────────────────────

const S: Record<string, React.CSSProperties> = {
  root: {
    display: 'flex', flexDirection: 'column', height: '100vh',
    backgroundColor: 'var(--bg)',
    backgroundImage: [
      'radial-gradient(circle at top left, color-mix(in srgb, var(--accent) 15%, transparent) 0%, transparent 38%)',
      'radial-gradient(circle at 85% 12%, rgba(255,255,255,0.05) 0%, transparent 28%)',
      'linear-gradient(135deg, color-mix(in srgb, var(--surface) 40%, var(--bg)) 0%, var(--bg) 52%, color-mix(in srgb, var(--accent) 7%, var(--bg)) 100%)',
      'var(--bg-texture-image)',
    ].join(','),
    backgroundSize: 'auto, auto, auto, var(--bg-texture-size)',
    backgroundRepeat: 'repeat',
    color: 'var(--text)',
    fontFamily: 'var(--font), system-ui, sans-serif',
    fontSize: 13,
    overflow: 'hidden',
  },
  body: {
    display: 'flex', flexDirection: 'row', flex: 1, minHeight: 0, overflow: 'hidden',
  },
  sidebar: {
    width: 224,
    backgroundColor: 'color-mix(in srgb, var(--surface) 92%, var(--bg))',
    borderRight: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
    display: 'flex', flexDirection: 'column', flexShrink: 0,
    boxShadow: 'inset -1px 0 0 rgba(255,255,255,0.03)',
  },
  sidebarCollapsed: {
    width: 72,
  },
  logo: {
    display: 'flex', alignItems: 'center', gap: 12,
    padding: '24px 18px 20px',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    fontWeight: 700, fontSize: 20, color: 'var(--text)', letterSpacing: '-0.7px',
  },
  logoCollapsed: {
    justifyContent: 'center',
    padding: '20px 0 16px',
  },
  logoVersion: {
    fontSize: 11,
    color: 'var(--text-muted)',
    fontWeight: 500,
    letterSpacing: 0.2,
  },
  logoMetaRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    marginTop: 1,
  },
  logoKofiLink: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 16,
    height: 16,
    borderRadius: 4,
    opacity: 0.5,
    lineHeight: 0,
  },
  logoKofiImage: {
    display: 'block',
    width: 13,
    height: 13,
    objectFit: 'contain',
  },
  logoImage: {
    width: 46,
    height: 46,
    objectFit: 'contain',
    flexShrink: 0,
  },
  nav: { padding: '18px 12px', flex: 1 },
  navCollapsed: {
    padding: '14px 10px',
  },
  sidebarToggle: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    alignSelf: 'flex-end',
    width: 30,
    height: 30,
    margin: '8px 12px 0 0',
    backgroundColor: 'transparent',
    border: '1px solid color-mix(in srgb, var(--border) 70%, transparent)',
    color: 'var(--text-muted)',
    borderRadius: 6,
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  sidebarToggleCollapsed: {
    alignSelf: 'center',
    margin: '8px 0 0',
  },
  sidebarSection: {
    marginTop: 18,
    paddingTop: 14,
    borderTop: '1px solid color-mix(in srgb, var(--border) 58%, transparent)',
  },
  sidebarSectionLabel: {
    padding: '0 14px 10px',
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 700,
    letterSpacing: 1.1,
    textTransform: 'uppercase',
  },
  libraryList: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  navItem: {
    display: 'flex', alignItems: 'center', gap: 12,
    width: '100%', padding: '11px 14px', marginBottom: 6,
    backgroundColor: 'transparent', border: 'none', color: 'var(--text-muted)',
    cursor: 'pointer', borderRadius: 14, fontSize: 14, textAlign: 'left',
    fontFamily: 'inherit', transition: 'all 0.15s',
    WebkitTapHighlightColor: 'transparent',
    letterSpacing: 0.1,
  },
  navItemCollapsed: {
    justifyContent: 'center',
    gap: 0,
    width: 48,
    height: 44,
    padding: 0,
    borderRadius: 10,
  },
  navItemActive: {
    backgroundColor: 'color-mix(in srgb, var(--accent) 16%, transparent)',
    color: 'color-mix(in srgb, var(--text) 78%, var(--accent))',
    boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--accent) 20%, transparent)',
  },
  libraryNavItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    width: '100%',
    padding: '9px 14px',
    backgroundColor: 'transparent',
    border: 'none',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    borderRadius: 12,
    fontSize: 13,
    textAlign: 'left',
    fontFamily: 'inherit',
    transition: 'all 0.15s',
    WebkitTapHighlightColor: 'transparent',
  },
  libraryNavItemCollapsed: {
    justifyContent: 'center',
    gap: 0,
    width: 48,
    height: 42,
    padding: 0,
    borderRadius: 10,
  },
  libraryNavItemActive: {
    backgroundColor: 'color-mix(in srgb, var(--accent) 12%, transparent)',
    color: 'var(--text)',
    boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent)',
  },
  libraryNavIcon: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 18,
    height: 18,
    flexShrink: 0,
    opacity: 0.8,
  },
  libraryNavText: {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    minWidth: 0,
  },
  libraryEmpty: {
    padding: '4px 14px 0',
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  sidebarBottom: {
    padding: '18px',
    borderTop: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    display: 'flex',
    gap: 18,
  },
  sidebarStats: { textAlign: 'center' },
  userFooter: {
    padding: '10px 16px',
    borderTop: '1px solid var(--border)',
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  },
  userFooterCollapsed: {
    justifyContent: 'center',
    padding: '10px 0',
  },
  logoutButton: {
    background: 'transparent',
    border: 'none',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    padding: 4,
    fontSize: 16,
    lineHeight: 1,
  },
  logoutButtonCollapsed: {
    width: 44,
    height: 44,
    padding: 0,
    borderRadius: 10,
  },
  main: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column',
    minWidth: 0,
    overflow: 'hidden',
    backgroundColor: 'color-mix(in srgb, var(--bg) 88%, transparent)',
  },
  statsBar: {
    display: 'flex',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 60%, transparent)',
    flexShrink: 0,
    backgroundColor: 'color-mix(in srgb, var(--surface) 24%, transparent)',
    backdropFilter: 'blur(12px)',
  },
  statItem: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    padding: '14px 24px',
    borderRight: '1px solid color-mix(in srgb, var(--border) 58%, transparent)',
  },
  statValue: { fontWeight: 700, fontSize: 20, color: 'var(--text)', fontVariantNumeric: 'tabular-nums', letterSpacing: '-0.4px' },
  statLabel: { fontSize: 10, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.9, marginTop: 3 },
  filterBar: {
    display: 'flex',
    gap: 10,
    padding: '22px 24px 16px',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 56%, transparent)',
    flexWrap: 'wrap',
    flexShrink: 0,
    position: 'relative' as const,
    alignItems: 'flex-start',
    background: [
      'radial-gradient(circle at top left, color-mix(in srgb, var(--accent) 16%, transparent) 0%, transparent 34%)',
      'linear-gradient(180deg, color-mix(in srgb, var(--surface) 28%, transparent) 0%, color-mix(in srgb, var(--surface) 12%, transparent) 100%)',
    ].join(','),
  },
  searchWrap: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    flex: 1,
    minWidth: 220,
    backgroundColor: 'color-mix(in srgb, var(--surface) 82%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    borderRadius: 999,
    padding: '0 16px',
    minHeight: 48,
    boxShadow: '0 12px 24px rgba(0,0,0,0.08)',
  },
  searchModeBar: { display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' as const },
  searchModeBtn: {
    minHeight: 38,
    padding: '0 14px',
    borderRadius: 999,
    border: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 62%, var(--bg))',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 700,
  },
  searchModeBtnActive: {
    background: 'color-mix(in srgb, var(--accent) 18%, var(--surface))',
    color: 'var(--text)',
    borderColor: 'color-mix(in srgb, var(--accent) 44%, var(--border))',
  },
  searchInput: { flex: 1, background: 'transparent', border: 'none', outline: 'none', color: 'var(--text)', fontSize: 14, padding: '12px 0', fontFamily: 'inherit' },
  clearBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: 0 },
  select: {
    backgroundColor: 'color-mix(in srgb, var(--surface) 64%, var(--bg))',
    border: '1px solid color-mix(in srgb, var(--border) 70%, transparent)',
    color: 'var(--text-muted)',
    borderRadius: 999,
    padding: '9px 13px',
    fontSize: 13,
    fontFamily: 'inherit',
    outline: 'none',
  },
  resultsMeta: { padding: '12px 20px 8px', flexShrink: 0 },
  resultsMetaRow: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' as const },
  muted: { color: 'var(--text-muted)' },
  tableHeader: { display: 'flex', padding: '10px 20px', borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)', flexShrink: 0, background: 'color-mix(in srgb, var(--surface) 42%, transparent)' },
  thCell: { display: 'flex', alignItems: 'center', gap: 4, color: 'var(--text-muted)', fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.9, cursor: 'pointer', userSelect: 'none', padding: '0 6px', fontWeight: 700 },
  tableBody: { flex: 1, overflowY: 'auto', minHeight: 0 },
  tableRow: { display: 'flex', padding: '10px 20px', cursor: 'pointer', borderBottom: '1px solid color-mix(in srgb, var(--border) 46%, transparent)', transition: 'background 0.1s' },
  tdCell: { padding: '0 4px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackTitle: { fontWeight: 700, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  pagination: { display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 16, padding: 12, borderTop: '1px solid var(--border)', flexShrink: 0 },
  pageBtn: { background: 'var(--surface)', border: '1px solid var(--border)', color: 'var(--text-muted)', borderRadius: 6, padding: '4px 14px', cursor: 'pointer', fontFamily: 'inherit' },
  panel: { padding: 24, maxWidth: 800, flex: 1, overflowY: 'auto', paddingBottom: 24 },
  panelTitle: { fontSize: 18, fontWeight: 700, marginBottom: 20, color: 'var(--text)' },
  addForm: { display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' },
  input: { flex: 1, backgroundColor: 'var(--bg)', border: '1px solid var(--border)', color: 'var(--text)', borderRadius: 6, padding: '8px 12px', fontSize: 13, fontFamily: 'inherit', outline: 'none' },
  btnPrimary: { display: 'flex', alignItems: 'center', gap: 6, backgroundColor: 'var(--accent)', color: '#fff', border: 'none', borderRadius: 6, padding: '8px 16px', cursor: 'pointer', fontSize: 13, fontFamily: 'inherit', fontWeight: 600 },
  btnSecondary: { display: 'flex', alignItems: 'center', gap: 6, backgroundColor: 'var(--bg)', color: 'var(--text-muted)', border: '1px solid var(--border)', borderRadius: 6, padding: '6px 12px', cursor: 'pointer', fontSize: 12, fontFamily: 'inherit' },
  btnDanger: { display: 'flex', alignItems: 'center', backgroundColor: 'transparent', color: 'var(--text-muted)', border: '1px solid var(--border)', borderRadius: 6, padding: '6px 8px', cursor: 'pointer' },
  errorMsg: { color: '#ef4444', marginTop: 8, fontSize: 12 },
  libCard: { backgroundColor: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, padding: '14px 16px', marginBottom: 10 },
  libCardHeader: { display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 12 },
  libName: { fontWeight: 600, color: 'var(--text)', marginBottom: 2 },
  libPath: { color: 'var(--accent)', fontSize: 12, marginBottom: 4 },
  libMeta: { color: 'var(--text-muted)', fontSize: 11 },
  libActions: { display: 'flex', gap: 6, flexShrink: 0 },
  progressWrap: { marginTop: 10 },
  progressBar: { height: 4, backgroundColor: 'var(--border)', borderRadius: 2, overflow: 'hidden' },
  progressFill: { height: '100%', transition: 'width 0.3s', borderRadius: 2 },
  progressText: { fontSize: 11, color: 'var(--text-muted)', marginTop: 4 },
  overlay: { position: 'fixed', inset: 0, backgroundColor: 'rgba(0,0,0,0.7)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 100 },
  modal: { backgroundColor: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 10, width: '90%', maxWidth: 600, maxHeight: '80vh', display: 'flex', flexDirection: 'column', overflow: 'hidden' },
  modalHeader: { display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', padding: '16px 20px', borderBottom: '1px solid var(--border)' },
  modalTitle: { fontWeight: 700, fontSize: 16, color: 'var(--text)', marginBottom: 4 },
  modalSub: { color: 'var(--text-muted)', fontSize: 13 },
  closeBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: 4, borderRadius: 4 },
  modalBody: { overflowY: 'auto', padding: '16px 20px' },
  metaTable: { width: '100%', borderCollapse: 'collapse' },
  metaLabel: { color: 'var(--text-muted)', fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.8, padding: '5px 12px 5px 0', verticalAlign: 'top', width: 110, whiteSpace: 'nowrap' },
  metaValue: { color: 'var(--text)', padding: '5px 0', wordBreak: 'break-all', fontSize: 13 },
  tabBar: { display: 'flex', borderBottom: '1px solid var(--border)', flexShrink: 0 },
  tab: { padding: '10px 20px', background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 13, fontFamily: 'inherit', borderBottom: '2px solid transparent', marginBottom: -1 },
  tabActive: { color: 'var(--accent)', borderBottomColor: 'var(--accent)' },
  playRowBtn: { background: 'transparent', border: 'none', color: 'var(--text-muted)', cursor: 'pointer', padding: '2px 4px', borderRadius: 4, display: 'flex', alignItems: 'center', opacity: 0.6 },
  // ── Search quick results (artists + albums) ──
  quickPanel: { borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)', flexShrink: 0, display: 'flex', gap: 8, flexDirection: 'column' as const, maxHeight: 320, overflowY: 'auto' as const, padding: '12px 16px 14px', background: 'color-mix(in srgb, var(--surface) 34%, transparent)' },
  quickSection: { ...phase2.tray, padding: '8px 0' },
  quickSectionLabel: { fontSize: 10, color: 'var(--text-muted)', textTransform: 'uppercase' as const, letterSpacing: 1, padding: '8px 16px 6px', fontWeight: 700 },
  quickRow: {
    display: 'flex', alignItems: 'center', gap: 10, width: '100%',
    padding: '10px 16px', background: 'transparent', border: 'none',
    color: 'var(--text)', cursor: 'pointer', fontFamily: 'inherit',
    textAlign: 'left' as const, transition: 'background 0.1s',
  },
  quickIcon: { color: 'var(--text-muted)', flexShrink: 0, display: 'flex', alignItems: 'center' },
  quickName: { fontSize: 13, fontWeight: 700, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' as const },
  quickMeta: { fontSize: 11, color: 'var(--text-muted)', flexShrink: 0, whiteSpace: 'nowrap' as const, fontWeight: 600 },
  quickArrow: { color: 'var(--text-muted)', flexShrink: 0, display: 'flex', alignItems: 'center', opacity: 0.5 },
  sectionDivider: { display: 'flex', alignItems: 'center', padding: '6px 16px 0', flexShrink: 0 },
  sectionDividerLabel: { fontSize: 10, color: 'var(--text-muted)', textTransform: 'uppercase' as const, letterSpacing: 1, fontWeight: 600 },
};
